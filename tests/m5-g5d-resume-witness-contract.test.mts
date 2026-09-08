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
const migrationV9Sql = readFileSync(
  resolve("tests/fixtures/m5/storage-v9/migration-v8-to-v9.sql"), "utf8",
);
const v3Phases = [
  "planEligibility", "freezeOperations", "freezeMessages", "verifyPrefix",
  "authorizeDelete", "deletePrefix", "verifyOperations", "verifyEquivalence",
  "releasePlan", "finalize",
] as const;

function readJson(path: string): Record<string, any> {
  return JSON.parse(readFileSync(path, "utf8")) as Record<string, any>;
}

function databaseThroughV8(): DatabaseSync {
  const database = new DatabaseSync(":memory:");
  for (const path of migrations.slice(0, -1)) {
    database.exec(readFileSync(resolve("tests/fixtures/m5", path), "utf8"));
  }
  return database;
}

function persistentSchema(database: DatabaseSync): unknown[] {
  return database.prepare(`
    SELECT type, name, tbl_name AS tableName, sql
    FROM sqlite_schema
    WHERE name NOT LIKE 'sqlite_autoindex%'
    ORDER BY type, name
  `).all();
}

function assertV9MigrationRollsBackBeforeDdl(database: DatabaseSync, label: string): void {
  const before = persistentSchema(database);
  assert.throws(() => database.exec(migrationV9Sql), /exact idle v8/);
  assert.deepEqual(persistentSchema(database), before, label);
  assert.equal(database.prepare(
    "SELECT storage_schema_version AS version FROM storage_schema_meta",
  ).get()!.version, 8);
  assert.throws(() => database.prepare("SELECT * FROM storage_compaction_plans"));
}

function insertMaintenanceJob(
  database: DatabaseSync,
  jobId: string,
  state: "pending" | "running" | "succeeded",
  cutoff: unknown,
): void {
  database.prepare(`
    INSERT INTO maintenance_jobs (
      job_id, job_kind, state, cutoff_json, started_at, completed_at,
      last_successful_at, failure_kind, created_at, updated_at
    ) VALUES (?, 'losslessCompaction', ?, ?, NULL, NULL, NULL, NULL, ?, ?)
  `).run(
    jobId, state, JSON.stringify(cutoff),
    "2026-09-08T00:00:00Z", "2026-09-08T00:00:00Z",
  );
}

function privateState(
  phase: string,
  afterPlanOrdinal: number,
  options: {
    stepThroughPlanOrdinal?: number | null;
    scanExhausted?: boolean;
    planRowsComplete?: boolean;
    operationsVerified?: boolean;
    equivalenceVerified?: boolean;
    planRowsReleased?: boolean;
  } = {},
): Record<string, unknown> {
  const phaseIndex = v3Phases.indexOf(phase as typeof v3Phases[number]);
  const scanExhausted = options.scanExhausted ?? false;
  return {
    storageCompactionV3PrivateState: {
      schemaVersion: 3,
      cursor: {
        schemaVersion: 3,
        cutoffEventRowid: 2000,
        phase,
        afterPlanOrdinal,
        stepThroughPlanOrdinal: options.stepThroughPlanOrdinal ?? null,
        scanExhausted,
      },
      planRowsComplete: options.planRowsComplete ?? (phaseIndex > 0 || scanExhausted),
      operationsVerified: options.operationsVerified
        ?? (phaseIndex > 6 || (phaseIndex === 6 && scanExhausted)),
      equivalenceVerified: options.equivalenceVerified
        ?? (phaseIndex > 7 || (phaseIndex === 7 && scanExhausted)),
      planRowsReleased: options.planRowsReleased
        ?? (phaseIndex > 8 || (phaseIndex === 8 && scanExhausted)),
    },
  };
}

