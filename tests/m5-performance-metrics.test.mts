import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, rmSync, symlinkSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { assessPerformance } from "../scripts/m5-performance-metrics.mjs";

const head = "a".repeat(40);
const MiB = 1024 * 1024;
test("G6 command assesses canonical runtime evidence rather than unit fixtures", () => {
  const manifest = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
  assert.equal(manifest.scripts['m5:test:performance'],
    'node scripts/m5-performance-metrics.mjs --input .artifacts/m5-performance/measurement.json');
});
function measurements(): any {
  return {
    schemaVersion: 1, head, dirty: false, warmupRuns: 1,
    environment: { machine: "fixture", cpu: "fixture", ramBytes: 16 * 1024 * MiB, os: "fixture",
      electron: "fixture", chromium: "fixture", node: "fixture", rust: "fixture",
      measuredAt: "2026-09-14T00:00:00.000Z", platform: "darwin", arch: "arm64" },
    fixture: { emptyStartupDatabase: true, packagedApp: true, sidecarMigration: false,
      treeFiles: 10000, treeDirectories: 1000, fileBytes: 5 * MiB, fileLines: 100000,
      diffFiles: 500, diffChangedLines: 20000, diffBytes: 10 * MiB,
      chatStreams: 4, eventsPerStreamPerSecond: 50, streamSeconds: 60, sentEvents: 12000,
      saveRaces: 100, detachedDescendants: true, memoryCloseWaitSeconds: 60,
      rendererClock: "performance.now", rssScope: "allAppProcesses" },
    timings: Object.fromEntries(Object.entries({ appInteractiveMs: 3000, sidecarReadyMs: 1000,
      treeRootMs: 1000, treeExpandMs: 250, fileEditableMs: 1500, inputPaintMs: 50,
      diffSummaryMs: 2000, diffFirstFileMs: 1500, eventPaintMs: 100 }).map(([key, value]) => [key, Array(10).fill(value)])),
    maxima: { appColdMs: 5000, treeDomNodes: 500, mainLongTaskMs: 200, rendererLongTaskMs: 200,
      idleRssBytes: 512 * MiB, peakRssBytes: 800 * MiB, retainedRssBytes: 120 * MiB },
    counters: { lostEvents: 0, duplicateEvents: 0, crossChatEvents: 0, reorderedEvents: 0,
      silentOverwrites: 0, unreportedConflicts: 0, survivingTrackedIdentities: 0, unreportedUncertainExits: 0 },
  };
}

test("complete supplied measurements pass at every provisional limit", () => {
  const report = assessPerformance(measurements(), head);
  assert.equal(report.status, "PASS");
  assert.equal(report.checks.length, 25);
});

test("p95 uses nearest rank, so one slow trial cannot be hidden by nine fast trials", () => {
  const data = measurements();
  data.timings.appInteractiveMs = [...Array(9).fill(1), 3001];
  const report = assessPerformance(data, head);
  assert.equal(report.status, "FAIL");
  assert.equal(report.checks.find((check: any) => check.metric === "appInteractiveMs")?.value, 3001);
});

test("diagnostic and ineligible observations cannot be promoted by otherwise complete numbers", () => {
  for (const flags of [{ diagnostic: true }, { gateEligible: false }, { diagnostic: "false" }, { gateEligible: null }]) {
    const data = { ...measurements(), ...flags };
    assert.equal(assessPerformance(data, head).status, "NOT_READY");
    data.maxima.peakRssBytes += 1;
    assert.equal(assessPerformance(data, head).status, "FAIL", "known failure must remain visible");
  }
  assert.equal(assessPerformance({ ...measurements(), diagnostic: false, gateEligible: true }, head).status, "PASS");
});

test("each limit and every safety counter fails independently", () => {
  for (const group of ["timings", "maxima", "counters"]) {
    for (const key of Object.keys(measurements()[group])) {
      const data = measurements();
      if (group === "timings") data[group][key][9] += 1;
      else data[group][key] += 1;
      assert.equal(assessPerformance(data, head).status, "FAIL", `${group}.${key}`);
    }
  }
});

