import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { PassThrough } from "node:stream";
import { resolve } from "node:path";
import test from "node:test";

import {
  AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1,
  AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1,
  AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1,
  MAX_CONTROL_LINE_BYTES,
  PROTOCOL_STREAM_IDS_PER_SESSION_MAX_V1,
  type EventV1,
} from "../src/m5/contracts/protocol-v1.ts";
import { G5_CAPABILITIES_V1, G5D_PRODUCT_CAPABILITIES_V1 } from "../src/m5/contracts/run-recovery-v1.ts";
import {
  G5A_CAPABILITIES_V1,
  G5D_AGENT_FIXTURE_CAPABILITIES_V1,
  computeAgentCapabilityFingerprintV1,
  type AgentCapabilitySnapshotV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import { QuitCoordinatorV1 } from "../electron/quit-coordinator.ts";
import { SidecarClientV1, type SidecarProcessV1 } from "../electron/sidecar-client.ts";
import {
  validateAbandonPreparedWorkspaceInput, validatePathInput, validatePrepareChatWorkspaceInput,
  validateRecoveryInspectInput, validateRoleSettingsGetInput, validateSaveBeginInput,
  validateSaveChunkInput, validateSaveCommitInput, validateTreeInput,
} from "../electron/bridge-input.ts";
import { EventCreditV1, MAX_IN_FLIGHT_EVENTS } from "../electron/event-credit.ts";
import {
  validateWorkspaceSaveChunkInput, validateWorkspaceSaveCommitInput, validateWorkspaceTreeInput,
} from "../electron/bridge-input.ts";

class FixtureSidecar extends EventEmitter {
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  readonly writes: string[] = [];
  terminations = 0;
  writeError: Error | undefined;
  readonly stdin = {
    writable: true,
    write: (chunk: Buffer | string, _encoding: string, callback: (error?: Error | null) => void) => {
      this.writes.push(Buffer.from(chunk).toString("utf8"));
      callback(this.writeError);
      return true;
    },
  } as unknown as SidecarProcessV1["stdin"];
  terminate() {
    this.terminations += 1;
  }
}

function createClient(timeout = 1_000) {
  const child = new FixtureSidecar();
  let spawns = 0;
  const client = new SidecarClientV1(
    { command: "fixture", args: ["--stdio"], cwd: "/private/tmp" },
    {
      readyTimeoutMs: timeout,
      spawn: () => {
        spawns += 1;
        return child as unknown as SidecarProcessV1;
      },
    },
  );
  return { child, client, spawns: () => spawns };
}

function readyLine() {
  return JSON.stringify({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "system",
    seq: 1,
    event: "system.ready",
    payload: {
      protocolVersion: 1,
      sidecarSchemaVersion: 1,
      appSchemaVersion: 1,
      pid: 42,
      platform: "darwin",
      capabilities: [...G5_CAPABILITIES_V1],
      sidecarInstanceId: "sidecar_test_01",
    },
  });
}

function g5aReadyLine() {
  return JSON.stringify({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "system",
    seq: 1,
    event: "system.ready",
    payload: {
      protocolVersion: 1,
      sidecarSchemaVersion: 1,
      appSchemaVersion: 1,
      pid: 42,
      platform: "darwin",
      capabilities: [...G5A_CAPABILITIES_V1],
      sidecarInstanceId: "sidecar_test_01",
    },
  });
}

function g5dReadyLine() {
  return JSON.stringify({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "system",
    seq: 1,
    event: "system.ready",
    payload: {
      protocolVersion: 1,
      sidecarSchemaVersion: 1,
      appSchemaVersion: 1,
      pid: 42,
      platform: "darwin",
      capabilities: [...G5D_PRODUCT_CAPABILITIES_V1],
      sidecarInstanceId: "sidecar_test_01",
    },
  });
}

function g5dAgentReadyLine() {
  const ready = JSON.parse(g5dReadyLine()) as { payload: { capabilities: string[] } };
  ready.payload.capabilities = [...G5D_AGENT_FIXTURE_CAPABILITIES_V1];
  return JSON.stringify(ready);
}

function g5dStatus(state: "verifying" | "ready" | "failed"): Record<string, unknown> {
  const admission = state === "verifying"
    ? { schemaVersion: 1, state, startedAt: "2026-09-08T04:00:00Z", completedAt: null, error: null }
    : state === "ready"
      ? { schemaVersion: 1, state, startedAt: "2026-09-08T04:00:00Z", completedAt: "2026-09-08T04:01:00Z", error: null }
      : { schemaVersion: 1, state, startedAt: "2026-09-08T04:00:00Z", completedAt: "2026-09-08T04:01:00Z", error: { code: "storageAdmissionFailed", kind: "storageUnavailable", detail: "storage admission failed", retryable: false, outcomeUnknown: true } };
  return {
    schemaVersion: 1,
    state: "shellReady",
    sidecar: {
      protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 42, platform: "darwin",
      capabilities: [...G5D_PRODUCT_CAPABILITIES_V1], sidecarInstanceId: "sidecar_test_01",
    },
    storageAdmission: admission,
    database: state === "verifying" ? { health: "verifying", schemaVersion: null } : state === "ready" ? { health: "healthy", schemaVersion: 1 } : { health: "unavailable", schemaVersion: null },
    recoveryRequired: state === "ready" ? false : null,
    blockerIds: [],
  };
}

function g5aFixture(name: string, valid = true): Record<string, unknown> {
  const cases = JSON.parse(readFileSync(resolve("tests/fixtures/m5/agent-adapter-v1/cases.json"), "utf8")) as {
    valid: Array<{ name: string; value: Record<string, unknown> }>;
    invalid: Array<{ name: string; value: Record<string, unknown> }>;
  };
  const entry = (valid ? cases.valid : cases.invalid).find((candidate) => candidate.name === name);
  assert.ok(entry, `missing G5A fixture ${name}`);
  return entry.value;
}

function g5bFixture(name: string): Record<string, unknown> {
  const cases = JSON.parse(readFileSync(resolve("tests/fixtures/m5/agent-adapter-v1/g5b-terminal-budget-cases.json"), "utf8")) as {
    valid: Array<{ name: string; value: Record<string, unknown> }>;
  };
  const entry = cases.valid.find((candidate) => candidate.name === name);
  assert.ok(entry, `missing G5B fixture ${name}`);
  return entry.value;
}

function g5Fixture(name: string): Record<string, unknown> {
  const cases = JSON.parse(readFileSync(resolve("tests/fixtures/m5/g5-v1/cases.json"), "utf8")) as {
    valid: Array<{ name: string; value: Record<string, unknown> }>;
  };
  const entry = cases.valid.find((candidate) => candidate.name === name);
  assert.ok(entry, `missing G5 fixture ${name}`);
  return entry.value;
}

function measuredCapability(): AgentCapabilitySnapshotV1 {
  const snapshot = {
    schemaVersion: 1 as const,
    runtime: "claude-code" as const,
    platform: "darwin" as const,
    installationState: "installed" as const,
    authState: "authenticated" as const,
    executable: {
      pathDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      fileIdentityDigest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      cliVersion: "2.1.241",
    },
    models: [{ modelSelector: "claude-opus-5", reasoningEfforts: ["high"], permissions: ["readOnly", "workspaceWrite"] }],
    discovery: {
      source: "exactVersionManifest" as const,
      verificationLevel: "measuredExactVersion" as const,
      exactCliVersion: "2.1.241",
      effectiveModelEvidence: "measuredExactVersion" as const,
    },
    canStart: true,
    blockedReason: null,
    fingerprint: "",
    observedAt: "2026-08-27T00:00:00Z",
  };
  return { ...snapshot, fingerprint: computeAgentCapabilityFingerprintV1(snapshot as AgentCapabilitySnapshotV1) };
}

function acceptedAgentRun(fingerprint: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    schemaVersion: 1,
    runId: "agent_run_accepted",
    chatId: "chat_01",
    taskId: "task_01",
    projectId: "project_01",
    worktreeId: "worktree_01",
    workspaceIdentity: "workspace_01",
    roleId: "planning",
    capabilityFingerprint: fingerprint,
    state: "preparing",
    ...overrides,
  };
}

function agentPublicEvent(fingerprint: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    schemaVersion: 1,
    eventId: "agent_event_accepted",
    chatId: "chat_01",
    taskId: "task_01",
    projectId: "project_01",
    worktreeId: "worktree_01",
    workspaceIdentity: "workspace_01",
    runId: "agent_run_accepted",
    roleId: "planning",
    capabilityFingerprint: fingerprint,
    kind: "status",
    text: "작업을 준비하고 있습니다.",
    createdAt: "2026-08-27T01:02:00Z",
    ...overrides,
  };
}

function agentPublicEventLine(payload: Record<string, unknown>, streamId = "agent_public", seq = 1): string {
  return eventLine({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId,
    seq,
    event: "agent.publicEvent",
    chatId: payload.chatId as string,
    taskId: payload.taskId as string,
    runId: payload.runId as string,
    payload,
  });
}

function agentRunStateChanged(fingerprint: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    schemaVersion: 1,
    eventId: "agent_state_event_accepted",
    run: {
      schemaVersion: 1,
      runId: "agent_run_accepted",
      chatId: "chat_01",
      taskId: "task_01",
      projectId: "project_01",
      worktreeId: "worktree_01",
      workspaceIdentity: "workspace_01",
      selectedProfile: {
        roleId: "planning",
        runtime: "claude-code",
        modelSelector: "claude-opus-5",
        reasoningEffort: null,
        permission: "readOnly",
      },
      source: "actualAgent",
      capabilityFingerprint: fingerprint,
      state: "running",
      stage: "planning",
      terminationOutcome: null,
      createdAt: "2026-08-27T01:01:00Z",
      updatedAt: "2026-08-27T01:02:00Z",
    },
    ...overrides,
  };
}

function agentRunStateChangedLine(payload: Record<string, unknown>, streamId = "agent_state", seq = 1): string {
  const run = payload.run as Record<string, unknown>;
  return eventLine({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId,
    seq,
    event: "agent.runStateChanged",
    chatId: run.chatId as string,
    taskId: run.taskId as string,
    runId: run.runId as string,
    payload,
  });
}

async function hydrateActualAgentRun(client: SidecarClientV1, child: FixtureSidecar) {
  const detail = client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const request = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(request.requestId, g5aFixture("actual-agent-chat-page-keeps-distinct-ledger")));
  await detail;
}

function plannedEmptyAgentDetail(): Record<string, unknown> {
  const page = structuredClone(g5aFixture("actual-agent-chat-page-keeps-distinct-ledger"));
  page.agentRuns = [];
  (page.messages as Record<string, unknown>[])[0]!.kind = "status";
  (page.messages as Record<string, unknown>[])[0]!.roleId = null;
  (page.messages as Record<string, unknown>[])[0]!.runId = null;
  return page;
}

function terminalHydrationPage(offset: number, count: number): Record<string, unknown> {
  const base = structuredClone(g5aFixture("actual-agent-chat-page-keeps-distinct-ledger"));
  (base.messages as Record<string, unknown>[])[0]!.runId = `agent_run_tombstone_${offset}`;
  base.agentRuns = Array.from({ length: count }, (_, index) => {
    const run = structuredClone((g5aFixture("actual-agent-chat-page-keeps-distinct-ledger").agentRuns as Record<string, unknown>[])[0]!);
    run.runId = `agent_run_tombstone_${offset + index}`;
    run.state = "succeeded";
    run.terminationOutcome = "cleanExit";
    run.updatedAt = "2026-08-27T01:03:00Z";
    return run;
  });
  return base;
}

