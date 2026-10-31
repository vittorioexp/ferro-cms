pub use sqlx::postgres::PgPool;
use ferro_core::database::DatabaseKind;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

use crate::{FerroError, Result};

#[path = "db_sqlite.rs"]
mod db_sqlite;

/// Connect to **PostgreSQL** and ensure system migrations are applied (API server path).
pub async fn connect(url: &str, kind_override: Option<&str>, max_connections: u32) -> Result<PgPool> {
    let kind = DatabaseKind::detect(url, kind_override).map_err(|e| FerroError::Invalid(e.0))?;
    if kind != DatabaseKind::Postgres {
        migrate_url(url, kind_override).await?;
        return Err(FerroError::Invalid(format!(
            "the API server requires PostgreSQL (got `{}`). \
             System migrations were applied; use a postgres:// URL to run `ferro serve`, or see docs/DATABASES.md",
            kind.as_str()
        )));
    }
    let pool = PgPoolOptions::new().max_connections(max_connections).connect(url).await?;
    migrate_system(&pool).await?;
    Ok(pool)
}

/// Apply system migrations for any supported backend (CLI `ferro migrate`, CI).
pub async fn migrate_url(url: &str, kind_override: Option<&str>) -> Result<DatabaseKind> {
    let kind = DatabaseKind::detect(url, kind_override).map_err(|e| FerroError::Invalid(e.0))?;
    match kind {
        DatabaseKind::Postgres => {
            let pool = PgPoolOptions::new().max_connections(2).connect(url).await?;
            migrate_system(&pool).await?;
        }
        DatabaseKind::Sqlite => {
            let pool = sqlite_pool(url, 2).await?;
            migrate_system_sqlite(&pool).await?;
        }
        DatabaseKind::Mysql => {
            let pool = MySqlPoolOptions::new().max_connections(2).connect(url).await?;
            migrate_system_mysql(&pool).await?;
        }
    }
    Ok(kind)
}

