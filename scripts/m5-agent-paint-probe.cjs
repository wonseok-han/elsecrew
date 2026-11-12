// Serialized into the renderer. Passive: no requests, credentials or model calls.
function startAgentPaintProbe(chatId) {
  if (globalThis.__m5AgentPaintProbe) throw new Error('Agent paint probe already active');
  const pending = [], samples = [], seen = new Set(), counts = new Map(), frames = new Set();
  let stopped = false, hidden = document.visibilityState !== 'visible', overflow = false;
  const visibility = () => { if (document.visibilityState !== 'visible') hidden = true; };
  document.addEventListener('visibilitychange', visibility);
  const frame = callback => {
    const id = requestAnimationFrame(() => { frames.delete(id); if (!stopped) callback(); });
    frames.add(id);
  };
  const rowFor = item => [...document.querySelectorAll('.m5-message-role')]
    .filter(node => node.dataset.chatId === chatId && node.dataset.runId === item.runId)[item.ordinal];
  const visible = node => node?.isConnected && node.getClientRects().length > 0;
  const observe = () => {
    for (const item of pending) {
      if (item.scheduled || item.done) continue;
      const node = rowFor(item);
      if (!visible(node)) continue;
      item.scheduled = true;
      frame(() => frame(() => {
        item.scheduled = false;
        if (!visible(node) || rowFor(item) !== node || hidden) return;
        item.done = true;
        samples.push({runId:item.runId, eventId:item.eventId, notificationToFrameMs:performance.now() - item.at});
      }));
    }
  };
  const observer = new MutationObserver(observe);
  observer.observe(document.body, {childList:true, subtree:true});
  const unsubscribe = window.m5.onAgentPublicEvent(event => {
    if (stopped || event.chatId !== chatId || event.kind !== 'final') return;
    if (seen.has(event.eventId)) { overflow = true; return; }
    if (pending.length >= 256) { overflow = true; return; }
    seen.add(event.eventId);
    const ordinal = counts.get(event.runId) ?? 0;
    counts.set(event.runId, ordinal + 1);
    pending.push({runId:event.runId, eventId:event.eventId, ordinal, at:performance.now()});
    observe();
  });
  globalThis.__m5AgentPaintProbe = {
    stop(runId) {
      stopped = true; unsubscribe(); observer.disconnect();
      document.removeEventListener('visibilitychange', visibility);
      for (const id of frames) cancelAnimationFrame(id);
      const selected = samples.filter(item => item.runId === runId);
      const expected = pending.filter(item => item.runId === runId).length;
      const complete = Boolean(runId) && expected > 0 && selected.length === expected && !hidden && !overflow;
      delete globalThis.__m5AgentPaintProbe;
      return {diagnostic:true, gateEligible:false, status:complete ? 'OBSERVED' : 'INCOMPLETE',
        expected, observed:selected.length, hidden, overflow, samples:selected,
        scope:'Final notifications to two renderer frames of durable role rows; no status events, physical compositor, IPC transit or full G6 evidence. Start before a new run; role rows correlate by run and final-event order.'};
    },
  };
}
module.exports = {startAgentPaintProbe};
