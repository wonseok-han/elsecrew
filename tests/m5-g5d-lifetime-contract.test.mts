import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";

import { M5_BRIDGE_V5_METHODS } from "../src/m5/contracts/bridge-v5.ts";
import { M5_BRIDGE_V6_METHODS } from "../src/m5/contracts/bridge-v6.ts";
import { METHODS_V1, PROTOCOL_VERSION } from "../src/m5/contracts/protocol-v1.ts";
import {
  LOSSLESS_COMPACTION_STEPS_V1,
  NEVER_CLEANUP_STATES_V1,
  OPERATION_RECEIPT_DIGEST_ALGORITHM_V2,
  OPERATION_RECEIPT_DIGEST_FORMAT_V2,
  OPERATION_RECEIPT_DIGEST_VERSION_V2,
  STORAGE_LIFETIME_INDEXES_V2,
  STORAGE_LIFETIME_SCHEMA_VERSION_V1,
  STORAGE_LIFETIME_TABLES_V2,
  STORAGE_STATUS_BRIDGE_METHOD_V1,
  STORAGE_STATUS_BRIDGE_VERSION_V1,
  STORAGE_STATUS_METHOD_V1,
  STORAGE_STATUS_SCHEMA_VERSION_V1,
  classifyOperationAgainstReceiptV2,
  validateStorageStatusRequestV1,
  validateStorageStatusV1,
} from "../src/m5/contracts/storage-lifetime-v1.ts";

interface Manifest {
  schemaVersion: number;
  storageSchemaVersion: number;
  protocolVersion: number;
  wire: {
    method: string;
    payloadSchemaVersion: number;
    bridgeVersion: number;
    bridgeMethod: string;
    advertisedBeforeD1: boolean;
    dispatchableBeforeD1: boolean;
    casesFile: string;
  };
  migration: {
    file: string;
    fromVersion: number;
    toVersion: number;
    mode: string;
    tables: string[];
    indexes: string[];
    existingTablesPreserved: string[];
    existingEventsDeleted: boolean;
    existingProjectionColumnsChanged: boolean;
    startupReadyBeforeMaterialization: boolean;
    materializationTrigger: string;
    downgradePolicy: string;
  };
  compaction: {
    mode: string;
    steps: string[];
    baselineTailReplayEquivalent: boolean;
    oldOperationDuplicateConflictPreserved: boolean;
    actualLegacyNamespacePreserved: boolean;
    failureRollbackRequired: boolean;
    logicalDeleteBeforeVerification: boolean;
    vacuumIsLogicalSuccessCriterion: boolean;
    physicalCleanupEnabled: boolean;
    cleanupCandidateApiEnabled: boolean;
    cleanupActionApiEnabled: boolean;
    operationReceiptsImmutable: boolean;
    chatMessagesImmutable: boolean;
    operationReceipt: {
      casesFile: string;
      immutableFields: string[];
      requestIdComparison: string;
      payloadComparison: string;
      digest: {
        algorithm: string;
        version: number;
        format: string;
        role: string;
        writerMustRecompute: boolean;
      };
    };
    neverCleanupStates: string[];
  };
  publicStatusPrivacy: {
    allowedContent: string;
    forbiddenFields: string[];
  };
}

interface StatusCases {
  valid: Array<{ name: string; request: unknown; response: unknown }>;
  invalid: Array<{ name: string; target: "request" | "response"; value: unknown }>;
}

interface OperationReceiptCases {
  cases: Array<{
    name: string;
    namespace: string;
    receipt: { canonicalPayloadJson: string; canonicalPayloadDigest: string };
    incoming: unknown;
    expected: "append" | "duplicateNoOp" | "conflict";
  }>;
  invalid: Array<{
    name: string;
    expectedError: string;
    receipt: { canonicalPayloadJson: string; canonicalPayloadDigest: string };
  }>;
}

const root = resolve("tests/fixtures/m5/storage-v2");
const manifest = readJson<Manifest>(resolve(root, "manifest.json"));
const statusCases = readJson<StatusCases>(resolve(root, manifest.wire.casesFile));
const receiptCases = readJson<OperationReceiptCases>(
  resolve(root, manifest.compaction.operationReceipt.casesFile),
);

