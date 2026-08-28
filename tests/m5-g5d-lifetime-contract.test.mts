import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";

import { M5_BRIDGE_V5_METHODS } from "../src/m5/contracts/bridge-v5.ts";
import { M5_BRIDGE_V6_METHODS } from "../src/m5/contracts/bridge-v6.ts";
import { METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import {
  LOSSLESS_COMPACTION_STEPS_V1,
  NEVER_CLEANUP_STATES_V1,
  OPERATION_RECEIPT_DIGEST_ALGORITHM_V2,
  OPERATION_RECEIPT_DIGEST_FORMAT_V2,
  OPERATION_RECEIPT_DIGEST_VERSION_V2,
  STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1,
  STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1,
  STORAGE_LIFETIME_INDEXES_V2,
  STORAGE_LIFETIME_KEYSET_SCAN_ORDER_V1,
  STORAGE_LIFETIME_KEYSET_SCAN_PREDICATE_V1,
  STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
  STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
  STORAGE_LIFETIME_PHASES_V1,
  STORAGE_LIFETIME_SCHEMA_VERSION_V1,
  STORAGE_LIFETIME_SCHEMA_VERSION_V3,
  STORAGE_LIFETIME_TABLES_V2,
  STORAGE_LIFETIME_TABLES_V3,
  STORAGE_STATUS_BRIDGE_METHOD_V1,
  STORAGE_STATUS_BRIDGE_VERSION_V1,
  STORAGE_STATUS_METHOD_V1,
  STORAGE_STATUS_SCHEMA_VERSION_V1,
  classifyOperationAgainstReceiptV2,
  validateStorageLifetimeBatchPolicyV1,
  validateStorageLifetimeBatchSourcePayloadsV1,
  validateStorageLifetimeCursorV1,
  validateStorageLifetimeCursorTransitionV1,
  validateStorageLifetimeLifecycleTransitionV1,
  validateStorageLifetimeMaterializationJobV1,
  validateStorageLifetimeMaterializationPairV1,
  validateStorageStatusRequestV1,
  validateStorageStatusV1,
} from "../src/m5/contracts/storage-lifetime-v1.ts";

interface Manifest {
  schemaVersion: number;
  storageSchemaVersion: number;
  protocolVersion: number;
  wire: {
    method: string;
    payloadSchemaVersion: number;
    bridgeVersion: number;
    bridgeMethod: string;
    advertisedBeforeD1: boolean;
    dispatchableBeforeD1: boolean;
    casesFile: string;
  };
  migration: {
    file: string;
    predecessorMigrationFile: string;
    predecessorMigrationMutationAllowed: boolean;
    fromVersion: number;
    toVersion: number;
    mode: string;
    tables: string[];
    indexes: string[];
    existingTablesPreserved: string[];
    existingEventsDeleted: boolean;
    existingProjectionColumnsChanged: boolean;
    startupReadyBeforeMaterialization: boolean;
    materializationTrigger: string;
    materializationStateReadyRequired: boolean;
    downgradePolicy: string;
  };
  batching: Record<string, unknown> & { casesFile: string };
  compaction: {
    mode: string;
    steps: string[];
    baselineTailReplayEquivalent: boolean;
    oldOperationDuplicateConflictPreserved: boolean;
    actualLegacyNamespacePreserved: boolean;
    failureRollbackRequired: boolean;
    failureRollbackScope: {
      derivedPreparationBatchesMayPersist: boolean;
      sourceLedgerMutationAtomicRollbackRequired: boolean;
      compactionTriggerChangeAtomicRollbackRequired: boolean;
    };
    logicalDeleteBeforeVerification: boolean;
    vacuumIsLogicalSuccessCriterion: boolean;
    physicalCleanupEnabled: boolean;
    cleanupCandidateApiEnabled: boolean;
    cleanupActionApiEnabled: boolean;
    operationReceiptsImmutable: boolean;
    chatMessagesImmutable: boolean;
    operationReceipt: {
      casesFile: string;
      immutableFields: string[];
      requestIdComparison: string;
      payloadComparison: string;
      digest: {
        algorithm: string;
        version: number;
        format: string;
        role: string;
        writerMustRecompute: boolean;
      };
    };
    neverCleanupStates: string[];
  };
  publicStatusPrivacy: {
    allowedContent: string;
    forbiddenFields: string[];
  };
}

interface ManifestV3 {
  schemaVersion: number;
  storageSchemaVersion: number;
  protocolVersion: number;
  payloadSchemaVersion: number;
  migration: {
    file: string;
    fromVersion: number;
    toVersion: number;
    mode: string;
    preflight: string;
    idempotency: string;
    partialOrExtraSchema: string;
    tablesAdded: string[];
    tablesRebuilt: string[];
    preservedTables: string[];
    preservedIndexes: string[];
    preservedTriggers: string[];
    freshState: string;
    downgradePolicy: string;
  };
  batching: Record<string, unknown> & { casesFile: string };
  compactionBoundary: {
    maintenanceJobsUnchanged: boolean;
    maintenanceJobKind: string;
    materializationJobVisibleToCompactionStatus: boolean;
    physicalCleanupEnabled: boolean;
    ledgerDeletionEnabled: boolean;
    downgradePolicy: string;
  };
}

interface StatusCases {
  valid: Array<{ name: string; request: unknown; response: unknown }>;
  invalid: Array<{ name: string; target: "request" | "response"; value: unknown }>;
}

interface OperationReceiptCases {
  cases: Array<{
    name: string;
    namespace: string;
    receipt: { canonicalPayloadJson: string; canonicalPayloadDigest: string };
    incoming: unknown;
    expected: "append" | "duplicateNoOp" | "conflict";
  }>;
  invalid: Array<{
    name: string;
    expectedError: string;
    receipt: { canonicalPayloadJson: string; canonicalPayloadDigest: string };
  }>;
}

interface BatchingCases {
  validCursors: Array<{ name: string; value: unknown }>;
  validTransitions: CursorTransitionCase[];
  validPayloadBatches: Array<{ name: string; value: unknown }>;
  invalidCursors: Array<{ name: string; value: unknown }>;
  invalidTransitions: CursorTransitionCase[];
  invalidPayloadBatches: Array<{ name: string; value: unknown }>;
  validJobs: Array<{ name: string; value: unknown }>;
  validPairs: Array<{ name: string; value: unknown }>;
  validLifecycleTransitions: LifecycleTransitionCase[];
  invalidJobs: Array<{ name: string; value: unknown }>;
  invalidPairs: Array<{ name: string; value: unknown }>;
  invalidLifecycleTransitions: LifecycleTransitionCase[];
  futureSidecarRegression: { name: string; steps: string[] };
  invalidPolicyMutations: Array<{
    name: string;
    path: string[];
    replacement: unknown;
  }>;
}

interface LifecycleTransitionCase {
  name: string;
  operation: "create" | "commitBatch" | "cancel" | "resume" | "fail" | "complete" | "append" | "catchUpReset";
  previous: unknown;
  next: unknown;
}

interface CursorTransitionCase {
  name: string;
  previous: unknown | null;
  next: unknown;
}

const root = resolve("tests/fixtures/m5/storage-v2");
const rootV3 = resolve("tests/fixtures/m5/storage-v3");
const manifest = readJson<Manifest>(resolve(root, "manifest.json"));
const manifestV3 = readJson<ManifestV3>(resolve(rootV3, "manifest.json"));
const statusCases = readJson<StatusCases>(resolve(root, manifest.wire.casesFile));
const receiptCases = readJson<OperationReceiptCases>(
  resolve(root, manifest.compaction.operationReceipt.casesFile),
);
const batchingCases = readJson<BatchingCases>(resolve(rootV3, manifestV3.batching.casesFile));

test("storage-v2 golden migration is additive and preserves v1 rows byte-for-byte", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.deepEqual(manifest.migration.tables, [...STORAGE_LIFETIME_TABLES_V2]);
  assert.deepEqual(manifest.migration.indexes, [...STORAGE_LIFETIME_INDEXES_V2]);
  assert.equal(manifest.migration.mode, "additiveDdlIndexesMetadataOnly");
  assert.equal(manifest.migration.existingEventsDeleted, false);
  assert.equal(manifest.migration.existingProjectionColumnsChanged, false);
  assert.equal(manifest.migration.startupReadyBeforeMaterialization, true);
  assert.equal(manifest.migration.materializationTrigger, "idleAfterSidecarHandshakeReady");
  assert.equal(manifest.migration.materializationStateReadyRequired, false);
  assert.equal(manifest.migration.downgradePolicy, "openQuestion");

  const temporaryRoot = mkdtempSync(join(tmpdir(), "hello-agent-g5d-contract-"));
  const database = new DatabaseSync(join(temporaryRoot, "storage.sqlite3"), {
    enableForeignKeyConstraints: false,
  });
  try {
    database.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
    database.exec(`
      INSERT INTO app_instances (
        sidecar_instance_id, pid, platform, started_at, heartbeat_at,
        clean_shutdown_at, last_event_sequence
      ) VALUES (
        'sidecar_before_v2', 123, 'darwin', '2026-08-28T00:59:00Z',
        '2026-08-28T01:00:00Z', NULL, 1
      );
      INSERT INTO events (
        event_id, operation_id, request_id, aggregate_type, aggregate_id,
        aggregate_sequence, event_type, payload_schema_version,
        canonical_json_payload, timestamp
      ) VALUES (
        'evt_before_v2', 'op_before_v2', 'req_before_v2', 'project', 'project_before_v2',
        1, 'project.upserted', 1,
        '{"project_id":"project_before_v2"}', '2026-08-28T01:00:00Z'
      );
      INSERT INTO projects (
        project_id, canonical_path, projection_json, last_event_sequence
      ) VALUES (
        'project_before_v2', '/fixture/project', '{"project_id":"project_before_v2"}', 1
      ), (
        'project_other', '/fixture/other', '{"project_id":"project_other"}', 1
      );
      INSERT INTO chats (
        chat_id, project_id, projection_json, last_event_sequence
      ) VALUES (
        'chat_before_v2', 'project_before_v2', '{"chat_id":"chat_before_v2"}', 1
      ), (
        'chat_other_project', 'project_other', '{"chat_id":"chat_other_project"}', 1
      );
      INSERT INTO runs (
        run_id, chat_id, state, worktree_path, projection_json, last_event_sequence
      ) VALUES (
        'run_before_v2', 'chat_before_v2', 'recoveryRequired', '/fixture/worktree',
        '{"run_id":"run_before_v2"}', 1
      );
      INSERT INTO locks (
        lock_id, project_id, writer_run_id, worktree_path, poisoned,
        recovery_required, evidence_json, projection_json, last_event_sequence
      ) VALUES (
        'lock_before_v2', 'project_before_v2', 'run_before_v2', '/fixture/worktree',
        1, 1, '{}', '{"lock_id":"lock_before_v2"}', 1
      );
    `);
    const preservedTables = manifest.migration.existingTablesPreserved;
    const before = Object.fromEntries(
      preservedTables.map((table) => [table, normalizedRows(database, table)]),
    );

    database.exec(readFileSync(resolve(root, manifest.migration.file), "utf8"));

    for (const table of preservedTables) {
      assert.deepEqual(normalizedRows(database, table), before[table], table);
    }
    assert.deepEqual(
      { ...database.prepare("SELECT schema_version, protocol_version, migration_state FROM schema_meta").get() },
      { schema_version: 1, protocol_version: 1, migration_state: "ready" },
    );
    assert.deepEqual(
      { ...database.prepare("SELECT storage_schema_version, protocol_version, migration_state, materialization_state FROM storage_schema_meta").get() },
      { storage_schema_version: 2, protocol_version: 1, migration_state: "ready", materialization_state: "pending" },
    );
    for (const table of STORAGE_LIFETIME_TABLES_V2) {
      assert.equal(
        database.prepare("SELECT count(*) AS count FROM sqlite_schema WHERE type = 'table' AND name = ?").get(table)?.count,
        1,
        table,
      );
    }
    for (const index of STORAGE_LIFETIME_INDEXES_V2) {
      assert.equal(
        database.prepare("SELECT count(*) AS count FROM sqlite_schema WHERE type = 'index' AND name = ?").get(index)?.count,
        1,
        index,
      );
    }
    assert.throws(() => database.exec("UPDATE events SET event_type = 'changed' WHERE event_id = 'evt_before_v2';"));
    assert.throws(() => database.exec("DELETE FROM events WHERE event_id = 'evt_before_v2';"));
    database.exec(`
      INSERT INTO events (
        event_id, operation_id, request_id, aggregate_type, aggregate_id,
        aggregate_sequence, event_type, payload_schema_version,
        canonical_json_payload, timestamp
      ) VALUES (
        'evt_after_v2', 'op_after_v2', NULL, 'project', 'project_before_v2',
        2, 'project.upserted', 1,
        '{"project_id":"project_before_v2"}', '2026-08-28T01:01:00Z'
      );
    `);
    assert.equal(database.prepare("SELECT count(*) AS count FROM events").get()?.count, 2);

    database.exec(`
      INSERT INTO prepared_workspaces (
        preparation_id, project_id, worktree_id, workspace_identity, binding_state,
        bound_chat_id, canonical_record_json, record_digest,
        created_at, updated_at, materialized_at
      ) VALUES
        ('prep_unbound', 'project_before_v2', 'worktree_unbound', 'workspace_unbound',
         'unbound', NULL, '{}', 'digest-unbound',
         '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z'),
        ('prep_abandoned', 'project_before_v2', 'worktree_abandoned', 'workspace_abandoned',
         'abandoned', NULL, '{}', 'digest-abandoned',
         '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z'),
        ('prep_bound', 'project_before_v2', 'worktree_bound', 'workspace_bound',
         'bound', 'chat_before_v2', '{}', 'digest-bound',
         '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z');
    `);
    assert.equal(database.prepare("SELECT count(*) AS count FROM prepared_workspaces").get()?.count, 3);
    assert.throws(() => insertPreparedWorkspace(database, "invalid_bound_null", "bound", null, "project_before_v2"));
    assert.throws(() => insertPreparedWorkspace(database, "invalid_unbound_chat", "unbound", "chat_before_v2", "project_before_v2"));
    assert.throws(() => insertPreparedWorkspace(database, "invalid_abandoned_chat", "abandoned", "chat_before_v2", "project_before_v2"));
    assert.throws(() => insertPreparedWorkspace(database, "invalid_cross_project", "bound", "chat_other_project", "project_before_v2"));
  } finally {
    database.close();
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v3 adds a dedicated singleton without aliasing compaction history", () => {
  assert.equal(manifestV3.schemaVersion, 1);
  assert.equal(manifestV3.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V3);
  assert.equal(manifestV3.protocolVersion, PROTOCOL_VERSION);
  assert.equal(manifestV3.payloadSchemaVersion, 1);
  assert.deepEqual(manifestV3.migration.tablesAdded, ["lifetime_materialization_job"]);
  assert.equal(manifestV3.migration.predecessorMigrationFile, "../storage-v2/migration-v1-to-v2.sql");
  assert.equal(manifestV3.migration.predecessorMigrationMutationAllowed, false);
  assert.deepEqual(manifestV3.migration.tablesRebuilt, ["storage_schema_meta"]);
  assert.equal(manifestV3.migration.preflight, "exactStorageV2RequiredBeforeBeginImmediate");
  assert.equal(manifestV3.migration.idempotency, "exactStorageV3NoOpBeforeMigrationSql");
  assert.equal(manifestV3.migration.partialOrExtraSchema, "failClosedBeforeMutation");
  assert.equal(manifestV3.migration.freshState, "pendingWithAbsentJob");
  assert.equal(manifestV3.migration.downgradePolicy, "openQuestion");
  assert.equal(manifestV3.compactionBoundary.maintenanceJobsUnchanged, true);
  assert.equal(manifestV3.compactionBoundary.maintenanceJobKind, "losslessCompaction");
  assert.equal(manifestV3.compactionBoundary.materializationJobVisibleToCompactionStatus, false);
  assert.equal(manifestV3.compactionBoundary.physicalCleanupEnabled, false);
  assert.equal(manifestV3.compactionBoundary.ledgerDeletionEnabled, false);
  assert.equal(manifestV3.compactionBoundary.downgradePolicy, "openQuestion");
  assert.deepEqual([...STORAGE_LIFETIME_TABLES_V3], [
    ...STORAGE_LIFETIME_TABLES_V2,
    "lifetime_materialization_job",
  ]);

  const temporaryRoot = mkdtempSync(join(tmpdir(), "hello-agent-g5d-v3-contract-"));
  const database = new DatabaseSync(join(temporaryRoot, "storage.sqlite3"), {
    enableForeignKeyConstraints: false,
  });
  try {
    database.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
    database.exec(readFileSync(resolve(rootV3, manifestV3.migration.predecessorMigrationFile), "utf8"));
    seedPopulatedStorageV2(database);

    const expectedV2Schema = schemaSignature(database);
    const preservedBefore = Object.fromEntries(
      manifestV3.migration.preservedTables.map((table) => [table, normalizedRows(database, table)]),
    );
    const metaBefore = normalizedRows(database, "storage_schema_meta");
    const preservedTableSqlBefore = namedSchemaSql(
      database,
      "table",
      manifestV3.migration.preservedTables,
    );
    const preservedIndexSqlBefore = namedSchemaSql(
      database,
      "index",
      manifestV3.migration.preservedIndexes,
    );
    const triggersBefore = schemaObjects(database, "trigger");
    const compactionBefore = terminalMaintenanceRows(database);
    const migrationSql = readFileSync(resolve(rootV3, manifestV3.migration.file), "utf8");

    applyExactStorageV3Migration(database, migrationSql, expectedV2Schema, null);
    const expectedV3Schema = schemaSignature(database);

    for (const table of manifestV3.migration.preservedTables) {
      assert.deepEqual(normalizedRows(database, table), preservedBefore[table], table);
    }
    assert.deepEqual(normalizedRows(database, "storage_schema_meta"), [
      { ...metaBefore[0] as Record<string, unknown>, storage_schema_version: 3 },
    ]);
    assert.deepEqual(
      namedSchemaSql(database, "table", manifestV3.migration.preservedTables),
      preservedTableSqlBefore,
    );
    assert.deepEqual(
      namedSchemaSql(database, "index", manifestV3.migration.preservedIndexes),
      preservedIndexSqlBefore,
    );
    assert.deepEqual(triggersBefore, schemaObjects(database, "trigger"));
    assert.deepEqual(terminalMaintenanceRows(database), compactionBefore);
    assert.equal(
      database.prepare("SELECT count(*) AS count FROM lifetime_materialization_job").get()?.count,
      0,
    );

    database.exec(`
      INSERT INTO lifetime_materialization_job (
        singleton, job_id, state, cutoff_event_rowid, phase, after_event_rowid,
        failure_kind, created_at, updated_at, completed_at
      ) VALUES (
        1, 'lifetime-1', 'pending', 1, 'receipts', 0,
        NULL, '2026-08-28T02:00:00Z', '2026-08-28T02:00:00Z', NULL
      );
    `);
    assert.throws(() => database.exec(`
      INSERT INTO lifetime_materialization_job (
        singleton, job_id, state, cutoff_event_rowid, phase, after_event_rowid,
        failure_kind, created_at, updated_at, completed_at
      ) VALUES (
        2, 'lifetime-2', 'pending', 1, 'receipts', 0,
        NULL, '2026-08-28T02:00:00Z', '2026-08-28T02:00:00Z', NULL
      );
    `));
    assert.throws(() => database.exec("UPDATE lifetime_materialization_job SET state = 'running';"));
    assert.throws(() => database.exec("UPDATE lifetime_materialization_job SET phase = 'compact';"));
    assert.throws(() => database.exec("UPDATE lifetime_materialization_job SET after_event_rowid = 2;"));
    assert.deepEqual(terminalMaintenanceRows(database), compactionBefore);
    database.exec("DELETE FROM lifetime_materialization_job;");
    assert.throws(() => database.exec(`
      INSERT INTO lifetime_materialization_job (
        singleton, job_id, state, cutoff_event_rowid, phase, after_event_rowid,
        failure_kind, created_at, updated_at, completed_at
      ) VALUES (
        1, 'lifetime-raw-error', 'failed', 1, 'receipts', 0,
        'database: /Users/private/project', '2026-08-28T02:00:00Z',
        '2026-08-28T02:01:00Z', '2026-08-28T02:01:00Z'
      );
    `));

    database.exec(`
      INSERT INTO events (
        event_id, operation_id, request_id, aggregate_type, aggregate_id,
        aggregate_sequence, event_type, payload_schema_version,
        canonical_json_payload, timestamp
      ) VALUES (
        'evt_after_v3', 'op_after_v3', NULL, 'project', 'project_v3',
        2, 'project.upserted', 1, '{"project_id":"project_v3","revision":2}',
        '2026-08-28T02:05:00Z'
      );
    `);
    assert.equal(database.prepare("SELECT count(*) AS count FROM events").get()?.count, 2);

    applyExactStorageV3Migration(database, migrationSql, expectedV2Schema, expectedV3Schema);
    assert.equal(
      database.prepare("SELECT storage_schema_version FROM storage_schema_meta").get()?.storage_schema_version,
      3,
    );

    for (const mutation of ["partial", "extra"] as const) {
      const path = join(temporaryRoot, `${mutation}.sqlite3`);
      const invalid = new DatabaseSync(path, { enableForeignKeyConstraints: false });
      try {
        invalid.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
        invalid.exec(readFileSync(resolve(rootV3, manifestV3.migration.predecessorMigrationFile), "utf8"));
        if (mutation === "partial") {
          invalid.exec("DROP INDEX maintenance_jobs_status_created;");
        } else {
          invalid.exec("CREATE TABLE unexpected_storage_object (id INTEGER PRIMARY KEY) STRICT;");
        }
        const before = schemaSignature(invalid);
        assert.throws(() => {
          applyExactStorageV3Migration(invalid, migrationSql, expectedV2Schema, expectedV3Schema);
        });
        assert.deepEqual(schemaSignature(invalid), before, mutation);
        assert.equal(
          invalid.prepare("SELECT storage_schema_version FROM storage_schema_meta").get()?.storage_schema_version,
          2,
        );
      } finally {
        invalid.close();
      }
    }
  } finally {
    database.close();
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("TypeScript and Rust share exact privacy-safe storage.status cases", () => {
  assert.equal(manifest.wire.method, STORAGE_STATUS_METHOD_V1);
  assert.equal(manifest.wire.payloadSchemaVersion, STORAGE_STATUS_SCHEMA_VERSION_V1);
  assert.equal(manifest.wire.bridgeVersion, STORAGE_STATUS_BRIDGE_VERSION_V1);
  assert.equal(manifest.wire.bridgeMethod, STORAGE_STATUS_BRIDGE_METHOD_V1);
  assert.equal(manifest.publicStatusPrivacy.allowedContent, "aggregateCountsBytesAndMaintenanceStateOnly");
  assert.deepEqual(manifest.publicStatusPrivacy.forbiddenFields, [
    "rawChatText",
    "rawProviderText",
    "rawReasoning",
    "prompt",
    "token",
    "auth",
    "absolutePath",
  ]);

  for (const fixture of statusCases.valid) {
    assert.deepEqual(validateStorageStatusRequestV1(fixture.request), fixture.request, fixture.name);
    assert.deepEqual(validateStorageStatusV1(fixture.response), fixture.response, fixture.name);
  }
  for (const fixture of statusCases.invalid) {
    const validate = fixture.target === "request"
      ? () => validateStorageStatusRequestV1(fixture.value)
      : () => validateStorageStatusV1(fixture.value);
    assert.throws(validate, undefined, fixture.name);
  }
});

test("G5D batching fixes bounded keyset progress, cancellation, and atomic rollback semantics", () => {
  const policy = validateStorageLifetimeBatchPolicyV1(manifestV3.batching);
  assert.equal(policy.schemaVersion, STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1);
  assert.equal(policy.guards.maxSourceRowsPerBatch, STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1);
  assert.equal(
    policy.guards.maxCanonicalSourcePayloadBytesPerBatch,
    STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
  );
  assert.deepEqual(policy.guards.appliesToPhases, [
    "receipts", "messages", "workspaces", "baselines", "verify",
  ]);
  assert.equal(policy.cursor.schemaVersion, STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1);
  assert.deepEqual(policy.cursor.phases, [...STORAGE_LIFETIME_PHASES_V1]);
  assert.deepEqual(policy.cursor.scanPhases, [
    "receipts", "messages", "workspaces", "baselines", "verify",
  ]);
  assert.equal(policy.scan.predicate, STORAGE_LIFETIME_KEYSET_SCAN_PREDICATE_V1);
  assert.equal(policy.scan.orderBy, STORAGE_LIFETIME_KEYSET_SCAN_ORDER_V1);
  assert.equal(policy.scan.offsetAllowed, false);
  assert.equal(policy.source.artifactAuthority, "eventsAtOrBelowFrozenCutoffOnly");
  assert.equal(
    policy.source.workspaceAndBaselineSource,
    "eventProjectionSnapshotsAtOrBelowFrozenCutoff",
  );
  assert.equal(
    policy.source.liveProjectionUse,
    "finalCatchUpEquivalenceOnlyAfterMaxEventRowidEqualsCutoff",
  );
  assert.equal(policy.transaction.committedDerivedRowsMayPersist, true);
  assert.equal(policy.transaction.committedProgressState, "pending");
  assert.equal(policy.transaction.implementationMayCommitRunning, false);
  assert.equal(policy.cancellation.outcome, "Suspended");
  assert.equal(policy.job.table, "lifetime_materialization_job");
  assert.equal(policy.job.maximumRows, 1);
  assert.deepEqual(policy.job.states, ["pending", "failed"]);
  assert.equal(policy.job.maintenanceJobsRole, "losslessCompactionOnlyUnchanged");
  assert.equal(policy.job.batchCommit, "derivedRowsAndAdvancedPendingSingletonAtomic");
  assert.equal(policy.job.terminalFailure, "sameSingletonPendingToFailedPreserveCursorAfterRollback");
  assert.equal(policy.job.completion, "deleteSingletonAndSetMetaReadySameTransaction");
  assert.equal(policy.job.automaticRetryEnabled, false);
  assert.deepEqual(policy.pairing.validPairs, [
    "pending+absent", "pending+pending", "pending+failed", "ready+absent",
  ]);
  assert.equal(policy.append.readyInvalidation, "pendingInSameAppendTransaction");
  assert.equal(policy.append.readyPublication, "separateAtomicCatchUpVerificationTransaction");
  assert.deepEqual(policy.append.readyRegressionSequence, batchingCases.futureSidecarRegression.steps);
  assert.deepEqual(policy.finalPublication.prerequisites, [
    "maxEventRowidEqualsCutoff",
    "exactDerivedCompletenessAndEquivalence",
    "liveProjectionEqualsCutoffReconstruction",
    "boundedSqlKeysetEvidence",
  ]);
  assert.equal(policy.activation.ledgerDeletionEnabled, false);
  assert.equal(policy.activation.automaticSchedulingEnabled, false);
  assert.equal(policy.activation.compactionImplementationEnabled, false);
  assert.equal(policy.activation.statusWireEnabled, false);

  for (const fixture of batchingCases.validCursors) {
    assert.deepEqual(validateStorageLifetimeCursorV1(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of batchingCases.validTransitions) {
    assert.deepEqual(
      validateStorageLifetimeCursorTransitionV1(fixture.previous, fixture.next),
      fixture.next,
      fixture.name,
    );
  }
  for (const fixture of batchingCases.validPayloadBatches) {
    assert.deepEqual(validateStorageLifetimeBatchSourcePayloadsV1(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of batchingCases.invalidCursors) {
    assert.throws(() => validateStorageLifetimeCursorV1(fixture.value), undefined, fixture.name);
  }
  for (const fixture of batchingCases.invalidTransitions) {
    assert.throws(
      () => validateStorageLifetimeCursorTransitionV1(fixture.previous, fixture.next),
      undefined,
      fixture.name,
    );
  }
  for (const fixture of batchingCases.invalidPayloadBatches) {
    assert.throws(
      () => validateStorageLifetimeBatchSourcePayloadsV1(fixture.value),
      undefined,
      fixture.name,
    );
  }
  for (const fixture of batchingCases.validJobs) {
    assert.deepEqual(validateStorageLifetimeMaterializationJobV1(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of batchingCases.validPairs) {
    assert.deepEqual(validateStorageLifetimeMaterializationPairV1(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of batchingCases.validLifecycleTransitions) {
    assert.deepEqual(
      validateStorageLifetimeLifecycleTransitionV1(fixture.operation, fixture.previous, fixture.next),
      fixture.next,
      fixture.name,
    );
  }
  for (const fixture of batchingCases.invalidJobs) {
    assert.throws(() => validateStorageLifetimeMaterializationJobV1(fixture.value), undefined, fixture.name);
  }
  for (const fixture of batchingCases.invalidPairs) {
    assert.throws(() => validateStorageLifetimeMaterializationPairV1(fixture.value), undefined, fixture.name);
  }
  for (const fixture of batchingCases.invalidLifecycleTransitions) {
    assert.throws(
      () => validateStorageLifetimeLifecycleTransitionV1(fixture.operation, fixture.previous, fixture.next),
      undefined,
      fixture.name,
    );
  }
  for (const fixture of batchingCases.invalidPolicyMutations) {
    const invalid = structuredClone(manifestV3.batching);
    setPath(invalid, fixture.path, fixture.replacement);
    assert.throws(() => validateStorageLifetimeBatchPolicyV1(invalid), undefined, fixture.name);
  }
});

test("D0A freezes lossless compaction invariants without enabling cleanup or thresholds", () => {
  assert.equal(manifest.compaction.mode, "losslessInternalOnly");
  assert.deepEqual(manifest.compaction.steps, [...LOSSLESS_COMPACTION_STEPS_V1]);
  assert.deepEqual(manifest.compaction.neverCleanupStates, [...NEVER_CLEANUP_STATES_V1]);
  assert.equal(manifest.compaction.baselineTailReplayEquivalent, true);
  assert.equal(manifest.compaction.oldOperationDuplicateConflictPreserved, true);
  assert.equal(manifest.compaction.actualLegacyNamespacePreserved, true);
  assert.equal(manifest.compaction.failureRollbackRequired, true);
  assert.deepEqual(manifest.compaction.failureRollbackScope, {
    derivedPreparationBatchesMayPersist: true,
    sourceLedgerMutationAtomicRollbackRequired: true,
    compactionTriggerChangeAtomicRollbackRequired: true,
  });
  assert.equal(manifest.compaction.logicalDeleteBeforeVerification, false);
  assert.equal(manifest.compaction.vacuumIsLogicalSuccessCriterion, false);
  assert.equal(manifest.compaction.physicalCleanupEnabled, false);
  assert.equal(manifest.compaction.cleanupCandidateApiEnabled, false);
  assert.equal(manifest.compaction.cleanupActionApiEnabled, false);
  assert.equal(manifest.compaction.operationReceiptsImmutable, true);
  assert.equal(manifest.compaction.chatMessagesImmutable, true);
  assert.equal(manifest.compaction.operationReceipt.payloadComparison, "exactCanonicalJson");
  assert.equal(manifest.compaction.operationReceipt.requestIdComparison, "ignoredLikeStorageV1");
  assert.deepEqual(manifest.compaction.operationReceipt.immutableFields, [
    "operationId", "eventId", "aggregateType", "aggregateId", "aggregateSequence",
    "eventType", "payloadSchemaVersion", "canonicalPayloadJson",
    "canonicalPayloadDigestAlgorithm", "canonicalPayloadDigestVersion",
    "canonicalPayloadDigestFormat", "canonicalPayloadDigest", "eventTimestamp",
  ]);
  assert.deepEqual(manifest.compaction.operationReceipt.digest, {
    algorithm: OPERATION_RECEIPT_DIGEST_ALGORITHM_V2,
    version: OPERATION_RECEIPT_DIGEST_VERSION_V2,
    format: OPERATION_RECEIPT_DIGEST_FORMAT_V2,
    role: "integrityCheckNotEqualitySubstitute",
    writerMustRecompute: true,
  });
  assert.equal(JSON.stringify(manifest).includes("warningThreshold"), false);
  assert.equal(JSON.stringify(manifest).includes("hardThreshold"), false);
});

test("operation receipts preserve exact v1 duplicate/conflict decisions across legacy, actual, and recovery namespaces", () => {
  const namespaces = new Set<string>();
  for (const fixture of receiptCases.cases) {
    namespaces.add(fixture.namespace);
    assert.equal(
      createHash("sha256").update(fixture.receipt.canonicalPayloadJson).digest("hex"),
      fixture.receipt.canonicalPayloadDigest,
      `${fixture.name} digest integrity`,
    );
    assert.equal(
      classifyOperationAgainstReceiptV2(fixture.receipt, fixture.incoming),
      fixture.expected,
      fixture.name,
    );
  }
  assert.deepEqual([...namespaces].sort(), ["actualAgent", "legacy", "recovery"]);
  for (const fixture of receiptCases.invalid) {
    assert.throws(
      () => classifyOperationAgainstReceiptV2(fixture.receipt, receiptCases.cases[0].incoming),
      new RegExp(fixture.expectedError),
      fixture.name,
    );
  }
});

test("planned bridge V6 is additive but storage.status remains unadvertised and undispatchable before D1", () => {
  assert.deepEqual(M5_BRIDGE_V6_METHODS.slice(0, M5_BRIDGE_V5_METHODS.length), M5_BRIDGE_V5_METHODS);
  assert.deepEqual(M5_BRIDGE_V6_METHODS.slice(M5_BRIDGE_V5_METHODS.length), ["getStorageStatus"]);
  assert.equal(manifest.wire.advertisedBeforeD1, false);
  assert.equal(manifest.wire.dispatchableBeforeD1, false);
  assert.equal(METHODS_V1.includes(STORAGE_STATUS_METHOD_V1 as never), false);
  assert.equal(M5_BRIDGE_V6_METHODS.some((method) => /purge|cleanup|delete/i.test(method)), false);
});

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

function normalizedRows(database: DatabaseSync, table: string): unknown[] {
  return JSON.parse(JSON.stringify(database.prepare(`SELECT * FROM ${table} ORDER BY rowid`).all())) as unknown[];
}

function schemaObjects(database: DatabaseSync, type: "table" | "index" | "trigger"): unknown[] {
  return JSON.parse(JSON.stringify(database.prepare(`
    SELECT type, name, tbl_name, sql
    FROM sqlite_schema
    WHERE type = ? AND name NOT LIKE 'sqlite_%'
    ORDER BY type, name
  `).all(type))) as unknown[];
}

function schemaSignature(database: DatabaseSync): string {
  return JSON.stringify(JSON.parse(JSON.stringify(database.prepare(`
    SELECT type, name, tbl_name, sql
    FROM sqlite_schema
    WHERE name NOT LIKE 'sqlite_%'
    ORDER BY type, name
  `).all())));
}

function namedSchemaSql(
  database: DatabaseSync,
  type: "table" | "index" | "trigger",
  names: string[],
): unknown[] {
  return names.map((name) => ({
    name,
    sql: database.prepare("SELECT sql FROM sqlite_schema WHERE type = ? AND name = ?").get(type, name)?.sql,
  }));
}

function applyExactStorageV3Migration(
  database: DatabaseSync,
  migrationSql: string,
  expectedV2Schema: string,
  expectedV3Schema: string | null,
): void {
  const row = database.prepare(`
    SELECT storage_schema_version
    FROM storage_schema_meta
    WHERE singleton = 1
  `).get();
  if (row?.storage_schema_version === 3) {
    if (expectedV3Schema === null || schemaSignature(database) !== expectedV3Schema) {
      throw new Error("storage-v3 schema is not exact");
    }
    return;
  }
  if (row?.storage_schema_version !== 2 || schemaSignature(database) !== expectedV2Schema) {
    throw new Error("storage-v2 schema is not exact");
  }
  database.exec(migrationSql);
}

function terminalMaintenanceRows(database: DatabaseSync): unknown[] {
  return JSON.parse(JSON.stringify(database.prepare(`
    SELECT job_id, job_kind, state, cutoff_json, started_at, completed_at,
           last_successful_at, failure_kind, created_at, updated_at
    FROM maintenance_jobs
    WHERE state IN ('succeeded', 'failed')
    ORDER BY completed_at DESC, created_at DESC
  `).all())) as unknown[];
}

function seedPopulatedStorageV2(database: DatabaseSync): void {
  database.exec(`
    INSERT INTO app_instances (
      sidecar_instance_id, pid, platform, started_at, heartbeat_at,
      clean_shutdown_at, last_event_sequence
    ) VALUES (
      'sidecar_v3', 321, 'darwin', '2026-08-28T01:59:00Z',
      '2026-08-28T02:00:00Z', NULL, 1
    );
    INSERT INTO events (
      event_id, operation_id, request_id, aggregate_type, aggregate_id,
      aggregate_sequence, event_type, payload_schema_version,
      canonical_json_payload, timestamp
    ) VALUES (
      'evt_before_v3', 'op_before_v3', 'req_before_v3', 'project', 'project_v3',
      1, 'project.upserted', 1, '{"project_id":"project_v3"}',
      '2026-08-28T02:00:00Z'
    );
    INSERT INTO projects (
      project_id, canonical_path, projection_json, last_event_sequence
    ) VALUES (
      'project_v3', '/fixture/v3', '{"project_id":"project_v3"}', 1
    );
    INSERT INTO chats (
      chat_id, project_id, projection_json, last_event_sequence
    ) VALUES (
      'chat_v3', 'project_v3', '{"chat_id":"chat_v3"}', 1
    );
    INSERT INTO runs (
      run_id, chat_id, state, worktree_path, projection_json, last_event_sequence
    ) VALUES (
      'run_v3', 'chat_v3', 'completed', '/fixture/v3-worktree',
      '{"run_id":"run_v3"}', 1
    );
    INSERT INTO locks (
      lock_id, project_id, writer_run_id, worktree_path, poisoned,
      recovery_required, evidence_json, projection_json, last_event_sequence
    ) VALUES (
      'lock_v3', 'project_v3', NULL, '/fixture/v3-worktree',
      0, 0, '{}', '{"lock_id":"lock_v3"}', 1
    );
    INSERT INTO ledger_baselines (
      baseline_id, aggregate_type, aggregate_id, cutoff_aggregate_sequence,
      payload_schema_version, canonical_projection_json, projection_digest,
      created_at, verified_at
    ) VALUES (
      'baseline_v3', 'project', 'project_v3', 1, 1,
      '{"project_id":"project_v3"}', 'baseline-digest',
      '2026-08-28T02:00:00Z', '2026-08-28T02:00:00Z'
    );
    INSERT INTO operation_receipts (
      operation_id, event_id, aggregate_type, aggregate_id, aggregate_sequence,
      event_type, payload_schema_version, canonical_payload_json,
      canonical_payload_digest_algorithm, canonical_payload_digest_version,
      canonical_payload_digest_format, canonical_payload_digest,
      event_timestamp, materialized_at
    ) VALUES (
      'op_before_v3', 'evt_before_v3', 'project', 'project_v3', 1,
      'project.upserted', 1, '{"project_id":"project_v3"}',
      'sha256', 1, 'lowercaseHex64',
      'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      '2026-08-28T02:00:00Z', '2026-08-28T02:00:00Z'
    );
    INSERT INTO chat_messages (
      message_id, chat_id, event_sequence, canonical_message_json,
      message_digest, created_at, materialized_at
    ) VALUES (
      'message_v3', 'chat_v3', 1, '{"kind":"teamMessage"}',
      'message-digest', '2026-08-28T02:00:00Z', '2026-08-28T02:00:00Z'
    );
    INSERT INTO prepared_workspaces (
      preparation_id, project_id, worktree_id, workspace_identity, binding_state,
      bound_chat_id, canonical_record_json, record_digest,
      created_at, updated_at, materialized_at
    ) VALUES (
      'prep_v3', 'project_v3', 'worktree_v3', 'workspace_v3', 'bound',
      'chat_v3', '{}', 'workspace-digest',
      '2026-08-28T02:00:00Z', '2026-08-28T02:00:00Z', '2026-08-28T02:00:00Z'
    );
    INSERT INTO maintenance_jobs (
      job_id, job_kind, state, cutoff_json, started_at, completed_at,
      last_successful_at, failure_kind, created_at, updated_at
    ) VALUES (
      'compaction_v3', 'losslessCompaction', 'succeeded', '{"eventRowid":1}',
      '2026-08-28T02:00:00Z', '2026-08-28T02:01:00Z',
      '2026-08-28T02:01:00Z', NULL,
      '2026-08-28T02:00:00Z', '2026-08-28T02:01:00Z'
    );
  `);
}

function setPath(target: Record<string, unknown>, path: string[], replacement: unknown): void {
  let current: Record<string, unknown> = target;
  for (const segment of path.slice(0, -1)) {
    const next = current[segment];
    assert.equal(typeof next, "object");
    assert.notEqual(next, null);
    assert.equal(Array.isArray(next), false);
    current = next as Record<string, unknown>;
  }
  current[path.at(-1) as string] = replacement;
}

function insertPreparedWorkspace(
  database: DatabaseSync,
  id: string,
  state: "unbound" | "bound" | "abandoned",
  chatId: string | null,
  projectId: string,
): void {
  database.prepare(`
    INSERT INTO prepared_workspaces (
      preparation_id, project_id, worktree_id, workspace_identity, binding_state,
      bound_chat_id, canonical_record_json, record_digest,
      created_at, updated_at, materialized_at
    ) VALUES (?, ?, ?, ?, ?, ?, '{}', ?,
      '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z')
  `).run(id, projectId, `worktree_${id}`, `workspace_${id}`, state, chatId, `digest-${id}`);
}
