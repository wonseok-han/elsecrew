// CDP Tracing ReportEvents; Chromium trace durations are microseconds.
// Keep only timing/identity fields, never task args or application payloads.
// https://chromedevtools.github.io/devtools-protocol/tot/Tracing/
export function mainTaskMaximum(observation) {
  const fail = () => { throw new Error('Incomplete main task trace'); };
  if (!observation || observation.valid !== true || observation.windowCompleted !== true ||
      observation.completed !== true || observation.dataLoss !== false || observation.incompleteMain !== false || observation.mainThreads !== 1 ||
      observation.limitMs !== 200 || !Array.isArray(observation.tasks) ||
      observation.tasks.length < 1 || observation.tasks.length > 10000) fail();
  for (const task of observation.tasks) {
    if (!Number.isSafeInteger(task.tid) || task.tid < 0 || !Number.isFinite(task.startUs) ||
        task.startUs < 0 || !Number.isFinite(task.durationMs) || task.durationMs < 0) fail();
  }
  if (new Set(observation.tasks.map(task => task.tid)).size !== 1) fail();
  const maximum = Math.max(...observation.tasks.map(task => task.durationMs));
  if (maximum !== observation.maximumMs ||
      observation.overLimitCount !== observation.tasks.filter(task => task.durationMs > 200).length) fail();
  return maximum;
}

export function createMainTaskTrace(rootPid) {
  if (!Number.isSafeInteger(rootPid) || rootPid < 1) throw new Error('Invalid trace root');
  const threads = new Set(), incompleteThreads = new Set(), tasks = [], phases = Object.create(null);
  let count = 0, invalid = false, completed = false, dataLoss = false;
  return {
    accept(message) {
      if (message.method === 'Tracing.tracingComplete') {
        if (completed || message.params?.dataLossOccurred !== false) invalid = true;
        dataLoss = message.params?.dataLossOccurred !== false;
        completed = true;
      }
      if (message.method !== 'Tracing.dataCollected') return;
      if (completed || !Array.isArray(message.params?.value)) { invalid = true; return; }
      for (const event of message.params.value) {
        // Long stream scenarios include events from every Chromium process.
        // Bound processing separately from the 10,000 retained task samples.
        if (++count > 1000000) { invalid = true; break; }
        if (event?.pid !== rootPid) continue;
        if (event.ph === 'M' && event.name === 'thread_name' && event.args?.name === 'CrBrowserMain') {
          if (!Number.isSafeInteger(event.tid) || event.tid < 0) invalid = true;
          else threads.add(event.tid);
        }
        if (typeof event.cat !== 'string' || !event.cat.split(',').includes('toplevel')) continue;
        phases[event.ph] = (phases[event.ph] ?? 0) + 1;
        // Instant markers and flow arrows have no duration and are not tasks.
        if (['I', 'i', 's', 't', 'f'].includes(event.ph)) continue;
        // B/E pairs and unfinished slices are not silently interpreted as zero.
        if (!Number.isSafeInteger(event.tid) || event.tid < 0 || tasks.length >= 10000) {
          invalid = true; continue;
        }
        if (!Number.isFinite(event.ts) || event.ts < 0 || event.ph !== 'X' ||
            !Number.isFinite(event.dur) || event.dur < 0) {
          incompleteThreads.add(event.tid); continue;
        }
        tasks.push({tid:event.tid, startUs:event.ts, durationMs:event.dur / 1000});
      }
    },
    get completed() { return completed; },
    finish(windowCompleted) {
      const selected = tasks.filter(task => threads.has(task.tid));
      const incompleteMain = [...threads].some(tid => incompleteThreads.has(tid));
      const valid = completed && !invalid && !dataLoss && !incompleteMain && threads.size === 1 && selected.length > 0;
      return {diagnostic:true, gateEligible:false, valid, windowCompleted,
        completed, dataLoss, incompleteMain, receivedEvents:count, mainThreads:threads.size, phases,
        tasks:selected, maximumMs:valid ? Math.max(...selected.map(task => task.durationMs)) : null,
        overLimitCount:valid ? selected.filter(task => task.durationMs > 200).length : null,
        limitMs:200, scope:'Chromium complete toplevel slices on the browser main thread inside the traced interval; excludes pre-attachment startup and untraced work.'};
    },
  };
}
