import { sha256FileVersionV1 } from "./file-v1.ts";
import {
  METHODS_V1,
  validateAgentPublicEventPayloadV1,
  validateAgentRunStateChangedPayloadV1,
  type ReadyPayloadV1,
} from "./protocol-v1.ts";
import {
  G5_CAPABILITIES_V1,
  G5_EVENT_PAGE_MAX_ITEMS_V1,
  G5_ID_MAX_BYTES_V1,
  G5_MODEL_SELECTOR_MAX_BYTES_V1,
  G5_PERMISSIONS_V1,
  G5_REASONING_EFFORT_MAX_BYTES_V1,
  G5_ROLE_IDS_V1,
  G5_RUN_STAGES_V1,
  G5_RUN_STATES_V1,
  G5_RUNTIMES_V1,
  parseG5ReadyPayloadV1,
  validateChatDetailPageV1,
  type ChatDetailPageV1,
  type PermissionV1,
  type RoleIdV1,
  type RuntimeV1,
  type RunStageV1,
  type RunStateV1,
} from "./run-recovery-v1.ts";

export const G5A_SCHEMA_VERSION_V1 = 1 as const;
export const G5A_CAPABILITIES_V1 = [
  ...G5_CAPABILITIES_V1,
  "agent.getCapabilities",
  "run.startAgent",
  "run.cancelAgent",
  "recovery.inspectAgent",
  "recovery.resolveAgent",
] as const;
export const AGENT_INSTALLATION_STATES_V1 = ["installed", "notInstalled", "incomplete"] as const;
export const AGENT_AUTH_STATES_V1 = ["authenticated", "notAuthenticated", "expired", "unknown"] as const;
export const AGENT_DISCOVERY_SOURCES_V1 = ["machineReadableCatalog", "exactVersionManifest", "unavailable"] as const;
export const AGENT_VERIFICATION_LEVELS_V1 = ["runtimeVerified", "measuredExactVersion", "unknown"] as const;
export const AGENT_MODEL_EVIDENCE_V1 = ["runtimeVerified", "measuredExactVersion", "argvOnly", "unknown"] as const;
export const AGENT_START_OUTCOMES_V1 = [
  "started",
  "platformUnsupported",
  "notInstalled",
  "installationIncomplete",
  "unsupportedVersion",
  "notAuthenticated",
  "authExpired",
  "capabilityUnknown",
  "exactModelUnavailable",
  "reasoningUnsupported",
  "permissionUnsupported",
  "profileChanged",
  "profileUnavailable",
  "workspaceAbandoned",
  "recoveryBlocked",
  "chatStateBlocked",
  "runActive",
  "writerLocked",
] as const;
export const AGENT_CAPABILITY_BLOCK_REASONS_V1 = [
  "platformUnsupported", "notInstalled", "installationIncomplete", "unsupportedVersion",
  "notAuthenticated", "authExpired", "capabilityUnknown", "exactModelUnavailable",
] as const;
export const AGENT_PUBLIC_EVENT_KINDS_V1 = ["status", "question", "final"] as const;
export const AGENT_RUN_STATE_EVENT_STATES_V1 = [
  "running", "succeeded", "failed", "canceled", "recoveryRequired",
] as const;

export type AgentInstallationStateV1 = (typeof AGENT_INSTALLATION_STATES_V1)[number];
export type AgentAuthStateV1 = (typeof AGENT_AUTH_STATES_V1)[number];
export type AgentDiscoverySourceV1 = (typeof AGENT_DISCOVERY_SOURCES_V1)[number];
export type AgentVerificationLevelV1 = (typeof AGENT_VERIFICATION_LEVELS_V1)[number];
export type AgentModelEvidenceV1 = (typeof AGENT_MODEL_EVIDENCE_V1)[number];
export type AgentStartOutcomeV1 = (typeof AGENT_START_OUTCOMES_V1)[number];

export interface AgentCapabilitiesRequestV1 {
  runtime: RuntimeV1;
  refresh: boolean;
}

export interface AgentExecutableIdentityV1 {
  pathDigest: string;
  fileIdentityDigest: string;
  cliVersion: string;
}

export interface AgentModelCapabilityV1 {
  modelSelector: string;
  reasoningEfforts: readonly string[];
  permissions: readonly PermissionV1[];
}

export interface AgentDiscoveryEvidenceV1 {
  source: AgentDiscoverySourceV1;
  verificationLevel: AgentVerificationLevelV1;
  exactCliVersion: string | null;
  effectiveModelEvidence: AgentModelEvidenceV1;
}

export interface AgentCapabilitySnapshotV1 {
  schemaVersion: 1;
  runtime: RuntimeV1;
  platform: "darwin" | "win32";
  installationState: AgentInstallationStateV1;
  authState: AgentAuthStateV1;
  executable: AgentExecutableIdentityV1 | null;
  models: readonly AgentModelCapabilityV1[];
  discovery: AgentDiscoveryEvidenceV1;
  canStart: boolean;
  blockedReason: (typeof AGENT_CAPABILITY_BLOCK_REASONS_V1)[number] | null;
  /** SHA-256 of canonical snapshot fields except fingerprint and observedAt. */
  fingerprint: string;
  observedAt: string;
}

export interface AgentRunStartRequestV1 {
  chatId: string;
  roleId: RoleIdV1;
  inputMessageIds: readonly string[];
  expectedCapabilityFingerprint: string;
}

export interface AgentRunContextV1 {
  chatId: string;
  taskId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  roleId: RoleIdV1;
  capabilityFingerprint: string;
}

export interface AgentRunBindingV1 extends AgentRunContextV1 {
  runId: string;
}

export interface AgentRunAcceptedV1 extends AgentRunBindingV1 {
  schemaVersion: 1;
  state: "preparing";
}

export interface AgentRunRecordV1 {
  schemaVersion: 1;
  runId: string;
  chatId: string;
  taskId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  selectedProfile: AgentResolvedProfileV1;
  source: "actualAgent";
  capabilityFingerprint: string;
  state: RunStateV1;
  stage: RunStageV1;
  terminationOutcome: "cleanExit" | "recoveryRequired" | null;
  createdAt: string;
  updatedAt: string;
}

