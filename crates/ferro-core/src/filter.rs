//! Directus-compatible filter language, evaluated in memory.
//!
//! ```json
//! { "_and": [ { "status": { "_eq": "published" } }, { "author": { "name": { "_icontains": "ros" } } } ] }
//! ```
//! REST list endpoints also accept bracket query params (`filter[status][_eq]=published`) via
//! [`parse_bracket_query`] and [`merge_query_filters`].
//! The SQL compiler lives in the engine; this evaluator is used for permission
//! `validation` rules and field validation on payloads.

use std::cmp::Ordering;

use serde_json::{Map, Value};

pub const OPERATORS: &[&str] = &[
    "_eq", "_neq", "_lt", "_lte", "_gt", "_gte", "_in", "_nin", "_null", "_nnull", "_empty", "_nempty",
    "_contains", "_ncontains", "_icontains", "_nicontains", "_starts_with", "_nstarts_with",
    "_istarts_with", "_ends_with", "_nends_with", "_iends_with", "_between", "_nbetween",
];

pub fn is_operator(k: &str) -> bool {
    OPERATORS.contains(&k)
}

/// True when `v` is an object made only of operators (`{ "_eq": 1 }`), i.e. a leaf condition.
pub fn is_condition(v: &Value) -> bool {
    match v.as_object() {
        Some(m) => !m.is_empty() && m.keys().all(|k| is_operator(k)),
        None => false,
    }
}

/// Replace dynamic variables (`$CURRENT_USER`, `$CURRENT_ROLE`, `$NOW`) recursively.
pub fn substitute_vars(filter: &Value, vars: &Map<String, Value>) -> Value {
    match filter {
        Value::String(s) if s.starts_with('$') => vars.get(s.as_str()).cloned().unwrap_or(Value::Null),
        Value::Array(a) => Value::Array(a.iter().map(|v| substitute_vars(v, vars)).collect()),
        Value::Object(m) => {
            Value::Object(m.iter().map(|(k, v)| (k.clone(), substitute_vars(v, vars))).collect())
        }
        other => other.clone(),
    }
}

/// Combine filters with `_and`, skipping `None`.
pub fn and_all(filters: Vec<Option<Value>>) -> Option<Value> {
    let mut parts: Vec<Value> = filters.into_iter().flatten().filter(|f| !is_empty_filter(f)).collect();
    match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(serde_json::json!({ "_and": parts })),
    }
}

pub fn is_empty_filter(f: &Value) -> bool {
    match f {
        Value::Null => true,
        Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

/// Evaluate `filter` against an item (JSON object, possibly with nested relations).
pub fn matches(filter: &Value, item: &Value) -> bool {
    let Some(obj) = filter.as_object() else { return true };
    obj.iter().all(|(key, cond)| match key.as_str() {
        "_and" => cond.as_array().map(|a| a.iter().all(|f| matches(f, item))).unwrap_or(true),
        "_or" => cond.as_array().map(|a| a.iter().any(|f| matches(f, item))).unwrap_or(true),
        field => {
            let value = item.get(field).unwrap_or(&Value::Null);
            if is_condition(cond) {
                cond.as_object().unwrap().iter().all(|(op, operand)| eval_op(op, value, operand))
            } else if cond.is_object() {
                // nested relation: { author: { name: { _eq: .. } } } or o2m arrays (any)
                match value {
                    Value::Array(items) => items.iter().any(|v| matches(cond, v)),
                    Value::Null => false,
                    v => matches(cond, v),
                }
            } else {
                // shorthand { field: value }
                eval_op("_eq", value, cond)
            }
        }
    })
}

fn cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64()?.partial_cmp(&y.as_f64()?),
        (Value::String(x), Value::String(y)) => {
            // numbers sent as strings (decimal) compare numerically
            match (x.parse::<f64>(), y.parse::<f64>()) {
                (Ok(fx), Ok(fy)) => fx.partial_cmp(&fy),
                _ => Some(x.cmp(y)),
            }
        }
        (Value::Number(x), Value::String(y)) => x.as_f64()?.partial_cmp(&y.parse::<f64>().ok()?),
        (Value::String(x), Value::Number(y)) => x.parse::<f64>().ok()?.partial_cmp(&y.as_f64()?),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    a == b || cmp(a, b) == Some(Ordering::Equal)
}

fn as_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn is_empty_value(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => s == "true" || s == "1",
        Value::Number(n) => n.as_f64() != Some(0.0),
        _ => false,
    }
}

