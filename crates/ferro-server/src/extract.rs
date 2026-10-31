use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, USER_AGENT};
use axum::http::{HeaderMap, Uri};
use axum::http::request::Parts;
use ferro_engine::auth;
use ferro_engine::Accountability;

use crate::{ApiError, AppState};

/// The caller, resolved from `Authorization: Bearer <token>` or `?access_token=`.
pub struct Acc(pub Accountability);

/// Bearer token from `Authorization` or `?access_token=` (same rules as [`Acc`]).
pub fn bearer_token(parts: &Parts) -> Option<String> {
    bearer_token_from_headers_uri(&parts.headers, &parts.uri)
}

pub fn bearer_token_from_headers_uri(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    if let Some(v) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
            return Some(t.trim().to_string());
        }
    }
    uri.query().and_then(|q| {
        q.split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == "access_token")
            .map(|(_, v)| v.to_string())
    })
}

#[axum::async_trait]
impl FromRequestParts<AppState> for Acc {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts);
        let schema = state.schema.get();
        let mut acc = auth::accountability(&state.pool, &schema, &state.auth, token.as_deref()).await?;
        acc.ip = parts
            .headers
            .get("x-forwarded-for")
            .or_else(|| parts.headers.get("x-real-ip"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
        acc.user_agent = parts.headers.get(USER_AGENT).and_then(|v| v.to_str().ok()).map(String::from);
        Ok(Acc(acc))
    }
}
