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
  STORAGE_LIFETIME_SCHEMA_VERSION_V4,
  STORAGE_LIFETIME_SCHEMA_VERSION_V5,
  STORAGE_LIFETIME_PHASES_V3,
  STORAGE_LIFETIME_TABLES_V2,
  STORAGE_LIFETIME_TABLES_V3,
  STORAGE_LIFETIME_V3_PREFLIGHT_ALLOWED_STATES_V1,
  STORAGE_LIFETIME_V3_PREFLIGHT_REJECTED_STATES_V1,
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
  validateStorageLifetimeV3PreflightPolicyV1,
  validateStorageLifetimeBatchPolicyV2,
  validateStorageLifetimeCursorTransitionV2,
  validateStorageLifetimeCursorV2,
  validateStorageLifetimeLifecycleTransitionV2,
  validateStorageLifetimeMaterializationJobV2,
  validateStorageLifetimeMaterializationPairV2,
  validateStorageLifetimeReopenPairV2,
  validateStorageLifetimeV4PreflightPolicyV1,
  validateStorageLifetimeVerificationEvidenceV2,
  validateStorageLifetimeBatchPolicyV3,
  validateStorageLifetimeCursorTransitionV3,
  validateStorageLifetimeCursorV3,
  validateStorageLifetimeLifecycleTransitionV3,
  validateStorageLifetimeMaterializationPairV3,
  validateStorageLifetimeV5PreflightPolicyV1,
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
    preflight: {
      schema: string;
      allowedMaterializationStates: string[];
      rejectedMaterializationStates: string[];
      timing: string;
    };
    idempotency: string;
    partialOrExtraSchema: string;
    tablesAdded: string[];
    tablesRebuilt: string[];
    preservedTables: string[];
    preservedIndexes: string[];
    preservedTriggers: string[];
    preservedMaterializationPairs: string[];
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

interface ManifestV4 {
  schemaVersion: number;
  storageSchemaVersion: number;
  protocolVersion: number;
  payloadSchemaVersion: number;
  migration: {
    file: string;
    predecessorMigrationFile: string;
    predecessorMigrationMutationAllowed: boolean;
    fromVersion: number;
    toVersion: number;
    mode: string;
    preflight: Record<string, unknown>;
    idempotency: string;
    partialExtraOrUnsafePair: string;
    tablesAdded: string[];
    tablesRebuilt: string[];
    preservedTables: string[];
    preservedIndexes: string[];
    preservedTriggers: string[];
    downgradePolicy: string;
  };
  boundedVerification: Record<string, unknown> & { casesFile: string };
}

interface V4Cases {
  v3MigrationCases: Array<{
    name: string; v3State: "pending" | "failed"; v3Phase: string; afterEventRowid: number;
    v4State: string; v4Phase: string; afterScanRowid: number; scanExhausted: boolean;
  }>;
  v3AbsentMigrationCases: Array<{ name: string; materializationState: "pending" | "ready" }>;
  removedLifecycleOperations: string[];
  validCursors: NamedValue[];
  invalidCursors: NamedValue[];
  validCursorTransitions: CursorTransitionCase[];
  invalidCursorTransitions: CursorTransitionCase[];
  validPayloadBatches: NamedValue[];
  invalidPayloadBatches: NamedValue[];
  validJobs: NamedValue[];
  invalidJobs: NamedValue[];
  validPairs: NamedValue[];
  invalidPairs: NamedValue[];
  runnableReopenPairs: number[];
  failClosedReopenPairs: number[];
  validLifecycleTransitions: Array<{
    name: string; operation: Parameters<typeof validateStorageLifetimeLifecycleTransitionV2>[0];
    previous: unknown; next: unknown;
  }>;
  invalidLifecycleTransitions: Array<{
    name: string; operation: Parameters<typeof validateStorageLifetimeLifecycleTransitionV2>[0];
    previous: unknown; next: unknown;
  }>;
  validVerificationEvidence: NamedValue[];
  invalidVerificationEvidence: NamedValue[];
  executionModels: {
    valid: Array<{ name: string; verifyBatchCommitted: boolean; outcome: string; finalPublicationSameCall: boolean }>;
    invalid: Array<{ name: string; verifyBatchCommitted: boolean; outcome: string; finalPublicationSameCall: boolean }>;
  };
  invalidPreflightMutations: PolicyMutation[];
  invalidPolicyMutations: PolicyMutation[];
}

interface ManifestV5 {
  schemaVersion: number;
  storageSchemaVersion: number;
  protocolVersion: number;
  payloadSchemaVersion: number;
  migration: ManifestV4["migration"];
  boundedProjectionVerification: Record<string, unknown> & { casesFile: string };
}

interface V5Cases {
  migrationMatrix: {
    jobStates: Array<"pending" | "attempting" | "failed">;
    positionPreservedPhases: string[];
    projectionRestartPhases: string[];
    absentMaterializationStates: Array<"pending" | "ready">;
    stateMapping: string;
    positionPreservedMapping: string;
    projectionRestartMapping: string;
  };
  phaseOrder: string[];
  validCatchUpResets: V5CatchUpCase[];
  invalidCatchUpResets: V5CatchUpCase[];
  validLifecycleOperations: string[];
  invalidPreflightMutations: PolicyMutation[];
  invalidPolicyMutations: PolicyMutation[];
}

interface V5CatchUpCase {
  name: string;
  previousState: "pending" | "attempting" | "failed";
  previousPhase: string;
  previousAfterScanRowid: number;
  previousScanExhausted: boolean;
  previousCutoff: number;
  observedCutoff: number;
}

