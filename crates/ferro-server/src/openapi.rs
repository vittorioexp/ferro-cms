//! OpenAPI 3.1 description of the public HTTP API (subset).

use axum::Json;
use serde_json::{json, Value};

use crate::{ApiResult, AppState};
use axum::extract::State;

pub fn spec(base_url: &str) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Ferro API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Headless CMS REST API. Authenticate with `Authorization: Bearer <access_token>`."
        },
        "servers": [{ "url": base_url }],
        "paths": {
            "/server/health": { "get": { "summary": "Health check" } },
            "/server/info": { "get": { "summary": "Environment info (authenticated)" } },
            "/server/openapi.json": { "get": { "summary": "This document" } },
            "/auth/login": { "post": { "summary": "Sign in (email + password)" } },
            "/items/{collection}": {
                "get": { "summary": "List items", "parameters": [
                    { "name": "collection", "in": "path", "required": true, "schema": { "type": "string" } },
                    { "name": "filter", "in": "query", "description": "JSON object or use bracket params e.g. filter[status][_eq]=published", "schema": { "type": "string" } },
                    { "name": "sort", "in": "query", "schema": { "type": "string" } },
                    { "name": "limit", "in": "query", "schema": { "type": "integer" } },
                    { "name": "meta", "in": "query", "description": "total_count, filter_count", "schema": { "type": "string" } }
                ]},
                "post": { "summary": "Create item(s)" }
            },
            "/items/{collection}/aggregate": {
                "post": {
                    "summary": "Aggregate (count, sum, avg, min, max) with optional group_by",
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "filter": { "type": "object" },
                                        "search": { "type": "string" },
                                        "group_by": { "type": "array", "items": { "type": "string" } },
                                        "ops": {
                                            "type": "array",
                                            "items": {
                                                "type": "object",
                                                "required": ["alias", "function"],
                                                "properties": {
                                                    "alias": { "type": "string" },
                                                    "function": { "enum": ["count", "sum", "avg", "min", "max"] },
                                                    "field": { "type": "string" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/graphql": { "post": { "summary": "GraphQL query or mutation" } },
            "/assistant/config": { "get": { "summary": "AI assistant availability (no secrets)" } },
            "/assistant/chat": { "post": { "summary": "Natural-language chat with data tools (see docs/AI_ASSISTANT.md)" } },
            "/sync/export": { "post": { "summary": "Export a data bundle (admin); response in data" } },
            "/sync/missing-files": { "post": { "summary": "List file ids whose binaries must be uploaded" } },
            "/sync/files/{id}": { "get": { "summary": "Download file binary for sync" }, "put": { "summary": "Upload file binary before import" } },
            "/sync/import": { "post": { "summary": "Import bundle (dry_run supported)" } },
            "/sync/state": { "delete": { "summary": "Reset sync state for safe strategy" } }
        },
        "components": {
            "securitySchemes": {
                "bearerAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "JWT" }
            }
        },
        "security": [{ "bearerAuth": [] }]
    })
}

pub async fn openapi_json(State(st): State<AppState>) -> ApiResult<Json<Value>> {
    let base = format!("http://{}", st.config.bind);
    Ok(Json(spec(&base)))
}