pub fn eval_op(op: &str, value: &Value, operand: &Value) -> bool {
    let text = |f: fn(&str, &str) -> bool, insensitive: bool| -> bool {
        match (as_text(value), as_text(operand)) {
            (Some(v), Some(o)) if insensitive => f(&v.to_lowercase(), &o.to_lowercase()),
            (Some(v), Some(o)) => f(&v, &o),
            _ => false,
        }
    };
    match op {
        "_eq" => loose_eq(value, operand),
        "_neq" => !loose_eq(value, operand),
        "_lt" => cmp(value, operand) == Some(Ordering::Less),
        "_lte" => matches!(cmp(value, operand), Some(Ordering::Less | Ordering::Equal)),
        "_gt" => cmp(value, operand) == Some(Ordering::Greater),
        "_gte" => matches!(cmp(value, operand), Some(Ordering::Greater | Ordering::Equal)),
        "_in" => operand.as_array().map(|a| a.iter().any(|o| loose_eq(value, o))).unwrap_or(false),
        "_nin" => !operand.as_array().map(|a| a.iter().any(|o| loose_eq(value, o))).unwrap_or(false),
        "_null" => value.is_null() == truthy(operand),
        "_nnull" => value.is_null() != truthy(operand),
        "_empty" => is_empty_value(value) == truthy(operand),
        "_nempty" => is_empty_value(value) != truthy(operand),
        "_contains" => match value {
            Value::Array(a) => a.iter().any(|v| loose_eq(v, operand)),
            _ => text(|v, o| v.contains(o), false),
        },
        "_ncontains" => !eval_op("_contains", value, operand),
        "_icontains" => text(|v, o| v.contains(o), true),
        "_nicontains" => !eval_op("_icontains", value, operand),
        "_starts_with" => text(|v, o| v.starts_with(o), false),
        "_nstarts_with" => !eval_op("_starts_with", value, operand),
        "_istarts_with" => text(|v, o| v.starts_with(o), true),
        "_ends_with" => text(|v, o| v.ends_with(o), false),
        "_nends_with" => !eval_op("_ends_with", value, operand),
        "_iends_with" => text(|v, o| v.ends_with(o), true),
        "_between" => match operand.as_array().map(|a| a.as_slice()) {
            Some([lo, hi]) => {
                matches!(cmp(value, lo), Some(Ordering::Greater | Ordering::Equal))
                    && matches!(cmp(value, hi), Some(Ordering::Less | Ordering::Equal))
            }
            _ => false,
        },
        "_nbetween" => !eval_op("_between", value, operand),
        _ => false,
    }
}

/// Parse Directus-style bracket query params (`filter[status][_eq]=published`) into filter JSON.
pub fn parse_bracket_query(query: &str) -> Option<Value> {
    let mut root = Value::Object(Map::new());
    let mut any = false;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let Some(path) = filter_key_path(&key) else {
            continue;
        };
        any = true;
        insert_filter_path(&mut root, &path, parse_query_scalar(&value));
    }
    if any { Some(root) } else { None }
}

/// Combine JSON `filter=` query value with bracket params (both → `_and`).
pub fn merge_query_filters(json_filter: Option<&str>, query: &str) -> Result<Option<Value>, String> {
    let json = match json_filter.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => Some(
            serde_json::from_str::<Value>(raw).map_err(|e| format!("invalid filter JSON: {e}"))?,
        ),
        None => None,
    };
    let bracket = parse_bracket_query(query);
    Ok(and_all(vec![json, bracket]))
}

fn filter_key_path(key: &str) -> Option<Vec<String>> {
    if key == "filter" {
        return None;
    }
    let rest = key.strip_prefix("filter[")?;
    if !rest.ends_with(']') {
        return None;
    }
    let inner = &rest[..rest.len() - 1];
    if inner.is_empty() {
        return None;
    }
    Some(inner.split("][").map(str::to_string).collect())
}

fn parse_query_scalar(raw: &str) -> Value {
    if raw == "true" {
        return Value::Bool(true);
    }
    if raw == "false" {
        return Value::Bool(false);
    }
    if raw == "null" {
        return Value::Null;
    }
    if (raw.starts_with('[') && raw.ends_with(']')) || (raw.starts_with('{') && raw.ends_with('}')) {
        if let Ok(v) = serde_json::from_str(raw) {
            return v;
        }
    }
    // Keep query values as strings (slugs, UUIDs, zero-padded codes); SQL cast handles typing.
    Value::String(raw.to_string())
}

