import {
  APP_SCHEMA_VERSION,
  PROTOCOL_VERSION,
  SIDECAR_SCHEMA_VERSION,
  parseReadyPayload,
  type ProtocolErrorV1,
  type EventV1,
  type PrepareQuitResultV1,
  type ReadyPayloadV1,
} from "./protocol-v1.ts";

export const G3_CAPABILITIES_V1 = ["system.status", "app.prepareQuit"] as const;

export const M5_INTERNAL_CHANNELS_V1 = {
  systemGetStatus: "m5:v1:system:get-status",
  appPrepareQuit: "m5:v1:app:prepare-quit",
  sidecarEvent: "m5:v1:sidecar:event",
} as const;

export const M5_DATA_DIRECTORY_V1 = {
  base: "userData",
  segments: ["com.wonseokhan.helloagent", "m5"] as const,
  sqliteFile: "hello-agent.sqlite3",
  separateFromLegacyTauriStore: true,
} as const;

export const M5_PRODUCTION_SIDECAR_V1 = {
  base: "process.resourcesPath",
  relativeSegments: ["sidecar", "hello-agent-sidecar"] as const,
  windowsSuffix: ".exe",
  argv: ["--stdio"] as const,
  shell: false,
  allowEnvironmentOverride: false,
  stdout: "pipe",
  stderr: "pipe",
} as const;

export interface DatabaseReadyV1 {
  health: "healthy";
  schemaVersion: typeof APP_SCHEMA_VERSION;
}

export interface DatabaseUnavailableV1 {
  health: "unavailable";
  schemaVersion: typeof APP_SCHEMA_VERSION | null;
}

interface SystemStatusBaseV1 {
  recoveryRequired: boolean;
  blockerIds: string[];
}

export type SystemStatusV1 =
  | (SystemStatusBaseV1 & {
      state: "ready";
      sidecar: ReadyPayloadV1;
      database: DatabaseReadyV1;
    })
  | (SystemStatusBaseV1 & {
      state: "unavailable";
      lastReadySidecar: ReadyPayloadV1 | null;
      database: DatabaseUnavailableV1;
      error: ProtocolErrorV1;
    });

export const STARTUP_STEPS_V1 = [
  "sidecarStarted",
  "storageConnectionVerified",
  "migrationComplete",
  "integrityCheckComplete",
  "projectionReplayComplete",
  "recoveryComplete",
  "readyEmitted",
] as const;

export type StartupStepV1 = (typeof STARTUP_STEPS_V1)[number];

export type StartupTraceV1 =
  | {
      outcome: "ready";
      steps: StartupStepV1[];
      ready: ReadyPayloadV1;
    }
  | {
      outcome: "failedBeforeReady";
      steps: Exclude<StartupStepV1, "readyEmitted">[];
      stdoutLines: string[];
      stderr: string;
      exitCode: number;
    };

export const PREPARE_QUIT_STEPS_V1 = [
  "mainQuitRequested",
  "mainQuitRequestCoalesced",
  "sidecarNewRequestsBlocked",
  "sidecarWorkStopped",
  "databaseCheckpointed",
  "cleanShutdownMarked",
  "sidecarSafeResponse",
  "mainExitAllowed",
] as const;

export type PrepareQuitStepV1 = (typeof PREPARE_QUIT_STEPS_V1)[number];

export interface PrepareQuitTraceV1 {
  rendererCallCount: number;
  sidecarRequestCount: number;
  steps: PrepareQuitStepV1[];
  response: PrepareQuitResultV1;
}

export function parseSystemStatusV1(value: unknown): SystemStatusV1 {
  const status = requireObject(value, "system status");
  requireExactKeys(status, [
    "state",
    "sidecar",
    "lastReadySidecar",
    "database",
    "recoveryRequired",
    "blockerIds",
    "error",
  ]);
  if (typeof status.recoveryRequired !== "boolean") {
    fail("system status recoveryRequired must be boolean");
  }
  validateBlockers(status.blockerIds, status.recoveryRequired);

  if (status.state === "ready") {
    if (!("sidecar" in status) || "lastReadySidecar" in status || "error" in status) {
      fail("ready status must contain only current sidecar identity");
    }
    validateReadyIdentity(status.sidecar);
    const database = requireObject(status.database, "ready database status");
    requireExactKeys(database, ["health", "schemaVersion"]);
    if (database.health !== "healthy" || database.schemaVersion !== APP_SCHEMA_VERSION) {
      fail("ready status requires a healthy v1 database");
    }
    return status as unknown as SystemStatusV1;
  }

  if (status.state === "unavailable") {
    if (!("lastReadySidecar" in status) || !("error" in status) || "sidecar" in status) {
      fail("unavailable status requires last identity and structured error");
    }
    if (status.lastReadySidecar !== null) validateReadyIdentity(status.lastReadySidecar);
    validateError(status.error);
    const database = requireObject(status.database, "unavailable database status");
    requireExactKeys(database, ["health", "schemaVersion"]);
    if (
      database.health !== "unavailable" ||
      (database.schemaVersion !== null && database.schemaVersion !== APP_SCHEMA_VERSION)
    ) {
      fail("unavailable status has an invalid database state");
    }
    return status as unknown as SystemStatusV1;
  }

  fail("system status state must be ready or unavailable");
}

