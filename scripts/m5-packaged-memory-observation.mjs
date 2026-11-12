import assert from 'node:assert/strict';
import { summarizeMemory } from './m5-process-memory.cjs';

// Validate observations, not the product gate. A valid observation may fail limits.
export function packagedMemoryObservation(result, combined = false) {
  const lifecycle = result.memoryLifecycle;
  assert.ok(lifecycle && !lifecycle.error && lifecycle.samples?.length, 'Incomplete memory lifecycle; fixture retained');
  assert.ok(Number.isFinite(lifecycle.closedAtMs) && lifecycle.closedAtMs >= 0
    && Number.isFinite(lifecycle.retainedStartedAtMs)
    && lifecycle.retainedStartedAtMs - lifecycle.closedAtMs >= 60000, 'Retention wait incomplete');
  if (combined) {
    assert.equal(lifecycle.largeFileRetainedWithDiff, true, 'Combined file/diff observation missing');
    assert.deepEqual(result.largeProject?.fixture,
      {treeFiles:10000, treeDirectories:1000, controlFiles:2, fileBytes:5242880, fileLines:100000});
    assert.equal(result.largeDiff?.diffFiles, 500);
    assert.equal(result.largeDiff.diffChangedLines, 20000);
    assert.equal(result.largeDiff.diffBytes, 10485760);
    assert.equal(result.largeDiff.originalUnchanged, true);
  }
  const observed = summarizeMemory(lifecycle.samples);
  assert.deepEqual(lifecycle.maxima, observed.maxima);
  assert.deepEqual(lifecycle.failedMetrics, observed.failedMetrics);
  return observed.maxima;
}
