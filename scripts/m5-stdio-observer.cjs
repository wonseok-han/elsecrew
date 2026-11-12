// Passive diagnostic for an owned fixture process. Never retain payloads or
// alter bytes, callbacks, backpressure, errors, or the production protocol.
const { StringDecoder } = require('node:string_decoder');
const { performance } = require('node:perf_hooks');
function observeStdio(child) {
  const report = { methods: {}, pendingPeak: 0, pendingAtStop: 0, dropped: 0, malformed: 0, oversized: 0 };
  const pending = new Map();
  function parseLines(onLine) {
    const decoder = new StringDecoder('utf8');
    let tail = '', skipping = false;
    return chunk => {
      const text = typeof chunk === 'string' ? chunk : decoder.write(chunk);
      const pieces = text.split('\n');
      for (let index = 0; index < pieces.length; index++) {
        if (!skipping) {
          tail += pieces[index];
          if (Buffer.byteLength(tail) > 1024 * 1024) { report.oversized++; tail = ''; skipping = true; }
        }
        if (index < pieces.length - 1) {
          if (!skipping && tail.trim()) {
            try { onLine(JSON.parse(tail)); } catch { report.malformed++; }
          }
          tail = ''; skipping = false;
        }
      }
    };
  }
  const sent = parseLines(value => {
    if (value.kind !== 'request' || typeof value.requestId !== 'string' || typeof value.method !== 'string') return;
    if (pending.size >= 512 || value.requestId.length > 256 || value.method.length > 128) { report.dropped++; return; }
    pending.set(value.requestId, { method: value.method, at: performance.now() });
    report.pendingPeak = Math.max(report.pendingPeak, pending.size);
  });
  const received = parseLines(value => {
    if (value.kind !== 'response') return;
    const request = pending.get(value.requestId);
    if (!request) return;
    pending.delete(value.requestId);
    let method = report.methods[request.method];
    if (!Object.hasOwn(report.methods, request.method)) {
      if (Object.keys(report.methods).length >= 64) { report.dropped++; return; }
      method = { count: 0, totalMs: 0, maximumMs: 0, samplesMs: [], droppedSamples: 0 };
      Object.defineProperty(report.methods, request.method, { value: method, enumerable: true });
    }
    const elapsed = performance.now() - request.at;
    method.count++; method.totalMs += elapsed; method.maximumMs = Math.max(method.maximumMs, elapsed);
    if (method.samplesMs.length < 2048) method.samplesMs.push(elapsed); else method.droppedSamples++;
  });
  const original = child.stdin.write;
  const wrapped = function(chunk, ...args) {
    sent(chunk);
    return Reflect.apply(original, this, [chunk, ...args]);
  };
  child.stdin.write = wrapped;
  child.stdout.on('data', received);
  return { report, stop() {
    if (child.stdin.write === wrapped) child.stdin.write = original;
    child.stdout.off('data', received);
    report.pendingAtStop = pending.size;
    pending.clear();
    return report;
  } };
}
module.exports = { observeStdio };
