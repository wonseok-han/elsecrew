// Serialized into the development renderer. Observes only public output heads
// and rendered rows; this does not measure provider generation or durable commit time.
function installOutputObserver() {
  const result = { heads: [], frames: [], visibility: [], droppedHeads: 0, droppedFrames: 0 };
  const latest = new Map();
  const frames = new Set();
  let stopped = false;
  let lastKey = '';
  const visibility = () => {
    if (result.visibility.length < 100) result.visibility.push({ state: document.visibilityState, at: performance.now() });
  };
  visibility();
  document.addEventListener('visibilitychange', visibility);
  const off = window.m5.onAgentOutputAvailable(head => {
    const sample = { runId: head.binding.runId, sequence: head.committedSequence, sealed: head.sealed, receivedAt: performance.now() };
    latest.set(sample.runId, sample);
    if (result.heads.length < 4096) result.heads.push(sample); else result.droppedHeads++;
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
    const runId = document.querySelector('.m5-durable-output select')?.value;
    const nodes = [...document.querySelectorAll('.m5-output-scroll article pre')];
    const node = nodes.reverse().find(visible);
    if (!node || !runId) return;
    const match = /^output (\d+)$/.exec(node.textContent);
    if (!match) return;
    const sequence = Number(match[1]), key = `${runId}:${sequence}`;
    if (key === lastKey) return;
    lastKey = key;
    const head = latest.get(runId), observedAt = performance.now();
    // Two RAFs are a rendering opportunity, not a physical compositor timestamp.
    frame(() => frame(() => {
      if (stopped || !node.isConnected || !visible(node)
        || document.querySelector('.m5-durable-output select')?.value !== runId
        || node.textContent !== `output ${sequence}`) return;
      const sample = { runId, sequence, observedAt, frameAt: performance.now(),
        headSequence: head?.sequence ?? null, headReceivedAt: head?.receivedAt ?? null };
      if (result.frames.length < 1024) result.frames.push(sample); else result.droppedFrames++;
    }));
  };
  const observer = new MutationObserver(observe);
  observer.observe(document.body, { childList: true, subtree: true, characterData: true });
  document.addEventListener('scroll', observe, true);
  return {
    latestVisible: () => visible(document.querySelector('.m5-output-scroll article:last-child pre')),
    stop() {
    stopped = true; off(); observer.disconnect();
    document.removeEventListener('scroll', observe, true);
    document.removeEventListener('visibilitychange', visibility);
    for (const id of frames) cancelAnimationFrame(id);
    latest.clear(); frames.clear();
    return result;
  } };
}
module.exports = { installOutputObserver };
