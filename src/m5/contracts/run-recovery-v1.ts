import { G4_CAPABILITIES_V1, exactKeys, requireId, requireObject, requireSafeUnsigned } from "./project-v1.ts";
import {
  APP_SCHEMA_VERSION,
  METHODS_V1,
  PROTOCOL_VERSION,
  SIDECAR_SCHEMA_VERSION,
  parseReadyPayload,
  type EventV1,
  type ReadyPayloadV1,
} from "./protocol-v1.ts";

export const G5_SCHEMA_VERSION_V1 = 1 as const;
export const G5_ROLE_IDS_V1 = ["planning", "coding", "review"] as const;
export const G5_RUNTIMES_V1 = ["codex", "claude-code"] as const;
export const G5_PERMISSIONS_V1 = ["readOnly", "workspaceWrite"] as const;
export const G5_CHAT_STATES_V1 = [
  "draft", "preparing", "running", "waitingForUser", "reviewReady", "applying",
  "completed", "needsAttention", "recoveryRequired", "canceled", "discarded",
] as const;
export const G5_RUN_STATES_V1 = [
  "preparing", "running", "cancelling", "succeeded", "failed", "canceled", "recoveryRequired",
] as const;
export const G5_RUN_STAGES_V1 = ["planning", "implementing", "verifying"] as const;
export const G5_FIXTURE_SCENARIOS_V1 = [
  "planning-success", "coding-success", "review-success", "cancel-clean", "cancel-uncertain",
] as const;
export const G5_PAGE_MAX_ITEMS_V1 = 100 as const;
export const G5_EVENT_PAGE_MAX_ITEMS_V1 = 50 as const;
export const G5_MODEL_SELECTOR_MAX_BYTES_V1 = 256 as const;
export const G5_REASONING_EFFORT_MAX_BYTES_V1 = 64 as const;
export const G5_GOAL_MAX_BYTES_V1 = 16 * 1024;
export const G5_PUBLIC_MESSAGE_MAX_BYTES_V1 = 64 * 1024;

export const G5_CAPABILITIES_V1 = [
  ...G4_CAPABILITIES_V1,
  "project.getRoleSettings",
  "project.updateRoleSettings",
  "chat.create",
  "chat.list",
  "chat.get",
  "run.startFixture",
  "run.cancel",
  "recovery.list",
  "recovery.inspect",
  "recovery.markResolved",
] as const;

export type RoleIdV1 = (typeof G5_ROLE_IDS_V1)[number];
export type RuntimeV1 = (typeof G5_RUNTIMES_V1)[number];
export type PermissionV1 = (typeof G5_PERMISSIONS_V1)[number];
export type ChatStateV1 = (typeof G5_CHAT_STATES_V1)[number];
export type RunStateV1 = (typeof G5_RUN_STATES_V1)[number];
export type RunStageV1 = (typeof G5_RUN_STAGES_V1)[number];
export type FixtureScenarioV1 = (typeof G5_FIXTURE_SCENARIOS_V1)[number];

export interface RoleProfileV1 {
  roleId: RoleIdV1;
  runtime: RuntimeV1;
  modelSelector: string;
  reasoningEffort: string | null;
  permission: PermissionV1;
}

export interface ProjectRoleSettingsV1 {
  schemaVersion: typeof G5_SCHEMA_VERSION_V1;
  projectId: string;
  revision: number;
  roles: readonly RoleProfileV1[];
  updatedAt: string;
}
export interface ProjectRoleSettingsUpdateRequestV1 {
  projectId: string;
  expectedRevision: number;
  roles: readonly RoleProfileV1[];
}

export interface ChatRecordV1 {
  schemaVersion: typeof G5_SCHEMA_VERSION_V1;
  chatId: string;
  taskId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  goal: string;
  settingsRevision: number;
  roleSnapshots: readonly RoleProfileV1[];
  state: ChatStateV1;
  createdAt: string;
  updatedAt: string;
}

export interface TeamMessageV1 {
  schemaVersion: typeof G5_SCHEMA_VERSION_V1;
  messageId: string;
  chatId: string;
  taskId: string;
  worktreeId: string;
  eventSequence: number;
  kind: "user" | "role" | "system" | "status";
  roleId: RoleIdV1 | null;
  runId: string | null;
  text: string;
  createdAt: string;
}

