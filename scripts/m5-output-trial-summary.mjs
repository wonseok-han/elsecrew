import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const { summarizeOutputGeneration } = createRequire(import.meta.url)('./m5-output-generation.cjs');

export function summarizeOutputTrial(result) {
  assert.equal(result.packagedQa, true);
  assert.equal(result.status, 'PASS', 'Incomplete/hidden output trial');
  assert.equal(result.functionalStatus, 'PASS');
  assert.equal(result.shutdown, 'accepted');
  assert.equal(result.visibility, 'visible');
  assert.ok(result.visibilityTrace?.length > 0);
  assert.ok(result.visibilityTrace.every(row => row.state === 'visible'));
  assert.deepEqual(result.restoredChats, [0,1,2,3]);
  const generation = summarizeOutputGeneration(result.generation);
  assert.deepEqual(result.generationSummary, generation);
  const observation = result.outputObservation;
  assert.equal(observation.droppedHeads, 0);
  assert.equal(observation.droppedFrames, 0);
  const live = observation.liveLatency;
  assert.equal(live.dropped, 0); assert.equal(live.unfinished, 0);
  assert.ok(live.samples.length > 0, 'No live latency observation');
  assert.ok(live.samples.every(row => Number.isFinite(row.notificationToFrameMs) && row.notificationToFrameMs >= 0));
  assert.equal(result.memory.errors, 0); assert.equal(result.memory.truncated, false);
  assert.ok(result.memory.samples.length > 0);
  for (const sample of result.memory.samples) {
    assert.ok(sample.processes.length > 0);
    assert.ok(sample.processes.every(row => Number.isSafeInteger(row.pid) && row.pid > 0
      && Number.isSafeInteger(row.rssBytes) && row.rssBytes >= 0));
    assert.equal(sample.totalBytes, sample.processes.reduce((sum,row) => sum+row.rssBytes,0));
  }
  return {persistedRows:generation.persistedRows,
    allStampedWithin60Seconds:generation.allStampedWithin60Seconds,
    maximumScheduleLagMs:Math.max(...generation.streams.map(row => row.maximumScheduleLagMs)),
    maximumSampledLiveLatencyMs:Math.max(...live.samples.map(row => row.notificationToFrameMs)),
    liveSampleCount:live.samples.length, superseded:live.superseded, interrupted:live.interrupted,
    peakRssBytes:Math.max(...result.memory.samples.map(row => row.totalBytes))};
}
