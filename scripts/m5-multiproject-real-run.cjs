// Real accounts, real providers (ADE multi-project P5): two Claude Code runs
// and two Codex runs at once with the app's own sign-ins, from two projects.
// Each answer is long (numbers in words), so the two runs of one agent are
// still working together once the engine has started both.
// While they work: the Claude sign-in check (which must leave renewal to the
// working Claude Code) and model lists run beside them. Every run must end
// succeeded, and the sign-ins must still be connected afterwards.
//
// Uses the development app's own data (where its sign-ins live): run only
// when no other Elsecrew app or engine uses it. Paid model calls: four tiny
// prompts. The two test projects are deleted at the end.
// Run: electron scripts/m5-multiproject-real-run.cjs <report-folder>
const { app, dialog } = require('electron');
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const repoRoot = path.resolve(__dirname, '..');
const report = path.resolve(process.argv.find((arg, index) => index > 1 && !arg.startsWith('-') && !arg.endsWith('.cjs')) ?? path.join(os.tmpdir(), 'elsecrew-real-run'));
fs.mkdirSync(report, { recursive: true });
const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-real-multiproject-')));
const repo = name => {
  const dir = path.join(root, name);
  fs.mkdirSync(dir);
  fs.writeFileSync(path.join(dir, 'README.md'), `# ${name}\n\nA tiny test project.\n`);
  const env = { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' };
  for (const args of [['init', '-q', '--template='], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'start']]) cp.execFileSync('/usr/bin/git', args, { cwd: dir, env });
  return dir;
};
const folders = { claude: repo('real-claude'), codex: repo('real-codex') };
const picks = [folders.claude, folders.codex];
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [picks.shift()] });
dialog.showMessageBox = async () => ({ response: 1, checkboxChecked: false });
// The development app's own data, as `pnpm start` uses it.
app.setPath('userData', path.join(os.homedir(), 'Library', 'Application Support', 'Elsecrew'));
// The engine's own diagnostics (fixed names, never provider text) beside the
// report: why a run failed is otherwise not kept anywhere.
const spawnProcess = cp.spawn;
cp.spawn = function (command, args, options) {
  const engine = typeof command === 'string' && path.basename(command) === 'elsecrew-sidecar' && args?.[0] === '--stdio';
  // ELSECREW_REAL_SIDECAR: another engine build (a saved baseline, say).
  const child = Reflect.apply(spawnProcess, this, [engine && process.env.ELSECREW_REAL_SIDECAR ? process.env.ELSECREW_REAL_SIDECAR : command, args, options]);
  if (engine) {
    child.stderr?.on('data', chunk => fs.appendFileSync(path.join(report, 'engine-stderr.log'), chunk));
  }
  return child;
};

