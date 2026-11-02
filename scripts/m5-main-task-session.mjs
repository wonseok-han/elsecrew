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
