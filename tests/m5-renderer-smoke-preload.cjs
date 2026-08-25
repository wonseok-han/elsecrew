const listeners = new Set();

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
  getProjectTree: async () => ({
    projectId: "smoke_project",
    parentPath: "",
    entries: [{ path: "src/example.ts", name: "example.ts", kind: "file" }],
    nextCursor: null,
  }),
  createProjectWorktree: async () => { throw new Error("not used by renderer smoke"); },
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
  watchProjectFiles: async () => ({ watching: true }),
  getDiffSummary: async () => ({ projectId: "smoke_project", baseCommit: "a".repeat(40), entries: [] }),
  getDiffFile: async () => { throw new Error("not used by renderer smoke"); },
  onProjectEvent: () => () => {},
};
