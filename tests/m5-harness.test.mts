import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import test from "node:test";
import { lowerQaPriority, qaEnvironment } from "../scripts/m5-qa-resources.mjs";

const harness = resolve("scripts/m5-harness.mjs");

function run(...args: string[]) {
  return spawnSync(process.execPath, [harness, ...args, "--no-report"], {
    encoding: "utf8",
  });
}

test("하네스가 M5 게이트와 준비 상태를 기계적으로 나열한다", () => {
  const result = run("--list");

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /baseline\s+READY/);
  assert.match(result.stdout, /g1-foundation\s+READY/);
  assert.match(result.stdout, /g2-contracts\s+READY/);
  assert.match(result.stdout, /g3-storage\s+READY/);
  assert.match(result.stdout, /g4-project-editor\s+READY/);
  assert.match(result.stdout, /g5-run-recovery\s+READY/);
  assert.match(result.stdout, /g5d-bounded-storage\s+READY/);
  assert.match(result.stdout, /g5d-storage-lifetime\s+NOT_READY/);
  assert.match(result.stdout, /g6-release\s+NOT_READY/);
});

test("G5 누적 게이트는 G5A~G5C actual-agent 회귀를 포함한다", () => {
  const packageJson = JSON.parse(readFileSync(resolve("package.json"), "utf8")) as {
    scripts: Record<string, string>;
  };
  const command = packageJson.scripts["m5:test:run-recovery"] ?? "";

  assert.match(command, /tests\/m5-g5a-agent-adapter-contract\.test\.mts/);
  assert.match(command, /tests\/m5-g5b-terminal-budget-contract\.test\.mts/);
  assert.match(command, /tests\/m5-g5c-provider-contract\.test\.mts/);
  assert.match(command, /tests\/m5-g5c-identityless-recovery-contract\.test\.mts/);
  assert.match(command, /tests\/m5-g5a-public-event-ui\.test\.mts/);
});

test("G5D bounded-storage 하위 게이트는 lifetime 계약과 V6 migration·worker를 포함한다", () => {
  const packageJson = JSON.parse(readFileSync(resolve("package.json"), "utf8")) as {
    scripts: Record<string, string>;
  };
  const command = packageJson.scripts["m5:test:bounded-storage"] ?? "";

  assert.match(command, /tests\/m5-g5d-lifetime-contract\.test\.mts/);
  assert.match(command, /hello-agent-protocol --locked storage_lifetime_v1/);
  assert.match(command, /hello-agent-sidecar --locked storage::tests/);
  assert.match(command, /hello-agent-sidecar --locked bounded_v6/);
});

test("전체 G5D는 scheduler, status wire, compaction 전까지 NOT_READY다", () => {
  const result = run("--gate", "g5d-storage-lifetime");

  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, /NOT_READY g5d-storage-lifetime/);
  assert.match(result.stderr, /m5:test:storage-scheduler/);
  assert.match(result.stderr, /m5:test:storage-status/);
  assert.match(result.stderr, /m5:test:storage-compaction/);
});

test("다음 미구현 게이트는 성공이나 skip 대신 NOT_READY로 끝난다", () => {
  const result = run("--gate", "g6-release");

  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, /NOT_READY g6-release/);
  assert.match(result.stderr, /m5:test:security/);
  assert.match(result.stderr, /m5:test:package/);
  assert.match(result.stderr, /m5:test:performance/);
});

test("저부하 QA는 Rust 동시 실행을 4로 제한하고 더 작은 설정을 보존한다", () => {
  const defaults = qaEnvironment({ KEEP_ME: "yes" });
  assert.equal(defaults.CARGO_BUILD_JOBS, "4");
  assert.equal(defaults.RUST_TEST_THREADS, "4");
  assert.equal(defaults.KEEP_ME, "yes");
  assert.equal(defaults.CI, "1");
  for (const value of ["64", "0", "-1", "NaN", "1.5", "", undefined]) {
    const env = qaEnvironment({ CARGO_BUILD_JOBS: value, RUST_TEST_THREADS: value });
    assert.equal(env.CARGO_BUILD_JOBS, "4");
    assert.equal(env.RUST_TEST_THREADS, "4");
  }
  const source = { CARGO_BUILD_JOBS: "2", RUST_TEST_THREADS: "1" };
  assert.equal(qaEnvironment(source).CARGO_BUILD_JOBS, "2");
  assert.equal(qaEnvironment(source).RUST_TEST_THREADS, "1");
  assert.deepEqual(source, { CARGO_BUILD_JOBS: "2", RUST_TEST_THREADS: "1" });
});

