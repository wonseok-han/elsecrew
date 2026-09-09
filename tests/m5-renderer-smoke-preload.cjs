const listeners = new Set();
const workspaceListeners = new Set();
const chatListeners = new Set();
const agentPublicEventListeners = new Set();
const agentRunStateListeners = new Set();
let snapshotCreated = false;

const unexpected = (method) => async () => { throw new Error(`renderer smoke fixture did not expect ${method}`); };
const unsubscribe = (listeners, listener) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

window.m5 = {
  version: 6,
  getSystemStatus: async () => ({
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
  }),
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
    return { projectId: "smoke_project", baseCommit: "a".repeat(40), entries: [] };
  },
  getDiffFile: async () => { throw new Error("not used by renderer smoke"); },
  onProjectEvent: () => () => {},
  getWorkspaceTree: unexpected("getWorkspaceTree"),
  closeWorkspace: unexpected("closeWorkspace"),
  readWorkspaceFile: unexpected("readWorkspaceFile"),
  beginWorkspaceFileSave: unexpected("beginWorkspaceFileSave"),
  appendWorkspaceFileSaveChunk: unexpected("appendWorkspaceFileSaveChunk"),
  commitWorkspaceFileSave: unexpected("commitWorkspaceFileSave"),
  watchWorkspaceFiles: unexpected("watchWorkspaceFiles"),
  getWorkspaceDiffSummary: unexpected("getWorkspaceDiffSummary"),
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
  listChats: async ({ projectId }) => ({ projectId, items: [], nextCursor: null }),
  getChat: unexpected("getChat"),
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