interface NamedValue { name: string; value: unknown }
interface PolicyMutation { name: string; path: string[]; replacement: unknown }

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
  invalidPreflightMutations: Array<{
    name: string;
    path: string[];
    replacement: unknown;
  }>;
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
const rootV4 = resolve("tests/fixtures/m5/storage-v4");
const rootV5 = resolve("tests/fixtures/m5/storage-v5");
const manifest = readJson<Manifest>(resolve(root, "manifest.json"));
const manifestV3 = readJson<ManifestV3>(resolve(rootV3, "manifest.json"));
const statusCases = readJson<StatusCases>(resolve(root, manifest.wire.casesFile));
const receiptCases = readJson<OperationReceiptCases>(
  resolve(root, manifest.compaction.operationReceipt.casesFile),
);
const batchingCases = readJson<BatchingCases>(resolve(rootV3, manifestV3.batching.casesFile));
const manifestV4 = readJson<ManifestV4>(resolve(rootV4, "manifest.json"));
const v4Cases = readJson<V4Cases>(resolve(rootV4, manifestV4.boundedVerification.casesFile));
const manifestV5 = readJson<ManifestV5>(resolve(rootV5, "manifest.json"));
const v5Cases = readJson<V5Cases>(
  resolve(rootV5, manifestV5.boundedProjectionVerification.casesFile),
);

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
  assert.deepEqual(
    validateStorageLifetimeV3PreflightPolicyV1(manifestV3.migration.preflight),
    manifestV3.migration.preflight,
  );
  assert.deepEqual(
    manifestV3.migration.preflight.allowedMaterializationStates,
    [...STORAGE_LIFETIME_V3_PREFLIGHT_ALLOWED_STATES_V1],
  );
  assert.deepEqual(
    manifestV3.migration.preflight.rejectedMaterializationStates,
    [...STORAGE_LIFETIME_V3_PREFLIGHT_REJECTED_STATES_V1],
  );
  assert.equal(manifestV3.migration.idempotency, "exactStorageV3NoOpBeforeMigrationSql");
  assert.equal(manifestV3.migration.partialOrExtraSchema, "failClosedBeforeMutation");
  assert.deepEqual(manifestV3.migration.preservedMaterializationPairs, [
    "pending->pending+absent",
    "ready->ready+absent",
  ]);
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
      validateStorageLifetimeMaterializationPairV1({ materializationState: "pending", job: null }),
      { materializationState: "pending", job: null },
    );
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

    const readyPath = new DatabaseSync(join(temporaryRoot, "ready.sqlite3"), {
      enableForeignKeyConstraints: false,
    });
    try {
      readyPath.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
      readyPath.exec(readFileSync(resolve(rootV3, manifestV3.migration.predecessorMigrationFile), "utf8"));
      readyPath.exec("UPDATE storage_schema_meta SET materialization_state = 'ready';");
      applyExactStorageV3Migration(readyPath, migrationSql, expectedV2Schema, expectedV3Schema);
      assert.deepEqual(
        { ...readyPath.prepare(`
          SELECT materialization_state FROM storage_schema_meta WHERE singleton = 1
        `).get() },
        { materialization_state: "ready" },
      );
      assert.equal(
        readyPath.prepare("SELECT count(*) AS count FROM lifetime_materialization_job").get()?.count,
        0,
      );
    } finally {
      readyPath.close();
    }

    for (const unsafeState of ["materializing", "failed"] as const) {
      const unsafe = new DatabaseSync(join(temporaryRoot, `${unsafeState}.sqlite3`), {
        enableForeignKeyConstraints: false,
      });
      try {
        unsafe.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
        unsafe.exec(readFileSync(resolve(rootV3, manifestV3.migration.predecessorMigrationFile), "utf8"));
        unsafe.exec(`UPDATE storage_schema_meta SET materialization_state = '${unsafeState}';`);
        const schemaBefore = schemaSignature(unsafe);
        const dataBefore = databaseRowsSnapshot(unsafe);
        assert.throws(() => {
          applyExactStorageV3Migration(unsafe, migrationSql, expectedV2Schema, expectedV3Schema);
        });
        assert.equal(schemaSignature(unsafe), schemaBefore, unsafeState);
        assert.equal(databaseRowsSnapshot(unsafe), dataBefore, unsafeState);
        assert.equal(
          unsafe.prepare("SELECT storage_schema_version FROM storage_schema_meta").get()?.storage_schema_version,
          2,
        );
      } finally {
        unsafe.close();
      }
    }

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

