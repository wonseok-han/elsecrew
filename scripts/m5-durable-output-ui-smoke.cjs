// Development integration diagnostic, not packaged G6 or a provider-account test.
// Requires current .vite/build, renderer and target/debug sidecar artifacts.
const { app, dialog } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const assert = require('node:assert/strict');
if (process.platform !== 'darwin' || app.isPackaged) throw new Error('Development macOS diagnostic only');
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
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [source] });
const report = { schemaVersion: 1, diagnostic: true, gateEligible: false, root,
  status: 'RUNNING', shutdown: 'pending', restoredChats: [], scrollChecks: [],
  limitations: ['Development artifacts, synthetic providers, no actual accounts.',
    'Full retained-window and history checks; no 50-events/s paint or packaged RSS gate.',
    'Synthetic runs have no native approval queue; approval inspection rejection is not native-provider evidence.'] };
const persist = () => fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(report, null, 2));
persist();
const spawn = cp.spawn;
const expectedBinary = path.join(repo, 'target/debug/hello-agent-sidecar');
let child;
cp.spawn = function(command, args, options) {
  if (typeof command === 'string' && path.basename(command) === 'hello-agent-sidecar') {
    assert.equal(command, expectedBinary);
    assert.deepEqual(args, ['--stdio']);
    assert.equal(fs.realpathSync(options.cwd), fs.realpathSync(path.join(userData, 'com.wonseokhan.helloagent', 'm5')));
    assert.equal(child, undefined, 'unexpected second sidecar');
    child = Reflect.apply(spawn, this, [command, ['--stdio-agent-fixture', 'long-stream'], options]);
    child.once('close', (code, signal) => { report.sidecarExit = { code, signal }; persist(); });
    return child;
  }
  return Reflect.apply(spawn, this, [command, args, options]);
};
let finished = false;
function finish(error) {
  if (finished) return;
  finished = true;
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
      for (let index = 0; index < 4; index++) {
        await run(`[...document.querySelectorAll('.m5-chat-chip')].find(b => b.querySelector('.m5-chat-goal')?.textContent === 'Output stream ${index}').click()`);
        // Every chat has identical output text. Wait for the selected run and the
        // async chat load, not just rows that may still belong to the prior chat.
        await wait(`document.querySelector('.m5-durable-output select')?.value === outputSmokeRuns[${index}].runId &&
          [...document.querySelectorAll('.m5-chat-chip')].some(b => b.getAttribute('aria-current') === 'true' && !b.disabled && b.querySelector('.m5-chat-goal')?.textContent === 'Output stream ${index}')`);
        await wait("[...document.querySelectorAll('.m5-output-scroll article pre')].length === 200 && document.querySelector('.m5-output-scroll article:last-child pre')?.textContent === 'output 3000'", 240000);
        const visible = await run("[...document.querySelectorAll('.m5-output-scroll article pre')].map(e => e.textContent)");
        assert.deepEqual(visible, Array.from({length:200}, (_, i) => `output ${2801+i}`));
        await run("[...document.querySelectorAll('.m5-durable-output button')].find(b => b.textContent === '처음 기록부터 보기').click()");
        await wait("document.querySelector('.m5-output-scroll article:first-of-type pre')?.textContent === 'output 1'");
        const history = await run("[...document.querySelectorAll('.m5-output-scroll article pre')].map(e => e.textContent)");
        assert.deepEqual(history, Array.from({length:100}, (_, i) => `output ${i+1}`));
        await wait("document.querySelector('.m5-output-scroll')?.scrollTop === 0");
        await run("[...document.querySelectorAll('.m5-durable-output button')].find(b => b.textContent === '다음 기록').click()");
        await wait("document.querySelector('.m5-output-scroll article:first-of-type pre')?.textContent === 'output 101'");
        const nextHistory = await run("[...document.querySelectorAll('.m5-output-scroll article pre')].map(e => e.textContent)");
        assert.deepEqual(nextHistory, Array.from({length:100}, (_, i) => `output ${101+i}`));
        await run("[...document.querySelectorAll('.m5-durable-output button')].find(b => b.textContent === '최신 대화').click()");
        await wait("document.querySelector('.m5-output-scroll article:last-child pre')?.textContent === 'output 3000'");
        await wait("(() => { const list = document.querySelector('.m5-output-scroll'); return list && list.clientHeight > 0 && list.scrollHeight - list.scrollTop - list.clientHeight <= 2; })()");
        report.scrollChecks.push({chat:index,historyTop:true,nextPage:true,latestBottom:true});
        report.restoredChats.push(index); persist();
      }
      assert.equal(fs.readFileSync(path.join(source, 'hello.txt'), 'utf8'), 'original\n');
      report.visibility = await run('document.visibilityState');
      report.visibilityTrace = await run('outputSmokeVisibility');
      await fs.promises.writeFile(path.join(root, 'output.png'), (await window.webContents.capturePage()).toPNG());
      finish();
    } catch (error) {
      try {
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
