-- G5D contract fixture only. The sidecar must verify the exact storage-v2
-- schema with materialization_state pending or ready before executing this
-- transaction and treat an exact storage-v3
-- schema as an idempotent no-op before this SQL is reached.
-- This migration preserves every v1/v2 ledger, projection, materialized,
-- maintenance, index, and trigger object. Only storage_schema_meta is rebuilt
-- to tighten its version CHECK, and the dedicated singleton cursor table is new.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE storage_schema_meta RENAME TO storage_schema_meta_v2;

CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 3),
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    migration_state TEXT NOT NULL CHECK (migration_state = 'ready'),
    materialization_state TEXT NOT NULL CHECK (
        materialization_state IN ('pending', 'materializing', 'ready', 'failed')
    ),
    migrated_at TEXT NOT NULL
) STRICT;

INSERT INTO storage_schema_meta (
    singleton,
    storage_schema_version,
    protocol_version,
    migration_state,
    materialization_state,
    migrated_at
)
SELECT
    singleton,
    3,
    protocol_version,
    migration_state,
    materialization_state,
    migrated_at
FROM storage_schema_meta_v2;

DROP TABLE storage_schema_meta_v2;

CREATE TABLE lifetime_materialization_job (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    job_id TEXT NOT NULL UNIQUE CHECK (length(job_id) > 0),
    state TEXT NOT NULL CHECK (state IN ('pending', 'failed')),
    cutoff_event_rowid INTEGER NOT NULL CHECK (
        cutoff_event_rowid >= 0 AND cutoff_event_rowid <= 9007199254740991
    ),
    phase TEXT NOT NULL CHECK (
        phase IN ('receipts', 'messages', 'workspaces', 'baselines', 'verify')
    ),
    after_event_rowid INTEGER NOT NULL CHECK (
        after_event_rowid >= 0 AND after_event_rowid <= cutoff_event_rowid
    ),
    failure_kind TEXT,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at TEXT NOT NULL CHECK (length(updated_at) > 0),
    completed_at TEXT,
    CHECK (
        (state = 'pending' AND failure_kind IS NULL AND completed_at IS NULL)
        OR (
            state = 'failed'
            AND failure_kind IS NOT NULL
            AND length(failure_kind) BETWEEN 1 AND 64
            AND failure_kind GLOB '[A-Za-z]*'
            AND failure_kind NOT GLOB '*[^A-Za-z0-9._-]*'
            AND completed_at IS NOT NULL
            AND length(completed_at) > 0
        )
    )
) STRICT;

COMMIT;
