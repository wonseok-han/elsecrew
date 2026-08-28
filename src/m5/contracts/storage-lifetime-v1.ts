import { requireObject, exactKeys, requireSafeUnsigned } from "./project-v1.ts";
import { sha256FileVersionV1 } from "./file-v1.ts";
import { canonicalJsonV1 } from "./storage-v1.ts";

// Kept at v2 until the sidecar consumer implements the contract-only v3 migration.
export const STORAGE_LIFETIME_SCHEMA_VERSION_V1 = 2 as const;
export const STORAGE_LIFETIME_SCHEMA_VERSION_V3 = 3 as const;
export const STORAGE_STATUS_SCHEMA_VERSION_V1 = 1 as const;
export const STORAGE_STATUS_METHOD_V1 = "storage.status" as const;
export const STORAGE_STATUS_BRIDGE_VERSION_V1 = 6 as const;
export const STORAGE_STATUS_BRIDGE_METHOD_V1 = "getStorageStatus" as const;
export const STORAGE_LIFETIME_TABLES_V2 = [
  "storage_schema_meta",
  "ledger_baselines",
  "operation_receipts",
  "chat_messages",
  "prepared_workspaces",
  "maintenance_jobs",
] as const;
export const STORAGE_LIFETIME_INDEXES_V2 = [
  "chats_identity_project",
  "ledger_baselines_cutoff",
  "operation_receipts_aggregate_sequence",
  "chat_messages_chat_sequence",
  "prepared_workspaces_project_state",
  "maintenance_jobs_status_created",
] as const;
export const STORAGE_LIFETIME_TABLES_V3 = [
  ...STORAGE_LIFETIME_TABLES_V2,
  "lifetime_materialization_job",
] as const;
export const STORAGE_LIFETIME_SCHEMA_VERSION_V4 = 4 as const;
export const STORAGE_LIFETIME_SCHEMA_VERSION_V5 = 5 as const;
export const STORAGE_LIFETIME_SCHEMA_VERSION_V6 = 6 as const;
export const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2 = 2 as const;
export const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V2 = 2 as const;
export const STORAGE_LIFETIME_PHASES_V2 = [
  "receipts",
  "messages",
  "workspaces",
  "baselines",
  "verifySource",
  "verifyReceipts",
  "verifyMessages",
  "verifyWorkspaces",
  "verifyBaselines",
] as const;
export const STORAGE_LIFETIME_EVENT_ROWID_PHASES_V2 = [
  "receipts", "messages", "workspaces", "baselines", "verifySource",
] as const;
export const STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V2 = [
  "verifyReceipts", "verifyMessages", "verifyWorkspaces", "verifyBaselines",
] as const;
export const STORAGE_LIFETIME_JOB_STATES_V2 = ["pending", "attempting", "failed"] as const;
export const STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2 = "LIMIT 100" as const;
export const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3 = 3 as const;
export const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V3 = 3 as const;
export const STORAGE_LIFETIME_PHASES_V3 = [
  "receipts", "messages", "workspaces", "baselines", "verifySource",
  "verifyProjects", "verifyChats", "verifyRuns", "verifyLocks", "verifyAppInstances",
  "verifyReceipts", "verifyMessages", "verifyWorkspaces", "verifyBaselines",
] as const;
export const STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3 = [
  "receipts", "messages", "workspaces", "baselines", "verifySource",
] as const;
export const STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V3 = [
  "verifyProjects", "verifyChats", "verifyRuns", "verifyLocks", "verifyAppInstances",
] as const;
export const STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V3 = [
  "verifyReceipts", "verifyMessages", "verifyWorkspaces", "verifyBaselines",
] as const;
export const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4 = 4 as const;
export const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V4 = 4 as const;
export const STORAGE_LIFETIME_PHASES_V4 = [
  "indexSource", "verifySourceIndex",
  "receipts", "messages", "workspaces", "baselines", "verifySource",
  "verifyProjects", "verifyChats", "verifyRuns", "verifyLocks", "verifyAppInstances",
  "verifyReceipts", "verifyMessages", "verifyWorkspaces", "verifyBaselines",
] as const;
export const STORAGE_LIFETIME_EVENT_ROWID_PHASES_V4 = [
  "indexSource", "verifySourceIndex",
  "receipts", "messages", "workspaces", "baselines", "verifySource",
] as const;
export const STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V4 = [
  "verifyProjects", "verifyChats", "verifyRuns", "verifyLocks", "verifyAppInstances",
] as const;
export const STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V4 = [
  "verifyReceipts", "verifyMessages", "verifyWorkspaces", "verifyBaselines",
] as const;
export const STORAGE_EVENT_SOURCE_INDEX_COLUMNS_V1 = [
  "event_rowid", "event_id", "operation_id", "aggregate_type", "aggregate_id",
  "aggregate_sequence", "message_id",
] as const;
export const OPERATION_RECEIPT_DIGEST_ALGORITHM_V2 = "sha256" as const;
export const OPERATION_RECEIPT_DIGEST_VERSION_V2 = 1 as const;
export const OPERATION_RECEIPT_DIGEST_FORMAT_V2 = "lowercaseHex64" as const;
export const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1 = 1 as const;
export const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1 = 1 as const;
export const STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1 = 100 as const;
export const STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 = 1_048_576 as const;
export const STORAGE_LIFETIME_KEYSET_SCAN_PREDICATE_V1 =
  "rowid > afterEventRowid AND rowid <= cutoffEventRowid" as const;
export const STORAGE_LIFETIME_KEYSET_SCAN_ORDER_V1 = "rowid" as const;
export const STORAGE_LIFETIME_PHASES_V1 = [
  "receipts",
  "messages",
  "workspaces",
  "baselines",
  "verify",
] as const;
export const STORAGE_LIFETIME_JOB_STATES_V1 = ["pending", "failed"] as const;
export const STORAGE_MATERIALIZATION_STATES_V1 = [
  "pending",
  "materializing",
  "ready",
  "failed",
] as const;
export const STORAGE_LIFETIME_V3_PREFLIGHT_ALLOWED_STATES_V1 = ["pending", "ready"] as const;
export const STORAGE_LIFETIME_V3_PREFLIGHT_REJECTED_STATES_V1 = [
  "materializing",
  "failed",
] as const;
export const LOSSLESS_COMPACTION_STEPS_V1 = [
  "beginImmediate",
  "freezeCutoff",
  "materializeOperationReceipts",
  "materializeChatMessagesAndPreparedWorkspaces",
  "materializeLedgerBaselines",
  "verifyBaselineTailProjectionEquivalent",
  "verifyOperationDuplicateConflictEquivalent",
  "verifyNamespaceAndRecoveryEquivalent",
  "deleteVerifiedLedgerCutoff",
  "verifyAppendOnlyGuardRestored",
  "commit",
  "checkpointBestEffort",
] as const;
export const NEVER_CLEANUP_STATES_V1 = [
  "activeRun",
  "cancellingRun",
  "recoveryRequiredRun",
  "unresolvedRecovery",
  "poisonedLock",
  "writerLock",
  "pendingIntent",
  "pendingSaveSession",
  "incompleteOperation",
  "activeWorkspace",
  "boundWorkspace",
  "roleSettings",
  "userChatHistory",
  "userEvidence",
  "identityMismatch",
  "orphanOrSymlinkUnknown",
] as const;

export type StoragePressureCollectionStatusV1 = "available" | "unavailable";
export type StorageCompactionAttemptStatusV1 = "neverRun" | "succeeded" | "failed";
export type StorageLifetimePhaseV1 = typeof STORAGE_LIFETIME_PHASES_V1[number];
export type StorageLifetimeJobStateV1 = typeof STORAGE_LIFETIME_JOB_STATES_V1[number];
export type StorageLifetimePhaseV2 = typeof STORAGE_LIFETIME_PHASES_V2[number];
export type StorageLifetimeJobStateV2 = typeof STORAGE_LIFETIME_JOB_STATES_V2[number];
export type StorageLifetimePhaseV3 = typeof STORAGE_LIFETIME_PHASES_V3[number];
export type StorageLifetimePhaseV4 = typeof STORAGE_LIFETIME_PHASES_V4[number];
export type StorageMaterializationStateV1 = typeof STORAGE_MATERIALIZATION_STATES_V1[number];
export type StorageLifetimeLifecycleOperationV1 =
  | "create"
  | "commitBatch"
  | "cancel"
  | "resume"
  | "fail"
  | "complete"
  | "append"
  | "catchUpReset";
export type StorageLifetimeLifecycleOperationV2 =
  | "create"
  | "arm"
  | "batchSuccess"
  | "phaseAdvance"
  | "cancel"
  | "fail"
  | "failureRecordUnavailable"
  | "finalPublish"
  | "finalReset"
  | "append";
export type StorageLifetimeLifecycleOperationV3 =
  | "create"
  | "arm"
  | "batchSuccess"
  | "phaseAdvance"
  | "cancel"
  | "fail"
  | "failureRecordUnavailable"
  | "finalPublish"
  | "catchUpReset"
  | "append";
export type StorageLifetimeLifecycleOperationV4 = StorageLifetimeLifecycleOperationV3;

export interface StorageLifetimeCursorV1 {
  schemaVersion: typeof STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV1;
  afterEventRowid: number;
}

