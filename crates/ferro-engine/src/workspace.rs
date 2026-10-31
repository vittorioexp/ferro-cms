//! Admin workspace: item comments, in-app notifications, saved list presets, dashboards, insights.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use ferro_core::Action;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Connection;
use uuid::Uuid;

use crate::items::{self, Ctx};
use crate::permissions;
use crate::ddl::q;
use sqlx::PgConnection;
use crate::{FerroError, Result};

pub fn require_app_user(acc: &permissions::Accountability) -> Result<Uuid> {
    let Some(uid) = acc.user else {
        return Err(FerroError::Unauthorized("sign in first".into()));
    };
    if !acc.app {
        return Err(FerroError::forbidden("admin app access required"));
    }
    Ok(uid)
}

async fn ensure_item_readable(ctx: &Ctx, conn: &mut PgConnection, collection: &str, item: Uuid) -> Result<()> {
    let info = ctx.schema.collection(collection)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Read)?;
    if !items::exists(conn, collection, item).await? {
        return Err(FerroError::not_found(format!("item `{item}` not found in `{collection}`")));
    }
    items::ensure_permitted(ctx, conn, &info, &perm, &[item]).await?;
    Ok(())
}

// ---- comments ----------------------------------------------------------------

pub async fn list_comments(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    item: Uuid,
) -> Result<Vec<Value>> {
    require_app_user(&ctx.acc)?;
    ensure_item_readable(ctx, conn, collection, item).await?;
    let rows = sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Option<Uuid>, String, DateTime<Utc>, DateTime<Utc>)>(
        "SELECT id, collection, \"user\", parent, text, date_created, date_updated \
         FROM ferro_comments WHERE collection = $1 AND item = $2 ORDER BY date_created ASC",
    )
    .bind(collection)
    .bind(item.to_string())
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, collection, user, parent, text, date_created, date_updated)| {
            json!({
                "id": id,
                "collection": collection,
                "item": item.to_string(),
                "user": user.map(|u| u.to_string()),
                "parent": parent.map(|u| u.to_string()),
                "text": text,
                "date_created": date_created.to_rfc3339(),
                "date_updated": date_updated.to_rfc3339(),
            })
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct CommentBody {
    pub text: String,
    pub parent: Option<Uuid>,
}

pub async fn create_comment(
    ctx: &Ctx,
    conn: &mut PgConnection,
    collection: &str,
    item: Uuid,
    body: CommentBody,
) -> Result<Value> {
    let author = require_app_user(&ctx.acc)?;
    ensure_item_readable(ctx, conn, collection, item).await?;
    let text = body.text.trim();
    if text.is_empty() {
        return Err(FerroError::invalid("comment text is required"));
    }
    if text.len() > 20_000 {
        return Err(FerroError::invalid("comment is too long"));
    }
    if let Some(parent) = body.parent {
        let ok: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM ferro_comments WHERE id = $1 AND collection = $2 AND item = $3",
        )
        .bind(parent)
        .bind(collection)
        .bind(item.to_string())
        .fetch_optional(&mut *conn)
        .await?;
        if ok.is_none() {
            return Err(FerroError::invalid("parent comment not found on this item"));
        }
    }
    let id = Uuid::now_v7();
    let now = Utc::now();
    let mut tx = conn.begin().await?;
    sqlx::query(
        "INSERT INTO ferro_comments (id, collection, item, \"user\", parent, text, date_created, date_updated) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7)",
    )
    .bind(id)
    .bind(collection)
    .bind(item.to_string())
    .bind(author)
    .bind(body.parent)
    .bind(text)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    notify_for_comment(&mut tx, author, collection, &item.to_string(), body.parent, text).await?;
    tx.commit().await?;

    Ok(json!({
        "id": id,
        "collection": collection,
        "item": item.to_string(),
        "user": author.to_string(),
        "parent": body.parent.map(|u| u.to_string()),
        "text": text,
        "date_created": now.to_rfc3339(),
        "date_updated": now.to_rfc3339(),
    }))
}

async fn notify_for_comment(
    conn: &mut PgConnection,
    author: Uuid,
    collection: &str,
    item: &str,
    parent: Option<Uuid>,
    text: &str,
) -> Result<()> {
    let mut recipients = HashSet::new();
    if let Some(parent) = parent {
        let parent_author: Option<Uuid> = sqlx::query_scalar("SELECT \"user\" FROM ferro_comments WHERE id = $1")
            .bind(parent)
            .fetch_one(&mut *conn)
            .await?;
        if let Some(uid) = parent_author {
            if uid != author {
                recipients.insert(uid);
            }
        }
    }
    for uid in mention_user_ids(text) {
        if uid != author {
            recipients.insert(uid);
        }
    }
    let subject = "New comment";
    let message = truncate(text, 240);
    for recipient in recipients {
        insert_notification(
            conn,
            recipient,
            Some(author),
            "comment",
            subject,
            &message,
            Some(collection),
            Some(item),
        )
        .await?;
    }
    Ok(())
}

