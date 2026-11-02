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
  if (maximum !== observation.maximumMs) fail();
  if (observation.aggregation === 'maximum-and-count-v1') {
    if (observation.tasks.length !== 1 || !Number.isSafeInteger(observation.taskCount) ||
        observation.taskCount < 1 || observation.taskCount > 1000000 ||
        !Number.isSafeInteger(observation.receivedEvents) || observation.receivedEvents < observation.taskCount ||
        observation.receivedEvents > 1000000 || !Number.isSafeInteger(observation.overLimitCount) ||
        observation.overLimitCount < 0 || observation.overLimitCount > observation.taskCount ||
        (maximum > 200 ? observation.overLimitCount < 1 : observation.overLimitCount !== 0)) fail();
  } else if (observation.aggregation !== undefined ||
      observation.overLimitCount !== observation.tasks.filter(task => task.durationMs > 200).length) fail();
  return maximum;
}

export function createMainTaskTrace(rootPid) {
  if (!Number.isSafeInteger(rootPid) || rootPid < 1) throw new Error('Invalid trace root');
  const threads = new Set(), incompleteThreads = new Set(), summaries = new Map(), phases = Object.create(null);
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
        // Count every event but retain only one maximum witness per root thread.
        if (count >= 1000000) { invalid = true; break; }
        count++;
        if (event?.pid !== rootPid) continue;
        if (event.ph === 'M' && event.name === 'thread_name' && event.args?.name === 'CrBrowserMain') {
          if (!Number.isSafeInteger(event.tid) || event.tid < 0) invalid = true;
          else if (threads.size >= 2 && !threads.has(event.tid)) invalid = true;
          else threads.add(event.tid);
        }
        if (typeof event.cat !== 'string' || !event.cat.split(',').includes('toplevel')) continue;
        const phase = ['X','B','E','I','i','s','t','f'].includes(event.ph) ? event.ph : 'other';
        phases[phase] = (phases[phase] ?? 0) + 1;
        // Instant markers and flow arrows have no duration and are not tasks.
        if (['I', 'i', 's', 't', 'f'].includes(event.ph)) continue;
        // B/E pairs and unfinished slices are not silently interpreted as zero.
        if (!Number.isSafeInteger(event.tid) || event.tid < 0) {
          invalid = true; continue;
        }
        if (!summaries.has(event.tid)) {
          if (summaries.size >= 128) { invalid = true; continue; }
          summaries.set(event.tid, {count:0, overLimitCount:0, maximum:null});
        }
        if (!Number.isFinite(event.ts) || event.ts < 0 || event.ph !== 'X' ||
            !Number.isFinite(event.dur) || event.dur < 0) {
          incompleteThreads.add(event.tid); continue;
        }
        const summary = summaries.get(event.tid);
        const durationMs = event.dur / 1000;
        summary.count++;
        if (durationMs > 200) summary.overLimitCount++;
        if (!summary.maximum || durationMs > summary.maximum.durationMs) {
          summary.maximum = {tid:event.tid, startUs:event.ts, durationMs};
        }
      }
    },
    get completed() { return completed; },
    finish(windowCompleted) {
      const summary = threads.size === 1 ? summaries.get([...threads][0]) : undefined;
      const selected = summary?.maximum ? [summary.maximum] : [];
      const incompleteMain = [...threads].some(tid => incompleteThreads.has(tid));
      const valid = completed && !invalid && !dataLoss && !incompleteMain && threads.size === 1 && selected.length > 0;
      return {diagnostic:true, gateEligible:false, valid, windowCompleted,
        completed, dataLoss, incompleteMain, receivedEvents:count, mainThreads:threads.size, phases,
        aggregation:'maximum-and-count-v1', taskCount:summary?.count ?? 0,
        tasks:selected, maximumMs:valid ? summary.maximum.durationMs : null,
        overLimitCount:valid ? summary.overLimitCount : null,
        limitMs:200, scope:'Chromium complete toplevel slices on the browser main thread inside the traced interval; excludes pre-attachment startup and untraced work.'};
    },
  };
}
