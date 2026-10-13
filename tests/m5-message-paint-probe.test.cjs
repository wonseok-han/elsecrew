const test = require('node:test');
const assert = require('node:assert/strict');
const { measureMessagePaint } = require('../scripts/m5-message-paint-probe.cjs');

test('message paint probe retains per-message observations and diagnostic limits', async () => {
  const raf = globalThis.requestAnimationFrame, cancel = globalThis.cancelAnimationFrame;
  globalThis.requestAnimationFrame = callback => setTimeout(callback, 1);
  globalThis.cancelAnimationFrame = clearTimeout;
  try {
    const ids = [];
    const report = await measureMessagePaint({ count: 3, rate: 50, emit: () => { const id = `message-${ids.length}`; ids.push(id); return id; }, visible: () => ids });
    assert.equal(report.sent, 3);
    assert.equal(report.observed, 3);
    assert.deepEqual(report.unobserved, []);
    assert.equal(report.diagnostic, true);
    assert.equal(report.gateEligible, false);
    assert.equal(report.samples.length, 3);
    assert.equal(report.maxPaintMs, Math.max(...report.samples.map(sample => sample.paintMs)));
    assert.ok(report.samples.every(sample => sample.paintMs >= 0 && sample.schedulingDelayMs >= 0));
    await assert.rejects(measureMessagePaint({ count: 2, rate: 50, emit: () => 'duplicate', visible: () => ['duplicate'] }), /duplicate/);
    await assert.rejects(measureMessagePaint({ count: 1, emit: () => { throw new Error('emit failed'); }, visible: () => [] }), /emit failed/);
    await assert.rejects(measureMessagePaint({ count: 0 }), /invalid/);
  } finally {
    if (raf) globalThis.requestAnimationFrame = raf; else delete globalThis.requestAnimationFrame;
    if (cancel) globalThis.cancelAnimationFrame = cancel; else delete globalThis.cancelAnimationFrame;
  }
});