/** V4 preserves G5 fixture runs and adds actual-agent runs in a separate ledger. */
export type AgentChatDetailPageV1 = ChatDetailPageV1 & {
  agentRuns: readonly AgentRunRecordV1[];
};

export type G5ChatDetailPageV1 = ChatDetailPageV1 & { readonly agentRuns?: never };
export type AgentChatGetResultV1 = G5ChatDetailPageV1 | AgentChatDetailPageV1;

export interface AgentResolvedProfileV1 {
  roleId: RoleIdV1;
  runtime: RuntimeV1;
  modelSelector: string;
  reasoningEffort: string | null;
  permission: PermissionV1;
}

export interface AgentExecutionModeV1 {
  invocation: "oneShot";
  promptTransport: "serverResolvedMessages";
  followUp: "nextRun";
  resume: "disabled";
  activeStdin: false;
}

export interface AgentProcessIdentityV1 {
  pid: number;
  startIdentityDigest: string;
  executablePathDigest: string;
  observedGone: boolean;
}

export interface AgentLaunchBarrierEvidenceV1 {
  launcherIdentity: AgentProcessIdentityV1;
  agentIdentity: AgentProcessIdentityV1;
  trackedProcesses: readonly AgentProcessIdentityV1[];
  treeIdentityDigest: string;
  launchSpecDigest: string;
  evidenceDigest: string;
  treeTrackerAttached: true;
  runAndLockPersistedBeforeRelease: true;
  identityPersistedBeforeRelease: true;
  releaseGranted: true;
  capturedAt: string;
}

export interface AgentRunStartTranscriptV1 {
  schemaVersion: 1;
  request: AgentRunStartRequestV1;
  capability: AgentCapabilitySnapshotV1;
  resolvedProfile: AgentResolvedProfileV1;
  executionMode: AgentExecutionModeV1;
  runId: string | null;
  preflightOutcome: AgentStartOutcomeV1;
  persistedRunCount: number;
  spawnCount: number;
  lockCount: number;
  barrier: AgentLaunchBarrierEvidenceV1 | null;
}

export interface AgentPublicEventV1 extends AgentRunBindingV1 {
  schemaVersion: 1;
  eventId: string;
  kind: (typeof AGENT_PUBLIC_EVENT_KINDS_V1)[number];
  text: string;
  createdAt: string;
}

export interface AgentRunStateChangedV1 {
  schemaVersion: 1;
  eventId: string;
  run: AgentRunRecordV1 & {
    state: (typeof AGENT_RUN_STATE_EVENT_STATES_V1)[number];
  };
}

export interface AgentTerminationTranscriptV1 {
  schemaVersion: 1;
  runId: string;
  outcome: "cleanExit" | "recoveryRequired";
  processesProvenGone: boolean;
  poisoned: boolean;
  writerLockState: "none" | "released" | "recoveryRequired";
  chatState: "succeeded" | "failed" | "canceled" | "recoveryRequired";
  trackedProcesses: readonly AgentProcessIdentityV1[];
  evidenceDigest: string;
  observedAt: string;
}

export interface AgentRunIdentityV1 {
  chatId: string;
  taskId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  runId: string;
}

export type AgentRunCancelRequestV1 = AgentRunIdentityV1;

export interface AgentRecoveryRecordV1 extends AgentRunIdentityV1 {
  schemaVersion: 1;
  recoveryId: string;
  source: "actualAgent";
  reason: "uncleanShutdown" | "cancelUncertain" | "processIdentityUncertain";
  state: "unresolved" | "resolvedAbandoned";
  trackedProcesses: readonly AgentProcessIdentityV1[];
  poisoned: boolean;
  revision: number;
  createdAt: string;
  resolvedAt: string | null;
}

export interface AgentRunCancelResultV1 {
  schemaVersion: 1;
  request: AgentRunCancelRequestV1;
  run: AgentRunRecordV1;
  termination: AgentTerminationTranscriptV1;
  recovery: AgentRecoveryRecordV1 | null;
  outcome: "canceled" | "recoveryRequired";
}

export interface AgentRunTerminationPersistenceV1 {
  schemaVersion: 1;
  run: AgentRunRecordV1;
  termination: AgentTerminationTranscriptV1;
}

export interface AgentRecoveryInspectRequestV1 extends AgentRunIdentityV1 {
  recoveryId: string;
}

export interface AgentRecoveryResolveRequestV1 extends AgentRecoveryInspectRequestV1 {
  expectedRevision: number;
  resolution: "abandon";
}

export interface AgentRecoveryResolveResultV1 {
  schemaVersion: 1;
  before: AgentRecoveryRecordV1;
  request: AgentRecoveryResolveRequestV1;
  after: AgentRecoveryRecordV1 | null;
  outcome: "resolvedAbandoned" | "processesRemain" | "revisionConflict";
}

export function validateAgentCapabilitiesRequestV1(value: unknown): AgentCapabilitiesRequestV1 {
  const object = exactObject(value, ["runtime", "refresh"], "agent capabilities request");
  oneOf(object.runtime, G5_RUNTIMES_V1, "runtime");
  if (typeof object.refresh !== "boolean") fail("refresh must be boolean");
  return object as unknown as AgentCapabilitiesRequestV1;
}

