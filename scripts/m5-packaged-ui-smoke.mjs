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
import { probePackagedApplication } from './m5-packaged-application-probe.mjs';
import { probeLargeProject } from './m5-packaged-large-project-probe.mjs';
import { probePackagedInput } from './m5-packaged-input-probe.mjs';
import { startPackagedChatStream } from './m5-packaged-chat-stream-probe.mjs';
import { beginLongTaskProbe, endLongTaskProbe } from './m5-packaged-long-task-probe.mjs';
import { createMainTaskTrace } from './m5-main-task-trace.mjs';
import { readPackagedStartupMark } from './m5-packaged-startup-probe.mjs';
import { probeLargeDiff } from './m5-packaged-diff-probe.mjs';
import diffFixture from '../tests/m5-native-diff-fixture.cjs';

const { createProcessMemoryTracker, summarizeMemory } = createRequire(import.meta.url)('./m5-process-memory.cjs');
const repo = fileURLToPath(new URL('..', import.meta.url));
assert.equal(process.platform, 'darwin');
assert.equal(process.arch, 'arm64');
const largeProject = process.argv.length === 3 && process.argv[2] === '--large-project';
const memoryDiff = process.argv.length === 3 && process.argv[2] === '--memory-diff';
const startupSeries = process.argv.length === 3 && process.argv[2] === '--startup';
const deferredCdp = process.argv.length === 3 && process.argv[2] === '--deferred-cdp';
const mainTraceDiagnostic = process.argv.length === 3 && process.argv[2] === '--main-trace';
const nativeChatStream = process.argv.length === 3 && process.argv[2] === '--native-chat-stream';
const largeDiff = memoryDiff || (process.argv.length === 3 && process.argv[2] === '--large-diff');
const diagnosticLanguage = process.argv.length === 3
  ? (process.argv[2] === '--english' ? 'en' : process.argv[2] === '--korean' ? 'ko' : undefined)
  : undefined;
