-- G5D Storage V9 contract fixture only. Before this transaction the consumer
-- must prove exact storage-v8, no pending/running losslessCompaction job, and
-- an empty ledger_compaction_delete_authority table. The runtime must rehash
-- canonical payload/message bytes in Rust; SQL equality is only a cross-check.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE storage_schema_meta RENAME TO storage_schema_meta_v8;

CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 9),
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
SELECT singleton, 9, protocol_version, migration_state, materialization_state, migrated_at
FROM storage_schema_meta_v8;

DROP INDEX maintenance_jobs_single_running_lossless_compaction;
CREATE UNIQUE INDEX maintenance_jobs_single_active_lossless_compaction
ON maintenance_jobs (job_kind)
WHERE job_kind = 'losslessCompaction' AND state IN ('pending', 'running');

CREATE TABLE storage_compaction_plans (
    job_id TEXT PRIMARY KEY NOT NULL REFERENCES maintenance_jobs(job_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    plan_schema_version INTEGER NOT NULL CHECK (plan_schema_version = 1),
    cutoff_event_rowid INTEGER NOT NULL CHECK (
        cutoff_event_rowid >= 1 AND cutoff_event_rowid <= 9007199254740991
    ),
    aggregate_type TEXT NOT NULL CHECK (aggregate_type IN ('project', 'chat', 'run', 'lock')),
    aggregate_id TEXT NOT NULL CHECK (length(aggregate_id) > 0),
    retained_boundary_sequence INTEGER NOT NULL CHECK (
        retained_boundary_sequence >= 0 AND retained_boundary_sequence <= 9007199254739990
    ),
    delete_through_sequence INTEGER NOT NULL CHECK (
        delete_through_sequence = retained_boundary_sequence + 1000
    ),
    anchor_aggregate_sequence INTEGER NOT NULL CHECK (
        anchor_aggregate_sequence = delete_through_sequence + 1
    ),
    expected_rows INTEGER NOT NULL CHECK (expected_rows = 1001),
    baseline_id TEXT NOT NULL REFERENCES ledger_baselines(baseline_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    baseline_projection_digest TEXT NOT NULL CHECK (
        length(baseline_projection_digest) = 64
        AND baseline_projection_digest NOT GLOB '*[^0-9a-f]*'
    ),
    previous_successful_job_id TEXT REFERENCES maintenance_jobs(job_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    planned_at TEXT NOT NULL CHECK (length(planned_at) > 0),
    UNIQUE (aggregate_type, aggregate_id, delete_through_sequence)
) STRICT, WITHOUT ROWID;

CREATE INDEX storage_compaction_plans_previous_success
ON storage_compaction_plans (previous_successful_job_id);

CREATE TABLE storage_compaction_plan_rows (
    job_id TEXT NOT NULL REFERENCES storage_compaction_plans(job_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 1 AND 1001),
    event_rowid INTEGER NOT NULL CHECK (
        event_rowid >= 1 AND event_rowid <= 9007199254740991
    ),
    event_id TEXT NOT NULL CHECK (length(event_id) > 0),
    operation_id TEXT NOT NULL CHECK (length(operation_id) > 0),
    request_id TEXT,
    aggregate_type TEXT NOT NULL CHECK (aggregate_type IN ('project', 'chat', 'run', 'lock')),
    aggregate_id TEXT NOT NULL CHECK (length(aggregate_id) > 0),
    aggregate_sequence INTEGER NOT NULL CHECK (
        aggregate_sequence >= 1 AND aggregate_sequence <= 9007199254740991
    ),
    event_type TEXT NOT NULL CHECK (length(event_type) > 0),
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version = 1),
    event_timestamp TEXT NOT NULL CHECK (length(event_timestamp) > 0),
    canonical_payload_json TEXT NOT NULL,
    canonical_payload_digest TEXT NOT NULL CHECK (
        length(canonical_payload_digest) = 64
        AND canonical_payload_digest NOT GLOB '*[^0-9a-f]*'
    ),
    introduced_message_id TEXT,
    introduced_message_digest TEXT CHECK (
        introduced_message_digest IS NULL OR (
            length(introduced_message_digest) = 64
            AND introduced_message_digest NOT GLOB '*[^0-9a-f]*'
        )
    ),
    decision TEXT NOT NULL CHECK (decision IN ('delete', 'retainAnchor')),
    baseline_id TEXT,
    baseline_projection_digest TEXT,
    operation_frozen_before_plan INTEGER NOT NULL CHECK (operation_frozen_before_plan IN (0, 1)),
    message_frozen_before_plan INTEGER NOT NULL CHECK (message_frozen_before_plan IN (0, 1)),
    PRIMARY KEY (job_id, ordinal),
    UNIQUE (job_id, event_rowid),
    UNIQUE (job_id, operation_id),
    UNIQUE (job_id, aggregate_type, aggregate_id, aggregate_sequence),
    CHECK ((introduced_message_id IS NULL) = (introduced_message_digest IS NULL)),
    CHECK (introduced_message_id IS NOT NULL OR message_frozen_before_plan = 0)
) STRICT, WITHOUT ROWID;

CREATE INDEX storage_compaction_plan_rows_source_rowid
ON storage_compaction_plan_rows (job_id, event_rowid);

CREATE INDEX storage_compaction_plan_rows_operation
ON storage_compaction_plan_rows (job_id, operation_id);

CREATE INDEX storage_compaction_plan_rows_aggregate
ON storage_compaction_plan_rows (job_id, aggregate_type, aggregate_id, aggregate_sequence);

CREATE TRIGGER storage_compaction_plans_exact_insert
BEFORE INSERT ON storage_compaction_plans
WHEN NOT EXISTS (
    SELECT 1
    FROM maintenance_jobs AS job
    JOIN ledger_baselines AS baseline ON baseline.baseline_id = NEW.baseline_id
    WHERE job.job_id = NEW.job_id
      AND job.job_kind = 'losslessCompaction'
      AND job.state = 'pending'
      AND baseline.aggregate_type = NEW.aggregate_type
      AND baseline.aggregate_id = NEW.aggregate_id
      AND baseline.cutoff_aggregate_sequence = NEW.delete_through_sequence
      AND baseline.projection_digest = NEW.baseline_projection_digest
      AND (
          (NEW.previous_successful_job_id IS NULL
           AND NEW.retained_boundary_sequence = COALESCE((
               SELECT MAX(prefix.deleted_through_sequence)
               FROM ledger_compaction_prefixes AS prefix
               WHERE prefix.aggregate_type = NEW.aggregate_type
                 AND prefix.aggregate_id = NEW.aggregate_id
           ), 0))
          OR EXISTS (
              SELECT 1
              FROM maintenance_jobs AS previous_job
              JOIN storage_compaction_plans AS previous_plan
                ON previous_plan.job_id = previous_job.job_id
              WHERE previous_job.job_id = NEW.previous_successful_job_id
                AND previous_job.state = 'succeeded'
                AND previous_plan.aggregate_type = NEW.aggregate_type
                AND previous_plan.aggregate_id = NEW.aggregate_id
                AND previous_plan.delete_through_sequence = NEW.retained_boundary_sequence
          )
      )
)
BEGIN
    SELECT RAISE(ABORT, 'compaction plan requires exact current job baseline and predecessor');
END;

CREATE TRIGGER storage_compaction_plans_immutable_update
BEFORE UPDATE ON storage_compaction_plans
BEGIN
    SELECT RAISE(ABORT, 'compaction plan headers are immutable');
END;

CREATE TRIGGER storage_compaction_plans_immutable_delete
BEFORE DELETE ON storage_compaction_plans
BEGIN
    SELECT RAISE(ABORT, 'compaction plan headers are retained');
END;

CREATE TRIGGER storage_compaction_plan_rows_exact_insert
BEFORE INSERT ON storage_compaction_plan_rows
WHEN NOT EXISTS (
    SELECT 1
    FROM storage_compaction_plans AS plan
    JOIN maintenance_jobs AS job ON job.job_id = plan.job_id
    JOIN events AS source ON source.rowid = NEW.event_rowid
    JOIN event_source_index AS source_index ON source_index.event_rowid = source.rowid
    JOIN operation_receipts AS receipt ON receipt.operation_id = source.operation_id
    WHERE plan.job_id = NEW.job_id
      AND job.state = 'pending'
      AND NEW.event_rowid <= plan.cutoff_event_rowid
      AND NEW.aggregate_type = plan.aggregate_type
      AND NEW.aggregate_id = plan.aggregate_id
      AND NEW.aggregate_sequence = plan.retained_boundary_sequence + NEW.ordinal
      AND source.event_id = NEW.event_id
      AND source.operation_id = NEW.operation_id
      AND source.request_id IS NEW.request_id
      AND source.aggregate_type = NEW.aggregate_type
      AND source.aggregate_id = NEW.aggregate_id
      AND source.aggregate_sequence = NEW.aggregate_sequence
      AND source.event_type = NEW.event_type
      AND source.payload_schema_version = NEW.payload_schema_version
      AND source.timestamp = NEW.event_timestamp
      AND source.canonical_json_payload = NEW.canonical_payload_json
      AND source_index.event_id = NEW.event_id
      AND source_index.operation_id = NEW.operation_id
      AND source_index.aggregate_type = NEW.aggregate_type
      AND source_index.aggregate_id = NEW.aggregate_id
      AND source_index.aggregate_sequence = NEW.aggregate_sequence
      AND receipt.canonical_payload_json = NEW.canonical_payload_json
      AND receipt.canonical_payload_digest = NEW.canonical_payload_digest
      AND receipt.canonical_payload_digest_algorithm = 'sha256'
      AND receipt.canonical_payload_digest_version = 1
      AND receipt.canonical_payload_digest_format = 'lowercaseHex64'
      AND NEW.operation_frozen_before_plan = EXISTS (
          SELECT 1 FROM frozen_operation_sources AS frozen
          WHERE frozen.operation_id = NEW.operation_id
            AND frozen.source_event_rowid = NEW.event_rowid
      )
      AND (
          (NEW.introduced_message_id IS NULL
           AND source_index.introduced_message_id IS NULL
           AND NEW.message_frozen_before_plan = 0)
          OR (source_index.introduced_message_id = NEW.introduced_message_id
              AND EXISTS (
                  SELECT 1 FROM chat_messages AS message
                  WHERE message.message_id = NEW.introduced_message_id
                    AND message.message_digest = NEW.introduced_message_digest
              )
              AND NEW.message_frozen_before_plan = EXISTS (
                  SELECT 1 FROM frozen_message_sources AS frozen_message
                  WHERE frozen_message.message_id = NEW.introduced_message_id
                    AND frozen_message.message_digest = NEW.introduced_message_digest
              ))
      )
      AND (
          (NEW.ordinal BETWEEN 1 AND 1000
           AND NEW.decision = 'delete'
           AND NEW.baseline_id = plan.baseline_id
           AND NEW.baseline_projection_digest = plan.baseline_projection_digest)
          OR (NEW.ordinal = 1001
              AND NEW.decision = 'retainAnchor'
              AND NEW.aggregate_sequence = plan.anchor_aggregate_sequence
              AND NEW.baseline_id IS NULL
              AND NEW.baseline_projection_digest IS NULL)
      )
)
BEGIN
    SELECT RAISE(ABORT, 'compaction plan row requires exact witnessed source and decision');
END;

CREATE TRIGGER storage_compaction_plan_rows_immutable_update
BEFORE UPDATE ON storage_compaction_plan_rows
BEGIN
    SELECT RAISE(ABORT, 'compaction plan rows are immutable');
END;

CREATE TRIGGER storage_compaction_plan_rows_release_only
BEFORE DELETE ON storage_compaction_plan_rows
WHEN NOT EXISTS (
    SELECT 1
    FROM maintenance_jobs AS job
    WHERE job.job_id = OLD.job_id
      AND job.state = 'pending'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'releasePlan'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified') = 1
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased') = 0
)
BEGIN
    SELECT RAISE(ABORT, 'compaction plan rows release requires terminal equivalence');
END;

DROP TRIGGER ledger_compaction_prefixes_require_exact_anchor;
CREATE TRIGGER ledger_compaction_prefixes_require_v9_plan
BEFORE INSERT ON ledger_compaction_prefixes
WHEN NOT EXISTS (
    SELECT 1
    FROM maintenance_jobs AS job
    JOIN storage_compaction_plans AS plan ON plan.job_id = job.job_id
    JOIN storage_compaction_plan_rows AS anchor
      ON anchor.job_id = plan.job_id AND anchor.ordinal = 1001
    WHERE job.job_kind = 'losslessCompaction'
      AND job.state = 'pending'
      AND plan.aggregate_type = NEW.aggregate_type
      AND plan.aggregate_id = NEW.aggregate_id
      AND plan.delete_through_sequence = NEW.deleted_through_sequence
      AND plan.anchor_aggregate_sequence = NEW.anchor_aggregate_sequence
      AND plan.baseline_id = NEW.baseline_id
      AND plan.baseline_projection_digest = NEW.baseline_projection_digest
      AND anchor.decision = 'retainAnchor'
      AND anchor.event_rowid = NEW.anchor_event_rowid
      AND anchor.event_id = NEW.anchor_event_id
      AND anchor.operation_id = NEW.anchor_operation_id
)
BEGIN
    SELECT RAISE(ABORT, 'compaction prefix requires exact v9 plan anchor');
END;

DROP TRIGGER ledger_compaction_delete_authority_require_exact_tuple;
CREATE TRIGGER ledger_compaction_delete_authority_require_v9_plan
BEFORE INSERT ON ledger_compaction_delete_authority
WHEN NOT EXISTS (
    SELECT 1
    FROM maintenance_jobs AS job
    JOIN storage_compaction_plans AS plan ON plan.job_id = job.job_id
    JOIN storage_compaction_plan_rows AS planned
      ON planned.job_id = plan.job_id AND planned.event_rowid = NEW.event_rowid
    JOIN ledger_compaction_prefixes AS prefix ON prefix.prefix_id = NEW.prefix_id
    WHERE job.job_id = NEW.job_id
      AND job.job_kind = 'losslessCompaction'
      AND job.state = 'running'
      AND planned.decision = 'delete'
      AND planned.event_id = NEW.event_id
      AND planned.operation_id = NEW.operation_id
      AND planned.aggregate_type = NEW.aggregate_type
      AND planned.aggregate_id = NEW.aggregate_id
      AND planned.aggregate_sequence = NEW.aggregate_sequence
      AND prefix.aggregate_type = plan.aggregate_type
      AND prefix.aggregate_id = plan.aggregate_id
      AND prefix.deleted_through_sequence = plan.delete_through_sequence
)
BEGIN
    SELECT RAISE(ABORT, 'delete authority requires exact v9 delete plan row');
END;

DROP TABLE storage_schema_meta_v8;
COMMIT;
