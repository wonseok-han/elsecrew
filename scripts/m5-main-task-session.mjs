import {monitorEventLoopDelay, performance} from 'node:perf_hooks';
import {createMainTaskTrace} from './m5-main-task-trace.mjs';

// QA only. Own this debugger attachment; never detach another observer.
export async function startMainTaskSession(debuggerApi, rootPid) {
  const trace = createMainTaskTrace(rootPid);
  debuggerApi.attach('1.3');
  const listener = (_event, method, params) => trace.accept({method, params});
  const detach = () => {
    debuggerApi.removeListener('message', listener);
    if (debuggerApi.isAttached()) debuggerApi.detach();
  };
  debuggerApi.on('message', listener);
  try {
    await debuggerApi.sendCommand('Tracing.start', {categories:'toplevel', transferMode:'ReportEvents'});
  } catch (error) { detach(); throw error; }
  let stopped = false;
  return {
    async stop(windowCompleted) {
      if (stopped) throw new Error('Main trace session already stopped');
      stopped = true;
      let timer, cancelled = false;
      try {
        await Promise.race([
          (async () => {
            await debuggerApi.sendCommand('Tracing.end');
            while (!trace.completed && !cancelled) await new Promise(resolve => setTimeout(resolve, 20));
          })(),
          new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('Main trace completion timeout')), 8000); })
        ]);
        return {observation:trace.finish(windowCompleted)};
      } catch (error) {
        return {observation:trace.finish(false), error:String(error)};
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
  const allowed=['starting','idle','preparing','loaded','closing','retained'];
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
