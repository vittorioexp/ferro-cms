//! Drafts, published snapshots and scheduled publishing (`meta.versioning.enabled`).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use ferro_core::fields::FieldTree;
use ferro_core::{Action, FieldType, SYSTEM_FIELDS};
use serde_json::{json, Map, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::activity;
use crate::items::{self, Ctx};
use crate::permissions::{self, Access, Accountability};
use crate::schema::Schema;
use crate::{FerroError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadVersion {
    Published,
    Draft,
}

impl ReadVersion {
    pub fn parse(s: Option<&str>) -> Result<Self> {
        match s.map(str::trim).filter(|x| !x.is_empty()) {
            None | Some("published") => Ok(ReadVersion::Published),
            Some("draft") => Ok(ReadVersion::Draft),
            Some(other) => Err(FerroError::invalid(format!("unknown version `{other}` (use published or draft)"))),
        }
    }
}

/// `version=draft` is only valid when the collection has `meta.versioning.enabled`.
pub fn require_draft_enabled(info: &crate::schema::CollectionInfo) -> Result<()> {
    if !info.def.versioning_enabled() {
        return Err(FerroError::invalid(format!(
            "`{}` does not use content versioning (omit `version` or use `version=published`)",
            info.name
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DraftMeta {
    pub has_draft: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_at: Option<DateTime<Utc>>,
}

struct DraftRow {
    data: Value,
    publish_at: Option<DateTime<Utc>>,
}

/// Locks the row until the end of the transaction (concurrent saves and publishes are serialized).
async fn load_draft(conn: &mut PgConnection, collection: &str, item: Uuid) -> Result<Option<DraftRow>> {
    let row: Option<(Value, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT data, publish_at FROM ferro_item_drafts WHERE collection = $1 AND item = $2 FOR UPDATE",
    )
    .bind(collection)
    .bind(item.to_string())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|(data, publish_at)| DraftRow { data, publish_at }))
}

fn schedule_from_payload(info: &crate::schema::CollectionInfo, payload: &Map<String, Value>) -> Option<DateTime<Utc>> {
    let field = info.def.meta.versioning.schedule_field.as_deref()?;
    let s = payload.get(field)?.as_str()?;
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

/// Draft fields over the live item, limited to the fields the live read returned (so `fields=` is respected).
fn overlay_draft(live: Value, draft: Value) -> Value {
    match (live, draft) {
        (Value::Object(mut l), Value::Object(d)) => {
            for (k, v) in d {
                if let Some(slot) = l.get_mut(&k) {
                    *slot = v;
                }
            }
            Value::Object(l)
        }
        (l, _) => l,
    }
}

fn strip_stored_draft(mut obj: Map<String, Value>) -> Value {
    obj.remove("id");
    for sys in SYSTEM_FIELDS {
        obj.remove(sys);
    }
    Value::Object(obj)
}

/// Drop fields the caller may not read from a stored draft snapshot.
pub fn filter_draft_by_permissions(info: &crate::schema::CollectionInfo, perm: &Access, mut value: Value) -> Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };
    obj.retain(|k, _| k == "id" || perm.allows(k));
    for f in &info.fields {
        if !perm.allows(&f.name) {
            continue;
        }
        if !f.ty.is_relational() {
            continue;
        }
        if matches!(f.ty, FieldType::M2o | FieldType::File) {
            continue;
        }
        if let Some(Value::Array(arr)) = obj.get_mut(&f.name) {
            for item in arr.iter_mut() {
                if let Value::Object(row) = item {
                    for sys in SYSTEM_FIELDS {
                        row.remove(sys);
                    }
                }
            }
        }
    }
    for sys in SYSTEM_FIELDS {
        obj.remove(sys);
    }
    value
}

pub async fn draft_meta(conn: &mut PgConnection, collection: &str, item: Uuid) -> Result<DraftMeta> {
    let row: Option<Option<DateTime<Utc>>> =
        sqlx::query_scalar("SELECT publish_at FROM ferro_item_drafts WHERE collection = $1 AND item = $2")
            .bind(collection)
            .bind(item.to_string())
            .fetch_optional(&mut *conn)
            .await?;
    Ok(match row {
        None => DraftMeta::default(),
        Some(publish_at) => DraftMeta { has_draft: true, publish_at },
    })
}

pub async fn read_one(ctx: &Ctx, conn: &mut PgConnection, collection: &str, id: Uuid, fields: FieldTree, version: ReadVersion) -> Result<Value> {
    match version {
        ReadVersion::Draft => read_draft(ctx, conn, collection, id, fields).await,
        ReadVersion::Published => items::read_one(ctx, conn, collection, &id.to_string(), fields).await,
    }
}

pub async fn update_one(ctx: &Ctx, conn: &mut PgConnection, collection: &str, id: Uuid, payload: Value, version: ReadVersion) -> Result<()> {
    match version {
        ReadVersion::Draft => save_draft(ctx, conn, collection, id, payload).await,
        ReadVersion::Published => items::update_one(ctx, conn, collection, id, payload).await,
    }
}

pub async fn read_draft(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    id: Uuid,
    fields: FieldTree,
) -> Result<Value> {
    let info = ctx.schema.collection(collection)?;
    require_draft_enabled(&info)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Read)?;
    items::ensure_permitted(ctx, conn, &info, &perm, &[id]).await?;
    let Some(draft) = load_draft(conn, collection, id).await? else {
        return items::read_one(ctx, conn, collection, &id.to_string(), fields).await;
    };
    let live = items::read_one(ctx, conn, collection, &id.to_string(), fields).await?;
    let merged = overlay_draft(live, draft.data);
    let mut out = filter_draft_by_permissions(&info, &perm, merged);
    if let Some(obj) = out.as_object_mut() {
        obj.insert("id".into(), json!(id.to_string()));
    }
    Ok(out)
}

pub async fn save_draft(ctx: &Ctx, conn: &mut PgConnection, collection: &str, id: Uuid, payload: Value) -> Result<()> {
    store_draft(ctx, conn, collection, id, payload, false).await
}

/// Only the changed fields are stored, so publishing does not overwrite live edits made since.
/// `replace` drops the previous draft instead of adding to it.
async fn store_draft(ctx: &Ctx, conn: &mut PgConnection, collection: &str, id: Uuid, payload: Value, replace: bool) -> Result<()> {
    let info = ctx.schema.collection(collection)?;
    require_draft_enabled(&info)?;
    if !items::exists(conn, collection, id).await? {
        return Err(FerroError::not_found(format!("item `{id}` not found in `{collection}`")));
    }
    let perm = permissions::access(&ctx.acc, collection, Action::Update)?;
    let Value::Object(mut obj) = payload else {
        return Err(FerroError::invalid("payload must be an object"));
    };
    obj.remove("id");
    items::check_fields(&perm, &info, &obj)?;
    items::validate_payload(&perm, &info, &obj, false)?;
    items::ensure_permitted(ctx, conn, &info, &perm, &[id]).await?;
    let previous = load_draft(conn, collection, id).await?;
    let publish_at = if info.def.meta.versioning.schedule_field.as_ref().is_some_and(|f| obj.contains_key(f)) {
        schedule_from_payload(&info, &obj)
    } else {
        previous.as_ref().and_then(|d| d.publish_at)
    };
    let mut data = match previous {
        Some(DraftRow { data: Value::Object(o), .. }) if !replace => o,
        _ => Map::new(),
    };
    data.extend(obj);
    write_draft_row(conn, collection, id, &strip_stored_draft(data), publish_at, ctx.acc.user).await
}

async fn write_draft_row(
    conn: &mut PgConnection,
    collection: &str,
    id: Uuid,
    data: &Value,
    publish_at: Option<DateTime<Utc>>,
    user: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO ferro_item_drafts (collection, item, data, publish_at, date_updated, user_updated) \
         VALUES ($1, $2, $3, $4, now(), $5) \
         ON CONFLICT (collection, item) DO UPDATE SET data = EXCLUDED.data, publish_at = EXCLUDED.publish_at, \
         date_updated = now(), user_updated = EXCLUDED.user_updated",
    )
    .bind(collection)
    .bind(id.to_string())
    .bind(data)
    .bind(publish_at)
    .bind(user)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn next_version_number(conn: &mut PgConnection, collection: &str, item: Uuid) -> Result<i32> {
    let n: Option<i32> = sqlx::query_scalar(
        "SELECT max(version_number) FROM ferro_item_versions WHERE collection = $1 AND item = $2",
    )
    .bind(collection)
    .bind(item.to_string())
    .fetch_one(&mut *conn)
    .await?;
    Ok(n.unwrap_or(0) + 1)
}

async fn archive_published(ctx: &Ctx, conn: &mut PgConnection, collection: &str, id: Uuid) -> Result<()> {
    let schema = ctx.schema.clone();
    let read_ctx = Ctx {
        schema,
        acc: Accountability::system(),
        origin: ctx.origin,
    };
    let snapshot = items::read_one(
        &read_ctx,
        conn,
        collection,
        &id.to_string(),
        FieldTree::parse(&["*"]),
    )
    .await?;
    let ver = next_version_number(conn, collection, id).await?;
    sqlx::query(
        "INSERT INTO ferro_item_versions (id, collection, item, version_number, data, published_by) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(collection)
    .bind(id.to_string())
    .bind(ver)
    .bind(&snapshot)
    .bind(ctx.acc.user)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Apply the draft to the live item, archive the previous live snapshot, and clear the draft.
pub async fn publish(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    id: Uuid,
    force: bool,
) -> Result<()> {
    let info = ctx.schema.collection(collection)?;
    require_draft_enabled(&info)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Update)?;
    items::ensure_permitted(ctx, conn, &info, &perm, &[id]).await?;
    let Some(draft) = load_draft(conn, collection, id).await? else {
        return Err(FerroError::invalid("there is no draft to publish"));
    };
    if let Some(at) = draft.publish_at {
        if at > Utc::now() && !force {
            return Err(FerroError::invalid(format!(
                "draft is scheduled for {} (pass force to publish now)",
                at.to_rfc3339()
            )));
        }
    }
    archive_published(ctx, conn, collection, id).await?;
    let payload = match draft.data {
        Value::Object(o) => Value::Object(o),
        _ => return Err(FerroError::Internal("invalid draft payload".into())),
    };
    items::update_one(ctx, conn, collection, id, payload).await?;
    sqlx::query("DELETE FROM ferro_item_drafts WHERE collection = $1 AND item = $2")
        .bind(collection)
        .bind(id.to_string())
        .execute(&mut *conn)
        .await?;
    activity::record(
        conn,
        &ctx.acc,
        "publish",
        collection,
        &id.to_string(),
        ctx.origin,
        None,
        None,
    )
    .await?;
    Ok(())
}

pub async fn discard_draft(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    id: Uuid,
) -> Result<()> {
    let info = ctx.schema.collection(collection)?;
    require_draft_enabled(&info)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Update)?;
    items::ensure_permitted(ctx, conn, &info, &perm, &[id]).await?;
    let n = sqlx::query("DELETE FROM ferro_item_drafts WHERE collection = $1 AND item = $2")
        .bind(collection)
        .bind(id.to_string())
        .execute(&mut *conn)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(FerroError::invalid("there is no draft to discard"));
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct VersionSummary {
    pub id: Uuid,
    pub version_number: i32,
    pub published_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_by: Option<Uuid>,
}

pub async fn list_versions(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    id: Uuid,
) -> Result<Vec<VersionSummary>> {
    let info = ctx.schema.collection(collection)?;
    require_draft_enabled(&info)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Read)?;
    items::ensure_permitted(ctx, conn, &info, &perm, &[id]).await?;
    let rows: Vec<(Uuid, i32, DateTime<Utc>, Option<Uuid>)> = sqlx::query_as(
        "SELECT id, version_number, published_at, published_by FROM ferro_item_versions \
         WHERE collection = $1 AND item = $2 ORDER BY version_number DESC",
    )
    .bind(collection)
    .bind(id.to_string())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, version_number, published_at, published_by)| VersionSummary {
            id,
            version_number,
            published_at,
            published_by,
        })
        .collect())
}

