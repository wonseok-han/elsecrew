// The open-loop sampler of `m5-latency-starting-probe.cjs`, kept apart so a
// test can drive it with virtual timers (PR #25 review I5). Its source runs in
// the app's page: no closure, no require; everything comes through `env`.
//   env: { now(), setTimeout(fn, ms), clearTimeout(id), startsPending(),
//          reads: { label: () => Promise }, tickMs }
'use strict';
function createSampler(env) {
  const s = { measurements: [], ticks: {}, condition: null, outstanding: 0, timer: null };
  const fire = (label, call) => {
    const condition = s.condition, t0 = env.now(), before = env.startsPending();
    s.outstanding++;
    Promise.resolve().then(call).then(() => null, e => String(e && e.message || e).slice(0, 160)).then(error => {
      const t1 = env.now();
      s.outstanding--;
      s.measurements.push({ label, condition, t0, t1, ok: error === null, error, totalMs: t1 - t0, startsBefore: before, startsAfter: env.startsPending() });
    });
  };
  const tick = () => {
    s.timer = null;
    if (!s.condition) return;
    s.ticks[s.condition] = (s.ticks[s.condition] ?? 0) + 1;
    for (const [label, call] of Object.entries(env.reads)) fire(label, call);
    s.timer = env.setTimeout(tick, env.tickMs);
  };
  // One timer at a time: a switch or an end cancels the pending one, so
  // rounds never double the rate.
  s.begin = condition => { if (s.timer !== null) env.clearTimeout(s.timer); s.timer = null; s.condition = condition; tick(); };
  s.end = () => { if (s.timer !== null) env.clearTimeout(s.timer); s.timer = null; s.condition = null; };
  return s;
}
module.exports = { createSampler };