test("storage-v2 golden migration is additive and preserves v1 rows byte-for-byte", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.storageSchemaVersion, STORAGE_LIFETIME_SCHEMA_VERSION_V1);
  assert.equal(manifest.protocolVersion, PROTOCOL_VERSION);
  assert.deepEqual(manifest.migration.tables, [...STORAGE_LIFETIME_TABLES_V2]);
  assert.deepEqual(manifest.migration.indexes, [...STORAGE_LIFETIME_INDEXES_V2]);
  assert.equal(manifest.migration.mode, "additiveDdlIndexesMetadataOnly");
  assert.equal(manifest.migration.existingEventsDeleted, false);
  assert.equal(manifest.migration.existingProjectionColumnsChanged, false);
  assert.equal(manifest.migration.startupReadyBeforeMaterialization, true);
  assert.equal(manifest.migration.materializationTrigger, "idleAfterReady");
  assert.equal(manifest.migration.downgradePolicy, "openQuestion");

  const temporaryRoot = mkdtempSync(join(tmpdir(), "hello-agent-g5d-contract-"));
  const database = new DatabaseSync(join(temporaryRoot, "storage.sqlite3"), {
    enableForeignKeyConstraints: false,
  });
  try {
    database.exec(readFileSync(resolve("tests/fixtures/m5/storage-v1/migration.sql"), "utf8"));
    database.exec(`
      INSERT INTO app_instances (
        sidecar_instance_id, pid, platform, started_at, heartbeat_at,
        clean_shutdown_at, last_event_sequence
      ) VALUES (
        'sidecar_before_v2', 123, 'darwin', '2026-08-28T00:59:00Z',
        '2026-08-28T01:00:00Z', NULL, 1
      );
      INSERT INTO events (
        event_id, operation_id, request_id, aggregate_type, aggregate_id,
        aggregate_sequence, event_type, payload_schema_version,
        canonical_json_payload, timestamp
      ) VALUES (
        'evt_before_v2', 'op_before_v2', 'req_before_v2', 'project', 'project_before_v2',
        1, 'project.upserted', 1,
        '{"project_id":"project_before_v2"}', '2026-08-28T01:00:00Z'
      );
      INSERT INTO projects (
        project_id, canonical_path, projection_json, last_event_sequence
      ) VALUES (
        'project_before_v2', '/fixture/project', '{"project_id":"project_before_v2"}', 1
      ), (
        'project_other', '/fixture/other', '{"project_id":"project_other"}', 1
      );
      INSERT INTO chats (
        chat_id, project_id, projection_json, last_event_sequence
      ) VALUES (
        'chat_before_v2', 'project_before_v2', '{"chat_id":"chat_before_v2"}', 1
      ), (
        'chat_other_project', 'project_other', '{"chat_id":"chat_other_project"}', 1
      );
      INSERT INTO runs (
        run_id, chat_id, state, worktree_path, projection_json, last_event_sequence
      ) VALUES (
        'run_before_v2', 'chat_before_v2', 'recoveryRequired', '/fixture/worktree',
        '{"run_id":"run_before_v2"}', 1
      );
      INSERT INTO locks (
        lock_id, project_id, writer_run_id, worktree_path, poisoned,
        recovery_required, evidence_json, projection_json, last_event_sequence
      ) VALUES (
        'lock_before_v2', 'project_before_v2', 'run_before_v2', '/fixture/worktree',
        1, 1, '{}', '{"lock_id":"lock_before_v2"}', 1
      );
    `);
    const preservedTables = manifest.migration.existingTablesPreserved;
    const before = Object.fromEntries(
      preservedTables.map((table) => [table, normalizedRows(database, table)]),
    );

    database.exec(readFileSync(resolve(root, manifest.migration.file), "utf8"));

    for (const table of preservedTables) {
      assert.deepEqual(normalizedRows(database, table), before[table], table);
    }
    assert.deepEqual(
      { ...database.prepare("SELECT schema_version, protocol_version, migration_state FROM schema_meta").get() },
      { schema_version: 1, protocol_version: 1, migration_state: "ready" },
    );
    assert.deepEqual(
      { ...database.prepare("SELECT storage_schema_version, protocol_version, migration_state, materialization_state FROM storage_schema_meta").get() },
      { storage_schema_version: 2, protocol_version: 1, migration_state: "ready", materialization_state: "pending" },
    );
    for (const table of STORAGE_LIFETIME_TABLES_V2) {
      assert.equal(
        database.prepare("SELECT count(*) AS count FROM sqlite_schema WHERE type = 'table' AND name = ?").get(table)?.count,
        1,
        table,
      );
    }
    for (const index of STORAGE_LIFETIME_INDEXES_V2) {
      assert.equal(
        database.prepare("SELECT count(*) AS count FROM sqlite_schema WHERE type = 'index' AND name = ?").get(index)?.count,
        1,
        index,
      );
    }
    assert.throws(() => database.exec("UPDATE events SET event_type = 'changed' WHERE event_id = 'evt_before_v2';"));
    assert.throws(() => database.exec("DELETE FROM events WHERE event_id = 'evt_before_v2';"));
    database.exec(`
      INSERT INTO events (
        event_id, operation_id, request_id, aggregate_type, aggregate_id,
        aggregate_sequence, event_type, payload_schema_version,
        canonical_json_payload, timestamp
      ) VALUES (
        'evt_after_v2', 'op_after_v2', NULL, 'project', 'project_before_v2',
        2, 'project.upserted', 1,
        '{"project_id":"project_before_v2"}', '2026-08-28T01:01:00Z'
      );
    `);
    assert.equal(database.prepare("SELECT count(*) AS count FROM events").get()?.count, 2);

    database.exec(`
      INSERT INTO prepared_workspaces (
        preparation_id, project_id, worktree_id, workspace_identity, binding_state,
        bound_chat_id, canonical_record_json, record_digest,
        created_at, updated_at, materialized_at
      ) VALUES
        ('prep_unbound', 'project_before_v2', 'worktree_unbound', 'workspace_unbound',
         'unbound', NULL, '{}', 'digest-unbound',
         '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z'),
        ('prep_abandoned', 'project_before_v2', 'worktree_abandoned', 'workspace_abandoned',
         'abandoned', NULL, '{}', 'digest-abandoned',
         '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z'),
        ('prep_bound', 'project_before_v2', 'worktree_bound', 'workspace_bound',
         'bound', 'chat_before_v2', '{}', 'digest-bound',
         '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z');
    `);
    assert.equal(database.prepare("SELECT count(*) AS count FROM prepared_workspaces").get()?.count, 3);
    assert.throws(() => insertPreparedWorkspace(database, "invalid_bound_null", "bound", null, "project_before_v2"));
    assert.throws(() => insertPreparedWorkspace(database, "invalid_unbound_chat", "unbound", "chat_before_v2", "project_before_v2"));
    assert.throws(() => insertPreparedWorkspace(database, "invalid_abandoned_chat", "abandoned", "chat_before_v2", "project_before_v2"));
    assert.throws(() => insertPreparedWorkspace(database, "invalid_cross_project", "bound", "chat_other_project", "project_before_v2"));
  } finally {
    database.close();
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

test("TypeScript and Rust share exact privacy-safe storage.status cases", () => {
  assert.equal(manifest.wire.method, STORAGE_STATUS_METHOD_V1);
  assert.equal(manifest.wire.payloadSchemaVersion, STORAGE_STATUS_SCHEMA_VERSION_V1);
  assert.equal(manifest.wire.bridgeVersion, STORAGE_STATUS_BRIDGE_VERSION_V1);
  assert.equal(manifest.wire.bridgeMethod, STORAGE_STATUS_BRIDGE_METHOD_V1);
  assert.equal(manifest.publicStatusPrivacy.allowedContent, "aggregateCountsBytesAndMaintenanceStateOnly");
  assert.deepEqual(manifest.publicStatusPrivacy.forbiddenFields, [
    "rawChatText",
    "rawProviderText",
    "rawReasoning",
    "prompt",
    "token",
    "auth",
    "absolutePath",
  ]);

  for (const fixture of statusCases.valid) {
    assert.deepEqual(validateStorageStatusRequestV1(fixture.request), fixture.request, fixture.name);
    assert.deepEqual(validateStorageStatusV1(fixture.response), fixture.response, fixture.name);
  }
  for (const fixture of statusCases.invalid) {
    const validate = fixture.target === "request"
      ? () => validateStorageStatusRequestV1(fixture.value)
      : () => validateStorageStatusV1(fixture.value);
    assert.throws(validate, undefined, fixture.name);
  }
});

test("D0A freezes lossless compaction invariants without enabling cleanup or thresholds", () => {
  assert.equal(manifest.compaction.mode, "losslessInternalOnly");
  assert.deepEqual(manifest.compaction.steps, [...LOSSLESS_COMPACTION_STEPS_V1]);
  assert.deepEqual(manifest.compaction.neverCleanupStates, [...NEVER_CLEANUP_STATES_V1]);
  assert.equal(manifest.compaction.baselineTailReplayEquivalent, true);
  assert.equal(manifest.compaction.oldOperationDuplicateConflictPreserved, true);
  assert.equal(manifest.compaction.actualLegacyNamespacePreserved, true);
  assert.equal(manifest.compaction.failureRollbackRequired, true);
  assert.equal(manifest.compaction.logicalDeleteBeforeVerification, false);
  assert.equal(manifest.compaction.vacuumIsLogicalSuccessCriterion, false);
  assert.equal(manifest.compaction.physicalCleanupEnabled, false);
  assert.equal(manifest.compaction.cleanupCandidateApiEnabled, false);
  assert.equal(manifest.compaction.cleanupActionApiEnabled, false);
  assert.equal(manifest.compaction.operationReceiptsImmutable, true);
  assert.equal(manifest.compaction.chatMessagesImmutable, true);
  assert.equal(manifest.compaction.operationReceipt.payloadComparison, "exactCanonicalJson");
  assert.equal(manifest.compaction.operationReceipt.requestIdComparison, "ignoredLikeStorageV1");
  assert.deepEqual(manifest.compaction.operationReceipt.immutableFields, [
    "operationId", "eventId", "aggregateType", "aggregateId", "aggregateSequence",
    "eventType", "payloadSchemaVersion", "canonicalPayloadJson",
    "canonicalPayloadDigestAlgorithm", "canonicalPayloadDigestVersion",
    "canonicalPayloadDigestFormat", "canonicalPayloadDigest", "eventTimestamp",
  ]);
  assert.deepEqual(manifest.compaction.operationReceipt.digest, {
    algorithm: OPERATION_RECEIPT_DIGEST_ALGORITHM_V2,
    version: OPERATION_RECEIPT_DIGEST_VERSION_V2,
    format: OPERATION_RECEIPT_DIGEST_FORMAT_V2,
    role: "integrityCheckNotEqualitySubstitute",
    writerMustRecompute: true,
  });
  assert.equal(JSON.stringify(manifest).includes("warningThreshold"), false);
  assert.equal(JSON.stringify(manifest).includes("hardThreshold"), false);
});

test("operation receipts preserve exact v1 duplicate/conflict decisions across legacy, actual, and recovery namespaces", () => {
  const namespaces = new Set<string>();
  for (const fixture of receiptCases.cases) {
    namespaces.add(fixture.namespace);
    assert.equal(
      createHash("sha256").update(fixture.receipt.canonicalPayloadJson).digest("hex"),
      fixture.receipt.canonicalPayloadDigest,
      `${fixture.name} digest integrity`,
    );
    assert.equal(
      classifyOperationAgainstReceiptV2(fixture.receipt, fixture.incoming),
      fixture.expected,
      fixture.name,
    );
  }
  assert.deepEqual([...namespaces].sort(), ["actualAgent", "legacy", "recovery"]);
  for (const fixture of receiptCases.invalid) {
    assert.throws(
      () => classifyOperationAgainstReceiptV2(fixture.receipt, receiptCases.cases[0].incoming),
      new RegExp(fixture.expectedError),
      fixture.name,
    );
  }
});

test("planned bridge V6 is additive but storage.status remains unadvertised and undispatchable before D1", () => {
  assert.deepEqual(M5_BRIDGE_V6_METHODS.slice(0, M5_BRIDGE_V5_METHODS.length), M5_BRIDGE_V5_METHODS);
  assert.deepEqual(M5_BRIDGE_V6_METHODS.slice(M5_BRIDGE_V5_METHODS.length), ["getStorageStatus"]);
  assert.equal(manifest.wire.advertisedBeforeD1, false);
  assert.equal(manifest.wire.dispatchableBeforeD1, false);
  assert.equal(METHODS_V1.includes(STORAGE_STATUS_METHOD_V1 as never), false);
  assert.equal(M5_BRIDGE_V6_METHODS.some((method) => /purge|cleanup|delete/i.test(method)), false);
});

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

function normalizedRows(database: DatabaseSync, table: string): unknown[] {
  return JSON.parse(JSON.stringify(database.prepare(`SELECT * FROM ${table} ORDER BY rowid`).all())) as unknown[];
}

function insertPreparedWorkspace(
  database: DatabaseSync,
  id: string,
  state: "unbound" | "bound" | "abandoned",
  chatId: string | null,
  projectId: string,
): void {
  database.prepare(`
    INSERT INTO prepared_workspaces (
      preparation_id, project_id, worktree_id, workspace_identity, binding_state,
      bound_chat_id, canonical_record_json, record_digest,
      created_at, updated_at, materialized_at
    ) VALUES (?, ?, ?, ?, ?, ?, '{}', ?,
      '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z', '2026-08-28T01:00:00Z')
  `).run(id, projectId, `worktree_${id}`, `workspace_${id}`, state, chatId, `digest-${id}`);
}
