import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { summarizeOutputTrial } from '../scripts/m5-output-trial-summary.mjs';
const {summarizeOutputGeneration}=createRequire(import.meta.url)('../scripts/m5-output-generation.cjs');
function result(): any {
  const generation=Array.from({length:4},(_,r)=>({runId:`run${r}`,
    samples:Array.from({length:3000},(_,i)=>[i+1,i*20000,1700000000000+i*20,`event${r}-${i}`,i+1])}));
  return {packagedQa:true,status:'PASS',functionalStatus:'PASS',shutdown:'accepted',visibility:'visible',
    mainLongTasks:{valid:true,windowCompleted:true,completed:true,dataLoss:false,incompleteMain:false,mainThreads:1,limitMs:200,tasks:[{tid:1,startUs:1,durationMs:10}],maximumMs:10,overLimitCount:0},
    rendererLongTasks:{supported:true,valid:true,windowCompleted:true,overflow:false,limitMs:200,startedAt:0,endedAt:1000,entries:[],maximumMs:0,overLimitCount:0},
    visibilityTrace:[{state:'visible'}],restoredChats:[0,1,2,3],generation,generationSummary:summarizeOutputGeneration(generation),
    outputObservation:{droppedHeads:0,droppedFrames:0,liveLatency:{dropped:0,unfinished:0,superseded:2,interrupted:1,samples:[{notificationToFrameMs:150}]}},
    memory:{errors:0,truncated:false,samples:[{totalBytes:100,processes:[{pid:123,rssBytes:100}]}]}};
}
test('keeps observed timing failures and coalesced counts instead of discarding them',()=>{
  const summary=summarizeOutputTrial(result());
  assert.equal(summary.persistedRows,12000);assert.equal(summary.maximumSampledLiveLatencyMs,150);
  assert.equal(summary.superseded,2);assert.equal(summary.peakRssBytes,100);
});
test('rejects incomplete visibility, truncated observations and inconsistent raw evidence',()=>{
  for(const mutate of [
    (r:any)=>{r.visibilityTrace.push({state:'hidden'});},
    (r:any)=>{r.outputObservation.liveLatency.dropped=1;},
    (r:any)=>{r.generation[0].samples.pop();},
    (r:any)=>{r.generationSummary.persistedRows=11999;},
    (r:any)=>{r.memory.samples[0].totalBytes=0;},
    (r:any)=>{r.memory.truncated=true;},
    (r:any)=>{r.restoredChats.pop();},
  ]){const r=result();mutate(r);assert.throws(()=>summarizeOutputTrial(r));}
});

function combinedResult(): any {
  const r=result();
  r.memory.samples=['idle','loaded','retained'].map((phase,i)=>({phase,at:[1000,2000,63000][i],totalBytes:100,processes:[{pid:123,rssBytes:100}]}));
  r.combinedWorkload={status:'OBSERVED',coexist:true,originalUnchanged:true,
    fixture:{treeFiles:10000,treeDirectories:1000,fileBytes:5242880,fileLines:100000},
    largeProject:{failedMetrics:[]},largeDiff:{failedMetrics:[],diffFiles:500,diffChangedLines:20000,diffBytes:10485760},
    liveCoexistence:{editor:true,diff:true,runs:4},closedAt:3000,retainedStartedAt:63000,
    idleStartedAt:900,idleEndedAt:1100,
    memory:{maxima:{idleRssBytes:100,peakRssBytes:100,retainedRssBytes:0},failedMetrics:[]},failedMetrics:[]};
  return r;
}
test('combined series requires workload evidence and recomputes memory from raw samples',()=>{
  const r=combinedResult();
  assert.equal(summarizeOutputTrial(r,{combinedWorkload:true}).combinedMemory.maxima.idleRssBytes,100);
  assert.throws(()=>summarizeOutputTrial(r));
  assert.throws(()=>summarizeOutputTrial(result(),{combinedWorkload:true}));
  for(const mutate of [
    (r:any)=>{r.combinedWorkload.coexist=false;},
    (r:any)=>{r.combinedWorkload.originalUnchanged=false;},
    (r:any)=>{r.combinedWorkload.retainedStartedAt=62000;},
    (r:any)=>{r.memory.samples[0].at=1200;},
    (r:any)=>{r.memory.samples[2].at=62000;},
    (r:any)=>{r.combinedWorkload.memory.maxima.idleRssBytes=1;},
    (r:any)=>{r.combinedWorkload.fixture.fileBytes=10;},
  ]){const r=combinedResult();mutate(r);assert.throws(()=>summarizeOutputTrial(r,{combinedWorkload:true}));}
});
test('combined memory failures cannot disappear behind a successful output-only summary',()=>{
  const r=combinedResult();const high=401*1024*1024;
  r.memory.samples[0].totalBytes=high;r.memory.samples[0].processes[0].rssBytes=high;
  r.combinedWorkload.memory={maxima:{idleRssBytes:high,peakRssBytes:high,retainedRssBytes:0},failedMetrics:['idleRssBytes']};
  r.combinedWorkload.failedMetrics=['idleRssBytes'];
  assert.deepEqual(summarizeOutputTrial(r,{combinedWorkload:true}).combinedMemory.failedMetrics,['idleRssBytes']);
});

test('output summary rejects missing long-task evidence and retains measured threshold failures',()=>{
  const r=result();delete r.rendererLongTasks;assert.throws(()=>summarizeOutputTrial(r));
  const high=result();Object.assign(high.rendererLongTasks,{entries:[{startTime:10,duration:250}],maximumMs:250,overLimitCount:1});
  assert.equal(summarizeOutputTrial(high).rendererLongTaskMs,250);
  high.rendererLongTasks.windowCompleted=false;assert.throws(()=>summarizeOutputTrial(high));
});

test('requires complete main trace and preserves an observed main task over the limit',()=>{
  for(const mutate of [
    (r:any)=>{delete r.mainLongTasks;},
    (r:any)=>{r.mainLongTasks.windowCompleted=false;},
    (r:any)=>{r.mainLongTasks.dataLoss=true;},
  ]){const r=result();mutate(r);assert.throws(()=>summarizeOutputTrial(r));}
  const r=result();r.mainLongTasks.tasks[0].durationMs=250;
  r.mainLongTasks.maximumMs=250;r.mainLongTasks.overLimitCount=1;
  assert.equal(summarizeOutputTrial(r).mainLongTaskMs,250);
});
