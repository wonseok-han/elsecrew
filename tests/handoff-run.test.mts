/**
 * 자동 인계 화면의 **실행 추적·보류 중단** 회귀 테스트.
 *
 * 프레임워크를 새로 들이지 않는다 — `website/`가 이미 쓰는 `node --test`를 그대로 쓴다.
 * 타입은 노드의 타입 제거 실행으로 확인한다(`--experimental-strip-types`).
 *
 * ```bash
 * npm test
 * ```
 */

import assert from "node:assert/strict";
import test from "node:test";

import {
  NO_RUN,
  ownsStage,
  requestCancel,
  runReserved,
  runSettled,
  startingRun,
  type RunTracker,
} from "../src/handoffRun.ts";

/**
 * 화면이 하는 일을 그대로 흉내 낸 최소 대역.
 * 백엔드로 나간 호출을 순서대로 기록해, **무엇이 실제로 전달됐는지**를 본다.
 */
class ScreenDouble {
  run: RunTracker = NO_RUN;
  readonly calls: string[] = [];
  /** 예약을 기다리는 동안 화면이 멈춰 있는 지점 */
  private resolveReserve?: (runId: string) => void;

  /** `start()`의 앞부분 — 예약을 요청하고 응답을 기다린다. */
  beginRun(): Promise<string> {
    // ★ `begin_handoff`를 부르기 **전에** 이전 실행 ID를 버린다
    this.run = startingRun();
    this.calls.push("begin_handoff");
    return new Promise<string>((resolve) => {
      this.resolveReserve = resolve;
    });
  }

  /** 백엔드가 예약 응답을 돌려줬다. 반환값은 이 실행이 계속 진행되는가. */
  reserveReturns(runId: string): boolean {
    this.resolveReserve?.(runId);
    const decision = runReserved(this.run, runId);
    this.run = decision.next;
    if (decision.abort) {
      // 보류된 중단을 전달하고 **사본을 만들기 전에** 멈춘다
      this.calls.push("cancel_handoff");
      this.calls.push("release_handoff");
      this.run = runSettled();
      return false;
    }
    this.calls.push("prepare_codex_pilot");
    this.calls.push("run_handoff");
    return true;
  }

  /** `stop()` — 중단 버튼. */
  stop(): void {
    const decision = requestCancel(this.run);
    this.run = decision.next;
    if (decision.deliverNow) {
      this.calls.push("cancel_handoff");
    }
  }

  /** 실행이 결과와 함께 끝났다. */
  finish(): void {
    this.run = runSettled();
  }

  /**
   * **결함이 있던 옛 화면**: 결과를 받고도 실행 ID를 계속 들고 있었다.
   *
   * 이 상태를 일부러 만들어 두는 이유는, 종료 시 정리와 **무관하게**
   * 새 실행 시작이 이전 ID를 버리는지를 따로 확인하기 위해서다.
   * 방어가 하나뿐이면 다른 하나가 사라질 때 조용히 뚫린다.
   */
  leaveStaleRunId(runId: string): void {
    this.run = { activeRunId: runId, pendingCancel: false };
  }

  /** 진행 상태 이벤트가 화면을 바꾸는가. */
  receivesStage(runId: string): boolean {
    return ownsStage(this.run, runId);
  }
}

// ───────────────── 예약 대기 중 중단 ─────────────────

test("예약을 기다리는 동안 누른 중단이 새 슬롯으로 전달된다", () => {
  const screen = new ScreenDouble();

  screen.beginRun();
  screen.stop(); // 아직 슬롯이 없다 — 보류돼야 한다
  assert.deepEqual(screen.calls, ["begin_handoff"], "슬롯도 없는데 취소를 보냈다");

  const proceeded = screen.reserveReturns("run-1");

  assert.equal(proceeded, false, "중단했는데 실행이 계속됐다");
  assert.deepEqual(screen.calls, [
    "begin_handoff",
    "cancel_handoff",
    "release_handoff",
  ]);
  assert.equal(
    screen.calls.includes("prepare_codex_pilot"),
    false,
    "그만두라고 했는데 사본을 만들었다",
  );
});

/**
 * **Codex가 지적한 경합 그대로다.**
 *
 * 첫 실행이 끝난 뒤에도 화면이 그 ID를 들고 있으면, 두 번째 실행의 예약 대기 중에
 * 누른 중단이 "이미 실행 중"으로 오인돼 **이미 끝난 슬롯**으로 나간다.
 * 그 취소는 아무 일도 하지 않고, 새 실행은 그대로 진행된다.
 */
