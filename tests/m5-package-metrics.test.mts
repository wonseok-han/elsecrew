import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, rm, stat, symlink, truncate, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  MACOS_ARM64_LIMITS,
  collectPackageMetrics,
  parsePackageMetricsArgs,
} from "../scripts/m5-package-metrics.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const script = resolve(root, "scripts/m5-package-metrics.mjs");

test("macOS arm64 package metrics pass exactly at and below both byte limits", async () => {
  await withFixture(async (fixture) => {
    await setPackageBytes(fixture, MACOS_ARM64_LIMITS.unpackedBytes);
    await setArchiveBytes(fixture.archive, MACOS_ARM64_LIMITS.compressedBytes);
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "PASS");
    assert.equal(report.package.unpacked.bytes.value, MACOS_ARM64_LIMITS.unpackedBytes.toString());
    assert.equal(report.archive?.compressed?.bytes.value, MACOS_ARM64_LIMITS.compressedBytes.toString());
  });
  await withFixture(async (fixture) => {
    await setPackageBytes(fixture, MACOS_ARM64_LIMITS.unpackedBytes - 1n);
    await setArchiveBytes(fixture.archive, MACOS_ARM64_LIMITS.compressedBytes - 1n);
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "PASS");
    assert.equal(report.assessment.unpacked, "PASS");
    assert.equal(report.assessment.compressed, "PASS");
  });
});

test("macOS arm64 package metrics fail when either exact-byte threshold is exceeded", async () => {
  await withFixture(async (fixture) => {
    await setPackageBytes(fixture, MACOS_ARM64_LIMITS.unpackedBytes + 1n);
    await setArchiveBytes(fixture.archive, MACOS_ARM64_LIMITS.compressedBytes + 1n);
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "FAIL");
    assert.equal(report.assessment.unpacked, "FAIL");
    assert.equal(report.assessment.compressed, "FAIL");
    assert.deepEqual(report.reasons.map((entry) => entry.code), ["unpackedLimitExceeded", "compressedLimitExceeded"]);
  });
});

test("missing required package evidence, wrong kinds, and invalid ZIP signatures are NOT_READY", async () => {
  await withFixture(async (fixture) => {
    await unlink(resolve(fixture.package, "hello-agent.app/Contents/Resources/sidecar/hello-agent-sidecar"));
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "NOT_READY");
    assert.equal(report.reasons[0]?.code, "unreadable");
  });
  await withFixture(async (fixture) => {
    const sidecar = resolve(fixture.package, "hello-agent.app/Contents/Resources/sidecar/hello-agent-sidecar");
    await unlink(sidecar);
    await mkdir(sidecar);
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "NOT_READY");
    assert.equal(report.reasons[0]?.code, "wrongKind");
  });
  await withFixture(async (fixture) => {
    await rm(fixture.archive);
    await mkdir(fixture.archive);
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "NOT_READY");
    assert.equal(report.reasons[0]?.code, "wrongKind");
  });
  await withFixture(async (fixture) => {
    await writeFile(fixture.archive, "NOPE");
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "NOT_READY");
    assert.equal(report.reasons[0]?.code, "invalidZipSignature");
  });
});

test("each required product asset must be non-empty", async () => {
  for (const relativePath of [
    "hello-agent.app/Contents/MacOS/hello-agent",
    "hello-agent.app/Contents/Resources/app.asar",
    "hello-agent.app/Contents/Resources/sidecar/hello-agent-sidecar",
  ]) {
    await withFixture(async (fixture) => {
      await truncate(resolve(fixture.package, relativePath), 0);
      const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
      assert.equal(report.status, "NOT_READY", relativePath);
      assert.equal(report.reasons[0]?.code, "emptyRequiredAsset", relativePath);
    });
  }
});

