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

class MarkerStub extends DebuggerStub {
  mode='valid';
  async sendCommand(method:string,params?:{syncId?:string}) {
    if(method==='Tracing.recordClockSyncMarker') {
      if(this.mode==='unsupported') throw new Error('Unsupported');
      if(this.mode==='missing') return;
      const marker={pid:7,tid:1,ph:'c',name:'clock_sync',ts:params?.syncId?.endsWith('start') ? 100 : 200,args:{sync_id:params?.syncId}};
      this.emit('message',{},'Tracing.dataCollected',{value:this.mode==='duplicate' ? [marker,marker] : [marker]});
    } else await super.sendCommand(method);
  }
}
test('clock boundaries preserve full-interval long task and bounded diagnostics',async()=>{
  for(const mode of ['valid','duplicate','missing','unsupported']) {
    const api=new MarkerStub();api.mode=mode;
    const session=await startMainTaskSession(api,7,{clockMarkers:true});
    const result=await session.stop(true);
    assert.equal(result.observation.maximumMs,250);
    assert.equal(result.observation.valid,true);
    assert.equal(result.clockBoundaries!.complete,mode==='valid');
    assert.ok(result.clockBoundaries!.markers.length<=2);
    assert.equal(api.detached,1);assert.equal(api.listenerCount('message'),0);
  }
});
test('aborted scenario cannot claim complete boundaries',async()=>{
  const api=new MarkerStub();const session=await startMainTaskSession(api,7,{clockMarkers:true});
  assert.equal((await session.stop(false)).clockBoundaries!.complete,false);
});
