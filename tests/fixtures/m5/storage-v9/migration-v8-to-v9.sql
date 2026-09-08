-- G5D Storage V9 contract fixture only. This transaction independently proves
-- exact storage-v8, no pending/running losslessCompaction job, and an empty
-- ledger_compaction_delete_authority table before persistent DDL. The runtime
-- must also preflight and rehash
-- canonical payload/message bytes in Rust; SQL equality is only a cross-check.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE storage_v9_migration_guard (admissible INTEGER NOT NULL) STRICT;
CREATE TEMP TRIGGER storage_v9_migration_guard_reject
BEFORE INSERT ON storage_v9_migration_guard
WHEN NEW.admissible <> 1
BEGIN
    SELECT RAISE(ROLLBACK, 'storage v9 migration requires exact idle v8');
END;
INSERT INTO storage_v9_migration_guard (admissible)
WITH expected(type, name, sql) AS (VALUES
('index', 'chat_messages_chat_sequence', 'CREATE INDEX chat_messages_chat_sequence
ON chat_messages(chat_id, event_sequence)'),
    ('index', 'chats_identity_project', 'CREATE UNIQUE INDEX chats_identity_project
ON chats(chat_id, project_id)'),
    ('index', 'event_source_index_aggregate_latest', 'CREATE INDEX event_source_index_aggregate_latest
ON event_source_index (aggregate_type, aggregate_id, event_rowid DESC)'),
    ('index', 'event_source_index_message_unique', 'CREATE UNIQUE INDEX event_source_index_message_unique
ON event_source_index (introduced_message_id)
WHERE introduced_message_id IS NOT NULL'),
    ('index', 'frozen_message_sources_chat_sequence', 'CREATE INDEX frozen_message_sources_chat_sequence
ON frozen_message_sources (chat_id, durable_sequence)'),
    ('index', 'frozen_operation_sources_aggregate', 'CREATE UNIQUE INDEX frozen_operation_sources_aggregate
ON frozen_operation_sources (aggregate_type, aggregate_id, aggregate_sequence)'),
    ('index', 'frozen_operation_sources_source_event', 'CREATE UNIQUE INDEX frozen_operation_sources_source_event
ON frozen_operation_sources (source_event_id)'),
    ('index', 'frozen_operation_sources_source_rowid', 'CREATE UNIQUE INDEX frozen_operation_sources_source_rowid
ON frozen_operation_sources (source_event_rowid)'),
    ('index', 'ledger_baselines_cutoff', 'CREATE INDEX ledger_baselines_cutoff
ON ledger_baselines(aggregate_type, aggregate_id, cutoff_aggregate_sequence)'),
    ('index', 'ledger_compaction_prefixes_latest', 'CREATE INDEX ledger_compaction_prefixes_latest
ON ledger_compaction_prefixes (
    aggregate_type, aggregate_id, deleted_through_sequence DESC
)'),
    ('index', 'maintenance_jobs_single_running_lossless_compaction', 'CREATE UNIQUE INDEX maintenance_jobs_single_running_lossless_compaction
ON maintenance_jobs (job_kind)
WHERE job_kind = ''losslessCompaction'' AND state = ''running'''),
    ('index', 'maintenance_jobs_status_created', 'CREATE INDEX maintenance_jobs_status_created
ON maintenance_jobs(state, created_at)'),
    ('index', 'operation_receipts_aggregate_sequence', 'CREATE INDEX operation_receipts_aggregate_sequence
ON operation_receipts(aggregate_type, aggregate_id, aggregate_sequence)'),
    ('index', 'prepared_workspaces_project_state', 'CREATE INDEX prepared_workspaces_project_state
ON prepared_workspaces(project_id, binding_state)'),
    ('table', 'app_instances', 'CREATE TABLE app_instances (
    sidecar_instance_id TEXT PRIMARY KEY NOT NULL,
    pid INTEGER NOT NULL CHECK (pid > 0),
    platform TEXT NOT NULL,
    started_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    clean_shutdown_at TEXT,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT'),
    ('table', 'chat_messages', 'CREATE TABLE chat_messages (
    message_id TEXT PRIMARY KEY NOT NULL,
    chat_id TEXT NOT NULL REFERENCES chats(chat_id),
    event_sequence INTEGER NOT NULL CHECK (event_sequence >= 1),
    canonical_message_json TEXT NOT NULL,
    message_digest TEXT NOT NULL,
    created_at TEXT NOT NULL,
    materialized_at TEXT NOT NULL,
    UNIQUE (chat_id, event_sequence)
) STRICT'),
    ('table', 'chats', 'CREATE TABLE chats (
    chat_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(project_id),
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT'),
    ('table', 'event_source_index', 'CREATE TABLE event_source_index (
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
) STRICT'),
    ('table', 'events', 'CREATE TABLE events (
    event_id TEXT PRIMARY KEY NOT NULL,
    operation_id TEXT NOT NULL UNIQUE,
    request_id TEXT,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_sequence INTEGER NOT NULL CHECK (aggregate_sequence >= 1),
    event_type TEXT NOT NULL,
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version = 1),
    canonical_json_payload TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence)
) STRICT'),
    ('table', 'frozen_message_sources', 'CREATE TABLE frozen_message_sources (
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
) STRICT'),
    ('table', 'frozen_operation_sources', 'CREATE TABLE frozen_operation_sources (
    operation_id TEXT PRIMARY KEY NOT NULL REFERENCES operation_receipts(operation_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    source_event_rowid INTEGER NOT NULL CHECK (
        source_event_rowid >= 1 AND source_event_rowid <= 9007199254740991
    ),
    source_event_id TEXT NOT NULL CHECK (length(source_event_id) > 0),
    request_id TEXT,
    aggregate_type TEXT NOT NULL CHECK (
        aggregate_type IN (''project'', ''chat'', ''run'', ''lock'')
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
        AND canonical_payload_digest NOT GLOB ''*[^0-9a-f]*''
    ),
    event_timestamp TEXT NOT NULL CHECK (length(event_timestamp) > 0),
    canonical_record_json TEXT NOT NULL,
    record_digest TEXT NOT NULL CHECK (
        length(record_digest) = 64 AND record_digest NOT GLOB ''*[^0-9a-f]*''
    ),
    frozen_at TEXT NOT NULL CHECK (length(frozen_at) > 0)
) STRICT, WITHOUT ROWID'),
    ('table', 'ledger_baselines', 'CREATE TABLE ledger_baselines (
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
) STRICT'),
    ('table', 'ledger_compaction_delete_authority', 'CREATE TABLE ledger_compaction_delete_authority (
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
        length(aggregate_type) > 0 AND aggregate_type <> ''app_instance''
    ),
    aggregate_id TEXT NOT NULL CHECK (length(aggregate_id) > 0),
    aggregate_sequence INTEGER NOT NULL CHECK (
        aggregate_sequence >= 1 AND aggregate_sequence <= 9007199254740991
    ),
    authorized_at TEXT NOT NULL CHECK (length(authorized_at) > 0),
    PRIMARY KEY (job_id, ordinal),
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence)
) STRICT'),
    ('table', 'ledger_compaction_prefixes', 'CREATE TABLE ledger_compaction_prefixes (
    prefix_id TEXT PRIMARY KEY NOT NULL CHECK (length(prefix_id) > 0),
    aggregate_type TEXT NOT NULL CHECK (
        length(aggregate_type) > 0 AND aggregate_type <> ''app_instance''
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
) STRICT'),
    ('table', 'lifetime_materialization_job', 'CREATE TABLE lifetime_materialization_job (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    job_id TEXT NOT NULL UNIQUE CHECK (length(job_id) > 0),
    state TEXT NOT NULL CHECK (state IN (''pending'', ''attempting'', ''failed'')),
    cutoff_event_rowid INTEGER NOT NULL CHECK (
        cutoff_event_rowid >= 0 AND cutoff_event_rowid <= 9007199254740991
    ),
    phase TEXT NOT NULL CHECK (
        phase IN (
            ''indexSource'', ''verifySourceIndex'',
            ''receipts'', ''messages'', ''workspaces'', ''baselines'', ''verifySource'',
            ''verifyProjects'', ''verifyChats'', ''verifyRuns'', ''verifyLocks'',
            ''verifyAppInstances'', ''verifyReceipts'', ''verifyMessages'',
            ''verifyWorkspaces'', ''verifyBaselines''
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
            state IN (''pending'', ''attempting'')
            AND failure_kind IS NULL
            AND completed_at IS NULL
        )
        OR (
            state = ''failed''
            AND failure_kind IS NOT NULL
            AND length(failure_kind) BETWEEN 1 AND 64
            AND failure_kind GLOB ''[A-Za-z]*''
            AND failure_kind NOT GLOB ''*[^A-Za-z0-9._-]*''
            AND completed_at IS NOT NULL
            AND length(completed_at) > 0
        )
    ),
    CHECK (
        phase NOT IN (
            ''indexSource'', ''verifySourceIndex'',
            ''receipts'', ''messages'', ''workspaces'', ''baselines'', ''verifySource''
        )
        OR after_scan_rowid <= cutoff_event_rowid
    )
) STRICT'),
    ('table', 'locks', 'CREATE TABLE locks (
    lock_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT REFERENCES projects(project_id),
    writer_run_id TEXT REFERENCES runs(run_id),
    worktree_path TEXT NOT NULL,
    poisoned INTEGER NOT NULL CHECK (poisoned IN (0, 1)),
    recovery_required INTEGER NOT NULL CHECK (recovery_required IN (0, 1)),
    evidence_json TEXT NOT NULL,
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT'),
    ('table', 'maintenance_jobs', 'CREATE TABLE maintenance_jobs (
    job_id TEXT PRIMARY KEY NOT NULL,
    job_kind TEXT NOT NULL CHECK (job_kind = ''losslessCompaction''),
    state TEXT NOT NULL CHECK (state IN (''pending'', ''running'', ''succeeded'', ''failed'')),
    cutoff_json TEXT,
    started_at TEXT,
    completed_at TEXT,
    last_successful_at TEXT,
    failure_kind TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT'),
    ('table', 'operation_receipts', 'CREATE TABLE operation_receipts (
    operation_id TEXT PRIMARY KEY NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_sequence INTEGER NOT NULL CHECK (aggregate_sequence >= 1),
    event_type TEXT NOT NULL,
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version = 1),
    canonical_payload_json TEXT NOT NULL,
    canonical_payload_digest_algorithm TEXT NOT NULL CHECK (
        canonical_payload_digest_algorithm = ''sha256''
    ),
    canonical_payload_digest_version INTEGER NOT NULL CHECK (
        canonical_payload_digest_version = 1
    ),
    canonical_payload_digest_format TEXT NOT NULL CHECK (
        canonical_payload_digest_format = ''lowercaseHex64''
    ),
    canonical_payload_digest TEXT NOT NULL CHECK (
        length(canonical_payload_digest) = 64
        AND canonical_payload_digest NOT GLOB ''*[^0-9a-f]*''
    ),
    event_timestamp TEXT NOT NULL,
    materialized_at TEXT NOT NULL,
    UNIQUE (aggregate_type, aggregate_id, aggregate_sequence)
) STRICT'),
('table', 'prepared_workspaces', 'CREATE TABLE prepared_workspaces (
    preparation_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(project_id),
    worktree_id TEXT NOT NULL UNIQUE,
    workspace_identity TEXT NOT NULL UNIQUE,
    binding_state TEXT NOT NULL CHECK (binding_state IN (''unbound'', ''bound'', ''abandoned'')),
    bound_chat_id TEXT,
    canonical_record_json TEXT NOT NULL,
    record_digest TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    materialized_at TEXT NOT NULL,
    CHECK (
        (binding_state = ''bound'' AND bound_chat_id IS NOT NULL)
        OR (binding_state IN (''unbound'', ''abandoned'') AND bound_chat_id IS NULL)
    ),
    FOREIGN KEY (bound_chat_id, project_id) REFERENCES chats(chat_id, project_id)
) STRICT'),
    ('table', 'projects', 'CREATE TABLE projects (
    project_id TEXT PRIMARY KEY NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT'),
    ('table', 'runs', 'CREATE TABLE runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    chat_id TEXT REFERENCES chats(chat_id),
    state TEXT NOT NULL,
    worktree_path TEXT,
    projection_json TEXT NOT NULL,
    last_event_sequence INTEGER NOT NULL CHECK (last_event_sequence >= 1)
) STRICT'),
    ('table', 'schema_meta', 'CREATE TABLE schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    migration_state TEXT NOT NULL CHECK (migration_state = ''ready''),
    migrated_at TEXT NOT NULL
) STRICT'),
    ('table', 'storage_schema_meta', 'CREATE TABLE storage_schema_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    storage_schema_version INTEGER NOT NULL CHECK (storage_schema_version = 8),
    protocol_version INTEGER NOT NULL CHECK (protocol_version = 1),
    migration_state TEXT NOT NULL CHECK (migration_state = ''ready''),
    materialization_state TEXT NOT NULL CHECK (
        materialization_state IN (''pending'', ''materializing'', ''ready'', ''failed'')
    ),
    migrated_at TEXT NOT NULL
) STRICT'),
    ('trigger', 'event_source_index_append_only_delete', 'CREATE TRIGGER event_source_index_append_only_delete
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
    SELECT RAISE(ABORT, ''source index deletion lacks exact v7 authority'');
END'),
    ('trigger', 'event_source_index_append_only_update', 'CREATE TRIGGER event_source_index_append_only_update
BEFORE UPDATE ON event_source_index
BEGIN
    SELECT RAISE(ABORT, ''event source index is append-only'');
END'),
    ('trigger', 'event_source_index_rejects_frozen_message_reuse', 'CREATE TRIGGER event_source_index_rejects_frozen_message_reuse
BEFORE INSERT ON event_source_index
WHEN NEW.introduced_message_id IS NOT NULL
 AND EXISTS (
    SELECT 1 FROM frozen_message_sources AS frozen
    WHERE frozen.message_id = NEW.introduced_message_id
)
BEGIN
    SELECT RAISE(ABORT, ''frozen message ids cannot be introduced again'');
END'),
    ('trigger', 'event_source_index_requires_exact_event', 'CREATE TRIGGER event_source_index_requires_exact_event
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
              source.aggregate_type <> ''chat''
              AND NEW.introduced_message_id IS NULL
              AND NEW.chat_durable_sequence IS NULL
              AND NEW.chat_snapshot_message_id IS NULL
          )
          OR (
              source.aggregate_type = ''chat''
              AND json_type(source.canonical_json_payload, ''$.durable_event_sequence'') = ''integer''
              AND NEW.chat_durable_sequence = json_extract(
                  source.canonical_json_payload, ''$.durable_event_sequence''
              )
              AND NEW.chat_durable_sequence >= 0
              AND NEW.chat_durable_sequence <= 9007199254740991
              AND (
                  (
                      json_type(source.canonical_json_payload, ''$.last_message'') = ''null''
                      AND NEW.chat_snapshot_message_id IS NULL
                  )
                  OR (
                      json_type(source.canonical_json_payload, ''$.last_message'') = ''object''
                      AND (
                          SELECT COUNT(*)
                          FROM json_each(source.canonical_json_payload, ''$.last_message'')
                      ) = 11
                      AND json_type(source.canonical_json_payload, ''$.last_message.schemaVersion'') = ''integer''
                      AND json_extract(source.canonical_json_payload, ''$.last_message.schemaVersion'') = 1
                      AND json_type(
                          source.canonical_json_payload, ''$.last_message.messageId''
                      ) = ''text''
                      AND length(json_extract(
                          source.canonical_json_payload, ''$.last_message.messageId''
                      )) > 0
                      AND NEW.chat_snapshot_message_id = json_extract(
                          source.canonical_json_payload, ''$.last_message.messageId''
                      )
                      AND json_type(source.canonical_json_payload, ''$.last_message.chatId'') = ''text''
                      AND length(json_extract(source.canonical_json_payload, ''$.last_message.chatId'')) > 0
                      AND json_extract(
                          source.canonical_json_payload, ''$.last_message.chatId''
                      ) = NEW.aggregate_id
                      AND json_type(source.canonical_json_payload, ''$.last_message.taskId'') = ''text''
                      AND length(json_extract(source.canonical_json_payload, ''$.last_message.taskId'')) > 0
                      AND json_type(source.canonical_json_payload, ''$.last_message.worktreeId'') = ''text''
                      AND length(json_extract(source.canonical_json_payload, ''$.last_message.worktreeId'')) > 0
                      AND json_type(source.canonical_json_payload, ''$.last_message.eventSequence'') = ''integer''
                      AND json_extract(source.canonical_json_payload, ''$.last_message.eventSequence'') >= 1
                      AND json_extract(source.canonical_json_payload, ''$.last_message.eventSequence'') <= 9007199254740991
                      AND json_extract(
                          source.canonical_json_payload, ''$.last_message.eventSequence''
                      ) = NEW.chat_durable_sequence
                      AND json_extract(source.canonical_json_payload, ''$.last_message.kind'')
                          IN (''user'', ''role'', ''system'', ''status'')
                      AND json_type(source.canonical_json_payload, ''$.last_message.text'') = ''text''
                      AND length(CAST(json_extract(
                          source.canonical_json_payload, ''$.last_message.text''
                      ) AS BLOB)) <= 65536
                      AND json_type(source.canonical_json_payload, ''$.last_message.createdAt'') = ''text''
                      AND length(json_extract(source.canonical_json_payload, ''$.last_message.createdAt'')) = 20
                      AND substr(json_extract(source.canonical_json_payload, ''$.last_message.createdAt''), 5, 1) = ''-''
                      AND substr(json_extract(source.canonical_json_payload, ''$.last_message.createdAt''), 8, 1) = ''-''
                      AND substr(json_extract(source.canonical_json_payload, ''$.last_message.createdAt''), 11, 1) = ''T''
                      AND substr(json_extract(source.canonical_json_payload, ''$.last_message.createdAt''), 14, 1) = '':''
                      AND substr(json_extract(source.canonical_json_payload, ''$.last_message.createdAt''), 17, 1) = '':''
                      AND substr(json_extract(source.canonical_json_payload, ''$.last_message.createdAt''), 20, 1) = ''Z''
                      AND (
                          (
                              json_extract(source.canonical_json_payload, ''$.last_message.kind'') = ''role''
                              AND json_extract(source.canonical_json_payload, ''$.last_message.roleId'')
                                  IN (''planning'', ''coding'', ''review'')
                              AND json_type(source.canonical_json_payload, ''$.last_message.runId'') = ''text''
                              AND length(json_extract(source.canonical_json_payload, ''$.last_message.runId'')) > 0
                          )
                          OR (
                              json_extract(source.canonical_json_payload, ''$.last_message.kind'') = ''user''
                              AND json_type(source.canonical_json_payload, ''$.last_message.roleId'') = ''null''
                              AND json_type(source.canonical_json_payload, ''$.last_message.runId'') = ''null''
                          )
                          OR (
                              json_extract(source.canonical_json_payload, ''$.last_message.kind'')
                                  IN (''system'', ''status'')
                              AND json_type(source.canonical_json_payload, ''$.last_message.roleId'') = ''null''
                              AND (
                                  json_type(source.canonical_json_payload, ''$.last_message.runId'') = ''null''
                                  OR (
                                      json_type(source.canonical_json_payload, ''$.last_message.runId'') = ''text''
                                      AND length(json_extract(
                                          source.canonical_json_payload, ''$.last_message.runId''
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
                                        ''$.last_message''
                                    ) = ''object''
                                    AND json_extract(
                                        predecessor_source.canonical_json_payload,
                                        ''$.last_message.messageId''
                                    ) = predecessor.chat_snapshot_message_id
                                    AND json_extract(
                                        predecessor_source.canonical_json_payload,
                                        ''$.last_message.chatId''
                                    ) = predecessor.aggregate_id
                                    AND json_extract(
                                        predecessor_source.canonical_json_payload,
                                        ''$.last_message.eventSequence''
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
                      AND json_type(source.canonical_json_payload, ''$.last_message'') = ''object''
                      AND json_type(
                          source.canonical_json_payload, ''$.last_message.eventSequence''
                      ) = ''integer''
                      AND json_extract(
                          source.canonical_json_payload, ''$.last_message.eventSequence''
                      ) = NEW.chat_durable_sequence
                      AND json_type(
                          source.canonical_json_payload, ''$.last_message.chatId''
                      ) = ''text''
                      AND json_extract(
                          source.canonical_json_payload, ''$.last_message.chatId''
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
    SELECT RAISE(ABORT, ''event source index does not match one exact source event'');
END'),
    ('trigger', 'events_are_append_only_delete', 'CREATE TRIGGER events_are_append_only_delete
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
    SELECT RAISE(ABORT, ''events ledger deletion lacks exact v7 authority'');
END'),
    ('trigger', 'events_are_append_only_update', 'CREATE TRIGGER events_are_append_only_update
BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, ''events ledger is append-only'');
END'),
    ('trigger', 'events_consume_exact_v7_delete_authority', 'CREATE TRIGGER events_consume_exact_v7_delete_authority
AFTER DELETE ON events
BEGIN
    DELETE FROM ledger_compaction_delete_authority
    WHERE event_rowid = OLD.rowid
      AND event_id = OLD.event_id
      AND operation_id = OLD.operation_id
      AND aggregate_type = OLD.aggregate_type
      AND aggregate_id = OLD.aggregate_id
      AND aggregate_sequence = OLD.aggregate_sequence;
END'),
    ('trigger', 'frozen_message_sources_append_only_delete', 'CREATE TRIGGER frozen_message_sources_append_only_delete
BEFORE DELETE ON frozen_message_sources
BEGIN
    SELECT RAISE(ABORT, ''frozen message sources are append-only'');
END'),
    ('trigger', 'frozen_message_sources_append_only_update', 'CREATE TRIGGER frozen_message_sources_append_only_update
BEFORE UPDATE ON frozen_message_sources
BEGIN
    SELECT RAISE(ABORT, ''frozen message sources are append-only'');
END'),
    ('trigger', 'frozen_message_sources_require_exact_source', 'CREATE TRIGGER frozen_message_sources_require_exact_source
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
      AND source_index.aggregate_type = ''chat''
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
    SELECT RAISE(ABORT, ''frozen message source requires exact frozen operation and message'');
END'),
    ('trigger', 'frozen_operation_sources_append_only_delete', 'CREATE TRIGGER frozen_operation_sources_append_only_delete
BEFORE DELETE ON frozen_operation_sources
BEGIN
    SELECT RAISE(ABORT, ''frozen operation sources are append-only'');
END'),
    ('trigger', 'frozen_operation_sources_append_only_update', 'CREATE TRIGGER frozen_operation_sources_append_only_update
BEFORE UPDATE ON frozen_operation_sources
BEGIN
    SELECT RAISE(ABORT, ''frozen operation sources are append-only'');
END'),
    ('trigger', 'frozen_operation_sources_require_exact_live_source', 'CREATE TRIGGER frozen_operation_sources_require_exact_live_source
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
      AND receipt.canonical_payload_digest_algorithm = ''sha256''
      AND receipt.canonical_payload_digest_version = 1
      AND receipt.canonical_payload_digest_format = ''lowercaseHex64''
      AND receipt.canonical_payload_digest = NEW.canonical_payload_digest
      AND receipt.event_timestamp = source.timestamp
      AND NEW.canonical_record_json = json_object(
          ''aggregateId'', NEW.aggregate_id,
          ''aggregateSequence'', NEW.aggregate_sequence,
          ''aggregateType'', NEW.aggregate_type,
          ''canonicalPayloadDigest'', NEW.canonical_payload_digest,
          ''canonicalPayloadJson'', NEW.canonical_payload_json,
          ''eventTimestamp'', NEW.event_timestamp,
          ''eventType'', NEW.event_type,
          ''frozenAt'', NEW.frozen_at,
          ''operationId'', NEW.operation_id,
          ''payloadSchemaVersion'', NEW.payload_schema_version,
          ''recordSchemaVersion'', 1,
          ''requestId'', NEW.request_id,
          ''sourceEventId'', NEW.source_event_id,
          ''sourceEventRowid'', NEW.source_event_rowid
      )
)
BEGIN
    SELECT RAISE(ABORT, ''frozen operation requires exact live source and receipt cross-check'');
END'),
    ('trigger', 'ledger_baselines_compaction_immutable_delete', 'CREATE TRIGGER ledger_baselines_compaction_immutable_delete
BEFORE DELETE ON ledger_baselines
BEGIN
    SELECT RAISE(ABORT, ''ledger baselines are immutable in storage v7'');
END'),
    ('trigger', 'ledger_baselines_compaction_immutable_update', 'CREATE TRIGGER ledger_baselines_compaction_immutable_update
BEFORE UPDATE ON ledger_baselines
BEGIN
    SELECT RAISE(ABORT, ''ledger baselines are immutable in storage v7'');
END'),
    ('trigger', 'ledger_compaction_delete_authority_delete_after_source', 'CREATE TRIGGER ledger_compaction_delete_authority_delete_after_source
BEFORE DELETE ON ledger_compaction_delete_authority
WHEN EXISTS (SELECT 1 FROM events WHERE rowid = OLD.event_rowid)
BEGIN
    SELECT RAISE(ABORT, ''live source retains delete authority until rollback or deletion'');
END'),
    ('trigger', 'ledger_compaction_delete_authority_no_update', 'CREATE TRIGGER ledger_compaction_delete_authority_no_update
BEFORE UPDATE ON ledger_compaction_delete_authority
BEGIN
    SELECT RAISE(ABORT, ''delete authority cannot be updated'');
END'),
    ('trigger', 'ledger_compaction_delete_authority_require_exact_tuple', 'CREATE TRIGGER ledger_compaction_delete_authority_require_exact_tuple
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
      AND job.job_kind = ''losslessCompaction''
      AND job.state = ''running''
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
    SELECT RAISE(ABORT, ''delete authority requires exact frozen operation and message tuple'');
END'),
    ('trigger', 'ledger_compaction_prefixes_append_only_delete', 'CREATE TRIGGER ledger_compaction_prefixes_append_only_delete
BEFORE DELETE ON ledger_compaction_prefixes
BEGIN
    SELECT RAISE(ABORT, ''compaction prefixes are append-only'');
END'),
    ('trigger', 'ledger_compaction_prefixes_append_only_update', 'CREATE TRIGGER ledger_compaction_prefixes_append_only_update
BEFORE UPDATE ON ledger_compaction_prefixes
BEGIN
    SELECT RAISE(ABORT, ''compaction prefixes are append-only'');
END'),
    ('trigger', 'ledger_compaction_prefixes_require_exact_anchor', 'CREATE TRIGGER ledger_compaction_prefixes_require_exact_anchor
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
    SELECT RAISE(ABORT, ''compaction prefix requires exact frozen anchor and baseline'');
END'),
    ('trigger', 'operation_receipts_compaction_immutable_delete', 'CREATE TRIGGER operation_receipts_compaction_immutable_delete
BEFORE DELETE ON operation_receipts
BEGIN
    SELECT RAISE(ABORT, ''operation receipts are immutable in storage v8'');
END'),
    ('trigger', 'operation_receipts_compaction_immutable_update', 'CREATE TRIGGER operation_receipts_compaction_immutable_update
BEFORE UPDATE ON operation_receipts
BEGIN
    SELECT RAISE(ABORT, ''operation receipts are immutable in storage v8'');
END')
), actual(type, name, sql) AS (
    SELECT type, name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_autoindex%'
)
SELECT CASE WHEN
    (SELECT COUNT(*) FROM storage_schema_meta) = 1
    AND EXISTS (
        SELECT 1 FROM storage_schema_meta
        WHERE singleton = 1
          AND storage_schema_version = 8
          AND protocol_version = 1
          AND migration_state = 'ready'
    )
    AND NOT EXISTS (
        SELECT 1 FROM maintenance_jobs
        WHERE job_kind = 'losslessCompaction' AND state IN ('pending', 'running')
    )
    AND NOT EXISTS (SELECT 1 FROM ledger_compaction_delete_authority)
    AND NOT EXISTS (SELECT type, name, sql FROM expected EXCEPT SELECT type, name, sql FROM actual)
    AND NOT EXISTS (SELECT type, name, sql FROM actual EXCEPT SELECT type, name, sql FROM expected)
THEN 1 ELSE 0 END;
DROP TRIGGER storage_v9_migration_guard_reject;
DROP TABLE storage_v9_migration_guard;

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
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'planEligibility'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') = 0
      AND json_type(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'null'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted') = 0
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete') = 0
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.operationsVerified') = 0
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified') = 0
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased') = 0
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
      AND job.state = 'running'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'planEligibility'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted') = 0
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete') = 0
      AND json_type(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'integer'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal')
          BETWEEN json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') + 1
              AND MIN(
                  json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') + 100,
                  1001
              )
      AND NEW.ordinal = json_extract(
          job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
      ) + 1
      AND NEW.ordinal <= json_extract(
          job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal'
      )
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

CREATE TRIGGER storage_compaction_plan_rows_advance_cursor
AFTER INSERT ON storage_compaction_plan_rows
BEGIN
    UPDATE maintenance_jobs
    SET cutoff_json = json_set(
        cutoff_json,
        '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal', NEW.ordinal,
        '$.storageCompactionV3PrivateState.cursor.scanExhausted',
            json(CASE WHEN NEW.ordinal = 1001 THEN 'true' ELSE 'false' END),
        '$.storageCompactionV3PrivateState.planRowsComplete',
            json(CASE WHEN NEW.ordinal = 1001 THEN 'true' ELSE 'false' END)
    )
    WHERE job_id = NEW.job_id;
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
      AND job.state = 'running'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'releasePlan'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete') = 1
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.operationsVerified') = 1
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified') = 1
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased') = 0
      AND json_type(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'integer'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal')
          BETWEEN json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') + 1
              AND MIN(
                  json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') + 100,
                  1000
              )
      AND OLD.ordinal = json_extract(
          job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
      ) + 1
      AND OLD.ordinal <= json_extract(
          job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal'
      )
      AND OLD.ordinal <= 1000
)
BEGIN
    SELECT RAISE(ABORT, 'compaction plan release requires next armed delete ordinal');
END;

CREATE TRIGGER storage_compaction_plan_rows_advance_release_cursor
AFTER DELETE ON storage_compaction_plan_rows
BEGIN
    UPDATE maintenance_jobs
    SET cutoff_json = json_set(
        cutoff_json,
        '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal', OLD.ordinal,
        '$.storageCompactionV3PrivateState.cursor.scanExhausted',
            json(CASE WHEN OLD.ordinal = 1000 THEN 'true' ELSE 'false' END),
        '$.storageCompactionV3PrivateState.planRowsReleased',
            json(CASE WHEN OLD.ordinal = 1000 THEN 'true' ELSE 'false' END)
    )
    WHERE job_id = OLD.job_id;
END;

CREATE TRIGGER maintenance_jobs_guard_v9_compaction_transition
BEFORE UPDATE OF state, cutoff_json ON maintenance_jobs
WHEN json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
  OR json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
BEGIN
    SELECT CASE WHEN NOT (
        json_valid(NEW.cutoff_json)
        AND json_type(NEW.cutoff_json, '$') = 'object'
        AND (SELECT COUNT(*) FROM json_each(NEW.cutoff_json)) = 1
        AND NOT EXISTS (
            SELECT 1 FROM json_each(NEW.cutoff_json)
            WHERE key <> 'storageCompactionV3PrivateState'
        )
        AND json_type(NEW.cutoff_json, '$.storageCompactionV3PrivateState') = 'object'
        AND (SELECT COUNT(*) FROM json_each(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState'
        )) = 6
        AND NOT EXISTS (
            SELECT 1 FROM json_each(NEW.cutoff_json, '$.storageCompactionV3PrivateState')
            WHERE key NOT IN (
                'schemaVersion', 'cursor', 'planRowsComplete', 'operationsVerified',
                'equivalenceVerified', 'planRowsReleased'
            )
        )
        AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
        AND json_type(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor') = 'object'
        AND (SELECT COUNT(*) FROM json_each(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor'
        )) = 6
        AND NOT EXISTS (
            SELECT 1 FROM json_each(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor')
            WHERE key NOT IN (
                'schemaVersion', 'cutoffEventRowid', 'phase', 'afterPlanOrdinal',
                'stepThroughPlanOrdinal', 'scanExhausted'
            )
        )
        AND json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.schemaVersion'
        ) = 3
        AND json_type(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.cutoffEventRowid'
        ) = 'integer'
        AND json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.cutoffEventRowid'
        ) BETWEEN 1 AND 9007199254740991
        AND json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
        ) IN (
            'planEligibility', 'freezeOperations', 'freezeMessages', 'verifyPrefix',
            'authorizeDelete', 'deletePrefix', 'verifyOperations', 'verifyEquivalence',
            'releasePlan', 'finalize'
        )
        AND json_type(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
        ) = 'integer'
        AND json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
        ) BETWEEN 0 AND CASE json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
        ) WHEN 'releasePlan' THEN 1000 WHEN 'finalize' THEN 1000 ELSE 1001 END
        AND json_type(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted'
        ) IN ('true', 'false')
        AND json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted'
        ) = (json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
        ) = CASE json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
        ) WHEN 'releasePlan' THEN 1000 WHEN 'finalize' THEN 1000 ELSE 1001 END)
        AND (
            json_type(
                NEW.cutoff_json,
                '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal'
            ) = 'null'
            OR (
                json_type(
                    NEW.cutoff_json,
                    '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal'
                ) = 'integer'
                AND json_extract(
                    NEW.cutoff_json,
                    '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal'
                ) BETWEEN json_extract(
                    NEW.cutoff_json,
                    '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
                ) AND MIN(
                    json_extract(
                        NEW.cutoff_json,
                        '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
                    ) + 100,
                    CASE json_extract(
                        NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
                    ) WHEN 'releasePlan' THEN 1000 ELSE 1001 END
                )
                AND (
                    json_extract(
                        NEW.cutoff_json,
                        '$.storageCompactionV3PrivateState.cursor.scanExhausted'
                    ) = 0
                    OR json_extract(
                        NEW.cutoff_json,
                        '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal'
                    ) = json_extract(
                        NEW.cutoff_json,
                        '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
                    )
                )
                AND json_extract(
                    NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
                ) <> 'finalize'
                AND NEW.state IN ('running', 'failed')
            )
        )
        AND json_type(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete'
        ) IN ('true', 'false')
        AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete')
            = (json_extract(
                NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
            ) <> 'planEligibility' OR json_extract(
                NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted'
            ) = 1)
        AND json_type(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.operationsVerified'
        ) IN ('true', 'false')
        AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.operationsVerified')
            = (json_extract(
                NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
            ) IN ('verifyEquivalence', 'releasePlan', 'finalize') OR (
                json_extract(
                    NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
                ) = 'verifyOperations' AND json_extract(
                    NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted'
                ) = 1
            ))
        AND json_type(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified'
        ) IN ('true', 'false')
        AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified')
            = (json_extract(
                NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
            ) IN ('releasePlan', 'finalize') OR (
                json_extract(
                    NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
                ) = 'verifyEquivalence' AND json_extract(
                    NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted'
                ) = 1
            ))
        AND json_type(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased'
        ) IN ('true', 'false')
        AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased')
            = (json_extract(
                NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
            ) = 'finalize' OR (
                json_extract(
                    NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase'
                ) = 'releasePlan' AND json_extract(
                    NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted'
                ) = 1
            ))
        AND (
            NEW.state IN ('pending', 'running')
            AND NEW.failure_kind IS NULL
            AND NEW.completed_at IS NULL
            OR NEW.state = 'succeeded'
            AND NEW.failure_kind IS NULL
            AND NEW.completed_at IS NOT NULL
            OR NEW.state = 'failed'
            AND length(NEW.failure_kind) > 0
            AND NEW.completed_at IS NOT NULL
        )
        AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
        AND json_extract(
            OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.schemaVersion'
        ) = 3
        AND json_extract(
            OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.cutoffEventRowid'
        ) = json_extract(
            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.cutoffEventRowid'
        )
        AND (
            (NEW.state = OLD.state AND NEW.cutoff_json = OLD.cutoff_json)
            OR (
                OLD.state = 'pending' AND NEW.state = 'running'
                AND NOT EXISTS (
                    SELECT 1 FROM ledger_compaction_delete_authority AS authority
                    WHERE authority.job_id = OLD.job_id
                )
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted')
                AND json_type(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'null'
                AND (
                    json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted') = 1
                    AND json_type(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'null'
                    OR json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted') = 0
                    AND json_type(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'integer'
                )
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete')
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.operationsVerified')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.operationsVerified')
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified')
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased')
            )
            OR (
                OLD.state = 'running' AND NEW.state = 'running'
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                    = json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') + 1
                AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal')
                    = json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal')
                AND (
                    json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'planEligibility'
                    AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
                         WHERE planned.job_id = OLD.job_id) = json_extract(
                             NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
                         )
                    OR json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'releasePlan'
                    AND NOT EXISTS (
                        SELECT 1 FROM storage_compaction_plan_rows AS planned
                        WHERE planned.job_id = OLD.job_id AND planned.ordinal <= json_extract(
                            NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
                        )
                    )
                    AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
                         WHERE planned.job_id = OLD.job_id) = 1001 - json_extract(
                             NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal'
                         )
                )
            )
            OR (
                OLD.state = 'running' AND NEW.state = 'pending'
                AND NOT EXISTS (
                    SELECT 1 FROM ledger_compaction_delete_authority AS authority
                    WHERE authority.job_id = OLD.job_id
                )
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                    = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                AND json_type(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'integer'
                AND json_type(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'null'
                AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                    = json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal')
                AND (
                    json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                        NOT IN ('planEligibility', 'releasePlan')
                    AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                        > json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                    AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
                         WHERE planned.job_id = OLD.job_id) = 1001
                    OR json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                        IN ('planEligibility', 'releasePlan')
                    AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                        = json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal')
                )
            )
            OR (
                OLD.state = 'running' AND NEW.state = 'pending'
                AND NOT EXISTS (
                    SELECT 1 FROM ledger_compaction_delete_authority AS authority
                    WHERE authority.job_id = OLD.job_id
                )
                AND json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted') = 1
                AND json_type(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'null'
                AND CASE json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                    WHEN 'planEligibility' THEN 'freezeOperations'
                    WHEN 'freezeOperations' THEN 'freezeMessages'
                    WHEN 'freezeMessages' THEN 'verifyPrefix'
                    WHEN 'verifyPrefix' THEN 'authorizeDelete'
                    WHEN 'authorizeDelete' THEN 'deletePrefix'
                    WHEN 'deletePrefix' THEN 'verifyOperations'
                    WHEN 'verifyOperations' THEN 'verifyEquivalence'
                    WHEN 'verifyEquivalence' THEN 'releasePlan'
                    WHEN 'releasePlan' THEN 'finalize'
                END = json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase')
                AND json_type(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'null'
                AND (
                    json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'finalize'
                    AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') = 1000
                    OR json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') <> 'finalize'
                    AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') = 0
                )
                AND (
                    json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'releasePlan'
                    AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
                         WHERE planned.job_id = OLD.job_id
                           AND planned.ordinal = 1001
                           AND planned.decision = 'retainAnchor') = 1
                    OR json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') <> 'releasePlan'
                    AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
                         WHERE planned.job_id = OLD.job_id) = 1001
                    AND (SELECT MIN(planned.ordinal) FROM storage_compaction_plan_rows AS planned
                         WHERE planned.job_id = OLD.job_id) = 1
                    AND (SELECT MAX(planned.ordinal) FROM storage_compaction_plan_rows AS planned
                         WHERE planned.job_id = OLD.job_id) = 1001
                )
                AND (
                    json_extract(OLD.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') <> 'verifyPrefix'
                    OR EXISTS (
                        SELECT 1
                        FROM storage_compaction_plans AS plan
                        JOIN ledger_compaction_prefixes AS prefix
                          ON prefix.aggregate_type = plan.aggregate_type
                         AND prefix.aggregate_id = plan.aggregate_id
                         AND prefix.deleted_through_sequence = plan.delete_through_sequence
                         AND prefix.anchor_aggregate_sequence = plan.anchor_aggregate_sequence
                         AND prefix.baseline_id = plan.baseline_id
                         AND prefix.baseline_projection_digest = plan.baseline_projection_digest
                        WHERE plan.job_id = OLD.job_id
                    )
                )
            )
            OR (
                OLD.state = 'pending' AND NEW.state = 'succeeded'
                AND NEW.cutoff_json = OLD.cutoff_json
                AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'finalize'
                AND json_extract(NEW.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') = 1000
                AND NOT EXISTS (
                    SELECT 1 FROM ledger_compaction_delete_authority AS authority
                    WHERE authority.job_id = OLD.job_id
                )
                AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
                     WHERE planned.job_id = OLD.job_id
                       AND planned.ordinal = 1001
                       AND planned.decision = 'retainAnchor') = 1
            )
            OR (
                OLD.state = 'running' AND NEW.state = 'failed'
                AND NEW.cutoff_json = OLD.cutoff_json
            )
        )
    ) THEN RAISE(ABORT, 'invalid storage v9 maintenance transition') END;
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
      AND job.state = 'running'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'verifyPrefix'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.afterPlanOrdinal') = 1001
      AND json_type(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal') = 'null'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.scanExhausted') = 1
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete') = 1
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.operationsVerified') = 0
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.equivalenceVerified') = 0
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased') = 0
      AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
           WHERE planned.job_id = plan.job_id) = 1001
      AND (SELECT COUNT(*) FROM storage_compaction_plan_rows AS planned
           WHERE planned.job_id = plan.job_id AND planned.decision = 'delete') = 1000
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
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.schemaVersion') = 3
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.cursor.phase') = 'authorizeDelete'
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsComplete') = 1
      AND json_extract(job.cutoff_json, '$.storageCompactionV3PrivateState.planRowsReleased') = 0
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
