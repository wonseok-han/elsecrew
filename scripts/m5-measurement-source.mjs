import assert from 'node:assert/strict';

// Detect HEAD/status changes; this is not a content identity or freshness proof.
export function assertMeasurementSourceUnchanged(before, after) {
  assert.equal(typeof before.head, 'string');
  assert.ok(before.head.length > 0);
  assert.equal(typeof before.gitStatus, 'string');
  assert.equal(after.head, before.head, 'Source HEAD changed during measurement');
  assert.equal(after.gitStatus, before.gitStatus, 'Working tree changed during measurement');
}
