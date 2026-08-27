import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  AGENT_RUN_STATE_EVENT_STATES_V1,
  agentPublicLedgerRecordBytesV1,
  validateAgentRunStateChangedAgainstAcceptedV1,
  validateAgentRunStateChangedV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import { M5_BRIDGE_V4_METHODS } from "../src/m5/contracts/bridge-v4.ts";
import { M5_BRIDGE_V5_METHODS } from "../src/m5/contracts/bridge-v5.ts";
import {
  AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1,
  AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1,
  AGENT_PUBLIC_LEDGER_BYTES_PER_RUN_MAX_V1,
  AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1,
  AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1,
  AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1,
  AGENT_STREAM_IDS_PER_SESSION_MAX_V1,
  EVENTS_V1,
  PROTOCOL_STREAM_IDS_PER_SESSION_MAX_V1,
  ProtocolSessionV1,
  decodeJsonl,
  type EventV1,
} from "../src/m5/contracts/protocol-v1.ts";

interface MutationCase {
  name: string;
  baseValidName: string;
  pointer?: string;
  value?: unknown;
  changes?: { pointer: string; value: unknown }[];
  valid?: boolean;
}

interface G5BCases {
  accepted: unknown;
  valid: { name: string; value: unknown }[];
  invalid: MutationCase[];
  bindings: MutationCase[];
}

const fixtureRoot = resolve("tests/fixtures/m5/agent-adapter-v1");
const manifest = JSON.parse(readFileSync(resolve(fixtureRoot, "manifest.json"), "utf8"));
const cases = JSON.parse(readFileSync(resolve(fixtureRoot, manifest.g5bCasesFile), "utf8")) as G5BCases;

test("G5B shared resource budgets and deterministic ledger-byte unit match the manifest", () => {
  assert.deepEqual(manifest.contract.resourceLimits, {
    publicEventsPerRun: AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1,
    publicTextBytesPerRun: AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1,
    immutablePublicLedgerBytesPerRun: AGENT_PUBLIC_LEDGER_BYTES_PER_RUN_MAX_V1,
    activeActualRunsPerSession: AGENT_ACTIVE_RUNS_PER_SESSION_MAX_V1,
    seenActualRunIdsPerSession: AGENT_SEEN_RUN_IDS_PER_SESSION_MAX_V1,
    seenActualEventIdsPerSession: AGENT_SEEN_EVENT_IDS_PER_SESSION_MAX_V1,
    actualStreamIdsPerSession: AGENT_STREAM_IDS_PER_SESSION_MAX_V1,
    protocolStreamIdsPerSession: PROTOCOL_STREAM_IDS_PER_SESSION_MAX_V1,
    publicTextBytesDefinition: "sumUtf8BytesOfAgentPublicEventTextPerRun",
    immutablePublicLedgerBytesDefinition: "sumUtf8BytesOfRecursiveAsciiKeySortedCompactJsonAgentPublicEventRecordsPerRun;arrayOrderPreserved;explicitNullIncluded",
    canonicalPublicLedgerFixture: "bounded-public-status-event",
    canonicalPublicLedgerRecordBytes: 422,
  });
  const publicEvent = JSON.parse(readFileSync(resolve(fixtureRoot, manifest.casesFile), "utf8"))
    .valid.find((entry: { name: string }) => entry.name === manifest.contract.resourceLimits.canonicalPublicLedgerFixture).value;
  assert.equal(agentPublicLedgerRecordBytesV1(publicEvent), manifest.contract.resourceLimits.canonicalPublicLedgerRecordBytes);
});

