#!/usr/bin/env node
// Runs the response-time probe (scripts/m5-latency-probe.cjs) twice on the
// same isolated app data and prints a per-condition table:
//   node scripts/m5-latency-run.mjs [--build] [--check] [--samples N] [--seed-chats N] [--seed-runs N] [--skip-background] [--sidecar <binary>] [--main <main.cjs>] [--out <dir>]
// --build builds main/preload/renderer into .vite/ first (no packaging).
// Needs target/debug/elsecrew-sidecar (`cargo build -p elsecrew-sidecar`).
// --check judges the run with scripts/m5-latency-judge.mjs: exit 1 when a
// target is missed or a condition did not hold (2: not run). A development
// diagnostic with the fake agent and no account: response time of the app's
// own path (screen, main, engine), not of a provider. It is not a G6 gate:
// the fake agent has no approval queue, so the approval path itself is not
// exercised (PR #15 review B1·I1).
//
// --starting (ADE stabilization A) measures instead while runs start: two
// Claude Code and two Codex runs at once beside a working Claude run, with
// REAL accounts and providers on the development app's own data (run only
// when no other Elsecrew app uses it; five tiny paid prompts, each run
// stopped once started; scripts/m5-latency-starting-probe.cjs). File read,
// approval list and getChat, against a control round with the warm run alone.
// With --sidecar <binary> the same probe runs another engine build (before).
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync, mkdtempSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { judgeLatency, percentile } from "./m5-latency-judge.mjs";

if (process.platform !== "darwin") {
  console.log(JSON.stringify({ status: "NOT_RUN", reason: "macOS only (the fake agent and /usr/bin/git)" }));
  process.exit(2);
}
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name, fallback) => { const at = args.indexOf(name); return at >= 0 ? args[at + 1] : fallback; };
if (args.includes("--build")) await buildApp();
for (const required of [".vite/build/main.cjs", ".vite/build/preload.cjs", ".vite/renderer/main_window/index.html", "target/debug/elsecrew-sidecar"]) {
  if (!existsSync(join(repoRoot, required))) throw new Error(`missing ${required}; run with --build and cargo build -p elsecrew-sidecar`);
}
const electron = createRequire(import.meta.url)("electron");
const root = mkdtempSync(join(tmpdir(), "elsecrew-latency-"));
const out = resolve(option("--out", join(repoRoot, ".artifacts", "m5-latency")));
mkdirSync(out, { recursive: true });
const starting = args.includes("--starting");
const stages = starting ? ["starting"] : args.includes("--skip-background") ? ["first"] : ["first", "second"];
if (starting) {
  // The development app's own data: no other engine may be using it.
  const others = spawnSync("/bin/ps", ["-axo", "args="], { encoding: "utf8" }).stdout.split("\n")
    .filter(line => /elsecrew-sidecar --stdio/.test(line) && line.includes("Application Support/Elsecrew/"));
  if (others.length) { console.error("another engine uses the development app's data:\n" + others.join("\n")); process.exit(2); }
}
const samples = Number(option("--samples", "12")), seedChats = Number(option("--seed-chats", "40")), seedRuns = Number(option("--seed-runs", "120"));
const exits = {};
for (const stage of stages) {
  const env = { ...process.env, LATENCY_STAGE: stage, LATENCY_ROOT: root, LATENCY_SAMPLES: String(samples), LATENCY_SEED_CHATS: String(seedChats), LATENCY_SEED_RUNS: String(seedRuns), ...(option("--sidecar") ? { LATENCY_SIDECAR: resolve(option("--sidecar")) } : {}), ...(option("--main") ? { LATENCY_MAIN: resolve(option("--main")) } : {}) };
  delete env.ELECTRON_RUN_AS_NODE;
  const probe = stage === "starting" ? "m5-latency-starting-probe.cjs" : "m5-latency-probe.cjs";
  const result = spawnSync(electron, [join(repoRoot, "scripts", probe)], { cwd: repoRoot, env, stdio: "inherit" });
  exits[stage] = result.status;
  if (result.status !== 0) { console.error(`stage ${stage} exited ${result.status}`); break; }
}

const round = value => value == null ? null : Math.round(value * 10) / 10;
const rows = [];
const reports = {};
for (const stage of stages) {
  const file = join(root, `result-${stage}.json`);
  if (!existsSync(file)) continue;
  const report = JSON.parse(readFileSync(file, "utf8"));
  reports[stage] = report;
  const groups = new Map();
  for (const m of report.measurements) {
    const key = `${m.condition}\t${m.label.startsWith("ui-open:") ? "ui-open" : m.label}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(m);
  }
  for (const [key, list] of groups) {
    const [condition, label] = key.split("\t");
    rows.push({ condition, label, n: list.length, failed: list.filter(m => !m.ok).length, loaded: list.filter(m => m.loaded === true).length,
      p50: round(percentile(list.map(m => m.totalMs), 50)), p95: round(percentile(list.map(m => m.totalMs), 95)), max: round(Math.max(...list.map(m => m.totalMs))) });
  }
}
const judged = judgeLatency({ stages, exits, reports, samples, seedChats, seedRuns });
// The engine's view per round (all requests of a method in the window, the
// screen's own polling included): queue and handling time, not per-call.
const engine = Object.values(reports).flatMap(r => (r.rounds ?? []).map(x => ({ stage: r.stage, condition: x.condition, samples: x.samples, loadedSamples: x.loadedSamples, engine: x.engine, slowPhases: x.slowPhases })));
const summary = { root, status: judged.status, problems: judged.problems, verdicts: judged.verdicts, rows, engine, exits,
  engineStarts: Object.fromEntries(Object.entries(reports).map(([stage, r]) => [stage, r.engineStarts])),
  starts: reports.starting ? { engine: reports.starting.engine, ms: reports.starting.startsMs, results: (reports.starting.starts ?? []).map(r => ({ runtime: r.runtime, attempt: r.attempt, error: r.error ?? null, at: r.at })) } : undefined,
  seed: { chats: reports.first?.seedChatsDone, runs: reports.first?.seedRunsDone, errors: reports.first?.seedRunErrors ?? [] },
  timingDropped: Object.values(reports).map(r => r.timingDropped ?? null),
  phases: Object.values(reports).map(r => ({ stage: r.stage, slowPhases: summarizePhases(r.phases ?? []) })),
  errors: Object.values(reports).filter(r => r.error).map(r => r.error.slice(0, 400)) };
writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2));
console.table(rows);
console.table(judged.verdicts);
console.log(JSON.stringify({ out: join(out, "summary.json"), root, status: judged.status, problems: judged.problems, engine }, null, 1));

if (args.includes("--check")) process.exit(judged.status === "PASS" ? 0 : 1);

function summarizePhases(phases) {
  const byName = {};
  for (const p of phases) { const row = byName[p.phase] ??= { count: 0, maxMs: 0, totalMs: 0 }; row.count++; row.maxMs = Math.max(row.maxMs, p.ms); row.totalMs += p.ms; }
  return byName;
}

async function buildApp() {
  const { buildDevBundle } = await import("./m5-build-dev-bundle.mjs");
  await buildDevBundle();
}
