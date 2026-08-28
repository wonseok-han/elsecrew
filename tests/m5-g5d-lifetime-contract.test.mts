import assert from "node:assert/strict";
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
  STORAGE_LIFETIME_INDEXES_V2,
  STORAGE_LIFETIME_SCHEMA_VERSION_V1,
  STORAGE_LIFETIME_TABLES_V2,
  STORAGE_STATUS_BRIDGE_METHOD_V1,
  STORAGE_STATUS_BRIDGE_VERSION_V1,
  STORAGE_STATUS_METHOD_V1,
  STORAGE_STATUS_SCHEMA_VERSION_V1,
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

const root = resolve("tests/fixtures/m5/storage-v2");
const manifest = readJson<Manifest>(resolve(root, "manifest.json"));
const statusCases = readJson<StatusCases>(resolve(root, manifest.wire.casesFile));

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
      );
    `);
    const beforeEvent = database.prepare("SELECT * FROM events WHERE event_id = ?").get("evt_before_v2");
    const beforeProject = database.prepare("SELECT * FROM projects WHERE project_id = ?").get("project_before_v2");

    database.exec(readFileSync(resolve(root, manifest.migration.file), "utf8"));

    assert.deepEqual(
      database.prepare("SELECT * FROM events WHERE event_id = ?").get("evt_before_v2"),
      beforeEvent,
    );
    assert.deepEqual(
      database.prepare("SELECT * FROM projects WHERE project_id = ?").get("project_before_v2"),
      beforeProject,
    );
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
  assert.equal(JSON.stringify(manifest).includes("warningThreshold"), false);
  assert.equal(JSON.stringify(manifest).includes("hardThreshold"), false);
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
