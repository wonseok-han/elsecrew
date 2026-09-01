-- G5D C0 contract fixture only. The consumer must prove exact storage-v6,
-- an admissible materialization pair, no active maintenance job, and no
-- compaction-authority schema before executing this transaction.
-- The trigger replacements below are a one-time storage-v7 schema change;
-- compaction batches must never drop or recreate guards at runtime.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE storage_schema_meta RENAME TO storage_schema_meta_v6;

CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 7),
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
SELECT singleton, 7, protocol_version, migration_state, materialization_state, migrated_at
FROM storage_schema_meta_v6;

CREATE TABLE ledger_compaction_prefixes (
    prefix_id TEXT PRIMARY KEY NOT NULL CHECK (length(prefix_id) > 0),
    aggregate_type TEXT NOT NULL CHECK (
        length(aggregate_type) > 0 AND aggregate_type <> 'app_instance'
    ),
    aggregate_id TEXT NOT NULL CHECK (length(aggregate_id) > 0),
    deleted_through_sequence INTEGER NOT NULL CHECK (
        deleted_through_sequence >= 1
        AND deleted_through_sequence < 9007199254740991
    ),
    anchor_event_rowid INTEGER NOT NULL CHECK (
        anchor_event_rowid >= 1 AND anchor_event_rowid <= 9007199254740991
    ),
    anchor_event_id TEXT NOT NULL CHECK (length(anchor_event_id) > 0),
    anchor_operation_id TEXT NOT NULL CHECK (length(anchor_operation_id) > 0),
    anchor_aggregate_sequence INTEGER NOT NULL CHECK (
        anchor_aggregate_sequence = deleted_through_sequence + 1
    ),
    baseline_id TEXT NOT NULL REFERENCES ledger_baselines(baseline_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    baseline_projection_digest TEXT NOT NULL CHECK (length(baseline_projection_digest) > 0),
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    verified_at TEXT NOT NULL CHECK (length(verified_at) > 0),
    UNIQUE (aggregate_type, aggregate_id, deleted_through_sequence),
    UNIQUE (anchor_event_rowid),
    UNIQUE (anchor_event_id),
    UNIQUE (anchor_operation_id)
) STRICT;

CREATE INDEX ledger_compaction_prefixes_latest
ON ledger_compaction_prefixes (
    aggregate_type, aggregate_id, deleted_through_sequence DESC
);

CREATE TABLE frozen_message_sources (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_messages(message_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    source_event_rowid INTEGER NOT NULL UNIQUE CHECK (
        source_event_rowid >= 1 AND source_event_rowid <= 9007199254740991
    ),
    source_event_id TEXT NOT NULL UNIQUE CHECK (length(source_event_id) > 0),
    source_operation_id TEXT NOT NULL UNIQUE CHECK (length(source_operation_id) > 0),
    chat_id TEXT NOT NULL CHECK (length(chat_id) > 0),
    source_aggregate_sequence INTEGER NOT NULL CHECK (
        source_aggregate_sequence >= 1
        AND source_aggregate_sequence <= 9007199254740991
    ),
    durable_sequence INTEGER NOT NULL CHECK (
        durable_sequence >= 1 AND durable_sequence <= 9007199254740991
    ),
    canonical_message_json TEXT NOT NULL,
    message_digest TEXT NOT NULL CHECK (length(message_digest) > 0),
    frozen_at TEXT NOT NULL CHECK (length(frozen_at) > 0)
) STRICT;

CREATE INDEX frozen_message_sources_chat_sequence
ON frozen_message_sources (chat_id, durable_sequence);

CREATE UNIQUE INDEX maintenance_jobs_single_running_lossless_compaction
ON maintenance_jobs (job_kind)
WHERE job_kind = 'losslessCompaction' AND state = 'running';

CREATE TABLE ledger_compaction_delete_authority (
    job_id TEXT NOT NULL REFERENCES maintenance_jobs(job_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 1 AND 100),
    prefix_id TEXT NOT NULL REFERENCES ledger_compaction_prefixes(prefix_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    event_rowid INTEGER NOT NULL UNIQUE CHECK (
        event_rowid >= 1 AND event_rowid <= 9007199254740991
    ),
    event_id TEXT NOT NULL UNIQUE CHECK (length(event_id) > 0),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) > 0),
    aggregate_type TEXT NOT NULL CHECK (
        length(aggregate_type) > 0 AND aggregate_type <> 'app_instance'
    ),
    aggregate_id TEXT NOT NULL CHECK (length(aggregate_id) > 0),
    aggregate_sequence INTEGER NOT NULL CHECK (
        aggregate_sequence >= 1 AND aggregate_sequence <= 9007199254740991
    ),
    authorized_at TEXT NOT NULL CHECK (length(authorized_at) > 0),
    PRIMARY KEY (job_id, ordinal),
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence)
) STRICT;

