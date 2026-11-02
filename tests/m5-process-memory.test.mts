import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
const { processTreeMemory, summarizeMemory, processTreeMemoryWithoutCollector } = createRequire(import.meta.url)("../scripts/m5-process-memory.cjs");

test("RSS includes all descendants regardless of row order, excluding other apps", () => {
  const result = processTreeMemory("30 20 300\n99 1 999\n20 10 200\n10 1 100\n", 10);
  assert.equal(result.rssBytes, 600 * 1024);
  assert.deepEqual(result.processes.map(p => p.pid), [30, 20, 10]);
});
test("missing root, malformed rows and duplicate PIDs do not become zero RSS", () => {
  for (const input of ["", "20 1 12", "10 1 NaN", "10 1 10\n10 1 20", "10 1 999999999999999999"]) assert.throws(() => processTreeMemory(input, 10));
});
test("memory maxima and retained delta use measured values and fixed limits", () => {
  const samples = [{ phase: "idle", rssBytes: 300 * 1024 ** 2 }, { phase: "loaded", rssBytes: 801 * 1024 ** 2 }, { phase: "retained", rssBytes: 421 * 1024 ** 2 }];
  assert.deepEqual(summarizeMemory(samples).failedMetrics, ["peakRssBytes", "retainedRssBytes"]);
  samples[2].rssBytes = 250 * 1024 ** 2;
  assert.equal(summarizeMemory(samples).maxima.retainedRssBytes, 0);
  assert.throws(() => summarizeMemory(samples.slice(0, 2)));
});

test("excludes only the exact collector PID while retaining sidecar and agent descendants", () => {
  const rows = "10 1 100\n20 10 200\n30 20 300\n40 10 50\n41 30 80\n99 1 900\n";
  const result = processTreeMemoryWithoutCollector(rows, 10, 40);
  assert.equal(result.rssBytes, 680 * 1024);
  assert.deepEqual(result.processes.map(p => p.pid), [10,20,30,41]);
  for (const pid of [undefined,NaN,-1,10,30,99,123]) {
    assert.throws(() => processTreeMemoryWithoutCollector(rows, 10, pid));
  }
  assert.throws(() => processTreeMemoryWithoutCollector(rows + "40 10 60\n", 10, 40));
});
