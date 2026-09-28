import assert from "node:assert/strict";
import test from "node:test";

import { ChatUiController } from "../src/m5/chat-state.ts";
import type { M5BridgeV3 } from "../src/m5/contracts/bridge-v3.ts";
import type { M5BridgeV7 } from "../src/m5/contracts/bridge-v7.ts";
import type { ChatAppendMessageRequestV1 } from "../src/m5/contracts/run-recovery-v1.ts";
import type { ChatRecordV1, ProjectRoleSettingsV1, RoleProfileV1, TeamMessageV1 } from "../src/m5/contracts/run-recovery-v1.ts";

const roles: readonly RoleProfileV1[] = [
  { roleId: "planning", runtime: "codex", modelSelector: "codex-sol-medium", reasoningEffort: "medium", permission: "readOnly" },
  { roleId: "coding", runtime: "claude-code", modelSelector: "claude-opus-5", reasoningEffort: null, permission: "workspaceWrite" },
  { roleId: "review", runtime: "codex", modelSelector: "codex-sol-medium", reasoningEffort: null, permission: "readOnly" },
];
const settings = (): ProjectRoleSettingsV1 => ({ schemaVersion: 1, projectId: "project_a", revision: 1, roles, updatedAt: "2026-08-26T00:00:00Z" });
const chat = (id: string, worktreeId = `worktree_${id}`): ChatRecordV1 => ({ schemaVersion: 1, chatId: id, taskId: `task_${id}`, projectId: "project_a", worktreeId, workspaceIdentity: `workspace_${id}`, goal: id, settingsRevision: 1, roleSnapshots: roles, state: "draft", createdAt: "2026-08-26T00:00:00Z", updatedAt: "2026-08-26T00:00:00Z" });

