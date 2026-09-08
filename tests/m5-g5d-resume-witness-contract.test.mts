import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { DatabaseSync } from "node:sqlite";

import {
  STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1,
  STORAGE_COMPACTION_RESUME_WITNESS_POLICY_V1,
  STORAGE_LIFETIME_ACTIVATION_POLICY_V1,
  STORAGE_LIFETIME_ACTIVATION_POLICY_V2,
  validateStorageCompactionAdmissionStateV3,
  validateStorageCompactionCursorTransitionV3,
  validateStorageCompactionCursorV3,
  validateStorageCompactionLifecycleTransitionV3,
  validateStorageCompactionPlanBatchV1,
  validateStorageCompactionPlanRowBindingV1,
  validateStorageCompactionPlanRowV1,
  validateStorageCompactionPlanV1,
  validateStorageCompactionResumeWitnessPolicyV1,
  validateStorageLifetimeActivationPolicyV1,
  validateStorageLifetimeActivationPolicyV2,
  validateStorageLifetimeV9MigrationPreflight,
} from "../src/m5/contracts/storage-lifetime-v1.ts";

const root = resolve("tests/fixtures/m5/storage-v9");
const manifest = readJson(resolve(root, "manifest.json"));
const policy = readJson(resolve(root, "witness-policy.json"));
const cases = readJson(resolve(root, "cases.json"));
const activation = readJson(resolve("tests/fixtures/m5/storage-activation-v2/manifest.json"));
const migrations = [
  "storage-v1/migration.sql",
  "storage-v2/migration-v1-to-v2.sql",
  "storage-v3/migration-v2-to-v3.sql",
  "storage-v4/migration-v3-to-v4.sql",
  "storage-v5/migration-v4-to-v5.sql",
  "storage-v6/migration-v5-to-v6.sql",
  "storage-v7/migration-v6-to-v7.sql",
  "storage-v8/migration-v7-to-v8.sql",
  "storage-v9/migration-v8-to-v9.sql",
] as const;

function readJson(path: string): Record<string, any> {
  return JSON.parse(readFileSync(path, "utf8")) as Record<string, any>;
}

function makeRows(template: Record<string, unknown>): Record<string, unknown>[] {
  return Array.from({ length: STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1 }, (_, index) => {
    const ordinal = index + 1;
    const row = structuredClone(template);
    Object.assign(row, {
      ordinal,
      eventRowid: ordinal,
      eventId: `event_${ordinal}`,
      operationId: `operation_${ordinal}`,
      aggregateSequence: ordinal,
    });
    if (ordinal === STORAGE_COMPACTION_PLAN_EXPECTED_ROWS_V1) {
      Object.assign(row, {
        decision: "retainAnchor",
        baselineId: null,
        baselineProjectionDigest: null,
      });
    }
    return row;
  });
}

function setPath(value: Record<string, any>, path: string, replacement: unknown): void {
  const segments = path.split(".");
  const leaf = segments.pop();
  assert.ok(leaf);
  let target = value;
  for (const segment of segments) target = target[segment] as Record<string, any>;
  target[leaf] = replacement;
}

function makeJob(
  state: "pending" | "running" | "succeeded" | "failed",
  cursor: Record<string, any>,
  authorityCount = 0,
): Record<string, any> {
  const phase = [
    "planEligibility", "freezeOperations", "freezeMessages", "verifyPrefix",
    "authorizeDelete", "deletePrefix", "verifyOperations", "verifyEquivalence",
    "releasePlan", "finalize",
  ].indexOf(cursor.phase);
  const exhausted = cursor.scanExhausted === true;
  return {
    jobId: "compact_v9_001",
    state,
    privateState: {
      schemaVersion: 3,
      cursor,
      planRowsComplete: phase > 0 || exhausted,
      operationsVerified: phase > 6 || (phase === 6 && exhausted),
      equivalenceVerified: phase > 7 || (phase === 7 && exhausted),
      planRowsReleased: phase > 8 || (phase === 8 && exhausted),
    },
    authorityCount,
    failureKind: null,
    completedAt: null,
  };
}