const log = [];
const note = (stage, detail) => { const line = { at: new Date().toISOString(), stage, ...detail }; log.push(line); console.log(JSON.stringify(line)); };
let finished = false;
const timer = setTimeout(() => finish(new Error('real run timeout')), 15 * 60 * 1000);
function finish(error, details = {}) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  const result = { root, error: error?.message, ...details, log, status: error ? 'FAIL' : 'PASS' };
  fs.writeFileSync(path.join(report, 'real-run.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ status: result.status, error: result.error, report }));
  app.once('will-quit', () => app.exit(error ? 1 : 0));
  app.quit();
}

app.on('browser-window-created', (_event, window) => {
  window.webContents.once('did-finish-load', async () => {
    const run = code => window.webContents.executeJavaScript(code);
    const sleep = ms => new Promise(r => setTimeout(r, ms));
    const wait = async (predicate, label, timeoutMs = 60000) => {
      const deadline = Date.now() + timeoutMs;
      while (Date.now() < deadline) { if (await run(predicate)) return; await sleep(250); }
      throw new Error(`timeout: ${label}`);
    };
    const created = [];
    try {
      await wait(`Boolean(window.m5Projects && window.m5) && document.body.innerText.length > 0`, 'app ready');
      await wait(`window.m5.getSystemStatus().then(s => s.state === 'ready' || s.state === 'shellReady').catch(() => false)`, 'engine ready', 90000);
      const before = await run(`window.m5Projects.request({action:'getProviderConnections'}).then(r => r.connections)`);
      note('connections', { before });
      if (before.codex !== true || before['claude-code'] !== true) throw new Error('both agents must be connected in the app');
      // Two projects: one whose coordinator is Claude Code, one Codex.
      const projects = {};
      for (const agent of ['claude', 'codex']) {
        const opened = await run(`window.m5.openProject()`);
        projects[agent] = opened.projectId;
        created.push(opened.projectId);
      }
      note('projects', { projects });
      const model = async runtime => run(`window.m5Models.request({runtime:${JSON.stringify(runtime)},refresh:false}).then(r => (r.models.find(m => m.isDefault) ?? r.models[0]))`);
      const models = { 'claude-code': await model('claude-code'), codex: await model('codex') };
      note('models', { claude: models['claude-code']?.modelSelector, codex: models.codex?.modelSelector });
      for (const [agent, runtime] of [['claude', 'claude-code'], ['codex', 'codex']]) {
        const chosen = models[runtime];
        await run(`(async () => {
          const projectId = ${JSON.stringify(projects[agent])};
          const settings = await window.m5.getProjectRoleSettings({ projectId });
          const models = ${JSON.stringify({ 'claude-code': models['claude-code'], codex: models.codex })};
          const effort = (runtime, model) => runtime === 'codex' ? (model.reasoningEfforts?.includes('low') ? 'low' : model.reasoningEfforts?.[0] ?? 'medium') : null;
          // Every member gets its agent's default model (the team must be complete); the coordinator this project's agent.
          const roles = settings.roles.map(role => role.roleId === 'coordinator'
            ? { ...role, runtime: ${JSON.stringify(runtime)}, modelSelector: ${JSON.stringify(chosen.modelSelector)}, reasoningEffort: effort(${JSON.stringify(runtime)}, models[${JSON.stringify(runtime)}]), permission: 'readOnly' }
            : { ...role, modelSelector: models[role.runtime].modelSelector, reasoningEffort: effort(role.runtime, models[role.runtime]) });
          await window.m5.updateProjectRoleSettings({ projectId, expectedRevision: settings.revision, roles, ...(settings.roleDefinitions ? { roleDefinitions: settings.roleDefinitions } : {}) });
        })()`);
      }
      // Two tasks per project.
      const tasks = [];
      // How long each answer is: Codex writes far slower than Claude Code
      // (about 290 s for three hundred numbers), so it counts less far.
      const upTo = { claude: process.env.ELSECREW_REAL_UPTO_CLAUDE ?? 'one thousand', codex: process.env.ELSECREW_REAL_UPTO_CODEX ?? 'three hundred' };
      // ELSECREW_REAL_AGENTS=codex (or claude) runs only that agent's two tasks.
      const only = process.env.ELSECREW_REAL_AGENTS;
      for (const agent of ['claude', 'codex'].filter(agent => !only || only === agent)) for (const n of [1, 2]) {
        const chatId = await run(`(async () => {
          const projectId = ${JSON.stringify(projects[agent])};
          const settings = await window.m5.getProjectRoleSettings({ projectId });
          const prepared = await window.m5.prepareChatWorkspace({ projectId });
          const m = prepared.manifest;
          const chat = await window.m5.createChat({ preparationId: prepared.preparationId, projectId, worktreeId: m.worktreeId, workspaceIdentity: m.workspaceIdentity,
            goal: ${JSON.stringify(`Parallel check ${n}: as your final answer, write the numbers from one to ${upTo[agent]} in English words, separated by commas. Do not delegate, do not read files.`)}, expectedSettingsRevision: settings.revision });
          return chat.chatId;
        })()`);
        tasks.push({ agent, n, chatId, runtime: agent === 'claude' ? 'claude-code' : 'codex' });
      }
      note('tasks', { tasks });
      // All four start at once (each checks its agent, then starts).
      const startedAt = Date.now();
      const starts = await run(`Promise.all(${JSON.stringify(tasks)}.map(async task => {
        const detail = await window.m5.getChat({ chatId: task.chatId, afterEventSequence: 0, cursor: null, limit: 50 });
        for (let attempt = 0; attempt < 40; attempt++) {
          try {
            const capability = await window.m5.getAgentCapabilities({ runtime: task.runtime, refresh: true, chatId: task.chatId, roleId: 'coordinator' });
            if (!capability.canStart) return { chatId: task.chatId, error: 'cannot start: ' + capability.blockedReason };
            const accepted = await window.m5.startAgentRun({ chatId: task.chatId, roleId: 'coordinator', inputMessageIds: [detail.messages[0].messageId], expectedCapabilityFingerprint: capability.fingerprint });
            return { chatId: task.chatId, runId: accepted.runId, attempt };
          } catch (error) {
            if (!/busy/i.test(String(error))) return { chatId: task.chatId, error: String(error) };
            await new Promise(r => setTimeout(r, 1000));
          }
        }
        return { chatId: task.chatId, error: 'stayed busy' };
      }))`);
      note('starts', { starts, ms: Date.now() - startedAt });
      if (starts.some(start => start.error)) throw new Error('a start failed: ' + JSON.stringify(starts.filter(start => start.error)));
      // While they work: a Claude sign-in check (as the settings screen asks
      // it) and model lists beside them.
      const claudeCheck = `(async () => {
        const attemptId = crypto.randomUUID();
        let status = await window.m5Login.request({ provider: 'claude', action: 'check', attemptId });
        for (let i = 0; i < 240 && status.attemptId === attemptId && ['diagnosing', 'starting', 'verifying'].includes(status.state); i++) {
          await new Promise(r => setTimeout(r, 500));
          status = await window.m5Login.request({ provider: 'claude', action: 'status' });
        }
        return status;
      })().catch(error => ({ error: String(error) }))`;
      const runStates = () => run(`Promise.all(${JSON.stringify(tasks)}.map(task => window.m5.getChat({ chatId: task.chatId, afterEventSequence: 0, cursor: null, limit: 1 }).then(d => ({ chatId: task.chatId, chat: d.chat.state, runs: (d.agentRuns ?? []).map(r => r.state) }))))`);
      let peak = 0;
      let besideChecked = false;
      const deadline = Date.now() + 10 * 60 * 1000;
      for (;;) {
        const states = await runStates();
        const live = states.filter(state => state.runs.some(r => ['preparing', 'running', 'cancelling'].includes(r))).length;
        peak = Math.max(peak, live);
        if (live >= 2 && !besideChecked) {
          besideChecked = true;
          const check = await run(claudeCheck);
          const lists = await run(`Promise.all(['claude-code','codex'].map(runtime => window.m5Models.request({ runtime, refresh: true }).then(r => r.models.length).catch(e => String(e))))`);
          note('beside runs', { live, claudeCheck: check, modelLists: lists });
        }
        if (states.every(state => state.runs.length > 0 && state.runs.every(r => !['preparing', 'running', 'cancelling'].includes(r))) || Date.now() > deadline) {
          note('ended', { states, peakLiveTasks: peak, ms: Date.now() - startedAt });
          // What each run ended with (before the test projects go away).
          const endings = await run(`Promise.all(${JSON.stringify(tasks)}.map(task => window.m5.getChat({ chatId: task.chatId, afterEventSequence: 0, cursor: null, limit: 50 }).then(d => ({
            chatId: task.chatId, runtime: task.runtime,
            runs: JSON.stringify(d.agentRuns ?? []).slice(0, 2000),
            messages: (d.messages ?? []).slice(-4).map(m => ({ role: m.role ?? m.authorRole ?? m.kind, text: String(m.text ?? m.body ?? '').slice(0, 300) })),
          }))))`);
          note('endings', { endings });
          const failed = states.filter(state => !state.runs.every(r => r === 'succeeded'));
          const after = await run(`window.m5Projects.request({action:'getProviderConnections'}).then(r => r.connections)`);
          const claudeAfter = await run(claudeCheck);
          note('after', { connections: after, claudeCheck: claudeAfter });
          // Clean up the two test projects.
          for (const projectId of created) {
            await run(`window.m5Projects.request({ action: 'deleteProject', projectId: ${JSON.stringify(projectId)}, confirmed: true }).catch(e => String(e))`);
          }
          await sleep(3000);
          await run(`window.m5Projects.request({ action: 'finishDeletions' }).catch(() => null)`);
          if (failed.length) return finish(new Error('not every run succeeded: ' + JSON.stringify(failed)), { states, peak });
          // Whether the two runs of each agent worked at the same time, from
          // their records (started, last changed).
          const spans = endings.map(ending => {
            const run = JSON.parse(ending.runs.endsWith(']') ? ending.runs : '[]')[0] ?? {};
            return { runtime: ending.runtime, from: Date.parse(run.createdAt), to: Date.parse(run.updatedAt) };
          });
          const together = Object.fromEntries(['claude-code', 'codex'].map(runtime => {
            const [a, b] = spans.filter(span => span.runtime === runtime);
            return [runtime, a && b ? Math.max(0, Math.min(a.to, b.to) - Math.max(a.from, b.from)) / 1000 : null];
          }).filter(([, seconds]) => seconds !== null));
          const atOnce = Math.max(...spans.map(span => spans.filter(other => other.from <= span.from && span.from < other.to).length));
          note('together', { spans, secondsTogether: together, mostAtOnce: atOnce, peakPolled: peak });
          if (!Object.values(together).every(seconds => seconds > 0)) return finish(new Error('the two runs of an agent did not work at the same time: ' + JSON.stringify(together)), { states, peak, together, atOnce });
          if (claudeAfter.state !== 'connected') return finish(new Error('Claude sign-in not connected afterwards: ' + JSON.stringify(claudeAfter)), { states, peak });
          return finish(null, { states, peak, together, atOnce });
        }
        await sleep(1000);
      }
    } catch (error) {
      for (const projectId of created) {
        try { await run(`window.m5Projects.request({ action: 'deleteProject', projectId: ${JSON.stringify(projectId)}, confirmed: true }).catch(e => String(e))`); } catch { /* best effort */ }
      }
      finish(error);
    }
  });
});

require(path.join(repoRoot, '.vite/build/main.cjs'));