async function hydrateTerminalRuns(client: SidecarClientV1, child: FixtureSidecar, count: number): Promise<void> {
  for (let offset = 0; offset < count; offset += 50) {
    const detail = client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
    const request = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string };
    child.stdout.write(responseLine(request.requestId, terminalHydrationPage(offset, Math.min(50, count - offset))));
    await detail;
  }
}

function eventLine(event: EventV1) {
  return `${JSON.stringify(event)}\n`;
}

test("sidecar waits for ready, accepts chunked JSONL, and exposes only ordered events", async () => {
  const { child, client } = createClient();
  const start = client.start();
  const ready = `${readyLine()}\n`;
  child.stdout.write(ready.slice(0, 20));
  child.stdout.write(ready.slice(20));
  await start;

  const received: EventV1[] = [];
  const unsubscribe = client.onEvent((event) => received.push(event));
  child.stdout.write(eventLine({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "system",
    seq: 2,
    event: "system.fatal",
    payload: { code: "diagnostic" },
  }));
  unsubscribe();
  child.stdout.write(eventLine({
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_test_01",
    streamId: "system",
    seq: 3,
    event: "system.fatal",
    payload: { code: "later" },
  }));
  assert.equal(received.length, 1);
  assert.equal(received[0]?.event, "system.fatal");
});

test("ready timeout, pre-ready stderr exit, and oversized partial output fail closed", async () => {
  const timeout = createClient(10);
  await assert.rejects(timeout.client.start(), /within 3 seconds/);
  assert.equal(timeout.child.terminations, 1);

  const preReady = createClient();
  const starting = preReady.client.start();
  preReady.child.stderr.write("migration failed\n");
  preReady.child.emit("exit", 70, null);
  await assert.rejects(starting, /sidecar exited/);

  const oversized = createClient();
  const oversizedStart = oversized.client.start();
  oversized.child.stdout.write(Buffer.alloc(MAX_CONTROL_LINE_BYTES + 1, 0x61));
  await assert.rejects(oversizedStart, /oversized or unterminated/);
  assert.equal(oversized.child.terminations, 1);

  const malformed = createClient();
  const malformedStart = malformed.client.start();
  malformed.child.stdout.write("not-json\n");
  await assert.rejects(malformedStart, /invalid JSON/);
  assert.equal(malformed.child.terminations, 1);
});

test("G5 rejects a ready handshake without the exact cumulative capabilities", async () => {
  const { child, client } = createClient();
  const starting = client.start();
  const ready = JSON.parse(readyLine()) as { payload: { capabilities: string[] } };
  ready.payload.capabilities.pop();
  child.stdout.write(`${JSON.stringify(ready)}\n`);
  await assert.rejects(starting, /capabilities do not match/);
  assert.equal(child.terminations, 1);
});

test("G5A agent IPC rejects launch extras before IPC and remains unavailable on the current G5 handshake", async () => {
  const { child, client } = createClient();
  const starting = client.start(); child.stdout.write(`${readyLine()}\n`); await starting;
  const start = {
    chatId: "chat_01",
    roleId: "coding" as const,
    inputMessageIds: ["message_01"],
    expectedCapabilityFingerprint: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  };
  for (const forbidden of ["executablePath", "argv", "model", "env", "cwd", "prompt", "userConfig", "addDir"]) {
    assert.throws(() => client.startAgentRun({ ...start, [forbidden]: "forbidden" } as unknown as typeof start), /fields/);
  }
  assert.equal(child.writes.length, 0, "invalid renderer launch input must not reach sidecar IPC");
  await assert.rejects(client.getAgentCapabilities({ runtime: "claude-code", refresh: false }), /agentAdapterUnavailable/);
  await assert.rejects(client.startAgentRun(start), /agentAdapterUnavailable/);
  assert.equal(child.writes.length, 0, "current G5 handshake must not advertise or invoke G5A calls");
});

test("login capability negotiates exact status and fixed requests", async () => {
  const { child, client } = createClient();
  const started = client.start();
  const ready = JSON.parse(g5dReadyLine());
  ready.payload.capabilities.push("chat.appendMessage", "provider.login");
  child.stdout.write(`${JSON.stringify(ready)}\n`);
  assert.equal((await started).capabilities.at(-1), "provider.login");
  const status = client.getSystemStatus();
  const result = g5dStatus("ready") as any;
  result.sidecar.capabilities.push("chat.appendMessage", "provider.login");
  child.stdout.write(responseLine(JSON.parse(child.writes.at(-1)!).requestId, result));
  await status;
  const login = client.login({ action: "status" });
  const sent = JSON.parse(child.writes.at(-1)!);
  assert.equal(sent.method, "provider.login");
  assert.deepEqual(sent.params, { action: "status" });
  const idle = { state: "idle", attemptId: null, authUrl: null, error: null };
  child.stdout.write(responseLine(sent.requestId, idle));
  assert.deepEqual(await login, idle);
});

test("follow-up capability binds stored messages and preserves exact status identity", async () => {
  const fixture = JSON.parse(readFileSync(new URL("./fixtures/m5/followup-message-v1.json", import.meta.url), "utf8"));
  for (const corrupt of [false, true]) {
    const { child, client } = createClient();
    const started = client.start();
    const ready = JSON.parse(g5dReadyLine());
    ready.payload.capabilities.push("chat.appendMessage");
    child.stdout.write(`${JSON.stringify(ready)}\n`);
    assert.equal((await started).capabilities.at(-1), "chat.appendMessage");
    const status = client.getSystemStatus();
    const statusRequest = JSON.parse(child.writes.at(-1)!);
    const result = g5dStatus("ready") as any;
    result.sidecar.capabilities.push("chat.appendMessage");
    child.stdout.write(responseLine(statusRequest.requestId, result));
    await status;
    const append = client.appendChatMessage(fixture.request);
    const sent = JSON.parse(child.writes.at(-1)!);
    assert.equal(sent.method, "chat.appendMessage");
    assert.deepEqual(sent.params, fixture.request);
    const message = { ...fixture.message, ...(corrupt ? { text: "wrong response" } : {}) };
    const outcome = corrupt ? assert.rejects(append) : append;
    child.stdout.write(responseLine(sent.requestId, message));
    if (corrupt) await outcome;
    else assert.deepEqual(await outcome, fixture.message);
  }
  const { child, client } = createClient();
  const started = client.start();
  child.stdout.write(`${g5dReadyLine()}\n`);
  await started;
  const writes = child.writes.length;
  await assert.rejects(client.appendChatMessage(fixture.request), /does not support/);
  assert.equal(child.writes.length, writes);
});

test("follow-up rejects misplaced, duplicated and unknown baseline capabilities", async () => {
  for (const capabilities of [
    ["chat.appendMessage", ...G5D_PRODUCT_CAPABILITIES_V1],
    [...G5D_PRODUCT_CAPABILITIES_V1, "chat.appendMessage", "chat.appendMessage"],
    [...G5_CAPABILITIES_V1, "chat.appendMessage"],
  ]) {
    const { child, client } = createClient();
    const started = client.start();
    const rejected = assert.rejects(started);
    const ready = JSON.parse(g5dReadyLine());
    ready.payload.capabilities = capabilities;
    child.stdout.write(`${JSON.stringify(ready)}\n`);
    await rejected;
  }
});

test("G5D resolves shell-ready before storage admission and accepts only exact status transitions", async () => {
  const { child, client } = createClient();
  const started = client.start();
  child.stdout.write(`${g5dReadyLine()}\n`);
  await started;

  const verifying = client.getSystemStatus();
  const verifyingRequest = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string; method: string };
  assert.equal(verifyingRequest.method, "system.status");
  child.stdout.write(responseLine(verifyingRequest.requestId, g5dStatus("verifying")));
  const verifyingStatus = await verifying;
  assert.equal("storageAdmission" in verifyingStatus && verifyingStatus.storageAdmission.state, "verifying");

  const ready = client.getSystemStatus();
  const readyRequest = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(readyRequest.requestId, g5dStatus("ready")));
  const readyStatus = await ready;
  assert.equal("storageAdmission" in readyStatus && readyStatus.storageAdmission.state, "ready");

  const storage = client.getStorageStatus({ schemaVersion: 1 });
  const storageRequest = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string; method: string; params: unknown };
  assert.equal(storageRequest.method, "storage.status");
  assert.deepEqual(storageRequest.params, { schemaVersion: 1 });
  child.stdout.write(responseLine(storageRequest.requestId, {
    schemaVersion: 1, collectedAt: "2026-09-08T04:01:00Z",
    database: { logicalBytes: 0, physicalBytes: 0, walBytes: 0 }, ledgerTail: { eventCount: 0, payloadBytes: 0 },
    snapshotAggregates: { count: 0, payloadBytes: 0 }, pressureFacts: { collectionStatus: "unavailable", volumeCapacityBytes: null, volumeAvailableBytes: null },
    compaction: { lastAttemptStatus: "neverRun", lastAttemptedAt: null, lastSuccessfulAt: null }, physicalCleanup: { enabled: false, reason: "userPolicyNotConfigured" },
  }));
  assert.equal((await storage).schemaVersion, 1);
});

test("G5D rejects malformed admission status without falling back to the legacy parser", async () => {
  const { child, client } = createClient();
  const started = client.start(); child.stdout.write(`${g5dReadyLine()}\n`); await started;
  const pending = client.getSystemStatus();
  const request = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string };
  const malformed = g5dStatus("failed");
  ((malformed.storageAdmission as Record<string, unknown>).error as Record<string, unknown>).detail = "SQLITE_CORRUPT /private/user.sqlite3";
  child.stdout.write(responseLine(request.requestId, malformed));
  await assert.rejects(pending, /storage admission/);
  assert.equal(child.terminations, 1);
});

test("G5D agent fixture keeps G5A availability and adds storage status only through the exact D1 handshake", async () => {
  const { child, client } = createClient();
  const started = client.start(); child.stdout.write(`${g5dAgentReadyLine()}\n`); await started;
  const storage = client.getStorageStatus({ schemaVersion: 1 });
  const request = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string; method: string };
  assert.equal(request.method, "storage.status");
  child.stdout.write(responseLine(request.requestId, {
    schemaVersion: 1, collectedAt: "2026-09-08T04:01:00Z",
    database: { logicalBytes: 0, physicalBytes: 0, walBytes: 0 }, ledgerTail: { eventCount: 0, payloadBytes: 0 },
    snapshotAggregates: { count: 0, payloadBytes: 0 }, pressureFacts: { collectionStatus: "unavailable", volumeCapacityBytes: null, volumeAvailableBytes: null },
    compaction: { lastAttemptStatus: "neverRun", lastAttemptedAt: null, lastSuccessfulAt: null }, physicalCleanup: { enabled: false, reason: "userPolicyNotConfigured" },
  }));
  await storage;
  const capability = client.getAgentCapabilities({ runtime: "claude-code", refresh: false });
  const capabilityRequest = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string; method: string };
  assert.equal(capabilityRequest.method, "agent.getCapabilities");
  child.stdout.write(responseLine(capabilityRequest.requestId, measuredCapability()));
  await capability;
});

