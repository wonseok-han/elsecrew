// Build once, run one warmup plus ten trials. All results remain QA diagnostics.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';
import { summarizeOutputTrial } from './m5-output-trial-summary.mjs';

assert.equal(process.platform,'darwin');
const combined = process.argv.includes('--combined-workload');
assert.deepEqual(process.argv.slice(2), combined ? ['--combined-workload'] : []);
const repo=fileURLToPath(new URL('..',import.meta.url));
const git=(...args)=>execFileSync('/usr/bin/git',args,{cwd:repo,encoding:'utf8'}).trim();
assert.equal(git('status','--porcelain'),'','Commit the measurement source first');
const output=fs.mkdtempSync(path.join(os.tmpdir(),'hello-agent-output-series-'));
const report={diagnostic:true,gateEligible:false,status:'RUNNING',head:git('rev-parse','HEAD'),
  workload:combined ? 'large-file-diff-four-streams' : 'four-streams',
  warmupRuns:1,measuredRuns:10,trials:[],limitations:[
    'Explicit QA artifact, not production signing/fuses or actual providers.',
    'Exact-head live samples do not measure every output row paint.',
    'Pre-write fixture stamps do not prove commit or delivery deadlines.',
    combined ? 'Combined workload includes sampled retained memory, but is not all G6 evidence.' : 'Sampled process-tree RSS does not establish retained memory or all G6 workloads.']};
