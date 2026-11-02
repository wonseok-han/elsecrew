import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createContext, runInContext } from "node:vm";
import test from "node:test";

import { M5_INTERNAL_CHANNELS_V1 } from "../src/m5/contracts/lifecycle-v1.ts";
import * as modelChoicesContract from "../src/m5/contracts/model-choices-v1.ts";
import * as deviceReportContract from "../src/m5/contracts/device-report-v1.ts";
import * as sessionDiagnosticsContract from "../src/m5/contracts/session-diagnostics-v1.ts";
import * as loginDiagnostic from "../src/m5/provider-login-diagnostic.ts";
import { M5_BRIDGE_V2_CHANNELS, M5_BRIDGE_V3_CHANNELS, M5_BRIDGE_V4_CHANNELS, M5_BRIDGE_V5_CHANNELS, M5_BRIDGE_V6_CHANNELS, M5_BRIDGE_V7_CHANNELS, M5_BRIDGE_V8_CHANNELS } from "../electron/ipc-channels.ts";

const require = createRequire(import.meta.url);
const typescript = require("typescript") as typeof import("typescript");
const root = resolve(".");

test("renderer status during load starts the sidecar before returning admission", async () => {
  const harness = await loadMain(undefined, true);
  assert.equal((harness.window.initialStatus as {state: string}).state, "ready");
  assert.deepEqual(harness.sidecarCalls, ["getSystemStatus"]);
});

test("login opens a validated browser URL only in main and never on cancel", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(loginContract.LOGIN_CHANNEL);
  assert.ok(handler);
  const response = await handler(harness.currentEvent(), { action: "start", attemptId: "test" }) as {authUrl: unknown};
  assert.equal(response.authUrl, null);
  assert.deepEqual(harness.sidecarCalls, ["login", "openExternal"]);
  await handler(harness.currentEvent(), { action: "cancel", attemptId: "test" });
  assert.deepEqual(harness.sidecarCalls, ["login", "openExternal", "login"]);
  await assertRejectedWithoutSidecar(handler, {sender: {}, senderFrame: {}}, harness);
});

test("approval IPC rejects foreign frames and cannot grant acceptance", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(approvalContract.APPROVAL_CHANNEL);
  assert.ok(handler);
  await assertRejectedWithoutSidecar(handler, {sender:{},senderFrame:{}}, harness);
  const identity = {chatId:"chat_01",taskId:"task_01",projectId:"project_01",worktreeId:"worktree_01",workspaceIdentity:"workspace_01",runId:"agent_run_01"};
  await assert.rejects(handler(harness.currentEvent(),{action:"accept",identity}));
  assert.deepEqual(harness.sidecarCalls,[]);
});

test("main-process IPC accepts only the current main frame at the exact allowed origin", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(M5_BRIDGE_V2_CHANNELS.projectGetTree);
  assert.ok(handler, "project tree handler must be registered");

  const valid = harness.currentEvent();
  assert.deepEqual(await handler(valid, { projectId: "project_1", parentPath: "", cursor: null }), { tree: true });
  assert.deepEqual(harness.sidecarCalls, ["getProjectTree"]);

  for (const event of [
    { sender: {}, senderFrame: {} },
    { sender: harness.window.webContents, senderFrame: { url: harness.window.webContents.mainFrame.url } },
  ]) {
    await assertRejectedWithoutSidecar(handler, event, harness);
  }

  harness.window.destroyed = true;
  await assertRejectedWithoutSidecar(handler, harness.currentEvent(), harness);
  harness.window.destroyed = false;

  for (const spoofedOrigin of [
    "app://user@m5/index.html",
    "app://m5:444/index.html",
    "app://m5.evil/index.html",
    "https://m5/index.html",
  ]) {
    harness.window.webContents.mainFrame.url = spoofedOrigin;
    await assertRejectedWithoutSidecar(handler, harness.currentEvent(), harness);
  }
});

