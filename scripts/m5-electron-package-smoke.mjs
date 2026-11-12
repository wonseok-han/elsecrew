import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { createInterface } from "node:readline";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { listPackage } from "@electron/asar";
import { FuseState, FuseV1Options, FuseVersion, getCurrentFuseWire } from "@electron/fuses";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appName = "Elsecrew";
const packageDirectory = resolve(root, "out", `${appName}-${process.platform}-${process.arch}`);
const appBundle = resolve(packageDirectory, `${appName}.app`);
const executable = process.platform === "darwin"
  ? resolve(appBundle, "Contents", "MacOS", appName)
  : resolve(packageDirectory, process.platform === "win32" ? `${appName}.exe` : appName);
const asar = process.platform === "darwin"
  ? resolve(appBundle, "Contents", "Resources", "app.asar")
  : resolve(packageDirectory, "resources", "app.asar");
const sidecar = process.platform === "darwin"
  ? resolve(appBundle, "Contents", "Resources", "sidecar", "elsecrew-sidecar")
  : resolve(packageDirectory, "resources", "sidecar", `elsecrew-sidecar${process.platform === "win32" ? ".exe" : ""}`);
const storageStatusD1Manifest = JSON.parse(await readFile(resolve(root, "tests/fixtures/m5/storage-status-d1/manifest.json"), "utf8"));
const { G5D_NATIVE_CAPABILITIES_V1 } = await import("../src/m5/contracts/agent-adapter-v1.ts");
const expectedProductCapabilities = process.platform === "darwin"
  ? G5D_NATIVE_CAPABILITIES_V1
  : storageStatusD1Manifest.modes?.generalProduct?.d1Capabilities;
const expectedCurrentCapabilities = [...expectedProductCapabilities,
  "chat.appendMessage", "agent.outputHead", "agent.outputPage", "provider.login", "run.approval", "project.catalog", "provider.install"];
const g5dAdmissionManifest = JSON.parse(await readFile(resolve(root, "tests/fixtures/m5/g5d-admission-v1/manifest.json"), "utf8"));
const verifyingStatusFixture = g5dAdmissionManifest.systemStatusCases.find(({ value }) => value.storageAdmission?.state === "verifying")?.value;
const failedStatusFixture = g5dAdmissionManifest.systemStatusCases.find(({ value }) => value.storageAdmission?.state === "failed")?.value;
assert.ok(verifyingStatusFixture && failedStatusFixture, "canonical G5D fixture must expose verifying and failed status shapes");
const expectedStatusFields = ["sidecar", ...Object.keys(verifyingStatusFixture)];
const expectedAdmissionFields = Object.keys(verifyingStatusFixture.storageAdmission);
const expectedDatabaseFields = Object.keys(verifyingStatusFixture.database);
const expectedErrorFields = Object.keys(failedStatusFixture.storageAdmission.error);
const expectedReadyPayloadFields = Object.keys(g5dAdmissionManifest.readyPayloads.generalProduct);
assert.ok(
  Array.isArray(expectedProductCapabilities)
    && expectedProductCapabilities.length > 0
    && expectedProductCapabilities.at(-1) === "storage.status",
  "canonical D1 product fixture must expose an ordered capability contract",
);

if (process.env.M5_PACKAGE_SMOKE_STATUS_VALIDATOR_TEST === "1") {
  runStatusValidatorRegression();
  console.log("PASS G5D product status validator regression");
  process.exit(0);
}

await access(executable);
await access(asar);
await access(sidecar);

const fuses = await getCurrentFuseWire(executable);
assert.equal(fuses.version, FuseVersion.V1, "packaged executable must expose a V1 fuse wire");
for (const [option, expected] of [
  [FuseV1Options.RunAsNode, FuseState.DISABLE],
  [FuseV1Options.EnableCookieEncryption, FuseState.DISABLE],
  [FuseV1Options.EnableNodeOptionsEnvironmentVariable, FuseState.DISABLE],
  [FuseV1Options.EnableNodeCliInspectArguments, FuseState.DISABLE],
  [FuseV1Options.EnableEmbeddedAsarIntegrityValidation, FuseState.ENABLE],
  [FuseV1Options.OnlyLoadAppFromAsar, FuseState.ENABLE],
  [FuseV1Options.LoadBrowserProcessSpecificV8Snapshot, FuseState.DISABLE],
  [FuseV1Options.GrantFileProtocolExtraPrivileges, FuseState.DISABLE],
  [FuseV1Options.WasmTrapHandlers, FuseState.ENABLE],
]) {
  assert.equal(fuses[option], expected, `unexpected fuse state for ${FuseV1Options[option]}`);
}

