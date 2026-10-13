import assert from "node:assert/strict";
import test from "node:test";
import { en } from "../src/locales/en.ts";
import { ko } from "../src/locales/ko.ts";

test("localized blocking notices preserve compatibility and clear stale keys", async () => {
  const controller = new ProjectEditorController(new FakeBridge());
  controller.setActionBlocked(true, { key: "m5.editorNotice.preparing" });
  assert.equal(controller.snapshot().noticeKey, "m5.editorNotice.preparing");
  assert.equal(controller.snapshot().notice, ko["m5.editorNotice.preparing"]);
  assert.equal(en[controller.snapshot().noticeKey!], "Changes cannot be made or saved while the workspace is being prepared.");
  controller.setActionBlocked(false);
  assert.equal(controller.snapshot().noticeKey, undefined);
  assert.equal(controller.snapshot().notice, null);
  assert.equal(controller.snapshot().actionBlocked, false);
  controller.setActionBlocked(true, "custom caller notice");
  assert.equal(controller.snapshot().noticeKey, undefined);
  assert.equal(controller.snapshot().notice, "custom caller notice");
  controller.setActionBlocked(false);
  assert.equal(controller.snapshot().notice, "custom caller notice");
  await controller.chooseProject();
  assert.equal(controller.snapshot().noticeKey, undefined);
  assert.equal(controller.snapshot().notice, null);
  controller.dispose();
});

import {
  DEFAULT_SNAPSHOT_CREATION_TIMEOUT_MS,
  MAX_DIFF_STREAM_BYTES,
  MAX_PENDING_STREAMS,
  MAX_RENDERED_TREE_ENTRIES,
  ProjectEditorController,
  STREAM_TIMEOUT_MS,
  splitUtf8,
  utf8ByteLength,
  type ProjectBridge,
} from "../src/m5/editor-state.ts";
import { sha256FileVersionV1, type FileReadDescriptorV1, type FileVersionV1 } from "../src/m5/contracts/file-v1.ts";
import type { DiffFileDescriptorV1, DiffSummaryV1 } from "../src/m5/contracts/diff-v1.ts";
import type { EventV1 } from "../src/m5/contracts/protocol-v1.ts";
import {
  FILE_MAX_BYTES_V1,
  type ProjectIdentityV1,
  type TreePageV1,
  type WatchEventV1,
  type WorktreeManifestV1,
} from "../src/m5/contracts/project-v1.ts";

const version = (text: string): FileVersionV1 => sha256FileVersionV1(new TextEncoder().encode(text));
const baseCommit = "a".repeat(40);

test("initial suspended watcher result preserves rescan after tree and diff finish", async () => {
  const bridge: ProjectBridge = new FakeBridge();
  bridge.watchProjectFiles = async () => ({ watching: false, rescanRequired: true });
  const controller = new ProjectEditorController(bridge);
  controller.start();
  try {
    await controller.chooseProject();
    assert.equal(controller.snapshot().rescanRequired, true);
    assert.equal(controller.snapshot().noticeKey, "m5.editorNotice.rescan");
    assert.equal(controller.snapshot().error, null);
  } finally { controller.dispose(); }
});

test("leaving a project view preserves the durable snapshot and rejects dirty views", async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  controller.start();
  try {
    await controller.chooseProject();
    await controller.openFile("src/file-000.ts");
    controller.updateText("src/file-000.ts", "draft");
    assert.equal(controller.leaveProjectView(), false);
    await controller.saveActive();
    bridge.calls.length = 0;
    assert.equal(controller.leaveProjectView(), true);
    assert.equal(controller.snapshot().project, null);
    assert.equal(controller.snapshot().workspace, null);
    assert.deepEqual(bridge.calls, []);
    await controller.chooseProject();
    assert.equal(controller.snapshot().project?.id, "project_01");
  } finally { controller.dispose(); }
});

test("saved project selection uses snapshot flow without invoking the folder picker", async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  controller.start();
  try {
    const project = await bridge.openProject(); bridge.calls.length = 0;
    let requests = 0;
    const reopen = async () => { requests++; return { schemaVersion: 1 as const, action: "reopen" as const, project }; };
    await controller.reopenProject(project.projectId, reopen);
    assert.equal(controller.snapshot().project?.id, project.projectId);
    assert.equal(bridge.calls.includes("openProject"), false);
    assert.ok(bridge.calls.includes("createProjectWorktree:project_01"));
    await controller.openFile("src/file-000.ts");
    controller.updateText("src/file-000.ts", "unsaved");
    await controller.reopenProject(project.projectId, reopen);
    assert.equal(requests, 1);
    assert.equal(controller.snapshot().tabs[0].text, "unsaved");
  } finally { controller.dispose(); }
});

test("saved project mismatched identity cannot create a snapshot", async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  try {
    const project = await bridge.openProject(); bridge.calls.length = 0;
    await controller.reopenProject("other_project", async () => ({ schemaVersion: 1, action: "reopen", project }));
    assert.deepEqual(bridge.calls, []);
    assert.equal(controller.snapshot().project, null);
    assert.ok(controller.snapshot().error);
  } finally { controller.dispose(); }
});

test("action blocking before or during saved project lookup prevents switching", async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  try {
    await controller.chooseProject();
    const project = await bridge.openProject(); bridge.calls.length = 0;
    const lookup = deferred<any>(); let requests = 0;
    const request = () => { requests++; return lookup.promise; };
    const opening = controller.reopenProject(project.projectId, request);
    const previous = controller.snapshot().project;
    controller.setActionBlocked(true);
    lookup.resolve({ schemaVersion: 1, action: "reopen", project });
    await opening;
    await controller.reopenProject(project.projectId, request);
    assert.equal(requests, 1);
    assert.deepEqual(bridge.calls, []);
    assert.equal(controller.snapshot().project, previous);
  } finally { controller.dispose(); }
});

test("editing while saved project lookup is pending prevents switching", async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  controller.start();
  try {
    await controller.chooseProject();
    await controller.openFile("src/file-000.ts");
    const project = await bridge.openProject(); bridge.calls.length = 0;
    const lookup = deferred<any>();
    const opening = controller.reopenProject(project.projectId, () => lookup.promise);
    controller.updateText("src/file-000.ts", "typed during lookup");
    lookup.resolve({ schemaVersion: 1, action: "reopen", project });
    await opening;
    assert.deepEqual(bridge.calls, []);
    assert.equal(controller.snapshot().tabs[0].text, "typed during lookup");
    assert.match(controller.snapshot().notice ?? "", /작업 내용이 바뀌었어요/);
  } finally { controller.dispose(); }
});

for (const closeFails of [false, true]) test(`blocked snapshot completion is cleaned up (close failure: ${closeFails})`, async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  try {
    const project = await bridge.openProject(); bridge.calls.length = 0;
    const creation = deferred<WorktreeManifestV1>();
    bridge.createProjectWorktree = async () => creation.promise;
    bridge.closeError = closeFails ? new Error("uncertain close") : null;
    const opening = controller.reopenProject(project.projectId, async () => ({ schemaVersion: 1, action: "reopen", project }));
    await nextTurn();
    controller.setActionBlocked(true);
    controller.setActionBlocked(false);
    creation.resolve(snapshotManifest(project.projectId, project.canonicalGitRoot));
    await opening;
    assert.equal(controller.snapshot().project, null);
    assert.deepEqual(bridge.calls, ["closeProject:project_01"]);
    assert.equal(controller.snapshot().selectionBlocked, closeFails);
  } finally { controller.dispose(); }
});

test("explicit panel requests repeat but background state updates do not reopen dock panels", async () => {
  const controller = new ProjectEditorController(new FakeBridge());
  assert.equal(controller.snapshot().panelRequest, null);
  controller.selectPanel("editor");
  const first = controller.snapshot().panelRequest;
  controller.setActionBlocked(false);
  assert.equal(controller.snapshot().panelRequest, first);
  controller.selectPanel("editor");
  assert.notEqual(controller.snapshot().panelRequest, first);
  assert.deepEqual(controller.snapshot().panelRequest, { panel: "editor" });
  controller.dispose();
});

test("cancel and restart cannot accumulate unresolved search reads", async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  try {
    await controller.chooseProject();
    const pending = deferred<TreePageV1>(); let reads = 0;
    bridge.getProjectTree = async () => { reads++; return pending.promise; };
    const request = new AbortController();
    const first = controller.searchFiles("file", request.signal);
    request.abort();
    for (let attempt = 0; attempt < 5; attempt++) await assert.rejects(controller.searchFiles("file", new AbortController().signal), /previous search read/);
    assert.equal(reads, 1);
    pending.resolve({ projectId: "project_01", parentPath: "", entries: [], nextCursor: null });
    await assert.rejects(first, { name: "AbortError" });
    const next = await controller.searchFiles("file", new AbortController().signal);
    assert.equal(next.limited, false);
    assert.equal(reads, 2);
  } finally { controller.dispose(); }
});

