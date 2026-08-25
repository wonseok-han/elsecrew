export const SQLITE_SCHEMA_VERSION = 1 as const;

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
  "content_hash",
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
  content_hash: string;
  timestamp: string;
}

export type OperationDecisionV1 = "append" | "duplicateNoOp" | "conflict";

export interface OperationRecordV1 {
  operation_id: string;
  request_id: string | null;
  content_hash: string;
}

export interface ProjectionStateV1 {
  app_instances: Record<string, Record<string, unknown>>;
  projects: Record<string, Record<string, unknown>>;
  chats: Record<string, Record<string, unknown>>;
  runs: Record<string, Record<string, unknown>>;
  locks: Record<string, Record<string, unknown>>;
}

export interface RecoveryInputV1 {
  previous_app_instance: Record<string, unknown>;
  runs: Record<string, unknown>[];
  locks: Record<string, unknown>[];
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

export function classifyOperationV1(
  existing: readonly OperationRecordV1[],
  incoming: OperationRecordV1,
): OperationDecisionV1 {
  requireId(incoming.operation_id, "operation_id");
  requireId(incoming.content_hash, "content_hash");
  const previous = existing.find((entry) => entry.operation_id === incoming.operation_id);
  if (previous === undefined) return "append";
  return previous.content_hash === incoming.content_hash ? "duplicateNoOp" : "conflict";
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
  const previous = cloneRecord(input.previous_app_instance);
  const runs = input.runs.map(cloneRecord);
  const locks = input.locks.map(cloneRecord);
  if (previous.clean_shutdown_at !== null) {
    return { recovery_required: false, blocker_ids: [], runs, locks };
  }

  const affectedRuns = new Set<string>();
  for (const run of runs) {
    const runId = requireRecordId(run, "run_id");
    if (typeof run.state === "string" && ACTIVE_RECOVERY_STATES.has(run.state)) {
      run.state = "recoveryRequired";
      affectedRuns.add(runId);
    }
  }

  const blockerIds = [...affectedRuns].map((runId) => `run:${runId}`);
  for (const lock of locks) {
    const lockId = requireRecordId(lock, "lock_id");
    if (typeof lock.writer_run_id === "string" && affectedRuns.has(lock.writer_run_id)) {
      lock.recovery_required = true;
      blockerIds.push(`lock:${lockId}`);
    }
  }
  blockerIds.sort();
  return {
    recovery_required: blockerIds.length > 0,
    blocker_ids: blockerIds,
    runs,
    locks,
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
    "content_hash",
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

function fail(message: string): never {
  throw new Error(message);
}
