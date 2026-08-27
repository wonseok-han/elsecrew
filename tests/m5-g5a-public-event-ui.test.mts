import assert from "node:assert/strict";
import test from "node:test";

import { ChatUiController } from "../src/m5/chat-state.ts";
import type { M5BridgeV4 } from "../src/m5/contracts/bridge-v4.ts";
import type { AgentPublicEventV1, AgentRunRecordV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import type { ChatRecordV1, ProjectRoleSettingsV1, RoleProfileV1, TeamMessageV1 } from "../src/m5/contracts/run-recovery-v1.ts";

const fingerprint = `sha256:${"a".repeat(64)}`;
const roles: readonly RoleProfileV1[] = [
  { roleId: "planning", runtime: "codex", modelSelector: "codex-sol-medium", reasoningEffort: "medium", permission: "readOnly" },
  { roleId: "coding", runtime: "claude-code", modelSelector: "claude-opus-5", reasoningEffort: null, permission: "workspaceWrite" },
  { roleId: "review", runtime: "codex", modelSelector: "codex-sol-medium", reasoningEffort: null, permission: "readOnly" },
];

function chat(id = "chat_a"): ChatRecordV1 {
  return { schemaVersion: 1, chatId: id, taskId: `task_${id}`, projectId: "project_a", worktreeId: `worktree_${id}`, workspaceIdentity: `workspace_${id}`, goal: id, settingsRevision: 1, roleSnapshots: roles, state: "draft", createdAt: "2026-08-27T00:00:00Z", updatedAt: "2026-08-27T00:00:00Z" };
}
function message(record: ChatRecordV1, sequence: number, text: string): TeamMessageV1 {
  return { schemaVersion: 1, messageId: `message_${record.chatId}_${sequence}`, chatId: record.chatId, taskId: record.taskId, worktreeId: record.worktreeId, eventSequence: sequence, kind: sequence === 1 ? "user" : "role", roleId: sequence === 1 ? null : "coding", runId: sequence === 1 ? null : "agent_run_01", text, createdAt: "2026-08-27T00:00:00Z" };
}
function agentRun(record: ChatRecordV1, state: AgentRunRecordV1["state"] = "running"): AgentRunRecordV1 {
  return { schemaVersion: 1, runId: "agent_run_01", chatId: record.chatId, taskId: record.taskId, projectId: record.projectId, worktreeId: record.worktreeId, workspaceIdentity: record.workspaceIdentity, selectedProfile: roles[1]!, source: "actualAgent", capabilityFingerprint: fingerprint, state, stage: "implementing", terminationOutcome: state === "recoveryRequired" ? "recoveryRequired" : ["preparing", "running", "cancelling"].includes(state) ? null : "cleanExit", createdAt: "2026-08-27T00:00:00Z", updatedAt: "2026-08-27T00:00:01Z" };
}
function publicEvent(record: ChatRecordV1, text = "raw text must never render"): AgentPublicEventV1 {
  return { schemaVersion: 1, eventId: "agent_event_01", chatId: record.chatId, taskId: record.taskId, projectId: record.projectId, worktreeId: record.worktreeId, workspaceIdentity: record.workspaceIdentity, runId: "agent_run_01", roleId: "coding", capabilityFingerprint: fingerprint, kind: "final", text, createdAt: "2026-08-27T00:00:01Z" };
}

class AgentBridge {
  readonly chatListeners = new Set<(event: TeamMessageV1) => void>();
  readonly agentListeners = new Set<(event: AgentPublicEventV1) => void>();
  readonly chatA = chat("chat_a");
  readonly chatB = chat("chat_b");
  readonly calls: number[] = [];
  readonly starts: unknown[] = [];
  readonly fixtureStarts: unknown[] = [];
  readonly cancels: unknown[] = [];
  messages = [message(this.chatA, 1, "durable goal")];
  agentRuns: AgentRunRecordV1[] = [];
  unavailable = false;

  async getProjectRoleSettings(): Promise<ProjectRoleSettingsV1> { return { schemaVersion: 1, projectId: "project_a", revision: 1, roles, updatedAt: "2026-08-27T00:00:00Z" }; }
  async listChats() { return { projectId: "project_a", items: [this.chatA, this.chatB], nextCursor: null }; }
  async getChat(input: { chatId: string; afterEventSequence: number }) {
    this.calls.push(input.afterEventSequence);
    const current = input.chatId === this.chatB.chatId ? this.chatB : this.chatA;
    const messages = input.chatId === this.chatB.chatId ? [] : this.messages.filter((entry) => entry.eventSequence > input.afterEventSequence).slice(0, 50);
    return { chat: current, afterEventSequence: input.afterEventSequence, messages, runs: [], nextEventSequence: messages.length === 0 ? null : messages[messages.length - 1]!.eventSequence, agentRuns: input.chatId === this.chatB.chatId ? [] : this.agentRuns };
  }
  async listRecoveries() { return { items: [], nextCursor: null }; }
  onChatEvent(listener: (event: TeamMessageV1) => void) { this.chatListeners.add(listener); return () => this.chatListeners.delete(listener); }
  onAgentPublicEvent(listener: (event: AgentPublicEventV1) => void) { this.agentListeners.add(listener); return () => this.agentListeners.delete(listener); }
  async getAgentCapabilities() {
    return { canStart: !this.unavailable, blockedReason: this.unavailable ? "notAuthenticated" : null, fingerprint };
  }
  async startAgentRun(input: unknown) {
    this.starts.push(input);
    this.agentRuns = [agentRun(this.chatA, "preparing")];
    return { schemaVersion: 1, runId: "agent_run_01", chatId: this.chatA.chatId, taskId: this.chatA.taskId, projectId: this.chatA.projectId, worktreeId: this.chatA.worktreeId, workspaceIdentity: this.chatA.workspaceIdentity, roleId: "coding", capabilityFingerprint: fingerprint, state: "preparing" };
  }
  async startFixtureRun(input: unknown) {
    this.fixtureStarts.push(input);
    return { runId: "run_fixture_01" };
  }
  async cancelAgentRun(input: unknown) {
    this.cancels.push(input);
    this.agentRuns = [agentRun(this.chatA, "recoveryRequired")];
    const run = this.agentRuns[0]!;
    return { outcome: "recoveryRequired", recovery: { recoveryId: "agent_recovery_01", ...run, source: "actualAgent" } };
  }
  emitAgent(event: AgentPublicEventV1) { for (const listener of this.agentListeners) listener(event); }
}

async function open(bridge: AgentBridge): Promise<ChatUiController> {
  const controller = new ChatUiController(bridge as unknown as M5BridgeV4);
  controller.start();
  await controller.openProject("project_a");
  const pending = await controller.prepareSwitch("chat_a", true);
  await controller.commitPrepared(pending!.chatId, true);
  return controller;
}
async function flush(): Promise<void> { await new Promise<void>((resolve) => setTimeout(resolve, 0)); }

test("actual public event는 원문을 붙이지 않고 durable chat.get 페이지와 actual ledger만 반영한다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.messages.push(message(bridge.chatA, 2, "durable result")); bridge.agentRuns = [agentRun(bridge.chatA, "running")];
  bridge.emitAgent(publicEvent(bridge.chatA)); await flush(); await flush();
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.text), ["durable goal", "durable result"]);
  assert.equal(controller.snapshot().messages.some((entry) => entry.text.includes("raw text")), false);
  assert.equal(controller.snapshot().agentRuns[0]?.state, "running", "final public text does not claim process termination");
  controller.dispose();
});