test("storage-v4 rebuilds only lifetime metadata and maps every admissible v3 state", () => {
  assert.equal(manifestV4.schemaVersion, 1);
  assert.equal(manifestV4.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V4);
  assert.equal(manifestV4.protocolVersion, PROTOCOL_VERSION);
  assert.equal(manifestV4.migration.fromVersion, 3);
  assert.equal(manifestV4.migration.toVersion, 4);
  assert.equal(manifestV4.migration.predecessorMigrationMutationAllowed, false);
  assert.deepEqual(manifestV4.migration.tablesAdded, []);
  assert.deepEqual(manifestV4.migration.tablesRebuilt, [
    "storage_schema_meta", "lifetime_materialization_job",
  ]);
  assert.deepEqual(
    validateStorageLifetimeV4PreflightPolicyV1(manifestV4.migration.preflight),
    manifestV4.migration.preflight,
  );

  const temporaryRoot = mkdtempSync(join(tmpdir(), "hello-agent-g5d-v4-contract-"));
  const migrationV3 = readFileSync(resolve(rootV4, manifestV4.migration.predecessorMigrationFile), "utf8");
  const migrationV4 = readFileSync(resolve(rootV4, manifestV4.migration.file), "utf8");
  try {
    const preservationPath = join(temporaryRoot, "preservation.sqlite3");
    const preservation = new DatabaseSync(preservationPath, { enableForeignKeyConstraints: false });
    preservation.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
    preservation.exec(readFileSync(resolve(rootV3, manifestV3.migration.predecessorMigrationFile), "utf8"));
    seedPopulatedStorageV2(preservation);
    const exactV2 = schemaSignature(preservation);
    applyExactStorageV3Migration(preservation, migrationV3, exactV2, null);
    const exactV3 = schemaSignature(preservation);
    insertV3LifetimeJob(preservation, "pending", "verify", 1);

    const preservedRows = Object.fromEntries(
      manifestV4.migration.preservedTables.map((table) => [table, normalizedRows(preservation, table)]),
    );
    const preservedTableSql = namedSchemaSql(preservation, "table", manifestV4.migration.preservedTables);
    const preservedIndexSql = namedSchemaSql(preservation, "index", manifestV4.migration.preservedIndexes);
    const preservedTriggers = namedSchemaSql(preservation, "trigger", manifestV4.migration.preservedTriggers);
    const allOtherSchema = schemaObjectsExceptLifetime(preservation);
    const maintenanceRows = terminalMaintenanceRows(preservation);

    applyExactStorageV4Migration(preservation, migrationV4, exactV3, null);
    const exactV4 = schemaSignature(preservation);
    assert.equal(
      preservation.prepare("SELECT storage_schema_version FROM storage_schema_meta").get()?.storage_schema_version,
      4,
    );
    assert.deepEqual({ ...preservation.prepare(`
      SELECT state, cutoff_event_rowid, phase, after_scan_rowid, scan_exhausted,
             failure_kind, completed_at
      FROM lifetime_materialization_job
    `).get() }, {
      state: "pending", cutoff_event_rowid: 20, phase: "verifySource", after_scan_rowid: 1,
      scan_exhausted: 0,
      failure_kind: null, completed_at: null,
    });
    for (const table of manifestV4.migration.preservedTables) {
      assert.deepEqual(normalizedRows(preservation, table), preservedRows[table], table);
    }
    assert.deepEqual(namedSchemaSql(preservation, "table", manifestV4.migration.preservedTables), preservedTableSql);
    assert.deepEqual(namedSchemaSql(preservation, "index", manifestV4.migration.preservedIndexes), preservedIndexSql);
    assert.deepEqual(namedSchemaSql(preservation, "trigger", manifestV4.migration.preservedTriggers), preservedTriggers);
    assert.deepEqual(schemaObjectsExceptLifetime(preservation), allOtherSchema);
    assert.deepEqual(terminalMaintenanceRows(preservation), maintenanceRows);
    assert.throws(() => preservation.exec("DELETE FROM events;"));
    preservation.close();

    assert.deepEqual(
      v4Cases.v3MigrationCases.map((fixture) => `${fixture.v3State}:${fixture.v3Phase}`),
      [
        "pending:receipts", "pending:messages", "pending:workspaces", "pending:baselines",
        "pending:verify", "failed:receipts", "failed:messages", "failed:workspaces",
        "failed:baselines", "failed:verify",
      ],
    );
    for (const fixture of v4Cases.v3MigrationCases) {
      const database = createExactStorageV3Database(
        join(temporaryRoot, `${fixture.v3State}-${fixture.v3Phase}.sqlite3`),
        migrationV3,
      );
      try {
        insertV3LifetimeJob(
          database,
          fixture.v3State,
          fixture.v3Phase,
          fixture.afterEventRowid,
        );
        applyExactStorageV4Migration(database, migrationV4, exactV3, exactV4);
        assert.deepEqual({ ...database.prepare(`
          SELECT state, phase, after_scan_rowid, scan_exhausted
          FROM lifetime_materialization_job
        `).get() }, {
          state: fixture.v4State,
          phase: fixture.v4Phase,
          after_scan_rowid: fixture.afterScanRowid,
          scan_exhausted: fixture.scanExhausted ? 1 : 0,
        }, fixture.name);
      } finally {
        database.close();
      }
    }

    for (const fixture of v4Cases.v3AbsentMigrationCases) {
      const database = createExactStorageV3Database(
        join(temporaryRoot, `${fixture.materializationState}-absent.sqlite3`),
        migrationV3,
      );
      try {
        database.exec(`
          UPDATE storage_schema_meta
          SET materialization_state = '${fixture.materializationState}'
        `);
        applyExactStorageV4Migration(database, migrationV4, exactV3, exactV4);
        assert.deepEqual({ ...database.prepare(`
          SELECT storage_schema_version, materialization_state FROM storage_schema_meta
        `).get() }, {
          storage_schema_version: 4,
          materialization_state: fixture.materializationState,
        }, fixture.name);
        assert.equal(
          database.prepare("SELECT count(*) AS count FROM lifetime_materialization_job").get()?.count,
          0,
          fixture.name,
        );
        applyExactStorageV4Migration(database, migrationV4, exactV3, exactV4);
      } finally {
        database.close();
      }
    }

    const invalidSetups: Array<[string, (database: DatabaseSync) => void]> = [
      ["ready-present", (database) => {
        insertV3LifetimeJob(database, "pending", "receipts", 0);
        database.exec("UPDATE storage_schema_meta SET materialization_state = 'ready';");
      }],
      ["materializing-absent", (database) => {
        database.exec("UPDATE storage_schema_meta SET materialization_state = 'materializing';");
      }],
      ["failed-absent", (database) => {
        database.exec("UPDATE storage_schema_meta SET materialization_state = 'failed';");
      }],
      ["extra-schema", (database) => {
        database.exec("CREATE TABLE unexpected_v4_preflight_object (id INTEGER PRIMARY KEY) STRICT;");
      }],
    ];
    for (const [name, setup] of invalidSetups) {
      const path = join(temporaryRoot, `${name}.sqlite3`);
      let invalid = createExactStorageV3Database(path, migrationV3);
      setup(invalid);
      invalid.close();
      const before = readFileSync(path);
      invalid = new DatabaseSync(path, { enableForeignKeyConstraints: false });
      assert.throws(
        () => applyExactStorageV4Migration(invalid, migrationV4, exactV3, exactV4),
        undefined,
        name,
      );
      invalid.close();
      assert.deepEqual(readFileSync(path), before, `${name} must be byte-zero mutation`);
    }
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v4 freezes bounded arm, verification, cancellation, and final publication", () => {
  assert.deepEqual(
    validateStorageLifetimeBatchPolicyV2(manifestV4.boundedVerification),
    manifestV4.boundedVerification,
  );
  for (const fixture of v4Cases.validCursors) {
    assert.deepEqual(validateStorageLifetimeCursorV2(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of v4Cases.invalidCursors) {
    assert.throws(() => validateStorageLifetimeCursorV2(fixture.value), undefined, fixture.name);
  }
  for (const fixture of v4Cases.validCursorTransitions) {
    assert.deepEqual(
      validateStorageLifetimeCursorTransitionV2(fixture.previous, fixture.next),
      fixture.next,
      fixture.name,
    );
  }
  for (const fixture of v4Cases.invalidCursorTransitions) {
    assert.throws(
      () => validateStorageLifetimeCursorTransitionV2(fixture.previous, fixture.next),
      undefined,
      fixture.name,
    );
  }
  for (const fixture of v4Cases.validPayloadBatches) {
    assert.deepEqual(validateStorageLifetimeBatchSourcePayloadsV1(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of v4Cases.invalidPayloadBatches) {
    assert.throws(() => validateStorageLifetimeBatchSourcePayloadsV1(fixture.value), undefined, fixture.name);
  }
  for (const fixture of v4Cases.validJobs) {
    assert.deepEqual(validateStorageLifetimeMaterializationJobV2(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of v4Cases.invalidJobs) {
    assert.throws(() => validateStorageLifetimeMaterializationJobV2(fixture.value), undefined, fixture.name);
  }
  for (const fixture of v4Cases.validPairs) {
    assert.deepEqual(validateStorageLifetimeMaterializationPairV2(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of v4Cases.invalidPairs) {
    assert.throws(() => validateStorageLifetimeMaterializationPairV2(fixture.value), undefined, fixture.name);
  }
  for (const index of v4Cases.runnableReopenPairs) {
    assert.deepEqual(
      validateStorageLifetimeReopenPairV2(v4Cases.validPairs[index].value),
      v4Cases.validPairs[index].value,
    );
  }
  for (const index of v4Cases.failClosedReopenPairs) {
    assert.throws(() => validateStorageLifetimeReopenPairV2(v4Cases.validPairs[index].value));
  }
  assert.deepEqual(v4Cases.removedLifecycleOperations, ["markScanExhausted"]);
  for (const operation of v4Cases.removedLifecycleOperations) {
    assert.throws(
      () => validateStorageLifetimeLifecycleTransitionV2(
        operation as Parameters<typeof validateStorageLifetimeLifecycleTransitionV2>[0],
        v4Cases.validPairs[2]?.value,
        v4Cases.validPairs[2]?.value,
      ),
      undefined,
      `${operation} must not be representable as a public lifecycle operation`,
    );
  }
  for (const fixture of v4Cases.validLifecycleTransitions) {
    assert.deepEqual(
      validateStorageLifetimeLifecycleTransitionV2(fixture.operation, fixture.previous, fixture.next),
      fixture.next,
      fixture.name,
    );
  }
  for (const fixture of v4Cases.invalidLifecycleTransitions) {
    assert.throws(
      () => validateStorageLifetimeLifecycleTransitionV2(fixture.operation, fixture.previous, fixture.next),
      undefined,
      fixture.name,
    );
  }
  const directForge = v4Cases.invalidLifecycleTransitions.find(
    (fixture) => fixture.name === "direct lifecycle input cannot set exhaustion",
  );
  assert.ok(directForge);
  const publicOperations: Parameters<typeof validateStorageLifetimeLifecycleTransitionV2>[0][] = [
    "create", "arm", "batchSuccess", "phaseAdvance", "cancel", "fail",
    "failureRecordUnavailable", "finalPublish", "finalReset", "append",
  ];
  for (const operation of publicOperations) {
    assert.throws(
      () => validateStorageLifetimeLifecycleTransitionV2(
        operation, directForge.previous, directForge.next,
      ),
      undefined,
      `${operation} must not set scanExhausted false to true`,
    );
  }
  for (const fixture of v4Cases.validVerificationEvidence) {
    assert.deepEqual(validateStorageLifetimeVerificationEvidenceV2(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of v4Cases.invalidVerificationEvidence) {
    assert.throws(
      () => validateStorageLifetimeVerificationEvidenceV2(fixture.value), undefined, fixture.name,
    );
  }
  for (const fixture of v4Cases.executionModels.valid) {
    assert.equal(fixture.outcome, "Suspended", fixture.name);
    assert.equal(fixture.finalPublicationSameCall, false, fixture.name);
  }
  for (const fixture of v4Cases.executionModels.invalid) {
    assert.equal(
      fixture.verifyBatchCommitted && fixture.outcome === "Suspended" && fixture.finalPublicationSameCall,
      true,
      fixture.name,
    );
  }
  for (const fixture of v4Cases.invalidPreflightMutations) {
    const invalid = structuredClone(manifestV4.migration.preflight);
    setPath(invalid, fixture.path, fixture.replacement);
    assert.throws(() => validateStorageLifetimeV4PreflightPolicyV1(invalid), undefined, fixture.name);
  }
  for (const fixture of v4Cases.invalidPolicyMutations) {
    const invalid = structuredClone(manifestV4.boundedVerification);
    setPath(invalid, fixture.path, fixture.replacement);
    assert.throws(() => validateStorageLifetimeBatchPolicyV2(invalid), undefined, fixture.name);
  }
});

test("storage-v5 migrates every v4 state and phase without mutating preserved storage", () => {
  assert.equal(manifestV5.schemaVersion, 1);
  assert.equal(manifestV5.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V5);
  assert.equal(manifestV5.protocolVersion, PROTOCOL_VERSION);
  assert.equal(manifestV5.payloadSchemaVersion, 1);
  assert.equal(manifestV5.migration.fromVersion, 4);
  assert.equal(manifestV5.migration.toVersion, 5);
  assert.equal(manifestV5.migration.predecessorMigrationMutationAllowed, false);
  assert.deepEqual(manifestV5.migration.tablesAdded, []);
  assert.deepEqual(manifestV5.migration.tablesRebuilt, [
    "storage_schema_meta", "lifetime_materialization_job",
  ]);
  assert.deepEqual(
    validateStorageLifetimeV5PreflightPolicyV1(manifestV5.migration.preflight),
    manifestV5.migration.preflight,
  );

  const temporaryRoot = mkdtempSync(join(tmpdir(), "hello-agent-g5d-v5-contract-"));
  const migrationV3 = readFileSync(resolve(rootV3, manifestV3.migration.file), "utf8");
  const migrationV4 = readFileSync(resolve(rootV4, manifestV4.migration.file), "utf8");
  const migrationV5 = readFileSync(resolve(rootV5, manifestV5.migration.file), "utf8");
  try {
    const reference = createExactStorageV4Database(
      join(temporaryRoot, "reference.sqlite3"), migrationV3, migrationV4,
    );
    const exactV4 = schemaSignature(reference);
    applyExactStorageV5Migration(reference, migrationV5, exactV4, null);
    const exactV5 = schemaSignature(reference);
    reference.close();

    const preservation = createExactStorageV4Database(
      join(temporaryRoot, "preservation.sqlite3"), migrationV3, migrationV4,
    );
    seedPopulatedStorageV2(preservation);
    const preservedRows = Object.fromEntries(
      manifestV5.migration.preservedTables.map((table) => [table, normalizedRows(preservation, table)]),
    );
    const preservedSchema = schemaObjectsExceptLifetime(preservation);
    insertV4LifetimeJob(preservation, "pending", "verifyReceipts", 900, true);
    applyExactStorageV5Migration(preservation, migrationV5, exactV4, exactV5);
    assert.deepEqual({ ...preservation.prepare(`
      SELECT storage_schema_version, materialization_state FROM storage_schema_meta
    `).get() }, { storage_schema_version: 5, materialization_state: "pending" });
    assert.deepEqual({ ...preservation.prepare(`
      SELECT state, phase, after_scan_rowid, scan_exhausted
      FROM lifetime_materialization_job
    `).get() }, {
      state: "pending", phase: "verifySource", after_scan_rowid: 0, scan_exhausted: 0,
    });
    for (const table of manifestV5.migration.preservedTables) {
      assert.deepEqual(normalizedRows(preservation, table), preservedRows[table], table);
    }
    assert.deepEqual(schemaObjectsExceptLifetime(preservation), preservedSchema);
    assert.throws(() => preservation.exec("DELETE FROM events;"));
    preservation.close();

    for (const state of v5Cases.migrationMatrix.jobStates) {
      for (const phase of [
        ...v5Cases.migrationMatrix.positionPreservedPhases,
        ...v5Cases.migrationMatrix.projectionRestartPhases,
      ]) {
        const path = join(temporaryRoot, `${state}-${phase}.sqlite3`);
        const database = createExactStorageV4Database(path, migrationV3, migrationV4);
        insertV4LifetimeJob(database, state, phase, 3, true);
        applyExactStorageV5Migration(database, migrationV5, exactV4, exactV5);
        const row = database.prepare(`
          SELECT state, phase, after_scan_rowid, scan_exhausted, failure_kind, completed_at
          FROM lifetime_materialization_job
        `).get();
        const restarts = v5Cases.migrationMatrix.projectionRestartPhases.includes(phase);
        assert.deepEqual({ ...row }, {
          state,
          phase: restarts ? "verifySource" : phase,
          after_scan_rowid: restarts ? 0 : 3,
          scan_exhausted: restarts ? 0 : 1,
          failure_kind: state === "failed" ? "contractValidation" : null,
          completed_at: state === "failed" ? "2026-08-28T12:02:00Z" : null,
        }, `${state}:${phase}`);
        database.close();
      }
    }

    for (const materializationState of v5Cases.migrationMatrix.absentMaterializationStates) {
      const database = createExactStorageV4Database(
        join(temporaryRoot, `${materializationState}-absent-v5.sqlite3`), migrationV3, migrationV4,
      );
      database.exec(`UPDATE storage_schema_meta SET materialization_state = '${materializationState}'`);
      applyExactStorageV5Migration(database, migrationV5, exactV4, exactV5);
      assert.equal(database.prepare(`
        SELECT materialization_state FROM storage_schema_meta
      `).get()?.materialization_state, materializationState);
      assert.equal(database.prepare(`
        SELECT count(*) AS count FROM lifetime_materialization_job
      `).get()?.count, 0);
      applyExactStorageV5Migration(database, migrationV5, exactV4, exactV5);
      database.close();
    }

    const invalidSetups: Array<[string, (database: DatabaseSync) => void]> = [
      ["ready-present", (database) => {
        insertV4LifetimeJob(database, "pending", "receipts", 0, false);
        database.exec("UPDATE storage_schema_meta SET materialization_state = 'ready';");
      }],
      ["partial-extra-schema", (database) => {
        database.exec("CREATE TABLE unexpected_v5_preflight_object (id INTEGER PRIMARY KEY) STRICT;");
      }],
    ];
    for (const [name, setup] of invalidSetups) {
      const path = join(temporaryRoot, `${name}.sqlite3`);
      let database = createExactStorageV4Database(path, migrationV3, migrationV4);
      setup(database);
      database.close();
      const before = readFileSync(path);
      database = new DatabaseSync(path, { enableForeignKeyConstraints: false });
      assert.throws(() => applyExactStorageV5Migration(database, migrationV5, exactV4, exactV5));
      database.close();
      assert.deepEqual(readFileSync(path), before, `${name} must be byte-zero mutation`);
    }
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v5 freezes projection order, lifecycle catch-up, and bounded query policy", () => {
  assert.deepEqual(
    validateStorageLifetimeBatchPolicyV3(manifestV5.boundedProjectionVerification),
    manifestV5.boundedProjectionVerification,
  );
  assert.deepEqual(v5Cases.phaseOrder, [...STORAGE_LIFETIME_PHASES_V3]);
  for (const [index, phase] of v5Cases.phaseOrder.entries()) {
    const cursor = {
      schemaVersion: 3,
      cutoffEventRowid: 20,
      phase,
      afterScanRowid: index <= 4 ? 20 : 900,
      scanExhausted: true,
    };
    assert.deepEqual(validateStorageLifetimeCursorV3(cursor), cursor, phase);
    const nextPhase = v5Cases.phaseOrder[index + 1];
    if (nextPhase !== undefined) {
      const next = {
        schemaVersion: 3, cutoffEventRowid: 20, phase: nextPhase,
        afterScanRowid: 0, scanExhausted: false,
      };
      assert.deepEqual(validateStorageLifetimeCursorTransitionV3(cursor, next), next, phase);
    }
  }
  assert.deepEqual(v5Cases.validLifecycleOperations, [
    "create", "arm", "batchSuccess", "phaseAdvance", "cancel", "fail",
    "failureRecordUnavailable", "finalPublish", "catchUpReset", "append",
  ]);
  const pendingAbsent = { materializationState: "pending", job: null };
  const readyAbsent = { materializationState: "ready", job: null };
  const created = v5Snapshot("pending", "receipts", 0, false);
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3("create", pendingAbsent, created),
    created,
  );
  const armed = v5Snapshot("attempting", "receipts", 0, false);
  assert.deepEqual(validateStorageLifetimeLifecycleTransitionV3("arm", created, armed), armed);
  const advancedBatch = v5Snapshot("pending", "receipts", 1, false);
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3("batchSuccess", armed, advancedBatch),
    advancedBatch,
  );
  const exhaustedProject = v5Snapshot("pending", "verifyProjects", 900, true);
  const enteredChats = v5Snapshot("pending", "verifyChats", 0, false);
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3("phaseAdvance", exhaustedProject, enteredChats),
    enteredChats,
  );
  const attemptingProject = v5Snapshot("attempting", "verifyProjects", 900, false);
  const cancelledProject = v5Snapshot("pending", "verifyProjects", 900, false);
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3("cancel", attemptingProject, cancelledProject),
    cancelledProject,
  );
  const failedProject = v5Snapshot("failed", "verifyProjects", 900, false);
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3("fail", attemptingProject, failedProject),
    failedProject,
  );
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3(
      "failureRecordUnavailable", attemptingProject, attemptingProject,
    ),
    attemptingProject,
  );
  const exhaustedBaselines = v5Snapshot("pending", "verifyBaselines", 901, true);
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3("finalPublish", exhaustedBaselines, readyAbsent),
    readyAbsent,
  );
  assert.deepEqual(
    validateStorageLifetimeLifecycleTransitionV3("append", readyAbsent, pendingAbsent),
    pendingAbsent,
  );
  for (const fixture of v5Cases.validCatchUpResets) {
    const previous = v5SnapshotForCase(fixture, false);
    const next = v5SnapshotForCase(fixture, true);
    assert.deepEqual(
      validateStorageLifetimeLifecycleTransitionV3("catchUpReset", previous, next),
      next,
      fixture.name,
    );
  }
  for (const fixture of v5Cases.invalidCatchUpResets) {
    assert.throws(
      () => validateStorageLifetimeLifecycleTransitionV3(
        "catchUpReset", v5SnapshotForCase(fixture, false), v5SnapshotForCase(fixture, true),
      ),
      undefined,
      fixture.name,
    );
  }
  const directExhaustionPrevious = v5Snapshot("pending", "verifyProjects", 0, false);
  const directExhaustionNext = v5Snapshot("pending", "verifyProjects", 0, true);
  for (const operation of v5Cases.validLifecycleOperations) {
    assert.throws(
      () => validateStorageLifetimeLifecycleTransitionV3(
        operation as Parameters<typeof validateStorageLifetimeLifecycleTransitionV3>[0],
        directExhaustionPrevious,
        directExhaustionNext,
      ),
      undefined,
      `${operation} cannot forge private exhaustion`,
    );
  }
  for (const fixture of v5Cases.invalidPreflightMutations) {
    const invalid = structuredClone(manifestV5.migration.preflight);
    setPath(invalid, fixture.path, fixture.replacement);
    assert.throws(() => validateStorageLifetimeV5PreflightPolicyV1(invalid), undefined, fixture.name);
  }
  for (const fixture of v5Cases.invalidPolicyMutations) {
    const invalid = structuredClone(manifestV5.boundedProjectionVerification);
    setPath(invalid, fixture.path, fixture.replacement);
    assert.throws(() => validateStorageLifetimeBatchPolicyV3(invalid), undefined, fixture.name);
  }
  assert.deepEqual(
    manifestV5.boundedProjectionVerification.snapshotAggregates,
    {
      includedProjectionTables: ["projects", "chats", "runs", "locks"],
      excludedProjectionTables: ["app_instances"],
      appInstancesExclusionReason: "lifecycleProjectionNotMaterializedSnapshotAggregate",
    },
  );
});

test("storage-v1 through storage-v4 golden files remain immutable", () => {
  const digests = new Map<string, string>([
    ["tests/fixtures/m5/storage-v1/idempotency.json", "a0c9d347a5005af0980c35d8e55de2ee1cdfaa47dc1c35959582da667ae0b641"],
    ["tests/fixtures/m5/storage-v1/migration.sql", "9734d16cca9cf5cd18188f4e75cde6bbedaff2eda14ab5bd8981fdc45ae82d12"],
    ["tests/fixtures/m5/storage-v1/manifest.json", "604be3bbe81e80876afc7334a4fb3ee42fc070319f35fccbfc24c963a79dd980"],
    ["tests/fixtures/m5/storage-v1/recovery.json", "10fc6695fda0a0d764dd8c147eef2de733b3817f34f685518f4b4ca739234862"],
    ["tests/fixtures/m5/storage-v1/replay.json", "322626cd1ef18a5b9dbc8f921fa1a23b0a771a76f1da8aa33198b0a3dc56c1ab"],
    ["tests/fixtures/m5/storage-v1/transaction.json", "5b3ff57be3a7be11ffcac8776721e6e349703a2707ae8e16071a9935c78d377f"],
    ["tests/fixtures/m5/storage-v2/batching-cases.json", "b1a4c77887f804f552e8c9893137e8aaae18f46f8aff90689e4618b5ee454629"],
    ["tests/fixtures/m5/storage-v2/manifest.json", "66c96a2a6e3aa6dff609981ac143ffa1d1f7d81ea620dabc5fedbe1f2f6d38df"],
    ["tests/fixtures/m5/storage-v2/migration-v1-to-v2.sql", "47ece81ddad1fdb0fd22531117cae298862557e52fe32ff20f0d9d4f2550b038"],
    ["tests/fixtures/m5/storage-v2/operation-receipt-cases.json", "b3480936dcfcfd66fb711d7ae80d4680699d5badc2cba59999cf6feee3d35fc4"],
    ["tests/fixtures/m5/storage-v2/status-cases.json", "932be0ec8eccd2a4a4c1cc08e2f3f1eec9c02499c983709fa387afcedf8ce951"],
    ["tests/fixtures/m5/storage-v3/manifest.json", "6cbaab2b0c01ccb3f94908ba0bef2ec054f2ba1689bea17fb672441c0aa71dc5"],
    ["tests/fixtures/m5/storage-v3/migration-v2-to-v3.sql", "39fc5199d64423512cd43fac9adc1edb13fe189f5580cfb96bf02d011385a58b"],
    ["tests/fixtures/m5/storage-v3/batching-cases.json", "95a3bd70bc8e49b5192465ef77e1832f2f8489f9a2847ed9ec31135343aab050"],
    ["tests/fixtures/m5/storage-v4/manifest.json", "f484c1adfefe9515d0d2d5aa16e0398a0051e77ca5f59bd7d2bc2a274fd8eb28"],
    ["tests/fixtures/m5/storage-v4/migration-v3-to-v4.sql", "e8711c4e8aa214c7cb4ecb2dd9962e5622bcd2844221ecccca998db24cc1b1c6"],
    ["tests/fixtures/m5/storage-v4/cases.json", "ea71b933225c77190ef87838037eeaa8c61293bd158adbfc80b7dd2df6aa3841"],
  ]);
  for (const [path, expected] of digests) {
    assert.equal(createHash("sha256").update(readFileSync(path)).digest("hex"), expected, path);
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
  for (const fixture of batchingCases.invalidPreflightMutations) {
    const invalid = structuredClone(manifestV3.migration.preflight);
    setPath(invalid as Record<string, unknown>, fixture.path, fixture.replacement);
    assert.throws(
      () => validateStorageLifetimeV3PreflightPolicyV1(invalid),
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

function schemaObjectsExceptLifetime(database: DatabaseSync): unknown[] {
  return JSON.parse(JSON.stringify(database.prepare(`
    SELECT type, name, tbl_name, sql
    FROM sqlite_schema
    WHERE name NOT LIKE 'sqlite_%'
      AND tbl_name NOT IN ('storage_schema_meta', 'lifetime_materialization_job')
    ORDER BY type, name
  `).all())) as unknown[];
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
    SELECT storage_schema_version, materialization_state
    FROM storage_schema_meta
    WHERE singleton = 1
  `).get();
  if (row?.storage_schema_version === 3) {
    if (expectedV3Schema === null || schemaSignature(database) !== expectedV3Schema) {
      throw new Error("storage-v3 schema is not exact");
    }
    return;
  }
  if (row?.storage_schema_version !== 2
    || (row.materialization_state !== "pending" && row.materialization_state !== "ready")
    || schemaSignature(database) !== expectedV2Schema) {
    throw new Error("storage-v2 schema is not exact");
  }
  database.exec(migrationSql);
}

function applyExactStorageV4Migration(
  database: DatabaseSync,
  migrationSql: string,
  expectedV3Schema: string,
  expectedV4Schema: string | null,
): void {
  const meta = database.prepare(`
    SELECT storage_schema_version, materialization_state
    FROM storage_schema_meta WHERE singleton = 1
  `).get();
  if (meta?.storage_schema_version === 4) {
    if (expectedV4Schema === null || schemaSignature(database) !== expectedV4Schema) {
      throw new Error("storage-v4 schema is not exact");
    }
    return;
  }
  if (meta?.storage_schema_version !== 3 || schemaSignature(database) !== expectedV3Schema) {
    throw new Error("storage-v3 schema is not exact");
  }
  const job = database.prepare(`
    SELECT singleton, job_id, state, cutoff_event_rowid, phase, after_event_rowid,
           failure_kind, created_at, updated_at, completed_at
    FROM lifetime_materialization_job WHERE singleton = 1
  `).get();
  validateStorageLifetimeMaterializationPairV1({
    materializationState: meta.materialization_state,
    job: job === undefined ? null : {
      singleton: job.singleton,
      jobId: job.job_id,
      state: job.state,
      cutoffEventRowid: job.cutoff_event_rowid,
      phase: job.phase,
      afterEventRowid: job.after_event_rowid,
      failureKind: job.failure_kind,
      createdAt: job.created_at,
      updatedAt: job.updated_at,
      completedAt: job.completed_at,
    },
  });
  database.exec(migrationSql);
  if (expectedV4Schema !== null && schemaSignature(database) !== expectedV4Schema) {
    throw new Error("storage-v4 migration did not produce exact schema");
  }
}

function createExactStorageV3Database(path: string, migrationV3: string): DatabaseSync {
  const database = new DatabaseSync(path, { enableForeignKeyConstraints: false });
  database.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
  database.exec(readFileSync(resolve(rootV3, manifestV3.migration.predecessorMigrationFile), "utf8"));
  database.exec(migrationV3);
  return database;
}

function createExactStorageV4Database(
  path: string,
  migrationV3: string,
  migrationV4: string,
): DatabaseSync {
  const database = createExactStorageV3Database(path, migrationV3);
  const exactV3 = schemaSignature(database);
  applyExactStorageV4Migration(database, migrationV4, exactV3, null);
  return database;
}

function applyExactStorageV5Migration(
  database: DatabaseSync,
  migrationSql: string,
  expectedV4Schema: string,
  expectedV5Schema: string | null,
): void {
  const meta = database.prepare(`
    SELECT storage_schema_version, materialization_state
    FROM storage_schema_meta WHERE singleton = 1
  `).get();
  if (meta?.storage_schema_version === 5) {
    if (expectedV5Schema === null || schemaSignature(database) !== expectedV5Schema) {
      throw new Error("storage-v5 schema is not exact");
    }
    return;
  }
  if (meta?.storage_schema_version !== 4 || schemaSignature(database) !== expectedV4Schema) {
    throw new Error("storage-v4 schema is not exact");
  }
  const row = database.prepare(`
    SELECT singleton, job_id, state, cutoff_event_rowid, phase, after_scan_rowid,
           scan_exhausted, failure_kind, created_at, updated_at, completed_at
    FROM lifetime_materialization_job WHERE singleton = 1
  `).get();
  validateStorageLifetimeMaterializationPairV2({
    materializationState: meta.materialization_state,
    job: row === undefined ? null : {
      singleton: row.singleton,
      jobId: row.job_id,
      state: row.state,
      cutoffEventRowid: row.cutoff_event_rowid,
      phase: row.phase,
      afterScanRowid: row.after_scan_rowid,
      scanExhausted: row.scan_exhausted === 1,
      failureKind: row.failure_kind,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
      completedAt: row.completed_at,
    },
  });
  database.exec(migrationSql);
  if (expectedV5Schema !== null && schemaSignature(database) !== expectedV5Schema) {
    throw new Error("storage-v5 migration did not produce exact schema");
  }
}

function insertV4LifetimeJob(
  database: DatabaseSync,
  state: "pending" | "attempting" | "failed",
  phase: string,
  afterScanRowid: number,
  scanExhausted: boolean,
): void {
  database.prepare(`
    INSERT INTO lifetime_materialization_job (
      singleton, job_id, state, cutoff_event_rowid, phase, after_scan_rowid,
      scan_exhausted, failure_kind, created_at, updated_at, completed_at
    ) VALUES (1, ?, ?, 20, ?, ?, ?, ?, ?, ?, ?)
  `).run(
    `lifetime-${state}-${phase}`,
    state,
    phase,
    afterScanRowid,
    scanExhausted ? 1 : 0,
    state === "failed" ? "contractValidation" : null,
    "2026-08-28T12:00:00Z",
    state === "failed" ? "2026-08-28T12:02:00Z" : "2026-08-28T12:01:00Z",
    state === "failed" ? "2026-08-28T12:02:00Z" : null,
  );
}

function v5SnapshotForCase(fixture: V5CatchUpCase, next: boolean): unknown {
  const state = next ? "pending" : fixture.previousState;
  return {
    materializationState: "pending",
    job: {
      singleton: 1,
      jobId: "lifetime-v5",
      state,
      cutoffEventRowid: next ? fixture.observedCutoff : fixture.previousCutoff,
      phase: next ? "receipts" : fixture.previousPhase,
      afterScanRowid: next ? 0 : fixture.previousAfterScanRowid,
      scanExhausted: next ? false : fixture.previousScanExhausted,
      failureKind: state === "failed" ? "contractValidation" : null,
      createdAt: "2026-08-28T12:00:00Z",
      updatedAt: state === "failed" ? "2026-08-28T12:02:00Z" : "2026-08-28T12:01:00Z",
      completedAt: state === "failed" ? "2026-08-28T12:02:00Z" : null,
    },
  };
}

function v5Snapshot(
  state: "pending" | "attempting" | "failed",
  phase: string,
  afterScanRowid: number,
  scanExhausted: boolean,
  cutoffEventRowid = 20,
): unknown {
  return {
    materializationState: "pending",
    job: {
      singleton: 1,
      jobId: "lifetime-v5",
      state,
      cutoffEventRowid,
      phase,
      afterScanRowid,
      scanExhausted,
      failureKind: state === "failed" ? "contractValidation" : null,
      createdAt: "2026-08-28T12:00:00Z",
      updatedAt: state === "failed" ? "2026-08-28T12:02:00Z" : "2026-08-28T12:01:00Z",
      completedAt: state === "failed" ? "2026-08-28T12:02:00Z" : null,
    },
  };
}

function insertV3LifetimeJob(
  database: DatabaseSync,
  state: "pending" | "failed",
  phase: string,
  afterEventRowid: number,
): void {
  database.prepare(`
    INSERT INTO lifetime_materialization_job (
      singleton, job_id, state, cutoff_event_rowid, phase, after_event_rowid,
      failure_kind, created_at, updated_at, completed_at
    ) VALUES (1, ?, ?, 20, ?, ?, ?, ?, ?, ?)
  `).run(
    `lifetime-${state}-${phase}`,
    state,
    phase,
    afterEventRowid,
    state === "failed" ? "contractValidation" : null,
    "2026-08-28T10:00:00Z",
    state === "failed" ? "2026-08-28T10:02:00Z" : "2026-08-28T10:00:00Z",
    state === "failed" ? "2026-08-28T10:02:00Z" : null,
  );
}

function databaseRowsSnapshot(database: DatabaseSync): string {
  const tables = database.prepare(`
    SELECT name FROM sqlite_schema
    WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
    ORDER BY name
  `).all().map((row) => row.name as string);
  return JSON.stringify(tables.map((table) => ({ table, rows: normalizedRows(database, table) })));
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
