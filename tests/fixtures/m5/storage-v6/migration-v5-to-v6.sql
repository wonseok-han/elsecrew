-- G5D contract fixture only. The consumer must prove an exact storage-v5
-- schema and an admissible meta/job pair before executing this transaction.
-- This migration creates only the empty source index schema. Historical event
-- rows are indexed later by the bounded indexSource phase.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

ALTER TABLE storage_schema_meta RENAME TO storage_schema_meta_v5;
ALTER TABLE lifetime_materialization_job RENAME TO lifetime_materialization_job_v5;

CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 6),
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
SELECT
    singleton,
    6,
    protocol_version,
    migration_state,
    CASE materialization_state WHEN 'ready' THEN 'pending' ELSE materialization_state END,
    migrated_at
FROM storage_schema_meta_v5;

CREATE TABLE event_source_index (
    event_rowid INTEGER PRIMARY KEY CHECK (
        event_rowid >= 1 AND event_rowid <= 9007199254740991
    ),
    event_id TEXT NOT NULL UNIQUE CHECK (length(event_id) > 0),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) > 0),
    aggregate_type TEXT NOT NULL CHECK (length(aggregate_type) > 0),
    aggregate_id TEXT NOT NULL CHECK (length(aggregate_id) > 0),
    aggregate_sequence INTEGER NOT NULL CHECK (
        aggregate_sequence >= 1 AND aggregate_sequence <= 9007199254740991
    ),
    introduced_message_id TEXT CHECK (
        introduced_message_id IS NULL OR length(introduced_message_id) > 0
    ),
    chat_durable_sequence INTEGER CHECK (
        chat_durable_sequence IS NULL OR (
            chat_durable_sequence >= 0 AND chat_durable_sequence <= 9007199254740991
        )
    ),
    chat_snapshot_message_id TEXT CHECK (
        chat_snapshot_message_id IS NULL OR length(chat_snapshot_message_id) > 0
    ),
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence),
    FOREIGN KEY (event_id) REFERENCES events(event_id) ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (operation_id) REFERENCES events(operation_id) ON UPDATE RESTRICT ON DELETE RESTRICT,
    FOREIGN KEY (aggregate_type, aggregate_id, aggregate_sequence)
        REFERENCES events(aggregate_type, aggregate_id, aggregate_sequence)
        ON UPDATE RESTRICT ON DELETE RESTRICT
) STRICT;

CREATE INDEX event_source_index_aggregate_latest
ON event_source_index (aggregate_type, aggregate_id, event_rowid DESC);

CREATE UNIQUE INDEX event_source_index_message_unique
ON event_source_index (introduced_message_id)
WHERE introduced_message_id IS NOT NULL;

