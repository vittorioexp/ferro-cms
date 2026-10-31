//! SQL aggregations (count, sum, avg, min, max) with optional `GROUP BY`.

use ferro_core::{Action, FieldType};
use serde::Deserialize;
use serde_json::{Map, Value};
use sqlx::PgConnection;

use crate::ddl::q;
use crate::items::{self, check_filter_access, Ctx};
use crate::permissions;
use crate::query::Sql;
use crate::{FerroError, Result};

#[derive(Debug, Deserialize)]
pub struct AggregateRequest {
    #[serde(default)]
    pub filter: Option<Value>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub ops: Vec<AggregateOp>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AggregateOp {
    pub alias: String,
    /// `count`, `sum`, `avg`, `min`, `max`
    #[serde(alias = "func")]
    pub function: String,
    /// `*` or omitted for `count(*)`; field name otherwise.
    #[serde(default)]
    pub field: Option<String>,
}

const MAX_GROUP_FIELDS: usize = 10;
const DEFAULT_GROUP_LIMIT: i64 = 500;

pub async fn run(ctx: &Ctx, conn: &mut PgConnection, collection: &str, req: AggregateRequest) -> Result<Vec<Value>> {
    if req.ops.is_empty() {
        return Err(FerroError::invalid("at least one aggregate operation is required"));
    }
    let info = ctx.schema.collection(collection)?;
    let perm = permissions::access(&ctx.acc, collection, Action::Read)?;
    if let Some(f) = &req.filter {
        check_filter_access(ctx, &info, &perm, f, 0)?;
    }
    check_group_fields(&info, &perm, &req.group_by)?;
    for op in &req.ops {
        validate_op(&info, &perm, op)?;
    }
    let mut aliases = std::collections::HashSet::new();
    for op in &req.ops {
        if !aliases.insert(op.alias.clone()) {
            return Err(FerroError::invalid(format!("duplicate aggregate alias `{}`", op.alias)));
        }
    }

    let mut sql = Sql::new();
    let where_sql = items::build_where(&ctx.schema, &info, &perm, req.filter.as_ref(), req.search.as_deref(), &mut sql)?;

    let mut select_parts = Vec::new();
    for g in &req.group_by {
        select_parts.push(format!("t0.{}", q(g)));
    }
    for op in &req.ops {
        select_parts.push(compile_agg_expr(op)?);
    }

    let group_sql = if req.group_by.is_empty() {
        String::new()
    } else {
        format!(
            " GROUP BY {}",
            req.group_by.iter().map(|g| format!("t0.{}", q(g))).collect::<Vec<_>>().join(", ")
        )
    };

    let limit = req.limit.unwrap_or(DEFAULT_GROUP_LIMIT).clamp(1, 2000);
    let lim = sql.bind(Some(limit.to_string()));
    let text = format!(
        "SELECT COALESCE(jsonb_agg(to_jsonb(s)), '[]'::jsonb) FROM (SELECT {} FROM {} AS t0 WHERE {where_sql}{group_sql} ORDER BY 1 LIMIT {lim}) s",
        select_parts.join(", "),
        q(&info.name),
    );

    let raw = items::fetch_json(conn, &text, sql.params).await?;
    let rows = match raw {
        Value::Array(a) => a,
        _ => vec![],
    };

    Ok(rows
        .into_iter()
        .map(|row| {
            let Some(obj) = row.as_object() else {
                return row;
            };
            if req.group_by.is_empty() {
                return Value::Object(obj.clone());
            }
            let mut out = Map::new();
            let mut group = Map::new();
            for g in &req.group_by {
                if let Some(v) = obj.get(g) {
                    group.insert(g.clone(), v.clone());
                }
            }
            out.insert("group".into(), Value::Object(group));
            for op in &req.ops {
                if let Some(v) = obj.get(&op.alias) {
                    out.insert(op.alias.clone(), v.clone());
                }
            }
            Value::Object(out)
        })
        .collect())
}

fn check_group_fields(info: &crate::schema::CollectionInfo, perm: &permissions::Access, fields: &[String]) -> Result<()> {
    if fields.len() > MAX_GROUP_FIELDS {
        return Err(FerroError::invalid(format!("group_by accepts at most {MAX_GROUP_FIELDS} fields")));
    }
    for name in fields {
        if !perm.allows(name) {
            return Err(FerroError::forbidden(format!("you don't have permission to group `{}` by `{name}`", info.name)));
        }
        let f = info.require_field(name)?;
        if !f.has_column() || f.ty.is_relational() || f.ty.is_alias() {
            return Err(FerroError::invalid(format!("`{name}` cannot be used in group_by")));
        }
    }
    Ok(())
}

fn validate_op(info: &crate::schema::CollectionInfo, perm: &permissions::Access, op: &AggregateOp) -> Result<()> {
    if op.alias.trim().is_empty() || !op.alias.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(FerroError::invalid("aggregate alias must be alphanumeric/underscore"));
    }
    let func = op.function.to_ascii_lowercase();
    match func.as_str() {
        "count" => {
            if let Some(f) = op.field.as_deref().filter(|s| *s != "*") {
                if !perm.allows(f) {
                    return Err(FerroError::forbidden(format!("you don't have permission to aggregate field `{f}`")));
                }
                info.require_field(f)?;
            }
        }
        "sum" | "avg" | "min" | "max" => {
            let f = op
                .field
                .as_deref()
                .filter(|s| !s.is_empty() && *s != "*")
                .ok_or_else(|| FerroError::invalid(format!("`{func}` requires a field")))?;
            if !perm.allows(f) {
                return Err(FerroError::forbidden(format!("you don't have permission to aggregate field `{f}`")));
            }
            let field = info.require_field(f)?;
            if !matches!(field.ty, FieldType::Integer | FieldType::BigInteger | FieldType::Float | FieldType::Decimal) {
                return Err(FerroError::invalid(format!("`{func}` on `{f}` requires a numeric field")));
            }
        }
        other => return Err(FerroError::invalid(format!("unknown aggregate function `{other}`"))),
    }
    Ok(())
}

fn compile_agg_expr(op: &AggregateOp) -> Result<String> {
    let func = op.function.to_ascii_lowercase();
    let alias = q(&op.alias);
    Ok(match func.as_str() {
        "count" => match op.field.as_deref() {
            None | Some("*") => format!("COUNT(*) AS {alias}"),
            Some(f) => {
                let col = format!("t0.{}", q(f));
                format!("COUNT({col}) AS {alias}")
            }
        },
        "sum" => {
            let f = op.field.as_deref().unwrap();
            format!("SUM(t0.{}) AS {alias}", q(f))
        }
        "avg" => {
            let f = op.field.as_deref().unwrap();
            format!("AVG(t0.{}) AS {alias}", q(f))
        }
        "min" => {
            let f = op.field.as_deref().unwrap();
            format!("MIN(t0.{}) AS {alias}", q(f))
        }
        "max" => {
            let f = op.field.as_deref().unwrap();
            format!("MAX(t0.{}) AS {alias}", q(f))
        }
        _ => return Err(FerroError::invalid("unknown aggregate function")),
    })
}
