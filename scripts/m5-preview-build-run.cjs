// Build-then-preview in the real app (결과 미리보기 확장 1단계).
//
//   electron scripts/m5-preview-build-run.cjs <report-folder>          synthetic: isolated app data, the
//                                                                     Vite + React todo app written by this script
//   ELSECREW_PREVIEW_BUILD_REAL=1 electron scripts/m5-preview-build-run.cjs <report-folder>
//                                                                     real: the development app's own data and
//                                                                     sign-ins; the team (Claude Code, planning on
//                                                                     Codex) writes the app. Paid model calls.
//
// Then, through the screen: the preview tab's build bar → "필요한 프로그램
// 설치" (main's box, real `npm install` over the network) → "빌드해서 보기"
// (main's box, real `vite build`) → the build output opens in the isolated
// preview → a todo is added with real key input → a route of the app is
// reloaded (history fallback) → a second build is stopped (its own process
// group ends). Main's boxes are answered by this script; what each box said
// is kept (`consent-boxes.json`) and drawn as a labelled image, since this
// computer does not allow screen capture of native windows.
//
// Needs .vite/ built (scripts/m5-multiproject-ui-run.mjs --build or
// scripts/m5-preview-isolation-probe.mjs --build) and target/debug/elsecrew-sidecar.
// Real mode: run only when no other Elsecrew app uses the development data
// (`ps`); the test project is deleted at the end. Only this app's own
// processes are ever stopped.
'use strict';
if (process.platform !== 'darwin') { console.log(JSON.stringify({ status: 'NOT_RUN', reason: 'macOS only' })); process.exit(2); }
const { app, BrowserWindow, dialog } = require('electron');
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const REAL = process.env.ELSECREW_PREVIEW_BUILD_REAL === '1';
const repoRoot = path.resolve(__dirname, '..');
const out = path.resolve(process.argv.find((arg, index) => index > 1 && !arg.startsWith('-') && !arg.endsWith('.cjs')) ?? path.join(repoRoot, '.artifacts/m5-preview-build'));
fs.mkdirSync(out, { recursive: true });
const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-preview-build-')));
const project = path.join(root, 'todo-vite');
fs.mkdirSync(project);
fs.writeFileSync(path.join(project, 'README.md'), '# todo-vite\n\n할 일 앱 시험 프로젝트.\n');
const gitEnv = { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' };
for (const args of [['init', '-q', '--template='], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'start']]) cp.execFileSync('/usr/bin/git', args, { cwd: project, env: gitEnv });
if (REAL) app.setPath('userData', path.join(os.homedir(), 'Library', 'Application Support', 'Elsecrew'));
else app.setPath('userData', path.join(root, 'app-data'));

const result = { mode: REAL ? 'real' : 'synthetic', root, steps: [], checks: [], captures: [], consentBoxes: [], errors: [], processes: [] };
const step = name => { result.steps.push({ name, at: new Date().toISOString() }); console.log(JSON.stringify({ stage: name })); };
const check = (name, ok, detail) => { result.checks.push({ name, ok: Boolean(ok), detail }); console.log(JSON.stringify({ check: name, ok: Boolean(ok) })); };
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let mainModule;
let window;

// Main's boxes: the folder confirmation (a new project) and the build's two
// boxes. Each is answered "yes" here, as the person would; what it said is kept.
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [project] });
dialog.showMessageBox = async (first, second) => {
  const options = second ?? first;
  result.consentBoxes.push({ message: options.message, detail: options.detail, buttons: options.buttons, defaultId: options.defaultId, cancelId: options.cancelId });
  return { response: 1, checkboxChecked: false };
};