export interface TerminationEvidenceV1 {
  outcome: "cleanExit" | "failed" | "canceled" | "uncertain";
  processesProvenGone: boolean;
  exitCode: number | null;
  signal: string | null;
  observedAt: string;
}

export interface RunRecordV1 {
  schemaVersion: typeof G5_SCHEMA_VERSION_V1;
  runId: string;
  chatId: string;
  taskId: string;
  worktreeId: string;
  workspaceIdentity: string;
  selectedProfile: RoleProfileV1;
  fixtureScenario: FixtureScenarioV1;
  state: RunStateV1;
  stage: RunStageV1;
  terminationEvidence: TerminationEvidenceV1 | null;
  createdAt: string;
  updatedAt: string;
}

export interface PageRequestV1 { cursor: string | null; limit: number }
export interface ChatCreateRequestV1 {
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  goal: string;
  expectedSettingsRevision: number;
}
export interface ChatListRequestV1 extends PageRequestV1 { projectId: string }
export interface ChatGetRequestV1 extends PageRequestV1 { chatId: string; afterEventSequence: number }
export interface ChatListPageV1 { projectId: string; items: readonly ChatRecordV1[]; nextCursor: string | null }
export interface ChatDetailPageV1 {
  chat: ChatRecordV1;
  afterEventSequence: number;
  messages: readonly TeamMessageV1[];
  runs: readonly RunRecordV1[];
  nextEventSequence: number | null;
}
export interface FixtureRunStartRequestV1 { chatId: string; roleId: RoleIdV1; scenario: FixtureScenarioV1 }
export interface RunCancelRequestV1 { chatId: string; runId: string }

export interface ProcessIdentityV1 {
  pid: number;
  startTime: string;
  executablePathDigest: string;
  observedGone: boolean;
}

export interface RecoveryRecordV1 {
  schemaVersion: typeof G5_SCHEMA_VERSION_V1;
  recoveryId: string;
  chatId: string;
  taskId: string;
  runId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  reason: "uncleanShutdown" | "cancelUncertain" | "processIdentityUncertain";
  state: "unresolved" | "resolvedAbandoned";
  processIdentities: readonly ProcessIdentityV1[];
  revision: number;
  createdAt: string;
  resolvedAt: string | null;
}
export interface RecoveryPageV1 { items: readonly RecoveryRecordV1[]; nextCursor: string | null }
export interface RecoveryMarkResolvedRequestV1 { recoveryId: string; expectedRevision: number; resolution: "abandon" }

export interface ChatCreationTranscriptV1 {
  request: {
    projectId: string;
    worktreeId: string;
    workspaceIdentity: string;
    goal: string;
    expectedSettingsRevision: number;
  };
  currentSettings: ProjectRoleSettingsV1;
  createdChat: ChatRecordV1 | null;
  outcome: "created" | "settingsDrift" | "identityMismatch";
}

export interface WriterStartTranscriptV1 {
  chat: ChatRecordV1;
  request: FixtureRunStartRequestV1;
  existingWriterLocks: readonly { worktreeId: string; writerRunId: string; state: "held" | "recoveryRequired" }[];
  persistedRun: RunRecordV1 | null;
  persistedLock: { worktreeId: string; writerRunId: string; state: "held" } | null;
  transactionCommitted: boolean;
  spawnAttempted: boolean;
  outcome: "started" | "writerLocked" | "readOnlyRole";
}

export interface RecoveryResolutionTranscriptV1 {
  before: RecoveryRecordV1;
  request: RecoveryMarkResolvedRequestV1;
  after: RecoveryRecordV1 | null;
  outcome: "resolvedAbandoned" | "processesRemain" | "revisionConflict";
}

