import assert from "node:assert/strict";
import test from "node:test";
import { shouldPollLogin } from "../src/m5/provider-login-poll.ts";

test("login polling observes work and ambiguous replies but stops at rest", () => {
  for (const state of ["diagnosing", "installed", "starting", "waiting", "verifying"] as const) {
    assert.equal(shouldPollLogin(state, false, false), true, state);
    assert.equal(shouldPollLogin(state, false, true), false, state);
  }
  for (const state of ["idle", "connected", "canceled", "failed", "unsupported"] as const) {
    assert.equal(shouldPollLogin(state, false, false), false, state);
    assert.equal(shouldPollLogin(state, true, false), true, state);
    assert.equal(shouldPollLogin(state, true, true), false, state);
  }
});
