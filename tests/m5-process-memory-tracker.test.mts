import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const { createProcessMemoryTracker } = createRequire(import.meta.url)('../scripts/m5-process-memory.cjs');
const row = (pid:number, parent:number, rss:number, second = '00') =>
  `${pid} ${parent} ${rss} Mon Oct 19 10:00:${second} 2026`;

test('observed children remain counted after reparenting, with their new children', () => {
  const read = createProcessMemoryTracker(10);
  assert.equal(read([row(10,1,100), row(20,10,200), row(99,1,999)].join('\n')).rssBytes, 300 * 1024);
  const next = read([row(10,1,100), row(20,1,200), row(30,20,300), row(99,1,999)].join('\n'));
  assert.equal(next.rssBytes, 600 * 1024);
  assert.deepEqual(next.processes.map(p => [p.pid,p.parentPid]), [[10,1],[20,1],[30,20]]);
  assert.equal(next.identityResolution, 'ps-lstart-seconds');
});

test('unrelated reused PID and its descendants are excluded', () => {
  const read = createProcessMemoryTracker(10);
  read([row(10,1,100), row(20,10,200)].join('\n'));
  const next = read([row(10,1,100), row(20,1,999,'01'), row(30,20,999)].join('\n'));
  assert.deepEqual(next.processes.map(p => p.pid), [10]);
  assert.equal(next.rssBytes, 100 * 1024);
});

test('missing identities retire and cannot return as unrelated processes', () => {
  const read = createProcessMemoryTracker(10);
  read([row(10,1,100), row(20,10,200)].join('\n'));
  read(row(10,1,100));
  assert.deepEqual(read([row(10,1,100), row(20,1,200)].join('\n')).processes.map(p => p.pid), [10]);
});

test('missing/reused root and malformed samples fail rather than produce zero', () => {
  for (const invalid of ['', row(20,1,10), row(10,1,100,'01'), row(10,1,100)+'\n'+row(10,1,100), '10 1 NaN invalid']) {
    const read = createProcessMemoryTracker(10);
    read(row(10,1,100));
    assert.throws(() => read(invalid));
  }
});
