// macOS native UI smoke for working in several projects at once (ADE
// multi-project PR-A). Real sidecar, isolated app data, fixture agents only:
// every run is the `cancel-clean` fixture, which reports a question and then
// keeps working for 30 seconds.
//
// 1. Project A's task has an agent at work.
// 2. Switching to project B is one click while A works; A shows "working".
// 3. B's task starts an agent too: two projects work at once.
// 4. Back in A: its task is there, still working.
//
// Requires current .vite/build output and target/debug/elsecrew-sidecar.
// Run: electron scripts/m5-multiproject-ui-smoke.cjs [capture-folder]
const { app, dialog } = require('electron');
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const repoRoot = path.resolve(__dirname, '..');
const sidecar = path.join(repoRoot, 'target/debug/elsecrew-sidecar');
const originalSpawn = cp.spawn;
cp.spawn = function (command, args, options) {
  if (typeof command === 'string' && path.basename(command) === 'elsecrew-sidecar') {
    if (path.resolve(command) !== sidecar || JSON.stringify(args) !== '["--stdio"]') throw new Error('unexpected sidecar launch');
    return Reflect.apply(originalSpawn, this, [command, ['--stdio-agent-fixture', 'cancel-clean'], options]);
  }
  return Reflect.apply(originalSpawn, this, arguments);
};

