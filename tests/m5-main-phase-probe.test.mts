import test from 'node:test';
import assert from 'node:assert/strict';
import {createMainPhaseProbe} from '../scripts/m5-main-task-session.mjs';
const wait=(ms:number)=>new Promise(resolve=>setTimeout(resolve,ms));
test('phase diagnostic observes real blocking, stops once and stays separate from task limits', async()=>{
  const probe=createMainPhaseProbe();
  try {
    await wait(25);probe.phase('loaded');await wait(25);
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,60);
    await wait(25);
    const result=probe.stop();
    assert.equal(result.gateEligible,false);
    const loaded=result.rows.find(row=>row.phase==='loaded');
    assert.ok(loaded.samples>0);assert.ok(loaded.maximumTimerDelayMs>=50);
    assert.ok(loaded.endedAt>=loaded.startedAt);
    assert.deepEqual(probe.stop(),result);
    assert.throws(()=>probe.phase('retained'));
  } finally {probe.stop();}
});
test('phase count is bounded and invalid labels rejected',()=>{
  const probe=createMainPhaseProbe();
  try {assert.throws(()=>probe.phase('untrusted-label'));for(let i=0;i<16;i++)probe.phase('idle');assert.throws(()=>probe.phase('idle'));assert.ok(probe.stop().rows.length<=17);}
  finally {probe.stop();}
});
