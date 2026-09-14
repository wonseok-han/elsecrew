// Real Chromium/product-renderer timing with an isolated synthetic data bridge.
const { app, BrowserWindow, net, protocol } = require("electron");
const { resolve, sep } = require("node:path");
const { pathToFileURL } = require("node:url");
const { mkdirSync, writeFileSync } = require("node:fs");
const { spawnSync } = require("node:child_process");
const { createHash } = require("node:crypto");
const { readFileSync } = require("node:fs");
const { cpus, totalmem, release } = require("node:os");
const root = resolve(__dirname, "..");
const renderer = resolve(root, ".vite/renderer/main_window");
const samples = [];
let window;
let timeout;
protocol.registerSchemesAsPrivileged([{ scheme: "m5perf", privileges: { standard: true, secure: true, supportFetchAPI: true } }]);
const report = { schemaVersion: 1, evidence: "m5-renderer-fixture-performance", status: "NOT_READY", head: process.env.M5_MEASUREMENT_HEAD, startedAt: new Date().toISOString(), samples,
  environment: { electron: process.versions.electron, chromium: process.versions.chrome, node: process.versions.node, platform: process.platform, arch: process.arch, cpu: cpus()[0]?.model, ramBytes: totalmem(), os: release() },
  limitations: ["Production renderer build with synthetic bridge, not packaged-app startup, filesystem/IPC throughput, real agent streams, or overall G6 readiness."] };
