import assert from "node:assert/strict";
import test from "node:test";

import { ChatUiController } from "../src/m5/chat-state.ts";
import type { M5BridgeV3 } from "../src/m5/contracts/bridge-v3.ts";
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
