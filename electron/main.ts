import { app, BrowserWindow, dialog, ipcMain, net, protocol } from "electron";
import { mkdirSync, realpathSync } from "node:fs";
import { join, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import {
  M5_INTERNAL_CHANNELS_V1,
  type SystemStatusV1,
} from "../src/m5/contracts/lifecycle-v1.ts";
import type { EventV1, PrepareQuitResultV1 } from "../src/m5/contracts/protocol-v1.ts";
import type { WatchEventV1 } from "../src/m5/contracts/project-v1.ts";
import type { SaveBeginV1 } from "../src/m5/contracts/file-v1.ts";
import { M5_BRIDGE_V2_CHANNELS } from "./ipc-channels.ts";
import { EventCreditV1 } from "./event-credit.ts";
import { QuitCoordinatorV1 } from "./quit-coordinator.ts";
import { SIDECAR_READY_TIMEOUT_MS, SidecarClientV1, type SidecarLaunchV1 } from "./sidecar-client.ts";

declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string | undefined;
declare const MAIN_WINDOW_VITE_NAME: string;
declare const MAIN_WINDOW_PRELOAD_VITE_PATH: string;

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
  const url = new URL(request.url);
  if (url.protocol !== `${APP_SCHEME}:` || url.host !== APP_HOST) {
    return new Response("Not found", { status: 404 });
  }

  const relativePath = decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html";
  const root = rendererDirectory();
  const asset = resolve(root, relativePath);
  if (asset !== root && !asset.startsWith(`${root}${sep}`)) {
    return new Response("Not found", { status: 404 });
  }

  return net.fetch(pathToFileURL(asset).toString());
}

function isAllowedNavigation(target: string) {
  try {
    const url = new URL(target);
    if (!MAIN_WINDOW_VITE_DEV_SERVER_URL) {
      return url.protocol === `${APP_SCHEME}:` && url.host === APP_HOST && url.port === "" &&
        url.username === "" && url.password === "";
    }
    return url.origin === new URL(MAIN_WINDOW_VITE_DEV_SERVER_URL).origin;
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

function startSidecar(): SidecarClientV1 {
  const client = new SidecarClientV1(sidecarLaunch(m5DataDirectory()));
  client.onEvent(forwardSidecarEvent);
  client.onProjectEvent(forwardProjectEvent);
  void client.start().catch(() => {
    // getSystemStatus exposes a structured unavailable state. There is no restart or retry.
  });
  return client;
}

async function getSystemStatus(): Promise<SystemStatusV1> {
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
}

async function createMainWindow() {
  const window = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 960,
    minHeight: 640,
    webPreferences: {
      preload: MAIN_WINDOW_PRELOAD_VITE_PATH,
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
