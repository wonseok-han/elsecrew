import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, renameSync, rmSync, symlinkSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";

import {
  expectedPackageArtifacts,
  exitCodeForPackageGate,
  runPackageGate,
  targetSupport,
} from "../scripts/m5-g6-package-gate.mjs";

const STARTED_AT = "2026-09-08T00:00:00.000Z";

test("G6 package gate uses only the exact Forge package and ZIP paths", () => {
  const artifacts = expectedPackageArtifacts({ root: "/repo", platform: "darwin", arch: "arm64", version: "0.1.2" });

  assert.deepEqual(artifacts, {
    packageDirectory: resolve("/repo/out/Elsecrew-darwin-arm64"),
    archive: resolve("/repo/out/make/zip/darwin/arm64/Elsecrew-darwin-arm64-0.1.2.zip"),
    requiredOutputs: [
      "/repo/out/Elsecrew-darwin-arm64",
      "/repo/out/Elsecrew-darwin-arm64/Elsecrew.app/Contents/MacOS/Elsecrew",
      "/repo/out/Elsecrew-darwin-arm64/Elsecrew.app/Contents/Resources/app.asar",
      "/repo/out/Elsecrew-darwin-arm64/Elsecrew.app/Contents/Resources/sidecar/elsecrew-sidecar",
      "/repo/out/make/zip/darwin/arm64/Elsecrew-darwin-arm64-0.1.2.zip",
    ].map(path => resolve(path)),
  });
});

test("G6 package gate orders package, smoke, make, then exact-path metrics and writes evidence", () => {
  withFixture(({ root, artifacts }) => {
    const calls: string[][] = [];
    let evidence: string | undefined;
    const outcome = runPackageGate({
      root,
      platform: "darwin",
      arch: "arm64",
      version: "0.1.2",
      startedAt: STARTED_AT,
      now: () => new Date("2026-09-08T00:00:01.000Z"),
      readGit: (_root: string, args: string[]) => args[0] === "rev-parse" ? "fixture-head" : "",
      spawnSync(command: string, args: string[]) {
        calls.push([command, ...args]);
        if (args[0] === "scripts/m5-package-metrics.mjs") {
          return { status: 0, stdout: JSON.stringify(metrics("PASS", artifacts)) };
        }
        return { status: 0 };
      },
      writeFileSync(path: string, contents: string) {
        evidence = contents;
        assert.equal(path, resolve(root, ".artifacts", "m5-g6-package", "darwin-arm64-2026-09-08T00-00-00-000Z.json"));
      },
      mkdirSync() {},
    });

    assert.equal(outcome.report.status, "PASS");
    assert.equal(outcome.exitCode, 0);
    assert.deepEqual(outcome.report.results.map((result) => result.id), ["package", "smoke", "make", "metrics"]);
    assert.deepEqual(calls, [
      ["pnpm", "exec", "electron-forge", "package"],
      [process.execPath, "scripts/m5-electron-package-smoke.mjs"],
      ["pnpm", "exec", "electron-forge", "make", "--skip-package"],
      [
        process.execPath,
        "scripts/m5-package-metrics.mjs",
        "--platform", "darwin", "--arch", "arm64",
        "--package", artifacts.packageDirectory,
        "--archive", artifacts.archive,
      ],
    ]);
    const report = JSON.parse(evidence!);
    assert.equal(report.head, "fixture-head");
    assert.equal(report.dirtyBefore, false);
    assert.equal(report.metrics.status, "PASS");
  });
});

test("G6 package gate stops on a command failure and does not trust later artifacts", () => {
  withFixture(({ root }) => {
    const calls: string[][] = [];
    const outcome = runPackageGate({
      root,
      platform: "darwin",
      arch: "arm64",
      version: "0.1.2",
      startedAt: STARTED_AT,
      readGit: () => "",
      spawnSync(command: string, args: string[]) {
        calls.push([command, ...args]);
        return { status: 7 };
      },
      writeFileSync() {},
      mkdirSync() {},
    });
    assert.equal(outcome.report.status, "FAIL");
    assert.equal(outcome.exitCode, 1);
    assert.deepEqual(calls, [["pnpm", "exec", "electron-forge", "package"]]);
    assert.equal(outcome.report.reasons[0]?.code, "commandFailed");
  });
});