/** The synthetic team's work: a Vite + React todo app with two routes. */
const APP_FILES = {
  'package.json': JSON.stringify({ name: 'todo-vite', private: true, version: '0.0.0', type: 'module',
    scripts: { dev: 'vite', build: 'vite build' },
    dependencies: { react: '^18.3.1', 'react-dom': '^18.3.1' },
    devDependencies: { vite: '^5.4.0', '@vitejs/plugin-react': '^4.3.0' } }, null, 2) + '\n',
  '.gitignore': 'node_modules\ndist\n',
  'vite.config.js': "import { defineConfig } from 'vite';\nimport react from '@vitejs/plugin-react';\nexport default defineConfig({ plugins: [react()] });\n",
  'index.html': '<!doctype html>\n<html lang="ko"><head><meta charset="UTF-8" /><title>할 일</title></head>\n<body><div id="root"></div><script type="module" src="/src/main.jsx"></script></body></html>\n',
  'src/main.jsx': "import React from 'react';\nimport { createRoot } from 'react-dom/client';\nimport App from './App.jsx';\nimport './App.css';\ncreateRoot(document.getElementById('root')).render(<App />);\n",
  'src/App.css': 'body{font-family:-apple-system,sans-serif;margin:24px;color:#222}nav a{margin-right:12px}li.done span{text-decoration:line-through;color:#999}form{margin:12px 0}\n',
  'src/App.jsx': `import { useEffect, useState } from 'react';
export default function App() {
  const [path, setPath] = useState(location.pathname);
  const [items, setItems] = useState([{ id: 1, text: '장보기', done: true }]);
  const [text, setText] = useState('');
  useEffect(() => { const back = () => setPath(location.pathname); addEventListener('popstate', back); return () => removeEventListener('popstate', back); }, []);
  const go = (to) => (event) => { event.preventDefault(); history.pushState(null, '', to); setPath(to); };
  const shown = path === '/active' ? items.filter(item => !item.done) : items;
  const add = (event) => { event.preventDefault(); if (!text.trim()) return; setItems([...items, { id: Date.now(), text: text.trim(), done: false }]); setText(''); };
  return <main>
    <h1>할 일</h1>
    <nav><a href="/" onClick={go('/')}>전체</a><a href="/active" onClick={go('/active')}>남은 일</a></nav>
    <p data-route>{path === '/active' ? '남은 일만 보는 중' : '전체 보는 중'}</p>
    <form onSubmit={add}><input placeholder="할 일 입력" value={text} onChange={event => setText(event.target.value)} /><button>추가</button></form>
    <ul>{shown.map(item => <li key={item.id} className={item.done ? 'done' : ''}>
      <input type="checkbox" checked={item.done} onChange={() => setItems(items.map(other => other.id === item.id ? { ...other, done: !other.done } : other))} />
      <span>{item.text}</span> <button onClick={() => setItems(items.filter(other => other.id !== item.id))}>삭제</button></li>)}</ul>
  </main>;
}
`,
};

const REAL_GOAL = [
  'Vite + React로 할 일(todo) 웹 앱을 만들어 주세요. 개발 역할에게 바로 맡기고 다른 단계는 생략해도 됩니다.',
  '- 파일만 직접 작성하고 npm 같은 명령은 실행하지 마세요. 설치와 빌드는 앱이 합니다. package-lock.json은 만들지 마세요.',
  '- package.json: scripts.build = "vite build", dependencies react·react-dom ^18.3.1, devDependencies vite ^5.4.0·@vitejs/plugin-react ^4.3.0.',
  '- vite.config.js, index.html(작업 공간 맨 위), src/main.jsx, src/App.jsx, src/App.css.',
  '- .gitignore에 node_modules와 dist를 넣으세요.',
  '- 기능: placeholder가 "할 일 입력"인 입력칸과 "추가" 버튼(form 제출), 목록, 완료 체크, 삭제 버튼.',
  '- 위쪽에 "전체"(/)와 "남은 일"(/active) 링크를 두고 history.pushState로 경로를 바꾸는 간단한 라우팅을 라이브러리 없이 구현하세요. 새로 고쳐도 경로에 맞는 화면이 보여야 합니다.',
].join('\n');

