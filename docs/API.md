# HTTP API overview

Base URL: your Ferro server (default `http://localhost:8055`). Responses use a **`{ "data": … }`** envelope unless noted. Errors: `{ "errors": [{ "message", "extensions": { "code" } }] }`.

Full design: [ARCHITECTURE.md](./ARCHITECTURE.md) §8–§10. Machine-readable subset: **`GET /server/openapi.json`**.

## Authentication

| Method | Usage |
|--------|--------|
| Bearer JWT | `Authorization: Bearer <access_token>` from `POST /auth/login` |
| Refresh | `POST /auth/refresh` with `refresh_token` |
| Static token | Long-lived token on user row; same Bearer header |
| Query token | `?access_token=` (assets, some tools) |

Permissions: **policies** on collections (read/create/update/delete, field lists, item filters). **`admin_access`** required for schema, sync, users, and most model editing.

## Core REST

| Area | Prefix | Notes |
|------|--------|--------|
| Items | `GET/POST/PATCH/DELETE /items/:collection` | Filters: [FILTERS.md](./FILTERS.md) |
| Item | `GET/PATCH/DELETE /items/:collection/:id` | Nested writes, drafts (`?version=draft`) |
| Collections / fields | `/collections`, `/fields/...` | Admin model (plan/apply pipeline) |
| Schema | `/schema/snapshot`, `/schema/plan`, `/schema/apply` | Snapshot as code |
| Files | `/files`, `/assets/:id` | Upload, transforms on assets |
| Users / auth | `/auth/*`, `/users` | SSO, LDAP, 2FA, invite |
| Flows | `/flows`, `/hooks/:id`, `/x/:slug` | Automations |
| GraphQL | `POST /graphql`, `GET /graphql/schema` | Same permissions as REST |
| Realtime | `GET /websocket` | Subscriptions |
| Workspace | `/comments`, `/notifications`, `/presets`, `/dashboard`, `/insights` | Admin collaboration |
| Activity | `/activity`, `/revisions` | Audit |

## Environment sync (administrator)

Promote **content and file binaries** between servers. See [DATA_SYNC.md](./DATA_SYNC.md).

| Method | Path | Body / response |
|--------|------|-----------------|
| `POST` | `/sync/export` | `{ "selections": [ Selection ] }` → `{ "data": Bundle }` |
| `POST` | `/sync/missing-files` | `[ { "id", "checksum"? } ]` → `{ "data": [ uuid, … ] }` |
| `GET` | `/sync/files/:id` | Raw bytes (`application/octet-stream`) |
| `PUT` | `/sync/files/:id` | Raw bytes; stores blob before import |
| `POST` | `/sync/import` | `{ "bundle", "options": ImportOptions }` → import report |
| `DELETE` | `/sync/state` | Optional `{ "collection" }`; clears sync hashes for `safe` strategy |

**Selection** (export):

```json
{
  "collection": "pages",
  "filter": { "slug": { "_eq": "home" } },
  "ids": [],
  "with_dependencies": true
}
```

CLI shorthand: `pages`, `pages:id=<uuid>,…`, `pages:{"slug":{"_eq":"home"}}`, flag `--shallow` sets `with_dependencies: false`.

**ImportOptions**:

| Field | Default | Meaning |
|-------|---------|---------|
| `strategy` | `overwrite` | `overwrite` \| `safe` \| `insert-only` |
| `prune` | false | Remove owned children gone from source |
| `dry_run` | false | Roll back; report is the diff |
| `ignore_missing_fields` | false | Strip unknown fields instead of error |
| `force` | false | Apply despite conflicts |

## AI assistant (signed-in user)

See [AI_ASSISTANT.md](./AI_ASSISTANT.md).

| Method | Path |
|--------|------|
| `GET` | `/assistant/config` |
| `POST` | `/assistant/chat` |

## Health and info

| Method | Path |
|--------|------|
| `GET` | `/server/health` |
| `GET` | `/server/info` | Environment name, schema hash, user when authenticated |
| `GET` | `/server/extensions` | Admin extension manifest list |

## JavaScript SDK

See [sdk/javascript/README.md](../sdk/javascript/README.md).