export function validateAgentCapabilitySnapshotV1(value: unknown): AgentCapabilitySnapshotV1 {
  const object = exactObject(value, [
    "schemaVersion", "runtime", "platform", "installationState", "authState", "executable",
    "models", "discovery", "canStart", "blockedReason", "fingerprint", "observedAt",
  ], "agent capability snapshot");
  schema(object.schemaVersion);
  oneOf(object.runtime, G5_RUNTIMES_V1, "runtime");
  oneOf(object.platform, ["darwin", "win32"] as const, "platform");
  const installation = oneOf(object.installationState, AGENT_INSTALLATION_STATES_V1, "installationState");
  const auth = oneOf(object.authState, AGENT_AUTH_STATES_V1, "authState");
  const executable = object.executable === null ? null : validateExecutable(object.executable);
  const models = array(object.models, "models", 128).map(validateModelCapability);
  unique(models.map((model) => model.modelSelector), "model selectors");
  const discovery = validateDiscovery(object.discovery);
  if (typeof object.canStart !== "boolean") fail("canStart must be boolean");
  const blockedReason = object.blockedReason === null
    ? null
    : oneOf(object.blockedReason, AGENT_CAPABILITY_BLOCK_REASONS_V1, "blockedReason");
  fingerprintDigest(object.fingerprint, "fingerprint");
  timestamp(object.observedAt, "observedAt");

  if ((installation === "installed") !== (executable !== null)) fail("installed state and executable identity must agree");
  if (discovery.source === "unavailable" && (models.length !== 0 || discovery.exactCliVersion !== null || discovery.verificationLevel !== "unknown" || discovery.effectiveModelEvidence !== "unknown")) {
    fail("unavailable discovery cannot claim model or version evidence");
  }
  if (discovery.exactCliVersion !== null && executable !== null && discovery.exactCliVersion !== executable.cliVersion) {
    if (blockedReason !== "unsupportedVersion") fail("CLI version mismatch must be unsupportedVersion");
  }
  const expected = capabilityBlocker(object.platform as string, installation, auth, discovery, executable, models);
  if (object.canStart !== (expected === null) || blockedReason !== expected) fail("canStart and blockedReason must match verified capability evidence");
  if (object.canStart && models.length === 0) fail("startable capability must have an exact model set");
  const snapshot = object as unknown as AgentCapabilitySnapshotV1;
  if (snapshot.fingerprint !== computeAgentCapabilityFingerprintV1(snapshot)) fail("fingerprint must bind the canonical capability snapshot");
  return snapshot;
}

/** UTF-8 compact JSON; recursively sorted ASCII keys; array order preserved; null explicit. */
export function canonicalAgentCapabilitySnapshotV1(snapshot: AgentCapabilitySnapshotV1): string {
  const { fingerprint: _fingerprint, observedAt: _observedAt, ...input } = snapshot;
  return JSON.stringify(canonicalizeFingerprintValue(input));
}

export function computeAgentCapabilityFingerprintV1(snapshot: AgentCapabilitySnapshotV1): string {
  return sha256FileVersionV1(new TextEncoder().encode(canonicalAgentCapabilitySnapshotV1(snapshot)));
}

export function validateAgentRunStartRequestV1(value: unknown): AgentRunStartRequestV1 {
  const object = exactObject(value, ["chatId", "roleId", "inputMessageIds", "expectedCapabilityFingerprint"], "agent run start request");
  id(object.chatId, "chatId");
  oneOf(object.roleId, G5_ROLE_IDS_V1, "roleId");
  const messageIds = array(object.inputMessageIds, "inputMessageIds", 100);
  if (messageIds.length === 0) fail("inputMessageIds must not be empty");
  for (const [index, messageId] of messageIds.entries()) id(messageId, `inputMessageIds[${index}]`);
  unique(messageIds as string[], "inputMessageIds");
  fingerprintDigest(object.expectedCapabilityFingerprint, "expectedCapabilityFingerprint");
  return object as unknown as AgentRunStartRequestV1;
}

export function validateAgentRunAcceptedV1(value: unknown): AgentRunAcceptedV1 {
  const object = exactObject(value, [
    "schemaVersion", "runId", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity",
    "roleId", "capabilityFingerprint", "state",
  ], "agent run accepted result");
  schema(object.schemaVersion);
  validateAgentRunBindingFieldsV1(object);
  if (object.state !== "preparing") fail("accepted agent run state must be preparing");
  return object as unknown as AgentRunAcceptedV1;
}

export function validateAgentRunAcceptedAgainstContextV1(
  value: unknown,
  expectedValue: unknown,
): AgentRunAcceptedV1 {
  const accepted = validateAgentRunAcceptedV1(value);
  const expected = validateAgentRunContextV1(expectedValue);
  assertSameAgentRunContext(expected, accepted, "accepted agent run");
  return accepted;
}

export function validateAgentRunRecordV1(value: unknown): AgentRunRecordV1 {
  const object = exactObject(value, [
    "schemaVersion", "runId", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "selectedProfile",
    "source", "capabilityFingerprint", "state", "stage", "terminationOutcome", "createdAt", "updatedAt",
  ], "agent run record");
  schema(object.schemaVersion);
  agentRunId(object.runId, "runId");
  for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) id(object[key], key);
  const profile = validateProfile(object.selectedProfile);
  if (object.source !== "actualAgent") fail("agent run source must be actualAgent");
  fingerprintDigest(object.capabilityFingerprint, "capabilityFingerprint");
  const state = oneOf(object.state, G5_RUN_STATES_V1, "state");
  const stage = oneOf(object.stage, G5_RUN_STAGES_V1, "stage");
  const expectedStage: Record<RoleIdV1, RunStageV1> = { planning: "planning", coding: "implementing", review: "verifying" };
  if (stage !== expectedStage[profile.roleId]) fail("agent run stage does not match selected role snapshot");
  const termination = object.terminationOutcome === null
    ? null
    : oneOf(object.terminationOutcome, ["cleanExit", "recoveryRequired"] as const, "terminationOutcome");
  timestamp(object.createdAt, "createdAt");
  timestamp(object.updatedAt, "updatedAt");
  if (["preparing", "running", "cancelling"].includes(state) !== (termination === null)) fail("active and terminal run evidence must agree");
  if ((state === "recoveryRequired") !== (termination === "recoveryRequired")) fail("recoveryRequired run must keep recovery termination evidence");
  return object as unknown as AgentRunRecordV1;
}

