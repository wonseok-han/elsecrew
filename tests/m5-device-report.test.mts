import test from "node:test";
import assert from "node:assert/strict";
import { validateDeviceReport } from "../src/m5/contracts/device-report-v1.ts";
import { readDeviceReport } from "../electron/device-report.ts";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";

const report = { platform: "darwin", arch: "arm64", osVersion: "25.0.0", memoryBytes: 16 * 1024 ** 3, availableBytes: null };
test("device report keeps unknown space distinct from zero", () => {
  assert.equal(validateDeviceReport(report).availableBytes, null);
  assert.equal(validateDeviceReport({ ...report, availableBytes: 0 }).availableBytes, 0);
});
test("device contract rejects unsafe counts, extra authority, and raw paths", () => {
  for (const input of [
    { ...report, availableBytes: -1 }, { ...report, availableBytes: Infinity },
    { ...report, availableBytes: Number.MAX_SAFE_INTEGER + 1 },
    { ...report, memoryBytes: 0 }, { ...report, platform: "macos" },
    { ...report, osVersion: "/private/account" }, { ...report, ready: true },
    { ...report, path: "/Users/example" },
  ]) assert.throws(() => validateDeviceReport(input));
});
test("device inspection is read-only and preserves unknown on inaccessible location", async () => {
  const observed = await readDeviceReport(tmpdir());
  assert.ok(observed.memoryBytes > 0);
  assert.ok(observed.availableBytes === null || observed.availableBytes >= 0);
  const absent = await readDeviceReport(join(tmpdir(), `elsecrew-absent-${randomUUID()}`));
  assert.equal(absent.availableBytes, null);
  assert.deepEqual(Object.keys(absent).sort(), Object.keys(report).sort());
});