test("missing or invalid observations never default to successful zero measurements", () => {
  for (const group of ["environment", "fixture", "timings", "maxima", "counters"]) {
    for (const key of Object.keys(measurements()[group])) {
      const data = measurements();
      delete data[group][key];
      assert.equal(assessPerformance(data, head).status, "NOT_READY", `${group}.${key}`);
    }
  }
  for (const bad of [[], Array(9).fill(1), Array(11).fill(1), Array(10).fill(NaN), Array(10).fill(-1), Array(10).fill("1")]) {
    const data = measurements(); data.timings.inputPaintMs = bad;
    assert.equal(assessPerformance(data, head).status, "NOT_READY");
  }
});

test("fixture mismatch, Windows, stale HEAD, dirty state and absent warmup are not readiness", () => {
  const cases = [
    (data: any) => { data.fixture.treeFiles = 9999; },
    (data: any) => { data.environment.platform = "win32"; },
    (data: any) => { data.head = "b".repeat(40); },
    (data: any) => { data.dirty = true; },
    (data: any) => { data.warmupRuns = 0; },
  ];
  for (const mutate of cases) { const data = measurements(); mutate(data); assert.equal(assessPerformance(data, head).status, "NOT_READY"); }
  assert.equal(assessPerformance(measurements(), null).status, "NOT_READY");
});

test("known failure remains visible alongside missing evidence", () => {
  const data = measurements(); delete data.timings; data.counters.lostEvents = 1;
  const report = assessPerformance(data, head);
  assert.equal(report.status, "FAIL"); assert.ok(report.reasons.length > 0);
});

test("CLI without runtime measurements returns NOT_READY instead of passing unit fixtures", () => {
  const result = spawnSync(process.execPath, ["scripts/m5-performance-metrics.mjs"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.equal(JSON.parse(result.stdout).status, "NOT_READY");
});

test("CLI assesses clean matching evidence and rejects malformed, stale, oversized and linked inputs", () => {
  const root = mkdtempSync(join(tmpdir(), "m5-performance-test-"));
  const git = (args: string[]) => {
    const result = spawnSync(process.platform === "win32" ? "git" : "/usr/bin/git", args, { cwd: root, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr); return result.stdout.trim();
  };
  try {
    mkdirSync(join(root, "scripts"));
    const script = join(root, "scripts", "m5-performance-metrics.mjs");
    copyFileSync(resolve("scripts/m5-performance-metrics.mjs"), script);
    writeFileSync(join(root, ".gitignore"), "measurements*\n");
    git(["init", "-q"]); git(["add", "."]);
    git(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
    const data = measurements(); data.head = git(["rev-parse", "HEAD"]);
    const input = join(root, "measurements.json");
    const run = (path = input) => {
      const result = spawnSync(process.execPath, [script, "--input", path], { encoding: "utf8" });
      assert.ok(result.stdout, `${result.stderr}\n${result.error ?? result.signal ?? result.status}`);
      return { exit: result.status, report: JSON.parse(result.stdout) };
    };
    assert.equal(run().exit, 2, "missing evidence must not become a successful gate");
    writeFileSync(input, JSON.stringify(data)); assert.equal(run().exit, 0);
    data.counters.silentOverwrites = 1;
    writeFileSync(input, JSON.stringify(data)); assert.equal(run().exit, 1);
    data.counters.silentOverwrites = 0; data.head = "b".repeat(40);
    writeFileSync(input, JSON.stringify(data)); assert.equal(run().exit, 2);
    for (const invalid of ["{", " ".repeat(4 * MiB + 1), Buffer.from([0xff])]) {
      writeFileSync(input, invalid); assert.equal(run().exit, 2);
    }
    data.head = git(["rev-parse", "HEAD"]);
    writeFileSync(input, JSON.stringify(data));
    if (process.platform !== "win32") {
      const link = join(root, "measurements-link.json"); symlinkSync(input, link);
      assert.equal(run(link).exit, 2);
    }
    writeFileSync(join(root, "dirty.txt"), "changed");
    assert.equal(run().exit, 2);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
