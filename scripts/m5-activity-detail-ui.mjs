// Isolated renderer capture of members' live work history, synthetic transport only.
// No sidecar, account or model calls: the rows come from a replay file
// (recorded provider frames run through the engine's row builders,
// `cargo test -p elsecrew-sidecar replay_recorded_frames -- --ignored`).
//
//   node scripts/m5-activity-detail-ui.mjs <replay.json> <out-dir> [--before]
//
// `--before` draws the rows the previous engine would have stored for the
// same tool uses (command description or first line only, no closing rows,
// no file names for Codex changes); set ELSECREW_ACTIVITY_REPO to a checkout
// of the previous screen code to render them with it.
import { build } from 'vite';
import react from '@vitejs/plugin-react';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const [replayPath, outDir] = process.argv.slice(2);
const before = process.argv.includes('--before');
if (!replayPath || !outDir) throw new Error('usage: m5-activity-detail-ui.mjs <replay.json> <out-dir> [--before]');
const repo = path.resolve(process.env.ELSECREW_ACTIVITY_REPO ?? path.resolve(import.meta.dirname, '..'));
const replay = JSON.parse(fs.readFileSync(replayPath, 'utf8'));

/** What the previous engine stored for the same tool uses. */
function previousRows(rows, provider) {
  const out = [];
  const files = new Set();
  for (const row of rows) {
    if (provider === 'claude') {
      if (row.phase === 'done') continue;
      if (row.action === 'command') out.push({ action: 'command', ...(row.note ? { target: row.note } : {}), ...(row.outcome ? { outcome: row.outcome } : {}) });
      else out.push(Object.fromEntries(Object.entries(row).filter(([key]) => ['action', 'target', 'tool', 'outcome'].includes(key))));
      continue;
    }
    const item = String(row.id ?? '').split(':')[0];
    const change = ['edit', 'write', 'delete'].includes(row.action) && String(row.id ?? '').includes(':');
    if (change) { if (row.phase !== 'done' && !files.has(item)) { files.add(item); out.push({ action: 'edit' }); } continue; }
    if (row.phase === 'done') { if (row.outcome) out.push({ action: 'command', outcome: row.outcome }); continue; }
    out.push({ action: 'command', ...(row.command ? { target: row.command.split('\n')[0] } : {}) });
  }
  return out;
}

