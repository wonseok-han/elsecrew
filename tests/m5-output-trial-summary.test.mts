import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { summarizeOutputTrial } from '../scripts/m5-output-trial-summary.mjs';
const {summarizeOutputGeneration}=createRequire(import.meta.url)('../scripts/m5-output-generation.cjs');
function result(): any {
  const generation=Array.from({length:4},(_,r)=>({runId:`run${r}`,
    samples:Array.from({length:3000},(_,i)=>[i+1,i*20000,1700000000000+i*20,`event${r}-${i}`,i+1])}));
  return {packagedQa:true,status:'PASS',functionalStatus:'PASS',shutdown:'accepted',visibility:'visible',
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
