export const SQLITE_SCHEMA_VERSION = 1 as const;
export const RECOVERY_VALIDATION_FAILURE_STATUS_V1 = "unavailable" as const;

export const SQLITE_TABLES_V1 = [
  "schema_meta",
  "app_instances",
  "events",
  "projects",
  "chats",
  "runs",
  "locks",
] as const;

export const LEDGER_FIELDS_V1 = [
  "event_id",
  "operation_id",
  "request_id",
  "aggregate_type",
  "aggregate_id",
  "aggregate_sequence",
  "event_type",
  "payload_schema_version",
  "canonical_json_payload",
  "timestamp",
] as const;

export type ProjectionTableV1 = "app_instances" | "projects" | "chats" | "runs" | "locks";
export type AggregateTypeV1 = "app_instance" | "project" | "chat" | "run" | "lock";

export interface LedgerEventV1 {
  event_id: string;
  operation_id: string;
  request_id: string | null;
  aggregate_type: AggregateTypeV1;
  aggregate_id: string;
  aggregate_sequence: number;
  event_type: string;
  payload_schema_version: typeof SQLITE_SCHEMA_VERSION;
  canonical_json_payload: string;
  timestamp: string;
}

export type OperationDecisionV1 = "append" | "duplicateNoOp" | "conflict";

export type OperationRecordV1 = LedgerEventV1;

export interface ProjectionStateV1 {
  app_instances: Record<string, Record<string, unknown>>;
  projects: Record<string, Record<string, unknown>>;
  chats: Record<string, Record<string, unknown>>;
  runs: Record<string, Record<string, unknown>>;
  locks: Record<string, Record<string, unknown>>;
}

export interface RecoveryInputV1 {
  previous_app_instance: Record<string, unknown> | null;
  runs: Record<string, unknown>[];
  locks: Record<string, unknown>[];
}

export interface StorageStateV1 {
  events: LedgerEventV1[];
  projection: ProjectionStateV1;
}

export type TransactionFailurePointV1 = "afterEventInsertBeforeProjection" | null;
export type TransactionOutcomeV1 = "committed" | "duplicateNoOp" | "conflict" | "rolledBack";

export interface StorageTransactionResultV1 {
  outcome: TransactionOutcomeV1;
  steps: string[];
  state: StorageStateV1;
  risky_operation_started: boolean;
}

export interface RecoveryResultV1 {
  recovery_required: boolean;
  blocker_ids: string[];
  runs: Record<string, unknown>[];
  locks: Record<string, unknown>[];
}

const AGGREGATE_TABLES: Record<AggregateTypeV1, ProjectionTableV1> = {
  app_instance: "app_instances",
  project: "projects",
  chat: "chats",
  run: "runs",
  lock: "locks",
};

const AGGREGATE_IDS: Record<AggregateTypeV1, string> = {
  app_instance: "sidecar_instance_id",
  project: "project_id",
  chat: "chat_id",
  run: "run_id",
  lock: "lock_id",
};

const ACTIVE_RECOVERY_STATES = new Set(["preparing", "running", "cancelling"]);
const APP_INSTANCE_FIELDS = [
  "sidecar_instance_id",
  "pid",
  "platform",
  "started_at",
  "heartbeat_at",
  "clean_shutdown_at",
  "last_event_sequence",
] as const;

export function classifyOperationV1(
  existing: readonly OperationRecordV1[],
  incoming: OperationRecordV1,
): OperationDecisionV1 {
  validateLedgerEvent(incoming);
  const previous = existing.find((entry) => entry.operation_id === incoming.operation_id);
  if (previous === undefined) return "append";
  validateLedgerEvent(previous);
  return immutableOperationContentEqual(previous, incoming) ? "duplicateNoOp" : "conflict";
}