test("첫 실행이 끝난 뒤에도 두 번째 실행의 예약 대기 중 중단이 살아남는다", () => {
  const screen = new ScreenDouble();

  // ① 첫 실행이 끝났는데 화면에 그 ID가 남아 있다
  screen.beginRun();
  screen.reserveReturns("run-1");
  screen.leaveStaleRunId("run-1");

  // ② 두 번째 실행 — 예약 응답을 기다린다
  const before = screen.calls.length;
  screen.beginRun();

  // ③ 그 사이 중단을 눌렀다
  screen.stop();

  const delivered = screen.calls.slice(before);
  assert.deepEqual(
    delivered,
    ["begin_handoff"],
    "끝난 실행의 슬롯으로 취소가 나갔다 (아무 일도 하지 않는다)",
  );

  // ④ 새 ID가 도착하면 보류된 중단이 전달되고 실행은 멈춘다
  const proceeded = screen.reserveReturns("run-2");

  assert.equal(proceeded, false, "이전 실행 ID 때문에 중단이 유실됐다");
  assert.deepEqual(screen.calls.slice(before), [
    "begin_handoff",
    "cancel_handoff",
    "release_handoff",
  ]);
});

test("실행 중에 누른 중단은 보류하지 않고 곧바로 전달한다", () => {
  const screen = new ScreenDouble();
  screen.beginRun();
  screen.reserveReturns("run-1");
  const before = screen.calls.length;

  screen.stop();

  assert.deepEqual(
    screen.calls.slice(before),
    ["cancel_handoff"],
    "실행 중인데 취소를 붙잡고 있었다",
  );
});

test("보류된 중단이 다음 실행으로 새지 않는다", () => {
  const screen = new ScreenDouble();
  screen.beginRun();
  screen.stop();
  screen.reserveReturns("run-1"); // 여기서 소비된다

  // 다음 실행은 멀쩡히 진행돼야 한다
  const before = screen.calls.length;
  screen.beginRun();
  const proceeded = screen.reserveReturns("run-2");

  assert.equal(proceeded, true, "지난 실행의 보류된 중단이 새 실행을 죽였다");
  assert.deepEqual(screen.calls.slice(before), [
    "begin_handoff",
    "prepare_codex_pilot",
    "run_handoff",
  ]);
});

// ───────────────── 늦게 도착한 진행 상태 ─────────────────

test("끝난 실행의 늦은 진행 상태가 새 화면을 덮지 않는다", () => {
  const screen = new ScreenDouble();
  screen.beginRun();
  screen.reserveReturns("run-1");
  assert.equal(screen.receivesStage("run-1"), true);

  // 끝난 실행의 ID가 남아 있더라도 새 실행 시작이 그것을 버려야 한다
  screen.leaveStaleRunId("run-1");
  screen.beginRun();

  assert.equal(
    screen.receivesStage("run-1"),
    false,
    "이전 실행의 늦은 진행 상태가 새 화면 상태를 덮었다",
  );

  screen.reserveReturns("run-2");
  assert.equal(screen.receivesStage("run-1"), false, "여전히 남의 상태다");
  assert.equal(screen.receivesStage("run-2"), true);
});

test("실행이 끝나면 그 실행의 진행 상태도 더는 받지 않는다", () => {
  const screen = new ScreenDouble();
  screen.beginRun();
  screen.reserveReturns("run-1");
  assert.equal(screen.receivesStage("run-1"), true);

  screen.finish();

  assert.equal(
    screen.receivesStage("run-1"),
    false,
    "끝난 실행의 진행 상태가 여전히 화면을 바꾼다",
  );
});

test("따라가는 실행이 없으면 어떤 진행 상태도 받지 않는다", () => {
  assert.equal(ownsStage(NO_RUN, "run-1"), false);
  // 빈 ID를 실은 이벤트가 오더라도 "일치"로 새면 안 된다
  assert.equal(ownsStage(NO_RUN, ""), false);
});

// ───────────────── 상태 전이 자체 ─────────────────

test("새 실행 시작과 종료는 이전 실행 ID를 남기지 않는다", () => {
  assert.deepEqual(startingRun(), NO_RUN);
  assert.deepEqual(runSettled(), NO_RUN);

  // 보류된 중단도 함께 사라진다
  const pending = requestCancel(NO_RUN).next;
  assert.equal(pending.pendingCancel, true);
  assert.equal(startingRun().pendingCancel, false);
  assert.equal(runSettled().pendingCancel, false);
});

test("예약은 보류된 중단을 소비한다", () => {
  const pending = requestCancel(NO_RUN).next;

  const decision = runReserved(pending, "run-1");

  assert.equal(decision.abort, true);
  assert.equal(decision.next.activeRunId, "run-1");
  assert.equal(
    decision.next.pendingCancel,
    false,
    "소비한 중단이 그대로 남으면 다음 판단이 또 걸린다",
  );
});
