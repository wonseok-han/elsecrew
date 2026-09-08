import { spawnSync } from "node:child_process";
import { lstatSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const APP_NAME = "hello-agent";

export function expectedPackageArtifacts({ root: repository, platform, arch, version }) {
  const packageDirectory = resolve(repository, "out", `${APP_NAME}-${platform}-${arch}`);
  const archive = resolve(repository, "out", "make", "zip", platform, arch, `${APP_NAME}-${platform}-${arch}-${version}.zip`);
  const assets = platform === "darwin"
    ? [
      `${APP_NAME}.app/Contents/MacOS/${APP_NAME}`,
      `${APP_NAME}.app/Contents/Resources/app.asar`,
      `${APP_NAME}.app/Contents/Resources/sidecar/hello-agent-sidecar`,
    ]
    : [
      `${APP_NAME}.exe`,
      "resources/app.asar",
      "resources/sidecar/hello-agent-sidecar.exe",
    ];
  return {
    packageDirectory,
    archive,
    requiredOutputs: [packageDirectory, ...assets.map((asset) => resolve(packageDirectory, asset)), archive],
  };
}

export function targetSupport(platform, arch) {
  if (platform === "darwin" && arch === "arm64") return { supported: true, reason: null };
  if (platform === "win32" && (arch === "x64" || arch === "arm64")) return { supported: true, reason: null };
  if (platform === "linux") return { supported: false, reason: "Linux package gate is not implemented." };
  return { supported: false, reason: `Unsupported M5 package gate target: ${platform}-${arch}.` };
}

export function exitCodeForPackageGate(status) {
  return status === "PASS" ? 0 : status === "FAIL" ? 1 : 2;
}

export function runPackageGate(options = {}) {
  const repository = options.root ?? root;
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const readFile = options.readFileSync ?? readFileSync;
  const lstat = options.lstatSync ?? lstatSync;
  const makeDirectory = options.mkdirSync ?? mkdirSync;
  const writeFile = options.writeFileSync ?? writeFileSync;
  const run = options.spawnSync ?? spawnSync;
  const now = options.now ?? (() => new Date());
  const readGit = options.readGit ?? defaultReadGit;
  const startedAt = options.startedAt ?? now().toISOString();
  const startedAtMs = Date.parse(startedAt);
  const version = options.version ?? JSON.parse(readFile(resolve(repository, "package.json"), "utf8")).version;
  const artifacts = expectedPackageArtifacts({ root: repository, platform, arch, version });
  const report = {
    schemaVersion: 1,
    evidence: "m5-g6-package-gate",
    startedAt,
    finishedAt: null,
    status: "NOT_READY",
    platform,
    arch,
    head: readGit(repository, ["rev-parse", "HEAD"]),
    dirtyBefore: readGit(repository, ["status", "--porcelain"]) !== "",
    artifacts: { packageDirectory: artifacts.packageDirectory, archive: artifacts.archive, requiredOutputs: artifacts.requiredOutputs },
    results: [],
    reasons: [],
    metrics: null,
    limitations: [
      "This incremental gate does not establish signing, notarization, Windows device behavior, GUI behavior, release distribution, or overall G6 readiness.",
    ],
  };

  const support = targetSupport(platform, arch);
  if (!support.supported) {
    report.reasons.push({ code: "unsupportedTarget", detail: support.reason });
    return finish(report, repository, makeDirectory, writeFile, now);
  }
  if (report.dirtyBefore) {
    report.reasons.push({ code: "dirtyWorkingTree", detail: "package gate requires a clean working tree to bind evidence to HEAD." });
    return finish(report, repository, makeDirectory, writeFile, now);
  }

  const runCommand = (command) => {
    const result = commandResult(command, run(command.command, command.args, { cwd: repository, encoding: "utf8", stdio: "inherit" }));
    report.results.push(result);
    if (result.exitCode !== 0) {
      report.status = "FAIL";
      report.reasons.push({ code: "commandFailed", detail: `${command.id} failed.` });
      return false;
    }
    return true;
  };
  const packageCommand = { id: "package", command: "pnpm", args: ["exec", "electron-forge", "package"] };
  if (!runCommand(packageCommand)) return finish(report, repository, makeDirectory, writeFile, now);

  const packageProvenance = validateFreshPackageArtifacts(artifacts, startedAtMs, lstat);
  if (packageProvenance.length > 0) {
    report.reasons.push(...packageProvenance);
    return finish(report, repository, makeDirectory, writeFile, now);
  }

  const smokeCommand = { id: "smoke", command: process.execPath, args: ["scripts/m5-electron-package-smoke.mjs"] };
  if (!runCommand(smokeCommand)) return finish(report, repository, makeDirectory, writeFile, now);
  const makeCommand = { id: "make", command: "pnpm", args: ["exec", "electron-forge", "make", "--skip-package"] };
  if (!runCommand(makeCommand)) return finish(report, repository, makeDirectory, writeFile, now);

  const archiveProvenance = validateFreshArtifacts([artifacts.archive], startedAtMs, lstat);
  if (archiveProvenance.length > 0) {
    report.reasons.push(...archiveProvenance);
    return finish(report, repository, makeDirectory, writeFile, now);
  }

  const metricsCommand = {
    id: "metrics",
    command: process.execPath,
    args: [
      "scripts/m5-package-metrics.mjs",
      "--platform", platform,
      "--arch", arch,
      "--package", artifacts.packageDirectory,
      "--archive", artifacts.archive,
    ],
  };
  const metricsRaw = run(metricsCommand.command, metricsCommand.args, { cwd: repository, encoding: "utf8" });
  const metricsResult = commandResult(metricsCommand, metricsRaw);
  report.results.push(metricsResult);
  const metrics = parseMetrics(metricsRaw?.stdout, artifacts);
  if (!metrics) {
    report.reasons.push({ code: "invalidMetricsReport", detail: "package metrics did not return the expected JSON report." });
    return finish(report, repository, makeDirectory, writeFile, now);
  }
  report.metrics = metrics;
  report.status = metrics.status;
  if (metricsResult.exitCode !== exitCodeForPackageGate(metrics.status)) {
    report.status = "NOT_READY";
    report.reasons.push({ code: "metricsExitMismatch", detail: "package metrics status and process exit code disagree." });
  }
  return finish(report, repository, makeDirectory, writeFile, now);
}

export function validateFreshArtifacts(paths, startedAtMs, lstat = lstatSync) {
  const reasons = [];
  for (const path of paths) {
    try {
      const metadata = lstat(path);
      if (metadata.isSymbolicLink()) {
        reasons.push({ code: "symlinkArtifact", detail: `required artifact is a symlink: ${path}` });
      } else if (metadata.mtimeMs < startedAtMs) {
        reasons.push({ code: "staleArtifact", detail: `required artifact was not updated by this run: ${path}` });
      }
    } catch (error) {
      reasons.push({ code: "missingArtifact", detail: `required artifact is missing: ${path} (${String(error)})` });
    }
  }
  return reasons;
}

export function validateFreshPackageArtifacts(artifacts, startedAtMs, lstat = lstatSync) {
  const packagePaths = artifacts.requiredOutputs.slice(0, -1);
  const reasons = validateFreshArtifacts(packagePaths, startedAtMs, lstat);
  const seen = new Set();
  for (const asset of packagePaths.slice(1)) {
    const parts = relative(artifacts.packageDirectory, asset).split(/[\\/]/).filter(Boolean);
    let current = artifacts.packageDirectory;
    for (const part of parts.slice(0, -1)) {
      current = resolve(current, part);
      if (seen.has(current)) continue;
      seen.add(current);
      try {
        if (lstat(current).isSymbolicLink()) {
          reasons.push({ code: "symlinkArtifact", detail: `required package path is a symlink: ${current}` });
        }
      } catch (error) {
        reasons.push({ code: "missingArtifact", detail: `required package path is missing: ${current} (${String(error)})` });
      }
    }
  }
  return reasons;
}

function commandResult(command, raw) {
  return {
    id: command.id,
    command: [command.command, ...command.args],
    exitCode: typeof raw?.status === "number" ? raw.status : null,
    error: raw?.error ? String(raw.error) : null,
  };
}

function parseMetrics(stdout, artifacts) {
  try {
    const report = JSON.parse(typeof stdout === "string" ? stdout : "");
    if (!report || !["PASS", "FAIL", "NOT_READY"].includes(report.status)) return null;
    if (resolve(report.package?.path ?? "") !== artifacts.packageDirectory) return null;
    if (resolve(report.archive?.path ?? "") !== artifacts.archive) return null;
    return report;
  } catch {
    return null;
  }
}

function defaultReadGit(repository, args) {
  const result = spawnSync("git", args, { cwd: repository, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}

function finish(report, repository, makeDirectory, writeFile, now) {
  report.finishedAt = now().toISOString();
  const timestamp = report.startedAt.replaceAll(":", "-").replaceAll(".", "-");
  const evidencePath = resolve(repository, ".artifacts", "m5-g6-package", `${report.platform}-${report.arch}-${timestamp}.json`);
  makeDirectory(dirname(evidencePath), { recursive: true });
  writeFile(evidencePath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  return { report, evidencePath, exitCode: exitCodeForPackageGate(report.status) };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const outcome = runPackageGate();
  process.stdout.write(`${JSON.stringify({ ...outcome.report, evidencePath: outcome.evidencePath })}\n`);
  process.exitCode = outcome.exitCode;
}
