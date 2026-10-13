import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const { observe } = createRequire(import.meta.url)('../scripts/m5-observe-process-memory.cjs');

test('bounded observer retains raw samples without claiming a performance pass', async () => {
  const delays = [];
  const result = await observe(10, 2, 'idle', () => '10 1 100\n20 10 200\n99 1 1000', async ms => delays.push(ms));
  assert.equal(result.status, 'OBSERVED');
  assert.equal(result.gateEligible, false);
  assert.equal(result.maxRssBytes, 300 * 1024);
  assert.equal(result.samples.length, 2);
  assert.deepEqual(delays, [250]);
});
test('failed sampling preserves partial evidence and never invents zero RSS', async () => {
  let reads = 0;
  const result = await observe(10, 3, 'loaded', () => ++reads === 1 ? '10 1 100' : '', async () => {});
  assert.equal(result.status, 'FAIL');
  assert.equal(result.samples.length, 1);
  assert.match(result.error, /root missing/);
  assert.equal(result.maxRssBytes, undefined);
});
test('invalid arguments cannot start sampling', async () => {
  for (const args of [[0, 1, 'idle'], [10, 241, 'idle'], [10, 1.5, 'idle'], [10, 1, 'unknown']]) {
    await assert.rejects(observe(...args, () => { throw new Error('must not read'); }, async () => {}), /invalid observation/);
  }
});