export function validateAgentRunStartTranscriptV1(value: unknown): AgentRunStartTranscriptV1 {
  const object = exactObject(value, [
    "schemaVersion", "request", "capability", "resolvedProfile", "executionMode", "runId",
    "preflightOutcome", "persistedRunCount", "spawnCount", "lockCount", "barrier",
  ], "agent run start transcript");
  schema(object.schemaVersion);
  const request = validateAgentRunStartRequestV1(object.request);
  const capability = validateAgentCapabilitySnapshotV1(object.capability);
  const profile = validateProfile(object.resolvedProfile);
  validateExecutionMode(object.executionMode);
  if (object.runId !== null) agentRunId(object.runId, "runId");
  const outcome = oneOf(object.preflightOutcome, AGENT_START_OUTCOMES_V1, "preflightOutcome");
  const persisted = smallCount(object.persistedRunCount, "persistedRunCount");
  const spawned = smallCount(object.spawnCount, "spawnCount");
  const locks = smallCount(object.lockCount, "lockCount");
  const barrier = object.barrier === null ? null : validateBarrier(object.barrier);
  let expected: AgentStartOutcomeV1 = capability.blockedReason ?? "started";
  if (expected === "started" && request.expectedCapabilityFingerprint !== capability.fingerprint) expected = "profileChanged";
  if (expected === "started" && (request.roleId !== profile.roleId || profile.runtime !== capability.runtime)) expected = "profileChanged";
  const model = capability.models.find((candidate) => candidate.modelSelector === profile.modelSelector);
  if (expected === "started" && model === undefined) expected = "exactModelUnavailable";
  if (expected === "started" && profile.reasoningEffort !== null && !model!.reasoningEfforts.includes(profile.reasoningEffort)) expected = "reasoningUnsupported";
  if (expected === "started" && !model!.permissions.includes(profile.permission)) expected = "permissionUnsupported";
  if (outcome !== expected) fail("preflightOutcome must match exact capability and profile evidence");
  if (outcome === "started") {
    if (object.runId === null || persisted !== 1 || spawned !== 1 || locks !== (profile.permission === "workspaceWrite" ? 1 : 0) || barrier === null) {
      fail("started run requires one persisted run, one spawn, the exact writer lock, and identity barrier");
    }
  } else if (object.runId !== null || persisted !== 0 || spawned !== 0 || locks !== 0 || barrier !== null) {
    fail("preflight rejection must create no run, process, lock, or barrier evidence");
  }
  return object as unknown as AgentRunStartTranscriptV1;
}

export function validateAgentPublicEventV1(value: unknown): AgentPublicEventV1 {
  return validateAgentPublicEventPayloadV1(value) as AgentPublicEventV1;
}

/** Canonical immutable-ledger amplification unit: sorted compact JSON UTF-8 bytes. */
export function agentPublicLedgerRecordBytesV1(value: unknown): number {
  const event = validateAgentPublicEventV1(value);
  return new TextEncoder().encode(JSON.stringify(canonicalizeFingerprintValue(event))).length;
}

export function validateAgentPublicEventAgainstRunV1(
  value: unknown,
  expectedValue: unknown,
): AgentPublicEventV1 {
  const event = validateAgentPublicEventV1(value);
  const expected = validateAgentRunBindingV1(expectedValue);
  assertSameAgentRunBinding(expected, event, "agent public event");
  return event;
}

export function validateAgentRunStateChangedV1(value: unknown): AgentRunStateChangedV1 {
  return validateAgentRunStateChangedPayloadV1(value) as unknown as AgentRunStateChangedV1;
}

export function validateAgentRunStateChangedAgainstAcceptedV1(
  value: unknown,
  expectedValue: unknown,
): AgentRunStateChangedV1 {
  const event = validateAgentRunStateChangedV1(value);
  const expected = validateAgentRunAcceptedV1(expectedValue);
  assertSameAgentRunBinding(expected, {
    chatId: event.run.chatId,
    taskId: event.run.taskId,
    projectId: event.run.projectId,
    worktreeId: event.run.worktreeId,
    workspaceIdentity: event.run.workspaceIdentity,
    runId: event.run.runId,
    roleId: event.run.selectedProfile.roleId,
    capabilityFingerprint: event.run.capabilityFingerprint,
  }, "agent run state event");
  return event;
}

export function validateAgentTerminationTranscriptV1(value: unknown): AgentTerminationTranscriptV1 {
  const object = exactObject(value, [
    "schemaVersion", "runId", "outcome", "processesProvenGone", "poisoned", "writerLockState",
    "chatState", "trackedProcesses", "evidenceDigest", "observedAt",
  ], "agent termination transcript");
  schema(object.schemaVersion);
  agentRunId(object.runId, "runId");
  const outcome = oneOf(object.outcome, ["cleanExit", "recoveryRequired"] as const, "outcome");
  if (typeof object.processesProvenGone !== "boolean" || typeof object.poisoned !== "boolean") fail("termination booleans are required");
  const lock = oneOf(object.writerLockState, ["none", "released", "recoveryRequired"] as const, "writerLockState");
  const chat = oneOf(object.chatState, ["succeeded", "failed", "canceled", "recoveryRequired"] as const, "chatState");
  const tracked = array(object.trackedProcesses, "trackedProcesses", 256).map(validateProcessIdentity);
  if (tracked.length === 0) fail("trackedProcesses must not be empty");
  uniqueProcessIdentities(tracked);
  digest(object.evidenceDigest, "evidenceDigest");
  timestamp(object.observedAt, "observedAt");
  if (outcome === "recoveryRequired") {
    if (object.processesProvenGone !== false || object.poisoned !== true || lock !== "recoveryRequired" || chat !== "recoveryRequired") {
      fail("uncertain termination must remain poisoned and recoveryRequired");
    }
  } else if (object.processesProvenGone !== true || tracked.some((process) => !process.observedGone)
    || object.poisoned !== false || lock === "recoveryRequired" || chat === "recoveryRequired") {
    fail("clean termination must prove every tracked process gone and release any writer lock");
  }
  return object as unknown as AgentTerminationTranscriptV1;
}