test("file search leaves browsing state intact and rejects results after project closure", async () => {
  const bridge = new FakeBridge(); const controller = new ProjectEditorController(bridge);
  try {
    await controller.chooseProject();
    const before = controller.snapshot();
    const pending = deferred<TreePageV1>();
    bridge.getProjectTree = async () => pending.promise;
    const result = controller.searchFiles("late", new AbortController().signal);
    assert.equal(controller.snapshot(), before);
    controller.dispose();
    pending.resolve({ projectId: "project_01", parentPath: "", entries: [{ name: "late.ts", path: "late.ts", kind: "file" }], nextCursor: null });
    await assert.rejects(result, { name: "AbortError" });
  } finally { controller.dispose(); }
});

test("tree pagination replaces each bounded page and resets on folder navigation", async () => {
  class PagedBridge extends FakeBridge {
    requests: Array<{ parentPath: string; cursor: string | null }> = [];
    async getProjectTree({ parentPath = "", cursor = null }: { projectId: string; parentPath?: string; cursor?: string | null }): Promise<TreePageV1> {
      this.requests.push({ parentPath, cursor });
      return { projectId: "project_01", parentPath, entries: Array.from({ length: 500 }, (_, i) => ({ kind: "file" as const, name: `${cursor ?? "first"}-${i}`, path: `${parentPath ? `${parentPath}/` : ""}${cursor ?? "first"}-${i}` })), nextCursor: cursor ? null : "page2" };
    }
  }
  const bridge = new PagedBridge(); const controller = new ProjectEditorController(bridge);
  try {
    await controller.chooseProject(); await controller.loadNextTreePage();
    assert.equal(controller.snapshot().tree.length, 500);
    assert.equal(controller.snapshot().tree[0].name, "page2-0");
    assert.equal(controller.snapshot().treePage, 2);
    assert.equal(controller.snapshot().treeNextCursor, null);
    await controller.loadNextTreePage(); assert.equal(bridge.requests.length, 2);
    await controller.loadTree("folder");
    assert.equal(controller.snapshot().treePage, 1);
    assert.deepEqual(bridge.requests.at(-1), { parentPath: "folder", cursor: null });
  } finally { controller.dispose(); }
});

test("late tree page cannot replace a newer folder, and repeated next clicks are coalesced", async () => {
  class DelayedBridge extends FakeBridge {
    pending = deferred<TreePageV1>(); nextCalls = 0;
    async getProjectTree({ parentPath = "", cursor = null }: { projectId: string; parentPath?: string; cursor?: string | null }): Promise<TreePageV1> {
      if (cursor) { this.nextCalls++; return this.pending.promise; }
      return { projectId: "project_01", parentPath, entries: [], nextCursor: parentPath ? null : "next" };
    }
  }
  const bridge = new DelayedBridge(); const controller = new ProjectEditorController(bridge);
  try {
    await controller.chooseProject();
    const pending = controller.loadNextTreePage(); await controller.loadNextTreePage();
    assert.equal(bridge.nextCalls, 1);
    await controller.loadTree("new-folder");
    bridge.pending.resolve({ projectId: "project_01", parentPath: "", entries: [], nextCursor: "stale" }); await pending;
    assert.equal(controller.snapshot().treeParent, "new-folder");
    assert.equal(controller.snapshot().treeNextCursor, null);
    assert.equal(controller.snapshot().treeLoading, false);
  } finally { controller.dispose(); }
});

test("failed next page preserves the current list and permits retry", async () => {
  class FailingBridge extends FakeBridge {
    failNext = true;
    async getProjectTree({ cursor = null }: { projectId: string; cursor?: string | null }): Promise<TreePageV1> {
      if (cursor && this.failNext) throw new Error("page unavailable");
      return { projectId: "project_01", parentPath: "", entries: [], nextCursor: cursor ? null : "next" };
    }
  }
  const bridge = new FailingBridge(); const controller = new ProjectEditorController(bridge);
  try {
    await controller.chooseProject(); await controller.loadNextTreePage();
    assert.equal(controller.snapshot().treePage, 1); assert.equal(controller.snapshot().treeLoading, false);
    assert.equal(controller.snapshot().treeNextCursor, "next");
    bridge.failNext = false; await controller.loadNextTreePage();
    assert.equal(controller.snapshot().treePage, 2);
  } finally { controller.dispose(); }
});

function snapshotManifest(projectId: string, canonicalGitRoot: string): WorktreeManifestV1 {
  return {
    schemaVersion: 1,
    projectId,
    canonicalGitRoot,
    worktreeId: `worktree_${projectId}`,
    worktreePath: `/private/workspaces/worktree_${projectId}`,
    workspaceKind: "standaloneSnapshot",
    snapshotTransport: "gitBundle",
    sourceMutationPolicy: "forbidden",
    objectFormat: "sha1",
    baseCommit,
    baseTree: "b".repeat(40),
    sourceIdentity: `source_${projectId}`,
    workspaceIdentity: `workspace_${projectId}`,
    headMode: "detached",
    lifecycle: "active",
  };
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(error: Error): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

async function nextTurn(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

class FakeBridge implements ProjectBridge {
  readonly sidecarListeners = new Set<(event: EventV1) => void>();
  readonly projectListeners = new Set<(event: WatchEventV1) => void>();
  readonly appended: { seq: number; text: string }[] = [];
  readonly calls: string[] = [];
  openError: Error | null = null;
  createError: Error | null = null;
  closeError: Error | null = null;
  readError: Error | null = null;
  classification: "utf8" | "binary" | "tooLarge" = "utf8";
  saveConflict = false;
  treeCount = 1;
  fileText = "export const greeting = '안녕';\n";
  savedText = "";
  autoReadChunks = true;
  autoDiffChunks = true;
  pendingCommit: Deferred<
    | { outcome: "committed"; version: FileVersionV1 }
    | { outcome: "dirtyConflict"; error: { code: string; kind: "dirtyConflict"; detail: string; retryable: false; outcomeUnknown: false } }
  > | null = null;

  async openProject() {
    this.calls.push("openProject");
    if (this.openError) throw this.openError;
    return {
      schemaVersion: 1 as const,
      projectId: "project_01",
      selectedPath: "/work/example",
      canonicalGitRoot: "/work/example",
    };
  }

  async getProjectTree() {
    this.calls.push("getProjectTree");
    return {
      projectId: "project_01",
      parentPath: "",
      entries: Array.from({ length: this.treeCount }, (_, index) => ({
        path: `src/file-${String(index).padStart(3, "0")}.ts`,
        name: `file-${String(index).padStart(3, "0")}.ts`,
        kind: "file" as const,
      })),
      nextCursor: null,
    };
  }

  async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
    this.calls.push(`createProjectWorktree:${projectId}`);
    if (this.createError) throw this.createError;
    return snapshotManifest(projectId, "/work/example");
  }

  async closeProject({ projectId }: { projectId: string }) {
    this.calls.push(`closeProject:${projectId}`);
    if (this.closeError) throw this.closeError;
    return { projectId, lifecycle: "abandoned" as const, physicallyDeleted: false as const };
  }

  async readFile({ path }: { projectId: string; path: string }) {
    if (this.readError) throw this.readError;
    const current = this.classification === "utf8" ? this.fileText : "";
    const currentVersion = version(current);
    if (this.classification === "utf8" && this.autoReadChunks) this.emitChunk("stream_read", current);
    return {
      projectId: "project_01",
      path,
      classification: this.classification,
      byteLength: this.classification === "tooLarge" ? 5 * 1024 * 1024 + 1 : new TextEncoder().encode(current).byteLength,
      version: currentVersion,
      stream: this.classification === "utf8" ? { streamId: "stream_read", chunkBytes: 262144 as const } : null,
    };
  }

  async beginFileSave() {
    return { saveId: "save_01", chunkBytes: 262144 as const };
  }

  async appendFileSaveChunk(input: { saveId: string; seq: number; text: string }) {
    this.appended.push({ seq: input.seq, text: input.text });
    return { acceptedSeq: input.seq };
  }

  async commitFileSave(input: { saveId: string; expectedVersion: FileVersionV1; lastSeq: number; totalBytes: number }) {
    this.savedText = this.appended.map((chunk) => chunk.text).join("");
    if (this.pendingCommit) return this.pendingCommit.promise;
    if (this.saveConflict) {
      return {
        outcome: "dirtyConflict" as const,
        error: { code: "E_DIRTY_CONFLICT", kind: "dirtyConflict" as const, detail: "file changed elsewhere", retryable: false as const, outcomeUnknown: false as const },
      };
    }
    return { outcome: "committed" as const, version: version(this.savedText) };
  }

  async watchProjectFiles() {
    this.calls.push("watchProjectFiles");
    return { watching: true as const };
  }

  async getDiffSummary(): Promise<DiffSummaryV1> {
    this.calls.push("getDiffSummary");
    return {
      projectId: "project_01",
      baseCommit,
      entries: [
        { kind: "renamed", path: "src/new.ts", oldPath: "src/old.ts", binary: false },
        { kind: "deleted", path: "src/removed.ts", binary: false },
        { kind: "binary", path: "assets/logo.png", binary: true },
      ],
    };
  }

  async getDiffFile({ path }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
    if (path === "src/removed.ts") {
      this.emitChunk("stream_deleted", "-removed\n");
      return {
        projectId: "project_01", kind: "deleted", path, oldPath: null,
        baseVersion: version("removed"), worktreeVersion: null, byteLength: 9,
        stream: { streamId: "stream_deleted", chunkBytes: 262144 },
      };
    }
    if (path === "assets/logo.png") {
      return {
        projectId: "project_01", kind: "binary", path, oldPath: null,
        baseVersion: version("old binary"), worktreeVersion: version("new binary"), byteLength: 10, stream: null,
      };
    }
    if (this.autoDiffChunks) this.emitChunk("stream_diff", "-old\n+new\n");
    return {
      projectId: "project_01",
      kind: "renamed" as const,
      path: "src/new.ts",
      oldPath: "src/old.ts",
      baseVersion: version("old"),
      worktreeVersion: version("new"),
      byteLength: 10,
      stream: { streamId: "stream_diff", chunkBytes: 262144 as const },
    };
  }

  onSidecarEvent(listener: (event: EventV1) => void) {
    this.sidecarListeners.add(listener);
    return () => this.sidecarListeners.delete(listener);
  }

  onProjectEvent(listener: (event: WatchEventV1) => void) {
    this.projectListeners.add(listener);
    return () => this.projectListeners.delete(listener);
  }

  emitProject(event: WatchEventV1): void {
    for (const listener of this.projectListeners) listener(event);
  }

  emitChunk(streamId: string, text: string, seq = 1): void {
    const event: EventV1 = {
      protocolVersion: 1,
      kind: "event",
      sidecarInstanceId: "sidecar_01",
      streamId,
      seq,
      event: "blob.chunk",
      payload: { text },
    };
    for (const listener of this.sidecarListeners) listener(event);
  }
}

async function openText(bridge = new FakeBridge()) {
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  await controller.openFile("src/file-000.ts");
  return { bridge, controller };
}

test("overlapping file reads keep one tab and preserve the already edited draft", async () => {
  const bridge = new FakeBridge();
  bridge.autoReadChunks = false;
  const descriptor = await bridge.readFile({ projectId: "project_01", path: "src/file-000.ts" });
  const first = deferred<typeof descriptor>();
  const second = deferred<typeof descriptor>();
  let calls = 0;
  bridge.readFile = () => (++calls === 1 ? first.promise : second.promise);
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  try {
    const a = controller.openFile(descriptor.path);
    const b = controller.openFile(descriptor.path);
    bridge.emitChunk("read_first", bridge.fileText);
    first.resolve({ ...descriptor, stream: { streamId: "read_first", chunkBytes: 262144 } });
    await a;
    controller.updateText(descriptor.path, "unsaved draft");
    const edited = controller.snapshot().tabs[0];
    bridge.emitChunk("read_second", bridge.fileText);
    second.resolve({ ...descriptor, stream: { streamId: "read_second", chunkBytes: 262144 } });
    await b;
    assert.equal(controller.snapshot().tabs.length, 1);
    assert.equal(controller.snapshot().tabs[0], edited);
    assert.equal(controller.snapshot().tabs[0].text, "unsaved draft");
  } finally { controller.dispose(); }
});

test("dispose settles unfinished file and diff reads without publishing stale state", { timeout: 2000 }, async () => {
  const bridge = new FakeBridge();
  bridge.autoReadChunks = false;
  bridge.autoDiffChunks = false;
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  const reading = controller.openFile("src/file-000.ts");
  const diffing = controller.openDiff("src/file-000.ts");
  await nextTurn();
  const before = controller.snapshot();
  controller.dispose();
  await Promise.all([reading, diffing]);
  assert.equal(controller.snapshot(), before);
  controller.start();
  bridge.autoReadChunks = true;
  try {
    await controller.openFile("src/file-000.ts");
    assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);
  } finally { controller.dispose(); }
});

