# Configuration (environment variables)

Ferro reads configuration from the process environment. Copy [.env.example](../.env.example) for local development. The authoritative long table is in [ARCHITECTURE.md](./ARCHITECTURE.md) (§13 and env tables); this page groups variables by topic.

## Required for `ferro serve`

| Variable | Description |
|----------|-------------|
| `FERRO_SECRET` | ≥32 chars; JWT and crypto |
| `FERRO_DATABASE_URL` | PostgreSQL URL for the API ([DATABASES.md](./DATABASES.md)) |

## Server core

| Variable | Default | Description |
|----------|---------|-------------|
| `FERRO_BIND` | `0.0.0.0:8055` | Listen address |
| `FERRO_ENV` | `local` | Shown in UI and reports |
| `FERRO_ADMIN_DIR` | — | Path to `admin/` → served at `/admin` |
| `FERRO_EXTENSIONS_DIR` | — | Admin extensions → `/extensions` |
| `FERRO_CORS_ORIGINS` | — | Comma-separated origins or `*` |
| `FERRO_MAX_BODY_MB` | 256 | Max request body |
| `FERRO_QUERY_LIMIT_MAX` | -1 | Cap on client `limit` (-1 = no cap) |
| `FERRO_DB_MAX_CONNECTIONS` | 10 | Pool size |
| `FERRO_DATABASE_KIND` | — | Only if URL scheme is unknown: `postgres` \| `sqlite` \| `mysql` |

## First admin user

| Variable | Description |
|----------|-------------|
| `FERRO_ADMIN_EMAIL` | Created when no users exist |
| `FERRO_ADMIN_PASSWORD` | Initial password |

## Auth, SSO, LDAP, email

See [ARCHITECTURE.md](./ARCHITECTURE.md) §9.

| Prefix / vars | Purpose |
|---------------|---------|
| `FERRO_PUBLIC_URL` | Base URL for links in email and SSO return |
| `FERRO_AUTH_REDIRECT_ALLOWLIST` | Extra redirect origins |
| `FERRO_ACCESS_TOKEN_TTL`, `FERRO_REFRESH_TOKEN_TTL` | Session lifetimes |
| `FERRO_LOGIN_MAX_FAILURES`, `FERRO_LOGIN_LOCKOUT_SECS` | Brute-force limits |
| `FERRO_TFA_ISSUER` | Authenticator app label |
| `FERRO_ENCRYPTION_KEY` | DB-stored secrets (defaults to `FERRO_SECRET`) |
| `FERRO_SMTP_*`, `FERRO_EMAIL_FROM` | Outgoing mail |
| `FERRO_AUTH_PROVIDERS`, `FERRO_AUTH_<NAME>_*` | OAuth/OIDC providers |
| `FERRO_LDAP_*` | Directory login |

## Storage and assets

| Variable | Description |
|----------|-------------|
| `FERRO_STORAGE_*` | Local root and/or S3/Azure locations ([ARCHITECTURE.md](./ARCHITECTURE.md) §10) |
| `FERRO_IMAGE_MAX_DIMENSION`, `FERRO_IMAGE_*` | Transform limits and cache |

## Performance and caching

| Variable | Default | Description |
|----------|---------|-------------|
| `FERRO_RATE_LIMIT_RPM` | 0 | Per-user/IP requests/minute (0=off) |
| `FERRO_HTTP_CACHE_TTL` | 0 | GET `/items/*` cache seconds |
| `FERRO_HTTP_CACHE_MAX_ENTRIES` | 512 | Cache size |

## AI assistant

See [AI_ASSISTANT.md](./AI_ASSISTANT.md).

| Variable | Default |
|----------|---------|
| `FERRO_AI_ENABLED` | false |
| `FERRO_AI_API_KEY` | — |
| `FERRO_AI_BASE_URL` | `https://api.openai.com/v1` |
| `FERRO_AI_MODEL` | `gpt-4o-mini` |
| `FERRO_AI_MAX_ROUNDS` | 12 (clamped 1–32) |

## Flows and automation

| Variable | Description |
|----------|-------------|
| `FERRO_FLOW_ENV_<NAME>` | Secrets in flows as `{{ $env.NAME }}` |
| `FERRO_FLOW_ALLOWED_HOSTS` | Outbound allowlist |
| `FERRO_FLOW_ALLOW_PRIVATE` | Allow loopback/private URLs |

## CLI project (not server)

See [CLI.md](./CLI.md). In **`ferro.toml`**: environment URLs, `token_env`, `protected`. Schema directory via `[project] schema = "schema"`.

## Migrations only (non-Postgres)

`ferro migrate` accepts SQLite/MySQL URLs; **`ferro serve` still requires PostgreSQL**. See [DATABASES.md](./DATABASES.md).
