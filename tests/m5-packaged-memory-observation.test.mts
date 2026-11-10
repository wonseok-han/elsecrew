import test from 'node:test';
import assert from 'node:assert/strict';
import { packagedMemoryObservation } from '../scripts/m5-packaged-memory-observation.mjs';
import { summarizeMemory } from '../scripts/m5-process-memory.cjs';

function observation() {
  const samples = [{phase:'idle', rssBytes:513 * 1024 ** 2},
    {phase:'loaded', rssBytes:700 * 1024 ** 2}, {phase:'retained', rssBytes:400 * 1024 ** 2}];
  return {
    memoryLifecycle: {samples, ...summarizeMemory(samples), closedAtMs:1000,
      retainedStartedAtMs:61000, largeFileRetainedWithDiff:true},
    largeProject: {fixture:{treeFiles:10000, treeDirectories:1000, controlFiles:2, fileBytes:5242880, fileLines:100000}},
    largeDiff: {diffFiles:500, diffChangedLines:20000, diffBytes:10485760, originalUnchanged:true},
  };
}

test('complete combined observation retains known memory failure', () => {
  const result = observation();
  assert.equal(packagedMemoryObservation(result, true).idleRssBytes, 513 * 1024 ** 2);
  assert.deepEqual(result.memoryLifecycle.failedMetrics, ['idleRssBytes']);
});
test('combined observation rejects missing coexistence or wrong fixtures', () => {
  for (const mutate of [
    r => { r.memoryLifecycle.largeFileRetainedWithDiff = false; },
    r => { r.largeProject.fixture.fileBytes--; },
    r => { r.largeDiff.diffFiles--; },
    r => { r.largeDiff.originalUnchanged = false; },
  ]) { const result = observation(); mutate(result); assert.throws(() => packagedMemoryObservation(result, true)); }
});
test('rejects short waits, incomplete phases and fabricated summaries', () => {
  for (const mutate of [
    r => { r.memoryLifecycle.retainedStartedAtMs--; },
    r => { r.memoryLifecycle.closedAtMs = NaN; },
    r => { r.memoryLifecycle.samples.pop(); },
    r => { r.memoryLifecycle.maxima.idleRssBytes = 1; },
    r => { r.memoryLifecycle.failedMetrics = []; },
  ]) { const result = observation(); mutate(result); assert.throws(() => packagedMemoryObservation(result, true)); }
});
test('existing diff-only observations do not require the combined flag', () => {
  const result = observation();
  result.memoryLifecycle.largeFileRetainedWithDiff = false;
  assert.equal(packagedMemoryObservation(result).peakRssBytes, 700 * 1024 ** 2);
});