function setJobState(
  database: DatabaseSync,
  state: "pending" | "running" | "succeeded" | "failed",
  cutoff: Record<string, unknown>,
): void {
  database.prepare(`
    UPDATE maintenance_jobs
    SET state = ?, cutoff_json = ?,
        completed_at = CASE WHEN ? IN ('succeeded', 'failed') THEN ? ELSE NULL END,
        failure_kind = CASE WHEN ? = 'failed' THEN 'contractError' ELSE NULL END,
        updated_at = ?
    WHERE job_id = 'compact_v9_sql'
  `).run(
    state,
    JSON.stringify(cutoff),
    state,
    "2026-09-08T01:00:02Z",
    state,
    "2026-09-08T01:00:02Z",
  );
}

function scanSqlPhase(database: DatabaseSync, phase: typeof v3Phases[number]): void {
  const limit = phase === "releasePlan" || phase === "finalize" ? 1000 : 1001;
  for (let start = 1; start <= limit; start += 100) {
    const through = Math.min(start + 99, limit);
    setJobState(database, "running", privateState(phase, start - 1, {
      stepThroughPlanOrdinal: through,
    }));
    setJobState(database, "pending", privateState(phase, through, {
      scanExhausted: through === limit,
    }));
  }
}

function advanceSqlPhase(
  database: DatabaseSync,
  phase: typeof v3Phases[number],
  nextPhase: typeof v3Phases[number],
): void {
  const limit = phase === "releasePlan" ? 1000 : 1001;
  setJobState(database, "running", privateState(phase, limit, { scanExhausted: true }));
  setJobState(database, "pending", privateState(
    nextPhase,
    nextPhase === "finalize" ? 1000 : 0,
    { scanExhausted: nextPhase === "finalize" },
  ));
}

function readJobCutoff(database: DatabaseSync): Record<string, any> {
  const row = database.prepare(`
    SELECT cutoff_json AS cutoff FROM maintenance_jobs WHERE job_id = 'compact_v9_sql'
  `).get() as { cutoff: string };
  return JSON.parse(row.cutoff) as Record<string, any>;
}

function databaseWithV9PlanSources(): DatabaseSync {
  const database = new DatabaseSync(":memory:");
  for (const path of migrations) {
    database.exec(readFileSync(resolve("tests/fixtures/m5", path), "utf8"));
  }
  database.exec("BEGIN IMMEDIATE");
  const event = database.prepare(`
    INSERT INTO events (
      event_id, operation_id, request_id, aggregate_type, aggregate_id,
      aggregate_sequence, event_type, payload_schema_version,
      canonical_json_payload, timestamp
    ) VALUES (?, ?, NULL, 'project', 'project_001', ?, 'project.updated', 1, '{}', ?)
  `);
  const receipt = database.prepare(`
    INSERT INTO operation_receipts (
      operation_id, event_id, aggregate_type, aggregate_id, aggregate_sequence,
      event_type, payload_schema_version, canonical_payload_json,
      canonical_payload_digest_algorithm, canonical_payload_digest_version,
      canonical_payload_digest_format, canonical_payload_digest, event_timestamp, materialized_at
    ) VALUES (?, ?, 'project', 'project_001', ?, 'project.updated', 1, '{}',
              'sha256', 1, 'lowercaseHex64', ?, ?, ?)
  `);
  const sourceIndex = database.prepare(`
    INSERT INTO event_source_index (
      event_rowid, event_id, operation_id, aggregate_type, aggregate_id,
      aggregate_sequence, introduced_message_id, chat_durable_sequence,
      chat_snapshot_message_id
    ) VALUES (?, ?, ?, 'project', 'project_001', ?, NULL, NULL, NULL)
  `);
  const digest = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
  const timestamp = "2026-09-08T01:00:00Z";
  for (let ordinal = 1; ordinal <= 1001; ordinal += 1) {
    event.run(`event_${ordinal}`, `operation_${ordinal}`, ordinal, timestamp);
    receipt.run(
      `operation_${ordinal}`, `event_${ordinal}`, ordinal, digest, timestamp, timestamp,
    );
    sourceIndex.run(ordinal, `event_${ordinal}`, `operation_${ordinal}`, ordinal);
  }
  database.prepare(`
    INSERT INTO ledger_baselines (
      baseline_id, aggregate_type, aggregate_id, cutoff_aggregate_sequence,
      payload_schema_version, canonical_projection_json, projection_digest,
      created_at, verified_at
    ) VALUES ('baseline_1000', 'project', 'project_001', 1000, 1, '{}', ?, ?, ?)
  `).run("b".repeat(64), timestamp, timestamp);
  database.exec("COMMIT");

  insertMaintenanceJob(
    database, "compact_v9_sql", "pending", privateState("planEligibility", 0),
  );
  database.prepare(`
    INSERT INTO storage_compaction_plans (
      job_id, plan_schema_version, cutoff_event_rowid, aggregate_type, aggregate_id,
      retained_boundary_sequence, delete_through_sequence, anchor_aggregate_sequence,
      expected_rows, baseline_id, baseline_projection_digest,
      previous_successful_job_id, created_at, planned_at
    ) VALUES ('compact_v9_sql', 1, 2000, 'project', 'project_001', 0, 1000, 1001,
              1001, 'baseline_1000', ?, NULL, ?, ?)
  `).run("b".repeat(64), timestamp, "2026-09-08T01:00:01Z");
  return database;
}

