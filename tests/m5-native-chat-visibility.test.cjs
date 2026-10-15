const test = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');

async function probe(initial) {
  const {startPackagedChatStream} = await import('../scripts/m5-packaged-chat-stream-probe.mjs');
  const listeners = new Set(), timers = [];
  const document = {
    visibilityState: initial,
    addEventListener: (_name, listener) => listeners.add(listener),
    removeEventListener: (_name, listener) => listeners.delete(listener),
    querySelector: () => null,
  };
  const context = vm.createContext({document, performance,
    window: {m5: new Proxy({}, {get() { throw new Error('No bridge call expected'); }})},
    setTimeout: callback => timers.push(callback), cancelAnimationFrame() {},
  });
  vm.runInContext(`(${startPackagedChatStream.toString()})('project_fixture')`, context);
  return {context, document, listeners, timers};
}

test('native chat probe rejects hidden startup without a bridge call', async () => {
  const {context, listeners} = await probe('hidden');
  assert.equal(context.__m5NativeChatStream.status, 'FAIL');
  assert.equal(context.__m5NativeChatStream.visibilityFailed, true);
  assert.equal(context.__m5NativeChatStream.sent, 0);
  assert.equal(listeners.size, 0);
});

test('native chat probe stops UI preparation after losing visibility', async () => {
  const {context, document, listeners, timers} = await probe('visible');
  assert.equal(context.__m5NativeChatStream.status, 'RUNNING');
  document.visibilityState = 'hidden';
  for (const listener of listeners) listener();
  for (const callback of timers.splice(0)) callback();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(context.__m5NativeChatStream.status, 'FAIL');
  assert.match(context.__m5NativeChatStream.error, /window became hidden/);
  assert.equal(context.__m5NativeChatStream.sent, 0);
  assert.equal(listeners.size, 0);
});
