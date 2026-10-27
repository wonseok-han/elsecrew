const test = require('node:test');
const assert = require('node:assert/strict');
const { createLiveOutputLatency } = require('../scripts/m5-live-output-latency.cjs');

test('live latency excludes selection catch-up and measures only matching new heads', () => {
  const tracker = createLiveOutputLatency();
  const epoch = tracker.select('a');
  tracker.receive('a', 80, 1);
  tracker.frame('a', 30, 100, epoch);
  tracker.receive('a', 81, 101);
  tracker.frame('a', 81, 500, epoch);
  assert.deepEqual(tracker.snapshot().samples, []);
  tracker.receive('a', 82, 510);
  tracker.frame('a', 82, 525, epoch);
  assert.equal(tracker.snapshot().samples[0].notificationToFrameMs, 15);
});

test('switching or hiding invalidates queued frames and requires catching up again', () => {
  const tracker = createLiveOutputLatency();
  const old = tracker.select('a');
  tracker.receive('a', 1, 1); tracker.frame('a', 1, 2, old);
  tracker.receive('a', 2, 3);
  tracker.select(null);
  const current = tracker.select('a');
  tracker.frame('a', 2, 4, old);
  tracker.receive('b', 999, 5);
  tracker.receive('a', 3, 5); tracker.frame('a', 3, 6, current);
  assert.equal(tracker.snapshot().interrupted, 1);
  assert.equal(tracker.snapshot().samples.length, 0);
  tracker.receive('a', 4, 7); tracker.frame('a', 4, 10, current);
  assert.equal(tracker.snapshot().samples.length, 1);
});

test('coalesced heads are reported as superseded, never given fabricated paint times', () => {
  const tracker = createLiveOutputLatency(); const epoch = tracker.select('a');
  tracker.receive('a', 1, 1); tracker.frame('a', 1, 2, epoch);
  tracker.receive('a', 2, 3); tracker.receive('a', 3, 4);
  tracker.receive('a', 3, 100); // Duplicate must not reset the original clock.
  tracker.frame('a', 3, 8, epoch);
  assert.equal(tracker.snapshot().superseded, 1);
  assert.equal(tracker.snapshot().samples[0].notificationToFrameMs, 4);
});

test('pending and retained diagnostics are bounded and losses remain explicit', () => {
  const tracker = createLiveOutputLatency(); const epoch = tracker.select('a');
  tracker.receive('a', 1, 0); tracker.frame('a', 1, 1, epoch);
  for (let n = 2; n < 302; n++) tracker.receive('a', n, n);
  assert.equal(tracker.snapshot().unfinished, 256);
  assert.equal(tracker.snapshot().dropped, 44);
  tracker.frame('a', 301, 400, epoch);
  for (let n = 302; n < 1402; n++) { tracker.receive('a', n, n); tracker.frame('a', n, n + 1, epoch); }
  assert.equal(tracker.snapshot().samples.length, 1024);
  assert.ok(tracker.snapshot().dropped > 44);
});