function insertSqlPlanRow(database: DatabaseSync, ordinal: number): void {
  const anchor = ordinal === 1001;
  database.prepare(`
    INSERT INTO storage_compaction_plan_rows (
      job_id, ordinal, event_rowid, event_id, operation_id, request_id,
      aggregate_type, aggregate_id, aggregate_sequence, event_type,
      payload_schema_version, event_timestamp, canonical_payload_json,
      canonical_payload_digest, introduced_message_id, introduced_message_digest,
      decision, baseline_id, baseline_projection_digest,
      operation_frozen_before_plan, message_frozen_before_plan
    ) VALUES ('compact_v9_sql', ?, ?, ?, ?, NULL, 'project', 'project_001', ?,
              'project.updated', 1, ?, '{}', ?, NULL, NULL, ?, ?, ?, 0, 0)
  `).run(
    ordinal, ordinal, `event_${ordinal}`, `operation_${ordinal}`, ordinal,
    "2026-09-08T01:00:00Z",
    "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
    anchor ? "retainAnchor" : "delete",
    anchor ? null : "baseline_1000",
    anchor ? null : "b".repeat(64),
  );
}

function planSqlBatch(database: DatabaseSync, start: number, end: number): void {
  setJobState(database, "running", privateState("planEligibility", start - 1, {
    stepThroughPlanOrdinal: end,
  }));
  database.exec("BEGIN IMMEDIATE");
  for (let ordinal = start; ordinal <= end; ordinal += 1) insertSqlPlanRow(database, ordinal);
  const committed = readJobCutoff(database);
  committed.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal = null;
  setJobState(database, "pending", committed);
  database.exec("COMMIT");
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
      "storage_compaction_plan_rows_advance_cursor",
      "storage_compaction_plan_rows_immutable_update",
      "storage_compaction_plan_rows_release_only",
      "storage_compaction_plan_rows_advance_release_cursor",
      "maintenance_jobs_guard_v9_compaction_transition",
      "ledger_compaction_prefixes_require_v9_plan",
      "ledger_compaction_delete_authority_require_v9_plan",
    ]) {
      assert.equal(actual.has(name), true, name);
    }
  } finally {
    database.close();
  }
});

