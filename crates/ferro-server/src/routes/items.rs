use axum::extract::{Path, Query, State};
use axum::http::Uri;
use axum::http::StatusCode;
use axum::Json;
use ferro_core::fields::FieldTree;
use ferro_core::system::{ACTIVITY, REVISIONS};
use ferro_engine::items::{self, Ctx, ReadArgs};
use ferro_engine::{Accountability, FerroError};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::extract::Acc;
use crate::{ApiError, ApiResult, AppState};

/// Query parameters shared by every listing (Directus compatible names).
#[derive(Debug, Default, Deserialize)]
pub struct ItemsQuery {
    pub fields: Option<String>,
    /// JSON filter, e.g. `{"status":{"_eq":"published"}}`, or use bracket form `filter[status][_eq]=published`
    pub filter: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub page: Option<i64>,
    pub search: Option<String>,
    /// `total_count`, `filter_count` or `*`
    pub meta: Option<String>,
    /// Language preference, e.g. `it-IT,en-US`: the translatable fields of collections with a `translations` field are
    /// returned on the item, each from the first language that has a value (see docs, "Translations").
    pub lang: Option<String>,
    /// `published` (default) or `draft` for collections with content versioning enabled.
    pub version: Option<String>,
}

/// Read rows, resolving translations when `lang` was asked for.
pub async fn read_rows(ctx: &Ctx, conn: &mut sqlx::PgConnection, collection: &str, args: ReadArgs, lang: Option<&str>) -> ApiResult<Vec<Value>> {
    match lang.map(str::trim).filter(|l| !l.is_empty()) {
        None => Ok(items::read_many(ctx, conn, collection, args).await?),
        Some(l) => {
            let langs = ferro_engine::translations::parse_languages(l)?;
            Ok(ferro_engine::translations::read_translated(ctx, conn, collection, args, &langs).await?)
        }
    }
}

pub fn field_tree(fields: Option<&str>) -> FieldTree {
    FieldTree::parse(&[fields.unwrap_or("*")])
}

pub fn read_args(q: &ItemsQuery, raw_query: Option<&str>, max_limit: i64) -> ApiResult<ReadArgs> {
    let filter = ferro_core::filter::merge_query_filters(q.filter.as_deref(), raw_query.unwrap_or(""))
        .map_err(|e| ApiError::bad_request(format!("invalid filter: {e}")))?;
    let limit = match q.limit {
        None => None,
        Some(l) if l < 0 => Some(if max_limit < 0 { -1 } else { max_limit }),
        Some(l) if max_limit >= 0 => Some(l.min(max_limit)),
        Some(l) => Some(l),
    };
    let effective = match limit {
        None => items::DEFAULT_LIMIT,
        Some(l) => l,
    };
    let offset = match (q.offset, q.page) {
        (Some(o), _) => Some(o),
        (None, Some(p)) if p > 1 && effective > 0 => Some((p - 1) * effective),
        _ => None,
    };
    Ok(ReadArgs {
        fields: field_tree(q.fields.as_deref()),
        filter,
        sort: q.sort.as_deref().map(|s| s.split(',').map(|x| x.trim().to_string()).collect()).unwrap_or_default(),
        limit,
        offset,
        search: q.search.clone(),
        lenient: false,
    })
}

pub fn ctx(st: &AppState, acc: Accountability) -> Ctx {
    Ctx::new(st.schema.get(), acc)
}

