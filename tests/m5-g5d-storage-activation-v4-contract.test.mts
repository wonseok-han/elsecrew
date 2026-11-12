import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  STORAGE_LIFETIME_ACTIVATION_POLICY_V3,
  STORAGE_LIFETIME_ACTIVATION_POLICY_V4,
  validateStorageLifetimeActivationPolicyV3,
  validateStorageLifetimeActivationPolicyV4,
} from "../src/m5/contracts/storage-lifetime-v1.ts";

const fixtureRoot = resolve("tests/fixtures/m5/storage-activation-v4");
const activation = JSON.parse(
  readFileSync(resolve(fixtureRoot, "manifest.json"), "utf8"),
) as Record<string, any>;
const cases = JSON.parse(
  readFileSync(resolve(fixtureRoot, "cases.json"), "utf8"),
) as { malformedActivationPaths: string[] };

function setContractPath(target: Record<string, any>, path: string, value: unknown): void {
  const parts = path.split(".");
  const leaf = parts.pop();
  assert.ok(leaf);
  let parent = target;
  for (const part of parts) parent = parent[part] as Record<string, any>;
  parent[leaf] = value;
}

function relaxedValue(current: unknown): unknown {
  if (typeof current === "boolean") return !current;
  if (typeof current === "number") return 999;
  if (Array.isArray(current)) return ["relaxed"];
  return "relaxed";
}

test("activation V4 records implemented scheduler truth without changing public D1", () => {
  assert.deepEqual(validateStorageLifetimeActivationPolicyV4(activation), activation);
  assert.deepEqual(activation, STORAGE_LIFETIME_ACTIVATION_POLICY_V4);
  assert.deepEqual(activation.publicWire, STORAGE_LIFETIME_ACTIVATION_POLICY_V3.publicWire);
  assert.equal(activation.scheduler.implementation.runtimeImplemented, true);
  assert.equal(activation.scheduler.implementation.productExecutionEnabled, true);
  assert.equal(activation.scheduler.implementation.automaticSchedulingEnabled, true);
  assert.equal(activation.scheduler.implementation.publicWireChanged, false);
  assert.equal(activation.activationProof.publicStatusWire, "alreadyImplementedUnchangedD1");
  assert.equal(activation.preservation.physicalCleanup, "disabled");
  assert.doesNotThrow(() => validateStorageLifetimeActivationPolicyV3(
    structuredClone(STORAGE_LIFETIME_ACTIVATION_POLICY_V3),
  ));
});

test("activation V4 rejects false runtime flags and every relaxed frozen invariant", () => {
  for (const path of cases.malformedActivationPaths) {
    const invalid = structuredClone(activation);
    const parts = path.split(".");
    let current: any = invalid;
    for (const part of parts) current = current[part];
    setContractPath(invalid, path, relaxedValue(current));
    assert.throws(() => validateStorageLifetimeActivationPolicyV4(invalid), undefined, path);
  }

  const unknown = structuredClone(activation);
  unknown.activationProof.releaseReady = true;
  assert.throws(() => validateStorageLifetimeActivationPolicyV4(unknown));
});

test("activation V4 proof references exist in the integrated focused Rust suites", () => {
  const writerSource = readFileSync(
    resolve("crates/elsecrew-sidecar/src/storage/tests/compaction_writer_v10.rs"),
    "utf8",
  );
  const sessionSource = readFileSync(
    resolve("crates/elsecrew-sidecar/src/tests.rs"),
    "utf8",
  );

  for (const reference of activation.activationProof.focusedRustSuites.writer as string[]) {
    const name = reference.split("::").at(-1);
    assert.ok(name && writerSource.includes(`fn ${name}(`), reference);
  }
  for (const reference of activation.activationProof.focusedRustSuites.scheduler as string[]) {
    const name = reference.split("::").at(-1);
    const source = reference.startsWith("storage::") ? writerSource : sessionSource;
    assert.ok(name && source.includes(`fn ${name}(`), reference);
  }
});