CREATE TRIGGER ledger_compaction_prefixes_require_exact_anchor
BEFORE INSERT ON ledger_compaction_prefixes
WHEN NOT EXISTS (
    SELECT 1
    FROM event_source_index AS anchor
    JOIN events AS source ON source.rowid = anchor.event_rowid
    JOIN ledger_baselines AS baseline ON baseline.baseline_id = NEW.baseline_id
    WHERE anchor.event_rowid = NEW.anchor_event_rowid
      AND anchor.event_id = NEW.anchor_event_id
      AND anchor.operation_id = NEW.anchor_operation_id
      AND anchor.aggregate_type = NEW.aggregate_type
      AND anchor.aggregate_id = NEW.aggregate_id
      AND anchor.aggregate_sequence = NEW.anchor_aggregate_sequence
      AND source.event_id = anchor.event_id
      AND source.operation_id = anchor.operation_id
      AND source.aggregate_type = anchor.aggregate_type
      AND source.aggregate_id = anchor.aggregate_id
      AND source.aggregate_sequence = anchor.aggregate_sequence
      AND baseline.aggregate_type = NEW.aggregate_type
      AND baseline.aggregate_id = NEW.aggregate_id
      AND baseline.cutoff_aggregate_sequence = NEW.deleted_through_sequence
      AND baseline.projection_digest = NEW.baseline_projection_digest
      AND NOT EXISTS (
          SELECT 1
          FROM ledger_compaction_prefixes AS previous
          WHERE previous.aggregate_type = NEW.aggregate_type
            AND previous.aggregate_id = NEW.aggregate_id
            AND previous.deleted_through_sequence >= NEW.deleted_through_sequence
      )
)
BEGIN
    SELECT RAISE(ABORT, 'compaction prefix requires exact immutable baseline and anchor');
END;

CREATE TRIGGER ledger_compaction_prefixes_append_only_update
BEFORE UPDATE ON ledger_compaction_prefixes
BEGIN
    SELECT RAISE(ABORT, 'compaction prefixes are append-only');
END;

CREATE TRIGGER ledger_compaction_prefixes_append_only_delete
BEFORE DELETE ON ledger_compaction_prefixes
BEGIN
    SELECT RAISE(ABORT, 'compaction prefixes are append-only');
END;

CREATE TRIGGER frozen_message_sources_require_exact_source
BEFORE INSERT ON frozen_message_sources
WHEN NOT EXISTS (
    SELECT 1
    FROM event_source_index AS source_index
    JOIN events AS source ON source.rowid = source_index.event_rowid
    JOIN chat_messages AS message ON message.message_id = NEW.message_id
    WHERE source_index.event_rowid = NEW.source_event_rowid
      AND source_index.event_id = NEW.source_event_id
      AND source_index.operation_id = NEW.source_operation_id
      AND source_index.aggregate_type = 'chat'
      AND source_index.aggregate_id = NEW.chat_id
      AND source_index.aggregate_sequence = NEW.source_aggregate_sequence
      AND source_index.introduced_message_id = NEW.message_id
      AND source_index.chat_durable_sequence = NEW.durable_sequence
      AND source.event_id = source_index.event_id
      AND source.operation_id = source_index.operation_id
      AND source.aggregate_type = source_index.aggregate_type
      AND source.aggregate_id = source_index.aggregate_id
      AND source.aggregate_sequence = source_index.aggregate_sequence
      AND message.chat_id = NEW.chat_id
      AND message.event_sequence = NEW.durable_sequence
      AND message.canonical_message_json = NEW.canonical_message_json
      AND message.message_digest = NEW.message_digest
)
BEGIN
    SELECT RAISE(ABORT, 'frozen message source requires exact message and introducing event');
END;

CREATE TRIGGER frozen_message_sources_append_only_update
BEFORE UPDATE ON frozen_message_sources
BEGIN
    SELECT RAISE(ABORT, 'frozen message sources are append-only');
END;

CREATE TRIGGER frozen_message_sources_append_only_delete
BEFORE DELETE ON frozen_message_sources
BEGIN
    SELECT RAISE(ABORT, 'frozen message sources are append-only');
END;

CREATE TRIGGER event_source_index_rejects_frozen_message_reuse
BEFORE INSERT ON event_source_index
WHEN NEW.introduced_message_id IS NOT NULL
 AND EXISTS (
    SELECT 1 FROM frozen_message_sources AS frozen
    WHERE frozen.message_id = NEW.introduced_message_id
)
BEGIN
    SELECT RAISE(ABORT, 'frozen message ids cannot be introduced again');
END;