export interface StorageLifetimeMaterializationJobV1 {
  singleton: 1;
  jobId: string;
  state: StorageLifetimeJobStateV1;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV1;
  afterEventRowid: number;
  failureKind: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface StorageLifetimeMaterializationSnapshotV1 {
  materializationState: StorageMaterializationStateV1;
  job: StorageLifetimeMaterializationJobV1 | null;
}

export interface StorageLifetimeCursorV2 {
  schemaVersion: typeof STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV2;
  afterScanRowid: number;
  scanExhausted: boolean;
}

export interface StorageLifetimeMaterializationJobV2 {
  singleton: 1;
  jobId: string;
  state: StorageLifetimeJobStateV2;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV2;
  afterScanRowid: number;
  scanExhausted: boolean;
  failureKind: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface StorageLifetimeMaterializationSnapshotV2 {
  materializationState: StorageMaterializationStateV1;
  job: StorageLifetimeMaterializationJobV2 | null;
}

export interface StorageLifetimeCursorV3 {
  schemaVersion: typeof STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV3;
  afterScanRowid: number;
  scanExhausted: boolean;
}

export interface StorageLifetimeMaterializationJobV3 {
  singleton: 1;
  jobId: string;
  state: StorageLifetimeJobStateV2;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV3;
  afterScanRowid: number;
  scanExhausted: boolean;
  failureKind: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface StorageLifetimeMaterializationSnapshotV3 {
  materializationState: StorageMaterializationStateV1;
  job: StorageLifetimeMaterializationJobV3 | null;
}

export interface StorageLifetimeCursorV4 {
  schemaVersion: typeof STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV4;
  afterScanRowid: number;
  scanExhausted: boolean;
}

export interface StorageLifetimeMaterializationJobV4 {
  singleton: 1;
  jobId: string;
  state: StorageLifetimeJobStateV2;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV4;
  afterScanRowid: number;
  scanExhausted: boolean;
  failureKind: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface StorageLifetimeMaterializationSnapshotV4 {
  materializationState: StorageMaterializationStateV1;
  job: StorageLifetimeMaterializationJobV4 | null;
}

export interface StorageEventSourceIndexEntryV1 {
  eventRowid: number;
  eventId: string;
  operationId: string;
  aggregateType: string;
  aggregateId: string;
  aggregateSequence: number;
  messageId: string | null;
}

export interface StorageLifetimeV3PreflightPolicyV1 {
  schema: "exactStorageV2";
  allowedMaterializationStates: readonly ["pending", "ready"];
  rejectedMaterializationStates: readonly ["materializing", "failed"];
  timing: "beforeAnyDdlOrMutation";
}

export interface StorageLifetimeV4PreflightPolicyV1 {
  schema: "exactStorageV3";
  admissiblePairs: readonly [
    "ready+absent", "pending+absent", "pending+pending", "pending+failed",
  ];
  migrationMappings: readonly [
    "pendingPhasesSameName", "verifyToVerifySource", "afterEventRowidToAfterScanRowid",
    "scanExhaustedFalse", "failedPreserved", "attemptingImpossibleInV3",
  ];
  reopenRunnablePairs: readonly ["ready+absent", "pending+absent", "pending+pending"];
  reopenFailClosedPairs: readonly ["pending+attempting", "pending+failed"];
  timing: "beforeAnyDdlOrMutation";
}

export interface StorageLifetimeV5PreflightPolicyV1 {
  schema: "exactStorageV4";
  admissiblePairs: readonly [
    "ready+absent", "pending+absent", "pending+pending", "pending+attempting",
    "pending+failed",
  ];
  migrationMappings: readonly [
    "materializationStateAndAbsentJobPreserved",
    "receiptsThroughVerifySourcePositionPreserved",
    "legacyDerivedVerificationRestartsVerifySourceAtZeroUnexhausted",
    "pendingAttemptingFailedStatePreserved",
    "failureEvidencePreserved",
  ];
  reopenRunnablePairs: readonly ["ready+absent", "pending+absent", "pending+pending"];
  reopenFailClosedPairs: readonly ["pending+attempting", "pending+failed"];
  timing: "beforeAnyDdlOrMutation";
}

export interface StorageLifetimeV6PreflightPolicyV1 {
  schema: "exactStorageV5";
  admissiblePairs: readonly [
    "ready+absent", "pending+absent", "pending+pending", "pending+attempting",
    "pending+failed",
  ];
  migrationMappings: readonly [
    "readyAbsentBecomesPendingAbsent",
    "pendingAbsentPreserved",
    "everyPresentJobRestartsIndexSourceAtZeroUnexhausted",
    "pendingAttemptingFailedStatePreserved",
    "jobIdentityAndTimestampsPreserved",
    "failureEvidencePreserved",
    "existingEventHistoryNotBackfilledByMigration",
  ];
  reopenRunnablePairs: readonly ["ready+absent", "pending+absent", "pending+pending"];
  reopenFailClosedPairs: readonly ["pending+attempting", "pending+failed"];
  timing: "beforeAnyDdlOrMutation";
}

export interface StorageLifetimeBatchPolicyV1 {
  schemaVersion: typeof STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1;
  casesFile: "batching-cases.json";
  guards: {
    maxSourceRowsPerBatch: typeof STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1;
    maxCanonicalSourcePayloadBytesPerBatch:
      typeof STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1;
    singleSourcePayloadOversize: "contractError";
    callerConfigurable: false;
    appliesToPhases: readonly ["receipts", "messages", "workspaces", "baselines", "verify"];
  };
  cursor: {
    schemaVersion: typeof STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1;
    phases: readonly StorageLifetimePhaseV1[];
    scanPhases: readonly ["receipts", "messages", "workspaces", "baselines", "verify"];
    phaseStartAfterEventRowid: 0;
    samePhaseProgress: "strictlyIncreasingWithinFrozenCutoff";
    phaseTransition: "currentAtCutoffThenNextPhaseAfterZero";
  };
  scan: {
    predicate: typeof STORAGE_LIFETIME_KEYSET_SCAN_PREDICATE_V1;
    orderBy: typeof STORAGE_LIFETIME_KEYSET_SCAN_ORDER_V1;
    offsetAllowed: false;
  };
  source: {
    artifactAuthority: "eventsAtOrBelowFrozenCutoffOnly";
    workspaceAndBaselineSource: "eventProjectionSnapshotsAtOrBelowFrozenCutoff";
    repeatedProjectionSnapshots: "idempotent";
    liveProjectionUse: "finalCatchUpEquivalenceOnlyAfterMaxEventRowidEqualsCutoff";
    unboundedCollectionAllowed: false;
  };
  transaction: {
    cadence: "oneBatchOneBeginImmediatePerIdleTurn";
    committedDerivedRowsMayPersist: true;
    committedProgressState: "pending";
    implementationMayCommitRunning: false;
  };
  cancellation: {
    beforeBegin: "noMutation";
    checks: readonly ["beforeEachSourceRow", "afterEachSourceRow", "immediatelyPrecommit"];
    duringTransaction: "rollbackCurrentBatch";
    outcome: "Suspended";
    crash: "rollbackCurrentBatchResumePrecedingCommittedPendingCursor";
    committedBatchWinsAfterFinalPrecommitCheck: true;
  };
  job: {
    table: "lifetime_materialization_job";
    maximumRows: 1;
    singletonValue: 1;
    uniqueJobId: true;
    states: readonly ["pending", "failed"];
    cursorColumns: readonly ["cutoff_event_rowid", "phase", "after_event_rowid"];
    maintenanceJobsRole: "losslessCompactionOnlyUnchanged";
    singleSqliteOwnerRequired: true;
    transactionSerialization: "beginImmediate";
    transactionBoundaryValidation: "exactMetaJobPairAndValidSingletonCursor";
    batchCommit: "derivedRowsAndAdvancedPendingSingletonAtomic";
    cancellation: "rollbackLeavesSamePendingSingletonAndCursor";
    terminalFailure: "sameSingletonPendingToFailedPreserveCursorAfterRollback";
    terminalFailureMetaState: "pending";
    failureRecordUnavailable: "failClosedRestartRevalidatesBeforeMutationNeverSuccess";
    automaticRetryEnabled: false;
    completion: "deleteSingletonAndSetMetaReadySameTransaction";
    appendAfterReady: "setMetaPendingKeepJobAbsentSameAppendTransaction";
    appendDuringPending: "doNotAlterFrozenSingletonCursor";
    catchUp: "samePendingSingletonResetToNewCutoffReceiptsZero";
    schedulerCreationImplemented: false;
    forbiddenCommittedStates: readonly ["running", "succeeded", "cancelled"];
  };
  pairing: {
    canonicalReadinessAuthority: "storage_schema_meta.materialization_state";
    validPairs: readonly [
      "pending+absent",
      "pending+pending",
      "pending+failed",
      "ready+absent",
    ];
    failClosedPairs: readonly ["materializing+any", "failed+any", "ready+present"];
  };
  append: {
    readyInvalidation: "pendingInSameAppendTransaction";
    readyPublication: "separateAtomicCatchUpVerificationTransaction";
    appendDuringMaterialization: "frozenCutoffUnchangedUntilFinalCatchUp";
    readyRegressionSequence: readonly [
      "materializeReady",
      "appendAndInvalidateReadyToPendingSameTransaction",
      "separateCatchUpVerification",
      "publishReady",
    ];
  };
  finalPublication: {
    transaction: "separateBeginImmediate";
    prerequisites: readonly [
      "maxEventRowidEqualsCutoff",
      "exactDerivedCompletenessAndEquivalence",
      "liveProjectionEqualsCutoffReconstruction",
      "boundedSqlKeysetEvidence",
    ];
    maxRowidChanged: "remainPendingResetSameSingletonAtObservedCutoffNeverPublishReady";
    unboundedCollectionAllowed: false;
  };
  rollback: {
    derivedPreparationBatchesMayPersist: true;
    sourceLedgerMutationAtomicRollbackRequired: true;
    compactionTriggerChangeAtomicRollbackRequired: true;
  };
  activation: {
    ledgerDeletionEnabled: false;
    automaticSchedulingEnabled: false;
    compactionImplementationEnabled: false;
    statusWireEnabled: false;
  };
}

export interface StorageStatusRequestV1 {
  schemaVersion: typeof STORAGE_STATUS_SCHEMA_VERSION_V1;
}

export interface StorageStatusV1 {
  schemaVersion: typeof STORAGE_STATUS_SCHEMA_VERSION_V1;
  collectedAt: string;
  database: {
    logicalBytes: number;
    physicalBytes: number;
    walBytes: number;
  };
  ledgerTail: {
    eventCount: number;
    payloadBytes: number;
  };
  snapshotAggregates: {
    count: number;
    payloadBytes: number;
  };
  pressureFacts: {
    collectionStatus: StoragePressureCollectionStatusV1;
    volumeCapacityBytes: number | null;
    volumeAvailableBytes: number | null;
  };
  compaction: {
    lastAttemptStatus: StorageCompactionAttemptStatusV1;
    lastAttemptedAt: string | null;
    lastSuccessfulAt: string | null;
  };
  physicalCleanup: {
    enabled: false;
    reason: "userPolicyNotConfigured";
  };
}

export interface OperationReceiptV2 {
  operationId: string;
  eventId: string;
  aggregateType: string;
  aggregateId: string;
  aggregateSequence: number;
  eventType: string;
  payloadSchemaVersion: 1;
  canonicalPayloadJson: string;
  canonicalPayloadDigestAlgorithm: typeof OPERATION_RECEIPT_DIGEST_ALGORITHM_V2;
  canonicalPayloadDigestVersion: typeof OPERATION_RECEIPT_DIGEST_VERSION_V2;
  canonicalPayloadDigestFormat: typeof OPERATION_RECEIPT_DIGEST_FORMAT_V2;
  canonicalPayloadDigest: string;
  eventTimestamp: string;
}

export interface OperationCandidateV1 {
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
}

export type ReceiptOperationDecisionV2 = "append" | "duplicateNoOp" | "conflict";

export function validateStorageLifetimeV3PreflightPolicyV1(
  value: unknown,
): StorageLifetimeV3PreflightPolicyV1 {
  const policy = requireObject(value, "storage lifetime v3 preflight policy");
  exactKeys(policy, [
    "schema", "allowedMaterializationStates", "rejectedMaterializationStates", "timing",
  ]);
  if (policy.schema !== "exactStorageV2"
    || !exactStringArray(
      policy.allowedMaterializationStates,
      STORAGE_LIFETIME_V3_PREFLIGHT_ALLOWED_STATES_V1,
    )
    || !exactStringArray(
      policy.rejectedMaterializationStates,
      STORAGE_LIFETIME_V3_PREFLIGHT_REJECTED_STATES_V1,
    )
    || policy.timing !== "beforeAnyDdlOrMutation") {
    fail("storage lifetime v3 preflight policy is not fail-closed");
  }
  return policy as unknown as StorageLifetimeV3PreflightPolicyV1;
}

export function validateStorageLifetimeV4PreflightPolicyV1(
  value: unknown,
): StorageLifetimeV4PreflightPolicyV1 {
  const policy = requireObject(value, "storage lifetime v4 preflight policy");
  const expected = {
    schema: "exactStorageV3",
    admissiblePairs: ["ready+absent", "pending+absent", "pending+pending", "pending+failed"],
    migrationMappings: [
      "pendingPhasesSameName", "verifyToVerifySource", "afterEventRowidToAfterScanRowid",
      "scanExhaustedFalse", "failedPreserved", "attemptingImpossibleInV3",
    ],
    reopenRunnablePairs: ["ready+absent", "pending+absent", "pending+pending"],
    reopenFailClosedPairs: ["pending+attempting", "pending+failed"],
    timing: "beforeAnyDdlOrMutation",
  };
  if (!sameContractValue(policy, expected)) {
    fail("storage lifetime v4 preflight policy is not fail-closed");
  }
  return policy as unknown as StorageLifetimeV4PreflightPolicyV1;
}

export function validateStorageLifetimeV5PreflightPolicyV1(
  value: unknown,
): StorageLifetimeV5PreflightPolicyV1 {
  const policy = requireObject(value, "storage lifetime v5 preflight policy");
  const expected = {
    schema: "exactStorageV4",
    admissiblePairs: [
      "ready+absent", "pending+absent", "pending+pending", "pending+attempting",
      "pending+failed",
    ],
    migrationMappings: [
      "materializationStateAndAbsentJobPreserved",
      "receiptsThroughVerifySourcePositionPreserved",
      "legacyDerivedVerificationRestartsVerifySourceAtZeroUnexhausted",
      "pendingAttemptingFailedStatePreserved",
      "failureEvidencePreserved",
    ],
    reopenRunnablePairs: ["ready+absent", "pending+absent", "pending+pending"],
    reopenFailClosedPairs: ["pending+attempting", "pending+failed"],
    timing: "beforeAnyDdlOrMutation",
  };
  if (!sameContractValue(policy, expected)) {
    fail("storage lifetime v5 preflight policy is not fail-closed");
  }
  return policy as unknown as StorageLifetimeV5PreflightPolicyV1;
}

export function validateStorageLifetimeV6PreflightPolicyV1(
  value: unknown,
): StorageLifetimeV6PreflightPolicyV1 {
  const policy = requireObject(value, "storage lifetime v6 preflight policy");
  const expected = {
    schema: "exactStorageV5",
    admissiblePairs: [
      "ready+absent", "pending+absent", "pending+pending", "pending+attempting",
      "pending+failed",
    ],
    migrationMappings: [
      "readyAbsentBecomesPendingAbsent",
      "pendingAbsentPreserved",
      "everyPresentJobRestartsIndexSourceAtZeroUnexhausted",
      "pendingAttemptingFailedStatePreserved",
      "jobIdentityAndTimestampsPreserved",
      "failureEvidencePreserved",
      "existingEventHistoryNotBackfilledByMigration",
    ],
    reopenRunnablePairs: ["ready+absent", "pending+absent", "pending+pending"],
    reopenFailClosedPairs: ["pending+attempting", "pending+failed"],
    timing: "beforeAnyDdlOrMutation",
  };
  if (!sameContractValue(policy, expected)) {
    fail("storage lifetime v6 preflight policy is not fail-closed");
  }
  return policy as unknown as StorageLifetimeV6PreflightPolicyV1;
}

export function validateStorageLifetimeCursorV1(value: unknown): StorageLifetimeCursorV1 {
  const cursor = requireObject(value, "storage lifetime cursor");
  exactKeys(cursor, ["schemaVersion", "cutoffEventRowid", "phase", "afterEventRowid"]);
  if (cursor.schemaVersion !== STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1) {
    fail("storage lifetime cursor schemaVersion must be 1");
  }
  const cutoff = requireSafeUnsigned(cursor.cutoffEventRowid, "cursor.cutoffEventRowid");
  const after = requireSafeUnsigned(cursor.afterEventRowid, "cursor.afterEventRowid");
  if (!STORAGE_LIFETIME_PHASES_V1.includes(cursor.phase as StorageLifetimePhaseV1)) {
    fail("storage lifetime cursor phase is invalid");
  }
  if (after > cutoff) {
    fail("storage lifetime cursor afterEventRowid cannot exceed cutoffEventRowid");
  }
  return cursor as unknown as StorageLifetimeCursorV1;
}

export function validateStorageLifetimeCursorTransitionV1(
  previousValue: unknown | null,
  nextValue: unknown,
): StorageLifetimeCursorV1 {
  const next = validateStorageLifetimeCursorV1(nextValue);
  if (previousValue === null) {
    if (next.phase !== "receipts" || next.afterEventRowid !== 0) {
      fail("storage lifetime initial cursor must start receipts at afterEventRowid 0");
    }
    return next;
  }
  const previous = validateStorageLifetimeCursorV1(previousValue);
  if (next.cutoffEventRowid === previous.cutoffEventRowid) {
    if (next.phase === previous.phase) {
      if (next.afterEventRowid <= previous.afterEventRowid) {
        fail("storage lifetime same-phase cursor must advance afterEventRowid");
      }
      return next;
    }
    if (nextPhase(previous.phase) !== next.phase
      || previous.afterEventRowid !== previous.cutoffEventRowid
      || next.afterEventRowid !== 0) {
      fail("storage lifetime phase transition requires completed current phase and next phase at afterEventRowid 0");
    }
    return next;
  }
  if (next.cutoffEventRowid > previous.cutoffEventRowid
    && previous.phase === "verify"
    && previous.afterEventRowid === previous.cutoffEventRowid
    && next.phase === "receipts"
    && next.afterEventRowid === 0) {
    return next;
  }
  fail("storage lifetime cutoff can change only after completed verify when catch-up restarts receipts at 0");
}

export function validateStorageLifetimeMaterializationJobV1(
  value: unknown,
): StorageLifetimeMaterializationJobV1 {
  const job = requireObject(value, "storage lifetime materialization job");
  exactKeys(job, [
    "singleton", "jobId", "state", "cutoffEventRowid", "phase", "afterEventRowid",
    "failureKind", "createdAt", "updatedAt", "completedAt",
  ]);
  if (job.singleton !== 1) fail("storage lifetime job singleton must be 1");
  if (typeof job.jobId !== "string" || job.jobId.length === 0) {
    fail("storage lifetime jobId must be non-empty");
  }
  if (!STORAGE_LIFETIME_JOB_STATES_V1.includes(job.state as StorageLifetimeJobStateV1)) {
    fail("storage lifetime job state is invalid");
  }
  const cursor = validateStorageLifetimeCursorV1({
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterEventRowid: job.afterEventRowid,
  });
  requireTimestamp(job.createdAt, "job.createdAt");
  requireTimestamp(job.updatedAt, "job.updatedAt");
  if (job.state === "pending") {
    if (job.failureKind !== null || job.completedAt !== null) {
      fail("pending storage lifetime job cannot carry failure or completion evidence");
    }
  } else {
    if (typeof job.failureKind !== "string"
      || !/^[A-Za-z][A-Za-z0-9._-]{0,63}$/.test(job.failureKind)) {
      fail("failed storage lifetime job requires sanitized failureKind");
    }
    requireTimestamp(job.completedAt, "job.completedAt");
  }
  void cursor;
  return job as unknown as StorageLifetimeMaterializationJobV1;
}

export function validateStorageLifetimeMaterializationPairV1(
  value: unknown,
): StorageLifetimeMaterializationSnapshotV1 {
  const snapshot = requireObject(value, "storage lifetime materialization pair");
  exactKeys(snapshot, ["materializationState", "job"]);
  if (!STORAGE_MATERIALIZATION_STATES_V1.includes(
    snapshot.materializationState as StorageMaterializationStateV1,
  )) {
    fail("storage materialization state is invalid");
  }
  const job = snapshot.job === null
    ? null
    : validateStorageLifetimeMaterializationJobV1(snapshot.job);
  const valid = snapshot.materializationState === "pending"
    || (snapshot.materializationState === "ready" && job === null);
  if (!valid || (snapshot.materializationState === "ready" && job !== null)) {
    fail("storage materialization meta/job pairing is fail-closed");
  }
  return {
    materializationState: snapshot.materializationState,
    job,
  } as StorageLifetimeMaterializationSnapshotV1;
}

export function validateStorageLifetimeLifecycleTransitionV1(
  operation: StorageLifetimeLifecycleOperationV1,
  previousValue: unknown,
  nextValue: unknown,
): StorageLifetimeMaterializationSnapshotV1 {
  const previous = validateStorageLifetimeMaterializationPairV1(previousValue);
  const next = validateStorageLifetimeMaterializationPairV1(nextValue);
  if (operation === "create") {
    if (previous.materializationState !== "pending" || previous.job !== null
      || next.materializationState !== "pending" || next.job?.state !== "pending") {
      fail("storage lifetime create requires pending+absent to pending+pending");
    }
    validateStorageLifetimeCursorTransitionV1(null, jobCursor(next.job));
    return next;
  }
  if (operation === "cancel" || operation === "resume") {
    if (previous.job?.state !== "pending" || !sameContractValue(previous, next)) {
      fail(`storage lifetime ${operation} must preserve the exact pending snapshot`);
    }
    return next;
  }
  if (operation === "commitBatch") {
    const [previousJob, nextJob] = requireSamePendingJob(previous, next, operation);
    requireSameJobIdentity(previousJob, nextJob);
    if (previousJob.cutoffEventRowid !== nextJob.cutoffEventRowid) {
      fail("storage lifetime batch commit cannot change the frozen cutoff");
    }
    validateStorageLifetimeCursorTransitionV1(jobCursor(previousJob), jobCursor(nextJob));
    return next;
  }
  if (operation === "fail") {
    if (previous.materializationState !== "pending" || next.materializationState !== "pending"
      || previous.job?.state !== "pending" || next.job?.state !== "failed") {
      fail("storage lifetime failure must transition the same pending singleton to failed");
    }
    requireSameJobIdentity(previous.job, next.job);
    if (!sameContractValue(jobCursor(previous.job), jobCursor(next.job))) {
      fail("storage lifetime failure must preserve the canonical cursor");
    }
    return next;
  }
  if (operation === "complete") {
    if (previous.materializationState !== "pending" || previous.job?.state !== "pending"
      || previous.job.phase !== "verify"
      || previous.job.afterEventRowid !== previous.job.cutoffEventRowid
      || next.materializationState !== "ready" || next.job !== null) {
      fail("storage lifetime completion requires completed verify and atomically removes the job");
    }
    return next;
  }
  if (operation === "append") {
    const readyInvalidation = previous.materializationState === "ready" && previous.job === null
      && next.materializationState === "pending" && next.job === null;
    const pendingPreserved = previous.materializationState === "pending"
      && sameContractValue(previous, next);
    if (!readyInvalidation && !pendingPreserved) {
      fail("storage lifetime append must invalidate ready or preserve the frozen pending snapshot");
    }
    return next;
  }
  if (operation === "catchUpReset") {
    const [previousJob, nextJob] = requireSamePendingJob(previous, next, operation);
    requireSameJobIdentity(previousJob, nextJob);
    if (previousJob.phase !== "verify"
      || previousJob.afterEventRowid !== previousJob.cutoffEventRowid
      || nextJob.cutoffEventRowid <= previousJob.cutoffEventRowid
      || nextJob.phase !== "receipts" || nextJob.afterEventRowid !== 0) {
      fail("storage lifetime catch-up reset requires completed verify and a larger cutoff");
    }
    return next;
  }
  fail("storage lifetime lifecycle operation is invalid");
}

export function validateStorageLifetimeBatchSourcePayloadsV1(value: unknown): readonly number[] {
  if (!Array.isArray(value)) fail("storage lifetime batch source payload sizes must be an array");
  if (value.length > STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1) {
    fail("storage lifetime batch exceeds the fixed 100 source row limit");
  }
  let total = 0;
  for (const payloadValue of value) {
    const payloadBytes = requireSafeUnsigned(payloadValue, "storage lifetime source payload bytes");
    if (payloadBytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1) {
      fail("storage lifetime source payload exceeds the fixed 1 MiB contract limit");
    }
    total += payloadBytes;
    if (total > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1) {
      fail("storage lifetime batch exceeds the fixed 1 MiB source payload limit");
    }
  }
  return value as readonly number[];
}

export function validateStorageLifetimeBatchPolicyV1(value: unknown): StorageLifetimeBatchPolicyV1 {
  const policy = requireObject(value, "storage lifetime batch policy");
  exactKeys(policy, [
    "schemaVersion", "casesFile", "guards", "cursor", "scan", "source",
    "transaction", "cancellation", "job", "pairing", "append", "finalPublication", "rollback",
    "activation",
  ]);
  const guards = requireObject(policy.guards, "storage lifetime batch guards");
  exactKeys(guards, [
    "maxSourceRowsPerBatch", "maxCanonicalSourcePayloadBytesPerBatch",
    "singleSourcePayloadOversize", "callerConfigurable", "appliesToPhases",
  ]);
  const cursor = requireObject(policy.cursor, "storage lifetime cursor policy");
  exactKeys(cursor, [
    "schemaVersion", "phases", "scanPhases", "phaseStartAfterEventRowid",
    "samePhaseProgress", "phaseTransition",
  ]);
  const scan = requireObject(policy.scan, "storage lifetime scan policy");
  exactKeys(scan, ["predicate", "orderBy", "offsetAllowed"]);
  const source = requireObject(policy.source, "storage lifetime source policy");
  exactKeys(source, [
    "artifactAuthority", "workspaceAndBaselineSource", "repeatedProjectionSnapshots",
    "liveProjectionUse", "unboundedCollectionAllowed",
  ]);
  const transaction = requireObject(policy.transaction, "storage lifetime transaction policy");
  exactKeys(transaction, [
    "cadence", "committedDerivedRowsMayPersist", "committedProgressState",
    "implementationMayCommitRunning",
  ]);
  const cancellation = requireObject(policy.cancellation, "storage lifetime cancellation policy");
  exactKeys(cancellation, [
    "beforeBegin", "checks", "duringTransaction", "outcome", "crash",
    "committedBatchWinsAfterFinalPrecommitCheck",
  ]);
  const job = requireObject(policy.job, "storage lifetime job policy");
  exactKeys(job, [
    "table", "maximumRows", "singletonValue", "uniqueJobId", "states", "cursorColumns",
    "maintenanceJobsRole", "singleSqliteOwnerRequired", "transactionSerialization",
    "transactionBoundaryValidation", "batchCommit", "cancellation", "terminalFailure",
    "terminalFailureMetaState", "failureRecordUnavailable", "automaticRetryEnabled", "completion", "appendAfterReady",
    "appendDuringPending", "catchUp", "schedulerCreationImplemented",
    "forbiddenCommittedStates",
  ]);
  const pairing = requireObject(policy.pairing, "storage lifetime pairing policy");
  exactKeys(pairing, ["canonicalReadinessAuthority", "validPairs", "failClosedPairs"]);
  const append = requireObject(policy.append, "storage lifetime append policy");
  exactKeys(append, [
    "readyInvalidation", "readyPublication", "appendDuringMaterialization",
    "readyRegressionSequence",
  ]);
  const finalPublication = requireObject(
    policy.finalPublication,
    "storage lifetime final publication policy",
  );
  exactKeys(finalPublication, [
    "transaction", "prerequisites", "maxRowidChanged", "unboundedCollectionAllowed",
  ]);
  const rollback = requireObject(policy.rollback, "storage lifetime rollback policy");
  exactKeys(rollback, [
    "derivedPreparationBatchesMayPersist", "sourceLedgerMutationAtomicRollbackRequired",
    "compactionTriggerChangeAtomicRollbackRequired",
  ]);
  const activation = requireObject(policy.activation, "storage lifetime activation policy");
  exactKeys(activation, [
    "ledgerDeletionEnabled", "automaticSchedulingEnabled", "compactionImplementationEnabled",
    "statusWireEnabled",
  ]);

  const exact = policy.schemaVersion === STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1
    && policy.casesFile === "batching-cases.json"
    && guards.maxSourceRowsPerBatch === STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1
    && guards.maxCanonicalSourcePayloadBytesPerBatch
      === STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1
    && guards.singleSourcePayloadOversize === "contractError"
    && guards.callerConfigurable === false
    && exactStringArray(guards.appliesToPhases, [
      "receipts", "messages", "workspaces", "baselines", "verify",
    ])
    && cursor.schemaVersion === STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1
    && exactStringArray(cursor.phases, STORAGE_LIFETIME_PHASES_V1)
    && exactStringArray(cursor.scanPhases, [
      "receipts", "messages", "workspaces", "baselines", "verify",
    ])
    && cursor.phaseStartAfterEventRowid === 0
    && cursor.samePhaseProgress === "strictlyIncreasingWithinFrozenCutoff"
    && cursor.phaseTransition === "currentAtCutoffThenNextPhaseAfterZero"
    && scan.predicate === STORAGE_LIFETIME_KEYSET_SCAN_PREDICATE_V1
    && scan.orderBy === STORAGE_LIFETIME_KEYSET_SCAN_ORDER_V1
    && scan.offsetAllowed === false
    && source.artifactAuthority === "eventsAtOrBelowFrozenCutoffOnly"
    && source.workspaceAndBaselineSource === "eventProjectionSnapshotsAtOrBelowFrozenCutoff"
    && source.repeatedProjectionSnapshots === "idempotent"
    && source.liveProjectionUse === "finalCatchUpEquivalenceOnlyAfterMaxEventRowidEqualsCutoff"
    && source.unboundedCollectionAllowed === false
    && transaction.cadence === "oneBatchOneBeginImmediatePerIdleTurn"
    && transaction.committedDerivedRowsMayPersist === true
    && transaction.committedProgressState === "pending"
    && transaction.implementationMayCommitRunning === false
    && cancellation.beforeBegin === "noMutation"
    && exactStringArray(cancellation.checks, [
      "beforeEachSourceRow", "afterEachSourceRow", "immediatelyPrecommit",
    ])
    && cancellation.duringTransaction === "rollbackCurrentBatch"
    && cancellation.outcome === "Suspended"
    && cancellation.crash === "rollbackCurrentBatchResumePrecedingCommittedPendingCursor"
    && cancellation.committedBatchWinsAfterFinalPrecommitCheck === true
    && job.table === "lifetime_materialization_job"
    && job.maximumRows === 1
    && job.singletonValue === 1
    && job.uniqueJobId === true
    && exactStringArray(job.states, STORAGE_LIFETIME_JOB_STATES_V1)
    && exactStringArray(job.cursorColumns, [
      "cutoff_event_rowid", "phase", "after_event_rowid",
    ])
    && job.maintenanceJobsRole === "losslessCompactionOnlyUnchanged"
    && job.singleSqliteOwnerRequired === true
    && job.transactionSerialization === "beginImmediate"
    && job.transactionBoundaryValidation === "exactMetaJobPairAndValidSingletonCursor"
    && job.batchCommit === "derivedRowsAndAdvancedPendingSingletonAtomic"
    && job.cancellation === "rollbackLeavesSamePendingSingletonAndCursor"
    && job.terminalFailure === "sameSingletonPendingToFailedPreserveCursorAfterRollback"
    && job.terminalFailureMetaState === "pending"
    && job.failureRecordUnavailable === "failClosedRestartRevalidatesBeforeMutationNeverSuccess"
    && job.automaticRetryEnabled === false
    && job.completion === "deleteSingletonAndSetMetaReadySameTransaction"
    && job.appendAfterReady === "setMetaPendingKeepJobAbsentSameAppendTransaction"
    && job.appendDuringPending === "doNotAlterFrozenSingletonCursor"
    && job.catchUp === "samePendingSingletonResetToNewCutoffReceiptsZero"
    && job.schedulerCreationImplemented === false
    && exactStringArray(job.forbiddenCommittedStates, ["running", "succeeded", "cancelled"])
    && pairing.canonicalReadinessAuthority === "storage_schema_meta.materialization_state"
    && exactStringArray(pairing.validPairs, [
      "pending+absent", "pending+pending", "pending+failed", "ready+absent",
    ])
    && exactStringArray(pairing.failClosedPairs, [
      "materializing+any", "failed+any", "ready+present",
    ])
    && append.readyInvalidation === "pendingInSameAppendTransaction"
    && append.readyPublication === "separateAtomicCatchUpVerificationTransaction"
    && append.appendDuringMaterialization === "frozenCutoffUnchangedUntilFinalCatchUp"
    && exactStringArray(append.readyRegressionSequence, [
      "materializeReady",
      "appendAndInvalidateReadyToPendingSameTransaction",
      "separateCatchUpVerification",
      "publishReady",
    ])
    && finalPublication.transaction === "separateBeginImmediate"
    && exactStringArray(finalPublication.prerequisites, [
      "maxEventRowidEqualsCutoff",
      "exactDerivedCompletenessAndEquivalence",
      "liveProjectionEqualsCutoffReconstruction",
      "boundedSqlKeysetEvidence",
    ])
    && finalPublication.maxRowidChanged
      === "remainPendingResetSameSingletonAtObservedCutoffNeverPublishReady"
    && finalPublication.unboundedCollectionAllowed === false
    && rollback.derivedPreparationBatchesMayPersist === true
    && rollback.sourceLedgerMutationAtomicRollbackRequired === true
    && rollback.compactionTriggerChangeAtomicRollbackRequired === true
    && activation.ledgerDeletionEnabled === false
    && activation.automaticSchedulingEnabled === false
    && activation.compactionImplementationEnabled === false
    && activation.statusWireEnabled === false;
  if (!exact) fail("storage lifetime batch policy contradicts the fixed v1 safety contract");
  return policy as unknown as StorageLifetimeBatchPolicyV1;
}

const STORAGE_LIFETIME_BATCH_POLICY_V2 = {
  schemaVersion: STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V2,
  casesFile: "cases.json",
  guards: {
    maxRowsPerBatch: STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
    maxCanonicalPayloadBytesPerBatch:
      STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    callerConfigurable: false,
    appliesToPhases: STORAGE_LIFETIME_PHASES_V2,
  },
  cursor: {
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2,
    column: "after_scan_rowid",
    phases: STORAGE_LIFETIME_PHASES_V2,
    eventRowidPhases: STORAGE_LIFETIME_EVENT_ROWID_PHASES_V2,
    derivedTableRowidPhases: STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V2,
    phaseStartAfterScanRowid: 0,
    phaseStartScanExhausted: false,
    eventRowidBound: "afterScanRowidLessThanOrEqualToCutoff",
    derivedRowidBound: "jsSafeUnsignedIndependentOfCutoff",
    scanExhaustedMutationAuthority: "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
    scanExhaustedProofBoundary: "implementationOwnedAndTestedInSidecarNotRepresentableAsPublicJson",
    phaseTransition: "scanExhaustedTrueThenExactNextAtZeroAndFalse",
  },
  scan: {
    sourcePredicate: "rowid > afterScanRowid AND rowid <= cutoffEventRowid",
    derivedPredicate: "rowid > afterScanRowid",
    orderBy: "rowid",
    limitSql: STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2,
    forbiddenWithinBatch: ["OFFSET", "COUNT", "window", "GROUP BY", "unboundedFullTableScan"],
  },
  transaction: {
    wordingRevision: "shortArmTransactionThenOneBoundedBatchTransaction",
    arm: "pendingToAttemptingCommittedBeforeBatch",
    success: "derivedRowsCursorAndAttemptingToPendingAtomic",
    nonemptyBatchScanExhausted: false,
    emptyBatchScanExhausted: "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
    cancellation: "rollbackBatchOrSavepointThenCommitAttemptingToPending",
    terminalValidationFailure: "rollbackBatchThenCommitAttemptingToFailed",
    failureRecordUnavailable: "durableAttemptingSentinelReopenRefusesRetryOrSuccess",
    reopenAttempting: "failClosedNonRunnable",
    reopenFailed: "failClosedNonRunnable",
  },
  verification: {
    verifySource: "eachEventAtOrBelowCutoffHasExactArtifactsCurrentProjectionAndSequence",
    verifyDerived: "eachDerivedRowHasExactSourceBackingAndNoSourceLessExtra",
    derivedPhaseTables: {
      verifyReceipts: "operation_receipts",
      verifyMessages: "chat_messages",
      verifyWorkspaces: "prepared_workspaces",
      verifyBaselines: "ledger_baselines",
    },
    historicalBaseline: "allowedWhenDeterministicDigestValidAndSourceBacked",
    newestBaseline: "requiredForEveryCompactableAggregate",
  },
  cancellation: {
    afterCommittedVerifyBatch: "returnSuspended",
    finalPublicationInSameCall: false,
    committedBatchWinsScope: "committedBatchTransactionOnly",
  },
  finalPublication: {
    beforeBegin: "cancelReturnsSuspendedNoMutation",
    transaction: "shortBeginImmediateOnly",
    preconditions: [
      "exactMetaJobPairAndCursor",
      "verifyBaselinesTableEndProvedByExactEmptyKeysetBatch",
      "scanExhaustedTrue",
      "maxEventRowidEqualsCutoff",
      "cancelBeforeMutation",
      "precommitCancel",
    ],
    mutation: "deleteJobAndSetMetaReady",
    forbiddenWork: ["artifactScan", "tableScan", "count"],
    maxRowidChanged: "pendingReceiptsObservedCutoffAfterScanRowidZero",
  },
  activation: {
    cleanupEnabled: false,
    deletionEnabled: false,
    compactionEnabled: false,
    wireEnabled: false,
    schedulerEnabled: false,
  },
} as const;

export function validateStorageLifetimeBatchPolicyV2(value: unknown): Record<string, unknown> {
  const policy = requireObject(value, "storage lifetime v4 bounded verification policy");
  if (!sameContractValue(policy, STORAGE_LIFETIME_BATCH_POLICY_V2)) {
    fail("storage lifetime v4 policy contradicts the fixed bounded verification contract");
  }
  return policy;
}

export function validateStorageLifetimeCursorV2(value: unknown): StorageLifetimeCursorV2 {
  const cursor = requireObject(value, "storage lifetime v4 cursor");
  exactKeys(cursor, [
    "schemaVersion", "cutoffEventRowid", "phase", "afterScanRowid", "scanExhausted",
  ]);
  if (cursor.schemaVersion !== STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2) {
    fail("storage lifetime v4 cursor schemaVersion must be 2");
  }
  const cutoff = requireSafeUnsigned(cursor.cutoffEventRowid, "cursor.cutoffEventRowid");
  const after = requireSafeUnsigned(cursor.afterScanRowid, "cursor.afterScanRowid");
  if (typeof cursor.scanExhausted !== "boolean") {
    fail("storage lifetime v4 cursor scanExhausted must be boolean");
  }
  if (!STORAGE_LIFETIME_PHASES_V2.includes(cursor.phase as StorageLifetimePhaseV2)) {
    fail("storage lifetime v4 cursor phase is invalid");
  }
  if (STORAGE_LIFETIME_EVENT_ROWID_PHASES_V2.includes(
    cursor.phase as typeof STORAGE_LIFETIME_EVENT_ROWID_PHASES_V2[number],
  ) && after > cutoff) {
    fail("event-derived storage lifetime cursor cannot exceed cutoffEventRowid");
  }
  return cursor as unknown as StorageLifetimeCursorV2;
}

export function validateStorageLifetimeCursorTransitionV2(
  previousValue: unknown | null,
  nextValue: unknown,
): StorageLifetimeCursorV2 {
  const next = validateStorageLifetimeCursorV2(nextValue);
  if (previousValue === null) {
    if (next.phase !== "receipts" || next.afterScanRowid !== 0 || next.scanExhausted) {
      fail("storage lifetime v4 initial cursor must start unexhausted receipts at zero");
    }
    return next;
  }
  const previous = validateStorageLifetimeCursorV2(previousValue);
  if (next.cutoffEventRowid !== previous.cutoffEventRowid) {
    fail("storage lifetime v4 batch transition cannot change the frozen cutoff");
  }
  if (next.phase === previous.phase) {
    if (previous.scanExhausted || next.scanExhausted
      || next.afterScanRowid <= previous.afterScanRowid) {
      fail("storage lifetime v4 same-phase cursor must advance afterScanRowid");
    }
    return next;
  }
  const previousIndex = STORAGE_LIFETIME_PHASES_V2.indexOf(previous.phase);
  if (!previous.scanExhausted || next.scanExhausted
    || STORAGE_LIFETIME_PHASES_V2[previousIndex + 1] !== next.phase
    || next.afterScanRowid !== 0) {
    fail("storage lifetime v4 phase transition requires exhausted current phase and unexhausted exact next phase at zero");
  }
  return next;
}

export function validateStorageLifetimeMaterializationJobV2(
  value: unknown,
): StorageLifetimeMaterializationJobV2 {
  const job = requireObject(value, "storage lifetime v4 materialization job");
  exactKeys(job, [
    "singleton", "jobId", "state", "cutoffEventRowid", "phase", "afterScanRowid",
    "scanExhausted",
    "failureKind", "createdAt", "updatedAt", "completedAt",
  ]);
  if (job.singleton !== 1 || typeof job.jobId !== "string" || job.jobId.length === 0) {
    fail("storage lifetime v4 job requires singleton 1 and non-empty jobId");
  }
  if (!STORAGE_LIFETIME_JOB_STATES_V2.includes(job.state as StorageLifetimeJobStateV2)) {
    fail("storage lifetime v4 job state is invalid");
  }
  validateStorageLifetimeCursorV2({
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterScanRowid: job.afterScanRowid,
    scanExhausted: job.scanExhausted,
  });
  requireTimestamp(job.createdAt, "job.createdAt");
  requireTimestamp(job.updatedAt, "job.updatedAt");
  if (job.state === "failed") {
    if (typeof job.failureKind !== "string"
      || !/^[A-Za-z][A-Za-z0-9._-]{0,63}$/.test(job.failureKind)) {
      fail("failed storage lifetime v4 job requires sanitized failureKind");
    }
    requireTimestamp(job.completedAt, "job.completedAt");
  } else if (job.failureKind !== null || job.completedAt !== null) {
    fail("pending or attempting storage lifetime v4 job cannot carry terminal evidence");
  }
  return job as unknown as StorageLifetimeMaterializationJobV2;
}

export function validateStorageLifetimeMaterializationPairV2(
  value: unknown,
): StorageLifetimeMaterializationSnapshotV2 {
  const snapshot = requireObject(value, "storage lifetime v4 materialization pair");
  exactKeys(snapshot, ["materializationState", "job"]);
  if (!STORAGE_MATERIALIZATION_STATES_V1.includes(
    snapshot.materializationState as StorageMaterializationStateV1,
  )) {
    fail("storage materialization state is invalid");
  }
  const job = snapshot.job === null ? null : validateStorageLifetimeMaterializationJobV2(snapshot.job);
  if (!((snapshot.materializationState === "pending")
    || (snapshot.materializationState === "ready" && job === null))) {
    fail("storage lifetime v4 meta/job pairing is fail-closed");
  }
  return { materializationState: snapshot.materializationState, job } as
    StorageLifetimeMaterializationSnapshotV2;
}

export function validateStorageLifetimeReopenPairV2(
  value: unknown,
): StorageLifetimeMaterializationSnapshotV2 {
  const snapshot = validateStorageLifetimeMaterializationPairV2(value);
  if (snapshot.job?.state === "attempting" || snapshot.job?.state === "failed") {
    fail("storage lifetime v4 reopen refuses attempting or failed job");
  }
  return snapshot;
}

export function validateStorageLifetimeLifecycleTransitionV2(
  operation: StorageLifetimeLifecycleOperationV2,
  previousValue: unknown,
  nextValue: unknown,
): StorageLifetimeMaterializationSnapshotV2 {
  const previous = validateStorageLifetimeMaterializationPairV2(previousValue);
  const next = validateStorageLifetimeMaterializationPairV2(nextValue);
  const previousJob = previous.job;
  const nextJob = next.job;
  if (operation === "create") {
    if (previous.materializationState !== "pending" || previousJob !== null
      || next.materializationState !== "pending" || nextJob?.state !== "pending") {
      fail("storage lifetime v4 create requires pending absent to pending job");
    }
    validateStorageLifetimeCursorTransitionV2(null, jobCursorV2(nextJob));
    return next;
  }
  if (operation === "append") {
    const invalidateReady = previous.materializationState === "ready" && previousJob === null
      && next.materializationState === "pending" && nextJob === null;
    if (!invalidateReady && !sameContractValue(previous, next)) {
      fail("storage lifetime v4 append must invalidate ready or preserve pending exactly");
    }
    return next;
  }
  if (operation === "finalPublish") {
    if (previous.materializationState !== "pending" || previousJob?.state !== "pending"
      || previousJob.phase !== "verifyBaselines" || !previousJob.scanExhausted
      || next.materializationState !== "ready" || nextJob !== null) {
      fail("final publication requires pending verifyBaselines and atomically publishes ready");
    }
    return next;
  }
  if (previous.materializationState !== "pending" || next.materializationState !== "pending"
    || previousJob === null) {
    fail(`storage lifetime v4 ${operation} requires a pending meta/job pair`);
  }
  if (operation === "failureRecordUnavailable") {
    if (previousJob.state !== "attempting" || !sameContractValue(previous, next)) {
      fail("failure record unavailable must leave the durable attempting sentinel unchanged");
    }
    return next;
  }
  if (nextJob === null) fail(`storage lifetime v4 ${operation} requires a next job`);
  requireSameJobIdentityV2(previousJob, nextJob);
  if (operation === "arm") {
    if (previousJob.state !== "pending" || nextJob.state !== "attempting"
      || previousJob.scanExhausted || nextJob.scanExhausted
      || !sameContractValue(jobCursorV2(previousJob), jobCursorV2(nextJob))) {
      fail("storage lifetime v4 arm must commit pending to attempting at the same cursor");
    }
  } else if (operation === "batchSuccess") {
    if (previousJob.state !== "attempting" || nextJob.state !== "pending") {
      fail("storage lifetime v4 batch success must commit attempting to pending");
    }
    validateStorageLifetimeCursorTransitionV2(jobCursorV2(previousJob), jobCursorV2(nextJob));
  } else if (operation === "phaseAdvance") {
    if (previousJob.state !== "pending" || nextJob.state !== "pending") {
      fail("storage lifetime v4 phase advance requires pending jobs");
    }
    validateStorageLifetimeCursorTransitionV2(jobCursorV2(previousJob), jobCursorV2(nextJob));
  } else if (operation === "cancel") {
    if (previousJob.state !== "attempting" || nextJob.state !== "pending"
      || !sameContractValue(jobCursorV2(previousJob), jobCursorV2(nextJob))) {
      fail("storage lifetime v4 cancellation must restore pending at the same cursor");
    }
  } else if (operation === "fail") {
    if (previousJob.state !== "attempting" || nextJob.state !== "failed"
      || !sameContractValue(jobCursorV2(previousJob), jobCursorV2(nextJob))) {
      fail("storage lifetime v4 terminal failure must preserve the attempting cursor");
    }
  } else if (operation === "finalReset") {
    if (previousJob.state !== "pending" || previousJob.phase !== "verifyBaselines"
      || !previousJob.scanExhausted
      || nextJob.state !== "pending" || nextJob.cutoffEventRowid <= previousJob.cutoffEventRowid
      || nextJob.phase !== "receipts" || nextJob.afterScanRowid !== 0
      || nextJob.scanExhausted) {
      fail("storage lifetime v4 final reset requires a larger cutoff and receipts at zero");
    }
  } else {
    fail("storage lifetime v4 lifecycle operation is invalid");
  }
  return next;
}

export function validateStorageLifetimeVerificationEvidenceV2(value: unknown): Record<string, unknown> {
  const evidence = requireObject(value, "storage lifetime v4 verification evidence");
  exactKeys(evidence, [
    "phase", "cutoffEventRowid", "scannedRowids", "exactExpectedArtifacts",
    "currentProjectionEquivalent", "sequenceEquivalent", "sourceBacked",
    "noSourceLessExtra", "deterministic", "digestValid", "newestBaselinePresent",
  ]);
  if (!["verifySource", ...STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V2].includes(
    evidence.phase as "verifySource",
  )) fail("storage lifetime v4 verification evidence phase is invalid");
  const cutoff = requireSafeUnsigned(evidence.cutoffEventRowid, "evidence.cutoffEventRowid");
  if (!Array.isArray(evidence.scannedRowids)
    || evidence.scannedRowids.length > STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1) {
    fail("storage lifetime v4 verification evidence exceeds LIMIT 100");
  }
  let previous = -1;
  for (const rowidValue of evidence.scannedRowids) {
    const rowid = requireSafeUnsigned(rowidValue, "evidence.scannedRowid");
    if (rowid <= previous) fail("verification evidence rowids must be strictly increasing");
    if (evidence.phase === "verifySource" && rowid > cutoff) {
      fail("verifySource rowid cannot exceed cutoff");
    }
    previous = rowid;
  }
  if (evidence.phase === "verifySource") {
    if (evidence.exactExpectedArtifacts !== true
      || evidence.currentProjectionEquivalent !== true
      || evidence.sequenceEquivalent !== true) {
      fail("verifySource must prove artifacts, current projection, and sequence");
    }
  } else if (evidence.sourceBacked !== true || evidence.noSourceLessExtra !== true) {
    fail("derived verification must reject every source-less extra");
  }
  if (evidence.phase === "verifyBaselines"
    && (evidence.deterministic !== true || evidence.digestValid !== true
      || evidence.newestBaselinePresent !== true)) {
    fail("baseline verification requires deterministic digest-valid source-backed history and newest baseline");
  }
  return evidence;
}

const STORAGE_LIFETIME_BATCH_POLICY_V3 = {
  schemaVersion: STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V3,
  casesFile: "cases.json",
  guards: {
    maxRowsPerBatch: STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
    maxCanonicalPayloadBytesPerBatch:
      STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    callerConfigurable: false,
    appliesToPhases: STORAGE_LIFETIME_PHASES_V3,
  },
  cursor: {
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3,
    column: "after_scan_rowid",
    phases: STORAGE_LIFETIME_PHASES_V3,
    eventRowidPhases: STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3,
    projectionRowidPhases: STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V3,
    derivedRowidPhases: STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V3,
    phaseStartAfterScanRowid: 0,
    phaseStartScanExhausted: false,
    scanExhaustedMutationAuthority: "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
    phaseTransition: "scanExhaustedTrueThenExactNextAtZeroAndFalse",
  },
  scan: {
    sourcePredicate: "rowid > afterScanRowid AND rowid <= cutoffEventRowid",
    tablePredicate: "rowid > afterScanRowid",
    orderBy: "rowid",
    limitSql: STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2,
    privateEmptyWitness: "sameBeginImmediateTransaction",
    forbidden: [
      "OFFSET", "COUNT", "window", "GROUP BY", "antiJoin",
      "unboundedFullTableScan", "unboundedNamespaceQuery",
    ],
  },
  maxEventRowidFence: {
    transaction: "BEGIN IMMEDIATE",
    requiredBefore: [
      "enterVerifySource", "everyProjectionOrDerivedVerificationBatch", "finalPublication",
    ],
    equal: "continue",
    less: "contractError",
    greater: "sameSingletonPendingReceiptsNewCutoffZeroUnexhaustedNoFailure",
    frozenMaterializationPhases: "mayContinueAtExistingCutoff",
    appendSerialization: "BEGIN IMMEDIATESerializesAppendDuringBatch",
    normalWorkerCheckState: "pendingBeforeArm",
    armToBatch: "sameSynchronousSidecarOwnerImmediatelyEntersBatch",
    attemptingCatchUpReset: "forbidden",
    attemptingOnReopen: "failClosed",
  },
  verification: {
    verifySource: {
      boundedBy: "eventsRowidAtOrBelowCutoff",
      requires: [
        "exactAggregateSequenceAndPredecessor", "requiredDerivedPresence",
        "latestEventHasProjectionRow", "appInstanceSourceHasAppInstancesRow",
      ],
      forbidden: ["liveProjectionTableFullScan", "antiJoin"],
    },
    projectionPhaseTables: {
      verifyProjects: "projects",
      verifyChats: "chats",
      verifyRuns: "runs",
      verifyLocks: "locks",
      verifyAppInstances: "app_instances",
    },
    projectionRule:
      "eachRowMatchesLatestExactSourceEventAtOrBelowCutoffIncludingMirroredAndIndexedColumns",
    projectionColumns: {
      projects: ["project_id", "canonical_path", "projection_json", "last_event_sequence"],
      chats: ["chat_id", "project_id", "projection_json", "last_event_sequence"],
      runs: [
        "run_id", "chat_id", "state", "worktree_path", "projection_json",
        "last_event_sequence",
      ],
      locks: [
        "lock_id", "project_id", "writer_run_id", "worktree_path", "poisoned",
        "recovery_required", "evidence_json", "projection_json", "last_event_sequence",
      ],
      app_instances: [
        "sidecar_instance_id", "pid", "platform", "started_at", "heartbeat_at",
        "clean_shutdown_at", "last_event_sequence",
      ],
    },
    derivedPhaseTables: {
      verifyReceipts: "operation_receipts",
      verifyMessages: "chat_messages",
      verifyWorkspaces: "prepared_workspaces",
      verifyBaselines: "ledger_baselines",
    },
    missingProjectionDetection: "verifySourceLatestSourcePresence",
    extraOrSourceLessDetection: "exactProjectionTablePhase",
    historicalBaseline: "allowedWhenDeterministicDigestValidAndSourceBacked",
    newestBaseline: "requiredForEveryCompactableAggregate",
  },
  snapshotAggregates: {
    includedProjectionTables: ["projects", "chats", "runs", "locks"],
    excludedProjectionTables: ["app_instances"],
    appInstancesExclusionReason: "lifecycleProjectionNotMaterializedSnapshotAggregate",
  },
  transaction: {
    arm: "pendingToAttemptingCommittedBeforeBatch",
    success: "verifiedRowsCursorAndAttemptingToPendingAtomic",
    nonemptyBatchScanExhausted: false,
    emptyBatchScanExhausted: "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
    cancellation: "rollbackBatchOrSavepointThenCommitAttemptingToPending",
    terminalValidationFailure: "rollbackBatchThenCommitAttemptingToFailed",
    failureRecordUnavailable: "durableAttemptingSentinelReopenRefusesRetryOrSuccess",
    reopenAttempting: "failClosedNonRunnable",
    reopenFailed: "failClosedNonRunnable",
  },
  finalPublication: {
    transaction: "shortBeginImmediateOnly",
    preconditions: [
      "exactMetaJobPairAndCursor",
      "verifyBaselinesTableEndProvedByExactEmptyKeysetBatch",
      "scanExhaustedTrue", "maxEventRowidEqualsCutoff", "cancelBeforeMutation",
      "precommitCancel",
    ],
    mutation: "deleteJobAndSetMetaReady",
    forbiddenWork: ["artifactScan", "tableScan", "count"],
  },
  activation: {
    cleanupEnabled: false,
    deletionEnabled: false,
    compactionEnabled: false,
    wireEnabled: false,
    schedulerEnabled: false,
  },
} as const;

export function validateStorageLifetimeBatchPolicyV3(value: unknown): Record<string, unknown> {
  const policy = requireObject(value, "storage lifetime v5 bounded projection policy");
  if (!sameContractValue(policy, STORAGE_LIFETIME_BATCH_POLICY_V3)) {
    fail("storage lifetime v5 policy contradicts bounded projection verification");
  }
  return policy;
}

export function validateStorageLifetimeCursorV3(value: unknown): StorageLifetimeCursorV3 {
  const cursor = requireObject(value, "storage lifetime v5 cursor");
  exactKeys(cursor, [
    "schemaVersion", "cutoffEventRowid", "phase", "afterScanRowid", "scanExhausted",
  ]);
  if (cursor.schemaVersion !== STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3) {
    fail("storage lifetime v5 cursor schemaVersion must be 3");
  }
  const cutoff = requireSafeUnsigned(cursor.cutoffEventRowid, "cursor.cutoffEventRowid");
  const after = requireSafeUnsigned(cursor.afterScanRowid, "cursor.afterScanRowid");
  if (typeof cursor.scanExhausted !== "boolean") {
    fail("storage lifetime v5 cursor scanExhausted must be boolean");
  }
  if (!STORAGE_LIFETIME_PHASES_V3.includes(cursor.phase as StorageLifetimePhaseV3)) {
    fail("storage lifetime v5 cursor phase is invalid");
  }
  if (STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3.includes(
    cursor.phase as typeof STORAGE_LIFETIME_EVENT_ROWID_PHASES_V3[number],
  ) && after > cutoff) {
    fail("event-derived storage lifetime v5 cursor cannot exceed cutoffEventRowid");
  }
  return cursor as unknown as StorageLifetimeCursorV3;
}

export function validateStorageLifetimeCursorTransitionV3(
  previousValue: unknown | null,
  nextValue: unknown,
): StorageLifetimeCursorV3 {
  const next = validateStorageLifetimeCursorV3(nextValue);
  if (previousValue === null) {
    if (next.phase !== "receipts" || next.afterScanRowid !== 0 || next.scanExhausted) {
      fail("storage lifetime v5 initial cursor must start unexhausted receipts at zero");
    }
    return next;
  }
  const previous = validateStorageLifetimeCursorV3(previousValue);
  if (next.cutoffEventRowid !== previous.cutoffEventRowid) {
    fail("storage lifetime v5 batch transition cannot change the frozen cutoff");
  }
  if (next.phase === previous.phase) {
    if (previous.scanExhausted || next.scanExhausted
      || next.afterScanRowid <= previous.afterScanRowid) {
      fail("storage lifetime v5 same-phase cursor must advance afterScanRowid");
    }
    return next;
  }
  const previousIndex = STORAGE_LIFETIME_PHASES_V3.indexOf(previous.phase);
  if (!previous.scanExhausted || next.scanExhausted
    || STORAGE_LIFETIME_PHASES_V3[previousIndex + 1] !== next.phase
    || next.afterScanRowid !== 0) {
    fail("storage lifetime v5 phase transition requires exhausted current phase and unexhausted exact next phase at zero");
  }
  return next;
}

export function validateStorageLifetimeMaterializationJobV3(
  value: unknown,
): StorageLifetimeMaterializationJobV3 {
  const job = requireObject(value, "storage lifetime v5 materialization job");
  exactKeys(job, [
    "singleton", "jobId", "state", "cutoffEventRowid", "phase", "afterScanRowid",
    "scanExhausted", "failureKind", "createdAt", "updatedAt", "completedAt",
  ]);
  if (job.singleton !== 1 || typeof job.jobId !== "string" || job.jobId.length === 0) {
    fail("storage lifetime v5 job requires singleton 1 and non-empty jobId");
  }
  if (!STORAGE_LIFETIME_JOB_STATES_V2.includes(job.state as StorageLifetimeJobStateV2)) {
    fail("storage lifetime v5 job state is invalid");
  }
  validateStorageLifetimeCursorV3({
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterScanRowid: job.afterScanRowid,
    scanExhausted: job.scanExhausted,
  });
  requireTimestamp(job.createdAt, "job.createdAt");
  requireTimestamp(job.updatedAt, "job.updatedAt");
  if (job.state === "failed") {
    if (typeof job.failureKind !== "string"
      || !/^[A-Za-z][A-Za-z0-9._-]{0,63}$/.test(job.failureKind)) {
      fail("failed storage lifetime v5 job requires sanitized failureKind");
    }
    requireTimestamp(job.completedAt, "job.completedAt");
  } else if (job.failureKind !== null || job.completedAt !== null) {
    fail("pending or attempting storage lifetime v5 job cannot carry terminal evidence");
  }
  return job as unknown as StorageLifetimeMaterializationJobV3;
}

export function validateStorageLifetimeMaterializationPairV3(
  value: unknown,
): StorageLifetimeMaterializationSnapshotV3 {
  const snapshot = requireObject(value, "storage lifetime v5 materialization pair");
  exactKeys(snapshot, ["materializationState", "job"]);
  if (!STORAGE_MATERIALIZATION_STATES_V1.includes(
    snapshot.materializationState as StorageMaterializationStateV1,
  )) fail("storage materialization state is invalid");
  const job = snapshot.job === null ? null : validateStorageLifetimeMaterializationJobV3(snapshot.job);
  if (!(snapshot.materializationState === "pending"
    || (snapshot.materializationState === "ready" && job === null))) {
    fail("storage lifetime v5 meta/job pairing is fail-closed");
  }
  return { materializationState: snapshot.materializationState, job } as
    StorageLifetimeMaterializationSnapshotV3;
}

export function validateStorageLifetimeReopenPairV3(
  value: unknown,
): StorageLifetimeMaterializationSnapshotV3 {
  const snapshot = validateStorageLifetimeMaterializationPairV3(value);
  if (snapshot.job?.state === "attempting" || snapshot.job?.state === "failed") {
    fail("storage lifetime v5 reopen refuses attempting or failed job");
  }
  return snapshot;
}

export function validateStorageLifetimeLifecycleTransitionV3(
  operation: StorageLifetimeLifecycleOperationV3,
  previousValue: unknown,
  nextValue: unknown,
): StorageLifetimeMaterializationSnapshotV3 {
  const previous = validateStorageLifetimeMaterializationPairV3(previousValue);
  const next = validateStorageLifetimeMaterializationPairV3(nextValue);
  const previousJob = previous.job;
  const nextJob = next.job;
  if (operation === "create") {
    if (previous.materializationState !== "pending" || previousJob !== null
      || next.materializationState !== "pending" || nextJob?.state !== "pending") {
      fail("storage lifetime v5 create requires pending absent to pending job");
    }
    validateStorageLifetimeCursorTransitionV3(null, jobCursorV3(nextJob));
    return next;
  }
  if (operation === "append") {
    const invalidateReady = previous.materializationState === "ready" && previousJob === null
      && next.materializationState === "pending" && nextJob === null;
    if (!invalidateReady && !sameContractValue(previous, next)) {
      fail("storage lifetime v5 append must invalidate ready or preserve pending exactly");
    }
    return next;
  }
  if (operation === "failureRecordUnavailable") {
    if (previousJob?.state !== "attempting" || !sameContractValue(previous, next)) {
      fail("failure record unavailable must leave the durable attempting sentinel unchanged");
    }
    return next;
  }
  if (operation === "finalPublish") {
    if (previousJob?.state !== "pending" || previousJob.phase !== "verifyBaselines"
      || !previousJob.scanExhausted || next.materializationState !== "ready" || nextJob !== null) {
      fail("storage lifetime v5 final publication requires exhausted verifyBaselines");
    }
    return next;
  }
  if (previousJob === null || nextJob === null) {
    fail(`storage lifetime v5 ${operation} requires a pending meta/job pair`);
  }
  requireSameJobIdentityV3(previousJob, nextJob);
  if (operation === "arm") {
    if (previousJob.state !== "pending" || nextJob.state !== "attempting"
      || previousJob.scanExhausted || nextJob.scanExhausted
      || !sameContractValue(jobCursorV3(previousJob), jobCursorV3(nextJob))) {
      fail("storage lifetime v5 arm must preserve an unexhausted cursor");
    }
  } else if (operation === "batchSuccess") {
    if (previousJob.state !== "attempting" || nextJob.state !== "pending") {
      fail("storage lifetime v5 batch success must commit attempting to pending");
    }
    validateStorageLifetimeCursorTransitionV3(jobCursorV3(previousJob), jobCursorV3(nextJob));
  } else if (operation === "phaseAdvance") {
    if (previousJob.state !== "pending" || nextJob.state !== "pending") {
      fail("storage lifetime v5 phase advance requires pending jobs");
    }
    validateStorageLifetimeCursorTransitionV3(jobCursorV3(previousJob), jobCursorV3(nextJob));
  } else if (operation === "cancel") {
    if (previousJob.state !== "attempting" || nextJob.state !== "pending"
      || !sameContractValue(jobCursorV3(previousJob), jobCursorV3(nextJob))) {
      fail("storage lifetime v5 cancellation must restore the same cursor");
    }
  } else if (operation === "fail") {
    if (previousJob.state !== "attempting" || nextJob.state !== "failed"
      || !sameContractValue(jobCursorV3(previousJob), jobCursorV3(nextJob))) {
      fail("storage lifetime v5 terminal failure must preserve the cursor");
    }
  } else if (operation === "catchUpReset") {
    if (previousJob.state !== "pending" || !isVerificationPhaseV3(previousJob.phase)
      || nextJob.state !== "pending" || nextJob.cutoffEventRowid <= previousJob.cutoffEventRowid
      || nextJob.phase !== "receipts" || nextJob.afterScanRowid !== 0
      || nextJob.scanExhausted) {
      fail("storage lifetime v5 catch-up reset requires verification drift and a larger cutoff");
    }
  } else {
    fail("storage lifetime v5 lifecycle operation is invalid");
  }
  return next;
}

function isVerificationPhaseV3(phase: StorageLifetimePhaseV3): boolean {
  return !["receipts", "messages", "workspaces", "baselines"].includes(phase);
}

function jobCursorV3(job: StorageLifetimeMaterializationJobV3): StorageLifetimeCursorV3 {
  return {
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V3,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterScanRowid: job.afterScanRowid,
    scanExhausted: job.scanExhausted,
  };
}

function requireSameJobIdentityV3(
  previous: StorageLifetimeMaterializationJobV3,
  next: StorageLifetimeMaterializationJobV3,
): void {
  if (previous.singleton !== next.singleton || previous.jobId !== next.jobId
    || previous.createdAt !== next.createdAt) {
    fail("storage lifetime v5 transition cannot replace job identity");
  }
}

const STORAGE_LIFETIME_BATCH_POLICY_V4 = {
  schemaVersion: STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V4,
  casesFile: "cases.json",
  guards: {
    maxRowsPerBatch: STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
    maxCanonicalPayloadBytesPerBatch:
      STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    callerConfigurable: false,
    appliesToPhases: STORAGE_LIFETIME_PHASES_V4,
  },
  cursor: {
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4,
    column: "after_scan_rowid",
    phases: STORAGE_LIFETIME_PHASES_V4,
    eventRowidPhases: STORAGE_LIFETIME_EVENT_ROWID_PHASES_V4,
    projectionRowidPhases: STORAGE_LIFETIME_PROJECTION_ROWID_PHASES_V4,
    derivedRowidPhases: STORAGE_LIFETIME_DERIVED_ROWID_PHASES_V4,
    phaseStartAfterScanRowid: 0,
    phaseStartScanExhausted: false,
    scanExhaustedMutationAuthority: "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
    phaseTransition: "scanExhaustedTrueThenExactNextAtZeroAndFalse",
  },
  sourceIndex: {
    table: "event_source_index",
    columns: STORAGE_EVENT_SOURCE_INDEX_COLUMNS_V1,
    identity: "eventRowidEventIdOperationIdAndAggregateTupleExact",
    messageId: "nullableExactLastMessageIdFromCanonicalChatPayload",
    foreignKeys: [
      "event_id->events.event_id",
      "operation_id->events.operation_id",
      "aggregateTuple->events.aggregateTuple",
    ],
    foreignKeyEnforcement: "PRAGMA foreign_keys=ONOnEveryConnection",
    insertGuard: "event_source_index_requires_exact_event",
    sameEventBinding:
      "eventRowidAndEveryMirroredFieldAndMessageProvenanceMustMatchOneEventsRow",
    separateForeignKeyCrossEventMix: "rejectedByInsertGuard",
    historyBackfillByMigration: false,
    historyBackfillPhase: "indexSource",
    indexSourceWrite: "insertOrExactExistingNoOpOtherwiseConflict",
    verificationPhase: "verifySourceIndex",
    aggregateLatestIndex: "event_source_index_aggregate_latest",
    messageUniqueIndex: "event_source_index_message_unique",
    mutationGuards: [
      "event_source_index_requires_exact_event",
      "event_source_index_append_only_update",
      "event_source_index_append_only_delete",
    ],
  },
  scan: {
    sourcePredicate: "rowid > afterScanRowid AND rowid <= cutoffEventRowid",
    sourceIndexPredicate: "event_rowid > afterScanRowid AND event_rowid <= cutoffEventRowid",
    tablePredicate: "rowid > afterScanRowid",
    orderBy: "rowid",
    limitSql: STORAGE_LIFETIME_KEYSET_LIMIT_SQL_V2,
    privateEmptyWitness: "sameBeginImmediateTransaction",
    forbidden: [
      "OFFSET", "COUNT", "window", "GROUP BY", "antiJoin",
      "unboundedFullTableScan", "unboundedNamespaceQuery",
    ],
  },
  appendAtomicity: {
    transaction: "sameBeginImmediateAsEventProjectionAndReadyInvalidation",
    eventInsert: "requiresExactSourceIndexInsertBeforeCommit",
    duplicateNoOp: "noSourceIndexMutation",
    conflict: "rollbackEventProjectionSourceIndexAndReadiness",
    messageIdentityConflict: "rollbackWholeAppend",
  },
  workspaceProvenance: {
    lookup: "latestProjectSourceByAggregateLatestIndex",
    payloadFetch: "singleEventsRowidLookup",
    payloadLimitBytes: STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    comparison: "rustExactCanonicalProjectPayloadAndWorkspaceProjectId",
    stopWhen: "aggregateRemovalOrTombstoneBecomesLegalUntilSourceIndexSemanticsAreVersioned",
  },
  queryPlanAcceptance: {
    mode: "exactExplainQueryPlanDetail",
    indexSource: "SEARCH events USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)",
    verifySourceIndex:
      "SEARCH event_source_index USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)",
    latestAggregate:
      "SEARCH event_source_index USING COVERING INDEX event_source_index_aggregate_latest (aggregate_type=? AND aggregate_id=?)",
    messageIdentity:
      "SEARCH event_source_index USING COVERING INDEX event_source_index_message_unique (message_id=?)",
    payloadByRowid: "SEARCH events USING INTEGER PRIMARY KEY (rowid=?)",
    reject: ["SCAN events", "SCAN event_source_index", "USE TEMP B-TREE"],
  },
  maxEventRowidFence: {
    transaction: "BEGIN IMMEDIATE",
    requiredBefore: [
      "enterVerifySourceIndex", "enterVerifySource",
      "everyProjectionOrDerivedVerificationBatch", "finalPublication",
    ],
    equal: "continue",
    less: "contractError",
    greater: "sameSingletonPendingIndexSourceNewCutoffZeroUnexhaustedNoFailure",
    frozenMaterializationPhases: "mayContinueAtExistingCutoff",
    appendSerialization: "BEGIN IMMEDIATESerializesAppendDuringBatch",
    normalWorkerCheckState: "pendingBeforeArm",
    armToBatch: "sameSynchronousSidecarOwnerImmediatelyEntersBatch",
    attemptingCatchUpReset: "forbidden",
    attemptingOnReopen: "failClosed",
  },
  verification: {
    verifySourceIndex: {
      boundedBy: "eventSourceIndexEventRowidAtOrBelowCutoff",
      requires: [
        "oneExactIndexRowPerEvent", "exactIdentityAndAggregateTuple",
        "exactNullableMessageId", "noSourceLessIndexRow",
      ],
    },
    verifySource: {
      boundedBy: "eventsRowidAtOrBelowCutoff",
      requires: [
        "exactAggregateSequenceAndPredecessor", "requiredDerivedPresence",
        "latestEventHasProjectionRow", "appInstanceSourceHasAppInstancesRow",
      ],
      forbidden: ["liveProjectionTableFullScan", "antiJoin"],
    },
    projectionPhaseTables: {
      verifyProjects: "projects",
      verifyChats: "chats",
      verifyRuns: "runs",
      verifyLocks: "locks",
      verifyAppInstances: "app_instances",
    },
    projectionRule:
      "eachRowMatchesLatestExactSourceEventAtOrBelowCutoffIncludingMirroredAndIndexedColumns",
    projectionColumns: {
      projects: ["project_id", "canonical_path", "projection_json", "last_event_sequence"],
      chats: ["chat_id", "project_id", "projection_json", "last_event_sequence"],
      runs: [
        "run_id", "chat_id", "state", "worktree_path", "projection_json",
        "last_event_sequence",
      ],
      locks: [
        "lock_id", "project_id", "writer_run_id", "worktree_path", "poisoned",
        "recovery_required", "evidence_json", "projection_json", "last_event_sequence",
      ],
      app_instances: [
        "sidecar_instance_id", "pid", "platform", "started_at", "heartbeat_at",
        "clean_shutdown_at", "last_event_sequence",
      ],
    },
    derivedPhaseTables: {
      verifyReceipts: "operation_receipts",
      verifyMessages: "chat_messages",
      verifyWorkspaces: "prepared_workspaces",
      verifyBaselines: "ledger_baselines",
    },
    missingProjectionDetection: "verifySourceLatestSourcePresence",
    extraOrSourceLessDetection: "exactProjectionTablePhase",
    historicalBaseline: "allowedWhenDeterministicDigestValidAndSourceBacked",
    newestBaseline: "requiredForEveryCompactableAggregate",
  },
  snapshotAggregates: {
    includedProjectionTables: ["projects", "chats", "runs", "locks"],
    excludedProjectionTables: ["app_instances", "event_source_index"],
    exclusionReason: "lifecycleAndSourceIndexTablesAreNotMaterializedSnapshotAggregates",
  },
  transaction: {
    arm: "pendingToAttemptingCommittedBeforeBatch",
    success: "verifiedRowsCursorAndAttemptingToPendingAtomic",
    nonemptyBatchScanExhausted: false,
    emptyBatchScanExhausted: "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
    cancellation: "rollbackBatchOrSavepointThenCommitAttemptingToPending",
    terminalValidationFailure: "rollbackBatchThenCommitAttemptingToFailed",
    failureRecordUnavailable: "durableAttemptingSentinelReopenRefusesRetryOrSuccess",
    reopenAttempting: "failClosedNonRunnable",
    reopenFailed: "failClosedNonRunnable",
  },
  finalPublication: {
    transaction: "shortBeginImmediateOnly",
    preconditions: [
      "exactMetaJobPairAndCursor",
      "verifyBaselinesTableEndProvedByExactEmptyKeysetBatch",
      "scanExhaustedTrue", "maxEventRowidEqualsCutoff", "cancelBeforeMutation",
      "precommitCancel",
    ],
    mutation: "deleteJobAndSetMetaReady",
    forbiddenWork: ["artifactScan", "tableScan", "count"],
  },
  activation: {
    cleanupEnabled: false,
    deletionEnabled: false,
    compactionEnabled: false,
    wireEnabled: false,
    schedulerEnabled: false,
  },
} as const;

export function validateStorageLifetimeBatchPolicyV4(value: unknown): Record<string, unknown> {
  const policy = requireObject(value, "storage lifetime v6 indexed bounded policy");
  if (!sameContractValue(policy, STORAGE_LIFETIME_BATCH_POLICY_V4)) {
    fail("storage lifetime v6 policy contradicts indexed bounded verification");
  }
  return policy;
}

export function validateStorageEventSourceIndexEntryV1(
  value: unknown,
): StorageEventSourceIndexEntryV1 {
  const entry = requireObject(value, "event source index entry");
  exactKeys(entry, [
    "eventRowid", "eventId", "operationId", "aggregateType", "aggregateId",
    "aggregateSequence", "messageId",
  ]);
  const eventRowid = requireSafeUnsigned(entry.eventRowid, "sourceIndex.eventRowid");
  const aggregateSequence = requireSafeUnsigned(
    entry.aggregateSequence,
    "sourceIndex.aggregateSequence",
  );
  if (eventRowid === 0 || aggregateSequence === 0
    || ![entry.eventId, entry.operationId, entry.aggregateType, entry.aggregateId]
      .every((field) => typeof field === "string" && field.length > 0)
    || !(entry.messageId === null
      || (typeof entry.messageId === "string" && entry.messageId.length > 0))) {
    fail("event source index identity and aggregate tuple must be non-empty and positive");
  }
  return entry as unknown as StorageEventSourceIndexEntryV1;
}

export function validateStorageEventSourceIndexBindingV1(
  eventRowid: number,
  entryValue: unknown,
  source: OperationCandidateV1,
): StorageEventSourceIndexEntryV1 {
  const entry = validateStorageEventSourceIndexEntryV1(entryValue);
  let payload: unknown;
  try {
    payload = JSON.parse(source.canonical_json_payload);
  } catch {
    fail("source index event payload must be valid JSON");
  }
  const object = requireObject(payload, "source index event payload");
  let expectedMessageId: string | null = null;
  if (source.aggregate_type === "chat" && object.last_message !== undefined
    && object.last_message !== null) {
    const message = requireObject(object.last_message, "chat source last_message");
    if (typeof message.messageId !== "string" || message.messageId.length === 0) {
      fail("chat source index requires exact non-empty last_message.messageId");
    }
    expectedMessageId = message.messageId;
  }
  if (entry.eventRowid !== eventRowid
    || entry.eventId !== source.event_id
    || entry.operationId !== source.operation_id
    || entry.aggregateType !== source.aggregate_type
    || entry.aggregateId !== source.aggregate_id
    || entry.aggregateSequence !== source.aggregate_sequence
    || entry.messageId !== expectedMessageId) {
    fail("event source index must match one exact source event and message provenance");
  }
  return entry;
}

export function validateStorageLifetimeWorkspaceProjectSourceV1(
  projectId: string,
  sourceAggregateType: string,
  sourceAggregateId: string,
  canonicalPayload: string,
): Record<string, unknown> {
  if (projectId.length === 0 || sourceAggregateType !== "project" || sourceAggregateId !== projectId) {
    fail("workspace provenance requires the indexed latest source for the exact project identity");
  }
  if (new TextEncoder().encode(canonicalPayload).byteLength
    > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1) {
    fail("workspace project source payload exceeds 1 MiB");
  }
  let payload: unknown;
  try {
    payload = JSON.parse(canonicalPayload);
  } catch {
    fail("workspace project source payload must be valid JSON");
  }
  const object = requireObject(payload, "workspace project source payload");
  if (canonicalJsonV1(payload) !== canonicalPayload || object.project_id !== projectId
    || typeof object.canonical_path !== "string" || object.canonical_path.length === 0) {
    fail("workspace provenance requires an exact canonical project payload");
  }
  return object;
}

export function validateStorageLifetimeCursorV4(value: unknown): StorageLifetimeCursorV4 {
  const cursor = requireObject(value, "storage lifetime v6 cursor");
  exactKeys(cursor, [
    "schemaVersion", "cutoffEventRowid", "phase", "afterScanRowid", "scanExhausted",
  ]);
  if (cursor.schemaVersion !== STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4) {
    fail("storage lifetime v6 cursor schemaVersion must be 4");
  }
  const cutoff = requireSafeUnsigned(cursor.cutoffEventRowid, "cursor.cutoffEventRowid");
  const after = requireSafeUnsigned(cursor.afterScanRowid, "cursor.afterScanRowid");
  if (typeof cursor.scanExhausted !== "boolean") {
    fail("storage lifetime v6 cursor scanExhausted must be boolean");
  }
  if (!STORAGE_LIFETIME_PHASES_V4.includes(cursor.phase as StorageLifetimePhaseV4)) {
    fail("storage lifetime v6 cursor phase is invalid");
  }
  if (STORAGE_LIFETIME_EVENT_ROWID_PHASES_V4.includes(
    cursor.phase as typeof STORAGE_LIFETIME_EVENT_ROWID_PHASES_V4[number],
  ) && after > cutoff) {
    fail("event-derived storage lifetime v6 cursor cannot exceed cutoffEventRowid");
  }
  return cursor as unknown as StorageLifetimeCursorV4;
}

export function validateStorageLifetimeCursorTransitionV4(
  previousValue: unknown | null,
  nextValue: unknown,
): StorageLifetimeCursorV4 {
  const next = validateStorageLifetimeCursorV4(nextValue);
  if (previousValue === null) {
    if (next.phase !== "indexSource" || next.afterScanRowid !== 0 || next.scanExhausted) {
      fail("storage lifetime v6 initial cursor must start unexhausted indexSource at zero");
    }
    return next;
  }
  const previous = validateStorageLifetimeCursorV4(previousValue);
  if (next.cutoffEventRowid !== previous.cutoffEventRowid) {
    fail("storage lifetime v6 batch transition cannot change the frozen cutoff");
  }
  if (next.phase === previous.phase) {
    if (previous.scanExhausted || next.scanExhausted
      || next.afterScanRowid <= previous.afterScanRowid) {
      fail("storage lifetime v6 same-phase cursor must advance afterScanRowid");
    }
    return next;
  }
  const previousIndex = STORAGE_LIFETIME_PHASES_V4.indexOf(previous.phase);
  if (!previous.scanExhausted || next.scanExhausted
    || STORAGE_LIFETIME_PHASES_V4[previousIndex + 1] !== next.phase
    || next.afterScanRowid !== 0) {
    fail("storage lifetime v6 phase transition requires exhausted current phase and unexhausted exact next phase at zero");
  }
  return next;
}

export function validateStorageLifetimeMaterializationJobV4(
  value: unknown,
): StorageLifetimeMaterializationJobV4 {
  const job = requireObject(value, "storage lifetime v6 materialization job");
  exactKeys(job, [
    "singleton", "jobId", "state", "cutoffEventRowid", "phase", "afterScanRowid",
    "scanExhausted", "failureKind", "createdAt", "updatedAt", "completedAt",
  ]);
  if (job.singleton !== 1 || typeof job.jobId !== "string" || job.jobId.length === 0) {
    fail("storage lifetime v6 job requires singleton 1 and non-empty jobId");
  }
  if (!STORAGE_LIFETIME_JOB_STATES_V2.includes(job.state as StorageLifetimeJobStateV2)) {
    fail("storage lifetime v6 job state is invalid");
  }
  validateStorageLifetimeCursorV4({
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterScanRowid: job.afterScanRowid,
    scanExhausted: job.scanExhausted,
  });
  requireTimestamp(job.createdAt, "job.createdAt");
  requireTimestamp(job.updatedAt, "job.updatedAt");
  if (job.state === "failed") {
    if (typeof job.failureKind !== "string"
      || !/^[A-Za-z][A-Za-z0-9._-]{0,63}$/.test(job.failureKind)) {
      fail("failed storage lifetime v6 job requires sanitized failureKind");
    }
    requireTimestamp(job.completedAt, "job.completedAt");
  } else if (job.failureKind !== null || job.completedAt !== null) {
    fail("pending or attempting storage lifetime v6 job cannot carry terminal evidence");
  }
  return job as unknown as StorageLifetimeMaterializationJobV4;
}

export function validateStorageLifetimeMaterializationPairV4(
  value: unknown,
): StorageLifetimeMaterializationSnapshotV4 {
  const snapshot = requireObject(value, "storage lifetime v6 materialization pair");
  exactKeys(snapshot, ["materializationState", "job"]);
  if (!STORAGE_MATERIALIZATION_STATES_V1.includes(
    snapshot.materializationState as StorageMaterializationStateV1,
  )) fail("storage materialization state is invalid");
  const job = snapshot.job === null ? null : validateStorageLifetimeMaterializationJobV4(snapshot.job);
  if (!(snapshot.materializationState === "pending"
    || (snapshot.materializationState === "ready" && job === null))) {
    fail("storage lifetime v6 meta/job pairing is fail-closed");
  }
  return { materializationState: snapshot.materializationState, job } as
    StorageLifetimeMaterializationSnapshotV4;
}

export function validateStorageLifetimeReopenPairV4(
  value: unknown,
): StorageLifetimeMaterializationSnapshotV4 {
  const snapshot = validateStorageLifetimeMaterializationPairV4(value);
  if (snapshot.job?.state === "attempting" || snapshot.job?.state === "failed") {
    fail("storage lifetime v6 reopen refuses attempting or failed job");
  }
  return snapshot;
}

export function validateStorageLifetimeLifecycleTransitionV4(
  operation: StorageLifetimeLifecycleOperationV4,
  previousValue: unknown,
  nextValue: unknown,
): StorageLifetimeMaterializationSnapshotV4 {
  const previous = validateStorageLifetimeMaterializationPairV4(previousValue);
  const next = validateStorageLifetimeMaterializationPairV4(nextValue);
  const previousJob = previous.job;
  const nextJob = next.job;
  if (operation === "create") {
    if (previous.materializationState !== "pending" || previousJob !== null
      || next.materializationState !== "pending" || nextJob?.state !== "pending") {
      fail("storage lifetime v6 create requires pending absent to pending job");
    }
    validateStorageLifetimeCursorTransitionV4(null, jobCursorV4(nextJob));
    return next;
  }
  if (operation === "append") {
    const invalidateReady = previous.materializationState === "ready" && previousJob === null
      && next.materializationState === "pending" && nextJob === null;
    if (!invalidateReady && !sameContractValue(previous, next)) {
      fail("storage lifetime v6 append must atomically index source and invalidate ready or preserve pending exactly");
    }
    return next;
  }
  if (operation === "failureRecordUnavailable") {
    if (previousJob?.state !== "attempting" || !sameContractValue(previous, next)) {
      fail("failure record unavailable must leave the durable attempting sentinel unchanged");
    }
    return next;
  }
  if (operation === "finalPublish") {
    if (previousJob?.state !== "pending" || previousJob.phase !== "verifyBaselines"
      || !previousJob.scanExhausted || next.materializationState !== "ready" || nextJob !== null) {
      fail("storage lifetime v6 final publication requires exhausted verifyBaselines");
    }
    return next;
  }
  if (previousJob === null || nextJob === null) {
    fail(`storage lifetime v6 ${operation} requires a pending meta/job pair`);
  }
  requireSameJobIdentityV4(previousJob, nextJob);
  if (operation === "arm") {
    if (previousJob.state !== "pending" || nextJob.state !== "attempting"
      || previousJob.scanExhausted || nextJob.scanExhausted
      || !sameContractValue(jobCursorV4(previousJob), jobCursorV4(nextJob))) {
      fail("storage lifetime v6 arm must preserve an unexhausted cursor");
    }
  } else if (operation === "batchSuccess") {
    if (previousJob.state !== "attempting" || nextJob.state !== "pending") {
      fail("storage lifetime v6 batch success must commit attempting to pending");
    }
    validateStorageLifetimeCursorTransitionV4(jobCursorV4(previousJob), jobCursorV4(nextJob));
  } else if (operation === "phaseAdvance") {
    if (previousJob.state !== "pending" || nextJob.state !== "pending") {
      fail("storage lifetime v6 phase advance requires pending jobs");
    }
    validateStorageLifetimeCursorTransitionV4(jobCursorV4(previousJob), jobCursorV4(nextJob));
  } else if (operation === "cancel") {
    if (previousJob.state !== "attempting" || nextJob.state !== "pending"
      || !sameContractValue(jobCursorV4(previousJob), jobCursorV4(nextJob))) {
      fail("storage lifetime v6 cancellation must restore the same cursor");
    }
  } else if (operation === "fail") {
    if (previousJob.state !== "attempting" || nextJob.state !== "failed"
      || !sameContractValue(jobCursorV4(previousJob), jobCursorV4(nextJob))) {
      fail("storage lifetime v6 terminal failure must preserve the cursor");
    }
  } else if (operation === "catchUpReset") {
    if (previousJob.state !== "pending" || !isVerificationPhaseV4(previousJob.phase)
      || nextJob.state !== "pending" || nextJob.cutoffEventRowid <= previousJob.cutoffEventRowid
      || nextJob.phase !== "indexSource" || nextJob.afterScanRowid !== 0
      || nextJob.scanExhausted) {
      fail("storage lifetime v6 catch-up reset requires verification drift and a larger cutoff");
    }
  } else {
    fail("storage lifetime v6 lifecycle operation is invalid");
  }
  return next;
}

function isVerificationPhaseV4(phase: StorageLifetimePhaseV4): boolean {
  return !["indexSource", "receipts", "messages", "workspaces", "baselines"].includes(phase);
}

function jobCursorV4(job: StorageLifetimeMaterializationJobV4): StorageLifetimeCursorV4 {
  return {
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V4,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterScanRowid: job.afterScanRowid,
    scanExhausted: job.scanExhausted,
  };
}

function requireSameJobIdentityV4(
  previous: StorageLifetimeMaterializationJobV4,
  next: StorageLifetimeMaterializationJobV4,
): void {
  if (previous.singleton !== next.singleton || previous.jobId !== next.jobId
    || previous.createdAt !== next.createdAt) {
    fail("storage lifetime v6 transition cannot replace job identity");
  }
}

function jobCursorV2(job: StorageLifetimeMaterializationJobV2): StorageLifetimeCursorV2 {
  return {
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V2,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterScanRowid: job.afterScanRowid,
    scanExhausted: job.scanExhausted,
  };
}

function requireSameJobIdentityV2(
  previous: StorageLifetimeMaterializationJobV2,
  next: StorageLifetimeMaterializationJobV2,
): void {
  if (previous.singleton !== next.singleton || previous.jobId !== next.jobId
    || previous.createdAt !== next.createdAt) {
    fail("storage lifetime v4 transition cannot replace job identity");
  }
}

export function validateStorageStatusRequestV1(value: unknown): StorageStatusRequestV1 {
  const request = requireObject(value, "storage status request");
  exactKeys(request, ["schemaVersion"]);
  if (request.schemaVersion !== STORAGE_STATUS_SCHEMA_VERSION_V1) {
    fail("storage status request schemaVersion must be 1");
  }
  return request as unknown as StorageStatusRequestV1;
}

export function validateStorageStatusV1(value: unknown): StorageStatusV1 {
  const status = requireObject(value, "storage status");
  exactKeys(status, [
    "schemaVersion",
    "collectedAt",
    "database",
    "ledgerTail",
    "snapshotAggregates",
    "pressureFacts",
    "compaction",
    "physicalCleanup",
  ]);
  if (status.schemaVersion !== STORAGE_STATUS_SCHEMA_VERSION_V1) {
    fail("storage status schemaVersion must be 1");
  }
  requireTimestamp(status.collectedAt, "collectedAt");

  const database = requireObject(status.database, "storage status database");
  exactKeys(database, ["logicalBytes", "physicalBytes", "walBytes"]);
  requireSafeUnsigned(database.logicalBytes, "database.logicalBytes");
  requireSafeUnsigned(database.physicalBytes, "database.physicalBytes");
  requireSafeUnsigned(database.walBytes, "database.walBytes");

  const ledgerTail = requireObject(status.ledgerTail, "storage status ledger tail");
  exactKeys(ledgerTail, ["eventCount", "payloadBytes"]);
  requireSafeUnsigned(ledgerTail.eventCount, "ledgerTail.eventCount");
  requireSafeUnsigned(ledgerTail.payloadBytes, "ledgerTail.payloadBytes");

  const snapshots = requireObject(status.snapshotAggregates, "storage status snapshot aggregates");
  exactKeys(snapshots, ["count", "payloadBytes"]);
  requireSafeUnsigned(snapshots.count, "snapshotAggregates.count");
  requireSafeUnsigned(snapshots.payloadBytes, "snapshotAggregates.payloadBytes");

  validatePressureFacts(status.pressureFacts);
  validateCompactionStatus(status.compaction);

  const cleanup = requireObject(status.physicalCleanup, "storage status physical cleanup");
  exactKeys(cleanup, ["enabled", "reason"]);
  if (cleanup.enabled !== false || cleanup.reason !== "userPolicyNotConfigured") {
    fail("physical cleanup must remain disabled until the user policy is decided");
  }
  return status as unknown as StorageStatusV1;
}

export function classifyOperationAgainstReceiptV2(
  receiptValue: unknown,
  incomingValue: unknown,
): ReceiptOperationDecisionV2 {
  const receipt = validateOperationReceiptV2(receiptValue);
  const incoming = validateOperationCandidateV1(incomingValue);
  if (receipt.operationId !== incoming.operation_id) return "append";
  return receipt.eventId === incoming.event_id
    && receipt.aggregateType === incoming.aggregate_type
    && receipt.aggregateId === incoming.aggregate_id
    && receipt.aggregateSequence === incoming.aggregate_sequence
    && receipt.eventType === incoming.event_type
    && receipt.payloadSchemaVersion === incoming.payload_schema_version
    && receipt.canonicalPayloadJson === incoming.canonical_json_payload
    && receipt.eventTimestamp === incoming.timestamp
    ? "duplicateNoOp"
    : "conflict";
}

function validateOperationReceiptV2(value: unknown): OperationReceiptV2 {
  const receipt = requireObject(value, "operation receipt");
  exactKeys(receipt, [
    "operationId", "eventId", "aggregateType", "aggregateId", "aggregateSequence",
    "eventType", "payloadSchemaVersion", "canonicalPayloadJson",
    "canonicalPayloadDigestAlgorithm", "canonicalPayloadDigestVersion",
    "canonicalPayloadDigestFormat", "canonicalPayloadDigest", "eventTimestamp",
  ]);
  for (const field of ["operationId", "eventId", "aggregateType", "aggregateId", "eventType", "canonicalPayloadJson"] as const) {
    if (typeof receipt[field] !== "string" || receipt[field].length === 0) {
      fail(`operation receipt ${field} is required`);
    }
  }
  if (requireSafeUnsigned(receipt.aggregateSequence, "operation receipt aggregateSequence") === 0) {
    fail("operation receipt aggregateSequence must be positive");
  }
  if (receipt.payloadSchemaVersion !== 1
    || receipt.canonicalPayloadDigestAlgorithm !== OPERATION_RECEIPT_DIGEST_ALGORITHM_V2
    || receipt.canonicalPayloadDigestVersion !== OPERATION_RECEIPT_DIGEST_VERSION_V2
    || receipt.canonicalPayloadDigestFormat !== OPERATION_RECEIPT_DIGEST_FORMAT_V2
    || typeof receipt.canonicalPayloadDigest !== "string"
    || !/^[0-9a-f]{64}$/.test(receipt.canonicalPayloadDigest)) {
    fail("operation receipt digest metadata must be sha256 v1 lowercaseHex64");
  }
  let payload: unknown;
  try {
    payload = JSON.parse(receipt.canonicalPayloadJson as string);
  } catch {
    fail("operation receipt canonicalPayloadJson must be valid JSON");
  }
  if (canonicalJsonV1(payload) !== receipt.canonicalPayloadJson) {
    fail("operation receipt canonicalPayloadJson must be canonical JSON");
  }
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    fail("operation receipt canonicalPayloadJson must be an object");
  }
  const digest = sha256FileVersionV1(new TextEncoder().encode(receipt.canonicalPayloadJson as string));
  if (digest !== `sha256:${receipt.canonicalPayloadDigest as string}`) {
    fail("operation receipt canonicalPayloadDigest does not match canonicalPayloadJson");
  }
  requireTimestamp(receipt.eventTimestamp, "operation receipt eventTimestamp");
  return receipt as unknown as OperationReceiptV2;
}

function validateOperationCandidateV1(value: unknown): OperationCandidateV1 {
  const event = requireObject(value, "operation candidate");
  exactKeys(event, [
    "event_id", "operation_id", "request_id", "aggregate_type", "aggregate_id",
    "aggregate_sequence", "event_type", "payload_schema_version",
    "canonical_json_payload", "timestamp",
  ]);
  for (const field of ["event_id", "operation_id", "aggregate_type", "aggregate_id", "event_type", "canonical_json_payload"] as const) {
    if (typeof event[field] !== "string" || event[field].length === 0) {
      fail(`operation candidate ${field} is required`);
    }
  }
  if (event.request_id !== null && (typeof event.request_id !== "string" || event.request_id.length === 0)) {
    fail("operation candidate request_id must be null or non-empty");
  }
  if (requireSafeUnsigned(event.aggregate_sequence, "operation candidate aggregate_sequence") === 0) {
    fail("operation candidate aggregate_sequence must be positive");
  }
  if (event.payload_schema_version !== 1) fail("operation candidate payload_schema_version must be 1");
  requireTimestamp(event.timestamp, "operation candidate timestamp");
  return event as unknown as OperationCandidateV1;
}

function validatePressureFacts(value: unknown): void {
  const facts = requireObject(value, "storage pressure facts");
  exactKeys(facts, ["collectionStatus", "volumeCapacityBytes", "volumeAvailableBytes"]);
  if (facts.collectionStatus === "available") {
    const capacity = requireSafeUnsigned(facts.volumeCapacityBytes, "pressureFacts.volumeCapacityBytes");
    const available = requireSafeUnsigned(facts.volumeAvailableBytes, "pressureFacts.volumeAvailableBytes");
    if (capacity === 0 || available > capacity) {
      fail("available pressure facts must have positive capacity and available bytes within capacity");
    }
  } else if (facts.collectionStatus === "unavailable") {
    if (facts.volumeCapacityBytes !== null || facts.volumeAvailableBytes !== null) {
      fail("unavailable pressure facts cannot include partial volume values");
    }
  } else {
    fail("storage pressure collectionStatus is invalid");
  }
}

function validateCompactionStatus(value: unknown): void {
  const status = requireObject(value, "storage compaction status");
  exactKeys(status, ["lastAttemptStatus", "lastAttemptedAt", "lastSuccessfulAt"]);
  if (status.lastAttemptStatus === "neverRun") {
    if (status.lastAttemptedAt !== null || status.lastSuccessfulAt !== null) {
      fail("neverRun compaction cannot include attempt timestamps");
    }
    return;
  }
  if (status.lastAttemptStatus === "succeeded") {
    requireTimestamp(status.lastAttemptedAt, "compaction.lastAttemptedAt");
    requireTimestamp(status.lastSuccessfulAt, "compaction.lastSuccessfulAt");
    if (status.lastAttemptedAt !== status.lastSuccessfulAt) {
      fail("succeeded compaction must report the same attempt and success time");
    }
    return;
  }
  if (status.lastAttemptStatus === "failed") {
    requireTimestamp(status.lastAttemptedAt, "compaction.lastAttemptedAt");
    if (status.lastSuccessfulAt !== null) {
      requireTimestamp(status.lastSuccessfulAt, "compaction.lastSuccessfulAt");
      if (status.lastSuccessfulAt > status.lastAttemptedAt) {
        fail("failed compaction lastSuccessfulAt cannot be later than lastAttemptedAt");
      }
    }
    return;
  }
  fail("storage compaction lastAttemptStatus is invalid");
}

function requireTimestamp(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) {
    fail(`${name} must be an RFC 3339 UTC second timestamp`);
  }
  const [year, month, day, hour, minute, second] = [
    value.slice(0, 4),
    value.slice(5, 7),
    value.slice(8, 10),
    value.slice(11, 13),
    value.slice(14, 16),
    value.slice(17, 19),
  ].map(Number);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const monthDays = [0, 31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (year === 0 || month < 1 || month > 12 || day < 1 || day > monthDays[month]
    || hour > 23 || minute > 59 || second > 59) {
    fail(`${name} must be a real RFC 3339 UTC calendar timestamp`);
  }
}

function exactStringArray(value: unknown, expected: readonly string[]): boolean {
  return Array.isArray(value)
    && value.length === expected.length
    && value.every((entry, index) => entry === expected[index]);
}

function jobCursor(job: StorageLifetimeMaterializationJobV1): StorageLifetimeCursorV1 {
  return {
    schemaVersion: STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1,
    cutoffEventRowid: job.cutoffEventRowid,
    phase: job.phase,
    afterEventRowid: job.afterEventRowid,
  };
}

function requireSamePendingJob(
  previous: StorageLifetimeMaterializationSnapshotV1,
  next: StorageLifetimeMaterializationSnapshotV1,
  operation: string,
): [StorageLifetimeMaterializationJobV1, StorageLifetimeMaterializationJobV1] {
  if (previous.materializationState !== "pending" || next.materializationState !== "pending"
    || previous.job?.state !== "pending" || next.job?.state !== "pending") {
    fail(`storage lifetime ${operation} requires pending meta and pending singleton`);
  }
  return [previous.job, next.job];
}

function requireSameJobIdentity(
  previous: StorageLifetimeMaterializationJobV1,
  next: StorageLifetimeMaterializationJobV1,
): void {
  if (previous.singleton !== next.singleton || previous.jobId !== next.jobId
    || previous.createdAt !== next.createdAt) {
    fail("storage lifetime transition must preserve singleton job identity");
  }
}

function sameContractValue(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function nextPhase(phase: StorageLifetimePhaseV1): StorageLifetimePhaseV1 | null {
  const index = STORAGE_LIFETIME_PHASES_V1.indexOf(phase);
  return STORAGE_LIFETIME_PHASES_V1[index + 1] ?? null;
}

function fail(message: string): never {
  throw new Error(message);
}