export function validateRoleProfileV1(value: unknown): RoleProfileV1 {
  const object = requireObject(value, "role profile");
  exactKeys(object, ["roleId", "runtime", "modelSelector", "reasoningEffort", "permission"]);
  const roleId = oneOf(object.roleId, G5_ROLE_IDS_V1, "roleId");
  oneOf(object.runtime, G5_RUNTIMES_V1, "runtime");
  boundedText(object.modelSelector, "modelSelector", G5_MODEL_SELECTOR_MAX_BYTES_V1);
  nullableBoundedText(object.reasoningEffort, "reasoningEffort", G5_REASONING_EFFORT_MAX_BYTES_V1);
  const permission = oneOf(object.permission, G5_PERMISSIONS_V1, "permission");
  const expected = roleId === "coding" ? "workspaceWrite" : "readOnly";
  if (permission !== expected) fail(`${roleId} permission must be ${expected}`);
  return object as unknown as RoleProfileV1;
}

export function validateProjectRoleSettingsV1(value: unknown): ProjectRoleSettingsV1 {
  const object = requireObject(value, "project role settings");
  exactKeys(object, ["schemaVersion", "projectId", "revision", "roles", "updatedAt"]);
  schema(object.schemaVersion);
  requireId(object.projectId, "projectId");
  positiveSequence(object.revision, "settings revision");
  validateRoleSet(object.roles, "roles");
  timestamp(object.updatedAt, "updatedAt");
  return object as unknown as ProjectRoleSettingsV1;
}

export function validateProjectRoleSettingsUpdateRequestV1(value: unknown): ProjectRoleSettingsUpdateRequestV1 {
  const object = requireObject(value, "project role settings update");
  exactKeys(object, ["projectId", "expectedRevision", "roles"]);
  requireId(object.projectId, "projectId");
  positiveSequence(object.expectedRevision, "expectedRevision");
  validateRoleSet(object.roles, "roles");
  return object as unknown as ProjectRoleSettingsUpdateRequestV1;
}

export function validateChatCreateRequestV1(value: unknown): ChatCreateRequestV1 {
  const object = requireObject(value, "chat create request");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "goal", "expectedSettingsRevision"]);
  for (const field of ["projectId", "worktreeId", "workspaceIdentity"] as const) requireId(object[field], field);
  boundedText(object.goal, "goal", G5_GOAL_MAX_BYTES_V1);
  positiveSequence(object.expectedSettingsRevision, "expectedSettingsRevision");
  return object as unknown as ChatCreateRequestV1;
}

export function validateChatRecordV1(value: unknown): ChatRecordV1 {
  const object = requireObject(value, "chat record");
  exactKeys(object, [
    "schemaVersion", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "goal",
    "settingsRevision", "roleSnapshots", "state", "createdAt", "updatedAt",
  ]);
  schema(object.schemaVersion);
  for (const field of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) {
    requireId(object[field], field);
  }
  boundedText(object.goal, "goal", G5_GOAL_MAX_BYTES_V1);
  positiveSequence(object.settingsRevision, "settingsRevision");
  validateRoleSet(object.roleSnapshots, "roleSnapshots");
  oneOf(object.state, G5_CHAT_STATES_V1, "chat state");
  timestamp(object.createdAt, "createdAt");
  timestamp(object.updatedAt, "updatedAt");
  return object as unknown as ChatRecordV1;
}

export function validateTeamMessageV1(value: unknown): TeamMessageV1 {
  const object = requireObject(value, "team message");
  exactKeys(object, [
    "schemaVersion", "messageId", "chatId", "taskId", "worktreeId", "eventSequence",
    "kind", "roleId", "runId", "text", "createdAt",
  ]);
  schema(object.schemaVersion);
  for (const field of ["messageId", "chatId", "taskId", "worktreeId"] as const) requireId(object[field], field);
  positiveSequence(object.eventSequence, "eventSequence");
  const kind = oneOf(object.kind, ["user", "role", "system", "status"] as const, "message kind");
  boundedText(object.text, "public message text", G5_PUBLIC_MESSAGE_MAX_BYTES_V1);
  timestamp(object.createdAt, "createdAt");
  if (kind === "role") {
    oneOf(object.roleId, G5_ROLE_IDS_V1, "roleId");
    requireId(object.runId, "runId");
  } else {
    if (object.roleId !== null) fail("non-role message roleId must be null");
    if (kind === "user" && object.runId !== null) fail("user message runId must be null");
    if (object.runId !== null) requireId(object.runId, "runId");
  }
  return object as unknown as TeamMessageV1;
}

