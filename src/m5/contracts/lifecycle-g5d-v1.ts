import { parseG5DAgentFixtureReadyPayloadV1 } from "./agent-adapter-v1.ts";
import { APP_SCHEMA_VERSION, type ProtocolErrorV1, type ReadyPayloadV1 } from "./protocol-v1.ts";
import { parseG5DProductReadyPayloadV1 } from "./run-recovery-v1.ts";

export const G5D_ALLOWED_WHILE_STORAGE_NOT_READY_V1 = [
  "system.status",
  "app.prepareQuit",
] as const;
export const G5D_STORAGE_READY_ONLY_METHODS_V1 = ["storage.status"] as const;
export const G5D_STARTUP_STEPS_V1 = [
  "sidecarStarted",
  "shellReadyEmitted",
  "storageConnectionVerified",
  "migrationComplete",
  "integrityCheckComplete",
  "projectionReplayComplete",
  "recoveryComplete",
  "verifiedForStartup",
  "appInstanceRegistered",
  "storageAdmissionReady",
] as const;
export const STORAGE_ADMISSION_FAILED_DETAIL_V1 = "storage admission failed";
export const STORAGE_VERIFYING_BLOCKED_DETAIL_V1 = "storage admission is still verifying";

export interface DatabaseVerifyingG5DV1 {
  health: "verifying";
  schemaVersion: null;
}

export interface DatabaseReadyG5DV1 {
  health: "healthy";
  schemaVersion: typeof APP_SCHEMA_VERSION;
}

export interface DatabaseFailedG5DV1 {
  health: "unavailable";
  schemaVersion: typeof APP_SCHEMA_VERSION | null;
}

export type StorageAdmissionG5DV1 =
  | {
      schemaVersion: 1;
      state: "verifying";
      startedAt: string;
      completedAt: null;
      error: null;
    }
  | {
      schemaVersion: 1;
      state: "ready";
      startedAt: string;
      completedAt: string;
      error: null;
    }
  | {
      schemaVersion: 1;
      state: "failed";
      startedAt: string;
      completedAt: string;
      error: ProtocolErrorV1;
    };

interface SystemStatusG5DBaseV1 {
  schemaVersion: 1;
  state: "shellReady";
  sidecar: ReadyPayloadV1;
  blockerIds: string[];
}

export type SystemStatusG5DV1 =
  | (SystemStatusG5DBaseV1 & {
      storageAdmission: Extract<StorageAdmissionG5DV1, { state: "verifying" }>;
      database: DatabaseVerifyingG5DV1;
      recoveryRequired: null;
    })
  | (SystemStatusG5DBaseV1 & {
      storageAdmission: Extract<StorageAdmissionG5DV1, { state: "ready" }>;
      database: DatabaseReadyG5DV1;
      recoveryRequired: boolean;
    })
  | (SystemStatusG5DBaseV1 & {
      storageAdmission: Extract<StorageAdmissionG5DV1, { state: "failed" }>;
      database: DatabaseFailedG5DV1;
      recoveryRequired: null;
    });

export type StorageAdmissionStateV1 = StorageAdmissionG5DV1["state"];

export interface StartupTraceG5DV1 {
  schemaVersion: 1;
  steps: readonly (typeof G5D_STARTUP_STEPS_V1)[number][];
}

export function parseG5DProductSystemStatusV1(value: unknown): SystemStatusG5DV1 {
  return parseSystemStatus(value, parseG5DProductReadyPayloadV1);
}

