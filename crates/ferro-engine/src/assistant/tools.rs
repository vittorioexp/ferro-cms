//! Assistant tools — all data access through `items` + permissions.

use ferro_core::fields::FieldTree;
use ferro_core::{Action, Collection};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::assistant::{truncate_value, ChatOptions};
use crate::items::{self, Ctx, ReadArgs};
use crate::permissions::{self, Accountability};
use crate::schema::{CollectionInfo, Schema};
use crate::{FerroError, Result};

const MAX_LIST: i64 = 50;

pub fn tool_definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "list_collections",
                "description": "List collection names visible to this user (same as the admin collection list)",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "describe_collection",
                "description": "Describe fields and meta for one collection",
                "parameters": {
                    "type": "object",
                    "properties": { "collection": { "type": "string" } },
                    "required": ["collection"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_items",
                "description": "List items in a collection with optional filter, search, sort, limit",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "filter": { "type": "object", "description": "REST filter object" },
                        "search": { "type": "string" },
                        "sort": { "type": "string", "description": "comma-separated fields, -prefix for desc" },
                        "limit": { "type": "integer" },
                        "fields": { "type": "string", "description": "field tree, default *" }
                    },
                    "required": ["collection"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_item",
                "description": "Get one item by id",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "id": { "type": "string" },
                        "fields": { "type": "string" }
                    },
                    "required": ["collection", "id"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "create_item",
                "description": "Create an item (requires allow_writes on the chat request)",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "data": { "type": "object" }
                    },
                    "required": ["collection", "data"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "update_item",
                "description": "Update fields on an item (requires allow_writes)",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "id": { "type": "string" },
                        "data": { "type": "object" }
                    },
                    "required": ["collection", "id", "data"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "delete_item",
                "description": "Delete one item by id (requires allow_writes)",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "collection": { "type": "string" },
                        "id": { "type": "string" }
                    },
                    "required": ["collection", "id"],
                    "additionalProperties": false
                }
            }
        }
    ])
}

/// Collections the user can read items from (for prompts and data tools).
pub fn readable_collection_names(schema: &Schema, acc: &Accountability) -> Vec<String> {
    schema
        .snapshot
        .collections
        .iter()
        .filter(|c| {
            acc.admin || permissions::access(acc, &c.name, Action::Read).is_ok()
        })
        .map(|c| c.name.clone())
        .take(40)
        .collect()
}

pub fn visible_collections<'a>(schema: &'a Schema, acc: &Accountability) -> Vec<&'a Collection> {
    schema
        .snapshot
        .collections
        .iter()
        .filter(|c| can_list_collection(acc, &c.name))
        .collect()
}

/// Same visibility as `GET /collections` (names in the admin), not necessarily item read access.
fn can_list_collection(acc: &Accountability, name: &str) -> bool {
    acc.admin || acc.app || permissions::access(acc, name, Action::Read).is_ok()
}

fn ensure_read(acc: &Accountability, collection: &str) -> Result<()> {
    if acc.admin {
        return Ok(());
    }
    permissions::access(acc, collection, Action::Read)?;
    Ok(())
}

fn field_summary(info: &CollectionInfo, acc: &Accountability) -> Result<Value> {
    let perm = if acc.admin {
        permissions::Access::full()
    } else {
        permissions::access(acc, &info.name, Action::Read)?
    };
    let fields: Vec<Value> = info
        .fields
        .iter()
        .filter(|f| perm.allows(&f.name))
        .map(|f| {
            json!({
                "name": f.name,
                "type": format!("{:?}", f.ty).to_lowercase(),
                "required": f.required,
                "note": f.meta.note,
            })
        })
        .collect();
    Ok(json!({
        "name": info.name,
        "singleton": info.def.meta.singleton,
        "hidden": info.def.meta.hidden,
        "fields": fields,
    }))
}

fn str_arg(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| FerroError::invalid(format!("tool argument `{key}` must be a string")))
}

fn optional_filter(v: Option<&Value>) -> Result<Option<Value>> {
    let Some(v) = v else { return Ok(None) };
    match v {
        Value::Null => Ok(None),
        Value::String(s) => {
            let parsed: Value = serde_json::from_str(s).map_err(|e| FerroError::invalid(format!("invalid filter JSON: {e}")))?;
            Ok(Some(parsed))
        }
        other => Ok(Some(other.clone())),
    }
}

fn object_arg(args: &Value, key: &str) -> Result<Value> {
    match args.get(key) {
        Some(Value::Object(_)) => Ok(args.get(key).unwrap().clone()),
        Some(Value::String(s)) => serde_json::from_str(s).map_err(|e| FerroError::invalid(format!("`{key}` must be a JSON object: {e}"))),
        _ => Err(FerroError::invalid(format!("tool argument `{key}` must be an object"))),
    }
}