export function validateAgentChatDetailPageV1(value: unknown): AgentChatDetailPageV1 {
  const object = exactObject(value, ["chat", "afterEventSequence", "messages", "runs", "nextEventSequence", "agentRuns"], "agent chat detail page");
  const base = {
    chat: object.chat, afterEventSequence: object.afterEventSequence, messages: object.messages,
    runs: object.runs, nextEventSequence: object.nextEventSequence,
  };
  const page = validateChatDetailPageV1(base);
  const agentRuns = array(object.agentRuns, "agentRuns", G5_EVENT_PAGE_MAX_ITEMS_V1).map(validateAgentRunRecordV1);
  const fixtureRunIds = new Set(page.runs.map((run) => run.runId));
  const agentRunIds = new Set<string>();
  for (const run of agentRuns) {
    if (run.chatId !== page.chat.chatId || run.taskId !== page.chat.taskId || run.projectId !== page.chat.projectId || run.worktreeId !== page.chat.worktreeId
      || run.workspaceIdentity !== page.chat.workspaceIdentity) fail("agent run changed exact chat/workspace identity");
    if (!agentRunIds.add(run.runId) || fixtureRunIds.has(run.runId)) fail("fixture and actual run ledgers must have distinct unique run ids");
  }
  for (const run of page.runs) if (run.runId.startsWith("agent_run_")) fail("fixture run ledger cannot use the actual-agent run namespace");
  for (const message of page.messages) {
    if (message.runId === null) continue;
    const inFixture = fixtureRunIds.has(message.runId);
    const inAgent = agentRunIds.has(message.runId);
    if (inFixture === inAgent) fail("message runId must resolve to exactly one fixture or actual run ledger");
  }
  return object as unknown as AgentChatDetailPageV1;
}

export function isAgentChatDetailPageV1(value: AgentChatGetResultV1): value is AgentChatDetailPageV1 {
  return Object.prototype.hasOwnProperty.call(value, "agentRuns");
}

export function validateAgentChatGetResultV1(value: unknown): AgentChatGetResultV1 {
  if (typeof value === "object" && value !== null && Object.prototype.hasOwnProperty.call(value, "agentRuns")) {
    return validateAgentChatDetailPageV1(value);
  }
  return validateChatDetailPageV1(value);
}

export function validateAgentRunCancelRequestV1(value: unknown): AgentRunCancelRequestV1 {
  return validateAgentRunIdentityV1(value, "agent run cancel request") as AgentRunCancelRequestV1;
}

export function validateAgentRecoveryRecordV1(value: unknown): AgentRecoveryRecordV1 {
  const object = exactObject(value, [
    "schemaVersion", "recoveryId", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId",
    "source", "reason", "state", "trackedProcesses", "poisoned", "revision", "createdAt", "resolvedAt",
  ], "agent recovery record");
  schema(object.schemaVersion);
  agentRecoveryId(object.recoveryId, "recoveryId");
  validateAgentRunIdentityFieldsV1(object);
  if (object.source !== "actualAgent") fail("agent recovery source must be actualAgent");
  oneOf(object.reason, ["uncleanShutdown", "cancelUncertain", "processIdentityUncertain"] as const, "reason");
  const state = oneOf(object.state, ["unresolved", "resolvedAbandoned"] as const, "state");
  const tracked = array(object.trackedProcesses, "trackedProcesses", 256).map(validateProcessIdentity);
  if (tracked.length === 0) fail("agent recovery trackedProcesses must not be empty");
  uniqueProcessIdentities(tracked);
  if (typeof object.poisoned !== "boolean" || object.poisoned !== (state === "unresolved")) fail("only unresolved recovery remains poisoned");
  revision(object.revision, "revision");
  timestamp(object.createdAt, "createdAt");
  if (state === "unresolved") {
    if (object.resolvedAt !== null) fail("unresolved recovery cannot have resolvedAt");
  } else {
    timestamp(object.resolvedAt, "resolvedAt");
    if (tracked.some((process) => !process.observedGone)) fail("resolved recovery must prove every tracked process gone");
  }
  return object as unknown as AgentRecoveryRecordV1;
}

export function validateAgentRunCancelResultV1(value: unknown): AgentRunCancelResultV1 {
  const object = exactObject(value, ["schemaVersion", "request", "run", "termination", "recovery", "outcome"], "agent run cancel result");
  schema(object.schemaVersion);
  const request = validateAgentRunCancelRequestV1(object.request);
  const run = validateAgentRunRecordV1(object.run);
  const termination = validateAgentTerminationTranscriptV1(object.termination);
  validateAgentRunTerminationPersistenceV1({ schemaVersion: 1, run, termination });
  const recovery = object.recovery === null ? null : validateAgentRecoveryRecordV1(object.recovery);
  const outcome = oneOf(object.outcome, ["canceled", "recoveryRequired"] as const, "outcome");
  assertSameAgentRunIdentity(request, run, "cancel result run");
  if (termination.runId !== request.runId) fail("cancel termination changed run identity");
  if (outcome === "canceled") {
    if (run.state !== "canceled" || run.terminationOutcome !== "cleanExit" || termination.outcome !== "cleanExit" || recovery !== null) fail("canceled outcome requires clean terminal run without recovery");
  } else {
    if (run.state !== "recoveryRequired" || run.terminationOutcome !== "recoveryRequired" || termination.outcome !== "recoveryRequired" || recovery === null || recovery.state !== "unresolved") fail("recoveryRequired outcome must keep run and recovery poisoned");
    assertSameAgentRunIdentity(request, recovery, "cancel recovery");
    if (!sameProcessEvidenceList(recovery.trackedProcesses, termination.trackedProcesses)) fail("cancel recovery must persist the exact termination process evidence");
  }
  return object as unknown as AgentRunCancelResultV1;
}

