import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { M5_BRIDGE_V2_METHODS } from "../src/m5/contracts/bridge-v2.ts";
import { M5_BRIDGE_V3_METHODS } from "../src/m5/contracts/bridge-v3.ts";
import { G4_CAPABILITIES_V1 } from "../src/m5/contracts/project-v1.ts";
import { METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import { SQLITE_SCHEMA_VERSION, SQLITE_TABLES_V1 } from "../src/m5/contracts/storage-v1.ts";
import {
  assertG5CapabilitiesV1,
  G5_CAPABILITIES_V1,
  G5_EVENT_PAGE_MAX_ITEMS_V1,
  G5_FIXTURE_SCENARIOS_V1,
  G5_GOAL_MAX_BYTES_V1,
  G5_ID_MAX_BYTES_V1,
  G5_MODEL_SELECTOR_MAX_BYTES_V1,
  G5_PAGE_MAX_ITEMS_V1,
  G5_PERMISSIONS_V1,
  G5_PUBLIC_MESSAGE_MAX_BYTES_V1,
  G5_REASONING_EFFORT_MAX_BYTES_V1,
  G5_ROLE_IDS_V1,
  G5_RUNTIMES_V1,
  G5_SCHEMA_VERSION_V1,
  parseG5ReadyPayloadV1,
  validateChatCreationTranscriptV1,
  validateChatBindingPagesV1,
  validateCancelTerminationRecoveryTranscriptV1,
  validateChatCreateRequestV1,
  validateChatDetailPageV1,
  validateChatGetRequestV1,
  validateChatListPageV1,
  validateChatListRequestV1,
  validateChatRecordV1,
  validateFixtureRunStartRequestV1,
  validatePageRequestV1,
  validateProjectRoleSettingsV1,
  validateProjectRoleSettingsUpdateRequestV1,
  validateRecoveryMarkResolvedRequestV1,
  validateRecoveryRecordV1,
  validateRecoveryPageV1,
  validateRecoveryResolutionTranscriptV1,
  validateRoleProfileV1,
  validateRunRecordV1,
  validateRunStartTranscriptV1,
  validateRunCancelRequestV1,
  validateRunTransitionV1,
  validateTeamMessageV1,
  validateWriterStartTranscriptV1,
} from "../src/m5/contracts/run-recovery-v1.ts";

type ValidatorName =
  | "roleProfile" | "projectRoleSettings" | "projectRoleSettingsUpdate" | "chatCreateRequest"
  | "chatRecord" | "teamMessage" | "runRecord" | "runTransition" | "chatCreation"
  | "chatDetailPage" | "chatListPage" | "chatListRequest" | "chatGetRequest" | "pageRequest"
  | "fixtureRunStart" | "runCancelRequest" | "writerStart" | "recoveryRecord" | "recoveryPage" | "recoveryMarkResolved"
  | "recoveryResolution" | "runStart" | "cancelTerminationRecovery";
interface FixtureCase { name: string; validator: ValidatorName; value: unknown }
interface DerivedCase {
  name: string; baseValidName: string; validator: ValidatorName;
  changes: Array<{ pointer: string; value: unknown }>; expectedValid: boolean;
}
interface Cases { valid: FixtureCase[]; invalid: FixtureCase[]; derived: DerivedCase[] }
interface Manifest {
  schemaVersion: number;
  protocolVersion: number;
  contract: {
    capabilities: string[]; bridgeMethods: string[]; roles: string[]; runtimes: string[];
    permissions: string[]; fixtureScenarios: string[]; pageMaxItems: number;
    eventPageMaxItems: number; idMaxBytes: number; modelSelectorMaxBytes: number; reasoningEffortMaxBytes: number;
    goalMaxBytes: number; publicMessageMaxBytes: number; protocolStreamSequence: string;
    durableEventSequence: string; workspaceBinding: string; writerStartOrdering: string[];
    recoveryResolution: string; platformExecution: Record<string, string>; agentSmokeAdvertised: boolean;
  };
  casesFile: string;
}

const root = resolve("tests/fixtures/m5/g5-v1");
const manifest = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8")) as Manifest;
const cases = JSON.parse(readFileSync(resolve(root, manifest.casesFile), "utf8")) as Cases;

test("G5 manifest가 additive V3 bridge, exact capability와 안전 경계를 고정한다", () => {
  assert.equal(manifest.schemaVersion, G5_SCHEMA_VERSION_V1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.deepEqual(manifest.contract.capabilities, [...G5_CAPABILITIES_V1]);
  assert.deepEqual(manifest.contract.capabilities.slice(0, G4_CAPABILITIES_V1.length), [...G4_CAPABILITIES_V1]);
  assert.deepEqual(manifest.contract.bridgeMethods, [...M5_BRIDGE_V3_METHODS]);
  assert.deepEqual(M5_BRIDGE_V3_METHODS.slice(0, M5_BRIDGE_V2_METHODS.length), M5_BRIDGE_V2_METHODS);
  assert.deepEqual(manifest.contract.roles, [...G5_ROLE_IDS_V1]);
  assert.deepEqual(manifest.contract.runtimes, [...G5_RUNTIMES_V1]);
  assert.deepEqual(manifest.contract.permissions, [...G5_PERMISSIONS_V1]);
  assert.deepEqual(manifest.contract.fixtureScenarios, [...G5_FIXTURE_SCENARIOS_V1]);
  assert.equal(manifest.contract.pageMaxItems, G5_PAGE_MAX_ITEMS_V1);
  assert.equal(manifest.contract.eventPageMaxItems, G5_EVENT_PAGE_MAX_ITEMS_V1);
  assert.equal(manifest.contract.idMaxBytes, G5_ID_MAX_BYTES_V1);
  assert.equal(manifest.contract.modelSelectorMaxBytes, G5_MODEL_SELECTOR_MAX_BYTES_V1);
  assert.equal(manifest.contract.reasoningEffortMaxBytes, G5_REASONING_EFFORT_MAX_BYTES_V1);
  assert.equal(manifest.contract.goalMaxBytes, G5_GOAL_MAX_BYTES_V1);
  assert.equal(manifest.contract.publicMessageMaxBytes, G5_PUBLIC_MESSAGE_MAX_BYTES_V1);
  assert.equal(manifest.contract.protocolStreamSequence, "connectionScopedPerStream");
  assert.equal(manifest.contract.durableEventSequence, "chatLedgerScopedContiguous");
  assert.equal(manifest.contract.workspaceBinding, "explicitProjectWorktreeWorkspaceIdentity");
  assert.deepEqual(manifest.contract.writerStartOrdering, ["beginImmediate", "persistRun", "persistWorkspaceLock", "commit", "spawn"]);
  assert.equal(manifest.contract.recoveryResolution, "abandonOnlyAfterAllTrackedProcessIdentitiesProvenGone");
  assert.deepEqual(manifest.contract.platformExecution, { macos: "fakeFixtureOnly", windows: "disabled" });
  assert.equal(manifest.contract.agentSmokeAdvertised, false);
  assert.equal(G5_CAPABILITIES_V1.includes("run.startAgentSmoke" as never), false);
  assert.ok(G5_CAPABILITIES_V1.every((capability) => METHODS_V1.includes(capability)));
  assertG5CapabilitiesV1();
  const ready = {
    protocolVersion: 1, sidecarSchemaVersion: 1, appSchemaVersion: 1, pid: 4321,
    platform: "darwin", capabilities: manifest.contract.capabilities,
    sidecarInstanceId: "sidecar_g5_fixture",
  };
  assert.doesNotThrow(() => parseG5ReadyPayloadV1(ready));
  assert.throws(() => parseG5ReadyPayloadV1({ ...ready, capabilities: [...ready.capabilities, "run.startAgentSmoke"] }));
});

test("M5BridgeV3는 V2를 보존하고 G5 동작별 typed API만 추가한다", () => {
  const source = readFileSync(resolve("src/m5/contracts/bridge-v3.ts"), "utf8");
  assert.match(source, /Omit<M5BridgeV2, "version">/);
  assert.doesNotMatch(source, /startAgentSmoke|ipcRenderer|genericInvoke|\binvoke\s*\(|\bsend\s*\(/);
  assert.deepEqual(M5_BRIDGE_V3_METHODS.slice(0, M5_BRIDGE_V2_METHODS.length), M5_BRIDGE_V2_METHODS);
});

test("TypeScript가 canonical G5 valid cases를 모두 승인한다", () => {
  for (const fixture of cases.valid) assert.doesNotThrow(() => validate(fixture), fixture.name);
});

test("TypeScript가 unknown field와 identity/state/permission/recovery 위반을 모두 거부한다", () => {
  for (const fixture of cases.invalid) assert.throws(() => validate(fixture), fixture.name);
});

test("TypeScript가 canonical 파생 사례의 설정 drift, cross-chat/run, recovery outcomes를 판정한다", () => {
  for (const fixture of cases.derived) {
    const base = cases.valid.find((candidate) => candidate.name === fixture.baseValidName);
    assert.ok(base, fixture.name);
    const value = structuredClone(base.value);
    for (const change of fixture.changes) applyJsonPointer(value, change.pointer, change.value);
    if (fixture.expectedValid) {
      assert.doesNotThrow(() => validate({ name: fixture.name, validator: fixture.validator, value }), fixture.name);
    } else {
      assert.throws(() => validate({ name: fixture.name, validator: fixture.validator, value }), fixture.name);
    }
  }
});

test("설정·텍스트·페이지 경계와 SQLite v1 migration 호환을 고정한다", () => {
  const model = "m".repeat(G5_MODEL_SELECTOR_MAX_BYTES_V1);
  assert.doesNotThrow(() => validateRoleProfileV1({ roleId: "coding", runtime: "codex", modelSelector: model, reasoningEffort: null, permission: "workspaceWrite" }));
  assert.throws(() => validateRoleProfileV1({ roleId: "coding", runtime: "codex", modelSelector: `${model}x`, reasoningEffort: null, permission: "workspaceWrite" }));
  const effort = "e".repeat(G5_REASONING_EFFORT_MAX_BYTES_V1);
  assert.doesNotThrow(() => validateRoleProfileV1({ roleId: "planning", runtime: "codex", modelSelector: "sol", reasoningEffort: effort, permission: "readOnly" }));
  assert.throws(() => validateRoleProfileV1({ roleId: "planning", runtime: "codex", modelSelector: "sol", reasoningEffort: `${effort}x`, permission: "readOnly" }));
  const boundaryId = `${"가".repeat(170)}aa`;
  assert.equal(new TextEncoder().encode(boundaryId).length, G5_ID_MAX_BYTES_V1);
  assert.doesNotThrow(() => validateFixtureRunStartRequestV1({ chatId: boundaryId, roleId: "planning", scenario: "planning-success" }));
  assert.throws(() => validateFixtureRunStartRequestV1({ chatId: `${boundaryId}a`, roleId: "planning", scenario: "planning-success" }));
  const bindings = Array.from({ length: G5_PAGE_MAX_ITEMS_V1 }, (_, index) => ({
    chatId: `chat_${index}`, projectId: "project_boundary",
    worktreeId: `worktree_${index}`, workspaceIdentity: `workspace_${index}`,
  }));
  assert.doesNotThrow(() => validateChatBindingPagesV1([{ cursor: null, items: bindings, nextCursor: null }]));
  assert.throws(() => validateChatBindingPagesV1([{
    cursor: null,
    items: [...bindings, { chatId: "chat_over", projectId: "project_boundary", worktreeId: "worktree_over", workspaceIdentity: "workspace_over" }],
    nextCursor: null,
  }]));
  assert.equal(SQLITE_SCHEMA_VERSION, 1);
  assert.ok(["chats", "runs", "locks"].every((table) => SQLITE_TABLES_V1.includes(table as never)));
});

function validate(fixture: FixtureCase): unknown {
  switch (fixture.validator) {
    case "roleProfile": return validateRoleProfileV1(fixture.value);
    case "projectRoleSettings": return validateProjectRoleSettingsV1(fixture.value);
    case "projectRoleSettingsUpdate": return validateProjectRoleSettingsUpdateRequestV1(fixture.value);
    case "chatCreateRequest": return validateChatCreateRequestV1(fixture.value);
    case "chatRecord": return validateChatRecordV1(fixture.value);
    case "teamMessage": return validateTeamMessageV1(fixture.value);
    case "runRecord": return validateRunRecordV1(fixture.value);
    case "runTransition": return validateRunTransitionV1(fixture.value);
    case "chatCreation": return validateChatCreationTranscriptV1(fixture.value);
    case "chatDetailPage": return validateChatDetailPageV1(fixture.value);
    case "chatListPage": return validateChatListPageV1(fixture.value);
    case "chatListRequest": return validateChatListRequestV1(fixture.value);
    case "chatGetRequest": return validateChatGetRequestV1(fixture.value);
    case "pageRequest": return validatePageRequestV1(fixture.value);
    case "fixtureRunStart": return validateFixtureRunStartRequestV1(fixture.value);
    case "runCancelRequest": return validateRunCancelRequestV1(fixture.value);
    case "writerStart": return validateWriterStartTranscriptV1(fixture.value);
    case "recoveryRecord": return validateRecoveryRecordV1(fixture.value);
    case "recoveryPage": return validateRecoveryPageV1(fixture.value);
    case "recoveryMarkResolved": return validateRecoveryMarkResolvedRequestV1(fixture.value);
    case "recoveryResolution": return validateRecoveryResolutionTranscriptV1(fixture.value);
    case "runStart": return validateRunStartTranscriptV1(fixture.value);
    case "cancelTerminationRecovery": return validateCancelTerminationRecoveryTranscriptV1(fixture.value);
  }
}

function applyJsonPointer(root: unknown, pointer: string, value: unknown): void {
  const segments = pointer.split("/").slice(1).map((segment) => segment.replaceAll("~1", "/").replaceAll("~0", "~"));
  let current = root;
  for (const segment of segments.slice(0, -1)) {
    if (Array.isArray(current)) current = current[Number(segment)];
    else if (typeof current === "object" && current !== null) current = (current as Record<string, unknown>)[segment];
    else throw new Error(`invalid fixture pointer ${pointer}`);
  }
  const last = segments.at(-1) ?? "";
  if (Array.isArray(current)) current[Number(last)] = value;
  else if (typeof current === "object" && current !== null) (current as Record<string, unknown>)[last] = value;
  else throw new Error(`invalid fixture pointer ${pointer}`);
}