export function validateRunRecordV1(value: unknown): RunRecordV1 {
  const object = requireObject(value, "run record");
  exactKeys(object, [
    "schemaVersion", "runId", "chatId", "taskId", "worktreeId", "workspaceIdentity",
    "selectedProfile", "fixtureScenario", "state", "stage", "terminationEvidence", "createdAt", "updatedAt",
  ]);
  schema(object.schemaVersion);
  for (const field of ["runId", "chatId", "taskId", "worktreeId", "workspaceIdentity"] as const) requireId(object[field], field);
  const profile = validateRoleProfileV1(object.selectedProfile);
  oneOf(object.fixtureScenario, G5_FIXTURE_SCENARIOS_V1, "fixture scenario");
  const state = oneOf(object.state, G5_RUN_STATES_V1, "run state");
  const stage = oneOf(object.stage, G5_RUN_STAGES_V1, "run stage");
  const expectedStage: Record<RoleIdV1, RunStageV1> = { planning: "planning", coding: "implementing", review: "verifying" };
  if (stage !== expectedStage[profile.roleId]) fail("run stage does not match selected role snapshot");
  if (["preparing", "running", "cancelling"].includes(state)) {
    if (object.terminationEvidence !== null) fail("active run cannot have termination evidence");
  } else {
    validateTerminationEvidence(object.terminationEvidence, state);
  }
  timestamp(object.createdAt, "createdAt");
  timestamp(object.updatedAt, "updatedAt");
  return object as unknown as RunRecordV1;
}

export function validateRunTransitionV1(value: unknown): { before: RunRecordV1; after: RunRecordV1 } {
  const object = requireObject(value, "run transition");
  exactKeys(object, ["before", "after"]);
  const before = validateRunRecordV1(object.before);
  const after = validateRunRecordV1(object.after);
  for (const field of ["runId", "chatId", "taskId", "worktreeId", "workspaceIdentity", "fixtureScenario"] as const) {
    if (before[field] !== after[field]) fail(`run transition changed ${field}`);
  }
  if (JSON.stringify(before.selectedProfile) !== JSON.stringify(after.selectedProfile)) fail("run transition changed selectedProfile");
  const allowed: Record<RunStateV1, readonly RunStateV1[]> = {
    preparing: ["running", "cancelling", "failed", "recoveryRequired"],
    running: ["cancelling", "succeeded", "failed", "recoveryRequired"],
    cancelling: ["canceled", "recoveryRequired"],
    succeeded: [], failed: [], canceled: [], recoveryRequired: [],
  };
  if (!allowed[before.state].includes(after.state)) fail(`invalid run transition ${before.state} -> ${after.state}`);
  return { before, after };
}

export function validateChatCreationTranscriptV1(value: unknown): ChatCreationTranscriptV1 {
  const object = requireObject(value, "chat creation transcript");
  exactKeys(object, ["request", "currentSettings", "createdChat", "outcome"]);
  const request = requireObject(object.request, "chat create request");
  exactKeys(request, ["projectId", "worktreeId", "workspaceIdentity", "goal", "expectedSettingsRevision"]);
  for (const field of ["projectId", "worktreeId", "workspaceIdentity"] as const) requireId(request[field], field);
  boundedText(request.goal, "goal", G5_GOAL_MAX_BYTES_V1);
  positiveSequence(request.expectedSettingsRevision, "expectedSettingsRevision");
  const settings = validateProjectRoleSettingsV1(object.currentSettings);
  const outcome = oneOf(object.outcome, ["created", "settingsDrift", "identityMismatch"] as const, "chat create outcome");
  if (outcome === "created") {
    const chat = validateChatRecordV1(object.createdChat);
    if (request.projectId !== settings.projectId || chat.projectId !== request.projectId
      || chat.worktreeId !== request.worktreeId || chat.workspaceIdentity !== request.workspaceIdentity) {
      fail("created chat identity mismatch");
    }
    if (request.expectedSettingsRevision !== settings.revision || chat.settingsRevision !== settings.revision) fail("created chat settings revision drift");
    if (JSON.stringify(chat.roleSnapshots) !== JSON.stringify(settings.roles)) fail("created chat did not snapshot exact roles");
  } else {
    if (object.createdChat !== null) fail("failed chat creation cannot return a chat");
    if (outcome === "settingsDrift" && request.expectedSettingsRevision === settings.revision) fail("settingsDrift requires a revision mismatch");
    if (outcome === "identityMismatch" && request.projectId === settings.projectId) fail("identityMismatch requires a project mismatch");
  }
  return object as unknown as ChatCreationTranscriptV1;
}

