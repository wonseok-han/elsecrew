// Serialized into the development renderer. Observes only public output heads
// and rendered rows; this does not measure provider generation or durable commit time.
function installOutputObserver(createLiveOutputLatency) {
  const latency = createLiveOutputLatency();
  const result = { heads: [], frames: [], visibility: [], droppedHeads: 0, droppedFrames: 0 };
  const latest = new Map();
  const frames = new Set();
  let stopped = false;
  let lastKey = '';
  const selection = () => latency.select(document.visibilityState === 'visible' ? document.querySelector('.m5-durable-output select')?.value ?? null : null);
  const visibility = () => {
    selection();
    if (result.visibility.length < 100) result.visibility.push({ state: document.visibilityState, at: performance.now() });
  };
  visibility();
  document.addEventListener('visibilitychange', visibility);
  const off = window.m5.onAgentOutputAvailable(head => {
    const sample = { runId: head.binding.runId, sequence: head.committedSequence, sealed: head.sealed, receivedAt: performance.now(), receivedUnixMs: performance.timeOrigin + performance.now() };
    selection();
    latency.receive(sample.runId, sample.sequence, sample.receivedAt);
    latest.set(sample.runId, sample);
    if (result.heads.length < 4096) result.heads.push(sample); else result.droppedHeads++;
    queueMicrotask(() => observe());
  });
  const frame = callback => {
    const id = requestAnimationFrame(() => { frames.delete(id); callback(); });
    frames.add(id);
  };
  const visible = node => {
    if (!node || document.visibilityState !== 'visible') return false;
    const r = node.getBoundingClientRect();
    let top = Math.max(0, r.top), bottom = Math.min(innerHeight, r.bottom);
    let left = Math.max(0, r.left), right = Math.min(innerWidth, r.right);
    for (let parent = node.parentElement; parent; parent = parent.parentElement) {
      const style = getComputedStyle(parent), box = parent.getBoundingClientRect();
      if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowY)) { top = Math.max(top, box.top); bottom = Math.min(bottom, box.bottom); }
      if (['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowX)) { left = Math.max(left, box.left); right = Math.min(right, box.right); }
    }
    return r.width > 0 && r.height > 0 && bottom > top && right > left;
  };
  const observe = () => {
    if (stopped) return;
    const capturedEpoch = selection();
    const runId = document.querySelector('.m5-durable-output select')?.value;
    const nodes = [...document.querySelectorAll('.m5-output-scroll article pre')];
    const node = nodes.reverse().find(visible);
    if (!node || !runId) { latency.select(null); return; }
    const text = node.textContent;
    const match = /^output (\d+)(?: \[emitted_us=(\d+),unix_ms=(\d+)\])?$/.exec(text);
    if (!match) return;
    const sequence = Number(match[1]), key = `${capturedEpoch}:${runId}:${sequence}:${latest.get(runId)?.sequence ?? 0}`;
    if (key === lastKey) return;
    lastKey = key;
    const head = latest.get(runId), observedAt = performance.now();
    // Two RAFs are a rendering opportunity, not a physical compositor timestamp.
    frame(() => frame(() => {
      if (stopped || !node.isConnected || !visible(node)
        || document.querySelector('.m5-durable-output select')?.value !== runId
        || node.textContent !== text) return;
      const sample = { runId, sequence, observedAt, frameAt: performance.now(),
        emittedUs: match[2] ? Number(match[2]) : null, emittedUnixMs: match[3] ? Number(match[3]) : null,
        frameUnixMs: performance.timeOrigin + performance.now(), headSequence: head?.sequence ?? null, headReceivedAt: head?.receivedAt ?? null };
      latency.frame(runId, sequence, sample.frameAt, capturedEpoch);
      if (result.frames.length < 1024) result.frames.push(sample); else result.droppedFrames++;
    }));
  };
  const observer = new MutationObserver(observe);
  observer.observe(document.body, { childList: true, subtree: true, characterData: true });
  document.addEventListener('scroll', observe, true);
  document.addEventListener('change', observe, true);
  return {
    snapshot: () => structuredClone({ ...result, liveLatency: latency.snapshot() }),
    latestVisible: () => visible(document.querySelector('.m5-output-scroll article:last-child pre')),
    stop() {
    stopped = true; off(); observer.disconnect();
    document.removeEventListener('scroll', observe, true);
    document.removeEventListener('change', observe, true);
    document.removeEventListener('visibilitychange', visibility);
    for (const id of frames) cancelAnimationFrame(id);
    latest.clear(); frames.clear();
    return { ...result, liveLatency: latency.snapshot() };
  } };
}
module.exports = { installOutputObserver };