fn int_arg(v: Option<&Value>, default: i64, min: i64, max: i64) -> i64 {
    let n = match v {
        Some(Value::Number(num)) => num.as_i64().or_else(|| num.as_f64().map(|f| f as i64)).unwrap_or(default),
        _ => default,
    };
    n.clamp(min, max)
}

pub async fn execute(pool: &PgPool, ctx: &Ctx, opts: &ChatOptions, name: &str, args: Value) -> Result<Value> {
    match name {
        "list_collections" => {
            let list: Vec<Value> = visible_collections(&ctx.schema, &ctx.acc)
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "singleton": c.meta.singleton,
                        "group": c.meta.group,
                    })
                })
                .collect();
            Ok(json!({ "collections": list }))
        }
        "describe_collection" => {
            let name = str_arg(&args, "collection")?;
            ensure_read(&ctx.acc, &name)?;
            let info = ctx.schema.collection(&name)?;
            Ok(field_summary(&info, &ctx.acc)?)
        }
        "list_items" => {
            let collection = str_arg(&args, "collection")?;
            ensure_read(&ctx.acc, &collection)?;
            let info = ctx.schema.collection(&collection)?;
            let filter = optional_filter(args.get("filter"))?;
            let search = args.get("search").and_then(|v| v.as_str()).map(String::from);
            let sort = args
                .get("sort")
                .and_then(|v| v.as_str())
                .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
                .unwrap_or_default();
            let limit = int_arg(args.get("limit"), 20, 1, MAX_LIST);
            let fields = args.get("fields").and_then(|v| v.as_str()).unwrap_or("*");
            let mut conn = pool.acquire().await?;
            let read_args = ReadArgs {
                fields: FieldTree::parse(&[fields]),
                filter,
                sort,
                limit: Some(if info.def.meta.singleton { 1 } else { limit }),
                search,
                lenient: true,
                ..Default::default()
            };
            let rows = items::read_many(ctx, &mut conn, &collection, read_args).await?;
            if info.def.meta.singleton {
                Ok(truncate_value(
                    json!({ "singleton": true, "item": rows.into_iter().next().unwrap_or(Value::Null) }),
                    12_000,
                ))
            } else {
                Ok(truncate_value(json!({ "count": rows.len(), "items": rows }), 12_000))
            }
        }
        "get_item" => {
            let collection = str_arg(&args, "collection")?;
            ensure_read(&ctx.acc, &collection)?;
            let id = str_arg(&args, "id")?;
            let fields = args.get("fields").and_then(|v| v.as_str()).unwrap_or("*");
            let mut conn = pool.acquire().await?;
            let tree = FieldTree::parse(&[fields]);
            let item = items::read_one(ctx, &mut conn, &collection, &id, tree).await?;
            Ok(truncate_value(item, 12_000))
        }
        "create_item" => {
            if !opts.allow_writes {
                return Ok(json!({ "error": "writes_disabled", "message": "Enable \"Allow writes\" in the assistant UI to create items." }));
            }
            let collection = str_arg(&args, "collection")?;
            let data = object_arg(&args, "data")?;
            let mut conn = pool.acquire().await?;
            let id = items::create_one(ctx, &mut conn, &collection, data).await?;
            Ok(json!({ "ok": true, "id": id.to_string() }))
        }
        "update_item" => {
            if !opts.allow_writes {
                return Ok(json!({ "error": "writes_disabled", "message": "Enable \"Allow writes\" in the assistant UI to update items." }));
            }
            let collection = str_arg(&args, "collection")?;
            let id = str_arg(&args, "id")?;
            let uuid = Uuid::parse_str(&id).map_err(|_| FerroError::invalid("invalid id uuid"))?;
            let data = object_arg(&args, "data")?;
            let mut conn = pool.acquire().await?;
            items::update_one(ctx, &mut conn, &collection, uuid, data).await?;
            Ok(json!({ "ok": true, "id": id }))
        }
        "delete_item" => {
            if !opts.allow_writes {
                return Ok(json!({ "error": "writes_disabled", "message": "Enable \"Allow writes\" in the assistant UI to delete items." }));
            }
            let collection = str_arg(&args, "collection")?;
            let id = str_arg(&args, "id")?;
            let uuid = Uuid::parse_str(&id).map_err(|_| FerroError::invalid("invalid id uuid"))?;
            let mut conn = pool.acquire().await?;
            let n = items::delete_many(ctx, &mut conn, &collection, vec![uuid]).await?;
            Ok(json!({ "ok": true, "deleted": n }))
        }
        other => Err(FerroError::invalid(format!("unknown tool `{other}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn optional_filter_parses_string() {
        let v = json!({ "status": { "_eq": "draft" } });
        let out = optional_filter(Some(&json!(v.to_string()))).unwrap();
        assert_eq!(out, Some(v));
    }
}
