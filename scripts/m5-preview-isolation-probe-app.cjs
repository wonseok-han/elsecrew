// Electron side of the result preview isolation probe. Started only by
// `scripts/m5-preview-isolation-probe.mjs`, with isolated app data, no
// provider and no remote debugging port. PROBE_WORKSPACE picks the task
// workspace kind: `snapshot` (copies inside the app data,
// ELSECREW_GIT_WORKTREES=0) or `git` (real Git worktrees in a task folder
// chosen inside the probe's temporary folder). Writes `app-result.json`;
// the launcher judges.
'use strict';
const { app, BrowserWindow, dialog, session } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { leakPage, controlPage, FILES } = require('./m5-preview-pages.cjs');

const root = process.env.PROBE_ROOT;
const httpPort = Number(process.env.PROBE_HTTP_PORT);
const udpPort = Number(process.env.PROBE_UDP_PORT);
const tcp6Port = Number(process.env.PROBE_TCP6_PORT);
const udp6Port = Number(process.env.PROBE_UDP6_PORT);
const marker = process.env.PROBE_MARKER;
const controlHost = process.env.PROBE_CONTROL_HOST;
const workspaceKind = process.env.PROBE_WORKSPACE;
const webrtcPage = JSON.parse(process.env.PROBE_WEBRTC || 'null');
const webrtcControl = JSON.parse(process.env.PROBE_WEBRTC_CONTROL || 'null');
if (!root || !httpPort || !udpPort || !tcp6Port || !udp6Port || !marker || !controlHost || !['git', 'snapshot'].includes(workspaceKind) || !webrtcPage || !webrtcControl || process.env.ELSECREW_PREVIEW !== '1') throw new Error('probe environment is incomplete');
const repoRoot = path.resolve(__dirname, '..');
const source = path.join(root, 'project');
const result = { steps: [], errors: [] };
const step = (name, value) => { result.steps.push(name); if (value !== undefined) result[name] = value; console.log(JSON.stringify({ stage: name })); };

app.commandLine.appendSwitch('log-net-log', path.join(root, 'netlog.json'));
app.commandLine.appendSwitch('net-log-capture-mode', 'Everything');
app.setPath('userData', path.join(root, 'userdata'));
const taskFolder = path.join(root, 'task-workspaces');
fs.mkdirSync(taskFolder, { recursive: true });
// The task folder picker (it may create folders) and the project picker.
dialog.showOpenDialog = async (...args) => {
  const options = args.find(arg => arg && typeof arg === 'object' && Array.isArray(arg.properties));
  return { canceled: false, filePaths: [options?.properties.includes('createDirectory') ? taskFolder : source] };
};

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let finished = false;
const deadline = setTimeout(() => finish(new Error('probe timeout')), 480_000);

function finish(error) {
  if (finished) return;
  finished = true;
  clearTimeout(deadline);
  if (error) result.errors.push(String(error.stack || error));
  fs.writeFileSync(path.join(root, 'app-result.json'), JSON.stringify(result, null, 2));
  app.once('will-quit', () => { result.shutdown = 'accepted'; fs.writeFileSync(path.join(root, 'app-result.json'), JSON.stringify(result, null, 2)); });
  app.quit();
  setTimeout(() => app.exit(error ? 1 : 0), 30_000).unref();
}

function treeHash(directory) {
  const hash = crypto.createHash('sha256');
  const walk = (current) => {
    for (const name of fs.readdirSync(current).sort()) {
      const full = path.join(current, name);
      const stat = fs.lstatSync(full);
      hash.update(`${path.relative(directory, full)}\0${stat.isSymbolicLink() ? 'link' : stat.isDirectory() ? 'dir' : 'file'}\0${stat.mode}\0`);
      if (stat.isSymbolicLink()) hash.update(fs.readlinkSync(full));
      else if (stat.isDirectory()) walk(full);
      else hash.update(fs.readFileSync(full));
    }
  };
  walk(directory);
  return hash.digest('hex');
}

