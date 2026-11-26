// Response-time probe of the light reads (file open, approval list, getChat)
// in the real development app: isolated app data, a synthetic Git project and
// the fake agent (no account, no provider). Started by
// scripts/m5-latency-run.mjs, which runs it twice on the same data: the first
// launch measures "idle" and "running" (a long fake stream), the second starts
// from the clean shutdown's seal and measures while the background storage
// check runs on its copy.
//
// Each call is split into segments: renderer → main (IPC in), main → sidecar
// pipe and back, time waiting behind the sidecar loop, sidecar handling, and
// main → renderer (IPC out). The sidecar side comes from its own timing log
// (ELSECREW_SIDECAR_TIMING_LOG: method names and milliseconds only).
'use strict';
const { app, dialog } = require('electron');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const cp = require('node:child_process');

const repoRoot = path.resolve(__dirname, '..');
const stage = process.env.LATENCY_STAGE || 'first';
const root = process.env.LATENCY_ROOT || fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-latency-'));
const samples = Number(process.env.LATENCY_SAMPLES || 12);
const seedChats = Number(process.env.LATENCY_SEED_CHATS || 6);
const seedRuns = Number(process.env.LATENCY_SEED_RUNS || 0);
const source = path.join(root, 'project');
const userData = path.join(root, 'userdata');
const timingLog = path.join(root, `sidecar-timing-${stage}.jsonl`);
const resultFile = path.join(root, `result-${stage}.json`);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