test("actual V8 to V9 SQL rolls back before persistent DDL for every unsafe ledger state", () => {
  for (const state of ["pending", "running"] as const) {
    const database = databaseThroughV8();
    try {
      insertMaintenanceJob(database, `legacy_v2_${state}`, state, {
        storageCompactionV2PrivateState: { schemaVersion: 2 },
      });
      assertV9MigrationRollsBackBeforeDdl(database, state);
    } finally {
      database.close();
    }
  }

  const database = databaseThroughV8();
  try {
    const trigger = database.prepare(`
      SELECT sql FROM sqlite_schema
      WHERE type = 'trigger' AND name = 'ledger_compaction_delete_authority_require_exact_tuple'
    `).get() as { sql: string };
    database.exec("PRAGMA foreign_keys = OFF");
    database.exec("DROP TRIGGER ledger_compaction_delete_authority_require_exact_tuple");
    database.prepare(`
      INSERT INTO ledger_compaction_delete_authority (
        job_id, ordinal, prefix_id, event_rowid, event_id, operation_id,
        aggregate_type, aggregate_id, aggregate_sequence, authorized_at
      ) VALUES ('old_done', 1, 'old_prefix', 1, 'old_event', 'old_operation',
                'project', 'project_001', 1, '2026-09-08T00:00:00Z')
    `).run();
    database.exec(trigger.sql);
    database.exec("PRAGMA foreign_keys = ON");
    assertV9MigrationRollsBackBeforeDdl(database, "nonempty authority");
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM ledger_compaction_delete_authority",
    ).get()!.count, 1);
  } finally {
    database.close();
  }

  const drifted = databaseThroughV8();
  try {
    drifted.exec("CREATE TABLE unexpected_v8_drift (id INTEGER PRIMARY KEY) STRICT");
    assertV9MigrationRollsBackBeforeDdl(drifted, "extra v8 object");
  } finally {
    drifted.close();
  }

  const weakenedTrigger = databaseThroughV8();
  try {
    weakenedTrigger.exec("DROP TRIGGER events_are_append_only_delete");
    weakenedTrigger.exec(`
      CREATE TRIGGER events_are_append_only_delete
      BEFORE DELETE ON events
      BEGIN
        SELECT 1;
      END
    `);
    assertV9MigrationRollsBackBeforeDdl(weakenedTrigger, "same-name weakened trigger");
  } finally {
    weakenedTrigger.close();
  }

  const weakenedIndex = databaseThroughV8();
  try {
    weakenedIndex.exec("DROP INDEX maintenance_jobs_single_running_lossless_compaction");
    weakenedIndex.exec(`
      CREATE INDEX maintenance_jobs_single_running_lossless_compaction
      ON maintenance_jobs (job_kind)
      WHERE job_kind = 'losslessCompaction'
    `);
    assertV9MigrationRollsBackBeforeDdl(weakenedIndex, "same-name weakened index");
  } finally {
    weakenedIndex.close();
  }

  const driftedTable = databaseThroughV8();
  try {
    driftedTable.exec("ALTER TABLE maintenance_jobs ADD COLUMN unchecked_state TEXT");
    assertV9MigrationRollsBackBeforeDdl(driftedTable, "same-name drifted table");
  } finally {
    driftedTable.close();
  }

  assert.throws(() => validateStorageLifetimeV9MigrationPreflight(true, 1, 0));
  assert.throws(() => validateStorageLifetimeV9MigrationPreflight(true, 0, 1));
  assert.throws(() => validateStorageLifetimeV9MigrationPreflight(false, 0, 0));
});

