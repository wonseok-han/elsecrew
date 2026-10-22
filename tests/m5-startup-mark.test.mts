import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFileSync} from 'node:fs';
import {markStartScreenReady} from '../src/m5/startup-mark.ts';

function fixture(markThrows=false) {
  let visible=true, disabled=false, hasButton=true, id=0;
  const frames=new Map<number,()=>void>(), listeners=new Set<()=>void>(), marks:string[]=[], logs:string[]=[];
  const doc={get visibilityState(){return visible?'visible':'hidden';},
    querySelector:()=>hasButton?{disabled,getClientRects:()=>[{}]}:null,
    addEventListener:(_name:string,fn:()=>void)=>listeners.add(fn),
    removeEventListener:(_name:string,fn:()=>void)=>listeners.delete(fn)};
  const context=vm.createContext({console:{info:(message:string)=>logs.push(message)},document:doc,performance:{getEntriesByName:()=>marks,mark:(name:string)=>{if(markThrows)throw Error('measurement unavailable');marks.push(name);}},
    requestAnimationFrame:(fn:()=>void)=>{frames.set(++id,fn);return id;},cancelAnimationFrame:(n:number)=>frames.delete(n)});
  return {marks,logs,frames,listeners,start:()=>vm.runInContext(`(${markStartScreenReady.toString()})()`,context),
    tick:()=>{const queued=[...frames.values()];frames.clear();queued.forEach(fn=>fn());},
    visibility:(value:boolean)=>{visible=value;listeners.forEach(fn=>fn());},
    disable:()=>{disabled=true;},missing:()=>{hasButton=false;}};
}
test('ready screen records once after two frames and releases observers',()=>{
  const f=fixture();f.start();f.tick();assert.equal(f.marks.length,0);f.tick();
  assert.deepEqual(f.marks,['m5:start-screen-ready']);assert.equal(f.listeners.size,0);
  f.start();assert.equal(f.frames.size,0);
  assert.deepEqual(f.logs,['m5:start-screen-ready']);
});
test('hidden transition resets both frames; hidden startup waits',()=>{
  const f=fixture();f.visibility(false);f.start();assert.equal(f.frames.size,0);
  f.visibility(true);f.tick();f.visibility(false);f.tick();assert.equal(f.marks.length,0);
  assert.equal(f.logs.length,0);
  f.visibility(true);f.tick();assert.equal(f.marks.length,0);f.tick();assert.equal(f.marks.length,1);
});
test('unmount, disabled or absent selection never mark ready',()=>{
  const f=fixture();const stop=f.start();f.tick();stop();f.tick();assert.equal(f.marks.length,0);
  for(const blocked of ['disable','missing'] as const){const g=fixture();g.start();g.tick();g[blocked]();g.tick();assert.equal(g.marks.length,0);assert.equal(g.logs.length,0);}
});
test('measurement failure does not escape into the app and releases listeners',()=>{
  const f=fixture(true);f.start();f.tick();assert.doesNotThrow(()=>f.tick());
  assert.equal(f.listeners.size,0);assert.equal(f.frames.size,0);assert.equal(f.marks.length,0);
  assert.equal(f.logs.length,0);
});

test('main forwards only the fixed marker from its allowed main frame once',()=>{
  const source=readFileSync(new URL('../electron/main.ts',import.meta.url),'utf8');
  const start=source.indexOf('  let startupReported = false;');
  const end=source.indexOf('  window.on("closed"',start);
  assert.ok(start>=0 && end>start);
  let handler:(event:{message:string;frame:{url:string}})=>void=()=>assert.fail('missing handler');
  const frame={url:'app://allowed'}, logs:string[]=[];
  vm.runInNewContext(source.slice(start,end),{
    window:{webContents:{mainFrame:frame,on:(name:string,callback:typeof handler)=>{assert.equal(name,'console-message');handler=callback;}}},
    isAllowedNavigation:(url:string)=>url==='app://allowed',
    console:{info:(message:string)=>logs.push(message)},
  });
  handler({message:'private renderer message',frame});
  handler({message:'m5:start-screen-ready',frame:{url:'app://allowed'}});
  frame.url='https://untrusted.invalid';handler({message:'m5:start-screen-ready',frame});
  assert.deepEqual(logs,[]);
  frame.url='app://allowed';handler({message:'m5:start-screen-ready',frame});
  handler({message:'m5:start-screen-ready',frame});
  assert.deepEqual(logs,['m5:start-screen-ready']);
});