const files = listPackage(asar, { isPack: false });
for (const file of ["/.vite/build/main.cjs", "/.vite/build/preload.cjs", "/.vite/renderer/main_window/index.html"]) {
  assert.ok(files.includes(file), `missing packaged asset: ${file}`);
}
assert.ok(files.some((file) => /^\/\.vite\/renderer\/main_window\/assets\/.+\.js$/.test(file)), "missing packaged renderer JavaScript");
assert.ok(files.some((file) => /^\/\.vite\/renderer\/main_window\/assets\/.+\.css$/.test(file)), "missing packaged renderer CSS");
assert.ok(files.some((file) => /^\/\.vite\/renderer\/main_window\/assets\/editor\.worker-.+\.js$/.test(file)), "missing packaged Monaco editor worker");
assert.ok(!files.some((file) => file.includes("elsecrew-sidecar")), "sidecar must remain outside app.asar");

await smokePackagedSidecar(sidecar);

console.log(`PASS M5 Electron package smoke: ${process.platform}-${process.arch}`);

async function smokePackagedSidecar(binary) {
  const dataDirectory = await mkdtemp(resolve(tmpdir(), "elsecrew-m5-packaged-sidecar-"));
  const child = spawn(binary, ["--stdio"], {
    cwd: dataDirectory,
    env: process.env,
    shell: false,
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => {
    stderr = `${stderr}${chunk}`.slice(-32 * 1024);
  });
  const lines = createInterface({ input: child.stdout, crlfDelay: Infinity })[Symbol.asyncIterator]();
  let primaryError;

  try {
    const ready = JSON.parse(await nextProtocolLine(lines, "system.ready"));
    assert.equal(ready.protocolVersion, 1);
    assert.equal(ready.kind, "event");
    assert.equal(typeof ready.sidecarInstanceId, "string");
    assert.ok(ready.sidecarInstanceId.length > 0);
    assert.equal(ready.streamId, "system");
    assert.equal(ready.event, "system.ready");
    assert.equal(ready.seq, 1);
    assert.equal(ready.payload.protocolVersion, 1);
    assert.equal(ready.payload.sidecarSchemaVersion, 1);
    assert.equal(ready.payload.appSchemaVersion, 1);
    assert.ok(Number.isSafeInteger(ready.payload.pid) && ready.payload.pid > 0);
    assert.equal(typeof ready.payload.platform, "string");
    assert.ok(ready.payload.platform.length > 0);
    assert.deepEqual(ready.payload.capabilities, expectedCurrentCapabilities);
    assert.equal(ready.payload.sidecarInstanceId, ready.sidecarInstanceId);

    const status = await request(lines, child, ready.sidecarInstanceId, "package_status_initial", "system.status");
    assert.equal(status.ok, true);
    assert.equal(status.result.state, "shellReady");
    assert.deepEqual(status.result.sidecar, ready.payload);
    const readyStatus = await waitForStorageReady(
      lines,
      child,
      ready.sidecarInstanceId,
      ready.payload,
      status,
    );
    assert.equal(readyStatus.ok, true);
    assert.equal(readyStatus.result.state, "shellReady");
    assert.equal(readyStatus.result.storageAdmission.state, "ready");
    assert.deepEqual(readyStatus.result.sidecar, ready.payload);
    assert.deepEqual(readyStatus.result.database, { health: "healthy", schemaVersion: 1 });
    assert.equal(readyStatus.result.recoveryRequired, false);
    assert.deepEqual(readyStatus.result.blockerIds, []);

    const quit = await request(lines, child, ready.sidecarInstanceId, "package_quit", "app.prepareQuit");
    assert.equal(quit.ok, true);
    assert.deepEqual(quit.result, { blockers: [], safeToQuit: true });

    child.stdin.end();
    const [code, signal] = await withDeadline(once(child, "exit"), 3_000, "clean sidecar exit");
    assert.equal(code, 0, `packaged sidecar stderr: ${stderr}`);
    assert.equal(signal, null);
    assertCleanMarker(dataDirectory, ready.sidecarInstanceId);
  } catch (error) {
    primaryError = error;
    throw error;
  } finally {
    try {
      await stopChild(child);
      await rm(dataDirectory, { recursive: true, force: true });
    } catch (cleanupError) {
      if (!primaryError) throw cleanupError;
      process.stderr.write(`packaged sidecar cleanup also failed: ${String(cleanupError)}\n`);
    }
  }
}

function validateProductSystemStatus(value, readyPayload) {
  const status = exactObject(value, expectedStatusFields, "G5D system status");
  assert.equal(status.schemaVersion, 1, "G5D status schemaVersion must be 1");
  assert.equal(status.state, "shellReady", "G5D status must be shellReady");
  assert.deepEqual(Object.keys(readyPayload).sort(), [...expectedReadyPayloadFields].sort(), "system.ready payload fields must be exact");
  assert.deepEqual(readyPayload.capabilities, expectedCurrentCapabilities, "packaged capabilities must match the current product contract");
  assert.deepEqual(status.sidecar, readyPayload, "G5D status sidecar must match system.ready exactly");

  const admission = exactObject(status.storageAdmission, expectedAdmissionFields, "storageAdmission");
  assert.equal(admission.schemaVersion, 1, "storageAdmission schemaVersion must be 1");
  requireTimestamp(admission.startedAt, "startedAt");
  const database = exactObject(status.database, expectedDatabaseFields, "database");
  assert.ok(Array.isArray(status.blockerIds), "blockerIds must be an array");

  if (admission.state === "verifying") {
    assert.equal(admission.completedAt, null, "verifying storage cannot have completedAt");
    assert.equal(admission.error, null, "verifying storage cannot have an error");
    assert.deepEqual(database, { health: "verifying", schemaVersion: null });
    assert.equal(status.recoveryRequired, null);
    assert.deepEqual(status.blockerIds, []);
  } else if (admission.state === "ready") {
    validateCompletedAt(admission);
    assert.equal(admission.error, null, "ready storage cannot have an error");
    assert.deepEqual(database, { health: "healthy", schemaVersion: 1 });
    assert.equal(typeof status.recoveryRequired, "boolean");
    validateBlockers(status.blockerIds, status.recoveryRequired);
  } else if (admission.state === "failed") {
    validateCompletedAt(admission);
    const error = exactObject(admission.error, expectedErrorFields, "storage admission error");
    assert.deepEqual(error, {
      code: "storageAdmissionFailed",
      kind: "storageUnavailable",
      detail: "storage admission failed",
      retryable: false,
      outcomeUnknown: true,
    });
    assert.equal(database.health, "unavailable");
    assert.ok(database.schemaVersion === null || database.schemaVersion === 1);
    assert.equal(status.recoveryRequired, null);
    assert.deepEqual(status.blockerIds, []);
  } else {
    assert.fail(`unknown storage admission state: ${String(admission.state)}`);
  }
  return status;
}

function exactObject(value, fields, name) {
  assert.equal(typeof value, "object", `${name} must be an object`);
  assert.ok(value !== null && !Array.isArray(value), `${name} must be an object`);
  assert.deepEqual(Object.keys(value).sort(), [...fields].sort(), `${name} must contain the exact contract fields`);
  return value;
}

function requireTimestamp(value, name) {
  const match = typeof value === "string"
    ? /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})Z$/.exec(value)
    : null;
  assert.ok(match, `${name} must be an RFC 3339 UTC second timestamp`);
  const [year, month, day, hour, minute, second] = match.slice(1).map(Number);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [0, 31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  assert.ok(
    year >= 1 && month >= 1 && month <= 12 && day >= 1 && day <= days[month]
      && hour <= 23 && minute <= 59 && second <= 59,
    `${name} must be a real RFC 3339 UTC calendar timestamp`,
  );
}

