export const PROTOCOL_VERSION = 1 as const;
export const SIDECAR_SCHEMA_VERSION = 1 as const;
export const APP_SCHEMA_VERSION = 1 as const;
export const MAX_CONTROL_LINE_BYTES = 1024 * 1024;
export const MAX_TEXT_CHUNK_BYTES = 256 * 1024;
export const MAX_SEQUENCE = Number.MAX_SAFE_INTEGER;

export const METHODS_V1 = [
  "system.status",
  "app.prepareQuit",
  "project.open",
  "project.getTree",
  "project.createWorktree",
  "project.close",
  "file.read",
  "file.save",
  "file.watch",
  "diff.getSummary",
  "diff.getFile",
  "project.getRoleSettings",
  "project.updateRoleSettings",
  "chat.create",
  "chat.list",
  "chat.get",
  "run.startFixture",
  "run.startAgentSmoke",
  "run.cancel",
  "recovery.list",
  "recovery.inspect",
  "recovery.markResolved",
] as const;

export const EVENTS_V1 = [
  "system.ready",
  "system.fatal",
  "file.changed",
  "file.conflict",
  "diff.changed",
  "chat.eventAppended",
  "run.event",
  "run.finished",
  "blob.chunk",
] as const;

export type MethodV1 = (typeof METHODS_V1)[number];
export type EventNameV1 = (typeof EVENTS_V1)[number];

export interface RequestV1 {
  protocolVersion: typeof PROTOCOL_VERSION;
  kind: "request";
  sidecarInstanceId: string;
  requestId: string;
  method: MethodV1;
  params: Record<string, unknown>;
}

export interface ProtocolErrorV1 {
  code: string;
  kind: string;
  detail: string;
  retryable: boolean;
  outcomeUnknown?: boolean;
}

export interface ResponseV1 {
  protocolVersion: typeof PROTOCOL_VERSION;
  kind: "response";
  sidecarInstanceId: string;
  requestId: string;
  ok: boolean;
  result?: unknown;
  error?: ProtocolErrorV1;
}

export interface EventV1 {
  protocolVersion: typeof PROTOCOL_VERSION;
  kind: "event";
  sidecarInstanceId: string;
  streamId: string;
  seq: number;
  event: EventNameV1;
  chatId?: string;
  taskId?: string;
  runId?: string;
  payload: Record<string, unknown>;
}

export type EnvelopeV1 = RequestV1 | ResponseV1 | EventV1;

export interface ReadyPayloadV1 {
  protocolVersion: typeof PROTOCOL_VERSION;
  sidecarSchemaVersion: typeof SIDECAR_SCHEMA_VERSION;
  appSchemaVersion: typeof APP_SCHEMA_VERSION;
  pid: number;
  platform: string;
  capabilities: string[];
  sidecarInstanceId: string;
}

export interface PrepareQuitResultV1 {
  safeToQuit: boolean;
  blockers: string[];
}

export class ProtocolSessionV1 {
  #readyReceived = false;
  #sidecarInstanceId: string | undefined;
  readonly #requestIds = new Set<string>();
  readonly #responseIds = new Set<string>();
  readonly #streamSequences = new Map<string, number>();

