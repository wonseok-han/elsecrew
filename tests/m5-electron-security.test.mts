import * as gitSelectionContract from "../src/m5/contracts/git-selection-v1.ts";
import * as saveRecoveryContract from "../src/m5/contracts/save-recovery-v1.ts";
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
import * as usageContract from "../src/m5/contracts/provider-usage-v1.ts";
import * as previewContract from "../src/m5/contracts/preview-v1.ts";
import * as previewPolicy from "../electron/preview-policy.ts";
import * as previewManagerModule from "../electron/preview-manager.ts";
import * as previewFlag from "../electron/preview-flag.ts";
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
  assert.deepEqual(harness.sidecarCalls, ["projectCatalog", "login", "openExternal"]);
  await handler(harness.currentEvent(), { action: "cancel", attemptId: "test" });
  assert.deepEqual(harness.sidecarCalls, ["projectCatalog", "login", "openExternal", "login"]);
  await assertRejectedWithoutSidecar(handler, {sender: {}, senderFrame: {}}, harness);

  // A provider the user has not connected in the app must not start a login attempt.
  harness.providerConnections.codex = false;
  harness.sidecarCalls.length = 0;
  await assert.rejects(handler(harness.currentEvent(), { action: "start", attemptId: "test" }), /providerDisconnected/);
  assert.deepEqual(harness.sidecarCalls, ["projectCatalog"]);
});

test("a report link opens outside only after a native confirmation in main, never for unsafe addresses or foreign frames", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(externalLinkContract.EXTERNAL_LINK_CHANNEL);
  assert.ok(handler);
  await assertRejectedWithoutSidecar(handler, {sender: {}, senderFrame: {}}, harness);
  for (const unsafe of ["javascript:alert(1)", "data:text/html,<b>x</b>", "file:///etc/passwd", "https://user:pw@example.com/", "/relative", "https://exa mple.com", 42, "https://example.com/" + "a".repeat(3000)]) {
    await assert.rejects(handler(harness.currentEvent(), unsafe));
  }
  assert.deepEqual(harness.sidecarCalls, []);
  externalLinkAnswer.value = 1;
  assert.equal(JSON.stringify(await handler(harness.currentEvent(), "https://example.com/docs")), JSON.stringify({ opened: false }));
  assert.deepEqual(harness.sidecarCalls, ["confirm:https://example.com/docs"]);
  externalLinkAnswer.value = 0;
  assert.equal(JSON.stringify(await handler(harness.currentEvent(), "https://example.com/docs")), JSON.stringify({ opened: true }));
  assert.deepEqual(harness.sidecarCalls, ["confirm:https://example.com/docs", "confirm:https://example.com/docs", "openExternal"]);
  externalLinkAnswer.value = 1;
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
  privilegedSchemes: { scheme: string; privileges: Record<string, unknown> }[];
  partitions: string[];
  switches: string[];
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

  getContentSize() {
    return [1280, 800];
  }

  async loadURL(url: string) {
    this.webContents.mainFrame.url = url;
    this.initialStatus = await FakeWindow.duringLoad?.(this);
  }
}