const persist=()=>fs.writeFileSync(path.join(output,'series.json'),JSON.stringify(report,null,2));
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const sourceUnchanged=()=>{assert.equal(git('rev-parse','HEAD'),report.head);assert.equal(git('status','--porcelain'),'');};
function ownedRoot(root,prefix) {
  assert.equal(typeof root,'string'); assert.ok(path.basename(root).startsWith(prefix));
  assert.equal(fs.realpathSync(path.dirname(root)),fs.realpathSync(os.tmpdir()));
  const stat=fs.lstatSync(root);assert.ok(stat.isDirectory()&&!stat.isSymbolicLink());
}
async function run(command,args,logPath,onMessage) {
  const env={...process.env};delete env.ELECTRON_RUN_AS_NODE;delete env.NODE_OPTIONS;
  const log=fs.createWriteStream(logPath);let pending='',parseError;
  const child=spawn(command,args,{cwd:repo,env,stdio:['ignore','pipe','pipe']});
  child.stdout.setEncoding('utf8');
  child.stdout.on('data',chunk=>{
    log.write(chunk);pending+=chunk;
    // Only short STARTED/build messages are needed. Preserve large reports in
    // the log rather than retaining a second copy in the line parser.
    let end;
    while((end=pending.indexOf('\n'))!==-1){const line=pending.slice(0,end);pending=pending.slice(end+1);
      if(line.length<16384)try{const value=JSON.parse(line);onMessage(value);}catch(error){
        if(!(error instanceof SyntaxError))parseError=error;
      }
    }
    if(pending.length>16*1024*1024){parseError=new Error('Oversized diagnostic output');pending='';}
  });
  child.stderr.on('data',chunk=>log.write(chunk));
  const termination=await new Promise(resolve=>{
    child.once('error',error=>resolve({error:String(error)}));
    child.once('close',(code,signal)=>resolve({code,signal}));
  });
  await new Promise(resolve=>log.end(resolve));
  if(parseError)throw parseError;
  assert.ok(!termination.error && termination.signal===null,'Collector process did not exit normally');
  return {pid:child.pid,...termination};
}
let qaRoot,appDirectory,qaCleanupSafe=false;
persist();console.log(JSON.stringify({stage:'SERIES_STARTED',output}));
try {
  const built=await run(process.execPath,['scripts/m5-package-output-qa.mjs'],path.join(output,'build.log'),message=>{
    if(message.stage==='QA_BUILD_STARTED'){assert.equal(qaRoot,undefined);qaRoot=message.root;}
  });
  assert.equal(built.code,0);ownedRoot(qaRoot,'hello-agent-output-qa-');
  const build=JSON.parse(fs.readFileSync(path.join(qaRoot,'build.json'),'utf8'));
  assert.equal(build.status,'BUILT');assert.equal(build.head,report.head);assert.equal(build.gitStatus,'');
  appDirectory=build.appDirectory;
  assert.ok(fs.realpathSync(appDirectory).startsWith(fs.realpathSync(qaRoot)+path.sep));
  const contents=path.join(appDirectory,'Hello Agent Output QA.app/Contents');
  const executable=path.join(contents,'MacOS/Hello Agent Output QA');
  const asar=path.join(contents,'Resources/app.asar'),sidecar=path.join(contents,'Resources/sidecar/hello-agent-sidecar');
  const hashes={asar:hash(asar),sidecar:hash(sidecar)};
  assert.equal(hashes.sidecar,build.sidecarSha256);report.build=build;report.hashes=hashes;persist();
  for(let index=0;index<=10;index++) {
    sourceUnchanged();assert.deepEqual({asar:hash(asar),sidecar:hash(sidecar)},hashes);
    const destination=path.join(output,`trial-${index}`);fs.mkdirSync(destination);
    let root;
    const termination=await run(executable,combined ? ['--combined-workload'] : [],path.join(destination,'trial.log'),message=>{
      if(message.stage==='STARTED'){assert.equal(root,undefined);root=message.root;}
    });
    ownedRoot(root,'hello-agent-output-ui-');
    const result=JSON.parse(fs.readFileSync(path.join(root,'result.json'),'utf8'));
    assert.equal(result.root,root);
    const trial={index,warmup:index===0,termination,status:result.status};report.trials.push(trial);persist();
    for(const name of ['result.json','output.png','dispatch-timing.log'])if(fs.existsSync(path.join(root,name)))fs.copyFileSync(path.join(root,name),path.join(destination,name));
    // Cleanup is independent of whether the timing/visibility check passed.
    assert.equal(result.shutdown,'accepted','Uncertain shutdown; fixture retained');
    const observed=new Set([termination.pid,...result.memory.samples.flatMap(row=>row.processes.map(p=>p.pid))]);
    const live=new Set(execFileSync('/bin/ps',['-axo','pid='],{encoding:'utf8'}).trim().split(/\s+/).map(Number));
    assert.ok([...observed].every(pid=>!live.has(pid)),'Observed process live; fixture retained');
    const db=new DatabaseSync(path.join(root,'userdata/com.wonseokhan.helloagent/m5/hello-agent.sqlite3'),{readOnly:true});
    try{const rows=db.prepare('SELECT clean_shutdown_at FROM app_instances').all();assert.ok(rows.length&&rows.every(row=>row.clean_shutdown_at));}finally{db.close();}
    fs.writeFileSync(path.join(destination,'cleanup.json'),JSON.stringify({root,observedPids:[...observed],cleanShutdown:true}));
    fs.rmSync(root,{recursive:true});trial.fixtureCleaned=true;persist();
    assert.equal(termination.code,0, `Trial ${index}: ${result.error ?? result.status}`);
    trial.values=summarizeOutputTrial(result, {combinedWorkload:combined});sourceUnchanged();persist();
    console.log(JSON.stringify({stage:'TRIAL_FINISHED',index,values:trial.values}));
  }
  const trials=report.trials.slice(1);
  report.maxima={rendererLongTaskMs:Math.max(...trials.map(t=>t.values.rendererLongTaskMs)),
    sampledLiveLatencyMs:Math.max(...trials.map(t=>t.values.maximumSampledLiveLatencyMs)),
    peakRssBytes:Math.max(...trials.map(t=>t.values.peakRssBytes))};
  if (combined) for (const key of ['idleRssBytes','retainedRssBytes']) {
    report.maxima[key]=Math.max(...trials.map(t=>t.values.combinedMemory.maxima[key]));
  }
  report.status=trials.every(t=>t.values.allStampedWithin60Seconds)&&report.maxima.sampledLiveLatencyMs<=100
    &&report.maxima.rendererLongTaskMs<=200
    &&report.maxima.peakRssBytes<=800*1024*1024
    &&(!combined || trials.every(t=>t.values.combinedMemory.failedMetrics.length===0))?'NOT_READY':'FAIL';
  // Preserve the build for diagnosis on any failed or incomplete series.
  qaCleanupSafe=report.status==='NOT_READY';
}catch(error){report.status='FAIL';report.error=String(error);}
finally{
  if(qaRoot&&qaCleanupSafe)try{
    ownedRoot(qaRoot,'hello-agent-output-qa-');
    fs.copyFileSync(path.join(qaRoot,'build.json'),path.join(output,'build.json'));
    fs.rmSync(qaRoot,{recursive:true});report.qaPackageCleaned=true;
  }catch(error){report.status='FAIL';report.cleanupError=String(error);}
  persist();console.log(JSON.stringify({output,status:report.status,error:report.error,cleanupError:report.cleanupError}));
}
process.exitCode=report.status==='FAIL'?1:2;
