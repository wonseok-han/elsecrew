import test from 'node:test';
import assert from 'node:assert/strict';
import {resolve,join} from 'node:path';
import { assertPackageNotRunning } from '../scripts/m5-package-live-guard.mjs';
const directory = resolve('/tmp/elsecrew-package-guard-absent');
const inventory = stdout => ({platform:'darwin',run:()=>({status:0,stdout})});
test('blocks root and helper executables even when the bundle is already missing', () => {
  for (const executable of ['Elsecrew.app/Contents/MacOS/Elsecrew', 'Elsecrew.app/Contents/Resources/sidecar/elsecrew-sidecar']) {
    assert.throws(() => assertPackageNotRunning(directory, inventory(` 123 ${join(directory,executable)}\n`)), /PID\(s\): 123/);
  }
});
test('does not match siblings or unrelated executable arguments', () => {
  assert.doesNotThrow(() => assertPackageNotRunning(directory, inventory(`12 ${directory}-other/app\n13 /bin/ps\n`)));
});
test('inventory failures cannot silently authorize replacement', () => {
  for (const response of [{status:0,stdout:''},{status:1,stdout:''},{status:0,stdout:'unparseable'}, {error:new Error('denied')}]) {
    assert.throws(() => assertPackageNotRunning(directory, {platform:'darwin',run:()=>response}));
  }
});
