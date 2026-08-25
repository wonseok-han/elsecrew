import { app, BrowserWindow, net, protocol } from "electron";
import { resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string | undefined;
declare const MAIN_WINDOW_VITE_NAME: string;
declare const MAIN_WINDOW_PRELOAD_VITE_PATH: string;

const APP_SCHEME = "app";
const APP_HOST = "m5";
const APP_ORIGIN = `${APP_SCHEME}://${APP_HOST}`;

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
    if (url.protocol === `${APP_SCHEME}:` && url.host === APP_HOST) return true;
    return Boolean(MAIN_WINDOW_VITE_DEV_SERVER_URL) && url.origin === new URL(MAIN_WINDOW_VITE_DEV_SERVER_URL).origin;
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

  secureWindow(window);
  if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
    await window.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL);
  } else {
    await window.loadURL(`${APP_ORIGIN}/index.html`);
  }
}

app.whenReady().then(async () => {
  await protocol.handle(APP_SCHEME, serveAppAsset);
  await createMainWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) void createMainWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
