const test = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const { observeStdio } = require('../scripts/m5-stdio-observer.cjs');
function fixture() {
  const writes = [];
  const child = { stdin: { write(chunk, callback) { writes.push(chunk); callback?.(); return false; } }, stdout: new EventEmitter() };
  return { child, writes };
}
test('passive observer preserves write behavior and matches fragmented replies without payload retention', () => {
  const { child, writes } = fixture(), original = child.stdin.write;
  const observer = observeStdio(child);
  let called = false;
  const request = JSON.stringify({kind:'request', requestId:'r1', method:'agent.outputPage', params:{secret:'never-retain-이름'}})+'\n';
  const bytes = Buffer.from(request), split = bytes.indexOf(Buffer.from('이름')) + 1;
  assert.equal(child.stdin.write(bytes.subarray(0, split)), false);
  assert.equal(child.stdin.write(bytes.subarray(split), () => { called = true; }), false);
  assert.equal(called, true);
  assert.deepEqual(Buffer.concat(writes), bytes);
  child.stdout.emit('data', Buffer.from('{"kind":"response","requestId":"r'));
  child.stdout.emit('data', Buffer.from('1","result":{"secret":"never-retain"}}\n'));
  const result = observer.stop();
  assert.equal(result.methods['agent.outputPage'].count, 1);
  assert.equal(result.methods['agent.outputPage'].samplesMs.length, 1);
  assert.equal(result.pendingPeak, 1);
  assert.equal(result.pendingAtStop, 0);
  assert.equal(result.malformed, 0);
  assert.equal(JSON.stringify(result).includes('never-retain'), false);
  assert.equal(child.stdin.write, original);
  assert.equal(child.stdout.listenerCount('data'), 0);
});
test('oversized or malformed observations recover at the next line without changing transport', () => {
  const { child } = fixture(), observer = observeStdio(child);
  child.stdin.write('x'.repeat(1024 * 1024 + 1)+'\nnot-json\n');
  child.stdin.write('{"kind":"request","requestId":"r2","method":"__proto__"}\n');
  child.stdout.emit('data', Buffer.from('{"kind":"response","requestId":"r2"}\n'));
  const result = observer.stop();
  assert.equal(result.oversized, 1);
  assert.equal(result.malformed, 1);
  assert.equal(Object.hasOwn(result.methods, '__proto__'), true);
  assert.equal(result.methods.__proto__.count, 1);
});
test('unanswered requests are bounded and reported', () => {
  const { child } = fixture(), observer = observeStdio(child);
  for (let index=0; index<513; index++) child.stdin.write(JSON.stringify({kind:'request',requestId:`r${index}`,method:'agent.outputHead'})+'\n');
  const result = observer.stop();
  assert.equal(result.pendingPeak, 512);
  assert.equal(result.pendingAtStop, 512);
  assert.equal(result.dropped, 1);
});
