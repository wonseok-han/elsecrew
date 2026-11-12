import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { lowerQaPriority, qaEnvironment } from "./m5-qa-resources.mjs";

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
    cumulativeScripts: [],
  },
  {
    id: "g1-foundation",
    description: "Electron scaffold와 Rust core 추출",
    scripts: ["m5:test:electron", "m5:test:core"],
    cumulativeScripts: ["m5:test:electron", "m5:test:core"],
  },
  {
    id: "g2-contracts",
    description: "TypeScript/Rust protocol golden fixture",
    scripts: ["m5:test:contracts"],
    cumulativeScripts: ["m5:test:contracts:node"],
  },
  {
    id: "g3-storage",
    description: "sidecar protocol, SQLite replay와 preload 경계",
    scripts: ["m5:test:storage"],
    cumulativeScripts: ["m5:test:storage:node"],
  },
  {
    id: "g4-project-editor",
    description: "standalone snapshot 프로젝트 안전 API와 Monaco 편집·diff",
    scripts: ["m5:test:project", "m5:test:editor"],
    cumulativeScripts: ["m5:test:project:node", "m5:test:editor"],
  },
  {
    id: "g5-run-recovery",
    description: "다중 stream, 중단, 종료와 복구",
    scripts: ["m5:test:run-recovery"],
    cumulativeScripts: ["m5:test:run-recovery:node"],
  },
  {
    id: "g5d-bounded-storage",
    description: "내부 source index와 bounded materialization",
    scripts: ["m5:test:bounded-storage"],
    cumulativeScripts: ["m5:test:bounded-storage:node"],
  },
  {
    id: "g5d-storage-lifetime",
    description: "scheduler, storage.status wire와 lossless compaction",
    scripts: [
      "m5:test:storage-scheduler",
      "m5:test:storage-status",
      "m5:test:storage-compaction",
    ],
    cumulativeScripts: [
      "m5:test:storage-scheduler",
      "m5:test:storage-status",
      "m5:test:storage-compaction",
    ],
  },
  {
    id: "g6-release",
    description: "보안, 패키징, 성능과 M5.0 완료 게이트",
    scripts: ["m5:test:security", "m5:test:package", "m5:test:performance"],
    cumulativeScripts: ["m5:test:security", "m5:test:package", "m5:test:performance"],
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
  try {
    const result = spawnSync("git", args, { cwd: repoRoot, encoding: "utf8" });
    return result.status === 0 && typeof result.stdout === "string" ? result.stdout.trim() : null;
  } catch {
    return null;
  }
}

function gitState() {
  const head = gitValue(["rev-parse", "HEAD"]);
  const dirtyValue = gitValue(["status", "--porcelain"]);
  if (!head || dirtyValue === null) return null;
  return { head, dirty: dirtyValue !== "" };
}

function sameCleanState(expected) {
  const current = gitState();
  if (!current) return { ok: false, code: "gitMetadataUnavailable", detail: "HEAD and clean-tree state must remain readable for every harness command." };
  if (current.head !== expected.head) return { ok: false, code: "headChanged", detail: `HEAD changed from ${expected.head} to ${current.head}.` };
  if (current.dirty) return { ok: false, code: "workingTreeDirty", detail: "Working tree became dirty while the cumulative harness was running." };
  return { ok: true, current };
}

const storageEvidenceGates = new Map([
  ["m5:test:storage-scheduler", "g5d-storage-scheduler"],
  ["m5:test:storage-status", "g5d-storage-status"],
  ["m5:test:storage-compaction", "g5d-storage-compaction"],
]);

function validateStorageEvidence(script, stdout, expectedHead) {
  const expectedGate = storageEvidenceGates.get(script);
  if (!expectedGate) return null;
  const line = stdout.trim().split("\n").reverse().find((value) => value.trim().startsWith("{"));
  if (!line) return { code: "storageEvidenceMalformed", detail: `${script} did not emit a JSON evidence report.` };
  let evidence;
  try {
    evidence = JSON.parse(line);
  } catch {
    return { code: "storageEvidenceMalformed", detail: `${script} emitted invalid JSON evidence.` };
  }
  if (evidence?.gate !== expectedGate || evidence.status !== "PASS" || evidence.head !== expectedHead
    || evidence.dirtyBefore !== false || typeof evidence.evidencePath !== "string" || evidence.evidencePath.length === 0) {
    return { code: "storageEvidenceInvalid", detail: `${script} evidence is not a clean PASS for ${expectedGate} at ${expectedHead}.` };
  }
  return null;
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
  console.log("Command registration only; no measurements or gate results are assessed by --list.");
  for (const gate of gates) {
    const missing = [...new Set([...gate.scripts, ...gate.cumulativeScripts])].filter((script) => !packageScripts[script]);
    const state = missing.length === 0 ? "COMMANDS_READY" : "NOT_READY";
    console.log(`${gate.id.padEnd(22)} ${state.padEnd(14)} ${gate.description}`);
    if (missing.length) console.log(`  missing: ${missing.join(", ")}`);
  }
  process.exit(0);
}

const gateIndex = gates.findIndex((gate) => gate.id === options.gate);
if (gateIndex < 0) usage(`Unknown gate: ${options.gate}`);

