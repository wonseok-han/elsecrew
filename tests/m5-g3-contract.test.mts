import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  G3_CAPABILITIES_V1,
  M5_DATA_DIRECTORY_V1,
  M5_INTERNAL_CHANNELS_V1,
  M5_PRODUCTION_SIDECAR_V1,
  parseSystemStatusV1,
  validatePrepareQuitTraceV1,
  validateStartupTraceV1,
  type PrepareQuitTraceV1,
  type StartupTraceV1,
} from "../src/m5/contracts/lifecycle-v1.ts";
import {
  classifyOperationV1,
  LEDGER_FIELDS_V1,
  recoverUncleanShutdownV1,
  replayLedgerV1,
  SQLITE_SCHEMA_VERSION,
  SQLITE_TABLES_V1,
  type LedgerEventV1,
  type OperationDecisionV1,
  type OperationRecordV1,
  type ProjectionStateV1,
  type RecoveryInputV1,
  type RecoveryResultV1,
} from "../src/m5/contracts/storage-v1.ts";

interface StorageManifest {
  schemaVersion: number;
  migration: {
    file: string;
    tables: string[];
    ledgerFields: string[];
    operationIdUnique: boolean;
    requestIdDurableKey: boolean;
    eventsAppendOnly: boolean;
  };
  replayFile: string;
  idempotencyFile: string;
  recoveryFile: string;
}

interface LifecycleCase<T> {
  name: string;
  expectedValid: boolean;
  value: T;
}

interface LifecycleManifest {
  schemaVersion: number;
  channels: Record<string, string>;
  capabilities: string[];
  dataDirectory: Record<string, unknown>;
  productionSidecar: Record<string, unknown>;
  systemStatusCases: LifecycleCase<unknown>[];
  startupCases: LifecycleCase<StartupTraceV1>[];
  prepareQuitCases: LifecycleCase<PrepareQuitTraceV1>[];
}

interface ReplayFixture {
  events: LedgerEventV1[];
  expected_projection: ProjectionStateV1;
}

interface IdempotencyFixture {
  existing: OperationRecordV1[];
  cases: Array<{
    name: string;
    incoming: OperationRecordV1;
    expected: OperationDecisionV1;
  }>;
}

interface RecoveryFixture {
  cases: Array<{
    name: string;
    input: RecoveryInputV1;
    expected: RecoveryResultV1;
  }>;
}

const storageRoot = resolve("tests/fixtures/m5/storage-v1");
const lifecycleRoot = resolve("tests/fixtures/m5/lifecycle-v1");
const storageManifest = readJson<StorageManifest>(resolve(storageRoot, "manifest.json"));
const lifecycleManifest = readJson<LifecycleManifest>(resolve(lifecycleRoot, "manifest.json"));

test("SQLite v1 migration이 7개 테이블과 durable operation ledger를 고정한다", () => {
  assert.equal(storageManifest.schemaVersion, SQLITE_SCHEMA_VERSION);
  assert.deepEqual(storageManifest.migration.tables, [...SQLITE_TABLES_V1]);
  assert.deepEqual(storageManifest.migration.ledgerFields, [...LEDGER_FIELDS_V1]);
  assert.equal(storageManifest.migration.operationIdUnique, true);
  assert.equal(storageManifest.migration.requestIdDurableKey, false);
  assert.equal(storageManifest.migration.eventsAppendOnly, true);

  const sql = readFileSync(resolve(storageRoot, storageManifest.migration.file), "utf8");
  assert.equal(sql.match(/CREATE TABLE /g)?.length, SQLITE_TABLES_V1.length);
  for (const table of SQLITE_TABLES_V1) {
    assert.match(sql, new RegExp(`CREATE TABLE ${table} \\(`));
  }
  const events = tableDefinition(sql, "events");
  for (const field of LEDGER_FIELDS_V1) assert.match(events, new RegExp(`\\b${field}\\b`));
  assert.match(events, /operation_id TEXT NOT NULL UNIQUE/);
  assert.doesNotMatch(events, /request_id TEXT NOT NULL UNIQUE/);
  assert.match(sql, /BEFORE UPDATE ON events/);
  assert.match(sql, /BEFORE DELETE ON events/);
});

