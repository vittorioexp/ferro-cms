# Database backends

Ferro stores all CMS data in a relational database. The engine uses **sqlx** with
**PostgreSQL** for the live API; **SQLite** and **MySQL/MariaDB** support system
migrations today (full API on those backends is planned).

## Supported backends

| Backend | URL scheme | Status |
|---------|------------|--------|
| **PostgreSQL** | `postgres://`, `postgresql://` | **Production + API server** (`ferro serve`). Full feature set. |
| **SQLite** | `sqlite://`, `sqlite:` | **System migrations only** via `ferro migrate` (local file / CI). API not yet supported on SQLite. |
| **MySQL / MariaDB** | `mysql://`, `mariadb://` | **System migrations only** (beta DDL shared with SQLite). API not yet supported. |

Configure with **`FERRO_DATABASE_URL`** (required). Optional **`FERRO_DATABASE_KIND`**
(`postgres` \| `sqlite` \| `mysql`) is used when the URL scheme is **not** one Ferro recognizes
(the scheme from a normal `postgres://`, `sqlite:`, or `mysql://` URL always wins).

## Architecture

```
FERRO_DATABASE_URL ──► DatabaseKind::detect()
                              │
         ferro migrate ───────┼──► PgPool | SqlitePool | MySqlPool
                              │         migrate_system*()
                              │
         ferro serve ─────────┘──► PgPool only (full API)
```

- **`ferro_core::DatabaseKind`** — inferred from the URL scheme (`FERRO_DATABASE_KIND` only if the scheme is unknown). `FieldDef::sql_type` / `cast_type_for` support non-Postgres types for a future dialect DDL layer (the engine still applies schema on PostgreSQL only).
- **`ferro_engine::db::migrate_url`** — applies versioned system DDL on Postgres, SQLite, or MySQL.
- **`ferro_engine::db::connect`** — Postgres pool for the API; non-Postgres URLs get migrations then a clear error.

Phase 2 (not done): dialect-aware queries in `items`, `query`, `ddl` so `ferro serve` can run on SQLite/MySQL.

## Feature differences

| Feature | PostgreSQL | SQLite | MySQL |
|---------|------------|--------|-------|
| Transactional schema apply | Yes | Best-effort (SQLite DDL limitations) | Yes |
| `LISTEN/NOTIFY` schema reload | Yes | Poll / manual reload | No (same as SQLite) |
| Realtime fan-out across instances | Yes | In-process only | In-process only |
| Deferrable FKs (sync import) | Yes | Deferred constraints where supported | Yes |
| JSON column type | `jsonb` | `TEXT` / JSON | `JSON` |

## Examples

```bash
# Production (default)
export FERRO_DATABASE_URL=postgres://ferro:ferro@localhost:5432/ferro

# Local file (created on first run)
export FERRO_DATABASE_URL=sqlite://./ferro.db

# In-memory (tests)
export FERRO_DATABASE_URL=sqlite::memory:

# MySQL
export FERRO_DATABASE_URL=mysql://ferro:ferro@127.0.0.1:3306/ferro
```

Run `ferro migrate` to apply system migrations for the detected backend. **`ferro serve` requires PostgreSQL**; other URLs fail after migrate with a pointer to this doc.

## Roadmap

- Optional read replicas / routing (Postgres only).
- Shared test suite matrix in CI for SQLite + MySQL.
- Dialect-specific query optimisations (MySQL fulltext, Postgres GIN).
