import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import test from "node:test";
import { lowerQaPriority, qaEnvironment } from "../scripts/m5-qa-resources.mjs";

const harness = resolve("scripts/m5-harness.mjs");
const canonicalScripts = [
  "m5:test:electron", "m5:test:core", "m5:test:contracts", "m5:test:storage",
  "m5:test:project", "m5:test:editor", "m5:test:run-recovery", "m5:test:bounded-storage",
  "m5:test:storage-scheduler", "m5:test:storage-status", "m5:test:storage-compaction",
  "m5:test:security", "m5:test:package", "m5:test:performance",
];
const cumulativeAliases = [
  "m5:test:contracts:node", "m5:test:storage:node", "m5:test:project:node",
  "m5:test:run-recovery:node", "m5:test:bounded-storage:node",
];

function run(...args: string[]) {
  return spawnSync(process.execPath, [harness, ...args, "--no-report"], { encoding: "utf8" });
}

function fixtureScripts(omit: string[] = []) {
  return Object.fromEntries([...canonicalScripts, ...cumulativeAliases]
    .filter((name) => !omit.includes(name))
    .map((name) => [name, "fixture"]));
}

function createFixture(options: { omit?: string[]; environment?: Record<string, string> } = {}) {
  const root = mkdtempSync(join(tmpdir(), "m5-harness-dedup-"));
  const scripts = join(root, "scripts");
  const bin = join(root, "bin");
  mkdirSync(scripts);
  mkdirSync(bin);
  for (const name of ["m5-harness.mjs", "m5-qa-resources.mjs"]) {
    copyFileSync(resolve("scripts", name), join(scripts, name));
  }
  writeFileSync(join(root, "package.json"), JSON.stringify({ scripts: fixtureScripts(options.omit) }));
  const stub = `#!${process.execPath}
const { appendFileSync, existsSync, writeFileSync } = require("node:fs");
const { basename } = require("node:path");
const name = basename(process.argv[1]);
const args = process.argv.slice(2);
const mutate = process.env.QA_FIXTURE_MUTATE;
const statePath = process.env.QA_FIXTURE_STATE;
if (name === "git" && args[0] === "rev-parse") {
  console.log(statePath && existsSync(statePath) && process.env.QA_FIXTURE_HEAD_DRIFT === "1" ? "fixture-head-drift" : "fixture-head");
} else if (name === "git" && args[0] === "branch") {
  console.log("fixture-branch");
} else if (name === "git" && args[0] === "status") {
  if (statePath && existsSync(statePath) && process.env.QA_FIXTURE_DIRTY_AFTER === "1") console.log(" M changed");
} else {
  if (process.env.QA_FIXTURE_LOG) appendFileSync(process.env.QA_FIXTURE_LOG, name + " " + args.join(" ") + "\\n");
  const target = name === "pnpm" ? args[1] : null;
  if (mutate && (mutate === "any" || mutate === name || mutate === target) && statePath && !existsSync(statePath)) writeFileSync(statePath, "changed");
  const expected = {
    "m5:test:storage-scheduler": "g5d-storage-scheduler",
    "m5:test:storage-status": "g5d-storage-status",
    "m5:test:storage-compaction": "g5d-storage-compaction",
  }[target];
  if (expected) {
    const gate = process.env.QA_FIXTURE_STORAGE_GATE ?? expected;
    const status = process.env.QA_FIXTURE_STORAGE_STATUS ?? "PASS";
    const head = process.env.QA_FIXTURE_STORAGE_HEAD ?? "fixture-head";
    const dirtyBefore = process.env.QA_FIXTURE_STORAGE_DIRTY === "1";
    const evidencePath = process.env.QA_FIXTURE_STORAGE_EVIDENCE === "none" ? null : "/fixture/evidence.json";
    process.stdout.write(process.env.QA_FIXTURE_STORAGE_MALFORMED === "1" ? "not-json\\n" : JSON.stringify({ gate, status, head, dirtyBefore, evidencePath }) + "\\n");
  } else {
    console.log(JSON.stringify({ jobs: process.env.CARGO_BUILD_JOBS, threads: process.env.RUST_TEST_THREADS }));
  }
  if (process.env.QA_FIXTURE_NOT_READY && process.env.QA_FIXTURE_NOT_READY === target) process.exit(2);
  if (process.env.QA_FIXTURE_FAIL && (process.env.QA_FIXTURE_FAIL === target || process.env.QA_FIXTURE_FAIL === name)) process.exit(7);
  if (process.env.QA_FIXTURE_SIGNAL && process.env.QA_FIXTURE_SIGNAL === target) process.kill(process.pid, "SIGTERM");
}
`;
  for (const name of ["git", "pnpm", "npm", "cargo"]) {
    const path = join(bin, name);
    writeFileSync(path, stub);
    chmodSync(path, 0o755);
  }
  return {
    root,
    execute: (gate: string, environment: Record<string, string> = {}) => spawnSync(process.execPath, [join(scripts, "m5-harness.mjs"), "--gate", gate], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, PATH: `${bin}${delimiter}${process.env.PATH}`, ...options.environment, ...environment },
    }),
    report() {
      const directory = join(root, ".artifacts", "m5-harness");
      const latest = readdirSync(directory).sort().at(-1)!;
      return JSON.parse(readFileSync(join(directory, latest), "utf8"));
    },
    cleanup() { rmSync(root, { recursive: true, force: true }); },
  };
}

