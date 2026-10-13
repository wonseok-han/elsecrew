// Real Chromium/product-renderer timing with an isolated synthetic data bridge.
const { app, BrowserWindow, net, protocol } = require("electron");
const { resolve, sep } = require("node:path");
const { pathToFileURL } = require("node:url");
const { mkdirSync, writeFileSync, mkdtempSync } = require("node:fs");
const { spawnSync, execFile } = require("node:child_process");
const { processTreeMemory, summarizeMemory } = require("../scripts/m5-process-memory.cjs");
const { summarizeHeapSampling } = require("../scripts/m5-heap-sampling.cjs");
const { createHash } = require("node:crypto");
const { readFileSync } = require("node:fs");
const { cpus, totalmem, release, tmpdir } = require("node:os");
const userData = mkdtempSync(resolve(tmpdir(), "hello-agent-renderer-perf-"));
app.setPath("userData", userData);
const root = resolve(__dirname, "..");
const renderer = resolve(root, ".vite/renderer/main_window");
const samples = [];
let window;
let timeout;
const measureMemory = process.env.M5_MEASURE_MEMORY === "1";
const diagnostic = process.env.M5_MEASURE_DIAGNOSTIC === "1";
const domCounters = process.env.M5_MEASURE_DOM_COUNTERS === "1";
const heapSampling = process.env.M5_MEASURE_HEAP_SAMPLING === "1";
const memorySamples = [];
let memoryPhase = "idle";
let memoryTimer;
let memoryPending;
let memoryError;
let finished = false;
function sampleMemory() {
  if (memoryPending) return memoryPending;
  const phase = memoryPhase;
  memoryPending = new Promise((resolveSample, reject) => {
    const child = execFile("/bin/ps", ["-axo", "pid=,ppid=,rss="], { timeout: 5000, maxBuffer: 4 * 1024 * 1024 }, (error, stdout) => {
      if (error) return reject(error);
      try {
        // Exclude only the collector process itself, not application helpers.
        const rows = stdout.split("\n").filter(line => Number(line.trim().split(/\s+/)[0]) !== child.pid).join("\n");
        const tree = processTreeMemory(rows, process.pid);
        const metrics = new Map(app.getAppMetrics().map(metric => [metric.pid, metric]));
        memorySamples.push({ elapsedMs: performance.now(), phase, ...tree,
          processes: tree.processes.map(entry => ({ ...entry,
            type: metrics.get(entry.pid)?.type ?? "unknown",
            name: metrics.get(entry.pid)?.name ?? null,
          })),
        });
        resolveSample();
      } catch (error) { reject(error); }
    });
  }).finally(() => { memoryPending = null; });
  return memoryPending;
}
protocol.registerSchemesAsPrivileged([{ scheme: "m5perf", privileges: { standard: true, secure: true, supportFetchAPI: true } }]);
const report = { schemaVersion: 1, evidence: "m5-renderer-fixture-performance", status: "NOT_READY", head: process.env.M5_MEASUREMENT_HEAD, startedAt: new Date().toISOString(), samples,
  diagnostic, gateEligible: !diagnostic, userData,
  environment: { electron: process.versions.electron, chromium: process.versions.chrome, node: process.versions.node, platform: process.platform, arch: process.arch, cpu: cpus()[0]?.model, ramBytes: totalmem(), os: release() },
  limitations: ["Production renderer build with synthetic bridge, not packaged-app startup, filesystem/IPC throughput, real agent streams, or overall G6 readiness."] };
