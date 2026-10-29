// Isolated renderer flow, synthetic transport only. No sidecar or model calls.
import { build } from 'vite';
import react from '@vitejs/plugin-react';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const repo = path.resolve(import.meta.dirname, '..');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'm5-discard-dialog-'));
const evidence = path.join(repo, '.artifacts/m5-discard-dialog', path.basename(root));
fs.mkdirSync(evidence, {recursive:true});
const fixture = path.join(evidence, 'fixture'); fs.mkdirSync(fixture);
const cases = JSON.parse(fs.readFileSync(path.join(repo,'tests/fixtures/m5/g5-v1/cases.json')));
const chat = {...cases.valid.find(entry=>entry.validator==='chatRecord').value, state:'reviewReady'};
fs.writeFileSync(path.join(fixture,'index.html'), '<div id="root"></div><script type="module" src="/entry.tsx"></script>');
fs.writeFileSync(path.join(fixture,'entry.tsx'), `
import React from 'react'; import {createRoot} from 'react-dom/client';
import {I18nProvider} from '${repo}/src/i18n.tsx';
import {ChatWorkspaceDiscard} from '${repo}/src/m5/chat-workspace-discard.tsx';
import '${repo}/src/m5/styles.css';
localStorage.setItem('hello-agent.lang','ko');
const chat=${JSON.stringify(chat)};
const inspection={chatId:chat.chatId,taskId:chat.taskId,projectId:chat.projectId,worktreeId:chat.worktreeId,workspaceIdentity:chat.workspaceIdentity,chatRevision:7};
const metrics=window.discardMetrics={requests:0,messages:0,releases:0,completed:0};
let stored=false, loseResponse=true, failRelease=true, key=0;
window.m5Projects={request:async request=>{
 if(request.action==='inspectChatDiscard') return {schemaVersion:1,action:request.action,inspection};
 if(request.action==='discardChatWorkspace') {
  metrics.requests++; if(!stored){stored=true;metrics.messages++;}
  if(loseResponse){loseResponse=false;throw Error('lost response');}
  return {schemaVersion:1,action:request.action,chat:{...chat,state:'discarded'}};
 }
 if(request.action==='releaseView') {metrics.releases++;if(failRelease){failRelease=false;throw Error('release failure');}return {schemaVersion:1,action:request.action,projectId:chat.projectId};}
 throw Error('unexpected action');
}};
const root=createRoot(document.getElementById('root'));
window.remountDiscard=()=>root.render(<I18nProvider><ChatWorkspaceDiscard key={++key} chat={chat} disabled={false} onCompleted={async()=>{
 await window.m5Projects.request({action:'releaseView',projectId:chat.projectId}); metrics.completed++;root.render(<p id="completed">Completed</p>);
}} /></I18nProvider>);
window.remountDiscard();
`);
await build({configFile:false,root:fixture,base:'./',plugins:[react()],logLevel:'error',build:{outDir:path.join(root,'dist'),emptyOutDir:true}});
const runner = path.join(root,'runner.cjs');
fs.writeFileSync(runner, `
const {app,BrowserWindow}=require('electron'); const fs=require('node:fs');
app.setPath('userData',${JSON.stringify(path.join(root,'profile'))});
let failed=false; const result={diagnosticOnly:true,syntheticTransport:true,normalQuit:false};
app.on('will-quit',()=>{result.normalQuit=true;fs.writeFileSync(${JSON.stringify(path.join(evidence,'result.json'))},JSON.stringify(result,null,2));app.exit(failed?1:0);});
app.whenReady().then(async()=>{const w=new BrowserWindow({width:900,height:720,webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false}});
const run=s=>w.webContents.executeJavaScript(s);
const wait=async s=>{let n=0;while(!await run(s)){if(n++>250)throw Error('timeout: '+s);await new Promise(r=>setTimeout(r,20));}};
const open=async()=>{await run("document.querySelector('.m5-chat-discard > button').click()");await wait("document.querySelector('dialog[open] input') !== null");};
const confirm=async()=>{await run("document.querySelector('dialog input').click()");await wait("!document.querySelector('.m5-discard-actions button:last-child').disabled");await run("document.querySelector('.m5-discard-actions button:last-child').click()");};
try {
 await w.loadFile(${JSON.stringify(path.join(root,'dist/index.html'))});await wait("Boolean(document.querySelector('.m5-chat-discard > button'))");
 await open();if(!await run("document.querySelector('.m5-discard-actions button:last-child').disabled"))throw Error('consent bypass');
 fs.writeFileSync(${JSON.stringify(path.join(evidence,'confirmation.png'))},(await w.webContents.capturePage()).toPNG());
 await run("document.querySelector('.m5-discard-actions button').click()");await wait("!document.querySelector('dialog').open");
 if(await run('discardMetrics.requests'))throw Error('cancel sent mutation');
 await open();await confirm();await wait("Boolean(document.querySelector('dialog [role=alert]'))");
 await run("document.querySelector('.m5-discard-actions button').click()");await run('remountDiscard()');await wait("Boolean(document.querySelector('.m5-chat-discard > button'))");
 await open();await confirm();await wait("Boolean(document.querySelector('dialog [role=alert]')) && discardMetrics.releases===1");
 await run("document.querySelector('.m5-discard-actions button:last-child').click()");await wait("Boolean(document.querySelector('#completed'))");
 result.metrics=await run('discardMetrics');
 if(JSON.stringify(result.metrics)!==JSON.stringify({requests:2,messages:1,releases:2,completed:1}))throw Error('duplicate mutation or incomplete release');
 result.status='PASS';
} catch(error){failed=true;result.status='FAIL';result.error=String(error);}finally{app.quit();}
});
`);
const child=spawn(require('electron'),[runner],{stdio:'inherit',env:{...process.env,ELECTRON_RUN_AS_NODE:''}});
const owned = new Set([child.pid]);
const collect = setInterval(()=>{
  const rows=execFileSync('/bin/ps',['-axo','pid=,ppid='],{encoding:'utf8'}).trim().split('\n').map(row=>row.trim().split(/\s+/).map(Number));
  let changed=true; while(changed){changed=false;for(const [pid,parent] of rows){if(owned.has(parent)&&!owned.has(pid)){owned.add(pid);changed=true;}}}
},100);
const code=await new Promise(resolve=>child.once('exit',resolve));
clearInterval(collect);
await new Promise(resolve=>setTimeout(resolve,500));
const live=[...owned].filter(pid=>{try{process.kill(pid,0);return true;}catch(error){return error.code!=='ESRCH';}});
const result=JSON.parse(fs.readFileSync(path.join(evidence,'result.json')));
const cleaned=result.normalQuit&&live.length===0;
if(cleaned){fs.rmSync(root,{recursive:true});fs.rmSync(fixture,{recursive:true});}
fs.writeFileSync(path.join(evidence,'cleanup.json'),JSON.stringify({ownedPids:[...owned],livePids:live,cleaned},null,2));
console.log(JSON.stringify({evidence,root,code}));
process.exitCode=code??1;