/// Shared listing implementation (also used by /files, /users, /activity...).
pub async fn list_collection(
    st: &AppState,
    acc: Accountability,
    collection: &str,
    q: &ItemsQuery,
    raw_query: Option<&str>,
) -> ApiResult<Json<Value>> {
    let ctx = ctx(st, acc);
    let info = ctx.schema.collection(collection)?;
    let mut conn = st.pool.acquire().await?;
    if info.def.meta.singleton {
        let args = ReadArgs { fields: field_tree(q.fields.as_deref()), limit: Some(1), ..Default::default() };
        let rows = read_rows(&ctx, &mut conn, collection, args, q.lang.as_deref()).await?;
        return Ok(Json(json!({ "data": rows.into_iter().next().unwrap_or(Value::Null) })));
    }
    let args = read_args(q, raw_query, st.config.query_limit_max)?;
    let filter = args.filter.clone();
    let search = args.search.clone();
    let rows = read_rows(&ctx, &mut conn, collection, args, q.lang.as_deref()).await?;
    let mut body = json!({ "data": rows });
    if let Some(meta) = q.meta.as_deref() {
        let mut m = serde_json::Map::new();
        if meta.contains("total_count") || meta == "*" {
            m.insert("total_count".into(), json!(items::count(&ctx, &mut conn, collection, None, None).await?));
        }
        if meta.contains("filter_count") || meta == "*" {
            m.insert(
                "filter_count".into(),
                json!(items::count(&ctx, &mut conn, collection, filter.as_ref(), search.as_deref()).await?),
            );
        }
        body["meta"] = Value::Object(m);
    }
    Ok(Json(body))
}

/// Read back an item after a write; callers without read access get only the id.
pub async fn read_back(
    st: &AppState,
    ctx: &Ctx,
    collection: &str,
    id: Uuid,
    fields: Option<&str>,
    version: Option<&str>,
) -> ApiResult<Value> {
    let mut conn = st.pool.acquire().await?;
    let ver = ferro_engine::versioning::ReadVersion::parse(version)?;
    match ver {
        ferro_engine::versioning::ReadVersion::Draft => {
            match ferro_engine::versioning::read_draft(ctx, &mut conn, collection, id, field_tree(fields)).await {
                Ok(v) => Ok(v),
                Err(FerroError::Forbidden(_)) | Err(FerroError::NotFound(_)) => Ok(json!({ "id": id })),
                Err(e) => Err(e.into()),
            }
        }
        ferro_engine::versioning::ReadVersion::Published => match items::read_one(ctx, &mut conn, collection, &id.to_string(), field_tree(fields)).await {
            Ok(v) => Ok(v),
            Err(FerroError::Forbidden(_)) | Err(FerroError::NotFound(_)) => Ok(json!({ "id": id })),
            Err(e) => Err(e.into()),
        },
    }
}

pub fn parse_id(id: &str) -> ApiResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| ApiError::bad_request(format!("`{id}` is not a valid id")))
}

pub async fn list(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(collection): Path<String>,
    uri: Uri,
    Query(q): Query<ItemsQuery>,
) -> ApiResult<Json<Value>> {
    list_collection(&st, acc, &collection, &q, uri.query()).await
}

pub async fn get_one(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id)): Path<(String, String)>,
    Query(q): Query<ItemsQuery>,
) -> ApiResult<Json<Value>> {
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let version = ferro_engine::versioning::ReadVersion::parse(q.version.as_deref())?;
    if version == ferro_engine::versioning::ReadVersion::Draft {
        let info = ctx.schema.collection(&collection)?;
        ferro_engine::versioning::require_draft_enabled(&info)?;
    }
    if q.lang.as_deref().map(|l| !l.trim().is_empty()).unwrap_or(false) {
        Uuid::parse_str(&id).map_err(|_| ApiError::bad_request(format!("`{id}` is not a valid id")))?;
        let args = ReadArgs { fields: field_tree(q.fields.as_deref()), filter: Some(json!({ "id": { "_eq": id } })), limit: Some(1), ..Default::default() };
        let mut rows = read_rows(&ctx, &mut conn, &collection, args, q.lang.as_deref()).await?;
        if rows.is_empty() {
            return Err(FerroError::not_found(format!("item `{id}` not found in `{collection}`")).into());
        }
        return Ok(Json(json!({ "data": rows.remove(0) })));
    }
    let item = match version {
        ferro_engine::versioning::ReadVersion::Draft => {
            let id = parse_id(&id)?;
            ferro_engine::versioning::read_draft(&ctx, &mut conn, &collection, id, field_tree(q.fields.as_deref())).await?
        }
        ferro_engine::versioning::ReadVersion::Published => {
            items::read_one(&ctx, &mut conn, &collection, &id, field_tree(q.fields.as_deref())).await?
        }
    };
    let mut body = json!({ "data": item });
    if q.meta.as_deref().map(|m| m.contains("draft") || m == "*").unwrap_or(false) {
        if ctx.schema.collection(&collection).ok().filter(|c| c.def.versioning_enabled()).is_some() {
            let id = parse_id(&id)?;
            let draft = ferro_engine::versioning::draft_meta(&mut conn, &collection, id).await?;
            body["meta"] = json!({ "draft": draft });
        }
    }
    Ok(Json(body))
}

