//! Admin workspace API: comments, notifications, presets, dashboard, insights.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use ferro_engine::workspace::{self, CommentBody, PresetBody};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::extract::Acc;
use crate::routes::items::{self, parse_id};
use crate::{ApiResult, AppState};

fn ctx(st: &AppState, acc: ferro_engine::Accountability) -> ferro_engine::items::Ctx {
    items::ctx(st, acc)
}

#[derive(Debug, Deserialize)]
pub struct LimitQuery {
    pub limit: Option<i64>,
    #[serde(default)]
    pub unread: bool,
}

#[derive(Debug, Deserialize)]
pub struct PresetListQuery {
    pub collection: Option<String>,
}

pub async fn list_item_comments(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id)?;
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::list_comments(&ctx, &mut conn, &collection, id).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn create_item_comment(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id)): Path<(String, String)>,
    Json(body): Json<CommentBody>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id)?;
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::create_comment(&ctx, &mut conn, &collection, id, body).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn patch_comment(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let text = body
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| crate::ApiError::bad_request("text is required"))?
        .to_string();
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::update_comment(&ctx, &mut conn, id, text).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn delete_comment(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    workspace::delete_comment(&ctx, &mut conn, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_notifications(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let mut conn = st.pool.acquire().await?;
    let data = workspace::list_notifications(&mut conn, user, q.limit.unwrap_or(30), q.unread).await?;
    let unread = workspace::unread_count(&mut conn, user).await?;
    Ok(Json(json!({ "data": data, "meta": { "unread_count": unread } })))
}

pub async fn mark_notification_read(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user = workspace::require_app_user(&acc)?;
    let mut conn = st.pool.acquire().await?;
    workspace::mark_notification_read(&mut conn, user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn mark_notifications_read_all(
    State(st): State<AppState>,
    Acc(acc): Acc,
) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let mut conn = st.pool.acquire().await?;
    let n = workspace::mark_all_notifications_read(&mut conn, user).await?;
    Ok(Json(json!({ "data": { "marked": n } })))
}

pub async fn list_presets(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Query(q): Query<PresetListQuery>,
) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::list_presets(&ctx, &mut conn, user, q.collection.as_deref()).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn create_preset(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Json(body): Json<PresetBody>,
) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::create_preset(&ctx, &mut conn, user, body).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn update_preset(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(id): Path<Uuid>,
    Json(body): Json<PresetBody>,
) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::update_preset(&ctx, &mut conn, user, id, body).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn delete_preset(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user = workspace::require_app_user(&acc)?;
    let mut conn = st.pool.acquire().await?;
    workspace::delete_preset(&mut conn, user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_dashboard(State(st): State<AppState>, Acc(acc): Acc) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let mut conn = st.pool.acquire().await?;
    let layout = workspace::get_dashboard(&mut conn, user).await?;
    Ok(Json(json!({ "data": { "layout": layout } })))
}

#[derive(Debug, Deserialize)]
pub struct DashboardBody {
    pub layout: Value,
}

pub async fn put_dashboard(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Json(body): Json<DashboardBody>,
) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let mut conn = st.pool.acquire().await?;
    let data = workspace::put_dashboard(&mut conn, user, body.layout).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn insights(State(st): State<AppState>, Acc(acc): Acc) -> ApiResult<Json<Value>> {
    let user = workspace::require_app_user(&acc)?;
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::insights(&ctx, &mut conn, user).await?;
    Ok(Json(json!({ "data": data })))
}

#[derive(Debug, Deserialize)]
pub struct RecentActivityQuery {
    pub limit: Option<i64>,
}

pub async fn recent_activity(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Query(q): Query<RecentActivityQuery>,
) -> ApiResult<Json<Value>> {
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = workspace::recent_activity_for_user(&ctx, &mut conn, q.limit.unwrap_or(10)).await?;
    Ok(Json(json!({ "data": data })))
}