test("V9 witness and activation V2 goldens are additive and public-wire neutral", () => {
  assert.equal(manifest.storageSchemaVersion, 9);
  assert.equal(manifest.migration.fromVersion, 8);
  assert.equal(manifest.migration.toVersion, 9);
  assert.equal(manifest.migration.existingRowsDeleted, false);
  assert.equal(manifest.runtimeImplemented, false);
  assert.equal(manifest.publicWireChanged, false);
  assert.deepEqual(validateStorageCompactionResumeWitnessPolicyV1(policy), policy);
  assert.deepEqual(policy, STORAGE_COMPACTION_RESUME_WITNESS_POLICY_V1);
  assert.deepEqual(validateStorageLifetimeActivationPolicyV2(activation), activation);
  assert.deepEqual(activation, STORAGE_LIFETIME_ACTIVATION_POLICY_V2);
  assert.equal(STORAGE_LIFETIME_ACTIVATION_POLICY_V1.databaseMigration, "none");
  assert.deepEqual(activation.publicWire, STORAGE_LIFETIME_ACTIVATION_POLICY_V1.publicWire);
  assert.doesNotThrow(() => validateStorageLifetimeActivationPolicyV1(
    structuredClone(STORAGE_LIFETIME_ACTIVATION_POLICY_V1),
  ));
});

test("exact V1 through V9 fixture migration creates guarded witness objects", () => {
  const database = new DatabaseSync(":memory:");
  try {
    for (const path of migrations) {
      database.exec(readFileSync(resolve("tests/fixtures/m5", path), "utf8"));
    }
    assert.equal(
      database.prepare("SELECT storage_schema_version AS version FROM storage_schema_meta").get()!
        .version,
      9,
    );
    const planRows = database.prepare(
      "SELECT sql FROM sqlite_schema WHERE type='table' AND name='storage_compaction_plan_rows'",
    ).get() as { sql: string };
    assert.match(planRows.sql, /WITHOUT ROWID/);
    const names = database.prepare(
      "SELECT name FROM sqlite_schema WHERE type IN ('index','trigger') AND name LIKE ? ORDER BY name",
    ).all("%compaction%") as Array<{ name: string }>;
    const actual = new Set(names.map(({ name }) => name));
    for (const name of [
      "maintenance_jobs_single_active_lossless_compaction",
      "storage_compaction_plans_exact_insert",
      "storage_compaction_plans_immutable_update",
      "storage_compaction_plans_immutable_delete",
      "storage_compaction_plan_rows_exact_insert",
      "storage_compaction_plan_rows_immutable_update",
      "storage_compaction_plan_rows_release_only",
      "ledger_compaction_prefixes_require_v9_plan",
      "ledger_compaction_delete_authority_require_v9_plan",
    ]) {
      assert.equal(actual.has(name), true, name);
    }
  } finally {
    database.close();
  }
});

test("V8 to V9 migration rejects an active legacy compaction before persistent DDL", () => {
  const database = new DatabaseSync(":memory:");
  try {
    for (const path of migrations.slice(0, -1)) {
      database.exec(readFileSync(resolve("tests/fixtures/m5", path), "utf8"));
    }
    database.prepare(`
      INSERT INTO maintenance_jobs (
        job_id, job_kind, state, cutoff_json, started_at, completed_at,
        last_successful_at, failure_kind, created_at, updated_at
      ) VALUES (?, 'losslessCompaction', 'pending', ?, NULL, NULL, NULL, NULL, ?, ?)
    `).run(
      "legacy_v2_pending",
      JSON.stringify({ storageCompactionV2PrivateState: { schemaVersion: 1 } }),
      "2026-09-08T00:00:00Z",
      "2026-09-08T00:00:00Z",
    );
    assert.throws(() => validateStorageLifetimeV9MigrationPreflight(true, 1, 0));
    assert.throws(() => validateStorageLifetimeV9MigrationPreflight(true, 0, 1));
    assert.throws(() => validateStorageLifetimeV9MigrationPreflight(false, 0, 0));
    assert.equal(
      database.prepare("SELECT storage_schema_version AS version FROM storage_schema_meta").get()!
        .version,
      8,
    );
    assert.throws(() => database.prepare("SELECT * FROM storage_compaction_plans"));
  } finally {
    database.close();
  }
});

