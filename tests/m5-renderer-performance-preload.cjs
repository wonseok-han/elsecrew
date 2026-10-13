const { contextBridge } = require("electron");
contextBridge.exposeInMainWorld('m5PerformanceEnvironment', { sandboxed: process.sandboxed === true });
require("./m5-renderer-smoke-preload.cjs");
// Performance samples measure the normal ready path, not the recovery fixture.
window.m5SmokeBlockers = [];
const { getFileText, patch, tree } = require("./m5-renderer-performance-fixture.cjs");
const bridge = window.m5;
const streamListeners = new Set();
let streamIndex = 0;
// Computed from the exact fixture by the pre-measurement bundle builder.
function fileVersion() { return __M5_PERF_FILE_VERSION__; }
function send(text) {
  const streamId = `performance_${++streamIndex}`;
  // UTF-8 safe chunks, including the Korean content in the large-file fixture.
  let seq = 0;
  for (let offset = 0; offset < text.length; offset += 60000) {
    const chunk = text.slice(offset, offset + 60000);
    for (const listener of streamListeners) listener({ protocolVersion: 1, kind: "event", sidecarInstanceId: "smoke_sidecar", streamId, seq: ++seq, event: "blob.chunk", payload: { text: chunk } });
  }
  return { streamId, chunkBytes: 262144 };
}
const getOriginalTree = bridge.getProjectTree;
bridge.getProjectTree = async ({ parentPath, cursor }) => {
  await getOriginalTree({ parentPath, cursor }); // Keep snapshot-before-tree guard.
  return tree(parentPath, cursor);
};
bridge.onSidecarEvent = (listener) => { streamListeners.add(listener); return () => streamListeners.delete(listener); };
bridge.readFile = async ({ path }) => { const text = getFileText(); return { projectId: "smoke_project", path, classification: "utf8", byteLength: Buffer.byteLength(text), version: fileVersion(), stream: send(text) }; };
bridge.getDiffSummary = async () => ({ projectId: "smoke_project", baseCommit: "a".repeat(40), entries: Array.from({ length: 500 }, (_, i) => ({ kind: "modified", path: `file-${i}.txt`, binary: false })) });
bridge.getDiffFile = async ({ path }) => {
  const index = Number(/^file-(\d+)\.txt$/.exec(path)?.[1]);
  if (!Number.isInteger(index) || index < 0 || index >= 500) throw new Error("invalid fixture diff");
  const text = patch(index);
  return { projectId: "smoke_project", path, kind: "modified", oldPath: null, baseVersion: fileVersion(), worktreeVersion: fileVersion(), byteLength: Buffer.byteLength(text), stream: send(text) };
};
contextBridge.exposeInMainWorld("m5", bridge);
// Preserve the same optional UI entry points used for project release and setup.
for (const name of ['m5Projects', 'm5Install', 'm5Login']) {
  contextBridge.exposeInMainWorld(name, window[name]);
}
