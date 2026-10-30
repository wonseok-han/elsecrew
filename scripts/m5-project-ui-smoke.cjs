// macOS native UI smoke: synthetic project and isolated app data, no providers selected.
// Requires current .vite/build output and native sidecar; this script does not build them.
const { app, BrowserWindow, dialog, ipcMain } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { execFile, execFileSync } = require('node:child_process');
const { processTreeMemory, summarizeMemory } = require('./m5-process-memory.cjs');
const discardFlow = process.env.HELLO_AGENT_SMOKE_DISCARD === '1';
const measureMemory = process.env.HELLO_AGENT_SMOKE_MEMORY === '1';
const idleOnly = process.env.HELLO_AGENT_SMOKE_IDLE_ONLY === '1';
const nativeInput = process.env.HELLO_AGENT_SMOKE_NATIVE_INPUT === '1';
if (idleOnly && !measureMemory) throw new Error('Idle-only diagnostic requires memory mode');
const largeFile = process.env.HELLO_AGENT_SMOKE_LARGE_FILE === '1';
const largeTree = process.env.HELLO_AGENT_SMOKE_LARGE_TREE === '1';
const largeDiff = process.env.HELLO_AGENT_SMOKE_LARGE_DIFF === '1';
if (nativeInput && (measureMemory || largeFile || largeTree || largeDiff)) throw new Error('Native input is a separate manual diagnostic scenario');
if (idleOnly && (largeFile || largeTree || largeDiff || process.env.HELLO_AGENT_SMOKE_APPLY === '1')) throw new Error('Idle-only diagnostic cannot include project scenarios');
if (largeDiff && (measureMemory || largeFile || largeTree || process.env.HELLO_AGENT_SMOKE_APPLY === '1')) throw new Error('Large diff is a separate diagnostic scenario');
if (discardFlow && (measureMemory || idleOnly || nativeInput || largeFile || largeTree || largeDiff || process.env.HELLO_AGENT_SMOKE_APPLY === '1')) throw new Error('Discard is a separate synthetic integration scenario');
if (discardFlow) {
  const cp = require('node:child_process');
  const assert = require('node:assert/strict');
  const originalSpawn = cp.spawn;
  cp.spawn = function(command, args, options) {
    if (typeof command === 'string' && path.basename(command) === 'hello-agent-sidecar') {
      assert.equal(command, path.resolve(__dirname, '../target/debug/hello-agent-sidecar'));
      assert.deepEqual(args, ['--stdio']);
      return Reflect.apply(originalSpawn, this, [command, ['--stdio-agent-fixture', 'clean'], options]);
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
const root = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'hello-agent-project-ui-'));
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
dialog.showOpenDialog = async () => ({canceled:false,filePaths:[source]});
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
    const showPanel = title => run(`(()=>{ const tab=Array.from(document.querySelectorAll('.m5-dock-group-tabs button')).find(b=>b.textContent===${JSON.stringify(title)}&&b.getClientRects().length); if(tab)tab.click(); })()`);
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
          memoryReport = { status: idleRssBytes > 400 * 1024 ** 2 ? 'FAIL' : 'NOT_READY', maxima: { idleRssBytes },
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
          finish(idleRssBytes > 400 * 1024 ** 2 ? new Error('native idle memory exceeds limit') : null, { screenshot, idleOnly: true });
          return;
        }
        memoryPhase = 'loaded';
      }
      if (!await run(`Boolean(document.querySelector('.m5-guided-start'))&&!document.querySelector('.m5-dock-tree')`)) throw new Error('initial guided start missing or empty dock exposed');
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
      await run(`(()=>{ const d=document.querySelector('.m5-provider-selection'); if(d?.open)d.querySelector('summary').click(); })()`);
      await wait(`document.querySelector('.m5-provider-selection')?.open===false`);
      if(await run(`document.querySelector('.m5-layout-settings')?.open!==false`)) throw new Error('layout settings not initially collapsed');
      await wait(`document.querySelectorAll('.m5-team-preview > div').length===3`);
      await run(`document.querySelector('.m5-goal-examples button').click()`);
      await wait(`document.querySelector('.m5-new-chat-form > input').value.length>0&&Array.from(document.querySelectorAll('.m5-goal-examples button')).every(b=>b.disabled)`);
      if (await run(`document.querySelectorAll('.m5-chat-chip').length!==0`)) throw new Error('goal example created a chat');
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      const exampleGoal = await run(`document.querySelector('.m5-new-chat-form > input').value`);
      await run(`document.querySelector('.m5-new-chat-dialog header button').click()`);
      await wait(`!document.querySelector('.m5-new-chat-dialog')?.open`);
      await run(`document.querySelector('.m5-new-chat-button').click()`);
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      if (await run(`document.querySelector('.m5-new-chat-form > input').value!==${JSON.stringify(exampleGoal)}`)) throw new Error('closing new chat lost goal draft');
      await fill('.m5-new-chat-form > input', '');
      await run(`document.querySelector('.m5-new-chat-dialog header button').click()`);
      await wait(`!document.querySelector('.m5-new-chat-dialog')?.open`);
      await clickText('역할과 모델');
      await wait(`document.querySelector('.m5-settings-dialog')?.matches(':modal')`);
      const originalModel = await run(`document.querySelector('.m5-role-card input').value`);
      if (discardFlow && originalModel !== 'fixture-default') throw new Error('new project did not inherit app defaults');
      const modelInput = '.m5-role-card input';
      await fill(modelInput, 'ui-smoke-unsaved');
      window.show(); window.focus(); window.webContents.focus();
      window.webContents.sendInputEvent({type:'keyDown',keyCode:'Escape'});
      window.webContents.sendInputEvent({type:'keyUp',keyCode:'Escape'});
      await wait(`Boolean(document.querySelector('.m5-settings-dialog [role=alertdialog]'))`);
      await clickText('설정 계속 편집', '.m5-settings-dialog');
      await wait(`!document.querySelector('.m5-settings-dialog [role=alertdialog]')`);
      if (await run(`document.querySelector('.m5-role-card input').value!=='ui-smoke-unsaved'`)) throw new Error('Escape lost settings draft');
      await clickText('작업대로 돌아가기', '.m5-settings-dialog');
      await clickText('설정 변경 버리고 닫기', '.m5-settings-dialog');
      await wait(`!document.querySelector('.m5-settings-dialog')`);
      await wait(`document.activeElement?.textContent==='역할과 모델'`);
      await clickText('역할과 모델');
      await wait(`document.querySelector('.m5-settings-dialog')?.matches(':modal')`);
      if (await run(`document.querySelector('.m5-role-card input').value!==${JSON.stringify(originalModel)}`)) throw new Error('discarded role draft persisted');
      await fill(modelInput, 'ui-smoke-saved');
      await clickText('새 채팅 설정 저장', '.m5-settings-dialog');
      await wait(`!Array.from(document.querySelectorAll('.m5-settings-dialog [role=status]')).some(e=>e.textContent==='저장하지 않은 설정 변경이 있어요.')&&!Array.from(document.querySelectorAll('.m5-settings-dialog button')).some(b=>b.textContent==='처리 중…')`);
      await clickText('작업대로 돌아가기', '.m5-settings-dialog');
      await wait(`!document.querySelector('.m5-settings-dialog')`);
      await clickText('역할과 모델');
      await wait(`document.querySelector('.m5-settings-dialog')?.matches(':modal')`);
      if (await run(`document.querySelector('.m5-role-card input').value!=='ui-smoke-saved'`)) throw new Error('saved role model not preserved');
      await fill(modelInput, originalModel);
      await clickText('새 채팅 설정 저장', '.m5-settings-dialog');
      await wait(`!Array.from(document.querySelectorAll('.m5-settings-dialog [role=status]')).some(e=>e.textContent==='저장하지 않은 설정 변경이 있어요.')&&!Array.from(document.querySelectorAll('.m5-settings-dialog button')).some(b=>b.textContent==='처리 중…')`);
      await clickText('작업대로 돌아가기', '.m5-settings-dialog');
      await wait(`!document.querySelector('.m5-settings-dialog')`);
      console.log(JSON.stringify({stage:'START_AND_SETTINGS',root}));
      await run(`document.querySelector('.m5-new-chat-button').click()`);
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      await run(`(()=>{const e=document.querySelector('.m5-new-chat-form input:not([type=checkbox])');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'Improve hello');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await run(`document.querySelector('.m5-new-chat-form input[type=checkbox]').click()`);
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
        await wait(`Boolean(document.querySelector('.m5-guided-start'))&&!document.querySelector('.m5-dock-tree')`);
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
      await showPanel('팀 대화');
      await run(`Array.from(document.querySelectorAll('.m5-chat-review button')).find(b=>b.textContent==='작업 사본 diff 열기').click()`);
      await wait(`Array.from(document.querySelectorAll('.m5-dock-group-tab')).some(b=>b.textContent.includes('변경')&&b.getAttribute('aria-selected')==='true') || Boolean(document.querySelector('.m5-changes')?.getBoundingClientRect().height)`);
      // Review opens a fresh summary; the previous rows are intentionally
      // cleared until that request resolves. Panel visibility is not readiness.
      await wait(`Array.from(document.querySelectorAll('.m5-change-row')).some(b=>b.title==='hello.txt'&&!b.disabled)`);
      await run(`Array.from(document.querySelectorAll('.m5-change-row')).find(b=>b.title==='hello.txt'&&!b.disabled).click()`);
      await wait(`document.querySelector('.m5-diff-details')?.textContent.includes('edited in snapshot')`);
      await new Promise(r=>setTimeout(r,700));
      if(await run(`document.body.innerText.includes('열어 둔 파일이 밖에서 바뀌었어요')`)) throw new Error('self-save reported as external change');
      if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='original\n') throw new Error('source changed');
      const files = execFileSync('/usr/bin/find',[path.join(root,'userdata'),'-name','hello.txt'],{encoding:'utf8'}).trim().split('\n').filter(Boolean);
      if(!files.some(file=>fs.readFileSync(file,'utf8')==='edited in snapshot\noriginal\n')) throw new Error('saved snapshot content missing');
      const followupReview = await run(`(${require('../tests/m5-diff-followup-probe.cjs').toString()})()`);
      console.log(JSON.stringify({stage:'DIFF_FOLLOWUP_REVIEW',...followupReview}));
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='이 채팅의 반영 전 변경 목록 보기').click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='이 변경 묶음으로 확인 준비'&&!b.disabled)`);
      const confirmationStarted = performance.now();
      confirmationReport = {status:'pending',diagnostic:true,gateEligible:false};
      await run(`(()=>{const button=Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='이 변경 묶음으로 확인 준비'); if(!button||button.disabled)throw new Error('confirmation trigger unavailable'); button.click();})()`);
      try {
        await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='확인 취소'&&!b.disabled)`, largeTree ? 65000 : 15000);
        confirmationReport = {...confirmationReport,status:'ready',readyMs:performance.now()-confirmationStarted};
      } catch(error) {
        confirmationReport = {...confirmationReport,status:'failed',elapsedMs:performance.now()-confirmationStarted,ui:await run(`({busy:document.querySelector('.m5-chat-review')?.getAttribute('aria-busy'),statuses:Array.from(document.querySelectorAll('.m5-chat-review [role=status]')).map(e=>e.textContent)})`)};
        throw error;
      }
      console.log(JSON.stringify({stage:'CONFIRMATION_READY',...confirmationReport}));
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='확인 취소').click()`);
      await wait(`!Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='확인 취소')`);
      if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='original\n') throw new Error('preview cancellation changed source');
      if (process.env.HELLO_AGENT_SMOKE_APPLY === '1') {
        await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='이 변경 묶음으로 확인 준비').click()`);
        await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='백업 후 원본에 반영')`);
        await run(`Array.from(document.querySelectorAll('label')).find(e=>e.textContent.includes('원본 파일을 변경하는 작업')).querySelector('input').click()`);
        await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='백업 후 원본에 반영').click()`);
        await wait(`document.body.innerText.includes('기록된 변경 묶음의 반영이 완료됐어요.')`);
        if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='edited in snapshot\noriginal\n') throw new Error('application did not write reviewed source');
        console.log(JSON.stringify({stage:'APPLIED',root}));
        if (process.env.HELLO_AGENT_SMOKE_RELOAD === '1') {
          await new Promise(resolve => { window.webContents.once('did-finish-load', resolve); window.webContents.reload(); });
          await wait(`document.querySelector('.m5-project-rail .m5-project-catalog-list li > button')&&!document.querySelector('.m5-project-rail .m5-project-catalog-list li > button').disabled`);
          await run(`document.querySelector('.m5-project-catalog-list li > button').click()`);
          await wait(`Array.from(document.querySelectorAll('.m5-chat-chip')).some(b=>b.querySelector('.m5-chat-goal')?.textContent==='Improve hello')`);
          await run(`Array.from(document.querySelectorAll('.m5-chat-chip')).find(b=>b.querySelector('.m5-chat-goal')?.textContent==='Improve hello').click()`);
        }
        await run(`Array.from(document.querySelectorAll('button')).filter(b=>b.textContent==='반영·복원 기록 확인').forEach(b=>b.click())`);
        await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='복구 기록의 파일 목록 확인')`);
        await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='복구 기록의 파일 목록 확인').click()`);
        await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='기록을 확인하고 복원 시작')`);
        await run(`Array.from(document.querySelectorAll('label')).filter(e=>e.textContent.includes('원본 파일을 변경하는 작업')).find(e=>!e.querySelector('input').checked).querySelector('input').click()`);
        await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='기록을 확인하고 복원 시작').click()`);
        await wait(`document.body.innerText.includes('기록된 변경 묶음의 복원이 완료됐어요.')`);
        if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='original\n') throw new Error('restoration did not restore source');
        console.log(JSON.stringify({stage:'RESTORED',root}));
      }
      await run(`document.querySelector('.m5-new-chat-button').click()`);
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      await run(`(()=>{const e=document.querySelector('.m5-new-chat-form input:not([type=checkbox])');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'Second task');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await wait(`!document.querySelector('.m5-new-chat-form button[type=submit]').disabled`);
      await run(`document.querySelector('.m5-new-chat-form button[type=submit]').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent==='Second task'`, largeTree ? 65000 : 15000);
      if (await run(`document.querySelector('.m5-provider-selection')?.open!==false`)) throw new Error('project workbench selection should start collapsed');
      await wait(`document.querySelector('.m5-chat-chip[aria-current=true] .m5-chat-goal')?.textContent==='Second task' && Boolean(document.querySelector('.m5-chat-chip[aria-current=true] .m5-chat-status')?.textContent)`);
      console.log(JSON.stringify({stage:'SECOND_CHAT_WINDOW',visible:window.isVisible(),minimized:window.isMinimized(),focused:window.isFocused(),backgroundThrottling:window.webContents.getBackgroundThrottling(),page:await run(`({visibility:document.visibilityState,focus:document.hasFocus()})`)}));
      window.show(); window.focus(); window.webContents.focus();
      await showPanel('파일');
      await run(`Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('hello.txt')).click()`);
      await wait(`document.querySelector('.monaco-editor textarea')!==null`);
      await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'})`);
      await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('original')`);
      if(await run(`document.querySelector('.monaco-editor .view-lines').textContent.includes('edited')`)) throw new Error('chat snapshots mixed');
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
        await wait(`Boolean(document.querySelector('.m5-guided-start'))&&!document.querySelector('.m5-dock-tree')`);
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
