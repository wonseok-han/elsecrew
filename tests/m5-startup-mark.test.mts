import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {markStartScreenReady} from '../src/m5/startup-mark.ts';

function fixture(markThrows=false) {
  let visible=true, disabled=false, hasButton=true, id=0;
  const frames=new Map<number,()=>void>(), listeners=new Set<()=>void>(), marks:string[]=[];
  const doc={get visibilityState(){return visible?'visible':'hidden';},
    querySelector:()=>hasButton?{disabled,getClientRects:()=>[{}]}:null,
    addEventListener:(_name:string,fn:()=>void)=>listeners.add(fn),
    removeEventListener:(_name:string,fn:()=>void)=>listeners.delete(fn)};
  const context=vm.createContext({document:doc,performance:{getEntriesByName:()=>marks,mark:(name:string)=>{if(markThrows)throw Error('measurement unavailable');marks.push(name);}},
    requestAnimationFrame:(fn:()=>void)=>{frames.set(++id,fn);return id;},cancelAnimationFrame:(n:number)=>frames.delete(n)});
  return {marks,frames,listeners,start:()=>vm.runInContext(`(${markStartScreenReady.toString()})()`,context),
    tick:()=>{const queued=[...frames.values()];frames.clear();queued.forEach(fn=>fn());},
    visibility:(value:boolean)=>{visible=value;listeners.forEach(fn=>fn());},
    disable:()=>{disabled=true;},missing:()=>{hasButton=false;}};
}
test('ready screen records once after two frames and releases observers',()=>{
  const f=fixture();f.start();f.tick();assert.equal(f.marks.length,0);f.tick();
  assert.deepEqual(f.marks,['m5:start-screen-ready']);assert.equal(f.listeners.size,0);
  f.start();assert.equal(f.frames.size,0);
});
test('hidden transition resets both frames; hidden startup waits',()=>{
  const f=fixture();f.visibility(false);f.start();assert.equal(f.frames.size,0);
  f.visibility(true);f.tick();f.visibility(false);f.tick();assert.equal(f.marks.length,0);
  f.visibility(true);f.tick();assert.equal(f.marks.length,0);f.tick();assert.equal(f.marks.length,1);
});
test('unmount, disabled or absent selection never mark ready',()=>{
  const f=fixture();const stop=f.start();f.tick();stop();f.tick();assert.equal(f.marks.length,0);
  for(const blocked of ['disable','missing'] as const){const g=fixture();g.start();g.tick();g[blocked]();g.tick();assert.equal(g.marks.length,0);}
});
test('measurement failure does not escape into the app and releases listeners',()=>{
  const f=fixture(true);f.start();f.tick();assert.doesNotThrow(()=>f.tick());
  assert.equal(f.listeners.size,0);assert.equal(f.frames.size,0);assert.equal(f.marks.length,0);
});