// Under the screen code's checkout so its packages resolve (as m5-run-failure-ui).
const root = fs.mkdtempSync(path.join(fs.mkdirSync(path.join(repo, '.artifacts/m5-activity-detail'), { recursive: true }) ?? path.join(repo, '.artifacts/m5-activity-detail'), 'run-'));
const fixture = path.join(root, 'fixture');
fs.mkdirSync(fixture, { recursive: true });
fs.mkdirSync(outDir, { recursive: true });
const runs = [
  { provider: 'codex', roleId: 'planning', runtime: 'codex', rows: replay.codex },
  { provider: 'claude', roleId: 'coding', runtime: 'claude-code', rows: replay.claude },
].map(run => ({ ...run, rows: before ? previousRows(run.rows, run.provider) : run.rows }));
fs.writeFileSync(path.join(fixture, 'runs.json'), JSON.stringify(runs));
fs.writeFileSync(path.join(fixture, 'index.html'), '<!doctype html><meta charset="utf-8"><body class="m5-app"><div class="m5-conversation-scroll"><div id="root" style="max-width:720px;margin:16px auto;display:grid;gap:16px"></div></div><script type="module" src="/entry.tsx"></script>');
fs.writeFileSync(path.join(fixture, 'entry.tsx'), `
import React from 'react'; import {createRoot} from 'react-dom/client';
import {I18nProvider} from '${repo}/src/i18n.tsx';
import {MemberCard} from '${repo}/src/m5/member-card.tsx';
import {M5_BRIDGE_V8_METHODS} from '${repo}/src/m5/contracts/bridge-v8.ts';
import '${repo}/src/m5/styles.css';
import runs from './runs.json';
localStorage.setItem('elsecrew.lang','ko');
const fingerprint='sha256:'+'a'.repeat(64);
const now=new Date(Date.now()-95000).toISOString().replace(/\\.\\d+Z$/,'Z');
const records=runs.map(spec=>({spec,run:{schemaVersion:1,runId:'agent_run_'+spec.provider,chatId:'chat_01',taskId:'task_01',projectId:'project_01',worktreeId:'worktree_01',workspaceIdentity:'workspace_01',source:'actualAgent',state:'running',stage:'implementing',createdAt:now,updatedAt:now,selectedProfile:{roleId:spec.roleId,runtime:spec.runtime,modelSelector:'fixture',permission:'workspaceWrite',reasoningEffort:null},capabilityFingerprint:fingerprint}}));
const bindingOf=run=>({chatId:run.chatId,taskId:run.taskId,projectId:run.projectId,worktreeId:run.worktreeId,workspaceIdentity:run.workspaceIdentity,runId:run.runId,roleId:run.selectedProfile.roleId,capabilityFingerprint:run.capabilityFingerprint});
const outputOf=runId=>{const {spec,run}=records.find(value=>value.run.runId===runId);const b=bindingOf(run);
  return spec.rows.map((row,index)=>({outputSequence:index+1,event:{schemaVersion:1,eventId:'agent_event_'+spec.provider+'_'+(index+1),...b,kind:'status',text:JSON.stringify(row),createdAt:now},itemUpdate:{itemId:'activity_'+spec.provider+'_'+(index+1),operation:'complete',channel:'activity'}}));};
const bridge={version:8};
for(const method of M5_BRIDGE_V8_METHODS) bridge[method]=method.startsWith('on')?()=>()=>{}:async()=>null;
bridge.getAgentOutputHead=async binding=>({formatVersion:1,binding,committedSequence:outputOf(binding.runId).length,sealed:true});
bridge.getAgentOutputPage=async request=>{const rows=outputOf(request.binding.runId).filter(row=>row.outputSequence>request.afterSequence&&row.outputSequence<=request.throughSequence).slice(0,request.limit);return {request,rows,nextAfterSequence:rows.length?rows[rows.length-1].outputSequence:request.afterSequence};};
window.m5=bridge;
window.opened=[];
const clock=new Intl.DateTimeFormat('ko-KR',{hour:'2-digit',minute:'2-digit'});
createRoot(document.getElementById('root')).render(<I18nProvider>{records.map(({run})=><MemberCard key={run.runId} run={run} reports={[]} teamRoleIds={null} phase={null} clock={clock} onOpenFile={path=>window.opened.push(path)} changedFiles={new Set(["NOTES.md","app.js"])} />)}</I18nProvider>);
`);
await build({ configFile: false, root: fixture, base: './', plugins: [react()], logLevel: 'error', build: { outDir: path.join(root, 'dist'), emptyOutDir: true } });
const runner = path.join(root, 'runner.cjs');
const label = before ? 'before' : 'after';
fs.writeFileSync(runner, `
const {app,BrowserWindow}=require('electron'); const fs=require('node:fs'); const path=require('node:path');
app.setPath('userData',${JSON.stringify(path.join(root, 'profile'))});
let failed=false; const result={diagnosticOnly:true,syntheticTransport:true,mode:${JSON.stringify(label)}};
app.whenReady().then(async()=>{const w=new BrowserWindow({width:820,height:2100,show:false,enableLargerThanScreen:true,paintWhenInitiallyHidden:true,webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false,backgroundThrottling:false}});
const run=s=>w.webContents.executeJavaScript(s);
const consoleLines=[];w.webContents.on('console-message',(event)=>consoleLines.push(String(event.message??event).slice(0,300)));
const wait=async s=>{let n=0;while(!await run(s)){if(n++>250)throw Error('timeout: '+s);await new Promise(r=>setTimeout(r,20));}};
// One picture per card: the window cannot be taller than the screen.
const shot=async name=>{const count=await run("document.querySelectorAll('.m5-member-card').length");for(let index=0;index<count;index++){
 const rect=await run("(()=>{const card=document.querySelectorAll('.m5-member-card')["+index+"];card.scrollIntoView({block:'start'});const r=card.getBoundingClientRect();return {x:Math.floor(r.x)-8,y:Math.max(0,Math.floor(r.y)-8),width:Math.ceil(r.width)+16,height:Math.min(Math.ceil(r.height)+16,window.innerHeight)};})()");
 await new Promise(r=>setTimeout(r,150));
 fs.writeFileSync(path.join(${JSON.stringify(outDir)},name.replace('.png','-'+(index+1)+'.png')),(await w.webContents.capturePage(rect)).toPNG());}};
try {
 await w.loadFile(${JSON.stringify(path.join(root, 'dist/index.html'))});
 await wait("document.querySelectorAll('.m5-member-card .m5-activity-row').length>4");
 await new Promise(r=>setTimeout(r,300));
 result.lines=await run("[...document.querySelectorAll('.m5-member-card')].map(card=>({role:card.dataset.roleId,lines:[...card.querySelectorAll('.m5-activity-row')].map(row=>(row.querySelector('summary')??row).innerText.replace(/\\\\s+/g,' ').trim())}))");
 await shot(${JSON.stringify(label + '-cards.png')});
 if(${!before}){
  // Open a finished command, a failed one, and press a changed file's line.
  await run("for(const row of document.querySelectorAll('details.m5-activity-folded')){const text=row.innerText;if(text.includes('npm test')||text.includes('missing-folder'))row.open=true;}");
  await new Promise(r=>setTimeout(r,600));
  result.detail=await run("[...document.querySelectorAll('details.m5-activity-folded[open] .m5-activity-detail')].map(node=>node.innerText)");
  await shot('after-detail.png');
  await run("document.querySelector('button.m5-activity-file')?.click()");
  result.opened=await run('window.opened');
  const html=await run("document.getElementById('root').innerHTML");
  result.secretShown=/ghp_[A-Za-z0-9]{8}/.test(html);
 }
 result.status='PASS';
} catch(error){failed=true;result.status='FAIL';result.error=String(error);result.console=consoleLines.slice(-10);try{result.text=(await run("document.body.innerText")).slice(0,1500);await shot(${JSON.stringify(label + '-failure.png')});}catch{}}finally{fs.writeFileSync(path.join(${JSON.stringify(outDir)},${JSON.stringify(label + '-result.json')}),JSON.stringify(result,null,2));app.exit(failed?1:0);}
});
`);
const child = spawn(require('electron'), [runner], { stdio: 'inherit', env: { ...process.env, ELECTRON_RUN_AS_NODE: '' } });
child.on('exit', code => { fs.rmSync(root, { recursive: true, force: true }); process.exit(code ?? 1); });