test("QA 우선순위는 낮추기만 하고 실패를 결과로 남긴다", () => {
  let current = 0;
  const changed = lowerQaPriority(() => current, (pid: number, value: number) => {
    assert.equal(pid, 0);
    current = value;
  });
  assert.deepEqual(changed, { requested: 10, actual: 10, error: null });
  assert.deepEqual(lowerQaPriority(() => 15, () => assert.fail("must not raise priority")),
    { requested: 15, actual: 15, error: null });
  const denied = lowerQaPriority(() => 0, () => { throw new Error("denied"); });
  assert.equal(denied.actual, null);
  assert.match(denied.error!, /denied/);
  assert.match(lowerQaPriority(() => 0, () => {}).error!, /not lowered/);
});

test("실제 하네스 자식에 제한을 전달하고 보고하며 실패 게이트를 보존한다", {
  skip: process.platform === "win32" ? "POSIX executable fixture; Windows priority inheritance is not claimed" : false,
}, () => {
  const fixture = mkdtempSync(join(tmpdir(), "m5-qa-resource-test-"));
  try {
    const scripts = join(fixture, "scripts");
    const bin = join(fixture, "bin");
    mkdirSync(scripts);
    mkdirSync(bin);
    for (const name of ["m5-harness.mjs", "m5-qa-resources.mjs"]) {
      copyFileSync(resolve("scripts", name), join(scripts, name));
    }
    writeFileSync(join(fixture, "package.json"), JSON.stringify({ scripts: {} }));
    const stub = `#!${process.execPath}
const { basename } = require("node:path");
const { getPriority } = require("node:os");
const name = basename(process.argv[1]);
if (name === "git" && process.argv[2] === "rev-parse") console.log("fixture-head");
else if (name === "git" && process.argv[2] === "branch") console.log("fixture-branch");
else if (!(name === "git" && process.argv[2] === "status")) {
  let priority = null;
  try { priority = getPriority(0); } catch {}
  console.log(JSON.stringify({ jobs: process.env.CARGO_BUILD_JOBS, threads: process.env.RUST_TEST_THREADS, priority }));
}
process.exit(name === process.env.QA_FIXTURE_FAIL ? 7 : 0);
`;
    for (const name of ["git", "pnpm", "npm", "cargo"]) {
      const path = join(bin, name);
      writeFileSync(path, stub);
      chmodSync(path, 0o755);
    }
    for (const failure of ["", "cargo"]) {
      const result = spawnSync(process.execPath, [join(scripts, "m5-harness.mjs"), "--gate", "baseline"], {
        cwd: fixture,
        encoding: "utf8",
        env: { ...process.env, PATH: `${bin}${delimiter}${process.env.PATH}`, CARGO_BUILD_JOBS: "64", RUST_TEST_THREADS: "2", QA_FIXTURE_FAIL: failure },
      });
      assert.equal(result.status, failure ? 1 : 0, result.stderr);
      const reports = join(fixture, ".artifacts", "m5-harness");
      const latest = readdirSync(reports).sort().at(-1)!;
      const report = JSON.parse(readFileSync(join(reports, latest), "utf8"));
      assert.equal(report.status, failure ? "FAIL" : "PASS");
      assert.equal(report.resources.profile, "low-impact");
      assert.equal(report.resources.cargoBuildJobs, 4);
      assert.equal(report.resources.rustTestThreads, 2);
      assert.equal(report.results.length, 5);
      for (const entry of report.results) {
        const child = JSON.parse(entry.stdout);
        assert.equal(child.jobs, "4");
        assert.equal(child.threads, "2");
        if (report.resources.priority.error === null) {
          assert.ok(child.priority >= report.resources.priority.requested);
        } else {
          assert.match(result.stdout, /priority=unavailable|priority=\d+/);
          assert.match(result.stderr, /\[WARN\] QA priority/);
        }
      }
      if (failure) assert.equal(report.results.at(-1).exitCode, 7);
    }
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});
