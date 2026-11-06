import { mainTaskMaximum } from './m5-main-task-trace.mjs';
import assert from 'node:assert/strict';
import { packagedLongTaskMaximum } from './m5-packaged-long-task-probe.mjs';
import { createRequire } from 'node:module';
const { summarizeOutputGeneration } = createRequire(import.meta.url)('./m5-output-generation.cjs');

const { summarizeMemory } = createRequire(import.meta.url)('./m5-process-memory.cjs');

export function summarizeOutputTrial(result, {combinedWorkload = false} = {}) {
  assert.equal(Boolean(result.combinedWorkload), combinedWorkload, 'Unexpected workload evidence');
  assert.equal(result.packagedQa, true);
  assert.equal(result.status, 'PASS', 'Incomplete/hidden output trial');
  assert.equal(result.functionalStatus, 'PASS');
  assert.equal(result.shutdown, 'accepted');
  assert.equal(result.userInput?.dropped, 0, 'Missing/truncated navigation input evidence');
  assert.deepEqual(result.userInput.events, [], 'External navigation input interrupted trial');
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
  let combinedMemory, inputPaintMs;
  if (combinedWorkload) {
    const c = result.combinedWorkload;
    assert.equal(c.status, 'OBSERVED');
    assert.equal(c.coexist, true); assert.equal(c.originalUnchanged, true);
    assert.deepEqual(c.fixture, {treeFiles:10000,treeDirectories:1000,fileBytes:5242880,fileLines:100000});
    assert.deepEqual([c.largeDiff.diffFiles,c.largeDiff.diffChangedLines,c.largeDiff.diffBytes], [500,20000,10485760]);
    assert.deepEqual(c.largeProject.failedMetrics, []); assert.deepEqual(c.largeDiff.failedMetrics, []);
    assert.equal(c.liveCoexistence.editor, true); assert.equal(c.liveCoexistence.diff, true);
    assert.equal(c.liveCoexistence.runs, 4);
    assert.ok(Number.isFinite(c.closedAt) && c.retainedStartedAt - c.closedAt >= 60000);
    assert.ok(Number.isFinite(result.outputObserverStoppedAt) && result.outputObserverStoppedAt >= 0
      && result.outputObserverStoppedAt <= c.closedAt, 'Output observer must be released before project close');
    const trace = result.mainTraceTiming;
    assert.ok(trace && [c.idleStartedAt,c.idleEndedAt,trace.requestedAt,trace.readyAt,trace.workloadRequestedAt]
      .every(value => Number.isFinite(value) && value >= 0), 'Missing workload trace boundary evidence');
    assert.ok(c.idleStartedAt <= c.idleEndedAt && c.idleEndedAt <= trace.requestedAt
      && trace.requestedAt <= trace.readyAt && trace.readyAt <= trace.workloadRequestedAt,
      'Tracing must start after idle and be ready before workload');
    const idle = result.memory.samples.filter(s=>s.phase==='idle');
    assert.ok(idle.length && idle.every(s=>Number.isFinite(s.at) && s.at >= c.idleStartedAt && s.at <= c.idleEndedAt));
    assert.ok(idle.every(sample => Number.isFinite(sample.completedAt)
      && sample.at <= sample.completedAt && sample.completedAt <= trace.requestedAt),
      'Idle RSS collection overlapped tracing');
    assert.ok(result.memory.samples.filter(s=>s.phase==='retained').every(s=>Number.isFinite(s.at) && s.at >= c.retainedStartedAt));
    combinedMemory = summarizeMemory(result.memory.samples.map(s=>({...s,rssBytes:s.totalBytes})));
    assert.deepEqual(c.memory, combinedMemory, 'Combined memory summary does not match samples');
    const input = c.editorInput;
    assert.equal(input?.restored, true); assert.equal(input.trustedInputs, 1); assert.equal(input.liveRuns, 4);
    assert.ok([input.startedAt,input.receivedAt,input.paintAt].every(value => Number.isFinite(value) && value >= 0));
    assert.ok(input.startedAt <= input.receivedAt && input.receivedAt <= input.paintAt);
    assert.equal(input.inputPaintMs,input.paintAt-input.startedAt);
    assert.equal(input.inputDeliveryMs,input.receivedAt-input.startedAt);
    inputPaintMs = input.inputPaintMs;
    assert.deepEqual(c.failedMetrics, [...combinedMemory.failedMetrics,...(inputPaintMs > 50 ? ['inputPaintMs'] : [])]);
  }
  const rendererLongTaskMs = packagedLongTaskMaximum(result.rendererLongTasks);
  const mainLongTaskMs = mainTaskMaximum(result.mainLongTasks);
  return {mainLongTaskMs,rendererLongTaskMs,...(combinedMemory ? {combinedMemory,inputPaintMs} : {}),persistedRows:generation.persistedRows,
    allStampedWithin60Seconds:generation.allStampedWithin60Seconds,
    maximumScheduleLagMs:Math.max(...generation.streams.map(row => row.maximumScheduleLagMs)),
    maximumSampledLiveLatencyMs:Math.max(...live.samples.map(row => row.notificationToFrameMs)),
    liveSampleCount:live.samples.length, superseded:live.superseded, interrupted:live.interrupted,
    peakRssBytes:Math.max(...result.memory.samples.map(row => row.totalBytes))};
}
