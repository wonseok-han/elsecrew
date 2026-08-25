import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { M5_BRIDGE_V1_METHODS } from "../src/m5/contracts/bridge-v1.ts";
import { M5_BRIDGE_V2_METHODS } from "../src/m5/contracts/bridge-v2.ts";
import {
  assertG4CapabilitiesV1,
  FILE_MAX_BYTES_V1,
  FILE_VERSION_ALGORITHM_V1,
  FILE_VERSION_DIGEST_INPUT_V1,
  FILE_VERSION_PREFIX_V1,
  G4_CAPABILITIES_V1,
  G4_ERROR_KINDS_V1,
  PROJECT_SCHEMA_VERSION_V1,
  SNAPSHOT_CREATION_PHASES_V1,
  SNAPSHOT_FAILURE_KINDS_V1,
  SNAPSHOT_LIMITS_V1,
  SNAPSHOT_TRANSPORT_V1,
  SOURCE_MUTATION_POLICY_V1,
  TREE_PAGE_MAX_ENTRIES_V1,
  validateG4SafetyErrorV1,
  validateProjectCloseResultV1,
  validateProjectIdentityV1,
  validateSnapshotFailureV1,
  validateSnapshotActivationEvidenceV1,
  validateSnapshotLifecycleV1,
  validateSnapshotMaterializationV1,
  validateSnapshotSizingV1,
  validateSourceMutationTraceV1,
  validateStandaloneSnapshotInvariantV1,
  validateTreePageV1,
  validateWatchEventV1,
  validateWorktreeManifestV1,
  WORKTREE_HEAD_MODE_V1,
  WORKTREE_LIFECYCLES_V1,
  WORKSPACE_KIND_V1,
} from "../src/m5/contracts/project-v1.ts";
import {
  sha256FileVersionV1,
  validateFileClassificationV1,
  validateFileDigestVectorV1,
  validateFileReadDescriptorV1,
  validateSaveTranscriptV1,
} from "../src/m5/contracts/file-v1.ts";
import {
  validateDiffFileDescriptorV1,
  validateDiffSummaryV1,
} from "../src/m5/contracts/diff-v1.ts";
import { MAX_TEXT_CHUNK_BYTES, METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import {
  parseG4ReadyPayloadV1,
  parseG4SystemStatusV1,
} from "../src/m5/contracts/lifecycle-v1.ts";

type ValidatorName =
  | "projectIdentity"
  | "worktreeManifest"
  | "standaloneSnapshotInvariant"
  | "sourceMutationTrace"
  | "snapshotSizing"
  | "snapshotMaterialization"
  | "snapshotLifecycle"
  | "snapshotFailure"
  | "snapshotActivationEvidence"
  | "treePage"
  | "fileClassification"
  | "fileDigest"
  | "fileRead"
  | "saveTranscript"
  | "diffSummary"
  | "diffFile"
  | "watchEvent"
  | "projectClose"
  | "safetyError"
  | "g4Ready"
  | "g4Status";

interface FixtureCase {
  name: string;
  validator: ValidatorName;
  value: unknown;
}

interface GeneratedCase {
  name: string;
  validator: "treePage" | "saveTranscript" | "fileClassification";
  entryCount?: number;
  textBytes?: number;
  byteLength?: number;
  utf8Valid?: boolean;
  classification?: string;
  expectedValid: boolean;
}

interface Manifest {
  schemaVersion: number;
  protocolVersion: number;
  contract: {
    capabilities: string[];
    bridgeMethods: string[];
    treePageMaxEntries: number;
    textChunkMaxBytes: number;
    fileMaxBytes: number;
    fileVersionAlgorithm: string;
    fileVersionPrefix: string;
    fileVersionDigestInput: string;
    canonicalPathFormat: string;
    worktreeHeadMode: string;
    workspaceKind: string;
    snapshotTransport: string;
    sourceMutationPolicy: string;
    worktreeLifecycles: string[];
    snapshotLimits: typeof SNAPSHOT_LIMITS_V1;
    snapshotCreationPhases: string[];
    snapshotFailureKinds: string[];
    materialization: Record<string, unknown>;
    threatModel: Record<string, unknown>;
    platformSupport: Record<string, string>;
    projectCloseLifecycle: string;
    projectClosePhysicallyDeletes: boolean;
    treeCursorSemantics: string;
    saveConcurrency: string;
    watchOverflow: string;
    safetyErrorKinds: string[];
  };
  casesFile: string;
  activationEvidenceFile: string;
  generatedCases: GeneratedCase[];
}

interface Cases {
  valid: FixtureCase[];
  invalid: FixtureCase[];
}

interface ActivationEvidenceFixture {
  schemaVersion: number;
  validator: "snapshotActivationEvidence";
  valid: unknown;
  invalidCases: Array<{
    name: string;
    changes: Array<{ pointer: string; value: unknown }>;
  }>;
}

const fixtureRoot = resolve("tests/fixtures/m5/project-v1");
const manifest = readJson<Manifest>(resolve(fixtureRoot, "manifest.json"));
const cases = readJson<Cases>(resolve(fixtureRoot, manifest.casesFile));
const activationEvidence = readJson<ActivationEvidenceFixture>(
  resolve(fixtureRoot, manifest.activationEvidenceFile),
);

test("G4 manifest 한 곳이 capability, page/chunk/size/hash/lifecycle 계약을 고정한다", () => {
  assert.equal(manifest.schemaVersion, PROJECT_SCHEMA_VERSION_V1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.deepEqual(manifest.contract.capabilities, [...G4_CAPABILITIES_V1]);
  assert.deepEqual(manifest.contract.bridgeMethods, [...M5_BRIDGE_V2_METHODS]);
  assert.deepEqual(M5_BRIDGE_V2_METHODS.slice(0, M5_BRIDGE_V1_METHODS.length), M5_BRIDGE_V1_METHODS);
  assert.equal(manifest.contract.treePageMaxEntries, TREE_PAGE_MAX_ENTRIES_V1);
  assert.equal(manifest.contract.textChunkMaxBytes, MAX_TEXT_CHUNK_BYTES);
  assert.equal(manifest.contract.fileMaxBytes, FILE_MAX_BYTES_V1);
  assert.equal(manifest.contract.fileVersionAlgorithm, FILE_VERSION_ALGORITHM_V1);
  assert.equal(manifest.contract.fileVersionPrefix, FILE_VERSION_PREFIX_V1);
  assert.equal(manifest.contract.fileVersionDigestInput, FILE_VERSION_DIGEST_INPUT_V1);
  assert.equal(
    manifest.contract.canonicalPathFormat,
    "absoluteRealPathForwardSlashesUppercaseWindowsDrive",
  );
  assert.equal(manifest.contract.worktreeHeadMode, WORKTREE_HEAD_MODE_V1);
  assert.equal(manifest.contract.workspaceKind, WORKSPACE_KIND_V1);
  assert.equal(manifest.contract.snapshotTransport, SNAPSHOT_TRANSPORT_V1);
  assert.equal(manifest.contract.sourceMutationPolicy, SOURCE_MUTATION_POLICY_V1);
  assert.deepEqual(manifest.contract.worktreeLifecycles, [...WORKTREE_LIFECYCLES_V1]);
  assert.deepEqual(manifest.contract.snapshotLimits, SNAPSHOT_LIMITS_V1);
  assert.deepEqual(manifest.contract.snapshotCreationPhases, [...SNAPSHOT_CREATION_PHASES_V1]);
  assert.deepEqual(manifest.contract.snapshotFailureKinds, [...SNAPSHOT_FAILURE_KINDS_V1]);
  assert.deepEqual(manifest.contract.materialization, {
    blobSource: "rawCommittedBytes",
    modeSource: "committedTreeMode",
    symlinkPolicy: "noFollow",
    submodulePolicy: "noInit",
    forbiddenTransforms: ["checkout", "filter", "eol", "encoding", "ident", "exportAttributes", "lfs"],
    failClosedEntries: ["caseCollision", "nonUtf8Path", "dotGitEntry", "abnormalMode"],
  });
  assert.deepEqual(manifest.contract.threatModel, {
    protected: ["selectedRootPathSwap", "managedTargetPathSwap", "ordinaryConcurrentSourceChange"],
    requiredChecks: ["exportedBaseCommit", "exportedBaseTree", "objectFormat", "sourceDigestBeforeAfter"],
    outOfBoundary: ["activelyMaliciousSameUserEntryMutationInsideHeldSourceDirectoryDuringExport"],
  });
  assert.deepEqual(manifest.contract.platformSupport, {
    macos: "contracted",
    windows: "unsupportedUntilSeparateGate",
  });
  for (const limit of Object.values(SNAPSHOT_LIMITS_V1)) {
    if (typeof limit === "number") assert.ok(Number.isSafeInteger(limit));
  }
  assert.equal(manifest.contract.projectCloseLifecycle, "abandoned");
  assert.equal(manifest.contract.projectClosePhysicallyDeletes, false);
  assert.equal(manifest.contract.treeCursorSemantics, "opaque");
  assert.equal(manifest.contract.saveConcurrency, "expectedVersionMustEqualCurrentVersion");
  assert.equal(manifest.contract.watchOverflow, "rescanRequired");
  assert.deepEqual(manifest.contract.safetyErrorKinds, [...G4_ERROR_KINDS_V1]);
  assert.ok(G4_CAPABILITIES_V1.every((capability) => METHODS_V1.includes(capability)));
  assertG4CapabilitiesV1();
});

test("M5BridgeV2는 V1을 보존하고 project/file/diff 전용 API만 추가한다", () => {
  const source = readFileSync(resolve("src/m5/contracts/bridge-v2.ts"), "utf8");
  assert.match(source, /Omit<M5BridgeV1, "version">/);
  assert.doesNotMatch(source, /ipcRenderer|genericInvoke|\binvoke\s*\(|\bsend\s*\(/);
  assert.deepEqual(M5_BRIDGE_V2_METHODS.slice(0, M5_BRIDGE_V1_METHODS.length), M5_BRIDGE_V1_METHODS);
});

test("TypeScript가 모든 canonical valid G4 payload를 승인한다", () => {
  for (const fixture of cases.valid) {
    assert.doesNotThrow(() => validate(fixture.validator, fixture.value), fixture.name);
  }
});

test("TypeScript가 모든 canonical invalid G4 payload를 fail-closed 거부한다", () => {
  for (const fixture of cases.invalid) {
    assert.throws(() => validate(fixture.validator, fixture.value), fixture.name);
  }
});

test("TypeScript가 active 전 composite evidence의 모든 identity를 교차 검증한다", () => {
  assert.equal(activationEvidence.schemaVersion, PROJECT_SCHEMA_VERSION_V1);
  assert.doesNotThrow(() => validateSnapshotActivationEvidenceV1(activationEvidence.valid));
  for (const fixture of activationEvidence.invalidCases) {
    const value = structuredClone(activationEvidence.valid);
    for (const change of fixture.changes) applyJsonPointer(value, change.pointer, change.value);
    assert.throws(() => validateSnapshotActivationEvidenceV1(value), fixture.name);
  }
});

test("TypeScript가 manifest로 생성한 page/chunk/size 경계를 정확히 판정한다", () => {
  for (const fixture of manifest.generatedCases) {
    const value = materialize(fixture);
    if (fixture.expectedValid) {
      assert.doesNotThrow(() => validate(fixture.validator, value), fixture.name);
    } else {
      assert.throws(() => validate(fixture.validator, value), fixture.name);
    }
  }
});

function validate(name: ValidatorName, value: unknown): unknown {
  switch (name) {
    case "projectIdentity":
      return validateProjectIdentityV1(value);
    case "worktreeManifest":
      return validateWorktreeManifestV1(value);
    case "standaloneSnapshotInvariant":
      return validateStandaloneSnapshotInvariantV1(value);
    case "sourceMutationTrace":
      return validateSourceMutationTraceV1(value);
    case "snapshotSizing":
      return validateSnapshotSizingV1(value);
    case "snapshotMaterialization":
      return validateSnapshotMaterializationV1(value);
    case "snapshotLifecycle":
      return validateSnapshotLifecycleV1(value);
    case "snapshotFailure":
      return validateSnapshotFailureV1(value);
    case "snapshotActivationEvidence":
      return validateSnapshotActivationEvidenceV1(value);
    case "treePage":
      return validateTreePageV1(value);
    case "fileClassification":
      return validateFileClassificationV1(value);
    case "fileDigest":
      return validateFileDigestVectorV1(value);
    case "fileRead":
      return validateFileReadDescriptorV1(value);
    case "saveTranscript":
      return validateSaveTranscriptV1(value);
    case "diffSummary":
      return validateDiffSummaryV1(value);
    case "diffFile":
      return validateDiffFileDescriptorV1(value);
    case "watchEvent":
      return validateWatchEventV1(value);
    case "projectClose":
      return validateProjectCloseResultV1(value);
    case "safetyError":
      return validateG4SafetyErrorV1(value);
    case "g4Ready":
      return parseG4ReadyPayloadV1(value);
    case "g4Status":
      return parseG4SystemStatusV1(value);
  }
}

function materialize(fixture: GeneratedCase): unknown {
  if (fixture.validator === "treePage") {
    return {
      projectId: "project_generated",
      parentPath: "src",
      entries: Array.from({ length: fixture.entryCount ?? 0 }, (_, index) => {
        const name = `file-${String(index).padStart(4, "0")}.ts`;
        return { path: `src/${name}`, name, kind: "file" };
      }),
      nextCursor: null,
    };
  }
  if (fixture.validator === "saveTranscript") {
    const text = "a".repeat(fixture.textBytes ?? 0);
    const version = sha256FileVersionV1(new TextEncoder().encode(text));
    return {
      begin: {
        saveId: "save_generated",
        projectId: "project_generated",
        path: "src/generated.ts",
        expectedVersion: version,
        totalBytes: fixture.textBytes,
        encoding: "utf8",
        chunkBytes: manifest.contract.textChunkMaxBytes,
      },
      chunks: [{ saveId: "save_generated", seq: 1, text }],
      commit: {
        saveId: "save_generated",
        expectedVersion: version,
        lastSeq: 1,
        totalBytes: fixture.textBytes,
      },
      currentVersion: version,
      outcome: "committed",
    };
  }
  return {
    byteLength: fixture.byteLength,
    utf8Valid: fixture.utf8Valid,
    classification: fixture.classification,
  };
}

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

function applyJsonPointer(root: unknown, pointer: string, value: unknown): void {
  const segments = pointer.split("/").slice(1).map((segment) =>
    segment.replaceAll("~1", "/").replaceAll("~0", "~"));
  let current = root;
  for (const segment of segments.slice(0, -1)) {
    if (typeof current !== "object" || current === null || Array.isArray(current)) {
      throw new Error(`invalid fixture pointer ${pointer}`);
    }
    current = (current as Record<string, unknown>)[segment];
  }
  if (typeof current !== "object" || current === null || Array.isArray(current)) {
    throw new Error(`invalid fixture pointer ${pointer}`);
  }
  (current as Record<string, unknown>)[segments.at(-1) ?? ""] = value;
}
