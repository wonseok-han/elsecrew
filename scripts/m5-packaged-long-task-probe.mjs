// Validate persisted observations before aggregating repeated package trials.
export function packagedLongTaskMaximum(observation) {
  const fail = () => { throw new Error('Incomplete or inconsistent renderer long-task observation'); };
  if (!observation || observation.supported !== true || observation.valid !== true ||
      observation.windowCompleted !== true || observation.overflow !== false ||
      observation.limitMs !== 200 || !Number.isFinite(observation.startedAt) ||
      observation.startedAt < 0 || !Number.isFinite(observation.endedAt) ||
      observation.endedAt < observation.startedAt || !Array.isArray(observation.entries) ||
      observation.entries.length > 10000) fail();
  let maximum = 0, overLimit = 0;
  for (const entry of observation.entries) {
    if (!entry || !Number.isFinite(entry.startTime) || entry.startTime < observation.startedAt ||
        entry.startTime > observation.endedAt || !Number.isFinite(entry.duration) || entry.duration < 0) fail();
    maximum = Math.max(maximum, entry.duration);
    if (entry.duration > 200) overLimit++;
  }
  if (observation.maximumMs !== maximum || observation.overLimitCount !== overLimit) fail();
  return maximum;
}

// Self-contained functions serialized into the real packaged renderer.
export function beginLongTaskProbe() {
  if (globalThis.__m5LongTasks) throw new Error('Long-task observer already active');
  if (!globalThis.PerformanceObserver?.supportedEntryTypes?.includes('longtask')) {
    return {supported:false};
  }
  const state = {startedAt:performance.now(), entries:[], overflow:false};
  const append = entries => {
    for (const entry of entries) {
      if (state.entries.length >= 10000) { state.overflow = true; break; }
      state.entries.push({startTime:entry.startTime, duration:entry.duration});
    }
  };
  const observer = new PerformanceObserver(list => append(list.getEntries()));
  observer.observe({type:'longtask', buffered:false});
  globalThis.__m5LongTasks = {state, observer, append};
  return {supported:true, startedAt:state.startedAt};
}

export function endLongTaskProbe() {
  const active = globalThis.__m5LongTasks;
  if (!active) return {supported:false, maximumMs:null, overLimitCount:null};
  try {
    active.append(active.observer.takeRecords());
    const {state} = active;
    const valid = !state.overflow && state.entries.every(entry =>
      Number.isFinite(entry.duration) && entry.duration >= 0 &&
      Number.isFinite(entry.startTime) && entry.startTime >= state.startedAt);
    return {supported:true, valid, ...state, endedAt:performance.now(), limitMs:200,
      maximumMs:valid ? Math.max(0, ...state.entries.map(entry => entry.duration)) : null,
      overLimitCount:valid ? state.entries.filter(entry => entry.duration > 200).length : null};
  } finally {
    active.observer.disconnect();
    delete globalThis.__m5LongTasks;
  }
}
