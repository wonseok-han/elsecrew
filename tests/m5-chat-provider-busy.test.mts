import assert from "node:assert/strict";
import test from "node:test";
import { isProviderBusy, whileProviderBusy } from "../src/m5/chat-state.ts";

const busy = () => new Error("Error invoking remote method 'm5:v4:agent:get-capabilities': Error: requestBlocked: another provider check currently owns this profile; retry after it finishes");

test("a capability check waits out a briefly busy provider", async () => {
  let calls = 0;
  const slept: number[] = [];
  const result = await whileProviderBusy(async () => { calls += 1; if (calls < 3) throw busy(); return "ready"; }, () => true,
    { sleep: async (ms) => { slept.push(ms); } });
  assert.equal(result, "ready");
  assert.equal(calls, 3);
  assert.deepEqual(slept, [1_000, 1_000]);
});

test("other errors, a long wait or a check no longer wanted end at once", async () => {
  assert.equal(isProviderBusy(busy()), true);
  assert.equal(isProviderBusy(new Error("requestBlocked: connect the provider in settings first")), false);
  await assert.rejects(whileProviderBusy(async () => { throw new Error("notAuthenticated"); }, () => true, { sleep: async () => {} }), /notAuthenticated/);
  let calls = 0;
  await assert.rejects(whileProviderBusy(async () => { calls += 1; throw busy(); }, () => true, { waitMs: 0, sleep: async () => {} }), /another provider check/);
  assert.equal(calls, 1);
  let wanted = true;
  await assert.rejects(whileProviderBusy(async () => { throw busy(); }, () => wanted, { sleep: async () => { wanted = false; } }));
});