if (stage === 'first') {
  fs.mkdirSync(source, { recursive: true });
  // About 400 small source files in 20 folders, one 400 KB text and a marker file.
  for (let d = 0; d < 20; d++) {
    fs.mkdirSync(path.join(source, 'src', `part${d}`), { recursive: true });
    for (let f = 0; f < 20; f++) fs.writeFileSync(path.join(source, 'src', `part${d}`, `file${f}.ts`), `// part ${d} file ${f}\n` + `export const value${f} = ${d * 100 + f};\n`.repeat(40));
  }
  fs.writeFileSync(path.join(source, 'big.txt'), Array.from({ length: 8000 }, (_, i) => `line ${i} ${'x'.repeat(40)}`).join('\n') + '\n');
  fs.writeFileSync(path.join(source, 'marker.md'), '# latency-marker\n\nopen me\n');
  for (const args of [['init', '-q', '--template='], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Probe', '-c', 'user.email=probe@example.invalid', 'commit', '-qm', 'fixture']]) {
    cp.execFileSync('/usr/bin/git', args, { cwd: source, env: { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' } });
  }
}
fs.mkdirSync(userData, { recursive: true });
app.setPath('userData', userData);
for (const [name, key] of [['session-data', 'sessionData']]) { fs.mkdirSync(path.join(root, name), { recursive: true }); app.setPath(key, path.join(root, name)); }
fs.mkdirSync(path.join(root, 'logs'), { recursive: true });
app.setAppLogsPath(path.join(root, 'logs'));
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [source] });

// The sidecar runs the fake agent and writes its timing log; main-side times
// of every request line and its response, by request id.
const stdio = new Map();
const spawn = cp.spawn;
cp.spawn = function (command, args, options) {
  if (typeof command === 'string' && path.basename(command) === 'elsecrew-sidecar' && args?.[0] === '--stdio') {
    // LATENCY_SIDECAR: another build of the engine (a saved baseline, say).
    const binary = process.env.LATENCY_SIDECAR || command;
    const child = Reflect.apply(spawn, this, [binary, ['--stdio-agent-fixture', 'paced-long-stream-read-only', ...args.slice(1)], { ...options, env: { ...(options?.env ?? process.env), ELSECREW_SIDECAR_TIMING_LOG: timingLog } }]);
    const write = child.stdin.write;
    child.stdin.write = function (chunk, ...rest) {
      try { const value = JSON.parse(String(chunk)); if (value.kind === 'request') stdio.set(value.requestId, { method: value.method, sent: Date.now() }); } catch { /* not one line */ }
      return Reflect.apply(write, this, [chunk, ...rest]);
    };
    let tail = '';
    child.stdout.on('data', chunk => {
      tail += chunk.toString('utf8');
      let end;
      while ((end = tail.indexOf('\n')) !== -1) {
        const line = tail.slice(0, end); tail = tail.slice(end + 1);
        if (!line.includes('"kind":"response"')) continue;
        try { const value = JSON.parse(line); const entry = stdio.get(value.requestId); if (entry) entry.received = Date.now(); } catch { /* ignore */ }
      }
    });
    return child;
  }
  return Reflect.apply(spawn, this, [command, args, options]);
};

const report = { stage, root, samples, seedChats, measurements: [], notes: [] };
const persist = () => fs.writeFileSync(resultFile, JSON.stringify(report, null, 2));
let finished = false;
let lastWindow = null;
async function finish(error) {
  if (finished) return;
  finished = true;
  if (error && lastWindow && !lastWindow.isDestroyed()) {
    try {
      fs.writeFileSync(path.join(root, `failure-${stage}.png`), (await lastWindow.webContents.capturePage()).toPNG());
      report.failureState = await lastWindow.webContents.executeJavaScript(`({ text: document.body.innerText.slice(0, 1500), rows: Array.from(document.querySelectorAll('.m5-task-row')).map(r => r.outerHTML.slice(0, 400)) })`);
    } catch { /* best effort */ }
  }
  if (error) report.error = String(error.stack || error);
  report.status = error ? 'FAIL' : 'DONE';
  persist();
  console.log(JSON.stringify({ stage, status: report.status, error: report.error?.slice(0, 400), resultFile }));
  setTimeout(() => app.quit(), 500);
  setTimeout(() => app.exit(error ? 1 : 0), 30_000).unref();
}
setTimeout(() => finish(new Error('probe timeout')), 20 * 60_000).unref();

app.on('browser-window-created', (_event, window) => {
  lastWindow = window;
  window.webContents.once('did-finish-load', () => { run(window).then(() => finish(null), finish); });
});

function sidecarTimings() {
  const byId = new Map();
  const phases = [];
  if (!fs.existsSync(timingLog)) return { byId, phases };
  for (const line of fs.readFileSync(timingLog, 'utf8').split('\n')) {
    if (!line) continue;
    try { const v = JSON.parse(line); if (v.kind === 'request') byId.set(v.requestId, v); else phases.push(v); } catch { /* partial */ }
  }
  return { byId, phases };
}

async function run(window) {
  const page = code => window.webContents.executeJavaScript(code);
  const waitFor = async (predicate, ms = 30_000, label = 'condition') => {
    const end = Date.now() + ms;
    for (;;) {
      const value = await predicate();
      if (value) return value;
      if (Date.now() > end) throw new Error(`timeout: ${label}`);
      await sleep(50);
    }
  };
  window.setSize(1600, 940);
  window.show();
  await waitFor(() => page(`Array.from(document.querySelectorAll('button')).some(b => b.textContent === '프로젝트 선택' && !b.disabled) || Boolean(document.querySelector('[data-testid=task-layout]'))`), 120_000, 'start screen');
  if (stage === 'first') {
    // A complete team whose lead runs on the fake agent's model.
    await page(`(async () => {
      const projects = window.m5Projects;
      const current = (await projects.request({ action: 'getAppRoleDefaults' })).settings;
      const roles = current.roles.map(role => role.roleId === 'coordinator'
        ? { ...role, runtime: 'claude-code', modelSelector: 'claude-opus-5', reasoningEffort: null }
        : role.modelSelector ? role : { ...role, modelSelector: role.runtime === 'codex' ? 'fixture-codex' : 'opus', reasoningEffort: role.runtime === 'codex' ? 'medium' : null });
      await projects.request({ action: 'updateAppRoleDefaults', expectedRevision: current.revision, roles, ...(current.roleDefinitions ? { roleDefinitions: current.roleDefinitions } : {}) });
    })()`);
    await page(`Array.from(document.querySelectorAll('button')).find(b => b.textContent === '프로젝트 선택').click()`);
  }
  await waitFor(() => page(`Boolean(document.querySelector('[data-testid=task-layout]'))`), 120_000, 'project open');
  const projectId = await page(`window.m5Projects.request({ action: 'list', cursor: null }).then(r => r.projects[0]?.projectId ?? null)`);
  if (stage === 'first') {
    // Seed tasks so the stored records are not trivially small.
    const make = goal => page(`(async () => {
      const projectId = ${JSON.stringify(projectId)};
      const settings = await window.m5.getProjectRoleSettings({ projectId });
      const prepared = await window.m5.prepareChatWorkspace({ projectId });
      const m = prepared.manifest;
      const chat = await window.m5.createChat({ preparationId: prepared.preparationId, projectId, worktreeId: m.worktreeId, workspaceIdentity: m.workspaceIdentity, goal: ${JSON.stringify(goal)}, expectedSettingsRevision: settings.revision });
      return { chatId: chat.chatId, worktreeId: m.worktreeId, workspaceIdentity: m.workspaceIdentity };
    })()`);
    report.task = await make('지연 측정 작업');
    const seeded = [];
    for (let i = 0; i < seedChats; i++) seeded.push(await make(`채움 작업 ${i}`));
    // Runs started and stopped on the seed tasks: stored run records like a
    // project that has been used for a while.
    report.seedRunErrors = [];
    const seedStarted = Date.now();
    // Each run needs its own user message: the first one on the task, then a
    // follow-up message per further run.
    for (let r = 0; r < seedRuns && seeded.length; r++) {
      const chat = seeded[r % seeded.length];
      const outcome = await page(`(async () => {
        const api = window.m5;
        const detail = await api.getChat({ chatId: ${JSON.stringify(chat.chatId)}, afterEventSequence: 0, cursor: null, limit: 50 });
        let messageId = detail.messages[0].messageId;
        if (${r} >= ${JSON.stringify(seeded.length)}) {
          const c = detail.chat, last = detail.messages[detail.messages.length - 1];
          const message = await api.appendChatMessage({ projectId: c.projectId, chatId: c.chatId, taskId: c.taskId, worktreeId: c.worktreeId, workspaceIdentity: c.workspaceIdentity,
            clientMessageId: 'user_' + crypto.randomUUID().replace(/-/g, ''), expectedEventSequence: last.eventSequence, text: 'seed follow-up ${r}' });
          messageId = message.messageId;
        }
        const capability = await api.getAgentCapabilities({ runtime: 'claude-code', refresh: false });
        await api.startAgentRun({ chatId: ${JSON.stringify(chat.chatId)}, roleId: 'coordinator', inputMessageIds: [messageId], expectedCapabilityFingerprint: capability.fingerprint });
        const c = detail.chat;
        await new Promise(resolve => setTimeout(resolve, 300));
        await window.m5Projects.request({ action: 'cancelTeamTask', chatId: c.chatId, taskId: c.taskId, projectId: c.projectId, worktreeId: c.worktreeId, workspaceIdentity: c.workspaceIdentity, triggerMessageId: messageId });
        for (let i = 0; i < 100; i++) {
          const after = await api.getChat({ chatId: ${JSON.stringify(chat.chatId)}, afterEventSequence: 0, cursor: null, limit: 1 });
          if (!(after.agentRuns ?? []).some(run => ['starting', 'running', 'stopping'].includes(run.state))) break;
          await new Promise(resolve => setTimeout(resolve, 100));
        }
        return null;
      })().catch(e => String(e && e.message || e).slice(0, 200))`);
      if (outcome) { report.seedRunErrors.push(outcome); if (report.seedRunErrors.length > 5) break; }
    }
    report.seedRunsMs = Date.now() - seedStarted;
    fs.writeFileSync(path.join(root, 'task.json'), JSON.stringify({ projectId, ...report.task }));
  } else {
    report.task = JSON.parse(fs.readFileSync(path.join(root, 'task.json'), 'utf8'));
  }
  const task = report.task;
  if (stage === 'first') {
    window.webContents.reload();
    await sleep(500);
    await waitFor(() => page(`document.readyState === 'complete' && Boolean(document.querySelector('[data-testid=task-layout]'))`).catch(() => false), 120_000, 'reload');
  }
  // Open the task in the screen, then the files tab.
  await waitFor(() => page(`(() => { const b = Array.from(document.querySelectorAll('.m5-task-row button.m5-chat-chip')).find(b => b.textContent.includes('지연 측정 작업')); if (!b) return false; if (b.getAttribute('aria-current') === 'true' || b.classList.contains('selected')) return true; if (!b.disabled) b.click(); return false; })()`), 60_000, 'task open');
  await page(`document.querySelector('[data-testid=right-tab-files]')?.click()`);
  await waitFor(() => page(`Boolean(document.querySelector('.m5-file-row[title="marker.md"]'))`), 60_000, 'file tree');
  persist();

  // One measured call of the bridge from the renderer, with wall-clock edges.
  const timed = async (label, condition, expression) => {
    const value = await page(`(async () => { const t0 = performance.timeOrigin + performance.now(); let ok = true, error = null; try { await (${expression}); } catch (e) { ok = false; error = String(e && e.message || e).slice(0, 160); } const t1 = performance.timeOrigin + performance.now(); return { t0, t1, ok, error }; })()`);
    report.measurements.push({ label, condition, ...value, totalMs: value.t1 - value.t0 });
  };
  // A file opened from the files tab: click → text visible in the editor.
  const uiOpen = async (condition, file, marker) => {
    await page(`(() => { for (const b of document.querySelectorAll('[data-testid=sheet-close]')) b.click(); })()`);
    await sleep(150);
    const folder = path.posix.dirname(file);
    // The tree draws only the rows in view: fold other part folders first.
    await page(`(() => { for (const row of document.querySelectorAll('.m5-file-row[aria-expanded=true]')) if (row.title.startsWith('src/part') && row.title !== ${JSON.stringify(folder)}) row.click(); })()`);
    await sleep(100);
    // Rows outside the view are not drawn: scroll the tree until the row is.
    const reveal = (title, open) => waitFor(() => page(`(() => {
      const row = document.querySelector('.m5-file-row[title=${JSON.stringify(title)}]');
      if (row) { ${open ? "if (row.getAttribute('aria-expanded') !== 'true') row.click();" : ''} row.scrollIntoView({ block: 'nearest' }); return true; }
      let list = document.querySelector('.m5-file-row');
      while (list && !(list.scrollHeight > list.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(list).overflowY))) list = list.parentElement;
      if (!list) return false;
      list.scrollTop = list.scrollTop + list.clientHeight >= list.scrollHeight - 1 ? 0 : list.scrollTop + Math.max(80, list.clientHeight / 2);
      return false;
    })()`), 20_000, `row ${title}`);
    // What a person sees: from the first click until the text shows. A failed
    // open (an error notice, a tree locked while the engine restarts) is
    // clicked again every 3 s, up to a minute.
    const visible = () => page(`Array.from(document.querySelectorAll('.monaco-editor .view-lines')).some(v => v.textContent.replace(/\\u00a0/g, ' ').includes(${JSON.stringify(marker)}))`);
    const click = async () => {
      try { await reveal('src', true); await reveal(folder, true); await reveal(file, false); } catch { return false; }
      return page(`(() => { const row = document.querySelector('.m5-file-row[title=${JSON.stringify(file)}]'); if (!row || row.disabled) return false; row.click(); return true; })()`);
    };
    let t0 = null, clicks = 0, ok = false;
    const until = Date.now() + 60_000;
    while (Date.now() < until && !ok) {
      if (await click()) { t0 ??= Date.now(); clicks++; }
      const retryAt = Date.now() + 3000;
      while (Date.now() < retryAt && !ok) { ok = await visible(); if (!ok) await sleep(20); }
    }
    const t1 = Date.now();
    t0 ??= t1;
    report.measurements.push({ label: `ui-open:${file}`, condition, t0, t1, ok, clicks, totalMs: t1 - t0 });
  };
  const getChat = `window.m5.getChat({ chatId: ${JSON.stringify(task.chatId)}, afterEventSequence: 0, cursor: null, limit: 50 })`;
  let uiOpened = stage === 'first' ? 0 : 200;
  const measureRound = async (condition, identity) => {
    for (let i = 0; i < samples; i++) {
      await timed('getChat', condition, getChat);
      if (identity) {
        await timed('approval:list', condition, `window.m5Approval.request({ action: 'list', identity: ${JSON.stringify(identity)} })`);
        const live = await page(`window.m5.getChat({ chatId: ${JSON.stringify(task.chatId)}, afterEventSequence: 0, cursor: null, limit: 1 }).then(d => (d.agentRuns ?? []).some(r => r.runId === ${JSON.stringify(identity.runId)} && ['starting', 'running'].includes(r.state)), () => null)`);
        report.measurements.at(-1).runLive = live;
      }
      // Files are opened the way a person does (the screen's file tree), a
      // different one each time; direct bridge reads would leave streams the
      // screen never asked for.
      const part = uiOpened % 20, file = Math.floor(uiOpened / 20) % 20;
      uiOpened++;
      await uiOpen(condition, `src/part${part}/file${file}.ts`, `part ${part} file ${file}`);
      await sleep(200);
    }
    persist();
  };

  if (stage === 'first') {
    await sleep(3000);
    await measureRound('idle', null);
    // A long fake stream on the measured task.
    const binding = await page(`(async () => {
      const api = window.m5;
      const capability = await api.getAgentCapabilities({ runtime: 'claude-code', refresh: true });
      const detail = await api.getChat({ chatId: ${JSON.stringify(task.chatId)}, afterEventSequence: 0, cursor: null, limit: 50 });
      return api.startAgentRun({ chatId: ${JSON.stringify(task.chatId)}, roleId: 'coordinator', inputMessageIds: [detail.messages[0].messageId], expectedCapabilityFingerprint: capability.fingerprint });
    })()`);
    const identity = Object.fromEntries(['chatId', 'taskId', 'projectId', 'worktreeId', 'workspaceIdentity', 'runId'].map(key => [key, binding[key]]));
    report.runIdentity = identity;
    await sleep(2500);
    await measureRound('running', identity);
    const ended = await page(`(async () => {
      const detail = await window.m5.getChat({ chatId: ${JSON.stringify(task.chatId)}, afterEventSequence: 0, cursor: null, limit: 50 });
      const c = detail.chat, trigger = [...detail.messages].reverse().find(m => m.kind === 'user');
      return window.m5Projects.request({ action: 'cancelTeamTask', chatId: c.chatId, taskId: c.taskId, projectId: c.projectId, worktreeId: c.worktreeId, workspaceIdentity: c.workspaceIdentity, triggerMessageId: trigger.messageId });
    })().then(() => true, e => String(e && e.message || e))`);
    report.notes.push({ cancel: ended });
    await sleep(3000);
  } else {
    // Started from the seal: the background check copies the database about a
    // minute after the start and checks the copy on a low-priority thread.
    const checkDir = path.join(userData, 'com.wonseokhan.elsecrew', 'm5', 'background-check-v1');
    const busy = () => fs.existsSync(checkDir) && fs.readdirSync(checkDir).length > 0;
    await measureRound('idle-from-seal', null);
    const appeared = await waitFor(() => busy(), 180_000, 'background check').then(() => Date.now(), () => null);
    report.backgroundCheckStartedAt = appeared;
    if (appeared) {
      await measureRound('background-check', null);
      report.backgroundCheckStillBusyAfter = busy();
    }
  }

  // Join the renderer calls with the sidecar requests they made.
  await sleep(500);
  const { byId, phases } = sidecarTimings();
  const methodOf = label => label.startsWith('ui-open') ? 'workspace.file.read' : label === 'getChat' ? 'chat.get' : label === 'approval:list' ? 'run.approval' : null;
  const used = new Set();
  for (const m of report.measurements) {
    const method = methodOf(m.label);
    if (!method) continue;
    let match = null;
    for (const [id, entry] of stdio) {
      if (used.has(id) || entry.method !== method || entry.sent < m.t0 - 1 || entry.sent > m.t1) continue;
      if (!match || entry.sent < match[1].sent) match = [id, entry];
    }
    if (!match) continue;
    used.add(match[0]);
    const [id, entry] = match;
    const side = byId.get(id);
    m.segments = {
      ipcInMs: entry.sent - m.t0,
      sidecarRoundTripMs: entry.received - entry.sent,
      queueMs: side?.queueMs ?? null,
      handleMs: side?.handleMs ?? null,
      ipcOutMs: m.t1 - entry.received,
    };
  }
  report.unmatched = report.measurements.filter(m => methodOf(m.label) && !m.segments).length;
  report.stdioSample = [...stdio.values()].filter(e => ['workspace.file.read', 'chat.get', 'run.approval'].includes(e.method)).slice(0, 5);
  report.phases = phases.map(({ at, phase, ms }) => ({ at, phase, ms }));
  report.requestsByMethod = {};
  for (const v of byId.values()) {
    const key = v.action ? `${v.method}:${v.action}` : v.method;
    const row = report.requestsByMethod[key] ??= { count: 0, maxQueueMs: 0, maxHandleMs: 0, totalHandleMs: 0 };
    row.count++; row.maxQueueMs = Math.max(row.maxQueueMs, v.queueMs); row.maxHandleMs = Math.max(row.maxHandleMs, v.handleMs); row.totalHandleMs += v.handleMs;
  }
  persist();
}

console.log(JSON.stringify({ stage, root }));
// LATENCY_MAIN: another build of the app's main bundle (a saved baseline).
require(process.env.LATENCY_MAIN || path.join(repoRoot, '.vite/build/main.cjs'));