CREATE TRIGGER event_source_index_requires_exact_event
BEFORE INSERT ON event_source_index
WHEN NOT EXISTS (
    SELECT 1
    FROM events AS source
    WHERE source.rowid = NEW.event_rowid
      AND source.event_id = NEW.event_id
      AND source.operation_id = NEW.operation_id
      AND source.aggregate_type = NEW.aggregate_type
      AND source.aggregate_id = NEW.aggregate_id
      AND source.aggregate_sequence = NEW.aggregate_sequence
      AND (
          (
              source.aggregate_type <> 'chat'
              AND NEW.introduced_message_id IS NULL
              AND NEW.chat_durable_sequence IS NULL
              AND NEW.chat_snapshot_message_id IS NULL
          )
          OR (
              source.aggregate_type = 'chat'
              AND json_type(source.canonical_json_payload, '$.durable_event_sequence') = 'integer'
              AND NEW.chat_durable_sequence = json_extract(
                  source.canonical_json_payload, '$.durable_event_sequence'
              )
              AND NEW.chat_durable_sequence >= 0
              AND NEW.chat_durable_sequence <= 9007199254740991
              AND (
                  (
                      json_type(source.canonical_json_payload, '$.last_message') = 'null'
                      AND NEW.chat_snapshot_message_id IS NULL
                  )
                  OR (
                      json_type(source.canonical_json_payload, '$.last_message') = 'object'
                      AND (
                          SELECT COUNT(*)
                          FROM json_each(source.canonical_json_payload, '$.last_message')
                      ) = 11
                      AND json_type(source.canonical_json_payload, '$.last_message.schemaVersion') = 'integer'
                      AND json_extract(source.canonical_json_payload, '$.last_message.schemaVersion') = 1
                      AND json_type(
                          source.canonical_json_payload, '$.last_message.messageId'
                      ) = 'text'
                      AND length(json_extract(
                          source.canonical_json_payload, '$.last_message.messageId'
                      )) > 0
                      AND NEW.chat_snapshot_message_id = json_extract(
                          source.canonical_json_payload, '$.last_message.messageId'
                      )
                      AND json_type(source.canonical_json_payload, '$.last_message.chatId') = 'text'
                      AND length(json_extract(source.canonical_json_payload, '$.last_message.chatId')) > 0
                      AND json_extract(
                          source.canonical_json_payload, '$.last_message.chatId'
                      ) = NEW.aggregate_id
                      AND json_type(source.canonical_json_payload, '$.last_message.taskId') = 'text'
                      AND length(json_extract(source.canonical_json_payload, '$.last_message.taskId')) > 0
                      AND json_type(source.canonical_json_payload, '$.last_message.worktreeId') = 'text'
                      AND length(json_extract(source.canonical_json_payload, '$.last_message.worktreeId')) > 0
                      AND json_type(source.canonical_json_payload, '$.last_message.eventSequence') = 'integer'
                      AND json_extract(source.canonical_json_payload, '$.last_message.eventSequence') >= 1
                      AND json_extract(source.canonical_json_payload, '$.last_message.eventSequence') <= 9007199254740991
                      AND json_extract(
                          source.canonical_json_payload, '$.last_message.eventSequence'
                      ) = NEW.chat_durable_sequence
                      AND json_extract(source.canonical_json_payload, '$.last_message.kind')
                          IN ('user', 'role', 'system', 'status')
                      AND json_type(source.canonical_json_payload, '$.last_message.text') = 'text'
                      AND length(CAST(json_extract(
                          source.canonical_json_payload, '$.last_message.text'
                      ) AS BLOB)) <= 65536
                      AND json_type(source.canonical_json_payload, '$.last_message.createdAt') = 'text'
                      AND length(json_extract(source.canonical_json_payload, '$.last_message.createdAt')) = 20
                      AND substr(json_extract(source.canonical_json_payload, '$.last_message.createdAt'), 5, 1) = '-'
                      AND substr(json_extract(source.canonical_json_payload, '$.last_message.createdAt'), 8, 1) = '-'
                      AND substr(json_extract(source.canonical_json_payload, '$.last_message.createdAt'), 11, 1) = 'T'
                      AND substr(json_extract(source.canonical_json_payload, '$.last_message.createdAt'), 14, 1) = ':'
                      AND substr(json_extract(source.canonical_json_payload, '$.last_message.createdAt'), 17, 1) = ':'
                      AND substr(json_extract(source.canonical_json_payload, '$.last_message.createdAt'), 20, 1) = 'Z'
                      AND (
                          (
                              json_extract(source.canonical_json_payload, '$.last_message.kind') = 'role'
                              AND json_extract(source.canonical_json_payload, '$.last_message.roleId')
                                  IN ('planning', 'coding', 'review')
                              AND json_type(source.canonical_json_payload, '$.last_message.runId') = 'text'
                              AND length(json_extract(source.canonical_json_payload, '$.last_message.runId')) > 0
                          )
                          OR (
                              json_extract(source.canonical_json_payload, '$.last_message.kind') = 'user'
                              AND json_type(source.canonical_json_payload, '$.last_message.roleId') = 'null'
                              AND json_type(source.canonical_json_payload, '$.last_message.runId') = 'null'
                          )
                          OR (
                              json_extract(source.canonical_json_payload, '$.last_message.kind')
                                  IN ('system', 'status')
                              AND json_type(source.canonical_json_payload, '$.last_message.roleId') = 'null'
                              AND (
                                  json_type(source.canonical_json_payload, '$.last_message.runId') = 'null'
                                  OR (
                                      json_type(source.canonical_json_payload, '$.last_message.runId') = 'text'
                                      AND length(json_extract(
                                          source.canonical_json_payload, '$.last_message.runId'
                                      )) > 0
                                  )
                              )
                          )
                      )
                  )
              )
              AND (
                  (
                      NEW.aggregate_sequence = 1
                      OR EXISTS (
                          SELECT 1
                          FROM event_source_index AS predecessor
                          JOIN events AS predecessor_source
                            ON predecessor_source.rowid = predecessor.event_rowid
                          WHERE predecessor.aggregate_type = NEW.aggregate_type
                            AND predecessor.aggregate_id = NEW.aggregate_id
                            AND predecessor.aggregate_sequence = NEW.aggregate_sequence - 1
                            AND (
                                predecessor.chat_snapshot_message_id IS NULL
                                OR (
                                    json_type(
                                        predecessor_source.canonical_json_payload,
                                        '$.last_message'
                                    ) = 'object'
                                    AND json_extract(
                                        predecessor_source.canonical_json_payload,
                                        '$.last_message.messageId'
                                    ) = predecessor.chat_snapshot_message_id
                                    AND json_extract(
                                        predecessor_source.canonical_json_payload,
                                        '$.last_message.chatId'
                                    ) = predecessor.aggregate_id
                                    AND json_extract(
                                        predecessor_source.canonical_json_payload,
                                        '$.last_message.eventSequence'
                                    ) = predecessor.chat_durable_sequence
                                )
                            )
                      )
                  )
                  AND (
                    (
                      NEW.chat_durable_sequence = CASE
                          WHEN NEW.aggregate_sequence = 1 THEN 1
                          ELSE (
                              SELECT predecessor.chat_durable_sequence + 1
                              FROM event_source_index AS predecessor
                              WHERE predecessor.aggregate_type = NEW.aggregate_type
                                AND predecessor.aggregate_id = NEW.aggregate_id
                                AND predecessor.aggregate_sequence = NEW.aggregate_sequence - 1
                          )
                      END
                      AND json_type(source.canonical_json_payload, '$.last_message') = 'object'
                      AND json_type(
                          source.canonical_json_payload, '$.last_message.eventSequence'
                      ) = 'integer'
                      AND json_extract(
                          source.canonical_json_payload, '$.last_message.eventSequence'
                      ) = NEW.chat_durable_sequence
                      AND json_type(
                          source.canonical_json_payload, '$.last_message.chatId'
                      ) = 'text'
                      AND json_extract(
                          source.canonical_json_payload, '$.last_message.chatId'
                      ) = NEW.aggregate_id
                      AND NEW.introduced_message_id = NEW.chat_snapshot_message_id
                    )
                    OR (
                      NEW.chat_durable_sequence = CASE
                          WHEN NEW.aggregate_sequence = 1 THEN 0
                          ELSE (
                              SELECT predecessor.chat_durable_sequence
                              FROM event_source_index AS predecessor
                              WHERE predecessor.aggregate_type = NEW.aggregate_type
                                AND predecessor.aggregate_id = NEW.aggregate_id
                                AND predecessor.aggregate_sequence = NEW.aggregate_sequence - 1
                          )
                      END
                      AND NEW.introduced_message_id IS NULL
                      AND (
                          NEW.chat_snapshot_message_id IS NULL
                          OR NEW.chat_snapshot_message_id = CASE
                              WHEN NEW.aggregate_sequence = 1 THEN NULL
                              ELSE (
                                  SELECT predecessor.chat_snapshot_message_id
                                  FROM event_source_index AS predecessor
                                  WHERE predecessor.aggregate_type = NEW.aggregate_type
                                    AND predecessor.aggregate_id = NEW.aggregate_id
                                    AND predecessor.aggregate_sequence = NEW.aggregate_sequence - 1
                              )
                          END
                      )
                    )
                  )
              )
          )
      )
)
BEGIN
    SELECT RAISE(ABORT, 'event source index does not match one exact source event');
