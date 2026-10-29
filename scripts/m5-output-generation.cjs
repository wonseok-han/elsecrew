// Summarize all persisted fixture rows, not the coalesced head notifications.
// emitted_us is captured before the fixture's stdout write; it is not a commit
// or renderer timestamp. A late complete stream must stay visibly late.
function summarizeOutputGeneration(runs) {
  if (!Array.isArray(runs) || runs.length !== 4) throw new Error('Four output runs required');
  const runIds = new Set(), eventIds = new Set();
  const streams = runs.map(run => {
    if (typeof run.runId !== 'string' || !run.runId || runIds.has(run.runId)) throw new Error('Output run identity differs');
    runIds.add(run.runId);
    if (!Array.isArray(run.samples) || run.samples.length !== 3000) throw new Error('Incomplete output samples');
    let previousUs = 0, maximumIntervalMs = 0, maximumScheduleLagMs = 0, stampedWithin60Seconds = 0;
    for (let i = 0; i < run.samples.length; i++) {
      const [sequence, elapsedUs, unixMs, eventId, outputSequence] = run.samples[i];
      if (sequence !== i + 1 || outputSequence !== sequence
        || !Number.isSafeInteger(elapsedUs) || elapsedUs < previousUs
        || !Number.isSafeInteger(unixMs) || unixMs <= 0
        || typeof eventId !== 'string' || !eventId || eventIds.has(eventId)) throw new Error('Output sample identity, order or clock differs');
      eventIds.add(eventId);
      if (i > 0) maximumIntervalMs = Math.max(maximumIntervalMs, (elapsedUs - previousUs) / 1000);
      maximumScheduleLagMs = Math.max(maximumScheduleLagMs, elapsedUs / 1000 - i * 20);
      if (elapsedUs < 60_000_000) stampedWithin60Seconds++;
      previousUs = elapsedUs;
    }
    return {runId:run.runId, persistedRows:run.samples.length, stampedWithin60Seconds,
      firstStampMs:run.samples[0][1] / 1000, lastStampMs:previousUs / 1000,
      maximumIntervalMs, maximumScheduleLagMs};
  });
  return {diagnostic:true, gateEligible:false, persistedRows:eventIds.size, streams,
    allStampedWithin60Seconds:streams.every(stream => stream.stampedWithin60Seconds === 3000),
    scope:'Complete persisted fixture rows and pre-write timestamps; not wire receipt, commit or every-row paint timing.'};
}
module.exports = { summarizeOutputGeneration };
