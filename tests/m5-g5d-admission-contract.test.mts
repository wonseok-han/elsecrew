import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { M5_BRIDGE_V5_METHODS } from "../src/m5/contracts/bridge-v5.ts";
import { M5_BRIDGE_V6_METHODS } from "../src/m5/contracts/bridge-v6.ts";
import {
  G5D_ALLOWED_WHILE_STORAGE_NOT_READY_V1,
  G5D_STARTUP_STEPS_V1,
  G5D_STORAGE_READY_ONLY_METHODS_V1,
  parseG5DAgentFixtureSystemStatusV1,
  parseG5DProductSystemStatusV1,
  validateG5DBlockedRequestErrorV1,
  validateG5DStartupTraceV1,
  type StorageAdmissionStateV1,
} from "../src/m5/contracts/lifecycle-g5d-v1.ts";
import { PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import { STORAGE_STATUS_BRIDGE_METHOD_V1 } from "../src/m5/contracts/storage-lifetime-v1.ts";

interface Manifest {
  schemaVersion: number;
  protocolVersion: number;
  readyPayloads: { generalProduct: unknown; agentFixture: unknown };
  policy: {
    shellReadyEvent: string;
    allowedWhileNotReady: string[];
    readyOnly: string[];
    gateOrder: string;
    blockedRequestSideEffects: string;
    internalOnlyStep: string;
    publicStates: string[];
  };
  systemStatusCases: Array<{
    name: string;
    expectedValid: boolean;
    value: Record<string, unknown>;
  }>;
  malformedCases: Array<{
    name: string;
    expectedValid: false;
    value: Record<string, unknown>;
  }>;
  blockedRequestCases: Array<{
    name: string;
    state: StorageAdmissionStateV1;
    expectedValid: boolean;
    sideEffects: string;
    error: unknown;
  }>;
  startupCases: Array<{ name: string; expectedValid: boolean; value: unknown }>;
}

const manifest = JSON.parse(
  readFileSync(resolve("tests/fixtures/m5/g5d-admission-v1/manifest.json"), "utf8"),
) as Manifest;

test("G5D admission policy keeps only status and quit available before storage ready", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.equal(manifest.policy.shellReadyEvent, "system.ready");
  assert.deepEqual(manifest.policy.allowedWhileNotReady, [
    ...G5D_ALLOWED_WHILE_STORAGE_NOT_READY_V1,
  ]);
  assert.deepEqual(manifest.policy.readyOnly, [...G5D_STORAGE_READY_ONLY_METHODS_V1]);
  assert.equal(manifest.policy.gateOrder, "beforeParameterValidationAndSideEffects");
  assert.equal(manifest.policy.blockedRequestSideEffects, "knownNoOp");
  assert.equal(manifest.policy.internalOnlyStep, "verifiedForStartup");
  assert.deepEqual(manifest.policy.publicStates, ["verifying", "ready", "failed"]);
  assert.deepEqual(M5_BRIDGE_V6_METHODS, [
    ...M5_BRIDGE_V5_METHODS,
    STORAGE_STATUS_BRIDGE_METHOD_V1,
  ]);
});

test("product와 agent-fixture parser가 세 admission 상태와 malformed 조합을 동일하게 판정한다", () => {
  assert.equal(manifest.malformedCases.every((fixture) => !fixture.expectedValid), true);
  for (const fixture of [...manifest.systemStatusCases, ...manifest.malformedCases]) {
    for (const [mode, sidecar, parse] of [
      ["product", manifest.readyPayloads.generalProduct, parseG5DProductSystemStatusV1],
      ["agentFixture", manifest.readyPayloads.agentFixture, parseG5DAgentFixtureSystemStatusV1],
    ] as const) {
      const value = { ...structuredClone(fixture.value), sidecar };
      if (fixture.expectedValid) {
        assert.doesNotThrow(() => parse(value), `${fixture.name} (${mode})`);
      } else {
        assert.throws(() => parse(value), undefined, `${fixture.name} (${mode})`);
      }
    }
  }

  const verifying = manifest.systemStatusCases[0].value;
  assert.throws(() => parseG5DProductSystemStatusV1({
    ...structuredClone(verifying),
    sidecar: manifest.readyPayloads.agentFixture,
  }));
  assert.throws(() => parseG5DAgentFixtureSystemStatusV1({
    ...structuredClone(verifying),
    sidecar: manifest.readyPayloads.generalProduct,
  }));
});

test("blocked request는 admission 상태별 exact error와 known-no-op만 허용한다", () => {
  for (const fixture of manifest.blockedRequestCases) {
    assert.equal(fixture.sideEffects, "knownNoOp", fixture.name);
    if (fixture.expectedValid) {
      assert.doesNotThrow(
        () => validateG5DBlockedRequestErrorV1(fixture.state, fixture.error),
        fixture.name,
      );
    } else {
      assert.throws(
        () => validateG5DBlockedRequestErrorV1(fixture.state, fixture.error),
        undefined,
        fixture.name,
      );
    }
  }
});

test("shell ready가 storage 검증보다 먼저이고 public admission은 마지막이다", () => {
  for (const fixture of manifest.startupCases) {
    if (fixture.expectedValid) {
      assert.deepEqual(validateG5DStartupTraceV1(fixture.value).steps, G5D_STARTUP_STEPS_V1);
    } else {
      assert.throws(() => validateG5DStartupTraceV1(fixture.value), fixture.name);
    }
  }
});

test("bridge v6는 G5D system status를 재선언하고 storage status를 유지한다", () => {
  const source = readFileSync(resolve("src/m5/contracts/bridge-v6.ts"), "utf8");
  assert.match(source, /Omit<M5BridgeV5, "version" \| "getSystemStatus">/);
  assert.match(source, /getSystemStatus\(\): Promise<SystemStatusG5DV1>/);
  assert.match(source, /getStorageStatus\(input: StorageStatusRequestV1\): Promise<StorageStatusV1>/);
});