/** Optional follow-up suffix over an otherwise exact D1 baseline. */
export function parseCurrentSystemStatusV1(value: unknown, agentFixture: boolean): SystemStatusG5DV1 {
  return parseSystemStatus(value, (sidecar) => {
    const baseline = agentFixture ? parseG5DAgentFixtureReadyPayloadV1 : parseG5DProductReadyPayloadV1;
    if (typeof sidecar === "object" && sidecar !== null && "capabilities" in sidecar && Array.isArray(sidecar.capabilities) && sidecar.capabilities[sidecar.capabilities.length - 1] === "project.catalog") {
      const caps = sidecar.capabilities;
      const suffix = ["chat.appendMessage", "provider.login", "run.approval", "project.catalog"];
      if (caps.slice(-4).join("|") !== suffix.join("|")) throw new Error("invalid catalog capability baseline");
      const parsed = baseline({ ...sidecar, capabilities: caps.slice(0, -4) });
      return { ...parsed, capabilities: [...parsed.capabilities, ...suffix] };
    }
    if (typeof sidecar === "object" && sidecar !== null && "capabilities" in sidecar && Array.isArray(sidecar.capabilities) && sidecar.capabilities[sidecar.capabilities.length - 1] === "run.approval") {
      const caps = sidecar.capabilities;
      if (caps[caps.length - 2] !== "provider.login" || caps[caps.length - 3] !== "chat.appendMessage") throw new Error("invalid approval capability baseline");
      const parsed = baseline({ ...sidecar, capabilities: caps.slice(0,-3) });
      return { ...parsed, capabilities: [...parsed.capabilities, "chat.appendMessage", "provider.login", "run.approval"] };
    }
    if (typeof sidecar === "object" && sidecar !== null && "capabilities" in sidecar && Array.isArray(sidecar.capabilities) && sidecar.capabilities[sidecar.capabilities.length-1] === "provider.login") {
      const caps = sidecar.capabilities;
      if (caps[caps.length-2] !== "chat.appendMessage") throw new Error("invalid login capability baseline");
      const parsed = baseline({ ...sidecar, capabilities: caps.slice(0,-2) });
      return { ...parsed, capabilities: [...parsed.capabilities, "chat.appendMessage", "provider.login"] };
    }
    if (typeof sidecar !== "object" || sidecar === null || !("capabilities" in sidecar) || !Array.isArray(sidecar.capabilities) || sidecar.capabilities[sidecar.capabilities.length - 1] !== "chat.appendMessage") return baseline(sidecar);
    const parsed = baseline({ ...sidecar, capabilities: sidecar.capabilities.slice(0, -1) });
    return { ...parsed, capabilities: [...parsed.capabilities, "chat.appendMessage"] };
  });
}

export function parseG5DAgentFixtureSystemStatusV1(value: unknown): SystemStatusG5DV1 {
  return parseSystemStatus(value, parseG5DAgentFixtureReadyPayloadV1);
}

export function validateG5DBlockedRequestErrorV1(
  state: StorageAdmissionStateV1,
  value: unknown,
): ProtocolErrorV1 {
  const error = exactObject(
    value,
    ["code", "kind", "detail", "retryable", "outcomeUnknown"],
    "blocked request error",
  );
  const expected = state === "verifying"
    ? {
        code: "storageVerifying",
        kind: "requestBlocked",
        detail: STORAGE_VERIFYING_BLOCKED_DETAIL_V1,
        retryable: true,
        outcomeUnknown: false,
      }
    : state === "failed"
      ? {
          code: "storageAdmissionFailed",
          kind: "requestBlocked",
          detail: STORAGE_ADMISSION_FAILED_DETAIL_V1,
          retryable: false,
          outcomeUnknown: false,
        }
      : null;
  if (expected === null || !sameRecord(error, expected)) {
    fail("blocked request error does not match storage admission state");
  }
  return error as unknown as ProtocolErrorV1;
}

export function validateG5DStartupTraceV1(value: unknown): StartupTraceG5DV1 {
  const trace = exactObject(value, ["schemaVersion", "steps"], "G5D startup trace");
  if (trace.schemaVersion !== 1 || !sameStrings(trace.steps, G5D_STARTUP_STEPS_V1)) {
    fail("G5D startup must emit shell ready before storage verification and admit only after verifiedForStartup and app registration");
  }
  return trace as unknown as StartupTraceG5DV1;
}