export function validateChatDetailPageV1(value: unknown): ChatDetailPageV1 {
  const object = requireObject(value, "chat detail page");
  exactKeys(object, ["chat", "afterEventSequence", "messages", "runs", "nextEventSequence"]);
  const chat = validateChatRecordV1(object.chat);
  const after = requireSafeUnsigned(object.afterEventSequence, "afterEventSequence");
  const messages = array(object.messages, "messages", G5_EVENT_PAGE_MAX_ITEMS_V1).map(validateTeamMessageV1);
  const runs = array(object.runs, "runs", G5_EVENT_PAGE_MAX_ITEMS_V1).map(validateRunRecordV1);
  let previous = after;
  for (const message of messages) {
    sameChatIdentity(chat, message);
    if (message.eventSequence !== previous + 1) fail("durable eventSequence must be contiguous and ordered");
    previous = message.eventSequence;
  }
  for (const run of runs) sameChatIdentity(chat, run);
  if (object.nextEventSequence !== null) {
    const next = positiveSequence(object.nextEventSequence, "nextEventSequence");
    if (next !== previous) fail("nextEventSequence must equal the last returned durable event sequence");
  }
  return object as unknown as ChatDetailPageV1;
}

export function validateChatListPageV1(value: unknown): ChatListPageV1 {
  const object = requireObject(value, "chat list page");
  exactKeys(object, ["projectId", "items", "nextCursor"]);
  requireId(object.projectId, "projectId");
  const chats = array(object.items, "chat items", G5_PAGE_MAX_ITEMS_V1).map(validateChatRecordV1);
  const worktrees = new Set<string>();
  const workspaces = new Set<string>();
  for (const chat of chats) {
    if (chat.projectId !== object.projectId) fail("chat list project mismatch");
    if (!worktrees.add(chat.worktreeId) || !workspaces.add(chat.workspaceIdentity)) fail("chat/worktree/workspace binding must be 1:1");
  }
  if (object.nextCursor !== null) boundedText(object.nextCursor, "nextCursor", 512);
  return object as unknown as ChatListPageV1;
}

export function validateChatListRequestV1(value: unknown): ChatListRequestV1 {
  const object = requireObject(value, "chat list request");
  exactKeys(object, ["projectId", "cursor", "limit"]);
  requireId(object.projectId, "projectId");
  validatePageRequestV1({ cursor: object.cursor, limit: object.limit });
  return object as unknown as ChatListRequestV1;
}

export function validateChatGetRequestV1(value: unknown): ChatGetRequestV1 {
  const object = requireObject(value, "chat get request");
  exactKeys(object, ["chatId", "afterEventSequence", "cursor", "limit"]);
  requireId(object.chatId, "chatId");
  requireSafeUnsigned(object.afterEventSequence, "afterEventSequence");
  validatePageRequestV1({ cursor: object.cursor, limit: object.limit }, G5_EVENT_PAGE_MAX_ITEMS_V1);
  return object as unknown as ChatGetRequestV1;
}

export function validatePageRequestV1(value: unknown, maximum: number = G5_PAGE_MAX_ITEMS_V1): PageRequestV1 {
  const object = requireObject(value, "page request");
  exactKeys(object, ["cursor", "limit"]);
  if (object.cursor !== null) boundedText(object.cursor, "cursor", 512);
  const limit = positiveSequence(object.limit, "page limit");
  if (limit > maximum) fail(`page limit exceeds ${maximum}`);
  return object as unknown as PageRequestV1;
}

export function validateFixtureRunStartRequestV1(value: unknown): FixtureRunStartRequestV1 {
  const object = requireObject(value, "fixture run request");
  exactKeys(object, ["chatId", "roleId", "scenario"]);
  requireId(object.chatId, "chatId");
  oneOf(object.roleId, G5_ROLE_IDS_V1, "roleId");
  oneOf(object.scenario, G5_FIXTURE_SCENARIOS_V1, "fixture scenario");
  return object as unknown as FixtureRunStartRequestV1;
}

