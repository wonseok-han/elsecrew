const { app, BrowserWindow, net, protocol } = require("electron");
const { resolve, sep } = require("node:path");
const { pathToFileURL } = require("node:url");

const rendererDirectory = resolve(process.env.M5_RENDERER_DIR || "");
const preload = resolve(__dirname, "m5-renderer-smoke-preload.cjs");

protocol.registerSchemesAsPrivileged([{ scheme: "m5smoke", privileges: { standard: true, secure: true, supportFetchAPI: true } }]);

function fail(error) {
  process.stderr.write(`${error instanceof Error ? error.stack || error.message : String(error)}\n`);
  app.exit(1);
}

function waitFor(script, label) {
  return `new Promise((resolve, reject) => {
    const deadline = Date.now() + 7000;
    const timer = setInterval(() => {
      if (${script}) { clearInterval(timer); resolve(true); }
      else if (Date.now() > deadline) { clearInterval(timer); reject(new Error(\`renderer smoke timed out: ${label}; body=\${document.body?.innerText?.trim().slice(0, 1000) || "<empty>"}; controls=\${JSON.stringify([...document.querySelectorAll("button")].map((button) => ({ text: button.textContent?.trim(), ariaLabel: button.getAttribute("aria-label"), disabled: button.disabled }))) }\`)); }
    }, 25);
  })`;
}

app.whenReady().then(async () => {
  if (!rendererDirectory || rendererDirectory === resolve("")) throw new Error("M5_RENDERER_DIR is required");
  await protocol.handle("m5smoke", (request) => {
    const url = new URL(request.url);
    const relative = decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html";
    const asset = resolve(rendererDirectory, relative);
    if (asset !== rendererDirectory && !asset.startsWith(`${rendererDirectory}${sep}`)) return new Response("not found", { status: 404 });
    return net.fetch(pathToFileURL(asset).toString());
  });
  const window = new BrowserWindow({ show: false, webPreferences: { preload, contextIsolation: false, sandbox: false, nodeIntegration: false } });
  window.webContents.on("console-message", (_event, _level, message) => process.stderr.write(`renderer: ${message}\n`));
  window.webContents.on("render-process-gone", (_event, details) => fail(new Error(`renderer exited: ${details.reason}`)));
  await window.loadURL("m5smoke://m5/index.html");
  const result = await window.webContents.executeJavaScript(`(async () => {
    delete globalThis.MonacoEnvironment;
    if (typeof globalThis.MonacoEnvironment !== "undefined" || Object.hasOwn(globalThis, "MonacoEnvironment")) {
      throw new Error("MonacoEnvironment should be created by the product renderer");
    }
    const controlLabel = "프로젝트 선택";
    await ${waitFor("[...document.querySelectorAll('button')].some((button) => !button.disabled && (button.getAttribute('aria-label') === controlLabel || button.textContent?.trim() === controlLabel))", "enabled accessible project-selection control")};
    const projectButton = [...document.querySelectorAll("button")].find((button) => !button.disabled && (button.getAttribute("aria-label") === controlLabel || button.textContent?.trim() === controlLabel));
    if (!projectButton) throw new Error("renderer smoke lost its enabled accessible project-selection control after it was observed");
    projectButton.click();
    await ${waitFor("document.querySelector('.m5-file-row')", "project tree")};
    document.querySelector(".m5-file-row").click();
    await ${waitFor("document.querySelector('.monaco-editor') && Object.hasOwn(globalThis, 'MonacoEnvironment') && typeof globalThis.MonacoEnvironment.getWorker === 'function'", "Monaco mount")};
    const localWorker = globalThis.MonacoEnvironment.getWorker("smoke", "editor");
    localWorker.terminate();
    return { workerCreated: true, text: document.body.textContent };
  })()`);
  if (!result.workerCreated || !result.text.includes("example.ts")) throw new Error("Monaco editor or local worker did not mount");
  process.stdout.write(`${JSON.stringify(result)}\n`);
  await window.close();
  app.exit(0);
}).catch(fail);