CREATE TRIGGER ledger_compaction_delete_authority_require_exact_tuple
BEFORE INSERT ON ledger_compaction_delete_authority
WHEN NOT EXISTS (
    SELECT 1
    FROM maintenance_jobs AS job
    JOIN ledger_compaction_prefixes AS prefix ON prefix.prefix_id = NEW.prefix_id
    JOIN event_source_index AS source_index ON source_index.event_rowid = NEW.event_rowid
    JOIN events AS source ON source.rowid = NEW.event_rowid
    WHERE job.job_id = NEW.job_id
      AND job.job_kind = 'losslessCompaction'
      AND job.state = 'running'
      AND prefix.aggregate_type = NEW.aggregate_type
      AND prefix.aggregate_id = NEW.aggregate_id
      AND NEW.aggregate_sequence <= prefix.deleted_through_sequence
      AND source_index.event_id = NEW.event_id
      AND source_index.operation_id = NEW.operation_id
      AND source_index.aggregate_type = NEW.aggregate_type
      AND source_index.aggregate_id = NEW.aggregate_id
      AND source_index.aggregate_sequence = NEW.aggregate_sequence
      AND source.event_id = NEW.event_id
      AND source.operation_id = NEW.operation_id
      AND source.aggregate_type = NEW.aggregate_type
      AND source.aggregate_id = NEW.aggregate_id
      AND source.aggregate_sequence = NEW.aggregate_sequence
      AND NOT EXISTS (
          SELECT 1
          FROM ledger_compaction_delete_authority AS existing
          WHERE existing.job_id <> NEW.job_id
      )
      AND (
          source_index.introduced_message_id IS NULL
          OR EXISTS (
              SELECT 1 FROM frozen_message_sources AS frozen
              WHERE frozen.message_id = source_index.introduced_message_id
                AND frozen.source_event_rowid = source_index.event_rowid
                AND frozen.source_event_id = source_index.event_id
                AND frozen.source_operation_id = source_index.operation_id
          )
      )
)
BEGIN
    SELECT RAISE(ABORT, 'delete authority requires running job and exact frozen tuple');
END;

CREATE TRIGGER ledger_compaction_delete_authority_no_update
BEFORE UPDATE ON ledger_compaction_delete_authority
BEGIN
    SELECT RAISE(ABORT, 'delete authority cannot be updated');
END;

CREATE TRIGGER ledger_compaction_delete_authority_delete_after_source
BEFORE DELETE ON ledger_compaction_delete_authority
WHEN EXISTS (SELECT 1 FROM events WHERE rowid = OLD.event_rowid)
BEGIN
    SELECT RAISE(ABORT, 'live source retains delete authority until rollback or deletion');
END;

DROP TRIGGER events_are_append_only_delete;
CREATE TRIGGER events_are_append_only_delete
BEFORE DELETE ON events
WHEN NOT EXISTS (
    SELECT 1 FROM ledger_compaction_delete_authority AS authority
    WHERE authority.event_rowid = OLD.rowid
      AND authority.event_id = OLD.event_id
      AND authority.operation_id = OLD.operation_id
      AND authority.aggregate_type = OLD.aggregate_type
      AND authority.aggregate_id = OLD.aggregate_id
      AND authority.aggregate_sequence = OLD.aggregate_sequence
)
BEGIN
    SELECT RAISE(ABORT, 'events ledger deletion lacks exact v7 authority');
END;

DROP TRIGGER event_source_index_append_only_delete;
CREATE TRIGGER event_source_index_append_only_delete
BEFORE DELETE ON event_source_index
WHEN NOT EXISTS (
    SELECT 1 FROM ledger_compaction_delete_authority AS authority
    WHERE authority.event_rowid = OLD.event_rowid
      AND authority.event_id = OLD.event_id
      AND authority.operation_id = OLD.operation_id
      AND authority.aggregate_type = OLD.aggregate_type
      AND authority.aggregate_id = OLD.aggregate_id
      AND authority.aggregate_sequence = OLD.aggregate_sequence
)
BEGIN
    SELECT RAISE(ABORT, 'source index deletion lacks exact v7 authority');
END;

CREATE TRIGGER events_consume_exact_v7_delete_authority
AFTER DELETE ON events
BEGIN
    DELETE FROM ledger_compaction_delete_authority
    WHERE event_rowid = OLD.rowid
      AND event_id = OLD.event_id
      AND operation_id = OLD.operation_id
      AND aggregate_type = OLD.aggregate_type
      AND aggregate_id = OLD.aggregate_id
      AND aggregate_sequence = OLD.aggregate_sequence;
END;

CREATE TRIGGER ledger_baselines_compaction_immutable_update
BEFORE UPDATE ON ledger_baselines
BEGIN
    SELECT RAISE(ABORT, 'ledger baselines are immutable in storage v7');
END;

CREATE TRIGGER ledger_baselines_compaction_immutable_delete
BEFORE DELETE ON ledger_baselines
BEGIN
    SELECT RAISE(ABORT, 'ledger baselines are immutable in storage v7');
END;

DROP TABLE storage_schema_meta_v6;
COMMIT;
