const test = require('node:test');
const assert = require('node:assert/strict');
const { summarizeOutputGeneration } = require('../scripts/m5-output-generation.cjs');
const fixture = () => Array.from({length:4}, (_, r) => ({runId:`run${r}`,
  samples:Array.from({length:3000}, (_, i) => [i+1,i*20000,1700000000000+i*20,`event${r}-${i}`,i+1])}));
test('complete rows count separately from heads and late rows remain late', () => {
  const runs = fixture();
  assert.equal(summarizeOutputGeneration(runs).persistedRows,12000);
  runs[3].samples[2999][1] = 61_000_000;
  const summary = summarizeOutputGeneration(runs);
  assert.equal(summary.allStampedWithin60Seconds,false);
  assert.equal(summary.streams[3].stampedWithin60Seconds,2999);
  assert.equal(summary.streams[3].maximumScheduleLagMs,1020);
});
test('reject missing, duplicate, crossed sequence and regressing clock evidence', () => {
  for (const mutate of [
    r => r.pop(), r => r[0].samples.pop(),
    r => {r[1].runId=r[0].runId;},
    r => {r[1].samples[0][3]=r[0].samples[0][3];},
    r => {r[0].samples[2][4]=1;},
    r => {r[0].samples[2][1]=1;},
  ]) {const runs=fixture(); mutate(runs); assert.throws(() => summarizeOutputGeneration(runs));}
});