test("late read descriptor after disposal and restart cannot start an orphan stream", { timeout: 2000 }, async () => {
  const bridge = new FakeBridge();
  bridge.autoReadChunks = false;
  const descriptor = await bridge.readFile({ projectId: "project_01", path: "src/file-000.ts" });
  const pending = deferred<typeof descriptor>();
  bridge.readFile = () => pending.promise;
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  const reading = controller.openFile("src/file-000.ts");
  controller.dispose();
  controller.start();
  const before = controller.snapshot();
  pending.resolve(descriptor);
  try {
    await reading;
    assert.equal(controller.snapshot(), before);
  } finally { controller.dispose(); }
});

test("confirmed reload clears a conflict only after saved text is available without writing", async () => {
  const { bridge, controller } = await openText();
  try {
    controller.updateText("src/file-000.ts", "draft");
    bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });
    const draft = controller.snapshot().tabs[0]!;
    assert.equal(draft.saveState, "conflicted");
    bridge.fileText = "saved externally";
    assert.equal(await controller.reloadConfirmedDraft(draft), true);
    assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);
    assert.equal(controller.snapshot().tabs[0]?.saveState, "clean");
    assert.deepEqual(controller.snapshot().tabs[0]?.baseVersion, version(bridge.fileText));
    assert.equal(bridge.savedText, "");
    assert.equal(bridge.appended.length, 0);
  } finally { controller.dispose(); }
});

test("reload failure and a stale confirmation never discard the current draft", async () => {
  const { bridge, controller } = await openText();
  try {
    controller.updateText("src/file-000.ts", "draft");
    const draft = controller.snapshot().tabs[0]!;
    bridge.readError = new Error("unavailable");
    assert.equal(await controller.reloadConfirmedDraft(draft), false);
    assert.equal(controller.snapshot().tabs[0], draft);
    bridge.readError = null;
    controller.updateText(draft.path, "newer");
    assert.equal(await controller.reloadConfirmedDraft(draft), false);
    assert.equal(controller.snapshot().tabs[0]?.text, "newer");
    const latest = controller.snapshot().tabs[0]!;
    bridge.autoReadChunks = false;
    const reload = controller.reloadConfirmedDraft(latest);
    await nextTurn();
    controller.updateText(latest.path, "typed while reading");
    bridge.emitChunk("stream_read", bridge.fileText);
    assert.equal(await reload, false);
    assert.equal(controller.snapshot().tabs[0]?.text, "typed while reading");
  } finally { controller.dispose(); }
});

test("typing during save preserves the newer draft and advances the committed base", async () => {
  const bridge = new FakeBridge();
  const pending = deferred<{ outcome: "committed"; version: FileVersionV1 }>();
  bridge.pendingCommit = pending;
  const { controller } = await openText(bridge);
  try {
    controller.updateText("src/file-000.ts", "first");
    const saving = controller.saveActive();
    controller.updateText("src/file-000.ts", "newer 🦦");
    assert.equal(controller.snapshot().tabs[0]?.saveState, "saving");
    await controller.saveActive(); // Duplicate save cannot race the active one.
    pending.resolve({ outcome: "committed", version: version("first") });
    await saving;
    assert.equal(bridge.savedText, "first");
    assert.equal(controller.snapshot().tabs[0]?.text, "newer 🦦");
    assert.equal(controller.snapshot().tabs[0]?.saveState, "dirty");
    assert.deepEqual(controller.snapshot().tabs[0]?.baseVersion, version("first"));
    assert.equal(controller.closeTab("src/file-000.ts"), false);
    bridge.pendingCommit = null;
    bridge.appended.length = 0;
    const commit = bridge.commitFileSave.bind(bridge);
    bridge.commitFileSave = async request => {
      assert.deepEqual(request.expectedVersion, version("first"));
      return commit(request);
    };
    await controller.saveActive();
    assert.equal(bridge.savedText, "newer 🦦");
    assert.equal(controller.snapshot().tabs[0]?.saveState, "clean");
  } finally { controller.dispose(); }
});

test("save failure and conflict preserve input added while saving", async () => {
  for (const outcome of ["failure", "conflict"] as const) {
    const { bridge, controller } = await openText();
    bridge.pendingCommit = deferred();
    try {
      const base = controller.snapshot().tabs[0]!.baseVersion;
      controller.updateText("src/file-000.ts", "submitted");
      const saving = controller.saveActive();
      await nextTurn();
      controller.updateText("src/file-000.ts", "newer");
      if (outcome === "failure") bridge.pendingCommit.reject(new Error("save failed"));
      else bridge.pendingCommit.resolve({ outcome: "dirtyConflict", error: { code: "E_DIRTY_CONFLICT", kind: "dirtyConflict", detail: "changed", retryable: false, outcomeUnknown: false } });
      await saving;
      assert.equal(controller.snapshot().tabs[0]?.text, "newer");
      assert.equal(controller.snapshot().tabs[0]?.saveState, outcome === "failure" ? "dirty" : "conflicted");
      assert.deepEqual(controller.snapshot().tabs[0]?.baseVersion, base);
    } finally { controller.dispose(); }
  }
});