export function validateRunCancelRequestV1(value: unknown): RunCancelRequestV1 {
  const object = requireObject(value, "run cancel request");
  exactKeys(object, ["chatId", "runId"]);
  requireId(object.chatId, "chatId"); requireId(object.runId, "runId");
  return object as unknown as RunCancelRequestV1;
}

export function validateWriterStartTranscriptV1(value: unknown): WriterStartTranscriptV1 {
  const object = requireObject(value, "writer start transcript");
  exactKeys(object, ["chat", "request", "existingWriterLocks", "persistedRun", "persistedLock", "transactionCommitted", "spawnAttempted", "outcome"]);
  const chat = validateChatRecordV1(object.chat);
  const request = validateFixtureRunStartRequestV1(object.request);
  if (request.chatId !== chat.chatId) fail("run request chatId mismatch");
  const profile = chat.roleSnapshots.find((role) => role.roleId === request.roleId);
  if (profile === undefined) fail("role snapshot is missing");
  const locks = array(object.existingWriterLocks, "existingWriterLocks", G5_PAGE_MAX_ITEMS_V1).map((entry) => {
    const lock = requireObject(entry, "writer lock");
    exactKeys(lock, ["worktreeId", "writerRunId", "state"]);
    requireId(lock.worktreeId, "worktreeId"); requireId(lock.writerRunId, "writerRunId");
    oneOf(lock.state, ["held", "recoveryRequired"] as const, "writer lock state");
    return lock as unknown as { worktreeId: string; writerRunId: string; state: "held" | "recoveryRequired" };
  });
  const writerLocked = locks.some((lock) => lock.worktreeId === chat.worktreeId);
  const outcome = oneOf(object.outcome, ["started", "writerLocked", "readOnlyRole"] as const, "writer outcome");
  if (profile.permission !== "workspaceWrite") {
    if (outcome !== "readOnlyRole") fail("read-only role cannot start a writer run");
  } else if (writerLocked) {
    if (outcome !== "writerLocked") fail("second writer for a worktree must be rejected");
  } else if (outcome !== "started") fail("eligible writer must start or report a storage failure outside this transcript");
  if (outcome === "started") {
    const run = validateRunRecordV1(object.persistedRun);
    const lock = requireObject(object.persistedLock, "persisted writer lock");
    exactKeys(lock, ["worktreeId", "writerRunId", "state"]);
    if (lock.worktreeId !== chat.worktreeId || lock.writerRunId !== run.runId || lock.state !== "held") fail("persisted run/lock identity mismatch");
    sameChatIdentity(chat, run);
    if (JSON.stringify(run.selectedProfile) !== JSON.stringify(profile)) fail("run profile must resolve from chat snapshot");
    if (object.transactionCommitted !== true || object.spawnAttempted !== true) fail("run and lock must commit atomically before spawn");
  } else if (object.persistedRun !== null || object.persistedLock !== null || object.transactionCommitted !== false || object.spawnAttempted !== false) {
    fail("rejected writer start cannot persist or spawn");
  }
  return object as unknown as WriterStartTranscriptV1;
}

export function validateRecoveryRecordV1(value: unknown): RecoveryRecordV1 {
  const object = requireObject(value, "recovery record");
  exactKeys(object, [
    "schemaVersion", "recoveryId", "chatId", "taskId", "runId", "projectId", "worktreeId",
    "workspaceIdentity", "reason", "state", "processIdentities", "revision", "createdAt", "resolvedAt",
  ]);
  schema(object.schemaVersion);
  for (const field of ["recoveryId", "chatId", "taskId", "runId", "projectId", "worktreeId", "workspaceIdentity"] as const) requireId(object[field], field);
  oneOf(object.reason, ["uncleanShutdown", "cancelUncertain", "processIdentityUncertain"] as const, "recovery reason");
  const state = oneOf(object.state, ["unresolved", "resolvedAbandoned"] as const, "recovery state");
  const processes = array(object.processIdentities, "process identities", G5_PAGE_MAX_ITEMS_V1);
  if (processes.length === 0) fail("recovery requires tracked process identities");
  for (const entry of processes) validateProcessIdentity(entry);
  positiveSequence(object.revision, "recovery revision");
  timestamp(object.createdAt, "createdAt");
  if (state === "resolvedAbandoned") {
    if (processes.some((entry) => requireObject(entry, "process identity").observedGone !== true)) fail("recovery cannot resolve while a process identity remains");
    timestamp(object.resolvedAt, "resolvedAt");
  } else if (object.resolvedAt !== null) fail("unresolved recovery resolvedAt must be null");
  return object as unknown as RecoveryRecordV1;
}

