import test from 'node:test';
import assert from 'node:assert/strict';
import {createMainTaskTrace,mainTaskMaximum} from '../scripts/m5-main-task-trace.mjs';
const metadata = {pid:12, tid:8, ph:'M', name:'thread_name', args:{name:'CrBrowserMain'}};
const task = {pid:12, tid:8, ph:'X', cat:'toplevel', ts:100, dur:201000};
function collect(events: unknown[], loss = false) {
  const trace = createMainTaskTrace(12);
  trace.accept({method:'Tracing.dataCollected', params:{value:events}});
  trace.accept({method:'Tracing.tracingComplete', params:{dataLossOccurred:loss}});
  return trace.finish(true);
}
test('main trace selects exact root and main thread, converts microseconds and strips args', () => {
  const result = collect([task, {...task,pid:99,dur:999999}, {...task,tid:9,dur:999999}, metadata]);
  assert.equal(result.valid, true);
  assert.equal(result.maximumMs, 201);
  assert.equal(mainTaskMaximum(result),201);
  assert.equal(result.overLimitCount, 1);
  assert.deepEqual(result.tasks, [{tid:8,startUs:100,durationMs:201}]);
});
test('aggregation rechecks raw measurements and completion', () => {
  const result=collect([metadata,task]);
  for (const patch of [{maximumMs:0},{overLimitCount:0},{windowCompleted:false},{tasks:[]},{dataLoss:true}]) {
    assert.throws(()=>mainTaskMaximum({...result,...patch}));
  }
});
test('missing, incomplete, unsupported and lost traces never manufacture zero', () => {
  for (const events of [[], [metadata], [task], [metadata,{...task,ph:'B'}],
    [metadata,{...task,dur:-1}], [metadata,{...metadata,tid:9},task]]) {
    assert.equal(collect(events).maximumMs, null);
  }
  assert.equal(collect([metadata,task],true).valid,false);
  const trace = createMainTaskTrace(12);
  trace.accept({method:'Tracing.dataCollected',params:{value:[metadata,task]}});
  assert.equal(trace.finish(false).maximumMs,null);
});
test('trace bounds and completion ordering fail closed', () => {
  assert.equal(collect([metadata,...Array(10001).fill(task)]).valid,false);
  const trace = createMainTaskTrace(12);
  trace.accept({method:'Tracing.tracingComplete',params:{dataLossOccurred:false}});
  trace.accept({method:'Tracing.dataCollected',params:{value:[metadata,task]}});
  assert.equal(trace.finish(true).valid,false);
});
test('instant markers and flow arrows are not timed tasks', () => {
  const result=collect([metadata,task,...['I','i','s','t','f'].map(ph=>({...task,ph,dur:undefined}))]);
  assert.equal(mainTaskMaximum(result),201);
  assert.equal(result.tasks.length,1);
});
test('unfinished worker tasks do not invalidate complete main-thread observations', () => {
  assert.equal(mainTaskMaximum(collect([metadata,task,{...task,ph:'B',tid:9}])),201);
  assert.equal(collect([metadata,task,{...task,ph:'B'}]).valid,false);
});
