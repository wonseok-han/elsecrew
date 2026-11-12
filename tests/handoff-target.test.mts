/**
 * 자동 인계 **대상 선택** 회귀 테스트 (§7.6A 실사용에서 드러난 A5 실패).
 *
 * 고정하는 것: 암묵적 선택이 없다 · 비Git은 시작되지 않는다 ·
 * 첫 프로젝트가 비Git이어도 뒤의 Git 프로젝트가 대신 골라지지 않는다.
 *
 * ```bash
 * npm test
 * ```
 */

import assert from "node:assert/strict";
import test from "node:test";

import {
  canStartPilot,
  chooseTarget,
  initialTarget,
  retainTarget,
  type PilotCandidate,
} from "../src/handoffTarget.ts";

/** 실사용에서 실제로 있었던 목록 모양 — 첫 프로젝트가 Git이 아니다. */
const PROJECTS: PilotCandidate[] = [
  { path: "/p/notes", pilotEligible: false }, // CLAUDE.md만 있고 .git 없음
  { path: "/p/elsecrew", pilotEligible: true },
  { path: "/p/scratch", pilotEligible: false },
];

const find = (path: string | null): PilotCandidate | null =>
  PROJECTS.find((project) => project.path === path) ?? null;

/**
 * 화면이 하는 일을 그대로 흉내 낸 최소 대역.
 * 백엔드로 나간 호출을 기록해 **비Git에서 실행이 시작되지 않는지**를 본다.
 */
class HomeDouble {
  target: string | null = initialTarget();
  goal = "";
  readonly calls: string[] = [];

  /** 프로젝트 카드의 "맡기기" 버튼. */
  pick(path: string): void {
    this.target = chooseTarget(PROJECTS, path);
  }

  /** 시작 버튼이 눌리는가 — 화면은 누를 수 없으면 비활성으로 그린다. */
  get startEnabled(): boolean {
    return canStartPilot(find(this.target), this.goal);
  }

  /** 시작 버튼을 누른다(비활성이면 아무 일도 없어야 한다). */
  start(): void {
    if (!this.startEnabled) return;
    this.calls.push(`begin_handoff:${this.target}`);
  }
}

// ───────────────── 암묵적 선택 없음 ─────────────────

test("화면이 처음 뜰 때 대상이 자동으로 정해지지 않는다", () => {
  assert.equal(initialTarget(), null);

  const home = new HomeDouble();
  home.goal = "인사말을 바꿔 줘";

  assert.equal(home.startEnabled, false, "고른 것도 없는데 시작할 수 있다");
  home.start();
  assert.deepEqual(home.calls, [], "고르지 않은 프로젝트에 작업이 갔다");
});

/**
 * **실사용에서 터진 그대로다.** 첫 프로젝트가 Git이 아닌데 자동으로 대상이 됐고,
 * 사용자는 시작을 누른 뒤에야 영문 오류를 봤다.
 *
 * `projects[0]`을 `projects.find(p => p.pilotEligible)`로 바꾸는 것도 답이 아니다 —
 * 그러면 이 테스트는 통과하지만 사용자는 여전히 **자기가 고르지 않은** `/p/elsecrew`에
 * 작업을 맡기게 된다. 그래서 "무엇도 고르지 않는다"를 확인한다.
 */
test("첫 프로젝트가 비Git이어도 뒤의 Git 프로젝트가 대신 골라지지 않는다", () => {
  const home = new HomeDouble();
  home.goal = "hi";

  assert.equal(home.target, null, `자동 선택됐다: ${home.target}`);
  assert.notEqual(home.target, "/p/notes", "비Git 프로젝트가 자동 선택됐다");
  assert.notEqual(
    home.target,
    "/p/elsecrew",
    "사용자가 고르지 않은 프로젝트가 자동 선택됐다",
  );
});

// ───────────────── 비Git 프로젝트 ─────────────────

test("비Git 프로젝트는 실행 가능 대상으로 잡히지 않는다", () => {
  assert.equal(chooseTarget(PROJECTS, "/p/notes"), null);
  assert.equal(chooseTarget(PROJECTS, "/p/scratch"), null);
});

test("비Git 프로젝트에서는 시작 동작이 호출되지 않는다", () => {
  const home = new HomeDouble();
  home.goal = "인사말을 바꿔 줘";

  home.pick("/p/notes");

  assert.equal(home.startEnabled, false, "비Git인데 시작 버튼이 살아 있다");
  home.start();
  assert.deepEqual(home.calls, [], "비Git 프로젝트에서 실행이 시작됐다");
});

test("목록에 없는 경로는 고를 수 없다", () => {
  assert.equal(chooseTarget(PROJECTS, "/p/does-not-exist"), null);
});

// ───────────────── Git 프로젝트 ─────────────────

test("Git 저장소인 프로젝트는 명시적으로 고른 뒤 실행할 수 있다", () => {
  const home = new HomeDouble();
  home.goal = "인사말을 더 친근하게 바꿔 줘";

  home.pick("/p/elsecrew");

  assert.equal(home.target, "/p/elsecrew");
  assert.equal(home.startEnabled, true);
  home.start();
  assert.deepEqual(home.calls, ["begin_handoff:/p/elsecrew"]);
});

test("목표를 적기 전에는 시작할 수 없다", () => {
  const home = new HomeDouble();
  home.pick("/p/elsecrew");

  assert.equal(home.startEnabled, false, "빈 목표로 시작할 수 있다");
  home.goal = "   ";
  assert.equal(home.startEnabled, false, "공백만 적어도 시작할 수 있다");
  home.goal = "인사말 바꿔 줘";
  assert.equal(home.startEnabled, true);
});

// ───────────────── 목록 새로고침 ─────────────────

test("고른 프로젝트가 자격을 잃으면 선택을 놓는다", () => {
  // 사용자가 고른 뒤 그 폴더의 .git이 사라졌다
  const after: PilotCandidate[] = [
    { path: "/p/elsecrew", pilotEligible: false },
  ];

  assert.equal(
    retainTarget(after, "/p/elsecrew"),
    null,
    "자격을 잃은 프로젝트를 계속 붙잡고 있다",
  );
});

test("고른 프로젝트가 목록에서 사라지면 선택을 놓는다", () => {
  assert.equal(retainTarget([], "/p/elsecrew"), null);
});

test("새로고침해도 멀쩡한 선택은 유지된다", () => {
  assert.equal(
    retainTarget(PROJECTS, "/p/elsecrew"),
    "/p/elsecrew",
    "새로고침마다 선택이 풀리면 쓸 수 없다",
  );
  assert.equal(retainTarget(PROJECTS, null), null);
});
