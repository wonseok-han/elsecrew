import { spawnSync } from "node:child_process";
import { lstatSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const APP_NAME = "Elsecrew";

export function expectedPackageArtifacts({ root: repository, platform, arch, version }) {
  const packageDirectory = resolve(repository, "out", `${APP_NAME}-${platform}-${arch}`);
  const archive = resolve(repository, "out", "make", "zip", platform, arch, `${APP_NAME}-${platform}-${arch}-${version}.zip`);
  const assets = platform === "darwin"
    ? [
      `${APP_NAME}.app/Contents/MacOS/${APP_NAME}`,
      `${APP_NAME}.app/Contents/Resources/app.asar`,
      `${APP_NAME}.app/Contents/Resources/sidecar/elsecrew-sidecar`,
    ]
    : [
      `${APP_NAME}.exe`,
      "resources/app.asar",
      "resources/sidecar/elsecrew-sidecar.exe",
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

  const preflight = validateExistingArtifactPaths(repository, artifacts.requiredOutputs, lstat);
  if (preflight.length > 0) {
    report.reasons.push(...preflight);
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

  const packageProvenance = validateFreshPackageArtifacts(repository, artifacts, startedAtMs, lstat);
  if (packageProvenance.length > 0) {
    report.reasons.push(...packageProvenance);
    return finish(report, repository, makeDirectory, writeFile, now);
  }

  const smokeCommand = { id: "smoke", command: process.execPath, args: ["scripts/m5-electron-package-smoke.mjs"] };
  if (!runCommand(smokeCommand)) return finish(report, repository, makeDirectory, writeFile, now);
  const makeCommand = { id: "make", command: "pnpm", args: ["exec", "electron-forge", "make", "--skip-package"] };
  if (!runCommand(makeCommand)) return finish(report, repository, makeDirectory, writeFile, now);

  const archiveProvenance = [
    ...validateFreshArtifacts([artifacts.archive], startedAtMs, lstat),
    ...validateArtifactAncestors(repository, [artifacts.archive], lstat),
  ];
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

export function validateFreshPackageArtifacts(repository, artifacts, startedAtMs, lstat = lstatSync) {
  const packagePaths = artifacts.requiredOutputs.slice(0, -1);
  return [
    ...validateFreshArtifacts(packagePaths, startedAtMs, lstat),
    ...validateArtifactAncestors(repository, packagePaths, lstat),
  ];
}

export function validateExistingArtifactPaths(repository, paths, lstat = lstatSync) {
  const reasons = [];
  const seen = new Set();
  for (const target of paths) {
    for (const path of artifactPathComponents(repository, target)) {
      if (seen.has(path)) continue;
      seen.add(path);
      try {
        if (lstat(path).isSymbolicLink()) {
          reasons.push({ code: "symlinkArtifact", detail: `existing artifact path is a symlink: ${path}` });
        }
      } catch (error) {
        if (error?.code === "ENOENT") break;
        reasons.push({ code: "unreadableArtifactPath", detail: `cannot inspect artifact path: ${path} (${String(error)})` });
        break;
      }
    }
  }
  return reasons;
}

export function validateArtifactAncestors(repository, paths, lstat = lstatSync) {
  const reasons = [];
  const directPaths = new Set(paths);
  const seen = new Set();
  for (const target of paths) {
    for (const path of artifactPathComponents(repository, target)) {
      if (directPaths.has(path) || seen.has(path)) continue;
      seen.add(path);
      try {
        if (lstat(path).isSymbolicLink()) {
          reasons.push({ code: "symlinkArtifact", detail: `required artifact parent is a symlink: ${path}` });
        }
      } catch (error) {
        reasons.push({ code: "missingArtifact", detail: `required artifact parent is missing: ${path} (${String(error)})` });
      }
    }
  }
  return reasons;
}

function artifactPathComponents(repository, target) {
  const parts = relative(repository, target).split(/[\\/]/).filter(Boolean);
  if (parts.length === 0 || parts.some((part) => part === "..")) throw new Error(`artifact path escapes repository: ${target}`);
  const paths = [];
  let current = repository;
  for (const part of parts) {
    current = resolve(current, part);
    paths.push(current);
  }
  return paths;
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