class FakeBridge {
  readonly listeners = new Set<(event: TeamMessageV1) => void>();
  readonly created: Array<{ preparationId: string; worktreeId: string; workspaceIdentity: string; revision: number }> = [];
  revision = 1;
  failCreate = false;
  readonly prepared: Array<{ preparationId: string; worktreeId: string; workspaceIdentity: string }> = [];
  readonly abandoned: Array<{ preparationId: string; projectId: string; worktreeId: string; workspaceIdentity: string }> = [];
  chats: ChatRecordV1[] = [chat("chat_a"), chat("chat_b")];
  async getProjectRoleSettings() { return { ...settings(), revision: this.revision }; }
  async updateProjectRoleSettings(input: { expectedRevision: number; roles: readonly RoleProfileV1[] }) { if (input.expectedRevision !== this.revision) throw new Error("revision conflict"); this.revision++; return { ...settings(), revision: this.revision, roles: input.roles }; }
  async listChats() { return { projectId: "project_a", items: this.chats, nextCursor: null }; }
  async prepareChatWorkspace() { const number = this.prepared.length + 1; const prepared = { preparationId: `prep_${number}`, worktreeId: `fresh_${number}`, workspaceIdentity: `fresh_identity_${number}` }; this.prepared.push(prepared); return { schemaVersion: 1 as const, preparationId: prepared.preparationId, manifest: { schemaVersion: 1 as const, projectId: "project_a", canonicalGitRoot: "/safe/project", worktreeId: prepared.worktreeId, worktreePath: `/private/${prepared.worktreeId}`, workspaceKind: "standaloneSnapshot" as const, snapshotTransport: "gitBundle" as const, sourceMutationPolicy: "forbidden" as const, objectFormat: "sha1" as const, baseCommit: "a".repeat(40), baseTree: "b".repeat(40), sourceIdentity: "source", workspaceIdentity: prepared.workspaceIdentity, headMode: "detached" as const, lifecycle: "active" as const }, bindingState: "unbound" as const, boundChatId: null, createdAt: "2026-08-26T00:00:00Z", updatedAt: "2026-08-26T00:00:00Z" }; }
  async abandonPreparedWorkspace(input: { preparationId: string; projectId: string; worktreeId: string; workspaceIdentity: string }) { this.abandoned.push(input); return { ...input, bindingState: "abandoned" as const, physicallyDeleted: false as const }; }
  async createChat(input: { preparationId: string; worktreeId: string; workspaceIdentity: string; expectedSettingsRevision: number; goal: string }) { if (this.failCreate) throw new Error("create failed"); this.created.push({ preparationId: input.preparationId, worktreeId: input.worktreeId, workspaceIdentity: input.workspaceIdentity, revision: input.expectedSettingsRevision }); const next = { ...chat(`chat_new_${this.created.length}`, input.worktreeId), workspaceIdentity: input.workspaceIdentity, goal: input.goal }; this.chats = [next, ...this.chats]; return next; }
  async getChat({ chatId }: { chatId: string }) { const current = this.chats.find((item) => item.chatId === chatId)!; return { chat: current, afterEventSequence: 0, messages: [], runs: [], nextEventSequence: null }; }
  async listRecoveries() { return { items: [], nextCursor: null }; }
  onChatEvent(listener: (event: TeamMessageV1) => void) { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  emit(event: TeamMessageV1) { for (const listener of this.listeners) listener(event); }
}

async function followupFixture() {
  const calls: ChatAppendMessageRequestV1[] = [];
  let complete!: (message: TeamMessageV1) => void;
  let fail!: (error: Error) => void;
  const bridge = Object.assign(new FakeBridge(), { version: 7 as const,
    appendChatMessage(request: ChatAppendMessageRequestV1) {
      calls.push(request);
      return new Promise<TeamMessageV1>((resolve, reject) => { complete = resolve; fail = reject; });
    },
  });
  const controller = new ChatUiController(bridge as unknown as M5BridgeV7);
  controller.start();
  await controller.openProject("project_a");
  await controller.prepareSwitch("chat_a", true);
  await controller.commitPrepared("chat_a", true);
  bridge.emit({ schemaVersion: 1, messageId: "initial", chatId: "chat_a", taskId: "task_chat_a", worktreeId: "worktree_chat_a", eventSequence: 1, kind: "user", roleId: null, runId: null, text: "first", createdAt: "2026-09-14T00:00:00Z" });
  return { controller, bridge, calls, fail: (error: Error) => fail(error), complete: () => {
    const request = calls[calls.length - 1]!;
    const message: TeamMessageV1 = { schemaVersion: 1, messageId: request.clientMessageId, chatId: request.chatId, taskId: request.taskId, worktreeId: request.worktreeId, eventSequence: request.expectedEventSequence + 1, kind: "user", roleId: null, runId: null, text: request.text, createdAt: "2026-09-14T00:00:01Z" };
    complete(message);
    return message;
  } };
}

test("follow-up preserves newer input and deduplicates the response/event pair", async () => {
  const f = await followupFixture();
  f.controller.editFollowup("send this");
  await f.controller.appendFollowup(false);
  assert.equal(f.calls.length, 0);
  const pending = f.controller.appendFollowup(true);
  await f.controller.appendFollowup(true);
  assert.equal(f.calls.length, 1);
  f.controller.editFollowup("newer draft");
  const message = f.complete();
  f.bridge.emit(message);
  await pending;
  assert.equal(f.controller.snapshot().messages.length, 2);
  assert.equal(f.controller.snapshot().followups?.chat_a?.text, "newer draft");
  assert.equal(f.controller.snapshot().followups?.chat_a?.request, null);
});

test("follow-up retries the exact unknown-outcome request and isolates a switched chat", async () => {
  const f = await followupFixture();
  f.controller.editFollowup("original");
  const first = f.controller.appendFollowup(true);
  f.fail(new Error("connection lost"));
  await first;
  f.controller.editFollowup("additional draft");
  const retry = f.controller.appendFollowup(true);
  assert.strictEqual(f.calls[1], f.calls[0]);
  await f.controller.prepareSwitch("chat_b", true);
  await f.controller.commitPrepared("chat_b", true);
  f.controller.editFollowup("chat b input");
  f.complete();
  await retry;
  assert.equal(f.controller.snapshot().activeChat?.chatId, "chat_b");
  assert.equal(f.controller.snapshot().messages.length, 0);
  assert.equal(f.controller.snapshot().followups?.chat_b?.text, "chat b input");
  assert.equal(f.controller.snapshot().followups?.chat_a?.text, "additional draft");
  assert.equal(f.controller.snapshot().followups?.chat_a?.sending, false);
});

test("follow-up clears unchanged input only after success and blocks invalid text or recovery", async () => {
  const f = await followupFixture();
  f.controller.editFollowup("가".repeat(6000));
  await f.controller.appendFollowup(true);
  assert.equal(f.calls.length, 0);
  assert.ok(f.controller.snapshot().followups?.chat_a?.error);
  f.controller.editFollowup("valid");
  const pending = f.controller.appendFollowup(true);
  f.complete();
  await pending;
  assert.equal(f.controller.snapshot().followups?.chat_a?.text, "");
  f.bridge.chats[1] = { ...f.bridge.chats[1]!, state: "recoveryRequired" };
  await f.controller.prepareSwitch("chat_b", true);
  await f.controller.commitPrepared("chat_b", true);
  f.controller.editFollowup("blocked");
  await f.controller.appendFollowup(true);
  assert.equal(f.calls.length, 1);
});

test("explicit stale-sequence rejection refreshes and rebases the same message ID", async () => {
  const f = await followupFixture();
  f.controller.editFollowup("follow up");
  const pending = f.controller.appendFollowup(true);
  const initial = f.controller.snapshot().messages[0]!;
  f.bridge.getChat = async () => ({ chat: chat("chat_a"), afterEventSequence: 1, messages: [{ ...initial, messageId: "concurrent", eventSequence: 2, text: "new event" }], runs: [], nextEventSequence: null });
  f.fail(new Error("dirtyWorktree: chat message sequence changed before append"));
  await pending;
  const retry = f.controller.appendFollowup(true);
  assert.equal(f.calls[1]?.clientMessageId, f.calls[0]?.clientMessageId);
  assert.equal(f.calls[1]?.expectedEventSequence, 2);
  f.complete();
  await retry;
  assert.equal(f.controller.snapshot().messages.length, 3);
});

test("chat pagination preserves existing items, deduplicates overlap and retries failures", async () => {
  let calls = 0, fail = true;
  const bridge = Object.assign(new FakeBridge(), {
    async listChats({ cursor }: { cursor: string | null }) {
      calls++;
      if (cursor === null) return { projectId: "project_a", items: [chat("chat_a")], nextCursor: "offset_100" };
      assert.equal(cursor, "offset_100");
      if (fail) throw new Error("temporary read failure");
      return { projectId: "project_a", items: [chat("chat_a"), chat("chat_more"), chat("chat_more")], nextCursor: null };
    },
  });
  const controller = new ChatUiController(bridge as unknown as M5BridgeV3);
  await controller.openProject("project_a");
  await controller.loadMoreChats();
  assert.equal(controller.snapshot().chats.length, 1);
  assert.equal(controller.snapshot().nextChatCursor, "offset_100");
  assert.equal(controller.snapshot().loadingMoreChats, false);
  assert.ok(controller.snapshot().error);
  fail = false;
  await controller.loadMoreChats();
  assert.deepEqual(controller.snapshot().chats.map(item => item.chatId), ["chat_a", "chat_more"]);
  assert.equal(controller.snapshot().nextChatCursor, null);
  await controller.loadMoreChats();
  assert.equal(calls, 3);
});

test("chat pagination coalesces clicks and ignores old project responses", async () => {
  let complete!: (page: { projectId: string; items: ChatRecordV1[]; nextCursor: string | null }) => void;
  let requests = 0;
  const bridge = Object.assign(new FakeBridge(), {
    async listChats({ projectId, cursor }: { projectId: string; cursor: string | null }) {
      if (cursor === null) return { projectId, items: [], nextCursor: "offset_100" };
      requests++;
      return new Promise<{ projectId: string; items: ChatRecordV1[]; nextCursor: string | null }>(resolve => { complete = resolve; });
    },
  });
  const controller = new ChatUiController(bridge as unknown as M5BridgeV3);
  await controller.openProject("project_a");
  const loading = controller.loadMoreChats();
  await controller.loadMoreChats();
  assert.equal(requests, 1);
  await controller.openProject("project_b");
  complete({ projectId: "project_a", items: [chat("old")], nextCursor: null });
  await loading;
  assert.equal(controller.snapshot().projectId, "project_b");
  assert.equal(controller.snapshot().nextChatCursor, "offset_100");
  assert.deepEqual(controller.snapshot().chats, []);
  assert.equal(controller.snapshot().loadingMoreChats, false);
});

for (const rejected of [false, true]) test(`프로젝트 전환 후 이전 설정 저장 ${rejected ? "실패" : "성공"}은 현재 프로젝트를 바꾸지 않는다`, async () => {
  let complete!: (value: ProjectRoleSettingsV1) => void;
  let fail!: (error: Error) => void;
  const pending = new Promise<ProjectRoleSettingsV1>((resolve, reject) => { complete = resolve; fail = reject; });
  const bridge = new FakeBridge();
  bridge.updateProjectRoleSettings = async () => pending;
  const controller = new ChatUiController(bridge as unknown as M5BridgeV3);
  await controller.openProject("project_a");
  const saving = controller.updateSettings(roles);
  bridge.getProjectRoleSettings = async () => ({ ...settings(), projectId: "project_b" });
  await controller.openProject("project_b");
  if (rejected) fail(new Error("late failure")); else complete({ ...settings(), revision: 2 });
  await saving;
  assert.equal(controller.snapshot().projectId, "project_b");
  assert.equal(controller.snapshot().settings?.projectId, "project_b");
  assert.equal(controller.snapshot().error, null);
  controller.dispose();
});

test("settings save failure and explicit reload preserve the active chat and settings draft source", async () => {
  const bridge = new FakeBridge();
  const controller = new ChatUiController(bridge as unknown as M5BridgeV3);
  await controller.openProject("project_a");
  await controller.prepareSwitch("chat_a", true);
  await controller.commitPrepared("chat_a", true);
  const before = controller.snapshot();
  assert.equal(before.activeChat?.chatId, "chat_a");
  bridge.updateProjectRoleSettings = async () => { throw new Error("offline"); };
  await controller.updateSettings(roles);
  assert.equal(controller.snapshot().settings, before.settings);
  assert.equal(controller.snapshot().activeChat, before.activeChat);
  assert.equal(controller.snapshot().chats, before.chats);
  bridge.revision = 2;
  await controller.reloadSettings();
  assert.equal(controller.snapshot().settings?.revision, 2);
  assert.equal(controller.snapshot().activeChat, before.activeChat);
  assert.equal(controller.snapshot().chats, before.chats);
  const loaded = controller.snapshot().settings;
  bridge.getProjectRoleSettings = async () => { throw new Error("offline"); };
  await controller.reloadSettings();
  assert.equal(controller.snapshot().settings, loaded);
  controller.dispose();
});

test("late settings reload cannot overwrite a newer successful save", async () => {
  const bridge = new FakeBridge();
  const controller = new ChatUiController(bridge as unknown as M5BridgeV3);
  await controller.openProject("project_a");
  let complete!: (value: ProjectRoleSettingsV1) => void;
  bridge.getProjectRoleSettings = () => new Promise(resolve => { complete = resolve; });
  const reload = controller.reloadSettings();
  await controller.updateSettings(roles);
  complete(settings());
  await reload;
  assert.equal(controller.snapshot().settings?.revision, 2);
  controller.dispose();
});

test("새 프로젝트 설정을 불러오는 동안 이전 프로젝트 설정을 표시하지 않는다", async () => {
  const bridge = new FakeBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV3);
  await controller.openProject("project_a");
  let complete!: (value: ProjectRoleSettingsV1) => void;
  bridge.getProjectRoleSettings = () => new Promise(resolve => { complete = resolve; });
  const opening = controller.openProject("project_b");
  assert.equal(controller.snapshot().settings, null);
  complete({ ...settings(), projectId: "project_b" }); await opening;
  assert.equal(controller.snapshot().settings?.projectId, "project_b");
  controller.dispose();
});