async fn migrate_system_sqlite(pool: &sqlx::SqlitePool) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ferro_system_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT (datetime('now')))",
    )
    .execute(&mut *tx)
    .await?;
    let current: Option<i32> = sqlx::query_scalar("SELECT max(version) FROM ferro_system_migrations")
        .fetch_one(&mut *tx)
        .await?;
    let current = current.unwrap_or(0);
    for (version, sql) in db_sqlite::MIGRATIONS {
        if *version <= current {
            continue;
        }
        tracing::info!(version, backend = "sqlite", "applying system migration");
        sqlx::raw_sql(sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO ferro_system_migrations (version) VALUES (?)")
            .bind(*version)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn migrate_system_mysql(pool: &sqlx::MySqlPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    let locked: Option<i32> = sqlx::query_scalar("SELECT GET_LOCK('ferro_migrate', 30)")
        .fetch_one(&mut *tx)
        .await?;
    if locked != Some(1) {
        return Err(FerroError::Invalid(
            "could not acquire MySQL migration lock (GET_LOCK returned 0 or null)".into(),
        ));
    }
    let result = async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS ferro_system_migrations (version INT PRIMARY KEY, applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&mut *tx)
        .await?;
        let current: Option<i32> = sqlx::query_scalar("SELECT max(version) FROM ferro_system_migrations")
            .fetch_one(&mut *tx)
            .await?;
        let current = current.unwrap_or(0);
        for (version, sql) in db_sqlite::MIGRATIONS {
            if *version <= current {
                continue;
            }
            tracing::info!(version, backend = "mysql", "applying system migration");
            // Beta: SQLite-oriented DDL; production MySQL should get a dedicated migration set later.
            sqlx::raw_sql(sql).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO ferro_system_migrations (version) VALUES (?)")
                .bind(*version)
                .execute(&mut *tx)
                .await?;
        }
        Ok::<(), FerroError>(())
    }
    .await;
    let _ = sqlx::query("SELECT RELEASE_LOCK('ferro_migrate')")
        .execute(&mut *tx)
        .await;
    result?;
    tx.commit().await?;
    Ok(())
}

fn ensure_sqlite_parent_dir(url: &str) {
    let lower = url.to_ascii_lowercase();
    if lower.contains(":memory:") || lower.contains("mode=memory") {
        return;
    }
    let path_part = url
        .trim_start_matches("sqlite:")
        .trim_start_matches("//");
    if path_part.is_empty() {
        return;
    }
    let file = path_part.split(['?', '#']).next().unwrap_or(path_part);
    if let Some(parent) = std::path::Path::new(file).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
}

async fn sqlite_pool(url: &str, max_connections: u32) -> Result<sqlx::SqlitePool> {
    let mut opts =
        SqliteConnectOptions::from_str(url).map_err(|e| FerroError::Invalid(format!("invalid sqlite URL: {e}")))?;
    if !url.to_ascii_lowercase().contains(":memory:") && !url.to_ascii_lowercase().contains("mode=memory") {
        opts = opts.create_if_missing(true);
        ensure_sqlite_parent_dir(url);
    }
    Ok(SqlitePoolOptions::new().max_connections(max_connections).connect_with(opts).await?)
}

/// Versioned DDL of the system tables. Append only: never edit a released entry.
const SYSTEM_MIGRATIONS: &[(i32, &str)] = &[(
    1,
    r#"
CREATE TABLE ferro_collections (
    name        text PRIMARY KEY,
    definition  jsonb NOT NULL
);
CREATE TABLE ferro_roles (
    id          uuid PRIMARY KEY,
    definition  jsonb NOT NULL
);
CREATE TABLE ferro_policies (
    id          uuid PRIMARY KEY,
    definition  jsonb NOT NULL
);
CREATE TABLE ferro_schema_history (
    id          uuid PRIMARY KEY,
    applied_at  timestamptz NOT NULL DEFAULT now(),
    applied_by  uuid,
    from_hash   text NOT NULL,
    to_hash     text NOT NULL,
    plan        jsonb NOT NULL,
    snapshot    jsonb NOT NULL
);
CREATE TABLE ferro_users (
    id            uuid PRIMARY KEY,
    email         varchar(255) NOT NULL UNIQUE,
    password_hash text,
    first_name    varchar(255),
    last_name     varchar(255),
    role          uuid,
    status        varchar(32) NOT NULL DEFAULT 'active',
    language      varchar(32),
    token_hash    text UNIQUE,
    last_access   timestamptz,
    date_created  timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE ferro_sessions (
    token_hash  text PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    expires     timestamptz NOT NULL,
    created     timestamptz NOT NULL DEFAULT now(),
    ip          text,
    user_agent  text
);
CREATE TABLE ferro_folders (
    id      uuid PRIMARY KEY,
    name    varchar(255) NOT NULL,
    parent  uuid REFERENCES ferro_folders(id) ON DELETE SET NULL DEFERRABLE INITIALLY IMMEDIATE
);
CREATE TABLE ferro_files (
    id                uuid PRIMARY KEY,
    storage           varchar(64) NOT NULL,
    filename_disk     varchar(255),
    filename_download varchar(255) NOT NULL,
    title             varchar(255),
    description       text,
    type              varchar(255),
    folder            uuid REFERENCES ferro_folders(id) ON DELETE SET NULL DEFERRABLE INITIALLY IMMEDIATE,
    filesize          bigint,
    checksum          varchar(64),
    width             integer,
    height            integer,
    tags              jsonb,
    metadata          jsonb,
    uploaded_by       uuid REFERENCES ferro_users(id) ON DELETE SET NULL DEFERRABLE INITIALLY IMMEDIATE,
    uploaded_on       timestamptz NOT NULL DEFAULT now(),
    modified_on       timestamptz
);
CREATE TABLE ferro_activity (
    id          uuid PRIMARY KEY,
    action      varchar(32) NOT NULL,
    "user"      uuid REFERENCES ferro_users(id) ON DELETE SET NULL,
    timestamp   timestamptz NOT NULL DEFAULT now(),
    ip          varchar(64),
    user_agent  text,
    collection  varchar(64) NOT NULL,
    item        varchar(255) NOT NULL,
    origin      varchar(32),
    comment     text
);
CREATE INDEX ferro_activity_item_idx ON ferro_activity (collection, item);
CREATE TABLE ferro_revisions (
    id          uuid PRIMARY KEY,
    activity    uuid NOT NULL REFERENCES ferro_activity(id) ON DELETE CASCADE,
    collection  varchar(64) NOT NULL,
    item        varchar(255) NOT NULL,
    data        jsonb,
    delta       jsonb
);
CREATE INDEX ferro_revisions_item_idx ON ferro_revisions (collection, item);
CREATE TABLE ferro_sync_state (
    collection  varchar(64) NOT NULL,
    item        uuid NOT NULL,
    source      varchar(255),
    hash        varchar(64) NOT NULL,
    synced_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (collection, item)
);
CREATE TABLE ferro_settings (
    key    text PRIMARY KEY,
    value  jsonb NOT NULL
);
"#,
), (
    2,
    r#"
CREATE TABLE ferro_flows (
    id          uuid PRIMARY KEY,
    definition  jsonb NOT NULL
);
CREATE TABLE ferro_flow_queue (
    id         uuid PRIMARY KEY,
    flow_id    uuid NOT NULL,
    trigger    jsonb NOT NULL,
    status     varchar(16) NOT NULL DEFAULT 'pending',
    attempts   integer NOT NULL DEFAULT 0,
    created    timestamptz NOT NULL DEFAULT now(),
    started    timestamptz,
    run_at     timestamptz NOT NULL DEFAULT now(),
    dedupe_key text UNIQUE
);
CREATE INDEX ferro_flow_queue_pending_idx ON ferro_flow_queue (status, run_at);
CREATE TABLE ferro_flow_runs (
    id         uuid PRIMARY KEY,
    flow_id    uuid NOT NULL,
    flow_name  text,
    source     varchar(16) NOT NULL,
    status     varchar(16) NOT NULL,
    started    timestamptz NOT NULL,
    finished   timestamptz NOT NULL,
    trigger    jsonb,
    log        jsonb,
    error      text
);
CREATE INDEX ferro_flow_runs_flow_idx ON ferro_flow_runs (flow_id, started DESC);
"#,
), (
    3,
    r#"
ALTER TABLE ferro_users
    ADD COLUMN provider            varchar(64),
    ADD COLUMN external_identifier varchar(255),
    ADD COLUMN tfa_secret          text,
    ADD COLUMN tfa_last_step       bigint,
    ADD COLUMN tfa_recovery        jsonb,
    ADD COLUMN tfa_enabled         boolean GENERATED ALWAYS AS (tfa_secret IS NOT NULL) STORED;
CREATE UNIQUE INDEX ferro_users_external_idx ON ferro_users (provider, external_identifier) WHERE external_identifier IS NOT NULL;
CREATE TABLE ferro_auth_tokens (
    token_hash  text PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    kind        varchar(16) NOT NULL,
    expires     timestamptz NOT NULL,
    created     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ferro_auth_tokens_user_idx ON ferro_auth_tokens (user_id, kind);
"#,
), (
    4,
    r#"
CREATE TABLE ferro_item_drafts (
    collection    varchar(64) NOT NULL,
    item          varchar(255) NOT NULL,
    data          jsonb NOT NULL,
    publish_at    timestamptz,
    date_updated  timestamptz NOT NULL DEFAULT now(),
    user_updated  uuid REFERENCES ferro_users(id) ON DELETE SET NULL,
    PRIMARY KEY (collection, item)
);
CREATE INDEX ferro_item_drafts_publish_idx ON ferro_item_drafts (publish_at) WHERE publish_at IS NOT NULL;
CREATE TABLE ferro_item_versions (
    id              uuid PRIMARY KEY,
    collection      varchar(64) NOT NULL,
    item            varchar(255) NOT NULL,
    version_number  integer NOT NULL,
    data            jsonb NOT NULL,
    published_at    timestamptz NOT NULL DEFAULT now(),
    published_by    uuid REFERENCES ferro_users(id) ON DELETE SET NULL,
    UNIQUE (collection, item, version_number)
);
CREATE INDEX ferro_item_versions_item_idx ON ferro_item_versions (collection, item, version_number DESC);
"#,
), (
    5,
    r#"
CREATE TABLE ferro_comments (
    id            uuid PRIMARY KEY,
    collection    varchar(64) NOT NULL,
    item          varchar(255) NOT NULL,
    "user"        uuid REFERENCES ferro_users(id) ON DELETE SET NULL,
    parent        uuid REFERENCES ferro_comments(id) ON DELETE CASCADE,
    text          text NOT NULL,
    date_created  timestamptz NOT NULL DEFAULT now(),
    date_updated  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ferro_comments_item_idx ON ferro_comments (collection, item, date_created);
CREATE TABLE ferro_notifications (
    id            uuid PRIMARY KEY,
    recipient     uuid NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    sender        uuid REFERENCES ferro_users(id) ON DELETE SET NULL,
    kind          varchar(32) NOT NULL,
    subject       varchar(255) NOT NULL,
    message       text,
    collection    varchar(64),
    item          varchar(255),
    read_at       timestamptz,
    date_created  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ferro_notifications_recipient_idx ON ferro_notifications (recipient, date_created DESC);
CREATE INDEX ferro_notifications_unread_idx ON ferro_notifications (recipient) WHERE read_at IS NULL;
CREATE TABLE ferro_presets (
    id            uuid PRIMARY KEY,
    "user"        uuid NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    collection    varchar(64) NOT NULL,
    name          varchar(255) NOT NULL,
    layout        jsonb NOT NULL,
    shared        boolean NOT NULL DEFAULT false,
    date_created  timestamptz NOT NULL DEFAULT now(),
    date_updated  timestamptz NOT NULL DEFAULT now(),
    UNIQUE ("user", collection, name)
);
CREATE INDEX ferro_presets_collection_idx ON ferro_presets (collection, "user");
CREATE TABLE ferro_dashboards (
    "user"        uuid PRIMARY KEY REFERENCES ferro_users(id) ON DELETE CASCADE,
    layout        jsonb NOT NULL,
    date_updated  timestamptz NOT NULL DEFAULT now()
);
"#,
)];

/// Create / upgrade the system tables. Safe to run concurrently (advisory lock).
pub async fn migrate_system(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(7466524)").execute(&mut *tx).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ferro_system_migrations (version integer PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now())",
    )
    .execute(&mut *tx)
    .await?;
    let current: Option<i32> = sqlx::query_scalar("SELECT max(version) FROM ferro_system_migrations")
        .fetch_one(&mut *tx)
        .await?;
    let current = current.unwrap_or(0);
    for (version, sql) in SYSTEM_MIGRATIONS {
        if *version <= current {
            continue;
        }
        tracing::info!(version, "applying system migration");
        sqlx::raw_sql(sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO ferro_system_migrations (version) VALUES ($1)")
            .bind(*version)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferro_core::database::DatabaseKind;

    #[test]
    fn postgres_and_sqlite_migration_versions_match() {
        let pg: Vec<i32> = SYSTEM_MIGRATIONS.iter().map(|(v, _)| *v).collect();
        let sl: Vec<i32> = db_sqlite::MIGRATIONS.iter().map(|(v, _)| *v).collect();
        assert_eq!(pg, sl);
    }

    #[tokio::test]
    async fn migrate_sqlite_shared_memory_is_idempotent() {
        let url = "sqlite:file:ferro_migrate_test?mode=memory&cache=shared";
        let kind = migrate_url(url, None).await.unwrap();
        assert_eq!(kind, DatabaseKind::Sqlite);
        migrate_url(url, None).await.unwrap();
    }
}