test("하네스가 M5 게이트와 canonical·cumulative 준비 상태를 기계적으로 나열한다", () => {
  const result = run("--list");
  assert.equal(result.status, 0, result.stderr);
  for (const gate of ["baseline", "g1-foundation", "g2-contracts", "g3-storage", "g4-project-editor", "g5-run-recovery", "g5d-bounded-storage", "g5d-storage-lifetime"]) {
    assert.match(result.stdout, new RegExp(`${gate}\\s+COMMANDS_READY`));
  }
  assert.match(result.stdout, /g6-release\s+COMMANDS_READY/);
  assert.match(result.stdout, /no measurements or gate results are assessed/);
});

test("독립 wrapper의 Rust 범위는 유지하고 cumulative alias는 정확한 Node 파일만 실행한다", () => {
  const scripts = (JSON.parse(readFileSync(resolve("package.json"), "utf8")) as { scripts: Record<string, string> }).scripts;
  assert.equal(scripts["m5:test:core"], "cargo test --workspace --manifest-path Cargo.toml --locked");
  assert.match(scripts["m5:test:contracts"], /cargo test -p hello-agent-protocol/);
  assert.match(scripts["m5:test:storage"], /cargo test -p hello-agent-sidecar --locked/);
  assert.match(scripts["m5:test:project"], /cargo test -p hello-agent-sidecar --locked project::/);
  assert.match(scripts["m5:test:run-recovery"], /cargo test -p hello-agent-sidecar --locked/);
  assert.match(scripts["m5:test:bounded-storage"], /hello-agent-protocol --locked storage_lifetime_v1/);
  assert.equal(scripts["m5:test:contracts:node"], "node --test --experimental-strip-types tests/m5-protocol-contract.test.mts");
  assert.equal(scripts["m5:test:storage:node"], "node --test --experimental-strip-types tests/m5-g3-contract.test.mts tests/m5-electron-bridge.test.mts");
  assert.equal(scripts["m5:test:project:node"], "node --test --experimental-strip-types tests/m5-g4-contract.test.mts tests/m5-electron-bridge.test.mts");
  assert.equal(scripts["m5:test:bounded-storage:node"], "node --test --experimental-strip-types tests/m5-g5d-lifetime-contract.test.mts");
  for (const alias of cumulativeAliases) assert.doesNotMatch(scripts[alias], /\bcargo\b/);
  for (const file of ["m5-g5a-agent-adapter-contract", "m5-g5b-terminal-budget-contract", "m5-g5c-provider-contract", "m5-g5c-identityless-recovery-contract", "m5-g5a-public-event-ui", "m5-g5-workspace-editor-state"]) {
    assert.match(scripts["m5:test:run-recovery:node"], new RegExp(`tests/${file}\\.test\\.mts`));
  }
});