app.on('browser-window-created', (_event, created) => {
  if (window) return;
  window = created;
  created.webContents.once('did-finish-load', () => {
    run().catch(async error => {
      // What the screen showed when it failed.
      try { result.screen = await created.webContents.executeJavaScript(`document.body.innerText.slice(0, 1500)`); } catch { /* gone */ }
      await capture('error').catch(() => undefined);
      finish(error);
    });
  });
});

async function run() {
  const page = code => window.webContents.executeJavaScript(code);
  const waitFor = async (predicate, ms, label) => {
    const end = Date.now() + ms;
    for (;;) {
      const value = await predicate().catch(() => false);
      if (value) return value;
      if (Date.now() > end) throw new Error(`timeout: ${label}`);
      await sleep(200);
    }
  };
  window.setSize(1680, 980);
  window.show();
  await waitFor(() => page(`Boolean(window.m5Projects && window.m5) && document.body.innerText.length > 0`), 60_000, 'app ready');
  await waitFor(() => page(`window.m5.getSystemStatus().then(s => s.state === 'ready' || s.state === 'shellReady')`), 90_000, 'engine ready');
  if (!REAL) await page(require('./m5-smoke-complete-team.cjs'));
  // Opened as the person would (the start screen's button; main's folder picker answers).
  // Test projects an earlier interrupted run of this script left behind
  // (their folders are this script's own temporary folders), removed first.
  if (REAL) {
    const left = await page(`window.m5Projects.request({ action: 'list', cursor: null }).then(r => r.projects.filter(p => /\\/elsecrew-preview-build-[^/]+\\/todo-vite$/.test(p.canonicalPath)).map(p => p.projectId))`).catch(() => []);
    for (const id of left) await page(`window.m5Projects.request({ action: 'deleteProject', projectId: ${JSON.stringify(id)}, confirmed: true }).catch(e => String(e))`);
    result.leftoverProjectsRemoved = left.length;
  }
  // The start screen's button, or (an app that reopened its last project)
  // "프로젝트 추가" in the project switcher.
  await waitFor(() => page(`Array.from(document.querySelectorAll('button')).some(b => b.textContent === '프로젝트 선택' && !b.disabled) || Boolean(document.querySelector('[data-testid=project-switcher]')) || Boolean(document.querySelector('.m5-project-add:not([disabled])'))`), 6 * 60_000, 'start screen or project');
  await page(`(() => {
    const choose = Array.from(document.querySelectorAll('button')).find(b => b.textContent === '프로젝트 선택' && !b.disabled);
    if (choose) return choose.click();
    const switcher = document.querySelector('[data-testid=project-switcher]');
    if (switcher) switcher.open = true;
  })()`);
  await waitFor(() => page(`(() => { if (document.querySelector('[data-testid=task-layout]') && document.body.innerText.includes('todo-vite')) return true; const add = document.querySelector('.m5-project-add'); if (add && !add.disabled && !window.__added) { window.__added = true; add.click(); } return false; })()`), 60_000, 'project open');
  const projectId = await page(`window.m5Projects.request({ action: 'list', cursor: null }).then(r => (r.projects.find(p => p.canonicalPath === ${JSON.stringify(project)}) ?? r.projects[0])?.projectId ?? null)`);
  if (!projectId) throw new Error('project id unavailable');
  result.projectId = projectId;
  step('project');
  try {
    if (REAL) await setupRoles(page, projectId);
    const task = await page(`(async () => {
      const projectId = ${JSON.stringify(projectId)};
      const settings = await window.m5.getProjectRoleSettings({ projectId });
      const prepared = await window.m5.prepareChatWorkspace({ projectId, applicationBaseline: true });
      const m = prepared.manifest;
      const chat = await window.m5.createChat({ preparationId: prepared.preparationId, projectId, worktreeId: m.worktreeId, workspaceIdentity: m.workspaceIdentity,
        goal: ${JSON.stringify(REAL ? REAL_GOAL : 'Vite + React 할 일 앱 만들기')}, expectedSettingsRevision: settings.revision });
      return { chatId: chat.chatId, worktreePath: m.worktreePath };
    })()`);
    result.task = task;
    if (REAL) await teamWrites(page, projectId, task);
    else for (const [name, text] of Object.entries(APP_FILES)) {
      fs.mkdirSync(path.dirname(path.join(task.worktreePath, name)), { recursive: true });
      fs.writeFileSync(path.join(task.worktreePath, name), text);
    }
    result.files = cp.execFileSync('/usr/bin/find', ['.', '-path', './node_modules', '-prune', '-o', '-type', 'f', '-not', '-path', './.git', '-not', '-path', './.git/*', '-print'], { cwd: task.worktreePath, encoding: 'utf8' }).split('\n').filter(Boolean).sort();
    step('app written');

    window.webContents.reload();
    await waitFor(() => page(`document.readyState === 'complete' && Boolean(document.querySelector('[data-testid=task-layout]'))`), 60_000, 'reload');
    await waitFor(() => page(`(() => { const b = Array.from(document.querySelectorAll('.m5-task-row button.m5-chat-chip'))[0]; if (!b) return false; if (b.getAttribute('aria-current') === 'true') return true; if (!b.disabled) b.click(); return false; })()`), 30_000, 'task selected');
    await page(`document.querySelector('[data-testid=right-tab-preview]').click()`);
    result.plan = await page(`(window.m5PreviewBuild ? window.m5PreviewBuild.plan({ projectId: ${JSON.stringify(projectId)}, chatId: ${JSON.stringify(task.chatId)} }) : Promise.resolve('no bridge')).catch(e => String(e))`);
    result.tab = await page(`({ phase: document.querySelector('[data-testid=preview-tab]')?.dataset.phase, text: document.querySelector('[data-testid=preview-tab]')?.innerText.slice(0, 300) })`);
    await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-build-install]'))`), 30_000, 'build bar');
    await sleep(1200);
    await capture('1-build-bar');
    const barText = await page(`document.querySelector('[data-testid=preview-build]').innerText`);
    check('the bar offers installing first (no node_modules yet); building waits', await page(`document.querySelector('[data-testid=preview-build-start]').disabled`), { barText });

    // Installing: main's box, then a real npm install (network).
    const boxesBefore = result.consentBoxes.length;
    await page(`document.querySelector('[data-testid=preview-build-install]').click()`);
    await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-build-running]'))`), 20_000, 'install running');
    check('installing asked main\'s box first', result.consentBoxes.length === boxesBefore + 1 && /인터넷/.test(result.consentBoxes.at(-1).message), result.consentBoxes.at(-1));
    await sleep(2500);
    noteProcesses('install');
    await capture('2-installing');
    await waitFor(() => page(`document.querySelector('[data-testid=preview-build]')?.dataset.state === 'succeeded'`), 6 * 60_000, 'install finished');
    step('installed');
    check('node_modules is ignored by Git (nothing recorded)', cp.execFileSync('/usr/bin/git', ['status', '--porcelain'], { cwd: task.worktreePath, env: gitEnv, encoding: 'utf8' }).split('\n').every(line => !/node_modules/.test(line)));

    // Building: main's box, the real build, then its output in the preview.
    await waitFor(() => page(`!document.querySelector('[data-testid=preview-build-start]')?.disabled`), 20_000, 'build enabled');
    await page(`document.querySelector('[data-testid=preview-build-start]').click()`);
    await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-build-badge]')) && document.querySelector('[data-testid=preview-tab]').dataset.phase === 'open'`), 3 * 60_000, 'build shown');
    check('building asked main\'s box with the command and folder', /빌드할까요/.test(result.consentBoxes.at(-1).message) && result.consentBoxes.at(-1).detail.includes(task.worktreePath) && result.consentBoxes.at(-1).detail.includes('npm run build'), result.consentBoxes.at(-1));
    const manager = mainModule.previewManager;
    await waitFor(async () => manager.currentLayout?.state === 'shown', 30_000, 'view shown');
    const view = () => manager.currentWebContents;
    await waitFor(async () => (await view().executeJavaScript(`document.querySelector('h1')?.textContent ?? ''`)).length > 0, 30_000, 'app rendered');
    await sleep(800);
    await capture('3-built-preview');
    const entry = await page(`document.querySelector('.m5-preview-entry')?.textContent`);
    check('the preview shows the build output, not the sources', /index\.html$/.test(entry ?? '') && entry.split('/').length === 2, { entry });

    // Using the page: real key input and a click.
    const contents = view();
    const before = await contents.executeJavaScript(`document.querySelectorAll('li').length`);
    await contents.executeJavaScript(`(() => { const i = Array.from(document.querySelectorAll('input')).find(i => i.type !== 'checkbox'); i.focus(); return Boolean(i); })()`);
    await contents.insertText('우유 사기');
    for (const type of ['keyDown', 'char', 'keyUp']) contents.sendInputEvent({ type, keyCode: type === 'char' ? '\r' : 'Enter' });
    await waitFor(async () => (await contents.executeJavaScript(`document.querySelectorAll('li').length`)) > before, 10_000, 'todo added');
    await sleep(600);
    const added = await contents.executeJavaScript(`document.body.innerText`);
    check('a todo was added with real key input', added.includes('우유 사기'), { before });
    await capture('4-todo-added');
    // A route of the app, reloaded: the server answers with the page (history fallback).
    await contents.executeJavaScript(`(() => { const a = Array.from(document.querySelectorAll('a')).find(a => a.getAttribute('href') === '/active'); a.click(); })()`);
    await sleep(500);
    contents.reloadIgnoringCache();
    await waitFor(async () => (await contents.executeJavaScript(`location.pathname + '|' + (document.querySelector('h1')?.textContent ?? '')`).catch(() => '')).startsWith('/active|') && (await contents.executeJavaScript(`document.querySelector('h1')?.textContent ?? ''`)).length > 0, 20_000, 'route reloaded');
    await waitFor(async () => manager.currentLayout?.state === 'shown', 20_000, 'view shown again');
    await sleep(800);
    const routed = await contents.executeJavaScript(`({ path: location.pathname, text: document.body.innerText.slice(0, 200) })`);
    check('a reloaded route of the app is served the page (history fallback)', routed.path === '/active' && routed.text.length > 0, routed);
    await capture('5-route-reloaded');

    // The build log (masked) and stopping a second build: its own group ends.
    // This build is slow on purpose and starts a child of its own (a
    // grandchild of the app), so stopping must end the whole group.
    const packageFile = path.join(task.worktreePath, 'package.json');
    const pkg = JSON.parse(fs.readFileSync(packageFile, 'utf8'));
    pkg.scripts.build = 'node slow-build.mjs && vite build';
    fs.writeFileSync(packageFile, JSON.stringify(pkg, null, 2) + '\n');
    fs.writeFileSync(path.join(task.worktreePath, 'slow-build.mjs'), "import { spawn } from 'node:child_process';\nconsole.log('slow build: token=ghp_abcdefghijklmnopqrstuvwxyz0123 waits');\nspawn(process.execPath, ['-e', 'setTimeout(() => {}, 120000)'], { stdio: 'ignore' });\nsetTimeout(() => {}, 120000);\n");
    await page(`(() => { const t = document.querySelector('[data-testid=preview-build-log-toggle]'); if (t.getAttribute('aria-expanded') !== 'true') t.click(); })()`);
    await page(`document.querySelector('[data-testid=preview-build-start]').click()`);
    await waitFor(() => page(`Boolean(document.querySelector('[data-testid=preview-build-running]'))`), 20_000, 'rebuild running');
    await sleep(2500);
    const pids = mainModule.previewBuildManager.runningPids;
    noteProcesses('rebuild');
    await capture('6-building-log');
    await page(`document.querySelector('[data-testid=preview-build-cancel]').click()`);
    await waitFor(() => page(`document.querySelector('[data-testid=preview-build]')?.dataset.state === 'cancelled'`), 20_000, 'cancelled');
    await sleep(500);
    const alive = pids.filter(pid => { try { process.kill(pid, 0); return true; } catch { return false; } });
    const group = pids.length ? cp.spawnSync('/bin/ps', ['-A', '-o', 'pid=,pgid=,command='], { encoding: 'utf8' }).stdout.split('\n').filter(line => line.trim().split(/\s+/)[1] === String(pids[0])) : [];
    const groupBefore = result.processes.at(-1).group;
    check('stopping ends the build and every process of its group', pids.length === 1 && groupBefore.length >= 3 && alive.length === 0 && group.length === 0, { pids, groupBefore, alive, group });
    await capture('7-cancelled');
    const logText = await page(`document.querySelector('[data-testid=preview-build-log]')?.innerText ?? ''`);
    result.logTail = logText.split('\n').slice(-20);
    check('a key the build printed is masked in the log', logText.includes('slow build: token=') && !logText.includes('abcdefghijklmnopqrstuvwxyz0123'), { tail: result.logTail.slice(-6) });
    const logFiles = fs.readdirSync(path.join(mainModule.m5DataDirectory(), 'preview-builds')).filter(name => /^[0-9a-f]{32}\.log$/.test(name));
    check('the masked log is kept in the app data folder', logFiles.length >= 1, { logFiles });
    await renderBoxes();
  } finally {
    await page(`window.m5Projects.request({ action: 'deleteProject', projectId: ${JSON.stringify(projectId)}, confirmed: true }).catch(e => String(e))`).catch(() => undefined);
    await sleep(3000);
    await page(`window.m5Projects.request({ action: 'finishDeletions' }).catch(() => null)`).catch(() => undefined);
  }
  finish(null);
}

