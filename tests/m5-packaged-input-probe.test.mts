import assert from 'node:assert/strict';
import test from 'node:test';
import { createContext, runInContext } from 'node:vm';
import { probePackagedInput } from '../scripts/m5-packaged-input-probe.mjs';

function fixture(frameMs = 16) {
  let now = 0;
  let text = '';
  let listener: ((event: unknown) => void) | undefined;
  let observer: { callback: () => void; connected: boolean } | undefined;
  let deadline: (() => void) | undefined;
  let frames = 0;
  const document = { activeElement: null as unknown,
    querySelector: () => input, querySelectorAll: () => [{ textContent: text }] };
  const input = { focus: () => { document.activeElement = input; }, closest: () => ({}),
    addEventListener: (_name: string, value: typeof listener) => { listener = value; },
    removeEventListener: () => { listener = undefined; } };
  const context = createContext({ document, performance: { now: () => now }, queueMicrotask,
    MutationObserver: class {
      constructor(callback: () => void) { observer = { callback, connected: false }; }
      observe() { observer!.connected = true; }
      disconnect() { observer!.connected = false; }
    },
    setTimeout: (callback: () => void) => { deadline = callback; return 1; },
    clearTimeout: () => { deadline = undefined; },
    requestAnimationFrame: (callback: () => void) => {
      frames++; queueMicrotask(() => { now += frameMs; callback(); }); return frames;
    }, cancelAnimationFrame() {},
  });
  return {
    evaluate: async (code: string) => runInContext(code, context),
    insert: async (marker: string) => {
      assert.ok(observer?.connected, 'observer precedes input dispatch');
      assert.ok(listener);
      // Visible text alone, and untrusted input, must not count as delivery.
      text = marker;
      observer.callback();
      listener({ target: input, isTrusted: false, data: marker });
      assert.equal(runInContext('globalThis.__m5PackagedInput.received', context), null);
      assert.equal(observer.connected, true);
      now += 1;
      listener({ target: input, isTrusted: true, data: marker });
      observer.callback();
    },
    expire: async () => { deadline!(); },
    assertClean: () => {
      assert.equal(observer?.connected, false);
      assert.equal(listener, undefined);
      assert.equal(deadline, undefined);
      assert.equal(runInContext('globalThis.__m5PackagedInput', context), undefined);
    },
    frameCount: () => frames,
  };
}

test('input probe observes before dispatch and retains trusted input plus two frames', async () => {
  const f = fixture();
  const result = await probePackagedInput(f.evaluate, f.insert);
  assert.equal(result.samples.length, 11);
  assert.equal(result.p95, 33);
  assert.equal(result.limitMs, 50);
  assert.equal(result.gateEligible, false);
  assert.equal(f.frameCount(), 22);
  f.assertClean();
});

test('input probe disconnects on dispatch failure and missing text deadline', async () => {
  for (const mode of ['dispatch', 'deadline']) {
    const f = fixture();
    await assert.rejects(probePackagedInput(f.evaluate, async () => {
      if (mode === 'dispatch') throw new Error('dispatch failed');
      await f.expire();
    }), mode === 'dispatch' ? /dispatch failed/ : /rendered text missing/);
    f.assertClean();
  }
});

test('input probe still rejects an observed latency above the unchanged limit', async () => {
  const f = fixture(30);
  const result = await probePackagedInput(f.evaluate, f.insert);
  assert.equal(result.p95, 61);
  assert.equal(result.withinLimit, false);
  f.assertClean();
});
