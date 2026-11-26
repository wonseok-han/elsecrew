// ADE 1차 통합 검증(V, 2026-11-26)에서 실제 앱으로 찾은 작은 결함의 시험.
import assert from "node:assert/strict";
import test from "node:test";

import { startingSentence } from "../src/m5/agent-output-window.ts";
import { en } from "../src/locales/en.ts";
import { ko } from "../src/locales/ko.ts";

test("a member that just started says what it is doing until its first activity (완료 기준 2)", () => {
  // Real runs showed 11–31 s of "저장된 출력을 불러오고 있어요." at every start.
  for (const state of ["preparing", "running"]) {
    assert.equal(startingSentence(state, null), true, `${state}, output not open yet`);
    assert.equal(startingSentence(state, []), true, `${state}, nothing yet`);
    assert.equal(startingSentence(state, [{ channel: "message" }]), true, `${state}, a message but no activity`);
    assert.equal(startingSentence(state, [{ channel: "activity" }]), false, `${state}, its own activity is shown`);
  }
  for (const state of ["completed", "failed", "cancelled", "cancelling"]) assert.equal(startingSentence(state, null), false, state);
  assert.equal(ko["m5.output.starting"], "받은 일을 읽고 시작하는 중이에요");
  assert.ok(en["m5.output.starting"]);
});

test("applying to the folder does not say 'commit' to a non-developer (완료 기준 6)", () => {
  assert.doesNotMatch(ko["m5.apply.confirm"], /커밋|브랜치/);
  assert.doesNotMatch(en["m5.apply.confirm"], /\bcommit|\bbranch/i);
  assert.match(ko["m5.apply.confirm"], /되돌릴 수 없어요/, "the warning itself stays");
});
