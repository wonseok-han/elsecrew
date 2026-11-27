// Response time of the light reads while runs start (ADE stabilization A):
// file read, approval list and getChat, measured from the screen while two
// Claude Code and two Codex runs start at once, beside one Codex run that is
// already working ("warm"). Before the fix a start held the engine's one loop
// for 13–18 s and four starts held every request for over a minute.
//
// Real accounts and providers: the development app's own data (where its
// sign-ins live). Run only when no other Elsecrew app or engine uses it.
// Paid model calls: five tiny prompts, each run stopped once it has started.
// The two test projects are deleted at the end.
//
// Started by `node scripts/m5-latency-run.mjs --starting` (its --sidecar
// option points this at another engine build, a saved baseline, say):
//   LATENCY_ROOT=<dir> electron scripts/m5-latency-starting-probe.cjs
'use strict';
const { app, dialog } = require('electron');
const cp = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createSampler } = require('./m5-latency-sampler.cjs');

const repoRoot = path.resolve(__dirname, '..');
const root = process.env.LATENCY_ROOT || fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-latency-starting-'));
const timingLog = path.join(root, 'sidecar-timing-starting.jsonl');
const resultFile = path.join(root, 'result-starting.json');
const controlSamples = Number(process.env.LATENCY_SAMPLES || 12);
// How often the screen sends the three reads (open loop).
const TICK_MS = 1000;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

const work = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-starting-projects-')));
const repo = name => {
  const dir = path.join(work, name);
  fs.mkdirSync(dir);
  fs.writeFileSync(path.join(dir, 'README.md'), `# ${name}\n\nA tiny test project.\n`);
  const env = { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' };
  for (const args of [['init', '-q', '--template='], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'start']]) cp.execFileSync('/usr/bin/git', args, { cwd: dir, env });
  return dir;
};
const picks = [repo('starting-claude'), repo('starting-codex')];
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [picks.shift()] });
dialog.showMessageBox = async () => ({ response: 1, checkboxChecked: false });
// The development app's own data, as `pnpm start` uses it.
app.setPath('userData', path.join(os.homedir(), 'Library', 'Application Support', 'Elsecrew'));

const engineStarts = [];
const spawn = cp.spawn;
cp.spawn = function (command, args, options) {
  if (typeof command === 'string' && path.basename(command) === 'elsecrew-sidecar' && args?.[0] === '--stdio') {
    engineStarts.push({ at: Date.now() });
    const binary = process.env.LATENCY_SIDECAR || command;
    return Reflect.apply(spawn, this, [binary, args, { ...options, env: { ...(options?.env ?? process.env), ELSECREW_SIDECAR_TIMING_LOG: timingLog } }]);
  }
  return Reflect.apply(spawn, this, [command, args, options]);
};

const report = { stage: 'starting', root, engine: process.env.LATENCY_SIDECAR || 'target/debug/elsecrew-sidecar', engineStarts, rounds: [], measurements: [], starts: [], notes: [] };
const persist = () => fs.writeFileSync(resultFile, JSON.stringify(report, null, 2));
let finished = false;
function finish(error) {
  if (finished) return;
  finished = true;
  if (error) report.error = String(error.stack || error);
  report.status = error ? 'FAIL' : 'DONE';
  persist();
  console.log(JSON.stringify({ stage: 'starting', status: report.status, error: report.error?.slice(0, 400), resultFile }));
  app.once('will-quit', () => app.exit(error ? 1 : 0));
  setTimeout(() => app.quit(), 500);
  setTimeout(() => app.exit(error ? 1 : 0), 60_000).unref();
}
setTimeout(() => finish(new Error('probe timeout')), 15 * 60_000).unref();

app.on('browser-window-created', (_event, window) => {
  window.webContents.once('did-finish-load', () => { run(window).then(() => finish(null), finish); });
});