export function validateRecoveryMarkResolvedRequestV1(value: unknown): RecoveryMarkResolvedRequestV1 {
  const object = requireObject(value, "recovery resolution request");
  exactKeys(object, ["recoveryId", "expectedRevision", "resolution"]);
  requireId(object.recoveryId, "recoveryId"); positiveSequence(object.expectedRevision, "expectedRevision");
  if (object.resolution !== "abandon") fail("G5 recovery resolution is abandon-only");
  return object as unknown as RecoveryMarkResolvedRequestV1;
}

export function validateRecoveryPageV1(value: unknown): RecoveryPageV1 {
  const object = requireObject(value, "recovery page");
  exactKeys(object, ["items", "nextCursor"]);
  const records = array(object.items, "recovery items", G5_PAGE_MAX_ITEMS_V1).map(validateRecoveryRecordV1);
  if (new Set(records.map((record) => record.recoveryId)).size !== records.length) fail("recovery page contains duplicate identities");
  if (object.nextCursor !== null) boundedText(object.nextCursor, "nextCursor", 512);
  return object as unknown as RecoveryPageV1;
}

export function validateRecoveryResolutionTranscriptV1(value: unknown): RecoveryResolutionTranscriptV1 {
  const object = requireObject(value, "recovery resolution transcript");
  exactKeys(object, ["before", "request", "after", "outcome"]);
  const before = validateRecoveryRecordV1(object.before);
  const request = validateRecoveryMarkResolvedRequestV1(object.request);
  const outcome = oneOf(object.outcome, ["resolvedAbandoned", "processesRemain", "revisionConflict"] as const, "recovery outcome");
  if (request.recoveryId !== before.recoveryId) fail("recovery request identity mismatch");
  const allGone = before.processIdentities.every((identity) => identity.observedGone);
  if (outcome === "resolvedAbandoned") {
    const after = validateRecoveryRecordV1(object.after);
    if (!allGone || request.expectedRevision !== before.revision || after.state !== "resolvedAbandoned" || after.revision !== before.revision + 1) fail("invalid abandon resolution");
    for (const field of ["recoveryId", "chatId", "taskId", "runId", "projectId", "worktreeId", "workspaceIdentity"] as const) {
      if (before[field] !== after[field]) fail(`recovery resolution changed ${field}`);
    }
  } else {
    if (object.after !== null) fail("failed recovery resolution cannot return an updated record");
    if (outcome === "processesRemain" && allGone) fail("processesRemain requires a live or uncertain identity");
    if (outcome === "revisionConflict" && request.expectedRevision === before.revision) fail("revisionConflict requires revision drift");
  }
  return object as unknown as RecoveryResolutionTranscriptV1;
}

export function assertG5CapabilitiesV1(): void {
  if (G5_CAPABILITIES_V1.some((capability) => !METHODS_V1.includes(capability))) fail("G5 capability is not reserved by protocol v1");
  if (G5_CAPABILITIES_V1.includes("run.startAgentSmoke" as never)) fail("agent smoke must remain unadvertised");
  if (new Set(G5_CAPABILITIES_V1).size !== G5_CAPABILITIES_V1.length) fail("G5 capabilities must be unique");
}

export function parseG5ReadyPayloadV1(value: unknown): ReadyPayloadV1 {
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
  if (parsed.protocolVersion !== PROTOCOL_VERSION
    || parsed.sidecarSchemaVersion !== SIDECAR_SCHEMA_VERSION
    || parsed.appSchemaVersion !== APP_SCHEMA_VERSION) fail("G5 ready identity versions do not match v1");
  if (parsed.capabilities.length !== G5_CAPABILITIES_V1.length
    || parsed.capabilities.some((capability, index) => capability !== G5_CAPABILITIES_V1[index])) {
    fail("G5 ready capabilities do not match the exact supported method set");
  }
  return parsed;
}