test("dirty 저장은 UTF-8 chunk와 새 baseVersion을 사용해 clean으로 끝난다", async () => {
  const { bridge, controller } = await openText();
  assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);
  controller.updateText("src/file-000.ts", "const emoji = '🦦';\n");
  await controller.saveActive();

  const tab = controller.snapshot().tabs[0];
  assert.equal(tab?.saveState, "clean");
  assert.equal(bridge.savedText, "const emoji = '🦦';\n");
  assert.equal(tab?.baseVersion, version(bridge.savedText));
  assert.equal(controller.closeTab("src/file-000.ts"), true);
  controller.dispose();
});

test("stale save는 초안을 보존한 conflicted 상태로 닫기·프로젝트 전환을 막는다", async () => {
  const { bridge, controller } = await openText();
  bridge.saveConflict = true;
  controller.updateText("src/file-000.ts", "내 초안\n");
  await controller.saveActive();

  const tab = controller.snapshot().tabs[0];
  assert.equal(tab?.saveState, "conflicted");
  assert.equal(tab?.text, "내 초안\n");
  assert.equal(controller.closeTab("src/file-000.ts"), false);
  assert.equal(controller.canLeaveProject(), false);
  await controller.chooseProject();
  assert.match(controller.snapshot().notice ?? "", /프로젝트를 바꿀 수 없어요/);
  controller.dispose();
});

test("clean file watcher checks the saved version before reporting an external change", async () => {
  const { bridge, controller } = await openText();
  const emit = () => bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });
  const settle = () => new Promise(resolve => setTimeout(resolve, 0));
  const notice = controller.snapshot().notice;
  emit();
  await settle();
  assert.equal(controller.snapshot().notice, notice);
  bridge.fileText = "external update\n";
  emit();
  await settle();
  assert.equal(controller.snapshot().noticeKey, "m5.editorNotice.external");
  assert.notEqual(controller.snapshot().tabs[0]?.text, bridge.fileText);
  controller.dispose();
});

test("watcher version checks preserve newer drafts and do not read through a blocked gate", async () => {
  const { bridge, controller } = await openText();
  const pending = deferred<Awaited<ReturnType<FakeBridge["readFile"]>>>();
  let reads = 0;
  bridge.readFile = async () => { reads++; return pending.promise; };
  const emit = () => bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });
  controller.setActionBlocked(true);
  emit();
  assert.equal(reads, 0);
  controller.setActionBlocked(false);
  emit();
  controller.updateText("src/file-000.ts", "new draft");
  pending.resolve({ projectId: "project_01", path: "src/file-000.ts", classification: "binary", byteLength: 0, version: version("different"), stream: null });
  await nextTurn();
  assert.equal(controller.snapshot().tabs[0]?.saveState, "conflicted");
  assert.equal(controller.snapshot().tabs[0]?.text, "new draft");
  controller.dispose();
});

test("a delayed watcher read cannot overwrite a newly blocked workspace notice", async () => {
  const { bridge, controller } = await openText();
  const pending = deferred<Awaited<ReturnType<FakeBridge["readFile"]>>>();
  bridge.readFile = async () => pending.promise;
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });
  controller.setActionBlocked(true);
  const before = controller.snapshot();
  pending.resolve({ projectId: "project_01", path: "src/file-000.ts", classification: "binary", byteLength: 0, version: version("different"), stream: null });
  await nextTurn();
  assert.deepEqual(controller.snapshot(), before);
  controller.dispose();
});

test("외부 파일 event는 dirty draft를 conflicted로 바꾸되 draft를 보존한다", async () => {
  const { bridge, controller } = await openText();
  controller.updateText("src/file-000.ts", "보존할 초안\n");
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });

  const tab = controller.snapshot().tabs[0];
  assert.equal(tab?.saveState, "conflicted");
  assert.equal(tab?.text, "보존할 초안\n");
  controller.dispose();
});

test("선택 뒤 스냅샷을 먼저 만들고 나서만 tree·변경·watch를 시작한다", async () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();

  await controller.chooseProject();

  assert.deepEqual(bridge.calls.slice(0, 5), [
    "openProject",
    "createProjectWorktree:project_01",
    "getProjectTree",
    "getDiffSummary",
    "watchProjectFiles",
  ]);
  assert.equal(controller.snapshot().project?.name, "example");
  assert.equal(controller.snapshot().project?.root, "/work/example");
  controller.dispose();
});

test("picker AbortError는 상태를 바꾸지 않는 조용한 취소이고 open 오류는 쉬운 오류와 technical detail을 남긴다", async () => {
  const cancelled = new FakeBridge();
  cancelled.openError = Object.assign(new Error("native picker cancelled"), { name: "AbortError" });
  const cancelledController = new ProjectEditorController(cancelled);
  cancelledController.start();
  await cancelledController.chooseProject();
  assert.equal(cancelledController.snapshot().project, null);
  assert.equal(cancelledController.snapshot().notice, null);
  assert.equal(cancelledController.snapshot().error, null);

  const failed = new FakeBridge();
  failed.openError = new Error("E_NOT_GIT /tmp/not-a-project");
  const failedController = new ProjectEditorController(failed);
  failedController.start();
  await failedController.chooseProject();
  assert.match(failedController.snapshot().error?.message ?? "", /열지 못했어요/);
  assert.match(failedController.snapshot().error?.technicalDetail ?? "", /E_NOT_GIT/);
  const storedError = failedController.snapshot().error!;
  assert.equal(storedError.messageKey, "m5.editorError.openProject");
  assert.equal(ko[storedError.messageKey], storedError.message);
  assert.equal(en[storedError.messageKey], "Could not open the project. Try another Git project.");
  assert.equal(failedController.snapshot().error, storedError, "display language must not require another controller operation");
  assert.equal(storedError.technicalDetail, "E_NOT_GIT /tmp/not-a-project");
  cancelledController.dispose();
  failedController.dispose();
});

test("스냅샷 생성 실패는 프로젝트를 활성화하거나 tree·변경·watch를 호출하지 않고 증거를 남긴다", async () => {
  const bridge = new FakeBridge();
  bridge.createError = new Error("E_SNAPSHOT_RECOVERY_REQUIRED export evidence is retained");
  const controller = new ProjectEditorController(bridge);
  controller.start();

  await controller.chooseProject();

  assert.equal(controller.snapshot().project, null);
  assert.match(controller.snapshot().error?.message ?? "", /작업 복사본을 준비하지 못했어요/);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /E_SNAPSHOT_RECOVERY_REQUIRED/);
  assert.deepEqual(bridge.calls, ["openProject", "createProjectWorktree:project_01"]);
  controller.dispose();
});

test("binary·tooLarge 파일, watcher overflow, bounded tree와 rename diff를 분명히 표시한다", async () => {
  const bridge = new FakeBridge();
  bridge.treeCount = MAX_RENDERED_TREE_ENTRIES + 37;
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  assert.equal(controller.snapshot().tree.length, MAX_RENDERED_TREE_ENTRIES);

  bridge.classification = "binary";
  await controller.openFile("assets/logo.png");
  assert.equal(controller.snapshot().tabs[0]?.mode, "binary");
  bridge.classification = "tooLarge";
  await controller.openFile("logs/large.log");
  assert.equal(controller.snapshot().tabs[1]?.mode, "tooLarge");
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: [], rescanRequired: true });
  assert.equal(controller.snapshot().rescanRequired, true);

  await controller.openDiff("src/new.ts");
  assert.equal(controller.snapshot().diffFile?.kind, "renamed");
  assert.equal(controller.snapshot().diffFile?.oldPath, "src/old.ts");
  assert.match(controller.snapshot().diffText, /\+new/);
  await controller.openDiff("src/removed.ts");
  assert.equal(controller.snapshot().diffFile?.kind, "deleted");
  assert.match(controller.snapshot().diffText, /-removed/);
  await controller.openDiff("assets/logo.png");
  assert.equal(controller.snapshot().diffFile?.kind, "binary");
  assert.equal(controller.snapshot().diffText, "");
  controller.dispose();
});

