# Admin UI

Plain JavaScript SPA under **`/admin`** when `FERRO_ADMIN_DIR=./admin`. No build step. Develop against another API: `/admin/?api=https://host` (CORS on that host).

Code layout: [admin/README.md](../admin/README.md).

## Navigation

| Route | Section | Who |
|-------|---------|-----|
| `#/content` | Dashboard / content home | Users with app access |
| `#/content/:collection` | Item list | Per collection permissions |
| `#/content/:collection/:id` | Item editor | Per collection permissions |
| `#/files` | File library | App access |
| `#/graphql` | GraphQL playground | App access |
| `#/assistant` | AI chat | Signed-in users; needs [AI_ASSISTANT.md](./AI_ASSISTANT.md) on server |
| `#/flows` | Automations | Admin |
| `#/users` | Users & invites | Admin |
| `#/model`, `#/model/:name` | Data model | Admin |
| `#/access` | Roles & policies | Admin |
| `#/schema` | **Environment** (schema + data promotion) | Admin |
| `#/activity` | Audit log | Admin |
| `#/account` | Profile, 2FA | Signed-in |
| `#/reset`, `#/invite`, `#/sso` | Auth flows | Public (before login) |
| `#/ext/:id` | Extension pages | Per extension |

## Content

- Lists: search, filter builder, sort, pagination, presets, bulk delete, realtime banner on changes.
- Editor: form generated from schema; M2O/M2M/O2M/M2A blocks; translations tabs; revisions; comments (workspace).
- Draft/publish when `meta.versioning.enabled` on the collection.

## Data model & access

Every structural change runs **`/schema/plan`** first (dry run), shows SQL summary, then **`/schema/apply`** on confirm. Destructive ops need explicit confirmation.

- **Translations** field type: guided wizard creates languages + translations collection.
- Collection **sync** settings: `natural_key`, `exclude` (never export in data promotion).

## Environment (`#/schema`)

| Block | Action |
|-------|--------|
| Snapshot download | YAML via `/schema/snapshot` |
| Apply JSON file | Plan + apply local snapshot |
| Align to another server | Remote URL + admin token → fetch snapshot → plan/apply **schema** |
| Promote content | Same URL/token → export selection → **upload file binaries** → import **data** ([DATA_SYNC.md](./DATA_SYNC.md)) |
| History | Recent schema applies |

## Assistant (`#/assistant`)

Natural-language queries and optional writes. Toggle **Allow create, update and delete** before the model may call write tools. Uses `context.collection` from the last content collection you opened.

## Files

Folders, upload, metadata, replace binary, download. Thumbnails in grids via `/assets/:id?width=&height=&fit=cover`.

## GraphQL

Editor, variables JSON, run as current user, download SDL from `/graphql/schema`.

## Extensions

Folder per extension under `FERRO_EXTENSIONS_DIR` with `manifest.json` + ES module `register(ferro)`. See [ARCHITECTURE.md](./ARCHITECTURE.md) §15.

## Security notes

- Text rendered via DOM text nodes (no `innerHTML` for user content).
- Refresh token in `localStorage`; access token in memory only.
- Extensions run with the **signed-in user's** privileges.

## Tests

```bash
cd admin && node --test test/*.test.mjs
```

See [tests/README.md](../tests/README.md) for full CI.