function validateRoleSet(value: unknown, name: string): readonly RoleProfileV1[] {
  const roles = array(value, name, G5_ROLE_IDS_V1.length).map(validateRoleProfileV1);
  if (roles.length !== G5_ROLE_IDS_V1.length) fail(`${name} must contain exactly three roles`);
  if (roles.some((role, index) => role.roleId !== G5_ROLE_IDS_V1[index])) fail(`${name} must use canonical planning/coding/review order`);
  return roles;
}

function validateTerminationEvidence(value: unknown, state: RunStateV1): TerminationEvidenceV1 {
  const object = requireObject(value, "termination evidence");
  exactKeys(object, ["outcome", "processesProvenGone", "exitCode", "signal", "observedAt"]);
  const outcome = oneOf(object.outcome, ["cleanExit", "failed", "canceled", "uncertain"] as const, "termination outcome");
  if (typeof object.processesProvenGone !== "boolean") fail("processesProvenGone must be boolean");
  if (object.exitCode !== null && (typeof object.exitCode !== "number" || !Number.isSafeInteger(object.exitCode))) fail("exitCode must be an integer or null");
  if (object.signal !== null) boundedText(object.signal, "signal", 64);
  timestamp(object.observedAt, "observedAt");
  const expected: Partial<Record<RunStateV1, string>> = { succeeded: "cleanExit", failed: "failed", canceled: "canceled", recoveryRequired: "uncertain" };
  if (outcome !== expected[state]) fail("termination outcome does not match run state");
  if (state === "recoveryRequired" && object.processesProvenGone !== false) fail("recoveryRequired must retain uncertain process evidence");
  if (state !== "recoveryRequired" && object.processesProvenGone !== true) fail("terminal run requires proven process disappearance");
  return object as unknown as TerminationEvidenceV1;
}

function validateProcessIdentity(value: unknown): ProcessIdentityV1 {
  const object = requireObject(value, "process identity");
  exactKeys(object, ["pid", "startTime", "executablePathDigest", "observedGone"]);
  const pid = positiveSequence(object.pid, "pid");
  if (pid > 0xffff_ffff) fail("pid exceeds u32");
  timestamp(object.startTime, "process startTime");
  boundedText(object.executablePathDigest, "executablePathDigest", 256);
  if (typeof object.observedGone !== "boolean") fail("observedGone must be boolean");
  return object as unknown as ProcessIdentityV1;
}

function sameChatIdentity(chat: ChatRecordV1, child: Pick<TeamMessageV1 | RunRecordV1, "chatId" | "taskId" | "worktreeId">): void {
  if (child.chatId !== chat.chatId || child.taskId !== chat.taskId || child.worktreeId !== chat.worktreeId) fail("cross-chat/task/worktree record");
  if ("workspaceIdentity" in child && child.workspaceIdentity !== chat.workspaceIdentity) fail("workspace identity mismatch");
}

function array(value: unknown, name: string, maximum: number): unknown[] {
  if (!Array.isArray(value) || value.length > maximum) fail(`${name} must be a bounded array`);
  return value;
}

function schema(value: unknown): void { if (value !== G5_SCHEMA_VERSION_V1) fail("schemaVersion must be 1"); }
function positiveSequence(value: unknown, name: string): number {
  const result = requireSafeUnsigned(value, name);
  if (result === 0) fail(`${name} must be positive`);
  return result;
}
function boundedText(value: unknown, name: string, maximumBytes: number): string {
  if (typeof value !== "string" || value.trim().length === 0 || new TextEncoder().encode(value).length > maximumBytes) fail(`${name} must be nonempty and bounded`);
  return value;
}
function nullableBoundedText(value: unknown, name: string, maximumBytes: number): void {
  if (value !== null) boundedText(value, name, maximumBytes);
}
function oneOf<const Values extends readonly string[]>(value: unknown, values: Values, name: string): Values[number] {
  if (typeof value !== "string" || !values.includes(value)) fail(`invalid ${name}`);
  return value as Values[number];
}
function timestamp(value: unknown, name: string): void {
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) fail(`${name} must be an RFC 3339 UTC second timestamp`);
}
function fail(message: string): never { throw new Error(message); }