test("manual diff refreshes share an active read but mutations request fresh data", async () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  try {
    await controller.chooseProject();
    const requests: ReturnType<typeof deferred<DiffSummaryV1>>[] = [];
    bridge.getDiffSummary = async () => {
      const request = deferred<DiffSummaryV1>();
      requests.push(request);
      return request.promise;
    };
    const first = controller.refreshChanges();
    await nextTurn();
    const shared = controller.refreshChanges();
    await nextTurn();
    assert.equal(requests.length, 1);
    const changed = controller.refreshChanges(true);
    await nextTurn();
    assert.equal(requests.length, 2);
    const summary = { projectId: "project_01", baseCommit, entries: [] };
    requests[0].resolve(summary);
    await Promise.all([first, shared]);
    const latestShared = controller.refreshChanges();
    await nextTurn();
    assert.equal(requests.length, 2, "older completion must not clear the latest pending read");
    requests[1].resolve(summary);
    await Promise.all([changed, latestShared]);
    const beforeWatch = controller.refreshChanges();
    await nextTurn();
    bridge.emitProject({ projectId: "project_01", event: "diff.changed", paths: ["src/new.ts"], rescanRequired: false });
    await nextTurn();
    assert.equal(requests.length, 4, "watcher changes must not share a pre-change read");
    requests[2].resolve(summary);
    requests[3].resolve(summary);
    await beforeWatch;
    await nextTurn();
    const failed = controller.refreshChanges();
    const failedShared = controller.refreshChanges();
    await nextTurn();
    assert.equal(requests.length, 5);
    requests[4].reject(new Error("summary unavailable"));
    await Promise.all([failed, failedShared]);
    assert.ok(controller.snapshot().error);
  } finally { controller.dispose(); }
});

test("refreshing changes clears stale diff content and invalidates an in-flight selection", async () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  try {
    await controller.chooseProject();
    await controller.openDiff("src/removed.ts");
    assert.equal(controller.snapshot().diffText, "-removed\n");
    await controller.refreshChanges();
    assert.equal(controller.snapshot().diffFile, null);
    assert.equal(controller.snapshot().diffText, "");
    bridge.autoDiffChunks = false;
    const pending = controller.openDiff("src/new.ts");
    await nextTurn();
    await controller.refreshChanges();
    bridge.emitChunk("stream_diff", "-old\n+new\n");
    await pending;
    assert.equal(controller.snapshot().diffFile, null);
    assert.equal(controller.snapshot().diffText, "");
  } finally { controller.dispose(); }
});

test("latest diff selection survives an older descriptor, stream, or failure", async () => {
  for (const late of ["descriptor", "stream", "failure"] as const) {
    class DelayedDiffBridge extends FakeBridge {
      pending = deferred<DiffFileDescriptorV1>();
      override async getDiffFile({ path }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
        if (path === "src/first.ts") return this.pending.promise;
        return super.getDiffFile({ projectId: "project_01", path });
      }
    }
    const bridge = new DelayedDiffBridge();
    const controller = new ProjectEditorController(bridge);
    controller.start();
    try {
      await controller.chooseProject();
      const first = controller.openDiff("src/first.ts");
      const descriptor: DiffFileDescriptorV1 = {
        projectId: "project_01", path: "src/first.ts", oldPath: null, kind: "modified",
        baseVersion: version("old"), worktreeVersion: version("first"), byteLength: 7,
        stream: { streamId: "stream_late_diff", chunkBytes: 262144 },
      };
      if (late === "stream") { bridge.pending.resolve(descriptor); await nextTurn(); }
      await controller.openDiff("src/removed.ts");
      if (late === "failure") bridge.pending.reject(new Error("stale diff failure"));
      else {
        bridge.pending.resolve(descriptor);
        await nextTurn();
        bridge.emitChunk("stream_late_diff", "+first\n");
      }
      await first;
      assert.equal(controller.snapshot().diffFile?.path, "src/removed.ts", late);
      assert.equal(controller.snapshot().diffText, "-removed\n", late);
      assert.equal(controller.snapshot().error, null, late);
    } finally { controller.dispose(); }
  }
});

test("allocation-free byte lengths match TextEncoder for Unicode and lone surrogates", () => {
  const encoder = new TextEncoder();
  const samples = ["", "ascii", "한글", "🦦", "\ud800", "\udc00", "\ud800a\udc00", "🦦나".repeat(10000)];
  // Every UTF-16 code unit individually, and mixed boundaries/pairs in context.
  for (let unit = 0; unit <= 0xffff; unit++) {
    const text = String.fromCharCode(unit);
    assert.equal(utf8ByteLength(text), encoder.encode(text).byteLength);
  }
  for (const left of [0, 0x7f, 0x80, 0x7ff, 0x800, 0xd7ff, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xe000, 0xffff]) {
    for (const right of [0x61, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xffff]) {
      samples.push(String.fromCharCode(left, right));
    }
  }
  for (const text of samples) assert.equal(utf8ByteLength(text), encoder.encode(text).byteLength);
});

test("UTF-8 splitter는 문자 경계를 깨지 않고 protocol byte limit을 지킨다", () => {
  const chunks = splitUtf8("a🦦나", 4);
  assert.deepEqual(chunks, ["a", "🦦", "나"]);
  assert.ok(chunks.every((chunk) => new TextEncoder().encode(chunk).byteLength <= 4));
});

test("UTF-8 splitter preserves TextEncoder byte boundaries including lone surrogates", () => {
  const encoder = new TextEncoder();
  const texts = ["", "plain ASCII", "\0\u007f\u0080\u07ff\u0800\uffff", "가🦦e\u0301", "\ud800a\udc00", "🦦".repeat(100)];
  for (const text of texts) for (const limit of [1, 2, 3, 4, 5, 16, 64]) {
    if ([...text].some(char => encoder.encode(char).byteLength > limit)) {
      assert.throws(() => splitUtf8(text, limit));
      continue;
    }
    const expected: string[] = [];
    let chunk = "";
    for (const char of text) {
      if (encoder.encode(chunk + char).byteLength > limit) { expected.push(chunk); chunk = ""; }
      chunk += char;
    }
    if (chunk) expected.push(chunk);
    assert.deepEqual(splitUtf8(text, limit), expected);
    assert.equal(splitUtf8(text, limit).join(""), text);
  }
});

test("cross-IPC에서 descriptor 전에 온 single·multi chunk를 원자적으로 채택하고 정상 post-descriptor stream도 계속 받는다", async () => {
  class PreResponseBridge extends FakeBridge {
    override async readFile({ path }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      const text = "첫 파일\n";
      this.emitChunk("stream_pre_file", "첫 ", 1);
      this.emitChunk("stream_pre_file", "파일\n", 2);
      return {
        projectId: "project_01", path, classification: "utf8", byteLength: new TextEncoder().encode(text).byteLength,
        version: version(text), stream: { streamId: "stream_pre_file", chunkBytes: 262144 },
      };
    }

    override async getDiffFile({ path }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
      const text = "-before\n+after\n";
      this.emitChunk("stream_pre_diff", "-before\n", 1);
      this.emitChunk("stream_pre_diff", "+after\n", 2);
      return {
        projectId: "project_01", kind: "modified", path, oldPath: null,
        baseVersion: version("before"), worktreeVersion: version("after"), byteLength: new TextEncoder().encode(text).byteLength,
        stream: { streamId: "stream_pre_diff", chunkBytes: 262144 },
      };
    }
  }

  const bridge = new PreResponseBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  await controller.openFile("src/pre-response.ts");
  assert.equal(controller.snapshot().tabs[0]?.text, "첫 파일\n");
  await controller.openDiff("src/pre-response.ts");
  assert.equal(controller.snapshot().diffText, "-before\n+after\n");

  const postBridge = new FakeBridge();
  postBridge.autoReadChunks = false;
  postBridge.fileText = "after descriptor\n";
  const postController = new ProjectEditorController(postBridge);
  postController.start();
  await postController.chooseProject();
  const opening = postController.openFile("src/post-response.ts");
  await nextTurn();
  postBridge.emitChunk("stream_read", postBridge.fileText, 1);
  await opening;
  assert.equal(postController.snapshot().tabs[0]?.text, postBridge.fileText);
  postController.dispose();
  controller.dispose();
});

test("pending과 active stream은 256 chunk까지 허용하고 257번째는 pre/post descriptor 모두 fail-closed한다", async () => {
  class ChunkCountBridge extends FakeBridge {
    count = 256;
    preDescriptor = true;

    override async readFile({ path }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      const streamId = this.preDescriptor ? "stream_count_pre" : "stream_count_post";
      if (this.preDescriptor) {
        for (let index = 0; index < this.count; index += 1) this.emitChunk(streamId, "x", index + 1);
      }
      return {
        projectId: "project_01", path, classification: "utf8", byteLength: this.count,
        version: version("x".repeat(this.count)), stream: { streamId, chunkBytes: 262144 },
      };
    }
  }

  const bridge = new ChunkCountBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();

  await controller.openFile("src/pre-256.ts");
  assert.equal(controller.snapshot().tabs[0]?.text.length, 256);
  assert.equal(controller.closeTab("src/pre-256.ts"), true);

  bridge.count = 257;
  await controller.openFile("src/pre-257.ts");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /exceeds 256 chunks/);

  bridge.preDescriptor = false;
  const opening = controller.openFile("src/post-257.ts");
  await nextTurn();
  for (let index = 0; index < bridge.count; index += 1) bridge.emitChunk("stream_count_post", "x", index + 1);
  await opening;
  assert.equal(controller.snapshot().project?.id, "project_01");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /exceeds 256 chunks/);

  bridge.preDescriptor = true;
  bridge.count = 1;
  await controller.openFile("src/recovered.ts");
  assert.equal(controller.snapshot().tabs[0]?.text, "x");
  controller.dispose();
});

