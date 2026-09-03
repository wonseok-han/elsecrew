/**
 * Package-size-only evidence collector for the provisional M5 G6 limits.
 *
 * Usage:
 *   node scripts/m5-package-metrics.mjs --platform darwin --arch arm64 \
 *     --package /path/to/hello-agent-darwin-arm64 --archive /path/to/HelloAgent-darwin-arm64.zip
 *
 * The supplied package is never modified or launched. `unpackedBytes` is the
 * sum of regular-file byte lengths plus symlink link-text byte lengths; it
 * excludes directory metadata/allocation and never follows symlinks. This is
 * package-size-only evidence, not proof of freshness, signing, launchability,
 * runtime behavior, or G6 readiness.
 */
import { open, opendir, lstat, readlink } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const MACOS_ARM64_LIMITS = Object.freeze({
  unpackedBytes: 350n * 1024n * 1024n,
  compressedBytes: 200n * 1024n * 1024n,
});

const MAX_DIRECTORY_ENTRIES = 100_000;
const MAX_TOTAL_ENTRIES = 1_000_000;
const MAX_DIRECTORY_DEPTH = 128;
const ZIP_SIGNATURES = new Set(["504b0304", "504b0506", "504b0708"]);

class PackageMetricsError extends Error {
  constructor(code, detail) {
    super(detail);
    this.code = code;
  }
}

export function parsePackageMetricsArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (!new Set(["--platform", "--arch", "--package", "--archive"]).has(flag)) {
      throw new PackageMetricsError("invalidArgument", `unsupported argument: ${String(flag)}`);
    }
    if (Object.hasOwn(options, flag.slice(2))) {
      throw new PackageMetricsError("invalidArgument", `duplicate argument: ${flag}`);
    }
    const value = argv[index + 1];
    if (typeof value !== "string" || value.length === 0 || value.startsWith("--")) {
      throw new PackageMetricsError("invalidArgument", `${flag} requires one non-empty value`);
    }
    options[flag.slice(2)] = value;
    index += 1;
  }
  for (const required of ["platform", "arch", "package"]) {
    if (!Object.hasOwn(options, required)) {
      throw new PackageMetricsError("missingArgument", `missing required argument: --${required}`);
    }
  }
  validateTarget(options.platform, options.arch);
  return options;
}

export async function collectPackageMetrics(input) {
  const options = validateOptions(input);
  const packagePath = resolve(options.package);
  const archivePath = options.archive ? resolve(options.archive) : null;
  const report = initialReport(options.platform, options.arch, packagePath, archivePath);

  try {
    await assertDirectory(packagePath, "package directory");
    await assertRecognizedLayout(packagePath, options.platform);
    const unpacked = await measureUnpacked(packagePath);
    report.package.unpacked = unpacked;
    await assertRecognizedLayout(packagePath, options.platform);

    if (archivePath) {
      report.archive.compressed = await measureArchive(archivePath);
    } else {
      report.reasons.push(reason(
        "missingArchive",
        "compressed artifact was not supplied; macOS threshold evidence is incomplete",
      ));
    }
  } catch (error) {
    const failure = toFailureReason(error);
    report.status = "NOT_READY";
    if (failure.code === "filesystemChangedDuringMeasurement") {
      report.snapshot.observation = "invalidatedByDetectedChange";
    }
    report.reasons.push(failure);
    return report;
  }

  applyAssessment(report);
  return report;
}

export async function runPackageMetricsCli(argv = process.argv.slice(2), write = console.log) {
  let report;
  try {
    report = await collectPackageMetrics(parsePackageMetricsArgs(argv));
  } catch (error) {
    report = invalidArgumentsReport(error);
  }
  write(JSON.stringify(report));
  return exitCodeFor(report.status);
}

function validateOptions(input) {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    throw new PackageMetricsError("invalidArgument", "options must be an object");
  }
  const { platform, arch, package: packageDirectory, archive } = input;
  if (typeof platform !== "string" || typeof arch !== "string" || typeof packageDirectory !== "string" ||
      packageDirectory.length === 0) {
    throw new PackageMetricsError("invalidArgument", "platform, arch, and package must be non-empty strings");
  }
  if (archive !== undefined && (typeof archive !== "string" || archive.length === 0)) {
    throw new PackageMetricsError("invalidArgument", "archive must be a non-empty string when supplied");
  }
  validateTarget(platform, arch);
  return { platform, arch, package: packageDirectory, archive };
}

function validateTarget(platform, arch) {
  const supported = (platform === "darwin" && arch === "arm64") ||
    (platform === "win32" && (arch === "x64" || arch === "arm64"));
  if (!supported) {
    throw new PackageMetricsError("unsupportedTarget", `unsupported package target: ${platform}-${arch}`);
  }
}

