import test from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {startMainTaskSession} from '../scripts/m5-main-task-session.mjs';
class DebuggerStub extends EventEmitter {
  attached=false; detached=0; failure='';
  attach() { if(this.attached) throw new Error('Already attached'); this.attached=true; }
  isAttached() { return this.attached; }
  detach() { this.detached++;this.attached=false; }
  async sendCommand(method:string) {
    if(this.failure===method) throw new Error('Command failed');
    if(method==='Tracing.end') {
      this.emit('message',{},'Tracing.dataCollected',{value:[
        {pid:7,tid:1,ph:'M',name:'thread_name',args:{name:'CrBrowserMain'}},
        {pid:7,tid:1,ph:'X',cat:'toplevel',ts:0,dur:250000},
      ]});
      this.emit('message',{},'Tracing.tracingComplete',{dataLossOccurred:false});
    }
  }
}
test('session preserves task timing and detaches on completion',async()=>{
  const api=new DebuggerStub();const session=await startMainTaskSession(api,7);
  const result=await session.stop(true);
  assert.equal(result.observation.maximumMs,250);assert.equal(result.observation.windowCompleted,true);
  assert.equal(api.detached,1);assert.equal(api.listenerCount('message'),0);
  await assert.rejects(()=>session.stop(true));
});
test('start failure releases only the newly owned debugger',async()=>{
  const api=new DebuggerStub();api.failure='Tracing.start';
  await assert.rejects(()=>startMainTaskSession(api,7));
  assert.equal(api.detached,1);assert.equal(api.listenerCount('message'),0);
  api.attached=true;await assert.rejects(()=>startMainTaskSession(api,7));
  assert.equal(api.detached,1);assert.equal(api.attached,true);
});
test('end failure keeps incomplete evidence and releases the debugger',async()=>{
  const api=new DebuggerStub();const session=await startMainTaskSession(api,7);api.failure='Tracing.end';
  const result=await session.stop(true);
  assert.match(result.error!,/Command failed/);assert.equal(result.observation.windowCompleted,false);
  assert.equal(result.observation.valid,false);assert.equal(api.detached,1);assert.equal(api.listenerCount('message'),0);
});
