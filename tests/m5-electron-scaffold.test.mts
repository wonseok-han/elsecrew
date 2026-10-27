import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(".");

function source(path: string) {
  return readFileSync(resolve(root, path), "utf8");
}

test("패키지 smoke는 플랫폼별 native/product capability 순서를 exact 검증한다", () => {
  const smoke = source("scripts/m5-electron-package-smoke.mjs");

  assert.match(smoke, /tests\/fixtures\/m5\/storage-status-d1\/manifest\.json/);
  assert.match(smoke, /await import\("\.\.\/src\/m5\/contracts\/agent-adapter-v1\.ts"\)/);
  assert.match(smoke, /const expectedProductCapabilities = process\.platform === "darwin"\s*\? G5D_NATIVE_CAPABILITIES_V1\s*: storageStatusD1Manifest\.modes\?\.generalProduct\?\.d1Capabilities/);
  assert.match(smoke, /assert\.deepEqual\(ready\.payload\.capabilities, expectedCurrentCapabilities\)/);
  assert.match(smoke, /"chat\.appendMessage", "agent\.outputHead", "agent\.outputPage", "provider\.login", "run\.approval", "project\.catalog", "provider\.install"\]/);
  assert.match(smoke, /expectedProductCapabilities\.at\(-1\) === "storage\.status"/);
  assert.match(smoke, /assert\.equal\(status\.result\.state, "shellReady"\)/);
  assert.match(smoke, /await waitForStorageReady\(/);
  assert.match(smoke, /admissionState === "ready"/);
  assert.match(smoke, /assert\.equal\(admissionState, "verifying"/);
  assert.match(smoke, /validateProductSystemStatus\(response\.result, readyPayload\)/);
  assert.match(smoke, /Object\.keys\(value\)\.sort\(\)/);
  assert.match(smoke, /requireTimestamp\(admission\.startedAt, "startedAt"\)/);
  assert.match(smoke, /packaged product storage admission timed out/);
  assert.doesNotMatch(smoke, /tests\/fixtures\/m5\/g5-v1\/manifest\.json/);
  assert.doesNotMatch(smoke, /modes\?\.agentFixture/);
  assert.match(smoke, /editor\\\.worker-.+\\\.js/);
  assert.doesNotMatch(smoke, /ready\.payload\.capabilities, \["system\.status", "app\.prepareQuit"\]\)/);
});

test("패키지 smoke의 D1 status validator는 injected malformed product 상태를 fail-closed 한다", () => {
  const output = execFileSync(
    process.execPath,
    ["scripts/m5-electron-package-smoke.mjs"],
    {
      cwd: root,
      env: { ...process.env, M5_PACKAGE_SMOKE_STATUS_VALIDATOR_TEST: "1" },
      encoding: "utf8",
    },
  );
  assert.match(output, /PASS G5D product status validator regression/);
});