test("G5A only wires exact negotiated capabilities and validates actual-agent response identity", async () => {
  const { child, client } = createClient();
  const starting = client.start(); child.stdout.write(`${g5aReadyLine()}\n`); await starting;

  const status = client.getSystemStatus();
  const statusRequest = JSON.parse(child.writes[0] ?? "{}") as { requestId: string; method: string };
  assert.equal(statusRequest.method, "system.status");
  child.stdout.write(responseLine(statusRequest.requestId, {
    state: "ready",
    sidecar: {
      protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 42, platform: "darwin",
      capabilities: [...G5A_CAPABILITIES_V1], sidecarInstanceId: "sidecar_test_01",
    },
    database: { health: "healthy", schemaVersion: 1 }, recoveryRequired: false, blockerIds: [],
  }));
  assert.equal((await status).state, "ready");

  const capabilities = client.getAgentCapabilities({ runtime: "claude-code", refresh: true });
  const capabilityRequest = JSON.parse(child.writes[1] ?? "{}") as { requestId: string; method: string; params: unknown };
  assert.equal(capabilityRequest.method, "agent.getCapabilities");
  assert.deepEqual(capabilityRequest.params, { runtime: "claude-code", refresh: true });
  child.stdout.write(responseLine(capabilityRequest.requestId, measuredCapability()));
  assert.equal((await capabilities).canStart, true);

  const detail = client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const detailRequest = JSON.parse(child.writes[2] ?? "{}") as { requestId: string; method: string };
  assert.equal(detailRequest.method, "chat.get");
  child.stdout.write(responseLine(detailRequest.requestId, g5aFixture("actual-agent-chat-page-keeps-distinct-ledger")));
  assert.equal((await detail).chat.taskId, "task_01");

  const start = client.startAgentRun({
    chatId: "chat_01", roleId: "planning", inputMessageIds: ["message_01"],
    expectedCapabilityFingerprint: measuredCapability().fingerprint,
  });
  const startRequest = JSON.parse(child.writes[3] ?? "{}") as { requestId: string; method: string; params: unknown };
  assert.equal(startRequest.method, "run.startAgent");
  assert.deepEqual(startRequest.params, {
    chatId: "chat_01", roleId: "planning", inputMessageIds: ["message_01"],
    expectedCapabilityFingerprint: measuredCapability().fingerprint,
  });
  child.stdout.write(responseLine(startRequest.requestId, acceptedAgentRun(measuredCapability().fingerprint, { chatId: "chat_other" })));
  await assert.rejects(start, /accepted agent run changed chatId/);
  assert.equal(child.terminations, 1, "cross-chat actual-run response must poison the session");
});

test("G5A accepted agent runs bind every chat context field and the requested fingerprint", async () => {
  const fingerprint = measuredCapability().fingerprint;
  const mutated: Array<[string, string]> = [
    ["chatId", "chat_other"], ["taskId", "task_other"], ["projectId", "project_other"],
    ["worktreeId", "worktree_other"], ["workspaceIdentity", "workspace_other"],
    ["roleId", "review"], ["capabilityFingerprint", `sha256:${"1".repeat(64)}`],
  ];
  for (const [field, replacement] of mutated) {
    const { child, client } = createClient();
    const starting = client.start(); child.stdout.write(`${g5aReadyLine()}\n`); await starting;
    const detail = client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
    const detailRequest = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
    child.stdout.write(responseLine(detailRequest.requestId, g5aFixture("actual-agent-chat-page-keeps-distinct-ledger")));
    await detail;
    const start = client.startAgentRun({
      chatId: "chat_01", roleId: "planning", inputMessageIds: ["message_01"], expectedCapabilityFingerprint: fingerprint,
    });
    const startRequest = JSON.parse(child.writes[1] ?? "{}") as { requestId: string };
    child.stdout.write(responseLine(startRequest.requestId, acceptedAgentRun(fingerprint, { [field]: replacement })));
    await assert.rejects(start, new RegExp(`accepted agent run changed ${field}`));
    assert.equal(child.terminations, 1, `${field} mutation must poison the planned G5A session`);
  }
});

test("G5A accepted run registry rejects a reused run ID before any future public event route", async () => {
  const { child, client } = createClient();
  const fingerprint = measuredCapability().fingerprint;
  const starting = client.start(); child.stdout.write(`${g5aReadyLine()}\n`); await starting;
  const detail = client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const detailRequest = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(detailRequest.requestId, g5aFixture("actual-agent-chat-page-keeps-distinct-ledger")));
  await detail;
  const input = { chatId: "chat_01", roleId: "planning" as const, inputMessageIds: ["message_01"], expectedCapabilityFingerprint: fingerprint };
  const first = client.startAgentRun(input);
  const firstRequest = JSON.parse(child.writes[1] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(firstRequest.requestId, acceptedAgentRun(fingerprint)));
  assert.equal((await first).runId, "agent_run_accepted");
  const duplicate = client.startAgentRun(input);
  const duplicateRequest = JSON.parse(child.writes[2] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(duplicateRequest.requestId, acceptedAgentRun(fingerprint)));
  await assert.rejects(duplicate, /accepted actual-agent runId was reused/);
  assert.equal(child.terminations, 1);
});

test("G5A emits normalized public events only after exact hydrated or accepted run binding", async () => {
  const hydrated = createClient();
  const hydratedStart = hydrated.client.start(); hydrated.child.stdout.write(`${g5aReadyLine()}\n`); await hydratedStart;
  await hydrateActualAgentRun(hydrated.client, hydrated.child);
  const hydratedEvents: Record<string, unknown>[] = [];
  hydrated.client.onAgentPublicEvent((event) => hydratedEvents.push(event));
  const hydratedEvent = g5aFixture("bounded-public-status-event");
  hydrated.child.stdout.write(agentPublicEventLine(hydratedEvent));
  assert.equal(hydratedEvents.length, 1);
  assert.equal(hydratedEvents[0]?.eventId, "agent_event_01");
  assert.equal(hydrated.child.terminations, 0);

  const accepted = createClient();
  const acceptedStart = accepted.client.start(); accepted.child.stdout.write(`${g5aReadyLine()}\n`); await acceptedStart;
  await hydrateActualAgentRun(accepted.client, accepted.child);
  const start = accepted.client.startAgentRun({
    chatId: "chat_01", roleId: "planning", inputMessageIds: ["message_01"], expectedCapabilityFingerprint: measuredCapability().fingerprint,
  });
  const request = JSON.parse(accepted.child.writes.at(-1) ?? "{}") as { requestId: string };
  accepted.child.stdout.write(responseLine(request.requestId, acceptedAgentRun(measuredCapability().fingerprint)));
  await start;
  const acceptedEvents: Record<string, unknown>[] = [];
  accepted.client.onAgentPublicEvent((event) => acceptedEvents.push(event));
  accepted.child.stdout.write(agentPublicEventLine(agentPublicEvent(measuredCapability().fingerprint)));
  assert.equal(acceptedEvents.length, 1);
  assert.equal(acceptedEvents[0]?.runId, "agent_run_accepted");
  assert.equal(accepted.child.terminations, 0);
});

test("G5A public events fail closed on unnegotiated, identity, event, stream, and payload violations", async () => {
  const unnegotiated = createClient();
  const unnegotiatedStart = unnegotiated.client.start(); unnegotiated.child.stdout.write(`${readyLine()}\n`); await unnegotiatedStart;
  unnegotiated.child.stdout.write(agentPublicEventLine(g5aFixture("bounded-public-status-event")));
  assert.match(unnegotiated.client.failure?.message ?? "", /require the exact planned G5A/);

  const mutations: Array<[string, string]> = [
    ["chatId", "chat_other"], ["taskId", "task_other"], ["projectId", "project_other"],
    ["worktreeId", "worktree_other"], ["workspaceIdentity", "workspace_other"], ["roleId", "review"],
    ["capabilityFingerprint", `sha256:${"1".repeat(64)}`],
  ];
  for (const [field, replacement] of mutations) {
    const invalid = createClient();
    const starting = invalid.client.start(); invalid.child.stdout.write(`${g5aReadyLine()}\n`); await starting;
    await hydrateActualAgentRun(invalid.client, invalid.child);
    const payload = { ...g5aFixture("bounded-public-status-event"), [field]: replacement };
    invalid.child.stdout.write(agentPublicEventLine(payload, `agent_public_${field}`));
    assert.match(invalid.client.failure?.message ?? "", new RegExp(`agent public event changed ${field}`));
    assert.equal(invalid.child.terminations, 1);
  }

  const unknown = createClient();
  const unknownStart = unknown.client.start(); unknown.child.stdout.write(`${g5aReadyLine()}\n`); await unknownStart;
  await hydrateActualAgentRun(unknown.client, unknown.child);
  unknown.child.stdout.write(agentPublicEventLine({ ...g5aFixture("bounded-public-status-event"), runId: "agent_run_unknown" }));
  assert.match(unknown.client.failure?.message ?? "", /unknown actual-agent run/);

  const duplicate = createClient();
  const duplicateStart = duplicate.client.start(); duplicate.child.stdout.write(`${g5aReadyLine()}\n`); await duplicateStart;
  await hydrateActualAgentRun(duplicate.client, duplicate.child);
  const first = g5aFixture("bounded-public-status-event");
  duplicate.child.stdout.write(agentPublicEventLine(first));
  duplicate.child.stdout.write(agentPublicEventLine(first, "agent_public", 2));
  assert.match(duplicate.client.failure?.message ?? "", /eventId was reused/);

  const streamReuse = createClient();
  const streamReuseStart = streamReuse.client.start(); streamReuse.child.stdout.write(`${g5aReadyLine()}\n`); await streamReuseStart;
  await hydrateActualAgentRun(streamReuse.client, streamReuse.child);
  const start = streamReuse.client.startAgentRun({
    chatId: "chat_01", roleId: "planning", inputMessageIds: ["message_01"], expectedCapabilityFingerprint: measuredCapability().fingerprint,
  });
  const startRequest = JSON.parse(streamReuse.child.writes.at(-1) ?? "{}") as { requestId: string };
  streamReuse.child.stdout.write(responseLine(startRequest.requestId, acceptedAgentRun(measuredCapability().fingerprint)));
  await start;
  streamReuse.child.stdout.write(agentPublicEventLine(agentPublicEvent(measuredCapability().fingerprint), "agent_public"));
  streamReuse.child.stdout.write(agentPublicEventLine(g5aFixture("bounded-public-status-event"), "agent_public", 2));
  assert.match(streamReuse.client.failure?.message ?? "", /stream changed actual-agent run binding/);

  for (const payload of [
    { ...g5aFixture("bounded-public-status-event"), reasoning: "private" },
    { ...g5aFixture("bounded-public-status-event"), text: "x".repeat(64 * 1024 + 1) },
  ]) {
    const malformed = createClient();
    const malformedStart = malformed.client.start(); malformed.child.stdout.write(`${g5aReadyLine()}\n`); await malformedStart;
    let delivered = 0;
    malformed.client.onAgentPublicEvent(() => { delivered += 1; });
    malformed.child.stdout.write(agentPublicEventLine(payload));
    assert.equal(delivered, 0);
    assert.equal(malformed.child.terminations, 1);
  }
});

