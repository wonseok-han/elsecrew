// Explicit user approval required: one small model run per provider, generated project only.
const { app, dialog } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
if (process.env.HELLO_AGENT_APPROVED_LIVE_SMOKE !== '1') throw new Error('Explicit live-run approval required');
const selectedProvider = process.env.HELLO_AGENT_LIVE_PROVIDER;
if (selectedProvider && !['codex', 'claude'].includes(selectedProvider)) throw new Error('Unknown live provider selection');
const providers = selectedProvider ? [selectedProvider] : ['codex', 'claude'];
const userData = process.env.HELLO_AGENT_LIVE_USER_DATA;
if (!userData || !path.isAbsolute(userData) || !fs.statSync(userData).isDirectory()) throw new Error('Existing app userData required');
const root = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'hello-agent-live-'));
const source = path.join(root, 'project');
fs.mkdirSync(source);
fs.writeFileSync(path.join(source, 'hello.txt'), 'original\n');
for (const args of [['init','-q','--template='],['add','.'],['-c','core.hooksPath=/dev/null','-c','user.name=Smoke','-c','user.email=smoke@example.invalid','commit','-qm','fixture']]) execFileSync('/usr/bin/git', args, {cwd:source, env:{...process.env,GIT_CONFIG_NOSYSTEM:'1',GIT_CONFIG_GLOBAL:'/dev/null',GIT_CONFIG_COUNT:'0'}});
const appUserData = fs.realpathSync(userData);
app.setPath('userData', appUserData);
dialog.showOpenDialog = async () => ({canceled:false,filePaths:[source]});
const outcomes = [];
const {startAgentPaintProbe} = require('./m5-agent-paint-probe.cjs');
function checkedLoginResult(status, attemptId) {
  if (status.attemptId !== attemptId) throw new Error('Login check attempt changed');
  if (['diagnosing','starting','verifying'].includes(status.state)) return null;
  return {state:status.state,error:status.error};
}
// Observe only fixed sidecar checkpoints, never export its raw stderr (which
// may contain private provider data). This does not affect run admission.
const transportCheckpoints = new Set();
const childProcess = require('node:child_process');
const originalSpawn = childProcess.spawn;
childProcess.spawn = function (...args) {
  const child = Reflect.apply(originalSpawn, this, args);
  if (typeof args[0] === 'string' && path.basename(args[0]) === 'hello-agent-sidecar') {
    let tail = '';
    child.stderr?.on('data', chunk => {
      const text = tail + chunk.toString('utf8');
      const lines = text.split('\n');
      tail = lines.pop().slice(-256);
      for (const line of lines) {
        const codex = /^native Codex transport failed at (thread\/start|turn start(?: state| envelope| notification shape| notification correlation| notification state| reply shape| reply correlation| reply state| stream binding)?|turn stream(?: state| thread status| token usage| started| completed| item started| item completed| message delta| reasoning| request resolved| provider error| diff metadata| plan metadata| thread name| thread settings| hook| model metadata| warning| account metadata| mcp metadata| moderation metadata| other frame)?)$/.exec(line);
        const claude = /^native Claude transport failed at (start binding|control initialization|profile matching|owner preparation|prompt admission|handoff cancellation|session initialization|session output|session input|session channel|session receipt|session process observation|session persistence|session handler|channel state|channel send|channel receive|session state|session budget|session tag|session before init|session event|init expected|init shape|init identity|init cwd|init model|init permission|init tools|init extensions|init mcp|init skills|init plugins(?: disabled builtins| organization guard| telemetry| other builtins| other registration)?|init agents|init plugin errors|init capabilities)$/.exec(line);
        if (codex) transportCheckpoints.add(`codex:${codex[1]}`);
        if (claude) transportCheckpoints.add(`claude:${claude[1]}`);
      }
    });
  }
  return child;
};
// Separate read-only probe: a catalog PASS must never imply a model-run PASS.
const catalogOnly = process.env.HELLO_AGENT_LIVE_MODEL_CHOICES_ONLY === '1';
let finished = false;
const timer = setTimeout(() => finish('timeout'), 240000);
function finish(error) {
  if (finished) return;
  finished = true; clearTimeout(timer);
  const unchanged = execFileSync('/usr/bin/git', ['status','--porcelain'], {cwd:source,encoding:'utf8'}).trim() === '';
  const runsPassed = !error && unchanged && (catalogOnly
    ? outcomes.length===1 && outcomes[0].state==='catalogObserved'
    : outcomes.length===providers.length && providers.every(p=>outcomes.some(o=>o.provider===p&&o.state==='succeeded'&&o.terminationOutcome==='cleanExit'&&o.responseObserved)));
  const result = {root, appUserData, verificationKind:catalogOnly?'modelChoicesOnly':'actualProviderRuns', providers, outcomes, transportCheckpoints:[...transportCheckpoints], sourceUnchanged:unchanged, error, shutdown:'pending', status:'PENDING'};
  const persist = () => fs.writeFileSync(path.join(root,'result.json'), JSON.stringify(result,null,2));
  persist();
  console.log(JSON.stringify({stage:'RUNS_FINISHED',root,outcomes,sourceUnchanged:unchanged,error}));
  // A successful model response is not proof that app cleanup accepted exit.
  // Preserve failure evidence without killing the app or bypassing its blockers.
  const shutdownTimer = setTimeout(() => {
    result.shutdown = 'notConfirmed';
    result.status = 'FAIL';
    persist();
    console.log(JSON.stringify(result));
  }, 15000);
  app.once('will-quit', () => {
    clearTimeout(shutdownTimer);
    result.shutdown = 'accepted';
    result.status = runsPassed ? 'PASS' : 'FAIL';
    persist();
    console.log(JSON.stringify(result));
    app.exit(result.status==='PASS'?0:1);
  });
  app.quit();
}
app.on('browser-window-created', (_event, window) => {
  window.webContents.setBackgroundThrottling(false);
  window.webContents.once('did-finish-load', async () => {
    const run = async code => {
      // The watchdog may finish while an IPC/UI request is still pending.
      // Never let a late response advance to another paid run after shutdown.
      if (finished) throw new Error('Smoke session has finished');
      const result = await window.webContents.executeJavaScript(code);
      if (finished) throw new Error('Smoke session has finished');
      return result;
    };
    const wait = async predicate => { for(let i=0;i<300;i++){if(await run(predicate))return;await new Promise(r=>setTimeout(r,100));}throw new Error('UI readiness timeout'); };
    try {
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='프로젝트 선택'&&!b.disabled)`);
      const connected = {};
      for(const provider of providers) {
        // The mounted login panel diagnoses and checks automatically. Let that
        // owner finish before issuing the runner's explicit request.
        await wait(`window.m5Login.request({action:'status',provider:${JSON.stringify(provider)}}).then(s=>!['installed','diagnosing','starting','verifying'].includes(s.state))`);
        if (process.env.HELLO_AGENT_APPROVED_LOGIN_RECOVERY === provider) {
          const recovery = await run(`window.m5Login.request({action:'recover',provider:${JSON.stringify(provider)},attemptId:crypto.randomUUID()}).then(s=>({state:s.state,error:s.error}))`);
          console.log(JSON.stringify({stage:'LOGIN_RECOVERY',provider,...recovery}));
        }
        const attemptId = require('node:crypto').randomUUID();
        let checked = checkedLoginResult(await run(`window.m5Login.request({action:'check',provider:${JSON.stringify(provider)},attemptId:${JSON.stringify(attemptId)}}).then(s=>({state:s.state,error:s.error,attemptId:s.attemptId}))`), attemptId);
        for (let poll = 0; !checked && poll < 300; poll++) {
          await new Promise(resolve => setTimeout(resolve,100));
          checked = checkedLoginResult(await run(`window.m5Login.request({action:'status',provider:${JSON.stringify(provider)}}).then(s=>({state:s.state,error:s.error,attemptId:s.attemptId}))`), attemptId);
        }
        if (!checked) throw new Error('Login check completion timeout');
        connected[provider] = checked;
      }
      console.log(JSON.stringify({stage:'LOGIN_CHECK',connected}));
      if (!Object.values(connected).some(s=>s.state==='connected')) return finish('Neither app-owned login is connected');
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='프로젝트 선택'&&!b.disabled).click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent.includes('hello.txt'))`);
      await wait(`document.querySelector('.m5-new-chat-button:not(:disabled)')!==null`);
      await run(`document.querySelector('.m5-new-chat-button').click()`);
      await wait(`document.querySelector('.m5-new-chat-dialog')?.matches(':modal')`);
      await run(`(()=>{const e=document.querySelector('.m5-new-chat-form > input');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'Reply exactly HELLO_AGENT_SMOKE_OK. Do not use tools, read or write files, or execute commands. This is a connectivity test.');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await wait(`!document.querySelector('.m5-new-chat-form button[type=submit]').disabled`);
      await run(`document.querySelector('.m5-new-chat-form button[type=submit]').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent.startsWith('Reply exactly')`);
      const projectId = await run(`(async()=>{let cursor=null;do{const p=await window.m5Projects.request({action:'list',cursor});const found=p.projects.find(p=>p.canonicalPath===${JSON.stringify(fs.realpathSync(source))});if(found)return found.projectId;cursor=p.nextCursor;}while(cursor);throw new Error('Generated project missing');})()`);
      let chatId = await run(`window.m5.listChats({projectId:${JSON.stringify(projectId)},cursor:null,limit:50}).then(p=>{if(p.items.length!==1)throw new Error('Unexpected generated chat count');return p.items[0].chatId;})`);
      if (catalogOnly) {
        const catalog = await run(`window.m5Models.request({runtime:'codex',refresh:true,chatId:${JSON.stringify(chatId)},roleId:'planning'})`);
        outcomes.push({provider:'codex',state:'catalogObserved',models:catalog.models});
        return finish();
      }
      const testModel = process.env.HELLO_AGENT_LIVE_CODEX_MODEL;
      if (testModel) {
        // Explicit test input, never a product default/fallback. Only this
        // runner's newly generated project gets new role settings and chat.
        const catalog = await run(`window.m5Models.request({runtime:'codex',refresh:true,chatId:${JSON.stringify(chatId)},roleId:'planning'})`);
        const model = catalog.models.find(item => item.modelSelector === testModel);
        if (!model || !model.reasoningEfforts.includes('medium')) throw new Error('Requested test model is not observed with medium effort');
        chatId = await run(`(async()=>{
          const projectId=${JSON.stringify(projectId)};
          const settings=await window.m5.getProjectRoleSettings({projectId});
          const roles=settings.roles.map(role=>role.roleId==='planning'?{...role,modelSelector:${JSON.stringify(testModel)},reasoningEffort:'medium'}:role);
          const updated=await window.m5.updateProjectRoleSettings({projectId,expectedRevision:settings.revision,roles});
          const prepared=await window.m5.prepareChatWorkspace({projectId});
          const manifest=prepared.manifest;
          const chat=await window.m5.createChat({preparationId:prepared.preparationId,projectId,worktreeId:manifest.worktreeId,workspaceIdentity:manifest.workspaceIdentity,goal:'Reply exactly HELLO_AGENT_SMOKE_OK. Do not use tools, read or write files, or execute commands. This is a connectivity test.',expectedSettingsRevision:updated.revision});
          return chat.chatId;
        })()`);
        console.log(JSON.stringify({stage:'TEST_MODEL_SELECTED',provider:'codex',model:testModel,reasoningEffort:'medium',generatedProjectOnly:true}));
      }
      for(const [provider,roleId,runtime] of [['codex','planning','codex'],['claude','coding','claude-code']]) {
        if (!providers.includes(provider)) continue;
        if(connected[provider].state!=='connected'){outcomes.push({provider,state:'loginBlocked'});continue;}
        let observedRunId;
        try {
          const capability = await run(`window.m5.getAgentCapabilities({runtime:${JSON.stringify(runtime)},refresh:true,chatId:${JSON.stringify(chatId)},roleId:${JSON.stringify(roleId)}})`);
          if(!capability.canStart){outcomes.push({provider,state:'admissionBlocked',reason:capability.blockedReason});continue;}
          await run(`(${startAgentPaintProbe.toString()})(${JSON.stringify(chatId)})`);
          const accepted = await run(`(async()=>{const chat=await window.m5.getChat({chatId:${JSON.stringify(chatId)},cursor:null,limit:50,afterEventSequence:0});return window.m5.startAgentRun({chatId:${JSON.stringify(chatId)},roleId:${JSON.stringify(roleId)},inputMessageIds:chat.messages.map(m=>m.messageId),expectedCapabilityFingerprint:${JSON.stringify(capability.fingerprint)}});})()`);
          observedRunId = accepted.runId;
          console.log(JSON.stringify({stage:'RUN_ACCEPTED',provider,runId:accepted.runId,chatId}));
          let terminal;
          for(let i=0;i<900;i++){
            const detail=await run(`window.m5.getChat({chatId:${JSON.stringify(chatId)},cursor:null,limit:50,afterEventSequence:0})`);
            const record=detail.agentRuns?.find(r=>r.runId===accepted.runId);
            if(record && ['succeeded','failed','canceled','recoveryRequired'].includes(record.state)){terminal={provider,runId:accepted.runId,chatId,state:record.state,stage:record.stage,terminationOutcome:record.terminationOutcome,responseObserved:detail.messages.some(m=>m.runId===accepted.runId && m.kind==='role' && m.text.includes('HELLO_AGENT_SMOKE_OK'))};break;}
            await new Promise(r=>setTimeout(r,100));
          }
          outcomes.push(terminal??{provider,runId:accepted.runId,chatId,state:'timeout'});
          if(!terminal) break;
        } catch(error) {
          if (finished) return;
          const message = String(error.message).slice(0,500);
          outcomes.push({provider,state:'requestFailed',error:message});
          if (message.includes('sidecar exited')) {
            // Classify only known internal messages. Never export raw stderr:
            // system status's technical detail is not credential-redacted.
            const diagnostic = await run(`window.m5.getSystemStatus().then(s=>({state:s.state,knownCause:[
              'provider discovery cleanup is unconfirmed',
              'discovery cleanup owner differs',
              'native terminal requires recovery',
              'stdin closed before app.prepareQuit; clean shutdown was not marked'
            ].find(c=>s.error?.detail?.includes(c))??'unclassified'}))`).catch(()=>({state:'unavailable',knownCause:'status unavailable'}));
            console.log(JSON.stringify({stage:'SERVICE_FAILURE',...diagnostic}));
          }
        } finally {
          // Observation failure never authorizes another run or changes its outcome.
          const paint = await window.webContents.executeJavaScript(`globalThis.__m5AgentPaintProbe?.stop(${JSON.stringify(observedRunId ?? null)}) ?? null`).catch(() => ({status:'INCOMPLETE'}));
          const outcome = outcomes.findLast(item => item.provider === provider);
          if (outcome && paint) outcome.paint = paint;
        }
      }
      finish();
    } catch(error) { finish(String(error.message).slice(0,500)); }
  });
});
require(path.resolve(__dirname,'../.vite/build/main.cjs'));