END;

CREATE TRIGGER event_source_index_append_only_update
BEFORE UPDATE ON event_source_index
BEGIN
    SELECT RAISE(ABORT, 'event source index is append-only');
END;

CREATE TRIGGER event_source_index_append_only_delete
BEFORE DELETE ON event_source_index
BEGIN
    SELECT RAISE(ABORT, 'event source index is append-only');
END;

CREATE TABLE lifetime_materialization_job (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    job_id TEXT NOT NULL UNIQUE CHECK (length(job_id) > 0),
    state TEXT NOT NULL CHECK (state IN ('pending', 'attempting', 'failed')),
    cutoff_event_rowid INTEGER NOT NULL CHECK (
        cutoff_event_rowid >= 0 AND cutoff_event_rowid <= 9007199254740991
    ),
    phase TEXT NOT NULL CHECK (
        phase IN (
            'indexSource', 'verifySourceIndex',
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
        phase NOT IN (
            'indexSource', 'verifySourceIndex',
            'receipts', 'messages', 'workspaces', 'baselines', 'verifySource'
        )
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
    'indexSource',
    0,
    0,
    failure_kind,
    created_at,
    updated_at,
    completed_at
FROM lifetime_materialization_job_v5;

DROP TABLE lifetime_materialization_job_v5;
DROP TABLE storage_schema_meta_v5;

COMMIT;