pub async fn restore_version_to_draft(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    item: Uuid,
    version_id: Uuid,
) -> Result<()> {
    let info = ctx.schema.collection(collection)?;
    require_draft_enabled(&info)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Update)?;
    items::ensure_permitted(ctx, conn, &info, &perm, &[item]).await?;
    let data: Option<Value> = sqlx::query_scalar(
        "SELECT data FROM ferro_item_versions WHERE id = $1 AND collection = $2 AND item = $3",
    )
    .bind(version_id)
    .bind(collection)
    .bind(item.to_string())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(data) = data else {
        return Err(FerroError::not_found("version not found"));
    };
    let payload = match data {
        Value::Object(mut o) => {
            for sys in SYSTEM_FIELDS {
                o.remove(sys);
            }
            o.remove("id");
            Value::Object(o)
        }
        _ => return Err(FerroError::Internal("invalid version payload".into())),
    };
    store_draft(ctx, conn, collection, item, payload, true).await
}

pub async fn schedule_publish(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    id: Uuid,
    publish_at: DateTime<Utc>,
) -> Result<()> {
    let info = ctx.schema.collection(collection)?;
    require_draft_enabled(&info)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Update)?;
    items::ensure_permitted(ctx, conn, &info, &perm, &[id]).await?;
    let n = sqlx::query("UPDATE ferro_item_drafts SET publish_at = $3 WHERE collection = $1 AND item = $2")
        .bind(collection)
        .bind(id.to_string())
        .bind(publish_at)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(FerroError::invalid("save a draft before scheduling publish"));
    }
    Ok(())
}

