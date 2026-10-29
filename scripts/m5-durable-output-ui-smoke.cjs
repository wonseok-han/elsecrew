// Development integration diagnostic, not packaged G6 or a provider-account test.
// Requires current .vite/build, renderer and target/debug sidecar artifacts.
function runOutputSmoke(packagedQa = false) {
const { app, dialog, powerMonitor } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const assert = require('node:assert/strict');
const { installOutputObserver } = require('./m5-durable-output-observer.cjs');
const { createLiveOutputLatency } = require('./m5-live-output-latency.cjs');
const { observeStdio } = require('./m5-stdio-observer.cjs');
const { summarizeOutputGeneration } = require('./m5-output-generation.cjs');
if (process.platform !== 'darwin' || app.isPackaged !== packagedQa) throw new Error('Output diagnostic build mode differs');
if (packagedQa) app.setName('Hello Agent Output QA');
const live = packagedQa || process.argv.includes('--live');
const paced = live || process.argv.includes('--paced');
const repo = path.resolve(__dirname, '..');
const root = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'hello-agent-output-ui-'));
const source = path.join(root, 'project');
const userData = path.join(root, 'userdata');
fs.mkdirSync(source); fs.mkdirSync(userData);
fs.writeFileSync(path.join(source, 'hello.txt'), 'original\n');
for (const args of [['init', '-q', '--template='], ['add', '.'],
  ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture']]) {
  cp.execFileSync('/usr/bin/git', args, { cwd: source, env: { ...process.env,
    GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_COUNT: '0' } });
}
app.setPath('userData', userData);
// Electron may have resolved these paths before userData was overridden.
// Pin caches and logs explicitly before loading the product main bundle.
const sessionData = path.join(root, 'session-data');
const logs = path.join(root, 'logs');
fs.mkdirSync(sessionData); fs.mkdirSync(logs);
app.setPath('sessionData', sessionData);
app.setAppLogsPath(logs);
assert.equal(fs.realpathSync(app.getPath('userData')), fs.realpathSync(userData));
assert.equal(fs.realpathSync(app.getPath('sessionData')), fs.realpathSync(sessionData));
assert.equal(fs.realpathSync(app.getPath('logs')), fs.realpathSync(logs));
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [source] });
const report = { schemaVersion: 1, diagnostic: true, gateEligible: false, root, packagedQa,
  paced, live, liveWindows: [], status: 'RUNNING', shutdown: 'pending', restoredChats: [], scrollChecks: [],
  limitations: [packagedQa ? 'Explicit QA package, synthetic providers; not the production artifact or actual accounts.' : 'Development artifacts, synthetic providers, no actual accounts.',
    'Full retained-window and history checks; no 50-events/s paint or packaged RSS gate.',
    'Synthetic runs have no native approval queue; approval inspection rejection is not native-provider evidence.',
    'RSS includes transient sampling overhead and counts shared pages per process; not a packaged idle-memory gate.',
    'Two RAFs are rendering opportunities, not physical compositor timestamps or provider generation latency.',
    'Stdio round trips include queueing and observer parsing overhead; they are not isolated database execution times.'] };
