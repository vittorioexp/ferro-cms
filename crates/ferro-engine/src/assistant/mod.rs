//! Natural-language assistant: tool execution against items + OpenAI-compatible chat loop.

mod llm;
mod tools;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::items::Ctx;
use crate::schema::Schema;
use crate::{FerroError, Result};

pub use llm::AssistantLlmConfig;

#[derive(Debug, Clone)]
pub struct AssistantConfig {
    pub enabled: bool,
    pub llm: AssistantLlmConfig,
    pub max_rounds: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ChatContext {
    pub collection: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatTurn {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatReply {
    pub message: ChatTurn,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ChatOptions {
    pub allow_writes: bool,
    pub context: ChatContext,
}

pub async fn chat(
    pool: &PgPool,
    schema: Arc<Schema>,
    cfg: &AssistantConfig,
    ctx: Ctx,
    messages: Vec<ChatTurn>,
    opts: ChatOptions,
) -> Result<ChatReply> {
    if !cfg.enabled {
        return Err(FerroError::invalid("AI assistant is not enabled on this server"));
    }
    if !cfg.llm.configured() {
        return Err(FerroError::invalid("AI assistant is missing FERRO_AI_API_KEY or FERRO_AI_MODEL"));
    }
    llm::run(pool, schema, cfg, ctx, messages, opts).await
}

pub(crate) fn system_prompt(schema: &Schema, ctx: &Ctx, chat_ctx: &ChatContext, allow_writes: bool) -> String {
    let mut lines = vec![
        "You are the Ferro CMS assistant. Help users find and manage their content.".into(),
        "Use the provided tools to read or change data; never invent item ids or field names.".into(),
        "Filters use Ferro/REST JSON, e.g. {\"status\":{\"_eq\":\"published\"}}.".into(),
        "Prefer small limits when listing. Summarize results clearly.".into(),
    ];
    if !allow_writes {
        lines.push("The user has NOT enabled writes: do not call create_item, update_item, or delete_item.".into());
    }
    if let Some(c) = &chat_ctx.collection {
        lines.push(format!("The user is viewing collection `{c}` — prefer that collection when relevant."));
    }
    let names = tools::readable_collection_names(schema, &ctx.acc);
    if !names.is_empty() {
        lines.push(format!("Collections this user can read items from include: {}.", names.join(", ")));
    }
    lines.join("\n")
}

pub(crate) fn truncate_value(v: Value, max_chars: usize) -> Value {
    let s = v.to_string();
    if s.len() <= max_chars {
        return v;
    }
    let end = floor_char_boundary(&s, max_chars);
    json!({
        "_truncated": true,
        "preview": &s[..end],
        "note": format!("output truncated from {} bytes", s.len())
    })
}

pub(crate) fn floor_char_boundary(s: &str, max_bytes: usize) -> usize {
    if s.len() <= max_bytes {
        return s.len();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_long_json() {
        let big = json!({ "a": "x".repeat(20_000) });
        let t = truncate_value(big, 100);
        assert_eq!(t.get("_truncated"), Some(&json!(true)));
    }

    #[test]
    fn truncate_respects_utf8() {
        let s = "é".repeat(50); // 2 bytes per char in UTF-8
        let t = truncate_value(json!(s), 3);
        assert_eq!(t.get("_truncated"), Some(&json!(true)));
        let preview = t.get("preview").and_then(|v| v.as_str()).unwrap();
        assert!(preview.is_char_boundary(preview.len()));
    }
}
