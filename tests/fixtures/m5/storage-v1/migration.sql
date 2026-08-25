PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TABLE schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    migration_state TEXT NOT NULL CHECK (migration_state = 'ready'),
    migrated_at TEXT NOT NULL
) STRICT;

CREATE TABLE app_instances (
    sidecar_instance_id TEXT PRIMARY KEY NOT NULL,
    pid INTEGER NOT NULL CHECK (pid > 0),
    platform TEXT NOT NULL,
    started_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    clean_shutdown_at TEXT,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT;

CREATE TABLE events (
    event_id TEXT PRIMARY KEY NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    request_id TEXT,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_sequence INTEGER NOT NULL CHECK (aggregate_sequence >= 1),
    event_type TEXT NOT NULL,
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version = 1),
    canonical_json_payload TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence)
) STRICT;

CREATE TABLE projects (
    project_id TEXT PRIMARY KEY NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT;

CREATE TABLE chats (
    chat_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(project_id),
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT;

CREATE TABLE runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    chat_id TEXT REFERENCES chats(chat_id),
    state TEXT NOT NULL,
    worktree_path TEXT,
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT;

CREATE TABLE locks (
    lock_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT REFERENCES projects(project_id),
    writer_run_id TEXT REFERENCES runs(run_id),
    worktree_path TEXT NOT NULL,
    poisoned INTEGER NOT NULL CHECK (poisoned IN (0, 1)),
    recovery_required INTEGER NOT NULL CHECK (recovery_required IN (0, 1)),
    evidence_json TEXT NOT NULL,
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT;

CREATE TRIGGER events_are_append_only_update
BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, 'events ledger is append-only');
END;

CREATE TRIGGER events_are_append_only_delete
BEFORE DELETE ON events
BEGIN
    SELECT RAISE(ABORT, 'events ledger is append-only');
END;

INSERT INTO schema_meta (
    singleton,
    schema_version,
    protocol_version,
    migration_state,
    migrated_at
) VALUES (1, 1, 1, 'ready', CURRENT_TIMESTAMP);

COMMIT;