test("누적 실행은 core를 한 번만 실행하고 이후 Rust wrapper 대신 Node alias와 ordered storage evidence를 쓴다", {
  skip: process.platform === "win32" ? "POSIX executable fixture" : false,
}, () => {
  const fixture = createFixture();
  try {
    const result = fixture.execute("g5d-storage-lifetime");
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const report = fixture.report();
    assert.equal(report.schemaVersion, 2);
    assert.equal(report.status, "PASS");
    assert.equal(report.head, "fixture-head");
    assert.equal(report.headAfter, "fixture-head");
    assert.equal(report.dirtyBefore, false);
    assert.equal(report.dirtyAfter, false);
    const ids = report.results.map((entry: { id: string }) => entry.id);
    assert.equal(ids.filter((id: string) => id === "m5:test:core").length, 1);
    for (const script of cumulativeAliases) assert.ok(ids.includes(script), `missing ${script}`);
    for (const script of ["m5:test:contracts", "m5:test:storage", "m5:test:project", "m5:test:run-recovery", "m5:test:bounded-storage"]) assert.ok(!ids.includes(script), `unexpected ${script}`);
    assert.deepEqual(ids.slice(-3), ["m5:test:storage-scheduler", "m5:test:storage-status", "m5:test:storage-compaction"]);
    assert.deepEqual(report.scriptPlan.find((entry: { gate: string }) => entry.gate === "g5-run-recovery"), {
      gate: "g5-run-recovery", canonicalScripts: ["m5:test:run-recovery"], cumulativeScripts: ["m5:test:run-recovery:node"],
    });
  } finally { fixture.cleanup(); }
});

test("missing cumulative alias, dirty tree, unavailable git은 명령 없이 NOT_READY로 실패한다", {
  skip: process.platform === "win32" ? "POSIX executable fixture" : false,
}, () => {
  const missing = createFixture({ omit: ["m5:test:storage:node"] });
  const dirty = createFixture();
  const unavailable = createFixture();
  try {
    const missingResult = missing.execute("g3-storage");
    assert.equal(missingResult.status, 2, missingResult.stderr);
    assert.equal(missing.report().results.length, 0);
    const dirtyState = join(dirty.root, "state");
    writeFileSync(dirtyState, "dirty");
    const dirtyResult = dirty.execute("g3-storage", { QA_FIXTURE_STATE: dirtyState, QA_FIXTURE_DIRTY_AFTER: "1" });
    assert.equal(dirtyResult.status, 2, dirtyResult.stderr);
    assert.equal(dirty.report().results.length, 0);
    const unavailableResult = unavailable.execute("baseline", { PATH: "/not-a-real-path" });
    assert.equal(unavailableResult.status, 2, unavailableResult.stderr);
    assert.equal(unavailable.report().results.length, 0);
  } finally { missing.cleanup(); dirty.cleanup(); unavailable.cleanup(); }
});