test("G6 package gate rejects a dirty starting tree before it creates release artifacts", () => {
  withFixture(({ root }) => {
    const calls: string[][] = [];
    const outcome = runPackageGate({
      root,
      platform: "darwin",
      arch: "arm64",
      version: "0.1.2",
      startedAt: STARTED_AT,
      readGit: (_root: string, args: string[]) => args[0] === "status" ? " M package.json" : "fixture-head",
      spawnSync(command: string, args: string[]) { calls.push([command, ...args]); return { status: 0 }; },
      writeFileSync() {},
      mkdirSync() {},
    });
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.equal(outcome.report.reasons[0]?.code, "dirtyWorkingTree");
    assert.deepEqual(calls, []);
  });
});

test("G6 package gate preflight rejects an out symlink before package can write outside the repository", () => {
  withFixture(({ root, artifacts }) => {
    const out = resolve(root, "out");
    const externalOut = resolve(root, "external-out");
    renameSync(out, externalOut);
    symlinkSync(externalOut, out);
    const calls: string[][] = [];
    const outcome = runWithSuccessfulCommands(root, artifacts, calls);
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.ok(outcome.report.reasons.some((reason) => reason.code === "symlinkArtifact"));
    assert.deepEqual(calls, []);
  });
});

test("G6 package gate preserves the package-metrics PASS, FAIL, and NOT_READY exit mapping", () => {
  for (const [status, expectedExitCode] of [["PASS", 0], ["FAIL", 1], ["NOT_READY", 2]] as const) {
    withFixture(({ root, artifacts }) => {
      const outcome = runPackageGate({
        root,
        platform: "darwin",
        arch: "arm64",
        version: "0.1.2",
        startedAt: STARTED_AT,
        readGit: () => "",
        spawnSync(_command: string, args: string[]) {
          return args[0] === "scripts/m5-package-metrics.mjs"
            ? { status: expectedExitCode, stdout: JSON.stringify(metrics(status, artifacts)) }
            : { status: 0 };
        },
        writeFileSync() {},
        mkdirSync() {},
      });
      assert.equal(outcome.report.status, status);
      assert.equal(outcome.exitCode, expectedExitCode);
    });
  }
});

test("G6 package gate rejects stale package assets before smoke or make", () => {
  withFixture(({ root, artifacts }) => {
    for (const path of artifacts.requiredOutputs.slice(0, -1)) {
      utimesSync(path, new Date("2026-09-07T00:00:00.000Z"), new Date("2026-09-07T00:00:00.000Z"));
    }
    const calls: string[][] = [];
    const outcome = runWithSuccessfulCommands(root, artifacts, calls);
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.ok(outcome.report.reasons.every((reason) => reason.code === "staleArtifact"));
    assert.deepEqual(calls, [["pnpm", "exec", "electron-forge", "package"]]);
  });
  withFixture(({ root, artifacts }) => {
    const sidecarDirectory = dirname(artifacts.requiredOutputs.at(-2)!);
    rmSync(sidecarDirectory, { recursive: true });
    const external = resolve(root, "external-sidecar");
    mkdirSync(external);
    writeFileSync(resolve(external, "elsecrew-sidecar"), "external fixture");
    symlinkSync(external, sidecarDirectory);
    const calls: string[][] = [];
    const outcome = runWithSuccessfulCommands(root, artifacts, calls);
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.ok(outcome.report.reasons.some((reason) => reason.code === "symlinkArtifact"));
    assert.deepEqual(calls, []);
  });
});

test("G6 package gate rejects a missing archive after make and before metrics", () => {
  withFixture(({ root, artifacts }) => {
    rmSync(artifacts.archive);
    const calls: string[][] = [];
    const outcome = runWithSuccessfulCommands(root, artifacts, calls);
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.ok(outcome.report.reasons.some((reason) => reason.code === "missingArtifact"));
    assert.equal(calls.length, 3);
  });
});

