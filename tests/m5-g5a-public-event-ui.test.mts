import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { validateAgentCapabilitySnapshotV1 } from "../src/m5/contracts/agent-adapter-v1.ts";

import { canAbandonAgentRecovery, ChatUiController } from "../src/m5/chat-state.ts";
import type { M5BridgeV4 } from "../src/m5/contracts/bridge-v4.ts";
import type { AgentPublicEventV1, AgentRecoveryRecordV1, AgentRunRecordV1, AgentRunStateChangedV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
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
function agentRecovery(record: ChatRecordV1, recoveryId = "agent_recovery_random_7f4c", reason: AgentRecoveryRecordV1["reason"] = "uncleanShutdown"): AgentRecoveryRecordV1 {
  return { schemaVersion: 1, recoveryId, chatId: record.chatId, taskId: record.taskId, projectId: record.projectId, worktreeId: record.worktreeId, workspaceIdentity: record.workspaceIdentity, runId: "agent_run_01", source: "actualAgent", reason, state: "unresolved", trackedProcesses: reason === "processIdentityUnavailable" ? [] : [{ pid: 4401, startIdentityDigest: "sha256:agent-start-1", executablePathDigest: "sha256:agent-path-1", observedGone: false }], poisoned: true, revision: 7, createdAt: "2026-08-27T00:00:01Z", resolvedAt: null };
}
function publicEvent(record: ChatRecordV1, text = "raw text must never render"): AgentPublicEventV1 {
  return { schemaVersion: 1, eventId: "agent_event_01", chatId: record.chatId, taskId: record.taskId, projectId: record.projectId, worktreeId: record.worktreeId, workspaceIdentity: record.workspaceIdentity, runId: "agent_run_01", roleId: "coding", capabilityFingerprint: fingerprint, kind: "final", text, createdAt: "2026-08-27T00:00:01Z" };
}
function stateEvent(record: ChatRecordV1, state: AgentRunRecordV1["state"] = "succeeded", eventId = "agent_state_event_01"): AgentRunStateChangedV1 {
  return { schemaVersion: 1, eventId, run: agentRun(record, state) as AgentRunStateChangedV1["run"] };
}

class AgentBridge {
  readonly chatListeners = new Set<(event: TeamMessageV1) => void>();
  readonly agentListeners = new Set<(event: AgentPublicEventV1) => void>();
  readonly agentStateListeners = new Set<(event: AgentRunStateChangedV1) => void>();
  readonly chatA = chat("chat_a");
  readonly chatB = chat("chat_b");
  readonly calls: number[] = [];
  readonly starts: unknown[] = [];
  readonly fixtureStarts: unknown[] = [];
  readonly cancels: unknown[] = [];
  readonly agentRecoveryInspects: unknown[] = [];
  readonly agentRecoveryResolves: unknown[] = [];
  readonly legacyRecoveryInspects: unknown[] = [];
  readonly legacyRecoveryResolves: unknown[] = [];
  messages = [message(this.chatA, 1, "durable goal")];
  agentRuns: AgentRunRecordV1[] = [];
  agentRecovery: AgentRecoveryRecordV1 | null = null;
  agentDetail = true;
  recoveryResolveOutcome: "resolvedAbandoned" | "processesRemain" | "revisionConflict" = "resolvedAbandoned";
  unavailable = false;

  async getProjectRoleSettings(): Promise<ProjectRoleSettingsV1> { return { schemaVersion: 1, projectId: "project_a", revision: 1, roles, updatedAt: "2026-08-27T00:00:00Z" }; }
  async listChats() { return { projectId: "project_a", items: [this.chatA, this.chatB], nextCursor: null }; }
  async getChat(input: { chatId: string; afterEventSequence: number }) {
    this.calls.push(input.afterEventSequence);
    const current = input.chatId === this.chatB.chatId ? this.chatB : this.chatA;
    const messages = input.chatId === this.chatB.chatId ? [] : this.messages.filter((entry) => entry.eventSequence > input.afterEventSequence).slice(0, 50);
    const detail = { chat: current, afterEventSequence: input.afterEventSequence, messages, runs: [], nextEventSequence: messages.length === 0 ? null : messages[messages.length - 1]!.eventSequence };
    if (!this.agentDetail) return detail;
    return { ...detail, agentRuns: input.chatId === this.chatB.chatId ? [] : this.agentRuns, agentRecovery: input.chatId === this.chatA.chatId ? this.agentRecovery : null };
  }
  async listRecoveries() { return { items: [], nextCursor: null }; }
  async inspectRecovery(input: unknown) { this.legacyRecoveryInspects.push(input); throw new Error("legacy recovery must not handle actual agents"); }
  async markRecoveryResolved(input: unknown) { this.legacyRecoveryResolves.push(input); throw new Error("legacy recovery must not handle actual agents"); }
  onChatEvent(listener: (event: TeamMessageV1) => void) { this.chatListeners.add(listener); return () => this.chatListeners.delete(listener); }
  onAgentPublicEvent(listener: (event: AgentPublicEventV1) => void) { this.agentListeners.add(listener); return () => this.agentListeners.delete(listener); }
  onAgentRunStateChanged(listener: (event: AgentRunStateChangedV1) => void) { this.agentStateListeners.add(listener); return () => this.agentStateListeners.delete(listener); }
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
    this.agentRecovery = agentRecovery(this.chatA, "agent_recovery_01");
    return { outcome: "recoveryRequired", recovery: this.agentRecovery };
  }
  async inspectAgentRecovery(input: unknown) { this.agentRecoveryInspects.push(input); if (!this.agentRecovery) throw new Error("agent recovery missing"); return this.agentRecovery; }
  async resolveAgentRecovery(input: unknown) {
    this.agentRecoveryResolves.push(input);
    if (!this.agentRecovery) throw new Error("agent recovery missing");
    const before = this.agentRecovery;
    if (this.recoveryResolveOutcome === "resolvedAbandoned") this.agentRecovery = null;
    return { schemaVersion: 1, before, request: input, after: this.agentRecovery, outcome: this.recoveryResolveOutcome };
  }
  emitAgent(event: AgentPublicEventV1) { for (const listener of this.agentListeners) listener(event); }
  emitState(event: AgentRunStateChangedV1) { for (const listener of this.agentStateListeners) listener(event); }
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

test("zero-public-output terminal state notification rehydrates the durable ledger and releases the running lock", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.agentRuns = [agentRun(bridge.chatA, "running")]; bridge.calls.length = 0;
  bridge.agentRuns = [agentRun(bridge.chatA, "succeeded")]; bridge.emitState(stateEvent(bridge.chatA, "succeeded")); await flush(); await flush();
  assert.deepEqual(bridge.calls, [1]);
  assert.equal(controller.snapshot().messages.length, 1, "the terminal notification carried no public output");
  assert.equal(controller.snapshot().agentRuns[0]?.state, "succeeded");
  await controller.startAgent("planning");
  assert.equal(bridge.starts.length, 1, "the durable terminal state releases the actual-run action lock");
  controller.dispose();
});

