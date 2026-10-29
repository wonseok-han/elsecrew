const test = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const { installOutputObserver } = require('../scripts/m5-durable-output-observer.cjs');

test('window reads retain scoped new heads without copying accumulated traces', () => {
  let receive, unsubscribed = false;
  const context = vm.createContext({
    document: { visibilityState: 'visible', querySelector: () => null,
      addEventListener() {}, removeEventListener() {}, body: {} },
    window: { m5: { onAgentOutputAvailable(listener) {
      receive = listener; return () => { unsubscribed = true; };
    } } },
    performance: { now: () => 10, timeOrigin: 1000 },
    queueMicrotask() {},
    MutationObserver: class { observe() {} disconnect() {} },
    // A live-window read must not clone the accumulated global report.
    structuredClone() { throw new Error('Unexpected full trace copy'); },
  });
  const observer = vm.runInContext(`(${installOutputObserver.toString()})(() => ({
    select() {}, receive() {}, snapshot() { return {samples:[]}; }
  }))`, context);
  const emit = (runId, sequence) => receive({binding:{runId}, committedSequence:sequence, sealed:false});
  emit('a', 1);
  const cursor = observer.mark();
  emit('b', 2); emit('a', 3);
  const window = observer.readWindow(cursor, 'a');
  assert.equal(window.heads.length, 1);
  assert.equal(window.heads[0].sequence, 3);
  window.heads[0].sequence = 999;
  assert.equal(observer.readWindow(cursor, 'a').heads[0].sequence, 3);
  assert.throws(() => observer.readWindow({heads:-1,frames:0}, 'a'), /cursor/);
  assert.throws(() => observer.readWindow({heads:9999,frames:0}, 'a'), /cursor/);
  for (let n = 4; n < 4100; n++) emit('b', n);
  assert.ok(observer.readWindow(cursor, 'a').droppedHeads > 0);
  const full = observer.stop();
  assert.equal(full.heads.length, 4096);
  assert.equal(full.heads[0].sequence, 1);
  assert.equal(unsubscribed, true);
});