test("G5B forwards only validated state-change hydration triggers and closes the active run after terminal delivery", async () => {
  const { child, client } = createClient();
  const fingerprint = measuredCapability().fingerprint;
  const starting = client.start(); child.stdout.write(`${g5aReadyLine()}\n`); await starting;
  await hydrateActualAgentRun(client, child);
  const start = client.startAgentRun({
    chatId: "chat_01", roleId: "planning", inputMessageIds: ["message_01"], expectedCapabilityFingerprint: fingerprint,
  });
  const request = JSON.parse(child.writes.at(-1) ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(request.requestId, acceptedAgentRun(fingerprint)));
  await start;

  const publicEvents: unknown[] = [];
  const stateEvents: unknown[] = [];
  client.onAgentPublicEvent((event) => publicEvents.push(event));
  client.onAgentRunStateChanged((event) => stateEvents.push(event));
  child.stdout.write(agentPublicEventLine(agentPublicEvent(fingerprint), "agent_run_accepted_stream", 1));
  child.stdout.write(agentRunStateChangedLine(agentRunStateChanged(fingerprint), "agent_run_accepted_stream", 2));
  const terminal = agentRunStateChanged(fingerprint, {
    eventId: "agent_state_event_terminal",
    run: { ...agentRunStateChanged(fingerprint).run as Record<string, unknown>, state: "succeeded", terminationOutcome: "cleanExit", updatedAt: "2026-08-27T01:03:00Z" },
  });
  child.stdout.write(agentRunStateChangedLine(terminal, "agent_run_accepted_stream", 3));

  assert.equal(publicEvents.length, 1);
  assert.deepEqual(stateEvents.map((event) => (event as { run: { state: string } }).run.state), ["running", "succeeded"]);
  assert.equal(child.terminations, 0);

  child.stdout.write(agentPublicEventLine(agentPublicEvent(fingerprint, { eventId: "agent_event_after_terminal" }), "agent_run_accepted_stream", 4));
  assert.match(client.failure?.message ?? "", /after terminal/);
  assert.equal(child.terminations, 1, "post-terminal public output must terminate exactly once before a listener receives it");
  assert.equal(publicEvents.length, 1);
});

test("G5B permits a zero-public terminal notification but rejects mutation, invalid transition, replay, and cross-run streams before listeners", async () => {
  const zeroPublic = createClient();
  const zeroStart = zeroPublic.client.start(); zeroPublic.child.stdout.write(`${g5aReadyLine()}\n`); await zeroStart;
  await hydrateActualAgentRun(zeroPublic.client, zeroPublic.child);
  const delivered: unknown[] = [];
  zeroPublic.client.onAgentRunStateChanged((event) => delivered.push(event));
  const terminal = structuredClone(g5bFixture("succeeded-state-notifies-hydration"));
  zeroPublic.child.stdout.write(agentRunStateChangedLine(terminal, "agent_run_01_stream", 1));
  assert.equal(delivered.length, 1, "terminal notification must not require public text first");
  zeroPublic.child.stdout.write(agentPublicEventLine({ ...g5aFixture("bounded-public-status-event"), eventId: "agent_event_after_zero_public_terminal" }, "agent_run_01_stream", 2));
  assert.match(zeroPublic.client.failure?.message ?? "", /after terminal/);
  assert.equal(zeroPublic.child.terminations, 1);

  const mutations: Array<{ name: string; payload?: Record<string, unknown>; envelope?: Partial<EventV1> }> = [
    { name: "fingerprint", payload: { ...agentRunStateChanged(measuredCapability().fingerprint), run: { ...agentRunStateChanged(measuredCapability().fingerprint).run as Record<string, unknown>, capabilityFingerprint: `sha256:${"1".repeat(64)}` } } },
    { name: "envelope", envelope: { chatId: "chat_other" } },
  ];
  for (const mutation of mutations) {
    const invalid = createClient();
    const started = invalid.client.start(); invalid.child.stdout.write(`${g5aReadyLine()}\n`); await started;
    await hydrateActualAgentRun(invalid.client, invalid.child);
    let stateDelivered = 0;
    invalid.client.onAgentRunStateChanged(() => { stateDelivered += 1; });
    const payload = mutation.payload ?? structuredClone(g5bFixture("running-state-notifies-hydration"));
    const line = JSON.parse(agentRunStateChangedLine(payload)) as EventV1;
    invalid.child.stdout.write(eventLine({ ...line, ...mutation.envelope }));
    assert.equal(stateDelivered, 0, `${mutation.name} must fail before state listener delivery`);
    assert.equal(invalid.child.terminations, 1, `${mutation.name} must terminate once`);
  }

  const transition = createClient();
  const transitionStart = transition.client.start(); transition.child.stdout.write(`${g5aReadyLine()}\n`); await transitionStart;
  await hydrateActualAgentRun(transition.client, transition.child);
  transition.child.stdout.write(agentRunStateChangedLine(g5bFixture("running-state-notifies-hydration"), "agent_run_01_stream", 1));
  transition.child.stdout.write(agentRunStateChangedLine({ ...g5bFixture("running-state-notifies-hydration"), eventId: "agent_state_event_running_twice" }, "agent_run_01_stream", 2));
  assert.match(transition.client.failure?.message ?? "", /transition is invalid/);
  assert.equal(transition.child.terminations, 1);

  const replay = createClient();
  const replayStart = replay.client.start(); replay.child.stdout.write(`${g5aReadyLine()}\n`); await replayStart;
  await hydrateActualAgentRun(replay.client, replay.child);
  const running = g5bFixture("running-state-notifies-hydration");
  replay.child.stdout.write(agentRunStateChangedLine(running, "agent_run_01_stream", 1));
  replay.child.stdout.write(agentRunStateChangedLine(running, "agent_run_01_stream", 2));
  assert.equal(replay.child.terminations, 1, "eventId replay must poison once");

  const crossRun = createClient();
  const crossStart = crossRun.client.start(); crossRun.child.stdout.write(`${g5aReadyLine()}\n`); await crossStart;
  await hydrateActualAgentRun(crossRun.client, crossRun.child);
  const start = crossRun.client.startAgentRun({
    chatId: "chat_01", roleId: "planning", inputMessageIds: ["message_01"], expectedCapabilityFingerprint: measuredCapability().fingerprint,
  });
  const request = JSON.parse(crossRun.child.writes.at(-1) ?? "{}") as { requestId: string };
  crossRun.child.stdout.write(responseLine(request.requestId, acceptedAgentRun(measuredCapability().fingerprint)));
  await start;
  crossRun.child.stdout.write(agentRunStateChangedLine(g5bFixture("running-state-notifies-hydration"), "agent_cross_run_stream", 1));
  crossRun.child.stdout.write(agentRunStateChangedLine(agentRunStateChanged(measuredCapability().fingerprint), "agent_cross_run_stream", 2));
  assert.equal(crossRun.child.terminations, 1, "one actual-agent stream cannot change run binding");
});

test("G5B Electron registries enforce each shared session limit at the exact bound and poison at plus one", async () => {
  const active = createClient();
  const activeStart = active.client.start(); active.child.stdout.write(`${g5aReadyLine()}\n`); await activeStart;
  await hydrateActualAgentRun(active.client, active.child);
  const input = { chatId: "chat_01", roleId: "planning" as const, inputMessageIds: ["message_01"], expectedCapabilityFingerprint: measuredCapability().fingerprint };
  for (let index = 0; index < AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1 - 1; index += 1) {
    const pending = active.client.startAgentRun(input);
    const request = JSON.parse(active.child.writes.at(-1) ?? "{}") as { requestId: string };
    active.child.stdout.write(responseLine(request.requestId, acceptedAgentRun(input.expectedCapabilityFingerprint, { runId: `agent_run_active_${index}` })));
    await pending;
  }
  const overflow = active.client.startAgentRun(input);
  const overflowRequest = JSON.parse(active.child.writes.at(-1) ?? "{}") as { requestId: string };
  active.child.stdout.write(responseLine(overflowRequest.requestId, acceptedAgentRun(input.expectedCapabilityFingerprint, { runId: "agent_run_active_overflow" })));
  await assert.rejects(overflow, /active run session budget exceeded/);
  assert.equal(active.child.terminations, 1);

  const tombstones = createClient();
  const tombstoneStart = tombstones.client.start(); tombstones.child.stdout.write(`${g5aReadyLine()}\n`); await tombstoneStart;
  await hydrateTerminalRuns(tombstones.client, tombstones.child, AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1);
  const extra = tombstones.client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const extraRequest = JSON.parse(tombstones.child.writes.at(-1) ?? "{}") as { requestId: string };
  tombstones.child.stdout.write(responseLine(extraRequest.requestId, terminalHydrationPage(AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1, 1)));
  await assert.rejects(extra, /seen runId session budget exceeded/);
  assert.equal(tombstones.child.terminations, 1, "terminal hydration remains tombstoned but cannot grow without bound");

  const eventIds = createClient();
  const eventStart = eventIds.client.start(); eventIds.child.stdout.write(`${g5aReadyLine()}\n`); await eventStart;
  await hydrateActualAgentRun(eventIds.client, eventIds.child);
  let delivered = 0;
  eventIds.client.onAgentPublicEvent(() => { delivered += 1; });
  for (let sequence = 1; sequence <= AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1; sequence += 1) {
    eventIds.child.stdout.write(agentPublicEventLine({ ...g5aFixture("bounded-public-status-event"), eventId: `agent_event_budget_${sequence}` }, "agent_event_budget_stream", sequence));
  }
  assert.equal(delivered, AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1);
  eventIds.child.stdout.write(agentPublicEventLine({ ...g5aFixture("bounded-public-status-event"), eventId: "agent_event_budget_overflow" }, "agent_event_budget_stream", AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1 + 1));
  assert.equal(eventIds.child.terminations, 1, "seen eventId plus one must poison once");

  const agentStreams = createClient();
  const agentStreamStart = agentStreams.client.start(); agentStreams.child.stdout.write(`${g5aReadyLine()}\n`); await agentStreamStart;
  const detail = agentStreams.client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const detailRequest = JSON.parse(agentStreams.child.writes.at(-1) ?? "{}") as { requestId: string };
  agentStreams.child.stdout.write(responseLine(detailRequest.requestId, plannedEmptyAgentDetail()));
  await detail;
  for (let index = 0; index < AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1; index += 1) {
    const pending = agentStreams.client.startAgentRun(input);
    const request = JSON.parse(agentStreams.child.writes.at(-1) ?? "{}") as { requestId: string };
    const runId = `agent_run_stream_budget_${index}`;
    agentStreams.child.stdout.write(responseLine(request.requestId, acceptedAgentRun(input.expectedCapabilityFingerprint, { runId })));
    await pending;
    const terminal = agentRunStateChanged(input.expectedCapabilityFingerprint, {
      eventId: `agent_state_event_stream_budget_${index}`,
      run: { ...agentRunStateChanged(input.expectedCapabilityFingerprint).run as Record<string, unknown>, runId, state: "succeeded", terminationOutcome: "cleanExit", updatedAt: "2026-08-27T01:03:00Z" },
    });
    agentStreams.child.stdout.write(agentRunStateChangedLine(terminal, `agent_stream_budget_${index}`, 1));
  }
  assert.equal(agentStreams.client.failure, undefined, "terminal tombstones retain exact stream bindings through the shared bound");
  const agentStreamOverflow = agentStreams.client.startAgentRun(input);
  const agentStreamOverflowRequest = JSON.parse(agentStreams.child.writes.at(-1) ?? "{}") as { requestId: string };
  agentStreams.child.stdout.write(responseLine(agentStreamOverflowRequest.requestId, acceptedAgentRun(input.expectedCapabilityFingerprint, { runId: "agent_run_stream_budget_overflow" })));
  await assert.rejects(agentStreamOverflow, /seen runId session budget exceeded/);
  assert.equal(agentStreams.child.terminations, 1, "the stream/tombstone shared plus-one boundary must poison once");

  const protocolStreams = createClient();
  const streamStart = protocolStreams.client.start(); protocolStreams.child.stdout.write(`${g5aReadyLine()}\n`); await streamStart;
  for (let index = 0; index < PROTOCOL_STREAM_IDS_PER_SESSION_MAX_V1 - 1; index += 1) {
    protocolStreams.child.stdout.write(eventLine({
      protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: `budget_stream_${index}`, seq: 1,
      event: "system.fatal", payload: { code: "bounded" },
    }));
  }
  assert.equal(protocolStreams.client.failure, undefined);
  protocolStreams.child.stdout.write(eventLine({
    protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "budget_stream_overflow", seq: 1,
    event: "system.fatal", payload: { code: "overflow" },
  }));
  assert.equal(protocolStreams.child.terminations, 1, "protocol streamId plus one must poison once");
});