function initialReport(platform, arch, packagePath, archivePath) {
  const macos = platform === "darwin" && arch === "arm64";
  return {
    schemaVersion: 1,
    evidence: "package-size-only",
    evidenceScope: "localDeveloperOnly",
    platform,
    arch,
    status: "NOT_READY",
    reasons: [],
    snapshot: {
      atomic: false,
      observation: "metadataStableDuringWalkOnly",
      limitation: "No filesystem walk is an atomic package snapshot; detected changes invalidate this measurement.",
    },
    package: {
      path: packagePath,
      expectedLayout: expectedLayout(platform),
      unpacked: null,
    },
    archive: archivePath ? { path: archivePath, compressed: null } : null,
    thresholds: macos ? {
      unpackedMaxBytes: bytes(MACOS_ARM64_LIMITS.unpackedBytes),
      compressedMaxBytes: bytes(MACOS_ARM64_LIMITS.compressedBytes),
    } : null,
    assessment: {
      unpacked: "NOT_READY",
      compressed: "NOT_READY",
    },
    limitations: [
      "This does not establish artifact freshness, provenance, signing, notarization, launchability, GUI behavior, runtime behavior, or G6 readiness.",
    ],
  };
}

function expectedLayout(platform) {
  if (platform === "darwin") {
    return [
      "hello-agent.app/Contents/MacOS/hello-agent",
      "hello-agent.app/Contents/Resources/app.asar",
      "hello-agent.app/Contents/Resources/sidecar/hello-agent-sidecar",
    ];
  }
  return [
    "hello-agent.exe",
    "resources/app.asar",
    "resources/sidecar/hello-agent-sidecar.exe",
  ];
}

async function assertRecognizedLayout(packagePath, platform) {
  for (const relativePath of expectedLayout(platform)) {
    const path = resolve(packagePath, relativePath);
    assertWithin(packagePath, path);
    await assertRegularFile(path, `required package asset ${relativePath}`);
  }
}

async function measureUnpacked(root) {
  const rootSnapshot = snapshot(await assertDirectory(root, "package directory"));
  const totals = { regularFileBytes: 0n, symlinkTextBytes: 0n, regularFiles: 0, symlinks: 0, directories: 0 };
  const state = { totalEntries: 0 };
  await walkDirectory(root, root, 0, totals, state);
  const finished = snapshot(await assertDirectory(root, "package directory"));
  assertStable(root, rootSnapshot, finished);
  return {
    bytes: bytes(totals.regularFileBytes + totals.symlinkTextBytes),
    regularFileBytes: bytes(totals.regularFileBytes),
    symlinkTextBytes: bytes(totals.symlinkTextBytes),
    regularFiles: totals.regularFiles,
    symlinks: totals.symlinks,
    directories: totals.directories,
    definition: "sum of regular-file byte lengths plus symlink link-text bytes; directories and filesystem allocation are excluded; symlinks are not followed",
  };
}

async function walkDirectory(root, directory, depth, totals, state) {
  if (depth > MAX_DIRECTORY_DEPTH) {
    throw new PackageMetricsError("directoryDepthExceeded", `directory depth exceeds ${MAX_DIRECTORY_DEPTH}: ${directory}`);
  }
  const before = snapshot(await assertDirectory(directory, "walked directory"));
  totals.directories += 1;
  let directoryEntries = 0;
  let handle;
  try {
    handle = await openDirectory(directory);
    for await (const entry of handle) {
      directoryEntries += 1;
      state.totalEntries += 1;
      if (directoryEntries > MAX_DIRECTORY_ENTRIES) {
        throw new PackageMetricsError("directoryEntryLimitExceeded", `directory exceeds ${MAX_DIRECTORY_ENTRIES} entries: ${directory}`);
      }
      if (state.totalEntries > MAX_TOTAL_ENTRIES) {
        throw new PackageMetricsError("totalEntryLimitExceeded", `package exceeds ${MAX_TOTAL_ENTRIES} entries`);
      }
      const path = resolve(directory, entry.name);
      assertWithin(root, path);
      const beforeEntry = snapshot(await lstatChecked(path, "package entry"));
      if (beforeEntry.kind === "file") {
        totals.regularFileBytes += beforeEntry.size;
        totals.regularFiles += 1;
      } else if (beforeEntry.kind === "symlink") {
        const linkText = await readlinkChecked(path);
        totals.symlinkTextBytes += BigInt(linkText.byteLength);
        totals.symlinks += 1;
      } else if (beforeEntry.kind === "directory") {
        await walkDirectory(root, path, depth + 1, totals, state);
      } else {
        throw new PackageMetricsError("specialFile", `unsupported package entry type at ${path}`);
      }
      const afterEntry = snapshot(await lstatChecked(path, "package entry"));
      assertStable(path, beforeEntry, afterEntry);
    }
  } finally {
    if (handle) await handle.close().catch(() => undefined);
  }
  const after = snapshot(await assertDirectory(directory, "walked directory"));
  assertStable(directory, before, after);
}

