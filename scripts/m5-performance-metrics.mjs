/** Assess supplied G6 measurements; never synthesize missing runtime observations. */
import { openSync, readSync, closeSync, fstatSync, realpathSync, constants } from "node:fs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const MiB = 1024 * 1024;
const MAX_INPUT_BYTES = 4 * MiB;
// Nearest-rank p95 of ten post-warmup trials is the slowest trial.
const TIMING_LIMITS = Object.freeze({
  appInteractiveMs: 3000, sidecarReadyMs: 1000, treeRootMs: 1000,
  treeExpandMs: 250, fileEditableMs: 1500, inputPaintMs: 50,
  diffSummaryMs: 2000, diffFirstFileMs: 1500, eventPaintMs: 100,
});
const MAXIMUM_LIMITS = Object.freeze({
  appColdMs: 5000, treeDomNodes: 500, mainLongTaskMs: 200,
  rendererLongTaskMs: 200, idleRssBytes: 400 * MiB,
  peakRssBytes: 800 * MiB, retainedRssBytes: 120 * MiB,
});
const ZERO_COUNTERS = ["lostEvents", "duplicateEvents", "crossChatEvents", "reorderedEvents",
  "silentOverwrites", "unreportedConflicts", "survivingTrackedIdentities", "unreportedUncertainExits"];
const FIXTURE = Object.freeze({
  emptyStartupDatabase: true, packagedApp: true, sidecarMigration: false,
  treeFiles: 10000, treeDirectories: 1000, fileBytes: 5 * MiB, fileLines: 100000,
  diffFiles: 500, diffChangedLines: 20000, diffBytes: 10 * MiB,
  chatStreams: 4, eventsPerStreamPerSecond: 50, streamSeconds: 60, sentEvents: 12000,
  saveRaces: 100, detachedDescendants: true, memoryCloseWaitSeconds: 60,
  rendererClock: "performance.now", rssScope: "allAppProcesses",
});

function number(value) { return typeof value === "number" && Number.isFinite(value) && value >= 0; }
function text(value) { return typeof value === "string" && value.trim().length > 0; }

export function assessPerformance(input, expectedHead) {
  const report = {
    schemaVersion: 1, evidence: "m5-performance-measurements", status: "NOT_READY",
    head: expectedHead ?? null, checks: [], reasons: [],
    limitations: ["Assesses supplied local measurements; does not collect or authenticate them. Package size, signing, real providers and Windows safety are separate gates."],
  };
  const missing = (field) => report.reasons.push({ code: "missingOrInvalidMeasurement", field });
  if (!input || typeof input !== "object" || Array.isArray(input)) { missing("report"); return report; }
  if (input.schemaVersion !== 1) missing("schemaVersion");
  if (!/^[a-f0-9]{40}$|^[a-f0-9]{64}$/.test(expectedHead ?? "") || input.head !== expectedHead || input.dirty !== false) missing("cleanMatchingHead");
  for (const field of ["machine", "cpu", "os", "electron", "chromium", "node", "rust", "measuredAt"]) {
    if (!text(input.environment?.[field])) missing(`environment.${field}`);
  }
  if (!Number.isFinite(Date.parse(input.environment?.measuredAt ?? ""))) missing("environment.measuredAt");
  if (!number(input.environment?.ramBytes) || input.environment.ramBytes === 0) missing("environment.ramBytes");
  if (input.environment?.platform !== "darwin" || input.environment?.arch !== "arm64") missing("macosArm64Target");
  if (input.warmupRuns !== 1) missing("warmupRuns");
  for (const [key, value] of Object.entries(FIXTURE)) {
    if (input.fixture?.[key] !== value) missing(`fixture.${key}`);
  }
  for (const [metric, limit] of Object.entries(TIMING_LIMITS)) {
    const samples = input.timings?.[metric];
    if (!Array.isArray(samples) || samples.length !== 10 || !samples.every(number)) { missing(metric); continue; }
    const p95 = [...samples].sort((a, b) => a - b)[9];
    report.checks.push({ metric, statistic: "nearestRankP95", samples: 10, value: p95, limit, passed: p95 <= limit });
    if (metric === "sidecarReadyMs") report.checks.push({ metric: "sidecarHardTimeoutMs", value: Math.max(...samples), limit: 3000, passed: samples.every((value) => value <= 3000) });
  }
  for (const [metric, limit] of Object.entries(MAXIMUM_LIMITS)) {
    const value = input.maxima?.[metric];
    if (!number(value)) { missing(metric); continue; }
    report.checks.push({ metric, statistic: "maximum", value, limit, passed: value <= limit });
  }
  for (const metric of ZERO_COUNTERS) {
    const value = input.counters?.[metric];
    if (!Number.isSafeInteger(value) || value < 0) { missing(metric); continue; }
    report.checks.push({ metric, value, limit: 0, passed: value === 0 });
  }
  // Preserve known failures even when other measurements are missing.
  if (report.checks.some((check) => !check.passed)) report.status = "FAIL";
  else if (report.reasons.length === 0) report.status = "PASS";
  return report;
}

function readBoundedJson(path) {
  const fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
  try {
    const before = fstatSync(fd);
    if (!before.isFile() || before.size < 1 || before.size > MAX_INPUT_BYTES) throw new Error("invalid evidence file");
    const bytes = Buffer.alloc(before.size + 1);
    let count = 0;
    while (count < bytes.length) {
      const read = readSync(fd, bytes, count, bytes.length - count, null);
      if (read === 0) break;
      count += read;
    }
    const after = fstatSync(fd);
    if (count !== before.size || after.size !== before.size || after.mtimeMs !== before.mtimeMs || after.ctimeMs !== before.ctimeMs) throw new Error("evidence changed");
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(0, count)));
  } finally { closeSync(fd); }
}

export function runPerformanceCli(argv = process.argv.slice(2)) {
  const repository = resolve(fileURLToPath(new URL("..", import.meta.url)));
  const git = (args) => {
    const result = spawnSync(process.platform === "win32" ? "git" : "/usr/bin/git", args, { cwd: repository, encoding: "utf8" });
    return result.status === 0 ? result.stdout.trim() : null;
  };
  let report;
  try {
    if (argv.length !== 2 || argv[0] !== "--input" || !text(argv[1])) throw new Error("supply --input with a measurement JSON file");
    const head = git(["rev-parse", "HEAD"]);
    if (!head || git(["status", "--porcelain"]) !== "") throw new Error("measurement assessment requires a clean checkout");
    report = assessPerformance(readBoundedJson(argv[1]), head);
    if (git(["rev-parse", "HEAD"]) !== head || git(["status", "--porcelain"]) !== "") throw new Error("checkout changed during assessment");
  } catch {
    report = { schemaVersion: 1, evidence: "m5-performance-measurements", status: "NOT_READY", reasons: [{ code: "measurementInputUnavailable", detail: "Supply --input with complete measurements from this clean HEAD; the file must be a bounded regular UTF-8 JSON file." }] };
  }
  console.log(JSON.stringify(report));
  return report.status === "PASS" ? 0 : report.status === "FAIL" ? 1 : 2;
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = runPerformanceCli();