test("G5A keeps actual and fixture ledgers separate and fails closed on raw agent events", async () => {
  const valid = createClient();
  const validStart = valid.client.start(); valid.child.stdout.write(`${g5aReadyLine()}\n`); await validStart;
  const detail = valid.client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const detailRequest = JSON.parse(valid.child.writes[0] ?? "{}") as { requestId: string };
  valid.child.stdout.write(responseLine(detailRequest.requestId, g5aFixture("actual-agent-chat-page-keeps-distinct-ledger")));
  assert.equal((await detail).chat.chatId, "chat_01");

  const crossed = createClient();
  const crossedStart = crossed.client.start(); crossed.child.stdout.write(`${g5aReadyLine()}\n`); await crossedStart;
  const badDetail = crossed.client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const badRequest = JSON.parse(crossed.child.writes[0] ?? "{}") as { requestId: string };
  crossed.child.stdout.write(responseLine(badRequest.requestId, g5aFixture("agent-chat-page-rejects-cross-project-run", false)));
  await assert.rejects(badDetail, /identity/);
  assert.equal(crossed.child.terminations, 1);

  const actualRunCarriers: Array<{ name: string; envelopeRunId?: string; payloadRunId: string | null; kind: "role" | "status"; roleId: "planning" | null }> = [
    { name: "payload", payloadRunId: "agent_run_payload", kind: "role", roleId: "planning" },
    { name: "envelope", envelopeRunId: "agent_run_envelope", payloadRunId: null, kind: "status", roleId: null },
  ];
  for (const carrier of actualRunCarriers) {
    const raw = createClient();
    const rawStart = raw.client.start(); raw.child.stdout.write(`${g5aReadyLine()}\n`); await rawStart;
    let delivered = 0;
    raw.client.onChatEvent(() => { delivered += 1; });
    raw.child.stdout.write(eventLine({
      protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: `agent_raw_${carrier.name}`, seq: 1,
      event: "chat.eventAppended", chatId: "chat_01", taskId: "task_01", ...(carrier.envelopeRunId === undefined ? {} : { runId: carrier.envelopeRunId }),
      payload: {
        schemaVersion: 1, messageId: `message_${carrier.name}`, chatId: "chat_01", taskId: "task_01", worktreeId: "worktree_01",
        eventSequence: 1, kind: carrier.kind, roleId: carrier.roleId, runId: carrier.payloadRunId,
        text: "raw stdout and reasoning must never reach renderer", createdAt: "2026-08-27T00:00:00Z",
      },
    }));
    assert.match(raw.client.failure?.message ?? "", /actual-agent chat events are forbidden/);
    assert.equal(raw.child.terminations, 1, `${carrier.name} actual run carrier must poison the session`);
    assert.equal(delivered, 0, `${carrier.name} raw text must not be published through chat events`);
  }

  const mismatch = createClient();
  const mismatchStart = mismatch.client.start(); mismatch.child.stdout.write(`${g5aReadyLine()}\n`); await mismatchStart;
  let mismatchDelivered = 0;
  mismatch.client.onChatEvent(() => { mismatchDelivered += 1; });
  mismatch.child.stdout.write(eventLine({
    protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "chat_run_mismatch", seq: 1,
    event: "chat.eventAppended", chatId: "chat_01", taskId: "task_01", runId: "run_envelope",
    payload: {
      schemaVersion: 1, messageId: "message_mismatch", chatId: "chat_01", taskId: "task_01", worktreeId: "worktree_01",
      eventSequence: 1, kind: "role", roleId: "planning", runId: "run_payload",
      text: "mismatched raw run identity", createdAt: "2026-08-27T00:00:00Z",
    },
  }));
  assert.match(mismatch.client.failure?.message ?? "", /chat event runId does not match public message/);
  assert.equal(mismatch.child.terminations, 1);
  assert.equal(mismatchDelivered, 0, "mismatched run carriers must never be published");
});

test("current G5 chat detail remains valid without an actual-agent ledger", async () => {
  const { child, client } = createClient();
  const starting = client.start(); child.stdout.write(`${readyLine()}\n`); await starting;
  const detail = client.getChat({ chatId: "chat_01", afterEventSequence: 0, cursor: null, limit: 50 });
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(request.requestId, g5Fixture("chat-page-explicit-identity-and-sequence")));
  const result = await detail;
  assert.equal(result.chat.chatId, "chat_01");
  assert.equal("agentRuns" in result, false, "G5 detail must not require planned G5A agentRuns");
});

test("G5A cancel and recovery responses retain the exact actual-agent namespace", async () => {
  const { child, client } = createClient();
  const starting = client.start(); child.stdout.write(`${g5aReadyLine()}\n`); await starting;
  const cancelFixture = g5aFixture("actual-agent-clean-cancel-result");
  const cancelRequest = cancelFixture.request as Record<string, unknown>;
  const cancel = client.cancelAgentRun(cancelRequest as never);
  const cancelEnvelope = JSON.parse(child.writes[0] ?? "{}") as { requestId: string; method: string; params: unknown };
  assert.equal(cancelEnvelope.method, "run.cancelAgent");
  assert.deepEqual(cancelEnvelope.params, cancelRequest);
  child.stdout.write(responseLine(cancelEnvelope.requestId, cancelFixture));
  assert.equal((await cancel).outcome, "canceled");

  const recoveryFixture = g5aFixture("actual-agent-recovery-resolves-only-after-process-proof");
  const recoveryRequest = recoveryFixture.request as Record<string, unknown>;
  const recovery = client.resolveAgentRecovery(recoveryRequest as never);
  const recoveryEnvelope = JSON.parse(child.writes[1] ?? "{}") as { requestId: string; method: string; params: unknown };
  assert.equal(recoveryEnvelope.method, "recovery.resolveAgent");
  assert.deepEqual(recoveryEnvelope.params, recoveryRequest);
  child.stdout.write(responseLine(recoveryEnvelope.requestId, recoveryFixture));
  assert.equal((await recovery).outcome, "resolvedAbandoned");
});

test("a partial spawned process is terminated before any pipe listener can leak", async () => {
  const child = new FixtureSidecar();
  let spawns = 0;
  const client = new SidecarClientV1(
    { command: "fixture", args: ["--stdio"], cwd: "/private/tmp" },
    {
      spawn: () => {
        spawns += 1;
        return {
          stdin: child.stdin,
          stdout: child.stdout,
          stderr: null,
          once: (event, listener) => child.once(event, listener),
          terminate: () => child.terminate(),
        } as unknown as SidecarProcessV1;
      },
    },
  );
  await assert.rejects(client.start(), /stdio pipes are unavailable/);
  assert.equal(spawns, 1);
  assert.equal(child.terminations, 1);
  assert.equal(child.stdout.listenerCount("data"), 0);
  assert.equal(child.stderr.listenerCount("data"), 0);
  await assert.rejects(client.start(), /stdio pipes are unavailable/);
  assert.equal(spawns, 1, "partial startup is never retried automatically");
});

test("unknown response and abrupt exit reject pending work without restart or retry", async () => {
  const { child, client, spawns } = createClient();
  const start = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await start;

  const pending = client.getSystemStatus();
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "response",
    sidecarInstanceId: "sidecar_test_01",
    requestId: `${request.requestId}_wrong`,
    ok: true,
    result: {},
  })}\n`);
  await assert.rejects(pending, /unknown requestId|no pending request/);
  assert.equal(child.terminations, 1);
  assert.equal(spawns(), 1);
  await assert.rejects(client.getSystemStatus(), /unknown requestId|no pending request/);
  assert.equal(child.writes.length, 1, "the failed request must not be retried automatically");
});

test("abrupt sidecar exit rejects every pending request", async () => {
  const { child, client } = createClient();
  const start = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await start;
  const pending = client.prepareQuit();
  child.emit("exit", 9, null);
  await assert.rejects(pending, /sidecar exited/);
  assert.equal(child.terminations, 1);
});

test("system.status identity mismatch and stderr tails corrupt the session without leaking control bytes", async () => {
  const identity = createClient();
  const started = identity.client.start();
  identity.child.stdout.write(`${readyLine()}\n`);
  await started;
  const pending = identity.client.getSystemStatus();
  const request = JSON.parse(identity.child.writes[0] ?? "{}") as { requestId: string };
  identity.child.stdout.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "response",
    sidecarInstanceId: "sidecar_test_01",
    requestId: request.requestId,
    ok: true,
    result: {
      state: "ready",
      sidecar: {
        protocolVersion: 1,
        sidecarSchemaVersion: 1,
        appSchemaVersion: 1,
        pid: 99,
        platform: "darwin",
        capabilities: [...G5_CAPABILITIES_V1],
        sidecarInstanceId: "sidecar_test_02",
      },
      database: { health: "healthy", schemaVersion: 1 },
      recoveryRequired: false,
      blockerIds: [],
    },
  })}\n`);
  await assert.rejects(pending, /identity does not match/);
  assert.equal(identity.child.terminations, 1);

  const diagnostics = createClient();
  const starting = diagnostics.client.start();
  diagnostics.child.stderr.write(`\u0001${"x".repeat(33 * 1024)}\r\nmigration tail`);
  diagnostics.child.emit("exit", 70, null);
  await assert.rejects(starting, /sidecar exited/);
  assert.ok(diagnostics.client.technicalDetail.endsWith("\nmigration tail"));
  assert.ok(!diagnostics.client.technicalDetail.includes("\u0001"));
  assert.ok(Buffer.byteLength(diagnostics.client.technicalDetail) <= 32 * 1024 + 2);
});

test("G5 recovery-ready status stays connected for recovery APIs and unsafe quit remains blocked", async () => {
  const { child, client } = createClient();
  const started = client.start(); child.stdout.write(`${readyLine()}\n`); await started;
  const status = client.getSystemStatus();
  const statusRequest = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(statusRequest.requestId, {
    state: "ready",
    sidecar: { protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 42, platform: "darwin", capabilities: [...G5_CAPABILITIES_V1], sidecarInstanceId: "sidecar_test_01" },
    database: { health: "healthy", schemaVersion: 1 }, recoveryRequired: true, blockerIds: ["recovery_01"],
  }));
  assert.deepEqual((await status).blockerIds, ["recovery_01"]);
  const recoveries = client.listRecoveries({ cursor: null, limit: 1 });
  const recoveryRequest = JSON.parse(child.writes[1] ?? "{}") as { requestId: string; method: string };
  assert.equal(recoveryRequest.method, "recovery.list");
  child.stdout.write(responseLine(recoveryRequest.requestId, { items: [], nextCursor: null }));
  await recoveries;

  const exits: string[] = [];
  const coordinator = new QuitCoordinatorV1({
    prepareSidecarQuit: async () => ({ safeToQuit: false, blockers: ["recovery_01"] }),
    failedBeforeReady: () => false,
    allowProcessExit: (kind) => exits.push(kind),
  });
  assert.deepEqual(await coordinator.prepareQuit(), { safeToQuit: false, blockers: ["recovery_01"] });
  assert.equal(coordinator.exitAllowed, false);
  assert.deepEqual(exits, []);
});

