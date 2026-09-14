import { app, BrowserWindow, dialog, ipcMain, net, protocol } from "electron";
import { mkdirSync, realpathSync, statSync } from "node:fs";
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
const DEV_FIXTURE_SIDECAR = resolve(__dirname, "..", "..", "scripts", "m5-sidecar-fixture.mjs");

let mainWindow: BrowserWindow | undefined;
let sidecar: SidecarClientV1 | undefined;
const eventCredit = new EventCreditV1((error) => sidecar?.failConsumerTooSlow());
const quitCoordinator = new QuitCoordinatorV1({
  prepareSidecarQuit: () => {
    if (!sidecar) return Promise.reject(new Error("sidecar was not started"));
    return sidecar.prepareQuit();
  },
  failedBeforeReady: () => sidecar?.failedBeforeReady ?? false,
  allowProcessExit: () => app.quit(),
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
]);

function rendererDirectory() {
  return resolve(__dirname, "..", "renderer", MAIN_WINDOW_VITE_NAME);
}

function m5DataDirectory(): string {
  const candidate = resolve(
    app.getPath("userData"),
    "com.wonseokhan.helloagent",
    "m5",
  );
  mkdirSync(candidate, { recursive: true });
  return realpathSync(candidate);
}

function sidecarLaunch(dataDirectory: string): SidecarLaunchV1 {
  if (app.isPackaged) {
    return {
      command: join(
        process.resourcesPath,
        "sidecar",
        `hello-agent-sidecar${process.platform === "win32" ? ".exe" : ""}`,
      ),
      args: ["--stdio"],
      cwd: dataDirectory,
    };
  }
  return {
    command: process.execPath,
    args: [DEV_FIXTURE_SIDECAR, "--stdio"],
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

  return net.fetch(pathToFileURL(asset).toString());
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

function startSidecar(): SidecarClientV1 {
  const client = new SidecarClientV1(sidecarLaunch(m5DataDirectory()));
  client.onEvent(forwardSidecarEvent);
  client.onProjectEvent(forwardProjectEvent);
  client.onWorkspaceEvent(forwardWorkspaceEvent);
  client.onChatEvent(forwardChatEvent);
  client.onAgentPublicEvent(forwardAgentPublicEvent);
  client.onAgentRunStateChanged(forwardAgentRunStateChanged);
  void client.start().catch(() => {
    // getSystemStatus exposes a structured unavailable state. There is no restart or retry.
  });
  return client;
}

async function getSystemStatus(): Promise<SystemStatusV1 | SystemStatusG5DV1> {
  if (!sidecar) return unavailableStatus(new Error("sidecar was not started"));
  try {
    await sidecar.start();
    return await sidecar.getSystemStatus();
  } catch (error) {
    return unavailableStatus(error);
  }
}

function prepareQuit(): Promise<PrepareQuitResultV1> {
  return quitCoordinator.prepareQuit();
}

function registerIpcHandlers(): void {
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
    const result = await dialog.showOpenDialog(mainWindow!, { properties: ["openDirectory"] });
    if (result.canceled || result.filePaths.length !== 1) return { outcome: "cancelled" };
    if (!sidecar) throw new Error("sidecar was not started");
    return sidecar.openProject(result.filePaths[0]!);
  });
  const invoke = <T>(action: (client: SidecarClientV1, input: T) => Promise<unknown>) =>
    async (event: Electron.IpcMainInvokeEvent, input: T) => {
      assertCurrentMainFrame(event);
      if (!sidecar) throw new Error("sidecar was not started");
      return action(sidecar, input);
    };
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
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.runStartFixture, invoke((client, input: FixtureRunStartRequestV1) => client.startFixtureRun(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.runCancel, invoke((client, input: RunCancelRequestV1) => client.cancelRun(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.recoveryList, invoke((client, input: PageRequestV1) => client.listRecoveries(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.recoveryInspect, invoke((client, input: { recoveryId: string }) => client.inspectRecovery(input)));
  ipcMain.handle(M5_BRIDGE_V3_CHANNELS.recoveryMarkResolved, invoke((client, input: RecoveryMarkResolvedRequestV1) => client.markRecoveryResolved(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentGetCapabilities, invoke((client, input: AgentCapabilitiesRequestV1) => client.getAgentCapabilities(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentStartRun, invoke((client, input: AgentRunStartRequestV1) => client.startAgentRun(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentCancelRun, invoke((client, input: AgentRunCancelRequestV1) => client.cancelAgentRun(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentRecoveryInspect, invoke((client, input: AgentRecoveryInspectRequestV1) => client.inspectAgentRecovery(input)));
  ipcMain.handle(M5_BRIDGE_V4_CHANNELS.agentRecoveryResolve, invoke((client, input: AgentRecoveryResolveRequestV1) => client.resolveAgentRecovery(input)));
}

async function createMainWindow() {
  const window = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 960,
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
  window.on("closed", () => {
    if (mainWindow !== window) return;
    mainWindow = undefined;
    if (quitCoordinator.exitAllowed) return;
    if (eventCredit.outstanding !== 0) sidecar?.failRendererDisconnected();
    else eventCredit.resetIfEmpty();
  });
  if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
    await window.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL);
  } else {
    await window.loadURL(`${APP_ORIGIN}/index.html`);
  }
}

app.whenReady().then(async () => {
  await protocol.handle(APP_SCHEME, serveAppAsset);
  registerIpcHandlers();
  await createMainWindow();
  sidecar = startSidecar();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) void createMainWindow();
  });
});

app.on("before-quit", (event) => {
  if (quitCoordinator.exitAllowed) return;
  event.preventDefault();
  quitCoordinator.handleBeforeQuit();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

export {
  APP_ORIGIN,
  DEV_FIXTURE_SIDECAR,
  SIDECAR_READY_TIMEOUT_MS,
  assertCurrentMainFrame,
  m5DataDirectory,
  sidecarLaunch,
};
import { M5_BRIDGE_V7_CHANNELS } from "./ipc-channels.ts";
import type { ChatAppendMessageRequestV1 } from "../src/m5/contracts/run-recovery-v1.ts";
