import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  assertG5AContractV1,
  AGENT_CAPABILITY_BLOCK_REASONS_V1,
  AGENT_START_OUTCOMES_V1,
  G5A_CAPABILITIES_V1,
  parsePlannedG5AReadyPayloadV1,
  validateAgentCapabilitiesRequestV1,
  validateAgentCapabilitySnapshotV1,
  validateAgentPublicEventV1,
  validateAgentRunStartRequestV1,
  validateAgentRunAcceptedV1,
  validateAgentRunRecordV1,
  validateAgentRunStartTranscriptV1,
  validateAgentTerminationTranscriptV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import { M5_BRIDGE_V3_METHODS } from "../src/m5/contracts/bridge-v3.ts";
import { M5_BRIDGE_V4_METHODS } from "../src/m5/contracts/bridge-v4.ts";
import { METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import { G5_CAPABILITIES_V1, parseG5ReadyPayloadV1 } from "../src/m5/contracts/run-recovery-v1.ts";

type ValidatorName = "capabilitiesRequest" | "capabilitySnapshot" | "runStartRequest" | "runAccepted" | "runRecord" | "runStartTranscript" | "publicEvent" | "terminationTranscript";
interface FixtureCase { name: string; validator: ValidatorName; value: unknown }
interface Cases { valid: FixtureCase[]; invalid: FixtureCase[] }
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
    fingerprint: string;
    modelStartEvidence: string[];
    startOutcomes: string[];
    capabilityBlockReasons: string[];
    argvOnlyCanStart: boolean;
    publicEvents: string[];
    preflightFailure: { spawnCount: number; lockCount: number };
    launcherBarrier: string[];
    uncertainTermination: string;
  };
  casesFile: string;
}

const root = resolve("tests/fixtures/m5/agent-adapter-v1");
const manifest = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8")) as Manifest;
const cases = JSON.parse(readFileSync(resolve(root, manifest.casesFile), "utf8")) as Cases;

test("G5A manifest는 기존 G5 handshake를 보존한 planned V4 계약만 고정한다", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.deepEqual(manifest.contract.capabilities, [...G5A_CAPABILITIES_V1]);
  assert.deepEqual(G5A_CAPABILITIES_V1.slice(0, G5_CAPABILITIES_V1.length), G5_CAPABILITIES_V1);
  assert.deepEqual(G5A_CAPABILITIES_V1.slice(-2), ["agent.getCapabilities", "run.startAgent"]);
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

test("M5BridgeV4는 V3를 보존하고 두 typed 메서드만 추가한다", () => {
  assert.deepEqual(M5_BRIDGE_V4_METHODS.slice(0, M5_BRIDGE_V3_METHODS.length), M5_BRIDGE_V3_METHODS);
  assert.deepEqual(M5_BRIDGE_V4_METHODS.slice(M5_BRIDGE_V3_METHODS.length), manifest.contract.bridgeMethodsAdded);
  const source = readFileSync(resolve("src/m5/contracts/bridge-v4.ts"), "utf8");
  assert.match(source, /Omit<M5BridgeV3, "version" \| "getChat">/);
  assert.doesNotMatch(source, /startAgentSmoke|RunRecordV1|fixtureScenario|ipcRenderer|genericInvoke/);
});

test("TypeScript는 canonical actual-agent valid cases를 모두 승인한다", () => {
  for (const fixture of cases.valid) assert.doesNotThrow(() => validate(fixture), fixture.name);
});

test("TypeScript는 launch injection, 불확실 model, raw event, barrier 위반을 모두 거부한다", () => {
  for (const fixture of cases.invalid) assert.throws(() => validate(fixture), fixture.name);
});

test("manifest는 one-shot, next-run, redacted evidence와 fail-closed 종료를 명시한다", () => {
  assert.equal(manifest.contract.executionMode, "oneShot");
  assert.equal(manifest.contract.followUpMode, "nextRun");
  assert.equal(manifest.contract.rendererLaunchFields, "forbidden");
  assert.equal(manifest.contract.fingerprint, "sha256CanonicalCapabilitySnapshotWithoutFingerprintOrObservedAt");
  assert.deepEqual(manifest.contract.modelStartEvidence, ["runtimeVerified", "measuredExactVersion"]);
  assert.deepEqual(manifest.contract.startOutcomes, [...AGENT_START_OUTCOMES_V1]);
  assert.deepEqual(manifest.contract.capabilityBlockReasons, [...AGENT_CAPABILITY_BLOCK_REASONS_V1]);
  assert.equal(manifest.contract.argvOnlyCanStart, false);
  assert.deepEqual(manifest.contract.publicEvents, ["status", "question", "final"]);
  assert.deepEqual(manifest.contract.preflightFailure, { spawnCount: 0, lockCount: 0 });
  assert.deepEqual(manifest.contract.launcherBarrier, ["persistRunAndLock", "captureLauncherAndAgentIdentity", "attachTreeTracker", "persistIdentity", "release"]);
  assert.equal(manifest.contract.uncertainTermination, "recoveryRequiredAndPoisoned");
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
  }
}

function ready(capabilities: string[]): unknown {
  return {
    protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 4321,
    platform: "darwin", capabilities, sidecarInstanceId: "sidecar_g5a_fixture",
  };
}