const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-multiproject-')));
const captures = process.argv.find((arg, index) => index > 1 && !arg.startsWith('-') && !arg.endsWith('.cjs')) ?? path.join(root, 'captures');
fs.mkdirSync(captures, { recursive: true });
const repo = name => {
  const dir = path.join(root, name);
  fs.mkdirSync(dir);
  fs.writeFileSync(path.join(dir, 'hello.txt'), `${name}\n`);
  const env = { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' };
  for (const args of [['init', '-q', '--template='], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Smoke', '-c', 'user.email=smoke@example.invalid', 'commit', '-qm', 'fixture']]) cp.execFileSync('/usr/bin/git', args, { cwd: dir, env });
  return dir;
};
const alpha = repo('alpha');
const beta = repo('beta');
app.setPath('userData', path.join(root, 'userdata'));
const picks = [alpha, beta];
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [picks.shift() ?? alpha] });
dialog.showMessageBox = async () => ({ response: 1, checkboxChecked: false });

let finished = false;
const timer = setTimeout(() => finish(new Error('flow timeout')), 150000);
const checks = [];
function finish(error, details = {}) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  const result = { root, captures, error: error?.message, checks, ...details, status: error ? 'FAIL' : 'PASS' };
  fs.writeFileSync(path.join(captures, 'result.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
  app.once('will-quit', () => app.exit(error ? 1 : 0));
  app.quit();
}

app.on('browser-window-created', (_event, window) => {
  window.webContents.setBackgroundThrottling(false);
  window.webContents.on('console-message', details => { if (details.level === 'error') console.log(JSON.stringify({ stage: 'RENDERER_ERROR', message: details.message })); });
  window.webContents.once('did-finish-load', async () => {
    const run = code => window.webContents.executeJavaScript(code);
    const wait = async (predicate, label, timeoutMs = 20000) => {
      const deadline = performance.now() + timeoutMs;
      while (performance.now() < deadline) { if (await run(predicate)) { checks.push(label); return; } await new Promise(r => setTimeout(r, 100)); }
      throw new Error(`UI condition timeout (${label}): ${predicate}\n${(await run('document.body.innerText')).slice(0, 2000)}`);
    };
    const shot = async name => {
      await new Promise(r => setTimeout(r, 400));
      await run('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
      fs.writeFileSync(path.join(captures, `${name}.png`), (await window.webContents.capturePage()).toPNG());
    };
    const openProject = `document.querySelector('.m5-switcher-projects li.m5-project-open')?.dataset.path`;
    const switchTo = async name => {
      await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
      await wait(`Boolean(document.querySelector('.m5-switcher-projects li[data-path$="/${name}"] .m5-project-row'))`, `switcher lists ${name}`);
      const reason = await run(`document.querySelector('.m5-switch-blocked')?.textContent ?? null`);
      if (reason && !reason.includes('추가')) throw new Error(`switching is blocked: ${reason}`);
      if (await run(`document.querySelector('.m5-switcher-projects li[data-path$="/${name}"] .m5-project-row').disabled`)) throw new Error(`${name} cannot be chosen`);
      await run(`document.querySelector('.m5-switcher-projects li[data-path$="/${name}"] .m5-project-row').click()`);
      await wait(`(${openProject})?.endsWith('/${name}') && Boolean(document.querySelector('[data-testid=task-layout]'))`, `switched to ${name}`);
    };
    // A task and its developer's run, made through the bridge the screen uses.
    // The fixture engine's storage keeps the first role set (no coordinator,
    // no V3 roles), so the new-task window and a whole team cannot run here;
    // a team across projects is covered by the engine's race test
    // (other_tasks_save_merge_and_discard_while_a_team_writes_its_own_workspace).
    const projectIdOf = name => run(`window.m5Projects.request({action:'list',cursor:null}).then(r=>r.projects.find(p=>p.canonicalPath.endsWith('/${name}'))?.projectId ?? null)`);
    const makeTask = (projectId, goal) => run(`(async()=>{
      const api = window.m5;
      let settings = await api.getProjectRoleSettings({ projectId: ${JSON.stringify(projectId)} });
      // The fixture agent offers claude-opus-5 for Claude.
      if (settings.roles.some(role => role.runtime === 'claude-code' && role.modelSelector !== 'claude-opus-5')) {
        settings = await api.updateProjectRoleSettings({ projectId: ${JSON.stringify(projectId)}, expectedRevision: settings.revision,
          roles: settings.roles.map(role => role.runtime === 'claude-code' ? { ...role, modelSelector: 'claude-opus-5', reasoningEffort: null } : role) });
      }
      const prepared = await api.prepareChatWorkspace({ projectId: ${JSON.stringify(projectId)} });
      const m = prepared.manifest;
      const chat = await api.createChat({ preparationId: prepared.preparationId, projectId: ${JSON.stringify(projectId)}, worktreeId: m.worktreeId, workspaceIdentity: m.workspaceIdentity, goal: ${JSON.stringify(goal)}, expectedSettingsRevision: settings.revision });
      return chat.chatId;
    })()`);
    const startTeam = chatId => run(`(async()=>{
      const api = window.m5;
      const capability = await api.getAgentCapabilities({ runtime: 'claude-code', refresh: true });
      const detail = await api.getChat({ chatId: ${JSON.stringify(chatId)}, afterEventSequence: 0, cursor: null, limit: 50 });
      const started = await api.startAgentRun({ chatId: ${JSON.stringify(chatId)}, roleId: 'coding', inputMessageIds: [detail.messages[0].messageId], expectedCapabilityFingerprint: capability.fingerprint });
      return started.runId;
    })()`);
    const runState = chatId => run(`window.m5.getChat({ chatId: ${JSON.stringify(chatId)}, afterEventSequence: 0, cursor: null, limit: 1 }).then(d => (d.agentRuns ?? []).map(r => r.state))`);
    try {
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='프로젝트 선택'&&!b.disabled)`, 'first run');
      await run(require('./m5-smoke-complete-team.cjs'));
      // Two projects: A from the first run, B added from the home.
      await run(`Array.from(document.querySelectorAll('.m5-first-run button')).find(b=>b.textContent==='프로젝트 선택').click()`);
      await wait(`(${openProject})?.endsWith('/alpha')`, 'alpha open', 30000);
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='프로젝트 화면 닫기')?.click()`);
      await wait(`Boolean(document.querySelector('.m5-home:not([hidden]) .m5-project-add'))`, 'home');
      await run(`document.querySelector('.m5-home .m5-project-add').click()`);
      await wait(`(${openProject})?.endsWith('/beta')`, 'beta open', 30000);
      const alphaId = await projectIdOf('alpha');
      const betaId = await projectIdOf('beta');
      const alphaTask = await makeTask(alphaId, 'Alpha task');
      const betaTask = await makeTask(betaId, 'Beta task');
      // 1. A's team works.
      await switchTo('alpha');
      const alphaRun = await startTeam(alphaTask);
      await wait(`document.body.innerText.includes('Alpha task') && Boolean(document.querySelector('[data-testid=stage-bar] .m5-stage-now'))`, 'alpha team working', 30000);
      await shot('01-alpha-working');
      // 2. One click to B while A works; A's row says it works.
      await switchTo('beta');
      await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
      await wait(`Boolean(document.querySelector('.m5-switcher-projects li[data-path$="/alpha"] .m5-badge-running'))`, 'alpha shows working');
      await wait(`Boolean(document.querySelector('[data-testid=project-switcher] .m5-switcher-others'))`, 'switcher marks another project');
      if (await run(`Boolean(document.querySelector('.m5-switcher-projects li[data-path$="/beta"] .m5-badge-running'))`)) throw new Error('beta shows working before its agent started');
      checks.push('beta not working before its start');
      await shot('02-beta-alpha-working');
      await run(`document.querySelector('[data-testid=project-switcher]').open=false`);
      // 3. B works too: two teams at once.
      const betaRun = await startTeam(betaTask);
      await wait(`document.body.innerText.includes('Beta task') && Boolean(document.querySelector('[data-testid=stage-bar] .m5-stage-now'))`, 'beta team working', 30000);
      const both = { alpha: await runState(alphaTask), beta: await runState(betaTask) };
      if (!both.alpha.includes('running') || !both.beta.includes('running')) throw new Error('two teams were not working at once: ' + JSON.stringify(both));
      checks.push('two teams working at once');
      await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
      await wait(`Boolean(document.querySelector('.m5-switcher-projects li[data-path$="/alpha"] .m5-badge-running'))`, 'alpha still working while beta works');
      await shot('03-both-working');
      await run(`document.querySelector('[data-testid=project-switcher]').open=false`);
      // 4. Back to A: its task, still working.
      await switchTo('alpha');
      await wait(`document.body.innerText.includes('Alpha task') && Boolean(document.querySelector('[data-testid=stage-bar] .m5-stage-now'))`, 'alpha task shown again, still working');
      await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
      await wait(`Boolean(document.querySelector('.m5-switcher-projects li[data-path$="/beta"] .m5-badge-running'))`, 'beta shows working from alpha');
      await shot('04-back-in-alpha');
      await run(`document.querySelector('[data-testid=project-switcher]').open=false`);
      const runs = { alphaRun, betaRun, ...(await (async () => ({ alpha: await runState(alphaTask), beta: await runState(betaTask) }))()) };
      finish(null, { runs });
    } catch (error) {
      try { await shot('failure'); } catch { /* best effort */ }
      finish(error);
    }
  });
});

require(path.join(repoRoot, '.vite/build/main.cjs'));