test("runStateChanged accepts running and every terminal wire state", () => {
  assert.equal(manifest.contract.agentStateEventIdNamespace, "agent_state_event_");
  assert.deepEqual(manifest.contract.runStatePayloadFields, ["schemaVersion", "eventId", "run"]);
  assert.deepEqual(manifest.contract.runStateWireStates, [...AGENT_RUN_STATE_EVENT_STATES_V1]);
  assert.deepEqual(manifest.contract.runStateEnvelopeBindingFields, ["chatId", "taskId", "runId"]);
  assert.deepEqual(manifest.contract.runStateAcceptedBindingFields, [
    "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId", "roleId", "capabilityFingerprint",
  ]);
  assert.equal(manifest.contract.rendererRunStateSemantics, "hydrationTriggerOnly");
  assert.deepEqual(cases.valid.map((entry) => validateAgentRunStateChangedV1(entry.value).run.state), [
    ...AGENT_RUN_STATE_EVENT_STATES_V1,
  ]);
  for (const entry of cases.valid) {
    assert.doesNotThrow(() => validateAgentRunStateChangedAgainstAcceptedV1(entry.value, cases.accepted), entry.name);
  }
});

test("runStateChanged rejects namespace, hidden fields, forbidden phases, and active-terminal mismatch", () => {
  for (const entry of cases.invalid) {
    assert.throws(() => validateAgentRunStateChangedV1(materialize(entry)), entry.name);
  }
  for (const entry of cases.bindings) {
    const validate = () => validateAgentRunStateChangedAgainstAcceptedV1(materialize(entry), cases.accepted);
    if (entry.valid) assert.doesNotThrow(validate, entry.name);
    else assert.throws(validate, entry.name);
  }
});

test("M5BridgeV5 preserves V4 and exposes a hydration trigger only", () => {
  assert.deepEqual(M5_BRIDGE_V5_METHODS.slice(0, M5_BRIDGE_V4_METHODS.length), M5_BRIDGE_V4_METHODS);
  assert.deepEqual(M5_BRIDGE_V5_METHODS.slice(M5_BRIDGE_V4_METHODS.length), manifest.contract.g5bBridgeMethodsAdded);
  const source = readFileSync(resolve("src/m5/contracts/bridge-v5.ts"), "utf8");
  assert.match(source, /Omit<M5BridgeV4, "version">/);
  assert.match(source, /onAgentRunStateChanged\(listener: \(event: AgentRunStateChangedV1\) => void\): \(\) => void/);
  assert.match(source, /hydrate the durable chat\/run ledger/);
  assert.doesNotMatch(source, /ipcRenderer|genericInvoke|renderRun|appendMessage/);
});

test("public and state notifications share one exclusive run stream and event IDs never replay", () => {
  assert.ok(EVENTS_V1.includes("agent.runStateChanged"));
  assert.equal(manifest.contract.runStateProtocolEvent, "agent.runStateChanged");
  assert.equal(manifest.contract.runStateStreamSemantics, "sameExclusivePerRunStreamAsAgentPublicEvent");
  const lines = readFileSync(resolve("tests/fixtures/m5/protocol-v1/valid/agent-run-state-changed.jsonl"), "utf8")
    .trimEnd().split("\n").map((line) => decodeJsonl(`${line}\n`));
  const valid = new ProtocolSessionV1();
  for (const envelope of lines) valid.observe(envelope);

  const mismatched = new ProtocolSessionV1();
  mismatched.observe(lines[0]!);
  mismatched.observe(lines[1]!);
  const state = lines[2] as EventV1;
  assert.throws(() => mismatched.observe({ ...state, streamId: "stream_agent_run_other", seq: 1 }));

  assert.throws(() => valid.observe({ ...state, seq: 3 }), "state eventId replay must fail");
});

function materialize(entry: MutationCase): unknown {
  const base = cases.valid.find((candidate) => candidate.name === entry.baseValidName);
  assert.ok(base, entry.baseValidName);
  const value = structuredClone(base.value);
  if (entry.pointer !== undefined) setPointer(value, entry.pointer, entry.value);
  for (const change of entry.changes ?? []) setPointer(value, change.pointer, change.value);
  return value;
}

function setPointer(root: unknown, pointer: string, value: unknown): void {
  const segments = pointer.split("/").slice(1);
  let target = root as Record<string, unknown>;
  for (const segment of segments.slice(0, -1)) target = target[segment] as Record<string, unknown>;
  target[segments.at(-1)!] = value;
}
