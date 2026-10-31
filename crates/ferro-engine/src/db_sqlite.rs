//! SQLite system migrations (same version numbers as PostgreSQL).

pub const MIGRATIONS: &[(i32, &str)] = &[
    (
        1,
        r#"
CREATE TABLE ferro_collections (
    name        TEXT PRIMARY KEY,
    definition  TEXT NOT NULL
);
CREATE TABLE ferro_roles (
    id          TEXT PRIMARY KEY,
    definition  TEXT NOT NULL
);
CREATE TABLE ferro_policies (
    id          TEXT PRIMARY KEY,
    definition  TEXT NOT NULL
);
CREATE TABLE ferro_schema_history (
    id          TEXT PRIMARY KEY,
    applied_at  TEXT NOT NULL DEFAULT (datetime('now')),
    applied_by  TEXT,
    from_hash   TEXT NOT NULL,
    to_hash     TEXT NOT NULL,
    plan        TEXT NOT NULL,
    snapshot    TEXT NOT NULL
);
CREATE TABLE ferro_users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT,
    first_name    TEXT,
    last_name     TEXT,
    role          TEXT,
    status        TEXT NOT NULL DEFAULT 'active',
    language      TEXT,
    token_hash    TEXT UNIQUE,
    last_access   TEXT,
    date_created  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE ferro_sessions (
    token_hash  TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    expires     TEXT NOT NULL,
    created     TEXT NOT NULL DEFAULT (datetime('now')),
    ip          TEXT,
    user_agent  TEXT
);
CREATE TABLE ferro_folders (
    id      TEXT PRIMARY KEY,
    name    TEXT NOT NULL,
    parent  TEXT REFERENCES ferro_folders(id) ON DELETE SET NULL
);
CREATE TABLE ferro_files (
    id                TEXT PRIMARY KEY,
    storage           TEXT NOT NULL,
    filename_disk     TEXT,
    filename_download TEXT NOT NULL,
    title             TEXT,
    description       TEXT,
    type              TEXT,
    folder            TEXT REFERENCES ferro_folders(id) ON DELETE SET NULL,
    filesize          INTEGER,
    checksum          TEXT,
    width             INTEGER,
    height            INTEGER,
    tags              TEXT,
    metadata          TEXT,
    uploaded_by       TEXT REFERENCES ferro_users(id) ON DELETE SET NULL,
    uploaded_on       TEXT NOT NULL DEFAULT (datetime('now')),
    modified_on       TEXT
);
CREATE TABLE ferro_activity (
    id          TEXT PRIMARY KEY,
    action      TEXT NOT NULL,
    "user"      TEXT REFERENCES ferro_users(id) ON DELETE SET NULL,
    timestamp   TEXT NOT NULL DEFAULT (datetime('now')),
    ip          TEXT,
    user_agent  TEXT,
    collection  TEXT NOT NULL,
    item        TEXT NOT NULL,
    origin      TEXT,
    comment     TEXT
);
CREATE INDEX ferro_activity_item_idx ON ferro_activity (collection, item);
CREATE TABLE ferro_revisions (
    id          TEXT PRIMARY KEY,
    activity    TEXT NOT NULL REFERENCES ferro_activity(id) ON DELETE CASCADE,
    collection  TEXT NOT NULL,
    item        TEXT NOT NULL,
    data        TEXT,
    delta       TEXT
);
CREATE INDEX ferro_revisions_item_idx ON ferro_revisions (collection, item);
CREATE TABLE ferro_sync_state (
    collection  TEXT NOT NULL,
    item        TEXT NOT NULL,
    source      TEXT,
    hash        TEXT NOT NULL,
    synced_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (collection, item)
);
CREATE TABLE ferro_settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
"#,
    ),
    (
        2,
        r#"
CREATE TABLE ferro_flows (
    id          TEXT PRIMARY KEY,
    definition  TEXT NOT NULL
);
CREATE TABLE ferro_flow_queue (
    id         TEXT PRIMARY KEY,
    flow_id    TEXT NOT NULL,
    trigger    TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'pending',
    attempts   INTEGER NOT NULL DEFAULT 0,
    created    TEXT NOT NULL DEFAULT (datetime('now')),
    started    TEXT,
    run_at     TEXT NOT NULL DEFAULT (datetime('now')),
    dedupe_key TEXT UNIQUE
);
CREATE INDEX ferro_flow_queue_pending_idx ON ferro_flow_queue (status, run_at);
CREATE TABLE ferro_flow_runs (
    id         TEXT PRIMARY KEY,
    flow_id    TEXT NOT NULL,
    flow_name  TEXT,
    source     TEXT NOT NULL,
    status     TEXT NOT NULL,
    started    TEXT NOT NULL,
    finished   TEXT NOT NULL,
    trigger    TEXT,
    log        TEXT,
    error      TEXT
);
CREATE INDEX ferro_flow_runs_flow_idx ON ferro_flow_runs (flow_id, started DESC);
"#,
    ),
    (
        3,
        r#"
ALTER TABLE ferro_users ADD COLUMN provider TEXT;
ALTER TABLE ferro_users ADD COLUMN external_identifier TEXT;
ALTER TABLE ferro_users ADD COLUMN tfa_secret TEXT;
ALTER TABLE ferro_users ADD COLUMN tfa_last_step INTEGER;
ALTER TABLE ferro_users ADD COLUMN tfa_recovery TEXT;
ALTER TABLE ferro_users ADD COLUMN tfa_enabled INTEGER GENERATED ALWAYS AS (tfa_secret IS NOT NULL) STORED;
CREATE UNIQUE INDEX ferro_users_external_idx ON ferro_users (provider, external_identifier) WHERE external_identifier IS NOT NULL;
CREATE TABLE ferro_auth_tokens (
    token_hash  TEXT PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    expires     TEXT NOT NULL,
    created     TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX ferro_auth_tokens_user_idx ON ferro_auth_tokens (user_id, kind);
"#,
    ),
    (
        4,
        r#"
CREATE TABLE ferro_item_drafts (
    collection    TEXT NOT NULL,
    item          TEXT NOT NULL,
    data          TEXT NOT NULL,
    publish_at    TEXT,
    date_updated  TEXT NOT NULL DEFAULT (datetime('now')),
    user_updated  TEXT REFERENCES ferro_users(id) ON DELETE SET NULL,
    PRIMARY KEY (collection, item)
);
CREATE INDEX ferro_item_drafts_publish_idx ON ferro_item_drafts (publish_at) WHERE publish_at IS NOT NULL;
CREATE TABLE ferro_item_versions (
    id              TEXT PRIMARY KEY,
    collection      TEXT NOT NULL,
    item            TEXT NOT NULL,
    version_number  INTEGER NOT NULL,
    data            TEXT NOT NULL,
    published_at    TEXT NOT NULL DEFAULT (datetime('now')),
    published_by    TEXT REFERENCES ferro_users(id) ON DELETE SET NULL,
    UNIQUE (collection, item, version_number)
);
CREATE INDEX ferro_item_versions_item_idx ON ferro_item_versions (collection, item, version_number DESC);
"#,
    ),
    (
        5,
        r#"
CREATE TABLE ferro_comments (
    id            TEXT PRIMARY KEY,
    collection    TEXT NOT NULL,
    item          TEXT NOT NULL,
    "user"        TEXT REFERENCES ferro_users(id) ON DELETE SET NULL,
    parent        TEXT REFERENCES ferro_comments(id) ON DELETE CASCADE,
    text          TEXT NOT NULL,
    date_created  TEXT NOT NULL DEFAULT (datetime('now')),
    date_updated  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX ferro_comments_item_idx ON ferro_comments (collection, item, date_created);
CREATE TABLE ferro_notifications (
    id            TEXT PRIMARY KEY,
    recipient     TEXT NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    sender        TEXT REFERENCES ferro_users(id) ON DELETE SET NULL,
    kind          TEXT NOT NULL,
    subject       TEXT NOT NULL,
    message       TEXT,
    collection    TEXT,
    item          TEXT,
    read_at       TEXT,
    date_created  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX ferro_notifications_recipient_idx ON ferro_notifications (recipient, date_created DESC);
CREATE INDEX ferro_notifications_unread_idx ON ferro_notifications (recipient) WHERE read_at IS NULL;
CREATE TABLE ferro_presets (
    id            TEXT PRIMARY KEY,
    "user"        TEXT NOT NULL REFERENCES ferro_users(id) ON DELETE CASCADE,
    collection    TEXT NOT NULL,
    name          TEXT NOT NULL,
    layout        TEXT NOT NULL,
    shared        INTEGER NOT NULL DEFAULT 0,
    date_created  TEXT NOT NULL DEFAULT (datetime('now')),
    date_updated  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE ("user", collection, name)
);
CREATE INDEX ferro_presets_collection_idx ON ferro_presets (collection, "user");
CREATE TABLE ferro_dashboards (
    "user"        TEXT PRIMARY KEY REFERENCES ferro_users(id) ON DELETE CASCADE,
    layout        TEXT NOT NULL,
    date_updated  TEXT NOT NULL DEFAULT (datetime('now'))
);
"#,
    ),
];
