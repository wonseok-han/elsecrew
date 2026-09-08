import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  STORAGE_LIFETIME_ACTIVATION_PHASES_V1,
  STORAGE_LIFETIME_ACTIVATION_POLICY_V1,
  STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1,
  STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1,
  STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1,
  STORAGE_LOSSLESS_COMPACTION_MIN_ELIGIBLE_LEDGER_EVENTS_V1,
  validateStorageLifetimeActivationPolicyV1,
} from "../src/m5/contracts/storage-lifetime-v1.ts";

const fixture = JSON.parse(readFileSync(
  resolve("tests/fixtures/m5/storage-activation-v1/manifest.json"),
  "utf8",
)) as Record<string, unknown>;

function setPath(target: Record<string, unknown>, path: string, value: unknown): void {
  const segments = path.split(".");
  const leaf = segments.pop();
  assert.ok(leaf);
  let current = target;
  for (const segment of segments) {
    current = current[segment] as Record<string, unknown>;
  }
  current[leaf] = value;
}

test("storage activation golden is exact and shares named budgets", () => {
  assert.deepEqual(validateStorageLifetimeActivationPolicyV1(fixture), fixture);
  assert.deepEqual(fixture, STORAGE_LIFETIME_ACTIVATION_POLICY_V1);
  assert.equal(STORAGE_LOSSLESS_COMPACTION_MIN_ELIGIBLE_LEDGER_EVENTS_V1, 1_000);
  assert.equal(STORAGE_LIFETIME_MAX_SOURCE_ROWS_PER_BATCH_V1, 100);
  assert.equal(STORAGE_LIFETIME_MAX_CANONICAL_SOURCE_PAYLOAD_BYTES_PER_BATCH_V1, 1_048_576);
  assert.equal(STORAGE_LIFETIME_MAX_RETAINED_DEPENDENCY_INPUT_BYTES_PER_STEP_V1, 8_388_608);
  assert.deepEqual(fixture.phases, STORAGE_LIFETIME_ACTIVATION_PHASES_V1);
});

test("storage activation rejects relaxed threshold, restart, fatal, writer, and preservation policy", () => {
  const mutations: Array<[string, unknown]> = [
    ["schemaVersion", 2],
    ["databaseMigration", "required"],
    ["eligibility.exactSafePrefixMinLedgerEvents", 999],
    ["eligibility.belowThresholdAllowed", true],
    ["eligibility.ageTrigger", true],
    ["eligibility.byteTrigger", true],
    ["eligibility.preconditions", ["completeMaterialization"]],
    ["preservation.logicalDeleteRows", ["events", "chat_messages"]],
    ["preservation.preservedRecords", ["userChats"]],
    ["preservation.physicalCleanup", "enabled"],
    ["restart.pendingEmptyAuthority", "resumeDuringAdmission"],
    ["restart.runningSentinel", "autoReset"],
    ["restart.nonEmptyAuthority", "autoResume"],
    ["restart.ambiguousOrCommitUncertain", "retry"],
    ["restart.automaticResetOrResumeUnsafeState", true],
    ["runtime.owner", "backgroundWorker"],
    ["runtime.writerConnections", 2],
    ["runtime.backgroundWriterConnection", true],
    ["runtime.backgroundWriterThread", true],
    ["runtime.ticks", "allAdmissionStates"],
    ["runtime.zeroTickStates", ["failed"]],
    ["runtime.step", "unboundedWork"],
    ["fatalAfterReady.action", "readyToFailed"],
    ["fatalAfterReady.furtherMutationOrRun", "allowed"],
    ["fatalAfterReady.wireTransition", "readyToFailed"],
    ["budgets.maxWorkUnitsPerStep", 101],
    ["budgets.maxCanonicalPrimaryInputBytesPerStep", 1_048_577],
    ["budgets.maxRetainedAndDependencyInputBytesPerStep", 8_388_609],
    ["budgets.callerConfigurable", true],
    ["phases", ["pending", "armRunning", "delete", "finalize"]],
    ["quit", "checkpointBeforeStopTicks"],
    ["publicWire.g5dLifecycleShape", "v2"],
    ["publicWire.storageStatusShape", "v2"],
    ["publicWire.capabilityOrder", "changed"],
    ["publicWire.readyToFailedTransition", true],
    ["publicWire.physicalCleanupEnabled", true],
  ];
  for (const [path, value] of mutations) {
    const invalid = structuredClone(fixture);
    setPath(invalid, path, value);
    assert.throws(
      () => validateStorageLifetimeActivationPolicyV1(invalid),
      undefined,
      path,
    );
  }

  const unknown = structuredClone(fixture);
  (unknown.runtime as Record<string, unknown>).extraWriter = true;
  assert.throws(() => validateStorageLifetimeActivationPolicyV1(unknown));
});

test("storage activation rejects undefined unknown keys at root and nested levels", () => {
  const rootUnknown = structuredClone(fixture);
  rootUnknown.unknown = undefined;
  assert.throws(() => validateStorageLifetimeActivationPolicyV1(rootUnknown));

  const nestedUnknown = structuredClone(fixture);
  (nestedUnknown.runtime as Record<string, unknown>).unknown = undefined;
  assert.throws(() => validateStorageLifetimeActivationPolicyV1(nestedUnknown));
});

test("storage activation accepts semantically equal reordered root and nested objects", () => {
  const reorderedRoot = Object.fromEntries(Object.entries(structuredClone(fixture)).reverse());
  assert.doesNotThrow(() => validateStorageLifetimeActivationPolicyV1(reorderedRoot));

  const reorderedNested = structuredClone(fixture);
  reorderedNested.runtime = Object.fromEntries(
    Object.entries(reorderedNested.runtime as Record<string, unknown>).reverse(),
  );
  assert.doesNotThrow(() => validateStorageLifetimeActivationPolicyV1(reorderedNested));
});

test("storage activation rejects non-plain objects and getters without invoking them", () => {
  const inherited = structuredClone(fixture);
  Object.setPrototypeOf(inherited, { inherited: true });
  assert.throws(() => validateStorageLifetimeActivationPolicyV1(inherited));

  let getterInvoked = false;
  const getter = structuredClone(fixture);
  Object.defineProperty(getter.runtime as Record<string, unknown>, "owner", {
    enumerable: true,
    get() {
      getterInvoked = true;
      return "singleSQLiteOwnerInSidecarEventLoop";
    },
  });
  assert.throws(() => validateStorageLifetimeActivationPolicyV1(getter));
  assert.equal(getterInvoked, false);
});

test("historical V6 V7 V8 activation manifests remain disabled", () => {
  const v6 = JSON.parse(readFileSync(
    resolve("tests/fixtures/m5/storage-v6/manifest.json"), "utf8",
  )) as { indexedBoundedVerification: { activation: Record<string, boolean> } };
  const v7 = JSON.parse(readFileSync(
    resolve("tests/fixtures/m5/storage-v7/manifest.json"), "utf8",
  )) as { losslessCompaction: { activation: Record<string, boolean> } };
  const v8 = JSON.parse(readFileSync(
    resolve("tests/fixtures/m5/storage-v8/manifest.json"), "utf8",
  )) as { compactionV2: { activation: Record<string, boolean> } };

  for (const activation of [
    v6.indexedBoundedVerification.activation,
    v7.losslessCompaction.activation,
    v8.compactionV2.activation,
  ]) {
    assert.equal(Object.values(activation).every((value) => value === false), true);
  }
});
