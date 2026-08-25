import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(".");

function source(path: string) {
  return readFileSync(resolve(root, path), "utf8");
}

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
  assert.equal(manifest.main, ".vite/build/main.js");
  assert.equal(manifest.scripts["m5:test:core"], "cargo test --workspace --manifest-path Cargo.toml");
  assert.equal(manifest.scripts["m5:package"], "electron-forge package");
  assert.equal(manifest.scripts["m5:package:smoke"], "pnpm run m5:package && node scripts/m5-electron-package-smoke.mjs");
  assert.equal(
    manifest.scripts["m5:test:electron"],
    "node --test --experimental-strip-types tests/m5-electron*.test.mts && pnpm run m5:package:smoke",
  );
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
  assert.doesNotMatch(preload, /\b(send|sendSync|sendTo|postMessage)\s*\(/);
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