test("새 채팅은 prepareChatWorkspace의 서로 다른 identity와 exact preparationId를 사용한다", async () => {
  const bridge = new FakeBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV3); controller.start();
  await controller.openProject("project_a");
  await controller.createChat({ projectId: "project_a" }, "첫 목표");
  await controller.createChat({ projectId: "project_a" }, "둘째 목표");
  assert.deepEqual(bridge.created.map((item) => item.worktreeId), ["fresh_1", "fresh_2"]);
  assert.deepEqual(bridge.created.map((item) => item.preparationId), ["prep_1", "prep_2"]);
  assert.equal(new Set(bridge.created.map((item) => item.workspaceIdentity)).size, 2);
  assert.equal(bridge.abandoned.length, 0);
  controller.dispose();
});

test("B chat.create 실패는 B preparation만 exact abandon하고 기존 A 기록은 그대로 둔다", async () => {
  const bridge = new FakeBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV3); await controller.openProject("project_a");
  await controller.createChat({ projectId: "project_a" }, "A"); const a = controller.snapshot().chats[0]!;
  bridge.failCreate = true; await controller.createChat({ projectId: "project_a" }, "B");
  assert.equal(controller.snapshot().chats.some((item) => item.chatId === a.chatId), true);
  assert.deepEqual(bridge.abandoned, [{ preparationId: "prep_2", projectId: "project_a", worktreeId: "fresh_2", workspaceIdentity: "fresh_identity_2" }]);
  controller.dispose();
});

