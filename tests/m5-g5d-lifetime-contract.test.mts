import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";

import {
  G5A_CAPABILITIES_V1,
  G5D_AGENT_FIXTURE_CAPABILITIES_V1,
  parseG5DAgentFixtureReadyPayloadV1,
  parsePlannedG5AReadyPayloadV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import { M5_BRIDGE_V5_METHODS } from "../src/m5/contracts/bridge-v5.ts";
import { M5_BRIDGE_V6_METHODS } from "../src/m5/contracts/bridge-v6.ts";
import { METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import {
  G5_CAPABILITIES_V1,
  G5D_PRODUCT_CAPABILITIES_V1,
  parseG5DProductReadyPayloadV1,
  parseG5ReadyPayloadV1,
} from "../src/m5/contracts/run-recovery-v1.ts";
import { canonicalJsonV1 } from "../src/m5/contracts/storage-v1.ts";
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
  STORAGE_LIFETIME_SCHEMA_VERSION_V6,
  STORAGE_LIFETIME_SCHEMA_VERSION_V7,
  STORAGE_LIFETIME_SCHEMA_VERSION_V8,
  STORAGE_COMPACTION_PHASES_V1,
  STORAGE_COMPACTION_PHASES_V2,
  STORAGE_LIFETIME_PHASES_V3,
  STORAGE_LIFETIME_PHASES_V4,
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
  validateStorageLifetimeReopenPairV3,
  validateStorageLifetimeV5PreflightPolicyV1,
  validateStorageLifetimeBatchPolicyV4,
  validateStorageLifetimeCursorTransitionV4,
  validateStorageLifetimeCursorV4,
  validateStorageLifetimeLifecycleTransitionV4,
  validateStorageLifetimeMaterializationPairV4,
  validateStorageLifetimeReopenPairV4,
  validateStorageLifetimeV6PreflightPolicyV1,
  validateStorageLifetimeV7PreflightPolicyV1,
  validateStorageLifetimeV8PreflightPolicyV1,
  validateStorageLosslessCompactionPolicyV1,
  canonicalFrozenOperationRecordV1,
  validateFrozenOperationSourceV1,
  validateFrozenOperationSourceBatchV1,
  validateFrozenOperationSourceBindingV1,
  validateFrozenOperationReceiptBindingV1,
  validateStorageFrozenOperationSourcePolicyV1,
  validateStorageRuntimeSourceAuthorityPolicyV1,
  validateStorageRuntimeSourceAuthorityV1,
  validateStorageCompactionPolicyV2,
  validateStorageCompactionCursorTransitionV2 as validateStorageCompactionCursorTransitionV2Contract,
  validateStorageCompactionLifecycleTransitionV2,
  validateStorageCompactionCursorV1,
  validateStorageCompactionCursorTransitionV1,
  validateStorageCompactionPrefixV1,
  validateFrozenMessageSourceV1,
  validateStorageCompactionDeleteAuthorityBatchV1,
  validateStorageCompactionLifecycleTransitionV1,
  validateStorageCompactionEquivalenceV1,
  validateStorageEventSourceIndexEntryV1,
  validateStorageEventSourceIndexBindingV1,
  validateStorageEventSourceIndexTransitionV1,
  validateStorageLifetimeWorkspaceProjectSourceV1,
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

interface StorageStatusD1Manifest {
  schemaVersion: number;
  protocolVersion: number;
  activation: "D1";
  method: string;
  modes: {
    generalProduct: { legacyCapabilities: string[]; d1Capabilities: string[] };
    agentFixture: { legacyCapabilities: string[]; d1Capabilities: string[] };
  };
  compatibility: {
    legacyExactSetsRemainValid: boolean;
    d1AddsStorageStatusAtTail: boolean;
    runStartAgentSmokeAdvertised: boolean;
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
  reopenPairs: { valid: NamedValue[]; failClosed: NamedValue[] };
  validLifecycleOperations: string[];
  invalidPreflightMutations: PolicyMutation[];
  invalidPolicyMutations: PolicyMutation[];
}

interface ManifestV6 {
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
    preflight: unknown;
    idempotency: string;
    partialExtraOrUnsafePair: string;
    historyBackfill: string;
    tablesAdded: string[];
    tablesRebuilt: string[];
    indexesAdded: string[];
    triggersAdded: string[];
    preservedTables: string[];
    preservedIndexes: string[];
    preservedTriggers: string[];
    downgradePolicy: string;
  };
  indexedBoundedVerification: Record<string, unknown> & { casesFile: string };
}

interface V6Cases {
  migrationMatrix: {
    jobStates: Array<"pending" | "attempting" | "failed">;
    v5Phases: string[];
    presentJobMapping: string;
    stateIdentityTimestampsFailureEvidence: string;
    absentMappings: Array<{ from: string; to: string }>;
    historyBackfillRows: number;
  };
  phaseOrder: string[];
  sourceIndex: {
    columns: string[];
    sourceEvents: Array<{
      name: string;
      eventRowid: number;
      event: {
        event_id: string;
        operation_id: string;
        request_id: string | null;
        aggregate_type: string;
        aggregate_id: string;
        aggregate_sequence: number;
        event_type: string;
        payload_schema_version: 1;
        canonical_json_payload: string;
        timestamp: string;
      };
    }>;
    validRows: Array<{
      name: string;
      value: [
        number, string, string, string, string, number,
        string | null, number | null, string | null,
      ];
    }>;
    invalidBindingFields: string[];
    duplicateConflicts: string[];
    mutationGuards: string[];
    validTransitions: Array<{
      name: string;
      currentEventRowid: number;
      predecessorEventRowid: number | null;
      lookup: string;
    }>;
    invalidTransitions: string[];
  };
  queryPlans: Array<{
    name: "indexSource" | "verifySourceIndex" | "latestAggregate"
      | "introducedMessageIdentity" | "indexedPredecessor" | "payloadByRowid";
    sql: string;
    detail: string;
  }>;
  workspaceProvenance: {
    projectId: string;
    canonicalPayload: string;
    payloadBytes: number;
    maxPayloadBytes: number;
    comparison: string;
    removalsLegal: boolean;
    stopWhen: string;
  };
  reopenPairs: { valid: NamedValue[]; failClosed: NamedValue[] };
  validLifecycleOperations: string[];
  invalidPreflightMutations: PolicyMutation[];
  invalidPolicyMutations: PolicyMutation[];
}

interface ManifestV7 {
  schemaVersion: number;
  storageSchemaVersion: number;
  protocolVersion: number;
  payloadSchemaVersion: number;
  migration: {
    file: string;
    fromVersion: number;
    toVersion: number;
    mode: string;
    preflight: unknown;
    historyBackfill: string;
    tablesAdded: string[];
    indexesAdded: string[];
    triggersAdded: string[];
    triggersVersionReplaced: string[];
    runtimeGuardDdlAllowed: boolean;
    existingRowsDeleted: boolean;
    preservedTables: string[];
    preservedArtifacts: string[];
  };
  losslessCompaction: Record<string, unknown> & {
    casesFile: string;
    queryPlanAcceptance: Record<string, string | string[]>;
  };
}

interface V7Cases {
  migrationMatrix: {
    admissiblePairs: string[];
    rejectedPairs: string[];
    allowedMaintenanceStates: string[];
    rejectedMaintenanceStates: string[];
    newTableInitialCounts: Record<string, number>;
  };
  runningJobGuard: {
    index: string;
    predicate: string;
    maximumRows: number;
    insertSecondRunning: string;
    updateSecondPendingToRunning: string;
    c1Consumer: string;
  };
  validPrefixes: NamedValue[];
  invalidPrefixMutations: PolicyMutation[];
  validFrozenMessageSources: NamedValue[];
  invalidFrozenMessageMutations: PolicyMutation[];
  validAuthorityBatch: unknown[];
  invalidAuthorityMutations: PolicyMutation[];
  validCursorTransitions: CursorTransitionCase[];
  invalidCursors: NamedValue[];
  invalidCursorTransitions: CursorTransitionCase[];
  validLifecycleTransitions: Array<{
    name: string;
    operation: "arm" | "batchSuccess" | "cancelBeforeArm" | "cancelAfterArm" | "complete";
    previous: unknown;
    next: unknown;
  }>;
  invalidLifecycleMutations: Array<{
    name: string;
    caseIndex: number;
    path: string[];
    replacement: unknown;
  }>;
  equivalenceCases: { valid: unknown; invalidFields: string[]; invalidRowsInBatch: number[] };
  replayNamespaces: Array<{ namespace: string; duplicate: string; conflict: string }>;
  messageReuse: Record<string, string>;
  queryPlans: Array<{ name: string; sql: string; detail: string }>;
  invalidPreflightMutations: PolicyMutation[];
  invalidPolicyMutations: PolicyMutation[];
}

interface ManifestV8 {
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
    preflight: unknown;
    idempotency: string;
    historyBackfill: string;
    tablesAdded: string[];
    tablesRebuilt: string[];
    indexesAdded: string[];
    triggersAdded: string[];
    triggersVersionReplaced: string[];
    runtimeGuardDdlAllowed: boolean;
    existingRowsDeleted: boolean;
    preservedTables: string[];
  };
  frozenOperationSources: Record<string, unknown> & {
    casesFile: string;
    queryPlanAcceptance: Record<string, string | string[]>;
  };
  runtimeSourceAuthority: Record<string, unknown>;
  compactionV2: Record<string, unknown>;
}

interface V8Cases {
  migrationMatrix: {
    admissiblePairs: string[];
    rejectedPairs: string[];
    allowedMaintenanceStates: string[];
    rejectedMaintenanceStates: string[];
    requiredEmptyV7Tables: string[];
    newTableInitialCounts: Record<string, number>;
    historyBackfillRows: number;
  };
  validFrozenOperationSources: NamedValue[];
  invalidFrozenOperationMutations: PolicyMutation[];
  bindingTamperCases: string[];
  runtimeAuthorityCases: { valid: NamedValue[]; invalid: NamedValue[] };
  validCursorTransitionsV2: Array<CursorTransitionCase & { evidence: unknown | null }>;
  invalidCursorTransitionsV2: Array<CursorTransitionCase & { evidence: unknown | null }>;
  validLifecycleTransitionsV2: Array<{
    name: string;
    operation: "arm" | "batchSuccess" | "cancelBeforeArm" | "cancelAfterArm"
      | "commitUncertain" | "complete";
    previous: unknown;
    next: unknown;
    evidence: unknown | null;
  }>;
  invalidLifecycleMutationsV2: Array<{
    name: string;
    caseIndex: number;
    path: string[];
    replacement: unknown;
  }>;
  invalidLifecycleTransitionsV2: Array<{
    name: string;
    operation: "complete";
    previous: unknown;
    next: unknown;
    evidence: unknown | null;
  }>;
  privateEquivalenceCasesV2: {
    valid: Record<string, unknown>;
    invalidFields: string[];
    invalidRowsInBatch: number[];
  };
  namespaceReplay: Array<{ namespace: string; duplicate: string; conflict: string }>;
  preservedV7MessageReuse: {
    deletedIntroducingEvent: string;
    newIntroduction: string;
  };
  queryPlans: Array<{ name: string; sql: string; detail: string }>;
  invalidPreflightMutations: PolicyMutation[];
  invalidPolicyMutations: Array<PolicyMutation & {
    target: "frozenOperationSources" | "runtimeSourceAuthority" | "compactionV2";
  }>;
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
const rootV6 = resolve("tests/fixtures/m5/storage-v6");
const rootV7 = resolve("tests/fixtures/m5/storage-v7");
const rootV8 = resolve("tests/fixtures/m5/storage-v8");
const storageStatusD1Manifest = readJson<StorageStatusD1Manifest>(
  resolve("tests/fixtures/m5/storage-status-d1/manifest.json"),
);
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
const manifestV6 = readJson<ManifestV6>(resolve(rootV6, "manifest.json"));
const v6Cases = readJson<V6Cases>(
  resolve(rootV6, manifestV6.indexedBoundedVerification.casesFile),
);
const manifestV7 = readJson<ManifestV7>(resolve(rootV7, "manifest.json"));
const v7Cases = readJson<V7Cases>(
  resolve(rootV7, manifestV7.losslessCompaction.casesFile),
);
const manifestV8 = readJson<ManifestV8>(resolve(rootV8, "manifest.json"));
const v8Cases = readJson<V8Cases>(
  resolve(rootV8, manifestV8.frozenOperationSources.casesFile),
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

  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-g5d-contract-"));
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

  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-g5d-v3-contract-"));
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

  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-g5d-v4-contract-"));
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

  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-g5d-v5-contract-"));
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
        const migratedPair = readV5Pair(database);
        if (state === "pending") {
          assert.deepEqual(validateStorageLifetimeReopenPairV3(migratedPair), migratedPair);
        } else {
          assert.throws(
            () => validateStorageLifetimeReopenPairV3(migratedPair),
            undefined,
            `${state}:${phase} must remain fail-closed after migration`,
          );
        }
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
  for (const fixture of v5Cases.reopenPairs.valid) {
    assert.deepEqual(validateStorageLifetimeReopenPairV3(fixture.value), fixture.value, fixture.name);
  }
  for (const fixture of v5Cases.reopenPairs.failClosed) {
    assert.throws(() => validateStorageLifetimeReopenPairV3(fixture.value), undefined, fixture.name);
  }
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

test("storage-v6 migration creates an empty guarded source index and resets every job", () => {
  assert.equal(manifestV6.schemaVersion, 1);
  assert.equal(manifestV6.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V6);
  assert.equal(manifestV6.protocolVersion, PROTOCOL_VERSION);
  assert.equal(manifestV6.payloadSchemaVersion, 1);
  assert.equal(manifestV6.migration.fromVersion, 5);
  assert.equal(manifestV6.migration.toVersion, 6);
  assert.equal(manifestV6.migration.historyBackfill, "none");
  assert.deepEqual(manifestV6.migration.tablesAdded, ["event_source_index"]);
  assert.deepEqual(manifestV6.migration.indexesAdded, [
    "event_source_index_aggregate_latest", "event_source_index_message_unique",
  ]);
  assert.deepEqual(manifestV6.migration.triggersAdded, [
    "event_source_index_requires_exact_event",
    "event_source_index_append_only_update", "event_source_index_append_only_delete",
  ]);
  assert.deepEqual(
    validateStorageLifetimeV6PreflightPolicyV1(manifestV6.migration.preflight),
    manifestV6.migration.preflight,
  );
  assert.deepEqual(
    validateStorageLifetimeBatchPolicyV4(manifestV6.indexedBoundedVerification),
    manifestV6.indexedBoundedVerification,
  );
  assert.deepEqual(v6Cases.phaseOrder, [...STORAGE_LIFETIME_PHASES_V4]);

  const migrationV6 = readFileSync(resolve(rootV6, manifestV6.migration.file), "utf8");
  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-storage-v6-"));
  try {
    const reference = createExactStorageV5Database(join(temporaryRoot, "reference.db"), false);
    const exactV5 = schemaSignature(reference);
    applyExactStorageV6Migration(reference, migrationV6, exactV5, null);
    const exactV6 = schemaSignature(reference);
    assert.equal(reference.prepare("SELECT COUNT(*) AS count FROM event_source_index").get()?.count, 0);
    assert.deepEqual(readV6Pair(reference), { materializationState: "pending", job: null });
    reference.close();

    const preservation = createExactStorageV5Database(join(temporaryRoot, "preservation.db"), true);
    const beforeRows = new Map(
      manifestV6.migration.preservedTables.map((table) => [table, normalizedRows(preservation, table)]),
    );
    const beforeTriggers = schemaObjects(preservation, "trigger");
    applyExactStorageV6Migration(preservation, migrationV6, exactV5, exactV6);
    for (const table of manifestV6.migration.preservedTables) {
      assert.deepEqual(normalizedRows(preservation, table), beforeRows.get(table), table);
    }
    assert.deepEqual(
      schemaObjects(preservation, "trigger").filter((row) => String(row.name).startsWith("events_")),
      beforeTriggers.filter((row) => String(row.name).startsWith("events_")),
    );
    assert.equal(preservation.prepare("SELECT COUNT(*) AS count FROM event_source_index").get()?.count, 0);
    assert.deepEqual(readV6Pair(preservation), { materializationState: "pending", job: null });
    applyExactStorageV6Migration(preservation, migrationV6, exactV5, exactV6);
    assert.equal(schemaSignature(preservation), exactV6);
    preservation.close();

    const history = createExactStorageV5Database(join(temporaryRoot, "history.db"), false);
    const insertHistoricalEvent = history.prepare(`
      INSERT INTO events(
        event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
        event_type,payload_schema_version,canonical_json_payload,timestamp
      ) VALUES(?,?,?,?,?,?,?,?,?,?)
    `);
    for (const fixture of v6Cases.sourceIndex.sourceEvents) {
      const event = fixture.event;
      assert.equal(Number(insertHistoricalEvent.run(
        event.event_id, event.operation_id, event.request_id, event.aggregate_type,
        event.aggregate_id, event.aggregate_sequence, event.event_type,
        event.payload_schema_version, event.canonical_json_payload, event.timestamp,
      ).lastInsertRowid), fixture.eventRowid, fixture.name);
    }
    applyExactStorageV6Migration(history, migrationV6, exactV5, exactV6);
    assert.equal(history.prepare("SELECT COUNT(*) AS count FROM event_source_index").get()?.count, 0);
    const indexHistoricalSource = history.prepare(`
      INSERT INTO event_source_index(
        event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,
        introduced_message_id,chat_durable_sequence,chat_snapshot_message_id
      ) VALUES(?,?,?,?,?,?,?,?,?)
    `);
    for (const fixture of v6Cases.sourceIndex.validRows) indexHistoricalSource.run(...fixture.value);
    const historicalChatIndex = history.prepare(`
      SELECT aggregate_sequence,introduced_message_id,chat_durable_sequence,
             chat_snapshot_message_id
      FROM event_source_index WHERE aggregate_type='chat' ORDER BY aggregate_sequence
    `).all().map((row) => ({
      aggregate_sequence: row.aggregate_sequence,
      introduced_message_id: row.introduced_message_id,
      chat_durable_sequence: row.chat_durable_sequence,
      chat_snapshot_message_id: row.chat_snapshot_message_id,
    }));
    assert.deepEqual(historicalChatIndex, [
      { aggregate_sequence: 1, introduced_message_id: "message_a", chat_durable_sequence: 1, chat_snapshot_message_id: "message_a" },
      { aggregate_sequence: 2, introduced_message_id: null, chat_durable_sequence: 1, chat_snapshot_message_id: "message_a" },
      { aggregate_sequence: 3, introduced_message_id: null, chat_durable_sequence: 1, chat_snapshot_message_id: null },
      { aggregate_sequence: 4, introduced_message_id: "message_b", chat_durable_sequence: 2, chat_snapshot_message_id: "message_b" },
    ]);
    history.close();

    for (const state of v6Cases.migrationMatrix.jobStates) {
      for (const phase of v6Cases.migrationMatrix.v5Phases) {
        const database = createExactStorageV5Database(
          join(temporaryRoot, `${state}-${phase}.db`),
          false,
        );
        database.prepare(`
          UPDATE storage_schema_meta SET materialization_state='pending' WHERE singleton=1
        `).run();
        insertV5LifetimeJob(database, state, phase, 7, true);
        const before = readV5Pair(database) as { job: Record<string, unknown> };
        applyExactStorageV6Migration(database, migrationV6, exactV5, exactV6);
        const after = readV6Pair(database) as { materializationState: string; job: Record<string, unknown> };
        assert.equal(after.materializationState, "pending", `${state}:${phase}`);
        assert.equal(after.job.state, state, `${state}:${phase}`);
        assert.equal(after.job.phase, "indexSource", `${state}:${phase}`);
        assert.equal(after.job.afterScanRowid, 0, `${state}:${phase}`);
        assert.equal(after.job.scanExhausted, false, `${state}:${phase}`);
        for (const field of [
          "jobId", "cutoffEventRowid", "failureKind", "createdAt", "updatedAt", "completedAt",
        ]) assert.equal(after.job[field], before.job[field], `${state}:${phase}:${field}`);
        const validate = state === "pending"
          ? () => validateStorageLifetimeReopenPairV4(after)
          : () => assert.throws(() => validateStorageLifetimeReopenPairV4(after));
        validate();
        database.close();
      }
    }

    for (const materializationState of ["pending", "ready"] as const) {
      const database = createExactStorageV5Database(
        join(temporaryRoot, `absent-${materializationState}.db`),
        false,
      );
      database.prepare(`
        UPDATE storage_schema_meta SET materialization_state=? WHERE singleton=1
      `).run(materializationState);
      applyExactStorageV6Migration(database, migrationV6, exactV5, exactV6);
      assert.deepEqual(readV6Pair(database), { materializationState: "pending", job: null });
      database.close();
    }

    const unsafe = createExactStorageV5Database(join(temporaryRoot, "unsafe.db"), false);
    unsafe.prepare("UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1").run();
    insertV5LifetimeJob(unsafe, "pending", "verifyProjects", 1, false);
    const unsafeBefore = `${schemaSignature(unsafe)}\n${databaseRowsSnapshot(unsafe)}`;
    assert.throws(() => applyExactStorageV6Migration(unsafe, migrationV6, exactV5, exactV6));
    assert.equal(`${schemaSignature(unsafe)}\n${databaseRowsSnapshot(unsafe)}`, unsafeBefore);
    unsafe.close();

    const extraSchema = createExactStorageV5Database(join(temporaryRoot, "extra-schema.db"), false);
    extraSchema.exec("CREATE TABLE unexpected_v6_preflight_object(value TEXT) STRICT");
    const extraBefore = `${schemaSignature(extraSchema)}\n${databaseRowsSnapshot(extraSchema)}`;
    assert.throws(() => applyExactStorageV6Migration(
      extraSchema, migrationV6, exactV5, exactV6,
    ));
    assert.equal(`${schemaSignature(extraSchema)}\n${databaseRowsSnapshot(extraSchema)}`, extraBefore);
    extraSchema.close();
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v6 source index enforces exact identity, append-only guards, and atomic rollback", () => {
  const migrationV6 = readFileSync(resolve(rootV6, manifestV6.migration.file), "utf8");
  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-storage-v6-index-"));
  try {
    const database = createExactStorageV5Database(join(temporaryRoot, "index.db"), false);
    const exactV5 = schemaSignature(database);
    applyExactStorageV6Migration(database, migrationV6, exactV5, null);
    const insert = database.prepare(`
      INSERT INTO event_source_index(
        event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,
        introduced_message_id,chat_durable_sequence,chat_snapshot_message_id
      ) VALUES(?,?,?,?,?,?,?,?,?)
    `);
    assert.equal(database.prepare("PRAGMA foreign_keys").get()?.foreign_keys, 1);
    const foreignKeys = database.prepare("PRAGMA foreign_key_list(event_source_index)").all()
      .map((row) => `${row.from}->${row.table}.${row.to}`)
      .sort();
    assert.deepEqual(foreignKeys, [
      "aggregate_id->events.aggregate_id",
      "aggregate_sequence->events.aggregate_sequence",
      "aggregate_type->events.aggregate_type",
      "event_id->events.event_id",
      "operation_id->events.operation_id",
    ]);
    const insertEvent = database.prepare(`
      INSERT INTO events(
        event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
        event_type,payload_schema_version,canonical_json_payload,timestamp
      ) VALUES(?,?,?,?,?,?,?,?,?,?)
    `);
    for (const fixture of v6Cases.sourceIndex.sourceEvents) {
      const event = fixture.event;
      const result = insertEvent.run(
        event.event_id, event.operation_id, event.request_id, event.aggregate_type,
        event.aggregate_id, event.aggregate_sequence, event.event_type,
        event.payload_schema_version, event.canonical_json_payload, event.timestamp,
      );
      assert.equal(Number(result.lastInsertRowid), fixture.eventRowid, fixture.name);
    }
    const entryFromRow = (row: V6Cases["sourceIndex"]["validRows"][number]["value"]) => ({
      eventRowid: row[0],
      eventId: row[1],
      operationId: row[2],
      aggregateType: row[3],
      aggregateId: row[4],
      aggregateSequence: row[5],
      introducedMessageId: row[6],
      chatDurableSequence: row[7],
      chatSnapshotMessageId: row[8],
    });
    const projectEntry = entryFromRow(v6Cases.sourceIndex.validRows[0]!.value);
    const projectSource = v6Cases.sourceIndex.sourceEvents[0]!;
    const chatSource = v6Cases.sourceIndex.sourceEvents[1]!;
    const projectRevision = v6Cases.sourceIndex.sourceEvents[2]!;
    const invalidBindings = [
      { ...projectEntry, eventRowid: chatSource.eventRowid },
      { ...projectEntry, eventId: chatSource.event.event_id },
      { ...projectEntry, operationId: chatSource.event.operation_id },
      { ...projectEntry, aggregateType: chatSource.event.aggregate_type },
      { ...projectEntry, aggregateId: chatSource.event.aggregate_id },
      { ...projectEntry, aggregateSequence: projectRevision.event.aggregate_sequence },
      { ...projectEntry, introducedMessageId: "message_a" },
      { ...projectEntry, chatDurableSequence: 1 },
      { ...projectEntry, chatSnapshotMessageId: "message_a" },
      {
        ...projectEntry,
        eventId: chatSource.event.event_id,
        operationId: projectRevision.event.operation_id,
      },
    ];
    assert.equal(invalidBindings.length, v6Cases.sourceIndex.invalidBindingFields.length);
    assert.throws(() => insert.run(
      99, "evt_missing", "op_missing", "run", "run_missing", 1, null, null, null,
    ));
    for (const [index, invalid] of invalidBindings.entries()) {
      assert.throws(
        () => validateStorageEventSourceIndexBindingV1(
          projectSource.eventRowid, invalid, projectSource.event,
        ),
        undefined,
        v6Cases.sourceIndex.invalidBindingFields[index],
      );
      assert.throws(
        () => insert.run(
          invalid.eventRowid, invalid.eventId, invalid.operationId, invalid.aggregateType,
          invalid.aggregateId, invalid.aggregateSequence, invalid.introducedMessageId,
          invalid.chatDurableSequence, invalid.chatSnapshotMessageId,
        ),
        undefined,
        v6Cases.sourceIndex.invalidBindingFields[index],
      );
    }
    const insertedEntries = new Map<number, ReturnType<typeof entryFromRow>>();
    for (const fixture of v6Cases.sourceIndex.validRows) {
      const entry = entryFromRow(fixture.value);
      const source = v6Cases.sourceIndex.sourceEvents.find(
        (candidate) => candidate.eventRowid === entry.eventRowid,
      );
      assert.notEqual(source, undefined, fixture.name);
      validateStorageEventSourceIndexEntryV1(entry);
      validateStorageEventSourceIndexBindingV1(entry.eventRowid, entry, source!.event);
      const predecessor = source!.event.aggregate_type === "chat"
        && source!.event.aggregate_sequence > 1
        ? [...insertedEntries.values()].find((candidate) => (
          candidate.aggregateType === "chat"
          && candidate.aggregateId === entry.aggregateId
          && candidate.aggregateSequence + 1 === entry.aggregateSequence
        )) ?? null
        : null;
      const predecessorSource = predecessor === null ? null : v6Cases.sourceIndex.sourceEvents.find(
        (candidate) => candidate.eventRowid === predecessor.eventRowid,
      )?.event ?? null;
      validateStorageEventSourceIndexTransitionV1(
        entry.eventRowid, entry, source!.event, predecessor, predecessorSource,
      );
      insert.run(...fixture.value);
      insertedEntries.set(entry.eventRowid, entry);
    }
    assert.deepEqual(database.prepare("PRAGMA foreign_key_check").all(), []);

    const makeMessage = (
      messageId: string,
      chatId: string,
      eventSequence: number,
    ) => ({
      schemaVersion: 1, messageId, chatId, taskId: `task_${chatId}`,
      worktreeId: `worktree_${chatId}`, eventSequence, kind: "user",
      roleId: null, runId: null, text: messageId, createdAt: "2026-08-28T14:00:00Z",
    });
    let syntheticOrdinal = 0;
    const appendSyntheticChat = (
      chatId: string,
      aggregateSequence: number,
      durable: number,
      lastMessage: ReturnType<typeof makeMessage> | null,
      introducedMessageId: string | null,
    ) => {
      syntheticOrdinal += 1;
      const event = {
        event_id: `evt_synthetic_${syntheticOrdinal}`,
        operation_id: `op_synthetic_${syntheticOrdinal}`,
        request_id: null,
        aggregate_type: "chat",
        aggregate_id: chatId,
        aggregate_sequence: aggregateSequence,
        event_type: "chat.upserted",
        payload_schema_version: 1 as const,
        canonical_json_payload: JSON.stringify({
          durable_event_sequence: durable, last_message: lastMessage,
        }),
        timestamp: "2026-08-28T14:00:00Z",
      };
      const result = insertEvent.run(
        event.event_id, event.operation_id, null, "chat", chatId, aggregateSequence,
        event.event_type, 1, event.canonical_json_payload, event.timestamp,
      );
      const entry = {
        eventRowid: Number(result.lastInsertRowid), eventId: event.event_id,
        operationId: event.operation_id, aggregateType: "chat", aggregateId: chatId,
        aggregateSequence, introducedMessageId, chatDurableSequence: durable,
        chatSnapshotMessageId: lastMessage?.messageId ?? null,
      };
      return { event, entry };
    };
    const insertSyntheticIndex = (entry: ReturnType<typeof appendSyntheticChat>["entry"]) =>
      insert.run(
        entry.eventRowid, entry.eventId, entry.operationId, entry.aggregateType,
        entry.aggregateId, entry.aggregateSequence, entry.introducedMessageId,
        entry.chatDurableSequence, entry.chatSnapshotMessageId,
      );
    const seedSyntheticChat = (chatId: string) => {
      const message = makeMessage(`${chatId}_a`, chatId, 1);
      const first = appendSyntheticChat(chatId, 1, 1, message, message.messageId);
      validateStorageEventSourceIndexTransitionV1(
        first.entry.eventRowid, first.entry, first.event, null, null,
      );
      insertSyntheticIndex(first.entry);
      return first;
    };
    const assertInvalidTransition = (
      name: string,
      previous: ReturnType<typeof appendSyntheticChat>,
      current: ReturnType<typeof appendSyntheticChat>,
    ) => {
      assert.throws(() => validateStorageEventSourceIndexTransitionV1(
        current.entry.eventRowid, current.entry, current.event, previous.entry, previous.event,
      ), undefined, name);
      assert.throws(() => insertSyntheticIndex(current.entry), undefined, name);
    };

    {
      const chatId = "chat_decrease";
      const previous = seedSyntheticChat(chatId);
      const current = appendSyntheticChat(chatId, 2, 0, null, null);
      assertInvalidTransition("durable decrease", previous, current);
    }
    {
      const chatId = "chat_jump";
      const previous = seedSyntheticChat(chatId);
      const message = makeMessage(`${chatId}_c`, chatId, 3);
      const current = appendSyntheticChat(chatId, 2, 3, message, message.messageId);
      assertInvalidTransition("durable jump", previous, current);
    }
    {
      const chatId = "chat_missing_message";
      const previous = seedSyntheticChat(chatId);
      const current = appendSyntheticChat(chatId, 2, 2, null, null);
      assertInvalidTransition("increase without valid message", previous, current);
    }
    {
      const chatId = "chat_changed_snapshot";
      const previous = seedSyntheticChat(chatId);
      const message = makeMessage(`${chatId}_changed`, chatId, 1);
      const current = appendSyntheticChat(chatId, 2, 1, message, null);
      assertInvalidTransition("unchanged durable with changed snapshot", previous, current);
    }
    {
      const chatId = "chat_double_introduction";
      const previous = seedSyntheticChat(chatId);
      const message = makeMessage(`${chatId}_a`, chatId, 1);
      const current = appendSyntheticChat(chatId, 2, 1, message, message.messageId);
      assertInvalidTransition("double introduction", previous, current);
    }
    {
      const chatId = "chat_wrong_sequence";
      const previous = seedSyntheticChat(chatId);
      const message = makeMessage(`${chatId}_b`, chatId, 3);
      const current = appendSyntheticChat(chatId, 2, 2, message, message.messageId);
      assertInvalidTransition("wrong TeamMessage eventSequence", previous, current);
    }
    {
      const chatId = "chat_wrong_identity";
      const previous = seedSyntheticChat(chatId);
      const message = makeMessage(`${chatId}_b`, "chat_other", 2);
      const current = appendSyntheticChat(chatId, 2, 2, message, message.messageId);
      assertInvalidTransition("wrong TeamMessage chatId", previous, current);
    }
    {
      const chatId = "chat_repeat_wrong_identity";
      const previous = seedSyntheticChat(chatId);
      const repeated = makeMessage(`${chatId}_a`, "chat_other", 1);
      const current = appendSyntheticChat(chatId, 2, 1, repeated, null);
      assertInvalidTransition("indexed repeat keeps ID but changes chatId", previous, current);
    }
    {
      const chatId = "chat_repeat_wrong_sequence";
      const previous = seedSyntheticChat(chatId);
      const repeated = makeMessage(`${chatId}_a`, chatId, 2);
      const current = appendSyntheticChat(chatId, 2, 1, repeated, null);
      assertInvalidTransition("indexed repeat keeps ID but changes eventSequence", previous, current);
    }
    {
      const chatId = "chat_migration_pending";
      const firstMessage = makeMessage(`${chatId}_a`, chatId, 1);
      const first = appendSyntheticChat(chatId, 1, 1, firstMessage, firstMessage.messageId);
      const secondMessage = makeMessage(`${chatId}_b`, chatId, 2);
      const current = appendSyntheticChat(chatId, 2, 2, secondMessage, secondMessage.messageId);
      assert.throws(
        () => insertSyntheticIndex(current.entry),
        undefined,
        "raw predecessor event does not authorize a sequence-two source index",
      );
      assert.throws(() => validateStorageEventSourceIndexTransitionV1(
        current.entry.eventRowid, current.entry, current.event, null, first.event,
      ));
      insertSyntheticIndex(first.entry);
      validateStorageEventSourceIndexTransitionV1(
        current.entry.eventRowid, current.entry, current.event, first.entry, first.event,
      );
      insertSyntheticIndex(current.entry);
    }
    {
      const chatId = "chat_illegal_first_jump";
      const message = makeMessage(`${chatId}_b`, chatId, 2);
      const current = appendSyntheticChat(chatId, 1, 2, message, message.messageId);
      assert.throws(() => validateStorageEventSourceIndexTransitionV1(
        current.entry.eventRowid, current.entry, current.event, null, null,
      ));
      assert.throws(
        () => insertSyntheticIndex(current.entry),
        undefined,
        "sequence one cannot jump from synthetic zero to durable two",
      );
    }
    assert.equal(v6Cases.sourceIndex.validTransitions.length, 4);
    assert.equal(v6Cases.sourceIndex.invalidTransitions.length, 16);
    for (const invalid of [
      { eventRowid: 0, eventId: "evt", operationId: "op", aggregateType: "project", aggregateId: "p", aggregateSequence: 1, introducedMessageId: null, chatDurableSequence: null, chatSnapshotMessageId: null },
      { eventRowid: 1, eventId: "", operationId: "op", aggregateType: "project", aggregateId: "p", aggregateSequence: 1, introducedMessageId: null, chatDurableSequence: null, chatSnapshotMessageId: null },
      { eventRowid: 1, eventId: "evt", operationId: "op", aggregateType: "chat", aggregateId: "c", aggregateSequence: 1, introducedMessageId: "", chatDurableSequence: 1, chatSnapshotMessageId: "m" },
    ]) assert.throws(() => validateStorageEventSourceIndexEntryV1(invalid));
    assert.throws(() => database.prepare(`
      UPDATE event_source_index SET aggregate_id='changed' WHERE event_rowid=1
    `).run());
    assert.throws(() => database.prepare("DELETE FROM event_source_index WHERE event_rowid=1").run());
    for (const conflict of [
      [1, "evt_unique", "op_unique", "run", "run_1", 1, null, null, null],
      [3, "evt_project_1", "op_project_2", "project", "project_1", 2, null, null, null],
      [3, "evt_project_2", "op_project_1", "project", "project_1", 2, null, null, null],
      [3, "evt_project_2", "op_project_2", "project", "project_1", 1, null, null, null],
      [3, "evt_project_2", "op_project_2", "project", "project_1", 2, "message_a", null, null],
    ] as const) assert.throws(() => insert.run(...conflict));

    const indexCountBeforeAtomic = database.prepare(
      "SELECT COUNT(*) AS count FROM event_source_index",
    ).get()?.count;
    database.exec("BEGIN IMMEDIATE");
    try {
      database.prepare(`
        INSERT INTO events(
          event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
          event_type,payload_schema_version,canonical_json_payload,timestamp
        ) VALUES('evt_atomic','op_atomic',NULL,'chat','chat_atomic',1,'chat.upserted',1,
          '{"durable_event_sequence":1,"last_message":{"schemaVersion":1,"messageId":"message_a","chatId":"chat_atomic","taskId":"task_atomic","worktreeId":"worktree_atomic","eventSequence":1,"kind":"user","roleId":null,"runId":null,"text":"atomic","createdAt":"2026-08-28T13:00:00Z"}}','2026-08-28T13:00:00Z')
      `).run();
      const eventRowid = database.prepare(`
        SELECT rowid FROM events WHERE event_id='evt_atomic'
      `).get()?.rowid as number;
      insert.run(
        eventRowid, "evt_atomic", "op_atomic", "chat", "chat_atomic", 1,
        "message_a", 1, "message_a",
      );
      assert.fail("duplicate introduced_message_id must reject the append transaction");
    } catch {
      database.exec("ROLLBACK");
    }
    assert.equal(database.prepare("SELECT COUNT(*) AS count FROM events WHERE event_id='evt_atomic'").get()?.count, 0);
    assert.equal(
      database.prepare("SELECT COUNT(*) AS count FROM event_source_index").get()?.count,
      indexCountBeforeAtomic,
    );
    database.close();
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v6 freezes 16 bounded phases, indexed provenance, and exact query plans", () => {
  for (const fixture of v6Cases.reopenPairs.valid) {
    validateStorageLifetimeReopenPairV4(fixture.value);
  }
  for (const fixture of v6Cases.reopenPairs.failClosed) {
    assert.throws(() => validateStorageLifetimeReopenPairV4(fixture.value), undefined, fixture.name);
  }
  for (const [index, phase] of STORAGE_LIFETIME_PHASES_V4.entries()) {
    const current = {
      schemaVersion: 4,
      cutoffEventRowid: 20,
      phase,
      afterScanRowid: index < 7 ? 20 : 900,
      scanExhausted: true,
    };
    validateStorageLifetimeCursorV4(current);
    const nextPhase = STORAGE_LIFETIME_PHASES_V4[index + 1];
    if (nextPhase !== undefined) {
      validateStorageLifetimeCursorTransitionV4(current, {
        ...current, phase: nextPhase, afterScanRowid: 0, scanExhausted: false,
      });
    }
  }
  const pendingAbsent = { materializationState: "pending", job: null };
  const readyAbsent = { materializationState: "ready", job: null };
  const created = v6Snapshot("pending", "indexSource", 0, false);
  validateStorageLifetimeLifecycleTransitionV4("create", pendingAbsent, created);
  const armed = v6Snapshot("attempting", "indexSource", 0, false);
  validateStorageLifetimeLifecycleTransitionV4("arm", created, armed);
  validateStorageLifetimeLifecycleTransitionV4(
    "batchSuccess", armed, v6Snapshot("pending", "indexSource", 1, false),
  );
  validateStorageLifetimeLifecycleTransitionV4(
    "phaseAdvance",
    v6Snapshot("pending", "indexSource", 20, true),
    v6Snapshot("pending", "verifySourceIndex", 0, false),
  );
  validateStorageLifetimeLifecycleTransitionV4(
    "catchUpReset",
    v6Snapshot("pending", "verifySourceIndex", 20, true),
    v6Snapshot("pending", "indexSource", 0, false, 21),
  );
  validateStorageLifetimeLifecycleTransitionV4(
    "finalPublish", v6Snapshot("pending", "verifyBaselines", 900, true), readyAbsent,
  );
  validateStorageLifetimeLifecycleTransitionV4("append", readyAbsent, pendingAbsent);
  assert.throws(() => validateStorageLifetimeLifecycleTransitionV4(
    "catchUpReset",
    v6Snapshot("attempting", "verifySourceIndex", 20, false),
    v6Snapshot("pending", "indexSource", 0, false, 21),
  ));

  for (const fixture of v6Cases.invalidPreflightMutations) {
    const invalid = structuredClone(manifestV6.migration.preflight) as Record<string, unknown>;
    setPath(invalid, fixture.path, fixture.replacement);
    assert.throws(() => validateStorageLifetimeV6PreflightPolicyV1(invalid), undefined, fixture.name);
  }
  for (const fixture of v6Cases.invalidPolicyMutations) {
    const invalid = structuredClone(manifestV6.indexedBoundedVerification);
    setPath(invalid, fixture.path, fixture.replacement);
    assert.throws(() => validateStorageLifetimeBatchPolicyV4(invalid), undefined, fixture.name);
  }

  assert.equal(
    Buffer.byteLength(v6Cases.workspaceProvenance.canonicalPayload),
    v6Cases.workspaceProvenance.payloadBytes,
  );
  assert.ok(v6Cases.workspaceProvenance.payloadBytes <= v6Cases.workspaceProvenance.maxPayloadBytes);
  const projectPayload = JSON.parse(v6Cases.workspaceProvenance.canonicalPayload) as Record<string, unknown>;
  assert.equal(projectPayload.project_id, v6Cases.workspaceProvenance.projectId);
  assert.deepEqual(
    validateStorageLifetimeWorkspaceProjectSourceV1(
      v6Cases.workspaceProvenance.projectId,
      "project",
      v6Cases.workspaceProvenance.projectId,
      v6Cases.workspaceProvenance.canonicalPayload,
    ),
    projectPayload,
  );
  assert.throws(() => validateStorageLifetimeWorkspaceProjectSourceV1(
    v6Cases.workspaceProvenance.projectId,
    "chat",
    v6Cases.workspaceProvenance.projectId,
    v6Cases.workspaceProvenance.canonicalPayload,
  ));
  assert.throws(() => validateStorageLifetimeWorkspaceProjectSourceV1(
    v6Cases.workspaceProvenance.projectId,
    "project",
    v6Cases.workspaceProvenance.projectId,
    `{"project_id":"${v6Cases.workspaceProvenance.projectId}"}`,
  ));
  assert.throws(() => validateStorageLifetimeWorkspaceProjectSourceV1(
    v6Cases.workspaceProvenance.projectId,
    "project",
    v6Cases.workspaceProvenance.projectId,
    `{"canonical_path":"/${"x".repeat(1_048_576)}","project_id":"${v6Cases.workspaceProvenance.projectId}"}`,
  ));
  assert.equal(v6Cases.workspaceProvenance.removalsLegal, false);
  assert.equal(
    v6Cases.workspaceProvenance.stopWhen,
    "aggregateRemovalOrTombstoneBecomesLegalUntilSourceIndexSemanticsAreVersioned",
  );

  const migrationV6 = readFileSync(resolve(rootV6, manifestV6.migration.file), "utf8");
  const queryPlanAcceptance = manifestV6.indexedBoundedVerification
    .queryPlanAcceptance as Record<string, unknown>;
  const declaredQueryPlanKeys = Object.keys(queryPlanAcceptance)
    .filter((key) => key !== "mode" && key !== "reject")
    .sort();
  const fixtureQueryPlanKeys = v6Cases.queryPlans.map((plan) => plan.name).sort();
  const assertExactQueryPlanKeys = (declared: string[], actual: string[]) => {
    assert.deepEqual([...declared].sort(), [...actual].sort());
  };
  assertExactQueryPlanKeys(declaredQueryPlanKeys, fixtureQueryPlanKeys);
  assert.throws(() => assertExactQueryPlanKeys(
    [...declaredQueryPlanKeys, "missingIndexedPredecessorEvent"], fixtureQueryPlanKeys,
  ));
  assert.throws(() => assertExactQueryPlanKeys(
    declaredQueryPlanKeys.filter((key) => key !== "payloadByRowid"), fixtureQueryPlanKeys,
  ));
  assert.throws(() => assertExactQueryPlanKeys(
    declaredQueryPlanKeys, [...fixtureQueryPlanKeys, "undeclaredPlan"],
  ));
  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-storage-v6-plan-"));
  try {
    const database = createExactStorageV5Database(join(temporaryRoot, "plan.db"), false);
    applyExactStorageV6Migration(database, migrationV6, schemaSignature(database), null);
    for (const fixture of v6Cases.queryPlans) {
      const plans = database.prepare(fixture.sql).all();
      assert.equal(plans.length, 1, fixture.name);
      assert.equal(plans[0]?.detail, fixture.detail, fixture.name);
    }
    database.close();
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v7 freezes bounded lossless compaction without enabling product deletion", () => {
  assert.equal(manifestV7.schemaVersion, 1);
  assert.equal(manifestV7.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V7);
  assert.equal(manifestV7.protocolVersion, PROTOCOL_VERSION);
  assert.equal(manifestV7.payloadSchemaVersion, 1);
  assert.equal(manifestV7.migration.fromVersion, 6);
  assert.equal(manifestV7.migration.toVersion, 7);
  assert.equal(manifestV7.migration.historyBackfill, "none");
  assert.equal(manifestV7.migration.runtimeGuardDdlAllowed, false);
  assert.equal(manifestV7.migration.existingRowsDeleted, false);
  assert.deepEqual(manifestV7.migration.indexesAdded, [
    "ledger_compaction_prefixes_latest",
    "frozen_message_sources_chat_sequence",
    "maintenance_jobs_single_running_lossless_compaction",
  ]);
  assert.deepEqual(
    validateStorageLifetimeV7PreflightPolicyV1(manifestV7.migration.preflight),
    manifestV7.migration.preflight,
  );
  assert.deepEqual(
    validateStorageLosslessCompactionPolicyV1(manifestV7.losslessCompaction),
    manifestV7.losslessCompaction,
  );
  assert.deepEqual(STORAGE_COMPACTION_PHASES_V1, [
    "freezeMessages", "verifyPrefix", "authorizeDelete", "deletePrefix",
    "verifyEquivalence", "finalize",
  ]);
  assert.deepEqual(v7Cases.runningJobGuard, {
    index: "maintenance_jobs_single_running_lossless_compaction",
    predicate: "job_kind='losslessCompaction' AND state='running'",
    maximumRows: 1,
    insertSecondRunning: "uniqueConstraintRejectedBeforeAuthority",
    updateSecondPendingToRunning: "uniqueConstraintRejectedBeforeAuthority",
    c1Consumer: "exactSchemaGuardRequiredNotApplicationOnly",
  });

  for (const fixture of v7Cases.validPrefixes) {
    validateStorageCompactionPrefixV1(fixture.value);
    for (const mutation of v7Cases.invalidPrefixMutations) {
      const invalid = structuredClone(fixture.value);
      setPath(invalid, mutation.path, mutation.replacement);
      assert.throws(() => validateStorageCompactionPrefixV1(invalid), undefined, mutation.name);
    }
  }
  for (const fixture of v7Cases.validFrozenMessageSources) {
    validateFrozenMessageSourceV1(fixture.value);
    for (const mutation of v7Cases.invalidFrozenMessageMutations) {
      const invalid = structuredClone(fixture.value);
      setPath(invalid, mutation.path, mutation.replacement);
      assert.throws(() => validateFrozenMessageSourceV1(invalid), undefined, mutation.name);
    }
  }
  validateStorageCompactionDeleteAuthorityBatchV1(v7Cases.validAuthorityBatch);
  for (const mutation of v7Cases.invalidAuthorityMutations) {
    const invalid = structuredClone(v7Cases.validAuthorityBatch);
    setPath(invalid, mutation.path, mutation.replacement);
    assert.throws(
      () => validateStorageCompactionDeleteAuthorityBatchV1(invalid),
      undefined,
      mutation.name,
    );
  }
  for (const fixture of v7Cases.validCursorTransitions) {
    validateStorageCompactionCursorTransitionV1(fixture.previous, fixture.next);
  }
  for (const fixture of v7Cases.invalidCursors) {
    assert.throws(() => validateStorageCompactionCursorV1(fixture.value), undefined, fixture.name);
  }
  for (const fixture of v7Cases.invalidCursorTransitions) {
    assert.throws(
      () => validateStorageCompactionCursorTransitionV1(fixture.previous, fixture.next),
      undefined,
      fixture.name,
    );
  }
  for (const fixture of v7Cases.validLifecycleTransitions) {
    validateStorageCompactionLifecycleTransitionV1(
      fixture.operation, fixture.previous, fixture.next,
    );
  }
  for (const mutation of v7Cases.invalidLifecycleMutations) {
    const invalid = structuredClone(v7Cases.validLifecycleTransitions[mutation.caseIndex]);
    assert.ok(invalid);
    setPath(invalid, mutation.path, mutation.replacement);
    assert.throws(() => validateStorageCompactionLifecycleTransitionV1(
      invalid.operation, invalid.previous, invalid.next,
    ), undefined, mutation.name);
  }
  validateStorageCompactionEquivalenceV1(v7Cases.equivalenceCases.valid);
  for (const field of v7Cases.equivalenceCases.invalidFields) {
    const invalid = structuredClone(v7Cases.equivalenceCases.valid) as Record<string, unknown>;
    invalid[field] = false;
    assert.throws(() => validateStorageCompactionEquivalenceV1(invalid), undefined, field);
  }
  for (const rowsInBatch of v7Cases.equivalenceCases.invalidRowsInBatch) {
    const invalid = structuredClone(v7Cases.equivalenceCases.valid) as Record<string, unknown>;
    invalid.rowsInBatch = rowsInBatch;
    assert.throws(() => validateStorageCompactionEquivalenceV1(invalid));
  }
  for (const mutation of v7Cases.invalidPreflightMutations) {
    const invalid = structuredClone(manifestV7.migration.preflight);
    setPath(invalid, mutation.path, mutation.replacement);
    assert.throws(() => validateStorageLifetimeV7PreflightPolicyV1(invalid), undefined, mutation.name);
  }
  for (const mutation of v7Cases.invalidPolicyMutations) {
    const invalid = structuredClone(manifestV7.losslessCompaction);
    setPath(invalid, mutation.path, mutation.replacement);
    assert.throws(() => validateStorageLosslessCompactionPolicyV1(invalid), undefined, mutation.name);
  }
  assert.deepEqual(v7Cases.replayNamespaces.map((entry) => [
    entry.namespace, entry.duplicate, entry.conflict,
  ]), [
    ["legacy", "duplicateNoOp", "conflict"],
    ["actualAgent", "duplicateNoOp", "conflict"],
    ["recovery", "duplicateNoOp", "conflict"],
  ]);

  const migrationV6 = readFileSync(resolve(rootV6, manifestV6.migration.file), "utf8");
  const migrationV7 = readFileSync(resolve(rootV7, manifestV7.migration.file), "utf8");
  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-storage-v7-"));
  try {
    const reference = createExactStorageV5Database(join(temporaryRoot, "reference.db"), false);
    applyExactStorageV6Migration(reference, migrationV6, schemaSignature(reference), null);
    const exactV6 = schemaSignature(reference);
    applyExactStorageV7Migration(reference, migrationV7, exactV6, null);
    const exactV7 = schemaSignature(reference);
    for (const [table, count] of Object.entries(v7Cases.migrationMatrix.newTableInitialCounts)) {
      assert.equal(reference.prepare(`SELECT COUNT(*) AS count FROM ${table}`).get()?.count, count);
    }
    applyExactStorageV7Migration(reference, migrationV7, exactV6, exactV7);
    reference.close();

    for (const mapping of [
      { name: "ready-absent", materializationState: "ready", jobState: null },
      { name: "pending-absent", materializationState: "pending", jobState: null },
      { name: "pending-pending", materializationState: "pending", jobState: "pending" },
      { name: "pending-failed", materializationState: "pending", jobState: "failed" },
    ] as const) {
      const mapped = createExactStorageV5Database(
        join(temporaryRoot, `${mapping.name}.db`), false,
      );
      applyExactStorageV6Migration(mapped, migrationV6, schemaSignature(mapped), exactV6);
      mapped.prepare("UPDATE storage_schema_meta SET materialization_state=?").run(
        mapping.materializationState,
      );
      if (mapping.jobState !== null) {
        insertV5LifetimeJob(mapped, mapping.jobState, "indexSource", 7, true);
      }
      mapped.prepare(`
        INSERT INTO maintenance_jobs(
          job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
          failure_kind,created_at,updated_at
        ) VALUES('historic-success','losslessCompaction','succeeded','{}','start','done','done',NULL,'created','done')
      `).run();
      mapped.prepare(`
        INSERT INTO maintenance_jobs(
          job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
          failure_kind,created_at,updated_at
        ) VALUES('historic-failure','losslessCompaction','failed','{}','start','done',NULL,'verification','created','done')
      `).run();
      const pairBefore = readV6Pair(mapped);
      const maintenanceBefore = normalizedRows(mapped, "maintenance_jobs");
      applyExactStorageV7Migration(mapped, migrationV7, exactV6, exactV7);
      assert.deepEqual(readV6Pair(mapped), pairBefore, mapping.name);
      assert.deepEqual(normalizedRows(mapped, "maintenance_jobs"), maintenanceBefore, mapping.name);
      mapped.close();
    }

    const database = createExactStorageV5Database(join(temporaryRoot, "guarded.db"), false);
    applyExactStorageV6Migration(database, migrationV6, schemaSignature(database), exactV6);
    seedStorageV6CompactionFixture(database);
    const preservedBefore = new Map(manifestV7.migration.preservedTables.map((table) => [
      table, normalizedRows(database, table),
    ]));
    applyExactStorageV7Migration(database, migrationV7, exactV6, exactV7);
    for (const table of manifestV7.migration.preservedTables) {
      assert.deepEqual(normalizedRows(database, table), preservedBefore.get(table), table);
    }
    assert.equal(database.prepare("PRAGMA foreign_keys").get()?.foreign_keys, 1);

    const insertPrefix = database.prepare(`
      INSERT INTO ledger_compaction_prefixes(
        prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,
        anchor_event_rowid,anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,
        baseline_id,baseline_projection_digest,created_at,verified_at
      ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)
    `);
    const chatPrefix = [
      "prefix_chat_1_1", "chat", "chat_1", 1, 4, "evt_chat_2", "op_chat_2", 2,
      "baseline_chat_1_1", "digest-chat-1", "2026-09-01T09:00:00Z",
      "2026-09-01T09:00:01Z",
    ] as const;
    insertPrefix.run(...chatPrefix);
    assert.throws(() => insertPrefix.run(
      "prefix_bad_anchor", "chat", "chat_1", 1, 3, "evt_project_2", "op_project_2", 2,
      "baseline_chat_1_1", "digest-chat-1", "2026-09-01T09:00:00Z",
      "2026-09-01T09:00:01Z",
    ));
    assert.throws(() => insertPrefix.run(
      "prefix_app", "app_instance", "instance_1", 1, 7, "evt_instance_1", "op_instance_1", 2,
      "baseline_chat_1_1", "digest-chat-1", "2026-09-01T09:00:00Z",
      "2026-09-01T09:00:01Z",
    ));

    const insertFrozen = database.prepare(`
      INSERT INTO frozen_message_sources(
        message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,
        source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at
      ) VALUES(?,?,?,?,?,?,?,?,?,?)
    `);
    const frozenA = [
      "message_a", 2, "evt_chat_1", "op_chat_1", "chat_1", 1, 1,
      '{"messageId":"message_a"}', "digest-message-a", "2026-09-01T09:00:00Z",
    ] as const;
    insertFrozen.run(...frozenA);
    assert.throws(() => insertFrozen.run(
      "message_b", 2, "evt_chat_1", "op_chat_1", "chat_1", 1, 2,
      '{"messageId":"message_b"}', "digest-message-b", "2026-09-01T09:00:00Z",
    ));
    assert.throws(() => database.prepare(
      "UPDATE frozen_message_sources SET message_digest='forged' WHERE message_id='message_a'",
    ).run());
    assert.throws(() => database.prepare(
      "DELETE FROM frozen_message_sources WHERE message_id='message_a'",
    ).run());

    database.prepare(`
      INSERT INTO maintenance_jobs(
        job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
        failure_kind,created_at,updated_at
      ) VALUES('compact_1','losslessCompaction','running','{}','2026-09-01T09:01:00Z',NULL,NULL,NULL,
               '2026-09-01T09:00:00Z','2026-09-01T09:01:00Z')
    `).run();
    assert.throws(() => database.prepare(`
      INSERT INTO maintenance_jobs(
        job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
        failure_kind,created_at,updated_at
      ) VALUES('compact_2','losslessCompaction','running','{}','2026-09-01T09:01:00Z',NULL,NULL,NULL,
               '2026-09-01T09:00:00Z','2026-09-01T09:01:00Z')
    `).run());
    database.prepare(`
      INSERT INTO maintenance_jobs(
        job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
        failure_kind,created_at,updated_at
      ) VALUES('compact_2','losslessCompaction','pending','{}',NULL,NULL,NULL,NULL,
               '2026-09-01T09:00:00Z','2026-09-01T09:00:00Z')
    `).run();
    assert.throws(() => database.prepare(`
      UPDATE maintenance_jobs SET state='running',started_at='2026-09-01T09:01:00Z'
      WHERE job_id='compact_2'
    `).run());
    const insertAuthority = database.prepare(`
      INSERT INTO ledger_compaction_delete_authority(
        job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,
        aggregate_type,aggregate_id,aggregate_sequence,authorized_at
      ) VALUES(?,?,?,?,?,?,?,?,?,?)
    `);
    assert.throws(() => database.prepare("DELETE FROM event_source_index WHERE event_rowid=2").run());
    assert.throws(() => database.prepare("DELETE FROM events WHERE rowid=2").run());
    insertAuthority.run(
      "compact_1", 1, "prefix_chat_1_1", 2, "evt_chat_1", "op_chat_1",
      "chat", "chat_1", 1, "2026-09-01T09:01:00Z",
    );
    assert.throws(() => database.prepare(
      "DELETE FROM ledger_compaction_delete_authority WHERE job_id='compact_1' AND ordinal=1",
    ).run());
    insertPrefix.run(
      "prefix_project_1_1", "project", "project_1", 1, 3, "evt_project_2", "op_project_2", 2,
      "baseline_project_1_1", "digest-project-1", "2026-09-01T09:00:00Z",
      "2026-09-01T09:00:01Z",
    );
    assert.throws(() => insertAuthority.run(
      "compact_2", 1, "prefix_project_1_1", 1, "evt_project_1", "op_project_1",
      "project", "project_1", 1, "2026-09-01T09:01:00Z",
    ));
    assert.throws(() => insertAuthority.run(
      "compact_1", 101, "prefix_project_1_1", 1, "evt_project_1", "op_project_1",
      "project", "project_1", 1, "2026-09-01T09:01:00Z",
    ));

    const preservedArtifacts = [
      "operation_receipts", "chat_messages", "prepared_workspaces", "ledger_baselines",
      "projects", "chats", "runs", "locks", "app_instances",
    ];
    const artifactsBefore = new Map(preservedArtifacts.map((table) => [
      table, normalizedRows(database, table),
    ]));
    database.exec("BEGIN IMMEDIATE");
    database.prepare("DELETE FROM event_source_index WHERE event_rowid=2").run();
    database.prepare("DELETE FROM events WHERE rowid=2").run();
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM ledger_compaction_delete_authority",
    ).get()?.count, 0);
    database.exec("COMMIT");
    for (const table of preservedArtifacts) {
      assert.deepEqual(normalizedRows(database, table), artifactsBefore.get(table), table);
    }
    assert.equal(database.prepare("SELECT COUNT(*) AS count FROM events WHERE rowid=4").get()?.count, 1);
    assert.throws(() => database.prepare("DELETE FROM event_source_index WHERE event_rowid=7").run());
    assert.throws(() => database.prepare("DELETE FROM events WHERE rowid=7").run());

    database.prepare(`
      INSERT INTO events(
        event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
        event_type,payload_schema_version,canonical_json_payload,timestamp
      ) VALUES('evt_chat_5','op_chat_5',NULL,'chat','chat_1',5,'chat.upserted',1,
        '{"durable_event_sequence":3,"last_message":{"schemaVersion":1,"messageId":"message_a","chatId":"chat_1","taskId":"task_1","worktreeId":"worktree_1","eventSequence":3,"kind":"user","roleId":null,"runId":null,"text":"again","createdAt":"2026-09-01T09:02:00Z"}}',
        '2026-09-01T09:02:00Z')
    `).run();
    assert.throws(() => database.prepare(`
      INSERT INTO event_source_index(
        event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,
        introduced_message_id,chat_durable_sequence,chat_snapshot_message_id
      ) VALUES(8,'evt_chat_5','op_chat_5','chat','chat_1',5,'message_a',3,'message_a')
    `).run());

    insertAuthority.run(
      "compact_1", 1, "prefix_project_1_1", 1, "evt_project_1", "op_project_1",
      "project", "project_1", 1, "2026-09-01T09:03:00Z",
    );
    database.exec("BEGIN IMMEDIATE");
    database.prepare("DELETE FROM event_source_index WHERE event_rowid=1").run();
    database.prepare("DELETE FROM events WHERE rowid=1").run();
    database.exec("ROLLBACK");
    assert.equal(database.prepare("SELECT COUNT(*) AS count FROM events WHERE rowid=1").get()?.count, 1);
    assert.equal(database.prepare("SELECT COUNT(*) AS count FROM event_source_index WHERE event_rowid=1").get()?.count, 1);
    assert.equal(database.prepare("SELECT COUNT(*) AS count FROM ledger_compaction_delete_authority").get()?.count, 1);

    const declaredPlans = Object.keys(manifestV7.losslessCompaction.queryPlanAcceptance)
      .filter((key) => key !== "mode" && key !== "reject").sort();
    assert.deepEqual(declaredPlans, v7Cases.queryPlans.map((plan) => plan.name).sort());
    for (const fixture of v7Cases.queryPlans) {
      const plans = database.prepare(fixture.sql).all();
      assert.equal(plans.length, 1, fixture.name);
      assert.equal(plans[0]?.detail, fixture.detail, fixture.name);
    }
    database.close();

    for (const [name, prepareUnsafe] of [
      ["attempting", (db: DatabaseSync) => {
        db.prepare("UPDATE storage_schema_meta SET materialization_state='pending'").run();
        insertV5LifetimeJob(db, "attempting", "indexSource", 0, false);
      }],
      ["pending-maintenance", (db: DatabaseSync) => db.prepare(`
        INSERT INTO maintenance_jobs(
          job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
          failure_kind,created_at,updated_at
        ) VALUES('active','losslessCompaction','pending',NULL,NULL,NULL,NULL,NULL,'now','now')
      `).run()],
      ["running-maintenance", (db: DatabaseSync) => db.prepare(`
        INSERT INTO maintenance_jobs(
          job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
          failure_kind,created_at,updated_at
        ) VALUES('active','losslessCompaction','running',NULL,'now',NULL,NULL,NULL,'now','now')
      `).run()],
      ["delete-authority-object", (db: DatabaseSync) => db.exec(`
        CREATE TABLE ledger_compaction_delete_authority(job_id TEXT PRIMARY KEY) STRICT
      `)],
      ["extra-schema", (db: DatabaseSync) => db.exec("CREATE TABLE unexpected_v7(value TEXT) STRICT")],
    ] as Array<[string, (database: DatabaseSync) => unknown]>) {
      const unsafe = createExactStorageV5Database(join(temporaryRoot, `${name}.db`), false);
      applyExactStorageV6Migration(unsafe, migrationV6, schemaSignature(unsafe), exactV6);
      prepareUnsafe(unsafe);
      const before = `${schemaSignature(unsafe)}\n${databaseRowsSnapshot(unsafe)}`;
      assert.throws(() => applyExactStorageV7Migration(unsafe, migrationV7, exactV6, exactV7), undefined, name);
      assert.equal(`${schemaSignature(unsafe)}\n${databaseRowsSnapshot(unsafe)}`, before, name);
      unsafe.close();
    }
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v8 freezes operation provenance without activating product deletion", () => {
  assert.equal(manifestV8.schemaVersion, 1);
  assert.equal(manifestV8.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V8);
  assert.equal(manifestV8.protocolVersion, PROTOCOL_VERSION);
  assert.equal(manifestV8.payloadSchemaVersion, 1);
  assert.equal(manifestV8.migration.fromVersion, 7);
  assert.equal(manifestV8.migration.toVersion, 8);
  assert.equal(manifestV8.migration.predecessorMigrationFile,
    "../storage-v7/migration-v6-to-v7.sql");
  assert.equal(manifestV8.migration.predecessorMigrationMutationAllowed, false);
  assert.equal(manifestV8.migration.historyBackfill, "none");
  assert.equal(manifestV8.migration.runtimeGuardDdlAllowed, false);
  assert.equal(manifestV8.migration.existingRowsDeleted, false);
  assert.deepEqual(manifestV8.migration.tablesAdded, ["frozen_operation_sources"]);
  assert.deepEqual(validateStorageLifetimeV8PreflightPolicyV1(
    manifestV8.migration.preflight,
  ), manifestV8.migration.preflight);
  assert.deepEqual(validateStorageFrozenOperationSourcePolicyV1(
    manifestV8.frozenOperationSources,
  ), manifestV8.frozenOperationSources);
  assert.deepEqual(validateStorageRuntimeSourceAuthorityPolicyV1(
    manifestV8.runtimeSourceAuthority,
  ), manifestV8.runtimeSourceAuthority);
  assert.deepEqual(validateStorageCompactionPolicyV2(
    manifestV8.compactionV2,
  ), manifestV8.compactionV2);
  assert.deepEqual(STORAGE_COMPACTION_PHASES_V2, [
    "freezeOperations", "freezeMessages", "verifyPrefix", "authorizeDelete",
    "deletePrefix", "verifyOperations", "verifyEquivalence", "finalize",
  ]);

  const validFixture = v8Cases.validFrozenOperationSources[0]!.value;
  assert.deepEqual(validateFrozenOperationSourceV1(validFixture), validFixture);
  validateFrozenOperationSourceBatchV1([validFixture]);
  assert.throws(() => validateFrozenOperationSourceBatchV1([]));
  assert.throws(() => validateFrozenOperationSourceBatchV1(
    Array.from({ length: 101 }, () => validFixture),
  ));
  for (const mutation of v8Cases.invalidFrozenOperationMutations) {
    const invalid = structuredClone(validFixture);
    setPath(invalid, mutation.path, mutation.replacement);
    assert.throws(() => validateFrozenOperationSourceV1(invalid), undefined, mutation.name);
  }

  const projectEvent = v6Cases.sourceIndex.sourceEvents[0]!.event;
  const projectEntryRow = v6Cases.sourceIndex.validRows[0]!.value;
  const projectEntry = {
    eventRowid: projectEntryRow[0], eventId: projectEntryRow[1],
    operationId: projectEntryRow[2], aggregateType: projectEntryRow[3],
    aggregateId: projectEntryRow[4], aggregateSequence: projectEntryRow[5],
    introducedMessageId: projectEntryRow[6], chatDurableSequence: projectEntryRow[7],
    chatSnapshotMessageId: projectEntryRow[8],
  };
  const projectReceipt = {
    operationId: projectEvent.operation_id, eventId: projectEvent.event_id,
    aggregateType: projectEvent.aggregate_type, aggregateId: projectEvent.aggregate_id,
    aggregateSequence: projectEvent.aggregate_sequence, eventType: projectEvent.event_type,
    payloadSchemaVersion: 1, canonicalPayloadJson: projectEvent.canonical_json_payload,
    canonicalPayloadDigestAlgorithm: "sha256", canonicalPayloadDigestVersion: 1,
    canonicalPayloadDigestFormat: "lowercaseHex64",
    canonicalPayloadDigest: createHash("sha256")
      .update(projectEvent.canonical_json_payload).digest("hex"),
    eventTimestamp: projectEvent.timestamp,
  };
  validateFrozenOperationSourceBindingV1(
    validFixture, 1, projectEvent, projectEntry, projectReceipt,
  );
  for (const [name, invalidReceipt] of [
    ["receipt event id", { ...projectReceipt, eventId: "evt_other" }],
    ["receipt event type", { ...projectReceipt, eventType: "project.deleted" }],
    ["receipt timestamp", { ...projectReceipt, eventTimestamp: "2026-09-01T10:00:00Z" }],
  ] as const) {
    assert.throws(() => validateFrozenOperationSourceBindingV1(
      validFixture, 1, projectEvent, projectEntry, invalidReceipt,
    ), undefined, name);
    assert.throws(() => validateFrozenOperationReceiptBindingV1(
      validFixture, invalidReceipt,
    ), undefined, `${name} after source deletion`);
  }
  validateFrozenOperationReceiptBindingV1(validFixture, projectReceipt);
  assert.deepEqual(new Set(v8Cases.bindingTamperCases), new Set([
    "receiptEventId", "receiptEventType", "receiptEventTimestamp",
    "payloadAndDigestTogether", "aggregateAndOperationTogether",
    "sourceIndexIdentity", "receiptOnly", "appInstance",
  ]));

  for (const fixture of v8Cases.runtimeAuthorityCases.valid) {
    validateStorageRuntimeSourceAuthorityV1(fixture.value);
  }
  for (const fixture of v8Cases.runtimeAuthorityCases.invalid) {
    assert.throws(() => validateStorageRuntimeSourceAuthorityV1(fixture.value), undefined,
      fixture.name);
  }
  for (const fixture of v8Cases.validCursorTransitionsV2) {
    validateStorageCompactionCursorTransitionV2Contract(
      fixture.previous, fixture.next, fixture.evidence,
    );
  }
  for (const fixture of v8Cases.invalidCursorTransitionsV2) {
    assert.throws(() => validateStorageCompactionCursorTransitionV2Contract(
      fixture.previous, fixture.next, fixture.evidence,
    ), undefined, fixture.name);
  }
  for (const fixture of v8Cases.validLifecycleTransitionsV2) {
    validateStorageCompactionLifecycleTransitionV2(
      fixture.operation, fixture.previous, fixture.next, fixture.evidence,
    );
  }
  for (const mutation of v8Cases.invalidLifecycleMutationsV2) {
    const fixture = structuredClone(v8Cases.validLifecycleTransitionsV2[mutation.caseIndex]);
    assert.ok(fixture);
    setPath(fixture, mutation.path, mutation.replacement);
    assert.throws(() => validateStorageCompactionLifecycleTransitionV2(
      fixture.operation, fixture.previous, fixture.next, fixture.evidence,
    ), undefined, mutation.name);
  }
  for (const fixture of v8Cases.invalidLifecycleTransitionsV2) {
    assert.throws(() => validateStorageCompactionLifecycleTransitionV2(
      fixture.operation, fixture.previous, fixture.next, fixture.evidence,
    ), undefined, fixture.name);
  }
  assert.equal(v8Cases.privateEquivalenceCasesV2.valid.operationsVerified, true);
  assert.deepEqual(v8Cases.privateEquivalenceCasesV2.invalidFields, [
    "operationsVerified", "baselineTailBefore", "anchorTailAfter",
    "preservedArtifactsByteExact", "receiptReplayEquivalent",
    "namespaceDecisionsEquivalent", "frozenMessageIdsReserved", "authorityEmpty",
  ]);
  assert.deepEqual(v8Cases.privateEquivalenceCasesV2.invalidRowsInBatch, [0, 101]);
  assert.deepEqual(v8Cases.namespaceReplay.map((entry) => [
    entry.namespace, entry.duplicate, entry.conflict,
  ]), [
    ["legacy", "duplicateNoOp", "conflict"],
    ["actualAgent", "duplicateNoOp", "conflict"],
    ["recovery", "duplicateNoOp", "conflict"],
  ]);
  assert.deepEqual(v8Cases.preservedV7MessageReuse, {
    deletedIntroducingEvent: "frozenMessageIdentityRemainsReserved",
    newIntroduction: "uniqueConstraintRejected",
  });
  for (const mutation of v8Cases.invalidPreflightMutations) {
    const invalid = structuredClone(manifestV8.migration.preflight);
    setPath(invalid, mutation.path, mutation.replacement);
    assert.throws(() => validateStorageLifetimeV8PreflightPolicyV1(invalid), undefined,
      mutation.name);
  }
  const policyValidators = {
    frozenOperationSources: validateStorageFrozenOperationSourcePolicyV1,
    runtimeSourceAuthority: validateStorageRuntimeSourceAuthorityPolicyV1,
    compactionV2: validateStorageCompactionPolicyV2,
  };
  for (const mutation of v8Cases.invalidPolicyMutations) {
    const invalid = structuredClone(manifestV8[mutation.target]);
    setPath(invalid, mutation.path, mutation.replacement);
    assert.throws(() => policyValidators[mutation.target](invalid), undefined, mutation.name);
  }

  const migrationV6 = readFileSync(resolve(rootV6, manifestV6.migration.file), "utf8");
  const migrationV7 = readFileSync(resolve(rootV7, manifestV7.migration.file), "utf8");
  const migrationV8 = readFileSync(resolve(rootV8, manifestV8.migration.file), "utf8");
  assert.doesNotMatch(migrationV8, /sha3|load_extension|CREATE FUNCTION/i);
  assert.match(migrationV8, /consumer must independently recompute/i);
  assert.doesNotMatch(migrationV8, /DELETE FROM events\s*;/i);
  assert.doesNotMatch(migrationV8, /VACUUM/i);
  const temporaryRoot = mkdtempSync(join(tmpdir(), "elsecrew-storage-v8-"));
  try {
    const reference = createExactStorageV5Database(join(temporaryRoot, "reference.db"), false);
    applyExactStorageV6Migration(reference, migrationV6, schemaSignature(reference), null);
    const exactV6 = schemaSignature(reference);
    applyExactStorageV7Migration(reference, migrationV7, exactV6, null);
    const exactV7 = schemaSignature(reference);
    applyExactStorageV8Migration(reference, migrationV8, exactV7, null);
    const exactV8 = schemaSignature(reference);
    assert.equal(reference.prepare(
      "SELECT storage_schema_version FROM storage_schema_meta WHERE singleton=1",
    ).get()?.storage_schema_version, 8);
    for (const [table, count] of Object.entries(v8Cases.migrationMatrix.newTableInitialCounts)) {
      assert.equal(reference.prepare(`SELECT COUNT(*) AS count FROM ${table}`).get()?.count, count);
    }
    applyExactStorageV8Migration(reference, migrationV8, exactV7, exactV8);
    reference.close();

    for (const mapping of [
      { name: "ready-absent", materializationState: "ready", jobState: null },
      { name: "pending-absent", materializationState: "pending", jobState: null },
      { name: "pending-pending", materializationState: "pending", jobState: "pending" },
      { name: "pending-failed", materializationState: "pending", jobState: "failed" },
    ] as const) {
      const mapped = createExactStorageV5Database(join(temporaryRoot, `${mapping.name}.db`), false);
      applyExactStorageV6Migration(mapped, migrationV6, schemaSignature(mapped), exactV6);
      mapped.prepare("UPDATE storage_schema_meta SET materialization_state=?")
        .run(mapping.materializationState);
      if (mapping.jobState !== null) {
        insertV5LifetimeJob(mapped, mapping.jobState, "indexSource", 0, false);
      }
      mapped.prepare(`
        INSERT INTO maintenance_jobs(
          job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
          failure_kind,created_at,updated_at
        ) VALUES('historic','losslessCompaction','succeeded','{}','start','done','done',NULL,'created','done')
      `).run();
      const pairBefore = readV6Pair(mapped);
      const rowsBefore = databaseRowsSnapshot(mapped);
      applyExactStorageV7Migration(mapped, migrationV7, exactV6, exactV7);
      const rowsBeforeV8 = databaseRowsSnapshot(mapped);
      applyExactStorageV8Migration(mapped, migrationV8, exactV7, exactV8);
      assert.deepEqual(readV6Pair(mapped), pairBefore, mapping.name);
      assert.notEqual(rowsBefore, rowsBeforeV8, "v7 schema version transition is expected");
      assert.equal(normalizedRows(mapped, "maintenance_jobs")[0]?.job_id, "historic");
      assert.equal(mapped.prepare(
        "SELECT COUNT(*) AS count FROM frozen_operation_sources",
      ).get()?.count, 0);
      mapped.close();
    }

    const database = createExactStorageV5Database(join(temporaryRoot, "guarded.db"), false);
    applyExactStorageV6Migration(database, migrationV6, schemaSignature(database), exactV6);
    seedStorageV6CompactionFixture(database, true);
    seedV8OperationReceipts(database);
    applyExactStorageV7Migration(database, migrationV7, exactV6, exactV7);
    const preservedBefore = new Map(manifestV8.migration.preservedTables.map((table) => [
      table, normalizedRows(database, table),
    ]));
    applyExactStorageV8Migration(database, migrationV8, exactV7, exactV8);
    for (const table of manifestV8.migration.preservedTables) {
      assert.deepEqual(normalizedRows(database, table), preservedBefore.get(table), table);
    }
    assert.equal(database.prepare("PRAGMA foreign_keys").get()?.foreign_keys, 1);

    const project1 = frozenOperationForEvent(1, database);
    const insertCandidate = (candidate: typeof project1) => database.prepare(`
      INSERT INTO frozen_operation_sources(
        operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,aggregate_id,
        aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,
        canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at
      ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
    `).run(
      candidate.operationId, candidate.sourceEventRowid, candidate.sourceEventId,
      candidate.requestId, candidate.aggregateType, candidate.aggregateId,
      candidate.aggregateSequence, candidate.eventType, candidate.payloadSchemaVersion,
      candidate.canonicalPayloadJson, candidate.canonicalPayloadDigest,
      candidate.eventTimestamp, candidate.canonicalRecordJson, candidate.recordDigest,
      candidate.frozenAt,
    );
    for (const [name, candidate] of [
      ["nonexistent source", { ...project1, sourceEventRowid: 99 }],
      ["source event identity", { ...project1, sourceEventId: "evt_other" }],
      ["aggregate identity", { ...project1, aggregateId: "project_other" }],
      ["payload and digest", {
        ...project1, canonicalPayloadJson: "{}",
        canonicalPayloadDigest: createHash("sha256").update("{}").digest("hex"),
      }],
      ["canonical envelope", { ...project1, canonicalRecordJson: "{}" }],
    ] as const) {
      assert.throws(() => insertCandidate(candidate as typeof project1), undefined, name);
    }
    const wrongRecordDigest = { ...project1, recordDigest: "b".repeat(64) };
    assert.throws(() => validateFrozenOperationSourceV1(wrongRecordDigest), undefined,
      "consumer must reject wrong full-record digest before SQL");
    insertFrozenOperation(database, project1);
    assert.throws(() => database.prepare(
      "UPDATE frozen_operation_sources SET frozen_at='later' WHERE operation_id='op_project_1'",
    ).run());
    assert.throws(() => database.prepare(
      "DELETE FROM frozen_operation_sources WHERE operation_id='op_project_1'",
    ).run());
    assert.throws(() => database.prepare(
      "UPDATE operation_receipts SET event_type='tampered' WHERE operation_id='op_project_1'",
    ).run());
    assert.throws(() => database.prepare(
      "DELETE FROM operation_receipts WHERE operation_id='op_project_1'",
    ).run());

    const insertPrefix = database.prepare(`
      INSERT INTO ledger_compaction_prefixes(
        prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,
        anchor_event_rowid,anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,
        baseline_id,baseline_projection_digest,created_at,verified_at
      ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)
    `);
    const prefixValues = [
      "prefix_project_1_1", "project", "project_1", 1, 3, "evt_project_2", "op_project_2", 2,
      "baseline_project_1_1", "digest-project-1", "2026-09-01T10:01:00Z",
      "2026-09-01T10:01:01Z",
    ] as const;
    assert.throws(() => insertPrefix.run(...prefixValues));
    insertFrozenOperation(database, frozenOperationForEvent(3, database));
    insertPrefix.run(...prefixValues);

    const insertFrozenMessage = database.prepare(`
      INSERT INTO frozen_message_sources(
        message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,
        source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at
      ) VALUES(?,?,?,?,?,?,?,?,?,?)
    `);
    const messageValues = [
      "message_a", 2, "evt_chat_1", "op_chat_1", "chat_1", 1, 1,
      '{"messageId":"message_a"}', "digest-message-a", "2026-09-01T10:02:00Z",
    ] as const;
    assert.throws(() => insertFrozenMessage.run(...messageValues));
    insertFrozenOperation(database, frozenOperationForEvent(2, database));
    insertFrozenMessage.run(...messageValues);
    database.prepare(`
      INSERT INTO events(
        event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
        event_type,payload_schema_version,canonical_json_payload,timestamp
      ) VALUES('evt_chat_reuse','op_chat_reuse',NULL,'chat','chat_1',5,'chat.upserted',1,
        '{"durable_event_sequence":3,"last_message":{"schemaVersion":1,"messageId":"message_a","chatId":"chat_1","taskId":"task_1","worktreeId":"worktree_1","eventSequence":3,"kind":"user","roleId":null,"runId":null,"text":"reuse","createdAt":"2026-09-01T10:02:01Z"}}',
        '2026-09-01T10:02:01Z')
    `).run();
    assert.throws(() => database.prepare(`
      INSERT INTO event_source_index(
        event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,
        introduced_message_id,chat_durable_sequence,chat_snapshot_message_id
      ) VALUES(8,'evt_chat_reuse','op_chat_reuse','chat','chat_1',5,'message_a',3,'message_a')
    `).run());

    database.prepare(`
      INSERT INTO maintenance_jobs(
        job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
        failure_kind,created_at,updated_at
      ) VALUES('compact_v8','losslessCompaction','running','{}','now',NULL,NULL,NULL,'now','now')
    `).run();
    database.prepare(`
      INSERT INTO ledger_compaction_delete_authority(
        job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,
        aggregate_type,aggregate_id,aggregate_sequence,authorized_at
      ) VALUES('compact_v8',1,'prefix_project_1_1',1,'evt_project_1','op_project_1',
        'project','project_1',1,'2026-09-01T10:03:00Z')
    `).run();
    assert.throws(() => insertCandidate({
      ...project1, operationId: "op_instance_1", sourceEventRowid: 7,
      sourceEventId: "evt_instance_1", aggregateType: "app_instance" as "project",
      aggregateId: "instance_1", eventType: "app_instance.started",
    }));

    database.prepare(`
      INSERT INTO maintenance_jobs(
        job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
        failure_kind,created_at,updated_at
      ) VALUES('compact_private_proof','losslessCompaction','pending','{}',NULL,NULL,NULL,NULL,
               '2026-09-01T10:04:00Z','2026-09-01T10:04:00Z')
    `).run();
    const forgedExhaustion = v8Cases.invalidCursorTransitionsV2.find(
      (fixture) => fixture.name === "caller cannot forge exact-empty exhaustion",
    );
    assert.ok(forgedExhaustion);
    const durableJobsBeforeForgedExhaustion = JSON.stringify(
      normalizedRows(database, "maintenance_jobs"),
    );
    assert.throws(() => validateStorageCompactionCursorTransitionV2Contract(
      forgedExhaustion.previous, forgedExhaustion.next, forgedExhaustion.evidence,
    ));
    assert.equal(JSON.stringify(normalizedRows(database, "maintenance_jobs")),
      durableJobsBeforeForgedExhaustion,
      "public zero-row proof cannot mutate durable exhaustion");

    database.exec("BEGIN IMMEDIATE");
    const nonemptyKeyset = database.prepare(`
      SELECT rowid AS event_rowid FROM events
      WHERE rowid > ? AND rowid <= ? ORDER BY rowid LIMIT 100
    `).all(0, 6);
    assert.ok(nonemptyKeyset.length > 0);
    database.exec("ROLLBACK");
    assert.equal(JSON.stringify(normalizedRows(database, "maintenance_jobs")),
      durableJobsBeforeForgedExhaustion,
      "nonempty private keyset cannot persist exhaustion");

    const privatelyExhaustedCursor = {
      schemaVersion: 2, cutoffEventRowid: 6, phase: "freezeOperations",
      afterEventRowid: 6, scanExhausted: true,
    };
    database.exec("BEGIN IMMEDIATE");
    const exactEmptyKeyset = database.prepare(`
      SELECT rowid AS event_rowid FROM events
      WHERE rowid > ? AND rowid <= ? ORDER BY rowid LIMIT 100
    `).all(6, 6);
    assert.deepEqual(exactEmptyKeyset, []);
    database.prepare("UPDATE maintenance_jobs SET cutoff_json=?,updated_at=? WHERE job_id=?").run(
      JSON.stringify({ storageCompactionV2PrivateState: {
        schemaVersion: 1, cursor: privatelyExhaustedCursor,
        operationsVerified: false, equivalenceVerified: false,
      } }),
      "2026-09-01T10:04:01Z", "compact_private_proof",
    );
    database.exec("COMMIT");
    assert.deepEqual(JSON.parse(String(database.prepare(
      "SELECT cutoff_json FROM maintenance_jobs WHERE job_id='compact_private_proof'",
    ).get()?.cutoff_json)).storageCompactionV2PrivateState.cursor, privatelyExhaustedCursor);

    const forgedFinal = v8Cases.invalidLifecycleTransitionsV2[0]!;
    const durableJobsBeforeForgedFinal = JSON.stringify(
      normalizedRows(database, "maintenance_jobs"),
    );
    assert.throws(() => validateStorageCompactionLifecycleTransitionV2(
      forgedFinal.operation, forgedFinal.previous, forgedFinal.next, forgedFinal.evidence,
    ));
    assert.equal(JSON.stringify(normalizedRows(database, "maintenance_jobs")),
      durableJobsBeforeForgedFinal,
      "public final booleans cannot mutate durable verification state");

    const privateFinalCursor = {
      schemaVersion: 2, cutoffEventRowid: 6, phase: "finalize",
      afterEventRowid: 6, scanExhausted: true,
    };
    database.exec("BEGIN IMMEDIATE");
    const boundedOperations = database.prepare(`
      SELECT operation_id FROM frozen_operation_sources
      WHERE source_event_rowid <= ? ORDER BY source_event_rowid LIMIT 100
    `).all(6);
    assert.ok(boundedOperations.length > 0 && boundedOperations.length <= 100);
    assert.equal(database.prepare(`
      SELECT COUNT(*) AS count FROM ledger_compaction_delete_authority WHERE job_id=?
    `).get("compact_private_proof")?.count, 0);
    database.prepare(`
      UPDATE maintenance_jobs
      SET state='succeeded',cutoff_json=?,started_at=?,completed_at=?,last_successful_at=?,updated_at=?
      WHERE job_id=?
    `).run(
      JSON.stringify({ storageCompactionV2PrivateState: {
        schemaVersion: 1, cursor: privateFinalCursor,
        operationsVerified: true, equivalenceVerified: true,
      } }),
      "2026-09-01T10:04:02Z", "2026-09-01T10:04:03Z", "2026-09-01T10:04:03Z",
      "2026-09-01T10:04:03Z", "compact_private_proof",
    );
    database.exec("COMMIT");
    const privateFinal = database.prepare(`
      SELECT state,cutoff_json FROM maintenance_jobs WHERE job_id='compact_private_proof'
    `).get();
    assert.equal(privateFinal?.state, "succeeded");
    assert.deepEqual(JSON.parse(String(privateFinal?.cutoff_json)).storageCompactionV2PrivateState, {
      schemaVersion: 1, cursor: privateFinalCursor,
      operationsVerified: true, equivalenceVerified: true,
    });

    const declaredPlans = Object.keys(
      manifestV8.frozenOperationSources.queryPlanAcceptance,
    ).filter((key) => key !== "mode" && key !== "reject").sort();
    assert.deepEqual(declaredPlans, v8Cases.queryPlans.map((plan) => plan.name).sort());
    for (const fixture of v8Cases.queryPlans) {
      const plans = database.prepare(fixture.sql).all();
      assert.equal(plans.length, 1, fixture.name);
      assert.equal(plans[0]?.detail, fixture.detail, fixture.name);
      assert.doesNotMatch(String(plans[0]?.detail), /SCAN|USE TEMP B-TREE/, fixture.name);
    }
    database.close();

    const receiptTamper = createExactStorageV5Database(
      join(temporaryRoot, "receipt-tamper.db"), false,
    );
    applyExactStorageV6Migration(
      receiptTamper, migrationV6, schemaSignature(receiptTamper), exactV6,
    );
    seedStorageV6CompactionFixture(receiptTamper, true);
    seedV8OperationReceipts(receiptTamper);
    receiptTamper.prepare(
      "UPDATE operation_receipts SET event_id='evt_receipt_other' WHERE operation_id='op_project_1'",
    ).run();
    receiptTamper.prepare(
      "UPDATE operation_receipts SET event_type='chat.deleted' WHERE operation_id='op_chat_1'",
    ).run();
    receiptTamper.prepare(
      "UPDATE operation_receipts SET event_timestamp='2026-09-01T10:00:00Z' WHERE operation_id='op_project_2'",
    ).run();
    applyExactStorageV7Migration(receiptTamper, migrationV7, exactV6, exactV7);
    applyExactStorageV8Migration(receiptTamper, migrationV8, exactV7, exactV8);
    for (const [name, rowid] of [
      ["receipt event id", 1], ["receipt event type", 2], ["receipt timestamp", 3],
    ] as const) {
      assert.throws(() => insertFrozenOperation(
        receiptTamper, frozenOperationForEvent(rowid, receiptTamper),
      ), undefined, name);
    }
    receiptTamper.close();

    for (const [name, prepareUnsafe] of [
      ["attempting", (db: DatabaseSync) => {
        db.prepare("UPDATE storage_schema_meta SET materialization_state='pending'").run();
        insertV5LifetimeJob(db, "attempting", "indexSource", 0, false);
      }],
      ["pending-maintenance", (db: DatabaseSync) => db.prepare(`
        INSERT INTO maintenance_jobs(
          job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
          failure_kind,created_at,updated_at
        ) VALUES('active','losslessCompaction','pending',NULL,NULL,NULL,NULL,NULL,'now','now')
      `).run()],
      ["running-maintenance", (db: DatabaseSync) => db.prepare(`
        INSERT INTO maintenance_jobs(
          job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
          failure_kind,created_at,updated_at
        ) VALUES('active','losslessCompaction','running',NULL,'now',NULL,NULL,NULL,'now','now')
      `).run()],
      ["partial-target", (db: DatabaseSync) => db.exec(
        "CREATE TABLE frozen_operation_sources(operation_id TEXT PRIMARY KEY) STRICT",
      )],
      ["extra-schema", (db: DatabaseSync) => db.exec(
        "CREATE TABLE unexpected_v8(value TEXT) STRICT",
      )],
    ] as Array<[string, (database: DatabaseSync) => unknown]>) {
      const unsafe = createExactStorageV5Database(join(temporaryRoot, `${name}.db`), false);
      applyExactStorageV6Migration(unsafe, migrationV6, schemaSignature(unsafe), exactV6);
      applyExactStorageV7Migration(unsafe, migrationV7, exactV6, exactV7);
      prepareUnsafe(unsafe);
      const before = `${schemaSignature(unsafe)}\n${databaseRowsSnapshot(unsafe)}`;
      assert.throws(() => applyExactStorageV8Migration(
        unsafe, migrationV8, exactV7, exactV8,
      ), undefined, name);
      assert.equal(`${schemaSignature(unsafe)}\n${databaseRowsSnapshot(unsafe)}`, before, name);
      unsafe.close();
    }

    const compacted = createExactStorageV5Database(join(temporaryRoot, "compacted.db"), false);
    applyExactStorageV6Migration(compacted, migrationV6, schemaSignature(compacted), exactV6);
    seedStorageV6CompactionFixture(compacted);
    applyExactStorageV7Migration(compacted, migrationV7, exactV6, exactV7);
    compacted.prepare(`
      INSERT INTO ledger_compaction_prefixes(
        prefix_id,aggregate_type,aggregate_id,deleted_through_sequence,
        anchor_event_rowid,anchor_event_id,anchor_operation_id,anchor_aggregate_sequence,
        baseline_id,baseline_projection_digest,created_at,verified_at
      ) VALUES('existing-prefix','project','project_1',1,3,'evt_project_2','op_project_2',2,
        'baseline_project_1_1','digest-project-1','now','now')
    `).run();
    compacted.prepare(`
      INSERT INTO frozen_message_sources(
        message_id,source_event_rowid,source_event_id,source_operation_id,chat_id,
        source_aggregate_sequence,durable_sequence,canonical_message_json,message_digest,frozen_at
      ) VALUES('message_a',2,'evt_chat_1','op_chat_1','chat_1',1,1,
        '{"messageId":"message_a"}','digest-message-a','now')
    `).run();
    compacted.prepare(`
      INSERT INTO maintenance_jobs(
        job_id,job_kind,state,cutoff_json,started_at,completed_at,last_successful_at,
        failure_kind,created_at,updated_at
      ) VALUES('compacted-job','losslessCompaction','running','{}','now',NULL,NULL,NULL,'now','now')
    `).run();
    compacted.prepare(`
      INSERT INTO ledger_compaction_delete_authority(
        job_id,ordinal,prefix_id,event_rowid,event_id,operation_id,
        aggregate_type,aggregate_id,aggregate_sequence,authorized_at
      ) VALUES('compacted-job',1,'existing-prefix',1,'evt_project_1','op_project_1',
        'project','project_1',1,'now')
    `).run();
    compacted.prepare(`
      UPDATE maintenance_jobs SET state='succeeded',completed_at='done',updated_at='done'
      WHERE job_id='compacted-job'
    `).run();
    const compactedBefore = `${schemaSignature(compacted)}\n${databaseRowsSnapshot(compacted)}`;
    assert.throws(() => applyExactStorageV8Migration(
      compacted, migrationV8, exactV7, exactV8,
    ));
    assert.equal(`${schemaSignature(compacted)}\n${databaseRowsSnapshot(compacted)}`,
      compactedBefore);
    compacted.close();

    const rollback = createExactStorageV5Database(join(temporaryRoot, "rollback.db"), false);
    applyExactStorageV6Migration(rollback, migrationV6, schemaSignature(rollback), exactV6);
    applyExactStorageV7Migration(rollback, migrationV7, exactV6, exactV7);
    const rollbackBefore = `${schemaSignature(rollback)}\n${databaseRowsSnapshot(rollback)}`;
    const failingMigration = migrationV8.replace(
      "DROP TABLE storage_schema_meta_v7;",
      "SELECT * FROM missing_v8_failure;\nDROP TABLE storage_schema_meta_v7;",
    );
    assert.throws(() => rollback.exec(failingMigration));
    rollback.exec("ROLLBACK");
    assert.equal(`${schemaSignature(rollback)}\n${databaseRowsSnapshot(rollback)}`,
      rollbackBefore);
    rollback.close();
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("storage-v1 through storage-v7 golden files remain immutable", () => {
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
    ["tests/fixtures/m5/storage-v5/manifest.json", "58c7efcacbf1e9fe7f325520e78033c3ef93ba7fd806f27fdd0a6b3da1c1fceb"],
    ["tests/fixtures/m5/storage-v5/migration-v4-to-v5.sql", "22f9cc9cef5766369ccf5fb9c11262aaa33a05b51a8ccfc03ee3a6a73478889a"],
    ["tests/fixtures/m5/storage-v5/cases.json", "1dca57e669ff782bae856ab29fd00581ff2509ec811e5616754da579125e100d"],
    ["tests/fixtures/m5/storage-v6/cases.json", "d02c817fc7152b5d133cfa675a56896f6faec78bc3d7ccea0a01ba7f98fccfb1"],
    ["tests/fixtures/m5/storage-v6/manifest.json", "e9be63b3fe513eab97c378da8aab451dfbe19e1c7e9dc08928e60bffa02cf851"],
    ["tests/fixtures/m5/storage-v6/migration-v5-to-v6.sql", "14f0ce749503b087296b91b5d27205dfd41aa5c24af26bf5ccd7dbbeeba86dc2"],
    ["tests/fixtures/m5/storage-v7/cases.json", "938f3ece77be6672ede9aa13a8554dcaa685045bcb0d060b0f6c4e38db8ba3f8"],
    ["tests/fixtures/m5/storage-v7/manifest.json", "83774f4597ea9bf56979d995ac0d145b670bcd719152aa521e4a4b6d88dc463a"],
    ["tests/fixtures/m5/storage-v7/migration-v6-to-v7.sql", "ff4ed9aaace2c45d3a4ff0718baf7c173fbd4bbb5a166f557c8bf6070025c33f"],
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

test("historical D0A manifest remains pre-D1 while protocol reserves the activated D1 method", () => {
  assert.deepEqual(M5_BRIDGE_V6_METHODS.slice(0, M5_BRIDGE_V5_METHODS.length), M5_BRIDGE_V5_METHODS);
  assert.deepEqual(M5_BRIDGE_V6_METHODS.slice(M5_BRIDGE_V5_METHODS.length), ["getStorageStatus"]);
  assert.equal(manifest.wire.advertisedBeforeD1, false);
  assert.equal(manifest.wire.dispatchableBeforeD1, false);
  assert.equal(METHODS_V1.includes(STORAGE_STATUS_METHOD_V1), true);
  assert.equal(M5_BRIDGE_V6_METHODS.some((method) => /purge|cleanup|delete/i.test(method)), false);
});

test("D1 freezes exact product and agent-fixture storage.status capability sets", () => {
  assert.equal(storageStatusD1Manifest.schemaVersion, 1);
  assert.equal(storageStatusD1Manifest.protocolVersion, PROTOCOL_VERSION);
  assert.equal(storageStatusD1Manifest.activation, "D1");
  assert.equal(storageStatusD1Manifest.method, STORAGE_STATUS_METHOD_V1);
  assert.deepEqual(storageStatusD1Manifest.modes.generalProduct.legacyCapabilities, [...G5_CAPABILITIES_V1]);
  assert.deepEqual(storageStatusD1Manifest.modes.generalProduct.d1Capabilities, [...G5D_PRODUCT_CAPABILITIES_V1]);
  assert.deepEqual(storageStatusD1Manifest.modes.agentFixture.legacyCapabilities, [...G5A_CAPABILITIES_V1]);
  assert.deepEqual(storageStatusD1Manifest.modes.agentFixture.d1Capabilities, [...G5D_AGENT_FIXTURE_CAPABILITIES_V1]);
  assert.equal(storageStatusD1Manifest.compatibility.legacyExactSetsRemainValid, true);
  assert.equal(storageStatusD1Manifest.compatibility.d1AddsStorageStatusAtTail, true);
  assert.equal(storageStatusD1Manifest.compatibility.runStartAgentSmokeAdvertised, false);

  const ready = (capabilities: readonly string[]) => ({
    protocolVersion: 1,
    sidecarSchemaVersion: 1,
    appSchemaVersion: 1,
    pid: 4242,
    platform: "darwin",
    capabilities: [...capabilities],
    sidecarInstanceId: "sidecar_storage_status_d1",
  });
  const malformedReadyCases = (capabilities: readonly string[]) => {
    const swapped = [...capabilities];
    const last = swapped.length - 1;
    [swapped[last - 1], swapped[last]] = [swapped[last], swapped[last - 1]];

    return [
      ["capability order swap", ready(swapped)],
      ["duplicate storage.status", ready([...capabilities, STORAGE_STATUS_METHOD_V1])],
      ["unknown extra capability", ready([...capabilities, "storage.unknown"])],
      ["missing storage.status", ready(capabilities.slice(0, last))],
      ["extra ready payload field", { ...ready(capabilities), extra: true }],
    ] as const;
  };
  assert.doesNotThrow(() => parseG5ReadyPayloadV1(ready(G5_CAPABILITIES_V1)));
  assert.doesNotThrow(() => parsePlannedG5AReadyPayloadV1(ready(G5A_CAPABILITIES_V1)));
  assert.doesNotThrow(() => parseG5DProductReadyPayloadV1(ready(G5D_PRODUCT_CAPABILITIES_V1)));
  assert.doesNotThrow(() => parseG5DAgentFixtureReadyPayloadV1(ready(G5D_AGENT_FIXTURE_CAPABILITIES_V1)));
  assert.throws(() => parseG5ReadyPayloadV1(ready(G5D_PRODUCT_CAPABILITIES_V1)));
  assert.throws(() => parsePlannedG5AReadyPayloadV1(ready(G5D_AGENT_FIXTURE_CAPABILITIES_V1)));
  assert.throws(() => parseG5DProductReadyPayloadV1(ready(G5_CAPABILITIES_V1)));
  assert.throws(() => parseG5DAgentFixtureReadyPayloadV1(ready(G5A_CAPABILITIES_V1)));

  for (const [name, payload] of malformedReadyCases(G5D_PRODUCT_CAPABILITIES_V1)) {
    assert.throws(() => parseG5DProductReadyPayloadV1(payload), undefined, `product: ${name}`);
  }
  for (const [name, payload] of malformedReadyCases(G5D_AGENT_FIXTURE_CAPABILITIES_V1)) {
    assert.throws(() => parseG5DAgentFixtureReadyPayloadV1(payload), undefined, `agent fixture: ${name}`);
  }

  for (const version of [1, 2, 3, 4, 5]) {
    const source = readFileSync(resolve(`src/m5/contracts/bridge-v${version}.ts`), "utf8");
    assert.equal(source.includes("getStorageStatus"), false, `bridge V${version}`);
  }
  assert.equal(
    readFileSync(resolve("src/m5/contracts/bridge-v6.ts"), "utf8").includes("getStorageStatus"),
    true,
  );
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

function createExactStorageV5Database(path: string, populated: boolean): DatabaseSync {
  const database = new DatabaseSync(path, { enableForeignKeyConstraints: false });
  database.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
  database.exec(readFileSync(resolve(rootV3, manifestV3.migration.predecessorMigrationFile), "utf8"));
  if (populated) seedPopulatedStorageV2(database);
  database.exec(readFileSync(resolve(rootV3, manifestV3.migration.file), "utf8"));
  database.exec(readFileSync(resolve(rootV4, manifestV4.migration.file), "utf8"));
  database.exec(readFileSync(resolve(rootV5, manifestV5.migration.file), "utf8"));
  return database;
}

function applyExactStorageV6Migration(
  database: DatabaseSync,
  migrationSql: string,
  expectedV5Schema: string,
  expectedV6Schema: string | null,
): void {
  const meta = database.prepare(`
    SELECT storage_schema_version, materialization_state
    FROM storage_schema_meta WHERE singleton = 1
  `).get();
  if (meta?.storage_schema_version === 6) {
    if (expectedV6Schema === null || schemaSignature(database) !== expectedV6Schema) {
      throw new Error("storage-v6 schema is not exact");
    }
    return;
  }
  if (meta?.storage_schema_version !== 5 || schemaSignature(database) !== expectedV5Schema) {
    throw new Error("storage-v5 schema is not exact");
  }
  validateStorageLifetimeMaterializationPairV3(readV5Pair(database));
  database.exec(migrationSql);
  if (expectedV6Schema !== null && schemaSignature(database) !== expectedV6Schema) {
    throw new Error("storage-v6 migration did not produce exact schema");
  }
}

function applyExactStorageV7Migration(
  database: DatabaseSync,
  migrationSql: string,
  expectedV6Schema: string,
  expectedV7Schema: string | null,
): void {
  const meta = database.prepare(`
    SELECT storage_schema_version FROM storage_schema_meta WHERE singleton = 1
  `).get();
  if (meta?.storage_schema_version === 7) {
    if (expectedV7Schema === null || schemaSignature(database) !== expectedV7Schema) {
      throw new Error("storage-v7 schema is not exact");
    }
    return;
  }
  if (meta?.storage_schema_version !== 6 || schemaSignature(database) !== expectedV6Schema) {
    throw new Error("storage-v6 schema is not exact");
  }
  validateStorageLifetimeMaterializationPairV4(readV6Pair(database));
  if (database.prepare(`
    SELECT 1 FROM lifetime_materialization_job WHERE singleton=1 AND state='attempting'
  `).get() !== undefined) {
    throw new Error("storage-v7 migration refuses attempting materialization");
  }
  const activeMaintenance = database.prepare(`
    SELECT 1 FROM maintenance_jobs
    WHERE job_kind='losslessCompaction' AND state IN ('pending','running') LIMIT 1
  `).get();
  if (activeMaintenance !== undefined) {
    throw new Error("storage-v7 migration refuses active maintenance");
  }
  database.exec(migrationSql);
  if (expectedV7Schema !== null && schemaSignature(database) !== expectedV7Schema) {
    throw new Error("storage-v7 migration did not produce exact schema");
  }
}

function applyExactStorageV8Migration(
  database: DatabaseSync,
  migrationSql: string,
  expectedV7Schema: string,
  expectedV8Schema: string | null,
): void {
  const meta = database.prepare(`
    SELECT storage_schema_version FROM storage_schema_meta WHERE singleton = 1
  `).get();
  if (meta?.storage_schema_version === 8) {
    if (expectedV8Schema === null || schemaSignature(database) !== expectedV8Schema) {
      throw new Error("storage-v8 schema is not exact");
    }
    return;
  }
  if (meta?.storage_schema_version !== 7 || schemaSignature(database) !== expectedV7Schema) {
    throw new Error("storage-v7 schema is not exact");
  }
  validateStorageLifetimeMaterializationPairV4(readV6Pair(database));
  if (database.prepare(`
    SELECT 1 FROM lifetime_materialization_job WHERE singleton=1 AND state='attempting'
  `).get() !== undefined) {
    throw new Error("storage-v8 migration refuses attempting materialization");
  }
  if (database.prepare(`
    SELECT 1 FROM maintenance_jobs
    WHERE job_kind='losslessCompaction' AND state IN ('pending','running') LIMIT 1
  `).get() !== undefined) {
    throw new Error("storage-v8 migration refuses active maintenance");
  }
  for (const table of v8Cases.migrationMatrix.requiredEmptyV7Tables) {
    if (database.prepare(`SELECT 1 FROM ${table} LIMIT 1`).get() !== undefined) {
      throw new Error(`storage-v8 migration refuses compacted predecessor table ${table}`);
    }
  }
  database.exec(migrationSql);
  if (expectedV8Schema !== null && schemaSignature(database) !== expectedV8Schema) {
    throw new Error("storage-v8 migration did not produce exact schema");
  }
}

function seedV8OperationReceipts(database: DatabaseSync): void {
  const insertReceipt = database.prepare(`
    INSERT OR REPLACE INTO operation_receipts(
      operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,
      payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,
      canonical_payload_digest_version,canonical_payload_digest_format,canonical_payload_digest,
      event_timestamp,materialized_at
    ) VALUES(?,?,?,?,?,?,1,?,'sha256',1,'lowercaseHex64',?,?,?)
  `);
  for (const fixture of v6Cases.sourceIndex.sourceEvents) {
    const event = database.prepare(`
      SELECT event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
             event_type,payload_schema_version,canonical_json_payload,timestamp
      FROM events WHERE rowid=?
    `).get(fixture.eventRowid) as typeof fixture.event | undefined;
    assert.ok(event, `missing receipt source ${fixture.eventRowid}`);
    insertReceipt.run(
      event.operation_id, event.event_id, event.aggregate_type, event.aggregate_id,
      event.aggregate_sequence, event.event_type, event.canonical_json_payload,
      createHash("sha256").update(event.canonical_json_payload).digest("hex"),
      event.timestamp, "2026-09-01T09:59:59Z",
    );
  }
}

function frozenOperationForEvent(
  eventRowid: number,
  database?: DatabaseSync,
  frozenAt = "2026-09-01T10:00:00Z",
) {
  const fixture = v6Cases.sourceIndex.sourceEvents.find((entry) => entry.eventRowid === eventRowid);
  assert.ok(fixture, `missing source event ${eventRowid}`);
  const event = (database?.prepare(`
    SELECT event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
           event_type,payload_schema_version,canonical_json_payload,timestamp
    FROM events WHERE rowid=?
  `).get(eventRowid) ?? fixture.event) as typeof fixture.event;
  const source = {
    operationId: event.operation_id,
    sourceEventRowid: eventRowid,
    sourceEventId: event.event_id,
    requestId: event.request_id,
    aggregateType: event.aggregate_type as "project" | "chat" | "run" | "lock",
    aggregateId: event.aggregate_id,
    aggregateSequence: event.aggregate_sequence,
    eventType: event.event_type,
    payloadSchemaVersion: 1 as const,
    canonicalPayloadJson: event.canonical_json_payload,
    canonicalPayloadDigest: createHash("sha256")
      .update(event.canonical_json_payload).digest("hex"),
    eventTimestamp: event.timestamp,
    canonicalRecordJson: "",
    recordDigest: "",
    frozenAt,
  };
  source.canonicalRecordJson = canonicalFrozenOperationRecordV1(source);
  source.recordDigest = createHash("sha256").update(source.canonicalRecordJson).digest("hex");
  return source;
}

function insertFrozenOperation(database: DatabaseSync, source: ReturnType<typeof frozenOperationForEvent>) {
  validateFrozenOperationSourceV1(source);
  database.prepare(`
    INSERT INTO frozen_operation_sources(
      operation_id,source_event_rowid,source_event_id,request_id,aggregate_type,aggregate_id,
      aggregate_sequence,event_type,payload_schema_version,canonical_payload_json,
      canonical_payload_digest,event_timestamp,canonical_record_json,record_digest,frozen_at
    ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
  `).run(
    source.operationId, source.sourceEventRowid, source.sourceEventId, source.requestId,
    source.aggregateType, source.aggregateId, source.aggregateSequence, source.eventType,
    source.payloadSchemaVersion, source.canonicalPayloadJson, source.canonicalPayloadDigest,
    source.eventTimestamp, source.canonicalRecordJson, source.recordDigest, source.frozenAt,
  );
}

function seedStorageV6CompactionFixture(
  database: DatabaseSync,
  canonicalizePayloads = false,
): void {
  database.prepare(`
    UPDATE storage_schema_meta SET materialization_state='ready' WHERE singleton=1
  `).run();
  database.prepare(`
    INSERT INTO projects(project_id,canonical_path,projection_json,last_event_sequence)
    VALUES('project_1','/fixture/v7','{"projectId":"project_1"}',2)
  `).run();
  database.prepare(`
    INSERT INTO chats(chat_id,project_id,projection_json,last_event_sequence)
    VALUES('chat_1','project_1','{"chatId":"chat_1"}',2)
  `).run();
  database.prepare(`
    INSERT INTO app_instances(
      sidecar_instance_id,pid,platform,started_at,heartbeat_at,clean_shutdown_at,last_event_sequence
    ) VALUES('instance_1',123,'macos','2026-09-01T08:00:00Z','2026-09-01T08:01:00Z',NULL,1)
  `).run();
  const insertEvent = database.prepare(`
    INSERT INTO events(
      event_id,operation_id,request_id,aggregate_type,aggregate_id,aggregate_sequence,
      event_type,payload_schema_version,canonical_json_payload,timestamp
    ) VALUES(?,?,?,?,?,?,?,?,?,?)
  `);
  for (const fixture of v6Cases.sourceIndex.sourceEvents) {
    const event = fixture.event;
    const payload = canonicalizePayloads
      ? canonicalJsonV1(JSON.parse(event.canonical_json_payload))
      : event.canonical_json_payload;
    insertEvent.run(
      event.event_id, event.operation_id, event.request_id, event.aggregate_type,
      event.aggregate_id, event.aggregate_sequence, event.event_type,
      event.payload_schema_version, payload, event.timestamp,
    );
  }
  insertEvent.run(
    "evt_instance_1", "op_instance_1", null, "app_instance", "instance_1", 1,
    "app_instance.started", 1, "{\"sidecar_instance_id\":\"instance_1\"}",
    "2026-09-01T08:00:00Z",
  );
  const insertIndex = database.prepare(`
    INSERT INTO event_source_index(
      event_rowid,event_id,operation_id,aggregate_type,aggregate_id,aggregate_sequence,
      introduced_message_id,chat_durable_sequence,chat_snapshot_message_id
    ) VALUES(?,?,?,?,?,?,?,?,?)
  `);
  insertIndex.run(1, "evt_project_1", "op_project_1", "project", "project_1", 1, null, null, null);
  insertIndex.run(2, "evt_chat_1", "op_chat_1", "chat", "chat_1", 1, "message_a", 1, "message_a");
  insertIndex.run(3, "evt_project_2", "op_project_2", "project", "project_1", 2, null, null, null);
  insertIndex.run(4, "evt_chat_2", "op_chat_2", "chat", "chat_1", 2, null, 1, "message_a");
  insertIndex.run(5, "evt_chat_3", "op_chat_3", "chat", "chat_1", 3, null, 1, null);
  insertIndex.run(6, "evt_chat_4", "op_chat_4", "chat", "chat_1", 4, "message_b", 2, "message_b");
  insertIndex.run(7, "evt_instance_1", "op_instance_1", "app_instance", "instance_1", 1, null, null, null);
  database.prepare(`
    INSERT INTO chat_messages(
      message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at
    ) VALUES(?,?,?,?,?,?,?)
  `).run(
    "message_a", "chat_1", 1, '{"messageId":"message_a"}', "digest-message-a",
    "2026-09-01T08:01:00Z", "2026-09-01T08:01:01Z",
  );
  database.prepare(`
    INSERT INTO chat_messages(
      message_id,chat_id,event_sequence,canonical_message_json,message_digest,created_at,materialized_at
    ) VALUES(?,?,?,?,?,?,?)
  `).run(
    "message_b", "chat_1", 2, '{"messageId":"message_b"}', "digest-message-b",
    "2026-09-01T08:02:00Z", "2026-09-01T08:02:01Z",
  );
  const insertBaseline = database.prepare(`
    INSERT INTO ledger_baselines(
      baseline_id,aggregate_type,aggregate_id,cutoff_aggregate_sequence,
      payload_schema_version,canonical_projection_json,projection_digest,created_at,verified_at
    ) VALUES(?,?,?,?,1,?,?,?,?)
  `);
  insertBaseline.run(
    "baseline_chat_1_1", "chat", "chat_1", 1, '{"chatId":"chat_1"}',
    "digest-chat-1", "2026-09-01T08:03:00Z", "2026-09-01T08:03:01Z",
  );
  insertBaseline.run(
    "baseline_project_1_1", "project", "project_1", 1, '{"projectId":"project_1"}',
    "digest-project-1", "2026-09-01T08:03:00Z", "2026-09-01T08:03:01Z",
  );
  database.prepare(`
    INSERT INTO operation_receipts(
      operation_id,event_id,aggregate_type,aggregate_id,aggregate_sequence,event_type,
      payload_schema_version,canonical_payload_json,canonical_payload_digest_algorithm,
      canonical_payload_digest_version,canonical_payload_digest_format,canonical_payload_digest,
      event_timestamp,materialized_at
    ) VALUES(?,?,?,?,?,?,1,?,'sha256',1,'lowercaseHex64',?,?,?)
  `).run(
    "op_chat_1", "evt_chat_1", "chat", "chat_1", 1, "chat.upserted",
    '{"operation":"chat-one"}', "0".repeat(64),
    "2026-09-01T08:01:00Z", "2026-09-01T08:01:01Z",
  );
}

function insertV5LifetimeJob(
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
    `lifetime-v5-${state}-${phase}`,
    state,
    phase,
    afterScanRowid,
    scanExhausted ? 1 : 0,
    state === "failed" ? "contractValidation" : null,
    "2026-08-28T13:00:00Z",
    state === "failed" ? "2026-08-28T13:02:00Z" : "2026-08-28T13:01:00Z",
    state === "failed" ? "2026-08-28T13:02:00Z" : null,
  );
}

function readV6Pair(database: DatabaseSync): unknown {
  const meta = database.prepare(`
    SELECT materialization_state FROM storage_schema_meta WHERE singleton = 1
  `).get();
  const row = database.prepare(`
    SELECT singleton, job_id, state, cutoff_event_rowid, phase, after_scan_rowid,
           scan_exhausted, failure_kind, created_at, updated_at, completed_at
    FROM lifetime_materialization_job WHERE singleton = 1
  `).get();
  return {
    materializationState: meta?.materialization_state,
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
  };
}

function v6Snapshot(
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
      jobId: "lifetime-v6",
      state,
      cutoffEventRowid,
      phase,
      afterScanRowid,
      scanExhausted,
      failureKind: state === "failed" ? "contractValidation" : null,
      createdAt: "2026-08-28T13:00:00Z",
      updatedAt: state === "failed" ? "2026-08-28T13:02:00Z" : "2026-08-28T13:01:00Z",
      completedAt: state === "failed" ? "2026-08-28T13:02:00Z" : null,
    },
  };
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

function readV5Pair(database: DatabaseSync): unknown {
  const meta = database.prepare(`
    SELECT materialization_state FROM storage_schema_meta WHERE singleton = 1
  `).get();
  const row = database.prepare(`
    SELECT singleton, job_id, state, cutoff_event_rowid, phase, after_scan_rowid,
           scan_exhausted, failure_kind, created_at, updated_at, completed_at
    FROM lifetime_materialization_job WHERE singleton = 1
  `).get();
  return {
    materializationState: meta?.materialization_state,
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
  };
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
