import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { DatabaseSync } from "node:sqlite";

import {
  STORAGE_COMPACTION_DELETE_TRANSACTION_POLICY_V1,
  STORAGE_LIFETIME_ACTIVATION_POLICY_V2,
  STORAGE_LIFETIME_ACTIVATION_POLICY_V3,
  validateStorageCompactionDeleteTransactionPolicyV1,
  validateStorageLifetimeActivationPolicyV2,
  validateStorageLifetimeActivationPolicyV3,
  validateStorageLifetimeV10MigrationPreflight,
} from "../src/m5/contracts/storage-lifetime-v1.ts";

const fixtureRoot = resolve("tests/fixtures/m5");
const throughV9 = [
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
const migrationV10 = readFileSync(
  resolve(fixtureRoot, "storage-v10/migration-v9-to-v10.sql"), "utf8",
);
const manifest = readJson("storage-v10/manifest.json");
const policy = readJson("storage-v10/delete-policy.json");
const cases = readJson("storage-v10/cases.json");
const schemaIdentity = JSON.parse(readFileSync(
  resolve(fixtureRoot, "storage-v10/schema-identity.json"), "utf8",
)) as Record<string, any>;
const activation = readJson("storage-activation-v3/manifest.json");
const phases = [
  "planEligibility", "freezeOperations", "freezeMessages", "verifyPrefix",
  "authorizeDelete", "deletePrefix", "verifyOperations", "verifyEquivalence",
  "releasePlan", "finalize",
] as const;

function readJson(path: string): Record<string, any> {
  return JSON.parse(readFileSync(resolve(fixtureRoot, path), "utf8"));
}

function databaseThroughV9(): DatabaseSync {
  const database = new DatabaseSync(":memory:");
  for (const path of throughV9) {
    database.exec(readFileSync(resolve(fixtureRoot, path), "utf8"));
  }
  return database;
}

function schemaSnapshot(database: DatabaseSync): unknown[] {
  return database.prepare(`
    SELECT type, name, tbl_name AS tableName, sql
    FROM sqlite_schema
    WHERE name NOT LIKE 'sqlite_autoindex%'
    ORDER BY type, name
  `).all();
}

function schemaIdentitySnapshot(database: DatabaseSync): unknown[] {
  return (schemaSnapshot(database) as Array<{
    type: string;
    name: string;
    tableName: string;
    sql: string;
  }>).map(({ type, name, tableName, sql }) => ({
    type,
    name,
    tableName,
    canonicalSqlSha256: createHash("sha256").update(sql).digest("hex"),
  }));
}

function version(database: DatabaseSync): number {
  return Number(database.prepare(
    "SELECT storage_schema_version AS version FROM storage_schema_meta",
  ).get()!.version);
}

function assertMigrationRollback(database: DatabaseSync, label: string): void {
  const before = schemaSnapshot(database);
  assert.throws(() => database.exec(migrationV10), /exact idle v9/, label);
  assert.deepEqual(schemaSnapshot(database), before, label);
  assert.equal(version(database), 9, label);
  assert.equal(database.prepare(`
    SELECT COUNT(*) AS count FROM sqlite_schema WHERE name LIKE '%v10%'
  `).get()!.count, 0, label);
}

function privateState(
  phase: typeof phases[number],
  afterPlanOrdinal: number,
  options: { step?: number | null; exhausted?: boolean } = {},
): Record<string, unknown> {
  const phaseIndex = phases.indexOf(phase);
  const exhausted = options.exhausted ?? false;
  return {
    storageCompactionV3PrivateState: {
      schemaVersion: 3,
      cursor: {
        schemaVersion: 3,
        cutoffEventRowid: 2000,
        phase,
        afterPlanOrdinal,
        stepThroughPlanOrdinal: options.step ?? null,
        scanExhausted: exhausted,
      },
      planRowsComplete: phaseIndex > 0 || exhausted,
      operationsVerified: phaseIndex > 6 || (phaseIndex === 6 && exhausted),
      equivalenceVerified: phaseIndex > 7 || (phaseIndex === 7 && exhausted),
      planRowsReleased: phaseIndex > 8 || (phaseIndex === 8 && exhausted),
    },
  };
}

function insertJob(
  database: DatabaseSync,
  jobId: string,
  state: "pending" | "running" | "succeeded",
  cutoff: unknown,
): void {
  database.prepare(`
    INSERT INTO maintenance_jobs (
      job_id, job_kind, state, cutoff_json, started_at, completed_at,
      last_successful_at, failure_kind, created_at, updated_at
    ) VALUES (?, 'losslessCompaction', ?, ?, NULL, ?, NULL, NULL, ?, ?)
  `).run(
    jobId,
    state,
    JSON.stringify(cutoff),
    state === "succeeded" ? "2026-09-08T00:00:01Z" : null,
    "2026-09-08T00:00:00Z",
    "2026-09-08T00:00:00Z",
  );
}

function setJob(
  database: DatabaseSync,
  state: "pending" | "running" | "succeeded",
  cutoff: Record<string, unknown>,
): void {
  database.prepare(`
    UPDATE maintenance_jobs
    SET state = ?, cutoff_json = ?, failure_kind = NULL,
        completed_at = CASE WHEN ? = 'succeeded' THEN '2026-09-08T02:00:00Z' ELSE NULL END,
        updated_at = '2026-09-08T02:00:00Z'
    WHERE job_id = 'compact_v10_sql'
  `).run(state, JSON.stringify(cutoff), state);
}

function readCutoff(database: DatabaseSync): Record<string, any> {
  const row = database.prepare(`
    SELECT cutoff_json AS cutoff FROM maintenance_jobs WHERE job_id = 'compact_v10_sql'
  `).get() as { cutoff: string };
  return JSON.parse(row.cutoff);
}

function insertPlanRow(database: DatabaseSync, ordinal: number): void {
  const anchor = ordinal === 1001;
  database.prepare(`
    INSERT INTO storage_compaction_plan_rows (
      job_id, ordinal, event_rowid, event_id, operation_id, request_id,
      aggregate_type, aggregate_id, aggregate_sequence, event_type,
      payload_schema_version, event_timestamp, canonical_payload_json,
      canonical_payload_digest, introduced_message_id, introduced_message_digest,
      decision, baseline_id, baseline_projection_digest,
      operation_frozen_before_plan, message_frozen_before_plan
    ) VALUES ('compact_v10_sql', ?, ?, ?, ?, NULL, 'project', 'project_v10', ?,
              'project.updated', 1, '2026-09-08T01:00:00Z', '{}', ?, NULL, NULL,
              ?, ?, ?, 0, 0)
  `).run(
    ordinal,
    ordinal,
    `event_v10_${ordinal}`,
    `operation_v10_${ordinal}`,
    ordinal,
    "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
    anchor ? "retainAnchor" : "delete",
    anchor ? null : "baseline_v10_1000",
    anchor ? null : "b".repeat(64),
  );
}

function planBatch(database: DatabaseSync, start: number, through: number): void {
  setJob(database, "running", privateState("planEligibility", start - 1, { step: through }));
  database.exec("BEGIN IMMEDIATE");
  for (let ordinal = start; ordinal <= through; ordinal += 1) insertPlanRow(database, ordinal);
  const cutoff = readCutoff(database);
  cutoff.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal = null;
  setJob(database, "pending", cutoff);
  database.exec("COMMIT");
}

function scanPhase(database: DatabaseSync, phase: typeof phases[number]): void {
  const limit = phase === "releasePlan" || phase === "finalize" ? 1000 : 1001;
  for (let start = 1; start <= limit; start += 100) {
    const through = Math.min(start + 99, limit);
    setJob(database, "running", privateState(phase, start - 1, { step: through }));
    setJob(database, "pending", privateState(phase, through, { exhausted: through === limit }));
  }
}

function advancePhase(
  database: DatabaseSync,
  phase: typeof phases[number],
  next: typeof phases[number],
): void {
  const limit = phase === "releasePlan" ? 1000 : 1001;
  setJob(database, "running", privateState(phase, limit, { exhausted: true }));
  setJob(database, "pending", privateState(
    next,
    next === "finalize" ? 1000 : 0,
    { exhausted: next === "finalize" },
  ));
}

function buildDeleteReadyDatabase(): DatabaseSync {
  const database = databaseThroughV9();
  database.exec(migrationV10);
  database.exec("BEGIN IMMEDIATE");
  const event = database.prepare(`
    INSERT INTO events (
      event_id, operation_id, request_id, aggregate_type, aggregate_id,
      aggregate_sequence, event_type, payload_schema_version,
      canonical_json_payload, timestamp
    ) VALUES (?, ?, NULL, 'project', 'project_v10', ?, 'project.updated', 1,
              '{}', '2026-09-08T01:00:00Z')
  `);
  const receipt = database.prepare(`
    INSERT INTO operation_receipts (
      operation_id, event_id, aggregate_type, aggregate_id, aggregate_sequence,
      event_type, payload_schema_version, canonical_payload_json,
      canonical_payload_digest_algorithm, canonical_payload_digest_version,
      canonical_payload_digest_format, canonical_payload_digest,
      event_timestamp, materialized_at
    ) VALUES (?, ?, 'project', 'project_v10', ?, 'project.updated', 1, '{}',
              'sha256', 1, 'lowercaseHex64', ?,
              '2026-09-08T01:00:00Z', '2026-09-08T01:00:00Z')
  `);
  const sourceIndex = database.prepare(`
    INSERT INTO event_source_index (
      event_rowid, event_id, operation_id, aggregate_type, aggregate_id,
      aggregate_sequence, introduced_message_id, chat_durable_sequence,
      chat_snapshot_message_id
    ) VALUES (?, ?, ?, 'project', 'project_v10', ?, NULL, NULL, NULL)
  `);
  const digest = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
  for (let ordinal = 1; ordinal <= 1001; ordinal += 1) {
    event.run(`event_v10_${ordinal}`, `operation_v10_${ordinal}`, ordinal);
    receipt.run(`operation_v10_${ordinal}`, `event_v10_${ordinal}`, ordinal, digest);
    sourceIndex.run(ordinal, `event_v10_${ordinal}`, `operation_v10_${ordinal}`, ordinal);
  }
  database.prepare(`
    INSERT INTO ledger_baselines (
      baseline_id, aggregate_type, aggregate_id, cutoff_aggregate_sequence,
      payload_schema_version, canonical_projection_json, projection_digest,
      created_at, verified_at
    ) VALUES ('baseline_v10_1000', 'project', 'project_v10', 1000, 1, '{}', ?,
              '2026-09-08T01:00:00Z', '2026-09-08T01:00:00Z')
  `).run(`sha256:${"b".repeat(64)}`);
  database.exec("COMMIT");

  insertJob(database, "compact_v10_sql", "pending", privateState("planEligibility", 0));
  database.prepare(`
    INSERT INTO storage_compaction_plans (
      job_id, plan_schema_version, cutoff_event_rowid, aggregate_type, aggregate_id,
      retained_boundary_sequence, delete_through_sequence, anchor_aggregate_sequence,
      expected_rows, baseline_id, baseline_projection_digest,
      previous_successful_job_id, created_at, planned_at
    ) VALUES ('compact_v10_sql', 1, 2000, 'project', 'project_v10', 0, 1000, 1001,
              1001, 'baseline_v10_1000', ?, NULL,
              '2026-09-08T01:00:00Z', '2026-09-08T01:00:01Z')
  `).run("b".repeat(64));
  for (let start = 1; start <= 1001; start += 100) {
    planBatch(database, start, Math.min(start + 99, 1001));
  }
  advancePhase(database, "planEligibility", "freezeOperations");
  scanPhase(database, "freezeOperations");
  advancePhase(database, "freezeOperations", "freezeMessages");
  scanPhase(database, "freezeMessages");
  advancePhase(database, "freezeMessages", "verifyPrefix");
  scanPhase(database, "verifyPrefix");
  setJob(database, "running", privateState("verifyPrefix", 1001, { exhausted: true }));
  database.prepare(`
    INSERT INTO ledger_compaction_prefixes (
      prefix_id, aggregate_type, aggregate_id, deleted_through_sequence,
      anchor_event_rowid, anchor_event_id, anchor_operation_id,
      anchor_aggregate_sequence, baseline_id, baseline_projection_digest,
      created_at, verified_at
    ) VALUES ('prefix_v10', 'project', 'project_v10', 1000, 1001,
              'event_v10_1001', 'operation_v10_1001', 1001,
              'baseline_v10_1000', ?,
              '2026-09-08T01:10:00Z', '2026-09-08T01:10:00Z')
  `).run(`sha256:${"b".repeat(64)}`);
  setJob(database, "pending", privateState("authorizeDelete", 0));
  return database;
}

function arm(database: DatabaseSync, phase: "authorizeDelete" | "deletePrefix", after: number, through: number): void {
  setJob(database, "running", privateState(phase, after, { step: through }));
}

function insertAuthority(
  database: DatabaseSync,
  authorityOrdinal: number,
  planOrdinal: number,
  prefixId = "prefix_v10",
): void {
  database.prepare(`
    INSERT INTO ledger_compaction_delete_authority (
      job_id, ordinal, prefix_id, event_rowid, event_id, operation_id,
      aggregate_type, aggregate_id, aggregate_sequence, authorized_at
    ) VALUES ('compact_v10_sql', ?, ?, ?, ?, ?, 'project', 'project_v10', ?,
              '2026-09-08T01:20:00Z')
  `).run(
    authorityOrdinal,
    prefixId,
    planOrdinal,
    `event_v10_${planOrdinal}`,
    `operation_v10_${planOrdinal}`,
    planOrdinal,
  );
}

function runDeleteBatch(database: DatabaseSync, start: number, through: number): void {
  arm(database, "deletePrefix", start - 1, through);
  database.exec("BEGIN IMMEDIATE");
  for (let planOrdinal = start; planOrdinal <= Math.min(through, 1000); planOrdinal += 1) {
    insertAuthority(database, planOrdinal - start + 1, planOrdinal);
  }
  if (start > 1000) {
    setJob(database, "pending", privateState("deletePrefix", through, { exhausted: true }));
  }
  database.exec("COMMIT");
}

function sourceSnapshot(database: DatabaseSync): unknown {
  return {
    events: database.prepare("SELECT rowid, event_id FROM events ORDER BY rowid").all(),
    sourceIndex: database.prepare(
      "SELECT event_rowid, event_id FROM event_source_index ORDER BY event_rowid",
    ).all(),
    authority: database.prepare(
      "SELECT * FROM ledger_compaction_delete_authority ORDER BY job_id, ordinal",
    ).all(),
    cutoff: readCutoff(database),
  };
}

function databaseSnapshot(database: DatabaseSync): unknown {
  const tables = database.prepare(`
    SELECT name FROM sqlite_schema
    WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
    ORDER BY name
  `).all() as Array<{ name: string }>;
  return {
    schema: schemaSnapshot(database),
    tables: Object.fromEntries(tables.map(({ name }) => [
      name,
      database.prepare(`SELECT * FROM "${name.replaceAll('"', '""')}"`).all(),
    ])),
  };
}

test("V10 delete repair and activation V3 goldens are exact and public-wire neutral", () => {
  assert.equal(manifest.storageSchemaVersion, 10);
  assert.equal(manifest.migration.fromVersion, 9);
  assert.equal(manifest.migration.toVersion, 10);
  assert.equal(manifest.runtimeWriterImplemented, false);
  assert.equal(manifest.productExecutionEnabled, false);
  assert.equal(manifest.publicWireChanged, false);
  assert.deepEqual(validateStorageCompactionDeleteTransactionPolicyV1(policy), policy);
  assert.deepEqual(policy, STORAGE_COMPACTION_DELETE_TRANSACTION_POLICY_V1);
  assert.deepEqual(validateStorageLifetimeActivationPolicyV3(activation), activation);
  assert.deepEqual(activation, STORAGE_LIFETIME_ACTIVATION_POLICY_V3);
  assert.deepEqual(activation.publicWire, STORAGE_LIFETIME_ACTIVATION_POLICY_V2.publicWire);
  assert.doesNotThrow(() => validateStorageLifetimeActivationPolicyV2(
    structuredClone(STORAGE_LIFETIME_ACTIVATION_POLICY_V2),
  ));

  for (const path of cases.malformedPolicyPaths as string[]) {
    const invalid = structuredClone(policy);
    const parts = path.split(".");
    let target: Record<string, any> = invalid;
    for (const part of parts.slice(0, -1)) target = target[part];
    const leaf = parts.at(-1)!;
    const current = target[leaf];
    target[leaf] = typeof current === "boolean" ? !current
      : typeof current === "number" ? 999 : "relaxed";
    assert.throws(() => validateStorageCompactionDeleteTransactionPolicyV1(invalid), undefined, path);
  }
});

test("exact idle V9 migrates to V10 and preserves succeeded data", () => {
  const database = databaseThroughV9();
  try {
    insertJob(database, "succeeded_v9", "succeeded", { historical: true });
    const before = database.prepare(
      "SELECT * FROM maintenance_jobs WHERE job_id = 'succeeded_v9'",
    ).get();
    database.exec(migrationV10);
    assert.equal(version(database), 10);
    assert.deepEqual(schemaIdentitySnapshot(database), schemaIdentity.objects);
    assert.equal(
      database.prepare(`
        SELECT COUNT(*) AS count FROM sqlite_schema
        WHERE name NOT LIKE 'sqlite_autoindex%'
      `).get()!.count,
      manifest.schemaIdentity.persistentObjects,
    );
    assert.deepEqual(
      database.prepare("SELECT * FROM maintenance_jobs WHERE job_id = 'succeeded_v9'").get(),
      before,
    );
    for (const name of manifest.migration.triggersAdded as string[]) {
      assert.equal(database.prepare(
        "SELECT COUNT(*) AS count FROM sqlite_schema WHERE type='trigger' AND name=?",
      ).get(name)!.count, 1, name);
    }
    assert.equal(database.prepare(`
      SELECT COUNT(*) AS count FROM sqlite_schema
      WHERE name='ledger_compaction_delete_authority_require_v9_plan'
    `).get()!.count, 0);

    database.exec("DROP TRIGGER events_require_v10_complete_delete_batch");
    database.exec(`
      CREATE TRIGGER events_require_v10_complete_delete_batch
      BEFORE DELETE ON events
      BEGIN
        SELECT 1;
      END
    `);
    assert.notDeepEqual(schemaIdentitySnapshot(database), schemaIdentity.objects);
  } finally {
    database.close();
  }
});

test("V9 drift, active jobs, and authority roll back before persistent V10 DDL", () => {
  const drift = databaseThroughV9();
  try {
    drift.exec("CREATE TABLE unexpected_v9_drift(id INTEGER PRIMARY KEY) STRICT");
    assertMigrationRollback(drift, "schema drift");
  } finally {
    drift.close();
  }

  const weakened = databaseThroughV9();
  try {
    weakened.exec("DROP TRIGGER ledger_compaction_delete_authority_require_v9_plan");
    weakened.exec(`
      CREATE TRIGGER ledger_compaction_delete_authority_require_v9_plan
      BEFORE INSERT ON ledger_compaction_delete_authority
      BEGIN
        SELECT 1;
      END
    `);
    assertMigrationRollback(weakened, "same-name weakened v9 authority trigger");
  } finally {
    weakened.close();
  }

  for (const state of ["pending", "running"] as const) {
    const database = databaseThroughV9();
    try {
      insertJob(database, `active_${state}`, state, privateState("planEligibility", 0));
      assertMigrationRollback(database, state);
      assert.equal(database.prepare(
        "SELECT state FROM maintenance_jobs WHERE job_id=?",
      ).get(`active_${state}`)!.state, state);
    } finally {
      database.close();
    }
  }

  const authority = databaseThroughV9();
  try {
    const trigger = authority.prepare(`
      SELECT sql FROM sqlite_schema
      WHERE name='ledger_compaction_delete_authority_require_v9_plan'
    `).get() as { sql: string };
    authority.exec("PRAGMA foreign_keys = OFF");
    authority.exec("DROP TRIGGER ledger_compaction_delete_authority_require_v9_plan");
    authority.exec(`
      INSERT INTO ledger_compaction_delete_authority (
        job_id, ordinal, prefix_id, event_rowid, event_id, operation_id,
        aggregate_type, aggregate_id, aggregate_sequence, authorized_at
      ) VALUES ('old', 1, 'old_prefix', 1, 'old_event', 'old_operation',
                'project', 'old_project', 1, '2026-09-08T00:00:00Z')
    `);
    authority.exec(trigger.sql);
    authority.exec("PRAGMA foreign_keys = ON");
    assertMigrationRollback(authority, "authority");
    assert.equal(authority.prepare(
      "SELECT COUNT(*) AS count FROM ledger_compaction_delete_authority",
    ).get()!.count, 1);
  } finally {
    authority.close();
  }

  assert.doesNotThrow(() => validateStorageLifetimeV10MigrationPreflight(true, 0, 0));
  assert.throws(() => validateStorageLifetimeV10MigrationPreflight(false, 0, 0));
  assert.throws(() => validateStorageLifetimeV10MigrationPreflight(true, 1, 0));
  assert.throws(() => validateStorageLifetimeV10MigrationPreflight(true, 0, 1));
});

test("V10 authority is deletePrefix-only, exact, contiguous, bounded, and atomic", () => {
  const database = buildDeleteReadyDatabase();
  try {
    arm(database, "authorizeDelete", 0, 100);
    assert.throws(() => insertAuthority(database, 1, 1), /exact v10 armed deletePrefix/);
    setJob(database, "pending", privateState("authorizeDelete", 100));
    for (let start = 101; start <= 1001; start += 100) {
      const through = Math.min(start + 99, 1001);
      arm(database, "authorizeDelete", start - 1, through);
      setJob(database, "pending", privateState(
        "authorizeDelete", through, { exhausted: through === 1001 },
      ));
    }
    advancePhase(database, "authorizeDelete", "deletePrefix");

    assert.throws(() => arm(database, "deletePrefix", 0, 101), /invalid storage v9/);
    arm(database, "deletePrefix", 0, 100);
    const beforeNoAuthority = databaseSnapshot(database);
    assert.throws(() => database.exec(
      "DELETE FROM event_source_index WHERE event_rowid = 1",
    ), /exact v7 authority/);
    assert.throws(() => database.exec(
      "DELETE FROM events WHERE rowid = 1",
    ), /(complete v10 armed authority batch|exact v7 authority)/);
    assert.deepEqual(databaseSnapshot(database), beforeNoAuthority);
    assert.throws(() => insertAuthority(database, 2, 2), /exact v10 armed deletePrefix/);
    assert.throws(() => insertAuthority(database, 1, 101), /exact v10 armed deletePrefix/);
    assert.throws(() => insertAuthority(database, 1, 1, "wrong_prefix"), /exact v10 armed deletePrefix/);
    assert.throws(() => database.exec(`
      INSERT INTO ledger_compaction_delete_authority (
        job_id, ordinal, prefix_id, event_rowid, event_id, operation_id,
        aggregate_type, aggregate_id, aggregate_sequence, authorized_at
      ) VALUES ('compact_v10_sql', 1, 'prefix_v10', 1, 'wrong_event',
                'operation_v10_1', 'project', 'project_v10', 1,
                '2026-09-08T01:20:00Z')
    `), /exact v10 armed deletePrefix/);

    const beforeFailed = sourceSnapshot(database);
    database.exec("BEGIN IMMEDIATE");
    insertAuthority(database, 1, 1);
    const afterPartialAuthority = databaseSnapshot(database);
    assert.throws(() => database.exec(
      "DELETE FROM events WHERE rowid = 1",
    ), /complete v10 armed authority batch after source index deletion/);
    assert.deepEqual(databaseSnapshot(database), afterPartialAuthority);
    database.exec("ROLLBACK");
    assert.deepEqual(sourceSnapshot(database), beforeFailed);

    database.exec("BEGIN IMMEDIATE");
    for (let ordinal = 1; ordinal < 100; ordinal += 1) insertAuthority(database, ordinal, ordinal);
    assert.throws(() => database.exec(
      "DELETE FROM event_source_index WHERE event_rowid = 1",
    ), /complete v10 armed authority batch/);
    database.exec("ROLLBACK");
    assert.deepEqual(sourceSnapshot(database), beforeFailed);

    database.exec("BEGIN IMMEDIATE");
    for (let ordinal = 1; ordinal <= 100; ordinal += 1) insertAuthority(database, ordinal, ordinal);
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM ledger_compaction_delete_authority",
    ).get()!.count, 0);
    assert.equal(database.prepare(
      "SELECT state FROM maintenance_jobs WHERE job_id='compact_v10_sql'",
    ).get()!.state, "pending");
    assert.equal(
      readCutoff(database).storageCompactionV3PrivateState.cursor.afterPlanOrdinal,
      100,
    );
    database.exec("COMMIT");
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM events WHERE rowid <= 100",
    ).get()!.count, 0);
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM event_source_index WHERE event_rowid <= 100",
    ).get()!.count, 0);
  } finally {
    database.close();
  }
});

