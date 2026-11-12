// Diagnostic native map aggregation. No raw maps, addresses, or paths retained.
const assert=require('node:assert/strict');
function summarizeSurfaces(text) {
  assert.ok(typeof text==='string' && Buffer.byteLength(text)<=4*1024*1024,'Native map too large');
  assert.match(text,/\[\s*VSIZE\s+RSDNT\s+DIRTY\s+SWAP\]/,'Unknown native map columns');
  const groups=new Map();let metadataCount=0,metadataResidentBytes=0,regionCount=0;
  const bytes=value=>{
    const m=/^(\d+(?:\.\d+)?)([BKMG])$/.exec(value);assert.ok(m,'Unknown native map size');
    const result=Math.round(Number(m[1])*1024**'BKMG'.indexOf(m[2]));
    assert.ok(Number.isSafeInteger(result),'Native map size overflow');return result;
  };
  for(const line of text.split('\n')) {
    if(!/^IOSurface\s+[0-9a-f]+-/i.test(line)) continue;
    assert.ok(++regionCount<=2048,'Too many native surfaces');
    const row=/^IOSurface\s+[0-9a-f]+-[0-9a-f]+\s+\[\s*(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s*\]/i.exec(line);
    assert.ok(row,'Malformed surface row');const resident=bytes(row[2]);
    const shape=/SurfaceID:\s+0x[0-9a-f]+\s+(\d+)x(\d+)\s+\(([A-Za-z0-9_]{1,16})\)/.exec(line);
    if(!shape) {assert.ok(!line.includes('SurfaceID:'),'Unknown surface dimensions');metadataCount++;metadataResidentBytes+=resident;continue;}
    const width=Number(shape[1]),height=Number(shape[2]),format=shape[3];
    assert.ok(width>0 && height>0 && width<=32768 && height<=32768,'Invalid surface dimensions');
    const key=`${width}x${height}:${format}`;
    if(!groups.has(key)) {assert.ok(groups.size<128,'Too many surface groups');groups.set(key,{width,height,format,count:0,residentBytes:0});}
    const group=groups.get(key);group.count++;group.residentBytes+=resident;
  }
  assert.ok(regionCount>0 && groups.size>0,'No dimensioned IOSurfaces observed');
  return {regionCount,metadataCount,metadataResidentBytes,groups:[...groups.values()]};
}
async function collectSurfaceLifetime(rootPid,getGpuPid,observeWindow) {
  const {execFile}=require('node:child_process');const {promisify}=require('node:util');
  const {collectProcessTreeMemory}=require('./m5-process-memory.cjs');
  const gpuPid=getGpuPid(),startedAt=performance.now(),samples=[];
  for(let index=0;index<12;index++) {
    assert.equal(getGpuPid(),gpuPid,'GPU process changed');
    const sampleStartedAt=performance.now();
    const {stdout}=await promisify(execFile)('/usr/bin/vmmap',['-wide',String(gpuPid)],{encoding:'utf8',timeout:2000,maxBuffer:4*1024*1024});
    const surfaces=summarizeSurfaces(stdout);
    const memory=await collectProcessTreeMemory(rootPid);
    assert.equal(getGpuPid(),gpuPid,'GPU process changed');
    const gpu=memory.processes.find(row=>row.pid===gpuPid);assert.ok(gpu,'GPU missing from process tree');
    samples.push({elapsedMs:sampleStartedAt-startedAt,collectionMs:performance.now()-sampleStartedAt,
      window:await observeWindow(),gpuRssBytes:gpu.rssBytes,treeRssBytes:memory.rssBytes,surfaces});
    if(index<11) await new Promise(resolve=>setTimeout(resolve,Math.max(0,startedAt+(index+1)*1000-performance.now())));
  }
  return {diagnostic:true,gateEligible:false,gpuPid,samples,
    scope:'Native region residency is not RSS or additive physical usage. vmmap observer overhead remains; no forced GC.'};
}
module.exports={summarizeSurfaces,collectSurfaceLifetime};
