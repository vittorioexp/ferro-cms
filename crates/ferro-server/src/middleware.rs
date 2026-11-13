//! HTTP middleware: rate limiting and response caching.

use std::net::SocketAddr;

use axum::body::{to_bytes, Body, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use crate::extract::bearer_token_from_headers_uri;
use crate::http_cache::MAX_CACHED_BODY;
use crate::{ApiError, AppState};

fn cache_auth_fingerprint(request: &Request) -> String {
    match bearer_token_from_headers_uri(request.headers(), request.uri()) {
        Some(t) => hex::encode(&Sha256::digest(t.trim().as_bytes())[..8]),
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

/// Rate-limit key: the user of a valid access token (checked without the database), else the client address.
fn rate_limit_key(st: &AppState, request: &Request) -> String {
    if let Some(token) = bearer_token_from_headers_uri(request.headers(), request.uri()) {
        if let Ok(user) = ferro_engine::auth::decode_access_token(&st.auth, token.trim()) {
            return format!("user:{user}");
        }
    }
    let forwarded = request
        .headers()
        .get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let ip = forwarded
        .or_else(|| request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string()))
        .unwrap_or_else(|| "unknown".into());
    format!("ip:{ip}")
}

pub async fn rate_limit_middleware(State(st): State<AppState>, request: Request, next: Next) -> Response {
    if !st.rate_limit.enabled() {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if path == "/server/health" {
        return next.run(request).await;
    }
    if let Err(secs) = st.rate_limit.check(&rate_limit_key(&st, &request)) {
        let mut response =
            ApiError::new(StatusCode::TOO_MANY_REQUESTS, "TOO_MANY_REQUESTS", format!("rate limit exceeded; retry in {secs}s"))
                .into_response();
        response.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(secs));
        return response;
    }
    next.run(request).await
}

fn with_cache_headers(mut response: Response, etag: &str) -> Response {
    let h = response.headers_mut();
    // `no-cache`: browsers may keep the body but must revalidate (cheap 304), so edits show up at once.
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    if let Ok(v) = HeaderValue::from_str(etag) {
        h.insert(header::ETAG, v);
    }
    response
}

fn not_modified(request_etag: Option<&str>, etag: &str) -> bool {
    request_etag.is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"))
}

pub async fn http_cache_middleware(State(st): State<AppState>, request: Request, next: Next) -> Response {
    if !st.http_cache.enabled() {
        return next.run(request).await;
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        let path = request.uri().path();
        if path.starts_with("/auth/") || path.ends_with("/aggregate") {
            return next.run(request).await;
        }
        let response = next.run(request).await;
        // also on errors: a failed batch may have written part of its changes before failing
        st.http_cache.invalidate();
        return response;
    }
    if request.method() != Method::GET || skip_http_cache(request.uri().path(), request.uri().query()) {
        return next.run(request).await;
    }
    let key = format!("{}:{}:{}", st.schema.get().hash, cache_auth_fingerprint(&request), request.uri());
    let if_none_match = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    if let Some(c) = st.http_cache.get(&key) {
        if not_modified(if_none_match.as_deref(), &c.etag) {
            return with_cache_headers(StatusCode::NOT_MODIFIED.into_response(), &c.etag);
        }
        let mut response = Response::new(Body::from(c.body));
        *response.status_mut() = StatusCode::from_u16(c.status).unwrap_or(StatusCode::OK);
        if let Some(ct) = c.content_type.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
            response.headers_mut().insert(header::CONTENT_TYPE, ct);
        }
        return with_cache_headers(response, &c.etag);
    }

    let generation = st.http_cache.generation();
    let response = next.run(request).await;
    if response.status() != StatusCode::OK {
        return response;
    }
    let too_big = response.body().size_hint().upper().map_or(true, |n| n > MAX_CACHED_BODY as u64);
    if too_big {
        return response;
    }
    let (parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MAX_CACHED_BODY).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "http cache: reading the response body failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let etag = format!("\"{}\"", hex::encode(&Sha256::digest(&bytes)[..8]));
    let content_type = parts.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(String::from);
    st.http_cache.put(key, generation, parts.status.as_u16(), content_type, bytes.to_vec(), etag.clone());
    if not_modified(if_none_match.as_deref(), &etag) {
        return with_cache_headers(StatusCode::NOT_MODIFIED.into_response(), &etag);
    }
    with_cache_headers(Response::from_parts(parts, Body::from(bytes)), &etag)
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

    #[test]
    fn if_none_match_lists() {
        assert!(not_modified(Some("\"a\", \"b\""), "\"b\""));
        assert!(!not_modified(Some("\"a\""), "\"b\""));
        assert!(!not_modified(None, "\"b\""));
    }
}
