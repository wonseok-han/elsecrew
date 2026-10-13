import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { beginLongTaskProbe, endLongTaskProbe } from '../scripts/m5-packaged-long-task-probe.mjs';

function fixture(supported = true) {
  let deliver: (entries: unknown[]) => void;
  let queued: unknown[] = [];
  let disconnected = false;
  const context = vm.createContext({performance:{now:() => 100}, PerformanceObserver:class {
    static supportedEntryTypes = supported ? ['longtask'] : [];
    constructor(callback: (list: {getEntries: () => unknown[]}) => void) {
      deliver = entries => callback({getEntries:() => entries});
    }
    observe(options: unknown) { assert.equal(JSON.stringify(options), '{"type":"longtask","buffered":false}'); }
    takeRecords() { return queued; }
    disconnect() { disconnected = true; }
  }});
  const invoke = (fn: Function) => JSON.parse(JSON.stringify(vm.runInContext(`(${fn.toString()})()`, context)));
  return {begin:() => invoke(beginLongTaskProbe), end:() => invoke(endLongTaskProbe),
    deliver:(entries: unknown[]) => deliver(entries), queue:(entries: unknown[]) => {queued = entries;},
    disconnected:() => disconnected};
}

test('unsupported long-task observation never fabricates zero measurements', () => {
  const f = fixture(false);
  assert.equal(f.begin().supported, false);
  assert.equal(f.end().maximumMs, null);
  assert.equal(f.end().overLimitCount, null);
});

test('long tasks include pending records, enforce strict 200ms limit and disconnect', () => {
  const f = fixture();
  f.begin();
  assert.throws(() => f.begin(), /already active/);
  f.deliver([{startTime:100, duration:200}]);
  f.queue([{startTime:120, duration:201}]);
  const result = f.end();
  assert.equal(result.valid, true);
  assert.equal(result.maximumMs, 201);
  assert.equal(result.overLimitCount, 1);
  assert.equal(result.entries.length, 2);
  assert.equal(f.disconnected(), true);
  assert.equal(f.end().supported, false);
});

test('empty observed window is zero but overflow and invalid values are not', () => {
  const empty = fixture(); empty.begin();
  assert.equal(empty.end().maximumMs, 0);
  for (const entries of [[{startTime:100, duration:-1}], Array.from({length:10001}, () => ({startTime:100, duration:50}))]) {
    const f = fixture(); f.begin(); f.deliver(entries);
    const result = f.end();
    assert.equal(result.valid, false);
    assert.equal(result.maximumMs, null);
    assert.equal(result.overLimitCount, null);
    assert.equal(f.disconnected(), true);
  }
});
