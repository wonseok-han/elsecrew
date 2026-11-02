import {monitorEventLoopDelay, performance} from 'node:perf_hooks';
import {createMainTaskTrace} from './m5-main-task-trace.mjs';

// QA only. Own this debugger attachment; never detach another observer.
export async function startMainTaskSession(debuggerApi, rootPid, {clockMarkers=false}={}) {
  const trace = createMainTaskTrace(rootPid);
  debuggerApi.attach('1.3');
  const markers=[], markerErrors=[];
  let markerInvalid=false;
  const listener = (_event, method, params) => {
    trace.accept({method, params});
    if (!clockMarkers || method !== 'Tracing.dataCollected' || !Array.isArray(params?.value)) return;
    for (const event of params.value) {
      const id=event?.args?.sync_id;
      if (!['m5-workload-start','m5-workload-end'].includes(id)) continue;
      if (event.pid !== rootPid || event.name !== 'clock_sync' || event.ph !== 'c' ||
          !Number.isSafeInteger(event.tid) || event.tid<0 || !Number.isFinite(event.ts) || event.ts<0 ||
          markers.some(marker=>marker.id===id)) {markerInvalid=true;continue;}
      markers.push({id,tid:event.tid,ts:event.ts});
    }
  };
  const mark = async stage => {
    if (!clockMarkers) return;
    let timer;
    try {
      await Promise.race([
        debuggerApi.sendCommand('Tracing.recordClockSyncMarker',{syncId:`m5-workload-${stage}`}),
        new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('Clock marker timeout')),1000);}),
      ]);
    } catch {markerErrors.push(stage);}
    finally {clearTimeout(timer);}
  };
  const finish = windowCompleted => {
    const observation=trace.finish(windowCompleted);
    if (!clockMarkers) return {observation};
    const start=markers.find(marker=>marker.id==='m5-workload-start');
    const end=markers.find(marker=>marker.id==='m5-workload-end');
    const witness=observation.tasks[0];
    const complete=Boolean(!markerInvalid && markerErrors.length===0 && start && end &&
      start.ts<=end.ts && start.tid===end.tid && start.tid===witness?.tid &&
      observation.valid && observation.windowCompleted);
    return {observation,clockBoundaries:{diagnostic:true,gateEligible:false,complete,
      invalid:markerInvalid,errors:[...markerErrors],markers:[...markers],
      scope:'Same trace clock only. Does not exclude tasks or alter the performance gate.'}};
  };
  const detach = () => {
    debuggerApi.removeListener('message', listener);
    if (debuggerApi.isAttached()) debuggerApi.detach();
  };
  debuggerApi.on('message', listener);
  try {
    await debuggerApi.sendCommand('Tracing.start', {categories:'toplevel', transferMode:'ReportEvents'});
  } catch (error) { detach(); throw error; }
  await mark('start');
  let stopped = false;
  return {
    async stop(windowCompleted) {
      if (stopped) throw new Error('Main trace session already stopped');
      stopped = true;
      let timer, cancelled = false;
      try {
        await Promise.race([
          (async () => {
            await mark('end');
            if (cancelled) return;
            await debuggerApi.sendCommand('Tracing.end');
            while (!trace.completed && !cancelled) await new Promise(resolve => setTimeout(resolve, 20));
          })(),
          new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('Main trace completion timeout')), 8000); })
        ]);
        return finish(windowCompleted);
      } catch (error) {
        return {...finish(false), error:String(error)};
      } finally {
        cancelled = true;
        clearTimeout(timer);
        detach();
      }
    },
  };
}


// Diagnostic attribution only. Timer delay is not a Chromium task duration.
// https://nodejs.org/api/perf_hooks.html#perf_hooksmonitoreventloopdelayoptions
export function createMainPhaseProbe() {
  const histogram=monitorEventLoopDelay({resolution:10});
  const allowed=['starting','idle','preparing','loaded','live-output','history-pages','generation-audit','closing','retained'];
  const rows=[];
  let phase='starting', startedAt=performance.now(), baseline=performance.eventLoopUtilization(), stopped=false;
  histogram.enable();
  function record() {
    const endedAt=performance.now(), utilization=performance.eventLoopUtilization(baseline);
    const utilizationAvailable=utilization.active+utilization.idle>0;
    rows.push({phase,startedAt,endedAt,samples:histogram.count,utilizationAvailable,
      maximumTimerDelayMs:histogram.count ? histogram.max/1e6 : null,
      activeMs:utilizationAvailable ? utilization.active : null,
      idleMs:utilizationAvailable ? utilization.idle : null});
    startedAt=endedAt;baseline=performance.eventLoopUtilization();histogram.reset();
  }
  return {
    phase(next) {
      if(stopped || !allowed.includes(next) || rows.length>=16) throw new Error('Invalid diagnostic phase');
      record();phase=next;
    },
    stop() {
      if(!stopped) {record();histogram.disable();stopped=true;}
      return {diagnostic:true,gateEligible:false,rows:rows.map(row=>({...row})),
        scope:'Node timer delay and event-loop utilization by QA phase; phase boundaries approximate; never replaces Chromium task limit.'};
    },
  };
}