test("output head/page IPC rejects foreign frames before reaching sidecar", async () => {
  const harness = await loadMain();
  for (const channel of [M5_BRIDGE_V8_CHANNELS.agentOutputHead, M5_BRIDGE_V8_CHANNELS.agentOutputPage]) {
    const handler = harness.handlers.get(channel);
    assert.ok(handler);
    await assertRejectedWithoutSidecar(handler, { sender: {}, senderFrame: {} }, harness);
    await assertRejectedWithoutSidecar(handler, {
      sender: harness.window.webContents,
      senderFrame: { url: harness.window.webContents.mainFrame.url },
    }, harness);
  }
  assert.deepEqual(harness.sidecarCalls, []);
});

test("dev server IPC permits only its configured origin and rejects credential spoofing", async () => {
  const harness = await loadMain("http://127.0.0.1:5173");
  const handler = harness.handlers.get(M5_INTERNAL_CHANNELS_V1.systemGetStatus);
  assert.ok(handler, "system status handler must be registered");

  assert.equal((await handler(harness.currentEvent())).state, "ready");
  assert.deepEqual(harness.sidecarCalls, ["getSystemStatus"]);

  harness.window.webContents.mainFrame.url = "http://user@127.0.0.1:5173/";
  await assertRejectedWithoutSidecar(handler, harness.currentEvent(), harness);
  harness.window.webContents.mainFrame.url = "http://127.0.0.1:5174/";
  await assertRejectedWithoutSidecar(handler, harness.currentEvent(), harness);
});

test("main window blocks popup, webview attachment, and disallowed navigation at runtime", async () => {
  const harness = await loadMain();
  const webPreferences = (harness.window.options as { webPreferences: Record<string, unknown> }).webPreferences;
  assert.equal(webPreferences.nodeIntegration, false);
  assert.equal(webPreferences.contextIsolation, true);
  assert.equal(webPreferences.sandbox, true);
  assert.equal(webPreferences.webSecurity, true);
  assert.equal(harness.window.openHandler?.().action, "deny");

  const webview = preventedEvent();
  harness.window.webContents.listeners.get("will-attach-webview")?.(webview);
  assert.equal(webview.prevented, true);

  for (const target of ["https://example.test/", "app://user@m5/index.html", "app://m5:444/index.html"]) {
    const navigation = preventedEvent();
    harness.window.webContents.listeners.get("will-navigate")?.(navigation, target);
    assert.equal(navigation.prevented, true, target);
  }
  const allowed = preventedEvent();
  harness.window.webContents.listeners.get("will-navigate")?.(allowed, "app://m5/index.html");
  assert.equal(allowed.prevented, false);
});

test("app protocol rejects external, encoded, absolute, malformed, and missing assets without external fetches", async () => {
  const harness = await loadMain();
  const serve = harness.protocolHandlers.get("app");
  assert.ok(serve, "app protocol handler must be registered");

  const valid = await serve({ url: "app://m5/index.html" });
  assert.equal(valid.status, 200);
  assert.equal(valid.headers.get("Content-Security-Policy"), "frame-ancestors 'none'");
  assert.equal(await valid.text(), "local");
  assert.equal(harness.netFetches.length, 1);
  assert.match(harness.netFetches[0] ?? "", /^file:/);

  for (const url of [
    "https://example.test/index.html",
    "app://other/index.html",
    "app://user@m5/index.html",
    "app://m5:444/index.html",
    "app://m5/%2e%2e/secret.txt",
    "app://m5/%2Fetc%2Fpasswd",
    "app://m5/%5Cwindows%5Cwin.ini",
    "app://m5//etc/passwd",
    "app://m5/C:/Windows/System32",
    "app://m5/%",
    "app://m5/missing.js",
  ]) {
    const before = harness.netFetches.length;
    const response = await serve({ url });
    assert.equal(response.status, 404, url);
    assert.equal(harness.netFetches.length, before, `must not fetch rejected URL: ${url}`);
  }
});