test("terminal notification record is ignored until the durable chat.get ledger agrees", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.agentRuns = [agentRun(bridge.chatA, "running")]; bridge.emitState(stateEvent(bridge.chatA, "succeeded")); await flush(); await flush();
  assert.equal(controller.snapshot().agentRuns[0]?.state, "running");
  controller.dispose();
});

test("notification hydration failure is visible without changing durable messages or run state", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  const before = controller.snapshot();
  const original = bridge.getChat.bind(bridge);
  bridge.getChat = async () => { throw new Error("ledger read unavailable"); };
  bridge.emitAgent(publicEvent(bridge.chatA));
  await flush();
  assert.equal(controller.snapshot().error?.messageKey, "m5.chatError.history");
  assert.equal(controller.snapshot().error?.detail, "ledger read unavailable");
  assert.deepEqual(controller.snapshot().messages, before.messages);
  assert.deepEqual(controller.snapshot().agentRuns, before.agentRuns);
  await controller.retryHistory();
  assert.equal(controller.snapshot().error?.retryChatId, bridge.chatA.chatId);
  bridge.getChat = original;
  bridge.messages.push(message(bridge.chatA, 2, "later durable response"));
  await controller.retryHistory();
  assert.equal(controller.snapshot().error, null);
  assert.equal(controller.snapshot().messages.at(-1)?.text, "later durable response");
  controller.dispose();
});