test("TypeScript replay와 idempotency가 canonical storage fixture와 일치한다", () => {
  const replay = readJson<ReplayFixture>(resolve(storageRoot, storageManifest.replayFile));
  assert.deepEqual(replayLedgerV1(replay.events), replay.expected_projection);

  const idempotency = readJson<IdempotencyFixture>(
    resolve(storageRoot, storageManifest.idempotencyFile),
  );
  for (const fixture of idempotency.cases) {
    assert.equal(
      classifyOperationV1(idempotency.existing, fixture.incoming),
      fixture.expected,
      fixture.name,
    );
  }
});

test("TypeScript unclean recovery가 canonical storage fixture와 일치한다", () => {
  const recovery = readJson<RecoveryFixture>(
    resolve(storageRoot, storageManifest.recoveryFile),
  );
  for (const fixture of recovery.cases) {
    assert.deepEqual(recoverUncleanShutdownV1(fixture.input), fixture.expected, fixture.name);
  }
});

test("고정 channel, data path, production sidecar와 G3 capability가 fixture와 일치한다", () => {
  assert.equal(lifecycleManifest.schemaVersion, 1);
  assert.deepEqual(lifecycleManifest.channels, M5_INTERNAL_CHANNELS_V1);
  assert.deepEqual(lifecycleManifest.capabilities, [...G3_CAPABILITIES_V1]);
  assert.deepEqual(lifecycleManifest.dataDirectory, M5_DATA_DIRECTORY_V1);
  assert.deepEqual(lifecycleManifest.productionSidecar, M5_PRODUCTION_SIDECAR_V1);
});

test("system.status가 ready/unavailable identity, DB health, recovery blocker를 fail-closed 검증한다", () => {
  for (const fixture of lifecycleManifest.systemStatusCases) {
    if (fixture.expectedValid) {
      parseSystemStatusV1(fixture.value);
    } else {
      assert.throws(() => parseSystemStatusV1(fixture.value), fixture.name);
    }
  }
});

test("startup과 prepareQuit 수명주기가 ready/clean marker 순서와 main single-flight를 지킨다", () => {
  for (const fixture of lifecycleManifest.startupCases) {
    assertValidation(fixture, validateStartupTraceV1);
  }
  for (const fixture of lifecycleManifest.prepareQuitCases) {
    assertValidation(fixture, validatePrepareQuitTraceV1);
  }
});

test("공개 bridge와 G2 protocol 목록을 넓히거나 raw IPC를 노출하지 않는다", () => {
  const bridge = readFileSync(resolve("src/m5/contracts/bridge-v1.ts"), "utf8");
  assert.doesNotMatch(bridge, /ipcRenderer|\binvoke\s*\(/);
  assert.match(bridge, /getSystemStatus\(\): Promise<SystemStatusV1>/);
  assert.match(bridge, /prepareQuit\(\): Promise<PrepareQuitResultV1>/);
  const protocolManifest = readJson<{ contract: { bridgeMethods: string[] } }>(
    resolve("tests/fixtures/m5/protocol-v1/manifest.json"),
  );
  assert.deepEqual(protocolManifest.contract.bridgeMethods, [
    "getSystemStatus",
    "prepareQuit",
    "onSidecarEvent",
  ]);
});

function assertValidation<T>(fixture: LifecycleCase<T>, validate: (value: T) => void): void {
  if (fixture.expectedValid) validate(fixture.value);
  else assert.throws(() => validate(fixture.value), fixture.name);
}

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, "utf8")) as T;
}

function tableDefinition(sql: string, table: string): string {
  const start = sql.indexOf(`CREATE TABLE ${table} (`);
  assert.notEqual(start, -1);
  const tail = sql.slice(start);
  const end = tail.indexOf(") STRICT;");
  assert.notEqual(end, -1);
  return tail.slice(0, end);
}