test("fixed package-layout parents cannot be symlinks to external assets", async () => {
  const cases = [
    {
      parent: "hello-agent.app",
      files: [
        ["Contents/MacOS/hello-agent", "external executable"],
        ["Contents/Resources/app.asar", "external asar"],
        ["Contents/Resources/sidecar/hello-agent-sidecar", "external sidecar"],
      ],
    },
    {
      parent: "hello-agent.app/Contents",
      files: [
        ["MacOS/hello-agent", "external executable"],
        ["Resources/app.asar", "external asar"],
        ["Resources/sidecar/hello-agent-sidecar", "external sidecar"],
      ],
    },
    {
      parent: "hello-agent.app/Contents/Resources",
      files: [
        ["app.asar", "external asar"],
        ["sidecar/hello-agent-sidecar", "external sidecar"],
      ],
    },
  ] as const;
  for (const [index, testCase] of cases.entries()) {
    await withFixture(async (fixture, temporaryDirectory) => {
      const external = resolve(temporaryDirectory, `external-layout-${index}`);
      await writeFixtureFiles(external, testCase.files);
      const parent = resolve(fixture.package, testCase.parent);
      await rm(parent, { recursive: true, force: true });
      await symlink(external, parent);
      const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
      assert.equal(report.status, "NOT_READY", testCase.parent);
      assert.equal(report.reasons[0]?.code, "wrongKind", testCase.parent);
    });
  }
});

test("unpacked measurement counts external symlink text but never follows it", async () => {
  await withFixture(async (fixture, temporaryDirectory) => {
    const external = resolve(temporaryDirectory, "outside.bin");
    await writeFile(external, "outside");
    await truncate(external, 8 * 1024 * 1024);
    const link = resolve(fixture.package, "external-link");
    await symlink(external, link);
    const report = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
    assert.equal(report.status, "PASS");
    assert.equal(report.package.unpacked.symlinks, 1);
    assert.equal(report.package.unpacked.symlinkTextBytes.value, Buffer.byteLength(external).toString());
    assert.ok(BigInt(report.package.unpacked.bytes.value) < 1024n * 1024n);
  });
});

test("Windows emits measured package-size-only evidence but remains NOT_READY", async () => {
  await withFixture(async (fixture) => {
    const windows = await makeWindowsFixture(dirname(fixture.package));
    const report = await collectPackageMetrics({ platform: "win32", arch: "x64", ...windows });
    assert.equal(report.status, "NOT_READY");
    assert.equal(report.thresholds, null);
    assert.equal(report.package.unpacked.bytes.unit, "bytes");
    assert.equal(report.archive?.compressed?.bytes.unit, "bytes");
    assert.equal(report.reasons.at(-1)?.code, "windowsMeasureOnly");
  });
});

test("missing archive is NOT_READY rather than a partial macOS PASS", async () => {
  await withFixture(async (fixture) => {
    const report = await collectPackageMetrics({
      platform: "darwin",
      arch: "arm64",
      package: fixture.package,
    });
    assert.equal(report.status, "NOT_READY");
    assert.equal(report.assessment.unpacked, "PASS");
    assert.equal(report.assessment.compressed, "NOT_READY");
    assert.equal(report.reasons[0]?.code, "missingArchive");
  });
});

test("argument validation rejects malformed and unsupported target combinations", () => {
  assert.throws(() => parsePackageMetricsArgs(["--platform", "darwin", "--arch", "x64", "--package", "out"]));
  assert.throws(() => parsePackageMetricsArgs(["--platform", "darwin", "--arch", "arm64", "--package", "out", "--package", "again"]));
  assert.throws(() => parsePackageMetricsArgs(["--platform", "darwin", "--arch", "arm64"]));
});