/** Real mode: the team writes the app (coordinator Claude Code, planning on Codex). */
async function setupRoles(page, projectId) {
  await page(`(async () => {
    const projectId = ${JSON.stringify(projectId)};
    const settings = await window.m5.getProjectRoleSettings({ projectId });
    const pick = async runtime => (await window.m5Models.request({ runtime, refresh: false })).models.find(m => m.isDefault) ?? null;
    const models = { 'claude-code': await pick('claude-code'), codex: await pick('codex') };
    const effort = (runtime, model) => runtime === 'codex' ? (model.reasoningEfforts?.includes('low') ? 'low' : model.reasoningEfforts?.[0] ?? 'medium') : null;
    const roles = settings.roles.map(role => {
      const runtime = role.roleId === 'planning' ? 'codex' : 'claude-code';
      return { ...role, runtime, modelSelector: models[runtime].modelSelector, reasoningEffort: effort(runtime, models[runtime]) };
    });
    await window.m5.updateProjectRoleSettings({ projectId, expectedRevision: settings.revision, roles, ...(settings.roleDefinitions ? { roleDefinitions: settings.roleDefinitions } : {}) });
  })()`);
}

async function teamWrites(page, projectId, task) {
  const started = await page(`(async () => {
    const chatId = ${JSON.stringify(task.chatId)};
    const detail = await window.m5.getChat({ chatId, afterEventSequence: 0, cursor: null, limit: 50 });
    const capability = await window.m5.getAgentCapabilities({ runtime: 'claude-code', refresh: true, chatId, roleId: 'coordinator' });
    if (!capability.canStart) return { error: 'cannot start: ' + capability.blockedReason };
    return window.m5.startAgentRun({ chatId, roleId: 'coordinator', inputMessageIds: [detail.messages[0].messageId], expectedCapabilityFingerprint: capability.fingerprint });
  })()`);
  if (started.error) throw new Error(started.error);
  step('team started');
  const deadline = Date.now() + 20 * 60_000;
  let quietSince = 0;
  const approvals = [];
  for (;;) {
    const state = await page(`window.m5.getChat({ chatId: ${JSON.stringify(task.chatId)}, afterEventSequence: 0, cursor: null, limit: 1 }).then(d => ({ chat: d.chat.state, runs: (d.agentRuns ?? []).map(r => ({ state: r.state, roleId: r.roleId, runId: r.runId, taskId: r.taskId, chatId: r.chatId, projectId: r.projectId, worktreeId: r.worktreeId, workspaceIdentity: r.workspaceIdentity })) }))`);
    const live = state.runs.filter(run => ['preparing', 'running', 'cancelling'].includes(run.state));
    for (const run of live) {
      const identity = { chatId: run.chatId, taskId: run.taskId, projectId: run.projectId, worktreeId: run.worktreeId, workspaceIdentity: run.workspaceIdentity, runId: run.runId };
      const listed = await page(`window.m5Approval.request({ action: 'list', identity: ${JSON.stringify(identity)} }).catch(e => ({ error: String(e) }))`);
      for (const pending of listed.pending ?? []) {
        if (approvals.some(seen => seen.approvalId === pending.approvalId && seen.answered === 'ok')) continue;
        // As the card does (PR #19): read every page of the request before
        // answering; an unread request cannot be allowed.
        const seen = await page(`(async () => {
          const identity = ${JSON.stringify(identity)};
          const approvalId = ${JSON.stringify(pending.approvalId)};
          let first = await window.m5Approval.request({ action: 'inspect', identity, approvalId, page: 0 });
          const pages = first.inspection?.toolInputPages ?? 1;
          for (let page = 1; page < pages; page++) await window.m5Approval.request({ action: 'inspect', identity, approvalId, page });
          const i = first.inspection ?? {};
          return { tool: i.reason ?? null, command: i.command ?? null, input: String(i.toolInput ?? '').slice(0, 200) };
        })().catch(e => ({ error: String(e) }))`);
        if (seen.error) { approvals.push({ ...pending, roleId: run.roleId, seen, answered: 'inspect failed' }); continue; }
        // Writing files inside the workspace is the team's job; commands
        // (npm and the like) and the network are declined, as the goal says.
        const fileTool = pending.kind === 'fileChange' || (pending.kind === 'tool' && /^(Write|Edit|MultiEdit|NotebookEdit|Read|Glob|Grep|LS|TodoWrite)$/.test(seen.tool ?? ''));
        const action = fileTool ? 'acceptOnce' : 'decline';
        const answered = await page(`window.m5Approval.request({ action: ${JSON.stringify(action)}, identity: ${JSON.stringify(identity)}, approvalId: ${JSON.stringify(pending.approvalId)} }).then(() => 'ok').catch(e => String(e))`);
        approvals.push({ ...pending, roleId: run.roleId, tool: seen.tool, input: seen.input, action, answered });
      }
    }
    if (live.length === 0 && state.runs.length > 0 && state.chat !== 'running') {
      quietSince ||= Date.now();
      if (Date.now() - quietSince > 8000) { result.team = { chat: state.chat, runs: state.runs.map(run => `${run.roleId}:${run.state}`), approvals }; break; }
    } else quietSince = 0;
    if (Date.now() > deadline) throw new Error('the team did not finish in time');
    await sleep(2000);
  }
  step('team finished');
  check('the team wrote a package with a build script', fs.existsSync(path.join(task.worktreePath, 'package.json')), result.team);
}

