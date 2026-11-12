-- G5D Storage V8 contract fixture only. Before executing this transaction the
-- consumer must prove exact storage-v7, an admissible lifetime pair, no
-- pending/running compaction, and empty prefix/message/authority tables.
-- The migration does not backfill history or activate product deletion.
-- SQLite binds the immutable source fields, receipt digest, canonical envelope
-- text, and digest shape. The consumer must independently recompute both
-- SHA-256 digests before INSERT and during bounded verifyOperations reopen;
-- caller-provided digest equality alone is never sufficient authority.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE storage_schema_meta RENAME TO storage_schema_meta_v7;

CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 8),
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
SELECT singleton, 8, protocol_version, migration_state, materialization_state, migrated_at
FROM storage_schema_meta_v7;

CREATE TABLE frozen_operation_sources (
    operation_id TEXT PRIMARY KEY NOT NULL REFERENCES operation_receipts(operation_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    source_event_rowid INTEGER NOT NULL CHECK (
        source_event_rowid >= 1 AND source_event_rowid <= 9007199254740991
    ),
    source_event_id TEXT NOT NULL CHECK (length(source_event_id) > 0),
    request_id TEXT,
    aggregate_type TEXT NOT NULL CHECK (
        aggregate_type IN ('project', 'chat', 'run', 'lock')
    ),
    aggregate_id TEXT NOT NULL CHECK (length(aggregate_id) > 0),
    aggregate_sequence INTEGER NOT NULL CHECK (
        aggregate_sequence >= 1 AND aggregate_sequence <= 9007199254740991
    ),
    event_type TEXT NOT NULL CHECK (length(event_type) > 0),
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version = 1),
    canonical_payload_json TEXT NOT NULL,
    canonical_payload_digest TEXT NOT NULL CHECK (
        length(canonical_payload_digest) = 64
        AND canonical_payload_digest NOT GLOB '*[^0-9a-f]*'
    ),
    event_timestamp TEXT NOT NULL CHECK (length(event_timestamp) > 0),
    canonical_record_json TEXT NOT NULL,
    record_digest TEXT NOT NULL CHECK (
        length(record_digest) = 64 AND record_digest NOT GLOB '*[^0-9a-f]*'
    ),
    frozen_at TEXT NOT NULL CHECK (length(frozen_at) > 0)
) STRICT, WITHOUT ROWID;

CREATE UNIQUE INDEX frozen_operation_sources_source_rowid
ON frozen_operation_sources (source_event_rowid);

CREATE UNIQUE INDEX frozen_operation_sources_source_event
ON frozen_operation_sources (source_event_id);

CREATE UNIQUE INDEX frozen_operation_sources_aggregate
ON frozen_operation_sources (aggregate_type, aggregate_id, aggregate_sequence);

CREATE TRIGGER frozen_operation_sources_require_exact_live_source
BEFORE INSERT ON frozen_operation_sources
WHEN NOT EXISTS (
    SELECT 1
    FROM events AS source
    JOIN event_source_index AS source_index
      ON source_index.event_rowid = source.rowid
    JOIN operation_receipts AS receipt
      ON receipt.operation_id = NEW.operation_id
    WHERE source.rowid = NEW.source_event_rowid
      AND source.event_id = NEW.source_event_id
      AND source.operation_id = NEW.operation_id
      AND source.request_id IS NEW.request_id
      AND source.aggregate_type = NEW.aggregate_type
      AND source.aggregate_id = NEW.aggregate_id
      AND source.aggregate_sequence = NEW.aggregate_sequence
      AND source.event_type = NEW.event_type
      AND source.payload_schema_version = NEW.payload_schema_version
      AND source.canonical_json_payload = NEW.canonical_payload_json
      AND source.timestamp = NEW.event_timestamp
      AND source_index.event_id = source.event_id
      AND source_index.operation_id = source.operation_id
      AND source_index.aggregate_type = source.aggregate_type
      AND source_index.aggregate_id = source.aggregate_id
      AND source_index.aggregate_sequence = source.aggregate_sequence
      AND receipt.event_id = source.event_id
      AND receipt.aggregate_type = source.aggregate_type
      AND receipt.aggregate_id = source.aggregate_id
      AND receipt.aggregate_sequence = source.aggregate_sequence
      AND receipt.event_type = source.event_type
      AND receipt.payload_schema_version = source.payload_schema_version
      AND receipt.canonical_payload_json = source.canonical_json_payload
      AND receipt.canonical_payload_digest_algorithm = 'sha256'
      AND receipt.canonical_payload_digest_version = 1
      AND receipt.canonical_payload_digest_format = 'lowercaseHex64'
      AND receipt.canonical_payload_digest = NEW.canonical_payload_digest
      AND receipt.event_timestamp = source.timestamp
      AND NEW.canonical_record_json = json_object(
          'aggregateId', NEW.aggregate_id,
          'aggregateSequence', NEW.aggregate_sequence,
          'aggregateType', NEW.aggregate_type,
          'canonicalPayloadDigest', NEW.canonical_payload_digest,
          'canonicalPayloadJson', NEW.canonical_payload_json,
          'eventTimestamp', NEW.event_timestamp,
          'eventType', NEW.event_type,
          'frozenAt', NEW.frozen_at,
          'operationId', NEW.operation_id,
          'payloadSchemaVersion', NEW.payload_schema_version,
          'recordSchemaVersion', 1,
          'requestId', NEW.request_id,
          'sourceEventId', NEW.source_event_id,
          'sourceEventRowid', NEW.source_event_rowid
      )
)
BEGIN
    SELECT RAISE(ABORT, 'frozen operation requires exact live source and receipt cross-check');
