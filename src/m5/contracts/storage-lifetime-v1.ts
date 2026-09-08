import { requireObject, exactKeys, requireSafeUnsigned } from "./project-v1.ts";
import { sha256FileVersionV1 } from "./file-v1.ts";
import { canonicalJsonV1 } from "./storage-v1.ts";
import { validateTeamMessageV1 } from "./run-recovery-v1.ts";

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
export const STORAGE_LIFETIME_SCHEMA_VERSION_V7 = 7 as const;
export const STORAGE_LIFETIME_SCHEMA_VERSION_V8 = 8 as const;
export const STORAGE_LIFETIME_SCHEMA_VERSION_V9 = 9 as const;
export const STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V1 = 1 as const;
export const STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V2 = 2 as const;
export const STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V3 = 3 as const;
export const STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3 = 3 as const;
export const STORAGE_COMPACTION_PLAN_SCHEMA_VERSION_V1 = 1 as const;
export const STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1 = 1_000 as const;
export const STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1 = 1_001 as const;
export const STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1 = 100 as const;
export const STORAGE_COMPACTION_MAX_DELETE_ROWS_V1 = 100 as const;
export const STORAGE_COMPACTION_PHASES_V1 = [
  "freezeMessages", "verifyPrefix", "authorizeDelete", "deletePrefix",
  "verifyEquivalence", "finalize",
] as const;
export const STORAGE_COMPACTION_PHASES_V2 = [
  "freezeOperations", "freezeMessages", "verifyPrefix", "authorizeDelete",
  "deletePrefix", "verifyOperations", "verifyEquivalence", "finalize",
] as const;
export const STORAGE_COMPACTION_PHASES_V3 = [
  "planEligibility", "freezeOperations", "freezeMessages", "verifyPrefix",
  "authorizeDelete", "deletePrefix", "verifyOperations", "verifyEquivalence",
  "releasePlan", "finalize",
] as const;
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
  "aggregate_sequence", "introduced_message_id", "chat_durable_sequence",
  "chat_snapshot_message_id",
] as const;
export const OPERATION_RECEIPT_DIGEST_ALGORITHM_V2 = "sha256" as const;
export const OPERATION_RECEIPT_DIGEST_VERSION_V2 = 1 as const;
export const OPERATION_RECEIPT_DIGEST_FORMAT_V2 = "lowercaseHex64" as const;
export const STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1 = 1 as const;
export const STORAGE_LIFETIME_BATCH_POLICY_SCHEMA_VERSION_V1 = 1 as const;
export const STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1 = 100 as const;
export const STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1 = 1_048_576 as const;
export const STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1 = 8_388_608 as const;
export const STORAGE_LOSSLESS_COMPACTION_MIN_ELIGIBLE_LEDGER_EVENTS_V1 = 1_000 as const;
export const STORAGE_LIFETIME_ACTIVATION_PHASES_V1 = [
  "pending",
  "armRunning",
  "freeze",
  "materialize",
  "verify",
  "delete",
  "finalize",
] as const;
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
  introducedMessageId: string | null;
  chatDurableSequence: number | null;
  chatSnapshotMessageId: string | null;
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

export interface StorageLifetimeV7PreflightPolicyV1 {
  schema: "exactStorageV6";
  admissibleMaterializationPairs: readonly [
    "ready+absent", "pending+absent", "pending+pending", "pending+failed",
  ];
  rejectedMaterializationPairs: readonly ["pending+attempting"];
  maintenanceRequirement: "noPendingOrRunningLosslessCompaction";
  deleteAuthorityRequirement: "storageV7AuthorityObjectsAbsent";
  partialExtraOrUnknownSchema: "failClosedBeforeMutation";
  mapping: readonly [
    "materializationStatePreserved", "materializationJobPreservedByteExact",
    "completedAndFailedMaintenanceHistoryPreserved", "existingRowsNotBackfilled",
    "existingLedgerAndDerivedRowsNotDeleted",
  ];
  timing: "beforeAnyDdlOrMutation";
}

export interface StorageLifetimeV8PreflightPolicyV1 {
  schema: "exactStorageV7";
  requiredLedgerState: "uncompactedOnly";
  admissibleMaterializationPairs: readonly [
    "ready+absent", "pending+absent", "pending+pending", "pending+failed",
  ];
  rejectedMaterializationPairs: readonly ["pending+attempting"];
  maintenanceRequirement: "noPendingOrRunningLosslessCompaction";
  emptyTables: readonly [
    "ledger_compaction_prefixes", "frozen_message_sources",
    "ledger_compaction_delete_authority",
  ];
  targetObjectRequirement: "storageV8ObjectsAbsent";
  partialExtraOrTamperedSchema: "failClosedBeforeMutation";
  mapping: readonly [
    "materializationPairPreservedByteExact",
    "terminalMaintenanceHistoryPreservedByteExact",
    "allExistingRowsAndTriggersPreservedExceptNamedVersionedGuards",
    "existingHistoryNotBackfilled",
  ];
  timing: "beforeAnyDdlOrMutation";
}

export type StorageCompactionPhaseV1 = typeof STORAGE_COMPACTION_PHASES_V1[number];
export type StorageCompactionJobStateV1 = "pending" | "running" | "succeeded" | "failed";
export type StorageCompactionLifecycleOperationV1 =
  | "arm" | "batchSuccess" | "cancelBeforeArm" | "cancelAfterArm" | "fail" | "complete";

export interface StorageCompactionCursorV1 {
  schemaVersion: typeof STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V1;
  cutoffEventRowid: number;
  phase: StorageCompactionPhaseV1;
  afterEventRowid: number;
  scanExhausted: boolean;
}

export type StorageCompactionPhaseV2 = typeof STORAGE_COMPACTION_PHASES_V2[number];

export interface StorageCompactionCursorV2 {
  schemaVersion: typeof STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V2;
  cutoffEventRowid: number;
  phase: StorageCompactionPhaseV2;
  afterEventRowid: number;
  scanExhausted: boolean;
}

export interface StorageCompactionProgressV2 {
  rowsInBatch: number;
  canonicalPayloadBytes: number;
}

export type StorageCompactionLifecycleOperationV2 =
  | "arm" | "batchSuccess" | "cancelBeforeArm" | "cancelAfterArm"
  | "commitUncertain" | "complete";

export interface StorageCompactionJobV2 {
  jobId: string;
  state: "pending" | "running" | "succeeded" | "failed";
  cursor: StorageCompactionCursorV2;
  authorityCount: number;
  operationsVerified: boolean;
  equivalenceVerified: boolean;
  failureKind: string | null;
  completedAt: string | null;
}

export type StorageCompactionPhaseV3 = typeof STORAGE_COMPACTION_PHASES_V3[number];
export type StorageCompactionPlanDecisionV1 = "delete" | "retainAnchor";

export interface StorageCompactionCursorV3 {
  schemaVersion: typeof STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V3;
  cutoffEventRowid: number;
  phase: StorageCompactionPhaseV3;
  afterPlanOrdinal: number;
  stepThroughPlanOrdinal: number | null;
  scanExhausted: boolean;
}

export interface StorageCompactionPlanV1 {
  jobId: string;
  planSchemaVersion: typeof STORAGE_COMPACTION_PLAN_SCHEMA_VERSION_V1;
  cutoffEventRowid: number;
  aggregateType: "project" | "chat" | "run" | "lock";
  aggregateId: string;
  retainedBoundarySequence: number;
  deleteThroughSequence: number;
  anchorAggregateSequence: number;
  expectedRows: typeof STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1;
  baselineId: string;
  /** Raw lowercase SHA-256 hex; the existing baseline row stores `sha256:${string}`. */
  baselineProjectionDigest: string;
  previousSuccessfulJobId: string | null;
  createdAt: string;
  plannedAt: string;
}

export interface StorageCompactionPlanRowV1 {
  jobId: string;
  ordinal: number;
  eventRowid: number;
  eventId: string;
  operationId: string;
  requestId: string | null;
  aggregateType: "project" | "chat" | "run" | "lock";
  aggregateId: string;
  aggregateSequence: number;
  eventType: string;
  payloadSchemaVersion: 1;
  eventTimestamp: string;
  canonicalPayloadJson: string;
  /** Raw lowercase SHA-256 hex, without an algorithm prefix. */
  canonicalPayloadDigest: string;
  introducedMessageId: string | null;
  /** Raw lowercase SHA-256 hex; existing message/frozen rows store `sha256:${string}`. */
  introducedMessageDigest: string | null;
  decision: StorageCompactionPlanDecisionV1;
  baselineId: string | null;
  /** Raw lowercase SHA-256 hex, without an algorithm prefix. */
  baselineProjectionDigest: string | null;
  operationFrozenBeforePlan: boolean;
  messageFrozenBeforePlan: boolean;
}

export interface StorageCompactionPrivateStateV3 {
  schemaVersion: typeof STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3;
  cursor: StorageCompactionCursorV3;
  planRowsComplete: boolean;
  operationsVerified: boolean;
  equivalenceVerified: boolean;
  planRowsReleased: boolean;
}

export interface StorageCompactionAdmissionStateV3 {
  jobId: string;
  state: "pending" | "running" | "succeeded" | "failed";
  privateState: StorageCompactionPrivateStateV3;
  authorityCount: number;
  planHeaderPresent: boolean;
}

export type StorageCompactionLifecycleOperationV3 =
  | "arm" | "batchSuccess" | "cancelBeforeArm" | "cancelAfterArm"
  | "commitUncertain" | "complete" | "fail";

export interface StorageCompactionJobV3 {
  jobId: string;
  state: "pending" | "running" | "succeeded" | "failed";
  privateState: StorageCompactionPrivateStateV3;
  authorityCount: number;
  failureKind: string | null;
  completedAt: string | null;
}

export interface FrozenOperationSourceV1 {
  operationId: string;
  sourceEventRowid: number;
  sourceEventId: string;
  requestId: string | null;
  aggregateType: "project" | "chat" | "run" | "lock";
  aggregateId: string;
  aggregateSequence: number;
  eventType: string;
  payloadSchemaVersion: 1;
  canonicalPayloadJson: string;
  canonicalPayloadDigest: string;
  eventTimestamp: string;
  canonicalRecordJson: string;
  recordDigest: string;
  frozenAt: string;
}

export interface StorageRuntimeSourceAuthorityV1 {
  aggregateType: "project" | "chat" | "run" | "lock";
  eventPresent: boolean;
  liveSourceIndexPresent: boolean;
  frozenOperationPresent: boolean;
  receiptPresent: boolean;
  liveSourceMatches: boolean;
  frozenOperationMatches: boolean;
  receiptMatches: boolean;
  chatPredecessorSource: "frozenOperationCanonicalPayload" | null;
}

export interface StorageCompactionPrefixV1 {
  prefixId: string;
  aggregateType: string;
  aggregateId: string;
  deletedThroughSequence: number;
  anchorEventRowid: number;
  anchorEventId: string;
  anchorOperationId: string;
  anchorAggregateSequence: number;
  baselineId: string;
  baselineProjectionDigest: string;
  createdAt: string;
  verifiedAt: string;
}

export interface FrozenMessageSourceV1 {
  messageId: string;
  sourceEventRowid: number;
  sourceEventId: string;
  sourceOperationId: string;
  chatId: string;
  sourceAggregateSequence: number;
  durableSequence: number;
  canonicalMessageJson: string;
  messageDigest: string;
  frozenAt: string;
}

export interface StorageCompactionDeleteAuthorityV1 {
  jobId: string;
  ordinal: number;
  prefixId: string;
  prefixDeletedThroughSequence: number;
  eventRowid: number;
  eventId: string;
  operationId: string;
  aggregateType: string;
  aggregateId: string;
  aggregateSequence: number;
  introducedMessageId: string | null;
  frozenMessageId: string | null;
  authorizedAt: string;
}

export interface StorageCompactionJobV1 {
  jobId: string;
  state: StorageCompactionJobStateV1;
  cursor: StorageCompactionCursorV1;
  authorityCount: number;
  equivalenceVerified: boolean;
  failureKind: string | null;
  completedAt: string | null;
}

