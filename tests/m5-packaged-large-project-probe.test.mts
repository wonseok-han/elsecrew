import assert from 'node:assert/strict';
import test from 'node:test';
import { createContext, runInContext } from 'node:vm';
import { probeLargeProject } from '../scripts/m5-packaged-large-project-probe.mjs';

test('large project observation bounds missing frames and preserves its failed phase', async () => {
  const canceled: number[] = [];
  const context = createContext({
    document: {visibilityState:'hidden', querySelectorAll: () => [
      {textContent:'aaa-tree', disabled:false, click() {}},
      {textContent:'folder-0001', disabled:false, click() {}},
    ]},
    performance: {now: () => 0},
    requestAnimationFrame: () => 7,
    cancelAnimationFrame: (id: number) => { canceled.push(id); },
    setTimeout: (callback: () => void, ms: number) => {
      assert.equal(ms, 2000);
      queueMicrotask(callback);
      return 1;
    },
    clearTimeout() {},
  });
  await assert.rejects(runInContext(`(${probeLargeProject.toString()})('test-project')`, context),
    /presentation deadline exceeded/);
  assert.deepEqual(canceled, [7]);
  assert.deepEqual(JSON.parse(runInContext('JSON.stringify(globalThis.__m5LargeProjectProgress)', context)),
    {stage:'aaa-tree', waiting:'frames', visibility:'hidden', frames:0});
});
