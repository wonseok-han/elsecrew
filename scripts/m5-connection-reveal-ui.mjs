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
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'm5-connection-reveal-'));
const evidence = path.join(repo, '.artifacts/m5-connection-reveal', path.basename(root));
fs.mkdirSync(evidence, {recursive:true});
const fixture = path.join(evidence, 'fixture'); fs.mkdirSync(fixture);
fs.writeFileSync(path.join(fixture,'index.html'), '<div id="root"></div><script type="module" src="/entry.tsx"></script>');
fs.writeFileSync(path.join(fixture,'entry.tsx'), `
import React,{useState} from 'react'; import {createRoot} from 'react-dom/client';
import {I18nProvider} from '${repo}/src/i18n.tsx';
import {ProviderLogin} from '${repo}/src/m5/provider-login.tsx';
import {chatConnectionTarget,chatNotice} from '${repo}/src/m5/chat-notice.ts';
import '${repo}/src/m5/styles.css';
localStorage.setItem('elsecrew.lang','ko');
localStorage.setItem('elsecrew.m5.providers.v1',JSON.stringify({version:1,providers:['codex']}));
window.requests=[];
window.m5Login={request:async request=>{window.requests.push(request);return {state:'connected',attemptId:null,authUrl:null,error:null};}};
function App(){const [reveal,setReveal]=useState();return <>
{['codex','claude-code'].map(runtime=><button id={runtime} key={runtime} onClick={()=>{const provider=chatConnectionTarget(chatNotice('m5.agentNotice.blocked',{roleId:'coding',runtime,reason:'notAuthenticated'}));setReveal(old=>({provider,sequence:(old?.sequence??0)+1}));}}>Check {runtime}</button>)}
<ProviderLogin disabled={false} compact reveal={reveal}/></>}
createRoot(document.getElementById('root')).render(<I18nProvider><App/></I18nProvider>);
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
 await wait("window.requests?.length === 1 && document.querySelector('#m5-agent-management-content').hidden");
 const initial=await run('JSON.stringify(window.requests)');
 await run("document.getElementById('claude-code').click()");
 await wait("!document.querySelector('#m5-agent-management-content').hidden && document.querySelector('.m5-provider-selection').open && document.activeElement.classList.contains('m5-agent-management')");
 if(!await run("document.querySelector('.m5-provider-selection').innerText.includes('Claude Code')"))throw Error('missing provider selection');
 if(await run('JSON.stringify(window.requests)')!==initial)throw Error('reveal invoked login');
 if(await run("localStorage.getItem('elsecrew.m5.providers.v1')")!==JSON.stringify({version:1,providers:['codex']}))throw Error('selection changed');
 await run("document.querySelector('.m5-management-toggle').click()");
 await wait("document.querySelector('#m5-agent-management-content').hidden");
 await run("document.getElementById('codex').click()");
 await wait("!document.querySelector('#m5-agent-management-content').hidden && document.activeElement.classList.contains('m5-agent-management')");
 if(await run('JSON.stringify(window.requests)')!==initial)throw Error('second reveal invoked login');
 fs.writeFileSync(${JSON.stringify(path.join(evidence,'connection.png'))},(await w.webContents.capturePage()).toPNG());
 result.requests=await run('window.requests');result.status='PASS';
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