test("unknown pre-descriptor stream은 만료되고 pending stream overflow는 프로젝트·draft를 손상시키지 않고 fail-closed한다", { timeout: 10_000 }, async () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  for (let index = 0; index < MAX_PENDING_STREAMS; index += 1) bridge.emitChunk(`orphan_${index}`, "x");
  bridge.emitChunk("orphan_overflow", "x");

  await controller.openFile("src/blocked.ts");
  assert.equal(controller.snapshot().project?.id, "project_01");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /pre-descriptor stream buffer overflow/);

  await new Promise<void>((resolve) => setTimeout(resolve, STREAM_TIMEOUT_MS + 30));
  await controller.openFile("src/after-expiry.ts");
  assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);
  controller.dispose();
});

test("descriptor 뒤의 late multi-chunk stream은 byteLength와 seq를 확인한 뒤 파일과 diff에만 노출한다", async () => {
  const bridge = new FakeBridge();
  bridge.autoReadChunks = false;
  bridge.autoDiffChunks = false;
  bridge.fileText = "첫 줄\n둘째 줄\n";
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();

  const opening = controller.openFile("src/file-000.ts");
  await nextTurn();
  bridge.emitChunk("stream_read", "첫 줄\n", 1);
  bridge.emitChunk("stream_read", "둘째 줄\n", 2);
  await opening;
  assert.equal(controller.snapshot().tabs[0]?.text, bridge.fileText);

  const openingDiff = controller.openDiff("src/new.ts");
  await nextTurn();
  bridge.emitChunk("stream_diff", "-old\n", 1);
  bridge.emitChunk("stream_diff", "+new\n", 2);
  await openingDiff;
  assert.equal(controller.snapshot().diffText, "-old\n+new\n");
  controller.dispose();
});

test("diff는 별도 10MiB 상한까지 정확한 chunk 조립을 허용하지만 파일 읽기는 5MiB를 넘기지 않는다", async () => {
  class BoundedStreamBridge extends FakeBridge {
    diffBytes = MAX_DIFF_STREAM_BYTES;
    oversizedFile = false;

    override async readFile({ path }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      if (!this.oversizedFile) return super.readFile({ projectId: "project_01", path });
      return {
        projectId: "project_01",
        path,
        classification: "utf8",
        byteLength: FILE_MAX_BYTES_V1 + 1,
        version: version("oversized"),
        stream: { streamId: "stream_oversized_file", chunkBytes: 262144 },
      };
    }

    override async getDiffFile({ path }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
      return {
        projectId: "project_01",
        kind: "modified",
        path,
        oldPath: null,
        baseVersion: version("old"),
        worktreeVersion: version("new"),
        byteLength: this.diffBytes,
        stream: { streamId: "stream_large_diff", chunkBytes: 262144 },
      };
    }
  }

  const bridge = new BoundedStreamBridge();
  bridge.autoReadChunks = false;
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();

  const openingDiff = controller.openDiff("src/large.ts");
  await nextTurn();
  const chunk = "a".repeat(256 * 1024);
  for (let index = 0; index < MAX_DIFF_STREAM_BYTES / (256 * 1024); index += 1) {
    bridge.emitChunk("stream_large_diff", chunk, index + 1);
  }
  await openingDiff;
  assert.equal(controller.snapshot().diffText.length, MAX_DIFF_STREAM_BYTES);

  bridge.diffBytes = MAX_DIFF_STREAM_BYTES + 1;
  await controller.openDiff("src/too-large-diff.ts");
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /unsafe byteLength/);
  assert.equal(controller.snapshot().diffFile?.path, "src/large.ts");

  bridge.oversizedFile = true;
  await controller.openFile("src/too-large-file.ts");
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /unsafe byteLength/);
  assert.equal(controller.snapshot().tabs.length, 0);
  controller.dispose();
});

test("empty stream, sequence/byteLength failure는 지연 대기 또는 부분 텍스트 노출 없이 판정한다", async () => {
  const empty = new FakeBridge();
  empty.autoReadChunks = false;
  empty.fileText = "";
  const emptyController = new ProjectEditorController(empty);
  emptyController.start();
  await emptyController.chooseProject();
  await emptyController.openFile("src/file-000.ts");
  assert.equal(emptyController.snapshot().tabs[0]?.text, "");

  const malformed = new FakeBridge();
  malformed.autoReadChunks = false;
  const malformedController = new ProjectEditorController(malformed);
  malformedController.start();
  await malformedController.chooseProject();
  const opening = malformedController.openFile("src/file-000.ts");
  await nextTurn();
  malformed.emitChunk("stream_read", malformed.fileText, 2);
  await opening;
  assert.equal(malformedController.snapshot().tabs.length, 0);
  assert.match(malformedController.snapshot().error?.technicalDetail ?? "", /expected seq 1/);

  const oversized = new FakeBridge();
  oversized.autoReadChunks = false;
  const oversizedController = new ProjectEditorController(oversized);
  oversizedController.start();
  await oversizedController.chooseProject();
  const tooLong = oversizedController.openFile("src/file-000.ts");
  await nextTurn();
  oversized.emitChunk("stream_read", `${oversized.fileText}unexpected`, 1);
  await tooLong;
  assert.match(oversizedController.snapshot().error?.technicalDetail ?? "", /descriptor byteLength/);
  emptyController.dispose();
  malformedController.dispose();
  oversizedController.dispose();
});

test("saving 중 watcher conflict가 늦은 commit 성공보다 우선하며 draft와 rescan 요구를 보존한다", async () => {
  const { bridge, controller } = await openText();
  bridge.pendingCommit = deferred();
  controller.updateText("src/file-000.ts", "보존할 저장 초안\n");
  const saving = controller.saveActive();
  await nextTurn();
  assert.equal(controller.snapshot().tabs[0]?.saveState, "saving");
  bridge.emitProject({ projectId: "project_01", event: "file.conflict", paths: ["src/file-000.ts"], rescanRequired: false });
  bridge.pendingCommit.resolve({ outcome: "committed", version: version("보존할 저장 초안\n") });
  await saving;
  assert.equal(controller.snapshot().tabs[0]?.saveState, "conflicted");
  assert.equal(controller.snapshot().tabs[0]?.text, "보존할 저장 초안\n");
  assert.equal(controller.snapshot().rescanRequired, true);
  controller.dispose();
});

test("a self-save watch notification before commit is reconciled without losing a newer draft", async () => {
  const { bridge, controller } = await openText();
  bridge.pendingCommit = deferred();
  controller.updateText("src/file-000.ts", "saved text");
  const saving = controller.saveActive();
  await nextTurn();
  bridge.fileText = "saved text";
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });
  controller.updateText("src/file-000.ts", "newer draft");
  bridge.pendingCommit.resolve({ outcome: "committed", version: version("saved text") });
  await saving;
  await nextTurn();
  assert.equal(controller.snapshot().tabs[0]?.saveState, "dirty");
  assert.equal(controller.snapshot().tabs[0]?.text, "newer draft");
  assert.equal(controller.snapshot().rescanRequired, false);
  assert.equal(controller.snapshot().noticeKey, "m5.editorNotice.newerDraft");
  controller.dispose();
});

test("controller 구독은 start/cleanup commit phase에만 생겨 StrictMode 재마운트 뒤 listener를 남기지 않는다", () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  assert.equal(bridge.sidecarListeners.size, 0);
  assert.equal(bridge.projectListeners.size, 0);
  controller.start();
  assert.equal(bridge.sidecarListeners.size, 1);
  assert.equal(bridge.projectListeners.size, 1);
  controller.dispose();
  assert.equal(bridge.sidecarListeners.size, 0);
  assert.equal(bridge.projectListeners.size, 0);
  controller.start();
  controller.dispose();
  assert.equal(bridge.sidecarListeners.size, 0);
  assert.equal(bridge.projectListeners.size, 0);
});

