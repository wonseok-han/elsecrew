const listeners = new Set();
let loginStatus = {state: "idle", attemptId: null, authUrl: null, error: null};
window.m5Login = { request: async (input) => {
  if (input.action === "start") loginStatus = {...loginStatus, state: "waiting", attemptId: input.attemptId};
  if (input.action === "cancel") loginStatus = {...loginStatus, state: "canceled"};
  return loginStatus;
} };
const workspaceListeners = new Set();
const chatListeners = new Set();
const agentPublicEventListeners = new Set();
const agentRunStateListeners = new Set();
let snapshotCreated = false;
let statusReads = 0;
const smokeChat = { schemaVersion: 1, chatId: "smoke_chat", taskId: "smoke_task", projectId: "smoke_project", worktreeId: "smoke_chat_worktree", workspaceIdentity: "smoke_chat_identity", goal: "후속 대화 확인", settingsRevision: 1, roleSnapshots: [], state: "draft", createdAt: "2026-09-14T00:00:00Z", updatedAt: "2026-09-14T00:00:00Z" };
const smokeMessages = [{ schemaVersion: 1, messageId: "smoke_initial", chatId: smokeChat.chatId, taskId: smokeChat.taskId, worktreeId: smokeChat.worktreeId, eventSequence: 1, kind: "user", roleId: null, runId: null, text: "처음 요청", createdAt: smokeChat.createdAt }];

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
    recoveryRequired: false,
    blockerIds: [],
    };
    if (++statusReads === 1) {
      status.storageAdmission.state = "verifying";
      status.storageAdmission.completedAt = null;
      status.database = {health: "verifying", schemaVersion: null};
      status.recoveryRequired = null;
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
  getProjectTree: async () => {
    if (!snapshotCreated) throw new Error("renderer requested the tree before creating its standalone snapshot");
    return {
      projectId: "smoke_project",
      parentPath: "",
      entries: [{ path: "src/example.ts", name: "example.ts", kind: "file" }],
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
  readFile: async () => {
    for (const listener of listeners) {
      listener({
        protocolVersion: 1,
        kind: "event",
        sidecarInstanceId: "smoke_sidecar",
        streamId: "smoke_file",
        seq: 1,
        event: "blob.chunk",
        payload: { text: "export const smoke = true;\n" },
      });
    }
    return {
      projectId: "smoke_project",
      path: "src/example.ts",
      classification: "utf8",
      byteLength: 27,
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
    roles: [],
    updatedAt: "2026-08-26T00:00:00Z",
  }),
  updateProjectRoleSettings: unexpected("updateProjectRoleSettings"),
  createChat: unexpected("createChat"),
  listChats: async ({ projectId }) => ({ projectId, items: [smokeChat], nextCursor: null }),
  getChat: async ({ afterEventSequence }) => ({ chat: smokeChat, afterEventSequence, messages: smokeMessages.filter(message => message.eventSequence > afterEventSequence), runs: [], nextEventSequence: null }),
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
