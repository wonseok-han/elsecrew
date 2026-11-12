const test = require('node:test');
const assert = require('node:assert/strict');
const {summarizeHeapSampling} = require('../scripts/m5-heap-sampling.cjs');
const node = (size, children = []) => ({selfSize:size,children,callFrame:{functionName:'allocate',url:'m5perf://m5/bundle.js',lineNumber:1,columnNumber:2}});
test('allocation summary counts exclusive sizes without counting children twice', () => {
  const result = summarizeHeapSampling({head:node(2,[node(8,[node(3)])])});
  assert.equal(result.sampledBytes,13);
  assert.equal(result.nodes,3);
  assert.deepEqual(result.top.map(x=>x.sampledBytes),[8,3,2]);
});
test('allocation report bounds rows and rejects invalid sizes', () => {
  const result = summarizeHeapSampling({head:node(0,Array.from({length:70},()=>node(1)))});
  assert.equal(result.top.length,50);
  assert.equal(result.omitted,20);
  assert.equal(result.sampledBytes,70);
  for (const size of [-1,NaN,Infinity]) assert.throws(()=>summarizeHeapSampling({head:node(size)}));
});
