/**
 * 파일럿 실패가 **기본 화면에 영문 원문으로 새지 않는지** 확인한다.
 *
 * §7.6A 실사용에서 비개발자가 `pilot source must be a Git repository`를
 * 그대로 읽어야 했다. A5 실패다 — 전문 로그를 해석하게 만들면 안 된다.
 *
 * 사전 판정이 생겨 보통은 여기까지 오지 않지만, 화면을 그린 뒤 폴더가 바뀌면
 * (TOCTOU) 여전히 이 경로로 온다. 그때도 쉬운 안내가 나와야 한다.
 *
 * ```bash
 * npm test
 * ```
 */

import assert from "node:assert/strict";
import test from "node:test";

import { diagnoseError, toAppError } from "../src/doctor.ts";
import { ko } from "../src/locales/ko.ts";
import { en } from "../src/locales/en.ts";

/** 실제로 백엔드가 돌려준 값. `AppError { kind, detail }`로 온다. */
const REAL_FAILURE = {
  kind: "generic",
  detail: "pilot source must be a Git repository",
};

test("Git 저장소가 아니라는 실패가 쉬운 안내로 번역된다", () => {
  const diagnosis = diagnoseError(toAppError(REAL_FAILURE));

  assert.equal(diagnosis.titleKey, "doctor.notgit.title");
  assert.equal(diagnosis.adviceKey, "doctor.notgit.advice");
});

test("기본 화면 문구에 내부 영문 오류가 섞이지 않는다", () => {
  const diagnosis = diagnoseError(toAppError(REAL_FAILURE));

  for (const [lang, messages] of [
    ["ko", ko],
    ["en", en],
  ] as const) {
    const title = messages[diagnosis.titleKey];
    const advice = messages[diagnosis.adviceKey];

    assert.ok(title, `${lang}: 제목 문구가 없다`);
    assert.ok(advice, `${lang}: 안내 문구가 없다`);
    for (const text of [title, advice]) {
      assert.ok(
        !text.includes("pilot source"),
        `${lang}: 내부 오류 원문이 화면 문구에 들어갔다 — ${text}`,
      );
      assert.ok(
        !/git repository/i.test(text),
        `${lang}: 전문용어가 화면 문구에 그대로 남았다 — ${text}`,
      );
    }
  }
});

test("한국어 안내에 영문 전문용어가 없다", () => {
  const advice = ko["doctor.notgit.advice"];
  // 초보자 대상이다 — 영어 단어가 섞이면 그 자체가 장벽이다
  assert.ok(
    !/[A-Za-z]{3,}/.test(advice),
    `영문 단어가 남아 있다: ${advice}`,
  );
});

/**
 * **실사용에서 실제로 사용자를 막은 실패다.** 계획 단계가 인증에서 죽었는데
 * 원인이 화면에 닿지 못해, 사용자는 "안전하게 멈췄어요"만 보고 무엇을 해야 할지 몰랐다.
 * 해결책이 분명한 상황("다시 로그인")이므로 반드시 그렇게 안내해야 한다.
 */
test("로그인 만료가 다시 로그인하라는 안내로 번역된다", () => {
  const real = {
    kind: "generic",
    detail:
      "Failed to authenticate. API Error: 401 OAuth access token has been revoked.",
  };

  const diagnosis = diagnoseError(real);

  assert.equal(diagnosis.titleKey, "doctor.auth.title");
  assert.equal(diagnosis.adviceKey, "doctor.auth.advice");

  for (const [lang, messages] of [
    ["ko", ko],
    ["en", en],
  ] as const) {
    const advice = messages[diagnosis.adviceKey];
    assert.ok(advice, `${lang}: 안내 문구가 없다`);
    assert.ok(
      !advice.includes("OAuth") && !advice.includes("401"),
      `${lang}: 내부 오류 원문이 화면 문구에 들어갔다 — ${advice}`,
    );
  }
  // 무엇을 해야 하는지 말해 줘야 한다
  assert.ok(ko["doctor.auth.advice"].includes("로그인"));
});

/** 다른 실패 분류가 이 규칙에 휩쓸리면 안 된다. */
test("무관한 실패는 여전히 원래 분류로 간다", () => {
  assert.equal(
    diagnoseError({ kind: "generic", detail: "network is unreachable" })
      .titleKey,
    "doctor.network.title",
  );
  assert.equal(
    diagnoseError({ kind: "generic", detail: "permission denied" }).titleKey,
    "doctor.permission.title",
  );
});