test("HEAD drift, dirty-after, core failure, and child exit 2 stop the cumulative run fail-closed", {
  skip: process.platform === "win32" ? "POSIX executable fixture" : false,
}, () => {
  const cases = [
    { environment: { QA_FIXTURE_STATE: "state", QA_FIXTURE_MUTATE: "git", QA_FIXTURE_HEAD_DRIFT: "1" }, gate: "baseline", expected: 1, reason: "headChanged" },
    { environment: { QA_FIXTURE_STATE: "state", QA_FIXTURE_MUTATE: "git", QA_FIXTURE_DIRTY_AFTER: "1" }, gate: "baseline", expected: 1, reason: "workingTreeDirty" },
    { environment: { QA_FIXTURE_FAIL: "m5:test:core" }, gate: "g5d-storage-lifetime", expected: 1, reason: "commandFailed" },
    { environment: { QA_FIXTURE_SIGNAL: "m5:test:core" }, gate: "g5d-storage-lifetime", expected: 1, reason: "commandFailed" },
    { environment: { QA_FIXTURE_NOT_READY: "m5:test:project:node" }, gate: "g5d-storage-lifetime", expected: 2, reason: "commandNotReady" },
  ];
  for (const item of cases) {
    const fixture = createFixture();
    try {
      const environment = Object.fromEntries(Object.entries(item.environment).map(([key, value]) => [key, key === "QA_FIXTURE_STATE" ? join(fixture.root, value) : value]));
      const result = fixture.execute(item.gate, environment);
      assert.equal(result.status, item.expected, `${result.stdout}\n${result.stderr}`);
      const report = fixture.report();
      assert.equal(report.status, item.expected === 2 ? "NOT_READY" : "FAIL");
      if (item.reason) assert.equal(report.reasons.at(-1).code, item.reason);
      if (item.environment.QA_FIXTURE_FAIL || item.environment.QA_FIXTURE_SIGNAL) assert.ok(!report.results.some((entry: { id: string }) => entry.id === "m5:test:contracts:node"));
      if (item.environment.QA_FIXTURE_NOT_READY) assert.equal(report.results.at(-1).id, "m5:test:project:node");
    } finally { fixture.cleanup(); }
  }
});

test("storage child evidence JSON은 malformed·mismatched gate/head/status/dirty/evidence를 모두 거부한다", {
  skip: process.platform === "win32" ? "POSIX executable fixture" : false,
}, () => {
  const invalidCases = [
    { QA_FIXTURE_STORAGE_MALFORMED: "1" },
    { QA_FIXTURE_STORAGE_GATE: "wrong-gate" },
    { QA_FIXTURE_STORAGE_HEAD: "wrong-head" },
    { QA_FIXTURE_STORAGE_STATUS: "FAIL" },
    { QA_FIXTURE_STORAGE_DIRTY: "1" },
    { QA_FIXTURE_STORAGE_EVIDENCE: "none" },
  ];
  for (const environment of invalidCases) {
    const fixture = createFixture();
    try {
      const result = fixture.execute("g5d-storage-lifetime", environment);
      assert.equal(result.status, 1, `${result.stdout}\n${result.stderr}`);
      const report = fixture.report();
      assert.equal(report.status, "FAIL");
      assert.match(report.reasons.at(-1).code, /^storageEvidence(?:Malformed|Invalid)$/);
      assert.equal(report.results.at(-1).id, "m5:test:storage-scheduler");
    } finally { fixture.cleanup(); }
  }
});

test("Windows pnpm.cmd 선택과 standalone storage wrapper는 source에서 보존된다", () => {
  const source = readFileSync(resolve("scripts/m5-harness.mjs"), "utf8");
  assert.match(source, /process\.platform === "win32" \? "pnpm\.cmd" : "pnpm"/);
  const scripts = (JSON.parse(readFileSync(resolve("package.json"), "utf8")) as { scripts: Record<string, string> }).scripts;
  for (const [script, gate] of [["m5:test:storage-scheduler", "scheduler"], ["m5:test:storage-status", "status"], ["m5:test:storage-compaction", "compaction"]] as const) {
    assert.equal(scripts[script], `node scripts/m5-g5d-storage-gate.mjs --gate ${gate}`);
  }
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
  const changed = lowerQaPriority(() => current, (_pid: number, value: number) => { current = value; });
  assert.deepEqual(changed, { requested: 10, actual: 10, error: null });
  assert.deepEqual(lowerQaPriority(() => 15, () => assert.fail("must not raise priority")), { requested: 15, actual: 15, error: null });
  const denied = lowerQaPriority(() => 0, () => { throw new Error("denied"); });
  assert.equal(denied.actual, null);
  assert.match(denied.error!, /denied/);
  assert.match(lowerQaPriority(() => 0, () => {}).error!, /not lowered/);
});
