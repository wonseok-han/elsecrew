const test = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const {startAgentPaintProbe} = require('../scripts/m5-agent-paint-probe.cjs');

function harness() {
  let listener, mutation, visibility, disconnected = false, unsubscribed = false, time = 0, next = 0;
  const frames = new Map(), rows = [];
  const document = {visibilityState:'visible', body:{}, querySelectorAll:() => rows,
    addEventListener:(_name, fn) => { visibility = fn; }, removeEventListener:() => { visibility = null; }};
  const context = vm.createContext({document, performance:{now:() => time},
    requestAnimationFrame:fn => { frames.set(++next, fn); return next; }, cancelAnimationFrame:id => frames.delete(id),
    MutationObserver:class { constructor(fn) { mutation = fn; } observe() {} disconnect() { disconnected = true; } },
    window:{m5:{onAgentPublicEvent:fn => { listener = fn; return () => { unsubscribed = true; }; }}},
  });
  vm.runInContext(`(${startAgentPaintProbe.toString()})('chat')`, context);
  return {
    emit:(id = 'event', kind = 'final') => listener({eventId:id, runId:'run', chatId:'chat', kind, text:'must not be retained'}),
    row() { const row = {dataset:{chatId:'chat',runId:'run'},isConnected:true,getClientRects:() => [1]}; rows.push(row); mutation(); return row; },
    frame() { time += 16; const callbacks = [...frames.values()]; frames.clear(); callbacks.forEach(fn => fn()); },
    hide() { document.visibilityState = 'hidden'; visibility(); },
    stop:() => context.__m5AgentPaintProbe.stop('run'),
    cleaned:() => disconnected && unsubscribed && visibility === null && frames.size === 0 && !context.__m5AgentPaintProbe,
  };
}

test('passive actual final observer correlates ordered durable rows and releases all resources', () => {
  const h = harness(); h.emit('ignored', 'progress'); h.emit(); h.row(); h.frame(); h.frame();
  const result = h.stop();
  assert.equal(result.status, 'OBSERVED'); assert.equal(result.expected, 1);
  assert.equal(result.samples[0].notificationToFrameMs, 32);
  assert.equal(result.gateEligible, false);
  assert.ok(!JSON.stringify(result).includes('must not be retained'));
  assert.ok(h.cleaned());
});

test('missing, hidden, removed, and duplicate rows/events never produce complete observations', () => {
  for (const mode of ['missing', 'hidden', 'removed', 'duplicate']) {
    const h = harness(); h.emit();
    if (mode !== 'missing') { const row = h.row(); if (mode === 'removed') row.isConnected = false; }
    if (mode === 'hidden') h.hide();
    if (mode === 'duplicate') h.emit();
    h.frame(); h.frame();
    assert.equal(h.stop().status, 'INCOMPLETE', mode); assert.ok(h.cleaned());
  }
});