fn mention_user_ids(text: &str) -> Vec<Uuid> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' && i + 37 <= bytes.len() {
            let slice = &text[i + 1..i + 37];
            if slice.chars().count() == 36 {
                if let Ok(uid) = Uuid::parse_str(slice) {
                    out.push(uid);
                }
            }
        }
        i += 1;
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
}

pub async fn update_comment(ctx: &Ctx, conn: &mut PgConnection, id: Uuid, text: String) -> Result<Value> {
    let author = require_app_user(&ctx.acc)?;
    let text = text.trim();
    if text.is_empty() {
        return Err(FerroError::invalid("comment text is required"));
    }
    let row: Option<(String, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT collection, item, \"user\" FROM ferro_comments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((collection, item, owner)) = row else {
        return Err(FerroError::not_found("comment not found"));
    };
    if owner != Some(author) && !ctx.acc.admin {
        return Err(FerroError::forbidden("you can only edit your own comments"));
    }
    let item_id = Uuid::parse_str(&item).map_err(|_| FerroError::Internal("invalid item id".into()))?;
    ensure_item_readable(ctx, conn, &collection, item_id).await?;
    let now = Utc::now();
    sqlx::query("UPDATE ferro_comments SET text = $2, date_updated = $3 WHERE id = $1")
        .bind(id)
        .bind(text)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    Ok(json!({
        "id": id,
        "collection": collection,
        "item": item,
        "user": owner.map(|u| u.to_string()),
        "text": text,
        "date_updated": now.to_rfc3339(),
    }))
}

pub async fn delete_comment(ctx: &Ctx, conn: &mut PgConnection, id: Uuid) -> Result<()> {
    let user = require_app_user(&ctx.acc)?;
    let row: Option<(String, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT collection, item, \"user\" FROM ferro_comments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((collection, item, owner)) = row else {
        return Err(FerroError::not_found("comment not found"));
    };
    if owner != Some(user) && !ctx.acc.admin {
        return Err(FerroError::forbidden("you can only delete your own comments"));
    }
    let item_id = Uuid::parse_str(&item).map_err(|_| FerroError::Internal("invalid item id".into()))?;
    ensure_item_readable(ctx, conn, &collection, item_id).await?;
    sqlx::query("DELETE FROM ferro_comments WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

// ---- notifications -----------------------------------------------------------

async fn insert_notification(
    conn: &mut PgConnection,
    recipient: Uuid,
    sender: Option<Uuid>,
    kind: &str,
    subject: &str,
    message: &str,
    collection: Option<&str>,
    item: Option<&str>,
) -> Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO ferro_notifications (id, recipient, sender, kind, subject, message, collection, item) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(recipient)
    .bind(sender)
    .bind(kind)
    .bind(subject)
    .bind(message)
    .bind(collection)
    .bind(item)
    .execute(&mut *conn)
    .await?;
    Ok(id)
}

pub async fn list_notifications(
    conn: &mut PgConnection,
    user: Uuid,
    limit: i64,
    unread_only: bool,
) -> Result<Vec<Value>> {
    let limit = limit.clamp(1, 100);
    let rows = if unread_only {
        sqlx::query_as::<_, (Uuid, Option<Uuid>, String, String, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>)>(
            "SELECT id, sender, kind, subject, message, collection, item, read_at, date_created \
             FROM ferro_notifications WHERE recipient = $1 AND read_at IS NULL \
             ORDER BY date_created DESC LIMIT $2",
        )
        .bind(user)
        .bind(limit)
        .fetch_all(&mut *conn)
        .await?
    } else {
        sqlx::query_as::<_, (Uuid, Option<Uuid>, String, String, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>)>(
            "SELECT id, sender, kind, subject, message, collection, item, read_at, date_created \
             FROM ferro_notifications WHERE recipient = $1 ORDER BY date_created DESC LIMIT $2",
        )
        .bind(user)
        .bind(limit)
        .fetch_all(&mut *conn)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(|(id, sender, kind, subject, message, collection, item, read_at, date_created)| {
            json!({
                "id": id,
                "sender": sender.map(|u| u.to_string()),
                "kind": kind,
                "subject": subject,
                "message": message,
                "collection": collection,
                "item": item,
                "read_at": read_at.map(|t| t.to_rfc3339()),
                "date_created": date_created.to_rfc3339(),
            })
        })
        .collect())
}

pub async fn unread_count(conn: &mut PgConnection, user: Uuid) -> Result<i64> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ferro_notifications WHERE recipient = $1 AND read_at IS NULL")
        .bind(user)
        .fetch_one(&mut *conn)
        .await?;
    Ok(n)
}

