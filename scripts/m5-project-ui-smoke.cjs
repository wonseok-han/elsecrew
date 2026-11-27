// macOS native UI smoke: synthetic project and isolated app data, no providers selected.
// Requires current .vite/build output and native sidecar; this script does not build them.
const { app, BrowserWindow, dialog, ipcMain } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { execFile, execFileSync } = require('node:child_process');
const { processTreeMemory, summarizeMemory } = require('./m5-process-memory.cjs');
const discardFlow = process.env.ELSECREW_SMOKE_DISCARD === '1';
const measureMemory = process.env.ELSECREW_SMOKE_MEMORY === '1';
const idleOnly = process.env.ELSECREW_SMOKE_IDLE_ONLY === '1';
const nativeInput = process.env.ELSECREW_SMOKE_NATIVE_INPUT === '1';
// Home, first run, switching, adding, deleting and the first task's yes against the real sidecar (ADE U2·U3, PR #12 review).
const projectsFlow = process.env.ELSECREW_SMOKE_PROJECTS === '1';
if (idleOnly && !measureMemory) throw new Error('Idle-only diagnostic requires memory mode');
const largeFile = process.env.ELSECREW_SMOKE_LARGE_FILE === '1';
const largeTree = process.env.ELSECREW_SMOKE_LARGE_TREE === '1';
const largeDiff = process.env.ELSECREW_SMOKE_LARGE_DIFF === '1';
if (nativeInput && (measureMemory || largeFile || largeTree || largeDiff)) throw new Error('Native input is a separate manual diagnostic scenario');
if (idleOnly && (largeFile || largeTree || largeDiff || process.env.ELSECREW_SMOKE_APPLY === '1')) throw new Error('Idle-only diagnostic cannot include project scenarios');
if (largeDiff && (measureMemory || largeFile || largeTree || process.env.ELSECREW_SMOKE_APPLY === '1')) throw new Error('Large diff is a separate diagnostic scenario');
if (discardFlow && (measureMemory || idleOnly || nativeInput || largeFile || largeTree || largeDiff || process.env.ELSECREW_SMOKE_APPLY === '1')) throw new Error('Discard is a separate synthetic integration scenario');
// The discard scenario runs a fixture agent. Every other flow keeps the real
// engine with no provider: the fixture engine keeps the first role set, so
// the new-task window cannot save its team there (m5-multiproject-ui-smoke.cjs).
const fixtureScenario = discardFlow ? 'clean' : null;
if (fixtureScenario) {
  const cp = require('node:child_process');
  const originalSpawn = cp.spawn;
  const sidecar = path.resolve(__dirname, '../target/debug/elsecrew-sidecar');
  cp.spawn = function(command, args, options) {
    if (typeof command === 'string' && path.basename(command) === 'elsecrew-sidecar') {
      // After a clean close the app adds that close's seal (`--storage-seal <value>`).
      if (path.resolve(command) !== sidecar || args[0] !== '--stdio' || (args.length !== 1 && !(args.length === 3 && args[1] === '--storage-seal'))) throw new Error('unexpected sidecar launch');
      return Reflect.apply(originalSpawn, this, [command, ['--stdio-agent-fixture', fixtureScenario, ...args.slice(1)], options]);
    }
    return Reflect.apply(originalSpawn, this, arguments);
  };
}
let largeDiffReport;
const diffRequests = [];
const treeRequests = [];
if (largeDiff || largeTree) {
  // Handler time includes sidecar queueing, not only backend processing.
  // Preserve the actual handler, arguments, result and rejection.
  const handle = ipcMain.handle.bind(ipcMain);
  ipcMain.handle = (channel, listener) => handle(channel, async (...args) => {
    if (!largeTree && !/diff/i.test(channel)) return listener(...args);
    const sample = { channel, startedMs: performance.now(), status: 'pending' };
    (largeTree ? treeRequests : diffRequests).push(sample);
    try {
      const result = await listener(...args);
      sample.status = 'resolved';
      return result;
    } catch (error) {
      sample.status = 'rejected';
      throw error;
    } finally {
      sample.durationMs = performance.now() - sample.startedMs;
    }
  });
}
let largeFileReport;
let largeTreeReport;
let confirmationReport;
let nativeFocusUnavailable = false;
const memorySamples = [];
let memoryPhase = 'idle';
let memoryTimer;
let memoryPending;
let memoryError;
let memoryReport;
function sampleMemory() {
  if (memoryPending) return memoryPending;
  const phase = memoryPhase;
  memoryPending = new Promise((resolveSample, reject) => {
    const child = execFile('/bin/ps', ['-axo', 'pid=,ppid=,rss='], { timeout: 5000, maxBuffer: 4 * 1024 * 1024 }, (error, stdout) => {
      if (error) return reject(error);
      try {
        const rows = stdout.split('\n').filter(line => Number(line.trim().split(/\s+/)[0]) !== child.pid).join('\n');
        const tree = processTreeMemory(rows, process.pid);
        const metrics = new Map(app.getAppMetrics().map(metric => [metric.pid, metric]));
        memorySamples.push({ elapsedMs: performance.now(), phase, ...tree, processes: tree.processes.map(entry => ({ ...entry, type: metrics.get(entry.pid)?.type ?? 'nonElectronChild' })) });
        resolveSample();
      } catch (failure) { reject(failure); }
    });
  }).finally(() => { memoryPending = null; });
  return memoryPending;
}
if (process.platform !== 'darwin') throw new Error('This native UI smoke currently supports macOS only');
const root = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'elsecrew-project-ui-'));
const repoRoot = path.resolve(__dirname, '..');
const source = path.join(root, 'project');
fs.mkdirSync(source);
fs.writeFileSync(path.join(source, 'hello.txt'), 'original\n');
if (largeDiff) require('../tests/m5-native-diff-fixture.cjs').prepare(source);
if (largeTree) {
  // 1,000 directories and 10,000 files in the measured subtree. Existing
  // smoke control files remain outside it and are reported separately.
  const treeRoot = path.join(source, 'aaa-tree');
  fs.mkdirSync(treeRoot);
  for (let directory = 0; directory < 1000; directory++) {
    const folder = directory === 0 ? treeRoot : path.join(treeRoot, `folder-${String(directory).padStart(4, '0')}`);
    if (directory) fs.mkdirSync(folder);
    for (let file = 0; file < 10; file++) fs.writeFileSync(path.join(folder, `file-${file}.txt`), 'tree fixture\n');
  }
}
if (largeFile) {
  // Generate outside the measured app so the fixture's cached 5 MiB string
  // does not become permanent main-process memory in this native measurement.
  execFileSync('node', ['-e', "require('node:fs').writeFileSync(process.argv[1], require(process.argv[2]).getFileText())",
    path.join(source, 'large.txt'), path.join(repoRoot, 'tests/m5-renderer-performance-fixture.cjs')]);
}
for (const args of [['init','-q','--template='],['add','.'],['-c','core.hooksPath=/dev/null','-c','user.name=Smoke','-c','user.email=smoke@example.invalid','commit','-qm','fixture']]) execFileSync('/usr/bin/git', args, {cwd:source, env:{...process.env,GIT_CONFIG_NOSYSTEM:'1',GIT_CONFIG_GLOBAL:'/dev/null',GIT_CONFIG_COUNT:'0'}});
app.setPath('userData', path.join(root,'userdata'));
// Task workspaces (real git worktrees) go here, never under the real ~/elsecrew.
const workspacesRoot = path.join(root, 'workspaces');
fs.mkdirSync(workspacesRoot);
// Folders the next pickers return, in order (null = the person cancels); then the default project.
const pickQueue = [];
dialog.showOpenDialog = async () => {
  const next = pickQueue.length ? pickQueue.shift() : source;
  return next === null ? {canceled:true,filePaths:[]} : {canceled:false,filePaths:[next]};
};
// main's own yes/no for a folder that becomes a project (PR #12 review B1): recorded, answered by the scenario.
const messageBoxes = [];
let messageBoxAnswer = 1;
dialog.showMessageBox = async (_window, options) => { messageBoxes.push({message:options.message,detail:options.detail,buttons:options.buttons,cancelId:options.cancelId}); return {response:messageBoxAnswer,checkboxChecked:false}; };
let secondSource, emptySource;
if (projectsFlow) {
  secondSource = path.join(root, 'shop');
  fs.mkdirSync(secondSource);
  fs.writeFileSync(path.join(secondSource, 'index.html'), '<h1>shop</h1>\n');
  for (const args of [['init','-q','--template='],['add','.'],['-c','core.hooksPath=/dev/null','-c','user.name=Smoke','-c','user.email=smoke@example.invalid','commit','-qm','fixture']]) execFileSync('/usr/bin/git', args, {cwd:secondSource});
  emptySource = path.join(root, 'empty');
  fs.mkdirSync(emptySource);
}
let finished = false;
const timer = setTimeout(() => finish(new Error('flow timeout')), nativeInput ? 180000 : largeTree ? 300000 : measureMemory ? 160000 : 90000);
function finish(error, details) {
  if (finished) return;
  finished = true; clearTimeout(timer); clearInterval(memoryTimer);
  if (measureMemory) fs.writeFileSync(path.join(root, 'memory.json'), JSON.stringify({
    schemaVersion: 1, diagnostic: true, gateEligible: false,
    scope: 'Native UI Electron root and current descendants, including its sidecar; escaped descendants and agents not measured',
    fixture: idleOnly ? 'Native startup only; no project selected or agent connected' : largeFile ? 'Generated project with 5 MiB/100000-line file and two chats; no streaming fixture' : 'Small generated project, two chats; not the G6 large-file or streaming fixture',
    ...(largeTree ? {treeFixture:{files:10000,directories:1000,controlFiles:largeFile ? 2 : 1}} : {}),
    intervalMs: 250, closeWaitMs: idleOnly ? null : 60000, samples: memorySamples,
    ...memoryReport, error: memoryError?.message ?? error?.message,
  }, null, 2));
  // Preserve normal before-quit/sidecar cleanup; set Electron's exit status only
  // after the app has accepted quitting. process.exitCode alone is ignored here.
  const result = {root,error:error?.message,...details,confirmation:confirmationReport,...(largeDiff ? {largeDiff:largeDiffReport} : {}),...(largeFile ? {largeFile:largeFileReport} : {}),...(largeTree ? {largeTree:largeTreeReport} : {}),...(measureMemory ? {memoryEvidence:path.join(root, 'memory.json'), memory:memoryReport} : {}),shutdown:'pending',status:'PENDING'};
  if (largeDiff) result.diffRequests = diffRequests;
  if (largeTree) result.treeRequests = treeRequests;
  const persist = () => fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(result, null, 2));
  persist();
  console.log(JSON.stringify(result));
  const shutdownTimer = setTimeout(() => {
    result.shutdown = 'notConfirmed'; result.status = 'FAIL';
    persist(); console.log(JSON.stringify(result));
  }, 15000);
  app.once('will-quit', () => {
    clearTimeout(shutdownTimer);
    result.shutdown = 'accepted'; result.status = error ? 'FAIL' : 'PASS';
    persist(); console.log(JSON.stringify(result));
    app.exit(error ? 1 : 0);
  });
  app.quit();
}
app.on('browser-window-created', (_event, window) => {
  window.webContents.setBackgroundThrottling(false);
  window.webContents.on('render-process-gone', (_event, details) => console.log(JSON.stringify({stage:'RENDER_PROCESS_GONE',details})));
  window.webContents.on('console-message', details => { if (details.level === 'error') console.log(JSON.stringify({stage:'RENDERER_ERROR',message:details.message})); });
  window.webContents.once('did-finish-load', async () => {
    const run = async code => {
      if (finished) throw new Error('UI smoke session has finished');
      const result = await window.webContents.executeJavaScript(code);
      if (finished) throw new Error('UI smoke session has finished');
      return result;
    };
    const clickText = (label, scope = 'body') => run(`Array.from(document.querySelectorAll(${JSON.stringify(scope + ' button')})).find(b=>b.textContent===${JSON.stringify(label)}&&!b.disabled)?.click()`);
    const fill = (selector, value) => run(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e||e.disabled)throw new Error('input unavailable');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,${JSON.stringify(value)});e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
    const showPanel = title => run(`(()=>{ const id={'변경':'right-tab-changes','파일':'right-tab-files'}[${JSON.stringify(title)}]; const tab=id&&document.querySelector('[data-testid='+id+']'); if(tab)tab.click(); })()`);
    const wait = async (predicate, timeoutMs = 15000) => {
      const deadline = performance.now() + timeoutMs;
      while(performance.now() < deadline) { if(await run(predicate)) return; await new Promise(r=>setTimeout(r,100)); }
      throw new Error('UI condition timeout: '+predicate+'\n'+await run('document.body.innerText'));
    };
    try {
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='프로젝트 선택'&&!b.disabled)`);
      if (measureMemory) {
        await sampleMemory();
        memoryTimer = setInterval(() => { void sampleMemory().catch(error => { memoryError = error; }); }, 250);
        await new Promise(resolve => setTimeout(resolve, 1000));
        if (memoryPending) await memoryPending;
        if (idleOnly) {
          clearInterval(memoryTimer);
          if (memoryError) throw memoryError;
          const idleRssBytes = Math.max(...memorySamples.map(sample => sample.rssBytes));
          memoryReport = { status: idleRssBytes > 512 * 1024 ** 2 ? 'FAIL' : 'NOT_READY', maxima: { idleRssBytes },
            scope: 'One native development startup; no loaded/retained measurement or packaged gate' };
          // Collect after the RSS window; heap figures are diagnostic subsets,
          // not replacements for the process-tree RSS acceptance metric.
          memoryReport.heapDiagnostic = {
            main: process.memoryUsage(),
            mainV8: require('node:v8').getHeapStatistics(),
            renderer: await run(`(() => {
              const memory = performance.memory;
              return { source: 'Chromium performance.memory (approximate)',
                usedJSHeapSize: memory?.usedJSHeapSize ?? null,
                totalJSHeapSize: memory?.totalJSHeapSize ?? null,
                resources: performance.getEntriesByType('resource').map(entry => ({
                  name: new URL(entry.name).pathname, transferSize: entry.transferSize,
                  decodedBodySize: entry.decodedBodySize
                })).slice(0, 100) };
            })()`),
          };
          const screenshot = path.join(root, 'idle.png');
          fs.writeFileSync(screenshot, (await window.webContents.capturePage()).toPNG());
          finish(idleRssBytes > 512 * 1024 ** 2 ? new Error('native idle memory exceeds limit') : null, { screenshot, idleOnly: true });
          return;
        }
        memoryPhase = 'loaded';
      }
      if (!await run(`Boolean(document.querySelector('.m5-guided-start'))&&!document.querySelector('[data-testid=task-layout]')`)) throw new Error('initial guided start missing or empty dock exposed');
      if (discardFlow) {
        await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='앱 기본 역할·모델').click()`);
        await wait(`document.querySelector('.m5-settings-dialog')?.matches(':modal')`);
        await run(`(()=>{const e=document.querySelector('.m5-role-card input');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'fixture-default');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
        await run(`(()=>{const e=document.querySelectorAll('.m5-role-card')[1].querySelector('input');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'claude-opus-5');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
        await run(`Array.from(document.querySelectorAll('.m5-settings-dialog button')).find(b=>b.textContent==='앱 기본값 저장').click()`);
        await wait(`document.querySelector('.m5-settings-dialog')?.textContent.includes('앱 기본값을 저장했습니다.')`);
        await run(`Array.from(document.querySelectorAll('.m5-settings-dialog button')).find(b=>b.textContent==='작업대로 돌아가기').click()`);
        await wait(`!document.querySelector('.m5-settings-dialog')`);
      }
      await run(require('./m5-smoke-complete-team.cjs'));
      // Settings → "작업 공간 폴더" through main's own picker.
      pickQueue.push(workspacesRoot);
      const folder = await run(`window.m5Projects.request({action:'chooseWorkspaceFolder'})`);
      if (folder.folder?.path !== fs.realpathSync(workspacesRoot)) throw new Error('workspace folder not set: ' + JSON.stringify(folder));
      if (projectsFlow) {
        const shots = {};
        const shot = async name => { await new Promise(r => setTimeout(r, 400)); await run(`new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))`); shots[name] = path.join(root, name + '.png'); fs.writeFileSync(shots[name], (await window.webContents.capturePage()).toPNG()); };
        const permissions = async () => run(`(async()=>{const list=await window.m5Projects.request({action:'list',cursor:null});const p=list.projects.find(e=>e.canonicalPath.endsWith('/project'));return p?await window.m5Projects.request({action:'getAutomationPermissions',projectId:p.projectId}):null;})()`);
        const startButton = `Array.from(document.querySelectorAll('.m5-first-run button')).find(b=>b.textContent==='프로젝트 선택')`;
        await shot('01-first-run');
        // B1: an empty folder is asked about in main's own box. "Cancel" opens nothing and writes nothing.
        pickQueue.push(emptySource); messageBoxAnswer = 1;
        await run(`${startButton}.click()`);
        const boxDeadline = performance.now() + 10000;
        while (messageBoxes.length === 0 && performance.now() < boxDeadline) await new Promise(r => setTimeout(r, 50));
        if (messageBoxes.length !== 1 || !messageBoxes[0].detail.includes(fs.realpathSync(emptySource))) throw new Error('empty folder was not confirmed in main');
        await wait(`${startButton}&&!${startButton}.disabled`);
        if (fs.readdirSync(emptySource).length !== 0) throw new Error('a declined empty folder was written');
        if (await run(`Boolean(document.querySelector('[data-testid=task-layout]'))`)) throw new Error('a declined folder opened');
        // A cancelled picker leaves the first run as it was.
        pickQueue.push(null);
        await run(`${startButton}.click()`);
        await wait(`${startButton}&&!${startButton}.disabled&&Boolean(document.querySelector('[data-testid=first-run]'))`);
        // First project, then the home it leads.
        await run(`${startButton}.click()`);
        await wait(`Boolean(document.querySelector('[data-testid=task-layout]'))&&!document.querySelector('.m5-new-chat-button').disabled`, 20000);
        await clickText('프로젝트 화면 닫기');
        await wait(`Boolean(document.querySelector('.m5-home:not([hidden]) .m5-home-hero'))&&!document.querySelector('[data-testid=task-layout]')`);
        // Add a second project from the home.
        pickQueue.push(secondSource);
        await run(`document.querySelector('.m5-home .m5-project-add').click()`);
        await wait(`Boolean(document.querySelector('[data-testid=task-layout]'))&&document.querySelector('.m5-switcher-projects li.m5-project-open')?.dataset.path?.endsWith('/shop')`, 20000);
        // The switcher with no task open: Add works, and a cancelled picker keeps the open project.
        await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
        await wait(`document.querySelector('.m5-switcher-projects li:not(.m5-project-open) .m5-project-row')`);
        if (await run(`document.querySelector('.m5-switcher-footer button').disabled`)) throw new Error('switcher add off with nothing open');
        if (await run(`document.querySelector('.m5-switcher-projects').innerText.includes(${JSON.stringify(root)})`)) throw new Error('switcher shows folder paths');
        await shot('03-switcher');
        pickQueue.push(null);
        await run(`document.querySelector('.m5-switcher-footer button').click()`);
        await new Promise(r => setTimeout(r, 700));
        if (!await run(`Boolean(document.querySelector('[data-testid=task-layout]'))&&document.querySelector('.m5-switcher-projects li.m5-project-open')?.dataset.path?.endsWith('/shop')`)) throw new Error('a cancelled add left the open project');
        // Switch in one step.
        await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
        await run(`document.querySelector('.m5-switcher-projects li:not(.m5-project-open) .m5-project-row').click()`);
        await wait(`Boolean(document.querySelector('[data-testid=task-layout]'))&&document.querySelector('.m5-switcher-projects li.m5-project-open')?.dataset.path?.endsWith('/project')`, 20000);
        // The home, both projects; delete asks with the whole folder (PR #12 review I5), Cancel first and keeps it.
        await clickText('프로젝트 화면 닫기');
        await wait(`document.querySelectorAll('.m5-home:not([hidden]) li[data-path]').length===2`);
        if (!await run(`document.querySelector('.m5-home-hero').dataset.path.endsWith('/project')`)) throw new Error('the most recently opened project does not lead');
        await shot('04-home');
        await run(`Array.from(document.querySelectorAll('.m5-home li')).find(li=>li.dataset.path.endsWith('/shop')).querySelector('.m5-project-context-popup .m5-danger').click()`);
        await wait(`document.querySelector('.m5-confirm-dialog[open] [data-testid=remove-target] code')`);
        if (await run(`document.querySelector('[data-testid=remove-target] code').textContent`) !== fs.realpathSync(secondSource)) throw new Error('delete confirmation does not show the whole folder');
        if (await run(`document.activeElement?.textContent`) !== '취소') throw new Error('delete confirmation must focus Cancel');
        await shot('05-delete');
        await run(`document.activeElement.click()`);
        await wait(`!document.querySelector('.m5-confirm-dialog[open]')`);
        if (!fs.existsSync(path.join(secondSource, 'index.html'))) throw new Error('cancelled delete touched the folder');
        // Continue the first project; its first task stores nothing before the separate yes (Q9, PR #12 review I1).
        await run(`document.querySelector('.m5-home .m5-home-continue').click()`);
        await wait(`Boolean(document.querySelector('[data-testid=task-layout]'))&&!document.querySelector('.m5-new-chat-button').disabled`, 20000);
        await run(`document.querySelector('.m5-new-chat-button').click()`);
        await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
        await fill('.m5-new-task-goal input', 'Improve hello');
        await wait(`document.querySelector('.m5-first-chat-confirm input')&&!document.querySelector('.m5-first-chat-confirm input').disabled`);
        if (await run(`Boolean(document.querySelector('.m5-new-chat-dialog .m5-permission-save'))`)) throw new Error('first task window has its own permission save');
        if ((await permissions()).recorded) throw new Error('permissions recorded before the first yes');
        await run(`document.querySelector('.m5-first-chat-confirm input').click()`);
        await wait(`!document.querySelector('.m5-new-chat-dialog [type=submit]').disabled`);
        // Changing a shown permission after the yes asks again.
        await run(`document.querySelectorAll('.m5-new-chat-dialog .m5-permission-options input')[1].click()`);
        await wait(`!document.querySelector('.m5-first-chat-confirm input').checked&&document.querySelector('.m5-new-chat-dialog [type=submit]').disabled`);
        if ((await permissions()).recorded) throw new Error('changing a permission stored it');
        await run(`document.querySelector('.m5-first-chat-confirm input').click()`);
        await wait(`!document.querySelector('.m5-new-chat-dialog [type=submit]').disabled`);
        await shot('06-first-task');
        await run(`document.querySelector('.m5-new-chat-dialog [type=submit]').click()`);
        await wait(`!document.querySelector('.m5-new-chat-dialog')?.open`, 20000);
        const stored = await permissions();
        if (!stored.recorded || stored.permissions.files !== true || stored.permissions.commands !== false) throw new Error('first task did not store the shown permissions: ' + JSON.stringify(stored));
        // With a task open, Add in the switcher is off and says why (PR #12 review I2).
        await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
        await wait(`document.querySelector('.m5-switcher-footer button')`);
        const addState = await run(`({disabled:document.querySelector('.m5-switcher-footer button').disabled,reason:document.querySelector('.m5-switch-blocked')?.textContent??null})`);
        if (!addState.disabled || !addState.reason) throw new Error('switcher add with a task open: ' + JSON.stringify(addState));
        await shot('07-switcher-task-open');
        finish(null, {scenario:'projects', messageBoxes, addState, shots});
        return;
      }
      const snapshotStarted = performance.now();
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='프로젝트 선택').click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent.includes('hello.txt'))`, largeTree ? 65000 : 15000);
      const snapshotReadyMs = performance.now() - snapshotStarted;
      console.log(JSON.stringify({stage:'SNAPSHOT_READY',snapshotReadyMs,root}));
      if (largeDiff) {
        const matches = execFileSync('/usr/bin/find',[path.join(root,'userdata'),'-type','d','-name','native-diff-fixture'],{encoding:'utf8'}).trim().split('\n').filter(Boolean);
        if(matches.length!==1)throw new Error('expected one generated diff snapshot');
        const snapshot = path.dirname(matches[0]);
        const dimensions = JSON.parse(execFileSync('node',['-e', 'console.log(JSON.stringify(require(process.argv[1]).change(process.argv[2])))',path.join(repoRoot,'tests/m5-native-diff-fixture.cjs'),snapshot],{encoding:'utf8'}));
        await showPanel('변경');
        largeDiffReport = await run(`(async()=>{
          const observe=async(ready)=>{const end=performance.now()+15000;while(!ready()){if(performance.now()>end)throw new Error('native diff timeout');await new Promise(r=>setTimeout(r,10));}await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));};
          const start=performance.now();
          const refresh=Array.from(document.querySelectorAll('.m5-changes button')).find(b=>b.textContent==='변경 새로 고침');
          if(!refresh||refresh.disabled)throw new Error('diff refresh unavailable');refresh.click();
          await observe(()=>document.querySelector('.m5-changes')?.textContent.includes('변경 (500)'));
          const diffSummaryMs=performance.now()-start;
          const file=Array.from(document.querySelectorAll('.m5-change-row')).find(b=>b.title==='native-diff-fixture/file-000.txt');
          if(!file)throw new Error('first diff file missing');
          const opened=performance.now();file.click();
          await observe(()=>document.querySelector('.m5-diff-details')?.textContent.includes('after--'));
          return {diffSummaryMs,diffFirstFileMs:performance.now()-opened};
        })()`);
        largeDiffReport={...largeDiffReport,...dimensions,diagnostic:true,gateEligible:false};
        const original = execFileSync('/usr/bin/git',['-C',source,'status','--porcelain'],{encoding:'utf8'}).trim();
        if(original)throw new Error('large diff source changed');
        if(largeDiffReport.diffSummaryMs>2000||largeDiffReport.diffFirstFileMs>1500)throw new Error('native diagnostic diff limits exceeded');
        const navigation = await run(`(${require('../tests/m5-change-navigation-probe.cjs').toString()})()`);
        finish(null,{scenario:'nativeLargeDiffOnly',navigation});return;
      }
      if (largeTree) {
        app.focus({steal:true}); window.show(); window.focus(); window.webContents.focus();
        const startedMs = performance.now();
        largeTreeReport = await run(`(async()=>{
          const observe=async(label,ready)=>{
            const start=performance.now();
            const button=Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes(label)&&!b.disabled);
            if(!button)throw new Error('tree target absent: '+label); button.click();
            while(!ready()) {
              if(performance.now()-start>15000)throw new Error('native tree timeout: '+label);
              await new Promise(r=>setTimeout(r,10));
            }
            await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
            return {ms:performance.now()-start,domNodes:document.querySelectorAll('.m5-file-list *').length};
          };
          const visible=label=>Array.from(document.querySelectorAll('.m5-files button')).some(b=>b.textContent.includes(label));
          const root=await observe('aaa-tree',()=>visible('folder-0001'));
          const expand=await observe('folder-0001',()=>visible('file-0.txt')&&!visible('folder-0002'));
          document.querySelector('.m5-breadcrumbs .m5-root').click();
          return {treeRootMs:root.ms,treeExpandMs:expand.ms,treeDomNodes:Math.max(root.domNodes,expand.domNodes),
            treeFiles:10000,treeDirectories:1000,controlFiles:${largeFile ? 2 : 1},diagnostic:true,gateEligible:false};
        })()`);
        await wait(`Array.from(document.querySelectorAll('.m5-files button')).some(b=>b.textContent.includes('hello.txt'))`);
        largeTreeReport.snapshotReadyMs = snapshotReadyMs;
        largeTreeReport.startedMs = startedMs;
        largeTreeReport.finishedMs = performance.now();
        console.log(JSON.stringify({stage:'NATIVE_LARGE_TREE',...largeTreeReport}));
      }
      if (largeFile) {
        app.focus({steal:true}); window.show(); window.focus(); window.webContents.focus();
        largeFileReport = await run(`(async()=>{
          const start=performance.now();
          const file=Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('large.txt'));
          if(!file)throw new Error('large fixture absent'); file.click();
          while(!document.querySelector('.monaco-editor .view-line')?.textContent.includes('가') || !document.querySelector('.monaco-editor textarea')) {
            if(performance.now()-start>15000)throw new Error('native large-file open timeout');
            await new Promise(r=>setTimeout(r,10));
          }
          await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
          return {fileEditableMs:performance.now()-start,bytes:5242880,lines:100000,diagnostic:true,gateEligible:false};
        })()`);
        console.log(JSON.stringify({stage:'NATIVE_LARGE_FILE',...largeFileReport}));
      }
      // The agents' selection lives in settings (since 2026-11-12) and the team shows as chips in the new-task window (ADE U3).
      await run(`document.querySelector('.m5-goal-examples button').click()`);
      await wait(`document.querySelector('.m5-new-task-goal input').value.length>0&&Array.from(document.querySelectorAll('.m5-goal-examples button')).every(b=>b.disabled)`);
      if (await run(`document.querySelectorAll('.m5-chat-chip').length!==0`)) throw new Error('goal example created a chat');
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      const exampleGoal = await run(`document.querySelector('.m5-new-task-goal input').value`);
      await run(`document.querySelector('.m5-new-chat-dialog header button').click()`);
      await wait(`!document.querySelector('.m5-new-chat-dialog')?.open`);
      await run(`document.querySelector('.m5-new-chat-button').click()`);
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      if (await run(`document.querySelector('.m5-new-task-goal input').value!==${JSON.stringify(exampleGoal)}`)) throw new Error('closing new chat lost goal draft');
      await fill('.m5-new-task-goal input', '');
      await run(`document.querySelector('.m5-new-chat-dialog header button').click()`);
      await wait(`!document.querySelector('.m5-new-chat-dialog')?.open`);
      // Team roles from the switcher menu (the PR #8 editor; its draft rules have their own tests). Closing returns
      // focus to a control on the page, not to the document (PR #12 review minor 1).
      await run(`document.querySelector('[data-testid=project-switcher]').open=true`);
      // The team roles item stays off until the project's data is read: wait for it, never click it disabled.
      const rolesItem = `document.querySelector('.m5-switcher-projects li.m5-project-open .m5-project-context-popup button:not(.m5-project-manage):not(.m5-danger)')`;
      await wait(`Boolean(${rolesItem})&&!${rolesItem}.disabled`);
      await run(`${rolesItem}.click();document.querySelector('[data-testid=project-switcher]').open=false`);
      await wait(`document.querySelector('.m5-settings-dialog')?.matches(':modal')`);
      await clickText('프로젝트로 돌아가기', '.m5-settings-dialog');
      await wait(`!document.querySelector('.m5-settings-dialog')`);
      await wait(`document.activeElement&&document.activeElement!==document.body&&document.activeElement.matches('button, summary')`);
      console.log(JSON.stringify({stage:'START_AND_SETTINGS',root}));
      await run(`document.querySelector('.m5-new-chat-button').click()`);
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      await run(`(()=>{const e=document.querySelector('.m5-new-chat-form input:not([type=checkbox])');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'Improve hello');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await wait(`document.querySelector('.m5-first-chat-confirm input')&&!document.querySelector('.m5-first-chat-confirm input').disabled`);
      await run(`document.querySelector('.m5-first-chat-confirm input').click()`);
      await wait(`!document.querySelector('.m5-new-chat-form button[type=submit]').disabled`);
      await run(`document.querySelector('.m5-new-chat-form button[type=submit]').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent==='Improve hello'`, largeTree ? 65000 : 15000);
      if (discardFlow) {
        const identity = await run(`(async()=>{const catalog=await window.m5Projects.request({action:'list',cursor:null});const projectId=catalog.projects[0].projectId;const page=await window.m5.listChats({projectId,cursor:null,limit:100});return page.items.find(c=>c.goal==='Improve hello');})()`);
        await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='실제 실행 준비 · 코딩').click()`);
        await wait(`Boolean(document.querySelector('.m5-chat-discard > button'))`, 25000);
        await run(`document.querySelector('.m5-chat-discard > button').click()`);
        await wait(`Boolean(document.querySelector('.m5-discard-dialog[open] input'))`);
        if (!await run(`document.querySelector('.m5-discard-actions button:last-child').disabled`)) throw new Error('discard consent missing');
        await run(`document.querySelector('.m5-discard-dialog input').click()`);
        await wait(`!document.querySelector('.m5-discard-actions button:last-child').disabled`);
        await run(`document.querySelector('.m5-discard-actions button:last-child').click()`);
        await wait(`Boolean(document.querySelector('.m5-guided-start'))&&!document.querySelector('[data-testid=task-layout]')`);
        const detail = await run(`window.m5.getChat({chatId:${JSON.stringify(identity.chatId)},afterEventSequence:0,cursor:null,limit:50})`);
        if (detail.chat.state !== 'discarded' || detail.messages.length === 0) throw new Error('discard did not preserve terminal chat history');
        if (fs.readFileSync(path.join(source,'hello.txt'),'utf8') !== 'original\n') throw new Error('discard changed original project');
        const screenshot=path.join(root,'discard.png');fs.writeFileSync(screenshot,(await window.webContents.capturePage()).toPNG());
        finish(null,{scenario:'appDefaultsAndDiscard',chatId:identity.chatId,screenshot,syntheticProvider:true});return;
      }
      await run(`Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('hello.txt')).click()`);
      app.focus({steal:true}); window.show(); window.focus(); window.webContents.focus();
      await wait(`document.querySelector('.monaco-editor textarea')!==null`);
      await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'})`);
      await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('original')`);
      window.show(); window.focus(); window.webContents.focus();
      await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'});document.querySelector('.monaco-editor textarea').focus()`);
      await new Promise(r=>setTimeout(r,500));
      await run(`document.querySelector('.monaco-editor textarea').focus()`);
      await wait(`document.activeElement===document.querySelector('.monaco-editor textarea')`);
      const focusDeadline = performance.now() + (nativeInput ? 45000 : 3000);
      if (nativeInput) console.log(JSON.stringify({stage:'WAITING_FOR_NATIVE_FOCUS',root}));
      if (!window.isFocused()) await app.dock?.show();
      while ((!window.isFocused() || !window.webContents.isFocused() || !await run('document.hasFocus()')) && performance.now() < focusDeadline) {
        app.focus({steal:true}); window.show(); window.focus(); window.webContents.focus();
        await run(`document.querySelector('.monaco-editor textarea')?.focus()`);
        await new Promise(r => setTimeout(r, 100));
      }
      const inputFocus = {
        window: window.isFocused(),
        contents: window.webContents.isFocused(),
        document: await run('document.hasFocus()'),
        appHidden: app.isHidden(),
        focusable: window.isFocusable(),
        enabled: window.isEnabled(),
        focusedWindowId: BrowserWindow.getFocusedWindow()?.id ?? null,
        windowId: window.id,
      };
      console.log(JSON.stringify({stage:'EDITOR_INPUT_FOCUS',...inputFocus}));
      if (!inputFocus.window || !inputFocus.contents || !inputFocus.document) {
        // Keep this failure in the final verdict, but collect independent
        // save/diff/chat evidence using targeted insertion before quitting.
        nativeFocusUnavailable = true;
      }
      if (nativeInput) {
        if (nativeFocusUnavailable) throw new Error('Native window focus unavailable; unlock the desktop before retrying');
        console.log(JSON.stringify({stage:'WAITING_FOR_NATIVE_INPUT',root,expected:'edited in snapshot'}));
        await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('edited in snapshot')`, 45000);
      } else await window.webContents.insertText('edited in snapshot\n');
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='저장'&&!b.disabled)`);
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='저장').click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='저장'&&b.disabled)`);
      await wait(`document.querySelector('.m5-changes')?.textContent.includes('hello.txt') || document.body.innerText.includes('변경 (1)')`);
      // L1 layout: the saved edit is a row in the changes tab, and its diff opens from there.
      await showPanel('변경');
      await wait(`Array.from(document.querySelectorAll('.m5-change-row')).some(b=>b.title==='hello.txt'&&!b.disabled)`);
      // The save starts a fresh read of the changes; a comparison opened while it is on its way is
      // dropped when it lands, so the row is opened again until its comparison shows.
      // Monaco draws spaces as no-break spaces.
      const diffShown = `document.querySelector('.m5-sheet-head h2')?.textContent==='바뀐 내용'&&(document.querySelector('.m5-diff-details')?.textContent??'').replace(/\\u00a0/g,' ').includes('edited in snapshot')`;
      const openHelloDiff = async () => {
        for (let attempt = 0; attempt < 10 && !await run(diffShown); attempt++) {
          await run(`Array.from(document.querySelectorAll('.m5-change-row')).find(b=>b.title==='hello.txt'&&!b.disabled)?.click()`);
          const until = performance.now() + 1500;
          while (performance.now() < until && !await run(diffShown)) await new Promise(r=>setTimeout(r,100));
        }
        await wait(`(${diffShown})&&Boolean(document.querySelector('.m5-diff-details .monaco-diff-editor, .m5-diff-details .monaco-editor'))`);
      };
      await openHelloDiff();
      await new Promise(r=>setTimeout(r,700));
      if(await run(`document.body.innerText.includes('열어 둔 파일이 밖에서 바뀌었어요')`)) throw new Error('self-save reported as external change');
      if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='original\n') throw new Error('source changed');
      // The task's workspace is a real git worktree under the chosen workspace folder.
      const workspaceFiles = () => execFileSync('/usr/bin/find',[workspacesRoot,'-name','hello.txt','-not','-path','*/.git/*'],{encoding:'utf8'}).trim().split('\n').filter(Boolean);
      if(!workspaceFiles().some(file=>fs.readFileSync(file,'utf8')==='edited in snapshot\noriginal\n')) throw new Error('saved workspace content missing');
      await openHelloDiff();
      const followupReview = await run(`(${require('../tests/m5-diff-followup-probe.cjs').toString()})()`);
      console.log(JSON.stringify({stage:'DIFF_FOLLOWUP_REVIEW',...followupReview}));
      // Task = PR (L1 layout): the task is a branch merged into the folder's base. The result bar
      // offers the merge only after the team ran, and no provider runs here, so the merge goes
      // through the same bridge request the bar sends (the old apply/restore buttons are gone).
      if (await run(`Array.from(document.querySelectorAll('button')).some(b=>['작업 사본 diff 열기','이 작업의 반영 전 변경 목록 보기','이 변경 묶음으로 확인 준비'].includes(b.textContent))`)) throw new Error('the old apply flow is still shown');
      const mergeAddress = `(async()=>{const catalog=await window.m5Projects.request({action:'list',cursor:null});const projectId=catalog.projects.find(p=>p.canonicalPath.endsWith('/project')).projectId;
        const page=await window.m5.listChats({projectId,cursor:null,limit:100});const chat=page.items.find(c=>c.goal==='Improve hello');
        return {chatId:chat.chatId,projectId:chat.projectId,worktreeId:chat.worktreeId,workspaceIdentity:chat.workspaceIdentity};})()`;
      const address = await run(mergeAddress);
      const inspect = () => run(`window.m5Projects.request(${JSON.stringify({action:'inspectChatMerge',...address})})`);
      const mergeStarted = performance.now();
      confirmationReport = {status:'pending',diagnostic:true,gateEligible:false};
      const before = (await inspect()).merge;
      confirmationReport = {...confirmationReport,status:'ready',readyMs:performance.now()-mergeStarted,merge:before};
      console.log(JSON.stringify({stage:'MERGE_STATE',...confirmationReport}));
      if (before.kind !== 'gitWorktree' || !before.uncommitted || !before.baseCheckedOut || !/^elsecrew\/task-/.test(before.branch ?? '')) throw new Error('the task is not a mergeable task branch: ' + JSON.stringify(before));
      if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='original\n') throw new Error('reading the merge state changed source');
      const applied = process.env.ELSECREW_SMOKE_APPLY === '1';
      if (applied) {
        // A confirmation for an outdated head is refused and changes nothing.
        const stale = await run(`window.m5Projects.request(${JSON.stringify({action:'mergeChatWorkspace',...address,expectedHead:'0'.repeat(40),expectedBase:before.baseTip,keepConflicts:false,confirmed:true})})`).catch(error => ({rejected:String(error?.message ?? error)}));
        if (stale?.outcome === 'merged') throw new Error('a stale confirmation merged');
        if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='original\n') throw new Error('a refused merge changed source');
        const merged = await run(`window.m5Projects.request(${JSON.stringify({action:'mergeChatWorkspace',...address,expectedHead:before.head,expectedBase:before.baseTip,keepConflicts:false,confirmed:true})})`);
        if (merged.outcome !== 'merged') throw new Error('merge did not complete: ' + JSON.stringify(merged));
        if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='edited in snapshot\noriginal\n') throw new Error('merge did not write the reviewed change into the folder');
        if(execFileSync('/usr/bin/git',['-C',source,'status','--porcelain'],{encoding:'utf8'}).trim()) throw new Error('merge left the folder with uncommitted changes');
        console.log(JSON.stringify({stage:'MERGED',root,stale}));
        if (process.env.ELSECREW_SMOKE_RELOAD === '1') {
          await new Promise(resolve => { window.webContents.once('did-finish-load', resolve); window.webContents.reload(); });
          // The app opens the last project again, or the home (ADE U2) leads with its continue button.
          await wait(`Boolean(document.querySelector('[data-testid=task-layout]'))||Boolean(document.querySelector('.m5-home:not([hidden]) .m5-home-continue:not(:disabled)'))`, 30000);
          if (!await run(`Boolean(document.querySelector('[data-testid=task-layout]'))`)) await run(`document.querySelector('.m5-home .m5-home-continue').click()`);
          await wait(`Array.from(document.querySelectorAll('.m5-chat-chip')).some(b=>b.querySelector('.m5-chat-goal')?.textContent==='Improve hello')`, 30000);
          await run(`Array.from(document.querySelectorAll('.m5-chat-chip')).find(b=>b.querySelector('.m5-chat-goal')?.textContent==='Improve hello').click()`);
          const after = (await inspect()).merge;
          if (after.blocked !== 'nothingToMerge') throw new Error('after the reload the task is not merged: ' + JSON.stringify(after));
          console.log(JSON.stringify({stage:'MERGED_AFTER_RELOAD',root}));
        }
      }
      // After a reload the button waits for the project's data.
      await wait(`!document.querySelector('.m5-new-chat-button').disabled`, 30000);
      await run(`document.querySelector('.m5-new-chat-button').click()`);
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      await run(`(()=>{const e=document.querySelector('.m5-new-chat-form input:not([type=checkbox])');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'Second task');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await wait(`!document.querySelector('.m5-new-chat-form button[type=submit]').disabled`);
      await run(`document.querySelector('.m5-new-chat-form button[type=submit]').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent==='Second task'`, largeTree ? 65000 : 15000);
      // The old provider selection block is gone from the L1 layout; if it ever returns it starts collapsed.
      if (await run(`document.querySelector('.m5-provider-selection')?.open===true`)) throw new Error('project workbench selection should start collapsed');
      await wait(`document.querySelector('.m5-chat-chip[aria-current=true] .m5-chat-goal')?.textContent==='Second task' && Boolean(document.querySelector('.m5-chat-chip[aria-current=true] .m5-chat-status')?.textContent)`);
      console.log(JSON.stringify({stage:'SECOND_CHAT_WINDOW',visible:window.isVisible(),minimized:window.isMinimized(),focused:window.isFocused(),backgroundThrottling:window.webContents.getBackgroundThrottling(),page:await run(`({visibility:document.visibilityState,focus:document.hasFocus()})`)}));
      window.show(); window.focus(); window.webContents.focus();
      await showPanel('파일');
      await run(`Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('hello.txt')).click()`);
      await wait(`document.querySelector('.monaco-editor textarea')!==null`);
      await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'})`);
      await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('original')`);
      // A new task starts from the folder's base: the first task's edit only after it was merged.
      if(await run(`document.querySelector('.monaco-editor .view-lines').textContent.includes('edited')`) !== applied) throw new Error(applied ? 'second task did not start from the merged base' : 'chat snapshots mixed');
      await run(`Array.from(document.querySelectorAll('.m5-chat-chip')).find(b=>b.querySelector('.m5-chat-goal')?.textContent==='Improve hello').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent==='Improve hello'`);
      await showPanel('파일');
      await run(`Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('hello.txt')).click()`);
      await wait(`document.querySelector('.monaco-editor textarea')!==null`);
      await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'})`);
      await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('edited')`);
      console.log(JSON.stringify({stage:'PROJECT_FLOW_COMPLETE',root}));
      // DOM assertions can complete before Chromium presents the updated editor.
      await run(`new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))`);
      const screenshot = path.join(root,'project.png');
      fs.writeFileSync(screenshot,(await window.webContents.capturePage()).toPNG());
      if (measureMemory) {
        await sampleMemory();
        memoryPhase = 'closing';
        await clickText('프로젝트 화면 닫기');
        // A saved project now leads to the home cards, not the first run (ADE U2).
        await wait(`Boolean(document.querySelector('.m5-home:not([hidden]) .m5-home-hero'))&&!document.querySelector('[data-testid=task-layout]')`);
        console.log(JSON.stringify({stage:'MEMORY_CLOSE_WAIT',root,waitMs:60000}));
        const closedAt = performance.now();
        await new Promise(resolve => setTimeout(resolve, 60000));
        if (memoryPending) await memoryPending;
        memoryPhase = 'retained';
        await sampleMemory();
        clearInterval(memoryTimer);
        if (memoryError) throw memoryError;
        const summary = summarizeMemory(memorySamples);
        memoryReport = { ...summary, actualCloseWaitMs: performance.now() - closedAt, status: summary.failedMetrics.length ? 'FAIL' : 'NOT_READY' };
        if (summary.failedMetrics.length) throw new Error('native diagnostic memory limits exceeded: '+summary.failedMetrics.join(', '));
      }
      if (largeFileReport?.fileEditableMs > 1500) throw new Error('native diagnostic file-open limit exceeded: '+largeFileReport.fileEditableMs);
      if (largeTreeReport && (largeTreeReport.treeRootMs > 1000 || largeTreeReport.treeExpandMs > 250 || largeTreeReport.treeDomNodes > 500)) throw new Error('native diagnostic tree limits exceeded');
      finish(nativeFocusUnavailable ? new Error('Native focus unavailable; input routing is not verified') : null,{screenshot,nativeFocusUnavailable,inputMethod:nativeInput?'native-window-input':'webContents.insertText',text:await run('document.body.innerText'),inputs:await run(`Array.from(document.querySelectorAll('input,textarea')).map(e=>({tag:e.tagName,placeholder:e.placeholder,disabled:e.disabled}))`)});
    } catch(error) {
      const screenshot = path.join(root,'failure.png');
      let captureError;
      try { fs.writeFileSync(screenshot,(await window.webContents.capturePage()).toPNG()); }
      catch (failure) { captureError = failure.message; }
      finish(error,{screenshot:captureError ? null : screenshot,captureError,details:await run(`Array.from(document.querySelectorAll('pre')).map(e=>e.textContent)`).catch(()=>[]),editor:await run(`Array.from(document.querySelectorAll('.m5-monaco,.monaco-editor,.view-lines')).map(e=>({class:e.className,rect:e.getBoundingClientRect().toJSON(),text:e.textContent?.slice(0,100)}))`).catch(()=>[])});
    }
  });
});
require(path.join(repoRoot, '.vite/build/main.cjs'));