function finish(error) {
  if (finished) return;
  finished = true;
  clearTimeout(timeout);
  clearInterval(memoryTimer);
  if (error) { report.status = "FAIL"; report.error = String(error.stack || error); }
  if (diagnostic && report.status === "PASS") report.status = "NOT_READY";
  report.finishedAt = new Date().toISOString();
  try {
    const directory = resolve(root, ".artifacts/m5-performance"); mkdirSync(directory, { recursive: true });
    report.evidencePath = resolve(directory, `renderer-${report.startedAt.replaceAll(":", "-")}-${process.pid}.json`);
    writeFileSync(report.evidencePath, JSON.stringify(report, null, 2) + "\n", { flag: "wx" });
  } catch { report.status = "FAIL"; }
  process.stdout.write(JSON.stringify(report) + "\n");
  if (window && !window.isDestroyed() && window.webContents.debugger.isAttached()) window.webContents.debugger.detach();
  if (window && !window.isDestroyed()) window.destroy();
  app.exit(report.status === "PASS" ? 0 : report.status === "NOT_READY" ? 2 : 1);
}
async function memoryCheckpoint(label) {
  if (!measureMemory) return;
  await sampleMemory();
  // Attribution only: V8 heap and Electron working set are not RSS and do not
  // replace the existing process-tree gate or trigger garbage collection.
  report.memory.checkpoints.push({ label, elapsedMs: performance.now(),
    ...(domCounters ? { domCounters: await window.webContents.debugger.sendCommand("Memory.getDOMCounters") } : {}),
    ...(heapSampling ? { liveAllocationSample: summarizeHeapSampling((await window.webContents.debugger.sendCommand("HeapProfiler.getSamplingProfile")).profile) } : {}),
    main: process.memoryUsage(),
    processes: app.getAppMetrics().map(({ pid, type, name, memory }) => ({ pid, type, name, memory })),
    renderer: await window.webContents.executeJavaScript(`({
      domNodes: document.querySelectorAll('*').length,
      heap: performance.memory ? {
        usedJSHeapSize: performance.memory.usedJSHeapSize,
        totalJSHeapSize: performance.memory.totalJSHeapSize,
        jsHeapSizeLimit: performance.memory.jsHeapSizeLimit,
      } : null,
    })`),
  });
}
app.on("window-all-closed", () => {});
app.whenReady().then(async () => {
  const git = (args) => { const result = spawnSync("/usr/bin/git", args, { cwd: root, encoding: "utf8" }); if (result.status !== 0) throw new Error("Git unavailable"); return result.stdout.trim(); };
  report.checkoutBefore = git(["status", "--porcelain"]);
  report.dirtyBefore = report.checkoutBefore !== "";
  if (process.platform !== "darwin" || process.arch !== "arm64" || !report.head || git(["rev-parse", "HEAD"]) !== report.head || (!diagnostic && report.dirtyBefore)) throw new Error("requires a clean macOS arm64 measurement build");
  if (diagnostic) report.limitations.push("Diagnostic only: includes working-tree changes and can never pass a release gate.");
  report.rendererHtmlSha256 = createHash("sha256").update(readFileSync(resolve(renderer, "index.html"))).digest("hex");
  timeout = setTimeout(() => finish(new Error("renderer benchmark timed out")), 120000);
  await protocol.handle("m5perf", (request) => {
    const url = new URL(request.url);
    const asset = resolve(renderer, decodeURIComponent(url.pathname).replace(/^\/+/, "") || "index.html");
    if (url.hostname !== "m5" || !asset.startsWith(renderer + sep)) return new Response("not found", { status: 404 });
    return net.fetch(pathToFileURL(asset).href);
  });
  window = new BrowserWindow({ width: 1440, height: 1000, show: true, webPreferences: { preload: resolve(__dirname, "m5-renderer-performance-preload.cjs"), contextIsolation: true, sandbox: false, nodeIntegration: false, backgroundThrottling: false } });
  if (domCounters || heapSampling) {
    if (!diagnostic || !measureMemory) throw new Error("Profiler observations require diagnostic memory mode");
    window.webContents.debugger.attach("1.3");
    report.limitations.push("DevTools attached for memory attribution; not comparable to an uninstrumented release measurement. No leak preparation, cache purge or forced GC.");
  }
  window.webContents.on("render-process-gone", (_event, details) => finish(new Error(details.reason)));
  if (measureMemory) {
    report.memory = { scope: "Electron process tree only; synthetic bridge, no sidecar or agent", intervalMs: 250, closeWaitMs: 60000, samples: memorySamples, checkpoints: [], attributionNote: "Checkpoint V8 heap is approximate; Chromium working set and V8 heap are not additive to RSS. No forced GC." };
    await window.loadURL("m5perf://m5/index.html");
    if (heapSampling) {
      // The initial about:blank target may not have a live renderer isolate.
      // Start only after navigation has created the synthetic document.
      await window.webContents.debugger.sendCommand("HeapProfiler.startSampling", { samplingInterval:65536, stackDepth:32 });
      report.limitations.push("HeapProfiler live-allocation sampling at 64KiB interval, depth32; top50 callsites per checkpoint. Not retained-size, RSS, or proof of a leak; profiler overhead included. Initial document bootstrap precedes sampling. No heap snapshot or collectGarbage command.");
    }
    await window.webContents.executeJavaScript(`new Promise((resolve, reject) => {
      const deadline = performance.now() + 10000;
      const check = () => { if (document.querySelector('.m5-workbench')) resolve(); else if (performance.now() > deadline) reject(new Error('idle UI missing')); else setTimeout(check, 10); }; check();
    })`);
    await sampleMemory();
    memoryTimer = setInterval(() => { sampleMemory().catch(error => { memoryError = error; }); }, 250);
    await new Promise(resolve => setTimeout(resolve, 1000));
    if (memoryPending) await memoryPending;
    await memoryCheckpoint("idle");
    memoryPhase = "loaded";
  }
  for (let trial = 0; trial < 11; trial++) {
    await window.loadURL("m5perf://m5/index.html");
    window.show(); window.focus(); window.webContents.focus();
    const sample = await window.webContents.executeJavaScript(`(async () => {
      const wait = async (predicate) => { const end = performance.now() + 10000; while (!predicate()) { if (performance.now() > end) throw new Error("missing benchmark control: " + predicate.toString() + " / " + document.body.innerText.slice(0, 500)); await new Promise(r => setTimeout(r, 5)); } };
      const paint = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
      const button = (label) => [...document.querySelectorAll('button')].find(b => b.textContent.trim() === label && !b.disabled);
      await wait(() => button('프로젝트 선택'));
      const longTasks = [];
      const observer = new PerformanceObserver(list => longTasks.push(...list.getEntries().map(e => e.duration)));
      observer.observe({ type: 'longtask' });
      const result = {};
      let start = performance.now(); button('프로젝트 선택').click();
      await wait(() => document.querySelectorAll('.m5-file-row').length === 120);
      // The previous trial ends in Changes; expose Files before scrolling it.
      [...document.querySelectorAll('.m5-dock-group-tabs button')].find(b => b.textContent === '파일' && b.getClientRects().length)?.click();
      await paint();
      result.treeRootMs = performance.now() - start;
      result.treeDomNodes = document.querySelector('.m5-file-list').querySelectorAll('*').length;
      // The default dock now groups Files and Changes. A hidden list reaching
      // the right row count is not evidence that the user has seen it paint.
      [...document.querySelectorAll('.m5-dock-group-tabs button')].find(b => b.textContent === '변경' && b.getClientRects().length)?.click();
      await wait(() => document.querySelector('.m5-change-row')?.getClientRects().length > 0);
      await wait(() => document.querySelectorAll('.m5-change-row').length === 100); await paint();
      result.diffSummaryMs = performance.now() - start;
      // Changes are paged separately from the virtualized tree. Keep all 500
      // fixture entries reachable without requiring 500 simultaneous DOM rows.
      for (let page = 0; page < 5; page++) {
        const rows = [...document.querySelectorAll('.m5-change-row')];
        if (rows.length !== 100 || rows.some((row, index) => row.title !== 'file-' + (page * 100 + index) + '.txt')) throw new Error('incomplete change page ' + page);
        const next = document.querySelector('.m5-change-pages button:last-child');
        if (!next || next.disabled !== (page === 4)) throw new Error('incorrect change pagination');
        if (page < 4) {
          next.click();
          await wait(() => document.querySelector('.m5-change-row')?.title === 'file-' + ((page + 1) * 100) + '.txt');
        }
      }
      for (let page = 3; page >= 0; page--) {
        document.querySelector('.m5-change-pages button:first-child').click();
        await wait(() => document.querySelector('.m5-change-row')?.title === 'file-' + (page * 100) + '.txt');
      }
      // Verify the bounded DOM still exposes the end of the same 500-entry page.
      [...document.querySelectorAll('.m5-dock-group-tabs button')].find(b => b.textContent === '파일' && b.getClientRects().length)?.click();
      await wait(() => document.querySelector('.m5-file-row')?.getClientRects().length > 0);
      const treeList = document.querySelector('.m5-file-list');
      treeList.scrollTop = treeList.scrollHeight;
      await wait(() => [...document.querySelectorAll('.m5-file-row')].some(e => e.textContent.includes('folder-499'))); await paint();
      result.treeDomNodes = Math.max(result.treeDomNodes, treeList.querySelectorAll('*').length);
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
    app.focus({ steal: true }); window.show(); window.focus(); window.webContents.focus();
    await window.webContents.executeJavaScript(`(async () => {
      const input = document.querySelector('.monaco-editor textarea');
      input?.focus();
      const deadline = performance.now() + 1000;
      while (!document.hasFocus() || document.activeElement !== input) {
        if (performance.now() > deadline) throw new Error('benchmark input focus unavailable');
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      globalThis.__performanceInputStart = performance.now();
      globalThis.__performanceInputReceived = null;
      const captureInput = event => {
        if (event.target !== input || !event.isTrusted || event.data !== 'Z') return;
        globalThis.__performanceInputReceived = performance.now();
        document.removeEventListener('beforeinput', captureInput, true);
      };
      document.addEventListener('beforeinput', captureInput, true);
    })()`);
    window.webContents.sendInputEvent({ type: "char", keyCode: "Z" });
    Object.assign(sample, await window.webContents.executeJavaScript(`(async () => {
      const wait = async (predicate) => { const end = performance.now() + 10000; while (!predicate()) { if (performance.now() > end) throw new Error("benchmark view did not settle: " + predicate.toString() + " / visibility=" + document.visibilityState + " / focused=" + document.hasFocus()); await new Promise(r => setTimeout(r, 5)); } };
      const paint = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
      await wait(() => document.querySelector('[aria-label="저장되지 않은 변경"]') && document.querySelector('.view-line')?.textContent.includes('Z')); await paint();
      const inputPaintAt = performance.now();
      const inputPaintMs = inputPaintAt - globalThis.__performanceInputStart;
      const received = globalThis.__performanceInputReceived;
      // Attribution only: neither component replaces the end-to-end gate.
      const inputDeliveryMs = typeof received === 'number' ? received - globalThis.__performanceInputStart : null;
      const inputAfterDeliveryMs = typeof received === 'number' ? inputPaintAt - received : null;
      let start = performance.now(); document.querySelector('.m5-change-row').click();
      await wait(() => document.querySelector('.m5-diff-lines tbody code')?.textContent.startsWith('--- a/file-0')); await paint();
      const diffFirstFileMs = performance.now() - start;
      await new Promise(r => setTimeout(r, 50));
      globalThis.__performanceLongTasks.push(...globalThis.__performanceObserver.takeRecords().map(e => e.duration));
      globalThis.__performanceObserver.disconnect();
      return { inputPaintMs, inputDeliveryMs, inputAfterDeliveryMs, diffFirstFileMs, rendererLongTaskMs: Math.max(0, ...globalThis.__performanceLongTasks) };
    })()`));
    if (trial === 0) report.warmup = sample; else samples.push(sample);
    await memoryCheckpoint(`trial-${trial}`);
  }
  const limits = { treeRootMs: 1000, treeExpandMs: 250, fileEditableMs: 1500, inputPaintMs: 50, diffSummaryMs: 2000, diffFirstFileMs: 1500, rendererLongTaskMs: 200, treeDomNodes: 500 };
  report.maxima = Object.fromEntries(Object.keys(limits).map(key => [key, Math.max(...samples.map(sample => sample[key]))]));
  report.failedMetrics = Object.keys(limits).filter(key => report.maxima[key] > limits[key]);
  if (measureMemory) {
    await sampleMemory();
    memoryPhase = "closing";
    // Reload the same empty workbench to release documents without force-GC.
    await window.loadURL("m5perf://m5/index.html");
    const closedAt = performance.now();
    await new Promise(resolve => setTimeout(resolve, 60000));
    if (memoryPending) await memoryPending;
    memoryPhase = "retained";
    await sampleMemory();
    await memoryCheckpoint("retained");
    report.memory.actualCloseWaitMs = performance.now() - closedAt;
    clearInterval(memoryTimer);
    if (memoryError) throw memoryError;
    Object.assign(report.memory, summarizeMemory(memorySamples));
    report.failedMetrics.push(...report.memory.failedMetrics);
  }
  report.status = report.failedMetrics.length ? "FAIL" : "PASS";
  report.headAfter = git(["rev-parse", "HEAD"]); report.checkoutAfter = git(["status", "--porcelain"]); report.dirtyAfter = report.checkoutAfter !== "";
  if (report.headAfter !== report.head || (!diagnostic && report.dirtyAfter)) throw new Error("checkout changed during measurement");
  finish();
}).catch(finish);
