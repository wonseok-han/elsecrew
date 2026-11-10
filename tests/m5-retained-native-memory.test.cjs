const { test } = require('node:test');
const assert = require('node:assert/strict');
const { summarizeRegions, collectRetainedNativeMemory } = require('../scripts/m5-retained-native-memory.cjs');
const map = `REGION TYPE START - END [ VSIZE RSDNT DIRTY SWAP ]
MALLOC 1000-2000 [ 2M 1M 512K 0K ] rw-/rwx /private/example
MALLOC 2000-3000 [ 1M 256K 128K 0K ] rw-/rwx
__TEXT 3000-4000 [ 4M 2M 0K 0K ] r-x/r-x /secret/path
`;
test('aggregates native classes without preserving addresses or paths', () => {
  const result = summarizeRegions(map);
  assert.equal(result.regions, 3);
  assert.deepEqual(result.groups[0], {type:'MALLOC',count:2,virtualBytes:3145728,residentBytes:1310720,dirtyBytes:655360,swappedBytes:0});
  assert.ok(!JSON.stringify(result).includes('/secret'));
  assert.ok(!JSON.stringify(result).includes('1000-2000'));
  assert.throws(() => summarizeRegions('unrecognized format'));
  assert.throws(() => summarizeRegions(map.replace('256K','NaNK')));
});
test('bounds sequential native probes and preserves failure without passing it', async () => {
  let calls = 0;
  const result = await collectRetainedNativeMemory([{role:'main',pid:100},{role:'renderer',pid:101}], async (file,args,options) => {
    assert.equal(file,'/usr/bin/vmmap');assert.deepEqual(args,['-wide',String(100+calls)]);
    assert.equal(options.timeout,5000);assert.equal(options.maxBuffer,4194304);
    if (++calls === 2) throw new Error('private path must not escape');
    return {stdout:map};
  });
  assert.equal(calls,2);assert.equal(result.gateEligible,false);
  assert.equal(result.processes[0].regions,3);
  assert.equal(result.processes[1].error,'nativeMapUnavailableOrInvalid');
  assert.equal(result.processes[1].stage,'execute');
  assert.ok(!JSON.stringify(result).includes('private path'));
  await assert.rejects(collectRetainedNativeMemory([{role:'main',pid:-1}], async () => {throw new Error('must not execute');}));
});

test('distinguishes parse rejection from permission failure without private messages', async () => {
  const parsed = await collectRetainedNativeMemory([{role:'main',pid:100}], async () => ({stdout:'unknown format'}));
  assert.equal(parsed.processes[0].stage,'parse');
  const denied = await collectRetainedNativeMemory([{role:'main',pid:100}], async () => {throw Object.assign(new Error('/private/path'), {code:'EPERM'});});
  assert.equal(denied.processes[0].stage,'execute');
  assert.equal(denied.processes[0].permissionDenied,true);
  assert.ok(!JSON.stringify(denied).includes('/private/path'));
});

test('native unitless region sizes are integer bytes', () => {
  const result = summarizeRegions(map.replace('2M 1M 512K 0K','2944 184 64 0'));
  assert.equal(result.groups[0].virtualBytes,2944+1048576);
  assert.equal(result.groups[0].residentBytes,184+262144);
  assert.throws(() => summarizeRegions(map.replace('2M','2.5')));
});

test('Electron terabyte virtual reservations remain distinct from resident bytes', () => {
  const result = summarizeRegions(map.replace('2M 1M 512K 0K','1.3T 0K 0K 0K'));
  assert.equal(result.groups[0].virtualBytes,Math.round(1.3*1024**4)+1048576);
  assert.equal(result.groups[0].residentBytes,262144);
});
