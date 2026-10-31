//! AI assistant HTTP API.

use axum::http::StatusCode;
use axum::extract::State;
use axum::Json;
use ferro_engine::assistant::{self, AssistantConfig, ChatContext, ChatOptions, ChatTurn};
use ferro_engine::assistant::AssistantLlmConfig;
use ferro_engine::Accountability;
use ferro_engine::items::Ctx;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::extract::Acc;
use crate::{ApiError, ApiResult, AppState};

impl AppState {
    pub fn assistant_config(&self) -> AssistantConfig {
        AssistantConfig {
            enabled: self.config.ai_enabled,
            llm: AssistantLlmConfig {
                api_key: self.config.ai_api_key.clone(),
                base_url: self.config.ai_base_url.clone(),
                model: self.config.ai_model.clone(),
            },
            max_rounds: self.config.ai_max_rounds,
        }
    }
}

const MAX_MESSAGE_CHARS: usize = 16_000;
const MAX_HISTORY_CHARS: usize = 200_000;

fn require_signed_in(acc: &Accountability) -> ApiResult<()> {
    if acc.user.is_some() {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "sign in to use the assistant",
        ))
    }
}

fn validate_messages(messages: &[ChatTurn]) -> ApiResult<()> {
    let mut total = 0usize;
    for m in messages {
        if m.role != "user" && m.role != "assistant" {
            return Err(ApiError::bad_request("each message role must be user or assistant"));
        }
        let len = m.content.as_deref().unwrap_or("").len();
        if len > MAX_MESSAGE_CHARS {
            return Err(ApiError::bad_request("a message exceeds the maximum length"));
        }
        total += len;
    }
    if total > MAX_HISTORY_CHARS {
        return Err(ApiError::bad_request("conversation is too long"));
    }
    match messages.last() {
        Some(m) if m.role == "user" && m.content.as_deref().is_some_and(|c| !c.trim().is_empty()) => Ok(()),
        _ => Err(ApiError::bad_request("the last message must be a non-empty user message")),
    }
}

pub async fn config(State(st): State<AppState>, Acc(acc): Acc) -> ApiResult<Json<Value>> {
    require_signed_in(&acc)?;
    let cfg = st.assistant_config();
    Ok(Json(json!({
        "data": {
            "enabled": cfg.enabled && cfg.llm.configured(),
            "model": if cfg.enabled && cfg.llm.configured() {
                cfg.llm.model.clone()
            } else {
                String::new()
            },
        }
    })))
}

#[derive(Debug, Deserialize)]
pub struct ChatContextBody {
    #[serde(default)]
    collection: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatBody {
    messages: Vec<ChatTurn>,
    #[serde(default)]
    allow_writes: bool,
    #[serde(default)]
    context: Option<ChatContextBody>,
}

pub async fn chat(State(st): State<AppState>, Acc(acc): Acc, Json(body): Json<ChatBody>) -> ApiResult<Json<Value>> {
    require_signed_in(&acc)?;
    if body.messages.is_empty() {
        return Err(ApiError::bad_request("messages required"));
    }
    if body.messages.len() > 40 {
        return Err(ApiError::bad_request("too many messages in one request"));
    }
    validate_messages(&body.messages)?;
    let cfg = st.assistant_config();
    if !cfg.enabled || !cfg.llm.configured() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "UNAVAILABLE",
            "AI assistant is not configured (see docs/AI_ASSISTANT.md)",
        ));
    }
    let ctx = Ctx::new(st.schema.get(), acc);
    let opts = ChatOptions {
        allow_writes: body.allow_writes,
        context: ChatContext {
            collection: body
                .context
                .and_then(|c| c.collection)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .filter(|name| st.schema.get().collection(name).is_ok()),
        },
    };
    let reply = assistant::chat(&st.pool, st.schema.get(), &cfg, ctx, body.messages, opts).await?;
    Ok(Json(json!({ "data": reply })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_empty_conversation() {
        assert!(validate_messages(&[]).is_err());
    }

    #[test]
    fn validate_requires_last_user_message() {
        let msgs = vec![
            ChatTurn { role: "user".into(), content: Some("hi".into()) },
            ChatTurn { role: "assistant".into(), content: Some("hello".into()) },
        ];
        assert!(validate_messages(&msgs).is_err());
    }

    #[test]
    fn validate_accepts_normal_turn() {
        let msgs = vec![
            ChatTurn { role: "user".into(), content: Some("hi".into()) },
            ChatTurn { role: "assistant".into(), content: Some("hello".into()) },
            ChatTurn { role: "user".into(), content: Some("list posts".into()) },
        ];
        validate_messages(&msgs).unwrap();
    }
}