test("V10 event deletion requires source-index-first order with a complete batch", () => {
  const database = buildDeleteReadyDatabase();
  try {
    scanPhase(database, "authorizeDelete");
    advancePhase(database, "authorizeDelete", "deletePrefix");
    arm(database, "deletePrefix", 0, 100);
    database.exec("DROP TRIGGER ledger_compaction_delete_authority_execute_v10_batch");
    for (let ordinal = 1; ordinal <= 100; ordinal += 1) {
      insertAuthority(database, ordinal, ordinal);
    }
    const beforeWrongOrder = databaseSnapshot(database);
    assert.throws(() => database.exec(
      "DELETE FROM events WHERE rowid = 1",
    ), /complete v10 armed authority batch after source index deletion/);
    assert.deepEqual(databaseSnapshot(database), beforeWrongOrder);
  } finally {
    database.close();
  }
});

test("V10 delete completion preserves anchor, release, and finalize path", () => {
  const database = buildDeleteReadyDatabase();
  try {
    scanPhase(database, "authorizeDelete");
    advancePhase(database, "authorizeDelete", "deletePrefix");
    for (let start = 1; start <= 1000; start += 100) {
      runDeleteBatch(database, start, Math.min(start + 99, 1000));
    }
    runDeleteBatch(database, 1001, 1001);
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM events WHERE rowid <= 1000",
    ).get()!.count, 0);
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM events WHERE rowid = 1001",
    ).get()!.count, 1);
    assert.equal(database.prepare(
      "SELECT COUNT(*) AS count FROM operation_receipts",
    ).get()!.count, 1001);

    advancePhase(database, "deletePrefix", "verifyOperations");
    scanPhase(database, "verifyOperations");
    advancePhase(database, "verifyOperations", "verifyEquivalence");
    scanPhase(database, "verifyEquivalence");
    advancePhase(database, "verifyEquivalence", "releasePlan");
    for (let start = 1; start <= 1000; start += 100) {
      const through = Math.min(start + 99, 1000);
      setJob(database, "running", privateState("releasePlan", start - 1, { step: through }));
      database.exec("BEGIN IMMEDIATE");
      database.exec(`
        DELETE FROM storage_compaction_plan_rows
        WHERE job_id='compact_v10_sql' AND ordinal BETWEEN ${start} AND ${through}
      `);
      const cutoff = readCutoff(database);
      cutoff.storageCompactionV3PrivateState.cursor.stepThroughPlanOrdinal = null;
      setJob(database, "pending", cutoff);
      database.exec("COMMIT");
    }
    const retained = database.prepare(`
      SELECT ordinal, decision FROM storage_compaction_plan_rows
      WHERE job_id='compact_v10_sql'
    `).all() as Array<{ ordinal: number; decision: string }>;
    assert.equal(retained.length, 1);
    assert.equal(retained[0]?.ordinal, 1001);
    assert.equal(retained[0]?.decision, "retainAnchor");
    advancePhase(database, "releasePlan", "finalize");
    setJob(database, "succeeded", privateState("finalize", 1000, { exhausted: true }));
    assert.equal(database.prepare(
      "SELECT state FROM maintenance_jobs WHERE job_id='compact_v10_sql'",
    ).get()!.state, "succeeded");
  } finally {
    database.close();
  }
});
