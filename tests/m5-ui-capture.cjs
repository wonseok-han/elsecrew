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
  await wait("document.querySelector('.m5-home-projects li button')?.getClientRects().length > 0");
  await capture(window, 'product-start');
  await window.webContents.executeJavaScript("window.m5SmokeCatalogEmpty = true; document.querySelector('.m5-project-rail .m5-project-pagination button').click()");
  await wait("document.querySelector('.m5-start-steps') && !document.querySelector('.m5-home-projects')");
  await wait("document.querySelector('.m5-provider-selection input')?.getClientRects().length > 0");
  await capture(window, 'product-first-visit');
  window.setContentSize(960, 640);
  await wait("document.querySelector('.m5-start-action button').getBoundingClientRect().bottom <= innerHeight");
  await capture(window, 'product-start-minimum');
  window.setContentSize(1440, 1000);
  await window.webContents.executeJavaScript("[...document.querySelectorAll('button')].find(b => b.textContent === '프로젝트 선택' && !b.disabled).click()");
  await wait("document.querySelector('.m5-file-row') && !document.querySelector('.m5-toolbar-actions button:last-child').disabled");
  await capture(window, 'product-workbench');
  await window.webContents.executeJavaScript("document.querySelector('.m5-first-chat > button').click()");
  await wait("document.querySelector('.m5-new-chat-dialog')?.open");
  await window.webContents.executeJavaScript(`(() => {
    const dialog = document.querySelector('.m5-new-chat-dialog');
    const form = dialog.querySelector('form');
    const team = dialog.querySelector('.m5-chat-team-details');
    if (team.open || form.getBoundingClientRect().top >= team.getBoundingClientRect().top) throw new Error('new chat must prioritize goal input');
    if (form.querySelector('button').getBoundingClientRect().bottom > innerHeight) throw new Error('new chat action outside viewport');
  })()`);
  await capture(window, 'product-new-chat');
  await window.webContents.executeJavaScript("document.querySelector('.m5-new-chat-dialog .m5-chat-team-details summary').click()");
  await wait("document.querySelector('.m5-new-chat-dialog .m5-new-chat-team')?.getClientRects().length > 0");
  await capture(window, 'product-new-chat-team');
  await window.webContents.executeJavaScript("document.querySelector('.m5-new-chat-dialog header button').click()");
  await wait("!document.querySelector('.m5-new-chat-dialog')?.open");

  await window.webContents.executeJavaScript("[...document.querySelectorAll('button')].find(b => b.textContent === '역할과 모델').click()");
  await wait("document.querySelector('.m5-settings-dialog')?.open");
  await capture(window, 'product-settings');
  await window.webContents.executeJavaScript(`(() => {
    const planning = document.querySelector('#m5-role-panel-planning input');
    window.roleDraftBeforeNavigation = planning.value;
    document.querySelector('[aria-controls="m5-role-panel-coding"]').click();
  })()`);
  await wait("document.querySelector('#m5-role-panel-coding')?.getClientRects().length > 0 && document.querySelector('#m5-role-panel-planning')?.getClientRects().length === 0");
  await capture(window, 'product-settings-coding');
  await window.webContents.executeJavaScript("document.querySelector('[aria-controls=\"m5-role-panel-planning\"]').click()");
  await wait("document.querySelector('#m5-role-panel-planning')?.getClientRects().length > 0");
  await window.webContents.executeJavaScript("if (document.querySelector('#m5-role-panel-planning input').value !== roleDraftBeforeNavigation) throw new Error('role navigation lost draft')");

  window.setContentSize(960, 640);
  await wait("document.querySelector('.m5-settings-actions').getBoundingClientRect().bottom <= innerHeight");
  await window.webContents.executeJavaScript(`(() => {
    const body = document.querySelector('.m5-settings-body');
    if (body.scrollWidth > body.clientWidth + 1) throw new Error('settings body overflows horizontally');
    if (!document.querySelector('.m5-settings-scope strong').textContent.trim()) throw new Error('settings project scope missing');
  })()`);
  await capture(window, 'product-settings-minimum');
  window.setContentSize(1440, 1000);
  await window.webContents.executeJavaScript("[...document.querySelectorAll('.m5-settings button')].find(b => b.textContent === '작업대로 돌아가기').click()");
  await wait("!document.querySelector('.m5-settings-dialog')?.open");
  await window.webContents.executeJavaScript("[...document.querySelectorAll('.m5-dock-group-tabs button')].find(b => b.textContent.startsWith('변경') && b.getClientRects().length).click()");
  await wait("document.querySelector('.m5-change-row')?.getClientRects().length");
  await window.webContents.executeJavaScript("document.querySelector('.m5-change-row').click()");
  await wait("document.querySelector('.m5-diff-details')");
  await wait("document.querySelector('.m5-project-guide')?.open === false");
  await capture(window, 'product-active-diff');
  await window.webContents.executeJavaScript("[...document.querySelectorAll('.m5-dock-group-tabs button')].find(b => b.textContent === '채팅' && b.getClientRects().length).click()");
  await window.webContents.executeJavaScript("document.querySelector('.m5-chat-chip').click()");
  await wait("document.querySelector('.m5-timeline') && !document.querySelector('.m5-refresh').disabled");
  await wait("document.querySelector('.m5-project-guide')?.open === false");
  await capture(window, 'product-active-conversation');
  await window.webContents.executeJavaScript(`(() => {
    const tools = document.querySelector('.m5-project-tools');
    if (tools.getBoundingClientRect().height > 45) throw new Error('closed utilities should share a row');
    const select = document.querySelector('.m5-toolbar-actions select');
    select.value = 'en'; select.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await wait("document.querySelector('main').lang === 'en'");
  window.setContentSize(960, 640);
  await wait("document.querySelector('.m5-dock').getBoundingClientRect().height > 200");
  await window.webContents.executeJavaScript(`(() => {
    if (document.documentElement.scrollWidth > innerWidth) throw new Error('English workbench overflows horizontally');
    const toggle = document.querySelector('.m5-management-toggle');
    if (!toggle.getClientRects().length) throw new Error('connection access missing');
    const composer = document.querySelector('.m5-followup');
    if (composer.querySelector('button[type=submit]').getBoundingClientRect().bottom > composer.getBoundingClientRect().bottom + 1) throw new Error('followup action requires scrolling at minimum window');
  })()`);
  await capture(window, 'product-conversation-english-minimum');
  window.setContentSize(1440, 1000);
  await window.webContents.executeJavaScript("window.m5SmokeFailedClaude()");
  await wait("document.querySelector('.m5-agent-runs details')");
  await capture(window, 'product-failed-run');
  await window.webContents.executeJavaScript("document.querySelector('.m5-management-toggle').click()");
  await wait("document.querySelector('.m5-management-toggle').getAttribute('aria-expanded') === 'true'");
  await window.webContents.executeJavaScript(`(() => {
    const panel = document.querySelector('.m5-agent-management');
    const tools = document.querySelector('.m5-project-tools');
    if (panel.getBoundingClientRect().width < tools.getBoundingClientRect().width - 2) throw new Error('expanded setup must have full width');
  })()`);
  await capture(window, 'product-connection-expanded');

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