END;

CREATE TRIGGER frozen_operation_sources_append_only_update
BEFORE UPDATE ON frozen_operation_sources
BEGIN
    SELECT RAISE(ABORT, 'frozen operation sources are append-only');
END;

CREATE TRIGGER frozen_operation_sources_append_only_delete
BEFORE DELETE ON frozen_operation_sources
BEGIN
    SELECT RAISE(ABORT, 'frozen operation sources are append-only');
END;

CREATE TRIGGER operation_receipts_compaction_immutable_update
BEFORE UPDATE ON operation_receipts
BEGIN
    SELECT RAISE(ABORT, 'operation receipts are immutable in storage v8');
END;

CREATE TRIGGER operation_receipts_compaction_immutable_delete
BEFORE DELETE ON operation_receipts
BEGIN
    SELECT RAISE(ABORT, 'operation receipts are immutable in storage v8');
END;

DROP TRIGGER ledger_compaction_prefixes_require_exact_anchor;
CREATE TRIGGER ledger_compaction_prefixes_require_exact_anchor
BEFORE INSERT ON ledger_compaction_prefixes
WHEN NOT EXISTS (
    SELECT 1
    FROM event_source_index AS anchor
    JOIN events AS source ON source.rowid = anchor.event_rowid
    JOIN frozen_operation_sources AS frozen
      ON frozen.operation_id = NEW.anchor_operation_id
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
      AND frozen.source_event_rowid = anchor.event_rowid
      AND frozen.source_event_id = anchor.event_id
      AND frozen.aggregate_type = anchor.aggregate_type
      AND frozen.aggregate_id = anchor.aggregate_id
      AND frozen.aggregate_sequence = anchor.aggregate_sequence
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
    SELECT RAISE(ABORT, 'compaction prefix requires exact frozen anchor and baseline');
END;

DROP TRIGGER frozen_message_sources_require_exact_source;
CREATE TRIGGER frozen_message_sources_require_exact_source
BEFORE INSERT ON frozen_message_sources
WHEN NOT EXISTS (
    SELECT 1
    FROM event_source_index AS source_index
    JOIN events AS source ON source.rowid = source_index.event_rowid
    JOIN frozen_operation_sources AS frozen
      ON frozen.operation_id = source_index.operation_id
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
      AND frozen.source_event_rowid = source_index.event_rowid
      AND frozen.source_event_id = source_index.event_id
      AND frozen.aggregate_type = source_index.aggregate_type
      AND frozen.aggregate_id = source_index.aggregate_id
      AND frozen.aggregate_sequence = source_index.aggregate_sequence
      AND message.chat_id = NEW.chat_id
      AND message.event_sequence = NEW.durable_sequence
      AND message.canonical_message_json = NEW.canonical_message_json
      AND message.message_digest = NEW.message_digest
)
BEGIN
    SELECT RAISE(ABORT, 'frozen message source requires exact frozen operation and message');
END;

DROP TRIGGER ledger_compaction_delete_authority_require_exact_tuple;
CREATE TRIGGER ledger_compaction_delete_authority_require_exact_tuple
BEFORE INSERT ON ledger_compaction_delete_authority
WHEN NOT EXISTS (
    SELECT 1
    FROM maintenance_jobs AS job
    JOIN ledger_compaction_prefixes AS prefix ON prefix.prefix_id = NEW.prefix_id
    JOIN event_source_index AS source_index ON source_index.event_rowid = NEW.event_rowid
    JOIN events AS source ON source.rowid = NEW.event_rowid
    JOIN frozen_operation_sources AS frozen
      ON frozen.operation_id = NEW.operation_id
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
      AND frozen.source_event_rowid = source_index.event_rowid
      AND frozen.source_event_id = source_index.event_id
      AND frozen.aggregate_type = source_index.aggregate_type
      AND frozen.aggregate_id = source_index.aggregate_id
      AND frozen.aggregate_sequence = source_index.aggregate_sequence
      AND NOT EXISTS (
          SELECT 1 FROM ledger_compaction_delete_authority AS existing
          WHERE existing.job_id <> NEW.job_id
      )
      AND (
          source_index.introduced_message_id IS NULL
          OR EXISTS (
              SELECT 1 FROM frozen_message_sources AS message_frozen
              WHERE message_frozen.message_id = source_index.introduced_message_id
                AND message_frozen.source_event_rowid = source_index.event_rowid
                AND message_frozen.source_event_id = source_index.event_id
                AND message_frozen.source_operation_id = source_index.operation_id
          )
      )
)
BEGIN
    SELECT RAISE(ABORT, 'delete authority requires exact frozen operation and message tuple');
END;

DROP TABLE storage_schema_meta_v7;
COMMIT;