/** Diagnostic (G6 step, plan 2.6): Electron processes' working set, KiB.
 * The sidecar is outside app metrics; this is the preview's share. */
function memory() {
  const metrics = app.getAppMetrics();
  return { processes: metrics.length, workingSetKiB: metrics.reduce((sum, metric) => sum + (metric.memory?.workingSetSize ?? 0), 0),
    renderers: metrics.filter(metric => metric.type === 'Tab').length,
    types: metrics.map(metric => `${metric.type}${metric.serviceName ? `:${metric.serviceName}` : ''}:${Math.round((metric.memory?.workingSetSize ?? 0) / 1024)}MiB`) };
}

/** Screenshots are evidence, not checks: a compositor hiccup is retried and
 * then recorded, never a probe failure. */
async function saveCapture(contents, name) {
  for (let attempt = 0; attempt < 3; attempt++) {
    try { fs.writeFileSync(path.join(root, name), (await contents.capturePage()).toPNG()); return; }
    catch (error) { (result.captureErrors ??= []).push(`${name}: ${String(error.message || error).slice(0, 60)}`); await sleep(500); }
  }
}

let started = false;
app.on('browser-window-created', (_event, window) => {
  if (started) return;
  started = true;
  window.webContents.once('did-finish-load', () => { run(window).then(() => finish(null), finish); });
});