export function replayLedgerV1(events: readonly LedgerEventV1[]): ProjectionStateV1 {
  const projection = emptyProjection();
  const operationRecords: OperationRecordV1[] = [];
  const eventIds = new Set<string>();
  const aggregateSequences = new Map<string, number>();

  for (const event of events) {
    validateLedgerEvent(event);
    const decision = classifyOperationV1(operationRecords, event);
    if (decision === "duplicateNoOp") continue;
    if (decision === "conflict") {
      fail(`operation_id reused with different content: ${event.operation_id}`);
    }
    operationRecords.push(event);
    if (!eventIds.add(event.event_id)) fail(`duplicate event_id: ${event.event_id}`);

    const aggregateKey = `${event.aggregate_type}:${event.aggregate_id}`;
    const expected = (aggregateSequences.get(aggregateKey) ?? 0) + 1;
    if (event.aggregate_sequence !== expected) {
      fail(`${aggregateKey} expected aggregate_sequence ${expected}`);
    }
    aggregateSequences.set(aggregateKey, event.aggregate_sequence);

    const payload = parseCanonicalObject(event.canonical_json_payload);
    const idField = AGGREGATE_IDS[event.aggregate_type];
    if (payload[idField] !== event.aggregate_id) {
      fail(`${idField} must match aggregate_id`);
    }
    const table = AGGREGATE_TABLES[event.aggregate_type];
    projection[table][event.aggregate_id] = {
      ...payload,
      last_event_sequence: event.aggregate_sequence,
    };
  }
  return projection;
}

export function recoverUncleanShutdownV1(input: RecoveryInputV1): RecoveryResultV1 {
  const previous = validatePreviousAppInstance(input.previous_app_instance);
  const runs = input.runs.map(cloneRecord);
  const locks = input.locks.map(cloneRecord);

  const affectedRuns = new Set<string>();
  if (previous !== null && previous.clean_shutdown_at === null) {
    for (const run of runs) {
      const runId = requireRecordId(run, "run_id");
      if (typeof run.state === "string" && ACTIVE_RECOVERY_STATES.has(run.state)) {
        run.state = "recoveryRequired";
        affectedRuns.add(runId);
      }
    }
  }

  const blockerIds = new Set([...affectedRuns].map((runId) => `run:${runId}`));
  for (const lock of locks) {
    const lockId = requireRecordId(lock, "lock_id");
    if (typeof lock.poisoned !== "boolean" || typeof lock.recovery_required !== "boolean") {
      fail("persisted lock safety flags must be explicit booleans");
    }
    if (typeof lock.writer_run_id === "string" && affectedRuns.has(lock.writer_run_id)) {
      lock.recovery_required = true;
    }
    if (lock.poisoned || lock.recovery_required) blockerIds.add(`lock:${lockId}`);
  }
  const sortedBlockers = [...blockerIds].sort();
  return {
    recovery_required: sortedBlockers.length > 0,
    blocker_ids: sortedBlockers,
    runs,
    locks,
  };
}

export function applyPreRiskyStorageTransactionV1(
  before: StorageStateV1,
  incoming: LedgerEventV1,
  failurePoint: TransactionFailurePointV1,
): StorageTransactionResultV1 {
  const initial = structuredClone(before);
  if (canonicalJsonV1(replayLedgerV1(initial.events)) !== canonicalJsonV1(initial.projection)) {
    fail("stored projection does not match deterministic replay");
  }
  const steps = ["beginImmediate", "operationLookup"];
  const decision = classifyOperationV1(initial.events, incoming);
  if (decision === "duplicateNoOp") {
    return {
      outcome: "duplicateNoOp",
      steps: [...steps, "duplicateNoOp", "commit"],
      state: initial,
      risky_operation_started: false,
    };
  }
  if (decision === "conflict") {
    return {
      outcome: "conflict",
      steps: [...steps, "operationConflict", "rollback"],
      state: initial,
      risky_operation_started: false,
    };
  }

  const events = [...initial.events, structuredClone(incoming)];
  steps.push("eventInserted");
  if (failurePoint === "afterEventInsertBeforeProjection") {
    return {
      outcome: "rolledBack",
      steps: [...steps, "failureInjected", "rollback"],
      state: initial,
      risky_operation_started: false,
    };
  }
  const projection = replayLedgerV1(events);
  steps.push("projectionUpdated", "commit", "riskyOperationStarted");
  return {
    outcome: "committed",
    steps,
    state: { events, projection },
    risky_operation_started: true,
  };
}