/** This app's build processes right now (ps), for the report. */
function noteProcesses(label) {
  const pids = mainModule.previewBuildManager.runningPids;
  const rows = cp.spawnSync('/bin/ps', ['-A', '-o', 'pid=,pgid=,ppid=,command='], { encoding: 'utf8' }).stdout.split('\n')
    .filter(line => pids.length && line.trim().split(/\s+/)[1] === String(pids[0])).map(line => line.trim().slice(0, 200));
  result.processes.push({ label, appPid: process.pid, buildLeaders: pids, group: rows });
}

/** The app page with the preview view composed at its place. */
async function capture(name) {
  try {
    const manager = mainModule.previewManager;
    const base = await window.webContents.capturePage();
    const layout = manager.currentLayout;
    const view = manager.currentWebContents;
    const overlay = view && layout && layout.state === 'shown' ? (await view.capturePage()).toDataURL() : null;
    const [contentWidth] = window.getContentSize();
    const scale = base.getSize().width / contentWidth;
    const png = await draw(`
      const base = await load(${JSON.stringify(base.toDataURL())});
      canvas.width = base.width; canvas.height = base.height; g.drawImage(base, 0, 0);
      ${overlay ? `const v = await load(${JSON.stringify(overlay)}); const b = ${JSON.stringify(layout.bounds)}; g.drawImage(v, b.x * ${scale}, b.y * ${scale}, b.width * ${scale}, b.height * ${scale});` : ''}`);
    const file = path.join(out, `${name}.png`);
    fs.writeFileSync(file, png);
    result.captures.push(file);
  } catch (error) { result.errors.push(`capture ${name}: ${String(error.message || error).slice(0, 160)}`); }
}