pub async fn create(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(collection): Path<String>,
    Query(q): Query<ItemsQuery>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let ctx = ctx(&st, acc);
    let info = ctx.schema.collection(&collection)?;
    let version = ferro_engine::versioning::ReadVersion::parse(q.version.as_deref())?;
    if version == ferro_engine::versioning::ReadVersion::Draft {
        ferro_engine::versioning::require_draft_enabled(&info)?;
    }
    let mut tx = st.pool.begin().await?;
    if info.def.meta.singleton {
        let id = match items::singleton_id(&mut tx, &info).await? {
            Some(id) => {
                match version {
                    ferro_engine::versioning::ReadVersion::Draft => {
                        ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, body).await?;
                    }
                    ferro_engine::versioning::ReadVersion::Published => {
                        items::update_one(&ctx, &mut tx, &collection, id, body).await?;
                    }
                }
                id
            }
            None => {
                let id = items::create_one(&ctx, &mut tx, &collection, body.clone()).await?;
                if version == ferro_engine::versioning::ReadVersion::Draft {
                    ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, body).await?;
                }
                id
            }
        };
        tx.commit().await?;
        return Ok(Json(json!({ "data": read_back(&st, &ctx, &collection, id, q.fields.as_deref(), q.version.as_deref()).await? })));
    }
    let data = match body {
        Value::Array(list) => {
            let mut ids = vec![];
            for item in list {
                let id = items::create_one(&ctx, &mut tx, &collection, item.clone()).await?;
                if version == ferro_engine::versioning::ReadVersion::Draft {
                    ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, item).await?;
                }
                ids.push(id);
            }
            tx.commit().await?;
            let mut out = vec![];
            for id in ids {
                out.push(read_back(&st, &ctx, &collection, id, q.fields.as_deref(), q.version.as_deref()).await?);
            }
            Value::Array(out)
        }
        other => {
            let id = items::create_one(&ctx, &mut tx, &collection, other.clone()).await?;
            if version == ferro_engine::versioning::ReadVersion::Draft {
                ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, other).await?;
            }
            tx.commit().await?;
            read_back(&st, &ctx, &collection, id, q.fields.as_deref(), q.version.as_deref()).await?
        }
    };
    Ok(Json(json!({ "data": data })))
}

pub async fn update(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id)): Path<(String, String)>,
    Query(q): Query<ItemsQuery>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id)?;
    let ctx = ctx(&st, acc);
    let info = ctx.schema.collection(&collection)?;
    let version = ferro_engine::versioning::ReadVersion::parse(q.version.as_deref())?;
    if version == ferro_engine::versioning::ReadVersion::Draft {
        ferro_engine::versioning::require_draft_enabled(&info)?;
    }
    let mut tx = st.pool.begin().await?;
    match version {
        ferro_engine::versioning::ReadVersion::Draft => {
            ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, body).await?;
        }
        ferro_engine::versioning::ReadVersion::Published => {
            items::update_one(&ctx, &mut tx, &collection, id, body).await?;
        }
    }
    tx.commit().await?;
    Ok(Json(json!({ "data": read_back(&st, &ctx, &collection, id, q.fields.as_deref(), q.version.as_deref()).await? })))
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum BatchUpdate {
    /// `{ "keys": [..], "data": {..} }` – same change on many items
    Keys { keys: Vec<Uuid>, data: Value },
    /// `[ { "id": .., ... }, ... ]` – different change per item
    List(Vec<Value>),
}

