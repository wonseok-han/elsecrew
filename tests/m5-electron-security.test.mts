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
import { PreviewBuildManagerV1 } from "../electron/preview-build.ts";
import { previewBuildConsentBox } from "../electron/preview-build-policy.ts";
import { EventEmitter } from "node:events";
import * as previewFlag from "../electron/preview-flag.ts";
import * as zoomMenuModel from "../electron/zoom-menu-model.ts";
/** The View menu's zoom callback main.ts installed (the real menu needs Electron). */
const installedZoom: { current: ((step: -1 | 0 | 1) => void) | null } = { current: null };
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
  nativeTheme: { themeSource: string; shouldUseDarkColors: boolean; emit(event: string): void };
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
    sent: [] as [string, unknown][],
    send: (channel: string, payload: unknown) => { this.webContents.sent.push([channel, payload]); },
    setWindowOpenHandler: (handler: () => { action: string }) => { this.openHandler = handler; },
    getZoomFactor: () => 1,
    zoomLevel: 0,
    zoomCalls: [] as string[],
    getZoomLevel: () => this.webContents.zoomLevel,
    setZoomLevel: (level: number) => { this.webContents.zoomLevel = level; this.webContents.zoomCalls.push(`setZoomLevel:${level}`); },
    setZoomFactor: (factor: number) => { this.webContents.zoomCalls.push(`setZoomFactor:${factor}`); },
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

  readonly backgrounds: string[] = [];
  setBackgroundColor(color: string) {
    this.backgrounds.push(color);
  }

  getContentSize() {
    return [1280, 800];
  }

  getBounds() {
    return { x: 0, y: 0, width: 1280, height: 800 };
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
  const themeListeners = new Map<string, (() => void)[]>();
  const nativeTheme = {
    themeSource: "system", shouldUseDarkColors: true,
    on: (event: string, listener: () => void) => { themeListeners.set(event, [...(themeListeners.get(event) ?? []), listener]); },
    emit: (event: string) => { for (const listener of themeListeners.get(event) ?? []) listener(); },
  };
  const electron = {
    nativeTheme,
    app: {
      whenReady: () => Promise.resolve(),
      on: () => undefined,
      getPath: () => "/app-data",
      getVersion: () => "0.0.0-test",
      getLocale: () => "ko-KR",
      setName: (name: string) => { assert.equal(name, "Elsecrew"); },
      setPath: (key: string, value: string) => { assert.equal(key, "userData"); assert.equal(value, "/app-data"); },
      isPackaged: false,
      commandLine: {
        appendSwitch: (name: string, value?: string) => { switches.push(value === undefined ? name : `${name}=${value}`); },
        getSwitchValue: (name: string) => name === "blink-settings" ? (env.LAUNCH_BLINK_SETTINGS ?? "") : "",
      },
      quit: () => undefined,
    },
    BrowserWindow: Object.assign(FakeWindow, { getAllWindows: () => FakeWindow.windows }),
    screen: { getDisplayMatching: () => ({ scaleFactor: 1 }) },
    dialog: { showOpenDialog: async () => folderPick.path ? { canceled: false, filePaths: [folderPick.path] } : { canceled: true, filePaths: [] }, showMessageBox: async (first: unknown, second?: { detail?: string }) => { const options = (second ?? first) as { detail?: string }; sidecarCalls.push(`confirm:${options.detail ?? ""}`); return { response: externalLinkAnswer.value }; } },
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
    async previewBuildPlan(input: { projectId: string; chatId: string }) {
      sidecarCalls.push(`previewBuildPlan:${JSON.stringify(input)}`);
      return { ...input, worktreeId: "worktree_1", workspaceIdentity: "workspace_1", ...fakePreviewBuild.plan };
    }
    async previewResolve(input: { projectId: string; chatId: string; entry: string | null }) {
      sidecarCalls.push(`previewResolve:${JSON.stringify(input)}`);
      return { ...input, worktreeId: "worktree_1", workspaceIdentity: "workspace_1", previewHandle: "ph_cccccccccccccccccccccccccccccccc", entry: null, refusal: null };
    }
    async getProjectTree() {
      sidecarCalls.push("getProjectTree");
      return { tree: true };
    }
    failRendererDisconnected() {}
    async install(input: { action: string }) {
      sidecarCalls.push(`install:${input.action}`);
      if (input.action === "start") return { state: "starting", operationId: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", provider: installAwaiting.plan.provider, plan: null, error: null, cancelRequested: false };
      return installAwaiting;
    }
    async openProject(path: string) { sidecarCalls.push(`openProject:${path}`); return { outcome: "opened" }; }
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
    readonly #options: { allowProcessExit?: (kind: string) => void };
    constructor(options: unknown) { this.#options = options as { allowProcessExit?: (kind: string) => void }; }
    prepareQuit() {
      if (quitExit.simulate && quitAnswer.value.safeToQuit) this.#options.allowProcessExit?.("clean");
      return Promise.resolve(quitAnswer.value);
    }
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
    // Timers are held, not run: a test fires them (the folder confirmation's time limit).
    setTimeout: (callback: () => void) => { heldTimers.push(callback); return heldTimers.length; },
    clearTimeout: () => undefined,
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
    nativeTheme,
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
  if (specifier === "./app-update-menu.ts") return { installAppUpdateMenu: (restart: (install: () => void) => Promise<boolean>, changed: (state: string) => void, reveal?: () => void) => { fakeUpdate.restartCallback = restart; fakeUpdate.changed = changed; fakeUpdate.reveal = reveal ?? null; return fakeUpdate.handle; } };
  if (specifier === "./zoom-menu.ts") return { installZoomGuardedMenu: (zoom: (step: -1 | 0 | 1) => void) => { installedZoom.current = zoom; }, zoomLevelAfter: zoomMenuModel.zoomLevelAfter };
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
  // The real runner with this computer's parts faked (no program ever starts).
  if (specifier === "./preview-build-host.ts") return { createPreviewBuildManager: fakePreviewBuild.create };
  // The platform is the harness's (macOS unless a test names another), not the machine's.
  if (specifier === "./preview-flag.ts") return { previewFlags: (packaged: boolean) => previewFlag.previewFlagsFromEnvironment(fakes.env, packaged, fakes.env.TEST_PLATFORM ?? "darwin") };
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
      readdirSync: (path: string) => { if (path === folderPick.path) return []; throw new Error("unexpected directory listing in main security harness"); },
      statSync: (path: string) => {
        if (path === folderPick.path) return { dev: 1n, ino: 2n };
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
  if (specifier === "./appearance.ts") return appearanceModule;
  if (specifier === "./sidecar-client.ts") return { SIDECAR_READY_TIMEOUT_MS: 3_000, SidecarClientV1: fakes.FakeSidecar };
  throw new Error(`unexpected main.ts runtime import: ${specifier}`);
}
import * as loginContract from "../src/m5/contracts/provider-login-v1.ts";
import * as appearanceModule from "../electron/appearance.ts";
import * as approvalContract from "../src/m5/contracts/approval-v1.ts";
import * as externalLinkContract from "../src/m5/contracts/external-link-v1.ts";
/** What the fake quit coordinator answers to a quit preparation. */
const quitAnswer = { value: { blockers: [] as string[], safeToQuit: true } };
/** The folder the fake open dialog picks (an empty one, on macOS), or none. */
const folderPick: { path: string | null } = { path: null };
/** Build-then-preview: the real runner, this computer's parts faked. */
const fakePreviewBuild = {
  plan: { refusal: null, workspacePath: "/Users/me/elsecrew/workspaces/app/task-1", packageManager: "npm", lockfile: true, buildScript: "vite build", outputDir: "dist", needsInstall: false } as Record<string, unknown>,
  spawns: [] as { program: string; args: string[]; cwd: unknown; env: Record<string, string>; detached: unknown }[],
  kills: [] as [number, string][],
  children: [] as (EventEmitter & { stdout: EventEmitter & { setEncoding(): void }; stderr: EventEmitter & { setEncoding(): void }; pid: number })[],
  create: (options: { enabled: boolean; sidecar(): never; ask(box: unknown): Promise<number>; korean(): boolean; log(label: string): void }) => new PreviewBuildManagerV1({
    enabled: options.enabled, sidecar: options.sidecar,
    confirm: async consent => (await options.ask(previewBuildConsentBox(consent, options.korean()))) === 1,
    findTools: () => ({ program: "/opt/homebrew/bin/npm", pathDirs: ["/opt/homebrew/bin", "/usr/bin", "/bin"] }),
    isWorkspaceFolder: () => true,
    spawn: (program, args, spawnOptions) => {
      fakePreviewBuild.spawns.push({ program, args, cwd: spawnOptions.cwd, env: spawnOptions.env as Record<string, string>, detached: spawnOptions.detached });
      const stream = () => Object.assign(new EventEmitter(), { setEncoding() {} });
      const child = Object.assign(new EventEmitter(), { stdout: stream(), stderr: stream(), pid: 4242 + fakePreviewBuild.children.length });
      fakePreviewBuild.children.push(child);
      return child as never;
    },
    killGroup: (pid, signal) => { fakePreviewBuild.kills.push([pid, signal]); },
    environment: { HOME: "/Users/me", PATH: "/usr/bin", ANTHROPIC_API_KEY: "sk-ant-api03-secretsecretsecret", OPENAI_API_KEY: "sk-proj-secretsecretsecret", GITHUB_TOKEN: "ghp_secretsecretsecretsecret", ELSECREW_PREVIEW: "1", LANG: "ko_KR.UTF-8" },
    writeLog: () => undefined,
    log: options.log,
  }),
};
/** Timers main.ts set, held so a test can run them. */
const heldTimers: (() => void)[] = [];
/** main's app update handle, played by the harness (ADE U8): its state and what main asked of it. */
const fakeUpdate = {
  state: "unavailable", calls: [] as string[], changed: null as ((state: string) => void) | null, reveal: null as (() => void) | null,
  /** main's real restart callback (screen readiness, then the quit preparation); the harness's controller calls it like the real one. */
  restartCallback: null as ((install: () => void) => Promise<boolean>) | null,
  handle: { state: () => fakeUpdate.state, check: () => { fakeUpdate.calls.push("check"); },
    restart: async () => {
      fakeUpdate.calls.push("restart");
      const safe = await fakeUpdate.restartCallback?.(() => { fakeUpdate.calls.push("install"); });
      fakeUpdate.calls.push(safe ? "safe" : "blocked");
    } },
};
/** The fake quit coordinator allows a clean exit on a safe result, as the real one does (only for the update test). */
const quitExit = { simulate: false };
/** The installation that awaits consent, as the sidecar reports it (PR #14 review B1). */
const installAwaiting = { state: "awaitingConsent", operationId: null, provider: "codex", error: null, cancelRequested: false,
  plan: { planId: "11111111-2222-4333-8444-555555555555", provider: "codex", action: "installOrUpdate", expiresInSeconds: 180,
    writeDirectories: ["/Users/me/.local/bin", "/Users/me/.codex/packages/standalone"], replacesExistingCommands: true, shellSettingsWritable: false, startsLogin: false } };
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

test("appearance IPC answers only the main frame, takes one of three choices and follows it natively", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(appearanceModule.APPEARANCE_CHANNEL)!;
  assert.ok(handler);
  assert.deepEqual(harness.window.options && (harness.window.options as { backgroundColor?: string }).backgroundColor, appearanceModule.WINDOW_BACKGROUND.dark);
  await assert.rejects(handler({ sender: {}, senderFrame: {} }, "light"));
  await assert.rejects(handler(harness.currentEvent(), "sepia"));
  await assert.rejects(handler(harness.currentEvent(), { theme: "light" }));
  await assert.rejects(handler(harness.currentEvent(), "light", "extra"));
  await assert.rejects(handler(harness.currentEvent()));
  assert.equal(harness.nativeTheme.themeSource, "system");
  // The harness's app data is not writable: the answer says the choice was not remembered (PR #10 review I1).
  assert.deepEqual(JSON.parse(JSON.stringify(await handler(harness.currentEvent(), "light"))), { dark: true, saved: false });
  assert.equal(harness.nativeTheme.themeSource, "light");
  assert.equal(harness.window.backgrounds.at(-1), appearanceModule.WINDOW_BACKGROUND.dark);
  harness.nativeTheme.shouldUseDarkColors = false;
  await handler(harness.currentEvent(), "system");
  assert.equal(harness.nativeTheme.themeSource, "system");
  assert.equal(harness.window.backgrounds.at(-1), appearanceModule.WINDOW_BACKGROUND.light);
  assert.deepEqual(harness.sidecarCalls, []);
});

test("appearance record IPC answers only the main frame, takes nothing, and reports main's record (PR #10 review I1)", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(appearanceModule.APPEARANCE_GET_CHANNEL)!;
  assert.ok(handler);
  await assert.rejects(handler({ sender: {}, senderFrame: {} }));
  await assert.rejects(handler(harness.currentEvent(), "dark"));
  // No readable record in the harness's app data: system, not saved.
  assert.deepEqual(JSON.parse(JSON.stringify(await handler(harness.currentEvent()))), { theme: "system", saved: false, dark: true });
  assert.deepEqual(harness.sidecarCalls, []);
});

test("a system brightness change repaints the open window's background (PR #10 review I2)", async () => {
  const harness = await loadMain();
  const before = harness.window.backgrounds.length;
  harness.nativeTheme.shouldUseDarkColors = false;
  harness.nativeTheme.emit("updated");
  assert.equal(harness.window.backgrounds.length, before + 1);
  assert.equal(harness.window.backgrounds.at(-1), appearanceModule.WINDOW_BACKGROUND.light);
  harness.nativeTheme.shouldUseDarkColors = true;
  harness.nativeTheme.emit("updated");
  assert.equal(harness.window.backgrounds.at(-1), appearanceModule.WINDOW_BACKGROUND.dark);
});

test("update readiness rejects unsolicited, stale and foreign replies", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get("m5:app-update:readiness-reply")!;
  assert.ok(handler);
  await assert.rejects(handler({sender:{},senderFrame:{}}, "fake", true));
  await assert.rejects(handler(harness.currentEvent(), "stale", true));
  await assert.rejects(handler(harness.currentEvent(), undefined, true));
});

test("result preview IPC is on by default on macOS, off with ELSECREW_PREVIEW=0 and elsewhere, answers only the main frame, and never takes a workspace, token or URL", async () => {
  const open = { action: "open", projectId: "project_1", chatId: "chat_1", entry: null, source: "workspace", bounds: { x: 700, y: 60, width: 500, height: 600 }, viewport: "wide", pixelRatio: 1, page: { width: 1280, height: 800 } };
  // Windows is not checked yet (PR #9 review I2·I5): off unless asked for.
  const windows = await loadMain(undefined, false, { TEST_PLATFORM: "win32" });
  assert.deepEqual(windows.switches, []);
  assert.equal(JSON.stringify(await windows.handlers.get(previewContract.PREVIEW_CHANNEL_V1)!(windows.currentEvent(), { action: "status" })), JSON.stringify({ action: "status", enabled: false }));
  const windowsOn = await loadMain(undefined, false, { TEST_PLATFORM: "win32", ELSECREW_PREVIEW: "1" });
  assert.deepEqual(windowsOn.switches, ["blink-settings=dnsPrefetchingEnabled=false"]);
  const off = await loadMain(undefined, false, { ELSECREW_PREVIEW: "0" });
  assert.deepEqual(off.privilegedSchemes.map(entry => entry.scheme), ["app"], "no preview scheme when turned off");
  assert.deepEqual(off.switches, [], "no Chromium switch when turned off");
  const offHandler = off.handlers.get(previewContract.PREVIEW_CHANNEL_V1);
  assert.ok(offHandler);
  assert.equal(JSON.stringify(await offHandler(off.currentEvent(), { action: "status" })), JSON.stringify({ action: "status", enabled: false }));
  await assert.rejects(offHandler(off.currentEvent(), open), /not enabled/);
  assert.deepEqual(off.sidecarCalls, []);

  // The default (no setting) is on.
  const on = await loadMain();
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
  // A stage not on screen at the window's zoom is refused in main too, and
  // so is a measurement taken at another zoom (PR #9 review B1).
  await assert.rejects(handler(on.currentEvent(), { ...open, bounds: { x: 1200, y: 0, width: 500, height: 600 } }), /not on screen/);
  await assert.rejects(handler(on.currentEvent(), { ...open, bounds: { x: 0, y: 0, width: 100, height: 600 } }), /too small|not on screen/);
  await assert.rejects(handler(on.currentEvent(), { ...open, pixelRatio: 2 }), /not on screen/);
  await assert.rejects(handler(on.currentEvent(), { ...open, page: { width: 1100, height: 800 } }), /not on screen/, "a measurement from another window size");
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

test("a launch blink-settings value is kept; quit preparation closes the preview and a called-off one reopens it (review I4)", async () => {
  const merged = await loadMain(undefined, false, { ELSECREW_PREVIEW: "1", LAUNCH_BLINK_SETTINGS: "hideScrollbars" });
  assert.deepEqual(merged.switches, ["blink-settings=hideScrollbars,dnsPrefetchingEnabled=false"]);
  const handler = merged.handlers.get(previewContract.PREVIEW_CHANNEL_V1)!;
  const prepare = merged.handlers.get(M5_INTERNAL_CHANNELS_V1.appPrepareQuit)!;
  const status = async () => JSON.parse(JSON.stringify(await handler(merged.currentEvent(), { action: "status" }))).enabled;
  assert.equal(await status(), true);
  // An update's quit preparation that is called off: the preview may open again.
  quitAnswer.value = { blockers: ["unsaved"], safeToQuit: false };
  try {
    await prepare(merged.currentEvent());
    assert.equal(await status(), true);
    // Quitting for real: refused from then on.
    quitAnswer.value = { blockers: [], safeToQuit: true };
    await prepare(merged.currentEvent());
    assert.equal(await status(), false);
  } finally {
    quitAnswer.value = { blockers: [], safeToQuit: true };
  }
  // The app page starting a new document closes the preview (manager tests
  // cover what closing does); the listener is wired on the main window.
  const navigation = merged.window.webContents.listeners.get("did-start-navigation");
  assert.ok(navigation);
  assert.ok(merged.window.webContents.listeners.get("render-process-gone"));
});

test("the View menu's zoom and a window resize hide the preview before anything moves (PR #9 re-review B1)", async () => {
  const order: string[] = [];
  const original = previewManagerModule.PreviewManagerV1.prototype.geometryChanged;
  previewManagerModule.PreviewManagerV1.prototype.geometryChanged = function geometryChanged(this: InstanceType<typeof previewManagerModule.PreviewManagerV1>) { order.push("hide"); return original.call(this); };
  try {
    const on = await loadMain();
    assert.ok(installedZoom.current, "main installs the guarded View menu");
    const contents = on.window.webContents as unknown as { zoomCalls: string[] };
    contents.zoomCalls.length = 0;
    const record = contents.zoomCalls.push.bind(contents.zoomCalls);
    contents.zoomCalls.push = (...items: string[]) => { order.push(...items); return record(...items); };
    installedZoom.current!(1);
    installedZoom.current!(1);
    installedZoom.current!(0);
    assert.deepEqual(order, ["hide", "setZoomLevel:0.5", "hide", "setZoomLevel:1", "hide", "setZoomLevel:0"]);
    // The window tells main before and as it resizes, and around full screen.
    for (const event of ["will-resize", "resize", "enter-full-screen", "leave-full-screen"]) {
      order.length = 0;
      on.window.listeners.get(event)!();
      assert.deepEqual(order, ["hide"], event);
    }
  } finally {
    previewManagerModule.PreviewManagerV1.prototype.geometryChanged = original;
  }
  // The menu's zoom items are the only zoom path, each through the callback.
  const steps: number[] = [];
  const view = zoomMenuModel.viewMenuTemplate(true, step => { steps.push(step); });
  const items = view.submenu as { role?: string; accelerator?: string; click?: () => void }[];
  assert.equal(items.some(item => /zoom/i.test(item.role ?? "")), false, "no built-in zoom role");
  for (const item of items.filter(entry => entry.accelerator)) item.click!();
  assert.deepEqual(steps, [0, 1, 1, -1]);
  assert.equal(zoomMenuModel.zoomLevelAfter(9, 1), 9);
  assert.equal(zoomMenuModel.zoomLevelAfter(-8, -1), -8);
  assert.equal(zoomMenuModel.zoomLevelAfter(2.5, 0), 0);
});

test("a folder that becomes a project is confirmed in main's own box; the page has no way to answer it (PR #12 review B1)", async () => {
  const harness = await loadMain();
  folderPick.path = "/Users/me/new-shop";
  const answer = externalLinkAnswer.value;
  try {
    const open = harness.handlers.get(M5_BRIDGE_V2_CHANNELS.projectOpen)!;
    // No channel lets the renderer say yes for main.
    assert.equal([...harness.handlers.keys()].some(channel => /folder-confirm/.test(channel)), false);
    // "Cancel" in main's box: nothing opens and nothing is sent to the page.
    externalLinkAnswer.value = 1;
    assert.equal((await open(harness.currentEvent()) as { outcome: string }).outcome, "cancelled");
    assert.ok(harness.sidecarCalls.some(call => call.startsWith("confirm:/Users/me/new-shop")), "the box names the folder");
    assert.ok(!harness.sidecarCalls.some(call => call.startsWith("openProject")));
    assert.deepEqual((harness.window.webContents as unknown as { sent: unknown[] }).sent, []);
    // "Start" in main's box: the folder main picked opens.
    externalLinkAnswer.value = 0;
    assert.equal((await open(harness.currentEvent()) as { outcome: string }).outcome, "opened");
    assert.ok(harness.sidecarCalls.includes("openProject:/Users/me/new-shop"));
  } finally {
    folderPick.path = null;
    externalLinkAnswer.value = answer;
  }
});

test("in-app update banner: the page reads main's state and asks; only main's own box can restart (ADE U8, Q7)", async () => {
  const harness = await loadMain();
  const state = harness.handlers.get("m5:app-update:state")!;
  const request = harness.handlers.get("m5:app-update:request")!;
  assert.ok(state && request);
  const answer = externalLinkAnswer.value;
  fakeUpdate.calls.length = 0;
  try {
    fakeUpdate.state = "ready";
    // Only the main frame, no input to read the state; one fixed request word to ask.
    await assert.rejects(state({ sender: {}, senderFrame: {} }));
    await assert.rejects(state(harness.currentEvent(), "x"));
    assert.equal(await state(harness.currentEvent()), "ready");
    for (const bad of [[], ["install"], ["restart", true], [{ action: "restart" }], ["quitAndInstall"]]) {
      await assert.rejects(request(harness.currentEvent(), ...bad), JSON.stringify(bad));
    }
    await assert.rejects(request({ sender: {}, senderFrame: {} }, "restart"));
    assert.deepEqual(fakeUpdate.calls, []);
    // A restart request alone does nothing: main asks in its own box; "Cancel" keeps the app running.
    externalLinkAnswer.value = 0;
    assert.equal(await request(harness.currentEvent(), "restart"), "ready");
    assert.ok(harness.sidecarCalls.some(call => call.startsWith("confirm:")), "main asked in its own box");
    assert.deepEqual(fakeUpdate.calls, []);
    externalLinkAnswer.value = 1;
    heldTimers.length = 0;
    const restarting = request(harness.currentEvent(), "restart");
    for (let tries = 0; tries < 50 && !heldTimers.length; tries++) await new Promise(resolve => setImmediate(resolve));
    heldTimers.at(-1)!(); // the screen never answers its readiness: not ready, nothing installs
    await restarting;
    assert.deepEqual(fakeUpdate.calls, ["restart", "blocked"]);
    // No downloaded update: nothing to restart into, and no box.
    fakeUpdate.state = "idle";
    const boxes = harness.sidecarCalls.filter(call => call.startsWith("confirm:")).length;
    await assert.rejects(request(harness.currentEvent(), "restart"));
    assert.equal(harness.sidecarCalls.filter(call => call.startsWith("confirm:")).length, boxes);
    // A check only starts a check (the controller decides whether it may).
    await request(harness.currentEvent(), "check");
    assert.deepEqual(fakeUpdate.calls, ["restart", "blocked", "check"]);
    // State changes reach the page.
    fakeUpdate.changed?.("downloading");
    const sent = (harness.window.webContents as unknown as { sent: [string, unknown][] }).sent;
    assert.deepEqual(sent.at(-1), ["m5:app-update:changed", "downloading"]);
  } finally {
    externalLinkAnswer.value = answer;
    fakeUpdate.state = "unavailable";
  }
});

test("installing starts only after main's own box: a page with a valid plan cannot give consent (PR #14 review B1)", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(installContract.INSTALL_CHANNEL)!;
  const answer = externalLinkAnswer.value;
  const start = { action: "start", provider: "codex", planId: installAwaiting.plan.planId };
  try {
    // "Cancel" in main's box: nothing starts, the plan still waits.
    externalLinkAnswer.value = 0;
    assert.equal((await handler(harness.currentEvent(), start) as { state: string }).state, "awaitingConsent");
    assert.ok(harness.sidecarCalls.some(call => call.startsWith("confirm:") && call.includes("/Users/me/.codex/packages/standalone")), "main names the folders itself");
    assert.ok(!harness.sidecarCalls.includes("install:start"));
    // A plan that is not the one awaiting consent, or another agent, is refused without a box.
    const boxes = () => harness.sidecarCalls.filter(call => call.startsWith("confirm:")).length;
    const before = boxes();
    await assert.rejects(handler(harness.currentEvent(), { ...start, planId: "99999999-2222-4333-8444-555555555555" }));
    await assert.rejects(handler(harness.currentEvent(), { ...start, provider: "claude" }));
    assert.equal(boxes(), before);
    assert.ok(!harness.sidecarCalls.includes("install:start"));
    // Only "Install or update" in main's box starts it.
    externalLinkAnswer.value = 1;
    assert.equal((await handler(harness.currentEvent(), start) as { state: string }).state, "starting");
    assert.equal(harness.sidecarCalls.filter(call => call === "install:start").length, 1);
    // Preparing and reading need no box.
    const boxesNow = boxes();
    await handler(harness.currentEvent(), { action: "prepare", provider: "claude" });
    await handler(harness.currentEvent(), { action: "status" });
    assert.equal(boxes(), boxesNow);
  } finally { externalLinkAnswer.value = answer; }
});

test("update restart: main's yes, then the screen's readiness and the quit preparation decide; only a clean result installs (PR #16 review I1)", async () => {
  const harness = await loadMain();
  const request = harness.handlers.get("m5:app-update:request")!;
  const reply = harness.handlers.get("m5:app-update:readiness-reply")!;
  const sent = (harness.window.webContents as unknown as { sent: [string, unknown][] }).sent;
  const answer = externalLinkAnswer.value;
  const quit = quitAnswer.value;
  const settle = () => new Promise(resolve => setImmediate(resolve));
  const readinessAsk = async () => {
    for (let tries = 0; tries < 50; tries++) {
      const ask = [...sent].reverse().find(([channel]) => channel === "m5:app-update:readiness");
      if (ask && !answered.has(ask[1] as string)) { answered.add(ask[1] as string); return ask[1] as string; }
      await settle();
    }
    throw new Error("main never asked the screen");
  };
  const answered = new Set<string>();
  const run = async (screenReady: boolean | "silent", safe: boolean) => {
    fakeUpdate.calls.length = 0;
    quitAnswer.value = { blockers: safe ? [] : ["running work"], safeToQuit: safe };
    heldTimers.length = 0;
    const pending = request(harness.currentEvent(), "restart");
    const nonce = await readinessAsk();
    if (screenReady === "silent") heldTimers.at(-1)!(); // no answer within its time: counts as not ready
    else await reply(harness.currentEvent(), nonce, screenReady);
    await pending;
    return [...fakeUpdate.calls];
  };
  try {
    fakeUpdate.state = "ready";
    externalLinkAnswer.value = 1;
    quitExit.simulate = true;
    // The screen has unsaved edits, or never answers: nothing installs.
    assert.deepEqual(await run(false, true), ["restart", "blocked"]);
    assert.deepEqual(await run("silent", true), ["restart", "blocked"]);
    // The screen is ready but work cannot stop safely: nothing installs.
    assert.deepEqual(await run(true, false), ["restart", "blocked"]);
    // Ready and safe: the quit preparation allows a clean exit, which installs.
    assert.deepEqual(await run(true, true), ["restart", "install", "safe"]);
    // A readiness answer for a request main did not make is refused.
    await assert.rejects(reply(harness.currentEvent(), "not-asked", true));
  } finally {
    externalLinkAnswer.value = answer;
    quitAnswer.value = quit;
    quitExit.simulate = false;
    fakeUpdate.state = "unavailable";
  }
});

test("build-then-preview: a build runs only after main's own box, in the folder the engine named, without the app's secrets (결과 미리보기 확장 1단계)", async () => {
  const harness = await loadMain();
  const handler = harness.handlers.get(previewContract.PREVIEW_BUILD_CHANNEL_V1)!;
  assert.ok(handler, "the build channel is registered");
  const answer = externalLinkAnswer.value;
  const start = { action: "start", projectId: "project_1", chatId: "chat_1", step: "build" };
  const boxes = () => harness.sidecarCalls.filter(call => call.startsWith("confirm:"));
  fakePreviewBuild.spawns.length = 0;
  try {
    // Another frame or page is refused before the engine or a box.
    const previewPage = { mainFrame: { url: `elsecrew-preview://${"a".repeat(32)}/index.html` } };
    await assert.rejects(handler({ sender: previewPage, senderFrame: previewPage.mainFrame }, start));
    // The page cannot name a folder, a command or a program.
    for (const bad of [{ ...start, cwd: "/tmp" }, { ...start, command: "rm -rf /" }, { ...start, step: "dev" }, { ...start, workspacePath: "/" }]) {
      await assert.rejects(handler(harness.currentEvent(), bad));
    }
    assert.equal(boxes().length, 0);
    assert.ok(!harness.sidecarCalls.some(call => call.startsWith("previewBuildPlan:")));
    // The plan the page sees has the commands, never the folder.
    const plan = await handler(harness.currentEvent(), { action: "plan", projectId: "project_1", chatId: "chat_1" }) as { plan: Record<string, unknown> };
    assert.deepEqual(plan.plan, { refusal: null, installCommand: "npm ci --no-audit --no-fund", buildCommand: "npm run build", needsInstall: false, outputDir: "dist" });
    assert.ok(!JSON.stringify(plan).includes("/Users/me"));
    // "취소" in main's box: nothing runs.
    externalLinkAnswer.value = 0;
    assert.equal((await handler(harness.currentEvent(), start) as { outcome: string }).outcome, "declined");
    assert.equal(fakePreviewBuild.spawns.length, 0);
    const box = boxes().at(-1)!;
    for (const shown of ["실행할 명령: npm run build", "위치: /Users/me/elsecrew/workspaces/app/task-1", "프로그램: /opt/homebrew/bin/npm", "빌드 스크립트: vite build"]) assert.ok(box.includes(shown), shown);
    // "빌드" in main's box: the fixed command in the engine's folder, its own group, no secrets.
    externalLinkAnswer.value = 1;
    assert.equal((await handler(harness.currentEvent(), start) as { outcome: string }).outcome, "started");
    assert.equal(fakePreviewBuild.spawns.length, 1);
    const spawned = fakePreviewBuild.spawns[0]!;
    assert.deepEqual([spawned.program, spawned.args, spawned.cwd, spawned.detached], ["/opt/homebrew/bin/npm", ["run", "build"], "/Users/me/elsecrew/workspaces/app/task-1", process.platform !== "win32"]);
    assert.deepEqual(Object.keys(spawned.env).filter(name => /KEY|TOKEN|ELSECREW/.test(name)), []);
    assert.equal(spawned.env.HOME, "/Users/me");
    // One job per task: a second start while it runs is busy, without a box.
    const asked = boxes().length;
    assert.equal((await handler(harness.currentEvent(), start) as { outcome: string }).outcome, "busy");
    assert.equal(boxes().length, asked);
    // A secret the build prints is masked before the page can read it.
    const child = fakePreviewBuild.children.at(-1)!;
    child.stdout.emit("data", "token=ghp_abcdefghijklmnopqrstuvwxyz0123\nbuilt in 1s\n");
    const status = await handler(harness.currentEvent(), { action: "status", projectId: "project_1", chatId: "chat_1", cursor: 0 }) as { job: { lines: string[]; state: string } };
    assert.ok(status.job.lines.some(line => line === "built in 1s"));
    assert.ok(!status.job.lines.join("\n").includes("abcdefghijklmnopqrstuvwxyz0123"));
    // The app page's process ending stops it: only its own group is signalled.
    fakePreviewBuild.kills.length = 0;
    harness.window.webContents.listeners.get("render-process-gone")!();
    assert.deepEqual(fakePreviewBuild.kills, [[child.pid, "SIGTERM"]]);
    child.emit("close", null);
    const ended = await handler(harness.currentEvent(), { action: "status", projectId: "project_1", chatId: "chat_1", cursor: 0 }) as { job: { state: string } };
    assert.equal(ended.job.state, "cancelled");
    // Packages missing: building is refused (the page offers installing), without a box.
    fakePreviewBuild.plan = { ...fakePreviewBuild.plan, needsInstall: true };
    const before = boxes().length;
    assert.equal((await handler(harness.currentEvent(), start) as { outcome: string }).outcome, "refused");
    assert.equal(boxes().length, before);
  } finally {
    externalLinkAnswer.value = answer;
    fakePreviewBuild.plan = { ...fakePreviewBuild.plan, needsInstall: false };
  }
});