function validateCompletedAt(admission) {
  requireTimestamp(admission.completedAt, "completedAt");
  assert.ok(admission.completedAt >= admission.startedAt, "completedAt cannot precede startedAt");
}

function validateBlockers(value, recoveryRequired) {
  assert.ok(value.every((item) => typeof item === "string" && item.length > 0), "blockerIds cannot contain an empty identity");
  assert.equal(new Set(value).size, value.length, "blockerIds must be unique");
  assert.equal(recoveryRequired, value.length > 0, "recoveryRequired must match blockerIds");
}

function runStatusValidatorRegression() {
  const readyPayload = { ...structuredClone(g5dAdmissionManifest.readyPayloads.generalProduct), capabilities: [...expectedCurrentCapabilities] };
  const makeStatus = (fixture) => ({ ...structuredClone(fixture), sidecar: structuredClone(readyPayload) });
  const verifying = makeStatus(verifyingStatusFixture);
  const failed = makeStatus(failedStatusFixture);
  assert.doesNotThrow(() => validateProductSystemStatus(verifying, readyPayload));
  assert.doesNotThrow(() => validateProductSystemStatus(failed, readyPayload));
  for (const capabilities of [
    expectedCurrentCapabilities.slice(0, -1),
    [...expectedCurrentCapabilities, "provider.install"],
    [...expectedCurrentCapabilities].reverse(),
  ]) {
    const staleReady = { ...readyPayload, capabilities };
    assert.throws(() => validateProductSystemStatus({ ...verifying, sidecar: staleReady }, staleReady), undefined,
      "missing, duplicated and reordered capabilities must be refused even when status matches ready");
  }

  const malformed = [
    ["missing schemaVersion", (status) => { delete status.storageAdmission.schemaVersion; }],
    ["extra field", (status) => { status.database.extra = true; }],
    ["invalid schemaVersion", (status) => { status.schemaVersion = 2; }],
    ["invalid timestamp", (status) => { status.storageAdmission.startedAt = "2026-02-30T04:00:00Z"; }],
    ["stale ready error", (status) => { status.storageAdmission = { ...status.storageAdmission, state: "ready", completedAt: "2026-09-08T04:01:00Z", error: { code: "storageAdmissionFailed", kind: "storageUnavailable", detail: "storage admission failed", retryable: false, outcomeUnknown: true } }; status.database = { health: "healthy", schemaVersion: 1 }; status.recoveryRequired = false; }],
  ];
  for (const [name, mutate] of malformed) {
    const candidate = structuredClone(verifying);
    mutate(candidate);
    assert.throws(() => validateProductSystemStatus(candidate, readyPayload), undefined, name);
  }
}