pub async fn update_many(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(collection): Path<String>,
    Query(q): Query<ItemsQuery>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let ctx = ctx(&st, acc);
    let info = ctx.schema.collection(&collection)?;
    let version = ferro_engine::versioning::ReadVersion::parse(q.version.as_deref())?;
    if version == ferro_engine::versioning::ReadVersion::Draft {
        ferro_engine::versioning::require_draft_enabled(&info)?;
    }
    let mut tx = st.pool.begin().await?;
    if info.def.meta.singleton {
        let id = match items::singleton_id(&mut tx, &info).await? {
            Some(id) => {
                match version {
                    ferro_engine::versioning::ReadVersion::Draft => {
                        ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, body).await?;
                    }
                    ferro_engine::versioning::ReadVersion::Published => {
                        items::update_one(&ctx, &mut tx, &collection, id, body).await?;
                    }
                }
                id
            }
            None => items::create_one(&ctx, &mut tx, &collection, body).await?,
        };
        tx.commit().await?;
        return Ok(Json(json!({ "data": read_back(&st, &ctx, &collection, id, q.fields.as_deref(), q.version.as_deref()).await? })));
    }
    let batch: BatchUpdate = serde_json::from_value(body)?;
    let mut ids = vec![];
    match batch {
        BatchUpdate::Keys { keys, data } => {
            for id in keys {
                match version {
                    ferro_engine::versioning::ReadVersion::Draft => {
                        ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, data.clone()).await?;
                    }
                    ferro_engine::versioning::ReadVersion::Published => {
                        items::update_one(&ctx, &mut tx, &collection, id, data.clone()).await?;
                    }
                }
                ids.push(id);
            }
        }
        BatchUpdate::List(list) => {
            for mut item in list {
                let id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| ApiError::bad_request("every item needs an `id`"))?;
                if let Some(o) = item.as_object_mut() {
                    o.remove("id");
                }
                match version {
                    ferro_engine::versioning::ReadVersion::Draft => {
                        ferro_engine::versioning::save_draft(&ctx, &mut tx, &collection, id, item).await?;
                    }
                    ferro_engine::versioning::ReadVersion::Published => {
                        items::update_one(&ctx, &mut tx, &collection, id, item).await?;
                    }
                }
                ids.push(id);
            }
        }
    }
    tx.commit().await?;
    let mut out = vec![];
    for id in ids {
        out.push(read_back(&st, &ctx, &collection, id, q.fields.as_deref(), q.version.as_deref()).await?);
    }
    Ok(Json(json!({ "data": out })))
}

pub async fn delete_one(State(st): State<AppState>, Acc(acc): Acc, Path((collection, id)): Path<(String, String)>) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let ctx = ctx(&st, acc);
    let mut tx = st.pool.begin().await?;
    items::delete_many(&ctx, &mut tx, &collection, vec![id]).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum BatchDelete {
    Keys { keys: Vec<Uuid> },
    List(Vec<Uuid>),
}

