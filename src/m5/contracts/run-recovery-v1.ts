import {
  G4_CAPABILITIES_V1,
  exactKeys,
  requireObject,
  requireSafeUnsigned,
  validateWorktreeManifestV1,
  type WorktreeManifestV1,
} from "./project-v1.ts";
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
  "stopping", "completed", "needsAttention", "recoveryRequired", "canceled", "discarded",
] as const;
export const G5_RUN_START_CHAT_STATES_V1 = ["draft", "running", "waitingForUser", "needsAttention"] as const;
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
export const G5_ID_MAX_BYTES_V1 = 512 as const;
export const G5_EXECUTION_PLATFORMS_V1 = ["darwin", "win32"] as const;

export const G5_CAPABILITIES_V1 = [
  ...G4_CAPABILITIES_V1,
  "workspace.getTree",
  "workspace.close",
  "workspace.file.read",
  "workspace.file.save",
  "workspace.file.watch",
  "workspace.diff.getSummary",
  "workspace.diff.getFile",
  "workspace.prepareChat",
  "workspace.abandonPrepared",
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
  preparationId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  goal: string;
  expectedSettingsRevision: number;
}
export interface PrepareChatWorkspaceRequestV1 { projectId: string }
export interface PreparedChatWorkspaceV1 {
  schemaVersion: typeof G5_SCHEMA_VERSION_V1;
  preparationId: string;
  manifest: WorktreeManifestV1;
  bindingState: "unbound" | "bound" | "abandoned";
  boundChatId: string | null;
  createdAt: string;
  updatedAt: string;
}
export interface PreparedChatWorkspacePageV1 {
  cursor: string | null;
  items: readonly PreparedChatWorkspaceV1[];
  nextCursor: string | null;
}
export interface PrepareChatWorkspaceTranscriptV1 {
  request: PrepareChatWorkspaceRequestV1;
  existingPreparationPages: readonly PreparedChatWorkspacePageV1[];
  candidate: PreparedChatWorkspaceV1;
  persisted: PreparedChatWorkspaceV1 | null;
  transactionCommitted: boolean;
  outcome: "prepared" | "identityCollision";
}
export interface AbandonPreparedWorkspaceRequestV1 {
  preparationId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
}
export interface AbandonPreparedWorkspaceResultV1 extends AbandonPreparedWorkspaceRequestV1 {
  bindingState: "abandoned";
  physicallyDeleted: false;
}
export interface AbandonPreparedWorkspaceTranscriptV1 {
  request: AbandonPreparedWorkspaceRequestV1;
  persistedBefore: PreparedChatWorkspaceV1 | null;
  unrelatedBefore: readonly PreparedChatWorkspaceV1[];
  persistedAfter: PreparedChatWorkspaceV1 | null;
  unrelatedAfter: readonly PreparedChatWorkspaceV1[];
  result: AbandonPreparedWorkspaceResultV1 | null;
  transactionCommitted: boolean;
  outcome: "abandoned" | "unknownPreparation" | "identityMismatch" | "alreadyBound" | "alreadyAbandoned";
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
export interface ChatTransitionV1 { before: ChatRecordV1; after: ChatRecordV1 }

export interface ChatCreationTranscriptV1 {
  request: ChatCreateRequestV1;
  currentSettings: ProjectRoleSettingsV1;
  preparedBefore: PreparedChatWorkspaceV1 | null;
  existingGlobalChatBindingPages: readonly ChatBindingPageV1[];
  preparedAfter: PreparedChatWorkspaceV1 | null;
  createdChat: ChatRecordV1 | null;
  createdUserMessage: TeamMessageV1 | null;
  transactionCommitted: boolean;
  outcome: "created" | "preparationUnknown" | "settingsDrift" | "preparationNotUnbound" | "preparationMismatch" | "identityMismatch" | "bindingConflict";
}

export interface ChatBindingV1 {
  chatId: string;
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
}
export interface ChatBindingPageV1 {
  cursor: string | null;
  items: readonly ChatBindingV1[];
  nextCursor: string | null;
}
export interface WorkspaceBindingV1 {
  projectId: string;
  worktreeId: string;
  workspaceIdentity: string;
  lifecycle: "active" | "recoveryRequired" | "abandoned";
}
export interface WriterLockV1 {
  worktreeId: string;
  workspaceIdentity: string;
  writerRunId: string;
  state: "held" | "recoveryRequired";
}
export interface ProfileResolutionV1 {
  outcome: "exactAvailable" | "unavailable";
  snapshotProfile: RoleProfileV1;
  resolvedProfile: RoleProfileV1 | null;
}

export interface WriterStartTranscriptV1 {
  chat: ChatRecordV1;
  request: FixtureRunStartRequestV1;
  workspace: WorkspaceBindingV1;
  existingRuns: readonly RunRecordV1[];
  existingWriterLocks: readonly WriterLockV1[];
  unresolvedRecoveries: readonly RecoveryRecordV1[];
  persistedRun: RunRecordV1 | null;
  persistedLock: WriterLockV1 | null;
  transactionCommitted: boolean;
  spawnAttempted: boolean;
  outcome: "started" | "writerLocked" | "recoveryBlocked" | "workspaceAbandoned" | "chatStateBlocked" | "runActive";
}

export interface RunStartTranscriptV1 {
  chat: ChatRecordV1;
  request: FixtureRunStartRequestV1;
  platform: (typeof G5_EXECUTION_PLATFORMS_V1)[number];
  workspace: WorkspaceBindingV1;
  existingRuns: readonly RunRecordV1[];
  existingWriterLocks: readonly WriterLockV1[];
  unresolvedRecoveries: readonly RecoveryRecordV1[];
  profileResolution: ProfileResolutionV1;
  persistedRun: RunRecordV1 | null;
  persistedLock: WriterLockV1 | null;
  transactionCommitted: boolean;
  spawnAttempted: boolean;
  outcome: "started" | "platformUnsupported" | "recoveryBlocked" | "workspaceAbandoned" | "chatStateBlocked" | "runActive" | "profileUnavailable" | "writerLocked";
}

export interface CancelTerminationRecoveryTranscriptV1 {
  beforeChat: ChatRecordV1;
  beforeRun: RunRecordV1;
  beforeWriterLock: WriterLockV1 | null;
  afterChat: ChatRecordV1;
  afterRun: RunRecordV1;
  afterWriterLock: WriterLockV1 | null;
  recovery: RecoveryRecordV1 | null;
  outcome: "canceled" | "recoveryRequired";
}

export interface RecoveryResolutionTranscriptV1 {
  before: RecoveryRecordV1;
  request: RecoveryMarkResolvedRequestV1;
  after: RecoveryRecordV1 | null;
  workspaceBefore: WorkspaceBindingV1;
  workspaceAfter: WorkspaceBindingV1 | null;
  chatAfter: ChatRecordV1 | null;
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

export function validatePrepareChatWorkspaceRequestV1(value: unknown): PrepareChatWorkspaceRequestV1 {
  const object = requireObject(value, "prepare chat workspace request");
  exactKeys(object, ["projectId"]);
  requireId(object.projectId, "projectId");
  return object as unknown as PrepareChatWorkspaceRequestV1;
}

export function validatePreparedChatWorkspaceV1(value: unknown): PreparedChatWorkspaceV1 {
  const object = requireObject(value, "prepared chat workspace");
  exactKeys(object, [
    "schemaVersion", "preparationId", "manifest", "bindingState", "boundChatId", "createdAt", "updatedAt",
  ]);
  schema(object.schemaVersion);
  requireId(object.preparationId, "preparationId");
  const manifest = validateWorktreeManifestV1(object.manifest);
  const bindingState = oneOf(object.bindingState, ["unbound", "bound", "abandoned"] as const, "prepared binding state");
  if (bindingState === "bound") requireId(object.boundChatId, "boundChatId");
  else if (object.boundChatId !== null) fail(`${bindingState} preparation cannot have boundChatId`);
  if (bindingState === "abandoned") {
    if (manifest.lifecycle !== "abandoned") fail("abandoned preparation requires abandoned manifest");
  } else if (manifest.lifecycle !== "active") {
    fail(`${bindingState} preparation requires active manifest`);
  }
  timestamp(object.createdAt, "createdAt");
  timestamp(object.updatedAt, "updatedAt");
  if ((object.updatedAt as string) < (object.createdAt as string)) fail("prepared workspace updatedAt must be monotonic");
  return object as unknown as PreparedChatWorkspaceV1;
}

export function validatePrepareChatWorkspaceTranscriptV1(value: unknown): PrepareChatWorkspaceTranscriptV1 {
  const object = requireObject(value, "prepare chat workspace transcript");
  exactKeys(object, ["request", "existingPreparationPages", "candidate", "persisted", "transactionCommitted", "outcome"]);
  const request = validatePrepareChatWorkspaceRequestV1(object.request);
  const existing = validatePreparedWorkspacePages(object.existingPreparationPages);
  const candidate = validatePreparedChatWorkspaceV1(object.candidate);
  if (candidate.bindingState !== "unbound" || candidate.manifest.projectId !== request.projectId) {
    fail("prepare candidate must be a new unbound active snapshot for the requested project");
  }
  const collision = existing.some((prepared) => prepared.preparationId === candidate.preparationId
    || prepared.manifest.worktreeId === candidate.manifest.worktreeId
    || prepared.manifest.workspaceIdentity === candidate.manifest.workspaceIdentity);
  const expected = collision ? "identityCollision" : "prepared";
  const outcome = oneOf(object.outcome, ["prepared", "identityCollision"] as const, "prepare outcome");
  if (outcome !== expected) fail(`prepare outcome must be ${expected}`);
  if (outcome === "prepared") {
    const persisted = validatePreparedChatWorkspaceV1(object.persisted);
    if (!sameJson(persisted, candidate) || object.transactionCommitted !== true) {
      fail("successful preparation must atomically persist the exact new snapshot");
    }
  } else if (object.persisted !== null || object.transactionCommitted !== false) {
    fail("identity collision cannot persist or publish a preparation");
  }
  return object as unknown as PrepareChatWorkspaceTranscriptV1;
}

export function validateAbandonPreparedWorkspaceRequestV1(value: unknown): AbandonPreparedWorkspaceRequestV1 {
  const object = requireObject(value, "abandon prepared workspace request");
  exactKeys(object, ["preparationId", "projectId", "worktreeId", "workspaceIdentity"]);
  for (const field of ["preparationId", "projectId", "worktreeId", "workspaceIdentity"] as const) requireId(object[field], field);
  return object as unknown as AbandonPreparedWorkspaceRequestV1;
}

export function validateAbandonPreparedWorkspaceResultV1(value: unknown): AbandonPreparedWorkspaceResultV1 {
  const object = requireObject(value, "abandon prepared workspace result");
  exactKeys(object, ["preparationId", "projectId", "worktreeId", "workspaceIdentity", "bindingState", "physicallyDeleted"]);
  validateAbandonPreparedWorkspaceRequestV1({
    preparationId: object.preparationId, projectId: object.projectId,
    worktreeId: object.worktreeId, workspaceIdentity: object.workspaceIdentity,
  });
  if (object.bindingState !== "abandoned" || object.physicallyDeleted !== false) {
    fail("prepared workspace abandon must preserve evidence without physical deletion");
  }
  return object as unknown as AbandonPreparedWorkspaceResultV1;
}

export function validateAbandonPreparedWorkspaceTranscriptV1(value: unknown): AbandonPreparedWorkspaceTranscriptV1 {
  const object = requireObject(value, "abandon prepared workspace transcript");
  exactKeys(object, [
    "request", "persistedBefore", "unrelatedBefore", "persistedAfter", "unrelatedAfter",
    "result", "transactionCommitted", "outcome",
  ]);
  const request = validateAbandonPreparedWorkspaceRequestV1(object.request);
  const before = object.persistedBefore === null ? null : validatePreparedChatWorkspaceV1(object.persistedBefore);
  const unrelatedBefore = validatePreparedWorkspaceArray(object.unrelatedBefore, "unrelated preparations before");
  const unrelatedAfter = validatePreparedWorkspaceArray(object.unrelatedAfter, "unrelated preparations after");
  if (!sameJson(unrelatedBefore, unrelatedAfter)) fail("abandon prepared cannot mutate another workspace");
  if (unrelatedBefore.some((prepared) => prepared.preparationId === request.preparationId
    || prepared.manifest.worktreeId === request.worktreeId
    || prepared.manifest.workspaceIdentity === request.workspaceIdentity)) {
    fail("target preparation cannot appear in unrelated evidence");
  }
  const mismatch = before !== null && (before.preparationId !== request.preparationId
    || before.manifest.projectId !== request.projectId || before.manifest.worktreeId !== request.worktreeId
    || before.manifest.workspaceIdentity !== request.workspaceIdentity);
  const expected = before === null ? "unknownPreparation"
    : mismatch ? "identityMismatch"
      : before.bindingState === "bound" ? "alreadyBound"
        : before.bindingState === "abandoned" ? "alreadyAbandoned"
          : "abandoned";
  const outcome = oneOf(object.outcome, [
    "abandoned", "unknownPreparation", "identityMismatch", "alreadyBound", "alreadyAbandoned",
  ] as const, "abandon prepared outcome");
  if (outcome !== expected) fail(`abandon prepared outcome must be ${expected}`);
  if (outcome === "abandoned") {
    const after = validatePreparedChatWorkspaceV1(object.persistedAfter);
    const result = validateAbandonPreparedWorkspaceResultV1(object.result);
    assertSamePreparedIdentity(before!, after, "abandon transition");
    assertSamePreparedIdentity(before!, result, "abandon result");
    if (after.bindingState !== "abandoned" || !sameManifestExceptLifecycle(before!.manifest, after.manifest)
      || after.createdAt !== before!.createdAt || after.updatedAt < before!.updatedAt
      || object.transactionCommitted !== true) fail("abandon must atomically terminally mark only the unbound preparation");
  } else if (!sameJson(object.persistedAfter, object.persistedBefore)
    || object.result !== null || object.transactionCommitted !== false) {
    fail("failed or duplicate abandon must be a fail-closed no-op");
  }
  return object as unknown as AbandonPreparedWorkspaceTranscriptV1;
}

export function validateChatCreateRequestV1(value: unknown): ChatCreateRequestV1 {
  const object = requireObject(value, "chat create request");
  exactKeys(object, ["preparationId", "projectId", "worktreeId", "workspaceIdentity", "goal", "expectedSettingsRevision"]);
  for (const field of ["preparationId", "projectId", "worktreeId", "workspaceIdentity"] as const) requireId(object[field], field);
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

export function validateChatTransitionV1(value: unknown): ChatTransitionV1 {
  const object = requireObject(value, "chat transition");
  exactKeys(object, ["before", "after"]);
  const before = validateChatRecordV1(object.before);
  const after = validateChatRecordV1(object.after);
  for (const field of [
    "schemaVersion", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity",
    "goal", "settingsRevision", "createdAt",
  ] as const) {
    if (before[field] !== after[field]) fail(`chat transition changed ${field}`);
  }
  if (!sameJson(before.roleSnapshots, after.roleSnapshots)) fail("chat transition changed roleSnapshots");
  const allowed: Record<ChatStateV1, readonly ChatStateV1[]> = {
    draft: ["preparing"],
    preparing: ["running", "stopping", "needsAttention", "recoveryRequired"],
    running: ["waitingForUser", "reviewReady", "stopping", "needsAttention", "recoveryRequired"],
    waitingForUser: ["running", "stopping", "needsAttention", "recoveryRequired"],
    reviewReady: ["applying", "discarded", "needsAttention"],
    applying: ["completed", "needsAttention", "recoveryRequired"],
    stopping: ["canceled", "recoveryRequired"],
    needsAttention: ["preparing", "discarded", "recoveryRequired"],
    completed: [], canceled: [], discarded: [], recoveryRequired: [],
  };
  if (!allowed[before.state].includes(after.state)) fail(`invalid chat transition ${before.state} -> ${after.state}`);
  if (after.updatedAt < before.updatedAt) fail("chat transition updatedAt must be monotonic");
  return { before, after };
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
  exactKeys(object, [
    "request", "currentSettings", "preparedBefore", "existingGlobalChatBindingPages", "preparedAfter",
    "createdChat", "createdUserMessage", "transactionCommitted", "outcome",
  ]);
  const request = validateChatCreateRequestV1(object.request);
  const settings = validateProjectRoleSettingsV1(object.currentSettings);
  const prepared = object.preparedBefore === null ? null : validatePreparedChatWorkspaceV1(object.preparedBefore);
  const bindings = validateChatBindingPagesV1(object.existingGlobalChatBindingPages);
  const bindingConflict = bindings.some((binding) =>
    binding.worktreeId === request.worktreeId || binding.workspaceIdentity === request.workspaceIdentity);
  const preparationMismatch = prepared !== null && (prepared.preparationId !== request.preparationId
    || prepared.manifest.projectId !== request.projectId || prepared.manifest.worktreeId !== request.worktreeId
    || prepared.manifest.workspaceIdentity !== request.workspaceIdentity);
  const expectedOutcome = prepared === null ? "preparationUnknown"
    : request.expectedSettingsRevision !== settings.revision ? "settingsDrift"
      : prepared.bindingState !== "unbound" ? "preparationNotUnbound"
        : preparationMismatch ? "preparationMismatch"
          : request.projectId !== settings.projectId ? "identityMismatch"
            : bindingConflict ? "bindingConflict"
              : "created";
  const outcome = oneOf(object.outcome, [
    "created", "preparationUnknown", "settingsDrift", "preparationNotUnbound",
    "preparationMismatch", "identityMismatch", "bindingConflict",
  ] as const, "chat create outcome");
  if (outcome !== expectedOutcome) fail(`chat create outcome must be ${expectedOutcome}`);
  if (outcome === "created") {
    const chat = validateChatRecordV1(object.createdChat);
    const message = validateTeamMessageV1(object.createdUserMessage);
    const preparedAfter = validatePreparedChatWorkspaceV1(object.preparedAfter);
    if (chat.state !== "draft" || chat.projectId !== request.projectId
      || chat.worktreeId !== request.worktreeId || chat.workspaceIdentity !== request.workspaceIdentity) {
      fail("created chat identity mismatch");
    }
    if (chat.goal !== request.goal) fail("created chat goal mismatch");
    if (request.expectedSettingsRevision !== settings.revision || chat.settingsRevision !== settings.revision) fail("created chat settings revision drift");
    if (JSON.stringify(chat.roleSnapshots) !== JSON.stringify(settings.roles)) fail("created chat did not snapshot exact roles");
    if (message.chatId !== chat.chatId || message.taskId !== chat.taskId || message.worktreeId !== chat.worktreeId
      || message.kind !== "user" || message.eventSequence !== 1 || message.text !== request.goal
      || message.roleId !== null || message.runId !== null) fail("chat creation must atomically snapshot the initial user message");
    assertSamePreparedIdentity(prepared!, preparedAfter, "chat preparation bind");
    if (!sameJson(prepared!.manifest, preparedAfter.manifest) || preparedAfter.bindingState !== "bound"
      || preparedAfter.boundChatId !== chat.chatId || preparedAfter.createdAt !== prepared!.createdAt
      || preparedAfter.updatedAt < prepared!.updatedAt || object.transactionCommitted !== true) {
      fail("chat creation must atomically bind the exact prepared snapshot");
    }
  } else {
    if (object.createdChat !== null || object.createdUserMessage !== null || object.transactionCommitted !== false) {
      fail("failed chat creation cannot persist a chat or user message");
    }
    if (!sameJson(object.preparedAfter, object.preparedBefore)) {
      fail("failed chat creation must leave the exact preparation cleanup-eligible");
    }
  }
  return object as unknown as ChatCreationTranscriptV1;
}

export function validateChatBindingPagesV1(value: unknown): readonly ChatBindingV1[] {
  const pages = array(value, "global chat binding pages", G5_PAGE_MAX_ITEMS_V1);
  if (pages.length === 0) fail("global chat binding scan requires at least one complete page");
  const bindings: ChatBindingV1[] = [];
  let expectedCursor: string | null = null;
  for (const [index, entry] of pages.entries()) {
    const page = requireObject(entry, "chat binding page");
    exactKeys(page, ["cursor", "items", "nextCursor"]);
    if (page.cursor !== expectedCursor) fail("chat binding pages must form one cursor chain");
    const items = array(page.items, "chat binding items", G5_PAGE_MAX_ITEMS_V1);
    for (const item of items) {
      const binding = requireObject(item, "chat binding");
      exactKeys(binding, ["chatId", "projectId", "worktreeId", "workspaceIdentity"]);
      for (const field of ["chatId", "projectId", "worktreeId", "workspaceIdentity"] as const) requireId(binding[field], field);
      bindings.push(binding as unknown as ChatBindingV1);
    }
    if (page.nextCursor !== null) requireId(page.nextCursor, "nextCursor");
    expectedCursor = page.nextCursor as string | null;
    if (index < pages.length - 1 && expectedCursor === null) fail("chat binding scan ended before its last page");
  }
  if (expectedCursor !== null) fail("global chat binding scan must end with nextCursor null");
  const chats = new Set<string>();
  const worktrees = new Set<string>();
  const workspaces = new Set<string>();
  for (const binding of bindings) {
    if (chats.has(binding.chatId) || worktrees.has(binding.worktreeId) || workspaces.has(binding.workspaceIdentity)) {
      fail("global chat/worktree/workspace binding must be 1:1 across pages");
    }
    chats.add(binding.chatId);
    worktrees.add(binding.worktreeId);
    workspaces.add(binding.workspaceIdentity);
  }
  return bindings;
}

function validatePreparedWorkspacePages(value: unknown): PreparedChatWorkspaceV1[] {
  const pages = array(value, "prepared workspace pages", G5_PAGE_MAX_ITEMS_V1);
  if (pages.length === 0) fail("prepared workspace scan requires at least one complete page");
  const prepared: PreparedChatWorkspaceV1[] = [];
  let expectedCursor: string | null = null;
  for (const [index, entry] of pages.entries()) {
    const page = requireObject(entry, "prepared workspace page");
    exactKeys(page, ["cursor", "items", "nextCursor"]);
    if (page.cursor !== expectedCursor) fail("prepared workspace pages must form one cursor chain");
    for (const item of array(page.items, "prepared workspace items", G5_PAGE_MAX_ITEMS_V1)) {
      prepared.push(validatePreparedChatWorkspaceV1(item));
    }
    if (page.nextCursor !== null) requireId(page.nextCursor, "prepared nextCursor");
    expectedCursor = page.nextCursor as string | null;
    if (index < pages.length - 1 && expectedCursor === null) fail("prepared workspace scan ended before its last page");
  }
  if (expectedCursor !== null) fail("prepared workspace scan must end with nextCursor null");
  const preparationIds = new Set<string>();
  const worktreeIds = new Set<string>();
  const workspaceIdentities = new Set<string>();
  for (const item of prepared) {
    if (!preparationIds.add(item.preparationId) || !worktreeIds.add(item.manifest.worktreeId)
      || !workspaceIdentities.add(item.manifest.workspaceIdentity)) {
      fail("prepared workspace identities must remain globally unique");
    }
  }
  return prepared;
}

function validatePreparedWorkspaceArray(value: unknown, name: string): PreparedChatWorkspaceV1[] {
  return array(value, name, G5_PAGE_MAX_ITEMS_V1).map(validatePreparedChatWorkspaceV1);
}

function assertSamePreparedIdentity(
  expected: PreparedChatWorkspaceV1,
  actual: PreparedChatWorkspaceV1 | AbandonPreparedWorkspaceResultV1,
  name: string,
): void {
  const projectId = "manifest" in actual ? actual.manifest.projectId : actual.projectId;
  const worktreeId = "manifest" in actual ? actual.manifest.worktreeId : actual.worktreeId;
  const workspaceIdentity = "manifest" in actual ? actual.manifest.workspaceIdentity : actual.workspaceIdentity;
  if (actual.preparationId !== expected.preparationId || projectId !== expected.manifest.projectId
    || worktreeId !== expected.manifest.worktreeId || workspaceIdentity !== expected.manifest.workspaceIdentity) {
    fail(`${name} changed preparation/project/worktree/workspace identity`);
  }
}

function sameManifestExceptLifecycle(before: WorktreeManifestV1, after: WorktreeManifestV1): boolean {
  const { lifecycle: _beforeLifecycle, ...beforeIdentity } = before;
  const { lifecycle: _afterLifecycle, ...afterIdentity } = after;
  return sameJson(beforeIdentity, afterIdentity);
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
  exactKeys(object, [
    "chat", "request", "workspace", "existingRuns", "existingWriterLocks", "unresolvedRecoveries",
    "persistedRun", "persistedLock", "transactionCommitted", "spawnAttempted", "outcome",
  ]);
  const chat = validateChatRecordV1(object.chat);
  const request = validateFixtureRunStartRequestV1(object.request);
  if (request.chatId !== chat.chatId) fail("run request chatId mismatch");
  const profile = chat.roleSnapshots.find((role) => role.roleId === request.roleId);
  if (profile === undefined) fail("role snapshot is missing");
  if (profile.permission !== "workspaceWrite" || profile.roleId !== "coding") fail("writer start contract is coding-only");
  const workspace = validateWorkspaceBindingV1(object.workspace);
  if (workspace.projectId !== chat.projectId || workspace.worktreeId !== chat.worktreeId
    || workspace.workspaceIdentity !== chat.workspaceIdentity) fail("writer workspace identity mismatch");
  const runs = array(object.existingRuns, "existing runs", G5_PAGE_MAX_ITEMS_V1).map(validateRunRecordV1);
  const locks = array(object.existingWriterLocks, "existingWriterLocks", G5_PAGE_MAX_ITEMS_V1).map((entry) => {
    return validateWriterLockV1(entry);
  });
  const recoveries = array(object.unresolvedRecoveries, "unresolved recoveries", G5_PAGE_MAX_ITEMS_V1).map(validateRecoveryRecordV1);
  if (recoveries.some((recovery) => recovery.state !== "unresolved")) fail("writer start recovery evidence must be unresolved");
  const recoveryBlocked = chat.state === "recoveryRequired" || workspace.lifecycle === "recoveryRequired"
    || runs.some((run) => run.state === "recoveryRequired"
      && (run.worktreeId === chat.worktreeId || run.workspaceIdentity === chat.workspaceIdentity))
    || locks.some((lock) => lock.state === "recoveryRequired"
      && (lock.worktreeId === chat.worktreeId || lock.workspaceIdentity === chat.workspaceIdentity))
    || recoveries.some((recovery) => recovery.chatId === chat.chatId
      || recovery.worktreeId === chat.worktreeId || recovery.workspaceIdentity === chat.workspaceIdentity);
  const chatStateBlocked = !G5_RUN_START_CHAT_STATES_V1.includes(chat.state as (typeof G5_RUN_START_CHAT_STATES_V1)[number]);
  const runActive = runs.some((run) => ["preparing", "running", "cancelling"].includes(run.state)
    && run.chatId === chat.chatId && run.worktreeId === chat.worktreeId
    && run.workspaceIdentity === chat.workspaceIdentity);
  const writerLocked = locks.some((lock) => lock.state === "held"
    && (lock.worktreeId === chat.worktreeId || lock.workspaceIdentity === chat.workspaceIdentity));
  const expectedOutcome = workspace.lifecycle === "abandoned" ? "workspaceAbandoned"
    : recoveryBlocked ? "recoveryBlocked"
      : chatStateBlocked ? "chatStateBlocked"
        : runActive ? "runActive"
          : writerLocked ? "writerLocked" : "started";
  const outcome = oneOf(object.outcome, [
    "started", "writerLocked", "recoveryBlocked", "workspaceAbandoned", "chatStateBlocked", "runActive",
  ] as const, "writer outcome");
  if (outcome !== expectedOutcome) fail(`writer outcome must be ${expectedOutcome}`);
  if (outcome === "started") {
    const run = validateRunRecordV1(object.persistedRun);
    const lock = validateWriterLockV1(object.persistedLock);
    if (lock.worktreeId !== chat.worktreeId || lock.workspaceIdentity !== chat.workspaceIdentity
      || lock.writerRunId !== run.runId || lock.state !== "held") fail("persisted run/lock identity mismatch");
    sameChatIdentity(chat, run);
    if (JSON.stringify(run.selectedProfile) !== JSON.stringify(profile)) fail("run profile must resolve from chat snapshot");
    if (run.fixtureScenario !== request.scenario) fail("persisted writer run changed fixture scenario");
    if (object.transactionCommitted !== true || object.spawnAttempted !== true) fail("run and lock must commit atomically before spawn");
  } else if (object.persistedRun !== null || object.persistedLock !== null || object.transactionCommitted !== false || object.spawnAttempted !== false) {
    fail("rejected writer start cannot persist or spawn");
  }
  return object as unknown as WriterStartTranscriptV1;
}

export function validateRunStartTranscriptV1(value: unknown): RunStartTranscriptV1 {
  const object = requireObject(value, "run start transcript");
  exactKeys(object, [
    "chat", "request", "platform", "workspace", "existingRuns", "existingWriterLocks",
    "unresolvedRecoveries", "profileResolution", "persistedRun", "persistedLock",
    "transactionCommitted", "spawnAttempted", "outcome",
  ]);
  const chat = validateChatRecordV1(object.chat);
  const request = validateFixtureRunStartRequestV1(object.request);
  if (request.chatId !== chat.chatId) fail("run request chatId mismatch");
  const platform = oneOf(object.platform, G5_EXECUTION_PLATFORMS_V1, "execution platform");
  const workspace = validateWorkspaceBindingV1(object.workspace);
  if (workspace.projectId !== chat.projectId || workspace.worktreeId !== chat.worktreeId
    || workspace.workspaceIdentity !== chat.workspaceIdentity) fail("run workspace identity mismatch");
  const existingRuns = array(object.existingRuns, "existing runs", G5_PAGE_MAX_ITEMS_V1).map(validateRunRecordV1);
  const locks = array(object.existingWriterLocks, "existing writer locks", G5_PAGE_MAX_ITEMS_V1).map(validateWriterLockV1);
  const recoveries = array(object.unresolvedRecoveries, "unresolved recoveries", G5_PAGE_MAX_ITEMS_V1).map(validateRecoveryRecordV1);
  if (recoveries.some((recovery) => recovery.state !== "unresolved")) fail("run start recovery evidence must be unresolved");
  const resolution = validateProfileResolutionV1(object.profileResolution);
  const snapshotProfile = chat.roleSnapshots.find((profile) => profile.roleId === request.roleId);
  if (snapshotProfile === undefined || !sameJson(snapshotProfile, resolution.snapshotProfile)) fail("profile resolution drifted from chat snapshot");
  const recoveryBlocked = chat.state === "recoveryRequired"
    || workspace.lifecycle === "recoveryRequired"
    || existingRuns.some((run) => run.state === "recoveryRequired"
      && (run.worktreeId === chat.worktreeId || run.workspaceIdentity === chat.workspaceIdentity))
    || locks.some((lock) => lock.state === "recoveryRequired"
      && (lock.worktreeId === chat.worktreeId || lock.workspaceIdentity === chat.workspaceIdentity))
    || recoveries.some((recovery) => recovery.chatId === chat.chatId
      || recovery.worktreeId === chat.worktreeId || recovery.workspaceIdentity === chat.workspaceIdentity);
  const chatStateBlocked = !G5_RUN_START_CHAT_STATES_V1.includes(chat.state as (typeof G5_RUN_START_CHAT_STATES_V1)[number]);
  const runActive = existingRuns.some((run) => ["preparing", "running", "cancelling"].includes(run.state)
    && run.chatId === chat.chatId && run.worktreeId === chat.worktreeId
    && run.workspaceIdentity === chat.workspaceIdentity);
  const writerLocked = snapshotProfile.permission === "workspaceWrite" && locks.some((lock) =>
    lock.worktreeId === chat.worktreeId || lock.workspaceIdentity === chat.workspaceIdentity);
  const expectedOutcome = platform === "win32"
    ? "platformUnsupported"
    : workspace.lifecycle === "abandoned"
      ? "workspaceAbandoned"
      : recoveryBlocked
        ? "recoveryBlocked"
        : chatStateBlocked
          ? "chatStateBlocked"
          : runActive
            ? "runActive"
            : resolution.outcome === "unavailable"
              ? "profileUnavailable"
              : writerLocked
                ? "writerLocked"
                : "started";
  const outcome = oneOf(object.outcome, [
    "started", "platformUnsupported", "recoveryBlocked", "workspaceAbandoned", "chatStateBlocked", "runActive",
    "profileUnavailable", "writerLocked",
  ] as const, "run start outcome");
  if (outcome !== expectedOutcome) fail(`run start outcome must be ${expectedOutcome}`);
  if (outcome === "started") {
    const run = validateRunRecordV1(object.persistedRun);
    sameChatIdentity(chat, run);
    if (resolution.resolvedProfile === null || !sameJson(run.selectedProfile, resolution.resolvedProfile)
      || !sameJson(run.selectedProfile, snapshotProfile)) fail("persisted run profile must exactly match the chat snapshot");
    if (run.fixtureScenario !== request.scenario) fail("persisted run changed fixture scenario");
    if (run.state !== "preparing") fail("new run must persist in preparing state");
    if (snapshotProfile.permission === "workspaceWrite") {
      const lock = validateWriterLockV1(object.persistedLock);
      if (lock.worktreeId !== chat.worktreeId || lock.workspaceIdentity !== chat.workspaceIdentity
        || lock.writerRunId !== run.runId || lock.state !== "held") fail("writer run and lock identity mismatch");
    } else if (object.persistedLock !== null) fail("read-only run must not acquire a writer lock");
    if (object.transactionCommitted !== true || object.spawnAttempted !== true) fail("run persistence must commit before spawn");
  } else if (object.persistedRun !== null || object.persistedLock !== null
    || object.transactionCommitted !== false || object.spawnAttempted !== false) {
    fail("rejected run start cannot persist, lock, or spawn");
  }
  return object as unknown as RunStartTranscriptV1;
}

export function validateCancelTerminationRecoveryTranscriptV1(value: unknown): CancelTerminationRecoveryTranscriptV1 {
  const object = requireObject(value, "cancel termination recovery transcript");
  exactKeys(object, [
    "beforeChat", "beforeRun", "beforeWriterLock", "afterChat", "afterRun",
    "afterWriterLock", "recovery", "outcome",
  ]);
  const beforeChat = validateChatRecordV1(object.beforeChat);
  const beforeRun = validateRunRecordV1(object.beforeRun);
  const afterChat = validateChatRecordV1(object.afterChat);
  const afterRun = validateRunRecordV1(object.afterRun);
  sameChatIdentity(beforeChat, beforeRun); sameChatIdentity(afterChat, afterRun);
  validateChatTransitionV1({ before: beforeChat, after: afterChat });
  if (beforeRun.state !== "cancelling") fail("cancel transcript must begin with a cancelling run");
  for (const field of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "settingsRevision"] as const) {
    if (beforeChat[field] !== afterChat[field]) fail(`cancel transcript changed chat ${field}`);
  }
  if (!sameJson(beforeChat.roleSnapshots, afterChat.roleSnapshots) || beforeChat.goal !== afterChat.goal
    || beforeRun.runId !== afterRun.runId || beforeRun.fixtureScenario !== afterRun.fixtureScenario
    || !sameJson(beforeRun.selectedProfile, afterRun.selectedProfile)) {
    fail("cancel transcript identity changed");
  }
  const beforeLock = object.beforeWriterLock === null ? null : validateWriterLockV1(object.beforeWriterLock);
  if (beforeRun.selectedProfile.permission === "workspaceWrite") {
    if (beforeLock === null || beforeLock.worktreeId !== beforeRun.worktreeId
      || beforeLock.workspaceIdentity !== beforeRun.workspaceIdentity
      || beforeLock.writerRunId !== beforeRun.runId || beforeLock.state !== "held") {
      fail("writer cancel requires the matching held writer lock");
    }
  } else if (beforeLock !== null) fail("read-only cancel cannot hold a writer lock");
  const outcome = oneOf(object.outcome, ["canceled", "recoveryRequired"] as const, "cancel outcome");
  if (outcome === "canceled") {
    if (beforeRun.fixtureScenario !== "cancel-clean" || afterRun.state !== "canceled" || afterChat.state !== "canceled"
      || afterRun.terminationEvidence?.processesProvenGone !== true
      || object.afterWriterLock !== null || object.recovery !== null) fail("clean cancel must release lock after proven termination");
  } else {
    const recovery = validateRecoveryRecordV1(object.recovery);
    if (beforeRun.fixtureScenario !== "cancel-uncertain" || afterRun.state !== "recoveryRequired" || afterChat.state !== "recoveryRequired"
      || afterRun.terminationEvidence?.processesProvenGone !== false || recovery.state !== "unresolved") {
      fail("uncertain cancel must atomically enter recoveryRequired");
    }
    if (recovery.chatId !== afterChat.chatId || recovery.taskId !== afterChat.taskId
      || recovery.projectId !== afterChat.projectId || recovery.runId !== afterRun.runId
      || recovery.worktreeId !== afterChat.worktreeId || recovery.workspaceIdentity !== afterChat.workspaceIdentity) {
      fail("cancel recovery identity mismatch");
    }
    if (beforeLock !== null) {
      const afterLock = validateWriterLockV1(object.afterWriterLock);
      if (afterLock.worktreeId !== beforeLock.worktreeId || afterLock.workspaceIdentity !== beforeLock.workspaceIdentity
        || afterLock.writerRunId !== beforeLock.writerRunId || afterLock.state !== "recoveryRequired") {
        fail("uncertain cancel must retain a recoveryRequired writer lock");
      }
    } else if (object.afterWriterLock !== null) fail("read-only cancel cannot create a writer lock");
  }
  return object as unknown as CancelTerminationRecoveryTranscriptV1;
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
  exactKeys(object, ["before", "request", "after", "workspaceBefore", "workspaceAfter", "chatAfter", "outcome"]);
  const before = validateRecoveryRecordV1(object.before);
  const request = validateRecoveryMarkResolvedRequestV1(object.request);
  const workspaceBefore = validateWorkspaceBindingV1(object.workspaceBefore);
  if (workspaceBefore.worktreeId !== before.worktreeId || workspaceBefore.workspaceIdentity !== before.workspaceIdentity
    || workspaceBefore.projectId !== before.projectId || workspaceBefore.lifecycle !== "recoveryRequired") {
    fail("recovery workspace identity or lifecycle mismatch");
  }
  const outcome = oneOf(object.outcome, ["resolvedAbandoned", "processesRemain", "revisionConflict"] as const, "recovery outcome");
  if (request.recoveryId !== before.recoveryId) fail("recovery request identity mismatch");
  const allGone = before.processIdentities.every((identity) => identity.observedGone);
  if (outcome === "resolvedAbandoned") {
    const after = validateRecoveryRecordV1(object.after);
    if (!allGone || request.expectedRevision !== before.revision || after.state !== "resolvedAbandoned" || after.revision !== before.revision + 1) fail("invalid abandon resolution");
    for (const field of ["recoveryId", "chatId", "taskId", "runId", "projectId", "worktreeId", "workspaceIdentity"] as const) {
      if (before[field] !== after[field]) fail(`recovery resolution changed ${field}`);
    }
    const workspaceAfter = validateWorkspaceBindingV1(object.workspaceAfter);
    const chatAfter = validateChatRecordV1(object.chatAfter);
    if (workspaceAfter.projectId !== workspaceBefore.projectId || workspaceAfter.worktreeId !== workspaceBefore.worktreeId
      || workspaceAfter.workspaceIdentity !== workspaceBefore.workspaceIdentity || workspaceAfter.lifecycle !== "abandoned") {
      fail("resolved recovery must terminally abandon the workspace");
    }
    if (chatAfter.chatId !== before.chatId || chatAfter.taskId !== before.taskId
      || chatAfter.projectId !== before.projectId || chatAfter.worktreeId !== before.worktreeId
      || chatAfter.workspaceIdentity !== before.workspaceIdentity || chatAfter.state !== "discarded") {
      fail("resolved recovery must terminally discard the old chat");
    }
  } else {
    if (object.after !== null) fail("failed recovery resolution cannot return an updated record");
    if (object.workspaceAfter !== null || object.chatAfter !== null) fail("failed recovery resolution cannot mutate workspace or chat");
    if (outcome === "processesRemain" && allGone) fail("processesRemain requires a live or uncertain identity");
    if (outcome === "revisionConflict" && request.expectedRevision === before.revision) fail("revisionConflict requires revision drift");
  }
  return object as unknown as RecoveryResolutionTranscriptV1;
}

export function validateWorkspaceBindingV1(value: unknown): WorkspaceBindingV1 {
  const object = requireObject(value, "workspace binding");
  exactKeys(object, ["projectId", "worktreeId", "workspaceIdentity", "lifecycle"]);
  for (const field of ["projectId", "worktreeId", "workspaceIdentity"] as const) requireId(object[field], field);
  oneOf(object.lifecycle, ["active", "recoveryRequired", "abandoned"] as const, "workspace lifecycle");
  return object as unknown as WorkspaceBindingV1;
}

export function validateWriterLockV1(value: unknown): WriterLockV1 {
  const object = requireObject(value, "writer lock");
  exactKeys(object, ["worktreeId", "workspaceIdentity", "writerRunId", "state"]);
  for (const field of ["worktreeId", "workspaceIdentity", "writerRunId"] as const) requireId(object[field], field);
  oneOf(object.state, ["held", "recoveryRequired"] as const, "writer lock state");
  return object as unknown as WriterLockV1;
}

export function validateProfileResolutionV1(value: unknown): ProfileResolutionV1 {
  const object = requireObject(value, "profile resolution");
  exactKeys(object, ["outcome", "snapshotProfile", "resolvedProfile"]);
  const snapshot = validateRoleProfileV1(object.snapshotProfile);
  const outcome = oneOf(object.outcome, ["exactAvailable", "unavailable"] as const, "profile resolution outcome");
  if (outcome === "exactAvailable") {
    const resolved = validateRoleProfileV1(object.resolvedProfile);
    if (!sameJson(snapshot, resolved)) fail("resolved profile must exactly equal the chat snapshot");
  } else if (object.resolvedProfile !== null) fail("unavailable profile cannot silently fall back");
  return object as unknown as ProfileResolutionV1;
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

function sameJson(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function array(value: unknown, name: string, maximum: number): unknown[] {
  if (!Array.isArray(value) || value.length > maximum) fail(`${name} must be a bounded array`);
  return value;
}

function schema(value: unknown): void { if (value !== G5_SCHEMA_VERSION_V1) fail("schemaVersion must be 1"); }
function requireId(value: unknown, name: string): asserts value is string {
  if (typeof value !== "string" || value.length === 0
    || new TextEncoder().encode(value).length > G5_ID_MAX_BYTES_V1) fail(`${name} is required and must fit the UTF-8 byte limit`);
}
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
