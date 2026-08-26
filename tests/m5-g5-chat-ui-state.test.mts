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
  readonly created: Array<{ worktreeId: string; workspaceIdentity: string; revision: number }> = [];
  revision = 1;
  chats: ChatRecordV1[] = [chat("chat_a"), chat("chat_b")];
  async getProjectRoleSettings() { return { ...settings(), revision: this.revision }; }
  async updateProjectRoleSettings(input: { expectedRevision: number; roles: readonly RoleProfileV1[] }) { if (input.expectedRevision !== this.revision) throw new Error("revision conflict"); this.revision++; return { ...settings(), revision: this.revision, roles: input.roles }; }
  async listChats() { return { projectId: "project_a", items: this.chats, nextCursor: null }; }
  async createProjectWorktree() { const number = this.created.length + 1; return { projectId: "project_a", worktreeId: `fresh_${number}`, workspaceIdentity: `fresh_identity_${number}`, workspaceKind: "standaloneSnapshot", lifecycle: "active" }; }
  async createChat(input: { worktreeId: string; workspaceIdentity: string; expectedSettingsRevision: number; goal: string }) { this.created.push({ worktreeId: input.worktreeId, workspaceIdentity: input.workspaceIdentity, revision: input.expectedSettingsRevision }); const next = { ...chat(`chat_new_${this.created.length}`, input.worktreeId), workspaceIdentity: input.workspaceIdentity, goal: input.goal }; this.chats = [next, ...this.chats]; return next; }
  async getChat({ chatId }: { chatId: string }) { const current = this.chats.find((item) => item.chatId === chatId)!; return { chat: current, afterEventSequence: 0, messages: [], runs: [], nextEventSequence: null }; }
  async listRecoveries() { return { items: [], nextCursor: null }; }
  onChatEvent(listener: (event: TeamMessageV1) => void) { this.listeners.add(listener); return () => this.listeners.delete(listener); }
  emit(event: TeamMessageV1) { for (const listener of this.listeners) listener(event); }
}

test("새 채팅은 매번 새 standalone snapshot triple과 당시 settings revision을 사용한다", async () => {
  const bridge = new FakeBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV3); controller.start();
  await controller.openProject("project_a");
  await controller.createChat({ projectId: "project_a" }, "첫 목표");
  await controller.createChat({ projectId: "project_a" }, "둘째 목표");
  assert.deepEqual(bridge.created.map((item) => item.worktreeId), ["fresh_1", "fresh_2"]);
  assert.equal(new Set(bridge.created.map((item) => item.workspaceIdentity)).size, 2);
  assert.deepEqual(bridge.created.map((item) => item.revision), [1, 1]);
  controller.dispose();
});

test("A 채팅 이벤트는 A triple에서만 보이고 B와 섞이지 않으며 구독은 정리된다", async () => {
  const bridge = new FakeBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV3); controller.start(); await controller.openProject("project_a"); await controller.switchChat("chat_a", true);
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