test("notification hydration rejection after disposal does not publish an error", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  let rejectRead!: (error: Error) => void;
  bridge.getChat = () => new Promise((_resolve, reject) => { rejectRead = reject; });
  bridge.emitState(stateEvent(bridge.chatA));
  controller.dispose();
  rejectRead(new Error("late failure"));
  await flush();
  assert.equal(controller.snapshot().error, null);
});

test("late notification hydration failure cannot overwrite another chat's error state", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  const original = bridge.getChat.bind(bridge);
  let rejectRead!: (error: Error) => void;
  bridge.getChat = () => new Promise((_resolve, reject) => { rejectRead = reject; });
  bridge.emitAgent(publicEvent(bridge.chatA));
  bridge.getChat = original;
  await controller.prepareSwitch(bridge.chatB.chatId, true);
  await controller.commitPrepared(bridge.chatB.chatId, true);
  rejectRead(new Error("old chat failure"));
  await flush();
  assert.equal(controller.snapshot().activeChat?.chatId, bridge.chatB.chatId);
  assert.equal(controller.snapshot().error, null);
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

test("actual event hydration appends contiguous pages without changing existing message identities", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  const first = controller.snapshot().messages[0];
  for (let sequence = 2; sequence <= 151; sequence++) {
    bridge.messages.push(message(bridge.chatA, sequence, `durable ${sequence}`));
  }
  bridge.calls.length = 0;
  bridge.emitAgent(publicEvent(bridge.chatA));
  await flush(); await flush();
  assert.deepEqual(bridge.calls, [1, 51, 101, 151]);
  assert.equal(controller.snapshot().messages[0], first);
  assert.deepEqual(controller.snapshot().messages.map(entry => entry.eventSequence),
    Array.from({length:151}, (_, index) => index + 1));
  assert.equal(controller.snapshot().error, null);
  controller.dispose();
});

test("actual event hydration still rejects a gap inside a multi-message page", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.messages.push(message(bridge.chatA, 2, "second"), message(bridge.chatA, 4, "gap"));
  bridge.emitAgent(publicEvent(bridge.chatA));
  await flush();
  assert.equal(controller.snapshot().error?.messageKey, "m5.chatError.history");
  assert.deepEqual(controller.snapshot().messages.map(entry => entry.eventSequence), [1]);
  controller.dispose();
});

test("public and state notification burst coalesces through the same durable tail hydration latch", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.agentRuns = [agentRun(bridge.chatA, "succeeded")]; bridge.calls.length = 0;
  bridge.emitState(stateEvent(bridge.chatA, "succeeded")); bridge.emitAgent(publicEvent(bridge.chatA)); await flush(); await flush();
  assert.deepEqual(bridge.calls, [1, 1], "a burst consumes one extra empty-tail check, not a second concurrent hydration");
  assert.equal(controller.snapshot().agentRuns[0]?.state, "succeeded");
  controller.dispose();
});

