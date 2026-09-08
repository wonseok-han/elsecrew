import assert from "node:assert/strict";
import test from "node:test";

import { StorageAdmissionPoller, STORAGE_ADMISSION_POLL_MS } from "../src/m5/admission-state.ts";
import type { SystemStatusG5DV1 } from "../src/m5/contracts/lifecycle-g5d-v1.ts";

function status(state: "verifying" | "ready" | "failed") {
  return { storageAdmission: { state } } as SystemStatusG5DV1;
}

test("G5D admission poller is one-at-a-time and settles verifying to ready", async () => {
  let calls = 0;
  let scheduled: (() => void) | undefined;
  const states: string[] = [];
  const poller = new StorageAdmissionPoller(
    async () => { calls += 1; return calls === 1 ? status("verifying") : status("ready"); },
    (state) => states.push(state),
    { set: (callback, delay) => { assert.equal(delay, STORAGE_ADMISSION_POLL_MS); scheduled = callback; return 1 as unknown as ReturnType<typeof setTimeout>; }, clear: () => { scheduled = undefined; } },
  );
  poller.start();
  poller.start();
  await Promise.resolve(); await Promise.resolve();
  assert.equal(calls, 1);
  scheduled?.();
  await Promise.resolve(); await Promise.resolve();
  assert.equal(calls, 2);
  assert.deepEqual(states, ["checking", "verifying", "ready"]);
});

test("G5D admission poller cancels scheduled and in-flight updates on unmount", async () => {
  let resolve!: (value: SystemStatusG5DV1) => void;
  const states: string[] = [];
  const poller = new StorageAdmissionPoller(
    () => new Promise<SystemStatusG5DV1>((nextResolve) => { resolve = nextResolve; }),
    (state) => states.push(state),
  );
  poller.start();
  poller.stop();
  resolve(status("ready"));
  await Promise.resolve(); await Promise.resolve();
  assert.deepEqual(states, ["checking"]);
});

test("G5D admission poller shows fixed failed state when strict status parsing rejects", async () => {
  const states: string[] = [];
  const poller = new StorageAdmissionPoller(async () => { throw new Error("malformed status"); }, (state) => states.push(state));
  poller.start();
  await Promise.resolve(); await Promise.resolve();
  assert.deepEqual(states, ["checking", "failed"]);
});