export interface StorageCompactionEquivalenceV1 {
  baselineTailBefore: boolean;
  anchorTailAfter: boolean;
  preservedArtifactsByteExact: boolean;
  receiptReplayEquivalent: boolean;
  namespaceDecisionsEquivalent: boolean;
  frozenMessageIdsReserved: boolean;
  authorityEmpty: boolean;
  rowsInBatch: number;
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

export function validateStorageLifetimeV7PreflightPolicyV1(
  value: unknown,
): StorageLifetimeV7PreflightPolicyV1 {
  const policy = requireObject(value, "storage lifetime v7 preflight policy");
  const expected = {
    schema: "exactStorageV6",
    admissibleMaterializationPairs: [
      "ready+absent", "pending+absent", "pending+pending", "pending+failed",
    ],
    rejectedMaterializationPairs: ["pending+attempting"],
    maintenanceRequirement: "noPendingOrRunningLosslessCompaction",
    deleteAuthorityRequirement: "storageV7AuthorityObjectsAbsent",
    partialExtraOrUnknownSchema: "failClosedBeforeMutation",
    mapping: [
      "materializationStatePreserved", "materializationJobPreservedByteExact",
      "completedAndFailedMaintenanceHistoryPreserved", "existingRowsNotBackfilled",
      "existingLedgerAndDerivedRowsNotDeleted",
    ],
    timing: "beforeAnyDdlOrMutation",
  };
  if (!sameContractValue(policy, expected)) {
    fail("storage lifetime v7 preflight policy is not fail-closed");
  }
  return policy as unknown as StorageLifetimeV7PreflightPolicyV1;
}

export function validateStorageLifetimeV8PreflightPolicyV1(
  value: unknown,
): StorageLifetimeV8PreflightPolicyV1 {
  const policy = requireObject(value, "storage lifetime v8 preflight policy");
  const expected = {
    schema: "exactStorageV7",
    requiredLedgerState: "uncompactedOnly",
    admissibleMaterializationPairs: [
      "ready+absent", "pending+absent", "pending+pending", "pending+failed",
    ],
    rejectedMaterializationPairs: ["pending+attempting"],
    maintenanceRequirement: "noPendingOrRunningLosslessCompaction",
    emptyTables: [
      "ledger_compaction_prefixes", "frozen_message_sources",
      "ledger_compaction_delete_authority",
    ],
    targetObjectRequirement: "storageV8ObjectsAbsent",
    partialExtraOrTamperedSchema: "failClosedBeforeMutation",
    mapping: [
      "materializationPairPreservedByteExact",
      "terminalMaintenanceHistoryPreservedByteExact",
      "allExistingRowsAndTriggersPreservedExceptNamedVersionedGuards",
      "existingHistoryNotBackfilled",
    ],
    timing: "beforeAnyDdlOrMutation",
  };
  if (!sameContractValue(policy, expected)) {
    fail("storage lifetime v8 preflight policy is not fail-closed");
  }
  return policy as unknown as StorageLifetimeV8PreflightPolicyV1;
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
    introducedMessageId: "nullableOnlyWhenThisEventIntroducesExactTeamMessage",
    chatDurableSequence: "nullableExactDurableEventSequenceFromCanonicalChatPayload",
    chatSnapshotMessageId: "nullableExactLastMessageIdFromCanonicalChatPayload",
    foreignKeys: [
      "event_id->events.event_id",
      "operation_id->events.operation_id",
      "aggregateTuple->events.aggregateTuple",
    ],
    foreignKeyEnforcement: "PRAGMA foreign_keys=ONOnEveryConnection",
    insertGuard: "event_source_index_requires_exact_event",
    sameEventBinding:
      "eventRowidIdentityAggregateAndChatSnapshotFieldsMustMatchOneEventsRow",
    separateForeignKeyCrossEventMix: "rejectedByInsertGuard",
    introductionTransition: {
      firstPredecessor: "durableZeroAndSnapshotNull",
      message:
        "currentDurableEqualsPreviousPlusOneAndValidTeamMessageEventSequenceEqualsCurrentAndChatIdEqualsAggregateIdAndIntroducedEqualsSnapshotMessageId",
      nonMessage:
        "durableUnchangedAndIntroducedNullAndSnapshotNullOrSameAsPreviousSnapshot",
      reject: [
        "durableDecrease", "durableJump", "increaseWithoutValidMessage",
        "unchangedWithChangedSnapshotMessageId", "doubleIntroduction",
      ],
    },
    predecessorLookup: {
      indexed: "exactAggregateTypeIdAndAggregateSequenceMinusOne",
      indexedNonNullSnapshotProvenance:
        "exactEventsRowidLookupMustBindSnapshotChatIdAndEventSequence",
      missingDuringMigrationPendingAppend:
        "rejectOrSuspendUntilExactPredecessorSourceIndexPresent",
      rawPredecessorEventFallback: "forbidden",
      forbidden: ["scan", "sort", "latestLookup", "rawEventFallback"],
    },
    historyBackfillByMigration: false,
    historyBackfillPhase: "indexSource",
    indexSourceWrite: "insertOrExactExistingNoOpOtherwiseConflict",
    verificationPhase: "verifySourceIndex",
    aggregateLatestIndex: "event_source_index_aggregate_latest",
    introducedMessageUniqueIndex: "event_source_index_message_unique",
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
    introducedMessageIdentityConflict: "rollbackWholeAppend",
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
    introducedMessageIdentity:
      "SEARCH event_source_index USING COVERING INDEX event_source_index_message_unique (introduced_message_id=?)",
    indexedPredecessor:
      "SEARCH event_source_index USING INDEX sqlite_autoindex_event_source_index_3 (aggregate_type=? AND aggregate_id=? AND aggregate_sequence=?)",
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

const STORAGE_LOSSLESS_COMPACTION_POLICY_V1 = {
  schemaVersion: 1,
  casesFile: "cases.json",
  activation: {
    productExecutionEnabled: false,
    automaticSchedulingEnabled: false,
    actualDeleteEnabledBeforeC1Consumer: false,
    wireStatusOrControlEnabled: false,
  },
  guardVersioning: {
    schemaVersion: 7,
    migrationReplacement: "oneTimeExactV6ToV7Transaction",
    runtimeDropRecreate: "forbidden",
    deleteBatchDdl: "forbidden",
    deleteBatchMechanism: "durableRunningJobPlusEphemeralExactAuthorityRows",
  },
  prefix: {
    table: "ledger_compaction_prefixes",
    identity: "appendOnlyPrefixIdAndAggregateDeletedThroughUnique",
    range: "aggregateSequenceOneThroughDeletedThroughContinuousOnly",
    anchor: "exactSameAggregateAtDeletedThroughPlusOneRemainsInEventsAndSourceIndex",
    baseline: "immutableSameAggregateAtDeletedThroughWithExactDigest",
    progression: "newPrefixStrictlyExtendsLatestVerifiedPrefix",
    appInstance: "forbidden",
    proof: "boundedPreDeleteAndPostDeleteEquivalenceBeforePrefixInsert",
  },
  frozenMessageSource: {
    table: "frozen_message_sources",
    whenRequired: "beforeDeletingAnyEventThatIntroducedMessage",
    fields: [
      "messageId", "sourceEventRowid", "sourceEventId", "sourceOperationId",
      "chatId", "sourceAggregateSequence", "durableSequence",
      "canonicalMessageJson", "messageDigest", "frozenAt",
    ],
    binding: "exactEventSourceIndexIntroductionAndExactChatMessage",
    appendOnly: true,
    messageIdReuse: "forbiddenForeverWhileFrozenRecordExists",
  },
  deleteAuthority: {
    table: "ledger_compaction_delete_authority",
    jobState: "running",
    maxExactTuples: STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
    globalAuthorityOwner: "oneRunningJobAtATime",
    identity: "eventRowidEventIdOperationIdAndAggregateTupleExact",
    prefixBound: "sameAggregateSequenceAtOrBelowDeletedThrough",
    introducedMessage: "requiresExactFrozenMessageSource",
    appInstance: "forbidden",
    sourceIndexDeleteBeforeEventDelete: true,
    eventDeleteConsumesAuthority: true,
    mustBeEmptyBeforeCommit: true,
    mustBeEmptyOnOpen: true,
    updates: "forbidden",
  },
  cursor: {
    schemaVersion: STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V1,
    phases: STORAGE_COMPACTION_PHASES_V1,
    columns: ["cutoffEventRowid", "phase", "afterEventRowid", "scanExhausted"],
    keyset: "eventRowidGreaterThanAfterAndAtOrBelowCutoffOrderByEventRowidLimit100",
    phaseTransition: "exhaustedCurrentThenNextAtZeroUnexhausted",
    callerConfigurable: false,
    maxRowsPerBatch: STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
    unboundedScanOrTransaction: "forbidden",
  },
  lifecycle: {
    durableTable: "maintenance_jobs",
    jobKind: "losslessCompaction",
    runningUniqueness: "schemaPartialUniqueBeforeAuthority",
    c1Consumer: "requiresExactSchemaGuardNeverApplicationOnlyCheck",
    arm: "pendingToRunningSentinelCommittedBeforeDeleteBatch",
    batchSuccess: "runningToPendingWithAdvancedCursorSameDeleteTransaction",
    cancelBeforeArm: "pendingUnchangedNoAuthority",
    cancelAfterArm: "rollbackDeleteBatchLeavesRunningSentinelFailClosed",
    commitUncertain: "reopenPendingAdvancedMeansCommittedRunningMeansManualRecoveryRequired",
    terminalSuccess: "authorityEmptyAndEquivalenceVerifiedThenSucceeded",
    terminalFailure: "rollbackBatchThenRunningToFailedWithSanitizedEvidence",
    automaticRetryRunning: false,
  },
  deletion: {
    aggregateRule: "retainOneExactAnchorPerCompactedAggregate",
    eligible: "onlyContiguousVerifiedPrefixBeforeAnchor",
    appInstance: "never",
    unchangedTables: [
      "operation_receipts", "chat_messages", "prepared_workspaces", "ledger_baselines",
      "projects", "chats", "runs", "locks", "app_instances",
    ],
    snapshotFiles: "neverMutated",
    checkpointOrVacuumRequiredForSuccess: false,
  },
  equivalence: {
    maxRowsPerBatch: STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
    before: [
      "baselinePlusTailReplaysCurrentProjection",
      "operationReceiptsClassifyEveryDeletedOperation",
      "introducedMessagesHaveFrozenSources",
      "namespaceDecisionsMatchLegacyActualRecovery",
    ],
    after: [
      "anchorPlusTailReplaysCurrentProjection",
      "preservedArtifactsByteExact",
      "deletedOperationsRemainDuplicateOrConflictByReceipt",
      "deletedIntroducedMessageIdsRemainReserved",
    ],
    failure: "rollbackWholeAtMost100RowDeleteBatch",
    finalization: "exactEmptyAuthorityQueryThenSucceededSameTransaction",
  },
  replay: {
    deletedOperationAuthority: "operationReceiptsCanonicalPayloadAndIdentity",
    duplicate: "exactCanonicalReceiptMatchIsDuplicateNoOp",
    conflict: "sameOperationIdAnyImmutableDifferenceIsConflict",
    namespaces: ["legacy", "actualAgent", "recovery"],
    namespaceDecisionEquality: true,
    deletedMessageIntroduction: "frozenMessageSourceReservesMessageId",
  },
  queryPlanAcceptance: {
    mode: "exactExplainQueryPlanDetail",
    prefixLatest:
      "SEARCH ledger_compaction_prefixes USING INDEX ledger_compaction_prefixes_latest (aggregate_type=? AND aggregate_id=?)",
    deleteBatch: "SEARCH event_source_index USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)",
    authorityEmpty:
      "SCAN ledger_compaction_delete_authority USING COVERING INDEX sqlite_autoindex_ledger_compaction_delete_authority_1",
    frozenMessage:
      "SEARCH frozen_message_sources USING INDEX sqlite_autoindex_frozen_message_sources_1 (message_id=?)",
    reject: ["USE TEMP B-TREE", "SCAN events", "SCAN event_source_index"],
  },
} as const;

export function validateStorageLosslessCompactionPolicyV1(
  value: unknown,
): Record<string, unknown> {
  const policy = requireObject(value, "storage v7 lossless compaction policy");
  if (!sameContractValue(policy, STORAGE_LOSSLESS_COMPACTION_POLICY_V1)) {
    fail("storage v7 lossless compaction policy is not exact");
  }
  return policy;
}

const STORAGE_FROZEN_OPERATION_SOURCE_POLICY_V1 = {
  schemaVersion: 1,
  casesFile: "cases.json",
  table: "frozen_operation_sources",
  identity: "operationIdPrimaryKey",
  allowedAggregateTypes: ["project", "chat", "run", "lock"],
  appInstance: "forbidden",
  requestId: "nullableAndExcludedFromDuplicateConflictAuthority",
  sourceAuthority: "exactLiveEventAndExactSourceIndex",
  receiptRole: "immutableCrossCheckNeverSourceReconstruction",
  consumerIntegration: "requiredBeforeAnyProductActivation",
  immutableFields: [
    "operationId", "sourceEventRowid", "sourceEventId", "requestId",
    "aggregateType", "aggregateId", "aggregateSequence", "eventType",
    "payloadSchemaVersion", "canonicalPayloadJson", "canonicalPayloadDigest",
    "eventTimestamp", "canonicalRecordJson", "recordDigest", "frozenAt",
  ],
  payloadDigest: {
    algorithm: "sha256",
    format: "lowercaseHex64",
    sqlAuthority: "exactImmutableReceiptCrossCheck",
    consumerValidation: "recomputeBeforeInsert",
  },
  recordDigest: {
    schemaVersion: 1,
    algorithm: "sha256",
    format: "lowercaseHex64",
    canonicalEnvelope: "canonicalJsonV1AllFieldsExceptRecordDigest",
    sqlAuthority: "exactCanonicalEnvelopeTextAndDigestShape",
    consumerValidation: "recomputeBeforeInsertRequired",
    reopenValidation: "boundedVerifyOperationsBeforeRunnable",
  },
  appendOnly: true,
  receiptMutation: "updateAndDeleteForbidden",
  dependentGuards: [
    "prefixAnchorRequiresFrozenOperation",
    "frozenMessageRequiresSameFrozenOperation",
    "deleteAuthorityRequiresSameFrozenOperation",
  ],
  queryPlanAcceptance: {
    mode: "exactExplainQueryPlanDetail",
    sourceEventRowid:
      "SEARCH frozen_operation_sources USING COVERING INDEX frozen_operation_sources_source_rowid (source_event_rowid=?)",
    operationId: "SEARCH frozen_operation_sources USING PRIMARY KEY (operation_id=?)",
    aggregateTuple:
      "SEARCH frozen_operation_sources USING COVERING INDEX frozen_operation_sources_aggregate (aggregate_type=? AND aggregate_id=? AND aggregate_sequence=?)",
    reject: ["SCAN frozen_operation_sources", "USE TEMP B-TREE"],
  },
} as const;

const STORAGE_RUNTIME_SOURCE_AUTHORITY_POLICY_V1 = {
  schemaVersion: 1,
  liveEvent: "exactLiveEventPlusSourceIndexPlusReceipt",
  deletedEvent: "exactFrozenOperationPlusReceipt",
  receiptOnly: "failClosed",
  requestIdComparison: "excluded",
  deletedIntroducingMessageIdReuse: "rejectedByPreservedV7FrozenMessageGuard",
  deletedOperationDuplicateConflict: "frozenOperationPlusReceiptExactV1Semantics",
  chatPredecessor: "frozenOperationCanonicalPayloadNeverReceiptSelfReconstruction",
  namespaces: ["legacy", "actualAgent", "recovery"],
  namespaceDecisionEquality: true,
} as const;

const STORAGE_COMPACTION_POLICY_V2 = {
  schemaVersion: 2,
  activation: {
    productExecutionEnabled: false,
    actualDeleteEnabled: false,
    automaticSchedulingEnabled: false,
    wireEnabled: false,
  },
  privateDurableState: {
    location: "maintenance_jobs.cutoff_json.storageCompactionV2PrivateState",
    schemaVersion: 1,
    fields: ["schemaVersion", "cursor", "operationsVerified", "equivalenceVerified"],
    writer: "sidecarOnlySameTransaction",
    publicInput: "forbidden",
  },
  cursor: {
    schemaVersion: 2,
    phases: STORAGE_COMPACTION_PHASES_V2,
    keyset: "eventRowidGreaterThanAfterAndAtOrBelowCutoffOrderByEventRowidLimit100",
    maxRowsPerBatch: STORAGE_COMPACTION_MAX_DELETE_ROWS_V1,
    maxCanonicalPayloadBytesPerBatch:
      STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    scanExhaustedMutationAuthority: "sidecarPrivateExactEmptyKeysetQuerySameTransaction",
    scanExhaustedProofBoundary: "implementationOwnedNotRepresentableAsPublicJsonOrWire",
    durablePrivateResult:
      "scanExhaustedPersistedInMaintenanceJobCutoffJsonOnlyAfterPrivateTransaction",
    publicProgressInput: "positiveBoundedRowsAndBytesNoExhaustionAuthority",
    phaseTransition: "durablePrivateExhaustionThenExactNextAtZeroAndFalse",
    callerConfigurable: false,
  },
  lifecycle: {
    arm: "pendingToRunningSentinelCommittedBeforeBatch",
    batchSuccess: "runningToPendingWithCursorAndEmptyAuthoritySameTransaction",
    cancelBeforeArm: "pendingUnchanged",
    cancelAfterArm: "rollbackLeavesRunningSentinel",
    commitUncertain: "runningRequiresManualRecoveryPendingAdvancedMeansCommitted",
    runtimeGuardDdl: "forbidden",
    terminalSuccessMutationAuthority:
      "sidecarPrivateBoundedOperationsAndEquivalenceVerificationSameTransaction",
    terminalSuccessProofBoundary: "implementationOwnedNotRepresentableAsPublicJsonOrWire",
    durablePrivateResult:
      "verifiedBooleansAndSucceededPersistedInMaintenanceJobSamePrivateTransaction",
    publicCompleteTransition: "alwaysRejected",
  },
  operationVerification: {
    everyAuthorizedDeleteHasFrozenOperation: true,
    deletedOperationReplayUsesFrozenOperationAndReceipt: true,
    receiptOnlyRejected: true,
    chatPredecessorUsesFrozenCanonicalPayload: true,
    publicBooleanAuthority: "forbidden",
    durablePrivateResult:
      "operationsVerifiedAndEquivalenceVerifiedPersistedOnlyBySidecarSameTransaction",
  },
} as const;

export function validateStorageFrozenOperationSourcePolicyV1(
  value: unknown,
): Record<string, unknown> {
  const policy = requireObject(value, "storage frozen operation source policy");
  if (!sameContractValue(policy, STORAGE_FROZEN_OPERATION_SOURCE_POLICY_V1)) {
    fail("storage frozen operation source policy is not exact");
  }
  return policy;
}

export function validateStorageRuntimeSourceAuthorityPolicyV1(
  value: unknown,
): Record<string, unknown> {
  const policy = requireObject(value, "storage runtime source authority policy");
  if (!sameContractValue(policy, STORAGE_RUNTIME_SOURCE_AUTHORITY_POLICY_V1)) {
    fail("storage runtime source authority policy is not exact");
  }
  return policy;
}

export function validateStorageCompactionPolicyV2(value: unknown): Record<string, unknown> {
  const policy = requireObject(value, "storage compaction v2 policy");
  if (!sameContractValue(policy, STORAGE_COMPACTION_POLICY_V2)) {
    fail("storage compaction v2 policy is not exact");
  }
  return policy;
}

export const STORAGE_LIFETIME_ACTIVATION_POLICY_V1 = {
  schemaVersion: 1,
  generation: "storageLifetimeActivationV1",
  databaseMigration: "none",
  publicWire: {
    g5dLifecycleShape: "unchanged",
    storageStatusShape: "unchanged",
    capabilityOrder: "unchanged",
    readyToFailedTransition: false,
    physicalCleanupEnabled: false,
  },
  eligibility: {
    exactSafePrefixMinLedgerEvents:
      STORAGE_LOSSLESS_COMPACTION_MIN_ELIGIBLE_LEDGER_EVENTS_V1,
    countBoundary: "sinceRetainedBoundary",
    belowThresholdAllowed: false,
    ageTrigger: false,
    byteTrigger: false,
    preconditions: [
      "allExistingSafetyExclusionsPass",
      "completeMaterialization",
      "exactBeforeAfterEquivalence",
    ],
  },
  preservation: {
    logicalDeleteRows: ["events", "event_source_index"],
    preservedRecords: [
      "userChats",
      "chatMessages",
      "userEvidence",
      "recoveries",
      "workspaces",
      "snapshots",
      "baselines",
      "projections",
      "operationReceipts",
    ],
    physicalCleanup: "disabled",
  },
  restart: {
    pendingEmptyAuthority: "exactVerifyDuringAdmissionThenReadyAndResumeAfterward",
    runningSentinel: "failAdmissionWithoutMutationManualRecovery",
    nonEmptyAuthority: "failAdmissionWithoutMutationManualRecovery",
    ambiguousOrCommitUncertain: "failAdmissionWithoutMutationManualRecovery",
    automaticResetOrResumeUnsafeState: false,
  },
  runtime: {
    owner: "singleSQLiteOwnerInSidecarEventLoop",
    writerConnections: 1,
    backgroundWriterConnection: false,
    backgroundWriterThread: false,
    ticks: "readyOnly",
    zeroTickStates: ["verifying", "failed", "quitting"],
    step: "oneBoundedCommittedStepPerTick",
  },
  fatalAfterReady: {
    causes: [
      "semanticCorruption",
      "commitUncertainty",
      "materializerFatal",
      "compactorFatal",
    ],
    action: "poisonStopMaintenanceAndTerminateSidecar",
    furtherMutationOrRun: "prohibited",
    nextProcess: "reenterVerifyingAndRunAdmission",
    wireTransition: "none",
  },
  budgets: {
    maxWorkUnitsPerStep: STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
    maxCanonicalPrimaryInputBytesPerStep:
      STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    maxRetainedAndDependencyInputBytesPerStep:
      STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
    callerConfigurable: false,
  },
  phases: STORAGE_LIFETIME_ACTIVATION_PHASES_V1,
  quit: "stopNewTicksBeforeCheckpointAndCleanMarker",
} as const;

export function validateStorageLifetimeActivationPolicyV1(
  value: unknown,
): typeof STORAGE_LIFETIME_ACTIVATION_POLICY_V1 {
  const policy = requireObject(value, "storage lifetime activation policy");
  let exact = false;
  try {
    exact = exactPlainContractValue(policy, STORAGE_LIFETIME_ACTIVATION_POLICY_V1);
  } catch {
    exact = false;
  }
  if (!exact) {
    fail("storage lifetime activation policy is not exact");
  }
  return policy as unknown as typeof STORAGE_LIFETIME_ACTIVATION_POLICY_V1;
}

export const STORAGE_COMPACTION_RESUME_WITNESS_POLICY_V1 = {
  schemaVersion: 1,
  generation: "storageCompactionResumeWitnessV1",
  storageSchema: {
    fromVersion: STORAGE_LIFETIME_SCHEMA_VERSION_V8,
    toVersion: STORAGE_LIFETIME_SCHEMA_VERSION_V9,
    precondition: "exactV8NoPendingOrRunningLosslessJobAndEmptyDeleteAuthority",
    sqlPreflight: "beginImmediateExactCanonicalSqlDefinitionsAndLedgerStateRaiseRollbackBeforePersistentDdl",
    mode: "additivePlanWitnessAndVersionedGuardsNoExistingRowDeletion",
    runtimeDigestAuthority: "rustRecomputesCanonicalPayloadAndMessageDigests",
  },
  plan: {
    headerTable: "storage_compaction_plans",
    rowTable: "storage_compaction_plan_rows",
    rowTableWithoutRowid: true,
    planSchemaVersion: STORAGE_COMPACTION_PLAN_SCHEMA_VERSION_V1,
    aggregateTypes: ["project", "chat", "run", "lock"],
    deleteRows: STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1,
    expectedRows: STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1,
    boundary: "deleteThroughEqualsRetainedPlus1000AndAnchorEqualsDeleteThroughPlusOne",
    headerMutation: "forbidden",
    rowMutation: "forbidden",
    rowIdentity: "jobIdAndOrdinalPrimaryKeyWithUniqueSourceRowidOperationAndAggregateTuple",
    baseline: "everyDeleteRowMatchesHeaderBaselineIdentityAndDigest",
    anchor: "ordinal1001IsRetainAnchorAndRemainsLive",
    message: "nullableIdentityAndDigestAppearTogether",
    digestRepresentation: {
      plan: "lowercaseHex64WithoutAlgorithmPrefix",
      existingBaselineMessageAndFrozen: "sha256ColonLowercaseHex64",
      receipt: "sha256V1LowercaseHex64Columns",
      normalization: "stripExactSha256ColonPrefixAfterShapeCheck",
      authority: "rustRehashesCanonicalPayloadBaselineAndMessage",
    },
  },
  discovery: {
    targetScan: "volatileBoundedAggregateKeyset",
    forbidden: ["OFFSET", "fullVec", "tempTable", "unboundedScan"],
    creationTransaction: "beginImmediateRecheckCutoffMaterializationSafetyBoundariesAndExact1001Rows",
    planningWrite: "armedAtMost100ConsecutiveOrdinalsAndTriggerAdvancedCursorSameTransaction",
    largerPrefix: "oneExact1000DeleteJobThenRepeat",
  },
  privateState: {
    schemaVersion: STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3,
    fields: [
      "schemaVersion", "cursor", "planRowsComplete", "operationsVerified",
      "equivalenceVerified", "planRowsReleased",
    ],
    cursorSchemaVersion: STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V3,
    phases: STORAGE_COMPACTION_PHASES_V3,
    cursorColumn: "afterPlanOrdinal",
    stepBoundaryColumn: "stepThroughPlanOrdinal",
    finalizeCursor: "afterPlanOrdinal1000RetainsReleasedThroughProofAndAnchor",
    resumeVerification: "beforeReleaseBoundedKeysetProvesOrdinalsOneThroughCursorNoGapsExtrasAndRehashesSourceReceiptMessageBaseline",
    exhausted: "trueOnlyAtExactly1001WitnessedRows",
    activeV2Pending: "failAdmissionUnsupportedNoMutation",
  },
  lifecycle: {
    arm: "pendingToRunningSeparateCommittedSentinel",
    success: "runningToPendingWithAdvancedCursorAndEmptyAuthorityAtomic",
    sqlTransitionGuard: "singleBeforeUpdateStateCutoffFailureCompletedExactAdjacentPhaseCursorFlagsAndRequiredArtifacts",
    terminalEvidence: "nullSafeImmutableRfc3339UtcSecondSqlCalendarCheckAndRuntimeRevalidation",
    crash: "pendingAdvancedCommittedRunningOrNonemptyAuthorityOrUncertainFailsAdmissionNoMutation",
    reset: "forbidden",
    appendAfterCutoff: "ignoredByPlanAndIncludedInRetainedTailEquivalence",
    currentStateSafetyBeforeDelete: "failureLeavesZeroMutationPending",
  },
  authority: {
    source: "onlyDecisionDeletePlanRows",
    joins: "planHeaderCurrentJobExactPlanRowAndExistingPrefix",
    singleActiveJob: "schemaPartialUniquePendingOrRunning",
    deleteBatchRows: STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1,
    mustBeEmptyAtPending: true,
  },
  release: {
    planRows: "armedNextContiguousAtMost100DeleteOrdinalsAfterPrivateEquivalenceOnly",
    cursorAdvance: "deleteTriggerAdvancesReleasedThroughOrdinalInSameTransaction",
    resumeVerification: "releasedOrdinalPrefixAbsentAndRetainedSuffixExactAgainstTerminalEvidence",
    header: "retainedAfterSuccess",
    terminalEvidence: "retainedAfterSuccessOrFailure",
    beforeEquivalence: "forbidden",
  },
  multiJob: {
    previousSuccessfulJob: "optionalExactImmediatePredecessor",
    verification: "boundedPrimaryKeyLookupNoChainTraversal",
    progression: "retainedBoundaryEqualsPreviousDeleteThroughAndNewJobDeletesNextExact1000",
    activationWithoutBoundedTerminalEvidence: "forbidden",
  },
  budgets: {
    maxWorkUnitsPerStep: STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
    maxCanonicalPrimaryInputBytesPerStep:
      STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
    maxRetainedAndDependencyInputBytesPerStep:
      STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
    callerConfigurable: false,
    writerConnections: 1,
  },
  activation: {
    contractOnly: true,
    runtimeImplemented: false,
    productExecutionEnabled: false,
    automaticSchedulingEnabled: false,
    publicWireChanged: false,
  },
} as const;

export function validateStorageCompactionResumeWitnessPolicyV1(
  value: unknown,
): typeof STORAGE_COMPACTION_RESUME_WITNESS_POLICY_V1 {
  const policy = requireObject(value, "storage compaction resume witness policy");
  if (!exactPlainContractValue(policy, STORAGE_COMPACTION_RESUME_WITNESS_POLICY_V1)) {
    fail("storage compaction resume witness policy is not exact");
  }
  return policy as unknown as typeof STORAGE_COMPACTION_RESUME_WITNESS_POLICY_V1;
}

export function validateStorageLifetimeV9MigrationPreflight(
  exactStorageV8: boolean,
  activeLosslessJobs: number,
  deleteAuthorityRows: number,
): void {
  const active = requireSafeUnsigned(activeLosslessJobs, "v9Preflight.activeLosslessJobs");
  const authority = requireSafeUnsigned(deleteAuthorityRows, "v9Preflight.deleteAuthorityRows");
  if (exactStorageV8 !== true || active !== 0 || authority !== 0) {
    fail("storage v9 migration requires exact v8 with no active compaction or authority");
  }
}

export const STORAGE_LIFETIME_ACTIVATION_POLICY_V2 = {
  ...STORAGE_LIFETIME_ACTIVATION_POLICY_V1,
  schemaVersion: 2,
  generation: "storageLifetimeActivationV2",
  databaseMigration: {
    fromVersion: STORAGE_LIFETIME_SCHEMA_VERSION_V8,
    toVersion: STORAGE_LIFETIME_SCHEMA_VERSION_V9,
    precondition: "exactV8NoPendingOrRunningLosslessJobAndEmptyDeleteAuthority",
    effect: "additiveResumeWitnessTablesAndVersionedGuards",
    existingRowsDeleted: false,
    activeV2Pending: "failAdmissionUnsupportedNoMutation",
  },
  resumeWitness: {
    generation: "storageCompactionResumeWitnessV1",
    privateStateSchemaVersion: STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3,
    pendingAdmission: "exactWitnessVerificationThenReadyResumeAfterward",
    runningOrAuthorityOrUncertain: "failAdmissionNoMutation",
    publicWire: "unchanged",
  },
} as const;

export function validateStorageLifetimeActivationPolicyV2(
  value: unknown,
): typeof STORAGE_LIFETIME_ACTIVATION_POLICY_V2 {
  const policy = requireObject(value, "storage lifetime activation policy v2");
  if (!exactPlainContractValue(policy, STORAGE_LIFETIME_ACTIVATION_POLICY_V2)) {
    fail("storage lifetime activation policy v2 is not exact");
  }
  return policy as unknown as typeof STORAGE_LIFETIME_ACTIVATION_POLICY_V2;
}

export function validateStorageCompactionPlanV1(value: unknown): StorageCompactionPlanV1 {
  const plan = requirePlainDataObjectV9(value, "storage compaction plan");
  exactKeys(plan, [
    "jobId", "planSchemaVersion", "cutoffEventRowid", "aggregateType", "aggregateId",
    "retainedBoundarySequence", "deleteThroughSequence", "anchorAggregateSequence",
    "expectedRows", "baselineId", "baselineProjectionDigest", "previousSuccessfulJobId",
    "createdAt", "plannedAt",
  ]);
  if (typeof plan.jobId !== "string" || plan.jobId.length === 0
    || plan.planSchemaVersion !== STORAGE_COMPACTION_PLAN_SCHEMA_VERSION_V1
    || typeof plan.aggregateType !== "string"
    || !["project", "chat", "run", "lock"].includes(plan.aggregateType)
    || typeof plan.aggregateId !== "string" || plan.aggregateId.length === 0
    || typeof plan.baselineId !== "string" || plan.baselineId.length === 0
    || !isLowercaseHexDigest(plan.baselineProjectionDigest)
    || plan.expectedRows !== STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1) {
    fail("storage compaction plan header is invalid");
  }
  const cutoff = requireSafeUnsigned(plan.cutoffEventRowid, "plan.cutoffEventRowid");
  const retained = requireSafeUnsigned(
    plan.retainedBoundarySequence, "plan.retainedBoundarySequence",
  );
  const deleted = requireSafeUnsigned(plan.deleteThroughSequence, "plan.deleteThroughSequence");
  const anchor = requireSafeUnsigned(plan.anchorAggregateSequence, "plan.anchorAggregateSequence");
  if (cutoff === 0 || deleted !== retained + STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1
    || anchor !== deleted + 1 || anchor > Number.MAX_SAFE_INTEGER) {
    fail("storage compaction plan boundary must be exact 1000 delete rows plus one anchor");
  }
  if (plan.previousSuccessfulJobId !== null
    && (typeof plan.previousSuccessfulJobId !== "string"
      || plan.previousSuccessfulJobId.length === 0
      || plan.previousSuccessfulJobId === plan.jobId)) {
    fail("storage compaction previous successful job identity is invalid");
  }
  requireTimestamp(plan.createdAt, "plan.createdAt");
  requireTimestamp(plan.plannedAt, "plan.plannedAt");
  if ((plan.plannedAt as string) < (plan.createdAt as string)) {
    fail("storage compaction plannedAt cannot precede createdAt");
  }
  return plan as unknown as StorageCompactionPlanV1;
}

export function validateStorageCompactionPlanRowV1(
  plan: StorageCompactionPlanV1,
  value: unknown,
): StorageCompactionPlanRowV1 {
  const row = requirePlainDataObjectV9(value, "storage compaction plan row");
  exactKeys(row, [
    "jobId", "ordinal", "eventRowid", "eventId", "operationId", "requestId",
    "aggregateType", "aggregateId", "aggregateSequence", "eventType",
    "payloadSchemaVersion", "eventTimestamp", "canonicalPayloadJson",
    "canonicalPayloadDigest", "introducedMessageId", "introducedMessageDigest",
    "decision", "baselineId", "baselineProjectionDigest", "operationFrozenBeforePlan",
    "messageFrozenBeforePlan",
  ]);
  const ordinal = requireSafeUnsigned(row.ordinal, "planRow.ordinal");
  const eventRowid = requireSafeUnsigned(row.eventRowid, "planRow.eventRowid");
  const sequence = requireSafeUnsigned(row.aggregateSequence, "planRow.aggregateSequence");
  if (row.jobId !== plan.jobId || ordinal < 1
    || ordinal > STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1 || eventRowid < 1
    || eventRowid > plan.cutoffEventRowid || typeof row.eventId !== "string"
    || row.eventId.length === 0 || typeof row.operationId !== "string"
    || row.operationId.length === 0 || (row.requestId !== null && typeof row.requestId !== "string")
    || row.aggregateType !== plan.aggregateType || row.aggregateId !== plan.aggregateId
    || typeof row.eventType !== "string" || row.eventType.length === 0
    || row.payloadSchemaVersion !== 1 || typeof row.canonicalPayloadJson !== "string"
    || typeof row.operationFrozenBeforePlan !== "boolean"
    || typeof row.messageFrozenBeforePlan !== "boolean") {
    fail("storage compaction plan row identity is invalid");
  }
  requireTimestamp(row.eventTimestamp, "planRow.eventTimestamp");
  const digest = sha256FileVersionV1(new TextEncoder().encode(row.canonicalPayloadJson)).slice(7);
  if (row.canonicalPayloadDigest !== digest) {
    fail("storage compaction plan row canonical payload digest must be recomputed");
  }
  if (sequence !== plan.retainedBoundarySequence + ordinal) {
    fail("storage compaction plan row aggregate sequence must match ordinal");
  }
  const messagePairValid = row.introducedMessageId === null && row.introducedMessageDigest === null
    ? row.messageFrozenBeforePlan === false
    : typeof row.introducedMessageId === "string" && row.introducedMessageId.length > 0
      && isLowercaseHexDigest(row.introducedMessageDigest);
  if (!messagePairValid) {
    fail("storage compaction plan row message identity and digest must be exact");
  }
  const anchor = ordinal === STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1;
  const decisionValid = anchor
    ? row.decision === "retainAnchor" && sequence === plan.anchorAggregateSequence
      && row.baselineId === null && row.baselineProjectionDigest === null
    : row.decision === "delete" && sequence <= plan.deleteThroughSequence
      && row.baselineId === plan.baselineId
      && row.baselineProjectionDigest === plan.baselineProjectionDigest;
  if (!decisionValid) fail("storage compaction plan row decision or baseline binding is invalid");
  return row as unknown as StorageCompactionPlanRowV1;
}

export function validateStorageCompactionPlanRowBindingV1(
  plan: StorageCompactionPlanV1,
  value: unknown,
  expectedOperationFrozenBeforePlan: boolean,
  expectedMessageDigest: string | null,
  expectedMessageFrozenBeforePlan: boolean,
): StorageCompactionPlanRowV1 {
  const row = validateStorageCompactionPlanRowV1(plan, value);
  if (row.operationFrozenBeforePlan !== expectedOperationFrozenBeforePlan
    || row.messageFrozenBeforePlan !== expectedMessageFrozenBeforePlan
    || row.introducedMessageDigest !== expectedMessageDigest) {
    fail("storage compaction plan row frozen source binding is invalid");
  }
  return row;
}

export function validateStorageCompactionPlanBatchV1(
  planValue: unknown,
  rowsValue: unknown,
  previousAfterPlanOrdinal: number,
  scanExhausted: boolean,
): StorageCompactionPlanRowV1[] {
  const plan = validateStorageCompactionPlanV1(planValue);
  const previous = requireSafeUnsigned(previousAfterPlanOrdinal, "plan.previousAfterPlanOrdinal");
  if (!Array.isArray(rowsValue) || rowsValue.length === 0
    || rowsValue.length > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1
    || previous >= STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1) {
    fail("storage compaction plan row batch must be positive and bounded");
  }
  const next = previous + rowsValue.length;
  if (next > STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1
    || scanExhausted !== (next === STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1)) {
    fail("storage compaction plan exhaustion requires exactly ordinal 1001");
  }
  const rowids = new Set<number>();
  const operations = new Set<string>();
  const sequences = new Set<number>();
  return rowsValue.map((value, index) => {
    const row = validateStorageCompactionPlanRowV1(plan, value);
    if (row.ordinal !== previous + index + 1 || rowids.has(row.eventRowid)
      || operations.has(row.operationId) || sequences.has(row.aggregateSequence)) {
      fail("storage compaction plan rows must be gapless and unique");
    }
    rowids.add(row.eventRowid);
    operations.add(row.operationId);
    sequences.add(row.aggregateSequence);
    return row;
  });
}

export function validateStorageCompactionCursorV3(value: unknown): StorageCompactionCursorV3 {
  const cursor = requirePlainDataObjectV9(value, "storage compaction v3 cursor");
  exactKeys(cursor, [
    "schemaVersion", "cutoffEventRowid", "phase", "afterPlanOrdinal",
    "stepThroughPlanOrdinal", "scanExhausted",
  ]);
  const cutoff = requireSafeUnsigned(cursor.cutoffEventRowid, "compactionV3.cutoffEventRowid");
  const after = requireSafeUnsigned(cursor.afterPlanOrdinal, "compactionV3.afterPlanOrdinal");
  const knownPhase = STORAGE_COMPACTION_PHASES_V3.includes(cursor.phase as StorageCompactionPhaseV3);
  const limit = cursor.phase === "releasePlan" || cursor.phase === "finalize"
    ? STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1
    : STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1;
  const step = cursor.stepThroughPlanOrdinal === null
    ? null
    : requireSafeUnsigned(
      cursor.stepThroughPlanOrdinal, "compactionV3.stepThroughPlanOrdinal",
    );
  const stepValid = step === null || (!cursor.scanExhausted && step > after && step <= limit
    && step - after <= STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1);
  const positionValid = after <= limit && cursor.scanExhausted === (after === limit)
    && stepValid && (cursor.phase !== "finalize"
      || (after === STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1 && step === null));
  if (cursor.schemaVersion !== STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V3 || cutoff === 0
    || !knownPhase || typeof cursor.scanExhausted !== "boolean" || !positionValid) {
    fail("storage compaction v3 cursor is invalid");
  }
  return cursor as unknown as StorageCompactionCursorV3;
}

export function validateStorageCompactionCursorTransitionV3(
  previousValue: unknown | null,
  nextValue: unknown,
  rowsInStep: number,
): StorageCompactionCursorV3 {
  const next = validateStorageCompactionCursorV3(nextValue);
  const rows = requireSafeUnsigned(rowsInStep, "compactionV3.rowsInStep");
  if (previousValue === null) {
    if (next.phase !== "planEligibility" || next.afterPlanOrdinal !== 0
      || next.stepThroughPlanOrdinal !== null
      || next.scanExhausted || rows !== 0) {
      fail("storage compaction v3 cursor must start at planEligibility zero");
    }
    return next;
  }
  const previous = validateStorageCompactionCursorV3(previousValue);
  if (previous.cutoffEventRowid !== next.cutoffEventRowid
    || rows > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1) {
    fail("storage compaction v3 cursor transition exceeds fixed boundary");
  }
  if (previous.phase === next.phase) {
    if (previous.scanExhausted || previous.stepThroughPlanOrdinal === null || rows === 0
      || next.afterPlanOrdinal !== previous.stepThroughPlanOrdinal
      || next.afterPlanOrdinal !== previous.afterPlanOrdinal + rows
      || next.stepThroughPlanOrdinal !== null) {
      fail("storage compaction v3 cursor progress must be positive and consecutive");
    }
    return next;
  }
  const previousIndex = STORAGE_COMPACTION_PHASES_V3.indexOf(previous.phase);
  const nextIndex = STORAGE_COMPACTION_PHASES_V3.indexOf(next.phase);
  const nextPositionValid = previous.phase === "releasePlan" && next.phase === "finalize"
    ? next.afterPlanOrdinal === STORAGE_COMPACTION_PLAN_DELETE_ROWS_V1
      && next.scanExhausted && next.stepThroughPlanOrdinal === null
    : next.afterPlanOrdinal === 0 && !next.scanExhausted
      && next.stepThroughPlanOrdinal === null;
  if (!previous.scanExhausted || nextIndex !== previousIndex + 1
    || !nextPositionValid || rows !== 0) {
    fail("storage compaction v3 phases must advance exactly in order");
  }
  return next;
}

export function validateStorageCompactionAdmissionStateV3(
  value: unknown,
): StorageCompactionAdmissionStateV3 | null {
  if (value === null) return null;
  const state = requirePlainDataObjectV9(value, "storage compaction admission state v3");
  exactKeys(state, [
    "jobId", "state", "privateState", "authorityCount", "planHeaderPresent",
  ]);
  validateStorageCompactionPrivateStateV3(state.privateState);
  if (typeof state.jobId !== "string" || state.jobId.length === 0 || state.state !== "pending"
    || requireSafeUnsigned(state.authorityCount, "admission.authorityCount") !== 0
    || state.planHeaderPresent !== true
    || (state.privateState as StorageCompactionPrivateStateV3).cursor.stepThroughPlanOrdinal !== null) {
    fail("only exact active v3 pending compaction is admission-runnable");
  }
  return state as unknown as StorageCompactionAdmissionStateV3;
}

export function validateStorageCompactionPrivateStateV3(
  value: unknown,
): StorageCompactionPrivateStateV3 {
  const state = requirePlainDataObjectV9(value, "storage compaction v3 private state");
  exactKeys(state, [
    "schemaVersion", "cursor", "planRowsComplete", "operationsVerified",
    "equivalenceVerified", "planRowsReleased",
  ]);
  const cursor = validateStorageCompactionCursorV3(state.cursor);
  const phase = STORAGE_COMPACTION_PHASES_V3.indexOf(cursor.phase);
  const expectedPlanComplete = phase > 0 || cursor.scanExhausted;
  const expectedOperationsVerified = phase > 6 || (phase === 6 && cursor.scanExhausted);
  const expectedEquivalenceVerified = phase > 7 || (phase === 7 && cursor.scanExhausted);
  const expectedRowsReleased = phase > 8 || (phase === 8 && cursor.scanExhausted);
  if (state.schemaVersion !== STORAGE_COMPACTION_PRIVATE_STATE_SCHEMA_VERSION_V3
    || state.planRowsComplete !== expectedPlanComplete
    || state.operationsVerified !== expectedOperationsVerified
    || state.equivalenceVerified !== expectedEquivalenceVerified
    || state.planRowsReleased !== expectedRowsReleased) {
    fail("storage compaction v3 private state contradicts its cursor phase");
  }
  return state as unknown as StorageCompactionPrivateStateV3;
}

export function validateStorageCompactionJobV3(value: unknown): StorageCompactionJobV3 {
  const job = requirePlainDataObjectV9(value, "storage compaction v3 job");
  exactKeys(job, [
    "jobId", "state", "privateState", "authorityCount", "failureKind", "completedAt",
  ]);
  const privateState = validateStorageCompactionPrivateStateV3(job.privateState);
  const authority = requireSafeUnsigned(job.authorityCount, "compactionV3.authorityCount");
  if (typeof job.jobId !== "string" || job.jobId.length === 0
    || authority > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1) {
    fail("storage compaction v3 job identity or authority is invalid");
  }
  if (job.state === "pending") {
    if (authority !== 0 || job.failureKind !== null || job.completedAt !== null) {
      fail("storage compaction v3 pending job must have empty authority");
    }
    if (privateState.cursor.stepThroughPlanOrdinal !== null) {
      fail("storage compaction v3 pending job cannot retain an armed step boundary");
    }
  } else if (job.state === "running") {
    if (job.failureKind !== null || job.completedAt !== null) {
      fail("storage compaction v3 running job cannot be terminal");
    }
  } else if (job.state === "succeeded") {
    if (authority !== 0 || job.failureKind !== null || privateState.cursor.phase !== "finalize"
      || !privateState.cursor.scanExhausted || !privateState.planRowsReleased
      || job.completedAt === null) {
      fail("storage compaction v3 success requires released final evidence");
    }
    requireTimestamp(job.completedAt, "compactionV3.completedAt");
  } else if (job.state === "failed") {
    if (authority !== 0 || typeof job.failureKind !== "string" || job.failureKind.length === 0
      || job.completedAt === null) {
      fail("storage compaction v3 failed job requires terminal evidence");
    }
    requireTimestamp(job.completedAt, "compactionV3.completedAt");
  } else {
    fail("storage compaction v3 job state is invalid");
  }
  return job as unknown as StorageCompactionJobV3;
}

export function validateStorageCompactionLifecycleTransitionV3(
  operation: StorageCompactionLifecycleOperationV3,
  previousValue: unknown,
  nextValue: unknown,
  rowsInStep: number,
): StorageCompactionJobV3 {
  const previous = validateStorageCompactionJobV3(previousValue);
  const next = validateStorageCompactionJobV3(nextValue);
  const rows = requireSafeUnsigned(rowsInStep, "compactionV3.rowsInStep");
  if (previous.jobId !== next.jobId || rows > STORAGE_COMPACTION_PLAN_MAX_ROWS_PER_STEP_V1) {
    fail("storage compaction v3 transition must preserve job and budget");
  }
  const samePrivate = exactPlainContractValue(previous.privateState, next.privateState);
  const sameArmFlags = previous.privateState.schemaVersion === next.privateState.schemaVersion
    && previous.privateState.planRowsComplete === next.privateState.planRowsComplete
    && previous.privateState.operationsVerified === next.privateState.operationsVerified
    && previous.privateState.equivalenceVerified === next.privateState.equivalenceVerified
    && previous.privateState.planRowsReleased === next.privateState.planRowsReleased;
  const armCursorValid = previous.privateState.cursor.schemaVersion
      === next.privateState.cursor.schemaVersion
    && previous.privateState.cursor.cutoffEventRowid === next.privateState.cursor.cutoffEventRowid
    && previous.privateState.cursor.phase === next.privateState.cursor.phase
    && previous.privateState.cursor.afterPlanOrdinal === next.privateState.cursor.afterPlanOrdinal
    && previous.privateState.cursor.scanExhausted === next.privateState.cursor.scanExhausted
    && previous.privateState.cursor.stepThroughPlanOrdinal === null
    && (previous.privateState.cursor.scanExhausted
      ? next.privateState.cursor.stepThroughPlanOrdinal === null
      : next.privateState.cursor.stepThroughPlanOrdinal !== null);
  const sameNonState = previous.authorityCount === next.authorityCount
    && previous.failureKind === next.failureKind && previous.completedAt === next.completedAt;
  const cursorProgress = (): boolean => {
    try {
      validateStorageCompactionCursorTransitionV3(
        previous.privateState.cursor,
        next.privateState.cursor,
        rows,
      );
      return true;
    } catch {
      return false;
    }
  };
  let valid = false;
  switch (operation) {
    case "arm":
      valid = rows === 0 && previous.state === "pending" && next.state === "running"
        && sameArmFlags && armCursorValid && sameNonState && previous.authorityCount === 0;
      break;
    case "batchSuccess":
      valid = previous.state === "running" && next.state === "pending"
        && next.authorityCount === 0 && previous.failureKind === null
        && next.failureKind === null && previous.completedAt === null
        && next.completedAt === null && cursorProgress();
      break;
    case "cancelBeforeArm":
      valid = rows === 0 && previous.state === "pending"
        && exactPlainContractValue(previous, next);
      break;
    case "cancelAfterArm":
      valid = rows === 0 && previous.state === "running"
        && exactPlainContractValue(previous, next);
      break;
    case "commitUncertain":
      valid = rows === 0 && previous.state === "running"
        && exactPlainContractValue(previous, next);
      if (!valid) {
        valid = previous.state === "running" && next.state === "pending"
          && next.authorityCount === 0 && cursorProgress();
      }
      break;
    case "complete":
      valid = rows === 0 && previous.state === "pending" && next.state === "succeeded"
        && samePrivate && previous.authorityCount === 0 && next.authorityCount === 0
        && next.failureKind === null && next.completedAt !== null;
      break;
    case "fail":
      valid = rows === 0 && previous.state === "running" && next.state === "failed"
        && samePrivate && next.authorityCount === 0
        && typeof next.failureKind === "string" && next.failureKind.length > 0
        && next.completedAt !== null;
      break;
  }
  if (!valid) fail("storage compaction v3 lifecycle transition is not atomic");
  return next;
}

function isLowercaseHexDigest(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

function requirePlainDataObjectV9(value: unknown, name: string): Record<string, unknown> {
  const object = requireObject(value, name);
  if (Object.getPrototypeOf(object) !== Object.prototype
    || Reflect.ownKeys(object).some((key) => {
      if (typeof key !== "string") return true;
      const descriptor = Object.getOwnPropertyDescriptor(object, key);
      return descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable;
    })) {
    fail(`${name} must be a plain enumerable data object`);
  }
  return object;
}

function exactPlainContractValue(
  actual: unknown,
  expected: unknown,
  ancestors = new WeakSet<object>(),
): boolean {
  if (actual === null || expected === null
    || typeof actual !== "object" || typeof expected !== "object") {
    return actual === expected;
  }
  const array = Array.isArray(actual);
  if (array !== Array.isArray(expected)) return false;
  const prototype = array ? Array.prototype : Object.prototype;
  if (Object.getPrototypeOf(actual) !== prototype || Object.getPrototypeOf(expected) !== prototype
    || ancestors.has(actual)) {
    return false;
  }

  const actualKeys = Reflect.ownKeys(actual);
  const expectedKeys = Reflect.ownKeys(expected);
  if (actualKeys.some((key) => typeof key !== "string")
    || expectedKeys.some((key) => typeof key !== "string")
    || actualKeys.length !== expectedKeys.length) {
    return false;
  }
  const sortedActualKeys = [...actualKeys as string[]].sort();
  const sortedExpectedKeys = [...expectedKeys as string[]].sort();
  if (sortedActualKeys.some((key, index) => key !== sortedExpectedKeys[index])) return false;

  ancestors.add(actual);
  try {
    for (const key of sortedActualKeys) {
      const actualDescriptor = Object.getOwnPropertyDescriptor(actual, key);
      const expectedDescriptor = Object.getOwnPropertyDescriptor(expected, key);
      if (actualDescriptor === undefined || expectedDescriptor === undefined
        || !("value" in actualDescriptor) || !("value" in expectedDescriptor)
        || actualDescriptor.enumerable !== expectedDescriptor.enumerable
        || !exactPlainContractValue(actualDescriptor.value, expectedDescriptor.value, ancestors)) {
        return false;
      }
    }
    return true;
  } finally {
    ancestors.delete(actual);
  }
}

export function canonicalFrozenOperationRecordV1(source: FrozenOperationSourceV1): string {
  return canonicalJsonV1({
    aggregateId: source.aggregateId,
    aggregateSequence: source.aggregateSequence,
    aggregateType: source.aggregateType,
    canonicalPayloadDigest: source.canonicalPayloadDigest,
    canonicalPayloadJson: source.canonicalPayloadJson,
    eventTimestamp: source.eventTimestamp,
    eventType: source.eventType,
    frozenAt: source.frozenAt,
    operationId: source.operationId,
    payloadSchemaVersion: source.payloadSchemaVersion,
    recordSchemaVersion: 1,
    requestId: source.requestId,
    sourceEventId: source.sourceEventId,
    sourceEventRowid: source.sourceEventRowid,
  });
}

export function validateFrozenOperationSourceV1(value: unknown): FrozenOperationSourceV1 {
  const source = requireObject(value, "frozen operation source");
  exactKeys(source, [
    "operationId", "sourceEventRowid", "sourceEventId", "requestId", "aggregateType",
    "aggregateId", "aggregateSequence", "eventType", "payloadSchemaVersion",
    "canonicalPayloadJson", "canonicalPayloadDigest", "eventTimestamp",
    "canonicalRecordJson", "recordDigest", "frozenAt",
  ]);
  const rowid = requireSafeUnsigned(source.sourceEventRowid, "frozen operation source rowid");
  const sequence = requireSafeUnsigned(
    source.aggregateSequence, "frozen operation aggregate sequence",
  );
  if (!nonEmpty(source.operationId) || rowid === 0 || !nonEmpty(source.sourceEventId)
    || !(source.requestId === null || nonEmpty(source.requestId))
    || !["project", "chat", "run", "lock"].includes(source.aggregateType as string)
    || !nonEmpty(source.aggregateId) || sequence === 0 || !nonEmpty(source.eventType)
    || source.payloadSchemaVersion !== 1 || !nonEmpty(source.canonicalPayloadJson)
    || typeof source.canonicalPayloadDigest !== "string"
    || !/^[0-9a-f]{64}$/.test(source.canonicalPayloadDigest)
    || !nonEmpty(source.canonicalRecordJson) || typeof source.recordDigest !== "string"
    || !/^[0-9a-f]{64}$/.test(source.recordDigest)) {
    fail("frozen operation source fields are invalid");
  }
  requireTimestamp(source.eventTimestamp, "frozen operation eventTimestamp");
  requireTimestamp(source.frozenAt, "frozen operation frozenAt");
  const typed = source as unknown as FrozenOperationSourceV1;
  let payload: unknown;
  try {
    payload = JSON.parse(typed.canonicalPayloadJson);
  } catch {
    fail("frozen operation canonical payload must be valid JSON");
  }
  if (payload === null || Array.isArray(payload) || typeof payload !== "object"
    || canonicalJsonV1(payload) !== typed.canonicalPayloadJson) {
    fail("frozen operation canonical payload must be one canonical JSON object");
  }
  const payloadDigest = sha256FileVersionV1(
    new TextEncoder().encode(typed.canonicalPayloadJson),
  );
  if (payloadDigest !== `sha256:${typed.canonicalPayloadDigest}`) {
    fail("frozen operation payload digest does not match canonical payload");
  }
  const expectedRecord = canonicalFrozenOperationRecordV1(typed);
  if (typed.canonicalRecordJson !== expectedRecord) {
    fail("frozen operation canonical record is not exact");
  }
  const recordDigest = sha256FileVersionV1(new TextEncoder().encode(expectedRecord));
  if (recordDigest !== `sha256:${typed.recordDigest}`) {
    fail("frozen operation record digest does not match canonical envelope");
  }
  return typed;
}

export function validateFrozenOperationSourceBatchV1(
  value: unknown,
): FrozenOperationSourceV1[] {
  if (!Array.isArray(value) || value.length === 0
    || value.length > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1) {
    fail("frozen operation batch must contain one through one hundred rows");
  }
  const operations = new Set<string>();
  const rowids = new Set<number>();
  let payloadBytes = 0;
  const sources = value.map((item) => {
    const source = validateFrozenOperationSourceV1(item);
    payloadBytes += new TextEncoder().encode(source.canonicalPayloadJson).byteLength;
    if (operations.has(source.operationId) || rowids.has(source.sourceEventRowid)) {
      fail("frozen operation batch identities must be distinct");
    }
    operations.add(source.operationId);
    rowids.add(source.sourceEventRowid);
    return source;
  });
  if (payloadBytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1) {
    fail("frozen operation batch canonical payload budget exceeded");
  }
  return sources;
}

export function validateFrozenOperationSourceBindingV1(
  value: unknown,
  eventRowid: number,
  event: OperationCandidateV1,
  sourceIndexValue: unknown,
  receiptValue: unknown,
): FrozenOperationSourceV1 {
  const frozen = validateFrozenOperationReceiptBindingV1(value, receiptValue);
  const sourceIndex = validateStorageEventSourceIndexBindingV1(
    eventRowid, sourceIndexValue, event,
  );
  if (frozen.sourceEventRowid !== eventRowid || frozen.sourceEventId !== event.event_id
    || frozen.operationId !== event.operation_id || frozen.requestId !== event.request_id
    || frozen.aggregateType !== event.aggregate_type || frozen.aggregateId !== event.aggregate_id
    || frozen.aggregateSequence !== event.aggregate_sequence
    || frozen.eventType !== event.event_type
    || frozen.payloadSchemaVersion !== event.payload_schema_version
    || frozen.canonicalPayloadJson !== event.canonical_json_payload
    || frozen.eventTimestamp !== event.timestamp
    || sourceIndex.operationId !== frozen.operationId) {
    fail("frozen operation must bind one exact live event, source index, and receipt cross-check");
  }
  return frozen;
}

export function validateFrozenOperationReceiptBindingV1(
  value: unknown,
  receiptValue: unknown,
): FrozenOperationSourceV1 {
  const frozen = validateFrozenOperationSourceV1(value);
  const receipt = validateOperationReceiptV2(receiptValue);
  if (receipt.operationId !== frozen.operationId || receipt.eventId !== frozen.sourceEventId
    || receipt.aggregateType !== frozen.aggregateType
    || receipt.aggregateId !== frozen.aggregateId
    || receipt.aggregateSequence !== frozen.aggregateSequence
    || receipt.eventType !== frozen.eventType
    || receipt.payloadSchemaVersion !== frozen.payloadSchemaVersion
    || receipt.canonicalPayloadJson !== frozen.canonicalPayloadJson
    || receipt.canonicalPayloadDigest !== frozen.canonicalPayloadDigest
    || receipt.eventTimestamp !== frozen.eventTimestamp) {
    fail("frozen operation must bind one exact immutable receipt");
  }
  return frozen;
}

export function validateStorageRuntimeSourceAuthorityV1(
  value: unknown,
): StorageRuntimeSourceAuthorityV1 {
  const authority = requireObject(value, "storage runtime source authority");
  exactKeys(authority, [
    "aggregateType", "eventPresent", "liveSourceIndexPresent", "frozenOperationPresent", "receiptPresent",
    "liveSourceMatches", "frozenOperationMatches", "receiptMatches",
    "chatPredecessorSource",
  ]);
  if (!["project", "chat", "run", "lock"].includes(authority.aggregateType as string)) {
    fail("runtime authority aggregateType is invalid");
  }
  for (const field of [
    "eventPresent", "liveSourceIndexPresent", "frozenOperationPresent", "receiptPresent",
    "liveSourceMatches", "frozenOperationMatches", "receiptMatches",
  ] as const) {
    if (typeof authority[field] !== "boolean") fail(`runtime authority ${field} must be boolean`);
  }
  if (!(authority.chatPredecessorSource === null
    || authority.chatPredecessorSource === "frozenOperationCanonicalPayload")) {
    fail("chat predecessor cannot be reconstructed from a receipt");
  }
  if (authority.aggregateType === "chat" && authority.eventPresent === false
    ? authority.chatPredecessorSource !== "frozenOperationCanonicalPayload"
    : authority.chatPredecessorSource !== null) {
    fail("deleted chat predecessor requires frozen operation canonical payload only");
  }
  const valid = authority.eventPresent === true
    ? authority.liveSourceIndexPresent === true && authority.liveSourceMatches === true
      && authority.receiptPresent === true && authority.receiptMatches === true
    : authority.liveSourceIndexPresent === false && authority.frozenOperationPresent === true
      && authority.frozenOperationMatches === true && authority.receiptPresent === true
      && authority.receiptMatches === true;
  if (!valid) fail("runtime source authority is incomplete");
  return authority as unknown as StorageRuntimeSourceAuthorityV1;
}

export function validateStorageCompactionCursorV2(value: unknown): StorageCompactionCursorV2 {
  const cursor = requireObject(value, "storage compaction cursor v2");
  exactKeys(cursor, [
    "schemaVersion", "cutoffEventRowid", "phase", "afterEventRowid", "scanExhausted",
  ]);
  const cutoff = requireSafeUnsigned(cursor.cutoffEventRowid, "compaction v2 cutoff");
  const after = requireSafeUnsigned(cursor.afterEventRowid, "compaction v2 after");
  if (cursor.schemaVersion !== STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V2
    || !STORAGE_COMPACTION_PHASES_V2.includes(cursor.phase as StorageCompactionPhaseV2)
    || typeof cursor.scanExhausted !== "boolean" || after > cutoff) {
    fail("storage compaction cursor v2 is invalid");
  }
  return cursor as unknown as StorageCompactionCursorV2;
}

export function validateStorageCompactionCursorTransitionV2(
  previousValue: unknown | null,
  nextValue: unknown,
  progressValue: unknown | null,
): StorageCompactionCursorV2 {
  const next = validateStorageCompactionCursorV2(nextValue);
  if (previousValue === null) {
    if (progressValue !== null || next.phase !== "freezeOperations"
      || next.afterEventRowid !== 0 || next.scanExhausted) {
      fail("storage compaction v2 must start freezeOperations without public progress");
    }
    return next;
  }
  const previous = validateStorageCompactionCursorV2(previousValue);
  if (next.cutoffEventRowid !== previous.cutoffEventRowid) {
    fail("storage compaction v2 cannot change frozen cutoff");
  }
  if (next.phase !== previous.phase) {
    const previousIndex = STORAGE_COMPACTION_PHASES_V2.indexOf(previous.phase);
    if (progressValue !== null || !previous.scanExhausted || next.scanExhausted
      || STORAGE_COMPACTION_PHASES_V2[previousIndex + 1] !== next.phase
      || next.afterEventRowid !== 0) {
      fail("storage compaction v2 phase advance requires exact exhausted predecessor");
    }
    return next;
  }
  if (next.scanExhausted !== previous.scanExhausted || previous.scanExhausted) {
    fail("public compaction v2 transition cannot mint private exhaustion authority");
  }
  const progress = requireObject(progressValue, "storage compaction v2 progress");
  exactKeys(progress, ["rowsInBatch", "canonicalPayloadBytes"]);
  const rows = requireSafeUnsigned(progress.rowsInBatch, "compaction v2 rows");
  const bytes = requireSafeUnsigned(progress.canonicalPayloadBytes, "compaction v2 bytes");
  if (rows > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
    || bytes > STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1
    || rows === 0 || next.afterEventRowid <= previous.afterEventRowid) {
    fail("storage compaction v2 progress requires a nonempty bounded batch");
  }
  return next;
}

export function validateStorageCompactionJobV2(value: unknown): StorageCompactionJobV2 {
  const job = requireObject(value, "storage compaction job v2");
  exactKeys(job, [
    "jobId", "state", "cursor", "authorityCount", "operationsVerified",
    "equivalenceVerified", "failureKind", "completedAt",
  ]);
  const cursor = validateStorageCompactionCursorV2(job.cursor);
  const authorityCount = requireSafeUnsigned(job.authorityCount, "compaction v2 authority");
  if (!nonEmpty(job.jobId) || !["pending", "running", "succeeded", "failed"].includes(
    job.state as string,
  ) || authorityCount > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
    || typeof job.operationsVerified !== "boolean"
    || typeof job.equivalenceVerified !== "boolean"
    || !(job.failureKind === null || nonEmpty(job.failureKind))
    || !(job.completedAt === null || nonEmpty(job.completedAt))) {
    fail("storage compaction job v2 fields are invalid");
  }
  if (job.state === "pending" || job.state === "running") {
    if (job.failureKind !== null || job.completedAt !== null) {
      fail("nonterminal compaction v2 job cannot carry terminal evidence");
    }
  } else if (job.state === "succeeded") {
    if (authorityCount !== 0 || job.operationsVerified !== true
      || job.equivalenceVerified !== true || cursor.phase !== "finalize"
      || cursor.scanExhausted !== true || job.failureKind !== null
      || job.completedAt === null) {
      fail("successful compaction v2 requires verified authority-empty finalization");
    }
    requireTimestamp(job.completedAt, "compaction v2 completedAt");
  } else {
    if (authorityCount !== 0 || !nonEmpty(job.failureKind) || job.completedAt === null) {
      fail("failed compaction v2 requires empty authority and failure evidence");
    }
    requireTimestamp(job.completedAt, "compaction v2 completedAt");
  }
  return job as unknown as StorageCompactionJobV2;
}

export function validateStorageCompactionLifecycleTransitionV2(
  operation: StorageCompactionLifecycleOperationV2,
  previousValue: unknown,
  nextValue: unknown,
  progressValue: unknown | null,
): StorageCompactionJobV2 {
  const previous = validateStorageCompactionJobV2(previousValue);
  const next = validateStorageCompactionJobV2(nextValue);
  if (previous.jobId !== next.jobId) fail("compaction v2 job identity cannot change");
  const same = sameContractValue(previous, next);
  const sameCursor = sameContractValue(previous.cursor, next.cursor);
  const batchCommitted = () => {
    validateStorageCompactionCursorTransitionV2(
      previous.cursor, next.cursor, progressValue,
    );
    return previous.state === "running" && next.state === "pending"
      && next.authorityCount === 0
      && previous.operationsVerified === next.operationsVerified
      && previous.equivalenceVerified === next.equivalenceVerified;
  };
  const valid = operation === "arm"
    ? previous.state === "pending" && next.state === "running"
      && previous.authorityCount === 0 && next.authorityCount === 0
      && sameCursor && previous.operationsVerified === next.operationsVerified
      && previous.equivalenceVerified === next.equivalenceVerified
      && progressValue === null
    : operation === "batchSuccess"
      ? batchCommitted()
      : operation === "cancelBeforeArm"
        ? previous.state === "pending" && progressValue === null && same
        : operation === "cancelAfterArm"
          ? previous.state === "running" && progressValue === null && same
          : operation === "commitUncertain"
            ? (previous.state === "running" && progressValue === null && same)
              || batchCommitted()
            : operation === "complete"
              ? false
              : false;
  if (!valid) fail("storage compaction v2 lifecycle transition is invalid");
  return next;
}

export function validateStorageCompactionCursorV1(value: unknown): StorageCompactionCursorV1 {
  const cursor = requireObject(value, "storage compaction cursor");
  exactKeys(cursor, [
    "schemaVersion", "cutoffEventRowid", "phase", "afterEventRowid", "scanExhausted",
  ]);
  const cutoff = requireSafeUnsigned(cursor.cutoffEventRowid, "compaction.cutoffEventRowid");
  const after = requireSafeUnsigned(cursor.afterEventRowid, "compaction.afterEventRowid");
  if (cursor.schemaVersion !== STORAGE_COMPACTION_CURSOR_SCHEMA_VERSION_V1
    || !STORAGE_COMPACTION_PHASES_V1.includes(cursor.phase as StorageCompactionPhaseV1)
    || typeof cursor.scanExhausted !== "boolean" || after > cutoff) {
    fail("storage compaction cursor is invalid");
  }
  return cursor as unknown as StorageCompactionCursorV1;
}

export function validateStorageCompactionCursorTransitionV1(
  previousValue: unknown | null,
  nextValue: unknown,
): StorageCompactionCursorV1 {
  const next = validateStorageCompactionCursorV1(nextValue);
  if (previousValue === null) {
    if (next.phase !== "freezeMessages" || next.afterEventRowid !== 0 || next.scanExhausted) {
      fail("storage compaction must start freezeMessages at zero unexhausted");
    }
    return next;
  }
  const previous = validateStorageCompactionCursorV1(previousValue);
  if (next.cutoffEventRowid !== previous.cutoffEventRowid) {
    fail("storage compaction cursor cannot change its frozen cutoff");
  }
  if (next.phase === previous.phase) {
    if (previous.scanExhausted || next.scanExhausted
      || next.afterEventRowid <= previous.afterEventRowid) {
      fail("storage compaction same-phase progress must increase within one bounded batch");
    }
    return next;
  }
  const previousIndex = STORAGE_COMPACTION_PHASES_V1.indexOf(previous.phase);
  if (!previous.scanExhausted || next.scanExhausted
    || STORAGE_COMPACTION_PHASES_V1[previousIndex + 1] !== next.phase
    || next.afterEventRowid !== 0) {
    fail("storage compaction phase advance requires exhausted exact predecessor");
  }
  return next;
}

export function validateStorageCompactionPrefixV1(value: unknown): StorageCompactionPrefixV1 {
  const prefix = requireObject(value, "storage compaction prefix");
  exactKeys(prefix, [
    "prefixId", "aggregateType", "aggregateId", "deletedThroughSequence",
    "anchorEventRowid", "anchorEventId", "anchorOperationId", "anchorAggregateSequence",
    "baselineId", "baselineProjectionDigest", "createdAt", "verifiedAt",
  ]);
  const deleted = requireSafeUnsigned(prefix.deletedThroughSequence, "prefix.deletedThroughSequence");
  const anchorRowid = requireSafeUnsigned(prefix.anchorEventRowid, "prefix.anchorEventRowid");
  const anchorSequence = requireSafeUnsigned(
    prefix.anchorAggregateSequence, "prefix.anchorAggregateSequence",
  );
  if (!nonEmpty(prefix.prefixId) || !nonEmpty(prefix.aggregateType)
    || prefix.aggregateType === "app_instance" || !nonEmpty(prefix.aggregateId)
    || deleted === 0 || anchorRowid === 0 || !nonEmpty(prefix.anchorEventId)
    || !nonEmpty(prefix.anchorOperationId) || anchorSequence !== deleted + 1
    || !nonEmpty(prefix.baselineId) || !nonEmpty(prefix.baselineProjectionDigest)) {
    fail("storage compaction prefix must bind an exact successor anchor and baseline");
  }
  requireTimestamp(prefix.createdAt, "prefix.createdAt");
  requireTimestamp(prefix.verifiedAt, "prefix.verifiedAt");
  return prefix as unknown as StorageCompactionPrefixV1;
}

export function validateFrozenMessageSourceV1(value: unknown): FrozenMessageSourceV1 {
  const source = requireObject(value, "frozen message source");
  exactKeys(source, [
    "messageId", "sourceEventRowid", "sourceEventId", "sourceOperationId", "chatId",
    "sourceAggregateSequence", "durableSequence", "canonicalMessageJson", "messageDigest",
    "frozenAt",
  ]);
  if (!nonEmpty(source.messageId)
    || requireSafeUnsigned(source.sourceEventRowid, "frozen.sourceEventRowid") === 0
    || !nonEmpty(source.sourceEventId) || !nonEmpty(source.sourceOperationId)
    || !nonEmpty(source.chatId)
    || requireSafeUnsigned(source.sourceAggregateSequence, "frozen.sourceAggregateSequence") === 0
    || requireSafeUnsigned(source.durableSequence, "frozen.durableSequence") === 0
    || !nonEmpty(source.canonicalMessageJson) || !nonEmpty(source.messageDigest)) {
    fail("frozen message source must preserve exact introduction provenance");
  }
  requireTimestamp(source.frozenAt, "frozen.frozenAt");
  return source as unknown as FrozenMessageSourceV1;
}

export function validateStorageCompactionDeleteAuthorityBatchV1(
  value: unknown,
): StorageCompactionDeleteAuthorityV1[] {
  if (!Array.isArray(value) || value.length === 0
    || value.length > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1) {
    fail("storage compaction authority batch must contain one through one hundred rows");
  }
  const rows = value.map((item) => requireObject(item, "storage compaction authority"));
  const expectedJob = rows[0]?.jobId;
  const ordinals = new Set<number>();
  const rowids = new Set<number>();
  const eventIds = new Set<string>();
  const operationIds = new Set<string>();
  const aggregateTuples = new Set<string>();
  for (const row of rows) {
    exactKeys(row, [
      "jobId", "ordinal", "prefixId", "prefixDeletedThroughSequence", "eventRowid",
      "eventId", "operationId", "aggregateType", "aggregateId", "aggregateSequence",
      "introducedMessageId", "frozenMessageId", "authorizedAt",
    ]);
    const ordinal = requireSafeUnsigned(row.ordinal, "authority.ordinal");
    const prefix = requireSafeUnsigned(
      row.prefixDeletedThroughSequence, "authority.prefixDeletedThroughSequence",
    );
    const rowid = requireSafeUnsigned(row.eventRowid, "authority.eventRowid");
    const sequence = requireSafeUnsigned(row.aggregateSequence, "authority.aggregateSequence");
    const tuple = `${String(row.aggregateType)}\u0000${String(row.aggregateId)}\u0000${sequence}`;
    if (!nonEmpty(row.jobId) || row.jobId !== expectedJob || ordinal === 0
      || ordinal > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1 || !nonEmpty(row.prefixId)
      || prefix === 0 || rowid === 0 || !nonEmpty(row.eventId) || !nonEmpty(row.operationId)
      || !nonEmpty(row.aggregateType) || row.aggregateType === "app_instance"
      || !nonEmpty(row.aggregateId) || sequence === 0 || sequence > prefix
      || row.introducedMessageId !== row.frozenMessageId
      || ordinals.has(ordinal) || rowids.has(rowid)
      || eventIds.has(row.eventId as string) || operationIds.has(row.operationId as string)
      || aggregateTuples.has(tuple)) {
      fail("storage compaction authority must be one exact distinct prefix-bound tuple");
    }
    ordinals.add(ordinal); rowids.add(rowid); eventIds.add(row.eventId as string);
    operationIds.add(row.operationId as string); aggregateTuples.add(tuple);
    requireTimestamp(row.authorizedAt, "authority.authorizedAt");
  }
  return rows as unknown as StorageCompactionDeleteAuthorityV1[];
}

export function validateStorageCompactionJobV1(value: unknown): StorageCompactionJobV1 {
  const job = requireObject(value, "storage compaction job");
  exactKeys(job, [
    "jobId", "state", "cursor", "authorityCount", "equivalenceVerified",
    "failureKind", "completedAt",
  ]);
  const cursor = validateStorageCompactionCursorV1(job.cursor);
  const authorityCount = requireSafeUnsigned(job.authorityCount, "compaction.authorityCount");
  if (!nonEmpty(job.jobId) || authorityCount > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
    || !["pending", "running", "succeeded", "failed"].includes(job.state as string)
    || typeof job.equivalenceVerified !== "boolean") {
    fail("storage compaction job identity or authority count is invalid");
  }
  if (job.state === "pending" || job.state === "running") {
    if (job.failureKind !== null || job.completedAt !== null) {
      fail("nonterminal compaction job cannot carry terminal evidence");
    }
  } else if (job.state === "succeeded") {
    if (authorityCount !== 0 || !job.equivalenceVerified || cursor.phase !== "finalize"
      || !cursor.scanExhausted || job.failureKind !== null) {
      fail("successful compaction requires empty authority and verified finalization");
    }
    requireTimestamp(job.completedAt, "compaction.completedAt");
  } else {
    if (authorityCount !== 0 || typeof job.failureKind !== "string"
      || !/^[A-Za-z][A-Za-z0-9._-]{0,63}$/.test(job.failureKind)) {
      fail("failed compaction requires empty authority and sanitized evidence");
    }
    requireTimestamp(job.completedAt, "compaction.completedAt");
  }
  return job as unknown as StorageCompactionJobV1;
}

export function validateStorageCompactionLifecycleTransitionV1(
  operation: StorageCompactionLifecycleOperationV1,
  previousValue: unknown,
  nextValue: unknown,
): StorageCompactionJobV1 {
  const previous = validateStorageCompactionJobV1(previousValue);
  const next = validateStorageCompactionJobV1(nextValue);
  if (previous.jobId !== next.jobId) fail("storage compaction lifecycle cannot change job identity");
  const sameCursor = sameContractValue(previous.cursor, next.cursor);
  const valid = operation === "arm"
    ? previous.state === "pending" && next.state === "running"
      && previous.authorityCount === 0 && next.authorityCount === 0 && sameCursor
      && previous.equivalenceVerified === next.equivalenceVerified
    : operation === "batchSuccess"
      ? previous.state === "running" && previous.authorityCount > 0
        && next.state === "pending" && next.authorityCount === 0
        && transitionAccepted(previous.cursor, next.cursor)
      : operation === "cancelBeforeArm"
        ? previous.state === "pending" && previous.authorityCount === 0
          && sameContractValue(previous, next)
        : operation === "cancelAfterArm"
          ? previous.state === "running" && previous.authorityCount === 0
            && sameContractValue(previous, next)
          : operation === "fail"
            ? previous.state === "running" && previous.authorityCount === 0
              && next.state === "failed" && next.authorityCount === 0 && sameCursor
            : previous.state === "pending" && previous.authorityCount === 0
              && previous.equivalenceVerified && previous.cursor.phase === "finalize"
              && previous.cursor.scanExhausted && next.state === "succeeded" && sameCursor;
  if (!valid) fail("storage compaction lifecycle transition is not fail-closed");
  return next;
}

export function validateStorageCompactionEquivalenceV1(
  value: unknown,
): StorageCompactionEquivalenceV1 {
  const evidence = requireObject(value, "storage compaction equivalence");
  exactKeys(evidence, [
    "baselineTailBefore", "anchorTailAfter", "preservedArtifactsByteExact",
    "receiptReplayEquivalent", "namespaceDecisionsEquivalent", "frozenMessageIdsReserved",
    "authorityEmpty", "rowsInBatch",
  ]);
  const rows = requireSafeUnsigned(evidence.rowsInBatch, "equivalence.rowsInBatch");
  if (rows === 0 || rows > STORAGE_COMPACTION_MAX_DELETE_ROWS_V1
    || evidence.baselineTailBefore !== true || evidence.anchorTailAfter !== true
    || evidence.preservedArtifactsByteExact !== true
    || evidence.receiptReplayEquivalent !== true
    || evidence.namespaceDecisionsEquivalent !== true
    || evidence.frozenMessageIdsReserved !== true || evidence.authorityEmpty !== true) {
    fail("storage compaction equivalence evidence is incomplete");
  }
  return evidence as unknown as StorageCompactionEquivalenceV1;
}

function transitionAccepted(previous: StorageCompactionCursorV1, next: StorageCompactionCursorV1) {
  try {
    validateStorageCompactionCursorTransitionV1(previous, next);
    return true;
  } catch {
    return false;
  }
}

function nonEmpty(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

export function validateStorageEventSourceIndexEntryV1(
  value: unknown,
): StorageEventSourceIndexEntryV1 {
  const entry = requireObject(value, "event source index entry");
  exactKeys(entry, [
    "eventRowid", "eventId", "operationId", "aggregateType", "aggregateId",
    "aggregateSequence", "introducedMessageId", "chatDurableSequence",
    "chatSnapshotMessageId",
  ]);
  const eventRowid = requireSafeUnsigned(entry.eventRowid, "sourceIndex.eventRowid");
  const aggregateSequence = requireSafeUnsigned(
    entry.aggregateSequence,
    "sourceIndex.aggregateSequence",
  );
  if (eventRowid === 0 || aggregateSequence === 0
    || ![entry.eventId, entry.operationId, entry.aggregateType, entry.aggregateId]
      .every((field) => typeof field === "string" && field.length > 0)
    || !(entry.introducedMessageId === null
      || (typeof entry.introducedMessageId === "string" && entry.introducedMessageId.length > 0))
    || !(entry.chatSnapshotMessageId === null
      || (typeof entry.chatSnapshotMessageId === "string"
        && entry.chatSnapshotMessageId.length > 0))
    || !(entry.chatDurableSequence === null
      || (typeof entry.chatDurableSequence === "number"
        && Number.isSafeInteger(entry.chatDurableSequence)
        && entry.chatDurableSequence >= 0))) {
    fail("event source index identity and aggregate tuple must be non-empty and positive");
  }
  return entry as unknown as StorageEventSourceIndexEntryV1;
}

function storageChatSourceStateV1(source: OperationCandidateV1): {
  durable: number;
  snapshotMessageId: string | null;
  message: ReturnType<typeof validateTeamMessageV1> | null;
} {
  let payload: unknown;
  try {
    payload = JSON.parse(source.canonical_json_payload);
  } catch {
    fail("source index event payload must be valid JSON");
  }
  const object = requireObject(payload, "chat source index payload");
  const durable = requireSafeUnsigned(
    object.durable_event_sequence,
    "sourceIndex.chatDurableSequence",
  );
  if (!("last_message" in object)
    || !(object.last_message === null || typeof object.last_message === "object")) {
    fail("chat source index last_message must be an object or null");
  }
  const message = object.last_message === null ? null : validateTeamMessageV1(object.last_message);
  if (message !== null
    && (message.chatId !== source.aggregate_id || message.eventSequence !== durable)) {
    fail("chat snapshot message must match aggregate id and durable sequence");
  }
  return { durable, snapshotMessageId: message?.messageId ?? null, message };
}

export function validateStorageEventSourceIndexBindingV1(
  eventRowid: number,
  entryValue: unknown,
  source: OperationCandidateV1,
): StorageEventSourceIndexEntryV1 {
  const entry = validateStorageEventSourceIndexEntryV1(entryValue);
  const expected = source.aggregate_type === "chat"
    ? storageChatSourceStateV1(source)
    : { durable: null, snapshotMessageId: null };
  if (entry.eventRowid !== eventRowid
    || entry.eventId !== source.event_id
    || entry.operationId !== source.operation_id
    || entry.aggregateType !== source.aggregate_type
    || entry.aggregateId !== source.aggregate_id
    || entry.aggregateSequence !== source.aggregate_sequence
    || entry.chatDurableSequence !== expected.durable
    || entry.chatSnapshotMessageId !== expected.snapshotMessageId
    || (source.aggregate_type !== "chat" && entry.introducedMessageId !== null)) {
    fail("event source index must match one exact source event and mirrored chat snapshot");
  }
  return entry;
}

export function validateStorageEventSourceIndexTransitionV1(
  eventRowid: number,
  entryValue: unknown,
  source: OperationCandidateV1,
  predecessorEntryValue: unknown | null,
  predecessorSource: OperationCandidateV1 | null,
): StorageEventSourceIndexEntryV1 {
  const entry = validateStorageEventSourceIndexBindingV1(eventRowid, entryValue, source);
  if (source.aggregate_type !== "chat") return entry;
  const current = storageChatSourceStateV1(source);
  let previousDurable: number;
  let previousSnapshot: string | null;
  if (source.aggregate_sequence === 1) {
    if (predecessorEntryValue !== null || predecessorSource !== null) {
      fail("first chat source index row must use the synthetic zero predecessor");
    }
    previousDurable = 0;
    previousSnapshot = null;
  } else if (predecessorEntryValue !== null) {
    const predecessor = validateStorageEventSourceIndexEntryV1(predecessorEntryValue);
    if (predecessor.aggregateType !== "chat"
      || predecessor.aggregateId !== source.aggregate_id
      || predecessor.aggregateSequence + 1 !== source.aggregate_sequence
      || predecessor.chatDurableSequence === null) {
      fail("indexed predecessor must be the exact aggregate sequence minus one");
    }
    if (predecessor.chatSnapshotMessageId !== null) {
      if (predecessorSource === null) {
        fail("indexed non-null snapshot predecessor requires exact source provenance");
      }
      validateStorageEventSourceIndexBindingV1(
        predecessor.eventRowid, predecessor, predecessorSource,
      );
    } else if (predecessorSource !== null) {
      validateStorageEventSourceIndexBindingV1(
        predecessor.eventRowid, predecessor, predecessorSource,
      );
    }
    previousDurable = predecessor.chatDurableSequence;
    previousSnapshot = predecessor.chatSnapshotMessageId;
  } else {
    fail("chat source index transition requires the exact predecessor source index row");
  }

  if (current.durable === previousDurable + 1) {
    if (current.message === null
      || current.message.eventSequence !== current.durable
      || current.message.chatId !== source.aggregate_id
      || entry.introducedMessageId !== current.snapshotMessageId) {
      fail("message introduction must match current durable sequence, chat, and snapshot");
    }
  } else if (current.durable === previousDurable) {
    if (entry.introducedMessageId !== null
      || (current.snapshotMessageId !== null
        && current.snapshotMessageId !== previousSnapshot)) {
      fail("non-message transition must keep durable sequence and null-or-same snapshot");
    }
  } else {
    fail("chat durable sequence must stay unchanged or increase by exactly one");
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