test("actual V9 triggers enforce exact transitions, planning, prefix proof, and release", () => {
  const database = databaseWithV9PlanSources();
  try {
    const changedCutoff = privateState("planEligibility", 0, {
      stepThroughPlanOrdinal: 100,
    }) as Record<string, any>;
    changedCutoff.storageCompactionV3PrivateState.cursor.cutoffEventRowid = 1999;
    assert.throws(() => setJobState(database, "running", changedCutoff),
      /invalid storage v9 maintenance transition/);
    assert.throws(() => setJobState(database, "running", privateState("planEligibility", 0, {
      stepThroughPlanOrdinal: 101,
    })), /invalid storage v9 maintenance transition/);
    setJobState(database, "running", privateState("planEligibility", 0, {
      stepThroughPlanOrdinal: 100,
    }));
    assert.throws(() => insertSqlPlanRow(database, 1001), /exact witnessed source/);
    assert.throws(() => insertSqlPlanRow(database, 2), /exact witnessed source/);
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM storage_compaction_plan_rows",
    ).get()!.count, 0);

    planSqlBatch(database, 1, 100);
    const prefixInsert = database.prepare(`
      INSERT INTO ledger_compaction_prefixes (
        prefix_id, aggregate_type, aggregate_id, deleted_through_sequence,
        anchor_event_rowid, anchor_event_id, anchor_operation_id,
        anchor_aggregate_sequence, baseline_id, baseline_projection_digest,
        created_at, verified_at
      ) VALUES ('prefix_v9_sql', 'project', 'project_001', 1000, 1001,
                'event_1001', 'operation_1001', 1001, 'baseline_1000', ?, ?, ?)
    `);
    assert.throws(() => prefixInsert.run(
      "b".repeat(64), "2026-09-08T01:10:00Z", "2026-09-08T01:10:00Z",
    ), /exact v9 plan anchor/);
    assert.throws(() => setJobState(database, "running", privateState("verifyPrefix", 100, {
      stepThroughPlanOrdinal: 200,
    })), /invalid storage v9 maintenance transition/);

    for (let start = 101; start <= 1001; start += 100) {
      planSqlBatch(database, start, Math.min(start + 99, 1001));
    }
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM storage_compaction_plan_rows",
    ).get()!.count, 1001);
    const completedPlanState = readJobCutoff(database).storageCompactionV3PrivateState;
    assert.equal(typeof completedPlanState.cursor.scanExhausted, "boolean");
    assert.equal(typeof completedPlanState.planRowsComplete, "boolean");

    const forgedFlags = readJobCutoff(database);
    forgedFlags.storageCompactionV3PrivateState.operationsVerified = true;
    assert.throws(() => setJobState(database, "running", forgedFlags),
      /invalid storage v9 maintenance transition/);
    assert.throws(() => setJobState(database, "running", privateState("releasePlan", 0)),
      /invalid storage v9 maintenance transition/);
    assert.throws(() => setJobState(database, "running", privateState("freezeMessages", 0, {
      stepThroughPlanOrdinal: 100,
    })), /invalid storage v9 maintenance transition/);
    assert.throws(() => setJobState(database, "running", privateState("planEligibility", 1000, {
      stepThroughPlanOrdinal: 1001,
    })), /invalid storage v9 maintenance transition/);

    assert.throws(() => prefixInsert.run(
      "b".repeat(64), "2026-09-08T01:10:00Z", "2026-09-08T01:10:00Z",
    ), /exact v9 plan anchor/);

    advanceSqlPhase(database, "planEligibility", "freezeOperations");
    assert.throws(() => setJobState(database, "running", privateState("planEligibility", 0, {
      stepThroughPlanOrdinal: 100,
    })), /invalid storage v9 maintenance transition/);
    scanSqlPhase(database, "freezeOperations");
    advanceSqlPhase(database, "freezeOperations", "freezeMessages");
    scanSqlPhase(database, "freezeMessages");
    advanceSqlPhase(database, "freezeMessages", "verifyPrefix");
    scanSqlPhase(database, "verifyPrefix");

    assert.throws(() => prefixInsert.run(
      "b".repeat(64), "2026-09-08T01:10:00Z", "2026-09-08T01:10:00Z",
    ), /exact v9 plan anchor/);
    setJobState(database, "running", privateState("verifyPrefix", 1001, {
      scanExhausted: true,
    }));
    prefixInsert.run(
      "b".repeat(64), "2026-09-08T01:10:00Z", "2026-09-08T01:10:00Z",
    );
    setJobState(database, "pending", privateState("authorizeDelete", 0));
    scanSqlPhase(database, "authorizeDelete");
    advanceSqlPhase(database, "authorizeDelete", "deletePrefix");
    scanSqlPhase(database, "deletePrefix");
    advanceSqlPhase(database, "deletePrefix", "verifyOperations");
    scanSqlPhase(database, "verifyOperations");
    advanceSqlPhase(database, "verifyOperations", "verifyEquivalence");
    scanSqlPhase(database, "verifyEquivalence");
    advanceSqlPhase(database, "verifyEquivalence", "releasePlan");

    const releaseState = (after: number, through: number | null) => privateState(
      "releasePlan", after, {
        stepThroughPlanOrdinal: through,
        scanExhausted: after === 1000,
      },
    );
    setJobState(database, "running", releaseState(0, 100));
    assert.throws(() => database.exec(
      "DELETE FROM storage_compaction_plan_rows WHERE job_id='compact_v9_sql' AND ordinal=1001",
    ), /next armed delete ordinal/);
    assert.throws(() => database.exec(
      "DELETE FROM storage_compaction_plan_rows WHERE job_id='compact_v9_sql' AND ordinal=2",
    ), /next armed delete ordinal/);
    assert.throws(() => setJobState(database, "running", releaseState(1, 100)),
      /invalid storage v9 maintenance transition/);

    assert.throws(() => database.exec(`
      DELETE FROM storage_compaction_plan_rows
      WHERE job_id='compact_v9_sql' AND ordinal BETWEEN 1 AND 101
    `), /next armed delete ordinal/);
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM storage_compaction_plan_rows",
    ).get()!.count, 1001);
    assert.equal(
      readJobCutoff(database).storageCompactionV3PrivateState.cursor.afterPlanOrdinal, 0,
    );

    database.exec("BEGIN IMMEDIATE");
    database.exec(`
      DELETE FROM storage_compaction_plan_rows
      WHERE job_id='compact_v9_sql' AND ordinal BETWEEN 1 AND 100
    `);
    assert.equal(
      readJobCutoff(database).storageCompactionV3PrivateState.cursor.afterPlanOrdinal, 100,
      "delete trigger advances the cursor inside the same transaction",
    );
    database.exec("ROLLBACK");
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM storage_compaction_plan_rows",
    ).get()!.count, 1001);
    assert.equal(
      readJobCutoff(database).storageCompactionV3PrivateState.cursor.afterPlanOrdinal, 0,
      "rollback restores both rows and cursor",
    );

    for (let start = 1; start <= 1000; start += 100) {
      const through = Math.min(start + 99, 1000);
      setJobState(database, "running", releaseState(start - 1, through));
      database.exec("BEGIN IMMEDIATE");
      database.exec(`
        DELETE FROM storage_compaction_plan_rows
        WHERE job_id='compact_v9_sql' AND ordinal BETWEEN ${start} AND ${through}
      `);
      const committed = readJobCutoff(database);
      committed.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal = null;
      setJobState(database, "pending", committed);
      database.exec("COMMIT");
    }
    const terminal = readJobCutoff(database).storageCompactionV3PrivateState;
    assert.equal(terminal.cursor.afterPlanOrdinal, 1000);
    assert.equal(Boolean(terminal.cursor.scanExhausted), true);
    assert.equal(Boolean(terminal.planRowsReleased), true);
    const retained = database.prepare(`
      SELECT ordinal, decision FROM storage_compaction_plan_rows
      WHERE job_id='compact_v9_sql'
    `).all() as Array<{ ordinal: number; decision: string }>;
    assert.equal(retained.length, 1);
    assert.equal(retained[0]?.ordinal, 1001);
    assert.equal(retained[0]?.decision, "retainAnchor");

    advanceSqlPhase(database, "releasePlan", "finalize");
    setJobState(database, "succeeded", privateState("finalize", 1000, {
      scanExhausted: true,
    }));
    assert.equal(database.prepare(
      "SELECT state FROM maintenance_jobs WHERE job_id='compact_v9_sql'",
    ).get()!.state, "succeeded");
  } finally {
    database.close();
  }
});

