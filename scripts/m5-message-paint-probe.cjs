// Serialized into the isolated test renderer; no product bridge or clock changes.
async function measureMessagePaint({ emit, visible, count = 3000, rate = 50 }) {
  if (!Number.isSafeInteger(count) || count < 1 || count > 3000 || !Number.isSafeInteger(rate) || rate < 1 || rate > 50) throw new Error('invalid message paint workload');
  const samples = [];
  const pending = new Map();
  const seen = new Set();
  const startedAt = performance.now();
  const intervalMs = 1000 / rate;
  let sent = 0;
  let timer;
  let frame;
  let deadline;
  let observationError;
  let stopped = false;
  await new Promise((resolve, reject) => {
    const stop = error => {
      if (stopped) return;
      stopped = true;
      clearTimeout(timer); clearTimeout(deadline); cancelAnimationFrame(frame);
      if (error) reject(error); else resolve();
    };
    const send = () => {
      try {
        const sentAt = performance.now();
        const id = emit();
        if (typeof id !== 'string' || seen.has(id)) throw new Error('missing or duplicate emitted message identity');
        seen.add(id);
        pending.set(id, { id, sentAt, schedulingDelayMs: Math.max(0, sentAt - (startedAt + sent * intervalMs)), frames: 0 });
        sent++;
        if (sent < count) timer = setTimeout(send, Math.max(0, startedAt + sent * intervalMs - performance.now()));
      } catch (error) { stop(error); }
    };
    const observe = () => {
      try {
        const ids = new Set(visible());
        for (const [id, sample] of pending) {
          if (!ids.has(id)) { sample.frames = 0; continue; }
          if (++sample.frames < 2) continue;
          samples.push({ id, sentAtMs: sample.sentAt - startedAt, paintMs: performance.now() - sample.sentAt, schedulingDelayMs: sample.schedulingDelayMs });
          pending.delete(id);
        }
        if (sent === count && pending.size === 0) { stop(); return; }
        frame = requestAnimationFrame(observe);
      } catch (error) { stop(error); }
    };
    deadline = setTimeout(() => { observationError = 'message observation deadline exceeded'; stop(); }, count * intervalMs + 10000);
    send();
    if (!stopped) frame = requestAnimationFrame(observe);
  });
  return {
    diagnostic: true, gateEligible: false, sent, observed: samples.length,
    unobserved: [...pending.keys()], elapsedMs: performance.now() - startedAt,
    maxPaintMs: samples.length ? Math.max(...samples.map(sample => sample.paintMs)) : null,
    observationError, samples,
    limitations: ['Synthetic single-chat bridge only; two animation frames are a presentation proxy, not a compositor timestamp.', 'Does not cover four-chat isolation, native sidecar transport, packaged startup, or the complete G6 gate.'],
  };
}
module.exports = { measureMessagePaint };