test("A 채팅 이벤트는 A triple에서만 보이고 B와 섞이지 않으며 구독은 정리된다", async () => {
  const bridge = new FakeBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV3); controller.start(); await controller.openProject("project_a"); const prepared = await controller.prepareSwitch("chat_a", true); await controller.commitPrepared(prepared!.chatId, true);
  bridge.emit({ schemaVersion: 1, messageId: "m_b", chatId: "chat_b", taskId: "task_chat_b", worktreeId: "worktree_chat_b", eventSequence: 1, kind: "status", roleId: null, runId: null, text: "B", createdAt: "2026-08-26T00:00:00Z" });
  bridge.emit({ schemaVersion: 1, messageId: "m_a", chatId: "chat_a", taskId: "task_chat_a", worktreeId: "worktree_chat_a", eventSequence: 1, kind: "status", roleId: null, runId: null, text: "A", createdAt: "2026-08-26T00:00:00Z" });
  assert.deepEqual(controller.snapshot().messages.map((item) => item.text), ["A"]);
  controller.dispose(); assert.equal(bridge.listeners.size, 0);
});

test("설정 변경은 새 revision만 바꾸고 기존 chat role snapshot은 변경하지 않는다", async () => {
  const bridge = new FakeBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV3); await controller.openProject("project_a"); const before = controller.snapshot().chats[0]!.roleSnapshots;
  await controller.updateSettings(roles.map((role) => role.roleId === "planning" ? { ...role, modelSelector: "codex-sol-high" } : role));
  assert.equal(controller.snapshot().settings?.revision, 2); assert.deepEqual(controller.snapshot().chats[0]!.roleSnapshots, before);
  controller.dispose();
});