pub async fn delete_many(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(collection): Path<String>,
    Json(body): Json<BatchDelete>,
) -> ApiResult<StatusCode> {
    let ids = match body {
        BatchDelete::Keys { keys } => keys,
        BatchDelete::List(l) => l,
    };
    let ctx = ctx(&st, acc);
    let mut tx = st.pool.begin().await?;
    items::delete_many(&ctx, &mut tx, &collection, ids).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn aggregate_items(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path(collection): Path<String>,
    Json(body): Json<ferro_engine::aggregate::AggregateRequest>,
) -> ApiResult<Json<Value>> {
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let data = ferro_engine::aggregate::run(&ctx, &mut conn, &collection, body).await?;
    Ok(Json(json!({ "data": data })))
}

pub async fn activity(
    State(st): State<AppState>,
    Acc(acc): Acc,
    uri: Uri,
    Query(q): Query<ItemsQuery>,
) -> ApiResult<Json<Value>> {
    list_collection(&st, acc, ACTIVITY, &q, uri.query()).await
}

pub async fn revisions(
    State(st): State<AppState>,
    Acc(acc): Acc,
    uri: Uri,
    Query(q): Query<ItemsQuery>,
) -> ApiResult<Json<Value>> {
    list_collection(&st, acc, REVISIONS, &q, uri.query()).await
}

#[derive(Debug, Deserialize)]
pub struct PublishBody {
    /// ISO-8601 time to publish; if in the future the draft stays until the scheduler runs.
    pub publish_at: Option<String>,
    #[serde(default)]
    pub force: bool,
}

pub async fn publish_item(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id)): Path<(String, String)>,
    Query(q): Query<ItemsQuery>,
    body: Option<Json<PublishBody>>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id)?;
    let ctx = ctx(&st, acc);
    let info = ctx.schema.collection(&collection)?;
    ferro_engine::versioning::require_draft_enabled(&info)?;
    let mut tx = st.pool.begin().await?;
    if let Some(Json(b)) = body {
        if let Some(at) = b.publish_at.as_deref() {
            let at = chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|e| ApiError::bad_request(format!("invalid publish_at: {e}")))?
                .with_timezone(&chrono::Utc);
            if at > chrono::Utc::now() {
                ferro_engine::versioning::schedule_publish(&ctx, &mut tx, &collection, id, at).await?;
                tx.commit().await?;
                return Ok(Json(json!({ "data": { "scheduled": true, "publish_at": at.to_rfc3339() } })));
            }
        }
        ferro_engine::versioning::publish(&ctx, &mut tx, &collection, id, b.force).await?;
    } else {
        ferro_engine::versioning::publish(&ctx, &mut tx, &collection, id, false).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({ "data": read_back(&st, &ctx, &collection, id, q.fields.as_deref(), None).await? })))
}

pub async fn discard_draft(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let ctx = ctx(&st, acc);
    let mut tx = st.pool.begin().await?;
    ferro_engine::versioning::discard_draft(&ctx, &mut tx, &collection, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_content_versions(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id)?;
    let ctx = ctx(&st, acc);
    let mut conn = st.pool.acquire().await?;
    let list = ferro_engine::versioning::list_versions(&ctx, &mut conn, &collection, id).await?;
    Ok(Json(json!({ "data": list })))
}

pub async fn restore_content_version(
    State(st): State<AppState>,
    Acc(acc): Acc,
    Path((collection, id, version_id)): Path<(String, String, String)>,
    Query(q): Query<ItemsQuery>,
) -> ApiResult<Json<Value>> {
    let id = parse_id(&id)?;
    let version_id = parse_id(&version_id)?;
    let ctx = ctx(&st, acc);
    let mut tx = st.pool.begin().await?;
    ferro_engine::versioning::restore_version_to_draft(&ctx, &mut tx, &collection, id, version_id).await?;
    tx.commit().await?;
    Ok(Json(json!({ "data": read_back(&st, &ctx, &collection, id, q.fields.as_deref(), Some("draft")).await? })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_args_parses_bracket_filter_from_raw_query() {
        let q = ItemsQuery { limit: Some(5), ..Default::default() };
        let args = read_args(&q, Some("filter[status][_eq]=published"), 100).unwrap();
        assert_eq!(args.filter, Some(json!({ "status": { "_eq": "published" } })));
        assert_eq!(args.limit, Some(5));
    }

    #[test]
    fn read_args_merges_json_filter_param_with_brackets() {
        let q = ItemsQuery {
            filter: Some(r#"{"slug":{"_eq":"home"}}"#.into()),
            ..Default::default()
        };
        let args = read_args(
            &q,
            Some(r#"filter={"slug":{"_eq":"home"}}&filter[status][_eq]=published"#),
            100,
        )
        .unwrap();
        assert_eq!(
            args.filter,
            Some(json!({
                "_and": [
                    { "slug": { "_eq": "home" } },
                    { "status": { "_eq": "published" } }
                ]
            }))
        );
    }

    #[test]
    fn read_args_rejects_invalid_json_filter() {
        let q = ItemsQuery {
            filter: Some("{not json}".into()),
            ..Default::default()
        };
        let err = read_args(&q, None, 100).unwrap_err();
        assert_eq!(err.code, "INVALID_PAYLOAD");
        assert!(err.message.contains("invalid filter"));
    }
}