export function validateAgentRunTerminationPersistenceV1(value: unknown): AgentRunTerminationPersistenceV1 {
  const object = exactObject(value, ["schemaVersion", "run", "termination"], "agent run termination persistence");
  schema(object.schemaVersion);
  const run = validateAgentRunRecordV1(object.run);
  const termination = validateAgentTerminationTranscriptV1(object.termination);
  if (run.runId !== termination.runId || run.terminationOutcome !== termination.outcome) fail("persisted run and termination identity/outcome must agree");
  if (["preparing", "running", "cancelling"].includes(run.state)) fail("termination persistence requires a terminal run");
  if (termination.outcome === "cleanExit") {
    const expectedLock = run.selectedProfile.permission === "workspaceWrite" ? "released" : "none";
    if (termination.writerLockState !== expectedLock) fail("clean termination writer lock state must match selected permission");
  }
  return object as unknown as AgentRunTerminationPersistenceV1;
}

export function validateAgentRecoveryInspectRequestV1(value: unknown): AgentRecoveryInspectRequestV1 {
  const object = exactObject(value, ["recoveryId", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId"], "agent recovery inspect request");
  agentRecoveryId(object.recoveryId, "recoveryId");
  validateAgentRunIdentityFieldsV1(object);
  return object as unknown as AgentRecoveryInspectRequestV1;
}

export function validateAgentRecoveryResolveRequestV1(value: unknown): AgentRecoveryResolveRequestV1 {
  const object = exactObject(value, ["recoveryId", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId", "expectedRevision", "resolution"], "agent recovery resolve request");
  agentRecoveryId(object.recoveryId, "recoveryId");
  validateAgentRunIdentityFieldsV1(object);
  revision(object.expectedRevision, "expectedRevision");
  if (object.resolution !== "abandon") fail("agent recovery resolution must be abandon");
  return object as unknown as AgentRecoveryResolveRequestV1;
}

export function validateAgentRecoveryResolveResultV1(value: unknown): AgentRecoveryResolveResultV1 {
  const object = exactObject(value, ["schemaVersion", "before", "request", "after", "outcome"], "agent recovery resolve result");
  schema(object.schemaVersion);
  const before = validateAgentRecoveryRecordV1(object.before);
  const request = validateAgentRecoveryResolveRequestV1(object.request);
  const after = object.after === null ? null : validateAgentRecoveryRecordV1(object.after);
  const outcome = oneOf(object.outcome, ["resolvedAbandoned", "processesRemain", "revisionConflict"] as const, "outcome");
  assertSameRecoveryIdentity(request, before, "recovery before");
  if (before.state !== "unresolved") fail("only unresolved recovery can be resolved");
  const allGone = before.trackedProcesses.every((process) => process.observedGone);
  const expected = request.expectedRevision !== before.revision ? "revisionConflict" : !allGone ? "processesRemain" : "resolvedAbandoned";
  if (outcome !== expected) fail("recovery resolution outcome must match revision and process evidence");
  if (outcome === "resolvedAbandoned") {
    if (after === null || after.state !== "resolvedAbandoned" || after.revision !== before.revision + 1) fail("resolved recovery must persist the next revision");
    assertSameRecoveryIdentity(before, after, "recovery after");
  } else if (after !== null) fail("failed recovery resolution cannot persist an after record");
  return object as unknown as AgentRecoveryResolveResultV1;
}

export function assertG5AContractV1(): void {
  if (!G5A_CAPABILITIES_V1.every((capability) => METHODS_V1.includes(capability))) fail("G5A capability is not reserved by protocol v1");
  if (G5A_CAPABILITIES_V1.includes("run.startAgentSmoke" as never)) fail("agent smoke must remain unadvertised");
  if (new Set(G5A_CAPABILITIES_V1).size !== G5A_CAPABILITIES_V1.length) fail("G5A capabilities must be unique");
  parseG5ReadyPayloadV1({
    protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 1, platform: "darwin",
    capabilities: [...G5_CAPABILITIES_V1], sidecarInstanceId: "g5_contract_only",
  });
}

export function parsePlannedG5AReadyPayloadV1(value: unknown): ReadyPayloadV1 {
  const object = exactObject(value, ["protocolVersion", "sidecarSchemaVersion", "appSchemaVersion", "pid", "platform", "capabilities", "sidecarInstanceId"], "G5A ready payload");
  if (object.protocolVersion !== 1 || object.sidecarSchemaVersion !== 1 || object.appSchemaVersion !== 1) fail("G5A ready versions must be 1");
  if (!Number.isSafeInteger(object.pid) || (object.pid as number) <= 0) fail("pid must be positive");
  bounded(object.platform, "platform", 64);
  id(object.sidecarInstanceId, "sidecarInstanceId");
  const capabilities = array(object.capabilities, "capabilities", G5A_CAPABILITIES_V1.length);
  if (capabilities.length !== G5A_CAPABILITIES_V1.length || !capabilities.every((item, index) => item === G5A_CAPABILITIES_V1[index])) {
    fail("G5A ready capabilities must match the exact planned method set");
  }
  return object as unknown as ReadyPayloadV1;
}

function capabilityBlocker(
  platform: string,
  installation: AgentInstallationStateV1,
  auth: AgentAuthStateV1,
  discovery: AgentDiscoveryEvidenceV1,
  executable: AgentExecutableIdentityV1 | null,
  models: readonly AgentModelCapabilityV1[],
): (typeof AGENT_CAPABILITY_BLOCK_REASONS_V1)[number] | null {
  if (platform !== "darwin") return "platformUnsupported";
  if (installation === "notInstalled") return "notInstalled";
  if (installation === "incomplete") return "installationIncomplete";
  if (executable === null) return "installationIncomplete";
  if (auth === "notAuthenticated" || auth === "unknown") return "notAuthenticated";
  if (auth === "expired") return "authExpired";
  if (discovery.source === "unavailable" || discovery.verificationLevel === "unknown" || discovery.effectiveModelEvidence === "unknown") return "capabilityUnknown";
  if (discovery.exactCliVersion === null) return "capabilityUnknown";
  if (discovery.exactCliVersion !== executable.cliVersion) return "unsupportedVersion";
  if (discovery.effectiveModelEvidence === "argvOnly") return "exactModelUnavailable";
  if (discovery.source === "machineReadableCatalog" && (discovery.verificationLevel !== "runtimeVerified" || discovery.effectiveModelEvidence !== "runtimeVerified")) return "capabilityUnknown";
  if (discovery.source === "exactVersionManifest" && (discovery.verificationLevel !== "measuredExactVersion" || discovery.effectiveModelEvidence !== "measuredExactVersion")) return "capabilityUnknown";
  return models.length === 0 ? "exactModelUnavailable" : null;
}

function validateExecutable(value: unknown): AgentExecutableIdentityV1 {
  const object = exactObject(value, ["pathDigest", "fileIdentityDigest", "cliVersion"], "executable identity");
  digest(object.pathDigest, "pathDigest");
  digest(object.fileIdentityDigest, "fileIdentityDigest");
  bounded(object.cliVersion, "cliVersion", 128);
  return object as unknown as AgentExecutableIdentityV1;
}

function validateModelCapability(value: unknown): AgentModelCapabilityV1 {
  const object = exactObject(value, ["modelSelector", "reasoningEfforts", "permissions"], "model capability");
  bounded(object.modelSelector, "modelSelector", G5_MODEL_SELECTOR_MAX_BYTES_V1);
  const efforts = array(object.reasoningEfforts, "reasoningEfforts", 32);
  for (const [index, effort] of efforts.entries()) bounded(effort, `reasoningEfforts[${index}]`, G5_REASONING_EFFORT_MAX_BYTES_V1);
  unique(efforts as string[], "reasoningEfforts");
  const permissions = array(object.permissions, "permissions", G5_PERMISSIONS_V1.length);
  if (permissions.length === 0) fail("permissions must not be empty");
  for (const permission of permissions) oneOf(permission, G5_PERMISSIONS_V1, "permission");
  unique(permissions as string[], "permissions");
  return object as unknown as AgentModelCapabilityV1;
}

function validateDiscovery(value: unknown): AgentDiscoveryEvidenceV1 {
  const object = exactObject(value, ["source", "verificationLevel", "exactCliVersion", "effectiveModelEvidence"], "discovery evidence");
  oneOf(object.source, AGENT_DISCOVERY_SOURCES_V1, "source");
  oneOf(object.verificationLevel, AGENT_VERIFICATION_LEVELS_V1, "verificationLevel");
  if (object.exactCliVersion !== null) bounded(object.exactCliVersion, "exactCliVersion", 128);
  oneOf(object.effectiveModelEvidence, AGENT_MODEL_EVIDENCE_V1, "effectiveModelEvidence");
  return object as unknown as AgentDiscoveryEvidenceV1;
}

function validateProfile(value: unknown): AgentResolvedProfileV1 {
  const object = exactObject(value, ["roleId", "runtime", "modelSelector", "reasoningEffort", "permission"], "resolved profile");
  oneOf(object.roleId, G5_ROLE_IDS_V1, "roleId");
  oneOf(object.runtime, G5_RUNTIMES_V1, "runtime");
  bounded(object.modelSelector, "modelSelector", G5_MODEL_SELECTOR_MAX_BYTES_V1);
  if (object.reasoningEffort !== null) bounded(object.reasoningEffort, "reasoningEffort", G5_REASONING_EFFORT_MAX_BYTES_V1);
  oneOf(object.permission, G5_PERMISSIONS_V1, "permission");
  return object as unknown as AgentResolvedProfileV1;
}

function validateExecutionMode(value: unknown): AgentExecutionModeV1 {
  const object = exactObject(value, ["invocation", "promptTransport", "followUp", "resume", "activeStdin"], "execution mode");
  if (object.invocation !== "oneShot" || object.promptTransport !== "serverResolvedMessages" || object.followUp !== "nextRun" || object.resume !== "disabled" || object.activeStdin !== false) {
    fail("agent v1 supports one-shot server prompts and next-run follow-up only");
  }
  return object as unknown as AgentExecutionModeV1;
}

function validateProcessIdentity(value: unknown): AgentProcessIdentityV1 {
  const object = exactObject(value, ["pid", "startIdentityDigest", "executablePathDigest", "observedGone"], "process identity");
  if (!Number.isSafeInteger(object.pid) || (object.pid as number) <= 0) fail("pid must be positive");
  digest(object.startIdentityDigest, "startIdentityDigest");
  digest(object.executablePathDigest, "executablePathDigest");
  if (typeof object.observedGone !== "boolean") fail("observedGone must be boolean");
  return object as unknown as AgentProcessIdentityV1;
}

function validateBarrier(value: unknown): AgentLaunchBarrierEvidenceV1 {
  const object = exactObject(value, [
    "launcherIdentity", "agentIdentity", "trackedProcesses", "treeIdentityDigest", "launchSpecDigest", "evidenceDigest",
    "treeTrackerAttached", "runAndLockPersistedBeforeRelease", "identityPersistedBeforeRelease", "releaseGranted", "capturedAt",
  ], "launch barrier evidence");
  const launcher = validateProcessIdentity(object.launcherIdentity);
  const agent = validateProcessIdentity(object.agentIdentity);
  const tracked = array(object.trackedProcesses, "trackedProcesses", 256).map(validateProcessIdentity);
  if (!tracked.some((item) => sameProcess(item, launcher)) || !tracked.some((item) => sameProcess(item, agent))) fail("trackedProcesses must contain launcher and agent identities");
  unique(tracked.map((item) => `${item.pid}:${item.startIdentityDigest}`), "tracked process identities");
  for (const key of ["treeIdentityDigest", "launchSpecDigest", "evidenceDigest"] as const) digest(object[key], key);
  for (const key of ["treeTrackerAttached", "runAndLockPersistedBeforeRelease", "identityPersistedBeforeRelease", "releaseGranted"] as const) {
    if (object[key] !== true) fail(`${key} must be true before release`);
  }
  timestamp(object.capturedAt, "capturedAt");
  return object as unknown as AgentLaunchBarrierEvidenceV1;
}

function validateAgentRunIdentityV1(value: unknown, name: string): AgentRunIdentityV1 {
  const object = exactObject(value, ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId"], name);
  validateAgentRunIdentityFieldsV1(object);
  return object as unknown as AgentRunIdentityV1;
}

export function validateAgentRunContextV1(value: unknown): AgentRunContextV1 {
  const object = exactObject(value, [
    "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "roleId", "capabilityFingerprint",
  ], "agent run context");
  validateAgentRunContextFieldsV1(object);
  return object as unknown as AgentRunContextV1;
}

export function validateAgentRunBindingV1(value: unknown): AgentRunBindingV1 {
  const object = exactObject(value, [
    "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId", "roleId", "capabilityFingerprint",
  ], "agent run binding");
  validateAgentRunBindingFieldsV1(object);
  return object as unknown as AgentRunBindingV1;
}

function validateAgentRunContextFieldsV1(object: Record<string, unknown>): void {
  for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) id(object[key], key);
  oneOf(object.roleId, G5_ROLE_IDS_V1, "roleId");
  fingerprintDigest(object.capabilityFingerprint, "capabilityFingerprint");
}

function validateAgentRunBindingFieldsV1(object: Record<string, unknown>): void {
  validateAgentRunContextFieldsV1(object);
  agentRunId(object.runId, "runId");
}

function validateAgentRunIdentityFieldsV1(object: Record<string, unknown>): void {
  for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) id(object[key], key);
  agentRunId(object.runId, "runId");
}

function assertSameAgentRunIdentity(expected: AgentRunIdentityV1, actual: AgentRunIdentityV1, name: string): void {
  for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId"] as const) {
    if (expected[key] !== actual[key]) fail(`${name} changed ${key}`);
  }
}

function assertSameAgentRunContext(expected: AgentRunContextV1, actual: AgentRunContextV1, name: string): void {
  for (const key of [
    "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "roleId", "capabilityFingerprint",
  ] as const) {
    if (expected[key] !== actual[key]) fail(`${name} changed ${key}`);
  }
}

function assertSameAgentRunBinding(expected: AgentRunBindingV1, actual: AgentRunBindingV1, name: string): void {
  assertSameAgentRunContext(expected, actual, name);
  if (expected.runId !== actual.runId) fail(`${name} changed runId`);
}

function assertSameRecoveryIdentity(
  expected: AgentRecoveryInspectRequestV1 | AgentRecoveryRecordV1,
  actual: AgentRecoveryRecordV1,
  name: string,
): void {
  assertSameAgentRunIdentity(expected, actual, name);
  if (expected.recoveryId !== actual.recoveryId) fail(`${name} changed recoveryId`);
}

function exactObject(value: unknown, keys: readonly string[], name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) fail(`${name} must be an object`);
  const object = value as Record<string, unknown>;
  const actual = Object.keys(object).sort();
  const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) fail(`${name} has unknown or missing fields`);
  return object;
}