test("last empty hydration response와 같은 turn의 terminal notification는 durable tail을 자동 재조회한다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.messages.push(message(bridge.chatA, 2, "second")); bridge.calls.length = 0;
  let releaseEmpty: (() => void) | undefined; let requestedEmpty: (() => void) | undefined;
  const emptyRequested = new Promise<void>((resolve) => { requestedEmpty = resolve; });
  const original = bridge.getChat.bind(bridge); let held = false; let tailRequests = 0;
  bridge.getChat = async (input) => {
    if (input.afterEventSequence === 2) tailRequests++;
    if (input.afterEventSequence === 2 && !held) {
      held = true; requestedEmpty!();
      return new Promise((resolve) => { releaseEmpty = () => {
        bridge.messages.push(message(bridge.chatA, 3, "tail after empty")); bridge.agentRuns = [agentRun(bridge.chatA, "succeeded")]; bridge.emitState(stateEvent(bridge.chatA, "succeeded"));
        resolve({ chat: bridge.chatA, afterEventSequence: 2, messages: [], runs: [], nextEventSequence: null, agentRuns: bridge.agentRuns });
      }; });
    }
    return original(input);
  };
  bridge.emitAgent(publicEvent(bridge.chatA)); await emptyRequested;
  releaseEmpty!();
  await flush(); await flush();
  assert.equal(tailRequests, 2, "the empty tail is replayed once after the same-turn event");
  assert.deepEqual(bridge.calls, [1, 2, 3]);
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.eventSequence), [1, 2, 3]);
  assert.equal(controller.snapshot().messages[2]?.text, "tail after empty");
  controller.dispose();
});

test("pending 및 inactive actual event는 텍스트를 보이지 않고 해당 채팅의 hydration 또는 refresh notice만 남긴다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.emitAgent(publicEvent(bridge.chatB, "inactive raw")); await flush();
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.text), ["durable goal"]);
  assert.deepEqual(controller.snapshot().refreshNeededChatIds, ["chat_b"]);
  controller.dispose();
});

test("inactive terminal notification only marks the chat for durable refresh", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.emitState(stateEvent(bridge.chatB, "succeeded")); await flush();
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.text), ["durable goal"]);
  assert.deepEqual(controller.snapshot().refreshNeededChatIds, ["chat_b"]);
  assert.equal(controller.snapshot().agentRuns.length, 0);
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
  bridge.agentDetail = false;
  const g5 = bridge as unknown as Record<string, unknown>;
  g5.onAgentPublicEvent = undefined; g5.onAgentRunStateChanged = undefined; g5.getAgentCapabilities = undefined; g5.startAgentRun = undefined; g5.cancelAgentRun = undefined;
  const controller = new ChatUiController(g5 as unknown as M5BridgeV4);
  controller.start(); await controller.openProject("project_a"); const pending = await controller.prepareSwitch("chat_a", true); await controller.commitPrepared(pending!.chatId, true);
  assert.equal(controller.snapshot().agentRuns.length, 0);
  await controller.startAgent("coding");
  assert.match(controller.snapshot().notice ?? "", /실제 에이전트 실행/);
  controller.dispose();
});

test("V4 bridge without the V5 state listener keeps the existing public-event fixture flow", async () => {
  const bridge = new AgentBridge();
  (bridge as unknown as Record<string, unknown>).onAgentRunStateChanged = undefined;
  const controller = new ChatUiController(bridge as unknown as M5BridgeV4); controller.start(); await controller.openProject("project_a");
  const pending = await controller.prepareSwitch("chat_a", true); await controller.commitPrepared(pending!.chatId, true);
  bridge.messages.push(message(bridge.chatA, 2, "durable V4 result")); bridge.agentRuns = [agentRun(bridge.chatA, "running")]; bridge.emitAgent(publicEvent(bridge.chatA)); await flush(); await flush();
  assert.deepEqual(controller.snapshot().messages.map((entry) => entry.text), ["durable goal", "durable V4 result"]);
  assert.equal(controller.snapshot().agentRuns[0]?.state, "running");
  controller.dispose();
});

