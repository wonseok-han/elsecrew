import assert from "node:assert/strict";
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
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
  const root = mkdtempSync(join(tmpdir(), "hello-agent-m5-g5d-storage-"));
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
      writeFileSync() { throw new Error("disk denied"); },
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

test("storage evidence writes a temporary sibling then renames it into the allowed directory", () => {
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
      lstatSync,
      mkdirSync,
      writeFileSync(path: string, contents: string) { writes.push(path); writeFileSync(path, contents); },
      renameSync(from: string, to: string) { renames.push([from, to]); renameSync(from, to); },
    });
    assert.equal(writes.length, 1);
    assert.equal(renames.length, 1);
    assert.equal(renames[0][0], writes[0]);
    assert.equal(renames[0][1], path);
    assert.equal(readdirSync(dirname(path)).some((name) => name.endsWith(".tmp")), false);
  });
});

test("storage gate exit mapping is fixed", () => {
  assert.equal(exitCodeForStorageGate("PASS"), 0);
  assert.equal(exitCodeForStorageGate("NOT_READY"), 2);
  assert.equal(exitCodeForStorageGate("FAIL"), 1);
});