test("V9 plan validators require exact 1000 delete rows and one retained anchor", () => {
  const plan = validateStorageCompactionPlanV1(cases.validPlan);
  const rows = makeRows(cases.validDeleteRowTemplate);
  assert.doesNotThrow(() => validateStorageCompactionPlanRowBindingV1(
    plan, rows[0], false, null, false,
  ));
  for (let previous = 0; previous < rows.length; previous += 100) {
    const batch = rows.slice(previous, previous + 100);
    assert.equal(
      validateStorageCompactionPlanBatchV1(
        cases.validPlan,
        batch,
        previous,
        previous + batch.length === 1001,
      ).length,
      batch.length,
    );
  }
  assert.equal(
    validateStorageCompactionPlanBatchV1(cases.validPlan, rows.slice(0, 100), 0, false).length,
    100,
  );

  const headerReplacements: Record<string, unknown> = {
    deleteThroughSequence: 999,
    anchorAggregateSequence: 1002,
    expectedRows: 1000,
    baselineProjectionDigest: "A".repeat(64),
    previousSuccessfulJobId: plan.jobId,
    plannedAt: "2026-09-08T00:59:59Z",
  };
  for (const path of cases.malformedHeaderPaths as string[]) {
    const invalid = structuredClone(cases.validPlan);
    invalid[path] = headerReplacements[path];
    assert.throws(() => validateStorageCompactionPlanV1(invalid), undefined, path);
  }

  const rowReplacements: Record<string, unknown> = {
    ordinal: 0,
    aggregateSequence: 2,
    decision: "retainAnchor",
    canonicalPayloadDigest: "a".repeat(64),
    baselineId: "other",
    operationFrozenBeforePlan: true,
    messageFrozenBeforePlan: true,
  };
  for (const path of cases.malformedRowPaths as string[]) {
    const invalid = structuredClone(rows[0]);
    invalid[path] = rowReplacements[path];
    const validate = path === "operationFrozenBeforePlan" || path === "messageFrozenBeforePlan"
      ? () => validateStorageCompactionPlanRowBindingV1(plan, invalid, false, null, false)
      : () => validateStorageCompactionPlanRowV1(plan, invalid);
    assert.throws(validate, undefined, path);
  }
});

test("V9 resume verifier rejects shared missing, gap, duplicate, and exhaustion cases", () => {
  const rows = makeRows(cases.validDeleteRowTemplate);
  for (const name of cases.malformedPrefixCases as string[]) {
    let invalid = structuredClone(rows.slice(0, 100));
    let previous = 0;
    let exhausted = false;
    switch (name) {
      case "missing": invalid.shift(); break;
      case "gap": invalid[1].ordinal = 3; break;
      case "duplicateOrdinal": invalid[1].ordinal = 1; break;
      case "duplicateSourceRowid": invalid[1].eventRowid = 1; break;
      case "duplicateOperation": invalid[1].operationId = "operation_1"; break;
      case "duplicateAggregateTuple": invalid[1].aggregateSequence = 1; break;
      case "exhaustedBefore1001": exhausted = true; break;
      case "notExhaustedAt1001": invalid = [structuredClone(rows[1000])]; previous = 1000; break;
      case "extraAfterCursor": invalid = [structuredClone(rows[1000])]; previous = 1001; break;
      default: assert.fail(`unknown prefix case ${name}`);
    }
    assert.throws(
      () => validateStorageCompactionPlanBatchV1(
        cases.validPlan, invalid, previous, exhausted,
      ),
      undefined,
      name,
    );
  }
});

