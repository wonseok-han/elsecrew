import assert from "node:assert/strict";
import test from "node:test";

import { ProjectEditorController, type ChatProjectBridge } from "../src/m5/editor-state.ts";
import type { ChatWorkspaceRefV1, WorkspaceEventV1 } from "../src/m5/contracts/workspace-addressing-v1.ts";
import { sha256FileVersionV1 } from "../src/m5/contracts/file-v1.ts";

const version = sha256FileVersionV1(new TextEncoder().encode("one\n"));
const a: ChatWorkspaceRefV1 = { chatId: "chat_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a" };
const b: ChatWorkspaceRefV1 = { chatId: "chat_b", projectId: "project_01", worktreeId: "worktree_b", workspaceIdentity: "workspace_b" };

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
