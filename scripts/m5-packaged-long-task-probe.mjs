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