function parseSystemStatus(
  value: unknown,
  parseSidecar: (value: unknown) => ReadyPayloadV1,
): SystemStatusG5DV1 {
  const status = exactObject(
    value,
    [
      "schemaVersion",
      "state",
      "sidecar",
      "storageAdmission",
      "database",
      "recoveryRequired",
      "blockerIds",
    ],
    "G5D system status",
  );
  if (status.schemaVersion !== 1 || status.state !== "shellReady") {
    fail("G5D status must describe the schema v1 shellReady state");
  }
  parseSidecar(status.sidecar);
  const admission = exactObject(
    status.storageAdmission,
    ["schemaVersion", "state", "startedAt", "completedAt", "error"],
    "storageAdmission",
  );
  if (admission.schemaVersion !== 1) fail("storageAdmission schemaVersion must be 1");
  requireTimestamp(admission.startedAt, "startedAt");
  const database = exactObject(status.database, ["health", "schemaVersion"], "database");
  if (!Array.isArray(status.blockerIds)) fail("blockerIds must be an array");

  if (admission.state === "verifying") {
    if (
      admission.completedAt !== null ||
      admission.error !== null ||
      database.health !== "verifying" ||
      database.schemaVersion !== null ||
      status.recoveryRequired !== null ||
      status.blockerIds.length !== 0
    ) {
      fail("verifying storage admission has an invalid cross-state combination");
    }
  } else if (admission.state === "ready") {
    validateCompletedAt(admission);
    if (
      admission.error !== null ||
      database.health !== "healthy" ||
      database.schemaVersion !== APP_SCHEMA_VERSION ||
      typeof status.recoveryRequired !== "boolean"
    ) {
      fail("ready storage admission requires a healthy v1 database and no error");
    }
    validateBlockers(status.blockerIds, status.recoveryRequired);
  } else if (admission.state === "failed") {
    validateCompletedAt(admission);
    const expected = {
      code: "storageAdmissionFailed",
      kind: "storageUnavailable",
      detail: STORAGE_ADMISSION_FAILED_DETAIL_V1,
      retryable: false,
      outcomeUnknown: true,
    };
    const error = exactObject(
      admission.error,
      ["code", "kind", "detail", "retryable", "outcomeUnknown"],
      "storage admission error",
    );
    if (
      !sameRecord(error, expected) ||
      database.health !== "unavailable" ||
      (database.schemaVersion !== null && database.schemaVersion !== APP_SCHEMA_VERSION) ||
      status.recoveryRequired !== null ||
      status.blockerIds.length !== 0
    ) {
      fail("failed storage admission has an invalid cross-state combination");
    }
  } else {
    fail("unknown storage admission state");
  }
  return status as unknown as SystemStatusG5DV1;
}

function validateCompletedAt(admission: Record<string, unknown>): void {
  requireTimestamp(admission.completedAt, "completedAt");
  if ((admission.completedAt as string) < (admission.startedAt as string)) {
    fail("completedAt cannot precede startedAt");
  }
}

function validateBlockers(value: unknown[], recoveryRequired: boolean): void {
  if (value.some((item) => typeof item !== "string" || item.length === 0)) {
    fail("blockerIds cannot contain an empty identity");
  }
  if (new Set(value).size !== value.length || recoveryRequired !== (value.length > 0)) {
    fail("recoveryRequired must match unique blockerIds");
  }
}

function exactObject(value: unknown, fields: readonly string[], name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${name} must be an object`);
  }
  const object = value as Record<string, unknown>;
  if (Object.keys(object).length !== fields.length || fields.some((field) => !(field in object))) {
    fail(`${name} must contain the exact contract fields`);
  }
  return object;
}

function requireTimestamp(value: unknown, name: string): asserts value is string {
  const match = typeof value === "string"
    ? /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})Z$/.exec(value)
    : null;
  if (match === null) fail(`${name} must be an RFC 3339 UTC second timestamp`);
  const [year, month, day, hour, minute, second] = match.slice(1).map(Number);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [0, 31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (year < 1 || month < 1 || month > 12 || day < 1 || day > days[month]
    || hour > 23 || minute > 59 || second > 59) {
    fail(`${name} must be a real RFC 3339 UTC calendar timestamp`);
  }
}

function sameStrings(value: unknown, expected: readonly string[]): boolean {
  return Array.isArray(value) && value.length === expected.length
    && value.every((item, index) => item === expected[index]);
}

function sameRecord(left: Record<string, unknown>, right: Record<string, unknown>): boolean {
  return Object.keys(right).every((key) => left[key] === right[key]);
}

function fail(message: string): never {
  throw new Error(message);
}
