//! OpenAI-compatible chat completions with tool loop.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::assistant::{floor_char_boundary, system_prompt, ChatOptions, ChatReply, ChatTurn, AssistantConfig};
use crate::assistant::tools::{self, execute};
use crate::items::Ctx;
use crate::schema::Schema;
use crate::{FerroError, Result};

#[derive(Debug, Clone)]
pub struct AssistantLlmConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
}

impl AssistantLlmConfig {
    pub fn configured(&self) -> bool {
        self.api_key.as_ref().is_some_and(|k| !k.is_empty()) && !self.model.trim().is_empty()
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<OaiMessage>,
    tools: Value,
    tool_choice: &'static str,
}

#[derive(Serialize, Deserialize, Clone)]
struct OaiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OaiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct OaiToolCall {
    #[serde(default)]
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: OaiFunctionCall,
}

#[derive(Serialize, Deserialize, Clone)]
struct OaiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: OaiMessage,
}

pub async fn run(
    pool: &PgPool,
    schema: Arc<Schema>,
    cfg: &AssistantConfig,
    ctx: Ctx,
    user_messages: Vec<ChatTurn>,
    opts: ChatOptions,
) -> Result<ChatReply> {
    let sys = system_prompt(&schema, &ctx, &opts.context, opts.allow_writes);
    let mut oai_messages: Vec<OaiMessage> = vec![OaiMessage {
        role: "system".into(),
        content: Some(sys),
        tool_calls: None,
        tool_call_id: None,
    }];
    for m in user_messages {
        if m.role != "user" && m.role != "assistant" {
            continue;
        }
        oai_messages.push(OaiMessage {
            role: m.role,
            content: m.content,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let tools = tools::tool_definitions();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| FerroError::Internal(e.to_string()))?;

    let mut steps: Vec<String> = vec![];

    for _ in 0..cfg.max_rounds {
        let req = ChatRequest {
            model: &cfg.llm.model,
            messages: oai_messages.clone(),
            tools: tools.clone(),
            tool_choice: "auto",
        };
        let url = format!("{}/chat/completions", cfg.llm.base_url.trim_end_matches('/'));
        let res = client
            .post(&url)
            .bearer_auth(cfg.llm.api_key.as_deref().unwrap_or(""))
            .json(&req)
            .send()
            .await
            .map_err(|e| FerroError::Internal(format!("AI request failed: {e}")))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(FerroError::Internal(format!("AI provider returned {status}: {body}")));
        }

        let parsed: ChatResponse = res.json().await.map_err(|e| FerroError::Internal(format!("AI response parse error: {e}")))?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            return Err(FerroError::Internal("AI provider returned no choices".into()));
        };
        let assistant = choice.message;

        if let Some(calls) = &assistant.tool_calls {
            if !calls.is_empty() {
                oai_messages.push(assistant.clone());
                let mut ran = 0usize;
                for call in calls {
                    if call.kind != "function" {
                        continue;
                    }
                    ran += 1;
                    let tool_id = if call.id.is_empty() {
                        format!("call_{}_{ran}", call.function.name)
                    } else {
                        call.id.clone()
                    };
                    let args: Value = serde_json::from_str(&call.function.arguments).unwrap_or(json!({}));
                    steps.push(call.function.name.clone());
                    let result = match execute(pool, &ctx, &opts, &call.function.name, args).await {
                        Ok(v) => v,
                        Err(e) => json!({ "error": true, "message": e.to_string() }),
                    };
                    let mut content = serde_json::to_string(&result).unwrap_or_else(|_| result.to_string());
                    const MAX_TOOL: usize = 14_000;
                    if content.len() > MAX_TOOL {
                        content.truncate(floor_char_boundary(&content, MAX_TOOL));
                        content.push_str("…");
                    }
                    oai_messages.push(OaiMessage {
                        role: "tool".into(),
                        content: Some(content),
                        tool_calls: None,
                        tool_call_id: Some(tool_id),
                    });
                }
                if ran > 0 {
                    continue;
                }
            }
        }

        let content = assistant.content.unwrap_or_default();
        return Ok(ChatReply {
            message: ChatTurn { role: "assistant".into(), content: Some(content) },
            steps: if steps.is_empty() { None } else { Some(steps) },
        });
    }

    Err(FerroError::invalid("assistant exceeded maximum tool rounds; try a simpler question"))
}
