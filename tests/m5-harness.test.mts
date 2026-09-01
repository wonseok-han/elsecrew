import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { readFileSync } from "node:fs";
import test from "node:test";

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
