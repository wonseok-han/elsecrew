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
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'm5-run-failure-'));
const evidence = path.join(repo, '.artifacts/m5-run-failure', path.basename(root));
fs.mkdirSync(evidence, {recursive:true});
const fixture = path.join(evidence, 'fixture'); fs.mkdirSync(fixture);
fs.writeFileSync(path.join(fixture,'index.html'), '<div id="root"></div><script type="module" src="/entry.tsx"></script>');
fs.writeFileSync(path.join(fixture,'entry.tsx'), `
import React from 'react'; import {createRoot} from 'react-dom/client';
import {I18nProvider} from '${repo}/src/i18n.tsx';
import {AgentRuns} from '${repo}/src/m5/workbench.tsx';
import '${repo}/src/m5/styles.css';
localStorage.setItem('elsecrew.lang','ko');
const run={schemaVersion:1,runId:'agent_run_fixture',chatId:'chat_fixture',taskId:'task_fixture',projectId:'project_fixture',worktreeId:'worktree_fixture',workspaceIdentity:'workspace_fixture',source:'actualAgent',state:'failed',stage:'implementing',terminationOutcome:'cleanExit',createdAt:'2026-11-12T03:15:00Z',updatedAt:'2026-11-12T03:15:20Z',selectedProfile:{roleId:'coding',runtime:'claude-code',modelSelector:'fixture',permission:'workspaceWrite',reasoningEffort:null},capabilityFingerprint:'sha256:'+'a'.repeat(64),failure:{stage:'approvalDelivery',code:'approvalUnavailable'}};
const root=createRoot(document.getElementById('root'));
window.showLegacy=()=>root.render(<I18nProvider><AgentRuns runs={[{...run,failure:undefined}]}/></I18nProvider>);
root.render(<I18nProvider><AgentRuns runs={[run]}/></I18nProvider>);
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
try {
 await w.loadFile(${JSON.stringify(path.join(root,'dist/index.html'))});
 await wait("Boolean(document.querySelector('.m5-agent-runs [role=alert]'))");
 const text=await run("document.querySelector('.m5-agent-runs').innerText");
 if(!text.includes('도구 사용 허용을 전달하는 중') || !text.includes('approvalDelivery/approvalUnavailable'))throw Error('missing visible diagnostic');
 if(text.includes('모델 설정을 확인'))throw Error('misleading generic model advice');
 fs.writeFileSync(${JSON.stringify(path.join(evidence,'failure.png'))},(await w.webContents.capturePage()).toPNG());
 await run('showLegacy()');
 await wait("!document.querySelector('.m5-agent-runs [role=alert]')");
 result.status='PASS';result.diagnosticVisible=true;result.legacyWithoutInventedCause=true;
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
