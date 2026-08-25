const listeners = new Set();
let snapshotCreated = false;

window.m5 = {
  version: 2,
  getSystemStatus: async () => ({ state: "unavailable" }),
  prepareQuit: async () => ({ safeToQuit: true, blockers: [] }),
  onSidecarEvent: (listener) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
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
};
