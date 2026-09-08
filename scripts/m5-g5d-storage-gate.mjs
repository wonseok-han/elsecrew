import { spawnSync } from "node:child_process";
import { lstatSync, mkdirSync, renameSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { qaEnvironment } from "./m5-qa-resources.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
let evidenceSequence = 0;

const GATES = {
  scheduler: {
    id: "g5d-storage-scheduler",
    version: 1,
    commands: [
      nodeTest("scheduler-contract", ["tests/m5-g5d-storage-activation-v4-contract.test.mts"]),
      cargoTest("scheduler-unit-and-sqlite", "cooperative_scheduler_"),
      cargoTest("scheduler-poison", "fatal_scheduler_error_poisons_later_mutation_and_agent_run"),
      cargoTest("scheduler-stdio", "binary_delayed_storage_still_emits_shell_ready_within_three_seconds"),
      diffCheck(),
    ],
  },
  status: {
    id: "g5d-storage-status",
    version: 1,
    commands: [
      nodeTest("status-contract-and-electron", [
        "tests/m5-g5d-lifetime-contract.test.mts",
        "tests/m5-g5d-admission-contract.test.mts",
        "tests/m5-g5d-electron-admission.test.mts",
        "tests/m5-electron-bridge.test.mts",
      ]),
      cargoTest("status-sqlite-facts", "storage_lifetime::tests::status_"),
      cargoTest("status-admission-boundary", "g5d_shell_ready_precedes_storage_and_verifying_blocks_before_params_or_side_effects"),
      cargoTest("status-ready-only-after-quit", "later_wave_method_is_structured_error_and_read_only_status_remains_available_after_quit"),
      diffCheck(),
    ],
  },
  compaction: {
    id: "g5d-storage-compaction",
    version: 1,
    commands: [
      nodeTest("v9-v10-contract", [
        "tests/m5-g5d-resume-witness-contract.test.mts",
        "tests/m5-g5d-delete-transaction-contract.test.mts",
      ]),
      cargoTest("v10-writer-and-failure-injection", "storage::tests::compaction_writer_v10::v10_writer_"),
      cargoTest("v9-v10-plan-aware-admission", "storage::tests::reopen_plan_v9::"),
      diffCheck(),
    ],
  },
};

function nodeTest(id, files) {
  return { id, command: process.execPath, args: ["--test", "--experimental-strip-types", ...files] };
}

function cargoTest(id, filter) {
  return {
    id,
    command: "cargo",
    args: ["test", "-p", "hello-agent-sidecar", "--locked", filter],
  };
}

function diffCheck() {
  return { id: "diff-check", command: "git", args: ["diff", "--check"] };
}

export function exitCodeForStorageGate(status) {
  return status === "PASS" ? 0 : status === "NOT_READY" ? 2 : 1;
}

export function runStorageGate(gateName, options = {}) {
  const gates = options.gates ?? GATES;
  const gate = gates[gateName];
  if (!gate || !isSafeGate(gate)) {
    return invalidGateOutcome(gateName);
  }

  const root = resolve(options.root ?? repositoryRoot);
  const run = options.spawnSync ?? spawnSync;
  const now = options.now ?? (() => new Date());
  const readGit = options.readGit ?? defaultReadGit;
  const environment = options.environment ?? qaEnvironment();
  const filesystem = {
    lstatSync: options.lstatSync ?? lstatSync,
    mkdirSync: options.mkdirSync ?? mkdirSync,
    writeFileSync: options.writeFileSync ?? writeFileSync,
    renameSync: options.renameSync ?? renameSync,
  };
  const startedAt = options.startedAt ?? now().toISOString();
  const head = readGit(root, ["rev-parse", "HEAD"]);
  const dirtyValue = readGit(root, ["status", "--porcelain"]);
  const report = {
    schemaVersion: 1,
    evidence: "m5-g5d-storage-gate",
    gate: gate.id,
    gateVersion: gate.version,
    status: "NOT_READY",
    startedAt,
    finishedAt: null,
    repository: root,
    head,
    dirtyBefore: dirtyValue === null ? null : dirtyValue !== "",
    resources: {
      cargoBuildJobs: environment.CARGO_BUILD_JOBS,
      rustTestThreads: environment.RUST_TEST_THREADS,
    },
    commands: [],
    reasons: [],
  };

  if (head === null || dirtyValue === null) {
    report.reasons.push({ code: "gitMetadataUnavailable", detail: "HEAD and clean-tree state must be readable before gate commands run." });
    return finish(report, root, filesystem, now);
  }
  if (report.dirtyBefore) {
    report.reasons.push({ code: "dirtyWorkingTree", detail: "Storage gate requires a clean working tree so evidence is bound to one HEAD." });
    return finish(report, root, filesystem, now);
  }

  for (const command of gate.commands) {
    if (!isCommand(command)) {
      report.status = "FAIL";
      report.reasons.push({ code: "missingCommand", detail: `Gate command ${String(command?.id ?? "unknown")} is malformed.` });
      return finish(report, root, filesystem, now);
    }
    const commandStartedAt = now().toISOString();
    const started = performance.now();
    let raw;
    try {
      raw = run(command.command, command.args, {
        cwd: root,
        encoding: "utf8",
        env: environment,
        maxBuffer: 20 * 1024 * 1024,
        timeout: 20 * 60 * 1000,
      });
    } catch (error) {
      raw = { status: null, signal: null, error };
    }
    const result = commandResult(command, raw, commandStartedAt, Math.round(performance.now() - started));
    report.commands.push(result);
    if (result.malformed) {
      report.status = "FAIL";
      report.reasons.push({ code: "malformedCommandResult", detail: `${command.id} did not return a usable process result.` });
      return finish(report, root, filesystem, now);
    }
    if (result.signal !== null || result.error !== null || result.exitCode !== 0) {
      report.status = "FAIL";
      report.reasons.push({ code: "commandFailed", detail: `${command.id} failed without a passing exit status.` });
      return finish(report, root, filesystem, now);
    }
  }

  report.status = "PASS";
  return finish(report, root, filesystem, now);
}

function isSafeGate(gate) {
  return typeof gate?.id === "string"
    && /^[a-z0-9-]+$/.test(gate.id)
    && Number.isSafeInteger(gate.version)
    && gate.version > 0
    && Array.isArray(gate.commands)
    && gate.commands.length > 0;
}

function isCommand(command) {
  return typeof command?.id === "string"
    && command.id.length > 0
    && typeof command.command === "string"
    && command.command.length > 0
    && Array.isArray(command.args)
    && command.args.every((value) => typeof value === "string");
}

function commandResult(command, raw, startedAt, durationMs) {
  const result = raw && typeof raw === "object" ? raw : null;
  const hasStatus = typeof result?.status === "number" || result?.status === null;
  const hasSignal = typeof result?.signal === "string" || result?.signal === null || result?.signal === undefined;
  const hasError = result?.error === undefined || result?.error === null || result?.error instanceof Error || typeof result?.error === "string";
  const malformed = !result || !hasStatus || !hasSignal || !hasError || (result.status === null && !result.signal && !result.error);
  return {
    id: command.id,
    command: [command.command, ...command.args],
    startedAt,
    durationMs,
    exitCode: typeof result?.status === "number" ? result.status : null,
    signal: typeof result?.signal === "string" ? result.signal : null,
    error: result?.error ? String(result.error) : null,
    stdout: typeof result?.stdout === "string" ? result.stdout : "",
    stderr: typeof result?.stderr === "string" ? result.stderr : "",
    malformed,
  };
}

function finish(report, root, filesystem, now) {
  report.finishedAt = now().toISOString();
  let evidencePath = null;
  try {
    evidencePath = writeEvidence(root, report, filesystem);
  } catch (error) {
    report.status = "FAIL";
    report.reasons.push({ code: "evidenceWriteFailed", detail: String(error) });
    evidencePath = null;
  }
  return { report, evidencePath, exitCode: exitCodeForStorageGate(report.status) };
}

export function writeEvidence(root, report, filesystem = { lstatSync, mkdirSync, writeFileSync, renameSync }) {
  const evidenceDirectory = safeEvidenceDirectory(root, filesystem);
  const timestamp = report.startedAt.replaceAll(":", "-").replaceAll(".", "-");
  const filename = `${report.gate}-v${report.gateVersion}-${timestamp}-${process.pid}-${evidenceSequence += 1}.json`;
  const finalPath = resolve(evidenceDirectory, filename);
  assertWithinDirectory(evidenceDirectory, finalPath);
  const temporaryPath = resolve(evidenceDirectory, `.${filename}.tmp`);
  assertWithinDirectory(evidenceDirectory, temporaryPath);
  filesystem.writeFileSync(temporaryPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  filesystem.renameSync(temporaryPath, finalPath);
  return finalPath;
}

export function safeEvidenceDirectory(root, filesystem = { lstatSync, mkdirSync, writeFileSync, renameSync }) {
  const repository = resolve(root);
  ensureNotSymlink(repository, filesystem.lstatSync, "repository root");
  let directory = repository;
  for (const segment of [".artifacts", "m5-g5d-storage"]) {
    directory = resolve(directory, segment);
    assertWithinDirectory(repository, directory);
    try {
      ensureNotSymlink(directory, filesystem.lstatSync, "evidence directory");
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
      filesystem.mkdirSync(directory, { recursive: false });
      ensureNotSymlink(directory, filesystem.lstatSync, "created evidence directory");
    }
  }
  return directory;
}

function ensureNotSymlink(path, lstat, label) {
  const metadata = lstat(path);
  if (metadata.isSymbolicLink()) throw new Error(`${label} must not be a symlink: ${path}`);
  if (!metadata.isDirectory()) throw new Error(`${label} must be a directory: ${path}`);
}

function assertWithinDirectory(directory, candidate) {
  const relation = relative(directory, candidate);
  if (relation === "" || relation === ".." || relation.startsWith(`..${process.platform === "win32" ? "\\\\" : "/"}`) || resolve(directory, relation) !== resolve(candidate)) {
    throw new Error(`evidence path escapes its directory: ${candidate}`);
  }
}

function defaultReadGit(root, args) {
  const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

function invalidGateOutcome(gateName) {
  return {
    report: null,
    evidencePath: null,
    exitCode: 64,
    error: `Unknown or unsafe G5D storage gate: ${String(gateName)}`,
  };
}

function parseArguments(args) {
  if (args.length === 2 && args[0] === "--gate" && Object.hasOwn(GATES, args[1])) return args[1];
  return null;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const gateName = parseArguments(process.argv.slice(2));
  if (!gateName) {
    process.stderr.write("Usage: node scripts/m5-g5d-storage-gate.mjs --gate <scheduler|status|compaction>\n");
    process.exitCode = 64;
  } else {
    const outcome = runStorageGate(gateName);
    process.stdout.write(`${JSON.stringify({ ...outcome.report, evidencePath: outcome.evidencePath })}\n`);
    process.exitCode = outcome.exitCode;
  }
}