test("A의 늦은 tree/read/diff 결과와 오류는 B 프로젝트 상태를 덮지 않는다", async () => {
  class EpochBridge extends FakeBridge {
    readonly projects: ProjectIdentityV1[] = [
      { schemaVersion: 1, projectId: "project_a", selectedPath: "/work/a", canonicalGitRoot: "/work/a" },
      { schemaVersion: 1, projectId: "project_b", selectedPath: "/work/b", canonicalGitRoot: "/work/b" },
    ];
    readonly trees: { projectId: string; deferred: Deferred<TreePageV1> }[] = [];
    readonly summaries: { projectId: string; deferred: Deferred<DiffSummaryV1> }[] = [];
    readonly reads: { projectId: string; deferred: Deferred<FileReadDescriptorV1> }[] = [];
    readonly diffs: { projectId: string; deferred: Deferred<DiffFileDescriptorV1> }[] = [];
    projectIndex = 0;

    async openProject(): Promise<ProjectIdentityV1> {
      return this.projects[this.projectIndex++]!;
    }

    async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
      const project = this.projects.find((candidate) => candidate.projectId === projectId);
      assert.ok(project, `missing ${projectId} identity`);
      return snapshotManifest(projectId, project.canonicalGitRoot);
    }

    async getProjectTree({ projectId }: { projectId: string; parentPath: string; cursor: string | null }): Promise<TreePageV1> {
      const request = { projectId, deferred: deferred<TreePageV1>() };
      this.trees.push(request);
      return request.deferred.promise;
    }

    async getDiffSummary({ projectId }: { projectId: string }): Promise<DiffSummaryV1> {
      const request = { projectId, deferred: deferred<DiffSummaryV1>() };
      this.summaries.push(request);
      return request.deferred.promise;
    }

    async readFile({ projectId }: { projectId: string; path: string }): Promise<FileReadDescriptorV1> {
      const request = { projectId, deferred: deferred<FileReadDescriptorV1>() };
      this.reads.push(request);
      return request.deferred.promise;
    }

    async getDiffFile({ projectId }: { projectId: string; path: string }): Promise<DiffFileDescriptorV1> {
      const request = { projectId, deferred: deferred<DiffFileDescriptorV1>() };
      this.diffs.push(request);
      return request.deferred.promise;
    }
  }

  const bridge = new EpochBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  const choosingA = controller.chooseProject();
  await nextTurn();
  const readA = controller.openFile("src/a.ts");
  const diffA = controller.openDiff("src/a.ts");
  await nextTurn();
  const choosingB = controller.chooseProject();
  await nextTurn();

  resolveTree(bridge, "project_b");
  resolveSummary(bridge, "project_b");
  await choosingB;
  assert.equal(controller.snapshot().project?.id, "project_b");

  resolveTree(bridge, "project_a");
  resolveSummary(bridge, "project_a");
  const text = "A가 늦게 도착\n";
  bridge.reads.find((request) => request.projectId === "project_a")?.deferred.resolve({
    projectId: "project_a", path: "src/a.ts", classification: "utf8", byteLength: new TextEncoder().encode(text).byteLength,
    version: version(text), stream: { streamId: "stream_a_read", chunkBytes: 262144 },
  });
  bridge.diffs.find((request) => request.projectId === "project_a")?.deferred.resolve({
    projectId: "project_a", kind: "modified", path: "src/a.ts", oldPath: null,
    baseVersion: version("old"), worktreeVersion: version(text), byteLength: new TextEncoder().encode("-old\n+new\n").byteLength,
    stream: { streamId: "stream_a_diff", chunkBytes: 262144 },
  });
  await nextTurn();
  bridge.emitChunk("stream_a_read", text);
  bridge.emitChunk("stream_a_diff", "-old\n+new\n");
  await Promise.all([choosingA, readA, diffA]);

  assert.equal(controller.snapshot().project?.id, "project_b");
  assert.equal(controller.snapshot().tabs.length, 0);
  assert.equal(controller.snapshot().diffFile, null);
  assert.equal(controller.snapshot().error, null);
  controller.dispose();
});

test("A의 늦은 스냅샷 생성은 B를 덮지 않고 닫은 뒤 명시적인 C 선택만 허용한다", async () => {
  class SnapshotRaceBridge extends FakeBridge {
    readonly projects: ProjectIdentityV1[] = [
      { schemaVersion: 1, projectId: "project_a", selectedPath: "/work/a", canonicalGitRoot: "/work/a" },
      { schemaVersion: 1, projectId: "project_b", selectedPath: "/work/b", canonicalGitRoot: "/work/b" },
      { schemaVersion: 1, projectId: "project_c", selectedPath: "/work/c", canonicalGitRoot: "/work/c" },
    ];
    readonly creations: { projectId: string; deferred: Deferred<WorktreeManifestV1> }[] = [];
    projectIndex = 0;

    async openProject(): Promise<ProjectIdentityV1> {
      const project = this.projects[this.projectIndex++]!;
      this.calls.push(`openProject:${project.projectId}`);
      return project;
    }

    async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
      this.calls.push(`createProjectWorktree:${projectId}`);
      const request = { projectId, deferred: deferred<WorktreeManifestV1>() };
      this.creations.push(request);
      return request.deferred.promise;
    }

    async getProjectTree({ projectId }: { projectId: string; parentPath: string; cursor: string | null }): Promise<TreePageV1> {
      this.calls.push(`getProjectTree:${projectId}`);
      return { projectId, parentPath: "", entries: [], nextCursor: null };
    }

    async getDiffSummary({ projectId }: { projectId: string }): Promise<DiffSummaryV1> {
      this.calls.push(`getDiffSummary:${projectId}`);
      return { projectId, baseCommit, entries: [] };
    }

    async watchProjectFiles({ projectId }: { projectId: string }): Promise<{ watching: true }> {
      this.calls.push(`watchProjectFiles:${projectId}`);
      return { watching: true };
    }
  }

  const bridge = new SnapshotRaceBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  const choosingA = controller.chooseProject();
  await nextTurn();
  assert.deepEqual(bridge.calls, ["openProject:project_a", "createProjectWorktree:project_a"]);

  const choosingB = controller.chooseProject();
  await nextTurn();
  assert.deepEqual(bridge.calls, [
    "openProject:project_a",
    "createProjectWorktree:project_a",
    "openProject:project_b",
  ]);

  bridge.creations[0]?.deferred.resolve(snapshotManifest("project_a", "/work/a"));
  await nextTurn();
  assert.deepEqual(bridge.calls.slice(3), ["closeProject:project_a"]);
  await Promise.all([choosingA, choosingB]);
  await nextTurn();
  assert.equal(controller.snapshot().project, null);
  assert.equal(bridge.calls.includes("createProjectWorktree:project_b"), false);

  const choosingC = controller.chooseProject();
  await nextTurn();
  assert.deepEqual(bridge.calls.slice(4), ["openProject:project_c", "createProjectWorktree:project_c"]);
  bridge.creations[1]?.deferred.resolve(snapshotManifest("project_c", "/work/c"));
  await choosingC;

  assert.equal(controller.snapshot().project?.id, "project_c");
  assert.equal(controller.snapshot().project?.root, "/work/c");
  assert.ok(bridge.calls.includes("closeProject:project_a"));
  assert.deepEqual(
    bridge.calls.filter((call) => /^(getProjectTree|getDiffSummary|watchProjectFiles):/.test(call)),
    ["getProjectTree:project_c", "getDiffSummary:project_c", "watchProjectFiles:project_c"],
  );
  controller.dispose();
});

test("stale A를 닫지 못하면 최신 B에 증거를 보이고 이후 선택을 모두 막는다", async () => {
  class CloseFailureBridge extends FakeBridge {
    readonly projects: ProjectIdentityV1[] = [
      { schemaVersion: 1, projectId: "project_a", selectedPath: "/work/a", canonicalGitRoot: "/work/a" },
      { schemaVersion: 1, projectId: "project_b", selectedPath: "/work/b", canonicalGitRoot: "/work/b" },
    ];
    readonly creations: Deferred<WorktreeManifestV1>[] = [];
    projectIndex = 0;

    async openProject(): Promise<ProjectIdentityV1> {
      const project = this.projects[this.projectIndex++]!;
      this.calls.push(`openProject:${project.projectId}`);
      return project;
    }

    async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
      this.calls.push(`createProjectWorktree:${projectId}`);
      const creation = deferred<WorktreeManifestV1>();
      this.creations.push(creation);
      return creation.promise;
    }
  }

  const bridge = new CloseFailureBridge();
  bridge.closeError = new Error("E_CLOSE_EVIDENCE_PENDING project_a");
  const controller = new ProjectEditorController(bridge);
  controller.start();
  const choosingA = controller.chooseProject();
  await nextTurn();
  const choosingB = controller.chooseProject();
  await nextTurn();
  bridge.creations[0]?.resolve(snapshotManifest("project_a", "/work/a"));
  await Promise.all([choosingA, choosingB]);

  assert.equal(controller.snapshot().selectionBlocked, true);
  assert.match(controller.snapshot().error?.message ?? "", /닫았는지 확인할 수 없어요/);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /E_CLOSE_EVIDENCE_PENDING/);
  assert.deepEqual(bridge.calls, [
    "openProject:project_a",
    "createProjectWorktree:project_a",
    "openProject:project_b",
    "closeProject:project_a",
  ]);
  await controller.chooseProject();
  assert.deepEqual(bridge.calls, [
    "openProject:project_a",
    "createProjectWorktree:project_a",
    "openProject:project_b",
    "closeProject:project_a",
  ]);
  assert.equal(controller.snapshot().project, null);
  controller.dispose();
});