export function validateStartupTraceV1(value: StartupTraceV1): void {
  const steps = [...value.steps];
  assertOrderedPrefix(steps, STARTUP_STEPS_V1, "startup");
  if (value.outcome === "ready") {
    if (!sameStrings(steps, STARTUP_STEPS_V1)) {
      fail("ready requires verified storage connection, migration, integrity, replay, and recovery");
    }
    validateReadyIdentity(value.ready);
    return;
  }
  if (steps.includes("readyEmitted")) fail("pre-ready failure cannot emit ready");
  if (value.stdoutLines.length !== 0) {
    fail("pre-ready failure cannot write a protocol envelope to stdout");
  }
  if (!Number.isSafeInteger(value.exitCode) || value.exitCode <= 0) {
    fail("pre-ready failure must use a nonzero exit code");
  }
  if (value.stderr.trim().length === 0) {
    fail("pre-ready failure must write diagnostic evidence to stderr");
  }
}

export function validatePrepareQuitTraceV1(value: PrepareQuitTraceV1): void {
  if (!Number.isSafeInteger(value.rendererCallCount) || value.rendererCallCount < 1) {
    fail("prepareQuit must originate from at least one renderer call");
  }
  if (value.sidecarRequestCount !== 1) {
    fail("main must coordinate prepareQuit as one sidecar request");
  }
  const expected =
    value.rendererCallCount > 1
      ? PREPARE_QUIT_STEPS_V1
      : PREPARE_QUIT_STEPS_V1.filter((step) => step !== "mainQuitRequestCoalesced");
  if (!sameStrings(value.steps, expected)) {
    fail("prepareQuit steps do not prove main-owned single-flight safe shutdown");
  }
  if (!value.response.safeToQuit || value.response.blockers.length !== 0) {
    fail("a completed prepareQuit trace must end in a safe response without blockers");
  }
  const checkpoint = value.steps.indexOf("databaseCheckpointed");
  const cleanMarker = value.steps.indexOf("cleanShutdownMarked");
  const response = value.steps.indexOf("sidecarSafeResponse");
  if (checkpoint < 0 || cleanMarker < checkpoint || response < cleanMarker) {
    fail("sidecar may respond safe only after checkpoint and clean marker");
  }
}

function validateReadyIdentity(value: unknown): ReadyPayloadV1 {
  const ready = value as ReadyPayloadV1;
  const event: EventV1 = {
    protocolVersion: PROTOCOL_VERSION,
    kind: "event",
    sidecarInstanceId: ready?.sidecarInstanceId,
    streamId: "system",
    seq: 1,
    event: "system.ready",
    payload: value as Record<string, unknown>,
  };
  const parsed = parseReadyPayload(event);
  if (!sameStrings(parsed.capabilities, G3_CAPABILITIES_V1)) {
    fail("G3 ready capabilities must contain only system.status and app.prepareQuit");
  }
  if (
    parsed.protocolVersion !== PROTOCOL_VERSION ||
    parsed.sidecarSchemaVersion !== SIDECAR_SCHEMA_VERSION ||
    parsed.appSchemaVersion !== APP_SCHEMA_VERSION
  ) {
    fail("ready identity versions do not match v1");
  }
  return parsed;
}

function validateError(value: unknown): ProtocolErrorV1 {
  const error = requireObject(value, "system status error");
  requireExactKeys(error, ["code", "kind", "detail", "retryable", "outcomeUnknown"]);
  for (const field of ["code", "kind", "detail"] as const) {
    if (typeof error[field] !== "string" || error[field].length === 0) {
      fail(`system status error ${field} is required`);
    }
  }
  if (typeof error.retryable !== "boolean") fail("system status error retryable is required");
  if ("outcomeUnknown" in error && typeof error.outcomeUnknown !== "boolean") {
    fail("system status error outcomeUnknown must be boolean");
  }
  if (error.outcomeUnknown === true && error.retryable === true) {
    fail("unknown dangerous outcome cannot be auto-retryable");
  }
  return error as unknown as ProtocolErrorV1;
}

function validateBlockers(value: unknown, recoveryRequired: boolean): void {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string" || item.length === 0)) {
    fail("system status blockerIds must be non-empty strings");
  }
  if (new Set(value).size !== value.length) fail("system status blockerIds must be unique");
  if (recoveryRequired !== (value.length > 0)) {
    fail("recoveryRequired must agree with blockerIds");
  }
}

function assertOrderedPrefix(
  actual: readonly string[],
  expected: readonly string[],
  name: string,
): void {
  if (actual.some((step, index) => step !== expected[index])) {
    fail(`${name} steps must be an ordered prefix without gaps or duplicates`);
  }
}

function requireObject(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    fail(`${name} must be an object`);
  }
  return value as Record<string, unknown>;
}

function requireExactKeys(object: Record<string, unknown>, allowed: readonly string[]): void {
  const allowedSet = new Set(allowed);
  const unknown = Object.keys(object).find((key) => !allowedSet.has(key));
  if (unknown !== undefined) fail(`unknown lifecycle field: ${unknown}`);
}

function sameStrings(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function fail(message: string): never {
  throw new Error(message);
}
