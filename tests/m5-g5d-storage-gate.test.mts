import assert from "node:assert/strict";
import {
  closeSync,
  fstatSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import test from "node:test";

import { exitCodeForStorageGate, runStorageGate, safeEvidenceDirectory, writeEvidence } from "../scripts/m5-g5d-storage-gate.mjs";

const STARTED_AT = "2026-09-09T00:00:00.000Z";

function fixtureGate(commands: unknown[]) {
  return { fixture: { id: "fixture-gate", version: 1, commands } };
}

function command(id = "fixture-command") {
  return { id, command: "fixture", args: [id] };
}

function cleanGit(_root: string, args: string[]) {
  if (args[0] === "rev-parse") return "fixture-head";
  if (args[0] === "status") return "";
  return null;
}

function withFixture(run: (root: string) => void) {
  const root = mkdtempSync(join(tmpdir(), "elsecrew-m5-g5d-storage-"));
  try {
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("storage gate records exact commands, capped resources, durations, HEAD, and atomically written evidence", () => {
  withFixture((root) => {
    const calls: string[][] = [];
    const outcome = runStorageGate("fixture", {
      root,
      gates: fixtureGate([command("first"), command("second")]),
      startedAt: STARTED_AT,
      now: () => new Date("2026-09-09T00:00:01.000Z"),
      readGit: cleanGit,
      environment: { CARGO_BUILD_JOBS: "4", RUST_TEST_THREADS: "4" },
      spawnSync(binary: string, args: string[]) {
        calls.push([binary, ...args]);
        return { status: 0, signal: null, stdout: "ok", stderr: "" };
      },
    });
    assert.equal(outcome.exitCode, 0);
    assert.equal(outcome.report.status, "PASS");
    assert.deepEqual(calls, [["fixture", "first"], ["fixture", "second"]]);
    assert.ok(outcome.evidencePath!.startsWith(resolve(root, ".artifacts", "m5-g5d-storage", "fixture-gate-v1-")));
    const evidence = JSON.parse(readFileSync(outcome.evidencePath!, "utf8"));
    assert.equal(evidence.head, "fixture-head");
    assert.equal(evidence.dirtyBefore, false);
    assert.equal(evidence.commands.length, 2);
    assert.equal(evidence.commands[0].durationMs >= 0, true);
    assert.equal(evidence.resources.cargoBuildJobs, "4");
    assert.equal(evidence.resources.rustTestThreads, "4");
  });
});

test("storage gate fails closed for missing command, nonzero exit, signal, and malformed child result", () => {
  const cases: Array<[string, unknown[], unknown]> = [
    ["missing", [{ id: "missing", command: "", args: [] }], { status: 0, signal: null }],
    ["nonzero", [command()], { status: 9, signal: null }],
    ["signal", [command()], { status: null, signal: "SIGTERM" }],
    ["malformed", [command()], {}],
  ];
  for (const [label, commands, raw] of cases) {
    withFixture((root) => {
      const outcome = runStorageGate("fixture", {
        root,
        gates: fixtureGate(commands),
        startedAt: STARTED_AT,
        readGit: cleanGit,
        spawnSync() { return raw; },
      });
      assert.equal(outcome.exitCode, 1, label);
      assert.equal(outcome.report.status, "FAIL", label);
      assert.equal(outcome.report.reasons.length, 1, label);
      assert.ok(outcome.evidencePath, label);
    });
  }
});

test("storage gate rejects dirty or unavailable git metadata before it starts a command", () => {
  for (const [label, readGit] of [
    ["dirty", (_root: string, args: string[]) => args[0] === "rev-parse" ? "fixture-head" : " M package.json"],
    ["unavailable", () => null],
  ] as const) {
    withFixture((root) => {
      let calls = 0;
      const outcome = runStorageGate("fixture", {
        root,
        gates: fixtureGate([command()]),
        startedAt: STARTED_AT,
        readGit,
        spawnSync() { calls += 1; return { status: 0, signal: null }; },
      });
      assert.equal(outcome.exitCode, 2, label);
      assert.equal(outcome.report.status, "NOT_READY", label);
      assert.equal(calls, 0, label);
    });
  }
});

test("storage gate never reports PASS when atomic evidence persistence fails", () => {
  withFixture((root) => {
    const outcome = runStorageGate("fixture", {
      root,
      gates: fixtureGate([command()]),
      startedAt: STARTED_AT,
      readGit: cleanGit,
      spawnSync() { return { status: 0, signal: null }; },
      openSync() { throw new Error("disk denied"); },
    });
    assert.equal(outcome.exitCode, 1);
    assert.equal(outcome.report.status, "FAIL");
    assert.equal(outcome.evidencePath, null);
    assert.equal(outcome.report.reasons.at(-1)?.code, "evidenceWriteFailed");
  });
});

test("storage evidence directory rejects symlink escape and gate-name path injection", () => {
  withFixture((root) => {
    const external = resolve(root, "external");
    writeFileSync(external, "not a directory");
    const artifacts = resolve(root, ".artifacts");
    symlinkSync(external, artifacts);
    assert.throws(() => safeEvidenceDirectory(root), /must not be a symlink/);
  });
  const injected = runStorageGate("../outside", { root: "/fixture" });
  assert.equal(injected.exitCode, 64);
  assert.equal(injected.evidencePath, null);
});

test("storage evidence exclusively creates, syncs, and renames a regular temporary sibling", () => {
  withFixture((root) => {
    const writes: string[] = [];
    const renames: Array<[string, string]> = [];
    const report = {
      gate: "fixture-gate",
      gateVersion: 1,
      startedAt: STARTED_AT,
      status: "PASS",
    };
    const path = writeEvidence(root, report, {
      closeSync,
      fsyncSync,
      fstatSync,
      lstatSync,
      mkdirSync,
      openSync,
      renameSync(from: string, to: string) { renames.push([from, to]); renameSync(from, to); },
      unlinkSync,
      writeSync(descriptor: number, bytes: Uint8Array, offset: number, length: number, position: number | null) {
        writes.push(String(descriptor));
        return writeSync(descriptor, bytes, offset, length, position);
      },
      randomToken: () => "predictable-token",
    });
    assert.equal(writes.length, 1);
    assert.equal(renames.length, 1);
    assert.match(renames[0][0], /\.predictable-token\.tmp$/);
    assert.equal(renames[0][1], path);
    assert.equal(readdirSync(dirname(path)).some((name) => name.endsWith(".tmp")), false);
  });
});

test("precreated temporary or final symlink leaves external data untouched and never yields evidence", () => {
  for (const mode of ["temporary", "final-race"] as const) {
    withFixture((root) => {
      const external = resolve(root, "external-sentinel");
      writeFileSync(external, "do not overwrite");
      const token = "predictable-token";
      let temporaryPath: string | null = null;
      let raced = false;
      const outcome = runStorageGate("fixture", {
        root,
        gates: fixtureGate([command()]),
        startedAt: STARTED_AT,
        readGit: cleanGit,
        randomToken: () => token,
        spawnSync() { return { status: 0, signal: null }; },
        openSync(path: string, flags: number, permissions: number) {
          temporaryPath = path;
          if (mode === "temporary") symlinkSync(external, path);
          return openSync(path, flags, permissions);
        },
        lstatSync(path: string) {
          if (mode === "final-race" && temporaryPath && !raced) {
            const suffix = `.${token}.tmp`;
            const name = temporaryPath.slice(dirname(temporaryPath).length + 2, -suffix.length);
            const finalPath = resolve(dirname(temporaryPath), name);
            if (path === finalPath) {
              raced = true;
              symlinkSync(external, path);
            }
          }
          return lstatSync(path);
        },
      });
      assert.equal(outcome.exitCode, 1, mode);
      assert.equal(outcome.report.status, "FAIL", mode);
      assert.equal(outcome.evidencePath, null, mode);
      assert.equal(readFileSync(external, "utf8"), "do not overwrite", mode);
      if (mode === "temporary") assert.ok(temporaryPath && lstatSync(temporaryPath).isSymbolicLink());
    });
  }
});

test("a preexisting regular temporary leaf is rejected before write and child commands inherit pnpm no-install", () => {
  withFixture((root) => {
    const token = "predictable-token";
    let temporaryPath: string | null = null;
    let childEnvironment: Record<string, string> | undefined;
    const outcome = runStorageGate("fixture", {
      root,
      gates: fixtureGate([command()]),
      startedAt: STARTED_AT,
      readGit: cleanGit,
      randomToken: () => token,
      spawnSync(_command: string, _args: string[], options: { env: Record<string, string> }) {
        childEnvironment = options.env;
        return { status: 0, signal: null };
      },
      openSync(path: string, flags: number, permissions: number) {
        temporaryPath = path;
        writeFileSync(path, "preexisting");
        return openSync(path, flags, permissions);
      },
    });
    assert.equal(outcome.exitCode, 1);
    assert.equal(outcome.evidencePath, null);
    assert.equal(readFileSync(temporaryPath!, "utf8"), "preexisting");
    assert.equal(childEnvironment?.pnpm_config_verify_deps_before_run, "false");
  });
});

test("replaced evidence parent fails closed without touching a sentinel or the detached owned temporary", () => {
  withFixture((root) => {
    const evidenceDirectory = resolve(root, ".artifacts", "m5-g5d-storage");
    const displacedDirectory = resolve(root, "displaced-evidence-directory");
    const sentinel = resolve(evidenceDirectory, "sentinel");
    let temporaryPath: string | null = null;
    const outcome = runStorageGate("fixture", {
      root,
      gates: fixtureGate([command()]),
      startedAt: STARTED_AT,
      readGit: cleanGit,
      randomToken: () => "predictable-token",
      spawnSync() { return { status: 0, signal: null }; },
      openSync(path: string, flags: number, permissions: number) {
        temporaryPath = path;
        // Model a descriptor bound to the original parent after replacement.
        // Rename before opening: Windows forbids moving a directory with open handles.
        renameSync(evidenceDirectory, displacedDirectory);
        mkdirSync(evidenceDirectory);
        writeFileSync(sentinel, "do not alter");
        return openSync(resolve(displacedDirectory, basename(path)), flags, permissions);
      },
    });
    assert.equal(outcome.exitCode, 1);
    assert.equal(outcome.report.status, "FAIL");
    assert.equal(outcome.evidencePath, null);
    assert.equal(readFileSync(sentinel, "utf8"), "do not alter");
    assert.ok(temporaryPath);
    assert.ok(lstatSync(resolve(displacedDirectory, basename(temporaryPath!))).isFile());
  });
});

test("substituted regular temporary leaf fails closed and is never unlinked during cleanup", () => {
  withFixture((root) => {
    const sentinel = resolve(root, "external-sentinel");
    writeFileSync(sentinel, "do not alter");
    let temporaryPath: string | null = null;
    let substituted = false;
    const outcome = runStorageGate("fixture", {
      root,
      gates: fixtureGate([command()]),
      startedAt: STARTED_AT,
      readGit: cleanGit,
      randomToken: () => "predictable-token",
      spawnSync() { return { status: 0, signal: null }; },
      openSync(path: string, flags: number, permissions: number) {
        temporaryPath = path;
        return openSync(path, flags, permissions);
      },
      closeSync(descriptor: number) {
        closeSync(descriptor);
        if (!substituted && temporaryPath) {
          substituted = true;
          unlinkSync(temporaryPath);
          writeFileSync(temporaryPath, "substituted");
        }
      },
    });
    assert.equal(outcome.exitCode, 1);
    assert.equal(outcome.report.status, "FAIL");
    assert.equal(outcome.evidencePath, null);
    assert.equal(readFileSync(sentinel, "utf8"), "do not alter");
    assert.equal(readFileSync(temporaryPath!, "utf8"), "substituted");
  });
});

test("storage package wrappers dispatch only through the runner and keep the project pnpm setting exact", () => {
  assert.equal(readFileSync(resolve("pnpm-workspace.yaml"), "utf8"), "allowBuilds:\n  esbuild: true\nnodeLinker: hoisted\nverifyDepsBeforeRun: false\n");
  assert.throws(() => readFileSync(resolve(".npmrc"), "utf8"), { code: "ENOENT" });
  const packageJson = JSON.parse(readFileSync(resolve("package.json"), "utf8")) as { scripts: Record<string, string> };
  assert.equal(packageJson.scripts["m5:test:storage-status"], "node scripts/m5-g5d-storage-gate.mjs --gate status");
});

test("storage gate exit mapping is fixed", () => {
  assert.equal(exitCodeForStorageGate("PASS"), 0);
  assert.equal(exitCodeForStorageGate("NOT_READY"), 2);
  assert.equal(exitCodeForStorageGate("FAIL"), 1);
});
