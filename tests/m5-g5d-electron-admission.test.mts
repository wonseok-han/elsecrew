import assert from "node:assert/strict";
import test from "node:test";

import { StorageAdmissionPoller, STORAGE_ADMISSION_POLL_MS } from "../src/m5/admission-state.ts";
import { resolveM5Bridge } from "../src/m5/bridge-boundary.ts";
import { M5_BRIDGE_V4_METHODS } from "../src/m5/contracts/bridge-v4.ts";
import { M5_BRIDGE_V5_METHODS } from "../src/m5/contracts/bridge-v5.ts";
import { M5_BRIDGE_V6_METHODS } from "../src/m5/contracts/bridge-v6.ts";
import { M5_BRIDGE_V7_METHODS } from "../src/m5/contracts/bridge-v7.ts";
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

function bridge(version: 4 | 5 | 6): Record<string, unknown> {
  const methods = version === 4 ? M5_BRIDGE_V4_METHODS : version === 5 ? M5_BRIDGE_V5_METHODS : M5_BRIDGE_V6_METHODS;
  return Object.fromEntries([["version", version], ...methods.map((method) => [method, () => { throw new Error(`bridge method ${method} must not run during boundary validation`); }])]);
}

test("G5D renderer bridge boundary blocks absent, unknown, and incomplete bridges before any durable method", () => {
  const calls: string[] = [];
  const incompleteV6 = { version: 6, getSystemStatus: () => calls.push("status") };
  const incompleteV7 = { ...bridge(6), version: 7 };
  for (const value of [undefined, incompleteV7, { ...bridge(6), version: 8 }, incompleteV6]) assert.equal(resolveM5Bridge(value), null);
  assert.deepEqual(calls, []);
});

test("complete V7 adds only follow-up and still requires all admission methods", () => {
  assert.deepEqual(M5_BRIDGE_V7_METHODS, [...M5_BRIDGE_V6_METHODS, "appendChatMessage"]);
  const v7 = { ...bridge(6), version: 7, appendChatMessage: () => {} };
  assert.equal(resolveM5Bridge(v7), v7);
  assert.equal(resolveM5Bridge({ ...v7, getStorageStatus: undefined }), null);
});

test("G5D renderer bridge boundary preserves complete V4 and V5 legacy bridges", () => {
  const v4 = bridge(4);
  const v5 = bridge(5);
  assert.equal(resolveM5Bridge(v4), v4);
  assert.equal(resolveM5Bridge(v5), v5);
});
