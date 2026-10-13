const listeners = new Set();
window.m5SmokeAdmissionReturned = false;
window.m5SmokePrematureInstallReads = 0;
window.m5Install = { request: async () => {
  if (!window.m5SmokeAdmissionReturned) window.m5SmokePrematureInstallReads++;
  return {state:"unsupported", operationId:null, provider:null, plan:null, error:null, cancelRequested:false};
} };
const preparationInspection = { projectId: "earlier_project", worktreeId: "worktree_failed", workspaceIdentity: "workspace_failed", lockId: "lock_worktree_failed", projectRevision: 8, lockRevision: 9 };
let preparationAttempts = 0;
window.m5Projects = { request: async (input) => {
  if (input.action === "inspectPreparation" && input.projectId === "earlier_project") return {schemaVersion: 1, action: input.action, inspection: preparationInspection};
  if (input.action === "abandonPreparation") {
    if (JSON.stringify(input.inspection) !== JSON.stringify(preparationInspection)) throw new Error("wrong preparation identity");
    if (++preparationAttempts === 1) throw new Error("synthetic response loss");
    return {schemaVersion: 1, action: input.action, inspection: preparationInspection};
  }
  if (input.action === "releaseView" && input.projectId === "smoke_project") return {schemaVersion: 1, action: "releaseView", projectId: input.projectId};
  if (input.action === "list") return input.cursor === null
    ? {schemaVersion: 1, action: "list", projects: [{projectId: "earlier_project", canonicalPath: "/work/earlier"}], nextCursor: "earlier_project"}
    : {schemaVersion: 1, action: "list", projects: [{projectId: "smoke_project", canonicalPath: "/work/smoke"}], nextCursor: null};
  if (input.action !== "reopen" || input.projectId !== "smoke_project") throw new Error("unexpected catalog selection");
  return {schemaVersion: 1, action: "reopen", project: {schemaVersion: 1, projectId: "smoke_project", selectedPath: "/work/smoke", canonicalGitRoot: "/work/smoke"}};
} };
const loginStates = {codex:{state: "idle", attemptId: null, authUrl: null, error: null},claude:{state: "idle", attemptId: null, authUrl: null, error: null}};
let codexRecoveryAttempts = 0;
let codexChecks = 0;
const diagnosedProviders = new Set();
window.m5SmokeLoginRequests = [];
window.m5SmokeLoginStatusFailure = false;
window.m5Login = { request: async (input) => {
  const provider = input.provider ?? "codex";
  window.m5SmokeLoginRequests.push({provider, action: input.action});
  if (provider === "codex" && input.action === "status" && window.m5SmokeLoginStatusFailure) throw new Error("synthetic status unavailable");
  let status = loginStates[provider];
  if (input.action === "diagnose") {
    if (status.error === "providerLoginRecoveryRequired") throw new Error("diagnosis cannot clear recovery");
    diagnosedProviders.add(provider);
    status = {...status,state:"installed",attemptId:input.attemptId,error:null};
  }
  if (input.action === "check") {
    if (!diagnosedProviders.has(provider)) throw new Error("account check before diagnosis");
    if (provider === "codex") codexChecks += 1;
    const recoveryRequired = provider === "codex" && codexChecks > 1 && codexRecoveryAttempts < 2;
    status = {...status,state:provider === "codex" ? (recoveryRequired ? "failed" : "connected") : "idle",attemptId:input.attemptId,error:recoveryRequired ? "providerLoginRecoveryRequired" : null};
  }
  if (input.action === "recover") {
    if (provider !== "codex" || status.error !== "providerLoginRecoveryRequired") throw new Error("unexpected recovery");
    codexRecoveryAttempts += 1;
    // First attempt has insufficient evidence. A later explicit attempt succeeds.
    await new Promise(resolve => setTimeout(resolve, 150));
    status = {...status,state:codexRecoveryAttempts < 2 ? "failed" : "idle",attemptId:input.attemptId,error:codexRecoveryAttempts < 2 ? "providerLoginRecoveryRequired" : null};
  }
  if (input.action === "start") {
    if (provider === "codex") throw new Error("restoration must not start OAuth");
    status = {...status, state: "waiting", attemptId: input.attemptId};
  }
  if (input.action === "cancel") status = {...status, state: "canceled"};
  if (input.action === "submitCode") {
    if (provider !== "claude" || status.state !== "waiting" || input.attemptId !== status.attemptId || input.code !== "synthetic#state") throw new Error("invalid code handoff");
    status = {...status,state:"connected"};
  }
  loginStates[provider] = status;
  return status;
} };
const workspaceListeners = new Set();
const chatListeners = new Set();
const agentPublicEventListeners = new Set();
const agentRunStateListeners = new Set();
let snapshotCreated = false;
let statusReads = 0;
const smokeChat = { schemaVersion: 1, chatId: "smoke_chat", taskId: "smoke_task", projectId: "smoke_project", worktreeId: "smoke_chat_worktree", workspaceIdentity: "smoke_chat_identity", goal: "후속 대화 확인", settingsRevision: 1, roleSnapshots: [], state: "draft", createdAt: "2026-09-14T00:00:00Z", updatedAt: "2026-09-14T00:00:00Z" };
const smokeMessages = [{ schemaVersion: 1, messageId: "smoke_initial", chatId: smokeChat.chatId, taskId: smokeChat.taskId, worktreeId: smokeChat.worktreeId, eventSequence: 1, kind: "user", roleId: null, runId: null, text: "처음 요청", createdAt: smokeChat.createdAt }];
let historyMessageIndex = 0;
let parallelPaint = null;
window.m5SmokeEnableParallelChats = () => {
  if (parallelPaint) throw new Error("parallel paint fixture already enabled");
  const { parallelChatFixture } = require("./m5-parallel-chat-fixture.cjs");
  parallelPaint = parallelChatFixture(smokeChat, message => { for (const listener of chatListeners) listener(message); });
  return parallelPaint.chats;
};
window.m5SmokeEmitParallelRound = () => {
  if (!parallelPaint) throw new Error("parallel paint fixture not enabled");
  return parallelPaint.emitRound();
};
window.m5SmokeParallelCounts = () => parallelPaint?.counts() ?? null;
window.m5SmokeAppendMessages = (count) => {
  if (!Number.isSafeInteger(count) || count < 1 || count > 5000) throw new Error("invalid synthetic message count");
  const emitted = [];
  for (let index = 0; index < count; index++) {
    const eventSequence = smokeMessages[smokeMessages.length - 1].eventSequence + 1;
    const message = { ...smokeMessages[0], messageId: `history_${++historyMessageIndex}`, eventSequence, text: `history message ${historyMessageIndex}` };
    smokeMessages.push(message);
    emitted.push(message.text);
    for (const listener of chatListeners) listener(message);
  }
  return emitted;
};