async function assertRejectedWithoutSidecar(
  handler: (...args: unknown[]) => Promise<unknown>,
  event: unknown,
  harness: MainHarness,
) {
  const before = harness.sidecarCalls.length;
  await assert.rejects(handler(event, { projectId: "project_1", parentPath: "", cursor: null }));
  assert.equal(harness.sidecarCalls.length, before, "rejected IPC must not call the sidecar");
}

function preventedEvent() {
  return {
    prevented: false,
    preventDefault() { this.prevented = true; },
  };
}

interface MainHarness {
  handlers: Map<string, (...args: unknown[]) => Promise<unknown>>;
  netFetches: string[];
  protocolHandlers: Map<string, (request: { url: string }) => Promise<Response>>;
  sidecarCalls: string[];
  window: FakeWindow;
  currentEvent(): { sender: FakeWindow["webContents"]; senderFrame: { url: string } };
}

class FakeWindow {
  static windows: FakeWindow[] = [];
  static duringLoad: ((window: FakeWindow) => Promise<unknown>) | undefined;
  initialStatus: unknown;
  destroyed = false;
  openHandler: (() => { action: string }) | undefined;
  readonly options: unknown;
  readonly listeners = new Map<string, (...args: unknown[]) => void>();
  readonly webContents = {
    listeners: new Map<string, (...args: unknown[]) => void>(),
    mainFrame: { url: "" },
    on: (event: string, listener: (...args: unknown[]) => void) => this.webContents.listeners.set(event, listener),
    send: () => undefined,
    setWindowOpenHandler: (handler: () => { action: string }) => { this.openHandler = handler; },
  };

  constructor(options: unknown) {
    this.options = options;
    FakeWindow.windows.push(this);
  }

  on(event: string, listener: (...args: unknown[]) => void) {
    this.listeners.set(event, listener);
  }

  isDestroyed() {
    return this.destroyed;
  }

  async loadURL(url: string) {
    this.webContents.mainFrame.url = url;
    this.initialStatus = await FakeWindow.duringLoad?.(this);
  }
}

