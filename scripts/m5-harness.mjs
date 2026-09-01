import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));
const packageScripts = packageJson.scripts ?? {};
const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const npm = process.platform === "win32" ? "npm.cmd" : "npm";

const baselineCommands = [
  { id: "diff", command: "git", args: ["diff", "--check"] },
  { id: "staged-diff", command: "git", args: ["diff", "--cached", "--check"] },
  { id: "frontend-build", command: pnpm, args: ["build"] },
  { id: "frontend-unit", command: npm, args: ["test"] },
  {
    id: "rust-regression",
    command: "cargo",
    args: ["test", "--manifest-path", "src-tauri/Cargo.toml"],
  },
];

const gates = [
  {
    id: "baseline",
    description: "현재 Tauri/React/Rust 기준선 회귀",
    scripts: [],
  },
  {
    id: "g1-foundation",
    description: "Electron scaffold와 Rust core 추출",
    scripts: ["m5:test:electron", "m5:test:core"],
  },
  {
    id: "g2-contracts",
    description: "TypeScript/Rust protocol golden fixture",
    scripts: ["m5:test:contracts"],
  },
  {
    id: "g3-storage",
    description: "sidecar protocol, SQLite replay와 preload 경계",
    scripts: ["m5:test:storage"],
  },
  {
    id: "g4-project-editor",
    description: "standalone snapshot 프로젝트 안전 API와 Monaco 편집·diff",
    scripts: ["m5:test:project", "m5:test:editor"],
  },
  {
    id: "g5-run-recovery",
    description: "다중 stream, 중단, 종료와 복구",
    scripts: ["m5:test:run-recovery"],
  },
  {
    id: "g5d-storage-lifetime",
    description: "source-indexed 저장 수명과 bounded materialization",
    scripts: ["m5:test:storage-lifetime"],
  },
  {
    id: "g6-release",
    description: "보안, 패키징, 성능과 M5.0 완료 게이트",
    scripts: ["m5:test:security", "m5:test:package", "m5:test:performance"],
  },
];

function usage(message) {
  if (message) console.error(message);
  console.error("Usage: node scripts/m5-harness.mjs --list | --gate <gate> [--no-report]");
  process.exit(64);
}

function parseArgs(args) {
  const parsed = { list: false, gate: undefined, report: true };
  for (let index = 0; index < args.length; index += 1) {
    const value = args[index];
    if (value === "--list") parsed.list = true;
    else if (value === "--no-report") parsed.report = false;
    else if (value === "--gate") parsed.gate = args[++index];
    else usage(`Unknown argument: ${value}`);
  }
  if (parsed.list === Boolean(parsed.gate)) usage("Choose exactly one of --list or --gate.");
  return parsed;
}

function commandText(command) {
  return [command.command, ...command.args].join(" ");
}

function gitValue(args) {
  const result = spawnSync("git", args, { cwd: repoRoot, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

function writeReport(report) {
  const timestamp = report.startedAt.replaceAll(":", "-").replaceAll(".", "-");
  const path = join(repoRoot, ".artifacts", "m5-harness", `${report.gate}-${timestamp}.json`);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  return path;
}

const options = parseArgs(process.argv.slice(2));

if (options.list) {
  for (const gate of gates) {
    const missing = gate.scripts.filter((script) => !packageScripts[script]);
    const state = missing.length === 0 ? "READY" : "NOT_READY";
    console.log(`${gate.id.padEnd(19)} ${state.padEnd(9)} ${gate.description}`);
    if (missing.length) console.log(`  missing: ${missing.join(", ")}`);
  }
  process.exit(0);
}

const gateIndex = gates.findIndex((gate) => gate.id === options.gate);
if (gateIndex < 0) usage(`Unknown gate: ${options.gate}`);

const selectedGates = gates.slice(0, gateIndex + 1);
const requiredScripts = selectedGates.flatMap((gate) => gate.scripts);
const missingScripts = requiredScripts.filter((script) => !packageScripts[script]);
const startedAt = new Date().toISOString();
const report = {
  schemaVersion: 1,
  gate: options.gate,
  status: "RUNNING",
  startedAt,
  finishedAt: null,
  repository: repoRoot,
  head: gitValue(["rev-parse", "HEAD"]),
  branch: gitValue(["branch", "--show-current"]),
  dirtyBefore: Boolean(gitValue(["status", "--porcelain"])),
  platform: process.platform,
  node: process.version,
  missingScripts,
  results: [],
};

if (missingScripts.length > 0) {
  report.status = "NOT_READY";
  report.finishedAt = new Date().toISOString();
  const path = options.report ? writeReport(report) : null;
  console.error(`NOT_READY ${options.gate}: missing package scripts: ${missingScripts.join(", ")}`);
  if (path) console.error(`report: ${path}`);
  process.exit(2);
}

const commands = [
  ...baselineCommands,
  ...requiredScripts.map((script) => ({
    id: script,
    command: pnpm,
    args: ["run", script],
  })),
];

for (const command of commands) {
  const commandStartedAt = new Date().toISOString();
  const start = performance.now();
  console.log(`\n[RUN] ${command.id}: ${commandText(command)}`);
  const result = spawnSync(command.command, command.args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env, CI: "1", NO_COLOR: "1", TZ: "UTC" },
    maxBuffer: 20 * 1024 * 1024,
    timeout: 20 * 60 * 1000,
  });
  const entry = {
    id: command.id,
    command: commandText(command),
    startedAt: commandStartedAt,
    durationMs: Math.round(performance.now() - start),
    exitCode: result.status,
    signal: result.signal,
    error: result.error?.message ?? null,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
  report.results.push(entry);
  if (entry.stdout) process.stdout.write(entry.stdout);
  if (entry.stderr) process.stderr.write(entry.stderr);
  if (result.status !== 0 || result.error) {
    report.status = "FAIL";
    break;
  }
}

if (report.status === "RUNNING") report.status = "PASS";
report.finishedAt = new Date().toISOString();
const path = options.report ? writeReport(report) : null;
console.log(`\n${report.status} ${report.gate}`);
if (path) console.log(`report: ${path}`);
process.exit(report.status === "PASS" ? 0 : 1);
