// Diagnostic controls, never a performance gate. Run each case in a fresh app.
const { app, BrowserWindow, net, protocol } = require('electron');
const { resolve, sep } = require('node:path');
const { pathToFileURL } = require('node:url');
const { mkdtempSync, mkdirSync, writeFileSync, readFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { createHash } = require('node:crypto');
const sha256 = value => createHash('sha256').update(value).digest('hex');
const { execFileSync } = require('node:child_process');
const { processTreeMemory } = require('../scripts/m5-process-memory.cjs');
const { buildPerformancePreload } = require('./m5-performance-preload-build.cjs');
const mode = process.argv[2];
if (!['blank', 'painted', 'localized', 'rendered', 'rendered-bridge', 'preload', 'product'].includes(mode)) throw new Error('unknown idle control');
const hasBridge = mode === 'preload' || mode === 'product' || mode === 'rendered-bridge';
const isStaticProduct = mode === 'rendered' || mode === 'rendered-bridge';
const softwareRendering = process.argv.includes('--software-rendering');
const spellcheck = !process.argv.includes('--no-spellcheck');
const deferShow = process.argv.includes('--defer-show');
const paintAfterMount = process.argv.includes('--paint-after-mount');
if (paintAfterMount && (mode !== 'product' || deferShow)) throw new Error('paint-after-mount requires product without defer-show');
const startedAt = performance.now();
let preSampleReadyAtMs;
const tileBudgetArg = process.argv.find(arg => arg.startsWith('--tile-budget-mib='));
const tileBudgetMiB = tileBudgetArg ? Number(tileBudgetArg.split('=')[1]) : undefined;
if (tileBudgetArg && (!Number.isSafeInteger(tileBudgetMiB) || tileBudgetMiB < 32 || tileBudgetMiB > 1024)) throw new Error('invalid diagnostic tile budget');
if (tileBudgetMiB) app.commandLine.appendSwitch('force-gpu-mem-available-mb', String(tileBudgetMiB));
if (softwareRendering) app.disableHardwareAcceleration();
// A shell may exec the last trial while retaining its PID. Capture the
// executable at launch instead of inferring concurrent apps from PPIDs later.
const launch = {pid:process.pid,parentPid:process.ppid,startedAt:new Date().toISOString(),
  parentExecutable:execFileSync('/bin/ps', ['-p',String(process.ppid),'-o','comm='], {encoding:'utf8',timeout:2000}).trim()};
const root = resolve(__dirname, '..');
const renderer = resolve(root, '.vite/renderer/main_window');
const staticMarkup = isStaticProduct ? readFileSync(resolve(root, '.artifacts/m5-performance/idle-static.html'), 'utf8') : undefined;
const userData = mkdtempSync(resolve(tmpdir(), 'hello-agent-idle-memory-'));
app.setPath('userData', userData);
protocol.registerSchemesAsPrivileged([{ scheme: 'm5idle', privileges: { standard: true, secure: true, supportFetchAPI: true } }]);
let window;
const timeout = setTimeout(() => { console.error({ mode, userData, error: 'timeout' }); app.exit(1); }, 15000);
app.whenReady().then(async () => {
  await protocol.handle('m5idle', request => {
    const url = new URL(request.url);
    if (url.hostname !== 'm5') return new Response('not found', { status: 404 });
    if (isStaticProduct && url.pathname === '/index.html') return new Response(staticMarkup, {headers:{'Content-Type':'text/html; charset=utf-8'}});
    if (mode !== 'product' && !isStaticProduct) {
      // Identical geometry/style for Latin and Korean text, with no app code or
      // preload. Compare these controls within one run series, not old sizes.
      const label = mode === 'localized' ? '프로젝트 선택 · 에이전트 연결 · 안전한 첫 작업'
        : mode === 'painted' ? 'Choose project · Connect agent · Start safely' : '';
      return new Response(`<!doctype html><html><head><meta charset="UTF-8"></head><body style="margin:0;min-height:100vh;background:#12161d;color:#e6edf3;font:13px/1.45 -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif"><main style="padding:24px">${label}</main></body></html>`, { headers: { 'Content-Type': 'text/html; charset=utf-8' } });
    }
    const asset = resolve(renderer, decodeURIComponent(url.pathname).replace(/^\/+/, '') || 'index.html');
    if (!asset.startsWith(renderer + sep)) return new Response('not found', { status: 404 });
    return net.fetch(pathToFileURL(asset).href);
  });
  window = new BrowserWindow({ width: 1280, height: 800, show: !deferShow && !paintAfterMount, paintWhenInitiallyHidden: !paintAfterMount, webPreferences: {
    ...(hasBridge ? { preload: buildPerformancePreload(resolve(userData, 'performance-preload.cjs')) } : {}),
    contextIsolation: true, sandbox: true, nodeIntegration: false, backgroundThrottling: false, spellcheck,
  } });
  if (deferShow) window.once('ready-to-show', () => window.show());
  const collectLayers = process.argv.includes('--layers');
  const debuggerApi = window.webContents.debugger;
  let layers = [];
  const onLayerMessage = (_event, method, params) => {
    if (method === 'LayerTree.layerTreeDidChange' && params.layers?.length) layers = params.layers;
  };
  // Subscribe before navigation: an already-idle page may not emit a new tree.
  let layerEnableResult = Promise.resolve(null);
  if (collectLayers) {
    debuggerApi.attach('1.3');
    debuggerApi.on('message', onLayerMessage);
    layerEnableResult = debuggerApi.sendCommand('LayerTree.enable').then(() => null, error => String(error));
  }
  await window.loadURL('m5idle://m5/index.html');
  if (paintAfterMount) {
    // Wait for the actual mounted UI, not ready-to-show (which requires paint).
    const deadline = performance.now() + 3000;
    while (!await window.webContents.executeJavaScript('Boolean(document.querySelector(".m5-workbench"))')) {
      if (performance.now() >= deadline) throw new Error('UI did not mount before first show');
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    window.show();
  }
  preSampleReadyAtMs = performance.now() - startedAt;
  const layerEnableError = await layerEnableResult;
  if (layerEnableError) throw new Error(layerEnableError);
  if (hasBridge && !await window.webContents.executeJavaScript('window.m5PerformanceEnvironment?.sandboxed === true')) throw new Error('preload is not sandboxed');
  await new Promise(r => setTimeout(r, 1000));
  const samples = [];
  for (let i = 0; i < 5; i++) {
    // The synchronous collector itself has exited before this output is read;
    // exclude its row using command name rather than count its transient RSS.
    const rows = execFileSync('/bin/ps', ['-axo', 'pid=,ppid=,rss=,comm='], { encoding: 'utf8' });
    const clean = rows.split('\n').filter(line => line.trim() && !/\s(?:\/bin\/)?ps$/.test(line)).map(line => line.trim().split(/\s+/).slice(0, 3).join(' ')).join('\n');
    samples.push(processTreeMemory(clean, process.pid));
    await new Promise(r => setTimeout(r, 250));
  }
  const report = { launch, diagnostic: true, rendererSandbox: true, mode, userData, head: execFileSync('/usr/bin/git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
    window: {width:1280,height:800}, softwareRendering, spellcheck, deferShow, paintAfterMount, tileBudgetMiB, preSampleReadyAtMs, visible:window.isVisible(),
    scope: 'fresh Electron app idle only; no sidecar/provider; five samples after one second; static modes have no renderer app code; rendered-bridge retains synthetic preload; not release evidence',
    samples, metrics: app.getAppMetrics(), dom: await window.webContents.executeJavaScript('({nodes:document.querySelectorAll("*").length,ready:Boolean(document.querySelector(".m5-workbench")),visibility:document.visibilityState})') };
  report.rendererEntrySha256 = sha256(readFileSync(resolve(renderer, 'index.html')));
  if (isStaticProduct) report.staticMarkupSha256 = sha256(staticMarkup);
  report.bridgeLoaded = hasBridge;
  if ((mode === 'product' || isStaticProduct) && !report.dom.ready) throw new Error('product did not mount');
  const directory = resolve(root, '.artifacts/m5-performance'); mkdirSync(directory, { recursive: true });
  if (mode === 'product') {
    // Own synthetic UI only: preserve DOM/CSS but remove executable app code.
    const markup = await window.webContents.executeJavaScript(`(() => {
      const clone = document.documentElement.cloneNode(true);
      clone.querySelectorAll('script, link[rel="modulepreload"]').forEach(node => node.remove());
      return '<!doctype html>' + clone.outerHTML;
    })()`);
    report.staticMarkupSha256 = sha256(markup);
    writeFileSync(resolve(directory, 'idle-static.html'), markup);
  }
  // Optional native allocation summary after RSS sampling. Only this owned
  // diagnostic app and its Electron-reported children are inspected; no dumps.
  if (process.argv.includes('--native-memory') || process.argv.includes('--native-regions')) {
    const regions = process.argv.includes('--native-regions');
    report.nativeMemoryScope = regions ? 'Region map after RSS sampling; mapped resident totals are not ps RSS or additive physical usage' : 'Summary after RSS sampling';
    report.nativeMemory = report.metrics.map(metric => {
      try {
        return {pid:metric.pid,type:metric.type,summary:execFileSync('/usr/bin/vmmap',
          [regions ? '-wide' : '-summary',String(metric.pid)],{encoding:'utf8',timeout:5000,maxBuffer:regions ? 4*1024*1024 : 256*1024})};
      } catch (error) { return {pid:metric.pid,type:metric.type,error:String(error)}; }
    });
  }
  if (collectLayers) {
    report.layerInstrumentation = 'Debugger attached before navigation; RSS is diagnostic and not comparable to uninstrumented samples';
    try {
      if (!layers.length) throw new Error('No compositor layers observed');
      report.layers = [];
      for (const layer of layers.slice(0, 50)) {
        const reasons = await debuggerApi.sendCommand('LayerTree.compositingReasons', {layerId:layer.layerId});
        const node = layer.backendNodeId ? (await debuggerApi.sendCommand('DOM.describeNode', {backendNodeId:layer.backendNodeId})).node : null;
        report.layers.push({...layer,reasons,node:node ? {name:node.nodeName,attributes:node.attributes} : null});
      }
      report.layerLimitExceeded = layers.length > 50;
    } catch (error) { report.layerError = String(error); }
    finally { debuggerApi.removeListener('message', onLayerMessage); if (debuggerApi.isAttached()) debuggerApi.detach(); }
  }
  // Allocator statistics only, after RSS sampling, in this synthetic app.
  // Do not force GC or use the instrumented result as a performance gate.
  if (process.argv.includes('--memory-infra')) {
    const events = [];
    let complete;
    const finished = new Promise(resolve => { complete = resolve; });
    const onTrace = (_event, method, params) => {
      if (method === 'Tracing.dataCollected') events.push(...params.value);
      if (method === 'Tracing.tracingComplete') complete(params);
    };
    debuggerApi.attach('1.3');
    debuggerApi.on('message', onTrace);
    try {
      await debuggerApi.sendCommand('Tracing.start', {transferMode:'ReportEvents', traceConfig:{
        recordMode:'recordUntilFull',traceBufferSizeInKb:16384,
        includedCategories:['disabled-by-default-memory-infra'],excludedCategories:['*'],
        memoryDumpConfig:{triggers:[]},
      }});
      const dump = await debuggerApi.sendCommand('Tracing.requestMemoryDump', {deterministic:false,levelOfDetail:'detailed'});
      await debuggerApi.sendCommand('Tracing.end');
      const completion = await finished;
      const tracePath = resolve(directory, `idle-memory-infra-${process.pid}.json`);
      writeFileSync(tracePath, JSON.stringify({traceEvents:events}) + '\n', {flag:'wx'});
      report.memoryInfra = {dump,completion,tracePath,versions:process.versions,scope:'Allocator statistics after RSS sampling; no forced GC; diagnostic only'};
    } finally {
      debuggerApi.removeListener('message', onTrace);
      if (debuggerApi.isAttached()) debuggerApi.detach();
    }
  }
  const evidencePath = resolve(directory, `idle-${mode}-${process.pid}.json`);
  report.completedAt = new Date().toISOString();
  writeFileSync(evidencePath, JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  console.log(JSON.stringify({ mode, userData, evidencePath, rssMiB: samples.map(s => s.rssBytes / 1024 ** 2), dom: report.dom }));
  clearTimeout(timeout); window.destroy(); app.exit(0);
}).catch(error => { console.error({ mode, userData, error: String(error) }); clearTimeout(timeout); app.exit(1); });
