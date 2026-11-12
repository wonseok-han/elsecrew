import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { cleanupCompletedNativeFixture } from '../scripts/m5-native-fixture-cleanup.mjs';

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'elsecrew-project-ui-'));
  fs.mkdirSync(path.join(root, 'project'));
  fs.mkdirSync(path.join(root, 'userdata'));
  fs.writeFileSync(path.join(root, 'project', 'hello.txt'), 'synthetic');
  fs.writeFileSync(path.join(root, 'project.png'), 'evidence');
  const result = { root, status: 'PASS', shutdown: 'accepted' };
  fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(result));
  return { root, sample: { code: 0, signal: null, result }, dispose: () => fs.rmSync(root, { recursive: true, force: true }) };
}

test('native fixture cleanup preserves evidence and removes only disposable children', () => {
  const f = fixture();
  try {
    assert.deepEqual(cleanupCompletedNativeFixture(f.sample).removed, ['project', 'userdata']);
    assert.equal(fs.existsSync(path.join(f.root, 'project')), false);
    assert.equal(fs.existsSync(path.join(f.root, 'userdata')), false);
    assert.equal(fs.readFileSync(path.join(f.root, 'project.png'), 'utf8'), 'evidence');
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(f.root, 'result.json'), 'utf8')), f.sample.result);
  } finally { f.dispose(); }
});

test('native cleanup refuses pending shutdown, signal, missing close, and conflicting evidence', () => {
  const f = fixture();
  try {
    for (const sample of [
      { ...f.sample, code: null }, { ...f.sample, signal: 'SIGTERM' },
      { ...f.sample, result: { ...f.sample.result, shutdown: 'pending' } },
      { ...f.sample, code: 1, result: { ...f.sample.result, status: 'FAIL' } },
      { ...f.sample, result: { ...f.sample.result, root: os.tmpdir() } },
    ]) assert.throws(() => cleanupCompletedNativeFixture(sample));
    assert.equal(fs.readFileSync(path.join(f.root, 'project', 'hello.txt'), 'utf8'), 'synthetic');
  } finally { f.dispose(); }
});

test('native cleanup refuses a linked child before deleting the other child', () => {
  const f = fixture();
  try {
    fs.rmdirSync(path.join(f.root, 'userdata'));
    fs.symlinkSync(path.join(f.root, 'project'), path.join(f.root, 'userdata'), 'dir');
    assert.throws(() => cleanupCompletedNativeFixture(f.sample));
    assert.equal(fs.readFileSync(path.join(f.root, 'project', 'hello.txt'), 'utf8'), 'synthetic');
  } finally { f.dispose(); }
});
