// The order of hiding, placing and showing the preview view (PR #9
// re-review B1·I1), with main's answers held back, reordered or never sent.
import assert from "node:assert/strict";
import test from "node:test";

import { PreviewLayoutLoop, type LoopLayout } from "../src/m5/preview-layout-loop.ts";

type Answer = { closed: boolean; snapshot: string | null };
interface Pending { layout: LoopLayout; resolve(answer?: Partial<Answer>): void; reject(): void }

function harness(extra: { released?: () => void } = {}) {
  const sent: Pending[] = [];
  const timers: { callback: () => void; ms: number; cleared: boolean }[] = [];
  const closes: { resolve(): void }[] = [];
  const stills: (string | null)[] = [];
  const state = { bounds: { x: 900, y: 150, width: 500, height: 600 } as LoopLayout["bounds"] | null, obscured: false, viewport: "wide" as const };
  const loop = new PreviewLayoutLoop({
    measure: () => ({ bounds: state.bounds, pixelRatio: 2, page: { width: 1440, height: 900 } }),
    viewport: () => state.viewport,
    obscured: () => state.obscured,
    send: layout => new Promise<Answer>((resolve, reject) => {
      sent.push({ layout, resolve: answer => resolve({ closed: false, snapshot: null, ...answer }), reject: () => reject(new Error("refused")) });
    }),
    close: () => new Promise<void>(resolve => { closes.push({ resolve }); }),
    still: image => { stills.push(image); },
    released: extra.released,
    setTimer: (callback, ms) => { const timer = { callback, ms, cleared: false }; timers.push(timer); return timer; },
    clearTimer: timer => { (timer as { cleared: boolean }).cleared = true; },
  });
  const settle = () => new Promise(resolve => setImmediate(resolve));
  const fire = (ms: number) => { for (const timer of timers.filter(entry => entry.ms === ms && !entry.cleared)) { timer.cleared = true; timer.callback(); } };
  return { loop, sent, timers, closes, stills, state, settle, fire };
}

/** Shown: one show sent and accepted. */
async function shown(h: ReturnType<typeof harness>) {
  h.loop.sync();
  assert.equal(h.sent.length, 1);
  assert.equal(h.sent[0]!.layout.obscured, false);
  h.sent[0]!.resolve();
  await h.settle();
  assert.equal(h.loop.accepted?.obscured, false);
}

test("a hide is sent at once even while a show still waits for its answer; showing waits for every answer", async () => {
  const h = harness();
  h.loop.sync();
  assert.equal(h.sent.length, 1, "the first show is on its way");
  // A cover comes up before main answered the show: the hide does not queue behind it.
  h.state.obscured = true;
  h.loop.sync();
  assert.equal(h.sent.length, 2);
  assert.equal(h.sent[1]!.layout.obscured, true);
  // The cover leaves before either answer: showing waits.
  h.state.obscured = false;
  h.loop.sync();
  assert.equal(h.sent.length, 2, "no show while answers are outstanding");
  h.sent[0]!.resolve(); h.sent[1]!.resolve();
  await h.settle();
  assert.equal(h.sent.length, 3);
  assert.equal(h.sent[2]!.layout.obscured, false);
  // Every layout carries the page's ratio and size (main refuses stale ones).
  assert.deepEqual([h.sent[2]!.layout.pixelRatio, h.sent[2]!.layout.page], [2, { width: 1440, height: 900 }]);
});

test("a held cover is let go only after main confirmed the view hidden (answer delayed)", async () => {
  const h = harness();
  await shown(h);
  let released = 0;
  h.loop.hold(() => { released++; });
  assert.equal(h.sent.at(-1)!.layout.obscured, true, "the hide goes out at once");
  await h.settle();
  assert.equal(released, 0, "held while the hide's answer is delayed");
  h.sent.at(-1)!.resolve();
  await h.settle();
  assert.equal(released, 1);
  // Nothing covers it any more: shown again.
  assert.equal(h.sent.at(-1)!.layout.obscured, false);
});

test("a cover held while a show is on its way waits for both answers, in any order", async () => {
  const h = harness();
  h.loop.sync();
  const show = h.sent[0]!;
  let released = 0;
  h.loop.hold(() => { released++; });
  const hide = h.sent[1]!;
  assert.equal(hide.layout.obscured, true);
  // The hide's answer first: the show is still on its way, so not yet.
  hide.resolve();
  await h.settle();
  assert.equal(released, 0);
  show.resolve();
  await h.settle();
  assert.equal(released, 1, "the older show's answer changes nothing; then it is let go");
  assert.equal(h.loop.accepted?.obscured, true, "the newest answer is the truth");
});

test("a band that moves the stage appears only once the view is hidden; the view is placed again after the page laid it out", async () => {
  const h = harness();
  await shown(h);
  let banded = false;
  h.loop.afterHidden(() => { banded = true; h.state.bounds = { x: 900, y: 190, width: 500, height: 560 }; });
  assert.equal(banded, false);
  assert.equal(h.sent.at(-1)!.layout.obscured, true);
  h.sent.at(-1)!.resolve();
  await h.settle();
  assert.equal(banded, true, "the band appears after main hid the view");
  const before = h.sent.length - 1;
  h.loop.sync();
  for (const sent of h.sent) sent.resolve();
  await h.settle();
  assert.ok(h.sent.slice(before).every(sent => sent.layout.obscured), "not shown before the page says it is laid out");
  h.loop.settled();
  const placed = h.sent.at(-1)!.layout;
  assert.equal(placed.obscured, false);
  assert.deepEqual(placed.bounds, { x: 900, y: 190, width: 500, height: 560 }, "shown at the stage's new place");
});

