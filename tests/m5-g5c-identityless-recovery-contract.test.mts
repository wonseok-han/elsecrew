import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  validateAgentChatDetailPageV1,
  validateAgentRecoveryRecordV1,
  validateAgentRecoveryResolveResultV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";

type ValidatorName = "agentChatDetailPage" | "recoveryRecord" | "recoveryResolveResult";
interface FixtureCase { name: string; validator: ValidatorName; value: unknown }
interface MutationCase {
  name: string;
  baseValidName: string;
  removals: string[];
  changes: { pointer: string; value: unknown }[];
}
interface IdentitylessCases { valid: FixtureCase[]; invalid: MutationCase[] }
interface Manifest {
  contract: {
    identitylessRecovery: {
      reason: string;
      trackedProcesses: string;
      state: string;
      poisoned: boolean;
      resolvedAt: null;
      matchingRevisionOutcome: string;
      staleRevisionOutcome: string;
      after: null;
      resolutionPriority: string[];
    };
  };
  g5cIdentitylessRecoveryCasesFile: string;
}

const root = resolve("tests/fixtures/m5/agent-adapter-v1");
const manifest = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8")) as Manifest;
const cases = JSON.parse(
  readFileSync(resolve(root, manifest.g5cIdentitylessRecoveryCasesFile), "utf8"),
) as IdentitylessCases;

test("G5C identityless recovery manifest는 비가변 resolve 우선순위를 고정한다", () => {
  assert.deepEqual(manifest.contract.identitylessRecovery, {
    reason: "processIdentityUnavailable",
    trackedProcesses: "mustBeEmpty",
    state: "unresolved",
    poisoned: true,
    resolvedAt: null,
    matchingRevisionOutcome: "identityUnavailable",
    staleRevisionOutcome: "revisionConflict",
    after: null,
    resolutionPriority: ["revisionConflict", "identityUnavailable", "processesRemain", "resolvedAbandoned"],
  });
});

test("TypeScript는 identityless record, chat detail, resolve와 기존 outcome 골든을 승인한다", () => {
  for (const fixture of cases.valid) {
    assert.doesNotThrow(() => validate(fixture.validator, fixture.value), fixture.name);
  }
});

test("TypeScript는 identityless 상태 변이, 일반 recovery 빈 증거, resolve identity drift를 거부한다", () => {
  for (const fixture of cases.invalid) {
    assert.throws(() => validate(
      cases.valid.find((entry) => entry.name === fixture.baseValidName)!.validator,
      materialize(fixture),
    ), fixture.name);
  }
});

test("stale revision은 identity unavailable보다 먼저 판정되고 matching revision은 기록을 바꾸지 않는다", () => {
  const stale = validateAgentRecoveryResolveResultV1(valid("identityless-stale-revision-keeps-conflict-priority"));
  assert.equal(stale.outcome, "revisionConflict");
  assert.equal(stale.after, null);

  const matching = validateAgentRecoveryResolveResultV1(valid("identityless-matching-revision-is-non-mutating"));
  assert.equal(matching.outcome, "identityUnavailable");
  assert.equal(matching.after, null);
  assert.deepEqual(matching.before, validRecord());
});

function validate(validator: ValidatorName, value: unknown): unknown {
  switch (validator) {
    case "agentChatDetailPage": return validateAgentChatDetailPageV1(value);
    case "recoveryRecord": return validateAgentRecoveryRecordV1(value);
    case "recoveryResolveResult": return validateAgentRecoveryResolveResultV1(value);
  }
}

function valid(name: string): unknown {
  const fixture = cases.valid.find((entry) => entry.name === name);
  assert.ok(fixture, `missing valid fixture ${name}`);
  return structuredClone(fixture.value);
}

function validRecord(): unknown {
  return valid("identityless-recovery-record");
}

function materialize(fixture: MutationCase): unknown {
  const value = valid(fixture.baseValidName);
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