export function canonicalJsonV1(value: unknown): string {
  return JSON.stringify(canonicalize(value));
}

function validateLedgerEvent(event: LedgerEventV1): void {
  for (const field of [
    "event_id",
    "operation_id",
    "aggregate_id",
    "event_type",
    "timestamp",
  ] as const) {
    requireId(event[field], field);
  }
  if (event.request_id !== null) requireId(event.request_id, "request_id");
  if (!(event.aggregate_type in AGGREGATE_TABLES)) fail("unknown aggregate_type");
  if (!Number.isSafeInteger(event.aggregate_sequence) || event.aggregate_sequence < 1) {
    fail("aggregate_sequence must be a positive safe integer");
  }
  if (event.payload_schema_version !== SQLITE_SCHEMA_VERSION) {
    fail("unsupported payload_schema_version");
  }
  if (event.event_type !== `${event.aggregate_type}.upserted`) {
    fail("G3 reducer accepts only aggregate upsert events");
  }
}

function immutableOperationContentEqual(left: LedgerEventV1, right: LedgerEventV1): boolean {
  return (
    left.event_id === right.event_id &&
    left.aggregate_type === right.aggregate_type &&
    left.aggregate_id === right.aggregate_id &&
    left.aggregate_sequence === right.aggregate_sequence &&
    left.event_type === right.event_type &&
    left.payload_schema_version === right.payload_schema_version &&
    left.canonical_json_payload === right.canonical_json_payload &&
    left.timestamp === right.timestamp
  );
}

function validatePreviousAppInstance(
  value: Record<string, unknown> | null,
): Record<string, unknown> | null {
  if (value === null) return null;
  const fields = Object.keys(value).sort();
  if (
    fields.length !== APP_INSTANCE_FIELDS.length ||
    APP_INSTANCE_FIELDS.some((field) => !fields.includes(field))
  ) {
    fail("previous app instance must contain the exact persisted fields");
  }
  requireId(value.sidecar_instance_id, "sidecar_instance_id");
  if (
    !Number.isSafeInteger(value.pid) ||
    (value.pid as number) < 1 ||
    (value.pid as number) > 0xffff_ffff
  ) {
    fail("previous pid is invalid");
  }
  requireId(value.platform, "platform");
  requireTimestamp(value.started_at, "started_at");
  requireTimestamp(value.heartbeat_at, "heartbeat_at");
  if (value.clean_shutdown_at !== null) {
    requireTimestamp(value.clean_shutdown_at, "clean_shutdown_at");
  }
  if (!Number.isSafeInteger(value.last_event_sequence) || (value.last_event_sequence as number) < 1) {
    fail("previous last_event_sequence is invalid");
  }
  return cloneRecord(value);
}

function parseCanonicalObject(source: string): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(source);
  } catch {
    fail("canonical_json_payload must be valid JSON");
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail("canonical_json_payload must be an object");
  }
  if (canonicalJsonV1(value) !== source) fail("canonical_json_payload is not canonical JSON");
  return value as Record<string, unknown>;
}

function canonicalize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, item]) => [key, canonicalize(item)]),
    );
  }
  return value;
}

function emptyProjection(): ProjectionStateV1 {
  return { app_instances: {}, projects: {}, chats: {}, runs: {}, locks: {} };
}

function cloneRecord(value: Record<string, unknown>): Record<string, unknown> {
  return structuredClone(value);
}

function requireRecordId(value: Record<string, unknown>, field: string): string {
  const candidate = value[field];
  requireId(candidate, field);
  return candidate;
}

function requireId(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || value.length === 0) fail(`${name} is required`);
}

function requireTimestamp(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) {
    fail(`${name} must be an RFC 3339 UTC second timestamp`);
  }
}

function fail(message: string): never {
  throw new Error(message);
}
