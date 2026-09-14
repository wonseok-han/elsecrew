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
    const editorNode = document.querySelector('.monaco-editor');
    [...document.querySelectorAll('button')].find(button => button.textContent.trim() === '기본 배치로').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden && document.querySelectorAll('.m5-dock-separator').length === 2", "reset panel visibility")};
    document.querySelector('[aria-label="편집기 패널에 집중"]').click();
    await ${waitFor("document.querySelector('[data-panel=files]').hidden && document.querySelector('[data-panel=changes]').hidden && document.querySelectorAll('.m5-dock-separator').length === 0", "focus editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('focus remounted Monaco');
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '파일').click();
    await ${waitFor("!document.querySelector('[data-panel=files]').hidden", "reopen files")};
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '변경').click();
    await ${waitFor("!document.querySelector('[data-panel=changes]').hidden", "reopen changes")};
    document.querySelector('[aria-label="편집기 패널 접기"]').click();
    await ${waitFor("document.querySelector('[data-panel=editor]').hidden", "collapse editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('collapse discarded Monaco');
    [...document.querySelectorAll('.m5-dock-visibility button')].find(button => button.textContent === '편집기').click();
    await ${waitFor("!document.querySelector('[data-panel=editor]').hidden", "restore editor")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('restore remounted Monaco');
    document.querySelector('[aria-label="편집기 패널 왼쪽으로"]').click();
    await ${waitFor("document.querySelector('.m5-dock-slot')?.dataset.panel === 'editor'", "panel reorder")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('reorder remounted Monaco');
    const separator = document.querySelector('.m5-dock-separator');
    const before = Number(separator.getAttribute('aria-valuenow'));
    separator.dispatchEvent(new KeyboardEvent('keydown', {key:'ArrowRight', bubbles:true}));
    await ${waitFor("Number(document.querySelector('.m5-dock-separator').getAttribute('aria-valuenow')) > before", "keyboard resize")};
    const transfer = new DataTransfer();
    transfer.setData('application/x-hello-agent-panel', 'changes');
    document.querySelector('[data-panel="editor"]').dispatchEvent(new DragEvent('drop', {dataTransfer:transfer, bubbles:true, cancelable:true}));
    await ${waitFor("document.querySelector('.m5-dock-slot')?.dataset.panel === 'changes'", "panel drop")};
    if (document.querySelector('.monaco-editor') !== editorNode) throw new Error('drop remounted Monaco');
    await ${waitFor("JSON.parse(localStorage.getItem('hello-agent.m5.dock.v1'))?.order[0] === 'changes'", "layout persistence")};
    return { workerCreated: true, dockVerified: true, text: document.body.textContent };
  })()`);
  if (!result.workerCreated || !result.text.includes("example.ts")) throw new Error("Monaco editor or local worker did not mount");
  if (!result.dockVerified) throw new Error("panel layout was not verified");
  process.stdout.write(`${JSON.stringify(result)}\n`);
  await window.close();
  app.exit(0);
}).catch(fail);
