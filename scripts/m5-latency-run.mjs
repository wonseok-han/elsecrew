#!/usr/bin/env node
// Runs the response-time probe (scripts/m5-latency-probe.cjs) twice on the
// same isolated app data and prints a per-condition table:
//   node scripts/m5-latency-run.mjs [--build] [--check] [--samples N] [--seed-chats N] [--seed-runs N] [--skip-background] [--sidecar <binary>] [--main <main.cjs>] [--out <dir>]
// --build builds main/preload/renderer into .vite/ first (no packaging).
// Needs target/debug/elsecrew-sidecar (`cargo build -p elsecrew-sidecar`).
// --check judges the targets below and exits 1 when one is missed (2: not run).
// Development build and the fake agent, no account: response time of the
// app's own path (screen, main, engine), not of a provider.
//
// Targets (latency work 2026-11-26): a file opened from the files tab shows
// its text within 300 ms at rest and within 1 s while a team runs or the
// background storage check runs (95th percentile); the approval list and
// getChat answer within 1 s in every condition; no call fails.
const TARGETS = {
  "ui-open": { idle: 300, "idle-from-seal": 300, running: 1000, "background-check": 1000 },
  "approval:list": { running: 1000 },
  getChat: { idle: 1000, "idle-from-seal": 1000, running: 1000, "background-check": 1000 },
};
import { spawnSync } from "node:child_process";
import { builtinModules, createRequire } from "node:module";
import { existsSync, mkdtempSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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
const stages = args.includes("--skip-background") ? ["first"] : ["first", "second"];
for (const stage of stages) {
  const env = { ...process.env, LATENCY_STAGE: stage, LATENCY_ROOT: root, LATENCY_SAMPLES: option("--samples", "12"), LATENCY_SEED_CHATS: option("--seed-chats", "6"), LATENCY_SEED_RUNS: option("--seed-runs", "0"), ...(option("--sidecar") ? { LATENCY_SIDECAR: resolve(option("--sidecar")) } : {}), ...(option("--main") ? { LATENCY_MAIN: resolve(option("--main")) } : {}) };
  delete env.ELECTRON_RUN_AS_NODE;
  const result = spawnSync(electron, [join(repoRoot, "scripts", "m5-latency-probe.cjs")], { cwd: repoRoot, env, stdio: "inherit" });
  if (result.status !== 0) { console.error(`stage ${stage} exited ${result.status}`); break; }
}

const percentile = (values, p) => { if (!values.length) return null; const sorted = [...values].sort((a, b) => a - b); return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)]; };
const round = value => value == null ? null : Math.round(value * 10) / 10;
const rows = [];
const reports = [];
for (const stage of stages) {
  const file = join(root, `result-${stage}.json`);
  if (!existsSync(file)) continue;
  const report = JSON.parse(readFileSync(file, "utf8"));
  reports.push(report);
  const groups = new Map();
  for (const m of report.measurements) {
    const key = `${m.condition}\t${m.label}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(m);
  }
  for (const [key, list] of groups) {
    const [condition, label] = key.split("\t");
    const pick = name => list.map(m => m.segments?.[name]).filter(v => typeof v === "number");
    rows.push({ condition, label, n: list.length, failed: list.filter(m => !m.ok).length,
      p50: round(percentile(list.map(m => m.totalMs), 50)), p95: round(percentile(list.map(m => m.totalMs), 95)), max: round(Math.max(...list.map(m => m.totalMs))),
      queueP50: round(percentile(pick("queueMs"), 50)), queueMax: round(Math.max(0, ...pick("queueMs"))),
      handleP50: round(percentile(pick("handleMs"), 50)), handleMax: round(Math.max(0, ...pick("handleMs"))),
      ipcP50: round(percentile(list.map(m => m.segments ? m.segments.ipcInMs + m.segments.ipcOutMs : null).filter(v => v != null), 50)) });
  }
}
const failures = reports.flatMap(r => r.measurements.filter(m => !m.ok).map(m => ({ stage: r.stage, label: m.label, condition: m.condition, error: m.error ?? null }))).slice(0, 20);
const verdicts = [];
for (const [label, budgets] of Object.entries(TARGETS)) {
  for (const [condition, budget] of Object.entries(budgets)) {
    const list = reports.flatMap(r => r.measurements).filter(m => m.condition === condition && (m.label === label || m.label.startsWith(`${label}:`)));
    if (stages.includes("second") || !condition.includes("seal") && condition !== "background-check") {
      const p95 = percentile(list.map(m => m.totalMs), 95);
      verdicts.push({ label, condition, budgetMs: budget, n: list.length, p95: round(p95), failed: list.filter(m => !m.ok).length,
        pass: list.length > 0 && p95 <= budget && list.every(m => m.ok) });
    }
  }
}
const summary = { root, verdicts, failures, seedRunErrors: reports.map(r => r.seedRunErrors ?? []), unmatched: reports.map(r => r.unmatched), phases: reports.map(r => ({ stage: r.stage, slowPhases: summarizePhases(r.phases ?? []) })), requests: reports.map(r => ({ stage: r.stage, byMethod: r.requestsByMethod })), errors: reports.filter(r => r.error).map(r => r.error.slice(0, 400)) };
writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2));
console.table(rows);
console.table(verdicts);
console.log(JSON.stringify({ out: join(out, "summary.json"), root, slowPhases: summary.phases }, null, 1));

if (args.includes("--check")) {
  const passed = verdicts.length > 0 && verdicts.every(v => v.pass) && reports.length === stages.length && reports.every(r => r.status === "DONE");
  console.log(JSON.stringify({ status: passed ? "PASS" : "FAIL", failed: verdicts.filter(v => !v.pass) }));
  process.exit(passed ? 0 : 1);
}

function summarizePhases(phases) {
  const byName = {};
  for (const p of phases) { const row = byName[p.phase] ??= { count: 0, maxMs: 0, totalMs: 0 }; row.count++; row.maxMs = Math.max(row.maxMs, p.ms); row.totalMs += p.ms; }
  return byName;
}

async function buildApp() {
  const { build } = await import("vite");
  const external = ["electron", "electron/common", ...builtinModules.flatMap(name => [name, `node:${name}`])];
  const define = { MAIN_WINDOW_VITE_DEV_SERVER_URL: "undefined", MAIN_WINDOW_VITE_NAME: JSON.stringify("main_window") };
  await build({ configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false, lib: { entry: "electron/main.ts", formats: ["cjs"], fileName: () => "main.cjs" }, rollupOptions: { external } } });
  await build({ configFile: false, root: repoRoot, mode: "production", logLevel: "warn", define,
    build: { emptyOutDir: false, outDir: ".vite/build", minify: false, rollupOptions: { external, input: "electron/preload.ts", output: { format: "cjs", inlineDynamicImports: true, entryFileNames: "[name].cjs", chunkFileNames: "[name].cjs" } } } });
  await build({ configFile: join(repoRoot, "electron", "vite.renderer.config.ts"), logLevel: "warn" });
}