test("one quit waits for both login workers to confirm cleanup", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const exits: string[] = [];
  let calls = 0;
  const coordinator = new QuitCoordinatorV1({
    prepareSidecarQuit: async () => {
      calls += 1;
      const blockers = calls === 1 ? ["provider-login:codex", "provider-login:claude"]
        : calls === 2 ? ["provider-login:claude"] : [];
      return { safeToQuit: blockers.length === 0, blockers };
    },
    failedBeforeReady: () => false,
    allowProcessExit: (kind) => exits.push(kind),
  });
  coordinator.handleBeforeQuit();
  const pending = coordinator.prepareQuit(); // joins the same single-quit operation
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(calls, 1);
  assert.deepEqual(exits, []);
  t.mock.timers.tick(250);
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(calls, 2);
  assert.deepEqual(exits, []);
  t.mock.timers.tick(250);
  assert.equal((await pending).safeToQuit, true);
  assert.equal(calls, 3);
  assert.deepEqual(exits, ["clean"]);
});

test("persistent login recovery remains blocked after the bounded quit retry budget", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let calls = 0;
  const exits: string[] = [];
  const coordinator = new QuitCoordinatorV1({
    prepareSidecarQuit: async () => {
      calls += 1;
      return { safeToQuit: false, blockers: ["provider-login:claude"] };
    },
    failedBeforeReady: () => false,
    allowProcessExit: (kind) => exits.push(kind),
  });
  coordinator.handleBeforeQuit();
  const pending = coordinator.prepareQuit();
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await new Promise<void>((resolve) => setImmediate(resolve));
    t.mock.timers.tick(250);
  }
  assert.equal((await pending).safeToQuit, false);
  assert.equal(calls, 41);
  assert.equal(coordinator.exitAllowed, false);
  assert.deepEqual(exits, []);
});

test("pre-ready startup failure permits one explicitly unclean app close without re-entering quit", () => {
  let sidecarCalls = 0;
  const exits: string[] = [];
  const coordinator = new QuitCoordinatorV1({
    prepareSidecarQuit: async () => {
      sidecarCalls += 1;
      return { safeToQuit: true, blockers: [] };
    },
    failedBeforeReady: () => true,
    allowProcessExit: (kind) => exits.push(kind),
  });
  coordinator.handleBeforeQuit();
  coordinator.handleBeforeQuit();
  assert.equal(sidecarCalls, 0, "failed startup must not falsely request a clean marker");
  assert.deepEqual(exits, ["uncleanStartupFailure"]);
  assert.equal(coordinator.exitAllowed, true);
});

test("G4 file streams correlate descriptors, chunks, hashes, and project watcher events", async () => {
  const { child, client } = createClient();
  const started = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await started;
  const watched: unknown[] = [];
  const unwatch = client.onProjectEvent((event) => watched.push(event));
  const read = client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(`${JSON.stringify({
    protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: request.requestId, ok: true,
    result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 5,
      version: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
      stream: { streamId: "read_stream_01", chunkBytes: 262144 } },
  })}\n`);
  await read;
  child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "read_stream_01", seq: 1, event: "blob.chunk", payload: { text: "hello" } }));
  child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "watch_stream_01", seq: 1, event: "file.changed", payload: { projectId: "project_01", event: "file.changed", paths: ["src/App.tsx"], rescanRequired: false } }));
  assert.equal(client.failure, undefined);
  assert.equal(watched.length, 1);
  unwatch();
  child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "watch_stream_01", seq: 2, event: "diff.changed", payload: { projectId: "project_01", event: "diff.changed", paths: ["src/App.tsx"], rescanRequired: false } }));
  assert.equal(watched.length, 1, "unsubscribe must remove the exact event wrapper");
});

