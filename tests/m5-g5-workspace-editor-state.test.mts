import assert from "node:assert/strict";
import test from "node:test";

import { ProjectEditorController, type ChatProjectBridge } from "../src/m5/editor-state.ts";
import type { ChatWorkspaceRefV1, WorkspaceEventV1 } from "../src/m5/contracts/workspace-addressing-v1.ts";
import { validateWorkspaceTreeRequestV1, validateWorkspaceWatchTranscriptV1 } from "../src/m5/contracts/workspace-addressing-v1.ts";
import { sha256FileVersionV1 } from "../src/m5/contracts/file-v1.ts";
import { createEditorDraftStore, EDITOR_DRAFTS_KEY, EDITOR_DRAFTS_MAX_CHARS, EDITOR_DRAFTS_MAX_WORKSPACES } from "../src/m5/editor-drafts.ts";

const version = sha256FileVersionV1(new TextEncoder().encode("one\n"));
const a: ChatWorkspaceRefV1 = { chatId: "chat_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a" };
const b: ChatWorkspaceRefV1 = { chatId: "chat_b", projectId: "project_01", worktreeId: "worktree_b", workspaceIdentity: "workspace_b" };

test("workspace watch transcript requires exact suspension and matching address", () => {
  for (const state of [{ watching: true }, { watching: false, rescanRequired: true }]) {
    assert.deepEqual(validateWorkspaceWatchTranscriptV1({ request: a, response: { ...a, ...state } }).response, { ...a, ...state });
  }
  for (const state of [{ watching: false }, { watching: false, rescanRequired: false }, { watching: true, rescanRequired: true }, { watching: false, rescanRequired: true, extra: 1 }]) {
    assert.throws(() => validateWorkspaceWatchTranscriptV1({ request: a, response: { ...a, ...state } }));
  }
  assert.throws(() => validateWorkspaceWatchTranscriptV1({ request: a, response: { ...b, watching: false, rescanRequired: true } }));
});

test("suspended workspace watch preserves binding and explicit rescan state", async () => {
  const bridge = new Bridge() as unknown as ChatProjectBridge;
  bridge.watchWorkspaceFiles = async input => ({ ...input, watching: false, rescanRequired: true });
  const controller = new ProjectEditorController(bridge);
  try {
    await controller.chooseProject();
    assert.equal(await controller.bindChat(a), true);
    assert.deepEqual(controller.snapshot().workspace, a);
    assert.equal(controller.snapshot().rescanRequired, true);
    assert.equal(controller.snapshot().noticeKey, "m5.editorNotice.rescan");
  } finally { controller.dispose(); }
});

test("delayed initial workspace scan requests rescan only for the bound workspace", async () => {
  const { bridge, controller } = await bound();
  try {
    assert.equal(controller.snapshot().rescanRequired, false);
    for (const address of [b, a]) {
      bridge.emit({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar", streamId: `initial_${address.chatId}`, seq: 1,
        event: "workspace.changed", payload: { ...address, event: "file.changed", paths: [], rescanRequired: true } });
      assert.equal(controller.snapshot().rescanRequired, address === a);
    }
    assert.equal(controller.snapshot().noticeKey, "m5.editorNotice.rescan");
    assert.deepEqual(controller.snapshot().workspace, a);
  } finally { controller.dispose(); }
});

test("binding a full chat record retains only the exact workspace address", async () => {
  const bridge = new Bridge();
  const controller = new ProjectEditorController(bridge as unknown as ChatProjectBridge);
  await controller.chooseProject();
  const original = bridge.getWorkspaceTree.bind(bridge);
  bridge.getWorkspaceTree = async input => { validateWorkspaceTreeRequestV1(input); return original(input); };
  assert.equal(await controller.bindChat({ ...a, ...{ goal: "goal", state: "draft", roleSnapshots: [], createdAt: "today" } }), true);
  assert.deepEqual(controller.snapshot().workspace, a);
  controller.dispose();
});