test("M5 Electron 의존성과 명령은 Forge 8 alpha exact pin 및 실제 명령으로 고정한다", () => {
  const manifest = JSON.parse(source("package.json"));
  const dependencies = manifest.devDependencies;

  assert.deepEqual(
    Object.fromEntries(
      [
        "electron",
        "@electron-forge/cli",
        "@electron-forge/maker-zip",
        "@electron-forge/plugin-fuses",
        "@electron-forge/plugin-vite",
        "@electron/fuses",
        "@electron/asar",
        "vite",
        "@vitejs/plugin-react",
      ].map((name) => [name, dependencies[name]]),
    ),
    {
      electron: "43.4.0",
      "@electron-forge/cli": "8.0.0-alpha.10",
      "@electron-forge/maker-zip": "8.0.0-alpha.10",
      "@electron-forge/plugin-fuses": "8.0.0-alpha.10",
      "@electron-forge/plugin-vite": "8.0.0-alpha.10",
      "@electron/fuses": "2.1.3",
      "@electron/asar": "4.3.0",
      vite: "8.2.2",
      "@vitejs/plugin-react": "6.1.0",
    },
  );
  assert.equal(manifest.config.forge, "./electron/forge.config.ts");
  assert.equal(manifest.type, "module");
  assert.equal(manifest.main, ".vite/build/main.cjs");
  assert.match(source("electron/vite.main.config.ts"), /formats: \["cjs"\]/);
  assert.match(source("electron/vite.main.config.ts"), /`\$\{entryName\}\.cjs`/);
  assert.match(source("electron/vite.preload.config.ts"), /entryFileNames: "\[name\]\.cjs"/);
  assert.match(source("electron/main.ts"), /preload: join\(__dirname, "preload\.cjs"\)/);
  assert.equal(manifest.scripts["m5:test:core"], "cargo test --workspace --manifest-path Cargo.toml --locked");
  assert.equal(manifest.scripts["m5:package"], "electron-forge package");
  assert.equal(manifest.scripts["m5:package:smoke"], "pnpm run m5:package && node scripts/m5-electron-package-smoke.mjs");
  assert.equal(
    manifest.scripts["m5:test:security"],
    "node --test --experimental-strip-types tests/m5-electron-security.test.mts tests/m5-renderer-csp.test.mts tests/m5-electron-scaffold.test.mts tests/m5-sidecar-staging.test.mts && node --experimental-strip-types --check electron/main.ts",
  );
  assert.equal(
    manifest.scripts["m5:test:package"],
    "node --test --experimental-strip-types tests/m5-g6-package-gate.test.mts tests/m5-package-metrics.test.mts tests/m5-sidecar-staging.test.mts && node scripts/m5-g6-package-gate.mjs",
  );
  assert.equal(
    manifest.scripts["m5:test:electron"],
    "node --test --experimental-strip-types tests/m5-electron*.test.mts && pnpm run m5:package:smoke",
  );
  assert.match(source("electron/preload.ts"), /onAgentPublicEvent: \(listener\) => subscribe\(agentPublicEventListeners, listener\)/);
  assert.match(source("electron/preload.ts"), /ipcRenderer\.on\(M5_BRIDGE_V4_CHANNELS\.agentPublicEvent/);
  assert.match(source("electron/preload.ts"), /onAgentRunStateChanged: \(listener\) => subscribe\(agentRunStateChangedListeners, listener\)/);
  assert.match(source("electron/preload.ts"), /ipcRenderer\.on\(M5_BRIDGE_V5_CHANNELS\.agentRunStateChanged/);
  assert.match(source("electron/preload.ts"), /validateAgentRunStateChangedV1\(payload\.event\)/);
  assert.match(source("electron/preload.ts"), /validateAgentPublicEventV1\(payload\.event\)/);
  assert.match(source("electron/preload.ts"), /const bridge: M5BridgeV8 = Object\.freeze\(\{/);
  assert.match(source("electron/preload.ts"), /version: 8/);
  assert.match(source("electron/preload.ts"), /getStorageStatus: \(input\) => ipcRenderer\.invoke\(M5_BRIDGE_V6_CHANNELS\.storageGetStatus, validateStorageStatusRequestV1\(input\)\)/);
  assert.match(source("electron/main.ts"), /ipcMain\.handle\(M5_BRIDGE_V6_CHANNELS\.storageGetStatus/);
  assert.match(source("pnpm-workspace.yaml"), /^nodeLinker: hoisted$/m);
});

test("M5 main은 app:// origin과 BrowserWindow 보안 경계를 고정한다", () => {
  const main = source("electron/main.ts");

  for (const setting of [
    "nodeIntegration: false",
    "contextIsolation: true",
    "sandbox: true",
    "webSecurity: true",
    "scheme: APP_SCHEME",
    "standard: true",
    "secure: true",
    'setWindowOpenHandler(() => ({ action: "deny" }))',
    '"will-attach-webview"',
    '"will-navigate"',
    "protocol.handle(APP_SCHEME, serveAppAsset)",
    "await window.loadURL(`${APP_ORIGIN}/index.html`)",
  ]) {
    assert.ok(main.includes(setting), `missing security setting: ${setting}`);
  }
});

test("preload, renderer, Forge 설정은 좁은 bridge와 로컬 자산만 허용한다", () => {
  const preload = source("electron/preload.ts");
  const html = source("src/m5/index.html");
  const forge = source("electron/forge.config.ts");
  const rendererVite = source("electron/vite.renderer.config.ts");

  assert.match(preload, /contextBridge\.exposeInMainWorld\("m5", bridge\)/);
  assert.match(preload, /import \{ contextBridge, ipcRenderer \} from "electron"/);
  assert.doesNotMatch(preload, /exposeInMainWorld\([^\n]+ipcRenderer/);
  assert.doesNotMatch(preload, /\bipcRenderer\.(sendSync|sendTo|postMessage)\s*\(/);
  const sends = [...preload.matchAll(/\bipcRenderer\.send\(([^;]+)\);/g)];
  assert.equal(sends.length, 1, "only the bounded event acknowledgement may use ipcRenderer.send");
  assert.equal(
    sends[0]?.[1],
    "M5_BRIDGE_V2_CHANNELS.eventAck, { streamId: event.streamId, seq: event.seq }",
  );
  assert.doesNotMatch(preload, /(?:^|[,{]\s*)(send|sendSync|sendTo|postMessage)\s*:/m);
  assert.doesNotMatch(html, /https?:\/\/|unsafe-eval|<script(?![^>]+src="\/bootstrap\.tsx")/);
  for (const directive of ["default-src 'self'", "script-src 'self'", "connect-src 'self'", "object-src 'none'"]) {
    assert.ok(html.includes(directive), `missing CSP directive: ${directive}`);
  }
  for (const fuse of [
    "asar: true",
    "strictlyRequireAllFuses: true",
    "FuseV1Options.RunAsNode]: false",
    "FuseV1Options.EnableCookieEncryption]: false",
    "FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false",
    "FuseV1Options.EnableNodeCliInspectArguments]: false",
    "FuseV1Options.OnlyLoadAppFromAsar]: true",
    "FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true",
    "FuseV1Options.LoadBrowserProcessSpecificV8Snapshot]: false",
    "FuseV1Options.GrantFileProtocolExtraPrivileges]: false",
    "FuseV1Options.WasmTrapHandlers]: true",
  ]) {
    assert.ok(forge.includes(fuse), `missing Forge security setting: ${fuse}`);
  }
  assert.match(rendererVite, /outDir: outputDirectory/);
});
