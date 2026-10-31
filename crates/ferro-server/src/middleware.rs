//! HTTP middleware: rate limiting and response caching.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use axum::http::Request as HttpRequest;

use crate::extract::{bearer_token_from_headers_uri, Acc};
use crate::AppState;

fn cache_auth_fingerprint<B>(request: &HttpRequest<B>) -> String {
    let token = bearer_token_from_headers_uri(request.headers(), request.uri());
    match token {
        Some(t) => {
            let d = Sha256::digest(t.trim().as_bytes());
            hex::encode(&d[..8])
        }
        None => "anon".into(),
    }
}

fn query_param<'a>(query: Option<&'a str>, name: &str) -> Option<&'a str> {
    query?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == name { Some(v) } else { None }
    })
}

/// Only cache collection list and single-item reads (not `/…/versions`, etc.).
fn cacheable_items_get(path: &str) -> bool {
    if !path.starts_with("/items/") || path.ends_with("/aggregate") {
        return false;
    }
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segs.len() == 2 || segs.len() == 3
}

fn skip_http_cache(path: &str, query: Option<&str>) -> bool {
    !cacheable_items_get(path) || query_param(query, "version") == Some("draft")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cacheable_items_paths() {
        assert!(cacheable_items_get("/items/pages"));
        assert!(cacheable_items_get("/items/pages/550e8400-e29b-41d4-a716-446655440000"));
        assert!(!cacheable_items_get("/items/pages/x/versions"));
        assert!(!cacheable_items_get("/items/pages/aggregate"));
        assert!(!cacheable_items_get("/files"));
    }

    #[test]
    fn draft_query_skips_cache() {
        assert!(skip_http_cache("/items/pages", Some("version=draft")));
        assert!(!skip_http_cache("/items/pages", Some("version=drafty")));
        assert!(!skip_http_cache("/items/pages", Some("version=published")));
    }
}

pub async fn rate_limit_middleware(State(st): State<AppState>, Acc(acc): Acc, request: Request, next: Next) -> Response {
    if !st.rate_limit.enabled() {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if path == "/server/health" || path == "/server/openapi.json" {
        return next.run(request).await;
    }
    let ip = request
        .headers()
        .get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    let key = match acc.user {
        Some(u) => format!("user:{u}"),
        None => format!("ip:{ip}"),
    };
    if let Err(secs) = st.rate_limit.check(&key) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, secs.to_string())],
            format!("rate limit exceeded; retry in {secs}s"),
        )
            .into_response();
    }
    next.run(request).await
}

pub async fn http_cache_middleware(State(st): State<AppState>, request: Request, next: Next) -> Response {
    if !st.http_cache.enabled() || request.method() != Method::GET {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if skip_http_cache(path, request.uri().query()) {
        return next.run(request).await;
    }
    let auth_fingerprint = cache_auth_fingerprint(&request);
    let key = format!("{}:{}:{}", st.schema.get().hash, auth_fingerprint, request.uri());
    if let Some(c) = st.http_cache.get(&key) {
        if request.headers().get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(c.etag.as_str()) {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        return Response::builder()
            .status(StatusCode::from_u16(c.status).unwrap_or(StatusCode::OK))
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::CACHE_CONTROL,
                format!("private, max-age={}", st.config.http_cache_ttl_secs),
            )
            .header(header::ETAG, &c.etag)
            .body(Body::from(c.body))
            .unwrap()
            .into_response();
    }

    let response = next.run(request).await;
    if response.status() != StatusCode::OK {
        return response;
    }
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 512 * 1024).await.unwrap_or_default();
    if bytes.is_empty() {
        return Response::from_parts(parts, Body::empty()).into_response();
    }
    let digest = Sha256::digest(&bytes);
    let etag = format!("\"{}\"", hex::encode(&digest[..8]));
    st.http_cache.put(key, parts.status.as_u16(), bytes.to_vec(), etag.clone());
    Response::builder()
        .status(parts.status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, format!("private, max-age={}", st.config.http_cache_ttl_secs))
        .header(header::ETAG, etag)
        .body(Body::from(bytes))
        .unwrap()
        .into_response()
}