class Bridge {
  calls: string[] = []; workspaceListeners = new Set<(event: WorkspaceEventV1) => void>(); sidecarListeners = new Set<(event: never) => void>(); projectListeners = new Set<(event: never) => void>();
  failTree = false; failWatch = false;
  async openProject() { return { schemaVersion: 1 as const, projectId: "project_01", selectedPath: "/safe/project", canonicalGitRoot: "/safe/project" }; }
  async createProjectWorktree() { return { projectId: "project_01", worktreeId: "pre_chat", worktreePath: "/private/pre", workspaceKind: "standaloneSnapshot", snapshotTransport: "gitBundle", sourceMutationPolicy: "forbidden", objectFormat: "sha1", baseCommit: "a".repeat(40), baseTree: "b".repeat(40), sourceIdentity: "source", workspaceIdentity: "pre", headMode: "detached", lifecycle: "active", schemaVersion: 1, canonicalGitRoot: "/safe/project" }; }
  async closeProject() { return { projectId: "project_01", lifecycle: "abandoned" as const, physicallyDeleted: false as const }; }
  async getProjectTree() { this.calls.push("v2.tree"); return { projectId: "project_01", parentPath: "", entries: [], nextCursor: null }; }
  async readFile() { this.calls.push("v2.read"); throw new Error("V2 must not be called after binding"); }
  async beginFileSave() { this.calls.push("v2.begin"); throw new Error("V2 must not be called after binding"); }
  async appendFileSaveChunk() { this.calls.push("v2.append"); throw new Error("V2 must not be called after binding"); }
  async commitFileSave() { this.calls.push("v2.commit"); throw new Error("V2 must not be called after binding"); }
  async watchProjectFiles() { this.calls.push("v2.watch"); return { watching: true as const }; }
  async getDiffSummary() { this.calls.push("v2.diff"); return { projectId: "project_01", baseCommit: "a".repeat(40), entries: [] }; }
  async getDiffFile() { this.calls.push("v2.diffFile"); throw new Error("V2 must not be called after binding"); }
  onSidecarEvent() { return () => {}; } onProjectEvent() { return () => {}; }
  async getWorkspaceTree(input: ChatWorkspaceRefV1 & { parentPath: string }) { this.calls.push(`v3.tree:${input.chatId}`); if (this.failTree && input.chatId === "chat_b") throw new Error("B tree failed"); return { ...input, entries: [{ path: "src/a.ts", name: "a.ts", kind: "file" as const }], nextCursor: null }; }
  async readWorkspaceFile(input: ChatWorkspaceRefV1 & { path: string }) { this.calls.push(`v3.read:${input.chatId}`); queueMicrotask(() => this.emit({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar", streamId: `read_${input.chatId}`, seq: 1, event: "workspace.blob.chunk", payload: { ...input, text: "one\n" } })); return { ...input, path: input.path, classification: "utf8" as const, byteLength: 4, version, stream: { streamId: `read_${input.chatId}`, chunkBytes: 262144 as const } }; }
  async beginWorkspaceFileSave(input: ChatWorkspaceRefV1) { this.calls.push(`v3.begin:${input.chatId}`); return { ...input, saveId: `save_${input.chatId}`, chunkBytes: 262144 as const }; }
  async appendWorkspaceFileSaveChunk(input: ChatWorkspaceRefV1 & { saveId: string; seq: number }) { this.calls.push(`v3.append:${input.chatId}:${input.saveId}`); return { ...input, acceptedSeq: input.seq }; }
  async commitWorkspaceFileSave(input: ChatWorkspaceRefV1 & { saveId: string }) { this.calls.push(`v3.commit:${input.chatId}:${input.saveId}`); return { ...input, saveId: input.saveId, outcome: "committed" as const, version }; }
  async watchWorkspaceFiles(input: ChatWorkspaceRefV1) { this.calls.push(`v3.watch:${input.chatId}`); if (this.failWatch && input.chatId === "chat_b") throw new Error("B watch failed"); return { ...input, watching: true as const }; }
  async getWorkspaceDiffSummary(input: ChatWorkspaceRefV1) { this.calls.push(`v3.diff:${input.chatId}`); return { ...input, baseCommit: "a".repeat(40), entries: [] }; }
  async getWorkspaceDiffFile(input: ChatWorkspaceRefV1 & { path: string }) { this.calls.push(`v3.diffFile:${input.chatId}`); return { ...input, kind: "modified" as const, path: input.path, oldPath: null, baseVersion: version, worktreeVersion: version, byteLength: 0, stream: null }; }
  async closeWorkspace(input: ChatWorkspaceRefV1) { this.calls.push(`v3.close:${input.chatId}`); return { ...input, lifecycle: "abandoned" as const, physicallyDeleted: false as const }; }
  onWorkspaceEvent(listener: (event: WorkspaceEventV1) => void) { this.workspaceListeners.add(listener); return () => this.workspaceListeners.delete(listener); }
  emit(event: WorkspaceEventV1) { for (const listener of this.workspaceListeners) listener(event); }
}

async function bound() { const bridge = new Bridge(); const controller = new ProjectEditorController(bridge as unknown as ChatProjectBridge); controller.start(); await controller.chooseProject(); bridge.calls = []; assert.equal(await controller.bindChat(a), true); bridge.calls = []; return { bridge, controller }; }

test("active chat tree/read/save/watch/diff는 V3 exact ref만 사용하고 V2 호출은 없다", async () => {
  const { bridge, controller } = await bound(); await controller.openFile("src/a.ts"); controller.updateText("src/a.ts", "two\n"); await controller.saveActive(); await controller.openDiff("src/a.ts");
  assert.ok(bridge.calls.every((call) => call.startsWith("v3.")), bridge.calls.join(","));
  assert.ok(bridge.calls.some((call) => call.includes("chat_a"))); controller.dispose();
});

test("file search uses only the active chat workspace and stops at a recovery gate", async () => {
  const { bridge, controller } = await bound();
  bridge.calls = [];
  bridge.getWorkspaceTree = async input => {
    assert.equal(input.workspaceIdentity, a.workspaceIdentity);
    bridge.calls.push(`v3.tree:${input.chatId}`);
    return { ...input, entries: [{ name: "a.ts", path: "a.ts", kind: "file" as const }], nextCursor: null };
  };
  const found = await controller.searchFiles("a.ts", new AbortController().signal);
  assert.equal(found.matches[0].path, "a.ts");
  assert.deepEqual(bridge.calls, ["v3.tree:chat_a"]);
  controller.setActionBlocked(true);
  await assert.rejects(controller.searchFiles("a", new AbortController().signal), { name: "AbortError" });
  assert.deepEqual(bridge.calls, ["v3.tree:chat_a"]);
  controller.dispose();
});

test("late search page from chat A cannot continue after switching to chat B", async () => {
  const { bridge, controller } = await bound();
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  let aReads = 0;
  bridge.getWorkspaceTree = async input => {
    if (input.chatId === a.chatId) { aReads++; await pending; }
    return { ...input, entries: [], nextCursor: null };
  };
  const search = controller.searchFiles("a", new AbortController().signal);
  assert.equal(await controller.bindChat(b), true);
  release();
  await assert.rejects(search, { name: "AbortError" });
  assert.equal(aReads, 1);
  assert.equal(controller.snapshot().workspace?.chatId, b.chatId);
  controller.dispose();
});

test("A에서 B로 clean switch하면 V3만 다시 로드하고, dirty A는 전환과 late B event를 막는다", async () => {
  const { bridge, controller } = await bound(); await controller.openFile("src/a.ts"); controller.updateText("src/a.ts", "draft\n"); assert.equal(await controller.bindChat(b), false); assert.equal(controller.snapshot().workspace?.chatId, "chat_a");
  bridge.emit({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar", streamId: "wrong", seq: 1, event: "workspace.changed", payload: { ...b, event: "file.changed", paths: ["src/a.ts"], rescanRequired: true } });
  assert.equal(controller.snapshot().rescanRequired, false); controller.dispose(); assert.equal(bridge.workspaceListeners.size, 0);
});

test("복구 또는 switching gate는 save·close·diff·file RPC를 시작하지 않는다", async () => {
  const { bridge, controller } = await bound(); const before = bridge.calls.length;
  controller.setActionBlocked(true, "복구가 필요해요.");
  await controller.openFile("src/a.ts"); await controller.openDiff("src/a.ts"); await controller.saveActive(); await controller.abandonProject();
  assert.equal(controller.closeTab("src/a.ts"), false);
  assert.equal(bridge.calls.length, before); controller.dispose();
});

test("B tree/watch bind 실패는 A ref와 tabs를 복원하고 이후 B diff/watch RPC를 시작하지 않는다", async () => {
  const { bridge, controller } = await bound(); await controller.openFile("src/a.ts"); const before = controller.snapshot(); bridge.calls = []; bridge.failTree = true;
  assert.equal(await controller.bindChat(b), false);
  assert.equal(controller.snapshot().workspace?.chatId, "chat_a"); assert.deepEqual(controller.snapshot().tabs, before.tabs);
  assert.deepEqual(bridge.calls, ["v3.tree:chat_b"]); controller.dispose();
});

// Unsaved edits across a restart (editor-drafts.ts). The "restart" is a new
// controller and bridge over the same browser storage.
function memoryStorage(failWrites = false) {
  const items = new Map<string, string>();
  return {
    items,
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => { if (failWrites) throw new Error("QuotaExceededError"); items.set(key, value); },
    removeItem: (key: string) => { items.delete(key); },
  };
}
async function opened(storage: ReturnType<typeof memoryStorage>, workspace: ChatWorkspaceRefV1 | null = a) {
  const bridge = new Bridge();
  const controller = new ProjectEditorController(bridge as unknown as ChatProjectBridge, { drafts: createEditorDraftStore(storage), draftDelayMs: 60_000 });
  controller.start();
  await controller.chooseProject();
  if (workspace) assert.equal(await controller.bindChat(workspace), true);
  return { bridge, controller };
}

test("unsaved edits come back after a restart, in the same workspace only, still unsaved", async () => {
  const storage = memoryStorage();
  const first = await opened(storage);
  await first.controller.openFile("src/a.ts");
  assert.equal(storage.items.size, 0, "an untouched file keeps nothing");
  first.controller.updateText("src/a.ts", "tw");
  assert.equal(createEditorDraftStore(storage).load(a)?.tabs[0]?.text, "tw", "the first unsaved change is kept at once");
  first.controller.updateText("src/a.ts", "two\n");
  assert.equal(createEditorDraftStore(storage).load(a)?.tabs[0]?.text, "tw", "typing is kept after a pause, not per keystroke");
  // The window is hidden or closed: the newest text is kept now.
  first.controller.flushDrafts();
  assert.equal(createEditorDraftStore(storage).load(a)?.tabs[0]?.text, "two\n");
  first.controller.dispose();

  const second = await opened(storage, b);
  assert.deepEqual(second.controller.snapshot().tabs, [], "another task's workspace gets nothing");
  second.controller.flushDrafts();
  assert.equal(createEditorDraftStore(storage).load(a)?.tabs.length, 1, "and the kept draft is not lost by opening it");
  assert.equal(await second.controller.bindChat(a), true);
  const restored = second.controller.snapshot();
  assert.deepEqual(restored.tabs.map(tab => [tab.path, tab.text, tab.saveState, tab.baseVersion, tab.mode]), [["src/a.ts", "two\n", "dirty", version, "text"]]);
  assert.equal(restored.activePath, "src/a.ts");
  assert.equal(restored.panel, "editor");
  assert.equal(restored.noticeKey, "m5.editorNotice.draftsRestored");
  assert.equal(second.controller.canLeaveProject(), false, "restored text is unsaved like any other");
  // Saving goes through the usual version check and then nothing is kept.
  second.bridge.calls = [];
  await second.controller.saveActive();
  assert.ok(second.bridge.calls.some(call => call.startsWith("v3.commit:chat_a")));
  assert.equal(second.controller.snapshot().tabs[0]?.saveState, "clean");
  assert.equal(storage.items.size, 0);
  second.controller.dispose();
});

test("a workspace that fails to open keeps its drafts; discarding a task forgets them", async () => {
  const storage = memoryStorage();
  createEditorDraftStore(storage).save(b, { activePath: "src/a.ts", tabs: [{ path: "src/a.ts", text: "kept\n", baseVersion: version, byteLength: 4, baseTextLength: 4 }] });
  const { bridge, controller } = await opened(storage);
  bridge.failTree = true;
  assert.equal(await controller.bindChat(b), false);
  controller.flushDrafts();
  assert.equal(createEditorDraftStore(storage).load(b)?.tabs[0]?.text, "kept\n");
  bridge.failTree = false;
  assert.equal(await controller.bindChat(b), true);
  assert.equal(controller.snapshot().tabs[0]?.text, "kept\n");
  // Removing the task from the list discards its unsaved text for good.
  assert.equal(controller.leaveProjectView(true), true);
  assert.equal(storage.items.size, 0);
  controller.dispose();
});

test("reloading a restored draft from disk forgets it", async () => {
  const storage = memoryStorage();
  createEditorDraftStore(storage).save(a, { activePath: "src/a.ts", tabs: [{ path: "src/a.ts", text: "mine\n", baseVersion: version, byteLength: 4 }] });
  const { controller } = await opened(storage);
  const tab = controller.snapshot().tabs[0]!;
  assert.equal(tab.saveState, "dirty");
  assert.equal(await controller.reloadConfirmedDraft(tab), true);
  assert.deepEqual(controller.snapshot().tabs.map(item => [item.text, item.saveState]), [["one\n", "clean"]]);
  assert.equal(storage.items.size, 0);
  controller.dispose();
});

test("the draft store is bounded and tolerates unreadable or refusing storage", () => {
  const storage = memoryStorage();
  const store = createEditorDraftStore(storage);
  const ref = (index: number): ChatWorkspaceRefV1 => ({ chatId: `chat_${index}`, projectId: "project_01", worktreeId: `worktree_${index}`, workspaceIdentity: `workspace_${index}` });
  const tab = (text: string) => ({ path: "src/a.ts", text, baseVersion: version, byteLength: text.length });
  const kept: boolean[] = [];
  for (let index = 0; index < EDITOR_DRAFTS_MAX_WORKSPACES + 3; index += 1) kept.push(store.save(ref(index), { activePath: null, tabs: [tab(`text ${index}`)] }));
  // What is kept stays kept (PR #18 review B2): a ninth task is not kept instead.
  assert.deepEqual(kept, [...Array(EDITOR_DRAFTS_MAX_WORKSPACES).fill(true), false, false, false]);
  assert.equal(store.load(ref(0))?.tabs[0]?.text, "text 0", "the oldest task keeps its draft");
  assert.equal(store.load(ref(EDITOR_DRAFTS_MAX_WORKSPACES)), null);
  assert.equal(JSON.parse(storage.items.get(EDITOR_DRAFTS_KEY)!).entries.length, EDITOR_DRAFTS_MAX_WORKSPACES);
  // A kept task's own draft can still change.
  assert.equal(store.save(ref(3), { activePath: null, tabs: [tab("text 3 again")] }), true);
  assert.equal(store.load(ref(3))?.tabs[0]?.text, "text 3 again");
  // The same task in another workspace copy is not this workspace.
  assert.equal(store.load({ ...ref(5), workspaceIdentity: "workspace_other" }), null);
  // Drafts too large to keep change nothing, not even this workspace's earlier draft (PR #18 re-review B2).
  assert.equal(store.save(ref(1), { activePath: null, tabs: [tab("x".repeat(EDITOR_DRAFTS_MAX_CHARS + 1)), tab("small")].map((item, index) => ({ ...item, path: `f${index}` })) }), false);
  assert.deepEqual(store.load(ref(1))?.tabs.map(item => item.text), ["text 1"]);
  assert.equal(store.save(ref(1), null), true);
  assert.equal(store.load(ref(1)), null);
  // Unreadable contents mean nothing was kept, never an error, and are never written over.
  for (const raw of ["not json", "[]", '{"version":2,"entries":[]}']) {
    storage.items.set(EDITOR_DRAFTS_KEY, raw);
    assert.equal(store.load(ref(5)), null, raw);
    assert.equal(store.save(ref(5), { activePath: null, tabs: [tab("new")] }), false, raw);
    assert.equal(storage.items.get(EDITOR_DRAFTS_KEY), raw, raw);
  }
  const refusing = createEditorDraftStore(memoryStorage(true));
  assert.doesNotThrow(() => refusing.save(ref(1), { activePath: null, tabs: [tab("text")] }));
  assert.equal(refusing.load(ref(1)), null);
  assert.equal(createEditorDraftStore(null).load(ref(1)), null);
  assert.equal(createEditorDraftStore(null).save(ref(1), { activePath: null, tabs: [tab("text")] }), false);
});

// PR #18 review B2: no kept draft is lost, by any path.
test("kept drafts survive other saves: an unreadable entry, a full store, a refusing write, another workspace of the same task", () => {
  const storage = memoryStorage();
  const store = createEditorDraftStore(storage);
  const ref = (index: number, workspace = `workspace_${index}`): ChatWorkspaceRefV1 => ({ chatId: `chat_${index}`, projectId: "project_01", worktreeId: `worktree_${index}`, workspaceIdentity: workspace });
  const tab = (text: string) => ({ path: "src/a.ts", text, baseVersion: version, byteLength: text.length });
  // An entry written by an older or other app version is kept as it was.
  const older = { workspace: { chatId: "chat_old" }, activePath: null, tabs: [], note: "older shape" };
  storage.items.set(EDITOR_DRAFTS_KEY, JSON.stringify({ version: 1, entries: [older, { workspace: ref(1), activePath: null, tabs: [tab("one")] }] }));
  assert.equal(store.save(ref(2), { activePath: null, tabs: [tab("two")] }), true);
  const raw = JSON.parse(storage.items.get(EDITOR_DRAFTS_KEY)!);
  assert.ok(raw.entries.some((item: unknown) => JSON.stringify(item) === JSON.stringify(older)), "the older entry is still there");
  assert.equal(store.load(ref(1))?.tabs[0]?.text, "one");
  // Another workspace of the same task does not replace it.
  assert.equal(store.save(ref(1, "workspace_1_new"), { activePath: null, tabs: [tab("one, new copy")] }), true);
  assert.equal(store.load(ref(1))?.tabs[0]?.text, "one");
  // Two drafts that together pass the size bound: the second is not kept, the first stays.
  const big = "y".repeat(EDITOR_DRAFTS_MAX_CHARS / 2 + 10);
  assert.equal(store.save(ref(3), { activePath: null, tabs: [tab(big)] }), true);
  assert.equal(store.save(ref(4), { activePath: null, tabs: [tab(big)] }), false);
  assert.equal(store.load(ref(3))?.tabs[0]?.text.length, big.length);
  assert.equal(store.load(ref(4)), null);
  // Storage that refuses writes keeps what it had.
  let refuse = false;
  const flaky = { getItem: (key: string) => storage.getItem(key), setItem: (key: string, value: string) => { if (refuse) throw new Error("QuotaExceededError"); storage.setItem(key, value); }, removeItem: (key: string) => { if (refuse) throw new Error("refused"); storage.removeItem(key); } };
  const flakyStore = createEditorDraftStore(flaky);
  refuse = true;
  assert.equal(flakyStore.save(ref(2), { activePath: null, tabs: [tab("two, changed")] }), false);
  assert.equal(flakyStore.save(ref(1), null), false);
  assert.equal(store.load(ref(2))?.tabs[0]?.text, "two");
  assert.equal(store.load(ref(1))?.tabs[0]?.text, "one");
});

test("files an agent writes show up in the change and file lists without a manual refresh", async () => {
  const bridge = new Bridge();
  const controller = new ProjectEditorController(bridge as unknown as ChatProjectBridge, { workspaceRefreshDelayMs: 20 });
  controller.start();
  await controller.chooseProject();
  assert.equal(await controller.bindChat(a), true);
  bridge.calls = [];
  const changed = (paths: string[]) => bridge.emit({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar", streamId: "watch_a", seq: 1,
    event: "workspace.changed", payload: { ...a, event: "file.changed", paths, rescanRequired: false } });
  // A burst of writes is reloaded once, after it settles.
  changed(["docs/plan.md"]);
  changed(["docs/plan.md", "index.html"]);
  await new Promise(resolve => setTimeout(resolve, 60));
  assert.deepEqual(bridge.calls.filter(call => call.startsWith("v3.diff:") || call.startsWith("v3.tree:")).sort(), ["v3.diff:chat_a", "v3.tree:chat_a"]);
  // While actions are blocked the reload waits, then happens once they resume.
  bridge.calls = [];
  controller.setActionBlocked(true);
  changed(["index.html"]);
  await new Promise(resolve => setTimeout(resolve, 60));
  assert.deepEqual(bridge.calls, []);
  controller.setActionBlocked(false);
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.deepEqual(bridge.calls.filter(call => call.startsWith("v3.diff:") || call.startsWith("v3.tree:")).sort(), ["v3.diff:chat_a", "v3.tree:chat_a"]);
  controller.dispose();
});

// ADE multi-project P1: switching projects keeps a task's unsaved text for it.
test("leaving a project keeps the open task's unsaved text and gives it back when the task opens again", async () => {
  const storage = memoryStorage();
  const { controller } = await opened(storage);
  await controller.openFile("src/a.ts");
  controller.updateText("src/a.ts", "edited while the team works\n");
  assert.equal(controller.canLeaveProject(), false);
  assert.equal(controller.canLeaveKeepingDrafts(), true);
  assert.equal(controller.leaveProjectView(), false, "plain leaving still refuses unsaved text");
  assert.equal(controller.leaveProjectView(false, true), true);
  assert.equal(controller.snapshot().project, null);
  assert.equal(createEditorDraftStore(storage).load(a)?.tabs[0]?.text, "edited while the team works\n");
  await controller.chooseProject();
  assert.equal(await controller.bindChat(a), true);
  assert.deepEqual(controller.snapshot().tabs.map(tab => [tab.path, tab.text, tab.saveState]), [["src/a.ts", "edited while the team works\n", "dirty"]]);
  controller.dispose();
});

test("text that cannot be kept holds the project: a store that refuses, a save on its way", async () => {
  const refusing = memoryStorage(true);
  const first = await opened(refusing);
  await first.controller.openFile("src/a.ts");
  first.controller.updateText("src/a.ts", "not kept\n");
  assert.equal(first.controller.canLeaveKeepingDrafts(), true, "it looks keepable");
  assert.equal(first.controller.leaveProjectView(false, true), false, "but the store did not take it, so the view stays");
  assert.equal(first.controller.snapshot().tabs[0]?.text, "not kept\n");
  first.controller.dispose();

  const saving = await opened(memoryStorage());
  await saving.controller.openFile("src/a.ts");
  saving.controller.updateText("src/a.ts", "saving\n");
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  const commit = saving.bridge.commitWorkspaceFileSave.bind(saving.bridge);
  saving.bridge.commitWorkspaceFileSave = async (input: Parameters<typeof commit>[0]) => { await held; return commit(input); };
  const pending = saving.controller.saveActive();
  for (let turn = 0; turn < 20 && saving.controller.snapshot().tabs[0]?.saveState !== "saving"; turn++) await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(saving.controller.snapshot().tabs[0]?.saveState, "saving");
  assert.equal(saving.controller.canLeaveKeepingDrafts(), false);
  assert.equal(saving.controller.leaveProjectView(false, true), false);
  release();
  await pending;
  saving.controller.dispose();
});

// PR #18 review B2·I1: switching never costs another task's kept draft, and
// a draft that cannot be kept holds the view before anything is let go.
test("leaving keeps another task's earlier draft; when the store is full the view stays and nothing kept is lost", async () => {
  const storage = memoryStorage();
  const store = createEditorDraftStore(storage);
  const filler = (index: number): ChatWorkspaceRefV1 => ({ chatId: `chat_f${index}`, projectId: "project_01", worktreeId: `worktree_f${index}`, workspaceIdentity: `workspace_f${index}` });
  // Task A left earlier with its edit kept.
  assert.equal(store.save(a, { activePath: "src/a.ts", tabs: [{ path: "src/a.ts", text: "A's edit\n", baseVersion: version, byteLength: 9 }] }), true);
  // The store fills up with other tasks' drafts.
  for (let index = 0; index < EDITOR_DRAFTS_MAX_WORKSPACES - 1; index++) assert.equal(store.save(filler(index), { activePath: null, tabs: [{ path: "f.ts", text: `f${index}`, baseVersion: version, byteLength: 2 }] }), true);
  // In task B, an edit that cannot be kept: the switch says no before letting go.
  const { controller } = await opened(storage, b);
  await controller.openFile("src/a.ts");
  controller.updateText("src/a.ts", "B's edit\n");
  controller.flushDrafts();
  assert.equal(controller.prepareLeaveKeepingDrafts(), false);
  assert.equal(controller.leaveProjectView(false, true), false);
  assert.equal(controller.snapshot().tabs[0]?.text, "B's edit\n", "B's edit stays on screen");
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(controller.snapshot().noticeKey, "m5.editorNotice.draftsNotKept");
  // A's kept edit is still there, and so is every other.
  assert.equal(createEditorDraftStore(storage).load(a)?.tabs[0]?.text, "A's edit\n");
  for (let index = 0; index < EDITOR_DRAFTS_MAX_WORKSPACES - 1; index++) assert.ok(createEditorDraftStore(storage).load(filler(index)), `filler ${index}`);
  controller.dispose();
});

// PR #18 re-review B2: a workspace's own earlier drafts (several files) are
// never cut down by a save that does not fit.
test("a save over the bound leaves this workspace's earlier drafts of several files whole", () => {
  const storage = memoryStorage();
  const store = createEditorDraftStore(storage);
  const tab = (path: string, text: string) => ({ path, text, baseVersion: version, byteLength: text.length });
  assert.equal(store.save(a, { activePath: "one.ts", tabs: [tab("one.ts", "first file\n"), tab("two.ts", "second file\n")] }), true);
  const before = storage.items.get(EDITOR_DRAFTS_KEY);
  // The same two files grown past the bound (together), and one alone past it.
  const half = "z".repeat(EDITOR_DRAFTS_MAX_CHARS / 2 + 1);
  assert.equal(store.save(a, { activePath: "one.ts", tabs: [tab("one.ts", half), tab("two.ts", half)] }), false);
  assert.equal(storage.items.get(EDITOR_DRAFTS_KEY), before, "nothing changed");
  assert.equal(store.save(a, { activePath: "one.ts", tabs: [tab("one.ts", "z".repeat(EDITOR_DRAFTS_MAX_CHARS + 1))] }), false);
  assert.deepEqual(store.load(a)?.tabs.map(item => [item.path, item.text]), [["one.ts", "first file\n"], ["two.ts", "second file\n"]]);
});

// The store's rule as a property (PR #18 re-review B2): over random orders of
// saves, forgets, leaves, bound overruns and refused writes, nothing kept is
// ever lost or cut down. A save either takes exactly what it was given or
// changes nothing at all.
test("property: whatever the order, a draft store only ever changes by a whole save or a forget", () => {
  let seed = 20261126;
  const random = () => { seed = (seed + 0x6d2b79f5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  const pick = <T,>(items: readonly T[]) => items[Math.floor(random() * items.length)]!;
  const workspaces = Array.from({ length: 12 }, (_, index): ChatWorkspaceRefV1 => ({ chatId: `chat_${index % 9}`, projectId: "project_01", worktreeId: `worktree_${index}`, workspaceIdentity: `workspace_${index}` }));
  const key = (ref: ChatWorkspaceRefV1) => `${ref.chatId}/${ref.worktreeId}/${ref.workspaceIdentity}`;
  const sizes = [1, 10, 500, 40_000, 300_000, 700_000, 1_100_000];
  for (let round = 0; round < 40; round++) {
    const backing = memoryStorage();
    let refuse = false;
    const storage = { getItem: (k: string) => backing.getItem(k), setItem: (k: string, v: string) => { if (refuse) throw new Error("QuotaExceededError"); backing.setItem(k, v); }, removeItem: (k: string) => { if (refuse) throw new Error("refused"); backing.removeItem(k); } };
    const store = createEditorDraftStore(storage);
    const model = new Map<string, { workspace: ChatWorkspaceRefV1; texts: [string, string][] }>();
    for (let step = 0; step < 80; step++) {
      const before = backing.items.get(EDITOR_DRAFTS_KEY);
      const workspace = pick(workspaces);
      const roll = random();
      if (roll < 0.1) { refuse = !refuse; continue; }
      if (roll < 0.25) {
        // Forget (a tab saved or closed, a task deleted).
        const done = store.save(workspace, null);
        if (done) model.delete(key(workspace)); else assert.equal(backing.items.get(EDITOR_DRAFTS_KEY), before, "a refused forget changes nothing");
      } else {
        // Save, or a leave that keeps the view's drafts (the same call).
        const count = 1 + Math.floor(random() * 3);
        const texts: [string, string][] = Array.from({ length: count }, (_, index) => [`f${index}.ts`, `${round}:${step}:${index}:` + "t".repeat(pick(sizes))]);
        const done = store.save(workspace, { activePath: null, tabs: texts.map(([path, text]) => ({ path, text, baseVersion: version, byteLength: text.length })) });
        if (done) model.set(key(workspace), { workspace, texts });
        else assert.equal(backing.items.get(EDITOR_DRAFTS_KEY), before, `round ${round} step ${step}: a save that did not fit or was refused changed the store`);
      }
      // Everything kept is exactly what was last kept; nothing else is there.
      for (const ref of workspaces) {
        const expected = model.get(key(ref));
        const loaded = store.load(ref);
        assert.deepEqual(loaded ? loaded.tabs.map(tab => [tab.path, tab.text]) : null, expected ? expected.texts : null, `round ${round} step ${step} ${key(ref)}`);
      }
      const total = [...model.values()].reduce((sum, item) => sum + item.texts.reduce((n, [, text]) => n + text.length, 0), 0);
      assert.ok(model.size <= EDITOR_DRAFTS_MAX_WORKSPACES && total <= EDITOR_DRAFTS_MAX_CHARS);
    }
  }
});
