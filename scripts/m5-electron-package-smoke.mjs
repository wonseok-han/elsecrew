import assert from "node:assert/strict";
import { access } from "node:fs/promises";
import { dirname, resolve } from "node:path";
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

await access(executable);
await access(asar);

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

console.log(`PASS M5 Electron package smoke: ${process.platform}-${process.arch}`);
