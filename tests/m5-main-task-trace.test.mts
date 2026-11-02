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
  assert.equal(collect([metadata,...Array.from({length:129}, (_,i)=>({...task,tid:i}))]).valid,false);
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
test('long cross-process streams retain only bounded root tasks and fail at the processing budget', () => {
  const trace = createMainTaskTrace(12);
  trace.accept({method:'Tracing.dataCollected',params:{value:[metadata,task]}});
  const batch = Array(10000).fill({...task,pid:99,args:{privatePayload:'not retained'}});
  for(let index=0;index<20;index++) trace.accept({method:'Tracing.dataCollected',params:{value:batch}});
  const partial=trace.finish(true);
  assert.equal(partial.receivedEvents,200002);
  assert.equal(partial.tasks.length,1);
  assert.equal(JSON.stringify(partial).includes('privatePayload'),false);
  for(let index=0;index<80;index++) trace.accept({method:'Tracing.dataCollected',params:{value:batch}});
  trace.accept({method:'Tracing.tracingComplete',params:{dataLossOccurred:false}});
  assert.equal(trace.finish(true).valid,false);
  const withinBudget=collect([metadata,task,...Array(100001).fill({...task,pid:99})]);
  assert.equal(mainTaskMaximum(withinBudget),201);
});

test('long root streams aggregate every task including late maxima and late main metadata', () => {
  const trace=createMainTaskTrace(12);
  for(let i=0;i<20;i++) trace.accept({method:'Tracing.dataCollected',params:{value:
    Array(10000).fill({...task,dur:1000})}});
  trace.accept({method:'Tracing.dataCollected',params:{value:[
    {...task,tid:9,dur:900000}, {...task,dur:501000}, {...task,dur:201000}, metadata]}});
  trace.accept({method:'Tracing.tracingComplete',params:{dataLossOccurred:false}});
  const result=trace.finish(true);
  assert.equal(mainTaskMaximum(result),501);
  assert.equal(result.taskCount,200002);
  assert.equal(result.overLimitCount,2);
  assert.equal(result.tasks.length,1);
  assert.ok(JSON.stringify(result).length<1500);
  for(const patch of [{taskCount:0},{taskCount:1},{taskCount:1000001},
    {overLimitCount:-1},{overLimitCount:200003},{aggregation:'sampled'}, {receivedEvents:1}]) {
    assert.throws(()=>mainTaskMaximum({...result,...patch}));
  }
});