fn insert_filter_path(cur: &mut Value, path: &[String], value: Value) {
    if path.is_empty() {
        *cur = value;
        return;
    }
    let head = &path[0];
    let tail = &path[1..];
    if let Ok(idx) = head.parse::<usize>() {
        if !cur.is_array() {
            *cur = Value::Array(vec![]);
        }
        let arr = cur.as_array_mut().expect("filter path array");
        while arr.len() <= idx {
            arr.push(Value::Null);
        }
        if tail.is_empty() {
            arr[idx] = value;
            return;
        }
        if arr[idx].is_null() {
            arr[idx] = if tail[0].parse::<usize>().is_ok() {
                Value::Array(vec![])
            } else {
                Value::Object(Map::new())
            };
        } else if !tail.is_empty() {
            if tail[0].parse::<usize>().is_ok() && !arr[idx].is_array() {
                arr[idx] = Value::Array(vec![]);
            } else if tail[0].parse::<usize>().is_err() && !arr[idx].is_object() {
                arr[idx] = Value::Object(Map::new());
            }
        }
        insert_filter_path(&mut arr[idx], tail, value);
        return;
    }
    if !cur.is_object() {
        *cur = Value::Object(Map::new());
    }
    let obj = cur.as_object_mut().expect("filter path object");
    let entry = obj.entry(head.clone()).or_insert(Value::Null);
    if tail.is_empty() {
        *entry = value;
        return;
    }
    if entry.is_null() {
        *entry = if tail[0].parse::<usize>().is_ok() {
            Value::Array(vec![])
        } else {
            Value::Object(Map::new())
        };
    } else if tail[0].parse::<usize>().is_ok() && !entry.is_array() {
        *entry = Value::Array(vec![]);
    } else if tail[0].parse::<usize>().is_err() && !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    insert_filter_path(entry, tail, value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn basic_operators() {
        let item = json!({"status": "published", "views": 12, "tags": ["a", "b"], "author": {"name": "Rossi"}});
        assert!(matches(&json!({"status": {"_eq": "published"}}), &item));
        assert!(matches(&json!({"status": "published"}), &item));
        assert!(!matches(&json!({"status": {"_neq": "published"}}), &item));
        assert!(matches(&json!({"views": {"_between": [10, 20]}}), &item));
        assert!(matches(&json!({"views": {"_gt": "11"}}), &item));
        assert!(matches(&json!({"tags": {"_contains": "b"}}), &item));
        assert!(matches(&json!({"author": {"name": {"_icontains": "ROS"}}}), &item));
        assert!(matches(&json!({"_or": [{"views": {"_lt": 1}}, {"missing": {"_null": true}}]}), &item));
        assert!(!matches(&json!({"_and": [{"views": {"_lt": 1}}, {"missing": {"_null": true}}]}), &item));
        assert!(matches(&json!({"status": {"_in": ["draft", "published"]}}), &item));
    }

    #[test]
    fn variables() {
        let mut vars = Map::new();
        vars.insert("$CURRENT_USER".into(), json!("u1"));
        let f = substitute_vars(&json!({"user_created": {"_eq": "$CURRENT_USER"}, "x": {"_eq": "$UNKNOWN"}}), &vars);
        assert_eq!(f, json!({"user_created": {"_eq": "u1"}, "x": {"_eq": null}}));
    }

    #[test]
    fn bracket_query_operators() {
        let q = "filter[status][_eq]=published&fields=id";
        assert_eq!(
            parse_bracket_query(q),
            Some(json!({ "status": { "_eq": "published" } }))
        );
        let q = "filter[status]=published";
        assert_eq!(parse_bracket_query(q), Some(json!({ "status": "published" })));
        let q = "filter[status][_in][0]=draft&filter[status][_in][1]=published";
        assert_eq!(
            parse_bracket_query(q),
            Some(json!({ "status": { "_in": ["draft", "published"] } }))
        );
        let q = "filter[_and][0][status][_eq]=published&filter[_and][1][views][_gt]=10";
        assert_eq!(
            parse_bracket_query(q),
            Some(json!({
                "_and": [
                    { "status": { "_eq": "published" } },
                    { "views": { "_gt": "10" } }
                ]
            }))
        );
        let q = "filter[author][name][_eq]=Ann";
        assert_eq!(
            parse_bracket_query(q),
            Some(json!({ "author": { "name": { "_eq": "Ann" } } }))
        );
        let q = "filter[code][_eq]=007";
        assert_eq!(parse_bracket_query(q), Some(json!({ "code": { "_eq": "007" } })));
        let q = "filter[year][_between][0]=2020&filter[year][_between][1]=2021";
        assert_eq!(
            parse_bracket_query(q),
            Some(json!({ "year": { "_between": ["2020", "2021"] } }))
        );
        let q = "filter[_or][0][status][_eq]=draft&filter[_or][1][status][_eq]=published";
        assert_eq!(
            parse_bracket_query(q),
            Some(json!({
                "_or": [
                    { "status": { "_eq": "draft" } },
                    { "status": { "_eq": "published" } }
                ]
            }))
        );
        assert_eq!(parse_bracket_query("filter[]=ignored&filter[status][_eq]=ok"), Some(json!({ "status": { "_eq": "ok" } })));
        let q = "filter[blocks][_some][collection][_eq]=block_text";
        assert_eq!(
            parse_bracket_query(q),
            Some(json!({ "blocks": { "_some": { "collection": { "_eq": "block_text" } } } }))
        );
    }

    #[test]
    fn merge_json_and_bracket_filters() {
        let q = "filter[title][_eq]=Hello";
        let merged = merge_query_filters(Some(r#"{"status":{"_eq":"published"}}"#), q).unwrap();
        assert_eq!(
            merged,
            Some(json!({
                "_and": [
                    { "status": { "_eq": "published" } },
                    { "title": { "_eq": "Hello" } }
                ]
            }))
        );
    }
}