async function loadMain(devServerUrl: string | undefined = undefined, requestDuringLoad = false): Promise<MainHarness> {
  FakeWindow.windows = [];
  const handlers = new Map<string, (...args: unknown[]) => Promise<unknown>>();
  FakeWindow.duringLoad = requestDuringLoad ? async (window) => handlers.get(M5_INTERNAL_CHANNELS_V1.systemGetStatus)!({sender: window.webContents, senderFrame: window.webContents.mainFrame}) : undefined;
  const protocolHandlers = new Map<string, (request: { url: string }) => Promise<Response>>();
  const netFetches: string[] = [];
  const sidecarCalls: string[] = [];
  const files = new Set(["/app/.vite/renderer/main_window/index.html"]);
  const electron = {
    app: {
      whenReady: () => Promise.resolve(),
      on: () => undefined,
      getPath: () => "/app-data",
      isPackaged: false,
      quit: () => undefined,
    },
    BrowserWindow: Object.assign(FakeWindow, { getAllWindows: () => FakeWindow.windows }),
    dialog: { showOpenDialog: async () => ({ canceled: true, filePaths: [] }) },
    shell: { openExternal: async () => { sidecarCalls.push("openExternal"); } },
    ipcMain: {
      handle: (channel: string, handler: (...args: unknown[]) => Promise<unknown>) => handlers.set(channel, handler),
      on: () => undefined,
    },
    net: { fetch: async (url: string) => {
      netFetches.push(url);
      return new Response("local", { status: 200 });
    } },
    protocol: {
      registerSchemesAsPrivileged: () => undefined,
      handle: async (scheme: string, handler: (request: { url: string }) => Promise<Response>) => protocolHandlers.set(scheme, handler),
    },
  };
  class FakeSidecar {
    technicalDetail = "";
    lastReady = null;
    onEvent() {}
    onProjectEvent() {}
    onWorkspaceEvent() {}
    onChatEvent() {}
    onAgentPublicEvent() {}
    onAgentRunStateChanged() {}
    onAgentOutputAvailable() {}
    async getAgentOutputHead() { sidecarCalls.push("getAgentOutputHead"); return null; }
    async getAgentOutputPage() { sidecarCalls.push("getAgentOutputPage"); return { rows: [] }; }
    async start() {}
    async login() {
      sidecarCalls.push("login");
      return {state: "waiting", attemptId: "test", authUrl: "https://auth.openai.com/authorize?state=private", error: null};
    }
    async getSystemStatus() {
      sidecarCalls.push("getSystemStatus");
      return { state: "ready" };
    }
    async getProjectTree() {
      sidecarCalls.push("getProjectTree");
      return { tree: true };
    }
    failRendererDisconnected() {}
  }
  class FakeEventCredit {
    outstanding = 0;
    constructor(_failure: unknown) {}
    deliver() {}
    acknowledge() {}
    resetIfEmpty() {}
  }
  class FakeQuitCoordinator {
    exitAllowed = false;
    constructor(_options: unknown) {}
    prepareQuit() { return Promise.resolve({ blockers: [], safeToQuit: true }); }
    handleBeforeQuit() {}
  }
  const module = { exports: {} };
  const context = createContext({
    MAIN_WINDOW_VITE_DEV_SERVER_URL: devServerUrl,
    MAIN_WINDOW_VITE_NAME: "main_window",
    Response,
    URL,
    __dirname: "/app/.vite/build",
    console,
    module,
    exports: module.exports,
    process: { platform: "darwin", resourcesPath: "/resources", execPath: "node" },
    require: (specifier: string) => moduleFor(specifier, { electron, files, FakeSidecar, FakeEventCredit, FakeQuitCoordinator }),
  });
  const source = readFileSync(resolve(root, "electron/main.ts"), "utf8");
  const compiled = typescript.transpileModule(source, {
    compilerOptions: { module: typescript.ModuleKind.CommonJS, target: typescript.ScriptTarget.ES2022 },
    fileName: "main.ts",
  }).outputText;
  runInContext(compiled, context, { filename: "electron/main.ts" });
  for (let index = 0; index < 12; index += 1) await Promise.resolve();
  assert.ok(handlers.size > 0, "main readiness must register IPC handlers");
  const window = FakeWindow.windows[0];
  assert.ok(window, "main readiness must create one BrowserWindow");
  return {
    handlers,
    netFetches,
    protocolHandlers,
    sidecarCalls,
    window,
    currentEvent: () => ({ sender: window.webContents, senderFrame: window.webContents.mainFrame }),
  };
}