test("G4 rejects unknown, oversized, and hash-mismatched blob streams without retry", async () => {
  const unknown = createClient();
  const unknownStart = unknown.client.start();
  unknown.child.stdout.write(`${readyLine()}\n`);
  await unknownStart;
  unknown.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "unknown_stream", seq: 1, event: "blob.chunk", payload: { text: "x" } }));
  assert.match(unknown.client.failure?.message ?? "", /unknown or completed stream/);
  assert.equal(unknown.child.terminations, 1);

  const mismatch = createClient();
  const mismatchStart = mismatch.client.start();
  mismatch.child.stdout.write(`${readyLine()}\n`);
  await mismatchStart;
  const pending = mismatch.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(mismatch.child.writes[0] ?? "{}") as { requestId: string };
  mismatch.child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: request.requestId, ok: true, result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 5, version: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824", stream: { streamId: "bad_hash_stream", chunkBytes: 262144 } } })}\n`);
  await pending;
  mismatch.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "bad_hash_stream", seq: 1, event: "blob.chunk", payload: { text: "world" } }));
  assert.match(mismatch.client.failure?.message ?? "", /hash does not match/);
  assert.equal(mismatch.child.terminations, 1);

  const oversized = createClient();
  const oversizedStart = oversized.client.start();
  oversized.child.stdout.write(`${readyLine()}\n`);
  await oversizedStart;
  const oversizedRead = oversized.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const oversizedRequest = JSON.parse(oversized.child.writes[0] ?? "{}") as { requestId: string };
  oversized.child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: oversizedRequest.requestId, ok: true, result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "too_much_text", chunkBytes: 262144 } } })}\n`);
  await oversizedRead;
  oversized.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "too_much_text", seq: 1, event: "blob.chunk", payload: { text: "xx" } }));
  assert.match(oversized.client.failure?.message ?? "", /exceeds declared stream size/);
  assert.equal(oversized.child.terminations, 1);

  const outOfOrder = createClient();
  const outOfOrderStart = outOfOrder.client.start();
  outOfOrder.child.stdout.write(`${readyLine()}\n`);
  await outOfOrderStart;
  const outOfOrderRead = outOfOrder.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const outOfOrderRequest = JSON.parse(outOfOrder.child.writes[0] ?? "{}") as { requestId: string };
  outOfOrder.child.stdout.write(`${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId: outOfOrderRequest.requestId, ok: true, result: { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "out_of_order_text", chunkBytes: 262144 } } })}\n`);
  await outOfOrderRead;
  outOfOrder.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "out_of_order_text", seq: 2, event: "blob.chunk", payload: { text: "x" } }));
  assert.match(outOfOrder.client.failure?.message ?? "", /expected seq 1, got 2/);
  assert.equal(outOfOrder.child.terminations, 1);

  const descriptorBound = createClient();
  const descriptorStart = descriptorBound.client.start();
  descriptorBound.child.stdout.write(`${readyLine()}\n`);
  await descriptorStart;
  const diff = descriptorBound.client.getDiffFile({ projectId: "project_01", path: "src/App.tsx" });
  const diffRequest = JSON.parse(descriptorBound.child.writes[0] ?? "{}") as { requestId: string };
  descriptorBound.child.stdout.write(responseLine(diffRequest.requestId, {
    projectId: "project_01", kind: "modified", path: "src/App.tsx", oldPath: null,
    baseVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
    worktreeVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
    byteLength: 10 * 1024 * 1024 + 1, stream: { streamId: "oversized_diff", chunkBytes: 262144 },
  }));
  await assert.rejects(diff, /exceeds 10 MiB client bound/);
  assert.equal(descriptorBound.child.terminations, 1);

  const exactDiff = createClient(); const exactStart = exactDiff.client.start(); exactDiff.child.stdout.write(`${readyLine()}\n`); await exactStart;
  const exact = exactDiff.client.getDiffFile({ projectId: "project_01", path: "src/App.tsx" }); const exactRequest = JSON.parse(exactDiff.child.writes[0] ?? "{}") as { requestId: string };
  exactDiff.child.stdout.write(responseLine(exactRequest.requestId, { projectId: "project_01", kind: "modified", path: "src/App.tsx", oldPath: null, baseVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", worktreeVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", byteLength: 10 * 1024 * 1024, stream: { streamId: "exact_diff", chunkBytes: 262144 } }));
  assert.equal((await exact).byteLength, 10 * 1024 * 1024);

  const tooLargeFile = createClient(); const fileStart = tooLargeFile.client.start(); tooLargeFile.child.stdout.write(`${readyLine()}\n`); await fileStart;
  const file = tooLargeFile.client.readFile({ projectId: "project_01", path: "src/App.tsx" }); const fileRequest = JSON.parse(tooLargeFile.child.writes[0] ?? "{}") as { requestId: string };
  tooLargeFile.child.stdout.write(responseLine(fileRequest.requestId, { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 5 * 1024 * 1024 + 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "too_large_file", chunkBytes: 262144 } }));
  await assert.rejects(file, /file classification must be tooLarge/);
});

test("G4 save responses are phase-specific and duplicate responses corrupt the session", async () => {
  const valid = createClient();
  const validStart = valid.client.start();
  valid.child.stdout.write(`${readyLine()}\n`);
  await validStart;
  const begin = valid.client.beginFileSave(saveBeginInput());
  const beginRequest = JSON.parse(valid.child.writes[0] ?? "{}") as { requestId: string };
  valid.child.stdout.write(responseLine(beginRequest.requestId, { saveId: "save_01", chunkBytes: 262144 }));
  assert.deepEqual(await begin, { saveId: "save_01", chunkBytes: 262144 });
  const chunk = valid.client.appendFileSaveChunk({ saveId: "save_01", seq: 1, text: "x" });
  const chunkRequest = JSON.parse(valid.child.writes[1] ?? "{}") as { requestId: string };
  valid.child.stdout.write(responseLine(chunkRequest.requestId, { acceptedSeq: 1 }));
  assert.deepEqual(await chunk, { acceptedSeq: 1 });
  const commit = valid.client.commitFileSave({ saveId: "save_01", expectedVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", lastSeq: 1, totalBytes: 1 });
  const commitRequest = JSON.parse(valid.child.writes[2] ?? "{}") as { requestId: string };
  valid.child.stdout.write(responseLine(commitRequest.requestId, { outcome: "dirtyConflict", error: { code: "E_DIRTY", kind: "dirtyConflict", detail: "file changed", retryable: false, outcomeUnknown: false } }));
  assert.equal((await commit).outcome, "dirtyConflict");

  const invalidPhase = createClient();
  const invalidStart = invalidPhase.client.start();
  invalidPhase.child.stdout.write(`${readyLine()}\n`);
  await invalidStart;
  const invalidBegin = invalidPhase.client.beginFileSave(saveBeginInput());
  const invalidRequest = JSON.parse(invalidPhase.child.writes[0] ?? "{}") as { requestId: string };
  invalidPhase.child.stdout.write(responseLine(invalidRequest.requestId, { acceptedSeq: 1 }));
  await assert.rejects(invalidBegin, /file\.save result is invalid/);
  assert.equal(invalidPhase.child.terminations, 1);

  for (const saveId of ["", "x".repeat(513)]) {
    const invalidId = createClient(); const invalidIdStart = invalidId.client.start(); invalidId.child.stdout.write(`${readyLine()}\n`); await invalidIdStart;
    const beginWithBadId = invalidId.client.beginFileSave(saveBeginInput()); const badIdRequest = JSON.parse(invalidId.child.writes[0] ?? "{}") as { requestId: string };
    invalidId.child.stdout.write(responseLine(badIdRequest.requestId, { saveId, chunkBytes: 262144 }));
    await assert.rejects(beginWithBadId, /file\.save result is invalid/); assert.equal(invalidId.child.terminations, 1);
  }

  const duplicate = createClient();
  const duplicateStart = duplicate.client.start();
  duplicate.child.stdout.write(`${readyLine()}\n`);
  await duplicateStart;
  const watch = duplicate.client.watchProjectFiles({ projectId: "project_01" });
  const duplicateRequest = JSON.parse(duplicate.child.writes[0] ?? "{}") as { requestId: string };
  const response = responseLine(duplicateRequest.requestId, { watching: true });
  duplicate.child.stdout.write(response);
  await watch;
  duplicate.child.stdout.write(response);
  assert.match(duplicate.client.failure?.message ?? "", /more than one response/);
  assert.equal(duplicate.child.terminations, 1);
});

test("G4 rejects a valid-shaped response for a different project or path", async () => {
  const { child, client } = createClient();
  const started = client.start();
  child.stdout.write(`${readyLine()}\n`);
  await started;
  const read = client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(request.requestId, {
    projectId: "project_02", path: "src/Other.tsx", classification: "utf8", byteLength: 1,
    version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
    stream: { streamId: "wrong_project_text", chunkBytes: 262144 },
  }));
  await assert.rejects(read, /projectId does not match request/);
  assert.equal(child.terminations, 1);
});

test("a stdin write EPIPE poisons every pending request without retry", async () => {
  const { child, client, spawns } = createClient();
  const started = client.start(); child.stdout.write(`${readyLine()}\n`); await started;
  const first = client.watchProjectFiles({ projectId: "project_01" });
  const firstRejected = assert.rejects(first, /EPIPE/);
  child.writeError = new Error("EPIPE");
  const second = client.getDiffSummary({ projectId: "project_01" });
  await firstRejected;
  await assert.rejects(second, /EPIPE/);
  assert.equal(child.terminations, 1); assert.equal(spawns(), 1);
  await assert.rejects(client.watchProjectFiles({ projectId: "project_01" }), /EPIPE/);
});

test("G4 bounds empty stream events and releases completed stream state", async () => {
  const bounded = createClient(); const start = bounded.client.start(); bounded.child.stdout.write(`${readyLine()}\n`); await start;
  const read = bounded.client.readFile({ projectId: "project_01", path: "src/App.tsx" });
  const request = JSON.parse(bounded.child.writes[0] ?? "{}") as { requestId: string };
  bounded.child.stdout.write(responseLine(request.requestId, { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "bounded", chunkBytes: 262144 } })); await read;
  for (let seq = 1; seq <= 257; seq += 1) bounded.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "bounded", seq, event: "blob.chunk", payload: { text: "" } }));
  assert.match(bounded.client.failure?.message ?? "", /exceeds 256 events/); assert.equal(bounded.child.terminations, 1);

  const complete = createClient(); const completeStart = complete.client.start(); complete.child.stdout.write(`${readyLine()}\n`); await completeStart;
  const completeRead = complete.client.readFile({ projectId: "project_01", path: "src/App.tsx" }); const completeRequest = JSON.parse(complete.child.writes[0] ?? "{}") as { requestId: string };
  complete.child.stdout.write(responseLine(completeRequest.requestId, { projectId: "project_01", path: "src/App.tsx", classification: "utf8", byteLength: 1, version: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", stream: { streamId: "completed", chunkBytes: 262144 } })); await completeRead;
  complete.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "completed", seq: 1, event: "blob.chunk", payload: { text: "x" } }));
  complete.child.stdout.write(eventLine({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "completed", seq: 2, event: "blob.chunk", payload: { text: "" } }));
  assert.match(complete.client.failure?.message ?? "", /unknown or completed/); assert.equal(complete.child.terminations, 1);
});

test("BridgeV2 runtime validators reject malformed and extra input before IPC", () => {
  assert.throws(() => validateTreeInput({ projectId: "project_01", parentPath: "../escape", cursor: null }));
  assert.throws(() => validatePathInput({ projectId: "project_01", path: "a", extra: true }));
  assert.throws(() => validateSaveBeginInput({ ...saveBeginInput(), totalBytes: 5 * 1024 * 1024 + 1 }));
  assert.throws(() => validateSaveChunkInput({ saveId: "save_01", seq: 0, text: "x" }));
  assert.throws(() => validateSaveCommitInput({ saveId: "save_01", expectedVersion: "sha256:bad", lastSeq: 1, totalBytes: 1 }));
  assert.deepEqual(validatePathInput({ projectId: "project_01", path: "src/App.tsx" }), { projectId: "project_01", path: "src/App.tsx" });
});

test("renderer event credit bounds stalled delivery and drains only exact acknowledgements", () => {
  let failure: Error | undefined; const credit = new EventCreditV1((error) => { failure = error; }); let sent = 0;
  const event = (seq: number) => ({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "credit", seq, event: "blob.chunk", payload: { text: "" } } as EventV1);
  for (let seq = 1; seq <= MAX_IN_FLIGHT_EVENTS; seq += 1) credit.deliver(event(seq), () => { sent += 1; });
  credit.deliver(event(MAX_IN_FLIGHT_EVENTS + 1), () => { sent += 1; });
  assert.equal(sent, MAX_IN_FLIGHT_EVENTS); assert.match(failure?.message ?? "", /consumerTooSlow/);
  let normalFailure: Error | undefined; const normal = new EventCreditV1((error) => { normalFailure = error; }); normal.deliver(event(1), () => {}); normal.acknowledge({ streamId: "credit", seq: 1 }); normal.deliver(event(2), () => {}); assert.equal(normalFailure, undefined);
  normal.acknowledge({ streamId: "credit", seq: 1 }); assert.match(normalFailure?.message ?? "", /consumerTooSlow/);
});

test("renderer close/reopen credit lifecycle fails only with unacknowledged delivery", () => {
  let failure: Error | undefined; const stalled = new EventCreditV1((error) => { failure = error; });
  stalled.deliver({ protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "close", seq: 1, event: "blob.chunk", payload: { text: "" } }, () => {});
  const outstanding = stalled.outstanding; assert.equal(outstanding, 1); if (Number(outstanding) !== 0) failure = new Error("rendererDisconnected");
  assert.match(failure?.message ?? "", /rendererDisconnected/);
  const clean = new EventCreditV1(() => assert.fail("clean close must not fail")); assert.equal(clean.outstanding, 0); assert.equal(clean.resetIfEmpty(), true);
});

test("main, preload, and package boundary retain fixed channels, origin checks, and an external sidecar", () => {
  const main = source("electron/main.ts");
  const sidecarClient = source("electron/sidecar-client.ts");
  const preload = source("electron/preload.ts");
  const forge = source("electron/forge.config.ts");
  const smoke = source("scripts/m5-electron-package-smoke.mjs");

  for (const required of [
    "event.sender !== window.webContents",
    "event.senderFrame !== event.sender.mainFrame",
    "isAllowedNavigation(event.senderFrame.url)",
    "process.resourcesPath",
    "hello-agent-sidecar",
    "args: [\"--stdio\"]",
    "app.getPath(\"userData\")",
    "com.wonseokhan.helloagent",
    "app.on(\"before-quit\"",
    "quitCoordinator.handleBeforeQuit()",
  ]) assert.ok(main.includes(required), `missing ${required}`);
  assert.match(sidecarClient, /shell: false/);
  assert.match(sidecarClient, /MAX_SIDECAR_STDERR_TAIL_BYTES = 32 \* 1024/);
  assert.match(sidecarClient, /terminate\(\): void/);
  assert.match(main, /kind: "sidecarUnavailable"/);
  assert.doesNotMatch(main, /process\.env/);
  assert.match(preload, /ipcRenderer\.invoke\(M5_INTERNAL_CHANNELS_V1\.systemGetStatus\)/);
  assert.match(preload, /ipcRenderer\.invoke\(M5_INTERNAL_CHANNELS_V1\.appPrepareQuit\)/);
  assert.match(preload, /ipcRenderer\.on\(M5_INTERNAL_CHANNELS_V1\.sidecarEvent/);
  assert.match(preload, /ipcRenderer\.send\(M5_BRIDGE_V2_CHANNELS\.eventAck/);
  assert.match(main, /ipcMain\.on\(M5_BRIDGE_V2_CHANNELS\.eventAck/);
  assert.match(main, /sidecar\?\.failRendererDisconnected\(\)/);
  assert.match(main, /quitCoordinator\.exitAllowed/);
  assert.match(main, /dialog\.showOpenDialog/);
  assert.match(main, /properties: \["openDirectory"\]/);
  assert.match(main, /return \{ outcome: "cancelled" \}/);
  assert.match(preload, /new DOMException\("Project selection cancelled", "AbortError"\)/);
  assert.match(preload, /validateProjectIdentityV1\(result\)/);
  assert.doesNotMatch(preload, /openProject:\s*\([^)]*input/);
  assert.doesNotMatch(preload, /selectedPath/);
  assert.doesNotMatch(preload, /exposeInMainWorld\([^\n]+ipcRenderer|sendSync|sendTo|postMessage/);
  assert.match(forge, /packageAfterCopy/);
  assert.match(forge, /m5-stage-sidecar\.mjs/);
  assert.match(smoke, /sidecar/);
  assert.match(smoke, /must remain outside app\.asar/);
  assert.match(smoke, /assert\.deepEqual\(status\.result\.sidecar, ready\.payload\)/);
  assert.match(smoke, /assertCleanMarker\(dataDirectory, ready\.sidecarInstanceId\)/);
  assert.match(smoke, /await stopChild\(child\)/);
  assert.match(smoke, /await withDeadline\(exited, 3_000, "failed packaged sidecar cleanup"\)/);
});

test("G5 V3 bridge keeps every workspace/chat action fixed, typed, and isolated", () => {
  const main = source("electron/main.ts");
  const preload = source("electron/preload.ts");
  const channels = source("electron/ipc-channels.ts");
  const client = source("electron/sidecar-client.ts");
  for (const name of [
    "workspaceGetTree", "workspaceClose", "workspaceFileRead", "workspaceFileSaveBegin",
    "workspaceFileSaveChunk", "workspaceFileSaveCommit", "workspaceFileWatch",
    "workspaceDiffGetSummary", "workspaceDiffGetFile", "projectGetRoleSettings",
    "workspacePrepareChat", "workspaceAbandonPrepared",
    "projectUpdateRoleSettings", "chatCreate", "chatList", "chatGet", "runStartFixture",
    "runCancel", "recoveryList", "recoveryInspect", "recoveryMarkResolved",
  ]) {
    assert.match(channels, new RegExp(`${name}: "m5:v3:`));
    assert.match(main, new RegExp(`M5_BRIDGE_V3_CHANNELS\\.${name}`));
    assert.match(preload, new RegExp(`M5_BRIDGE_V3_CHANNELS\\.${name}`));
  }
  assert.match(client, /parseG5ReadyPayloadV1/);
  assert.match(client, /workspace save references an unknown or late saveId/);
  assert.match(client, /assertWorkspaceResponse/);
  assert.match(client, /validateTeamMessageV1/);
  assert.doesNotMatch(preload, /run\.startAgentSmoke|ipcRenderer\.sendSync|ipcRenderer\.invoke\([^M]/);
  assert.doesNotMatch(channels, /generic|invokeAll|startAgentSmoke/);
  assert.throws(() => validateWorkspaceTreeInput({ projectId: "p", parentPath: "../escape", cursor: null, chatId: "c", worktreeId: "w", workspaceIdentity: "i" }));
  assert.throws(() => validateWorkspaceSaveChunkInput({ chatId: "c", projectId: "p", worktreeId: "w", workspaceIdentity: "i", saveId: "s", seq: 1, text: "x", extra: true }));
  assert.throws(() => validateWorkspaceSaveCommitInput({ chatId: "c", projectId: "p", worktreeId: "w", workspaceIdentity: "i", saveId: "s", expectedVersion: "sha256:bad", lastSeq: 1, totalBytes: 1 }));
});