function finish(error) {
  clearTimeout(timeout);
  if (error) { report.status = "FAIL"; report.error = String(error.stack || error); }
  report.finishedAt = new Date().toISOString();
  try {
    const directory = resolve(root, ".artifacts/m5-performance"); mkdirSync(directory, { recursive: true });
    report.evidencePath = resolve(directory, `renderer-${report.startedAt.replaceAll(":", "-")}-${process.pid}.json`);
    writeFileSync(report.evidencePath, JSON.stringify(report, null, 2) + "\n", { flag: "wx" });
  } catch { report.status = "FAIL"; }
  process.stdout.write(JSON.stringify(report) + "\n");
  if (window && !window.isDestroyed()) window.destroy();
  app.exit(report.status === "PASS" ? 0 : 1);
}
app.on("window-all-closed", () => {});
app.whenReady().then(async () => {
  const git = (args) => { const result = spawnSync("/usr/bin/git", args, { cwd: root, encoding: "utf8" }); if (result.status !== 0) throw new Error("Git unavailable"); return result.stdout.trim(); };
  if (process.platform !== "darwin" || process.arch !== "arm64" || !report.head || git(["rev-parse", "HEAD"]) !== report.head || git(["status", "--porcelain"]) !== "") throw new Error("requires a clean macOS arm64 measurement build");
  report.dirtyBefore = false;
  report.rendererHtmlSha256 = createHash("sha256").update(readFileSync(resolve(renderer, "index.html"))).digest("hex");
  timeout = setTimeout(() => finish(new Error("renderer benchmark timed out")), 120000);
  await protocol.handle("m5perf", (request) => {
    const url = new URL(request.url);
    const asset = resolve(renderer, decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html");
    if (url.hostname !== "m5" || !asset.startsWith(renderer + sep)) return new Response("not found", { status: 404 });
    return net.fetch(pathToFileURL(asset).href);
  });
  window = new BrowserWindow({ width: 1440, height: 1000, show: true, webPreferences: { preload: resolve(__dirname, "m5-renderer-performance-preload.cjs"), contextIsolation: true, sandbox: false, nodeIntegration: false, backgroundThrottling: false } });
  window.webContents.on("render-process-gone", (_event, details) => finish(new Error(details.reason)));
  for (let trial = 0; trial < 11; trial++) {
    await window.loadURL("m5perf://m5/index.html");
    const sample = await window.webContents.executeJavaScript(`(async () => {
      const wait = async (predicate) => { const end = performance.now() + 10000; while (!predicate()) { if (performance.now() > end) throw new Error("missing benchmark control: " + document.body.innerText.slice(0, 500)); await new Promise(r => setTimeout(r, 5)); } };
      const paint = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
      const button = (label) => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === label && !b.disabled);
      await wait(() => button('프로젝트 선택'));
      const longTasks = [];
      const observer = new PerformanceObserver(list => longTasks.push(...list.getEntries().map(e => e.duration)));
      observer.observe({ type: 'longtask' });
      const result = {};
      let start = performance.now(); button('프로젝트 선택').click();
      await wait(() => document.querySelectorAll('.m5-file-row').length === 500); await paint();
      result.treeRootMs = performance.now() - start;
      result.treeDomNodes = document.querySelector('.m5-file-list').querySelectorAll('*').length;
      await wait(() => document.querySelectorAll('.m5-change-row').length === 500); await paint();
      result.diffSummaryMs = performance.now() - start;
      button('다음 목록').click(); await wait(() => document.querySelector('.m5-file-row')?.textContent.includes('folder-500')); await paint();
      start = performance.now(); document.querySelector('.m5-file-row').click();
      await wait(() => document.querySelector('.m5-file-row')?.textContent.includes('large.txt')); await paint();
      result.treeExpandMs = performance.now() - start;
      start = performance.now(); document.querySelector('.m5-file-row').click();
      await wait(() => document.querySelector('.monaco-editor .view-line')?.textContent.includes('가')); await paint();
      result.fileEditableMs = performance.now() - start;
      document.querySelector('.monaco-editor textarea').focus();
      globalThis.__performanceInputStart = performance.now();
      globalThis.__performanceSample = result;
      globalThis.__performanceLongTasks = longTasks;
      globalThis.__performanceObserver = observer;
      return result;
    })()`);
    window.webContents.sendInputEvent({ type: "char", keyCode: "Z" });
    Object.assign(sample, await window.webContents.executeJavaScript(`(async () => {
      const wait = async (predicate) => { const end = performance.now() + 10000; while (!predicate()) { if (performance.now() > end) throw new Error("benchmark view did not settle"); await new Promise(r => setTimeout(r, 5)); } };
      const paint = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
      await wait(() => document.querySelector('[aria-label="저장되지 않은 변경"]') && document.querySelector('.view-line')?.textContent.includes('Z')); await paint();
      const inputPaintMs = performance.now() - globalThis.__performanceInputStart;
      let start = performance.now(); document.querySelector('.m5-change-row').click();
      await wait(() => document.querySelector('.m5-diff-details pre')?.textContent.startsWith('--- a/file-0')); await paint();
      const diffFirstFileMs = performance.now() - start;
      await new Promise(r => setTimeout(r, 50));
      globalThis.__performanceLongTasks.push(...globalThis.__performanceObserver.takeRecords().map(e => e.duration));
      globalThis.__performanceObserver.disconnect();
      return { inputPaintMs, diffFirstFileMs, rendererLongTaskMs: Math.max(0, ...globalThis.__performanceLongTasks) };
    })()`));
    if (trial === 0) report.warmup = sample; else samples.push(sample);
  }
  const limits = { treeRootMs: 1000, treeExpandMs: 250, fileEditableMs: 1500, inputPaintMs: 50, diffSummaryMs: 2000, diffFirstFileMs: 1500, rendererLongTaskMs: 200, treeDomNodes: 500 };
  report.maxima = Object.fromEntries(Object.keys(limits).map(key => [key, Math.max(...samples.map(sample => sample[key]))]));
  report.failedMetrics = Object.keys(limits).filter(key => report.maxima[key] > limits[key]);
  report.status = report.failedMetrics.length ? "FAIL" : "PASS";
  report.headAfter = git(["rev-parse", "HEAD"]); report.dirtyAfter = git(["status", "--porcelain"]) !== "";
  if (report.headAfter !== report.head || report.dirtyAfter) throw new Error("checkout changed during measurement");
  finish();
}).catch(finish);
