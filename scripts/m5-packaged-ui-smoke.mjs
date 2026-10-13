// Observe the actual packaged renderer and native sidecar. No bridge mocks,
// user project imports, logins, model calls or forced termination. --project
// edits only its newly generated project's isolated working copy.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import { createRequire } from 'node:module';
import { parseCurrentSystemStatusV1 } from '../src/m5/contracts/lifecycle-g5d-v1.ts';
import { seedPackagedProject } from './m5-packaged-project-seed.mjs';

const { processTreeMemory } = createRequire(import.meta.url)('./m5-process-memory.cjs');
const repo = fileURLToPath(new URL('..', import.meta.url));
assert.equal(process.platform, 'darwin');
assert.equal(process.arch, 'arm64');
const projectFlow = process.argv.length === 3 && process.argv[2] === '--project';
assert.ok(process.argv.length === 2 || projectFlow, 'Only the fixed --project scenario is accepted');
const bundle = path.join(repo, 'out/hello-agent-darwin-arm64/hello-agent.app/Contents');
const executable = path.join(bundle, 'MacOS/hello-agent');
const resources = path.join(bundle, 'Resources');
for (const file of [executable, path.join(resources, 'app.asar'), path.join(resources, 'sidecar/hello-agent-sidecar')]) {
  assert.ok(fs.lstatSync(file).isFile(), 'Package is missing a regular artifact');
}
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hello-agent-packaged-ui-'));
const userData = path.join(root, 'userdata');
fs.mkdirSync(userData, {mode: 0o700});
const git = (...args) => execFileSync('/usr/bin/git', args, {cwd: repo, encoding: 'utf8'}).trim();
const report = {
  schemaVersion: 1, diagnostic: true, gateEligible: false, status: 'RUNNING',
  root, head: git('rev-parse', 'HEAD'), gitStatusBefore: git('status', '--porcelain'),
  measuredAt: new Date().toISOString(), packagedApp: true, userData,
  asarSha256: createHash('sha256').update(fs.readFileSync(path.join(resources, 'app.asar'))).digest('hex'),
  sidecarSha256: createHash('sha256').update(fs.readFileSync(path.join(resources, 'sidecar/hello-agent-sidecar'))).digest('hex'),
  limitations: ['Single cold startup with migration and loopback CDP instrumentation.',
    'No native keyboard/focus, models, stress fixtures or whole G6 evidence.',
    'RSS includes current descendants only, not escaped processes.'],
  memory: [], shutdown: 'pending',
};
const persist = () => fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(report, null, 2));
persist();
const env = {...process.env};
delete env.ELECTRON_RUN_AS_NODE;
delete env.NODE_OPTIONS;
if (projectFlow) {
  try {
    report.project = await seedPackagedProject(path.join(resources, 'sidecar/hello-agent-sidecar'), root, userData, env);
    persist();
  } catch (error) {
    report.status = 'FAIL'; report.error = String(error); persist();
    throw error; // Do not start an app after an uncertain seed shutdown.
  }
}
const started = performance.now();
const child = spawn(executable, [`--user-data-dir=${userData}`, '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1'], {
  cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'],
});
report.pid = child.pid;
persist();
console.log(JSON.stringify({stage: 'STARTED', root, pid: child.pid}));
let termination;
const closed = new Promise(resolve => {
  child.once('error', error => { termination = {error: String(error)}; resolve(termination); });
  child.once('close', (code, signal) => { termination = {code, signal}; resolve(termination); });
});
let stderr = '';
child.stderr.setEncoding('utf8');
child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-65536); });
child.stdout.resume();
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(read, milliseconds = 15000) {
  const deadline = performance.now() + milliseconds;
  do {
    if (termination) throw new Error('Packaged app exited before observation completed');
    const value = await read();
    if (value) return value;
    await sleep(100);
  } while (performance.now() < deadline);
  throw new Error('Packaged observation deadline exceeded');
}
let socket, sessionId, nextId = 0;
const pending = new Map();
function send(method, params = {}, targetSession) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 10000);
    pending.set(id, {resolve, reject, timer});
    try { socket.send(JSON.stringify({id, method, params, ...(targetSession ? {sessionId: targetSession} : {})})); }
    catch (error) { clearTimeout(timer); pending.delete(id); reject(error); }
  });
}
async function evaluate(expression) {
  const reply = await send('Runtime.evaluate', {expression, awaitPromise: true, returnByValue: true}, sessionId);
  if (reply.exceptionDetails) throw new Error('Packaged renderer expression failed');
  return reply.result?.value;
}
try {
  const endpoint = await until(() => stderr.match(/DevTools listening on (ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[a-f0-9-]+)/)?.[1]);
  socket = new WebSocket(endpoint);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('CDP connection timeout')), 10000);
    socket.addEventListener('open', () => {clearTimeout(timer); resolve();}, {once: true});
    socket.addEventListener('error', () => {clearTimeout(timer); reject(new Error('CDP connection failed'));}, {once: true});
  });
  socket.addEventListener('message', event => {
    const reply = JSON.parse(event.data);
    const request = pending.get(reply.id);
    if (!request) return;
    pending.delete(reply.id); clearTimeout(request.timer);
    if (reply.error) request.reject(new Error(`CDP failure: ${reply.error.code}`));
    else request.resolve(reply.result);
  });
  const target = await until(async () => (await send('Target.getTargets')).targetInfos.find(item => item.type === 'page' && item.url === 'app://m5/index.html'));
  ({sessionId} = await send('Target.attachToTarget', {targetId: target.targetId, flatten: true}));
  const status = await until(async () => {
    const raw = await evaluate('window.m5 ? window.m5.getSystemStatus() : null');
    if (!raw || raw.state === 'starting') return null;
    const parsed = parseCurrentSystemStatusV1(raw, false, true);
    return parsed.storageAdmission.state === 'ready' ? parsed : null;
  });
  report.sidecarInstanceId = status.sidecar.sidecarInstanceId;
  report.storageObservedMs = performance.now() - started;
  // The database must exist under our explicit profile before further probes.
  const database = path.join(userData, 'com.wonseokhan.helloagent/m5/hello-agent.sqlite3');
  assert.ok(fs.lstatSync(database).isFile(), 'Packaged app did not use the isolated profile');
  report.ui = await until(() => evaluate(`(() => {
    const root = document.querySelector('#root');
    const choose = document.querySelector('.m5-start-action button');
    const buttons = [...document.querySelectorAll('button')].filter(b => !b.disabled && b.getClientRects().length);
    return root?.childElementCount && choose && !choose.disabled && choose.getClientRects().length
      ? {enabledButtons: buttons.length, projectSelectionEnabled: true, rootChildren: root.childElementCount, origin: location.origin} : null;
  })()`));
  report.uiObservedMs = performance.now() - started;
  // Attribute RSS without guessing helper roles from PID order. CDP describes
  // Chromium processes only; unmatched descendants remain explicitly unknown.
  report.chromiumProcessesBefore = (await send('SystemInfo.getProcessInfo')).processInfo;
  report.rendererHeapBefore = await send('Runtime.getHeapUsage', {}, sessionId);
  report.rendererAssets = await evaluate(`performance.getEntriesByType('resource')
    .filter(entry => entry.name.startsWith('app://m5/'))
    .map(entry => ({path: new URL(entry.name).pathname, decodedBodySize: entry.decodedBodySize}))`);
  for (let index = 0; index < 40; index++) {
    assert.ok(!termination, 'App exited during RSS collection');
    const rows = execFileSync('/bin/ps', ['-axo', 'pid=,ppid=,rss='], {encoding: 'utf8', timeout: 5000, maxBuffer: 4 * 1024 * 1024});
    report.memory.push({elapsedMs: performance.now() - started, ...processTreeMemory(rows, child.pid)});
    await sleep(250);
  }
  report.idleRssBytes = Math.max(...report.memory.map(sample => sample.rssBytes));
  report.idleRssWithinLimit = report.idleRssBytes <= 400 * 1024 * 1024;
  report.chromiumProcessesAfter = (await send('SystemInfo.getProcessInfo')).processInfo;
  report.rendererHeapAfter = await send('Runtime.getHeapUsage', {}, sessionId);
  if (projectFlow) {
    const expectedPath = fs.realpathSync(report.project.source);
    await until(() => evaluate(`(() => {
      const button = [...document.querySelectorAll('.m5-project-catalog-list li > button')]
        .find(item => item.title === ${JSON.stringify(expectedPath)} && !item.disabled);
      if (!button) return false;
      button.click(); return true;
    })()`));
    await until(() => evaluate(`!![...document.querySelectorAll('.m5-files button')]
      .find(button => button.textContent.includes('hello.txt') && !button.disabled)`));
    await evaluate(`document.querySelector('.m5-new-chat-button').click()`);
    await until(() => evaluate(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`));
    await evaluate(`(() => {
      const input = document.querySelector('.m5-new-chat-form input:not([type=checkbox])');
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, 'Packaged editing check');
      input.dispatchEvent(new Event('input', {bubbles:true}));
      document.querySelector('.m5-new-chat-form input[type=checkbox]').click();
    })()`);
    await until(() => evaluate(`!document.querySelector('.m5-new-chat-form button[type=submit]').disabled`));
    await evaluate(`document.querySelector('.m5-new-chat-form button[type=submit]').click()`);
    await until(() => evaluate(`document.querySelector('.m5-timeline h2')?.textContent === 'Packaged editing check'`));
    await evaluate(`[...document.querySelectorAll('.m5-files button')].find(button => button.textContent.includes('hello.txt')).click()`);
    report.project.editor = await until(() => evaluate(`(() => {
      const editor = document.querySelector('.monaco-editor');
      return editor && editor.innerText.includes('original') ? {opened: true, text: 'original'} : null;
    })()`));
    await evaluate(`document.querySelector('.monaco-editor textarea').focus()`);
    await send('Input.insertText', {text:'edited in packaged snapshot\n'}, sessionId);
    await until(() => evaluate(`!![...document.querySelectorAll('button')].find(button => button.textContent === '저장' && !button.disabled)`));
    await evaluate(`[...document.querySelectorAll('button')].find(button => button.textContent === '저장').click()`);
    await until(() => evaluate(`!![...document.querySelectorAll('button')].find(button => button.textContent === '저장' && button.disabled)`));
    await evaluate(`[...document.querySelectorAll('.m5-chat-review button')].find(button => button.textContent === '작업 사본 diff 열기').click()`);
    await until(() => evaluate(`!![...document.querySelectorAll('.m5-change-row')].find(button => button.title === 'hello.txt' && !button.disabled)`));
    await evaluate(`[...document.querySelectorAll('.m5-change-row')].find(button => button.title === 'hello.txt').click()`);
    await until(() => evaluate(`document.querySelector('.m5-diff-details')?.textContent.includes('edited in packaged snapshot')`));
    report.project.savedDiffVisible = true;
    assert.equal(fs.readFileSync(path.join(report.project.source, 'hello.txt'), 'utf8'), 'original\n');
    report.project.originalUnchanged = true;
  }
  const capture = await send('Page.captureScreenshot', {format: 'png'}, sessionId);
  fs.writeFileSync(path.join(root, 'startup.png'), Buffer.from(capture.data, 'base64'));
} catch (error) { report.error = String(error); }
finally {
  // Existing IPC invokes the normal main-owned shutdown coordinator. A lost
  // reply during exit is not success: require exit 0 and the durable DB marker.
  if (sessionId && !termination) {
    try { await evaluate('window.m5.prepareQuit()'); }
    catch (error) { report.quitReplyError = String(error); }
  }
  let exitTimer;
  const exit = await Promise.race([closed, new Promise(resolve => { exitTimer = setTimeout(() => resolve(null), 20000); })]);
  clearTimeout(exitTimer);
  if (exit?.code === 0 && exit.signal === null) {
    try {
      const db = new DatabaseSync(path.join(userData, 'com.wonseokhan.helloagent/m5/hello-agent.sqlite3'), {readOnly: true});
      try {
        assert.equal(typeof db.prepare('SELECT clean_shutdown_at FROM app_instances WHERE sidecar_instance_id = ?').get(report.sidecarInstanceId)?.clean_shutdown_at, 'string');
      } finally { db.close(); }
      report.shutdown = 'confirmed';
    } catch (error) { report.shutdownError = String(error); }
  }
  report.termination = exit;
  report.status = report.error || report.shutdown !== 'confirmed' || report.idleRssWithinLimit === false ? 'FAIL' : 'NOT_READY';
  report.headAfter = git('rev-parse', 'HEAD');
  report.gitStatusAfter = git('status', '--porcelain');
  persist();
  socket?.close();
  for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error('Observation ended')); }
  pending.clear();
  console.log(JSON.stringify({root, status: report.status, shutdown: report.shutdown, pid: child.pid, idleRssBytes: report.idleRssBytes, error: report.error}));
  // Retain failed/uncertain data. Never kill an app or remove its profile here.
  process.exitCode = report.status === 'FAIL' ? 1 : 2;
}
