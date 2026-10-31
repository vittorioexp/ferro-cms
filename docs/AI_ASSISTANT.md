# AI assistant

Natural-language chat in the admin UI to **read and optionally write** CMS data. All tool calls use the same permission engine as REST (`Accountability` on every request).

Related: [ADMIN.md](./ADMIN.md) (`#/assistant`), [API.md](./API.md), [CONFIGURATION.md](./CONFIGURATION.md).

## Enable

Set on the API server (**PostgreSQL** only, same as `ferro serve`):

| Variable | Description |
|----------|-------------|
| `FERRO_AI_ENABLED` | `true` to expose `/assistant/*` (default `false`) |
| `FERRO_AI_API_KEY` | Bearer token for an OpenAI-compatible chat API |
| `FERRO_AI_BASE_URL` | API root (default `https://api.openai.com/v1`) |
| `FERRO_AI_MODEL` | Model id (default `gpt-4o-mini`; must be non-empty when enabled) |
| `FERRO_AI_MAX_ROUNDS` | Max tool-call loops per message (default `12`, clamped **1–32** on the server) |

Without `FERRO_AI_ENABLED` and a key, `GET /assistant/config` reports `enabled: false` and the admin shows setup instructions.

Example `.env`:

```env
FERRO_AI_ENABLED=true
FERRO_AI_API_KEY=sk-...
# FERRO_AI_BASE_URL=https://api.openai.com/v1
# FERRO_AI_MODEL=gpt-4o-mini
```

Compatible providers: OpenAI, Azure OpenAI (custom base URL + key), local OpenAI-compatible proxies (LM Studio, Ollama with compatible layer, etc.).

## Admin usage

1. Open **Library → Assistant** (`#/assistant`).
2. Ask questions in plain language (“List published pages”, “How many items in …?”).
3. Optional: enable **Allow create, update and delete** before the model may mutate data.
4. Tool names used appear under replies when the server ran tools (`steps` in API response).

**Context:** After you browse a collection under **Content**, the server receives `context.collection` with that name so answers prefer the collection you were working in.

## Security

- Requires a **signed-in user** (Bearer token). Anonymous public role cannot call `/assistant/chat`.
- **Read tools** enforce collection **Read** permission (or admin).
- **`list_collections`** matches `GET /collections` visibility (app/admin/read).
- **Create / update / delete** run only when the client sends `allow_writes: true` (checkbox in UI). Without it, write tools return a `writes_disabled` payload to the model.
- No schema editing, user management, SQL, or flows from the assistant.
- Message size limits: 16k chars per message, 200k total history; last message must be a non-empty **user** message.
- Tool results truncated server-side before returning to the LLM.

## HTTP API

### `GET /assistant/config`

Response:

```json
{
  "data": {
    "enabled": true,
    "model": "gpt-4o-mini"
  }
}
```

No secrets exposed. Requires signed-in user.

### `POST /assistant/chat`

Request:

```json
{
  "messages": [
    { "role": "user", "content": "List up to 5 pages" },
    { "role": "assistant", "content": "…" },
    { "role": "user", "content": "…" }
  ],
  "allow_writes": false,
  "context": { "collection": "pages" }
}
```

Response:

```json
{
  "data": {
    "message": { "role": "assistant", "content": "…" },
    "steps": ["list_collections", "list_items"]
  }
}
```

Returns **503** with code `UNAVAILABLE` when AI is not configured.

## Tools (server-side)

The model can call these functions; execution goes through `ferro_engine::items` and permissions.

| Tool | Purpose |
|------|---------|
| `list_collections` | Names visible in admin / readable collections list |
| `describe_collection` | Field names, types, required, notes (Read on collection) |
| `list_items` | List with filter, search, sort, limit (max 50) |
| `get_item` | One item by id |
| `create_item` | Insert (requires `allow_writes`) |
| `update_item` | Patch fields (requires `allow_writes`) |
| `delete_item` | Delete by id (requires `allow_writes`) |

Filters use the same JSON as REST ([FILTERS.md](./FILTERS.md)).

## Implementation notes

- Module: `crates/ferro-engine/src/assistant/` (LLM loop + tools).
- Routes: `crates/ferro-server/src/routes/assistant.rs`.
- UI: `admin/js/views/assistant.js`.
- Errors from tools are returned to the model as JSON so it can explain failures to the user.

## Limitations

- No streaming responses in v1 (full reply after tool loop completes).
- No attachment / image understanding.
- Rate limits: global `FERRO_RATE_LIMIT_RPM` applies to chat like other authenticated routes.
- Costs and data sent to the LLM provider are your responsibility; review provider policies for PII in content fields.
