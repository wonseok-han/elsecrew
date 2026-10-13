// Diagnostic controls, never a performance gate. Run each case in a fresh app.
const { app, BrowserWindow, net, protocol } = require('electron');
const { resolve, sep } = require('node:path');
const { pathToFileURL } = require('node:url');
const { mkdtempSync, mkdirSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { execFileSync } = require('node:child_process');
const { processTreeMemory } = require('../scripts/m5-process-memory.cjs');
const mode = process.argv[2];
if (!['blank', 'preload', 'product'].includes(mode)) throw new Error('expected blank, preload, or product');
const root = resolve(__dirname, '..');
const renderer = resolve(root, '.vite/renderer/main_window');
const userData = mkdtempSync(resolve(tmpdir(), 'hello-agent-idle-memory-'));
app.setPath('userData', userData);
protocol.registerSchemesAsPrivileged([{ scheme: 'm5idle', privileges: { standard: true, secure: true, supportFetchAPI: true } }]);
let window;
const timeout = setTimeout(() => { console.error({ mode, userData, error: 'timeout' }); app.exit(1); }, 15000);
app.whenReady().then(async () => {
  await protocol.handle('m5idle', request => {
    const url = new URL(request.url);
    if (url.hostname !== 'm5') return new Response('not found', { status: 404 });
    if (mode !== 'product') return new Response('<!doctype html><html><body></body></html>', { headers: { 'Content-Type': 'text/html' } });
    const asset = resolve(renderer, decodeURIComponent(url.pathname).replace(/^\/+/, '') || 'index.html');
    if (!asset.startsWith(renderer + sep)) return new Response('not found', { status: 404 });
    return net.fetch(pathToFileURL(asset).href);
  });
  window = new BrowserWindow({ width: 1440, height: 1000, show: true, webPreferences: {
    ...(mode !== 'blank' ? { preload: resolve(__dirname, 'm5-renderer-performance-preload.cjs') } : {}),
    contextIsolation: true, sandbox: false, nodeIntegration: false, backgroundThrottling: false,
  } });
  await window.loadURL('m5idle://m5/index.html');
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
  const report = { diagnostic: true, mode, userData, head: execFileSync('/usr/bin/git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
    scope: 'fresh Electron app idle only; no sidecar/provider; five samples after one second; not release evidence',
    samples, metrics: app.getAppMetrics(), dom: await window.webContents.executeJavaScript('({nodes:document.querySelectorAll("*").length,ready:Boolean(document.querySelector(".m5-workbench"))})') };
  if (mode === 'product' && !report.dom.ready) throw new Error('product did not mount');
  const directory = resolve(root, '.artifacts/m5-performance'); mkdirSync(directory, { recursive: true });
  const evidencePath = resolve(directory, `idle-${mode}-${process.pid}.json`);
  writeFileSync(evidencePath, JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  console.log(JSON.stringify({ mode, userData, evidencePath, rssMiB: samples.map(s => s.rssBytes / 1024 ** 2), dom: report.dom }));
  clearTimeout(timeout); window.destroy(); app.exit(0);
}).catch(error => { console.error({ mode, userData, error: String(error) }); clearTimeout(timeout); app.exit(1); });