function moduleFor(
  specifier: string,
  fakes: {
    electron: Record<string, unknown>;
    files: Set<string>;
    FakeSidecar: new (...args: unknown[]) => unknown;
    FakeEventCredit: new (...args: unknown[]) => unknown;
    FakeQuitCoordinator: new (...args: unknown[]) => unknown;
  },
) {
  if (specifier === "electron") return fakes.electron;
  // Diagnostic persistence has its own filesystem tests. This harness exercises
  // the real main IPC/origin checks without writing outside its fake app path.
  if (specifier === "./session-diagnostic-log.ts") return {
    createSessionDiagnosticLog: () => ({
      capture: () => undefined,
      captureTransport: () => undefined,
      flush: async () => undefined,
    }),
  };
  if (specifier.endsWith("provider-login-v1.ts")) return loginContract;
  if (specifier.endsWith("provider-login-diagnostic.ts")) return loginDiagnostic;
  if (specifier.endsWith("provider-install-v1.ts")) return installContract;
  if (specifier.endsWith("device-report-v1.ts")) return deviceReportContract;
  if (specifier.endsWith("session-diagnostics-v1.ts")) return sessionDiagnosticsContract;
  if (specifier === "./device-report.ts") return { readDeviceReport: () => { throw new Error("unexpected device inspection"); } };
  if (specifier.endsWith("approval-v1.ts")) return approvalContract;
  if (specifier.endsWith("project-catalog-v1.ts")) return catalogContract;
  if (specifier.endsWith("model-choices-v1.ts")) return modelChoicesContract;
  if (specifier === "node:fs") {
    return {
      mkdirSync: () => undefined,
      realpathSync: (path: string) => path,
      statSync: (path: string) => {
        if (!fakes.files.has(path)) throw new Error("missing test asset");
        return { isFile: () => true };
      },
    };
  }
  if (specifier === "node:path" || specifier === "node:url") return require(specifier);
  if (specifier.endsWith("lifecycle-v1.ts")) return { M5_INTERNAL_CHANNELS_V1 };
  if (specifier === "./ipc-channels.ts") {
    return { M5_BRIDGE_V2_CHANNELS, M5_BRIDGE_V3_CHANNELS, M5_BRIDGE_V4_CHANNELS, M5_BRIDGE_V5_CHANNELS, M5_BRIDGE_V6_CHANNELS, M5_BRIDGE_V7_CHANNELS, M5_BRIDGE_V8_CHANNELS };
  }
  if (specifier === "./event-credit.ts") return { EventCreditV1: fakes.FakeEventCredit };
  if (specifier === "./quit-coordinator.ts") return { QuitCoordinatorV1: fakes.FakeQuitCoordinator };
  if (specifier === "./sidecar-client.ts") return { SIDECAR_READY_TIMEOUT_MS: 3_000, SidecarClientV1: fakes.FakeSidecar };
  throw new Error(`unexpected main.ts runtime import: ${specifier}`);
}
import * as loginContract from "../src/m5/contracts/provider-login-v1.ts";
import * as approvalContract from "../src/m5/contracts/approval-v1.ts";
import * as catalogContract from "../src/m5/contracts/project-catalog-v1.ts";
import * as installContract from "../src/m5/contracts/provider-install-v1.ts";

test("catalog IPC rejects foreign frames and renderer-supplied paths", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(catalogContract.PROJECT_CATALOG_CHANNEL);
  assert.ok(handler);
  await assertRejectedWithoutSidecar(handler, {sender: {}, senderFrame: {}}, harness);
  await assert.rejects(handler(harness.currentEvent(), {action: "reopen", projectId: "project_a", path: "/other"}));
  assert.deepEqual(harness.sidecarCalls, []);
});

test("session diagnostics accept only the current main frame and no renderer input", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(sessionDiagnosticsContract.SESSION_DIAGNOSTICS_CHANNEL);
  assert.ok(handler);
  const result = await handler(harness.currentEvent());
  assert.equal(sessionDiagnosticsContract.validateSessionDiagnostics(result).scope,"currentAppSession");
  for(const event of [{sender:{},senderFrame:{}},
    {sender:harness.window.webContents,senderFrame:{url:harness.window.webContents.mainFrame.url}}]) {
    await assertRejectedWithoutSidecar(handler,event,harness);
  }
  await assert.rejects(handler(harness.currentEvent(),{path:"/private/credentials"}));
  harness.window.webContents.mainFrame.url="https://untrusted.invalid";
  await assertRejectedWithoutSidecar(handler,harness.currentEvent(),harness);
  assert.deepEqual(harness.sidecarCalls,[]);
});

test("installation IPC rejects foreign frames, arbitrary paths, and recovery without an operation", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(installContract.INSTALL_CHANNEL);
  assert.ok(handler);
  await assertRejectedWithoutSidecar(handler, {sender: {}, senderFrame: {}}, harness);
  for (const input of [
    {action: "prepare", provider: "codex", path: "/other"},
    {action: "start", provider: "claude", command: "install"},
    {action: "recover"},
    {action: "recover", operationId: "11111111-1111-4111-8111-111111111111", force: true},
  ]) await assert.rejects(handler(harness.currentEvent(), input));
  assert.deepEqual(harness.sidecarCalls, []);
});
