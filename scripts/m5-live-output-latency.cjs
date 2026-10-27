// Exact-head notification-to-frame diagnostics. Never substitutes for every-row
// paint, provider generation, physical display time, or a complete G6 trial.
function createLiveOutputLatency() {
  const result = { samples: [], superseded: 0, interrupted: 0, dropped: 0, unfinished: 0 };
  const pending = new Map();
  let selected = null, armed = false, lastHead = 0, epoch = 0;
  const valid = value => Number.isFinite(value) && value >= 0;
  return {
    select(runId) {
      if (runId === selected) return epoch;
      result.interrupted += pending.size;
      pending.clear(); selected = runId; armed = false; lastHead = 0;
      return ++epoch;
    },
    receive(runId, sequence, at) {
      if (runId !== selected || !Number.isSafeInteger(sequence) || sequence < 1 || !valid(at)) return;
      if (sequence <= lastHead) return;
      lastHead = sequence;
      if (!armed) return;
      if (pending.size >= 256) { result.dropped++; return; }
      pending.set(sequence, at);
    },
    frame(runId, sequence, at, capturedEpoch) {
      if (runId !== selected || capturedEpoch !== epoch || !Number.isSafeInteger(sequence) || sequence < 1 || !valid(at)) return;
      for (const [head, receivedAt] of pending) {
        if (head > sequence) continue;
        pending.delete(head);
        if (head !== sequence) { result.superseded++; continue; }
        if (at < receivedAt || result.samples.length >= 1024) { result.dropped++; continue; }
        result.samples.push({ runId, sequence, receivedAt, frameAt: at, notificationToFrameMs: at - receivedAt });
      }
      // Arm only after catching up in this selection epoch. Old history never
      // becomes a live latency sample just because the user selected its chat.
      if (lastHead > 0 && sequence >= lastHead) armed = true;
    },
    snapshot() { return { ...result, samples: result.samples.slice(), unfinished: pending.size }; },
  };
}
module.exports = { createLiveOutputLatency };
