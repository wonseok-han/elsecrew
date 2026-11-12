-- G5D contract fixture only. The consumer must prove an exact storage-v4
-- schema and an admissible meta/job pair before executing this transaction.
-- Every object except storage_schema_meta and lifetime_materialization_job is
-- preserved. The predecessor migrations are immutable.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE storage_schema_meta RENAME TO storage_schema_meta_v4;
ALTER TABLE lifetime_materialization_job RENAME TO lifetime_materialization_job_v4;

CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 5),
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    migration_state TEXT NOT NULL CHECK (migration_state = 'ready'),
    materialization_state TEXT NOT NULL CHECK (
        materialization_state IN ('pending', 'materializing', 'ready', 'failed')
    ),
    migrated_at TEXT NOT NULL
) STRICT;

INSERT INTO storage_schema_meta (
    singleton, storage_schema_version, protocol_version, migration_state,
    materialization_state, migrated_at
)
SELECT singleton, 5, protocol_version, migration_state, materialization_state, migrated_at
FROM storage_schema_meta_v4;

CREATE TABLE lifetime_materialization_job (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    job_id TEXT NOT NULL UNIQUE CHECK (length(job_id) > 0),
    state TEXT NOT NULL CHECK (state IN ('pending', 'attempting', 'failed')),
    cutoff_event_rowid INTEGER NOT NULL CHECK (
        cutoff_event_rowid >= 0 AND cutoff_event_rowid <= 9007199254740991
    ),
    phase TEXT NOT NULL CHECK (
        phase IN (
            'receipts', 'messages', 'workspaces', 'baselines', 'verifySource',
            'verifyProjects', 'verifyChats', 'verifyRuns', 'verifyLocks',
            'verifyAppInstances', 'verifyReceipts', 'verifyMessages',
            'verifyWorkspaces', 'verifyBaselines'
        )
    ),
    after_scan_rowid INTEGER NOT NULL CHECK (
        after_scan_rowid >= 0 AND after_scan_rowid <= 9007199254740991
    ),
    scan_exhausted INTEGER NOT NULL CHECK (scan_exhausted IN (0, 1)),
    failure_kind TEXT,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at TEXT NOT NULL CHECK (length(updated_at) > 0),
    completed_at TEXT,
    CHECK (
        (
            state IN ('pending', 'attempting')
            AND failure_kind IS NULL
            AND completed_at IS NULL
        )
        OR (
            state = 'failed'
            AND failure_kind IS NOT NULL
            AND length(failure_kind) BETWEEN 1 AND 64
            AND failure_kind GLOB '[A-Za-z]*'
            AND failure_kind NOT GLOB '*[^A-Za-z0-9._-]*'
            AND completed_at IS NOT NULL
            AND length(completed_at) > 0
        )
    ),
    CHECK (
        phase NOT IN ('receipts', 'messages', 'workspaces', 'baselines', 'verifySource')
        OR after_scan_rowid <= cutoff_event_rowid
    )
) STRICT;

INSERT INTO lifetime_materialization_job (
    singleton, job_id, state, cutoff_event_rowid, phase, after_scan_rowid,
    scan_exhausted, failure_kind, created_at, updated_at, completed_at
)
SELECT
    singleton,
    job_id,
    state,
    cutoff_event_rowid,
    CASE phase
        WHEN 'verifyReceipts' THEN 'verifySource'
        WHEN 'verifyMessages' THEN 'verifySource'
        WHEN 'verifyWorkspaces' THEN 'verifySource'
        WHEN 'verifyBaselines' THEN 'verifySource'
        ELSE phase
    END,
    CASE phase
        WHEN 'verifyReceipts' THEN 0
        WHEN 'verifyMessages' THEN 0
        WHEN 'verifyWorkspaces' THEN 0
        WHEN 'verifyBaselines' THEN 0
        ELSE after_scan_rowid
    END,
    CASE phase
        WHEN 'verifyReceipts' THEN 0
        WHEN 'verifyMessages' THEN 0
        WHEN 'verifyWorkspaces' THEN 0
        WHEN 'verifyBaselines' THEN 0
        ELSE scan_exhausted
    END,
    failure_kind,
    created_at,
    updated_at,
    completed_at
FROM lifetime_materialization_job_v4;

DROP TABLE lifetime_materialization_job_v4;
DROP TABLE storage_schema_meta_v4;

COMMIT;