test("actual V9 transition guard permits exact failure but rejects failure cursor mutation", () => {
  const database = databaseWithV9PlanSources();
  try {
    assert.throws(() => database.exec(`
      UPDATE maintenance_jobs SET state='running' WHERE job_id='compact_v9_sql'
    `), /invalid storage v9 maintenance transition/);
    assert.throws(() => database.prepare(`
      UPDATE maintenance_jobs SET cutoff_json=? WHERE job_id='compact_v9_sql'
    `).run(JSON.stringify(privateState("planEligibility", 0, {
      planRowsComplete: true,
    }))), /invalid storage v9 maintenance transition/);

    const armed = privateState("planEligibility", 0, { stepThroughPlanOrdinal: 100 });
    setJobState(database, "running", armed);
    const changed = structuredClone(armed) as Record<string, any>;
    changed.storageCompactionV3PrivateState.cursor.afterPlanOrdinal = 1;
    assert.throws(() => setJobState(database, "failed", changed),
      /invalid storage v9 maintenance transition/);
    setJobState(database, "failed", armed);
    assert.equal(database.prepare(
      "SELECT state FROM maintenance_jobs WHERE job_id='compact_v9_sql'",
    ).get()!.state, "failed");
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

  let coerced = false;
  const hostileType = structuredClone(cases.validPlan);
  hostileType.aggregateType = {
    toString() {
      coerced = true;
      return "chat";
    },
  };
  assert.throws(() => validateStorageCompactionPlanV1(hostileType));
  assert.equal(coerced, false, "aggregateType is never coerced");

  const inherited = Object.assign(Object.create({ aggregateType: "chat" }), cases.validPlan);
  assert.throws(() => validateStorageCompactionPlanV1(inherited), /plain enumerable data object/);

  let getterRead = false;
  const accessor = structuredClone(cases.validPlan);
  Object.defineProperty(accessor, "aggregateType", {
    enumerable: true,
    get() {
      getterRead = true;
      return "chat";
    },
  });
  assert.throws(() => validateStorageCompactionPlanV1(accessor), /plain enumerable data object/);
  assert.equal(getterRead, false, "accessor is rejected before property evaluation");
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
  const armed100 = { ...start, stepThroughPlanOrdinal: 100 };
  const at100 = { ...start, afterPlanOrdinal: 100, stepThroughPlanOrdinal: null };
  assert.doesNotThrow(() => validateStorageCompactionCursorTransitionV3(armed100, at100, 100));
  const exhausted = {
    ...start, afterPlanOrdinal: 1001, stepThroughPlanOrdinal: null, scanExhausted: true,
  };
  const next = {
    ...start,
    phase: "freezeOperations",
  };
  assert.doesNotThrow(() => validateStorageCompactionCursorTransitionV3(exhausted, next, 0));

  const releaseArmed = {
    ...start, phase: "releasePlan", stepThroughPlanOrdinal: 100,
  };
  const released100 = {
    ...releaseArmed, afterPlanOrdinal: 100, stepThroughPlanOrdinal: null,
  };
  assert.doesNotThrow(() => validateStorageCompactionCursorTransitionV3(
    releaseArmed, released100, 100,
  ));
  const releaseEnd = {
    ...start, phase: "releasePlan", afterPlanOrdinal: 1000,
    stepThroughPlanOrdinal: null, scanExhausted: true,
  };
  const finalize = { ...releaseEnd, phase: "finalize" };
  assert.doesNotThrow(() => validateStorageCompactionCursorTransitionV3(
    releaseEnd, finalize, 0,
  ));
  assert.throws(() => validateStorageCompactionCursorV3({
    ...releaseEnd, afterPlanOrdinal: 1001,
  }));
  assert.throws(() => validateStorageCompactionCursorV3({
    ...finalize, afterPlanOrdinal: 0,
  }));

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
          { ...start, stepThroughPlanOrdinal: 101 },
          { ...start, afterPlanOrdinal: 101 }, 101,
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
  const runningStart = makeJob("running", structuredClone(armed100));
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
            afterPlanOrdinal: 1000,
            stepThroughPlanOrdinal: null,
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
    afterPlanOrdinal: 1000,
    stepThroughPlanOrdinal: null,
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
