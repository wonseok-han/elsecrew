-- D0A contract fixture only. The sidecar does not execute this migration until D1.
-- Applying it after storage-v1 adds tables, indexes, and version metadata only.
-- It must not delete or alter the v1 events ledger or projection tables.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 2),
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    migration_state TEXT NOT NULL CHECK (migration_state = 'ready'),
    materialization_state TEXT NOT NULL CHECK (
        materialization_state IN ('pending', 'materializing', 'ready', 'failed')
    ),
    migrated_at TEXT NOT NULL
) STRICT;

CREATE TABLE ledger_baselines (
    baseline_id TEXT PRIMARY KEY NOT NULL,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    cutoff_aggregate_sequence INTEGER NOT NULL CHECK (cutoff_aggregate_sequence >= 1),
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version = 1),
    canonical_projection_json TEXT NOT NULL,
    projection_digest TEXT NOT NULL,
    created_at TEXT NOT NULL,
    verified_at TEXT NOT NULL,
    UNIQUE (aggregate_type, aggregate_id, cutoff_aggregate_sequence)
) STRICT;

CREATE TABLE operation_receipts (
    operation_id TEXT PRIMARY KEY NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_sequence INTEGER NOT NULL CHECK (aggregate_sequence >= 1),
    event_type TEXT NOT NULL,
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version = 1),
    canonical_payload_json TEXT NOT NULL,
    canonical_payload_digest_algorithm TEXT NOT NULL CHECK (
        canonical_payload_digest_algorithm = 'sha256'
    ),
    canonical_payload_digest_version INTEGER NOT NULL CHECK (
        canonical_payload_digest_version = 1
    ),
    canonical_payload_digest_format TEXT NOT NULL CHECK (
        canonical_payload_digest_format = 'lowercaseHex64'
    ),
    canonical_payload_digest TEXT NOT NULL CHECK (
        length(canonical_payload_digest) = 64
        AND canonical_payload_digest NOT GLOB '*[^0-9a-f]*'
    ),
    event_timestamp TEXT NOT NULL,
    materialized_at TEXT NOT NULL,
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence)
) STRICT;

CREATE TABLE chat_messages (
    message_id TEXT PRIMARY KEY NOT NULL,
    chat_id TEXT NOT NULL REFERENCES chats(chat_id),
    event_sequence INTEGER NOT NULL CHECK (event_sequence >= 1),
    canonical_message_json TEXT NOT NULL,
    message_digest TEXT NOT NULL,
    created_at TEXT NOT NULL,
    materialized_at TEXT NOT NULL,
    UNIQUE (chat_id, event_sequence)
) STRICT;

CREATE TABLE prepared_workspaces (
    preparation_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(project_id),
    worktree_id TEXT NOT NULL UNIQUE,
    workspace_identity TEXT NOT NULL UNIQUE,
    binding_state TEXT NOT NULL CHECK (binding_state IN ('unbound', 'bound', 'abandoned')),
    bound_chat_id TEXT,
    canonical_record_json TEXT NOT NULL,
    record_digest TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    materialized_at TEXT NOT NULL,
    CHECK (
        (binding_state = 'bound' AND bound_chat_id IS NOT NULL)
        OR (binding_state IN ('unbound', 'abandoned') AND bound_chat_id IS NULL)
    ),
    FOREIGN KEY (bound_chat_id, project_id) REFERENCES chats(chat_id, project_id)
) STRICT;

CREATE TABLE maintenance_jobs (
    job_id TEXT PRIMARY KEY NOT NULL,
    job_kind TEXT NOT NULL CHECK (job_kind = 'losslessCompaction'),
    state TEXT NOT NULL CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
    cutoff_json TEXT,
    started_at TEXT,
    completed_at TEXT,
    last_successful_at TEXT,
    failure_kind TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX chats_identity_project
ON chats(chat_id, project_id);

CREATE INDEX ledger_baselines_cutoff
ON ledger_baselines(aggregate_type, aggregate_id, cutoff_aggregate_sequence);

CREATE INDEX operation_receipts_aggregate_sequence
ON operation_receipts(aggregate_type, aggregate_id, aggregate_sequence);

CREATE INDEX chat_messages_chat_sequence
ON chat_messages(chat_id, event_sequence);

CREATE INDEX prepared_workspaces_project_state
ON prepared_workspaces(project_id, binding_state);

CREATE INDEX maintenance_jobs_status_created
ON maintenance_jobs(state, created_at);

INSERT INTO storage_schema_meta (
    singleton,
    storage_schema_version,
    protocol_version,
    migration_state,
    materialization_state,
    migrated_at
) VALUES (1, 2, 1, 'ready', 'pending', CURRENT_TIMESTAMP);

COMMIT;