async function waitForStorageReady(lines, child, sidecarInstanceId, readyPayload, initialResponse) {
  const deadline = Date.now() + 3_000;
  let response = initialResponse;
  let attempt = 0;
  for (;;) {
    assert.equal(response.ok, true);
    const status = validateProductSystemStatus(response.result, readyPayload);
    const admissionState = status.storageAdmission.state;
    if (admissionState === "ready") return response;
    assert.equal(admissionState, "verifying", "packaged product admission must fail closed unless verifying or ready");
    const remaining = deadline - Date.now();
    assert.ok(remaining > 0, "packaged product storage admission timed out");
    await new Promise((resolve) => setTimeout(resolve, Math.min(10, remaining)));
    response = await request(
      lines,
      child,
      sidecarInstanceId,
      `package_status_${++attempt}`,
      "system.status",
      Math.min(500, Math.max(1, deadline - Date.now())),
    );
  }
}

async function request(lines, child, sidecarInstanceId, requestId, method, timeoutMs = 3_000) {
  child.stdin.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "request",
    sidecarInstanceId,
    requestId,
    method,
    params: {},
  })}\n`);
  const response = JSON.parse(await nextProtocolLine(lines, `${method} response`, timeoutMs));
  assert.equal(response.protocolVersion, 1);
  assert.equal(response.kind, "response");
  assert.equal(response.sidecarInstanceId, sidecarInstanceId);
  assert.equal(response.requestId, requestId);
  return response;
}

function assertCleanMarker(dataDirectory, sidecarInstanceId) {
  const database = new DatabaseSync(resolve(dataDirectory, "elsecrew.sqlite3"), { readOnly: true });
  try {
    const marker = database.prepare(
      "SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id = ?",
    ).get(sidecarInstanceId);
    assert.equal(typeof marker?.clean_shutdown_at, "string");
    assert.ok(marker.clean_shutdown_at.length > 0);
  } finally {
    database.close();
  }
}

async function stopChild(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = once(child, "exit");
  if (child.stdin.writable) child.stdin.end();
  child.kill();
  await withDeadline(exited, 3_000, "failed packaged sidecar cleanup");
}

async function nextProtocolLine(lines, label, timeoutMs = 3_000) {
  const result = await withDeadline(lines.next(), timeoutMs, label);
  assert.equal(result.done, false, `packaged sidecar ended before ${label}`);
  return result.value;
}

async function withDeadline(promise, timeoutMs, label) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`timed out waiting for ${label}`)), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