test("G6 package gate rejects a ZIP parent symlink after make before metrics", () => {
  withFixture(({ root, artifacts }) => {
    const calls: string[][] = [];
    const outcome = runPackageGate({
      root,
      platform: "darwin",
      arch: "arm64",
      version: "0.1.2",
      startedAt: STARTED_AT,
      readGit: () => "",
      spawnSync(command: string, args: string[]) {
        calls.push([command, ...args]);
        if (args[0] === "exec" && args[2] === "make") {
          const makeDirectory = resolve(root, "out", "make");
          const externalMake = resolve(root, "external-make");
          renameSync(makeDirectory, externalMake);
          symlinkSync(externalMake, makeDirectory);
        }
        if (args[0] === "scripts/m5-package-metrics.mjs") assert.fail("metrics must not run through a ZIP parent symlink");
        return { status: 0, stdout: JSON.stringify(metrics("PASS", artifacts)) };
      },
      writeFileSync() {},
      mkdirSync() {},
    });
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.ok(outcome.report.reasons.some((reason) => reason.code === "symlinkArtifact"));
    assert.deepEqual(calls, [
      ["pnpm", "exec", "electron-forge", "package"],
      [process.execPath, "scripts/m5-electron-package-smoke.mjs"],
      ["pnpm", "exec", "electron-forge", "make", "--skip-package"],
    ]);
  });
});

test("Windows remains measure-only NOT_READY while Linux and unsupported targets do not build", () => {
  withFixture(({ root }) => {
    const windows = expectedPackageArtifacts({ root, platform: "win32", arch: "x64", version: "0.1.2" });
    writeOutputs(windows);
    const calls: string[][] = [];
    const outcome = runPackageGate({
      root,
      platform: "win32",
      arch: "x64",
      version: "0.1.2",
      startedAt: STARTED_AT,
      readGit: () => "",
      spawnSync(command: string, args: string[]) {
        calls.push([command, ...args]);
        return args[0] === "scripts/m5-package-metrics.mjs"
          ? { status: 2, stdout: JSON.stringify(metrics("NOT_READY", windows)) }
          : { status: 0 };
      },
      writeFileSync() {},
      mkdirSync() {},
    });
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.equal(calls.length, 4);
  });
  for (const [platform, arch] of [["linux", "x64"], ["darwin", "x64"]] as const) {
    const calls: string[][] = [];
    const outcome = runPackageGate({
      root: "/fixture",
      platform,
      arch,
      version: "0.1.2",
      startedAt: STARTED_AT,
      readGit: () => "",
      spawnSync(command: string, args: string[]) { calls.push([command, ...args]); return { status: 0 }; },
      writeFileSync() {},
      mkdirSync() {},
    });
    assert.equal(outcome.report.status, "NOT_READY");
    assert.equal(outcome.exitCode, 2);
    assert.equal(calls.length, 0);
  }
  assert.deepEqual(targetSupport("win32", "arm64"), { supported: true, reason: null });
  assert.equal(exitCodeForPackageGate("FAIL"), 1);
});

function runWithSuccessfulCommands(root: string, artifacts: ReturnType<typeof expectedPackageArtifacts>, calls: string[][]) {
  return runPackageGate({
    root,
    platform: "darwin",
    arch: "arm64",
    version: "0.1.2",
    startedAt: STARTED_AT,
    readGit: () => "",
    spawnSync(command: string, args: string[]) {
      calls.push([command, ...args]);
      return args[0] === "scripts/m5-package-metrics.mjs"
        ? { status: 0, stdout: JSON.stringify(metrics("PASS", artifacts)) }
        : { status: 0 };
    },
    writeFileSync() {},
    mkdirSync() {},
  });
}

function withFixture(run: (fixture: { root: string; artifacts: ReturnType<typeof expectedPackageArtifacts> }) => void) {
  const root = mkdtempSync(resolve(tmpdir(), "elsecrew-m5-g6-package-"));
  try {
    writeFileSync(resolve(root, "package.json"), JSON.stringify({ version: "0.1.2" }));
    const artifacts = expectedPackageArtifacts({ root, platform: "darwin", arch: "arm64", version: "0.1.2" });
    writeOutputs(artifacts);
    run({ root, artifacts });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function writeOutputs(artifacts: ReturnType<typeof expectedPackageArtifacts>) {
  for (const path of artifacts.requiredOutputs) {
    if (path === artifacts.packageDirectory) {
      mkdirSync(path, { recursive: true });
    } else {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, "fixture");
    }
  }
}

function metrics(status: "PASS" | "FAIL" | "NOT_READY", artifacts: ReturnType<typeof expectedPackageArtifacts>) {
  return {
    status,
    package: { path: artifacts.packageDirectory },
    archive: { path: artifacts.archive },
  };
}