const projectFlow = nativeChatStream || largeDiff || largeProject || (process.argv.length === 3 && process.argv[2] === '--project');
assert.ok(process.argv.length === 2 || projectFlow || diagnosticLanguage || startupSeries || deferredCdp || mainTraceDiagnostic, 'Only fixed diagnostic scenarios are accepted');
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
    'RSS retains observed descendants after reparenting using second-resolution ps start times; unseen short-lived children and same-second PID reuse remain unverified.'],
  memory: [], shutdown: 'pending',
  requestedLanguage: diagnosticLanguage ?? null,
};
if (diagnosticLanguage) report.limitations.push('Language selected through the real UI before RSS sampling; not a cold per-language startup.');
const persist = () => fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(report, null, 2));
persist();
const env = {...process.env};
delete env.ELECTRON_RUN_AS_NODE;
delete env.NODE_OPTIONS;
if (projectFlow) {
  try {
    report.project = await seedPackagedProject(path.join(resources, 'sidecar/hello-agent-sidecar'), root, userData, env, largeProject, largeDiff);
    persist();
  } catch (error) {
    report.status = 'FAIL'; report.error = String(error); persist();
    throw error; // Do not start an app after an uncertain seed shutdown.
  }
}
async function observe(report, resume = false, resultFile = resume ? 'restart.json' : 'result.json') {
const persist = () => fs.writeFileSync(path.join(root, resultFile), JSON.stringify(report, null, 2));
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
let longTaskProbeStarted = false, longTaskWindowCompleted = false;
const mainTaskTrace = createMainTaskTrace(child.pid);
let mainTaskTraceStarted = false;
let mainTaskWindowCompleted = false;
let memoryTimer, memoryPhase = 'loaded';
const trackMemory = createProcessMemoryTracker(child.pid);
const readMemory = () => trackMemory(execFileSync('/bin/ps', ['-axo', 'pid=,ppid=,rss=,lstart='],
  {encoding:'utf8', timeout:5000, maxBuffer:4 * 1024 * 1024, env:{...process.env, LC_ALL:'C'}}));
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
  if (deferredCdp) {
    report.preAttachmentMemory = [];
    report.limitations.push('CDP server is enabled at launch; only client attachment is delayed. Pre-attachment samples do not establish UI readiness.');
    for (let index = 0; index < 48; index++) {
      assert.ok(!termination, 'App exited before delayed CDP attachment');
      report.preAttachmentMemory.push({elapsedMs:performance.now() - started, ...readMemory()});
      await sleep(250);
    }
    report.preAttachmentMaximumRssBytes = Math.max(...report.preAttachmentMemory.map(sample => sample.rssBytes));
    report.cdpAttachmentStartedAtMs = performance.now() - started;
    persist();
  }
  const endpoint = await until(() => stderr.match(/DevTools listening on (ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[a-f0-9-]+)/)?.[1]);
  socket = new WebSocket(endpoint);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('CDP connection timeout')), 10000);
    socket.addEventListener('open', () => {clearTimeout(timer); resolve();}, {once: true});
    socket.addEventListener('error', () => {clearTimeout(timer); reject(new Error('CDP connection failed'));}, {once: true});
  });
  socket.addEventListener('message', event => {
    const reply = JSON.parse(event.data);
    mainTaskTrace.accept(reply);
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
  if (resume) assert.notEqual(report.sidecarInstanceId, report.previousSidecarInstanceId, 'Restart must create a new sidecar instance');
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
  report.rendererStartupMark = await until(() => evaluate(`(${readPackagedStartupMark.toString()})()`));
  assert.ok(!report.rendererStartupMark.error, report.rendererStartupMark.error);
  if (diagnosticLanguage) {
    await evaluate(`(async () => {
      const select = document.querySelector('select:has(option[value="ko"])');
      if (!select) throw new Error('Language control missing');
      select.value = ${JSON.stringify(diagnosticLanguage)};
      select.dispatchEvent(new Event('change', {bubbles: true}));
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    })()`);
  }
  report.observedLanguage = await evaluate(`document.querySelector('select:has(option[value="ko"])')?.value`);
  if (diagnosticLanguage) assert.equal(report.observedLanguage, diagnosticLanguage, 'Requested diagnostic language was not applied');
  // Attribute RSS without guessing helper roles from PID order. CDP describes
  if (mainTraceDiagnostic) {
    await send('Tracing.start', {categories:'toplevel', transferMode:'ReportEvents'});
    mainTaskTraceStarted = true;
  }
  // Chromium processes only; unmatched descendants remain explicitly unknown.
  report.chromiumProcessesBefore = (await send('SystemInfo.getProcessInfo')).processInfo;
  report.rendererHeapBefore = await send('Runtime.getHeapUsage', {}, sessionId);
  report.rendererAssets = await evaluate(`performance.getEntriesByType('resource')
    .filter(entry => entry.name.startsWith('app://m5/'))
    .map(entry => ({path: new URL(entry.name).pathname, decodedBodySize: entry.decodedBodySize}))`);
  for (let index = 0; index < 40; index++) {
    assert.ok(!termination, 'App exited during RSS collection');
    report.memory.push({elapsedMs: performance.now() - started, ...readMemory()});
    await sleep(250);
  }
  report.idleRssBytes = Math.max(...report.memory.map(sample => sample.rssBytes));
  if (mainTraceDiagnostic) mainTaskWindowCompleted = true;
  report.idleRssWithinLimit = report.idleRssBytes <= 400 * 1024 * 1024;
  report.chromiumProcessesAfter = (await send('SystemInfo.getProcessInfo')).processInfo;
  report.rendererHeapAfter = await send('Runtime.getHeapUsage', {}, sessionId);
  if (memoryDiff) {
    report.memoryLifecycle = {diagnostic:true, gateEligible:false,
      intervalMs:250, samples:report.memory.map(sample => ({...sample, phase:'idle'})),
      limitations:['Observed descendants retained after reparenting; unseen children, same-second PID reuse and between-sample peak remain unverified.',
        'Large diff only; does not cover all large fixtures concurrently.']};
    const sample = () => {
      try {
        assert.ok(!termination, 'App exited during memory lifecycle observation');
        assert.ok(report.memoryLifecycle.samples.length < 10000, 'Memory sample budget exceeded');
        report.memoryLifecycle.samples.push({phase:memoryPhase, elapsedMs:performance.now() - started,
          ...readMemory()});
      } catch (error) {
        report.memoryLifecycle.error = String(error);
        clearInterval(memoryTimer);
      }
    };
    memoryTimer = setInterval(sample, 250);
    sample();
  }
  if (projectFlow) {
    // Focus only before the measured scenario; never retry a hidden sample
    // or disable the product's background throttling to manufacture a pass.
    await send('Page.bringToFront', {}, sessionId);
    report.projectVisibilityPrecondition = {state:await evaluate('document.visibilityState'), satisfied:false};
    try {
      await until(() => evaluate('document.visibilityState === "visible"'));
      report.projectVisibilityPrecondition = {state:'visible', satisfied:true};
    } catch (error) {
      report.projectVisibilityPrecondition.state = await evaluate('document.visibilityState');
      throw new Error(`Project visibility precondition failed (${report.projectVisibilityPrecondition.state}): ${error}`);
    }
    report.projectVisibilityStart = await evaluate(`(() => {
      const state = {initial:document.visibilityState, hiddenTransitions:0};
      const listener = () => { if (document.visibilityState !== 'visible') state.hiddenTransitions++; };
      document.addEventListener('visibilitychange', listener);
      globalThis.__m5ProjectVisibility = {state, listener};
      return state.initial;
    })()`);
    report.rendererLongTaskStart = await evaluate(`(${beginLongTaskProbe.toString()})()`);
    longTaskProbeStarted = true;
    await send('Tracing.start', {categories:'toplevel', transferMode:'ReportEvents'});
    mainTaskTraceStarted = true;
    const projectOpenedAt = performance.now();
    const expectedPath = fs.realpathSync(report.project.source);
    await until(() => evaluate(`(() => {
      const button = [...document.querySelectorAll('.m5-project-catalog-list li > button')]
        .find(item => item.title === ${JSON.stringify(expectedPath)} && !item.disabled);
      if (!button) return false;
      button.click(); return true;
    })()`));
    await until(() => evaluate(`!![...document.querySelectorAll('.m5-files button')]
      .find(button => button.textContent.includes('hello.txt') && !button.disabled)`), largeProject ? 65000 : 15000);
    report.project.snapshotObservedMs = performance.now() - projectOpenedAt;
    if (nativeChatStream) {
      await evaluate(`(${startPackagedChatStream.toString()})(${JSON.stringify(report.project.projectId)})`);
      report.nativeChatStream = await until(() => evaluate(`globalThis.__m5NativeChatStream?.status !== 'RUNNING' ? globalThis.__m5NativeChatStream : null`), 180000);
      if (report.nativeChatStream.status === 'FAIL') report.error = report.nativeChatStream.error;
    } else if (largeDiff) {
      const worktrees = path.join(userData, 'com.wonseokhan.helloagent/m5/worktrees-v1');
      const matches = fs.readdirSync(worktrees, {withFileTypes:true})
        .filter(entry => entry.isDirectory() && fs.existsSync(path.join(worktrees, entry.name, 'native-diff-fixture')))
        .map(entry => path.join(worktrees, entry.name));
      assert.equal(matches.length, 1, 'Expected one generated diff snapshot');
      const dimensions = diffFixture.change(matches[0]);
      report.largeDiff = {...await evaluate(`(${probeLargeDiff.toString()})()`), ...dimensions};
      assert.equal(execFileSync('/usr/bin/git', ['-C', report.project.source, 'status', '--porcelain'],
        {encoding:'utf8', env:{...env, GIT_CONFIG_NOSYSTEM:'1', GIT_CONFIG_GLOBAL:'/dev/null', GIT_CONFIG_COUNT:'0'}}).trim(), '', 'Large diff changed original project');
      report.largeDiff.originalUnchanged = true;
      if (report.largeDiff.failedMetrics.length) report.error = `Large diff limits exceeded: ${report.largeDiff.failedMetrics.join(', ')}`;
    } else if (largeProject) {
      report.largeProject = await evaluate(`(${probeLargeProject.toString()})(${JSON.stringify(report.project.projectId)})`);
      if (report.largeProject.failedMetrics.length) report.error = `Large project limits exceeded: ${report.largeProject.failedMetrics.join(', ')}`;
      report.largeProjectInput = await probePackagedInput(evaluate, text => send('Input.insertText', {text}, sessionId));
      if (!report.largeProjectInput.withinLimit) report.error ??= 'Large project input paint limit exceeded';
    } else {
    if (resume) {
      await until(() => evaluate(`!![...document.querySelectorAll('.m5-chat-chip')].find(button => button.textContent.includes('Packaged editing check') && !button.disabled)`));
      await evaluate(`[...document.querySelectorAll('.m5-chat-chip')].find(button => button.textContent.includes('Packaged editing check')).click()`);
    } else {
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
    }
    await until(() => evaluate(`document.querySelector('.m5-timeline h2')?.textContent === 'Packaged editing check'`));
    await evaluate(`[...document.querySelectorAll('.m5-files button')].find(button => button.textContent.includes('hello.txt')).click()`);
    report.project.editor = await until(() => evaluate(`(() => {
      const editor = document.querySelector('.monaco-editor');
      return editor && editor.innerText.includes('original') ? {opened: true, text: 'original'} : null;
    })()`));
    if (resume) {
      await until(() => evaluate(`document.querySelector('.monaco-editor .view-lines')?.textContent.replace(/\u00a0/g, ' ').includes('edited in packaged snapshot')`));
      report.project.persistedEditorText = true;
    } else {
    await evaluate(`document.querySelector('.monaco-editor textarea').focus()`);
    await send('Input.insertText', {text:'edited in packaged snapshot\n'}, sessionId);
    await until(() => evaluate(`!![...document.querySelectorAll('button')].find(button => button.textContent === '저장' && !button.disabled)`));
    await evaluate(`[...document.querySelectorAll('button')].find(button => button.textContent === '저장').click()`);
    await until(() => evaluate(`!![...document.querySelectorAll('button')].find(button => button.textContent === '저장' && button.disabled)`));
    }
    await evaluate(`[...document.querySelectorAll('.m5-chat-review button')].find(button => button.textContent === '작업 사본 diff 열기').click()`);
    await until(() => evaluate(`!![...document.querySelectorAll('.m5-change-row')].find(button => button.title === 'hello.txt' && !button.disabled)`));
    await evaluate(`[...document.querySelectorAll('.m5-change-row')].find(button => button.title === 'hello.txt').click()`);
    await until(() => evaluate(`document.querySelector('.m5-diff-details')?.textContent.includes('edited in packaged snapshot')`));
    report.project.savedDiffVisible = true;
    assert.equal(fs.readFileSync(path.join(report.project.source, 'hello.txt'), 'utf8'), 'original\n');
    report.project.originalUnchanged = true;
    if (resume) report.project.application = await probePackagedApplication(evaluate, until,
      () => fs.readFileSync(path.join(report.project.source, 'hello.txt'), 'utf8'));
    if (resume) {
      const closeProject = async () => {
        await until(() => evaluate(`!![...document.querySelectorAll('button')].find(button => button.textContent === '프로젝트 화면 닫기' && !button.disabled)`));
        await evaluate(`[...document.querySelectorAll('button')].find(button => button.textContent === '프로젝트 화면 닫기').click()`);
        await until(() => evaluate(`!document.querySelector('.m5-timeline')`));
      };
      const openProject = async source => {
        const title = JSON.stringify(fs.realpathSync(source));
        await until(() => evaluate(`!![...document.querySelectorAll('.m5-project-catalog-list li > button')].find(button => button.title === ${title} && !button.disabled)`));
        await evaluate(`[...document.querySelectorAll('.m5-project-catalog-list li > button')].find(button => button.title === ${title}).click()`);
      };
      await closeProject();
      await openProject(report.project.otherSource);
      await until(() => evaluate(`!![...document.querySelectorAll('.m5-files button')].find(button => button.textContent.includes('other.txt') && !button.disabled)`));
      await until(() => evaluate(`document.querySelector('.m5-new-chat-button')?.disabled === false`));
      assert.equal(await evaluate(`document.querySelectorAll('.m5-chat-chip').length`), 0, 'First project chats leaked into second project');
      assert.equal(await evaluate(`[...document.querySelectorAll('.m5-files button')].some(button => button.textContent.includes('hello.txt'))`), false, 'First project file leaked');
      await evaluate(`[...document.querySelectorAll('.m5-files button')].find(button => button.textContent.includes('other.txt')).click()`);
      await until(() => evaluate(`document.querySelector('.monaco-editor .view-lines')?.textContent.replace(/\u00a0/g, ' ').includes('second project only')`));
      await closeProject();
      await openProject(report.project.source);
      await until(() => evaluate(`!![...document.querySelectorAll('.m5-chat-chip')].find(button => button.textContent.includes('Packaged editing check') && !button.disabled)`));
      await evaluate(`[...document.querySelectorAll('.m5-chat-chip')].find(button => button.textContent.includes('Packaged editing check')).click()`);
      await until(() => evaluate(`document.querySelector('.m5-timeline h2')?.textContent === 'Packaged editing check'`));
      await until(() => evaluate(`!![...document.querySelectorAll('.m5-files button')].find(button => button.textContent.includes('hello.txt') && !button.disabled)`));
      await evaluate(`[...document.querySelectorAll('.m5-files button')].find(button => button.textContent.includes('hello.txt')).click()`);
      await until(() => evaluate(`document.querySelector('.monaco-editor .view-lines')?.textContent.replace(/\u00a0/g, ' ').includes('edited in packaged snapshot')`));
      assert.equal(fs.readFileSync(path.join(report.project.otherSource, 'other.txt'), 'utf8'), 'second project only\n');
      report.project.multiProject = {otherFilesIsolated:true, otherChatsEmpty:true, originalChatReopened:true, otherSourceUnchanged:true};
    }
    }
  }
  if (memoryDiff) {
    const loadedCapture = await send('Page.captureScreenshot', {format:'png'}, sessionId);
    fs.writeFileSync(path.join(root, 'loaded-diff.png'), Buffer.from(loadedCapture.data, 'base64'));
    await evaluate(`(() => {
      const close = [...document.querySelectorAll('button')].find(button =>
        ['프로젝트 화면 닫기', 'Close project view'].includes(button.textContent) && !button.disabled);
      if (!close) throw new Error('Project close control missing');
      close.click();
    })()`);
    await until(() => evaluate(`!document.querySelector('.m5-files') && !!document.querySelector('.m5-start-action')`));
    memoryPhase = 'closing';
    report.memoryLifecycle.closedAtMs = performance.now() - started;
    console.log(JSON.stringify({stage:'RETENTION_WAIT_STARTED', root}));
    await sleep(60000);
    report.memoryLifecycle.retainedStartedAtMs = performance.now() - started;
    memoryPhase = 'retained';
    await sleep(3000);
    clearInterval(memoryTimer);
    assert.ok(!report.memoryLifecycle.error, report.memoryLifecycle.error);
    Object.assign(report.memoryLifecycle, summarizeMemory(report.memoryLifecycle.samples));
    if (report.memoryLifecycle.failedMetrics.length) report.error ??= 'Memory lifecycle limits exceeded';
  }
  longTaskWindowCompleted = true;
  const capture = await send('Page.captureScreenshot', {format: 'png'}, sessionId);
  fs.writeFileSync(path.join(root, resume ? 'restart.png' : 'startup.png'), Buffer.from(capture.data, 'base64'));
} catch (error) { report.error = String(error); }
finally {
  clearInterval(memoryTimer);
  if (nativeChatStream && sessionId && !termination) {
    try { report.nativeChatStream = await evaluate('globalThis.__m5NativeChatStream ?? null'); }
    catch (error) { report.error ??= String(error); }
  }
  if (projectFlow && sessionId && !termination) {
    try {
      report.projectVisibility = await evaluate(`(() => {
        const active = globalThis.__m5ProjectVisibility;
        if (!active) return null;
        document.removeEventListener('visibilitychange', active.listener);
        delete globalThis.__m5ProjectVisibility;
        return {...active.state, final:document.visibilityState};
      })()`);
      if (!report.projectVisibility || report.projectVisibility.hiddenTransitions > 0 ||
          report.projectVisibility.final !== 'visible') report.error ??= 'Project observation window became hidden';
    } catch (error) { report.error ??= String(error); }
  }
  if (largeProject && sessionId && !termination) {
    try { report.largeProjectProgress = await evaluate('globalThis.__m5LargeProjectProgress ?? null'); }
    catch (error) { report.largeProjectProgressError = String(error); }
  }
  if (longTaskProbeStarted && sessionId && !termination) {
    try {
      report.rendererLongTasks = {...await evaluate(`(${endLongTaskProbe.toString()})()`),
        windowCompleted:longTaskWindowCompleted,
        scope:'Renderer project scenario only; excludes startup, main process and unobserved windows.'};
      if (report.rendererLongTasks.overLimitCount > 0) report.error ??= 'Renderer long-task limit exceeded';
    } catch (error) { report.rendererLongTaskError = String(error); }
  }
  if (mainTaskTraceStarted && !termination) {
    try {
      await send('Tracing.end');
      await until(() => mainTaskTrace.completed);
      report.mainLongTasks = mainTaskTrace.finish(mainTraceDiagnostic ? mainTaskWindowCompleted : longTaskWindowCompleted);
      report.mainLongTasks.scenario = mainTraceDiagnostic ? 'idle-trace-format-diagnostic' : 'project';
      if (!report.mainLongTasks.valid) report.error ??= 'Main task trace incomplete';
      else if (report.mainLongTasks.overLimitCount > 0) report.error ??= 'Main long-task limit exceeded';
    } catch (error) { report.mainLongTaskError = String(error); report.error ??= 'Main task trace incomplete'; }
  }
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
}
}
await observe(report);
let startup;
if (startupSeries) {
  startup = {schemaVersion:1, diagnostic:true, gateEligible:false, status:'RUNNING',
    root, head:report.head, cold:report, warmupRuns:1, measuredRuns:10, trials:[],
    limitations:['CDP polling includes observer attachment and query latency; not exact application readiness timestamps.',
      'First launch includes migration; subsequent launches reuse this generated empty project database.',
      'Warm labels distinguish repeated launches, not a guarantee about operating-system caches.',
      'This is a startup-only diagnostic, not the complete G6 gate.']};
  const saveStartup = () => fs.writeFileSync(path.join(root, 'startup-series.json'), JSON.stringify(startup, null, 2));
  saveStartup();
  try {
    let previous = report;
    for (let index = 0; index <= startup.measuredRuns; index++) {
      assert.equal(previous.shutdown, 'confirmed', 'Previous app shutdown not confirmed');
      assert.deepEqual(previous.termination, {code:0, signal:null});
      assert.ok(!previous.error, 'Previous startup observation incomplete');
      assert.equal(git('rev-parse', 'HEAD'), report.head);
      assert.equal(git('status', '--porcelain'), report.gitStatusBefore);
      assert.equal(createHash('sha256').update(fs.readFileSync(path.join(resources, 'app.asar'))).digest('hex'), report.asarSha256);
      assert.equal(createHash('sha256').update(fs.readFileSync(path.join(resources, 'sidecar/hello-agent-sidecar'))).digest('hex'), report.sidecarSha256);
      const live = new Set(execFileSync('/bin/ps', ['-axo', 'pid='], {encoding:'utf8'}).trim().split(/\s+/).map(Number));
      assert.ok([previous.pid, ...previous.memory.flatMap(row => row.processes.map(process => process.pid))].every(pid => !live.has(pid)), 'Previous observed process still live');
      const current = {...report, status:'RUNNING', measuredAt:new Date().toISOString(), memory:[], shutdown:'pending',
        previousSidecarInstanceId:previous.sidecarInstanceId,
        limitations:[...startup.limitations], error:undefined, termination:undefined};
      for (const key of ['pid', 'sidecarInstanceId', 'storageObservedMs', 'uiObservedMs', 'ui', 'rendererStartupMark', 'idleRssBytes', 'idleRssWithinLimit', 'quitReplyError', 'shutdownError']) delete current[key];
      startup.trials.push({index, warmup:index === 0, result:current});
      saveStartup();
      await observe(current, true, `startup-${index}.json`);
      saveStartup();
      previous = current;
      assert.equal(current.shutdown, 'confirmed');
      assert.deepEqual(current.termination, {code:0, signal:null});
      assert.ok(!current.error, 'Startup observation incomplete');
    }
    const measured = startup.trials.filter(trial => !trial.warmup);
    startup.metrics = {};
    for (const metric of ['storageObservedMs', 'uiObservedMs', 'idleRssBytes']) {
      const values = measured.map(trial => trial.result[metric]);
      assert.equal(values.length, 10);
      assert.ok(values.every(value => Number.isFinite(value) && value >= 0));
      startup.metrics[metric] = metric === 'idleRssBytes' ? {values, maximum:Math.max(...values)} : {values, p95:Math.max(...values)};
    }
    // Observer overhead can prove an upper bound, not the exact time a UI
    // became ready. Keep the raw names; never relabel these as G6 timings.
    startup.status = [report, ...startup.trials.map(trial => trial.result)].some(result => result.status === 'FAIL') ? 'FAIL' : 'NOT_READY';
    const live = new Set(execFileSync('/bin/ps', ['-axo', 'pid='], {encoding:'utf8'}).trim().split(/\s+/).map(Number));
    const observations = [report, ...startup.trials.map(trial => trial.result)];
    assert.ok(observations.flatMap(result => [result.pid, ...result.memory.flatMap(row => row.processes.map(process => process.pid))]).every(pid => !live.has(pid)), 'Observed process still live; profile retained');
    // This profile was created in this invocation, never supplied by a caller.
    // Keep all JSON/PNG evidence, including known memory failures.
    assert.ok(fs.lstatSync(userData).isDirectory() && !fs.lstatSync(userData).isSymbolicLink());
    assert.equal(fs.realpathSync(userData), path.join(fs.realpathSync(root), 'userdata'));
    fs.rmSync(userData, {recursive:true});
    startup.fixtureCleaned = true;
  } catch (error) { startup.status = 'FAIL'; startup.error = String(error); }
  finally { startup.headAfter = git('rev-parse', 'HEAD'); saveStartup(); }
  console.log(JSON.stringify({stage:'STARTUP_SERIES_FINISHED', root, status:startup.status, metrics:startup.metrics, error:startup.error}));
}
let restart;
if (projectFlow && report.shutdown === 'confirmed' && !report.error && report.project.savedDiffVisible) {
  restart = {
    schemaVersion: 1, diagnostic: true, gateEligible: false, status: 'RUNNING',
    root, head: git('rev-parse', 'HEAD'), measuredAt: new Date().toISOString(),
    packagedApp: true, userData, asarSha256: report.asarSha256, sidecarSha256: report.sidecarSha256,
    limitations: report.limitations, memory: [], shutdown: 'pending',
    project: {source: report.project.source, projectId: report.project.projectId,
      otherSource: report.project.otherSource, otherProjectId: report.project.otherProjectId},
    previousSidecarInstanceId: report.sidecarInstanceId,
  };
  await observe(restart, true);
}
process.exitCode = report.status === 'FAIL' || restart?.status === 'FAIL' || startup?.status === 'FAIL' ? 1 : 2;
