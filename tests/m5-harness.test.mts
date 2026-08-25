import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
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
  assert.match(result.stdout, /g1-foundation\s+NOT_READY/);
  assert.match(result.stdout, /g6-release\s+NOT_READY/);
});

test("필수 구현 스크립트가 없으면 성공이나 skip 대신 NOT_READY로 끝난다", () => {
  const result = run("--gate", "g1-foundation");

  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, /NOT_READY g1-foundation/);
  assert.match(result.stderr, /m5:test:electron/);
  assert.match(result.stderr, /m5:test:core/);
});
