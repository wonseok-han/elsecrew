import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { M5_BRIDGE_V2_METHODS } from "../src/m5/contracts/bridge-v2.ts";
import { M5_BRIDGE_V3_METHODS } from "../src/m5/contracts/bridge-v3.ts";
import { decodeJsonl, encodeJsonl, EVENTS_V1, METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import { G5_CAPABILITIES_V1, G5_ID_MAX_BYTES_V1 } from "../src/m5/contracts/run-recovery-v1.ts";
import {
  WORKSPACE_CAPABILITIES_V1,
  WORKSPACE_EVENTS_V1,
  validateWorkspaceChatBindingTranscriptV1,
  validateWorkspaceEventRouteTranscriptV1,
  validateWorkspaceEventV1,
  validateWorkspaceOperationTranscriptV1,
  validateWorkspaceRefV1,
  validateWorkspaceSaveTranscriptV1,
} from "../src/m5/contracts/workspace-addressing-v1.ts";

type ValidatorName = "workspaceRef" | "workspaceOperation" | "workspaceSave"
  | "workspaceEvent" | "workspaceEventRoute" | "workspaceChatBinding";
interface FixtureCase { name: string; validator: ValidatorName; value: unknown }
interface DerivedCase {
  name: string; baseValidName: string; validator: ValidatorName;
  changes: Array<{ pointer: string; value: unknown }>; expectedValid: boolean;
}
interface Cases { valid: FixtureCase[]; invalid: FixtureCase[]; derived: DerivedCase[] }
interface Manifest {
  schemaVersion: number; protocolVersion: number; casesFile: string;
  contract: {
    capabilities: string[]; events: string[]; bridgeV2Methods: string[];
    bridgeV3WorkspaceMethods: string[]; idMaxBytes: number; addressing: string; saveStreamBinding: string;
    v2ProjectTreeRequestLine: string;
  };
}

const fixtureRoot = resolve("tests/fixtures/m5/g5-workspace-v1");
const manifest = JSON.parse(readFileSync(resolve(fixtureRoot, "manifest.json"), "utf8")) as Manifest;
const cases = JSON.parse(readFileSync(resolve(fixtureRoot, manifest.casesFile), "utf8")) as Cases;

test("workspace-addressed V3 API는 V2를 그대로 보존하고 별도 메서드만 추가한다", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.deepEqual(manifest.contract.capabilities, [...WORKSPACE_CAPABILITIES_V1]);
  assert.deepEqual(manifest.contract.events, [...WORKSPACE_EVENTS_V1]);
  assert.deepEqual(manifest.contract.bridgeV2Methods, [...M5_BRIDGE_V2_METHODS]);
  assert.deepEqual(M5_BRIDGE_V3_METHODS.slice(0, M5_BRIDGE_V2_METHODS.length), M5_BRIDGE_V2_METHODS);
  assert.deepEqual(
    M5_BRIDGE_V3_METHODS.slice(M5_BRIDGE_V2_METHODS.length, M5_BRIDGE_V2_METHODS.length + manifest.contract.bridgeV3WorkspaceMethods.length),
    manifest.contract.bridgeV3WorkspaceMethods,
  );
  assert.ok(WORKSPACE_CAPABILITIES_V1.every((method) => METHODS_V1.includes(method)));
  assert.ok(WORKSPACE_CAPABILITIES_V1.every((method) => G5_CAPABILITIES_V1.includes(method)));
  assert.ok(WORKSPACE_EVENTS_V1.every((event) => EVENTS_V1.includes(event)));
  assert.equal(G5_CAPABILITIES_V1.includes("run.startAgentSmoke" as never), false);
  assert.equal(manifest.contract.idMaxBytes, G5_ID_MAX_BYTES_V1);
  assert.equal(manifest.contract.addressing, "explicitProjectWorktreeWorkspaceIdentity");
  assert.equal(manifest.contract.saveStreamBinding, "beginAndCommitEchoTripleChunksBoundBySaveId");
  const v2Request = decodeJsonl(manifest.contract.v2ProjectTreeRequestLine);
  assert.equal(v2Request.kind, "request");
  assert.equal(v2Request.kind === "request" && v2Request.method, "project.getTree");
  assert.equal(encodeJsonl(v2Request), manifest.contract.v2ProjectTreeRequestLine);

  const v2Source = readFileSync(resolve("src/m5/contracts/bridge-v2.ts"), "utf8");
  const v3Source = readFileSync(resolve("src/m5/contracts/bridge-v3.ts"), "utf8");
  assert.match(v3Source, /Omit<M5BridgeV2, "version">/);
  assert.doesNotMatch(v2Source, /WorkspaceRefV1|getWorkspace|workspaceIdentity|worktreeId/);
  assert.doesNotMatch(v3Source, /selected(?:Chat|Project)|current(?:Chat|Project)/i);

  const boundaryId = `${"가".repeat(170)}aa`;
  assert.equal(new TextEncoder().encode(boundaryId).length, manifest.contract.idMaxBytes);
  assert.doesNotThrow(() => validateWorkspaceRefV1({ projectId: boundaryId, worktreeId: "worktree", workspaceIdentity: "workspace" }));
  assert.throws(() => validateWorkspaceRefV1({ projectId: `${boundaryId}a`, worktreeId: "worktree", workspaceIdentity: "workspace" }));
});

test("TypeScript는 canonical workspace valid fixture를 모두 승인한다", () => {
  for (const fixture of cases.valid) assert.doesNotThrow(() => validate(fixture), fixture.name);
});

test("TypeScript는 project-only 주소, unknown field, 불완전 event triple을 거부한다", () => {
  for (const fixture of cases.invalid) assert.throws(() => validate(fixture), fixture.name);
});

test("TypeScript는 cross-chat alias와 잘못된 response/event/save 상관관계를 거부한다", () => {
  for (const fixture of cases.derived) {
    const base = cases.valid.find((candidate) => candidate.name === fixture.baseValidName);
    assert.ok(base, fixture.name);
    const value = structuredClone(base.value);
    for (const change of fixture.changes) applyJsonPointer(value, change.pointer, change.value);
    assert.equal(isValid({ name: fixture.name, validator: fixture.validator, value }), fixture.expectedValid, fixture.name);
  }
});

function isValid(fixture: FixtureCase): boolean {
  try { validate(fixture); return true; } catch { return false; }
}

function validate(fixture: FixtureCase): void {
  const validators: Record<ValidatorName, (value: unknown) => unknown> = {
    workspaceRef: validateWorkspaceRefV1,
    workspaceOperation: validateWorkspaceOperationTranscriptV1,
    workspaceSave: validateWorkspaceSaveTranscriptV1,
    workspaceEvent: validateWorkspaceEventV1,
    workspaceEventRoute: validateWorkspaceEventRouteTranscriptV1,
    workspaceChatBinding: validateWorkspaceChatBindingTranscriptV1,
  };
  validators[fixture.validator](fixture.value);
}

function applyJsonPointer(root: unknown, pointer: string, value: unknown): void {
  const segments = pointer.split("/").slice(1).map((segment) => segment.replaceAll("~1", "/").replaceAll("~0", "~"));
  let current = root as Record<string, unknown> | unknown[];
  for (const segment of segments.slice(0, -1)) {
    current = Array.isArray(current) ? current[Number(segment)] as typeof current : current[segment] as typeof current;
  }
  const last = segments.at(-1)!;
  if (Array.isArray(current)) current[Number(last)] = value;
  else current[last] = value;
}
