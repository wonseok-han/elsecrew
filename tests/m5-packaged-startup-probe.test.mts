import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readPackagedStartupMark} from '../scripts/m5-packaged-startup-probe.mjs';

function observe(entries: {startTime:number}[], visibility = 'visible', timeOrigin = 1000) {
  return vm.runInNewContext(`(${readPackagedStartupMark.toString()})()`, {
    document:{visibilityState:visibility},
    performance:{timeOrigin, getEntriesByName(name:string, kind:string) {
      assert.equal(name, 'm5:start-screen-ready'); assert.equal(kind, 'mark'); return entries;
    }},
  });
}
test('packaged startup waits for the two-frame mark instead of accepting an empty observation', () => {
  assert.equal(observe([]), null);
  const result = observe([{startTime:42}]);
  assert.deepEqual(JSON.parse(JSON.stringify(result)), {timeOrigin:1000, marks:[42], visibility:'visible',
    scope:'renderer-local two-frame start-screen readiness; not app spawn duration'});
});
test('hidden, duplicate or invalid startup observations fail rather than become timings', () => {
  assert.throws(() => observe([], 'hidden'), /hidden/);
  assert.throws(() => observe([{startTime:1}, {startTime:2}]), /ambiguous/);
  for (const startTime of [-1, NaN, Infinity]) assert.throws(() => observe([{startTime}]), /invalid/);
  for (const origin of [0, -1, NaN, Infinity]) assert.throws(() => observe([{startTime:1}], 'visible', origin), /invalid/);
});