const persist = () => fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(report, null, 2));
persist();
const spawn = cp.spawn;
const expectedBinary = packagedQa ? path.join(process.resourcesPath, 'sidecar/hello-agent-sidecar') : path.join(repo, 'target/debug/hello-agent-sidecar');
let child;
let transportObserver;
cp.spawn = function(command, args, options) {
  if (typeof command === 'string' && path.basename(command) === 'hello-agent-sidecar') {
    assert.equal(command, expectedBinary);
    assert.deepEqual(args, ['--stdio']);
    assert.equal(fs.realpathSync(options.cwd), fs.realpathSync(path.join(userData, 'com.wonseokhan.helloagent', 'm5')));
    assert.equal(child, undefined, 'unexpected second sidecar');
    child = Reflect.apply(spawn, this, [command, ['--stdio-agent-fixture', paced ? 'paced-long-stream' : 'long-stream'], options]);
    if (packagedQa) {
      // Copy only the fixed QA numeric summary, never arbitrary sidecar stderr.
      let diagnosticTail = '';
      child.stderr.on('data', chunk => {
        diagnosticTail += chunk.toString('utf8');
        let end;
        while ((end = diagnosticTail.indexOf('\n')) !== -1) {
          const line = diagnosticTail.slice(0, end); diagnosticTail = diagnosticTail.slice(end + 1);
          if (/^QA_DISPATCH_TIMING \{/.test(line) && line.length <= 4096) {
            try {
              const value = JSON.parse(line.slice('QA_DISPATCH_TIMING '.length));
              const phases = Object.fromEntries(['watch', 'runtime', 'request', 'maintenance'].map(name => {
                const row = value.phases?.[name];
                assert.ok(Array.isArray(row) && row.length === 3 && row.every(n => Number.isSafeInteger(n) && n >= 0));
                return [name, row];
              }));
              fs.appendFileSync(path.join(root, 'dispatch-timing.log'), JSON.stringify({diagnostic:true, gateEligible:false,
                units:'microseconds', columns:['count','total','maximum'], phases}) + '\n');
            } catch { /* Missing/invalid diagnostics are never a timing PASS. */ }
          }
        }
        if (diagnosticTail.length > 4096) diagnosticTail = '';
      });
    }
    transportObserver = observeStdio(child);
    child.once('close', (code, signal) => { report.sidecarExit = { code, signal }; persist(); });
    return child;
  }
  return Reflect.apply(spawn, this, [command, args, options]);
};
// Poll the process tree, including sidecar and fixture descendants. These are
// sampled RSS totals (shared pages may be counted more than once), not heap size.
report.memory = { scope: 'test-app-process-tree', samples: [], errors: 0, truncated: false };
const memoryTimer = setInterval(() => {
  try {
    const rows = cp.execFileSync('/bin/ps', ['-axo', 'pid=,ppid=,rss='], { encoding: 'utf8', timeout: 2000, maxBuffer: 4 * 1024 * 1024 })
      .trim().split('\n').map(line => line.trim().split(/\s+/).map(Number));
    const included = new Set([process.pid]);
    let grew = true;
    while (grew) { grew = false; for (const [pid, ppid] of rows) if (included.has(ppid) && !included.has(pid)) { included.add(pid); grew = true; } }
    const processes = rows.filter(([pid]) => included.has(pid)).map(([pid, , kib]) => ({pid, rssBytes:kib * 1024}));
    if (report.memory.samples.length < 600) report.memory.samples.push({at:Date.now(), processes, totalBytes:processes.reduce((sum, entry) => sum + entry.rssBytes, 0)});
    else report.memory.truncated = true;
  } catch { report.memory.errors++; }
}, 1000);
// Observe native state without forcing focus, disabling throttling, or changing
// the visibility-based acceptance rule. A visible native window may be occluded.
report.windowState = { samples: [], dropped: 0 };
let observedWindow;
const nativeListeners = [];
function sampleWindow(reason) {
  if (report.windowState.samples.length >= 600) { report.windowState.dropped++; return; }
  const window = observedWindow;
  const state = { at: Date.now(), reason, destroyed: !window || window.isDestroyed() };
  if (!state.destroyed) Object.assign(state, {
    visible: window.isVisible(), minimized: window.isMinimized(),
    focused: window.isFocused(), bounds: window.getBounds(),
  });
  report.windowState.samples.push(state);
}
function observeNative(emitter, event) {
  const listener = () => sampleWindow(event);
  emitter.on(event, listener);
  nativeListeners.push(() => emitter.removeListener(event, listener));
}
const windowTimer = setInterval(() => sampleWindow('sample'), 1000);
let finished = false;
function finish(error) {
  if (finished) return;
  finished = true;
  sampleWindow('finish');
  clearInterval(windowTimer);
  for (const remove of nativeListeners) remove();
  clearInterval(memoryTimer);
  report.transport = transportObserver?.stop();
  report.error = error?.message;
  report.status = error ? 'FAIL' : 'PENDING';
  persist(); console.log(JSON.stringify(report));
  const timer = setTimeout(() => { report.shutdown = 'notConfirmed'; report.status = 'FAIL'; persist(); console.log(JSON.stringify(report)); }, 15000);
  app.once('will-quit', () => {
    clearTimeout(timer);
    report.shutdown = 'accepted'; report.functionalStatus = error ? 'FAIL' : 'PASS';
    report.status = error ? 'FAIL' : report.visibility === 'visible' && report.visibilityTrace?.every(entry => entry.state === 'visible') ? 'PASS' : 'NOT_READY';
    persist(); console.log(JSON.stringify(report));
    app.exit(error ? 1 : 0);
  });
  app.quit(); // Preserve the normal termination gate; never force-kill a test.
}
app.on('browser-window-created', (_event, window) => {
  observedWindow = window;
  for (const event of ['show', 'hide', 'minimize', 'restore', 'focus', 'blur', 'closed']) observeNative(window, event);
  for (const event of ['lock-screen', 'unlock-screen', 'suspend', 'resume']) observeNative(powerMonitor, event);
  sampleWindow('created');
  window.webContents.once('did-finish-load', async () => {
    const run = code => window.webContents.executeJavaScript(code);
    const wait = async (predicate, timeout = 30000) => {
      report.waitingFor = predicate; persist();
      const until = Date.now() + timeout;
      while (Date.now() < until) {
        if (await run(predicate)) return;
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      throw new Error('UI condition timeout: ' + predicate);
    };
    try {
      window.show(); window.focus();
      await wait("document.visibilityState === 'visible' && window.m5?.version === 8");
      await run(`globalThis.outputSmokeVisibility = [{state:document.visibilityState,at:performance.now()}];
        document.addEventListener('visibilitychange', () => outputSmokeVisibility.push({state:document.visibilityState,at:performance.now()}));`);
      await wait("[...document.querySelectorAll('button')].some(b => b.textContent === '프로젝트 선택' && !b.disabled)");
      await run(`(async () => {
        const api = window.m5;
        const project = await api.openProject();
        const settings = await api.getProjectRoleSettings({projectId:project.projectId});
        const roles = settings.roles.map(role => role.roleId === 'coding'
          ? {...role,runtime:'claude-code',modelSelector:'claude-opus-5',reasoningEffort:null} : role);
        const saved = await api.updateProjectRoleSettings({projectId:project.projectId,expectedRevision:settings.revision,roles});
        globalThis.outputSmokeChats = [];
        for(let index=0;index<4;index++) {
          const prepared = await api.prepareChatWorkspace({projectId:project.projectId});
          outputSmokeChats.push(await api.createChat({preparationId:prepared.preparationId,projectId:project.projectId,
            worktreeId:prepared.manifest.worktreeId,workspaceIdentity:prepared.manifest.workspaceIdentity,
            goal:'Output stream '+index,expectedSettingsRevision:saved.revision}));
        }
      })()`);
      await run("[...document.querySelectorAll('button')].find(b => b.textContent === '프로젝트 선택' && !b.disabled).click()");
      await wait("document.querySelectorAll('.m5-chat-chip').length === 4");
      await run("document.querySelector('.m5-chat-chip').click()");
      await wait("!!document.querySelector('#m5-followup-text')");
      await run(`globalThis.outputObservation = (${installOutputObserver.toString()})((${createLiveOutputLatency.toString()})); void 0`);
      await run(`(async () => {
        const api = window.m5;
        const capability = await api.getAgentCapabilities({runtime:'claude-code',refresh:true});
        globalThis.outputSmokeRuns = [];
        for(const chat of outputSmokeChats) {
          const detail = await api.getChat({chatId:chat.chatId,afterEventSequence:0,cursor:null,limit:50});
          outputSmokeRuns.push(await api.startAgentRun({chatId:chat.chatId,roleId:'coding',
            inputMessageIds:[detail.messages[0].messageId],expectedCapabilityFingerprint:capability.fingerprint}));
        }
      })()`);
      if (live) for (let index = 0; index < 4; index++) {
        await run(`[...document.querySelectorAll('.m5-chat-chip')].find(b => b.querySelector('.m5-chat-goal')?.textContent === 'Output stream ${index}').click()`);
        await wait(`document.querySelector('.m5-durable-output select')?.value === outputSmokeRuns[${index}].runId &&
          [...document.querySelectorAll('.m5-chat-chip')].some(b => b.getAttribute('aria-current') === 'true' && !b.disabled && b.querySelector('.m5-chat-goal')?.textContent === 'Output stream ${index}')`);
        await wait("!!document.querySelector('.m5-output-scroll article:last-child pre')");
        // Model the user bringing the live output into view once. Do not keep
        // scrolling it in the probe: subsequent updates must follow naturally.
        await run("document.querySelector('.m5-output-scroll').scrollIntoView({block:'end',inline:'nearest'})");
        const observation = await run(`(async () => {
          const runId = outputSmokeRuns[${index}].runId;
          const before = outputObservation.mark();
          const startedAt = performance.now();
          await new Promise(resolve => setTimeout(resolve, 12000));
          const endedAt = performance.now();
          const readStartedAt = performance.now();
          const after = outputObservation.readWindow(before, runId);
          const observationReadMs = performance.now() - readStartedAt;
          return {runId,startedAt,endedAt,observationReadMs,heads:after.heads,frames:after.frames,
            latestVisible:outputObservation.latestVisible(),visibility:document.visibilityState,
            droppedHeads:after.droppedHeads,droppedFrames:after.droppedFrames};
        })()`);
        report.liveWindows.push(observation);
        const persistStartedAt = performance.now();
        persist();
        observation.mainPersistMs = performance.now() - persistStartedAt;
        assert.equal(observation.visibility, 'visible', 'Live window became hidden');
        assert.equal(observation.droppedHeads, 0, 'Live head observations truncated');
        assert.equal(observation.droppedFrames, 0, 'Live frame observations truncated');
        assert.ok(observation.heads.length >= 2 && observation.heads.at(-1).sequence > observation.heads[0].sequence,
          'Selected stream did not advance during live window');
        assert.ok(observation.heads[0].sequence < 3000, 'Live window started after completion');
        assert.ok(observation.frames.length >= 2 && observation.frames.at(-1).sequence > observation.frames[0].sequence,
          'Visible stream did not advance during live window');
        assert.equal(observation.latestVisible, true, 'Latest live output is clipped');
      }
      for (let index = 0; index < 4; index++) {
        await run(`[...document.querySelectorAll('.m5-chat-chip')].find(b => b.querySelector('.m5-chat-goal')?.textContent === 'Output stream ${index}').click()`);
        // Every chat has identical output text. Wait for the selected run and the
        // async chat load, not just rows that may still belong to the prior chat.
        await wait(`document.querySelector('.m5-durable-output select')?.value === outputSmokeRuns[${index}].runId &&
          [...document.querySelectorAll('.m5-chat-chip')].some(b => b.getAttribute('aria-current') === 'true' && !b.disabled && b.querySelector('.m5-chat-goal')?.textContent === 'Output stream ${index}')`);
        await wait("[...document.querySelectorAll('.m5-output-scroll article pre')].length === 200 && document.querySelector('.m5-output-scroll article:last-child pre')?.textContent?.split(' [')[0] === 'output 3000'", 240000);
        const visible = await run("[...document.querySelectorAll('.m5-output-scroll article pre')].map(e => e.textContent.split(' [')[0])");
        assert.deepEqual(visible, Array.from({length:200}, (_, i) => `output ${2801+i}`));
        await run("[...document.querySelectorAll('.m5-durable-output button')].find(b => b.textContent === '처음 기록부터 보기').click()");
        await wait("document.querySelector('.m5-output-scroll article:first-of-type pre')?.textContent?.split(' [')[0] === 'output 1'");
        const history = await run("[...document.querySelectorAll('.m5-output-scroll article pre')].map(e => e.textContent.split(' [')[0])");
        assert.deepEqual(history, Array.from({length:100}, (_, i) => `output ${i+1}`));
        await wait("document.querySelector('.m5-output-scroll')?.scrollTop === 0");
        await run("[...document.querySelectorAll('.m5-durable-output button')].find(b => b.textContent === '다음 기록').click()");
        await wait("document.querySelector('.m5-output-scroll article:first-of-type pre')?.textContent?.split(' [')[0] === 'output 101'");
        const nextHistory = await run("[...document.querySelectorAll('.m5-output-scroll article pre')].map(e => e.textContent.split(' [')[0])");
        assert.deepEqual(nextHistory, Array.from({length:100}, (_, i) => `output ${101+i}`));
        await run("[...document.querySelectorAll('.m5-durable-output button')].find(b => b.textContent === '최신 대화').click()");
        await wait("document.querySelector('.m5-output-scroll article:last-child pre')?.textContent?.split(' [')[0] === 'output 3000'");
        await wait("(() => { const list = document.querySelector('.m5-output-scroll'); return list && list.clientHeight > 0 && list.scrollHeight - list.scrollTop - list.clientHeight <= 2; })()");
        // A scrolled inner list can still be clipped by the outer conversation.
        if (await run("document.visibilityState === 'visible'")) await wait("outputObservation.latestVisible()");
        report.scrollChecks.push({chat:index,historyTop:true,nextPage:true,latestBottom:true});
        report.restoredChats.push(index); persist();
      }
      assert.equal(fs.readFileSync(path.join(source, 'hello.txt'), 'utf8'), 'original\n');
      if (paced) report.generation = await run(`(async () => {
        const generated = [];
        for (const accepted of outputSmokeRuns) {
          const {schemaVersion, state, ...binding} = accepted;
          const samples = [];
          for (let afterSequence = 0; afterSequence < 3000; afterSequence += 100) {
            const page = await window.m5.getAgentOutputPage({formatVersion:1,binding,afterSequence,throughSequence:3000,limit:100});
            for (const row of page.rows) {
              const match = /^output ([0-9]+) \\[emitted_us=([0-9]+),unix_ms=([0-9]+)\\]$/.exec(row.event.text);
              if (!match || Number(match[1]) !== samples.length + 1) throw new Error('Paced fixture emission evidence differs');
              for (const key of ['runId','chatId','taskId','projectId','worktreeId','workspaceIdentity']) {
                if (row.event[key] !== binding[key]) throw new Error('Output row crossed run binding');
              }
              samples.push([Number(match[1]),Number(match[2]),Number(match[3]),row.event.eventId,row.outputSequence]);
            }
          }
          if (samples.length !== 3000) throw new Error('Missing emission samples');
          generated.push({runId:binding.runId,samples});
        }
        return generated;
      })()`);
      if (paced) report.generationSummary = summarizeOutputGeneration(report.generation);
      report.outputObservation = await run('outputObservation.stop()');
      report.visibility = await run('document.visibilityState');
      report.visibilityTrace = await run('outputSmokeVisibility');
      await fs.promises.writeFile(path.join(root, 'output.png'), (await window.webContents.capturePage()).toPNG());
      finish();
    } catch (error) {
      sampleWindow('assertion-failed');
      try {
        report.outputObservation = await run('globalThis.outputObservation?.stop() ?? null');
        report.failureView = await run(`(() => {
          const list = document.querySelector('.m5-output-scroll');
          return {visibility:document.visibilityState,trace:globalThis.outputSmokeVisibility ?? [],
            scroll:list ? {top:list.scrollTop,height:list.scrollHeight,client:list.clientHeight,
              first:list.querySelector('article pre')?.textContent,last:list.querySelector('article:last-child pre')?.textContent} : null};
        })()`);
      } catch (diagnosticError) { report.diagnosticError = diagnosticError.message; }
      finish(error);
    }
  });
});
console.log(JSON.stringify({stage:'STARTED',root}));
require(path.join(repo, '.vite/build/main.cjs'));

}
module.exports = { runOutputSmoke };
if (require.main === module) runOutputSmoke();
