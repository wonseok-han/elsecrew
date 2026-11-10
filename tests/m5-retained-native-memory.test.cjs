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
  assert.ok(!JSON.stringify(result).includes('private path'));
  await assert.rejects(collectRetainedNativeMemory([{role:'main',pid:-1}], async () => {throw new Error('must not execute');}));
});