test("V3 cursor is afterPlanOrdinal-only, ordered, bounded, and admission rejects V2", () => {
  const start = cases.validCursorStart;
  assert.doesNotThrow(() => validateStorageCompactionCursorTransitionV3(null, start, 0));
  const at100 = { ...start, afterPlanOrdinal: 100 };
  assert.doesNotThrow(() => validateStorageCompactionCursorTransitionV3(start, at100, 100));
  const exhausted = { ...start, afterPlanOrdinal: 1001, scanExhausted: true };
  const next = {
    ...start,
    phase: "freezeOperations",
  };
  assert.doesNotThrow(() => validateStorageCompactionCursorTransitionV3(exhausted, next, 0));

  for (const name of cases.malformedCursorCases as string[]) {
    assert.throws(() => {
      switch (name) {
        case "legacyV2": return validateStorageCompactionCursorV3({ ...start, schemaVersion: 2 });
        case "wrongPhase": return validateStorageCompactionCursorV3({ ...start, phase: "freeze" });
        case "skipPhase": return validateStorageCompactionCursorTransitionV3(
          exhausted, { ...next, phase: "freezeMessages" }, 0,
        );
        case "wrongCursorColumn": {
          const { afterPlanOrdinal: _, ...invalid } = start;
          return validateStorageCompactionCursorV3({ ...invalid, afterEventRowid: 0 });
        }
        case "over100Work": return validateStorageCompactionCursorTransitionV3(
          start, { ...start, afterPlanOrdinal: 101 }, 101,
        );
        case "exhaustionBefore1001": return validateStorageCompactionCursorV3({
          ...start, afterPlanOrdinal: 100, scanExhausted: true,
        });
        case "progressAfterExhaustion": return validateStorageCompactionCursorTransitionV3(
          exhausted, exhausted, 1,
        );
        default: return assert.fail(`unknown cursor case ${name}`);
      }
    }, undefined, name);
  }

  const pendingStart = makeJob("pending", structuredClone(start));
  const runningStart = makeJob("running", structuredClone(start));
  const pending100 = makeJob("pending", structuredClone(at100));
  for (const name of cases.validLifecycleCases as string[]) {
    const result = name === "armSeparateCommit"
      ? () => validateStorageCompactionLifecycleTransitionV3(
        "arm", pendingStart, runningStart, 0,
      )
      : name === "batchSuccessAtomic"
        ? () => validateStorageCompactionLifecycleTransitionV3(
          "batchSuccess", runningStart, pending100, 100,
        )
        : name === "commitUncertainPendingAdvanced"
          ? () => validateStorageCompactionLifecycleTransitionV3(
            "commitUncertain", runningStart, pending100, 100,
          )
          : () => assert.fail(`unknown lifecycle case ${name}`);
    assert.doesNotThrow(result, name);
  }
  for (const name of cases.malformedLifecycleCases as string[]) {
    assert.throws(() => {
      switch (name) {
        case "armAdvancesCursor":
          return validateStorageCompactionLifecycleTransitionV3(
            "arm", pendingStart, makeJob("running", structuredClone(at100)), 0,
          );
        case "batchLeavesAuthority":
          return validateStorageCompactionLifecycleTransitionV3(
            "batchSuccess", runningStart, makeJob("pending", structuredClone(at100), 1), 100,
          );
        case "runningAutoResumeWithoutVerification":
          return validateStorageCompactionLifecycleTransitionV3(
            "batchSuccess", runningStart, pendingStart, 0,
          );
        case "completeBeforeRelease": {
          const finalCursor = {
            ...start,
            phase: "finalize",
            afterPlanOrdinal: 0,
            scanExhausted: true,
          };
          const pending = makeJob("pending", finalCursor);
          pending.privateState.planRowsReleased = false;
          const succeeded = structuredClone(pending);
          succeeded.state = "succeeded";
          succeeded.completedAt = "2026-09-08T02:00:00Z";
          return validateStorageCompactionLifecycleTransitionV3(
            "complete", pending, succeeded, 0,
          );
        }
        default: return assert.fail(`unknown lifecycle case ${name}`);
      }
    }, undefined, name);
  }

  assert.equal(validateStorageCompactionAdmissionStateV3(null), null);
  assert.doesNotThrow(() => validateStorageCompactionAdmissionStateV3(cases.validAdmissionPending));
  const releasedPending = makeJob("pending", {
    ...start,
    phase: "finalize",
    afterPlanOrdinal: 0,
    scanExhausted: true,
  });
  assert.doesNotThrow(() => validateStorageCompactionAdmissionStateV3({
    jobId: releasedPending.jobId,
    state: releasedPending.state,
    privateState: releasedPending.privateState,
    authorityCount: 0,
    planHeaderPresent: true,
  }));
  for (const name of cases.malformedAdmissionCases as string[]) {
    const invalid = structuredClone(cases.validAdmissionPending);
    switch (name) {
      case "activeV2Pending": invalid.privateState.schemaVersion = 2; break;
      case "running": invalid.state = "running"; break;
      case "nonEmptyAuthority": invalid.authorityCount = 1; break;
      case "missingPlan": invalid.planHeaderPresent = false; break;
      case "releasedBeforeEquivalence": invalid.privateState.planRowsReleased = true; break;
      case "unknownField": invalid.unknown = true; break;
      default: assert.fail(`unknown admission case ${name}`);
    }
    assert.throws(() => validateStorageCompactionAdmissionStateV3(invalid), undefined, name);
  }
});

test("shared policy rejects threshold, resume, release, writer, and public-wire drift", () => {
  for (const path of cases.malformedPolicyPaths as string[]) {
    const invalid = structuredClone(policy);
    const segments = path.split(".");
    let target: any = invalid;
    for (const segment of segments.slice(0, -1)) target = target[segment];
    const leaf = segments.at(-1)!;
    const current = target[leaf];
    setPath(invalid, path, typeof current === "boolean" ? !current
      : typeof current === "number" ? 999 : "relaxed");
    assert.throws(
      () => validateStorageCompactionResumeWitnessPolicyV1(invalid),
      undefined,
      path,
    );
  }
});