test("public event burst는 하나의 hydration으로 합치고 durable sequence를 끝까지 연속 조회한다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.messages.push(message(bridge.chatA, 2, "second"), message(bridge.chatA, 3, "third"));
  bridge.calls.length = 0;
  bridge.emitAgent(publicEvent(bridge.chatA)); bridge.emitAgent(publicEvent(bridge.chatA)); await flush(); await flush();
  assert.deepEqual(bridge.calls, [1, 3]);
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.eventSequence), [1, 2, 3]);
  controller.dispose();
});

test("pending 및 inactive actual event는 텍스트를 보이지 않고 해당 채팅의 hydration 또는 refresh notice만 남긴다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.emitAgent(publicEvent(bridge.chatB, "inactive raw")); await flush();
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.text), ["durable goal"]);
  assert.deepEqual(controller.snapshot().refreshNeededChatIds, ["chat_b"]);
  controller.dispose();
});

test("switch pending 중 public event도 durable hydration에 합쳐지고 raw text를 pending 메시지로 넣지 않는다", async () => {
  const bridge = new AgentBridge(); const controller = new ChatUiController(bridge as unknown as M5BridgeV4); controller.start(); await controller.openProject("project_a");
  let release: (() => void) | undefined;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const original = bridge.getChat.bind(bridge); let first = true;
  bridge.getChat = async (input) => { if (first) { first = false; await gate; } return original(input); };
  bridge.messages.push(message(bridge.chatA, 2, "pending durable result"));
  const switching = controller.prepareSwitch("chat_a", true); await flush(); bridge.emitAgent(publicEvent(bridge.chatA, "pending raw")); release!();
  const pending = await switching; await controller.commitPrepared(pending!.chatId, true);
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.text), ["durable goal", "pending durable result"]);
  assert.equal(controller.snapshot().messages.some((entry) => entry.text === "pending raw"), false);
  controller.dispose();
});