async function measureArchive(path) {
  const before = snapshot(await assertRegularFile(path, "archive"));
  let zipSignature = null;
  if (extname(path).toLowerCase() === ".zip") {
    const handle = await openChecked(path);
    try {
      const buffer = Buffer.alloc(4);
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      const signature = buffer.subarray(0, bytesRead).toString("hex");
      if (!ZIP_SIGNATURES.has(signature)) {
        throw new PackageMetricsError("invalidZipSignature", `archive has a .zip extension but no recognized ZIP signature: ${path}`);
      }
      zipSignature = signature;
    } finally {
      await handle.close();
    }
  }
  const after = snapshot(await assertRegularFile(path, "archive"));
  assertStable(path, before, after);
  return { bytes: bytes(before.size), zipSignature };
}

async function assertDirectory(path, label) {
  const stats = await lstatChecked(path, label);
  if (!stats.isDirectory()) throw new PackageMetricsError("wrongKind", `${label} must be a directory: ${path}`);
  return stats;
}

async function assertRegularFile(path, label) {
  const stats = await lstatChecked(path, label);
  if (!stats.isFile()) throw new PackageMetricsError("wrongKind", `${label} must be a regular file: ${path}`);
  return stats;
}

async function lstatChecked(path, label) {
  try {
    return await lstat(path, { bigint: true });
  } catch (error) {
    throw new PackageMetricsError("unreadable", `${label} is missing or unreadable: ${path} (${nodeError(error)})`);
  }
}

async function openDirectory(path) {
  try {
    return await opendir(path, { bufferSize: 128 });
  } catch (error) {
    throw new PackageMetricsError("unreadable", `directory is unreadable: ${path} (${nodeError(error)})`);
  }
}

async function readlinkChecked(path) {
  try {
    return await readlink(path, "buffer");
  } catch (error) {
    throw new PackageMetricsError("unreadable", `symlink is unreadable: ${path} (${nodeError(error)})`);
  }
}

async function openChecked(path) {
  try {
    return await open(path, "r");
  } catch (error) {
    throw new PackageMetricsError("unreadable", `archive is unreadable: ${path} (${nodeError(error)})`);
  }
}

function snapshot(stats) {
  const kind = stats.isFile() ? "file" : stats.isDirectory() ? "directory" : stats.isSymbolicLink() ? "symlink" : "special";
  return {
    kind,
    dev: stats.dev,
    ino: stats.ino,
    mode: stats.mode,
    size: stats.size,
    mtimeNs: stats.mtimeNs,
    ctimeNs: stats.ctimeNs,
  };
}

function assertStable(path, before, after) {
  for (const key of ["kind", "dev", "ino", "mode", "size", "mtimeNs", "ctimeNs"]) {
    if (before[key] !== after[key]) {
      throw new PackageMetricsError("filesystemChangedDuringMeasurement", `filesystem metadata changed during measurement: ${path}`);
    }
  }
}

function assertWithin(root, candidate) {
  if (candidate !== root && !candidate.startsWith(`${root}${sep}`)) {
    throw new PackageMetricsError("pathEscape", `package entry escaped supplied root: ${candidate}`);
  }
}

function applyAssessment(report) {
  if (report.platform === "win32") {
    report.status = "NOT_READY";
    report.reasons.push(reason("windowsMeasureOnly", "Windows package sizes were measured, but no G6 package-size threshold is defined."));
    return;
  }
  const unpacked = BigInt(report.package.unpacked.bytes.value);
  report.assessment.unpacked = unpacked <= MACOS_ARM64_LIMITS.unpackedBytes ? "PASS" : "FAIL";
  if (!report.archive?.compressed) {
    report.status = "NOT_READY";
    return;
  }
  const compressed = BigInt(report.archive.compressed.bytes.value);
  report.assessment.compressed = compressed <= MACOS_ARM64_LIMITS.compressedBytes ? "PASS" : "FAIL";
  report.status = report.assessment.unpacked === "PASS" && report.assessment.compressed === "PASS" ? "PASS" : "FAIL";
  if (report.status === "FAIL") {
    if (report.assessment.unpacked === "FAIL") {
      report.reasons.push(reason("unpackedLimitExceeded", "unpacked package bytes exceed the macOS arm64 limit"));
    }
    if (report.assessment.compressed === "FAIL") {
      report.reasons.push(reason("compressedLimitExceeded", "compressed artifact bytes exceed the macOS arm64 limit"));
    }
  }
}

function bytes(value) {
  return { value: value.toString(), unit: "bytes" };
}

function reason(code, detail) {
  return { code, detail };
}

function toFailureReason(error) {
  if (error instanceof PackageMetricsError) return reason(error.code, error.message);
  return reason("measurementFailed", nodeError(error));
}

function invalidArgumentsReport(error) {
  return {
    schemaVersion: 1,
    evidence: "package-size-only",
    evidenceScope: "localDeveloperOnly",
    status: "NOT_READY",
    reasons: [toFailureReason(error)],
    limitations: ["No package measurement was performed."],
  };
}

function exitCodeFor(status) {
  return status === "PASS" ? 0 : status === "FAIL" ? 1 : 2;
}

function nodeError(error) {
  return error instanceof Error ? error.message : String(error);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await runPackageMetricsCli();
}