async function run(window) {
  const main = require(path.join(repoRoot, '.vite/build/main.cjs'));
  const manager = main.previewManager;
  const page = code => window.webContents.executeJavaScript(code);
  const waitFor = async (predicate, ms = 30_000, label = 'condition') => {
    const end = Date.now() + ms;
    for (;;) {
      const value = await predicate();
      if (value) return value;
      if (Date.now() > end) throw new Error(`timeout: ${label}`);
      await sleep(100);
    }
  };
  window.show();
  await waitFor(async () => {
    const status = await page('window.m5.getSystemStatus()');
    result.lastStatus = { state: status.state, admission: status.storageAdmission?.state ?? null, error: status.error?.kind ?? null };
    return (status.state === 'ready' || status.state === 'shellReady') && (!status.storageAdmission || status.storageAdmission.state === 'ready');
  }, 60_000, 'engine ready');
  step('engineReady');
  await page(require('./m5-smoke-complete-team.cjs'));
  if (workspaceKind === 'git') {
    const chosen = await page(`window.m5Projects.request({ action: 'chooseWorkspaceFolder' })`);
    result.taskFolder = chosen.path ?? chosen.folder ?? null;
  }
  const project = await page('window.m5.openProject()');
  const projectId = project.projectId;
  const chats = {};
  for (const name of ['a', 'b']) {
    chats[name] = await page(`(async () => {
      const projectId = ${JSON.stringify(projectId)};
      const settings = await window.m5.getProjectRoleSettings({ projectId });
      const prepared = await window.m5.prepareChatWorkspace(${workspaceKind === 'git' ? '{ projectId, applicationBaseline: true }' : '{ projectId }'});
      const manifest = prepared.manifest;
      const chat = await window.m5.createChat({ preparationId: prepared.preparationId, projectId, worktreeId: manifest.worktreeId, workspaceIdentity: manifest.workspaceIdentity, goal: 'preview probe ${name}', expectedSettingsRevision: settings.revision });
      return { chatId: chat.chatId, worktreePath: manifest.worktreePath, workspaceKind: manifest.workspaceKind ?? null };
    })()`);
  }
  const appData = fs.realpathSync(path.join(root, 'userdata'));
  const tasks = fs.realpathSync(taskFolder);
  const inside = (base, target) => !path.relative(base, fs.realpathSync(target)).startsWith('..');
  step('chats', { kinds: Object.values(chats).map(chat => ({
    manifest: chat.workspaceKind,
    place: inside(appData, chat.worktreePath) ? 'insideAppData' : inside(tasks, chat.worktreePath) ? 'insideTaskFolder' : 'elsewhere',
  })) });

  // What a team would leave in the workspaces.
  const write = (directory, files) => { for (const [name, text] of Object.entries(files)) { fs.mkdirSync(path.dirname(path.join(directory, name)), { recursive: true }); fs.writeFileSync(path.join(directory, name), text); } };
  fs.writeFileSync(path.join(root, 'outside.txt'), 'outside secret\n');
  for (const chat of Object.values(chats)) {
    write(chat.worktreePath, { ...FILES, 'refresh.html': FILES['refresh.html'].replace('REFRESH_TARGET', `http://127.0.0.1:${httpPort}/refresh`), 'leak.html': leakPage({ httpPort, udpPort, tcp6Port, udp6Port, marker, webrtc: webrtcPage }), 'a/.secret/x.js': 'fixture-only' });
    fs.symlinkSync(path.join(root, 'outside.txt'), path.join(chat.worktreePath, 'link.txt'));
  }
  const hashBefore = treeHash(chats.a.worktreePath);
  const bounds = await page(`(() => ({ x: Math.round(innerWidth * 0.45), y: 60, width: Math.round(innerWidth * 0.5), height: innerHeight - 100 }))()`);
  const open = (chat, entry, viewport = 'wide') => page(`window.m5Preview.open(${JSON.stringify({ projectId, chatId: chat.chatId, entry, bounds, viewport })})`);
  const loaded = async () => {
    const contents = await waitFor(() => manager.currentWebContents, 10_000, 'preview view');
    await waitFor(() => !contents.isLoading() && contents.getURL().startsWith('elsecrew-preview://'), 15_000, 'preview load');
    await sleep(300);
    return contents;
  };
  const tokenOf = contents => new URL(contents.getURL()).hostname;

  // No preview yet: nothing extra runs.
  result.idleViews = manager.currentWebContents ? 1 : 0;
  // G6 step (diagnostic): idle, the todo preview open, then closed, before
  // any probe page starts camera or audio services.
  await sleep(1500);
  result.memory = { idle: memory() };
  const measured = await open(chats.a, null);
  await loaded();
  await sleep(1500);
  result.memory.todoOpen = memory();
  await page(`window.m5Preview.close(${JSON.stringify({ previewId: measured.previewId })})`);
  await sleep(3000);
  result.memory.afterClose = memory();

  // T12 setup: chat B's preview writes a key, then is replaced.
  const openedB = await open(chats.b, 'b.html');
  const bContents = await loaded();
  const tokenB = tokenOf(bContents);
  const partitionsB = bContents.session;
  result.bStorage = await bContents.executeJavaScript('localStorage.getItem("probe-key")');
  step('previewB', { outcome: openedB.outcome });

  // T1·T2·T4–T7·T9·T10·T13 in chat A's leak page.
  const openedA = await open(chats.a, 'leak.html');
  const contents = await loaded();
  const tokenA = tokenOf(contents);
  result.tokensDiffer = tokenA !== tokenB;
  result.previewIdHasToken = JSON.stringify(openedA).includes(tokenA);
  result.bClosedAfterReplace = bContents.isDestroyed();
  const { webContents } = require('electron');
  const contentsBefore = webContents.getAllWebContents().length;
  const navigationsBefore = manager.diagnostics.blockedNavigations;
  result.leak = await contents.executeJavaScript('window.runProbe()');
  // T10 and T9 again with a user gesture on a focused view: one call each,
  // so every attempt has its own activation.
  contents.focus();
  await sleep(200);
  result.permissions = {};
  for (const name of await contents.executeJavaScript('window.PERMISSION_NAMES')) {
    // Main's own count of refusals around this one call: the explicit denial.
    const denied = manager.diagnostics.deniedPermissions;
    const answer = await contents.executeJavaScript(`window.runPermission(${JSON.stringify(name)})`, true);
    await sleep(100);
    result.permissions[name] = { ...answer, deniedDelta: manager.diagnostics.deniedPermissions - denied };
  }
  result.newWebContents = webContents.getAllWebContents().length - contentsBefore;
  result.pageNavigationsBlocked = manager.diagnostics.blockedNavigations - navigationsBefore;
  // T15: chat B's token from chat A's page, after B was replaced.
  result.crossToken = await contents.executeJavaScript(`fetch('elsecrew-preview://${tokenB}/b.txt').then(r => 'status ' + r.status, e => 'error ' + e.name)`);
  // T9: top-level navigation attempts leave the page where it was.
  const before = contents.getURL();
  const topBlockedBefore = manager.diagnostics.blockedNavigations;
  await contents.executeJavaScript(`location.href = 'http://127.0.0.1:${httpPort}/navigate'`).catch(() => undefined);
  await sleep(800);
  await contents.executeJavaScript(`location.replace('https://${marker}-nav.example.com/')`).catch(() => undefined);
  await sleep(800);
  await contents.executeJavaScript(`location.href = 'elsecrew-preview://${tokenB}/b.html'`).catch(() => undefined);
  await sleep(800);
  result.urlAfterNavigation = { same: contents.getURL() === before, origin: new URL(contents.getURL()).hostname === tokenA };
  result.topNavigationsBlocked = manager.diagnostics.blockedNavigations - topBlockedBefore;
  step('leakPage');

  // Each network layer alone (T1·T2·T3): CSP bypassed through the debugger
  // protocol (test only), then also without the webRequest filter.
  const blockedBefore = manager.diagnostics.blockedRequests;
  contents.debugger.attach('1.3');
  await contents.debugger.sendCommand('Page.enable');
  await contents.debugger.sendCommand('Page.setBypassCSP', { enabled: true });
  contents.reloadIgnoringCache();
  await sleep(300);
  await waitFor(() => !contents.isLoading(), 15_000, 'bypass reload');
  result.layerWebRequest = await contents.executeJavaScript('window.runNetwork()');
  result.layerWebRequestBlocked = manager.diagnostics.blockedRequests - blockedBefore;
  contents.session.webRequest.onBeforeRequest(null);
  contents.reloadIgnoringCache();
  await sleep(300);
  await waitFor(() => !contents.isLoading(), 15_000, 'proxy-only reload');
  result.layerProxy = await contents.executeJavaScript('window.runNetwork()');
  contents.debugger.detach();
  step('layers');

  // T14: the app's own session cannot load a preview URL.
  const previewUrl = `elsecrew-preview://${tokenA}/data.txt`;
  result.appWindowFetch = await page(`fetch(${JSON.stringify(previewUrl)}).then(r => 'status ' + r.status, e => 'error ' + e.name)`);
  result.defaultSessionFetch = await session.defaultSession.fetch(previewUrl).then(r => `status ${r.status}`, e => `error ${e.message.slice(0, 80)}`);
  const plain = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true } });
  result.defaultSessionWindow = await plain.loadURL(previewUrl).then(() => `loaded ${plain.webContents.getURL()}`, e => `error ${String(e.code || e.message).slice(0, 80)}`);
  plain.destroy();
  // The preview page itself works within its origin (sanity).
  result.ownOrigin = await contents.executeJavaScript(`fetch('data.txt').then(r => r.text())`);

  // T6: a preview page has no IPC at all; the main window still works.
  result.previewHasIpc = await contents.executeJavaScript(`typeof window.require + ',' + typeof window.process + ',' + typeof window.m5`);

  // T7: the workspace did not change; the probe's own page write test above was 405.
  result.workspaceUnchanged = treeHash(chats.a.worktreePath) === hashBefore;

  // Todo app: JavaScript runs, storage works while open, wide and narrow.
  const openedTodo = await open(chats.a, null);
  const todo = await loaded();
  result.todoEntry = openedTodo.entry;
  await todo.executeJavaScript(`(() => { const input = document.getElementById('text'); input.value = '미리보기 확인'; document.querySelector('button').click(); input.value = '좁은 화면'; document.querySelector('button').click(); document.querySelector('li').click(); return document.getElementById('count').textContent; })()`).then(text => { result.todoCount = text; });
  result.todoStorage = await todo.executeJavaScript('localStorage.getItem("todos")');
  await page(`window.m5Preview.layout(${JSON.stringify({ previewId: openedTodo.previewId, bounds, viewport: 'wide', obscured: false })})`);
  await sleep(400);
  // A window capture holds only the app page; the view is captured on its own.
  await saveCapture(todo, 'todo-wide-view.png');
  await page(`window.m5Preview.layout(${JSON.stringify({ previewId: openedTodo.previewId, bounds, viewport: 'narrow', obscured: false })})`);
  await sleep(400);
  await saveCapture(todo, 'todo-narrow-view.png');
  result.todoNarrowWidth = await todo.executeJavaScript('innerWidth');
  const obscured = await page(`window.m5Preview.layout(${JSON.stringify({ previewId: openedTodo.previewId, bounds, viewport: 'narrow', obscured: true })})`);
  result.obscured = { state: obscured.state, snapshot: typeof obscured.snapshot === 'string' && obscured.snapshot.startsWith('data:image/jpeg;base64,') };
  step('todo');

  // T11: a page that never returns; the app stays responsive and closing ends it.
  const openedHang = await open(chats.a, 'hang.html');
  const hang = await loaded();
  const pid = hang.getOSProcessId();
  await sleep(1500);
  const latencies = [];
  for (let index = 0; index < 5; index++) { const start = performance.now(); await page('1'); latencies.push(performance.now() - start); await sleep(100); }
  result.hangAppLatencyMs = Math.max(...latencies);
  const closeStart = performance.now();
  const closed = await page(`window.m5Preview.close(${JSON.stringify({ previewId: openedHang.previewId })})`);
  result.hangCloseMs = performance.now() - closeStart;
  result.hangClosed = closed.state;
  result.hangProcessEnded = await waitFor(() => { try { process.kill(pid, 0); return false; } catch { return true; } }, 10_000, 'hung renderer exit').catch(() => false);
  step('hang');

  // Closing leaves no preview process and nothing on disk.
  result.viewsAfterClose = manager.currentWebContents ? 1 : 0;
  result.diagnostics = { ...manager.diagnostics };
  result.partitionsOnDisk = fs.existsSync(path.join(root, 'userdata', 'Partitions'))
    ? fs.readdirSync(path.join(root, 'userdata', 'Partitions')).filter(name => /preview/i.test(name)) : [];
  result.status = await page('window.m5Preview.status()');
  void partitionsB;
  // WebRTC positive control: the same per-transport code in an unrestricted
  // page of the app's own session, against control collectors: every
  // transport must reach its collector there, so silence in the preview is
  // the boundary at work, not a probe that never sends.
  const control = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true } });
  await control.loadURL(`data:text/html;base64,${Buffer.from(controlPage(webrtcControl)).toString('base64')}`);
  result.webrtcControl = await control.webContents.executeJavaScript('window.runControl()');
  control.destroy();
  // Net log positive control: a host lookup the log must show, made by the
  // app's own default session (never the preview's).
  result.controlLookup = await session.defaultSession.resolveHost
    ? await session.defaultSession.resolveHost(controlHost).then(() => 'resolved', error => `error ${String(error.message).slice(0, 60)}`)
    : await session.defaultSession.fetch(`http://${controlHost}/`).then(() => 'fetched', error => `error ${String(error.message).slice(0, 60)}`);
  step('done');
}

require(path.join(repoRoot, '.vite/build/main.cjs'));