test("시간 초과된 A는 B를 즉시 막고, 늦은 성공을 닫은 뒤에만 명시적인 C를 허용한다", async () => {
  class TimeoutBridge extends FakeBridge {
    readonly projects: ProjectIdentityV1[] = [
      { schemaVersion: 1, projectId: "project_a", selectedPath: "/work/a", canonicalGitRoot: "/work/a" },
      { schemaVersion: 1, projectId: "project_c", selectedPath: "/work/c", canonicalGitRoot: "/work/c" },
    ];
    readonly creations: Deferred<WorktreeManifestV1>[] = [];
    projectIndex = 0;

    async openProject(): Promise<ProjectIdentityV1> {
      const project = this.projects[this.projectIndex++]!;
      this.calls.push(`openProject:${project.projectId}`);
      return project;
    }

    async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
      this.calls.push(`createProjectWorktree:${projectId}`);
      const creation = deferred<WorktreeManifestV1>();
      this.creations.push(creation);
      return creation.promise;
    }

    async getProjectTree({ projectId }: { projectId: string; parentPath: string; cursor: string | null }): Promise<TreePageV1> {
      this.calls.push(`getProjectTree:${projectId}`);
      return { projectId, parentPath: "", entries: [], nextCursor: null };
    }

    async getDiffSummary({ projectId }: { projectId: string }): Promise<DiffSummaryV1> {
      this.calls.push(`getDiffSummary:${projectId}`);
      return { projectId, baseCommit, entries: [] };
    }

    async watchProjectFiles({ projectId }: { projectId: string }): Promise<{ watching: true }> {
      this.calls.push(`watchProjectFiles:${projectId}`);
      return { watching: true };
    }
  }

  assert.equal(DEFAULT_SNAPSHOT_CREATION_TIMEOUT_MS, 60_000);
  const bridge = new TimeoutBridge();
  const controller = new ProjectEditorController(bridge, { snapshotCreationTimeoutMs: 10 });
  controller.start();
  const started = Date.now();
  await controller.chooseProject();
  assert.ok(Date.now() - started < 500, "timeout must return control without waiting for the unresolved create");
  assert.equal(controller.snapshot().selectionBlocked, true);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /10ms/);

  await controller.chooseProject();
  assert.deepEqual(bridge.calls, ["openProject:project_a", "createProjectWorktree:project_a"]);
  bridge.creations[0]?.resolve(snapshotManifest("project_a", "/work/a"));
  await nextTurn();
  assert.equal(controller.snapshot().project, null);
  assert.equal(controller.snapshot().selectionBlocked, false);
  assert.ok(bridge.calls.includes("closeProject:project_a"));

  const choosingC = controller.chooseProject();
  await nextTurn();
  bridge.creations[1]?.resolve(snapshotManifest("project_c", "/work/c"));
  await choosingC;
  assert.equal(controller.snapshot().project?.id, "project_c");
  assert.deepEqual(
    bridge.calls.filter((call) => /^(getProjectTree|getDiffSummary|watchProjectFiles):/.test(call)),
    ["getProjectTree:project_c", "getDiffSummary:project_c", "watchProjectFiles:project_c"],
  );
  controller.dispose();
});

test("시간 초과 뒤 늦은 A를 닫지 못하면 차단과 기술 증거를 유지한다", async () => {
  class LateCloseFailureBridge extends FakeBridge {
    readonly creation = deferred<WorktreeManifestV1>();

    async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
      this.calls.push(`createProjectWorktree:${projectId}`);
      return this.creation.promise;
    }
  }

  const bridge = new LateCloseFailureBridge();
  bridge.closeError = new Error("E_LATE_CLOSE_FAILED evidence retained");
  const controller = new ProjectEditorController(bridge, { snapshotCreationTimeoutMs: 10 });
  controller.start();
  await controller.chooseProject();
  bridge.creation.resolve(snapshotManifest("project_01", "/work/example"));
  await nextTurn();

  assert.equal(controller.snapshot().selectionBlocked, true);
  assert.match(controller.snapshot().error?.technicalDetail ?? "", /E_LATE_CLOSE_FAILED/);
  const calls = [...bridge.calls];
  await controller.chooseProject();
  assert.deepEqual(bridge.calls, calls);
  controller.dispose();
});

test("picker 취소는 기존 프로젝트의 tree·tabs·notice·error를 그대로 보존한다", async () => {
  const bridge = new FakeBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  await controller.chooseProject();
  await controller.openFile("src/file-000.ts");
  bridge.readError = new Error("E_READ_EVIDENCE");
  await controller.openFile("src/other.ts");
  bridge.emitProject({ projectId: "project_01", event: "file.changed", paths: ["src/file-000.ts"], rescanRequired: false });
  await nextTurn();
  const before = controller.snapshot();
  bridge.openError = Object.assign(new Error("native picker cancelled"), { name: "AbortError" });

  await controller.chooseProject();

  assert.deepEqual(controller.snapshot(), before);
  controller.dispose();
});

test("dispose 뒤 늦은 create 성공은 publish/downstream 없이 한 번만 evidence-preserving close로 소비한다", async () => {
  class DeferredCreateBridge extends FakeBridge {
    readonly creation = deferred<WorktreeManifestV1>();

    async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
      this.calls.push(`createProjectWorktree:${projectId}`);
      return this.creation.promise;
    }
  }

  const bridge = new DeferredCreateBridge();
  const controller = new ProjectEditorController(bridge);
  const unhandled: unknown[] = [];
  const observeUnhandled = (reason: unknown) => unhandled.push(reason);
  process.on("unhandledRejection", observeUnhandled);
  try {
    controller.start();
    const choosing = controller.chooseProject();
    await nextTurn();
    controller.dispose();
    bridge.creation.resolve(snapshotManifest("project_01", "/work/example"));
    await choosing;
    await nextTurn();

    assert.equal(controller.snapshot().project, null);
    assert.equal(bridge.calls.filter((call) => call === "closeProject:project_01").length, 1);
    assert.deepEqual(
      bridge.calls.filter((call) => /^(getProjectTree|getDiffSummary|watchProjectFiles)/.test(call)),
      [],
    );
    assert.deepEqual(unhandled, []);
  } finally {
    process.off("unhandledRejection", observeUnhandled);
  }
});

test("dispose 뒤 늦은 create 거부는 close/downstream/state write 없이 소비한다", async () => {
  class DeferredCreateBridge extends FakeBridge {
    readonly creation = deferred<WorktreeManifestV1>();

    async createProjectWorktree({ projectId }: { projectId: string }): Promise<WorktreeManifestV1> {
      this.calls.push(`createProjectWorktree:${projectId}`);
      return this.creation.promise;
    }
  }

  const bridge = new DeferredCreateBridge();
  const controller = new ProjectEditorController(bridge);
  const unhandled: unknown[] = [];
  const observeUnhandled = (reason: unknown) => unhandled.push(reason);
  process.on("unhandledRejection", observeUnhandled);
  try {
    controller.start();
    const choosing = controller.chooseProject();
    await nextTurn();
    controller.dispose();
    bridge.creation.reject(new Error("E_LATE_CREATE_REJECTED"));
    await choosing;
    await nextTurn();

    assert.equal(controller.snapshot().project, null);
    assert.equal(bridge.calls.includes("closeProject:project_01"), false);
    assert.deepEqual(
      bridge.calls.filter((call) => /^(getProjectTree|getDiffSummary|watchProjectFiles)/.test(call)),
      [],
    );
    assert.deepEqual(unhandled, []);
  } finally {
    process.off("unhandledRejection", observeUnhandled);
  }
});

test("dispose 중 native open 완료는 opened identity를 close하지 않고 create를 시작하지 않는다", async () => {
  class DeferredOpenBridge extends FakeBridge {
    readonly opening = deferred<ProjectIdentityV1>();

    async openProject(): Promise<ProjectIdentityV1> {
      this.calls.push("openProject");
      return this.opening.promise;
    }
  }

  const bridge = new DeferredOpenBridge();
  const controller = new ProjectEditorController(bridge);
  controller.start();
  const choosing = controller.chooseProject();
  await nextTurn();
  controller.dispose();
  bridge.opening.resolve({
    schemaVersion: 1,
    projectId: "project_01",
    selectedPath: "/work/example",
    canonicalGitRoot: "/work/example",
  });
  await choosing;
  await nextTurn();

  // project.close validates an active snapshot, so an opened-only identity is left to the
  // sidecar lifecycle rather than issuing an invalid close request after unmount.
  assert.deepEqual(bridge.calls, ["openProject"]);
  assert.equal(controller.snapshot().project, null);
});

function resolveTree(bridge: { trees: { projectId: string; deferred: Deferred<TreePageV1> }[] }, projectId: string): void {
  const request = bridge.trees.find((item) => item.projectId === projectId);
  assert.ok(request, `missing ${projectId} tree request`);
  request.deferred.resolve({ projectId, parentPath: "", entries: [], nextCursor: null });
}

function resolveSummary(bridge: { summaries: { projectId: string; deferred: Deferred<DiffSummaryV1> }[] }, projectId: string): void {
  const request = bridge.summaries.find((item) => item.projectId === projectId);
  assert.ok(request, `missing ${projectId} summary request`);
  request.deferred.resolve({ projectId, baseCommit, entries: [] });
}