pub async fn mark_notification_read(conn: &mut PgConnection, user: Uuid, id: Uuid) -> Result<()> {
    let n = sqlx::query(
        "UPDATE ferro_notifications SET read_at = now() WHERE id = $1 AND recipient = $2 AND read_at IS NULL",
    )
    .bind(id)
    .bind(user)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if n > 0 {
        return Ok(());
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ferro_notifications WHERE id = $1 AND recipient = $2)")
        .bind(id)
        .bind(user)
        .fetch_one(&mut *conn)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(FerroError::not_found("notification not found"))
    }
}

pub async fn mark_all_notifications_read(conn: &mut PgConnection, user: Uuid) -> Result<u64> {
    let r = sqlx::query("UPDATE ferro_notifications SET read_at = now() WHERE recipient = $1 AND read_at IS NULL")
        .bind(user)
        .execute(&mut *conn)
        .await?;
    Ok(r.rows_affected())
}

// ---- presets (saved views) ---------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PresetLayout {
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub conditions: Vec<Value>,
}

pub async fn list_presets(ctx: &Ctx, conn: &mut PgConnection, user: Uuid, collection: Option<&str>) -> Result<Vec<Value>> {
    let rows = if let Some(c) = collection {
        sqlx::query_as::<_, (Uuid, String, String, Value, bool, DateTime<Utc>)>(
            "SELECT id, collection, name, layout, shared, date_updated FROM ferro_presets \
             WHERE collection = $1 AND (\"user\" = $2 OR shared = true) ORDER BY name",
        )
        .bind(c)
        .bind(user)
        .fetch_all(&mut *conn)
        .await?
    } else {
        sqlx::query_as::<_, (Uuid, String, String, Value, bool, DateTime<Utc>)>(
            "SELECT id, collection, name, layout, shared, date_updated FROM ferro_presets \
             WHERE \"user\" = $1 OR shared = true ORDER BY collection, name",
        )
        .bind(user)
        .fetch_all(&mut *conn)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(|(id, collection, name, layout, shared, date_updated)| {
            json!({
                "id": id,
                "collection": collection,
                "name": name,
                "layout": layout,
                "shared": shared,
                "date_updated": date_updated.to_rfc3339(),
            })
        })
        .filter(|v| {
            v.get("collection")
                .and_then(|c| c.as_str())
                .is_some_and(|name| permissions::access(&ctx.acc, name, Action::Read).is_ok())
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct PresetBody {
    pub collection: String,
    pub name: String,
    pub layout: PresetLayout,
    #[serde(default)]
    pub shared: bool,
}

pub async fn create_preset(ctx: &Ctx, conn: &mut PgConnection, user: Uuid, body: PresetBody) -> Result<Value> {
    ctx.schema.collection(&body.collection)?;
    permissions::access(&ctx.acc, &body.collection, Action::Read)?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(FerroError::invalid("preset name is required"));
    }
    let id = Uuid::now_v7();
    let layout = serde_json::to_value(&body.layout)?;
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO ferro_presets (id, \"user\", collection, name, layout, shared, date_created, date_updated) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7)",
    )
    .bind(id)
    .bind(user)
    .bind(&body.collection)
    .bind(name)
    .bind(layout.clone())
    .bind(body.shared)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(json!({
        "id": id,
        "collection": body.collection,
        "name": name,
        "layout": layout,
        "shared": body.shared,
        "date_updated": now.to_rfc3339(),
    }))
}

pub async fn update_preset(ctx: &Ctx, conn: &mut PgConnection, user: Uuid, id: Uuid, body: PresetBody) -> Result<Value> {
    ctx.schema.collection(&body.collection)?;
    permissions::access(&ctx.acc, &body.collection, Action::Read)?;
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT \"user\" FROM ferro_presets WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some(owner) = owner else {
        return Err(FerroError::not_found("preset not found"));
    };
    if owner != user {
        return Err(FerroError::forbidden("you can only edit your own presets"));
    }
    let name = body.name.trim();
    if name.is_empty() {
        return Err(FerroError::invalid("preset name is required"));
    }
    let layout = serde_json::to_value(&body.layout)?;
    let now = Utc::now();
    sqlx::query(
        "UPDATE ferro_presets SET collection = $2, name = $3, layout = $4, shared = $5, date_updated = $6 WHERE id = $1",
    )
    .bind(id)
    .bind(&body.collection)
    .bind(name)
    .bind(layout.clone())
    .bind(body.shared)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(json!({
        "id": id,
        "collection": body.collection,
        "name": name,
        "layout": layout,
        "shared": body.shared,
        "date_updated": now.to_rfc3339(),
    }))
}

pub async fn delete_preset(conn: &mut PgConnection, user: Uuid, id: Uuid) -> Result<()> {
    let n = sqlx::query("DELETE FROM ferro_presets WHERE id = $1 AND \"user\" = $2")
        .bind(id)
        .bind(user)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(FerroError::not_found("preset not found"));
    }
    Ok(())
}

// ---- dashboard ---------------------------------------------------------------

fn default_dashboard_layout() -> Value {
    json!({
        "panels": [
            { "type": "insights" },
            { "type": "recent_activity", "limit": 8 },
            { "type": "collections" },
            { "type": "notifications", "limit": 6 }
        ]
    })
}

pub async fn get_dashboard(conn: &mut PgConnection, user: Uuid) -> Result<Value> {
    let row: Option<Value> = sqlx::query_scalar("SELECT layout FROM ferro_dashboards WHERE \"user\" = $1")
        .bind(user)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.unwrap_or_else(default_dashboard_layout))
}

