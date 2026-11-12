import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  assertG5AContractV1,
  AGENT_CAPABILITY_BLOCK_REASONS_V1,
  AGENT_START_OUTCOMES_V1,
  canonicalAgentCapabilitySnapshotV1,
  computeAgentCapabilityFingerprintV1,
  G5A_CAPABILITIES_V1,
  isAgentChatDetailPageV1,
  parsePlannedG5AReadyPayloadV1,
  validateAgentCapabilitiesRequestV1,
  validateAgentCapabilitySnapshotV1,
  validateAgentChatDetailPageV1,
  validateAgentChatGetResultV1,
  validateAgentPublicEventV1,
  validateAgentPublicEventAgainstRunV1,
  validateAgentRecoveryInspectRequestV1,
  validateAgentRecoveryRecordV1,
  validateAgentRecoveryResolveRequestV1,
  validateAgentRecoveryResolveResultV1,
  validateAgentRunCancelRequestV1,
  validateAgentRunCancelResultV1,
  validateAgentRunTerminationPersistenceV1,
  validateAgentRunStartRequestV1,
  validateAgentRunAcceptedV1,
  validateAgentRunAcceptedAgainstContextV1,
  validateAgentRunRecordV1,
  validateAgentRunStartTranscriptV1,
  validateAgentTerminationTranscriptV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import type { AgentChatGetResultV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import { sha256FileVersionV1 } from "../src/m5/contracts/file-v1.ts";
import { M5_BRIDGE_V3_METHODS } from "../src/m5/contracts/bridge-v3.ts";
import { M5_BRIDGE_V4_METHODS } from "../src/m5/contracts/bridge-v4.ts";
import { decodeJsonl, EVENTS_V1, METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import { G5_CAPABILITIES_V1, G5_PUBLIC_MESSAGE_MAX_BYTES_V1, parseG5ReadyPayloadV1, validateRecoveryPageV1 } from "../src/m5/contracts/run-recovery-v1.ts";

type ValidatorName = "capabilitiesRequest" | "capabilitySnapshot" | "runStartRequest" | "runAccepted" | "runRecord" | "runStartTranscript" | "publicEvent" | "terminationTranscript" | "agentChatDetailPage" | "runCancelRequest" | "runCancelResult" | "runTerminationPersistence" | "recoveryRecord" | "recoveryInspectRequest" | "recoveryResolveRequest" | "recoveryResolveResult";
interface FixtureCase { name: string; validator: ValidatorName; value: unknown }
interface BindingCase { name: string; kind: "accepted" | "publicEvent"; valid: boolean; expected: unknown; value: unknown }
interface Cases { valid: FixtureCase[]; invalid: FixtureCase[]; bindings: BindingCase[] }
interface RecoveryDiscoveryCases {
  valid: { name: string; value: unknown }[];
  invalid: { name: string; baseValidName: string; removals: string[]; changes: { pointer: string; value: unknown }[] }[];
}
interface Manifest {
  schemaVersion: number;
  protocolVersion: number;
  contract: {
    capabilities: string[];
    bridgeMethodsAdded: string[];
    actualAgentAdvertisedByCurrentG5: boolean;
    agentSmokeAdvertised: boolean;
    fixtureRunSemantics: string;
    actualRunPersistence: string;
    executionMode: string;
    followUpMode: string;
    rendererLaunchFields: string;
    fingerprint: {
      algorithm: string; encoding: string; objectKeys: string; arrayOrder: string; explicitNull: string;
      excludedFields: string[]; canonicalFixture: string; canonicalDigest: string;
    };
    actualRunIdNamespace: string;
    actualRecoveryIdNamespace: string;
    actualEventIdNamespace: string;
    modelStartEvidence: string[];
    startOutcomes: string[];
    capabilityBlockReasons: string[];
    argvOnlyCanStart: boolean;
    publicEvents: string[];
    publicEventProtocolEvent: string;
    publicEventEnvelopeBindingFields: string[];
    acceptedBindingFields: string[];
    publicEventBindingFields: string[];
    preflightFailure: { spawnCount: number; lockCount: number };
    launcherBarrier: string[];
    uncertainTermination: string;
    chatDetailCapabilityDiscriminator: string;
  };
  casesFile: string;
  g5cRecoveryDiscoveryCasesFile: string;
}

const root = resolve("tests/fixtures/m5/agent-adapter-v1");
const manifest = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8")) as Manifest;
const cases = JSON.parse(readFileSync(resolve(root, manifest.casesFile), "utf8")) as Cases;
test("unobserved installation is unknown, never missing, authenticated or startable", () => {
  const value = JSON.parse(readFileSync(resolve(root, "unobserved-capability.json"), "utf8"));
  assert.equal(validateAgentCapabilitySnapshotV1(value).blockedReason, "capabilityUnknown");
  const withExecutable = {...value, executable:{pathDigest:"sha256:path",fileIdentityDigest:"sha256:file",cliVersion:"0.158.0"}};
  withExecutable.fingerprint = computeAgentCapabilityFingerprintV1(withExecutable);
  assert.throws(() => validateAgentCapabilitySnapshotV1(withExecutable), /installed state and executable identity must agree/);
  for (const change of [{canStart:true,blockedReason:null}, {blockedReason:"notInstalled"}, {authState:"authenticated"}, {discovery:{...value.discovery,source:"exactVersionManifest"}}, {models:[{modelSelector:"some-model",reasoningEfforts:[],permissions:["readOnly"]}]}]) {
    const changed = {...value,...change};
    changed.fingerprint = computeAgentCapabilityFingerprintV1(changed);
    assert.throws(() => validateAgentCapabilitySnapshotV1(changed));
  }
  const windows = {...value, platform:"win32", blockedReason:"platformUnsupported"};
  windows.fingerprint = computeAgentCapabilityFingerprintV1(windows);
  assert.equal(validateAgentCapabilitySnapshotV1(windows).canStart,false);
});
const recoveryDiscoveryCases = JSON.parse(
  readFileSync(resolve(root, manifest.g5cRecoveryDiscoveryCasesFile), "utf8"),
) as RecoveryDiscoveryCases;

test("G5A manifest는 기존 G5 handshake를 보존한 planned V4 계약만 고정한다", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.deepEqual(manifest.contract.capabilities, [...G5A_CAPABILITIES_V1]);
  assert.deepEqual(G5A_CAPABILITIES_V1.slice(0, G5_CAPABILITIES_V1.length), G5_CAPABILITIES_V1);
  assert.deepEqual(G5A_CAPABILITIES_V1.slice(-5), ["agent.getCapabilities", "run.startAgent", "run.cancelAgent", "recovery.inspectAgent", "recovery.resolveAgent"]);
  assert.ok(G5A_CAPABILITIES_V1.every((method) => METHODS_V1.includes(method)));
  assert.equal(manifest.contract.actualAgentAdvertisedByCurrentG5, false);
  assert.equal(manifest.contract.agentSmokeAdvertised, false);
  assert.equal(manifest.contract.fixtureRunSemantics, "fakeOnly");
  assert.equal(manifest.contract.actualRunPersistence, "separateAgentRunsLedger");
  assert.equal(G5A_CAPABILITIES_V1.includes("run.startAgentSmoke" as never), false);
  assertG5AContractV1();

  const currentReady = ready([...G5_CAPABILITIES_V1]);
  assert.doesNotThrow(() => parseG5ReadyPayloadV1(currentReady));
  assert.throws(() => parseG5ReadyPayloadV1(ready([...G5A_CAPABILITIES_V1])));
  assert.doesNotThrow(() => parsePlannedG5AReadyPayloadV1(ready([...G5A_CAPABILITIES_V1])));
});

test("M5BridgeV4는 V3를 보존하고 actual-agent typed 메서드만 추가한다", () => {
  assert.deepEqual(M5_BRIDGE_V4_METHODS.slice(0, M5_BRIDGE_V3_METHODS.length), M5_BRIDGE_V3_METHODS);
  assert.deepEqual(M5_BRIDGE_V4_METHODS.slice(M5_BRIDGE_V3_METHODS.length), manifest.contract.bridgeMethodsAdded);
  const source = readFileSync(resolve("src/m5/contracts/bridge-v4.ts"), "utf8");
  assert.match(source, /Omit<M5BridgeV3, "version" \| "getChat">/);
  assert.match(source, /getChat\(input: ChatGetRequestV1\): Promise<AgentChatGetResultV1>/);
  assert.match(source, /onAgentPublicEvent\(listener: \(event: AgentPublicEventV1\) => void\): \(\) => void/);
  assert.doesNotMatch(source, /startAgentSmoke|\bRunRecordV1\b|fixtureScenario|ipcRenderer|genericInvoke/);

  const planned = cases.valid.find((entry) => entry.validator === "agentChatDetailPage")?.value;
  assert.ok(planned && typeof planned === "object");
  const { agentRuns: _agentRuns, agentRecovery: _agentRecovery, ...current } = planned as Record<string, unknown>;
  assert.equal(countAgentRuns(validateAgentChatGetResultV1(current)), 0);
  assert.equal(countAgentRuns(validateAgentChatGetResultV1(planned)), 1);
  assert.equal(manifest.contract.chatDetailCapabilityDiscriminator, "agentRunsPresence");
});

test("agent chat detail은 unresolved actual recovery를 ID와 full identity로 발견하게 한다", () => {
  for (const fixture of recoveryDiscoveryCases.valid) {
    assert.doesNotThrow(() => validateAgentChatDetailPageV1(fixture.value), fixture.name);
  }
  for (const fixture of recoveryDiscoveryCases.invalid) {
    assert.throws(() => validateAgentChatDetailPageV1(materializeRecoveryDiscovery(fixture)), fixture.name);
  }
  const resolvedCase = recoveryDiscoveryCases.invalid.find(
    (fixture) => fixture.name === "fully-valid-resolved-recovery-cannot-remain-on-agent-chat",
  );
  assert.ok(resolvedCase);
  const resolvedDetail = materializeRecoveryDiscovery(resolvedCase) as Record<string, unknown>;
  assert.doesNotThrow(() => validateAgentRecoveryRecordV1(resolvedDetail.agentRecovery));
  assert.throws(() => validateAgentChatDetailPageV1(resolvedDetail));

  const recovered = recoveryDiscoveryCases.valid.find(
    (fixture) => fixture.name === "agent-chat-recovery-required-with-discoverable-recovery",
  )?.value as Record<string, unknown> | undefined;
  assert.ok(recovered?.agentRecovery);
  assert.throws(() => validateRecoveryPageV1({ items: [recovered.agentRecovery], nextCursor: null }));

  const durable = structuredClone(cases.valid.find(
    (fixture) => fixture.name === "actual-agent-recovery-resolves-only-after-process-proof",
  )?.value) as Record<string, unknown>;
  assert.ok(durable);
  (durable.request as Record<string, unknown>).recoveryId = "agent_recovery_random_substitute";
  assert.throws(() => validateAgentRecoveryResolveResultV1(durable));
});

test("TypeScript는 canonical actual-agent valid cases를 모두 승인한다", () => {
  for (const fixture of cases.valid) assert.doesNotThrow(() => validate(fixture), fixture.name);
});

test("TypeScript는 launch injection, 불확실 model, raw event, barrier 위반을 모두 거부한다", () => {
  for (const fixture of cases.invalid) assert.throws(() => validate(fixture), fixture.name);
});

test("accepted와 public event는 golden full identity와 capability fingerprint에 exact 결속된다", () => {
  for (const fixture of cases.bindings) {
    const validateBinding = () => fixture.kind === "accepted"
      ? validateAgentRunAcceptedAgainstContextV1(fixture.value, fixture.expected)
      : validateAgentPublicEventAgainstRunV1(fixture.value, fixture.expected);
    if (fixture.valid) assert.doesNotThrow(validateBinding, fixture.name);
    else assert.throws(validateBinding, fixture.name);
  }
});

test("agent.publicEvent envelope는 공개 payload와 chat/task/run identity를 exact 결속한다", () => {
  const payload = cases.valid.find((entry) => entry.name === "bounded-public-status-event")?.value;
  assert.ok(payload && typeof payload === "object");
  const envelope = {
    protocolVersion: 1,
    kind: "event",
    sidecarInstanceId: "sidecar_fixture_01",
    streamId: "stream_agent_public_01",
    seq: 1,
    event: "agent.publicEvent",
    chatId: "chat_01",
    taskId: "task_01",
    runId: "agent_run_01",
    payload,
  };
  assert.doesNotThrow(() => decodeJsonl(JSON.stringify(envelope)));
  assert.throws(() => validateAgentPublicEventV1({
    ...(payload as Record<string, unknown>),
    text: "a".repeat(G5_PUBLIC_MESSAGE_MAX_BYTES_V1 + 1),
  }));
  for (const field of ["chatId", "taskId", "runId"] as const) {
    const missing = { ...envelope } as Record<string, unknown>;
    delete missing[field];
    assert.throws(() => decodeJsonl(JSON.stringify(missing)), `missing ${field}`);
    assert.throws(
      () => decodeJsonl(JSON.stringify({ ...envelope, [field]: `${field}_other` })),
      `mismatched ${field}`,
    );
  }
});

test("manifest는 one-shot, next-run, redacted evidence와 fail-closed 종료를 명시한다", () => {
  assert.equal(manifest.contract.executionMode, "oneShot");
  assert.equal(manifest.contract.followUpMode, "nextRun");
  assert.equal(manifest.contract.rendererLaunchFields, "forbidden");
  assert.deepEqual(manifest.contract.fingerprint, {
    algorithm: "sha256", encoding: "utf8CompactJson", objectKeys: "recursiveAsciiAscending",
    arrayOrder: "preserved", explicitNull: "included", excludedFields: ["fingerprint", "observedAt"],
    canonicalFixture: "claude-exact-version-measured-capability",
    canonicalDigest: "sha256:93058dadc9163dcfbf9b8019ad1d980f017624e7c149b5f020085d2b22795fa3",
  });
  assert.equal(manifest.contract.actualRunIdNamespace, "agent_run_");
  assert.equal(manifest.contract.actualRecoveryIdNamespace, "agent_recovery_");
  assert.equal(manifest.contract.actualEventIdNamespace, "agent_event_");
  assert.deepEqual(manifest.contract.modelStartEvidence, ["runtimeVerified", "measuredExactVersion"]);
  assert.deepEqual(manifest.contract.startOutcomes, [...AGENT_START_OUTCOMES_V1]);
  assert.deepEqual(manifest.contract.capabilityBlockReasons, [...AGENT_CAPABILITY_BLOCK_REASONS_V1]);
  assert.equal(manifest.contract.argvOnlyCanStart, false);
  assert.deepEqual(manifest.contract.publicEvents, ["status", "question", "final"]);
  assert.equal(manifest.contract.publicEventProtocolEvent, "agent.publicEvent");
  assert.ok(EVENTS_V1.includes("agent.publicEvent"));
  assert.deepEqual(manifest.contract.publicEventEnvelopeBindingFields, ["chatId", "taskId", "runId"]);
  assert.deepEqual(manifest.contract.acceptedBindingFields, [
    "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "roleId", "capabilityFingerprint",
  ]);
  assert.deepEqual(manifest.contract.publicEventBindingFields, [
    "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "runId", "roleId", "capabilityFingerprint",
  ]);
  assert.deepEqual(manifest.contract.preflightFailure, { spawnCount: 0, lockCount: 0 });
  assert.deepEqual(manifest.contract.launcherBarrier, ["persistRunAndLock", "captureLauncherAndAgentIdentity", "attachTreeTracker", "persistIdentity", "release"]);
  assert.equal(manifest.contract.uncertainTermination, "recoveryRequiredAndPoisoned");
});

test("browser-safe SHA-256와 canonical capability digest가 Node oracle과 일치한다", () => {
  for (const vector of ["", "abc", "한글 capability"]) {
    const expected = `sha256:${createHash("sha256").update(vector, "utf8").digest("hex")}`;
    assert.equal(sha256FileVersionV1(new TextEncoder().encode(vector)), expected);
  }
  const fixture = cases.valid.find((entry) => entry.name === manifest.contract.fingerprint.canonicalFixture);
  assert.ok(fixture);
  const snapshot = fixture.value as Parameters<typeof computeAgentCapabilityFingerprintV1>[0];
  const canonical = canonicalAgentCapabilitySnapshotV1(snapshot);
  const oracle = `sha256:${createHash("sha256").update(canonical, "utf8").digest("hex")}`;
  assert.equal(computeAgentCapabilityFingerprintV1(snapshot), oracle);
  assert.equal(oracle, manifest.contract.fingerprint.canonicalDigest);
});

function validate(fixture: FixtureCase): unknown {
  switch (fixture.validator) {
    case "capabilitiesRequest": return validateAgentCapabilitiesRequestV1(fixture.value);
    case "capabilitySnapshot": return validateAgentCapabilitySnapshotV1(fixture.value);
    case "runStartRequest": return validateAgentRunStartRequestV1(fixture.value);
    case "runAccepted": return validateAgentRunAcceptedV1(fixture.value);
    case "runRecord": return validateAgentRunRecordV1(fixture.value);
    case "runStartTranscript": return validateAgentRunStartTranscriptV1(fixture.value);
    case "publicEvent": return validateAgentPublicEventV1(fixture.value);
    case "terminationTranscript": return validateAgentTerminationTranscriptV1(fixture.value);
    case "agentChatDetailPage": return validateAgentChatDetailPageV1(fixture.value);
    case "runCancelRequest": return validateAgentRunCancelRequestV1(fixture.value);
    case "runCancelResult": return validateAgentRunCancelResultV1(fixture.value);
    case "runTerminationPersistence": return validateAgentRunTerminationPersistenceV1(fixture.value);
    case "recoveryRecord": return validateAgentRecoveryRecordV1(fixture.value);
    case "recoveryInspectRequest": return validateAgentRecoveryInspectRequestV1(fixture.value);
    case "recoveryResolveRequest": return validateAgentRecoveryResolveRequestV1(fixture.value);
    case "recoveryResolveResult": return validateAgentRecoveryResolveResultV1(fixture.value);
  }
}

function ready(capabilities: string[]): unknown {
  return {
    protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 4321,
    platform: "darwin", capabilities, sidecarInstanceId: "sidecar_g5a_fixture",
  };
}

function countAgentRuns(detail: AgentChatGetResultV1): number {
  if (!isAgentChatDetailPageV1(detail)) return 0;
  return detail.agentRuns.length;
}

function materializeRecoveryDiscovery(
  fixture: RecoveryDiscoveryCases["invalid"][number],
): unknown {
  const base = recoveryDiscoveryCases.valid.find((entry) => entry.name === fixture.baseValidName);
  assert.ok(base, `missing recovery discovery base ${fixture.baseValidName}`);
  const value = structuredClone(base.value);
  for (const pointer of fixture.removals) removeJsonPointer(value, pointer);
  for (const change of fixture.changes) setJsonPointer(value, change.pointer, change.value);
  return value;
}

function setJsonPointer(rootValue: unknown, pointer: string, value: unknown): void {
  const [parentPointer, key] = splitPointer(pointer);
  const parent = resolveJsonPointer(rootValue, parentPointer);
  if (Array.isArray(parent)) {
    const index = Number(key);
    assert.ok(Number.isInteger(index) && index >= 0 && index <= parent.length);
    if (index === parent.length) parent.push(value);
    else parent[index] = value;
    return;
  }
  assert.ok(parent && typeof parent === "object");
  (parent as Record<string, unknown>)[key] = value;
}

function removeJsonPointer(rootValue: unknown, pointer: string): void {
  const [parentPointer, key] = splitPointer(pointer);
  const parent = resolveJsonPointer(rootValue, parentPointer);
  assert.ok(parent && typeof parent === "object" && !Array.isArray(parent));
  delete (parent as Record<string, unknown>)[key];
}

function splitPointer(pointer: string): [string, string] {
  const separator = pointer.lastIndexOf("/");
  assert.ok(separator >= 0, `invalid JSON pointer ${pointer}`);
  return [pointer.slice(0, separator), pointer.slice(separator + 1)];
}

function resolveJsonPointer(rootValue: unknown, pointer: string): unknown {
  if (pointer === "") return rootValue;
  return pointer.split("/").slice(1).reduce<unknown>((value, key) => {
    assert.ok(value && typeof value === "object");
    return (value as Record<string, unknown>)[key];
  }, rootValue);
}


test("stream spacing survives public validation while empty answers stay invalid", () => {
  const page = JSON.parse(readFileSync(resolve("tests/fixtures/m5/agent-output-page.json"), "utf8"));
  const event = { ...page.rows[0].event, kind: "status", text: "\n\n" };
  assert.equal(validateAgentPublicEventV1(event).text, "\n\n");
  assert.equal(validateAgentPublicEventV1({ ...event, text: " " }).text, " ");
  for (const candidate of [{ ...event, text: "" }, { ...event, kind: "final" }, { ...event, kind: "question" }, { ...event, text: " ".repeat(262145) }]) {
    assert.throws(() => validateAgentPublicEventV1(candidate));
  }
});
