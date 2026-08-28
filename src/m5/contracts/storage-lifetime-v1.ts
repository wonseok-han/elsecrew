import { requireObject, exactKeys, requireSafeUnsigned } from "./project-v1.ts";

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
  "ledger_baselines_cutoff",
  "operation_receipts_aggregate_sequence",
  "chat_messages_chat_sequence",
  "prepared_workspaces_project_state",
  "maintenance_jobs_status_created",
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

function fail(message: string): never {
  throw new Error(message);
}