test("long conversations require explicit bounded-context consent without deleting history", async () => {
  for (const [count, consent] of [[100, false], [101, false], [201, true]] as const) {
    const bridge = new AgentBridge();
    bridge.messages = Array.from({length:count}, (_, index) => message(bridge.chatA, index + 1, `message ${index + 1}`));
    const controller = await open(bridge);
    const previousConfirm = globalThis.confirm;
    let prompt = "";
    globalThis.confirm = value => { prompt = value ?? ""; return consent; };
    try { await controller.startAgent("coding", "en"); }
    finally { globalThis.confirm = previousConfirm; }
    if (count <= 100) {
      assert.equal(prompt, "");
      assert.equal(bridge.starts.length, 1);
    } else {
      assert.match(prompt, new RegExp(`has ${count} messages`));
      assert.match(prompt, /earlier decisions may be missing/);
      assert.equal(bridge.starts.length, consent ? 1 : 0);
    }
    if (consent) {
      assert.deepEqual((bridge.starts[0] as {inputMessageIds:string[]}).inputMessageIds,
        [bridge.messages[0]!.messageId, ...bridge.messages.slice(-99).map(item => item.messageId)]);
    }
    assert.equal(controller.snapshot().messages.length, count);
    assert.equal(controller.snapshot().agentStartingRoleId, null);
    controller.dispose();
  }
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

test("inactive product capability blocks execution without claiming missing installation", async () => {
  const bridge = new AgentBridge();
  const controller = await open(bridge);
  const capability = validateAgentCapabilitySnapshotV1(JSON.parse(readFileSync(new URL("./fixtures/m5/agent-adapter-v1/unobserved-capability.json", import.meta.url), "utf8")));
  bridge.getAgentCapabilities = async () => capability;
  await controller.startAgent("planning");
  assert.equal(bridge.starts.length, 0);
  assert.equal(bridge.fixtureStarts.length, 0);
  assert.equal(controller.snapshot().noticeContext?.reason, "capabilityUnknown");
  assert.doesNotMatch(controller.snapshot().notice ?? "", /설치되지|미설치/);
  controller.dispose();
});

test("actual cancel은 full identity만 보내고 recoveryRequired를 legacy abandon으로 바꾸지 않으며 dispose가 모든 listener를 정리한다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.agentRuns = [agentRun(bridge.chatA)]; bridge.messages.push(message(bridge.chatA, 2, "ready")); bridge.emitAgent(publicEvent(bridge.chatA)); await flush();
  await controller.cancelAgentRun(controller.snapshot().agentRuns[0]!);
  assert.deepEqual(bridge.cancels, [{ chatId: "chat_a", taskId: "task_chat_a", projectId: "project_a", worktreeId: "worktree_chat_a", workspaceIdentity: "workspace_chat_a", runId: "agent_run_01" }]);
  assert.equal(controller.snapshot().agentRecovery?.recoveryId, "agent_recovery_01");
  assert.match(controller.snapshot().notice ?? "", /복구가 필요/);
  controller.dispose();
  assert.equal(bridge.chatListeners.size, 0); assert.equal(bridge.agentListeners.size, 0); assert.equal(bridge.agentStateListeners.size, 0);
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

test("random agent recovery는 terminal durable hydration과 재개방 뒤에도 표시되고 모든 동작을 막는다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  bridge.agentRuns = [agentRun(bridge.chatA, "recoveryRequired")]; bridge.agentRecovery = agentRecovery(bridge.chatA, "agent_recovery_opaque_8db9");
  bridge.emitState(stateEvent(bridge.chatA, "recoveryRequired")); await flush(); await flush();
  assert.equal(controller.snapshot().agentRecovery?.recoveryId, "agent_recovery_opaque_8db9");
  assert.equal(controller.isActionBlocked(), true);
  await controller.startDemo("planning"); await controller.startAgent("planning");
  assert.equal(bridge.fixtureStarts.length, 0); assert.equal(bridge.starts.length, 0);
  controller.dispose();
  const reopened = await open(bridge);
  assert.equal(reopened.snapshot().agentRecovery?.recoveryId, "agent_recovery_opaque_8db9");
  assert.equal(reopened.isRecoveryBlocked(), true);
  reopened.dispose();
});

test("actual recovery abandon은 inspect 기록의 full identity와 revision만 resolve로 보내며 durable null 뒤에만 푼다", async () => {
  const bridge = new AgentBridge(); bridge.agentRuns = [agentRun(bridge.chatA, "recoveryRequired")]; bridge.agentRecovery = agentRecovery(bridge.chatA, "agent_recovery_opaque_9a6e");
  const controller = await open(bridge);
  const previousConfirm = globalThis.confirm;
  let prompt = "";
  globalThis.confirm = message => { prompt = message ?? ""; return false; };
  try {
    await controller.abandonAgentRecovery("en");
    assert.match(prompt, /rechecking termination evidence/);
    assert.deepEqual(bridge.agentRecoveryInspects, []);
    assert.deepEqual(bridge.agentRecoveryResolves, []);
    assert.equal(controller.isRecoveryBlocked(), true);
    globalThis.confirm = () => true;
    await controller.abandonAgentRecovery();
  }
  finally { globalThis.confirm = previousConfirm; }
  const identity = { chatId: "chat_a", taskId: "task_chat_a", projectId: "project_a", worktreeId: "worktree_chat_a", workspaceIdentity: "workspace_chat_a", runId: "agent_run_01", recoveryId: "agent_recovery_opaque_9a6e" };
  assert.deepEqual(bridge.agentRecoveryInspects, [identity]);
  assert.deepEqual(bridge.agentRecoveryResolves, [{ ...identity, expectedRevision: 7, resolution: "abandon" }]);
  assert.deepEqual(bridge.legacyRecoveryInspects, []);
  assert.deepEqual(bridge.legacyRecoveryResolves, []);
  assert.equal(controller.snapshot().agentRecovery, null, "only durable chat.get null clears the gate");
  assert.equal(controller.isRecoveryBlocked(), false);
  controller.dispose();
});

test("process identity가 없는 actual recovery는 재수화·재개방 뒤에도 보호 상태를 유지하고 abandon API를 호출하지 않는다", async () => {
  const bridge = new AgentBridge(); bridge.agentRuns = [agentRun(bridge.chatA, "recoveryRequired")]; bridge.agentRecovery = agentRecovery(bridge.chatA, "agent_recovery_identityless", "processIdentityUnavailable");
  const controller = await open(bridge);
  assert.equal(controller.snapshot().agentRecovery?.reason, "processIdentityUnavailable");
  assert.equal(controller.isActionBlocked(), true);
  let confirms = 0; const previousConfirm = globalThis.confirm; globalThis.confirm = () => { confirms += 1; return true; };
  try { await controller.abandonAgentRecovery(); }
  finally { globalThis.confirm = previousConfirm; }
  assert.equal(confirms, 0);
  assert.deepEqual(bridge.agentRecoveryInspects, []);
  assert.deepEqual(bridge.agentRecoveryResolves, []);
  bridge.emitState(stateEvent(bridge.chatA, "recoveryRequired")); await flush(); await flush();
  assert.equal(controller.snapshot().agentRecovery?.recoveryId, "agent_recovery_identityless");
  assert.equal(controller.isRecoveryBlocked(), true);
  controller.dispose();
  const reopened = await open(bridge);
  assert.equal(reopened.snapshot().agentRecovery?.reason, "processIdentityUnavailable");
  assert.equal(reopened.isActionBlocked(), true);
  reopened.dispose();
});

test("identityless recovery UI와 controller는 같은 abandon 가능 경계를 사용한다", () => {
  const record = agentRecovery(chat(), "agent_recovery_identityless_policy", "processIdentityUnavailable");
  assert.equal(canAbandonAgentRecovery(record), false);
  assert.equal(canAbandonAgentRecovery(agentRecovery(chat())), true);
  assert.equal(canAbandonAgentRecovery(null), false);
});

test("actual recovery resolve가 processesRemain 또는 revisionConflict면 durable 재조회 뒤에도 계속 막는다", async () => {
  for (const outcome of ["processesRemain", "revisionConflict"] as const) {
    const bridge = new AgentBridge(); bridge.agentRuns = [agentRun(bridge.chatA, "recoveryRequired")]; bridge.agentRecovery = agentRecovery(bridge.chatA, `agent_recovery_${outcome}`); bridge.recoveryResolveOutcome = outcome;
    const controller = await open(bridge);
    const previousConfirm = globalThis.confirm; globalThis.confirm = () => true;
    try { await controller.abandonAgentRecovery(); }
    finally { globalThis.confirm = previousConfirm; }
    assert.equal(controller.snapshot().agentRecovery?.recoveryId, `agent_recovery_${outcome}`);
    assert.equal(controller.isRecoveryBlocked(), true);
    assert.equal(bridge.agentRecoveryResolves.length, 1);
    controller.dispose();
  }
});

test("stale terminal hydration은 나중에 연 다른 채팅의 recovery 상태를 바꾸지 않는다", async () => {
  const bridge = new AgentBridge(); const controller = await open(bridge);
  const original = bridge.getChat.bind(bridge); let release: (() => void) | undefined; let held = false;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  bridge.getChat = async (input) => {
    if (input.chatId === bridge.chatA.chatId && !held) { held = true; await gate; }
    return original(input);
  };
  bridge.agentRuns = [agentRun(bridge.chatA, "recoveryRequired")]; bridge.agentRecovery = agentRecovery(bridge.chatA, "agent_recovery_stale_1");
  bridge.emitState(stateEvent(bridge.chatA, "recoveryRequired")); await flush();
  const pending = await controller.prepareSwitch("chat_b", true); await controller.commitPrepared(pending!.chatId, true);
  release!(); await flush(); await flush();
  assert.equal(controller.snapshot().activeChat?.chatId, "chat_b");
  assert.equal(controller.snapshot().agentRecovery, null);
  controller.dispose();
});


test("rejected capability or start clears the preparation notice without retrying", async () => {
  for (const phase of ["capability", "start"] as const) {
    const bridge = new AgentBridge();
    const controller = await open(bridge);
    let attempts = 0;
    const reject = async () => { attempts++; throw new Error(`rejected ${phase}`); };
    if (phase === "capability") bridge.getAgentCapabilities = reject;
    else bridge.startAgentRun = reject;
    await controller.startAgent("coding");
    assert.equal(attempts, 1);
    assert.equal(controller.snapshot().agentStartingRoleId, null);
    assert.equal(controller.snapshot().notice, null);
    assert.equal(controller.snapshot().noticeKey, undefined);
    assert.equal(controller.snapshot().error?.detail, `rejected ${phase}`);
    assert.equal(controller.snapshot().agentRuns.length, 0);
    controller.dispose();
  }
});

test("late start rejection does not overwrite another chat's notice or error", async () => {
  const bridge = new AgentBridge();
  const controller = await open(bridge);
  let reject!: (error: Error) => void;
  bridge.getAgentCapabilities = () => new Promise((_, fail) => { reject = fail; });
  const starting = controller.startAgent("coding");
  await controller.prepareSwitch("chat_b", true);
  await controller.commitPrepared("chat_b", true);
  const current = controller.snapshot();
  assert.equal(current.activeChat?.chatId, "chat_b");
  reject(new Error("old chat start rejected"));
  await starting;
  assert.equal(controller.snapshot().notice, current.notice);
  assert.equal(controller.snapshot().error, current.error);
  assert.equal(controller.snapshot().agentStartingRoleId, null);
  controller.dispose();
});

test("accepted start with failed hydration clears progress and later reconciles durable failure", async () => {
  const bridge = new AgentBridge();
  const controller = await open(bridge);
  const start = bridge.startAgentRun.bind(bridge);
  const getChat = bridge.getChat.bind(bridge);
  bridge.startAgentRun = async (input) => {
    const accepted = await start(input);
    bridge.getChat = async () => { throw new Error("post-start history unavailable"); };
    return accepted;
  };
  await controller.startAgent("coding");
  assert.equal(bridge.starts.length, 1);
  assert.equal(controller.snapshot().agentStartingRoleId, null);
  assert.equal(controller.snapshot().notice, null);
  assert.equal(controller.snapshot().error?.detail, "post-start history unavailable");
  bridge.getChat = getChat;
  const failed = agentRun(bridge.chatA, "failed");
  bridge.agentRuns = [failed];
  bridge.emitState(stateEvent(bridge.chatA, "failed"));
  await flush();
  assert.deepEqual(controller.snapshot().agentRuns, [failed]);
  assert.equal(bridge.starts.length, 1, "reconciliation never retries the provider");
  controller.dispose();
});
