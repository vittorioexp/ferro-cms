# Ferro documentation

Start here for guides. The long-form design reference is **[ARCHITECTURE.md](./ARCHITECTURE.md)** (model, API, auth, flows, deployment, CI).

## Guides by topic

| Guide | What it covers |
|--------|----------------|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Full system design: schema-as-code, sync, REST, GraphQL, flows, realtime, security, CLI, admin overview |
| [CONFIGURATION.md](./CONFIGURATION.md) | Environment variables grouped by area (database, auth, storage, AI, sync, …) |
| [DATABASES.md](./DATABASES.md) | PostgreSQL vs SQLite/MySQL for `ferro migrate` vs `ferro serve` |
| [DATA_SYNC.md](./DATA_SYNC.md) | Content promotion between environments **including file assets** (CLI, HTTP API, admin) |
| [ENVIRONMENT.md](./ENVIRONMENT.md) | `ferro.toml`, schema align vs data push, protected environments |
| [FILTERS.md](./FILTERS.md) | REST query filters: JSON and bracket syntax, operators, permissions |
| [API.md](./API.md) | REST conventions, auth, main routes, OpenAPI, sync and assistant endpoints |
| [AI_ASSISTANT.md](./AI_ASSISTANT.md) | Natural-language admin chat, tools, security, configuration |
| [ADMIN.md](./ADMIN.md) | Admin UI routes, content, model, Environment page, Assistant, extensions |
| [CLI.md](./CLI.md) | `ferro` CLI: schema, data, migrate, `ferro.toml` |

## Other docs in the repo

| Location | Purpose |
|----------|---------|
| [README.md](../README.md) | Project overview, quick start, typical CLI flows |
| [admin/README.md](../admin/README.md) | Admin app layout and local tests |
| [tests/README.md](../tests/README.md) | Integration and e2e test suites |
| [sdk/javascript/README.md](../sdk/javascript/README.md) | JavaScript SDK |
| [.env.example](../.env.example) | Minimal server env template |

## OpenAPI

With the server running: `GET /server/openapi.json` (subset of routes; full behaviour in ARCHITECTURE and API.md).

## Typical reading order

1. [README.md](../README.md) quick start  
2. [ENVIRONMENT.md](./ENVIRONMENT.md) + [DATA_SYNC.md](./DATA_SYNC.md) if you promote dev → prod  
3. [CONFIGURATION.md](./CONFIGURATION.md) when deploying  
4. [ARCHITECTURE.md](./ARCHITECTURE.md) when extending the model or integrations  