/** What main's boxes said, drawn as text (a record of the options main
 * passed to `dialog.showMessageBox`, not a screenshot of the box). */
async function renderBoxes() {
  const boxes = result.consentBoxes.filter(box => /빌드|설치/.test(box.message));
  fs.writeFileSync(path.join(out, 'consent-boxes.json'), JSON.stringify(boxes, null, 2));
  try {
    const png = await draw(`
      const boxes = ${JSON.stringify(boxes)};
      canvas.width = 1400; canvas.height = 140 + boxes.length * 300;
      g.fillStyle = '#f4f4f4'; g.fillRect(0, 0, canvas.width, canvas.height);
      g.fillStyle = '#555'; g.font = '22px -apple-system'; g.fillText('main 기본 대화상자에 전달된 내용 (dialog.showMessageBox 옵션 기록 — 화면 캡처 아님)', 30, 50);
      boxes.forEach((box, index) => {
        const top = 90 + index * 300;
        g.fillStyle = '#fff'; g.fillRect(30, top, 1340, 270); g.strokeStyle = '#bbb'; g.strokeRect(30, top, 1340, 270);
        g.fillStyle = '#111'; g.font = 'bold 26px -apple-system'; g.fillText(box.message, 50, top + 42);
        g.font = '19px -apple-system'; g.fillStyle = '#333';
        const lines = []; for (const raw of box.detail.split('\\n')) { let line = ''; for (const ch of raw) { if (g.measureText(line + ch).width > 1280) { lines.push(line); line = ''; } line += ch; } lines.push(line); }
        lines.slice(0, 8).forEach((line, i) => g.fillText(line, 50, top + 80 + i * 24));
        g.font = '20px -apple-system'; g.fillStyle = '#0a58ca'; g.fillText('[ ' + box.buttons.join(' ]   [ ') + ' ]   (기본·취소: ' + box.buttons[box.cancelId] + ')', 50, top + 255);
      });`);
    const file = path.join(out, '0-consent-boxes.png');
    fs.writeFileSync(file, png);
    result.captures.push(file);
  } catch (error) { result.errors.push(`boxes: ${String(error.message || error).slice(0, 160)}`); }
}

async function draw(body) {
  const helper = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true } });
  try {
    await helper.loadURL('about:blank');
    const data = await helper.webContents.executeJavaScript(`(async () => {
      const load = src => new Promise((resolve, reject) => { const i = new Image(); i.onload = () => resolve(i); i.onerror = reject; i.src = src; });
      const canvas = document.createElement('canvas'); const g = canvas.getContext('2d');
      ${body}
      return canvas.toDataURL('image/png');
    })()`);
    return Buffer.from(data.split(',')[1], 'base64');
  } finally { helper.destroy(); }
}

let finished = false;
const timer = setTimeout(() => finish(new Error('run timeout')), (REAL ? 35 : 15) * 60_000);
function finish(error) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  if (error) result.errors.push(String(error.stack || error).slice(0, 600));
  result.status = !error && result.checks.every(entry => entry.ok) ? 'PASS' : 'FAIL';
  fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ status: result.status, out }));
  app.once('will-quit', () => app.exit(result.status === 'PASS' ? 0 : 1));
  app.quit();
}

mainModule = require(path.join(repoRoot, '.vite/build/main.cjs'));
