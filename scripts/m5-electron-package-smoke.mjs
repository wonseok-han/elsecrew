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
const appName = "hello-agent";
const packageDirectory = resolve(root, "out", `${appName}-${process.platform}-${process.arch}`);
const appBundle = resolve(packageDirectory, `${appName}.app`);
const executable = process.platform === "darwin"
  ? resolve(appBundle, "Contents", "MacOS", appName)
  : resolve(packageDirectory, process.platform === "win32" ? `${appName}.exe` : appName);
const asar = process.platform === "darwin"
  ? resolve(appBundle, "Contents", "Resources", "app.asar")
  : resolve(packageDirectory, "resources", "app.asar");
const sidecar = process.platform === "darwin"
  ? resolve(appBundle, "Contents", "Resources", "sidecar", "hello-agent-sidecar")
  : resolve(packageDirectory, "resources", "sidecar", `hello-agent-sidecar${process.platform === "win32" ? ".exe" : ""}`);
const g5Manifest = JSON.parse(await readFile(resolve(root, "tests/fixtures/m5/g5-v1/manifest.json"), "utf8"));
const expectedG5Capabilities = g5Manifest.contract?.capabilities;
assert.ok(
  Array.isArray(expectedG5Capabilities) && expectedG5Capabilities.length > 0,
  "canonical G5 fixture must expose an ordered capability contract",
);

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
for (const file of ["/.vite/build/main.js", "/.vite/build/preload.js", "/.vite/renderer/main_window/index.html"]) {
  assert.ok(files.includes(file), `missing packaged asset: ${file}`);
}
assert.ok(files.some((file) => /^\/\.vite\/renderer\/main_window\/assets\/.+\.js$/.test(file)), "missing packaged renderer JavaScript");
assert.ok(files.some((file) => /^\/\.vite\/renderer\/main_window\/assets\/.+\.css$/.test(file)), "missing packaged renderer CSS");
assert.ok(!files.some((file) => file.includes("hello-agent-sidecar")), "sidecar must remain outside app.asar");

await smokePackagedSidecar(sidecar);

console.log(`PASS M5 Electron package smoke: ${process.platform}-${process.arch}`);

async function smokePackagedSidecar(binary) {
  const dataDirectory = await mkdtemp(resolve(tmpdir(), "hello-agent-m5-packaged-sidecar-"));
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
    assert.deepEqual(ready.payload.capabilities, expectedG5Capabilities);
    assert.equal(ready.payload.sidecarInstanceId, ready.sidecarInstanceId);

    const status = await request(lines, child, ready.sidecarInstanceId, "package_status", "system.status");
    assert.equal(status.ok, true);
    assert.equal(status.result.state, "ready");
    assert.deepEqual(status.result.sidecar, ready.payload);
    assert.deepEqual(status.result.database, { health: "healthy", schemaVersion: 1 });
    assert.equal(status.result.recoveryRequired, false);
    assert.deepEqual(status.result.blockerIds, []);

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

async function request(lines, child, sidecarInstanceId, requestId, method) {
  child.stdin.write(`${JSON.stringify({
    protocolVersion: 1,
    kind: "request",
    sidecarInstanceId,
    requestId,
    method,
    params: {},
  })}\n`);
  const response = JSON.parse(await nextProtocolLine(lines, `${method} response`));
  assert.equal(response.protocolVersion, 1);
  assert.equal(response.kind, "response");
  assert.equal(response.sidecarInstanceId, sidecarInstanceId);
  assert.equal(response.requestId, requestId);
  return response;
}

function assertCleanMarker(dataDirectory, sidecarInstanceId) {
  const database = new DatabaseSync(resolve(dataDirectory, "hello-agent.sqlite3"), { readOnly: true });
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

async function nextProtocolLine(lines, label) {
  const result = await withDeadline(lines.next(), 3_000, label);
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