const unexpected = (method) => async () => { throw new Error(`renderer smoke fixture did not expect ${method}`); };
const unsubscribe = (listeners, listener) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

window.m5 = {
  version: 7,
  appendChatMessage: async (request) => {
    const message = { ...smokeMessages[0], messageId: request.clientMessageId, text: request.text, eventSequence: request.expectedEventSequence + 1 };
    smokeMessages.push(message);
    for (const listener of chatListeners) listener(message);
    return message;
  },
  getSystemStatus: async function () {
    if (this !== window.m5) throw new TypeError("Illegal invocation");
    // Browser fixture: leave enough time for mount effects before admission.
    // The isolated contract VM has no timer and does not mount React.
    if (!window.m5SmokeAdmissionReturned && typeof setTimeout === 'function') await new Promise(resolve => setTimeout(resolve, 100));
    window.m5SmokeAdmissionReturned = true;
    const status = {
    schemaVersion: 1,
    state: "shellReady",
    sidecar: {
      protocolVersion: 1,
      sidecarSchemaVersion: 1,
      appSchemaVersion: 1,
      pid: 42,
      platform: "darwin",
      capabilities: [
        "system.status", "app.prepareQuit", "project.open", "project.getTree", "project.createWorktree", "project.close",
        "file.read", "file.save", "file.watch", "diff.getSummary", "diff.getFile", "workspace.getTree", "workspace.close",
        "workspace.file.read", "workspace.file.save", "workspace.file.watch", "workspace.diff.getSummary", "workspace.diff.getFile",
        "workspace.prepareChat", "workspace.abandonPrepared", "project.getRoleSettings", "project.updateRoleSettings", "chat.create", "chat.list", "chat.get",
        "run.startFixture", "run.cancel", "recovery.list", "recovery.inspect", "recovery.markResolved", "storage.status",
      ],
      sidecarInstanceId: "smoke_sidecar",
    },
    storageAdmission: {
      schemaVersion: 1,
      state: "ready",
      startedAt: "2026-09-09T00:00:00Z",
      completedAt: "2026-09-09T00:00:01Z",
      error: null,
    },
    database: { health: "healthy", schemaVersion: 1 },
    recoveryRequired: (window.m5SmokeBlockers ?? ["lock:before_chat"]).length > 0,
    blockerIds: window.m5SmokeBlockers ?? ["lock:before_chat"],
    };
    if (++statusReads === 1) {
      status.storageAdmission.state = "verifying";
      status.storageAdmission.completedAt = null;
      status.database = {health: "verifying", schemaVersion: null};
      status.recoveryRequired = null;
      status.blockerIds = [];
    }
    return status;
  },
  getStorageStatus: unexpected("getStorageStatus"),
  prepareQuit: async () => ({ safeToQuit: true, blockers: [] }),
  onSidecarEvent: (listener) => unsubscribe(listeners, listener),
  openProject: async () => ({
    schemaVersion: 1,
    projectId: "smoke_project",
    selectedPath: "/work/smoke",
    canonicalGitRoot: "/work/smoke",
  }),
  getProjectTree: async ({ parentPath = "" }) => {
    if (!snapshotCreated) throw new Error("renderer requested the tree before creating its standalone snapshot");
    return {
      projectId: "smoke_project",
      parentPath,
      entries: parentPath === "src" ? ["example.ts", "sample.rs", "sample.py", "sample.json"].map(name => ({ path: `src/${name}`, name, kind: "file" })) : [{ path: "src", name: "src", kind: "directory" }],
      nextCursor: null,
    };
  },
  createProjectWorktree: async ({ projectId }) => {
    if (projectId !== "smoke_project") throw new Error("renderer supplied an unexpected project ID");
    snapshotCreated = true;
    return {
      schemaVersion: 1,
      projectId,
      canonicalGitRoot: "/work/smoke",
      worktreeId: "worktree_smoke",
      worktreePath: "/private/workspaces/worktree_smoke",
      workspaceKind: "standaloneSnapshot",
      snapshotTransport: "gitBundle",
      sourceMutationPolicy: "forbidden",
      objectFormat: "sha1",
      baseCommit: "a".repeat(40),
      baseTree: "b".repeat(40),
      sourceIdentity: "source_smoke",
      workspaceIdentity: "workspace_smoke",
      headMode: "detached",
      lifecycle: "active",
    };
  },
  closeProject: async () => ({ projectId: "smoke_project", lifecycle: "abandoned", physicallyDeleted: false }),
  readFile: async ({ path }) => {
    const text = path.endsWith(".rs") ? "fn main() {}\n" : path.endsWith(".py") ? "def main():\n    pass\n" : path.endsWith(".json") ? '{"ready":true}\n' : "export const smoke = true;\n";
    for (const listener of listeners) {
      listener({
        protocolVersion: 1,
        kind: "event",
        sidecarInstanceId: "smoke_sidecar",
        streamId: "smoke_file",
        seq: 1,
        event: "blob.chunk",
        payload: { text },
      });
    }
    return {
      projectId: "smoke_project",
      path,
      classification: "utf8",
      byteLength: Buffer.byteLength(text),
      version: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      stream: { streamId: "smoke_file", chunkBytes: 262144 },
    };
  },
  beginFileSave: async () => ({ saveId: "smoke_save", chunkBytes: 262144 }),
  appendFileSaveChunk: async ({ seq }) => ({ acceptedSeq: seq }),
  commitFileSave: async () => ({ outcome: "committed", version: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }),
  watchProjectFiles: async () => {
    if (!snapshotCreated) throw new Error("renderer watched before creating its standalone snapshot");
    return { watching: true };
  },
  getDiffSummary: async () => {
    if (!snapshotCreated) throw new Error("renderer requested changes before creating its standalone snapshot");
    return { projectId: "smoke_project", baseCommit: "a".repeat(40), entries: [{ path: "src/example.ts", kind: "modified", binary: false }] };
  },
  getDiffFile: async () => {
    const text = "@@ -1 +1,401 @@\n-old\n" + Array.from({ length: 401 }, (_, index) => `+line ${index + 1}\n`).join("");
    for (const listener of listeners) listener({ protocolVersion: 1, kind: "event", sidecarInstanceId: "smoke_sidecar", streamId: "smoke_diff", seq: 1, event: "blob.chunk", payload: { text } });
    return { projectId: "smoke_project", path: "src/example.ts", kind: "modified", oldPath: null, baseVersion: "sha256:" + "a".repeat(64), worktreeVersion: "sha256:" + "b".repeat(64), byteLength: text.length, stream: { streamId: "smoke_diff", chunkBytes: 262144 } };
  },
  onProjectEvent: () => () => {},
  getWorkspaceTree: async (input) => ({ ...input, entries: [], nextCursor: null }),
  closeWorkspace: unexpected("closeWorkspace"),
  readWorkspaceFile: unexpected("readWorkspaceFile"),
  beginWorkspaceFileSave: unexpected("beginWorkspaceFileSave"),
  appendWorkspaceFileSaveChunk: unexpected("appendWorkspaceFileSaveChunk"),
  commitWorkspaceFileSave: unexpected("commitWorkspaceFileSave"),
  watchWorkspaceFiles: async (input) => ({ ...input, watching: true }),
  getWorkspaceDiffSummary: async (input) => ({ ...input, baseCommit: "a".repeat(40), entries: [] }),
  getWorkspaceDiffFile: unexpected("getWorkspaceDiffFile"),
  onWorkspaceEvent: (listener) => unsubscribe(workspaceListeners, listener),
  prepareChatWorkspace: unexpected("prepareChatWorkspace"),
  abandonPreparedWorkspace: unexpected("abandonPreparedWorkspace"),
  getProjectRoleSettings: async ({ projectId }) => ({
    schemaVersion: 1,
    projectId,
    revision: 1,
    roles: [
      { roleId: "planning", runtime: "codex", modelSelector: "test-model", reasoningEffort: "medium", permission: "readOnly" },
      { roleId: "coding", runtime: "claude-code", modelSelector: "test-model", reasoningEffort: null, permission: "workspaceWrite" },
      { roleId: "review", runtime: "codex", modelSelector: "test-model", reasoningEffort: null, permission: "readOnly" },
    ],
    updatedAt: "2026-08-26T00:00:00Z",
  }),
  updateProjectRoleSettings: unexpected("updateProjectRoleSettings"),
  createChat: unexpected("createChat"),
  listChats: async ({ projectId }) => ({ projectId, items: parallelPaint ? parallelPaint.chats : [smokeChat], nextCursor: null }),
  getChat: async (request) => parallelPaint ? parallelPaint.detail(request) : ({ chat: smokeChat, afterEventSequence: request.afterEventSequence, messages: smokeMessages.filter(message => message.eventSequence > request.afterEventSequence), runs: [], nextEventSequence: null }),
  startFixtureRun: unexpected("startFixtureRun"),
  cancelRun: unexpected("cancelRun"),
  listRecoveries: async () => ({ items: [], nextCursor: null }),
  inspectRecovery: unexpected("inspectRecovery"),
  markRecoveryResolved: unexpected("markRecoveryResolved"),
  onChatEvent: (listener) => unsubscribe(chatListeners, listener),
  getAgentCapabilities: unexpected("getAgentCapabilities"),
  startAgentRun: unexpected("startAgentRun"),
  cancelAgentRun: unexpected("cancelAgentRun"),
  inspectAgentRecovery: unexpected("inspectAgentRecovery"),
  resolveAgentRecovery: unexpected("resolveAgentRecovery"),
  onAgentPublicEvent: (listener) => unsubscribe(agentPublicEventListeners, listener),
  onAgentRunStateChanged: (listener) => unsubscribe(agentRunStateListeners, listener),
};