test("current G5 bridge는 agent listener 없이 legacy fixture chat을 계속 연다", async () => {
  const bridge = new AgentBridge();
  const g5 = bridge as unknown as Record<string, unknown>;
  g5.onAgentPublicEvent = undefined; g5.getAgentCapabilities = undefined; g5.startAgentRun = undefined; g5.cancelAgentRun = undefined;
  const controller = new ChatUiController(g5 as unknown as M5BridgeV4);
  controller.start(); await controller.openProject("project_a"); const pending = await controller.prepareSwitch("chat_a", true); await controller.commitPrepared(pending!.chatId, true);
  assert.equal(controller.snapshot().agentRuns.length, 0);
  await controller.startAgent("coding");
  assert.match(controller.snapshot().notice ?? "", /실제 에이전트 실행/);
  controller.dispose();
});

test("unavailable capability는 fallback 없이 막고 start는 immutable role snapshot, durable IDs, exact fingerprint만 보낸다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.unavailable = true; await controller.startAgent("coding");
  assert.equal(bridge.starts.length, 0); assert.match(controller.snapshot().notice ?? "", /자동 변경하지 않았어요/);
  bridge.unavailable = false; await controller.startAgent("coding");
  assert.deepEqual(bridge.starts, [{ chatId: "chat_a", roleId: "coding", inputMessageIds: ["message_chat_a_1"], expectedCapabilityFingerprint: fingerprint }]);
  assert.equal(controller.snapshot().agentRuns[0]?.selectedProfile.runtime, "claude-code");
  controller.dispose();
});

test("actual cancel은 full identity만 보내고 recoveryRequired를 legacy abandon으로 바꾸지 않으며 dispose가 두 listener를 정리한다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.agentRuns = [agentRun(bridge.chatA)]; bridge.messages.push(message(bridge.chatA, 2, "ready")); bridge.emitAgent(publicEvent(bridge.chatA)); await flush();
  await controller.cancelAgentRun(controller.snapshot().agentRuns[0]!);
  assert.deepEqual(bridge.cancels, [{ chatId: "chat_a", taskId: "task_chat_a", projectId: "project_a", worktreeId: "worktree_chat_a", workspaceIdentity: "workspace_chat_a", runId: "agent_run_01" }]);
  assert.equal(controller.snapshot().agentRecovery?.recoveryId, "agent_recovery_01");
  assert.match(controller.snapshot().notice ?? "", /복구가 필요/);
  controller.dispose();
  assert.equal(bridge.chatListeners.size, 0); assert.equal(bridge.agentListeners.size, 0);
});

test("actual cancel recoveryRequired는 durable hydration 대기 중에도 새 fixture·actual 시작을 즉시 막는다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.agentRuns = [agentRun(bridge.chatA)]; bridge.messages.push(message(bridge.chatA, 2, "running")); bridge.emitAgent(publicEvent(bridge.chatA)); await flush();
  let release: (() => void) | undefined;
  const pendingHydration = new Promise<void>((resolve) => { release = resolve; });
  const original = bridge.getChat.bind(bridge); let hold = true;
  bridge.getChat = async (input) => { if (hold) await pendingHydration; return original(input); };
  const cancel = controller.cancelAgentRun(controller.snapshot().agentRuns[0]!); await flush();
  assert.equal(controller.isRecoveryBlocked(), true);
  await controller.startDemo("planning"); await controller.startAgent("planning");
  assert.equal(bridge.fixtureStarts.length, 0);
  assert.equal(bridge.starts.length, 0);
  hold = false; release!(); await cancel;
  assert.equal(controller.isRecoveryBlocked(), true, "unresolved actual recovery stays fail-closed after hydration");
  controller.dispose();
});