test("G5A bridge exposes only fixed adapter channels with the existing main-frame boundary", () => {
  const main = source("electron/main.ts");
  const preload = source("electron/preload.ts");
  const channels = source("electron/ipc-channels.ts");
  for (const name of ["agentGetCapabilities", "agentStartRun", "agentCancelRun", "agentRecoveryInspect", "agentRecoveryResolve", "agentPublicEvent"]) {
    assert.match(channels, new RegExp(`${name}: "m5:v4:`));
    assert.match(main, new RegExp(`M5_BRIDGE_V4_CHANNELS\\.${name}`));
    assert.match(preload, new RegExp(`M5_BRIDGE_V4_CHANNELS\\.${name}`));
  }
  assert.match(preload, /validateAgentRunStartRequestV1\(input\)/);
  assert.match(preload, /validateAgentRunCancelRequestV1\(input\)/);
  assert.match(preload, /validateAgentPublicEventV1\(payload\.event\)/);
  assert.match(main, /assertCurrentMainFrame\(event\)/);
  assert.match(main, /forwardAgentPublicEvent/);
  assert.match(main, /eventCredit\.deliver\(protocolEvent/);
  assert.doesNotMatch(channels, /generic|invokeAll|startAgentSmoke/);
  assert.doesNotMatch(preload, /ipcRenderer\.invoke\([^M]/);
});

test("G5B bridge adds only the fixed state hydration trigger and preserves V3/V4 channels and credit", () => {
  const main = source("electron/main.ts");
  const preload = source("electron/preload.ts");
  const channels = source("electron/ipc-channels.ts");
  const client = source("electron/sidecar-client.ts");
  assert.match(channels, /agentRunStateChanged: "m5:v5:agent:run-state-changed"/);
  assert.match(main, /M5_BRIDGE_V5_CHANNELS\.agentRunStateChanged/);
  assert.match(main, /forwardAgentRunStateChanged/);
  assert.match(main, /eventCredit\.deliver\(protocolEvent/);
  assert.match(preload, /onAgentRunStateChanged: \(listener\) => subscribe\(agentRunStateChangedListeners, listener\)/);
  assert.match(preload, /validateAgentRunStateChangedV1\(payload\.event\)/);
  assert.match(client, /AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1/);
  assert.match(client, /AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1/);
  assert.match(client, /AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1/);
  assert.match(client, /AGENT_STREAM_IDS_PER_SESSION_MAX_V1/);
  assert.match(client, /#agentRunStreams/);
  assert.match(client, /agent public event arrived after terminal actual-agent state/);
  assert.doesNotMatch(channels, /generic|invokeAll|startAgentSmoke/);
  assert.doesNotMatch(preload, /ipcRenderer\.invoke\([^M]/);
});

test("G5 preparation uses exact IDs, binds chat creation to a known preparation, and never falls back", async () => {
  const { child, client } = createClient();
  const started = client.start(); child.stdout.write(`${readyLine()}\n`); await started;
  const prepared = client.prepareChatWorkspace({ projectId: "project_01" });
  const prepareRequest = JSON.parse(child.writes[0] ?? "{}") as { requestId: string; method: string; params: unknown };
  assert.equal(prepareRequest.method, "workspace.prepareChat");
  assert.deepEqual(prepareRequest.params, { projectId: "project_01" });
  child.stdout.write(responseLine(prepareRequest.requestId, preparedWorkspace("preparation_a")));
  await prepared;

  await assert.rejects(client.abandonPreparedWorkspace({
    preparationId: "preparation_a", projectId: "project_01", worktreeId: "worktree_other", workspaceIdentity: "workspace_a",
  }), /prepared workspace worktreeId does not match request/);
  await assert.rejects(client.createChat({
    preparationId: "preparation_other", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a", goal: "create", expectedSettingsRevision: 1,
  }), /unknown or no longer unbound/);
  assert.equal(child.writes.length, 1, "cross-preparation cleanup or chat creation must not reach the sidecar");

  const abandon = client.abandonPreparedWorkspace({
    preparationId: "preparation_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a",
  });
  const abandonRequest = JSON.parse(child.writes[1] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(abandonRequest.requestId, {
    preparationId: "preparation_a", projectId: "project_01", worktreeId: "worktree_b", workspaceIdentity: "workspace_a", bindingState: "abandoned", physicallyDeleted: false,
  }));
  await assert.rejects(abandon, /prepared workspace worktreeId does not match request/);
  assert.equal(child.terminations, 1, "a mismatched abandon response corrupts without retry");
});

test("G5 preparation validators reject project-only abandon, extra IDs, and UTF-8 overflow before IPC", () => {
  assert.throws(() => validatePrepareChatWorkspaceInput({ projectId: "project_01", extra: true }));
  assert.throws(() => validateAbandonPreparedWorkspaceInput({ projectId: "project_01" }));
  for (const id of ["a".repeat(513), "😀".repeat(129)]) {
    assert.throws(() => validatePrepareChatWorkspaceInput({ projectId: id }));
    assert.throws(() => validateAbandonPreparedWorkspaceInput({ preparationId: id, projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a" }));
  }
});

test("G5 rejects cross-chat workspace save continuation before it reaches the sidecar", async () => {
  const { child, client } = createClient();
  const started = client.start(); child.stdout.write(`${readyLine()}\n`); await started;
  const begin = client.beginWorkspaceFileSave({
    chatId: "chat_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a",
    path: "src/App.tsx", expectedVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", totalBytes: 1, encoding: "utf8",
  });
  const request = JSON.parse(child.writes[0] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(request.requestId, {
    chatId: "chat_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a", saveId: "save_a", chunkBytes: 262144,
  }));
  await begin;
  await assert.rejects(client.appendWorkspaceFileSaveChunk({
    chatId: "chat_b", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a", saveId: "save_a", seq: 1, text: "x",
  }), /workspace response chatId does not match request/);
  assert.equal(child.writes.length, 1, "cross-chat continuation must not be sent");
  const commit = client.commitWorkspaceFileSave({
    chatId: "chat_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a", saveId: "save_a",
    expectedVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881", lastSeq: 1, totalBytes: 1,
  });
  const commitRequest = JSON.parse(child.writes[1] ?? "{}") as { requestId: string };
  child.stdout.write(responseLine(commitRequest.requestId, {
    chatId: "chat_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a", saveId: "save_a", outcome: "dirtyConflict",
    error: { code: "E_DIRTY", kind: "dirtyConflict", detail: "changed", retryable: false, outcomeUnknown: false },
  }));
  await commit;
  await assert.rejects(client.appendWorkspaceFileSaveChunk({
    chatId: "chat_a", projectId: "project_01", worktreeId: "worktree_a", workspaceIdentity: "workspace_a", saveId: "save_a", seq: 2, text: "x",
  }), /unknown or late saveId/);
});

test("G5 keeps four independent chat/workspace streams ordered and explicitly addressed", async () => {
  const { child, client } = createClient();
  const started = client.start(); child.stdout.write(`${readyLine()}\n`); await started;
  const chats: unknown[] = []; const workspaces: unknown[] = []; const generic: unknown[] = [];
  client.onChatEvent((event) => chats.push(event));
  client.onWorkspaceEvent((event) => workspaces.push(event));
  client.onEvent((event) => generic.push(event));
  const message = (chatId: string, sequence: number) => eventLine({
    protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: `chat_stream_${chatId}`, seq: 1,
    event: "chat.eventAppended", chatId,
    payload: { schemaVersion: 1, messageId: `message_${chatId}`, chatId, taskId: `task_${chatId}`, worktreeId: `worktree_${chatId}`,
      eventSequence: sequence, kind: "status", roleId: null, runId: null, text: "safe public status", createdAt: "2026-08-26T00:00:00Z" },
  } as EventV1);
  const workspace = (chatId: string) => eventLine({
    protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: `workspace_stream_${chatId}`, seq: 1,
    event: "workspace.changed", payload: { chatId, projectId: "project_01", worktreeId: `worktree_${chatId}`,
      workspaceIdentity: `workspace_${chatId}`, event: "diff.changed", paths: ["src/App.tsx"], rescanRequired: false },
  } as EventV1);
  child.stdout.write(message("chat_a", 1)); child.stdout.write(workspace("chat_a"));
  child.stdout.write(message("chat_b", 1)); child.stdout.write(workspace("chat_b"));
  child.stdout.write(eventLine({
    protocolVersion: 1, kind: "event", sidecarInstanceId: "sidecar_test_01", streamId: "run_raw", seq: 1,
    event: "run.event", chatId: "chat_a", taskId: "task_chat_a", runId: "run_a", payload: { hiddenReasoning: "must not reach renderer" },
  }));
  assert.equal(client.failure, undefined);
  assert.deepEqual(chats.map((event) => (event as { chatId: string }).chatId), ["chat_a", "chat_b"]);
  assert.deepEqual(workspaces.map((event) => (event as { payload: { chatId: string } }).payload.chatId), ["chat_a", "chat_b"]);
  assert.deepEqual(generic, [], "workspace, public chat, and raw run events have no generic duplicate route");
});

test("G5 V3 IDs use the canonical UTF-8 byte limit before IPC", async () => {
  const tooLongAscii = "a".repeat(513); const tooLongEmoji = "😀".repeat(129);
  for (const id of [tooLongAscii, tooLongEmoji]) {
    assert.throws(() => validateRoleSettingsGetInput({ projectId: id }));
    assert.throws(() => validateRecoveryInspectInput({ recoveryId: id }));
  }
  const { child, client } = createClient();
  const started = client.start(); child.stdout.write(`${readyLine()}\n`); await started;
  assert.throws(() => client.getProjectRoleSettings({ projectId: tooLongEmoji }));
  assert.throws(() => client.inspectRecovery({ recoveryId: tooLongAscii }));
  assert.equal(child.writes.length, 0, "invalid V3 IDs must not reach the sidecar");
});

function source(path: string) {
  return readFileSync(resolve(path), "utf8");
}

function responseLine(requestId: string, result: Record<string, unknown>) {
  return `${JSON.stringify({ protocolVersion: 1, kind: "response", sidecarInstanceId: "sidecar_test_01", requestId, ok: true, result })}\n`;
}

function saveBeginInput() {
  return {
    projectId: "project_01",
    path: "src/App.tsx",
    expectedVersion: "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881" as const,
    totalBytes: 1,
    encoding: "utf8" as const,
  };
}

function preparedWorkspace(preparationId: string) {
  return {
    schemaVersion: 1,
    preparationId,
    manifest: {
      schemaVersion: 1, projectId: "project_01", canonicalGitRoot: "/fixtures/repository",
      worktreeId: "worktree_a", worktreePath: "/fixtures/workspaces/worktree_a",
      workspaceKind: "standaloneSnapshot", snapshotTransport: "gitBundle", sourceMutationPolicy: "forbidden",
      objectFormat: "sha1", baseCommit: "0123456789abcdef0123456789abcdef01234567",
      baseTree: "89abcdef0123456789abcdef0123456789abcdef", sourceIdentity: "source_repository_01",
      workspaceIdentity: "workspace_a", headMode: "detached", lifecycle: "active",
    },
    bindingState: "unbound", boundChatId: null, createdAt: "2026-08-26T01:00:00Z", updatedAt: "2026-08-26T01:00:00Z",
  };
}
