import { createClaudeUsageReader } from "./claude-usage.ts";
import { CLAUDE_USAGE_CHANNEL, validateClaudeUsageResult } from "../src/m5/contracts/provider-usage-v1.ts";
import { randomUUID } from "node:crypto";
import { installAppUpdateMenu, type AppUpdateHandle } from "./app-update-menu.ts";
import { installZoomGuardedMenu, zoomLevelAfter, type ZoomStep } from "./zoom-menu.ts";
import { GitPreparationClient, isGitPreparation, WINDOWS_GIT_PREPARATION_ENABLED } from "./git-preparation.ts";
import { GitSelectionStore } from "./git-selection.ts";
import { GIT_SELECTION_CHANNEL, validateGitSelectionAction, validateGitSelection } from "../src/m5/contracts/git-selection-v1.ts";
import { SAVE_RECOVERY_RESOLVE_CHANNEL, validateSaveRecoveryResolveRequest, validateSaveRecoveryResolveResult } from "../src/m5/contracts/save-recovery-v1.ts";
import { SAVE_RECOVERY_CHANNEL, validateSaveRecoveryRequest, validateSaveRecoveryResult } from "../src/m5/contracts/save-recovery-v1.ts";
import { createSessionDiagnosticLog } from "./session-diagnostic-log.ts";
import { MODEL_CHOICES_CHANNEL, validateModelChoicesRequest } from "../src/m5/contracts/model-choices-v1.ts";
import { DEVICE_REPORT_CHANNEL } from "../src/m5/contracts/device-report-v1.ts";
import { SESSION_DIAGNOSTICS_CHANNEL, classifySessionDiagnostics } from "../src/m5/contracts/session-diagnostics-v1.ts";
import { readDeviceReport } from "./device-report.ts";
import { app, BrowserWindow, dialog, ipcMain, Menu, nativeTheme, net, protocol, safeStorage, screen, session, shell, WebContentsView } from "electron";
import { APPEARANCE_CHANNEL, APPEARANCE_GET_CHANNEL, isThemePreference, readThemePreference, windowBackground, writeThemePreference, type ThemePreference } from "./appearance.ts";
import { PREVIEW_BUILD_CHANNEL_V1, PREVIEW_CHANNEL_V1, PREVIEW_EVENT_CHANNEL_V1, validatePreviewBuildIpcRequestV1, validatePreviewIpcRequestV1 } from "../src/m5/contracts/preview-v1.ts";
import { createPreviewBuildManager } from "./preview-build-host.ts";
import { previewFlags } from "./preview-flag.ts";
import { PreviewManagerV1 } from "./preview-manager.ts";
import { PREVIEW_SCHEME, PREVIEW_SCHEME_PRIVILEGES } from "./preview-policy.ts";
import { StorageSealStore } from "./storage-seal.ts";
import { LOGIN_CHANNEL, validateLoginRequest, validateLoginStatus } from "../src/m5/contracts/provider-login-v1.ts";
import { classifyLoginFailure } from "../src/m5/provider-login-diagnostic.ts";
import { INSTALL_CHANNEL, validateInstallRequest, validateInstallStatus } from "../src/m5/contracts/provider-install-v1.ts";
import { APPROVAL_CHANNEL, validateApprovalRequest, validateApprovalResult } from "../src/m5/contracts/approval-v1.ts";
import { EXTERNAL_LINK_CHANNEL, validateExternalLink } from "../src/m5/contracts/external-link-v1.ts";
import { PROJECT_CATALOG_CHANNEL, validateProjectCatalogRequest, validateProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";
import { mkdirSync, readFileSync, realpathSync, rmSync, statSync, readdirSync, writeFileSync } from "node:fs";
import { isAbsolute, join, resolve, sep, win32 } from "node:path";
import { pathToFileURL } from "node:url";

import {
  M5_INTERNAL_CHANNELS_V1,
  type SystemStatusV1,
} from "../src/m5/contracts/lifecycle-v1.ts";
import type { SystemStatusG5DV1 } from "../src/m5/contracts/lifecycle-g5d-v1.ts";
import type { StorageStatusRequestV1 } from "../src/m5/contracts/storage-lifetime-v1.ts";
import type { EventV1, PrepareQuitResultV1 } from "../src/m5/contracts/protocol-v1.ts";
import type { WatchEventV1 } from "../src/m5/contracts/project-v1.ts";
import type { SaveBeginV1 } from "../src/m5/contracts/file-v1.ts";
import type {
  ChatWorkspaceRefV1,
  WorkspaceDiffFileRequestV1,
  WorkspaceFileReadRequestV1,
  WorkspaceSaveBeginRequestV1,
  WorkspaceSaveChunkRequestV1,
  WorkspaceSaveCommitRequestV1,
  WorkspaceTreeRequestV1,
} from "../src/m5/contracts/workspace-addressing-v1.ts";
import type {
  ChatCreateRequestV1, ChatGetRequestV1, ChatListRequestV1, FixtureRunStartRequestV1,
  PageRequestV1, ProjectRoleSettingsUpdateRequestV1, PrepareChatWorkspaceRequestV1,
  AbandonPreparedWorkspaceRequestV1, RecoveryMarkResolvedRequestV1,
  RunCancelRequestV1,
} from "../src/m5/contracts/run-recovery-v1.ts";
import type {
  AgentCapabilitiesRequestV1,
  AgentPublicEventV1,
  AgentRunStateChangedV1,
  AgentRecoveryInspectRequestV1,
  AgentRecoveryResolveRequestV1,
  AgentRunCancelRequestV1,
  AgentRunStartRequestV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import { M5_BRIDGE_V2_CHANNELS, M5_BRIDGE_V3_CHANNELS, M5_BRIDGE_V4_CHANNELS, M5_BRIDGE_V5_CHANNELS, M5_BRIDGE_V6_CHANNELS } from "./ipc-channels.ts";
import { EventCreditV1 } from "./event-credit.ts";
import { QuitCoordinatorV1 } from "./quit-coordinator.ts";
import { SIDECAR_READY_TIMEOUT_MS, SidecarClientV1, type SidecarLaunchV1 } from "./sidecar-client.ts";

declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string | undefined;
declare const MAIN_WINDOW_VITE_NAME: string;

const APP_SCHEME = "app";
const APP_HOST = "m5";
const APP_ORIGIN = `${APP_SCHEME}://${APP_HOST}`;

let mainWindow: BrowserWindow | undefined;
let sidecar: SidecarClientV1 | undefined;
let gitPreparation: GitPreparationClient | undefined;
const eventCredit = new EventCreditV1((error) => sidecar?.failConsumerTooSlow());
let pendingUpdateInstall: (() => void) | undefined;
/** The app update main owns (ADE U8): the page shows its state and may ask; main decides. */
let appUpdate: AppUpdateHandle | undefined;
const APP_UPDATE_REQUESTS = new Set(["check", "restart"]);
let updateReadiness: { nonce: string; finish: (ready: boolean) => void } | undefined;
async function rendererReadyForUpdate(): Promise<boolean> {
  if (!mainWindow || mainWindow.isDestroyed() || updateReadiness) return false;
  return new Promise<boolean>((resolve) => {
    const nonce = randomUUID();
    const timer = setTimeout(() => finish(false), 5000);
    const finish = (ready: boolean) => {
      clearTimeout(timer);
      updateReadiness = undefined;
      if (!ready) mainWindow?.webContents.send("m5:app-update:resume");
      resolve(ready);
    };
    updateReadiness = { nonce, finish };
    try { mainWindow!.webContents.send("m5:app-update:readiness", nonce); }
    catch { finish(false); }
  });
}
/** Result preview (ADE 1차-나 F12): on by default on macOS, where its
 * isolation and screen checks run; `ELSECREW_PREVIEW=0|1` decides elsewhere
 * (electron/preview-flag.ts). */
const PREVIEW_FLAGS = previewFlags(app.isPackaged, process.platform);
const PREVIEW_ENABLED = PREVIEW_FLAGS.enabled;
// T3 (plan 2.4): `<link rel=dns-prefetch>` resolves host names outside the
// proxy, webRequest and CSP (seen in the isolation probe's net log). Blink's
// DNS prefetching is off for every page of the app while the preview is on
// (the default); the app's own page is local and never needs it.
// A `--blink-settings` given at launch (development, diagnosis) is kept: the
// switch holds one value, so this setting is added to it, not put in its place.
if (PREVIEW_ENABLED) {
  const existing = app.commandLine.getSwitchValue("blink-settings");
  app.commandLine.appendSwitch("blink-settings", existing ? `${existing},dnsPrefetchingEnabled=false` : "dnsPrefetchingEnabled=false");
}
const previewManager = new PreviewManagerV1({
  enabled: PREVIEW_ENABLED,
  devTools: PREVIEW_FLAGS.devTools,
  fromPartition: (partition) => session.fromPartition(partition),
  createView: (webPreferences) => new WebContentsView({ webPreferences }),
  window: () => mainWindow,
  // The page's zoom (View menu) and its display's scale turn the page's CSS
  // pixels into window coordinates (PR #9 review B1).
  zoomFactor: () => (mainWindow && !mainWindow.isDestroyed() ? mainWindow.webContents.getZoomFactor() : 1),
  displayScale: () => (mainWindow && !mainWindow.isDestroyed() ? screen.getDisplayMatching(mainWindow.getBounds()).scaleFactor : 1),
  sidecar: () => sidecar,
  send: (event) => {
    const window = mainWindow;
    if (window && !window.isDestroyed()) window.webContents.send(PREVIEW_EVENT_CHANNEL_V1, event);
  },
  log: (label) => console.info(`m5: ${label}`),
  buildOutput: (projectId, chatId) => previewBuildManager.buildOutput(projectId, chatId),
});
/** Build-then-preview (결과 미리보기 확장 1단계): main runs a task's install
 * or build only after the person's "예" in main's own box (PR #12·#14). Not
 * on Windows yet: its process tree and tool lookup are not checked. The
 * program lookup, environment and process group live in preview-build-host.ts. */
const previewBuildManager = createPreviewBuildManager({
  // macOS only: its sandbox, process tree and tool lookup are what was checked.
  enabled: PREVIEW_ENABLED && process.platform === "darwin",
  sidecar: () => sidecar,
  ask: async (box) => {
    const window = mainWindow;
    if (!window || window.isDestroyed()) return 0;
    return (await dialog.showMessageBox(window, { type: "question", noLink: true, defaultId: 0, cancelId: 0, ...box })).response;
  },
  korean: () => app.getLocale().toLowerCase().startsWith("ko"),
  dataDirectory: () => m5DataDirectory(),
  hiddenFolders: () => [m5DataDirectory(), app.getPath("userData")],
  log: (label) => console.info(`m5: ${label}`),
});
/** Stopping agent processes takes up to about 8 seconds each (terminate, then kill). */
const ENGINE_EXIT_WAIT_MS = 12_000;
/** Development only, never in a packaged app: an automated test names the
 * folder to open in a one-shot file in the development app data folder,
 * since a native folder picker cannot be driven in the background. Reading
 * the file consumes it, so every other pick shows the real dialog. */
function takeDevPickedDirectory(): string | null {
  if (app.isPackaged) return null;
  const file = join(app.getPath("userData"), "dev-pick-directory.txt");
  try {
    const path = readFileSync(file, "utf8").trim();
    rmSync(file);
    return path || null;
  } catch { return null; }
}

/** Quitting never turns back (it is never blocked). From then on the hidden
 * screen's background checks (a login status poll every half second, say)
 * would only be refused by the engine, which is preparing to quit, and each
 * refusal is logged as an error: such requests are left unanswered instead.
 * This relies on quitting never being cancelled (no `beforeunload` prompt,
 * no "keep running" path): if one is ever added, reset `quitStarted` there,
 * or the app would be left with a hidden window and unanswered requests. */
let quitStarted = false;
function quietAfterQuit<A extends unknown[], R>(handler: (...args: A) => Promise<R>): (...args: A) => Promise<R> {
  // A request already on its way when quitting began fails as the engine
  // stops (`sidecar exited`): that refusal is left unanswered too.
  return (...args: A) => quitStarted
    ? new Promise<R>(() => {})
    : handler(...args).catch(error => quitStarted ? new Promise<R>(() => {}) : Promise.reject(error));
}

const quitCoordinator = new QuitCoordinatorV1({
  prepareSidecarQuit: () => {
    if (!sidecar) return Promise.reject(new Error("sidecar was not started"));
    return sidecar.prepareQuit();
  },
  failedBeforeReady: () => sidecar?.failedBeforeReady ?? false,
  allowProcessExit: (kind) => {
    const install = pendingUpdateInstall;
    pendingUpdateInstall = undefined;
    if (kind === "clean" && install) { install(); return; }
    // Let the engine finish stopping its agent processes and release the app
    // data before the app ends, so opening it again right away never finds
    // the old engine still holding the data. The windows are already hidden.
    const client = sidecar;
    void (async () => {
      if (client) {
        client.closeInput();
        const exited = await client.waitForExit(ENGINE_EXIT_WAIT_MS);
        // A clean exit sealed the database: the next start need not check
        // the whole ledger again (`storage-seal.ts`).
        const seal = exited && client.exitDetail === "0/none" ? client.storageSeal : null;
        // The later of this session's start check and the engine's background check.
        const fullCheckAt = Math.min(Date.now(), Math.max(lastFullCheckAt, seal?.fullCheckAt ?? 0));
        if (seal) await storageSeals().seal(seal.token, join(m5DataDirectory(), "elsecrew.sqlite3"), fullCheckAt).catch(() => false);
      }
      // What the screen kept for the next start (unsaved editor text, lists)
      // reaches the disk before the app ends.
      try { for (const window of BrowserWindow.getAllWindows()) window.webContents.session.flushStorageData(); } catch { /* best effort */ }
      app.quit();
    })();
  },
  onQuitStarted: () => {
    quitStarted = true;
    // The preview closes at once; its pending reads find it closed.
    previewManager.shutdown();
    // A running install or build ends with the app.
    previewBuildManager.shutdown();
    // Look closed at once; running work stops in the background.
    for (const window of BrowserWindow.getAllWindows()) if (!window.isDestroyed()) window.hide();
  },
});

protocol.registerSchemesAsPrivileged([
  {
    scheme: APP_SCHEME,
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
    },
  },
  // Privileges only: the handler exists on each preview's own session, never
  // on the app's default session (plan 2.2, T14).
  ...(PREVIEW_ENABLED ? [{ scheme: PREVIEW_SCHEME, privileges: { ...PREVIEW_SCHEME_PRIVILEGES } }] : []),
]);

function rendererDirectory() {
  return resolve(__dirname, "..", "renderer", MAIN_WINDOW_VITE_NAME);
}

function m5DataDirectory(): string {
  const candidate = resolve(
    app.getPath("userData"),
    "com.wonseokhan.elsecrew",
    "m5",
  );
  mkdirSync(candidate, { recursive: true });
  // The OS spelling: a development run's userData is the lowercase package name,
  // and the app-only Claude login is keyed by this exact path string.
  return realpathSync.native(candidate);
}

function sidecarLaunch(dataDirectory: string): SidecarLaunchV1 {
  if (app.isPackaged) {
    return {
      command: join(
        process.resourcesPath,
        "sidecar",
        `elsecrew-sidecar${process.platform === "win32" ? ".exe" : ""}`,
      ),
      args: ["--stdio"],
      cwd: dataDirectory,
    };
  }
  return {
    command: resolve(__dirname, "..", "..", "target", "debug", `elsecrew-sidecar${process.platform === "win32" ? ".exe" : ""}`),
    args: ["--stdio"],
    cwd: dataDirectory,
  };
}

async function serveAppAsset(request: Request) {
  let url: URL;
  try {
    url = new URL(request.url);
  } catch {
    return new Response("Not found", { status: 404 });
  }
  if (
    url.protocol !== `${APP_SCHEME}:` || url.hostname !== APP_HOST || url.port !== "" ||
    url.username !== "" || url.password !== "" || url.search !== "" || url.hash !== ""
  ) {
    return new Response("Not found", { status: 404 });
  }

  const rawPath = request.url.slice(request.url.indexOf("/", request.url.indexOf("://") + 3));
  if (/%(?:2e|2f|5c)/i.test(rawPath)) return new Response("Not found", { status: 404 });
  let pathname: string;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch {
    return new Response("Not found", { status: 404 });
  }
  if (!pathname.startsWith("/") || pathname.startsWith("//")) return new Response("Not found", { status: 404 });
  const relativePath = pathname.slice(1) || "index.html";
  if (relativePath.includes("\0") || relativePath.includes("\\") || isAbsolute(relativePath) || win32.isAbsolute(relativePath)) {
    return new Response("Not found", { status: 404 });
  }
  const root = rendererDirectory();
  const asset = resolve(root, relativePath);
  if (asset !== root && !asset.startsWith(`${root}${sep}`)) {
    return new Response("Not found", { status: 404 });
  }
  try {
    if (!statSync(asset).isFile()) return new Response("Not found", { status: 404 });
  } catch {
    return new Response("Not found", { status: 404 });
  }

  const assetResponse = await net.fetch(pathToFileURL(asset).toString());
  const response = new Response(assetResponse.body, {
    status: assetResponse.status,
    statusText: assetResponse.statusText,
    headers: assetResponse.headers,
  });
  // Chromium ignores frame-ancestors in HTML meta policies. Keep the resource
  // policy in the document, and enforce embedding restrictions in the response.
  response.headers.set("Content-Security-Policy", "frame-ancestors 'none'");
  return response;
}

function isAllowedNavigation(target: string) {
  try {
    const url = new URL(target);
    if (!MAIN_WINDOW_VITE_DEV_SERVER_URL) {
      return url.protocol === `${APP_SCHEME}:` && url.hostname === APP_HOST && url.port === "" &&
        url.username === "" && url.password === "";
    }
    const allowed = new URL(MAIN_WINDOW_VITE_DEV_SERVER_URL);
    return allowed.username === "" && allowed.password === "" && url.username === "" && url.password === "" &&
      url.protocol === allowed.protocol && url.hostname === allowed.hostname && url.port === allowed.port;
  } catch {
    return false;
  }
}

function secureWindow(window: BrowserWindow) {
  window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  window.webContents.on("will-attach-webview", (event) => event.preventDefault());
  window.webContents.on("will-navigate", (event, target) => {
    if (!isAllowedNavigation(target)) event.preventDefault();
  });
}

function assertCurrentMainFrame(event: Electron.IpcMainInvokeEvent | Electron.IpcMainEvent): void {
  const window = mainWindow;
  if (!window || window.isDestroyed() || event.sender !== window.webContents) {
    throw new Error("M5 IPC sender is not the current main window");
  }
  if (event.senderFrame !== event.sender.mainFrame) {
    throw new Error("M5 IPC sender must be the main frame");
  }
  if (!isAllowedNavigation(event.senderFrame.url)) {
    throw new Error("M5 IPC sender origin is not allowed");
  }
}

function unavailableStatus(error: unknown): SystemStatusV1 {
  const fallback = error instanceof Error ? error.message : String(error);
  const stderr = sidecar?.technicalDetail;
  return {
    state: "unavailable",
    lastReadySidecar: sidecar?.lastReady ?? null,
    database: { health: "unavailable", schemaVersion: null },
    recoveryRequired: false,
    blockerIds: [],
    error: {
      code: "sidecarUnavailable",
      kind: "sidecarUnavailable",
      // Technical detail is intentionally separate from kind. Stderr is control-normalized
      // and bounded, but is not a token/secret redaction guarantee.
      detail: stderr ? `${fallback}\n${stderr}` : fallback,
      retryable: false,
      outcomeUnknown: true,
    },
  };
}

function forwardSidecarEvent(event: EventV1): void {
  if (["file.changed", "file.conflict", "diff.changed"].includes(event.event)) return;
  const window = mainWindow;
  if (!window || window.isDestroyed()) { sidecar?.failRendererDisconnected(); return; }
  eventCredit.deliver(event, () => window.webContents.send(M5_INTERNAL_CHANNELS_V1.sidecarEvent, event));
}

function forwardProjectEvent(event: WatchEventV1, protocolEvent: EventV1): void {
  const window = mainWindow;
  if (!window || window.isDestroyed()) { sidecar?.failRendererDisconnected(); return; }
  eventCredit.deliver(protocolEvent, () => window.webContents.send(M5_BRIDGE_V2_CHANNELS.projectEvent, { event, streamId: protocolEvent.streamId, seq: protocolEvent.seq }));
}

function forwardWorkspaceEvent(event: Parameters<SidecarClientV1["onWorkspaceEvent"]>[0] extends (event: infer T) => unknown ? T : never): void {
  const window = mainWindow;
  if (!window || window.isDestroyed()) { sidecar?.failRendererDisconnected(); return; }
  eventCredit.deliver(event, () => window.webContents.send(M5_BRIDGE_V3_CHANNELS.workspaceEvent, { event, streamId: event.streamId, seq: event.seq }));
}

function forwardChatEvent(event: Parameters<SidecarClientV1["onChatEvent"]>[0] extends (event: infer T, protocol: infer P) => unknown ? T : never, protocolEvent: EventV1): void {
  const window = mainWindow;
  if (!window || window.isDestroyed()) { sidecar?.failRendererDisconnected(); return; }
  eventCredit.deliver(protocolEvent, () => window.webContents.send(M5_BRIDGE_V3_CHANNELS.chatEvent, { event, streamId: protocolEvent.streamId, seq: protocolEvent.seq }));
}

function forwardAgentPublicEvent(event: AgentPublicEventV1, protocolEvent: EventV1): void {
  const window = mainWindow;
  if (!window || window.isDestroyed()) { sidecar?.failRendererDisconnected(); return; }
  eventCredit.deliver(protocolEvent, () => window.webContents.send(
    M5_BRIDGE_V4_CHANNELS.agentPublicEvent,
    { event, streamId: protocolEvent.streamId, seq: protocolEvent.seq },
  ));
}

function forwardAgentRunStateChanged(event: AgentRunStateChangedV1, protocolEvent: EventV1): void {
  const window = mainWindow;
  if (!window || window.isDestroyed()) { sidecar?.failRendererDisconnected(); return; }
  eventCredit.deliver(protocolEvent, () => window.webContents.send(
    M5_BRIDGE_V5_CHANNELS.agentRunStateChanged,
    { event, streamId: protocolEvent.streamId, seq: protocolEvent.seq },
  ));
}

let storageSealStore: StorageSealStore | undefined;
/** When the last full storage check ran: this start, unless it began from a
 * seal (then the seal's). A refused seal makes the engine check in full
 * anyway, so an older time here only brings the next full check sooner. */
let lastFullCheckAt = Date.now();
function storageSeals(): StorageSealStore {
  storageSealStore ??= new StorageSealStore(app.getPath("userData"), m5DataDirectory(), app.getVersion(), {
    available: () => safeStorage.isEncryptionAvailable(),
    encrypt: text => safeStorage.encryptString(text),
    decrypt: data => safeStorage.decryptString(data),
  });
  return storageSealStore;
}

function startSidecar(): SidecarClientV1 {
  const diagnosticLog = createSessionDiagnosticLog(app.getPath("logs"));
  const transportLog = createSessionDiagnosticLog(app.getPath("logs"), "transport");
  const launch = sidecarLaunch(m5DataDirectory());
  // Used once: read and removed here, so a crash after this leaves no seal.
  const seal = storageSeals().consume();
  lastFullCheckAt = seal?.fullCheckAt ?? Date.now();
  if (seal) launch.args = [...launch.args, "--storage-seal", seal.token];
  const client = new SidecarClientV1(launch, {
    onDiagnosticTail: diagnosticLog.capture,
    onTransportProgress: transportLog.captureTransport,
  });
  client.onEvent(forwardSidecarEvent);
  client.onProjectEvent(forwardProjectEvent);
  client.onWorkspaceEvent(forwardWorkspaceEvent);
  client.onChatEvent(forwardChatEvent);
  client.onAgentPublicEvent(forwardAgentPublicEvent);
  client.onAgentRunStateChanged(forwardAgentRunStateChanged);
  client.onAgentOutputAvailable((head, event) => {
    const window = mainWindow;
    if (!window || window.isDestroyed()) { sidecar?.failRendererDisconnected(); return; }
    eventCredit.deliver(event, () => window.webContents.send(M5_BRIDGE_V8_CHANNELS.agentOutputAvailable,
      { event: head, streamId: event.streamId, seq: event.seq }));
  });
  watchEngineFailure(client);
  // A build release the stopped engine never received is sent again: the
  // new engine restored that hold (ADE stabilization A).
  void client.start().then(() => { if (client === sidecar) previewBuildManager.engineReady(); }).catch(() => {
    // getSystemStatus exposes a structured unavailable state. A failure after
    // the engine once worked is restarted by `watchEngineFailure`.
  });
  return client;
}

// An engine that stops while in use is started again rather than asking the
// person to quit and reopen the app. Bounded so a failure that repeats right
// away is not retried forever; the screen then shows that the engine stopped.
const ENGINE_RESTART_WINDOW_MS = 5 * 60_000;
const ENGINE_RESTART_LIMIT = 3;
const engineRestarts: number[] = [];

function watchEngineFailure(client: SidecarClientV1): void {
  client.onFailure(code => {
    const now = Date.now();
    while (engineRestarts.length && now - engineRestarts[0]! > ENGINE_RESTART_WINDOW_MS) engineRestarts.shift();
    const restart = client === sidecar && !quitCoordinator.exitAllowed && !client.failedBeforeReady
      && engineRestarts.length < ENGINE_RESTART_LIMIT;
    void recordEngineFailure(client, code, restart);
    if (!restart) return;
    engineRestarts.push(now);
    void (async () => {
      // Never two engines on one database: wait until the old one has ended.
      if (!(await client.waitForExit(10_000))) return;
      if (client !== sidecar || quitCoordinator.exitAllowed) return;
      eventCredit.reset();
      // The screen notices the new engine by itself and refreshes when nothing
      // unsaved would be lost (see the admission poller). A run the old engine
      // left unfinished then shows up as needing recovery.
      sidecar = startSidecar();
    })();
  });
}

/** Fixed labels only: the failure code, exit code/signal and the code location
 * of a Rust panic. No paths, payloads, provider output or credentials. */
async function recordEngineFailure(client: SidecarClientV1, code: string, restarting: boolean): Promise<void> {
  await client.waitForExit(2_000);
  // The engine's own last words, and the app's reason when it is the app that
  // refused something (for example a protocol budget on its side).
  const detail = `${client.technicalDetail}\n${client.failure?.message ?? ""}`;
  const panic = /panicked at ((?:crates|src)\/[\w./-]+\.rs:\d+:\d+)/.exec(detail)?.[1] ?? null;
  // Fixed engine messages only (protocol budgets, ordering, quit): never paths or payloads.
  const reason = [...detail.matchAll(/(session budget exceeded|expected seq \d+, got \d+|was reused within a session|stdin closed before app\.prepareQuit|stdin reader stopped|storage maintenance (?:is )?poisoned)/g)].at(-1)?.[1] ?? null;
  const body = JSON.stringify({ schemaVersion: 1, at: new Date().toISOString(), code, exit: client.exitDetail, panic, reason, restarting }) + "\n";
  try {
    const directory = app.getPath("logs");
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    writeFileSync(join(directory, "last-engine-failure.json"), body, { mode: 0o600 });
  } catch { /* diagnostics never block the app */ }
  console.error(`m5: engine stopped (${code}, exit ${client.exitDetail ?? "unknown"}${panic ? `, panic at ${panic}` : ""}${reason ? `, ${reason}` : ""})${restarting ? ", restarting" : ""}`);
}

async function getSystemStatus(): Promise<SystemStatusV1 | SystemStatusG5DV1> {
  // The renderer can request status before loadURL resolves. Start exactly once
  // here rather than permanently presenting that startup race as unavailable.
  if (!sidecar) sidecar = startSidecar();
  try {
    const client = sidecar;
    await client.start();
    const status = await client.getSystemStatus();
    if ("storageAdmission" in status && status.storageAdmission.state === "failed") recordAdmissionFailure(client);
    return status;
  } catch (error) {
    return unavailableStatus(error);
  }
}

const admissionFailuresRecorded = new WeakSet<SidecarClientV1>();

/** The engine refused to open the app data. It keeps running, so nothing else
 * records why: keep its fixed failure kind (never paths or payloads). */
function recordAdmissionFailure(client: SidecarClientV1): void {
  if (admissionFailuresRecorded.has(client)) return;
  admissionFailuresRecorded.add(client);
  // Its stderr line travels on another pipe than the status answer.
  setTimeout(() => writeAdmissionFailure(client), 500);
}

function writeAdmissionFailure(client: SidecarClientV1): void {
  const reason = [...client.technicalDetail.matchAll(/storage admission failed: (\w+)/g)].at(-1)?.[1] ?? null;
  const body = JSON.stringify({ schemaVersion: 1, at: new Date().toISOString(), code: "storageAdmissionFailed", exit: null, panic: null, reason, restarting: false }) + "\n";
  try {
    const directory = app.getPath("logs");
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    // Its own file: a restart after a crash must not overwrite why the engine first stopped.
    writeFileSync(join(directory, "last-engine-admission-failure.json"), body, { mode: 0o600 });
  } catch { /* diagnostics never block the app */ }
  console.error(`m5: engine could not open the app data (${reason ?? "unknown"})`);
}

async function prepareQuit(): Promise<PrepareQuitResultV1> {
  // Every quit preparation (an update install included, which does not pass
  // through `before-quit`) closes the preview and stops its reads first; a
  // preparation called off lets it open again (PR #6 review I4).
  previewManager.suspend();
  previewBuildManager.suspend();
  await gitPreparation?.close();
  try {
    const result = await quitCoordinator.prepareQuit();
    if (!result.safeToQuit) { gitPreparation?.reopen(); previewManager.resume(); previewBuildManager.resume(); }
    return result;
  } catch (error) { gitPreparation?.reopen(); previewManager.resume(); previewBuildManager.resume(); throw error; }
}

function registerIpcHandlers(): void {
  ipcMain.handle("m5:app-update:readiness-reply", async (event, nonce: unknown, ready: unknown, ...extra: unknown[]) => {
    assertCurrentMainFrame(event);
    if (extra.length || typeof nonce !== "string" || typeof ready !== "boolean" || nonce !== updateReadiness?.nonce) throw new Error("invalid update readiness reply");
    updateReadiness.finish(ready);
  });
  // Screen brightness (system / light / dark). main's record is the one the
  // settings screen starts from (PR #10 review I1); the page sends a new
  // choice, main follows it for native parts (dialogs, the window background)
  // and remembers it for the next launch. Only the main frame of this app,
  // one fixed value; the answer says whether it was remembered.
  ipcMain.handle(APPEARANCE_GET_CHANNEL, async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 0) throw new Error("appearance takes no input");
    return { theme: rememberedTheme ?? "system", saved: rememberedTheme !== null, dark: nativeTheme.shouldUseDarkColors };
  });
  ipcMain.handle(APPEARANCE_CHANNEL, async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 1 || !isThemePreference(args[0])) throw new Error("invalid appearance");
    if (nativeTheme.themeSource !== args[0]) nativeTheme.themeSource = args[0];
    followNativeTheme();
    const saved = writeThemePreference(app.getPath("userData"), args[0]);
    if (saved) rememberedTheme = args[0];
    return { dark: nativeTheme.shouldUseDarkColors, saved };
  });
  // The in-app update banner (ADE U8, Q7). The page reads main's state and may
  // ask for a check or a restart. A restart is never the page's call: main asks
  // in its own native box, then the existing safe-quit checks still decide.
  ipcMain.handle("m5:app-update:state", async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 0) throw new Error("app update state takes no input");
    return appUpdate?.state() ?? "unavailable";
  });
  ipcMain.handle("m5:app-update:request", async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 1 || typeof args[0] !== "string" || !APP_UPDATE_REQUESTS.has(args[0])) throw new Error("invalid app update request");
    if (!appUpdate) throw new Error("app update is unavailable");
    if (args[0] === "check") { appUpdate.check(); return appUpdate.state(); }
    if (appUpdate.state() !== "ready") throw new Error("no downloaded update to install");
    const ko = app.getLocale().toLowerCase().startsWith("ko");
    const answer = await dialog.showMessageBox(mainWindow!, { type: "question", message: ko ? "업데이트를 적용하고 다시 시작할까요?" : "Install the update and restart?",
      detail: ko ? "편집 중인 내용을 먼저 저장하세요. 진행 중인 작업이 안전하게 멈추는 것을 확인할 수 없으면 다시 시작하지 않아요." : "Save your edits first. The app does not restart unless running work can be confirmed to stop safely.",
      buttons: ko ? ["취소", "다시 시작"] : ["Cancel", "Restart"], defaultId: 0, cancelId: 0 });
    if (answer.response === 1 && appUpdate.state() === "ready") await appUpdate.restart();
    return appUpdate.state();
  });
  ipcMain.handle("m5:app-update:open", async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 0) throw new Error("app update takes no input");
    const item = Menu.getApplicationMenu()?.getMenuItemById("elsecrew-app-update");
    if (!item?.enabled) throw new Error("app update is unavailable");
    await item.click(undefined, undefined, undefined);
  });
  // Result preview: the page names a chat, never a workspace, token or URL.
  ipcMain.handle(PREVIEW_CHANNEL_V1, quietAfterQuit(async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 1) throw new Error("preview takes one request");
    return previewManager.request(validatePreviewIpcRequestV1(args[0]));
  }));
  // Build-then-preview: the page names a chat and a step; main asks the
  // sidecar for the plan and the person in its own box (preview-build.ts).
  // Processes a build left when the app last ended without stopping them.
  if (previewBuildManager) { try { previewBuildManager.recoverLeftovers(); } catch { /* best effort */ } }
  ipcMain.handle(PREVIEW_BUILD_CHANNEL_V1, quietAfterQuit(async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 1) throw new Error("preview build takes one request");
    return previewBuildManager.request(validatePreviewBuildIpcRequestV1(args[0]));
  }));
  ipcMain.handle(SESSION_DIAGNOSTICS_CHANNEL, async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 0) throw new Error("session diagnostics takes no input");
    return classifySessionDiagnostics(sidecar?.technicalDetail ?? "");
  });
  ipcMain.handle(INSTALL_CHANNEL, quietAfterQuit(async (event, input: unknown) => {
    assertCurrentMainFrame(event);
    if (!sidecar) throw new Error("sidecar was not started");
    const request = validateInstallRequest(input);
    // Installing writes outside the app (PR #14 review B1). A page request is
    // not the person's consent: main reads the plan that awaits consent and
    // asks in its own native box; only "Install" there starts it. The page
    // can neither name the folders nor answer this box.
    if (request.action === "start") {
      const current = validateInstallStatus(await sidecar.install({ action: "status" }));
      const plan = current.state === "awaitingConsent" ? current.plan : null;
      if (!plan || plan.planId !== request.planId || plan.provider !== request.provider) throw new Error("installation plan is not awaiting consent");
      const ko = app.getLocale().toLowerCase().startsWith("ko");
      const name = plan.provider === "codex" ? "Codex" : "Claude Code";
      const answer = await dialog.showMessageBox(mainWindow!, { type: "question",
        message: ko ? `${name}을(를) 설치·업데이트할까요?` : `Install or update ${name}?`,
        detail: `${ko ? "공식 배포 경로로 설치하고, 아래 폴더의 기존 명령이 바뀔 수 있어요. 셸 설정 파일은 바꾸지 않아요." : "It installs from the official source; the existing commands in these folders may change. Shell settings files are not changed."}\n\n${plan.writeDirectories.join("\n")}`,
        buttons: ko ? ["취소", "설치·업데이트"] : ["Cancel", "Install or update"], defaultId: 0, cancelId: 0 });
      if (answer.response !== 1) return current;
    }
    return validateInstallStatus(await sidecar.install(request));
  }));
  const claudeUsage = createClaudeUsageReader(m5DataDirectory(), net.fetch as typeof fetch, async () => {
    if (!sidecar) return false;
    const attemptId = randomUUID();
    try {
      // Existing login admission/fence serializes renewal with Claude work.
      let status = validateLoginStatus(await sidecar.login({provider:"claude", action:"check", attemptId}), "claude");
      const deadline = Date.now() + 30_000;
      while (status.attemptId === attemptId && ["starting", "verifying"].includes(status.state) && Date.now() < deadline) {
        await new Promise(resolve => setTimeout(resolve, 200));
        status = validateLoginStatus(await sidecar.login({provider:"claude", action:"status"}), "claude");
      }
      return status.attemptId === attemptId && status.state === "connected" && status.error === null;
    } catch { return false; }
  });
  ipcMain.handle(CLAUDE_USAGE_CHANNEL, quietAfterQuit(async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length) throw new Error("usage takes no input");
    if (!sidecar) throw new Error("sidecar was not started");
    const before = await sidecar.projectCatalog({action:"getProviderConnections"});
    if (before.action !== "getProviderConnections" || !before.connections["claude-code"]) return {state:"notConnected",usage:null,checkedAt:Date.now()};
    const result = await claudeUsage.read();
    const after = await sidecar.projectCatalog({action:"getProviderConnections"});
    return validateClaudeUsageResult(after.action === "getProviderConnections" && after.connections["claude-code"] ? result : {state:"notConnected",usage:null,checkedAt:Date.now()});
  }));
  gitPreparation = new GitPreparationClient(sidecarLaunch(m5DataDirectory()));
  const gitSelection = new GitSelectionStore(m5DataDirectory(), process.platform, app.getPath("userData"));
  ipcMain.handle(GIT_SELECTION_CHANNEL, async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 1) throw new Error("Git selection takes one action");
    const action = validateGitSelectionAction(args[0]);
    return validateGitSelection(await gitSelection.request(action, async () => {
      const result = await dialog.showOpenDialog(mainWindow!, {
        properties: ["openFile"], filters: [{ name: "Git", extensions: ["exe"] }],
      });
      assertCurrentMainFrame(event);
      return result.canceled || result.filePaths.length !== 1 ? null : result.filePaths[0]!;
    }));
  });
  ipcMain.handle(DEVICE_REPORT_CHANNEL, async (event, ...args: unknown[]) => {
    assertCurrentMainFrame(event);
    if (args.length !== 0) throw new Error("device report takes no input");
    return readDeviceReport(app.getPath("userData"));
  });
  ipcMain.handle(SAVE_RECOVERY_RESOLVE_CHANNEL, async (event, input: unknown) => {
    assertCurrentMainFrame(event);
    if (!sidecar) throw new Error("sidecar was not started");
    const request = validateSaveRecoveryResolveRequest(input);
    return validateSaveRecoveryResolveResult(await sidecar.resolveWorkspaceSaveRecovery(request), request);
  });
  ipcMain.handle(SAVE_RECOVERY_CHANNEL, async (event, input: unknown) => {
    assertCurrentMainFrame(event);
    if (!sidecar) throw new Error("sidecar was not started");
    const request = validateSaveRecoveryRequest(input);
    return validateSaveRecoveryResult(await sidecar.inspectWorkspaceSaveRecovery(request), request);
  });
  ipcMain.handle(PROJECT_CATALOG_CHANNEL, quietAfterQuit(async (event, input: unknown) => {
    assertCurrentMainFrame(event);
    const request = validateProjectCatalogRequest(input);
    if (process.platform === "win32" && isGitPreparation(request)) {
      if (!WINDOWS_GIT_PREPARATION_ENABLED) throw new Error("Git preparation has not been activated");
      return gitPreparation!.request(request, () => assertCurrentMainFrame(event));
    }
    if (!sidecar) throw new Error("sidecar was not started");
    // The folder for new task worktrees is chosen here, never typed by the page.
    if (request.action === "setWorkspaceFolder") throw new Error("the workspace folder is chosen with a dialog");
    if (request.action === "chooseWorkspaceFolder") {
      const picked = await dialog.showOpenDialog(mainWindow!, { properties: ["openDirectory", "createDirectory"] });
      assertCurrentMainFrame(event);
      const next = picked.canceled || picked.filePaths.length !== 1
        ? { action: "getWorkspaceFolder" as const }
        : { action: "setWorkspaceFolder" as const, path: realpathSync(picked.filePaths[0]!) };
      const saved = validateProjectCatalogResult(await sidecar.projectCatalog(next), next);
      return { ...saved, action: "chooseWorkspaceFolder" };
    }
    const result = validateProjectCatalogResult(await sidecar.projectCatalog(request), request);
    if (request.action === "setProviderConnection") claudeUsage.invalidate();
    return result;
  }));
  // A link in a team report: checked again here, shown in full in a native
  // confirmation (the page cannot answer it), and only then opened outside.
  ipcMain.handle(EXTERNAL_LINK_CHANNEL, quietAfterQuit(async (event, input: unknown) => {
    assertCurrentMainFrame(event);
    const url = validateExternalLink(input);
    if (!url || !mainWindow) throw new Error("invalid external link");
    const ko = app.getLocale().toLowerCase().startsWith("ko");
    const confirmation = await dialog.showMessageBox(mainWindow, {
      type: "question", buttons: ko ? ["열기", "취소"] : ["Open", "Cancel"], defaultId: 1, cancelId: 1, noLink: true,
      message: ko ? "이 주소를 브라우저에서 열까요?" : "Open this address in your browser?",
      detail: url,
    });
    if (confirmation.response !== 0) return { opened: false };
    await shell.openExternal(url);
    return { opened: true };
  }));
  ipcMain.handle(APPROVAL_CHANNEL, quietAfterQuit(async (event, input: unknown) => {
    assertCurrentMainFrame(event);
    if (!sidecar) throw new Error("sidecar was not started");
    const request = validateApprovalRequest(input);
    return validateApprovalResult(await sidecar.approval(request), request);
  }));
  let lastClaudeConnectedAttempt: string | null = null;
  ipcMain.handle(LOGIN_CHANNEL, quietAfterQuit(async (event, input: unknown) => {
    assertCurrentMainFrame(event);
    if (!sidecar) throw new Error("sidecar was not started");
    const request = validateLoginRequest(input);
    if (request.action === "start" || request.action === "check" || request.action === "submitCode") {
      const connected = await sidecar.projectCatalog({action:"getProviderConnections"});
      const runtime = request.provider === "claude" ? "claude-code" : "codex";
      if (connected.action !== "getProviderConnections" || !connected.connections[runtime]) throw new Error("providerDisconnected");
    }
    if (request.provider === "claude" && !["status", "diagnose"].includes(request.action)) claudeUsage.invalidate();
    const result = await sidecar.login(request)
      .then(value => validateLoginStatus(value, request.provider))
      .catch((error: unknown) => { throw new Error(classifyLoginFailure(error)); });
    if (request.provider === "claude" && result.state === "connected" && result.attemptId !== lastClaudeConnectedAttempt) {
      lastClaudeConnectedAttempt = result.attemptId;
      claudeUsage.invalidate();
    }
    if (result.authUrl && request.action !== "cancel") {
      try { await shell.openExternal(result.authUrl); }
      catch {
        if (result.attemptId) await sidecar.login({action:"cancel",attemptId:result.attemptId,...(request.provider ? {provider:request.provider} : {})});
        throw new Error("login browser could not be opened");
      }
    }
    return { ...result, authUrl: null }; // OAuth URL never reaches renderer or persistent state.
  }));
  ipcMain.handle(M5_INTERNAL_CHANNELS_V1.systemGetStatus, async (event) => {
    assertCurrentMainFrame(event);
    return getSystemStatus();
  });
  ipcMain.handle(M5_INTERNAL_CHANNELS_V1.appPrepareQuit, async (event) => {
    assertCurrentMainFrame(event);
    return prepareQuit();
  });
  ipcMain.handle(M5_BRIDGE_V6_CHANNELS.storageGetStatus, async (event, input: StorageStatusRequestV1) => {
    assertCurrentMainFrame(event);
    if (!sidecar) throw new Error("sidecar was not started");
    return sidecar.getStorageStatus(input);
  });
  ipcMain.on(M5_BRIDGE_V2_CHANNELS.eventAck, (event, ack) => {
    assertCurrentMainFrame(event);
    eventCredit.acknowledge(ack);
  });
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.projectOpen, async (event) => {
    assertCurrentMainFrame(event);
    const picked = takeDevPickedDirectory();
    const result = picked ? { canceled: false, filePaths: [picked] } : await dialog.showOpenDialog(mainWindow!, { properties: ["openDirectory"] });
    if (result.canceled || result.filePaths.length !== 1) return { outcome: "cancelled" };
    if (!sidecar) throw new Error("sidecar was not started");
    const selected = realpathSync(result.filePaths[0]!);
    if (process.platform === "darwin" && readdirSync(selected).length === 0) {
      const identity = statSync(selected, { bigint: true });
      const ko = app.getLocale().toLowerCase().startsWith("ko");
      const confirmation = picked ? { response: 0 } : await dialog.showMessageBox(mainWindow!, {
        type: "question",
        message: ko ? "이 빈 폴더에서 새 프로젝트를 시작할까요?" : "Start a new project in this empty folder?",
        detail: `${selected}\n\n${ko ? "변경 내용을 비교하고 되돌릴 수 있도록 이 폴더에 로컬 기록(.git)을 만듭니다. 온라인에 업로드하지 않으며, 에이전트는 별도의 작업 복사본에서 작업합니다." : "Create local history (.git) in this folder so changes can be compared and restored. Nothing is uploaded. Agents work in a separate copy."}`,
        buttons: ko ? ["프로젝트 시작", "취소"] : ["Start project", "Cancel"], defaultId: 0, cancelId: 1,
      });
      if (confirmation.response !== 0) return { outcome: "cancelled" };
      return sidecar.openProject(selected, `${identity.dev}:${identity.ino}`);
    }
    try {
      return await sidecar.openProject(selected);
    } catch (error) {
      // A plain folder with files (user decision 2026-11-19 "링크 파일 방식"):
      // keep its history in app data, leaving only a small .git file here.
      const message = error instanceof Error ? error.message : String(error);
      if (process.platform !== "darwin" || !/not inside a supported Git repository/.test(message)) throw error;
      const identity = statSync(selected, { bigint: true });
      const ko = app.getLocale().toLowerCase().startsWith("ko");
      // Its earlier record is gone or another folder's: say what is lost.
      const again = /record is missing or belongs to another folder/.test(message);
      const confirmation = picked ? { response: 0 } : again ? await dialog.showMessageBox(mainWindow!, {
        type: "warning",
        message: ko ? "이 폴더의 이전 기록을 찾을 수 없어요" : "This folder's earlier record was not found",
        detail: `${selected}\n\n${ko ? "기록이 지워졌거나, 다른 폴더를 복사한 폴더일 수 있어요. 새로 기록하면 지금 폴더 상태부터 다시 시작하고, 이 폴더에서 진행 중이던 작업은 이어지지 않아요. 폴더의 파일은 바꾸지 않아요." : "The record was removed, or this folder is a copy of another one. Recording anew starts from the folder as it is now; tasks in progress here will not continue. No file in the folder changes."}`,
        buttons: ko ? ["새로 기록하고 시작", "취소"] : ["Record anew and start", "Cancel"], defaultId: 1, cancelId: 1,
      }) : await dialog.showMessageBox(mainWindow!, {
        type: "question",
        message: ko ? "이 폴더로 프로젝트를 시작할까요?" : "Start a project with this folder?",
        detail: `${selected}\n\n${ko ? "변경 내용을 비교하고 되돌릴 수 있도록 지금 폴더 상태를 앱 안에 기록해 둡니다. 폴더에는 이 기록을 가리키는 작은 .git 파일 하나만 생기고, 다른 파일은 바꾸지 않아요. 온라인에 업로드하지 않으며, 에이전트는 별도의 작업 공간에서 작업해요." : "The folder's current state is recorded inside the app so changes can be compared and restored. Only a small .git file pointing to that record is added here; no other file changes. Nothing is uploaded. Agents work in a separate workspace."}`,
        buttons: ko ? ["프로젝트 시작", "취소"] : ["Start project", "Cancel"], defaultId: 0, cancelId: 1,
      });
      if (confirmation.response !== 0) return { outcome: "cancelled" };
      return sidecar.openProject(selected, undefined, `${identity.dev}:${identity.ino}`);
    }
  });
  const invoke = <T>(action: (client: SidecarClientV1, input: T) => Promise<unknown>) =>
    quietAfterQuit(async (event: Electron.IpcMainInvokeEvent, input: T) => {
      assertCurrentMainFrame(event);
      if (!sidecar) throw new Error("sidecar was not started");
      return action(sidecar, input);
    });
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.projectGetTree, invoke((client, input: { projectId: string; parentPath: string; cursor: string | null }) => client.getProjectTree(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.projectCreateWorktree, invoke((client, input: { projectId: string }) => client.createProjectWorktree(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.projectClose, invoke((client, input: { projectId: string }) => client.closeProject(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.fileRead, invoke((client, input: { projectId: string; path: string }) => client.readFile(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.fileSaveBegin, invoke((client, input: Omit<SaveBeginV1, "saveId" | "chunkBytes">) => client.beginFileSave(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.fileSaveChunk, invoke((client, input: { saveId: string; seq: number; text: string }) => client.appendFileSaveChunk(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.fileSaveCommit, invoke((client, input: { saveId: string; expectedVersion: `sha256:${string}`; lastSeq: number; totalBytes: number }) => client.commitFileSave(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.fileWatch, invoke((client, input: { projectId: string }) => client.watchProjectFiles(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.diffGetSummary, invoke((client, input: { projectId: string }) => client.getDiffSummary(input)));
  ipcMain.handle(M5_BRIDGE_V2_CHANNELS.diffGetFile, invoke((client, input: { projectId: string; path: string }) => client.getDiffFile(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceGetTree, invoke((client, input: WorkspaceTreeRequestV1) => client.getWorkspaceTree(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceClose, invoke((client, input: ChatWorkspaceRefV1) => client.closeWorkspace(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceFileRead, invoke((client, input: WorkspaceFileReadRequestV1) => client.readWorkspaceFile(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceFileSaveBegin, invoke((client, input: WorkspaceSaveBeginRequestV1) => client.beginWorkspaceFileSave(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceFileSaveChunk, invoke((client, input: WorkspaceSaveChunkRequestV1) => client.appendWorkspaceFileSaveChunk(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceFileSaveCommit, invoke((client, input: WorkspaceSaveCommitRequestV1) => client.commitWorkspaceFileSave(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceFileWatch, invoke((client, input: ChatWorkspaceRefV1) => client.watchWorkspaceFiles(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceDiffGetSummary, invoke((client, input: ChatWorkspaceRefV1) => client.getWorkspaceDiffSummary(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceDiffGetFile, invoke((client, input: WorkspaceDiffFileRequestV1) => client.getWorkspaceDiffFile(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspacePrepareChat, invoke((client, input: PrepareChatWorkspaceRequestV1) => client.prepareChatWorkspace(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.workspaceAbandonPrepared, invoke((client, input: AbandonPreparedWorkspaceRequestV1) => client.abandonPreparedWorkspace(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.projectGetRoleSettings, invoke((client, input: { projectId: string }) => client.getProjectRoleSettings(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.projectUpdateRoleSettings, invoke((client, input: ProjectRoleSettingsUpdateRequestV1) => client.updateProjectRoleSettings(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.chatCreate, invoke((client, input: ChatCreateRequestV1) => client.createChat(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.chatList, invoke((client, input: ChatListRequestV1) => client.listChats(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.chatGet, invoke((client, input: ChatGetRequestV1) => client.getChat(input)));
  ipcMain.handle(M5_BRIDGE_V7_CHANNELS.chatAppendMessage, invoke((client, input: ChatAppendMessageRequestV1) => client.appendChatMessage(input)));
  ipcMain.handle(M5_BRIDGE_V8_CHANNELS.agentOutputHead, invoke((client, input: AgentRunBindingV1) => client.getAgentOutputHead(input)));
  ipcMain.handle(M5_BRIDGE_V8_CHANNELS.agentOutputPage, invoke((client, input: OutputPageRequest) => client.getAgentOutputPage(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.runStartFixture, invoke((client, input: FixtureRunStartRequestV1) => client.startFixtureRun(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.runCancel, invoke((client, input: RunCancelRequestV1) => client.cancelRun(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.recoveryList, invoke((client, input: PageRequestV1) => client.listRecoveries(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.recoveryInspect, invoke((client, input: { recoveryId: string }) => client.inspectRecovery(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.recoveryMarkResolved, invoke((client, input: RecoveryMarkResolvedRequestV1) => client.markRecoveryResolved(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentGetCapabilities, invoke((client, input: AgentCapabilitiesRequestV1) => client.getAgentCapabilities(input)));
  ipcMain.handle(MODEL_CHOICES_CHANNEL, invoke((client, input: unknown) => client.getModelChoices(validateModelChoicesRequest(input))));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentStartRun, invoke((client, input: AgentRunStartRequestV1) => client.startAgentRun(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentCancelRun, invoke((client, input: AgentRunCancelRequestV1) => client.cancelAgentRun(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentRecoveryInspect, invoke((client, input: AgentRecoveryInspectRequestV1) => client.inspectAgentRecovery(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentRecoveryResolve, invoke((client, input: AgentRecoveryResolveRequestV1) => client.resolveAgentRecovery(input)));
}

async function createMainWindow() {
  const window = new BrowserWindow({
    width: 1280,
    height: 800,
    // The saved brightness choice paints the first frame before the page loads.
    backgroundColor: windowBackground(nativeTheme.shouldUseDarkColors),
    minWidth: 640,
    minHeight: 640,
    webPreferences: {
      preload: join(__dirname, "preload.cjs"),
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
    },
  });

  mainWindow = window;
  secureWindow(window);
  let startupReported = false;
  window.webContents.on("console-message", (event) => {
    // Forward only the fixed readiness marker, never arbitrary renderer logs.
    // This observation grants no IPC, execution or shutdown authority.
    if (startupReported || event.message !== "m5:start-screen-ready" ||
      event.frame !== window.webContents.mainFrame || !isAllowedNavigation(event.frame.url)) return;
    startupReported = true;
    console.info("m5:start-screen-ready");
  });
  window.on("closed", () => {
    if (mainWindow !== window) return;
    previewManager.windowClosed();
    previewBuildManager.stopAll();
    mainWindow = undefined;
    if (quitCoordinator.exitAllowed) return;
    if (eventCredit.outstanding !== 0) sidecar?.failRendererDisconnected();
    else eventCredit.resetIfEmpty();
  });
  // A page that reloads or navigates never acknowledges what was sent to the
  // previous page, and nothing can listen until the new one has loaded: hold
  // deliveries in between and start the credit afresh. The page reads its
  // state anew, so what was not delivered meanwhile is not lost.
  window.webContents.on("did-start-navigation", (details: { isMainFrame?: boolean; isSameDocument?: boolean }) => {
    if (details?.isMainFrame && !details.isSameDocument) {
      eventCredit.pause();
      // The new page never learns the old preview: close it and any open on its way.
      previewManager.pageReplaced();
      previewBuildManager.stopAll();
    }
  });
  window.webContents.on("render-process-gone", () => { previewManager.pageReplaced(); previewBuildManager.stopAll(); });
  // A window about to change size hides the preview first; it shows again
  // only where the page measures after the change (PR #9 re-review B1).
  window.on("will-resize", () => previewManager.geometryChanged());
  window.on("resize", () => previewManager.geometryChanged());
  window.on("enter-full-screen", () => previewManager.geometryChanged());
  window.on("leave-full-screen", () => previewManager.geometryChanged());
  // A minimized or hidden window hides the preview view drawn over it.
  window.on("minimize", () => previewManager.setWindowHidden(true));
  window.on("hide", () => previewManager.setWindowHidden(true));
  window.on("restore", () => previewManager.setWindowHidden(false));
  window.on("show", () => previewManager.setWindowHidden(false));
  window.webContents.on("did-finish-load", () => eventCredit.resume());
  window.webContents.on("did-fail-load", () => eventCredit.resume());
  if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
    await window.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL);
  } else {
    await window.loadURL(`${APP_ORIGIN}/index.html`);
  }
}

/** The View menu's zoom: the preview is hidden before the page's zoom
 * changes (PR #9 re-review B1). */
function zoomPage(step: ZoomStep): void {
  if (!mainWindow || mainWindow.isDestroyed()) return;
  previewManager.geometryChanged();
  const contents = mainWindow.webContents;
  contents.setZoomLevel(zoomLevelAfter(contents.getZoomLevel(), step));
}

/** The same order for a given factor (the preview UI smoke's zoom levels). */
function setPageZoomFactor(factor: number): void {
  if (!mainWindow || mainWindow.isDestroyed() || !Number.isFinite(factor) || factor <= 0) return;
  previewManager.geometryChanged();
  mainWindow.webContents.setZoomFactor(factor);
}

// Fresh Elsecrew profile: do not import the retired application data.
const existingUserData = app.getPath("userData");
app.setName("Elsecrew");
app.setPath("userData", existingUserData);
// The remembered brightness; null when there is no readable record.
const startingTheme = readThemePreference(existingUserData);
let rememberedTheme: ThemePreference | null = startingTheme.saved ? startingTheme.theme : null;
nativeTheme.themeSource = startingTheme.theme;
/** The window background follows the theme in use, also when the system changes it (review I2). */
function followNativeTheme(): void {
  if (mainWindow && !mainWindow.isDestroyed()) mainWindow.setBackgroundColor(windowBackground(nativeTheme.shouldUseDarkColors));
}
nativeTheme.on("updated", followNativeTheme);

app.whenReady().then(async () => {
  await protocol.handle(APP_SCHEME, serveAppAsset);
  registerIpcHandlers();
  installZoomGuardedMenu(zoomPage);
  appUpdate = installAppUpdateMenu(async (install) => {
    if (!await rendererReadyForUpdate()) {
      const ko = app.getLocale().toLowerCase().startsWith("ko");
      await dialog.showMessageBox({ type: "info", message: ko ? "편집 중인 내용을 먼저 저장해 주세요." : "Please save your edits first.",
        detail: ko ? "열린 설정·새 대화를 정리한 뒤 다시 시도해 주세요. 화면 상태를 확인하지 못한 경우에도 업데이트를 적용하지 않아요." : "Finish open settings and new-chat drafts, then try again. Updates are also blocked when the editor state cannot be confirmed." });
      return false;
    }
    pendingUpdateInstall = install;
    let safe = false;
    try { safe = (await prepareQuit()).safeToQuit; return safe; }
    finally {
      pendingUpdateInstall = undefined;
      if (!safe && mainWindow && !mainWindow.isDestroyed()) mainWindow.webContents.send("m5:app-update:resume");
    }
  }, state => { if (mainWindow && !mainWindow.isDestroyed()) mainWindow.webContents.send("m5:app-update:changed", state); },
  () => { if (mainWindow && !mainWindow.isDestroyed()) mainWindow.webContents.send("m5:app-update:reveal"); });
  await createMainWindow();
  sidecar ??= startSidecar();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) void createMainWindow();
  });
});

app.on("before-quit", (event) => {
  if (gitPreparation?.pending) {
    event.preventDefault();
    void gitPreparation.close().then(() => app.quit());
    return;
  }
  if (quitCoordinator.exitAllowed) return;
  event.preventDefault();
  quitCoordinator.handleBeforeQuit();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

export {
  APP_ORIGIN,
  SIDECAR_READY_TIMEOUT_MS,
  assertCurrentMainFrame,
  m5DataDirectory,
  previewBuildManager,
  previewManager,
  setPageZoomFactor,
  sidecarLaunch,
  zoomPage,
};
import { M5_BRIDGE_V7_CHANNELS } from "./ipc-channels.ts";
import { M5_BRIDGE_V8_CHANNELS } from "./ipc-channels.ts";
import type { AgentRunBindingV1 } from "../src/m5/contracts/agent-adapter-v1.ts";
import type { OutputPageRequest } from "../src/m5/contracts/agent-output-pages.ts";
import type { ChatAppendMessageRequestV1 } from "../src/m5/contracts/run-recovery-v1.ts";