test("no answer in time: the preview is detached, and nothing is let go before main answers the close", async () => {
  const h = harness();
  await shown(h);
  let released = 0;
  h.loop.hold(() => { released++; });
  // The hide's answer never comes.
  h.fire(1500);
  assert.equal(h.closes.length, 1, "detached at the limit");
  await h.settle();
  assert.equal(released, 0, "the close is not answered yet: still held");
  h.closes[0]!.resolve();
  await h.settle();
  assert.equal(released, 1);
  assert.equal(h.loop.detached, true);
  // Nothing more is sent for a detached preview; later holds go at once.
  const count = h.sent.length;
  h.loop.sync();
  let later = 0;
  h.loop.hold(() => { later++; });
  assert.equal(h.sent.length, count);
  assert.equal(later, 1);
});

test("a refused layout is not remembered and is sent again; a closed preview lets go at once", async () => {
  const h = harness();
  h.loop.sync();
  h.sent[0]!.reject();
  await h.settle();
  assert.equal(h.loop.accepted, null);
  h.fire(250);
  assert.equal(h.sent.length, 2, "sent again");
  h.sent[1]!.resolve();
  await h.settle();
  assert.equal(h.loop.accepted?.obscured, false);
  let released = 0;
  h.loop.hold(() => { released++; });
  h.sent.at(-1)!.resolve({ closed: true });
  await h.settle();
  assert.equal(released, 1, "main no longer has the view");
});

test("a view never placed lets a cover go at once; disposing lets go only after the close is answered", async () => {
  const h = harness();
  h.state.bounds = null;
  let released = 0;
  h.loop.hold(() => { released++; });
  assert.equal(released, 1, "nothing to hide");
  const k = harness();
  await shown(k);
  let held = 0;
  k.loop.hold(() => { held++; });
  k.loop.dispose();
  assert.equal(k.closes.length, 1);
  await k.settle();
  assert.equal(held, 0);
  k.closes[0]!.resolve();
  await k.settle();
  assert.equal(held, 1);
});

test("several held covers are let go together and judged once, before the view may show again", async () => {
  const events: string[] = [];
  const h = harness({ released: () => { events.push(`judged:${h.sent.length}`); h.state.obscured = true; } });
  await shown(h);
  h.loop.hold(() => { events.push("bell"); });
  h.loop.hold(() => { events.push("panel"); });
  const hides = h.sent.filter(sent => sent.layout.obscured);
  assert.equal(hides.length, 1, "one hide for both");
  hides[0]!.resolve();
  await h.settle();
  const count = h.sent.length;
  assert.deepEqual(events, ["bell", "panel", `judged:${count}`], "both back first, then one look");
  // The look found the cover (obscured): nothing shows.
  assert.ok(h.sent.every(sent => sent.layout.obscured || sent === h.sent[0]));
});

test("a layout main never answers counts as failed after the answer limit and is sent again; the queue never stays blocked", async () => {
  const h = harness();
  h.loop.sync();
  assert.equal(h.sent.length, 1);
  // That show is never answered: no other show may go out meanwhile…
  h.state.bounds = { x: 900, y: 190, width: 500, height: 560 };
  h.loop.sync();
  assert.equal(h.sent.length, 1);
  // …until the limit: it counts as failed, and the newest layout goes out.
  h.fire(3000);
  await h.settle();
  h.fire(250);
  assert.equal(h.sent.length, 2);
  assert.deepEqual(h.sent[1]!.layout.bounds, { x: 900, y: 190, width: 500, height: 560 });
  h.sent[1]!.resolve();
  await h.settle();
  assert.equal(h.loop.accepted?.bounds.y, 190);
  // A late answer to the abandoned request changes nothing.
  h.sent[0]!.resolve();
  await h.settle();
  assert.equal(h.loop.accepted?.bounds.y, 190);
});

test("while hidden, a stage measured on and off sends nothing more; the next show carries the fresh place", async () => {
  const h = harness();
  await shown(h);
  h.state.obscured = true;
  h.loop.sync();
  h.sent.at(-1)!.resolve();
  await h.settle();
  const count = h.sent.length;
  for (const bounds of [null, { x: 568, y: 185, width: 424, height: 458 }, null, { x: 568, y: 185, width: 424, height: 458 }]) {
    h.state.bounds = bounds;
    h.loop.sync();
    await h.settle();
  }
  assert.equal(h.sent.length, count, "no stream of hides");
  h.state.obscured = false;
  h.loop.sync();
  assert.equal(h.sent.length, count + 1);
  assert.deepEqual(h.sent.at(-1)!.layout.bounds, { x: 568, y: 185, width: 424, height: 458 });
});