function array(value: unknown, name: string, maximum: number): unknown[] {
  if (!Array.isArray(value) || value.length > maximum) fail(`${name} must be a bounded array`);
  return value;
}

function oneOf<const T extends readonly string[]>(value: unknown, values: T, name: string): T[number] {
  if (typeof value !== "string" || !values.includes(value as T[number])) fail(`invalid ${name}`);
  return value as T[number];
}

function bounded(value: unknown, name: string, maximum: number): asserts value is string {
  if (typeof value !== "string" || value.trim() === "" || new TextEncoder().encode(value).length > maximum) fail(`${name} must be nonempty and bounded`);
}

function id(value: unknown, name: string): asserts value is string { bounded(value, name, G5_ID_MAX_BYTES_V1); }
function agentRunId(value: unknown, name: string): asserts value is string {
  id(value, name);
  if (!(value as string).startsWith("agent_run_")) fail(`${name} must use the actual-agent run namespace`);
}
function agentRecoveryId(value: unknown, name: string): asserts value is string {
  id(value, name);
  if (!(value as string).startsWith("agent_recovery_")) fail(`${name} must use the actual-agent recovery namespace`);
}
function digest(value: unknown, name: string): asserts value is string {
  bounded(value, name, G5_ID_MAX_BYTES_V1);
  if (!(value as string).startsWith("sha256:")) fail(`${name} must be a redacted sha256 digest`);
}
function fingerprintDigest(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || !/^sha256:[0-9a-f]{64}$/.test(value)) fail(`${name} must be an exact lowercase SHA-256 digest`);
}
function canonicalizeFingerprintValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalizeFingerprintValue);
  if (typeof value === "object" && value !== null) {
    const output: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) output[key] = canonicalizeFingerprintValue((value as Record<string, unknown>)[key]);
    return output;
  }
  return value;
}
function schema(value: unknown): void { if (value !== G5A_SCHEMA_VERSION_V1) fail("schemaVersion must be 1"); }
function timestamp(value: unknown, name: string): void {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) fail(`${name} must be an RFC 3339 UTC second timestamp`);
}
function revision(value: unknown, name: string): asserts value is number {
  if (!Number.isSafeInteger(value) || (value as number) < 1) fail(`${name} must be a positive safe integer`);
}
function uniqueProcessIdentities(processes: readonly AgentProcessIdentityV1[]): void {
  unique(processes.map((process) => `${process.pid}:${process.startIdentityDigest}`), "tracked process identities");
}
function unique(values: readonly string[], name: string): void { if (new Set(values).size !== values.length) fail(`${name} must be unique`); }
function smallCount(value: unknown, name: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0 || (value as number) > 1) fail(`${name} must be zero or one`);
  return value as number;
}
function sameProcess(left: AgentProcessIdentityV1, right: AgentProcessIdentityV1): boolean {
  return left.pid === right.pid && left.startIdentityDigest === right.startIdentityDigest && left.executablePathDigest === right.executablePathDigest;
}
function sameProcessEvidenceList(left: readonly AgentProcessIdentityV1[], right: readonly AgentProcessIdentityV1[]): boolean {
  return left.length === right.length && left.every((process, index) => {
    const candidate = right[index];
    return candidate !== undefined && sameProcess(process, candidate) && process.observedGone === candidate.observedGone;
  });
}
function fail(message: string): never { throw new Error(message); }