/// Publish every draft whose `publish_at` is due. Called from a background worker.
pub async fn publish_due(pool: &crate::db::PgPool, schema: Arc<Schema>) -> Result<u64> {
    let mut published = 0u64;
    loop {
        let mut tx = pool.begin().await?;
        let row: Option<(String, Uuid)> = sqlx::query_as(
            "SELECT collection, item::uuid FROM ferro_item_drafts \
             WHERE publish_at IS NOT NULL AND publish_at <= now() \
             FOR UPDATE SKIP LOCKED LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some((collection, item)) = row else {
            tx.commit().await?;
            break;
        };
        if schema.collection(&collection).ok().filter(|c| c.def.versioning_enabled()).is_none() {
            sqlx::query("UPDATE ferro_item_drafts SET publish_at = NULL WHERE collection = $1 AND item = $2")
                .bind(&collection)
                .bind(item.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            continue;
        }
        let ctx = Ctx {
            schema: schema.clone(),
            acc: Accountability::system(),
            origin: activity::ORIGIN_API,
        };
        // the update inside publish emits the realtime event
        match publish(&ctx, &mut tx, &collection, item, true).await {
            Ok(()) => {
                tx.commit().await?;
                published += 1;
            }
            Err(e) => {
                tracing::warn!(collection = %collection, item = %item, error = %e, "scheduled publish failed");
                sqlx::query("UPDATE ferro_item_drafts SET publish_at = NULL WHERE collection = $1 AND item = $2")
                    .bind(&collection)
                    .bind(item.to_string())
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
            }
        }
    }
    Ok(published)
}

pub async fn delete_drafts(conn: &mut PgConnection, collection: &str, ids: &[Uuid]) -> Result<()> {
    let ids: Vec<String> = ids.iter().map(Uuid::to_string).collect();
    sqlx::query("DELETE FROM ferro_item_drafts WHERE collection = $1 AND item = ANY($2)")
        .bind(collection)
        .bind(&ids)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{Access, FieldAccess};

    #[test]
    fn filter_draft_hides_protected_fields() {
        let def = ferro_core::Collection::new("pages");
        let fields = def.resolved_fields();
        let info = crate::schema::CollectionInfo {
            name: "pages".into(),
            def,
            fields,
            system: false,
        };
        let perm = Access {
            fields: FieldAccess::Some(std::collections::BTreeSet::from(["title".into()])),
            filter: None,
            validation: None,
            presets: serde_json::Map::new(),
        };
        let v = json!({ "id": "x", "title": "Hi", "secret": "no", "date_updated": "x" });
        let out = filter_draft_by_permissions(&info, &perm, v);
        assert_eq!(out["title"], "Hi");
        assert!(out.get("secret").is_none());
        assert!(out.get("date_updated").is_none());
    }

    #[test]
    fn overlay_keeps_only_requested_fields() {
        let live = json!({ "id": "x", "title": "Live" });
        let draft = json!({ "title": "Draft", "slug": "other" });
        assert_eq!(overlay_draft(live, draft), json!({ "id": "x", "title": "Draft" }));
    }

    #[test]
    fn parse_version_param() {
        assert_eq!(ReadVersion::parse(None).unwrap(), ReadVersion::Published);
        assert_eq!(ReadVersion::parse(Some("draft")).unwrap(), ReadVersion::Draft);
        assert!(ReadVersion::parse(Some("nope")).is_err());
    }
}