async function run(window) {
  const page = code => window.webContents.executeJavaScript(code);
  // The screen's timers must keep their pace while the window is not in front.
  window.webContents.setBackgroundThrottling(false);
  window.show();
  const wait = async (expression, label, ms = 60_000) => {
    const end = Date.now() + ms;
    for (;;) { if (await page(expression).catch(() => false)) return; if (Date.now() > end) throw new Error(`timeout: ${label}`); await sleep(250); }
  };
  const created = [];
  try {
    await wait(`Boolean(window.m5Projects && window.m5) && document.body.innerText.length > 0`, 'app ready');
    // Until storage admission is done (a start without the last clean
    // shutdown's seal checks the whole database first).
    await wait(`window.m5.getSystemStatus().then(s => s.storageAdmission ? s.storageAdmission.state === 'ready' : s.state === 'ready')`, 'engine ready', 600_000);
    const connections = await page(`window.m5Projects.request({action:'getProviderConnections'}).then(r => r.connections)`);
    if (connections.codex !== true || connections['claude-code'] !== true) throw new Error('both agents must be connected in the app');
    const projects = {};
    for (const agent of ['claude', 'codex']) { const opened = await page(`window.m5.openProject()`); projects[agent] = opened.projectId; created.push(opened.projectId); }
    const model = runtime => page(`window.m5Models.request({runtime:${JSON.stringify(runtime)},refresh:false}).then(r => (r.models.find(m => m.isDefault) ?? r.models[0]))`);
    const models = { 'claude-code': await model('claude-code'), codex: await model('codex') };
    for (const [agent, runtime] of [['claude', 'claude-code'], ['codex', 'codex']]) {
      await page(`(async () => {
        const projectId = ${JSON.stringify(projects[agent])};
        const settings = await window.m5.getProjectRoleSettings({ projectId });
        const models = ${JSON.stringify(models)};
        const effort = (runtime, model) => runtime === 'codex' ? (model.reasoningEfforts?.includes('low') ? 'low' : model.reasoningEfforts?.[0] ?? 'medium') : null;
        const roles = settings.roles.map(role => role.roleId === 'coordinator'
          ? { ...role, runtime: ${JSON.stringify(runtime)}, modelSelector: models[${JSON.stringify(runtime)}].modelSelector, reasoningEffort: effort(${JSON.stringify(runtime)}, models[${JSON.stringify(runtime)}]), permission: 'readOnly' }
          : { ...role, modelSelector: models[role.runtime].modelSelector, reasoningEffort: effort(role.runtime, models[role.runtime]) });
        await window.m5.updateProjectRoleSettings({ projectId, expectedRevision: settings.revision, roles, ...(settings.roleDefinitions ? { roleDefinitions: settings.roleDefinitions } : {}) });
      })()`);
    }
    const task = (agent, goal) => page(`(async () => {
      const projectId = ${JSON.stringify(projects[agent])};
      const settings = await window.m5.getProjectRoleSettings({ projectId });
      const prepared = await window.m5.prepareChatWorkspace({ projectId });
      const m = prepared.manifest;
      const chat = await window.m5.createChat({ preparationId: prepared.preparationId, projectId, worktreeId: m.worktreeId, workspaceIdentity: m.workspaceIdentity, goal: ${JSON.stringify(goal)}, expectedSettingsRevision: settings.revision });
      return { chatId: chat.chatId, runtime: ${JSON.stringify(agent === 'claude' ? 'claude-code' : 'codex')} };
    })()`);
    const startFn = `(async t => {
      const detail = await window.m5.getChat({ chatId: t.chatId, afterEventSequence: 0, cursor: null, limit: 50 });
      for (let attempt = 0; attempt < 40; attempt++) {
        try {
          const capability = await window.m5.getAgentCapabilities({ runtime: t.runtime, refresh: true, chatId: t.chatId, roleId: 'coordinator' });
          if (!capability.canStart) return { chatId: t.chatId, error: 'cannot start: ' + capability.blockedReason };
          const accepted = await window.m5.startAgentRun({ chatId: t.chatId, roleId: 'coordinator', inputMessageIds: [detail.messages[0].messageId], expectedCapabilityFingerprint: capability.fingerprint });
          return { chatId: t.chatId, runtime: t.runtime, accepted, attempt };
        } catch (error) {
          if (!/busy/i.test(String(error))) return { chatId: t.chatId, error: String(error).slice(0, 300) };
          await new Promise(r => setTimeout(r, 1000));
        }
      }
      return { chatId: t.chatId, error: 'stayed busy' };
    })`;
    const start = chat => `${startFn}(${JSON.stringify(chat)})`;

    // The warm run: one Codex run working while the others start.
    // Codex writes a long answer slowly (one to three hundred: about five
    // minutes), so the warm run outlives the starts; a Claude run answered
    // a long list in seconds (PR #25 review I5: the load must hold).
    const warm = await task('codex', 'Warm run: as your final answer, write the numbers from one to three hundred in English words, separated by commas. Do not delegate, do not read files.');
    const warmStart = await page(start(warm));
    if (warmStart.error) throw new Error('warm start: ' + warmStart.error);
    const identity = Object.fromEntries(['chatId', 'taskId', 'projectId', 'worktreeId', 'workspaceIdentity', 'runId'].map(key => [key, warmStart.accepted[key]]));
    report.warmIdentity = identity;
    const runLive = () => page(`window.m5.getChat({ chatId: ${JSON.stringify(identity.chatId)}, afterEventSequence: 0, cursor: null, limit: 1 }).then(d => (d.agentRuns ?? []).some(r => r.runId === ${JSON.stringify(identity.runId)} && r.state === 'running'), () => false)`);
    await wait(`window.m5.getChat({ chatId: ${JSON.stringify(identity.chatId)}, afterEventSequence: 0, cursor: null, limit: 1 }).then(d => (d.agentRuns ?? []).some(r => r.state === 'running'))`, 'warm running', 90_000);
    const starters = [await task('claude', 'Reply with the single word OK.'), await task('claude', 'Reply with the single word OK.'), await task('codex', 'Reply with the single word OK.'), await task('codex', 'Reply with the single word OK.')];

    // Open-loop sampling: every TICK_MS the screen sends the three reads
    // without waiting for earlier ones, as a person clicking would. A hold of
    // the engine then shows as every read sent during it waiting, not as one
    // slow sample (a closed loop would hide it).
    const read = `window.m5.readWorkspaceFile({ chatId: ${JSON.stringify(identity.chatId)}, projectId: ${JSON.stringify(identity.projectId)}, worktreeId: ${JSON.stringify(identity.worktreeId)}, workspaceIdentity: ${JSON.stringify(identity.workspaceIdentity)}, path: 'README.md' })`;
    await page(`void (window.__sampler = (${createSampler.toString()})({
      now: () => performance.timeOrigin + performance.now(),
      setTimeout: (fn, ms) => setTimeout(fn, ms),
      clearTimeout: id => clearTimeout(id),
      startsPending: () => Boolean(window.__starting && window.__starting.pending > 0),
      tickMs: ${TICK_MS},
      reads: {
        getChat: () => window.m5.getChat({ chatId: ${JSON.stringify(identity.chatId)}, afterEventSequence: 0, cursor: null, limit: 50 }),
        'approval:list': () => window.m5Approval.request({ action: 'list', identity: ${JSON.stringify(identity)} }),
        'file:read': () => ${read},
      },
    }))`);
    const settle = () => wait(`window.__sampler.outstanding === 0`, 'reads settled', 180_000);

    // Control: the warm run alone.
    const control = { condition: 'warm', start: Date.now() };
    report.rounds.push(control);
    const warmBefore = await runLive();
    await page(`window.__sampler.begin('warm')`);
    await wait(`(window.__sampler.ticks.warm ?? 0) >= ${controlSamples}`, 'warm samples', 120_000);
    await page(`window.__sampler.end()`);
    await settle();
    control.end = Date.now();
    control.warmHeld = warmBefore && await runLive();

    // Four starts at once; sample until every one has answered.
    const round = { condition: 'starting', start: Date.now() };
    report.rounds.push(round);
    await page(`window.__starting = { pending: 4, results: [] }; window.__sampler.begin('starting'); ${JSON.stringify(starters)}.forEach(t => ${startFn}(t).then(r => { window.__starting.results.push({ ...r, at: Date.now() }); window.__starting.pending--; }));`);
    await wait(`window.__starting.pending === 0`, 'starts answered', 10 * 60_000);
    await page(`window.__sampler.end()`);
    await settle();
    round.end = Date.now();
    round.warmStillRunning = await runLive();
    const sampled = await page(`({ measurements: window.__sampler.measurements, ticks: window.__sampler.ticks })`);
    for (const m of sampled.measurements) {
      // "warm": the warm run held for the whole round; "starting": a start
      // was still pending when the read was sent (a read the starts held
      // until they all ended is exactly what this measures).
      m.loaded = m.condition === 'warm' ? control.warmHeld === true : m.startsBefore === true;
      report.measurements.push(m);
    }
    for (const r of [control, round]) {
      const own = sampled.measurements.filter(m => m.condition === r.condition);
      r.samples = sampled.ticks[r.condition] ?? 0;
      r.loadedSamples = Math.min(...['getChat', 'approval:list', 'file:read'].map(label => own.filter(m => m.label === label && m.loaded).length));
    }
    report.starts = await page(`window.__starting.results`);
    report.startsMs = Math.max(...report.starts.map(r => r.at)) - round.start;
    persist();
    if (report.starts.some(r => r.error)) report.notes.push({ startErrors: report.starts.filter(r => r.error) });

    // Stop every run (they were started only to be started), then clean up.
    for (const chat of [warm, ...starters]) {
      await page(`(async () => {
        const detail = await window.m5.getChat({ chatId: ${JSON.stringify(chat.chatId)}, afterEventSequence: 0, cursor: null, limit: 50 });
        const c = detail.chat, trigger = [...detail.messages].reverse().find(m => m.kind === 'user');
        return window.m5Projects.request({ action: 'cancelTeamTask', chatId: c.chatId, taskId: c.taskId, projectId: c.projectId, worktreeId: c.worktreeId, workspaceIdentity: c.workspaceIdentity, triggerMessageId: trigger.messageId });
      })().catch(e => String(e))`);
    }
    await wait(`Promise.all(${JSON.stringify([warm, ...starters])}.map(t => window.m5.getChat({ chatId: t.chatId, afterEventSequence: 0, cursor: null, limit: 1 }).then(d => (d.agentRuns ?? []).every(r => !['preparing', 'running', 'cancelling'].includes(r.state))))).then(all => all.every(Boolean))`, 'runs stopped', 120_000).catch(error => report.notes.push({ stop: String(error) }));
  } finally {
    for (const projectId of created) await page(`window.m5Projects.request({ action: 'deleteProject', projectId: ${JSON.stringify(projectId)}, confirmed: true }).catch(e => String(e))`).catch(() => null);
    await sleep(3000);
    await page(`window.m5Projects.request({ action: 'finishDeletions' }).catch(() => null)`).catch(() => null);
    // Engine view of each round: requests of the measured methods in its window.
    const requests = [];
    const phases = [];
    if (fs.existsSync(timingLog)) for (const line of fs.readFileSync(timingLog, 'utf8').split('\n')) { if (!line) continue; try { const v = JSON.parse(line); (v.kind === 'request' ? requests : phases).push(v); } catch { /* partial */ } }
    const pct = (values, p) => { if (!values.length) return null; const sorted = [...values].sort((x, y) => x - y); return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)]; };
    for (const round of report.rounds) {
      round.engine = {};
      for (const method of ['workspace.file.read', 'chat.get', 'run.approval']) {
        const seen = requests.filter(v => v.method === method && v.at >= round.start && v.at <= (round.end ?? Date.now()));
        if (seen.length) round.engine[method] = { count: seen.length, queueP95: pct(seen.map(v => v.queueMs), 95), queueMax: Math.max(...seen.map(v => v.queueMs)), handleP95: pct(seen.map(v => v.handleMs), 95) };
      }
    }
    report.phases = phases.map(({ at, phase, ms }) => ({ at, phase, ms }));
    persist();
  }
}

console.log(JSON.stringify({ stage: 'starting', root }));
require(process.env.LATENCY_MAIN || path.join(repoRoot, '.vite/build/main.cjs'));