test("CLI exits 0/1/2 for PASS/FAIL/NOT_READY and writes JSON only", async () => {
  await withFixture(async (fixture) => {
    await setPackageBytes(fixture, MACOS_ARM64_LIMITS.unpackedBytes);
    await setArchiveBytes(fixture.archive, MACOS_ARM64_LIMITS.compressedBytes);
    const passed = runCli("darwin", "arm64", fixture);
    assert.equal(passed.status, 0);
    assert.equal(JSON.parse(passed.stdout).status, "PASS");

    await setArchiveBytes(fixture.archive, MACOS_ARM64_LIMITS.compressedBytes + 1n);
    const failed = runCli("darwin", "arm64", fixture);
    assert.equal(failed.status, 1);
    assert.equal(JSON.parse(failed.stdout).status, "FAIL");

    const notReady = runCli("win32", "x64", await makeWindowsFixture(dirname(fixture.package)));
    assert.equal(notReady.status, 2);
    assert.equal(JSON.parse(notReady.stdout).status, "NOT_READY");

    const malformed = spawnSync(process.execPath, [script, "--platform", "darwin"], { encoding: "utf8" });
    assert.equal(malformed.status, 2);
    assert.equal(JSON.parse(malformed.stdout).status, "NOT_READY");
  });
});

async function withFixture(run: (fixture: { package: string; archive: string }, temporaryDirectory: string) => Promise<void>) {
  const temporaryDirectory = await mkdtemp(resolve(tmpdir(), "hello-agent-m5-package-metrics-"));
  try {
    await run(await makeMacFixture(temporaryDirectory), temporaryDirectory);
  } finally {
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
}

async function makeMacFixture(temporaryDirectory: string) {
  const packageDirectory = resolve(temporaryDirectory, "hello-agent-darwin-arm64");
  await writeFixtureFiles(packageDirectory, [
    ["hello-agent.app/Contents/MacOS/hello-agent", "executable"],
    ["hello-agent.app/Contents/Resources/app.asar", "asar"],
    ["hello-agent.app/Contents/Resources/sidecar/hello-agent-sidecar", "sidecar"],
  ]);
  const archive = resolve(temporaryDirectory, "hello-agent-darwin-arm64.zip");
  await writeFile(archive, Buffer.from([0x50, 0x4b, 0x03, 0x04]));
  return { package: packageDirectory, archive };
}

async function makeWindowsFixture(temporaryDirectory: string) {
  const packageDirectory = resolve(temporaryDirectory, "hello-agent-win32-x64");
  await writeFixtureFiles(packageDirectory, [
    ["hello-agent.exe", "executable"],
    ["resources/app.asar", "asar"],
    ["resources/sidecar/hello-agent-sidecar.exe", "sidecar"],
  ]);
  const archive = resolve(temporaryDirectory, "hello-agent-win32-x64.zip");
  await writeFile(archive, Buffer.from([0x50, 0x4b, 0x03, 0x04]));
  return { package: packageDirectory, archive };
}

async function writeFixtureFiles(packageDirectory: string, files: readonly (readonly [string, string])[]) {
  for (const [relativePath, content] of files) {
    const path = resolve(packageDirectory, relativePath);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }
}

async function setPackageBytes(fixture: { package: string; archive: string }, target: bigint) {
  const baseline = await collectPackageMetrics({ platform: "darwin", arch: "arm64", ...fixture });
  const current = BigInt(baseline.package.unpacked.bytes.value);
  assert.ok(current <= target, "fixture base must not exceed requested sparse package size");
  const padding = resolve(fixture.package, "padding.bin");
  await writeFile(padding, "");
  await truncate(padding, Number(target - current));
  assert.equal(BigInt((await stat(padding)).size), target - current);
}

async function setArchiveBytes(archive: string, target: bigint) {
  assert.ok(target >= 4n, "ZIP signature needs four bytes");
  await truncate(archive, Number(target));
  assert.equal(BigInt((await stat(archive)).size), target);
}

function runCli(platform: string, arch: string, fixture: { package: string; archive: string }) {
  return spawnSync(process.execPath, [
    script,
    "--platform", platform,
    "--arch", arch,
    "--package", fixture.package,
    "--archive", fixture.archive,
  ], { encoding: "utf8" });
}
