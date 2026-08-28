import { requireObject, exactKeys, requireSafeUnsigned } from "./project-v1.ts";
import { sha256FileVersionV1 } from "./file-v1.ts";
import { canonicalJsonV1 } from "./storage-v1.ts";

export const STORAGE_LIFETIME_SCHEMA_VERSION_V1 = 2 as const;
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
  "compact",
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

export interface StorageLifetimeCursorV1 {
  schemaVersion: typeof STORAGE_LIFETIME_CURSOR_SCHEMA_VERSION_V1;
  cutoffEventRowid: number;
  phase: StorageLifetimePhaseV1;
  afterEventRowid: number;
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
    compact: "reservedDisabledAfterVerifyCompletionOnly";
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
    table: "maintenance_jobs";
    jobKind: "losslessCompaction";
    cursorColumn: "cutoff_json";
    cursorEncoding: "canonicalJsonCursorSchemaVersion1";
    maximumActiveJobs: 1;
    activeStates: readonly ["pending", "running"];
    resumableState: "pending";
    inMemoryOnlyOutcome: "Suspended";
    persistedMaintenanceJobFailClosedStates: readonly ["running"];
    terminalMaintenanceJobStates: readonly ["succeeded", "failed"];
    failedMaintenanceJobRole: "terminalHistoryNoAutomaticRetry";
    storageMetaFailClosedStates: readonly ["materializing", "failed"];
    materializingLocation: "storage_schema_meta.materialization_state";
    sqlUniqueConstraintPresent: false;
    singleSqliteOwnerRequired: true;
    transactionSerialization: "beginImmediate";
    transactionBoundaryValidation: "zeroOrOneActiveExactPendingJobIdentityAndValidCursor";
    secondJobCreationAllowedWhenAnyLifetimeJobExists: false;
    initialTransition: "absentToPendingCursor";
    appendReactivation: "succeededToPendingSameJobCursorInAppendTransaction";
    batchCommit: "derivedRowsAndAdvancedPendingCursorAtomic";
    cancellationOrCurrentFailure: "rollbackLeavesPriorPendingCursor";
    finalPublicationTransition: "pendingToSucceeded";
    postRollbackFailureRecord: "optionalSeparateBestEffortCreateOrUpdateSanitizedFailedTransaction";
    automaticRetryEnabled: false;
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
    maxRowidChanged: "remainPendingStartOrContinueCatchUpAtObservedCutoffNeverPublishReady";
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
  if (cursor.phase === "compact" && after !== 0) {
    fail("storage lifetime reserved compact cursor must use afterEventRowid 0");
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
  if (previous.phase === "compact") {
    fail("storage lifetime compact is reserved and cannot advance");
  }
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
    "transaction", "cancellation", "job", "append", "finalPublication", "rollback",
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
    "samePhaseProgress", "phaseTransition", "compact",
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
    "table", "jobKind", "cursorColumn", "cursorEncoding", "maximumActiveJobs",
    "activeStates", "resumableState", "inMemoryOnlyOutcome",
    "persistedMaintenanceJobFailClosedStates", "terminalMaintenanceJobStates",
    "failedMaintenanceJobRole", "storageMetaFailClosedStates", "materializingLocation",
    "sqlUniqueConstraintPresent", "singleSqliteOwnerRequired", "transactionSerialization",
    "transactionBoundaryValidation", "secondJobCreationAllowedWhenAnyLifetimeJobExists",
    "initialTransition", "appendReactivation", "batchCommit", "cancellationOrCurrentFailure",
    "finalPublicationTransition", "postRollbackFailureRecord", "automaticRetryEnabled",
  ]);
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
    && cursor.compact === "reservedDisabledAfterVerifyCompletionOnly"
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
    && job.table === "maintenance_jobs"
    && job.jobKind === "losslessCompaction"
    && job.cursorColumn === "cutoff_json"
    && job.cursorEncoding === "canonicalJsonCursorSchemaVersion1"
    && job.maximumActiveJobs === 1
    && exactStringArray(job.activeStates, ["pending", "running"])
    && job.resumableState === "pending"
    && job.inMemoryOnlyOutcome === "Suspended"
    && exactStringArray(job.persistedMaintenanceJobFailClosedStates, ["running"])
    && exactStringArray(job.terminalMaintenanceJobStates, ["succeeded", "failed"])
    && job.failedMaintenanceJobRole === "terminalHistoryNoAutomaticRetry"
    && exactStringArray(job.storageMetaFailClosedStates, ["materializing", "failed"])
    && job.materializingLocation === "storage_schema_meta.materialization_state"
    && job.sqlUniqueConstraintPresent === false
    && job.singleSqliteOwnerRequired === true
    && job.transactionSerialization === "beginImmediate"
    && job.transactionBoundaryValidation === "zeroOrOneActiveExactPendingJobIdentityAndValidCursor"
    && job.secondJobCreationAllowedWhenAnyLifetimeJobExists === false
    && job.initialTransition === "absentToPendingCursor"
    && job.appendReactivation === "succeededToPendingSameJobCursorInAppendTransaction"
    && job.batchCommit === "derivedRowsAndAdvancedPendingCursorAtomic"
    && job.cancellationOrCurrentFailure === "rollbackLeavesPriorPendingCursor"
    && job.finalPublicationTransition === "pendingToSucceeded"
    && job.postRollbackFailureRecord
      === "optionalSeparateBestEffortCreateOrUpdateSanitizedFailedTransaction"
    && job.automaticRetryEnabled === false
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
      === "remainPendingStartOrContinueCatchUpAtObservedCutoffNeverPublishReady"
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

function nextPhase(phase: StorageLifetimePhaseV1): StorageLifetimePhaseV1 | null {
  const index = STORAGE_LIFETIME_PHASES_V1.indexOf(phase);
  return STORAGE_LIFETIME_PHASES_V1[index + 1] ?? null;
}

function fail(message: string): never {
  throw new Error(message);
}