pub async fn put_dashboard(conn: &mut PgConnection, user: Uuid, layout: Value) -> Result<Value> {
    if !layout.is_object() {
        return Err(FerroError::invalid("dashboard layout must be an object"));
    }
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO ferro_dashboards (\"user\", layout, date_updated) VALUES ($1, $2, $3) \
         ON CONFLICT (\"user\") DO UPDATE SET layout = EXCLUDED.layout, date_updated = EXCLUDED.date_updated",
    )
    .bind(user)
    .bind(layout.clone())
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(json!({ "layout": layout, "date_updated": now.to_rfc3339() }))
}

// ---- insights ----------------------------------------------------------------

pub async fn insights(ctx: &Ctx, conn: &mut PgConnection, user: Uuid) -> Result<Value> {
    let unread = unread_count(conn, user).await?;
    let mut collections = Vec::new();
    for info in ctx.schema.user_collections() {
        if permissions::access(&ctx.acc, &info.name, Action::Read).is_err() {
            continue;
        }
        let perm = permissions::access(&ctx.acc, &info.name, Action::Read)?;
        let count = if perm.filter.is_none() {
            json!(count_accessible_items(conn, &info.name).await.unwrap_or(0))
        } else {
            Value::Null
        };
        collections.push(json!({ "name": info.name, "count": count }));
    }
    let activity = if ctx.acc.admin {
        activity_last_days(conn, 7).await?
    } else {
        Vec::new()
    };
    Ok(json!({
        "unread_notifications": unread,
        "collections": collections,
        "activity_by_day": activity,
    }))
}

async fn count_accessible_items(conn: &mut PgConnection, collection: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {}", q(collection));
    let n: i64 = sqlx::query_scalar(&sql).fetch_one(&mut *conn).await?;
    Ok(n)
}

async fn activity_last_days(conn: &mut PgConnection, days: i32) -> Result<Vec<Value>> {
    let rows = sqlx::query_as::<_, (chrono::NaiveDate, i64, i64, i64)>(
        "SELECT (timestamp AT TIME ZONE 'UTC')::date AS day, \
         COUNT(*) FILTER (WHERE action = 'create') AS creates, \
         COUNT(*) FILTER (WHERE action = 'update') AS updates, \
         COUNT(*) FILTER (WHERE action = 'delete') AS deletes \
         FROM ferro_activity WHERE timestamp >= now() - make_interval(days => $1) \
         GROUP BY day ORDER BY day DESC",
    )
    .bind(days)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(day, creates, updates, deletes)| {
            json!({ "day": day.to_string(), "creates": creates, "updates": updates, "deletes": deletes })
        })
        .collect())
}

pub async fn recent_activity_for_user(ctx: &Ctx, conn: &mut PgConnection, limit: i64) -> Result<Vec<Value>> {
    require_app_user(&ctx.acc)?;
    if !ctx.acc.admin {
        return Ok(vec![]);
    }
    let limit = limit.clamp(1, 50);
    let rows = sqlx::query_as::<_, (DateTime<Utc>, String, Option<Uuid>, String, String, String)>(
        "SELECT timestamp, action, \"user\", collection, item, origin FROM ferro_activity \
         ORDER BY timestamp DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(timestamp, action, user, collection, item, origin)| {
            json!({
                "timestamp": timestamp.to_rfc3339(),
                "action": action,
                "user": user.map(|u| u.to_string()),
                "collection": collection,
                "item": item,
                "origin": origin,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_parsing() {
        let uid = Uuid::now_v7();
        let ids = mention_user_ids(&format!("Hi @{uid} and @not-a-uuid"));
        assert_eq!(ids, vec![uid]);
        assert!(mention_user_ids(&format!("tail @{uid}")).contains(&uid));
    }
}
