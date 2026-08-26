import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { M5_BRIDGE_V2_METHODS } from "../src/m5/contracts/bridge-v2.ts";
import { M5_BRIDGE_V3_METHODS } from "../src/m5/contracts/bridge-v3.ts";
import { METHODS_V1 } from "../src/m5/contracts/protocol-v1.ts";
import {
  G5_CAPABILITIES_V1,
  validateAbandonPreparedWorkspaceRequestV1,
  validateAbandonPreparedWorkspaceTranscriptV1,
  validateChatCreationTranscriptV1,
  validatePreparedChatWorkspaceV1,
  validatePrepareChatWorkspaceTranscriptV1,
} from "../src/m5/contracts/run-recovery-v1.ts";

type ValidatorName = "prepareChatWorkspace" | "abandonPreparedWorkspace"
  | "abandonPreparedWorkspaceRequest" | "preparedChatWorkspace" | "chatCreation";
interface FixtureCase { name: string; validator: ValidatorName; value: unknown }
interface DerivedCase {
  name: string; baseValidName: string; validator: ValidatorName;
  changes: Array<{ pointer: string; value: unknown }>; expectedValid: boolean;
}
interface Cases { valid: FixtureCase[]; invalid: FixtureCase[]; derived: DerivedCase[] }
interface Manifest {
  schemaVersion: number; protocolVersion: number; casesFile: string;
  contract: {
    methods: string[]; bridgeMethods: string[]; prepareSemantics: string; chatBindSemantics: string;
    cleanupSemantics: string; physicalDeletion: boolean; idempotency: string;
  };
}

const root = resolve("tests/fixtures/m5/g5-preparation-v1");
const manifest = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8")) as Manifest;
const cases = JSON.parse(readFileSync(resolve(root, manifest.casesFile), "utf8")) as Cases;

test("G5 chat workspace 준비 API는 V2/V3를 보존하며 chat.create 전에 additive하다", () => {
  assert.deepEqual(manifest.contract.methods, ["workspace.prepareChat", "workspace.abandonPrepared", "chat.create"]);
  assert.ok(manifest.contract.methods.every((method) => METHODS_V1.includes(method as never)));
  assert.ok(manifest.contract.methods.slice(0, 2).every((method) => G5_CAPABILITIES_V1.includes(method as never)));
  assert.ok(G5_CAPABILITIES_V1.indexOf("workspace.prepareChat") < G5_CAPABILITIES_V1.indexOf("chat.create"));
  assert.ok(G5_CAPABILITIES_V1.indexOf("workspace.abandonPrepared") < G5_CAPABILITIES_V1.indexOf("chat.create"));
  assert.deepEqual(M5_BRIDGE_V3_METHODS.slice(0, M5_BRIDGE_V2_METHODS.length), M5_BRIDGE_V2_METHODS);
  assert.ok(manifest.contract.bridgeMethods.every((method) => M5_BRIDGE_V3_METHODS.includes(method as never)));
  assert.equal(manifest.contract.prepareSemantics, "alwaysNewStandaloneSnapshot");
  assert.equal(manifest.contract.chatBindSemantics, "unboundToBoundWithChatAndInitialMessageAtomically");
  assert.equal(manifest.contract.cleanupSemantics, "exactUnboundPreparationOnlyEvidencePreserving");
  assert.equal(manifest.contract.physicalDeletion, false);
  assert.equal(manifest.contract.idempotency, "duplicateOrLateMutationIsFailClosedNoOp");
});

test("TypeScript는 canonical preparation valid transcript를 모두 승인한다", () => {
  for (const fixture of cases.valid) assert.doesNotThrow(() => validate(fixture), fixture.name);
});

test("TypeScript는 project-only·unknown-field preparation payload를 거부한다", () => {
  for (const fixture of cases.invalid) assert.throws(() => validate(fixture), fixture.name);
});

test("TypeScript는 snapshot 재사용·cross-identity·A workspace 손상을 fail-closed 판정한다", () => {
  for (const fixture of cases.derived) {
    const base = cases.valid.find((candidate) => candidate.name === fixture.baseValidName);
    assert.ok(base, fixture.name);
    const value = structuredClone(base.value);
    for (const change of fixture.changes) applyJsonPointer(value, change.pointer, change.value);
    assert.equal(isValid({ name: fixture.name, validator: fixture.validator, value }), fixture.expectedValid, fixture.name);
  }
});

function validate(fixture: FixtureCase): void {
  const validators: Record<ValidatorName, (value: unknown) => unknown> = {
    prepareChatWorkspace: validatePrepareChatWorkspaceTranscriptV1,
    abandonPreparedWorkspace: validateAbandonPreparedWorkspaceTranscriptV1,
    abandonPreparedWorkspaceRequest: validateAbandonPreparedWorkspaceRequestV1,
    preparedChatWorkspace: validatePreparedChatWorkspaceV1,
    chatCreation: validateChatCreationTranscriptV1,
  };
  validators[fixture.validator](fixture.value);
}

function isValid(fixture: FixtureCase): boolean {
  try { validate(fixture); return true; } catch { return false; }
}

function applyJsonPointer(rootValue: unknown, pointer: string, value: unknown): void {
  const segments = pointer.split("/").slice(1).map((segment) => segment.replaceAll("~1", "/").replaceAll("~0", "~"));
  let current = rootValue as Record<string, unknown> | unknown[];
  for (const segment of segments.slice(0, -1)) {
    current = Array.isArray(current) ? current[Number(segment)] as typeof current : current[segment] as typeof current;
  }
  const last = segments.at(-1)!;
  if (Array.isArray(current)) current[Number(last)] = value;
  else current[last] = value;
}
