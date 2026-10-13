// Local visual evidence only: synthetic bridge, no sidecar or provider execution.
const { app, BrowserWindow, net, protocol, session } = require('electron');
const { resolve, sep } = require('node:path');
const { pathToFileURL } = require('node:url');
const { mkdirSync, mkdtempSync, writeFileSync } = require('node:fs');
const { tmpdir } = require('node:os');
const root = resolve(__dirname, '..');
const renderer = resolve(root, '.vite/renderer/main_window');
const userData = mkdtempSync(resolve(tmpdir(), 'hello-agent-ui-capture-'));
app.setPath('userData', userData);
protocol.registerSchemesAsPrivileged([{ scheme: 'm5visual', privileges: { standard: true, secure: true, supportFetchAPI: true } }]);
let output;
const windows = [];
const captures = [];
const timer = setTimeout(() => { console.error('visual capture timed out', { userData, output }); app.exit(1); }, 30000);
app.whenReady().then(async () => {
  const directory = resolve(root, '.artifacts/m5-visual');
  mkdirSync(directory, { recursive: true });
  output = mkdtempSync(resolve(directory, 'capture-'));
  const isolated = session.fromPartition('m5-visual');
  isolated.webRequest.onBeforeRequest((details, done) => done({ cancel: !/^(m5visual|file|data|blob):/.test(details.url) && !['about:blank', 'about:srcdoc'].includes(details.url) }));
  await isolated.protocol.handle('m5visual', request => {
    const url = new URL(request.url);
    const asset = resolve(renderer, decodeURIComponent(url.pathname).replace(/^\/+/, '') || 'index.html');
    if (url.hostname !== 'm5' || !asset.startsWith(renderer + sep)) return new Response('not found', { status: 404 });
    return net.fetch(pathToFileURL(asset).href);
  });
  const window = new BrowserWindow({ width: 1440, height: 1000, show: false, webPreferences: {
    preload: resolve(__dirname, 'm5-renderer-smoke-preload.cjs'), partition: 'm5-visual', contextIsolation: false,
    sandbox: false, nodeIntegration: false, backgroundThrottling: false,
  } });
  windows.push(window);
  const wait = expression => window.webContents.executeJavaScript(`(async () => {
    const end = Date.now() + 7000;
    while (!(${expression})) { if (Date.now() > end) throw new Error('missing visual control: ' + ${JSON.stringify(expression)}); await new Promise(r => setTimeout(r, 20)); }
    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
  })()`);
  const capture = async (target, name) => {
    writeFileSync(resolve(output, name + '.png'), (await target.webContents.capturePage()).toPNG());
    captures.push({ name, contentSize: target.getContentSize() });
  };
  await window.loadURL('m5visual://m5/index.html');
  await window.webContents.executeJavaScript('window.m5SmokeBlockers = []');
  await wait("document.querySelector('.m5-guided-start') && document.querySelector('.m5-provider-selection input')?.matches(':disabled') === false");
  await capture(window, 'product-start');
  window.setContentSize(960, 640);
  await wait("document.querySelector('.m5-start-action button').getBoundingClientRect().bottom <= innerHeight");
  await capture(window, 'product-start-minimum');
  window.setContentSize(1440, 1000);
  await window.webContents.executeJavaScript("[...document.querySelectorAll('button')].find(b => b.textContent === '프로젝트 선택' && !b.disabled).click()");
  await wait("document.querySelector('.m5-file-row') && !document.querySelector('.m5-toolbar-actions button:last-child').disabled");
  await capture(window, 'product-workbench');
  await window.webContents.executeJavaScript("[...document.querySelectorAll('button')].find(b => b.textContent === '역할과 모델').click()");
  await wait("document.querySelector('.m5-settings-dialog')?.open");
  await capture(window, 'product-settings');
  window.setContentSize(960, 640);
  await wait("document.querySelector('.m5-settings-actions').getBoundingClientRect().bottom <= innerHeight");
  await window.webContents.executeJavaScript(`(() => {
    const body = document.querySelector('.m5-settings-body');
    if (body.scrollWidth > body.clientWidth + 1) throw new Error('settings body overflows horizontally');
    if (!document.querySelector('.m5-settings-scope strong').textContent.trim()) throw new Error('settings project scope missing');
  })()`);
  await capture(window, 'product-settings-minimum');
  const concept = new BrowserWindow({ width: 1440, height: 1000, show: false, webPreferences: { partition: 'm5-visual', sandbox: true, contextIsolation: true, nodeIntegration: false, backgroundThrottling: false } });
  windows.push(concept);
  for (const name of ['agent-ide', 'guided-start']) {
    await concept.loadFile(resolve(root, 'docs/proposals/m5-' + name + '-concept.html'));
    // The concept embeds a sandboxed srcdoc document with its own paint cycle.
    await concept.webContents.executeJavaScript('new Promise(r => setTimeout(() => requestAnimationFrame(() => requestAnimationFrame(r)), 500))');
    await capture(concept, 'concept-' + name);
  }
  writeFileSync(resolve(output, 'scope.json'), JSON.stringify({ userData, output, scope: 'synthetic product UI and local concepts; external requests blocked; no provider or sidecar', captures }, null, 2));
  console.log(JSON.stringify({ userData, output }));
  clearTimeout(timer);
  for (const target of windows) target.destroy();
  app.exit(0);
}).catch(error => { console.error(error, { userData, output }); clearTimeout(timer); app.exit(1); });