const selectedGates = gates.slice(0, gateIndex + 1);
const requiredScripts = [...new Set(selectedGates.flatMap((gate) => [...gate.scripts, ...gate.cumulativeScripts]))];
const cumulativeScripts = selectedGates.flatMap((gate) => gate.cumulativeScripts);
const missingScripts = requiredScripts.filter((script) => !packageScripts[script]);
const startedAt = new Date().toISOString();
const childEnv = qaEnvironment();
const initialGitState = gitState();
const report = {
  schemaVersion: 2,
  gate: options.gate,
  status: "RUNNING",
  startedAt,
  finishedAt: null,
  repository: repoRoot,
  head: initialGitState?.head ?? null,
  headAfter: null,
  branch: gitValue(["branch", "--show-current"]),
  dirtyBefore: initialGitState?.dirty ?? null,
  dirtyAfter: null,
  platform: process.platform,
  node: process.version,
  resources: {
    profile: "low-impact",
    cargoBuildJobs: Number(childEnv.CARGO_BUILD_JOBS),
    rustTestThreads: Number(childEnv.RUST_TEST_THREADS),
    priority: null,
  },
  missingScripts,
  scriptPlan: selectedGates.map((gate) => ({
    gate: gate.id,
    canonicalScripts: gate.scripts,
    cumulativeScripts: gate.cumulativeScripts,
  })),
  results: [],
  reasons: [],
};

if (!initialGitState) {
  report.status = "NOT_READY";
  report.reasons.push({ code: "gitMetadataUnavailable", detail: "HEAD and clean-tree state must be readable before harness commands run." });
  report.finishedAt = new Date().toISOString();
  const path = options.report ? writeReport(report) : null;
  console.error(`NOT_READY ${options.gate}: git metadata unavailable`);
  if (path) console.error(`report: ${path}`);
  process.exit(2);
}

if (initialGitState.dirty) {
  report.status = "NOT_READY";
  report.reasons.push({ code: "dirtyWorkingTree", detail: "Cumulative harness requires a clean working tree before commands run." });
  report.finishedAt = new Date().toISOString();
  const path = options.report ? writeReport(report) : null;
  console.error(`NOT_READY ${options.gate}: working tree is dirty`);
  if (path) console.error(`report: ${path}`);
  process.exit(2);
}

if (missingScripts.length > 0) {
  report.status = "NOT_READY";
  report.reasons.push({ code: "missingScripts", detail: `Required package scripts are missing: ${missingScripts.join(", ")}.` });
  report.finishedAt = new Date().toISOString();
  const path = options.report ? writeReport(report) : null;
  console.error(`NOT_READY ${options.gate}: missing package scripts: ${missingScripts.join(", ")}`);
  if (path) console.error(`report: ${path}`);
  process.exit(2);
}

report.resources.priority = lowerQaPriority();
if (report.resources.priority.error) {
  console.warn(`[WARN] QA priority: ${report.resources.priority.error}; worker limits still apply.`);
}
console.log(`[QA] low-impact: cargo jobs=${childEnv.CARGO_BUILD_JOBS}, Rust test threads=${childEnv.RUST_TEST_THREADS}, priority=${report.resources.priority.actual ?? "unavailable"}`);

const commands = [
  ...baselineCommands,
  ...cumulativeScripts.map((script) => ({
    id: script,
    command: pnpm,
    args: ["run", script],
  })),
];

for (const command of commands) {
  const before = sameCleanState(initialGitState);
  if (!before.ok) {
    report.status = "FAIL";
    report.reasons.push(before);
    break;
  }
  const commandStartedAt = new Date().toISOString();
  const start = performance.now();
  console.log(`\n[RUN] ${command.id}: ${commandText(command)}`);
  const result = spawnSync(command.command, command.args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: childEnv,
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
  const after = sameCleanState(initialGitState);
  if (!after.ok) {
    report.status = "FAIL";
    report.reasons.push(after);
    break;
  }
  if (result.status === 2 && !result.error) {
    report.status = "NOT_READY";
    report.reasons.push({ code: "commandNotReady", detail: `${command.id} returned exit 2 (NOT_READY).` });
    break;
  }
  if (result.status !== 0 || result.error) {
    report.status = "FAIL";
    report.reasons.push({ code: "commandFailed", detail: `${command.id} failed with exit ${entry.exitCode ?? "none"}${entry.signal ? ` or signal ${entry.signal}` : ""}.` });
    break;
  }
  const storageFailure = validateStorageEvidence(command.id, entry.stdout, initialGitState.head);
  if (storageFailure) {
    report.status = "FAIL";
    report.reasons.push(storageFailure);
    break;
  }
}

if (report.status === "RUNNING") report.status = "PASS";
const finalGitState = gitState();
report.headAfter = finalGitState?.head ?? null;
report.dirtyAfter = finalGitState?.dirty ?? null;
report.finishedAt = new Date().toISOString();
const path = options.report ? writeReport(report) : null;
console.log(`\n${report.status} ${report.gate}`);
if (path) console.log(`report: ${path}`);
process.exit(report.status === "PASS" ? 0 : report.status === "NOT_READY" ? 2 : 1);