async function loadMain(devServerUrl: string | undefined = undefined, requestDuringLoad = false, env: Record<string, string> = {}): Promise<MainHarness> {
  FakeWindow.windows = [];
  const handlers = new Map<string, (...args: unknown[]) => Promise<unknown>>();
  FakeWindow.duringLoad = requestDuringLoad ? async (window) => handlers.get(M5_INTERNAL_CHANNELS_V1.systemGetStatus)!({sender: window.webContents, senderFrame: window.webContents.mainFrame}) : undefined;
  const protocolHandlers = new Map<string, (request: { url: string }) => Promise<Response>>();
  const netFetches: string[] = [];
  const sidecarCalls: string[] = [];
  const providerConnections: Record<"claude-code" | "codex", boolean> = {"claude-code": true, codex: true};
  const privilegedSchemes: { scheme: string; privileges: Record<string, unknown> }[] = [];
  const partitions: string[] = [];
  const switches: string[] = [];
  const files = new Set([resolve("/app/.vite/renderer/main_window/index.html")]);
  const electron = {
    app: {
      whenReady: () => Promise.resolve(),
      on: () => undefined,
      getPath: () => "/app-data",
      getVersion: () => "0.0.0-test",
      getLocale: () => "ko-KR",
      setName: (name: string) => { assert.equal(name, "Elsecrew"); },
      setPath: (key: string, value: string) => { assert.equal(key, "userData"); assert.equal(value, "/app-data"); },
      isPackaged: false,
      commandLine: { appendSwitch: (name: string, value?: string) => { switches.push(value === undefined ? name : `${name}=${value}`); } },
      quit: () => undefined,
    },
    BrowserWindow: Object.assign(FakeWindow, { getAllWindows: () => FakeWindow.windows }),
    dialog: { showOpenDialog: async () => ({ canceled: true, filePaths: [] }), showMessageBox: async (_window: unknown, options: { detail?: string }) => { sidecarCalls.push(`confirm:${options.detail ?? ""}`); return { response: externalLinkAnswer.value }; } },
    shell: { openExternal: async () => { sidecarCalls.push("openExternal"); } },
    safeStorage: { isEncryptionAvailable: () => false, encryptString: () => { throw new Error("unexpected encryption"); }, decryptString: () => { throw new Error("unexpected decryption"); } },
    ipcMain: {
      handle: (channel: string, handler: (...args: unknown[]) => Promise<unknown>) => handlers.set(channel, handler),
      on: () => undefined,
    },
    net: { fetch: async (url: string) => {
      netFetches.push(url);
      return new Response("local", { status: 200 });
    } },
    // A preview opening in this harness stops at the fake session: the
    // manager has its own tests (m5-preview-manager).
    session: { fromPartition: (partition: string) => { partitions.push(partition); throw new Error("unexpected preview session in main security harness"); } },
    WebContentsView: class { constructor() { throw new Error("unexpected preview view in main security harness"); } },
    protocol: {
      registerSchemesAsPrivileged: (schemes: { scheme: string; privileges: Record<string, unknown> }[]) => { privilegedSchemes.push(...schemes); },
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
    onFailure() {}
    async waitForExit() { return true; }
    exitDetail = null;
    failedBeforeReady = false;
    async getAgentOutputHead() { sidecarCalls.push("getAgentOutputHead"); return null; }
    async getAgentOutputPage() { sidecarCalls.push("getAgentOutputPage"); return { rows: [] }; }
    async start() {}
    async projectCatalog(request: {action: string}) {
      sidecarCalls.push("projectCatalog");
      if (request.action !== "getProviderConnections") throw new Error("unexpected catalog action in main security harness");
      return {action: "getProviderConnections", connections: {"claude-code": providerConnections["claude-code"], codex: providerConnections.codex}};
    }
    async login() {
      sidecarCalls.push("login");
      return {state: "waiting", attemptId: "test", authUrl: "https://auth.openai.com/authorize?state=private", error: null};
    }
    async inspectWorkspaceSaveRecovery(input: Record<string, unknown>) {
      sidecarCalls.push("inspectWorkspaceSaveRecovery");
      return { ...input, recovery: null };
    }
    async resolveWorkspaceSaveRecovery(input: Record<string, unknown>) {
      sidecarCalls.push("resolveWorkspaceSaveRecovery");
      return { chatId: input.chatId, projectId: input.projectId, worktreeId: input.worktreeId,
        workspaceIdentity: input.workspaceIdentity, saveId: input.saveId,
        outcome: input.decision === "abort" ? "aborted" : "committed", version: `sha256:${"a".repeat(64)}` };
    }
    async getSystemStatus() {
      sidecarCalls.push("getSystemStatus");
      return { state: "ready" };
    }
    async previewResolve(input: { projectId: string; chatId: string; entry: string | null }) {
      sidecarCalls.push(`previewResolve:${JSON.stringify(input)}`);
      return { ...input, worktreeId: "worktree_1", workspaceIdentity: "workspace_1", entry: null, refusal: null };
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
    require: (specifier: string) => moduleFor(specifier, { electron, files, FakeSidecar, FakeEventCredit, FakeQuitCoordinator, env }),
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
    providerConnections,
    window,
    privilegedSchemes,
    partitions,
    switches,
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
    env: Record<string, string>;
  },
) {
  if (specifier === "electron") return fakes.electron;
  if (specifier === "./app-update-menu.ts") return { installAppUpdateMenu() {} };
  // Usage reads touch the app keychain and network; the security harness only
  // needs the reader to exist and must fail loudly if a handler reaches it.
  if (specifier === "./claude-usage.ts") return {
    createClaudeUsageReader: () => ({
      read: async () => { throw new Error("unexpected usage read in main security harness"); },
      invalidate: () => undefined,
    }),
  };
  if (specifier.endsWith("provider-usage-v1.ts")) return usageContract;
  if (specifier.endsWith("preview-v1.ts")) return previewContract;
  if (specifier === "./preview-policy.ts") return previewPolicy;
  if (specifier === "./preview-manager.ts") return previewManagerModule;
  if (specifier === "./preview-flag.ts") return { previewFlags: (packaged: boolean) => previewFlag.previewFlagsFromEnvironment(fakes.env, packaged) };
  if (specifier.endsWith("git-selection-v1.ts")) return gitSelectionContract;
  if (specifier === "./git-selection.ts") return {
    GitSelectionStore: class {
      async request() { return { state: "unsupported", path: null, executionEnabled: false }; }
    },
  };
  if (specifier === "./git-preparation.ts") return {
    WINDOWS_GIT_PREPARATION_ENABLED: false,
    isGitPreparation: (request: { action: string }) => ["inspectGitTrust", "confirmGitTrust", "revokeGitTrust"].includes(request.action),
    GitPreparationClient: class {
      pending = false;
      reopen() {}
      async close() {}
      request() { throw new Error("unexpected preparation process in main security harness"); }
    },
  };

  // Diagnostic persistence has its own filesystem tests. This harness exercises
  // the real main IPC/origin checks without writing outside its fake app path.
  if (specifier === "./session-diagnostic-log.ts") return {
    createSessionDiagnosticLog: () => ({
      capture: () => undefined,
      captureTransport: () => undefined,
      flush: async () => undefined,
    }),
  };
  if (specifier.endsWith("save-recovery-v1.ts")) return saveRecoveryContract;
  if (specifier.endsWith("provider-login-v1.ts")) return loginContract;
  if (specifier.endsWith("provider-login-diagnostic.ts")) return loginDiagnostic;
  if (specifier.endsWith("provider-install-v1.ts")) return installContract;
  if (specifier.endsWith("device-report-v1.ts")) return deviceReportContract;
  if (specifier.endsWith("session-diagnostics-v1.ts")) return sessionDiagnosticsContract;
  if (specifier === "./device-report.ts") return { readDeviceReport: () => { throw new Error("unexpected device inspection"); } };
  // The shutdown seal has its own tests (m5-storage-seal): here there is none.
  if (specifier === "./storage-seal.ts") return { StorageSealStore: class { consume() { return null; } async seal() { return false; } } };
  if (specifier.endsWith("approval-v1.ts")) return approvalContract;
  if (specifier.endsWith("external-link-v1.ts")) return externalLinkContract;
  if (specifier.endsWith("project-catalog-v1.ts")) return catalogContract;
  if (specifier.endsWith("model-choices-v1.ts")) return modelChoicesContract;
  if (specifier === "node:fs") {
    return {
      mkdirSync: () => undefined,
      realpathSync: Object.assign((path: string) => path, { native: (path: string) => path }),
      readdirSync: () => { throw new Error("unexpected directory listing in main security harness"); },
      statSync: (path: string) => {
        if (!fakes.files.has(path)) throw new Error("missing test asset");
        return { isFile: () => true };
      },
    };
  }
  if (specifier === "node:path" || specifier === "node:url" || specifier === "node:crypto") return require(specifier);
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
import * as externalLinkContract from "../src/m5/contracts/external-link-v1.ts";
/** What the native "open this address?" confirmation answers (0 = open). */
const externalLinkAnswer = { value: 1 };
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


test("save recovery IPC isolates frames and validates confirmation before reaching the sidecar", async () => {
  const harness = await loadMain();
  const inspect = harness.handlers.get(saveRecoveryContract.SAVE_RECOVERY_CHANNEL)!;
  const resolveSave = harness.handlers.get(saveRecoveryContract.SAVE_RECOVERY_RESOLVE_CHANNEL)!;
  assert.ok(inspect); assert.ok(resolveSave);
  const address = {chatId:"chat_one",projectId:"project_one",worktreeId:"worktree_one",workspaceIdentity:"workspace_one"};
  const request = {...address,saveId:`save_${"a".repeat(32)}`,expectedLockRevision:1,decision:"abort",confirmed:true};
  await assertRejectedWithoutSidecar(inspect, {sender:{},senderFrame:{}}, harness);
  await assertRejectedWithoutSidecar(resolveSave, {sender:{},senderFrame:{}}, harness);
  await assert.rejects(inspect(harness.currentEvent(), {...address,path:"/private/other"}));
  for (const patch of [{confirmed:false},{expectedLockRevision:0},{decision:"force"},{force:true}]) {
    await assert.rejects(resolveSave(harness.currentEvent(), {...request,...patch}));
  }
  assert.deepEqual(harness.sidecarCalls, []);
  assert.deepEqual(JSON.parse(JSON.stringify(await inspect(harness.currentEvent(),address))), {...address,recovery:null});
  const result = await resolveSave(harness.currentEvent(),request) as {outcome:string};
  assert.equal(result.outcome,"aborted");
  assert.deepEqual(harness.sidecarCalls,["inspectWorkspaceSaveRecovery","resolveWorkspaceSaveRecovery"]);
});

test("app update UI IPC rejects foreign frames and renderer-supplied arguments", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get("m5:app-update:open")!;
  assert.ok(handler);
  await assertRejectedWithoutSidecar(handler, {sender:{},senderFrame:{}}, harness);
  await assert.rejects(handler(harness.currentEvent(), { url: "https://untrusted.example" }));
  assert.deepEqual(harness.sidecarCalls, []);
});

test("update readiness rejects unsolicited, stale and foreign replies", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get("m5:app-update:readiness-reply")!;
  assert.ok(handler);
  await assert.rejects(handler({sender:{},senderFrame:{}}, "fake", true));
  await assert.rejects(handler(harness.currentEvent(), "stale", true));
  await assert.rejects(handler(harness.currentEvent(), undefined, true));
});

test("result preview IPC is off by default, answers only the main frame, and never takes a workspace, token or URL", async () => {
  const open = { action: "open", projectId: "project_1", chatId: "chat_1", entry: null, bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide" };
  const off = await loadMain();
  assert.deepEqual(off.privilegedSchemes.map(entry => entry.scheme), ["app"], "no preview scheme without the flag");
  assert.deepEqual(off.switches, [], "no Chromium switch without the flag");
  const offHandler = off.handlers.get(previewContract.PREVIEW_CHANNEL_V1);
  assert.ok(offHandler);
  assert.equal(JSON.stringify(await offHandler(off.currentEvent(), { action: "status" })), JSON.stringify({ action: "status", enabled: false }));
  await assert.rejects(offHandler(off.currentEvent(), open), /not enabled/);
  assert.deepEqual(off.sidecarCalls, []);

  const on = await loadMain(undefined, false, { ELSECREW_PREVIEW: "1" });
  // T3: DNS prefetching (outside proxy, webRequest and CSP) is off app-wide.
  assert.deepEqual(on.switches, ["blink-settings=dnsPrefetchingEnabled=false"]);
  // Privileges only; the default session never gets a preview handler (T14).
  const registered = on.privilegedSchemes.find(entry => entry.scheme === "elsecrew-preview");
  assert.equal(JSON.stringify(registered?.privileges), JSON.stringify({ standard: true, secure: true, supportFetchAPI: true, corsEnabled: false, allowServiceWorkers: false, bypassCSP: false, stream: false, codeCache: false }));
  assert.deepEqual([...on.protocolHandlers.keys()], ["app"]);
  const handler = on.handlers.get(previewContract.PREVIEW_CHANNEL_V1)!;
  assert.equal(JSON.stringify(await handler(on.currentEvent(), { action: "status" })), JSON.stringify({ action: "status", enabled: true }));

  // T6: another page (a preview's own webContents), a subframe, or an
  // unknown origin is refused before anything reaches the sidecar.
  const previewPage = { mainFrame: { url: `elsecrew-preview://${"a".repeat(32)}/index.html` } };
  for (const event of [
    { sender: previewPage, senderFrame: previewPage.mainFrame },
    { sender: on.window.webContents, senderFrame: { url: "app://m5/index.html" } },
    { sender: {}, senderFrame: {} },
  ]) {
    await assert.rejects(handler(event, open));
    await assert.rejects(handler(event, { action: "status" }));
  }
  assert.deepEqual(on.sidecarCalls, []);
  // The renderer names a project and a chat only: never a workspace address,
  // a token, a URL, or a path that is not a page inside the workspace.
  for (const bad of [
    { ...open, worktreeId: "worktree_1" }, { ...open, workspaceIdentity: "workspace_1" }, { ...open, url: "elsecrew-preview://x/" },
    { ...open, token: "a".repeat(32) }, { ...open, entry: "../index.html" }, { ...open, entry: ".git/index.html" }, { ...open, entry: "app.js" },
    { ...open, entry: "/etc/index.html" }, { ...open, bounds: { x: 0, y: 0, width: Number.NaN, height: 10 } }, { ...open, viewport: "tall" },
  ]) await assert.rejects(handler(on.currentEvent(), bad));
  await assert.rejects(handler(on.currentEvent(), open, "extra"));
  // Bounds outside the window are refused in main too.
  await assert.rejects(handler(on.currentEvent(), { ...open, bounds: { x: 1200, y: 0, width: 500, height: 600 } }), /outside the window/);
  await assert.rejects(handler(on.currentEvent(), { ...open, bounds: { x: 0, y: 0, width: 100, height: 600 } }), /too small/);
  assert.deepEqual(on.sidecarCalls, []);
  // A task with no page yet: nothing is created.
  assert.equal(JSON.stringify(await handler(on.currentEvent(), open)), JSON.stringify({ action: "open", outcome: "empty", reason: "noEntry" }));
  assert.deepEqual(on.sidecarCalls, [`previewResolve:${JSON.stringify({ projectId: "project_1", chatId: "chat_1", entry: null })}`]);
  assert.deepEqual(on.partitions, []);
  // The app window still never navigates to a preview origin (T14).
  const navigate = on.window.webContents.listeners.get("will-navigate")!;
  const event = preventedEvent();
  navigate(event, `elsecrew-preview://${"a".repeat(32)}/index.html`);
  assert.equal(event.prevented, true);
});