  observe(envelope: EnvelopeV1): void {
    if (!this.#readyReceived) {
      if (envelope.kind !== "event" || envelope.event !== "system.ready") {
        fail("the first protocol envelope must be system.ready");
      }
      this.#readyReceived = true;
      this.#sidecarInstanceId = envelope.sidecarInstanceId;
    } else if (envelope.kind === "event" && envelope.event === "system.ready") {
      fail("system.ready can appear only once per session");
    }

    if (
      this.#sidecarInstanceId !== undefined &&
      this.#sidecarInstanceId !== envelope.sidecarInstanceId
    ) {
      fail("sidecarInstanceId changed within a session");
    }
    if (envelope.kind === "request") {
      if (this.#requestIds.has(envelope.requestId)) {
        fail("requestId must be unique within a session");
      }
      this.#requestIds.add(envelope.requestId);
      return;
    }
    if (envelope.kind === "response") {
      if (!this.#requestIds.has(envelope.requestId)) {
        fail("response references an unknown requestId");
      }
      if (this.#responseIds.has(envelope.requestId)) {
        fail("requestId received more than one response");
      }
      this.#responseIds.add(envelope.requestId);
      return;
    }

    const expected = (this.#streamSequences.get(envelope.streamId) ?? 0) + 1;
    if (envelope.seq !== expected) {
      fail(`stream ${envelope.streamId} expected seq ${expected}, got ${envelope.seq}`);
    }
    this.#streamSequences.set(envelope.streamId, envelope.seq);
  }
}

export function decodeJsonl(input: string): EnvelopeV1 {
  const line = stripDelimiter(input);
  if (line.length === 0) fail("protocol line is empty");
  if (utf8Bytes(line) > MAX_CONTROL_LINE_BYTES) fail("control line exceeds 1 MiB");
  if (line.includes("\n") || line.includes("\r")) {
    fail("protocol input must contain exactly one JSON line");
  }

  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch (error) {
    fail(`invalid JSON: ${String(error)}`);
  }
  const envelope = validateEnvelope(value);
  return envelope;
}

export function encodeJsonl(envelope: EnvelopeV1): string {
  validateEnvelope(envelope);
  const encoded = JSON.stringify(canonicalize(envelope));
  if (utf8Bytes(encoded) > MAX_CONTROL_LINE_BYTES) fail("control line exceeds 1 MiB");
  return `${encoded}\n`;
}

export function parseReadyPayload(event: EventV1): ReadyPayloadV1 {
  if (event.event !== "system.ready") fail("event is not system.ready");
  const payload = requireObject(event.payload, "system.ready payload");
  exactKeys(payload, [
    "protocolVersion",
    "sidecarSchemaVersion",
    "appSchemaVersion",
    "pid",
    "platform",
    "capabilities",
    "sidecarInstanceId",
  ]);
  if (payload.protocolVersion !== PROTOCOL_VERSION) {
    fail("ready protocolVersion does not match v1");
  }
  if (
    payload.sidecarSchemaVersion !== SIDECAR_SCHEMA_VERSION ||
    payload.appSchemaVersion !== APP_SCHEMA_VERSION
  ) {
    fail("ready schema version does not match v1");
  }
  requirePositiveInteger(payload.pid, "ready pid");
  requireId(payload.platform, "ready platform");
  requireId(payload.sidecarInstanceId, "ready sidecarInstanceId");
  if (payload.sidecarInstanceId !== event.sidecarInstanceId) {
    fail("ready sidecarInstanceId does not match its envelope");
  }
  if (
    !Array.isArray(payload.capabilities) ||
    payload.capabilities.length === 0 ||
    payload.capabilities.some((value) => typeof value !== "string" || value.length === 0)
  ) {
    fail("ready capabilities are required and cannot contain empty values");
  }
  if (new Set(payload.capabilities).size !== payload.capabilities.length) {
    fail("ready capabilities must be unique");
  }
  if (payload.capabilities.some((value) => !METHODS_V1.includes(value as MethodV1))) {
    fail("ready contains an unknown capability");
  }
  return payload as unknown as ReadyPayloadV1;
}

export function parsePrepareQuitResult(response: ResponseV1): PrepareQuitResultV1 {
  if (!response.ok) fail("prepareQuit response is an error");
  const result = requireObject(response.result, "prepareQuit result");
  exactKeys(result, ["safeToQuit", "blockers"]);
  if (typeof result.safeToQuit !== "boolean") fail("safeToQuit must be boolean");
  if (
    !Array.isArray(result.blockers) ||
    result.blockers.some((value) => typeof value !== "string" || value.length === 0)
  ) {
    fail("prepareQuit blockers must be non-empty strings");
  }
  if (result.safeToQuit && result.blockers.length !== 0) {
    fail("safeToQuit cannot include blockers");
  }
  if (!result.safeToQuit && result.blockers.length === 0) {
    fail("unsafe prepareQuit must include blockers");
  }
  return result as unknown as PrepareQuitResultV1;
}

function validateEnvelope(value: unknown): EnvelopeV1 {
  const object = requireObject(value, "protocol envelope");
  if (object.protocolVersion !== PROTOCOL_VERSION) {
    fail(`unsupported protocolVersion: ${String(object.protocolVersion)}`);
  }
  requireId(object.sidecarInstanceId, "sidecarInstanceId");

  switch (object.kind) {
    case "request":
      return validateRequest(object);
    case "response":
      return validateResponse(object);
    case "event":
      return validateEvent(object);
    default:
      fail(`unknown kind: ${String(object.kind)}`);
  }
}

function validateRequest(object: Record<string, unknown>): RequestV1 {
  exactKeys(object, [
    "protocolVersion",
    "kind",
    "sidecarInstanceId",
    "requestId",
    "method",
    "params",
  ]);
  requireId(object.requestId, "requestId");
  if (typeof object.method !== "string" || !METHODS_V1.includes(object.method as MethodV1)) {
    fail(`unknown protocol method: ${String(object.method)}`);
  }
  requireJsonObject(object.params, "request params");
  return object as unknown as RequestV1;
}

function validateResponse(object: Record<string, unknown>): ResponseV1 {
  exactKeys(object, [
    "protocolVersion",
    "kind",
    "sidecarInstanceId",
    "requestId",
    "ok",
    "result",
    "error",
  ]);
  requireId(object.requestId, "requestId");
  if (typeof object.ok !== "boolean") fail("response ok must be boolean");
  if (object.ok) {
    if (!("result" in object) || object.result === undefined || "error" in object) {
      fail("successful response must contain only result");
    }
    requireJsonValue(object.result, "response result");
  } else {
    if ("result" in object || !("error" in object)) {
      fail("error response must contain only error");
    }
    validateError(object.error);
  }
  return object as unknown as ResponseV1;
}

function validateError(value: unknown): ProtocolErrorV1 {
  const error = requireObject(value, "protocol error");
  exactKeys(error, ["code", "kind", "detail", "retryable", "outcomeUnknown"]);
  requireId(error.code, "error.code");
  requireId(error.kind, "error.kind");
  requireId(error.detail, "error.detail");
  if (typeof error.retryable !== "boolean") fail("error.retryable must be boolean");
  if ("outcomeUnknown" in error && typeof error.outcomeUnknown !== "boolean") {
    fail("error.outcomeUnknown must be boolean");
  }
  if (error.outcomeUnknown === true && error.retryable === true) {
    fail("an error with an unknown outcome cannot be auto-retryable");
  }
  return error as unknown as ProtocolErrorV1;
}

function validateEvent(object: Record<string, unknown>): EventV1 {
  exactKeys(object, [
    "protocolVersion",
    "kind",
    "sidecarInstanceId",
    "streamId",
    "seq",
    "event",
    "chatId",
    "taskId",
    "runId",
    "payload",
  ]);
  requireId(object.streamId, "streamId");
  requirePositiveInteger(object.seq, "event seq");
  if (typeof object.event !== "string" || !EVENTS_V1.includes(object.event as EventNameV1)) {
    fail(`unknown protocol event: ${String(object.event)}`);
  }
  for (const name of ["chatId", "taskId", "runId"] as const) {
    if (name in object) requireId(object[name], name);
  }
  if (
    (object.event === "run.event" || object.event === "run.finished") &&
    (!("chatId" in object) || !("taskId" in object) || !("runId" in object))
  ) {
    fail("run events require explicit chatId, taskId, and runId");
  }
  if (object.event === "chat.eventAppended" && !("chatId" in object)) {
    fail("chat.eventAppended requires explicit chatId");
  }
  const payload = requireJsonObject(object.payload, "event payload");
  if (object.event === "blob.chunk") {
    exactKeys(payload, ["text"]);
    if (typeof payload.text !== "string") {
      fail("blob.chunk payload must contain exactly one text field");
    }
  }
  if (typeof payload.text === "string" && utf8Bytes(payload.text) > MAX_TEXT_CHUNK_BYTES) {
    fail("text chunk exceeds 256 KiB");
  }
  const event = object as unknown as EventV1;
  if (event.event === "system.ready") parseReadyPayload(event);
  return event;
}

function exactKeys(object: Record<string, unknown>, allowed: readonly string[]): void {
  const allowedKeys = new Set(allowed);
  const unknown = Object.keys(object).find((key) => !allowedKeys.has(key));
  if (unknown !== undefined) fail(`unknown protocol field: ${unknown}`);
}

function requireObject(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${name} must be an object`);
  }
  return value as Record<string, unknown>;
}

function requireJsonObject(value: unknown, name: string): Record<string, unknown> {
  const object = requireObject(value, name);
  requireJsonValue(object, name);
  return object;
}

function requireJsonValue(value: unknown, name: string): void {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "boolean" ||
    (typeof value === "number" && Number.isFinite(value))
  ) {
    return;
  }
  if (Array.isArray(value)) {
    value.forEach((item) => requireJsonValue(item, name));
    return;
  }
  if (typeof value === "object") {
    for (const item of Object.values(value as Record<string, unknown>)) {
      requireJsonValue(item, name);
    }
    return;
  }
  fail(`${name} must contain only JSON values`);
}

function requireId(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || value.length === 0) fail(`${name} is required`);
}

function requirePositiveInteger(value: unknown, name: string): asserts value is number {
  if (!Number.isSafeInteger(value) || Number(value) < 1 || Number(value) > MAX_SEQUENCE) {
    fail(`${name} must be between 1 and 2^53-1`);
  }
}

function stripDelimiter(input: string): string {
  if (input.endsWith("\n")) {
    const withoutLf = input.slice(0, -1);
    return withoutLf.endsWith("\r") ? withoutLf.slice(0, -1) : withoutLf;
  }
  if (input.endsWith("\r")) fail("bare carriage return is not a JSONL delimiter");
  return input;
}

function canonicalize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (typeof value !== "object" || value === null) return value;
  return Object.fromEntries(
    Object.entries(value)
      .filter(([, item]) => item !== undefined)
      .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
      .map(([key, item]) => [key, canonicalize(item)]),
  );
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function fail(detail: string): never {
  throw new Error(detail);
}
