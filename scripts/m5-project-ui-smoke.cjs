// macOS native UI smoke: synthetic project and isolated app data, no providers selected.
// Requires current .vite/build output and native sidecar; this script does not build them.
const { app, dialog } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
if (process.platform !== 'darwin') throw new Error('This native UI smoke currently supports macOS only');
const root = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'hello-agent-project-ui-'));
const repoRoot = path.resolve(__dirname, '..');
const source = path.join(root, 'project');
fs.mkdirSync(source);
fs.writeFileSync(path.join(source, 'hello.txt'), 'original\n');
for (const args of [['init','-q','--template='],['add','.'],['-c','core.hooksPath=/dev/null','-c','user.name=Smoke','-c','user.email=smoke@example.invalid','commit','-qm','fixture']]) execFileSync('/usr/bin/git', args, {cwd:source, env:{...process.env,GIT_CONFIG_NOSYSTEM:'1',GIT_CONFIG_GLOBAL:'/dev/null',GIT_CONFIG_COUNT:'0'}});
app.setPath('userData', path.join(root,'userdata'));
dialog.showOpenDialog = async () => ({canceled:false,filePaths:[source]});
let finished = false;
const timer = setTimeout(() => finish(new Error('flow timeout')), 90000);
function finish(error, details) {
  if (finished) return;
  finished = true; clearTimeout(timer);
  // Preserve normal before-quit/sidecar cleanup; set Electron's exit status only
  // after the app has accepted quitting. process.exitCode alone is ignored here.
  app.once('will-quit', () => app.exit(error ? 1 : 0));
  console.log(JSON.stringify({status:error?'FAIL':'PASS',root,error:error?.message,...details}));
  app.quit();
}
app.on('browser-window-created', (_event, window) => {
  window.webContents.setBackgroundThrottling(false);
  window.webContents.once('did-finish-load', async () => {
    const run = code => window.webContents.executeJavaScript(code);
    const clickText = (label, scope = 'body') => run(`Array.from(document.querySelectorAll(${JSON.stringify(scope + ' button')})).find(b=>b.textContent===${JSON.stringify(label)}&&!b.disabled)?.click()`);
    const fill = (selector, value) => run(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e||e.disabled)throw new Error('input unavailable');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,${JSON.stringify(value)});e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
    const showPanel = title => run(`(()=>{ const tab=Array.from(document.querySelectorAll('.m5-dock-group-tabs button')).find(b=>b.textContent===${JSON.stringify(title)}&&b.getClientRects().length); if(tab)tab.click(); })()`);
    const wait = async predicate => {
      for(let i=0;i<150;i++) { if(await run(predicate)) return; await new Promise(r=>setTimeout(r,100)); }
      throw new Error('UI condition timeout: '+predicate+'\n'+await run('document.body.innerText'));
    };
    try {
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='프로젝트 선택'&&!b.disabled)`);
      if (!await run(`Boolean(document.querySelector('.m5-guided-start'))&&!document.querySelector('.m5-dock-tree')`)) throw new Error('initial guided start missing or empty dock exposed');
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='프로젝트 선택').click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent.includes('hello.txt'))`);
      await run(`(()=>{ const d=document.querySelector('.m5-provider-selection'); if(d?.open)d.querySelector('summary').click(); })()`);
      await wait(`document.querySelector('.m5-provider-selection')?.open===false`);
      if(await run(`document.querySelector('.m5-layout-settings')?.open!==false`)) throw new Error('layout settings not initially collapsed');
      await wait(`document.querySelectorAll('.m5-team-preview > div').length===3`);
      await run(`document.querySelector('.m5-goal-examples button').click()`);
      await wait(`document.querySelector('.m5-new-chat-form > input').value.length>0&&Array.from(document.querySelectorAll('.m5-goal-examples button')).every(b=>b.disabled)`);
      if (await run(`document.querySelectorAll('.m5-chat-chip').length!==0`)) throw new Error('goal example created a chat');
      await fill('.m5-new-chat-form > input', '');
      await clickText('역할과 모델');
      await wait(`document.querySelector('.m5-settings-dialog')?.matches(':modal')`);
      const originalModel = await run(`document.querySelector('.m5-role-card input').value`);
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
      await run(`(()=>{const e=document.querySelector('.m5-new-chat-form input:not([type=checkbox])');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'Improve hello');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await run(`document.querySelector('.m5-new-chat-form input[type=checkbox]').click()`);
      await wait(`!document.querySelector('.m5-new-chat-form button[type=submit]').disabled`);
      await run(`document.querySelector('.m5-new-chat-form button[type=submit]').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent==='Improve hello'`);
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
      await window.webContents.insertText('edited in snapshot\n');
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='저장'&&!b.disabled)`);
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='저장').click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='저장'&&b.disabled)`);
      await wait(`document.querySelector('.m5-changes')?.textContent.includes('hello.txt') || document.body.innerText.includes('변경 (1)')`);
      await showPanel('변경');
      await run(`document.querySelector('.m5-change-row').click()`);
      await wait(`document.querySelector('.m5-diff-details')?.textContent.includes('edited in snapshot')`);
      await new Promise(r=>setTimeout(r,700));
      if(await run(`document.body.innerText.includes('열어 둔 파일이 밖에서 바뀌었어요')`)) throw new Error('self-save reported as external change');
      if(fs.readFileSync(path.join(source,'hello.txt'),'utf8')!=='original\n') throw new Error('source changed');
      const files = execFileSync('/usr/bin/find',[path.join(root,'userdata'),'-name','hello.txt'],{encoding:'utf8'}).trim().split('\n').filter(Boolean);
      if(!files.some(file=>fs.readFileSync(file,'utf8')==='edited in snapshot\noriginal\n')) throw new Error('saved snapshot content missing');
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='이 채팅의 반영 전 변경 목록 보기').click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='이 변경 묶음으로 확인 준비'&&!b.disabled)`);
      await run(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='이 변경 묶음으로 확인 준비').click()`);
      await wait(`Array.from(document.querySelectorAll('button')).some(b=>b.textContent==='확인 취소'&&!b.disabled)`);
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
          await wait(`document.querySelector('.m5-project-catalog > button')&&!document.querySelector('.m5-project-catalog > button').disabled`);
          await run(`document.querySelector('.m5-project-catalog > button').click()`);
          await wait(`document.querySelector('.m5-project-catalog-list li > button')!==null`);
          await run(`document.querySelector('.m5-project-catalog-list li > button').click()`);
          await wait(`Array.from(document.querySelectorAll('.m5-chat-chip')).some(b=>b.textContent==='Improve hello')`);
          await run(`Array.from(document.querySelectorAll('.m5-chat-chip')).find(b=>b.textContent==='Improve hello').click()`);
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
      await run(`(()=>{const e=document.querySelector('.m5-new-chat-form input:not([type=checkbox])');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,'Second task');e.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await wait(`!document.querySelector('.m5-new-chat-form button[type=submit]').disabled`);
      await run(`document.querySelector('.m5-new-chat-form button[type=submit]').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent==='Second task'`);
      await showPanel('파일');
      await run(`Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('hello.txt')).click()`);
      await wait(`document.querySelector('.monaco-editor textarea')!==null`);
      await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'})`);
      await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('original')`);
      if(await run(`document.querySelector('.monaco-editor .view-lines').textContent.includes('edited')`)) throw new Error('chat snapshots mixed');
      await run(`Array.from(document.querySelectorAll('.m5-chat-chip')).find(b=>b.textContent==='Improve hello').click()`);
      await wait(`document.querySelector('.m5-timeline h2')?.textContent==='Improve hello'`);
      await showPanel('파일');
      await run(`Array.from(document.querySelectorAll('.m5-files button')).find(b=>b.textContent.includes('hello.txt')).click()`);
      await wait(`document.querySelector('.monaco-editor textarea')!==null`);
      await run(`document.querySelector('.monaco-editor textarea').scrollIntoView({block:'center'})`);
      await wait(`document.querySelector('.monaco-editor .view-lines')?.textContent.includes('edited')`);
      console.log(JSON.stringify({stage:'PROJECT_FLOW_COMPLETE',root}));
      const screenshot = path.join(root,'project.png');
      fs.writeFileSync(screenshot,(await window.webContents.capturePage()).toPNG());
      finish(null,{screenshot,text:await run('document.body.innerText'),inputs:await run(`Array.from(document.querySelectorAll('input,textarea')).map(e=>({tag:e.tagName,placeholder:e.placeholder,disabled:e.disabled}))`)});
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
