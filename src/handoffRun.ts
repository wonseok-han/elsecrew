/**
 * 자동 인계 화면이 **지금 어느 실행을 따라가고 있는가**, 그리고
 * **아직 전달하지 못한 중단이 있는가**.
 *
 * 이걸 컴포넌트 안의 ref 두 개로 굴리면 조용히 어긋난다. 실제로 어긋났던 경합:
 *
 * 1. 첫 실행이 끝났는데 화면은 그 실행 ID를 계속 들고 있다
 * 2. 두 번째 실행을 시작한다 (`begin_handoff` 응답 대기)
 * 3. 그 사이 사용자가 중단을 누른다
 * 4. 화면에 ID가 남아 있으니 "이미 실행 중"으로 보고 취소를 **곧바로 보낸다**
 * 5. 그런데 그 슬롯은 이미 끝난 실행의 것이라 취소는 아무 일도 하지 않는다
 * 6. 새 ID가 도착해도 보류된 중단이 없으므로 **두 번째 실행이 그대로 진행된다**
 *
 * 그래서 상태 전이를 순수 함수로 떼어 냈다. 규칙은 둘뿐이다 —
 * **새 실행을 시작할 때 이전 ID를 반드시 버린다**, 그리고
 * **실행이 끝나면 따라갈 대상이 없다고 표시한다.**
 */

export interface RunTracker {
  /** 화면이 따라가는 실행. 빈 문자열이면 따라갈 실행이 없다 */
  readonly activeRunId: string;
  /** 슬롯이 생기기 전에 눌린 중단. 예약되는 즉시 전달해야 한다 */
  readonly pendingCancel: boolean;
}

/** 따라갈 실행이 없는 상태. */
export const NO_RUN: RunTracker = { activeRunId: "", pendingCancel: false };

/**
 * 새 실행을 시작한다. **이전 실행 ID를 버리는 것이 이 함수의 전부이자 요점이다.**
 *
 * `begin_handoff`를 부르기 **전에** 불러야 한다. 응답을 기다리는 동안 중단을 누르면
 * 그 요청이 보류로 잡혀야 하는데, 이전 ID가 남아 있으면 "실행 중"으로 오인해
 * 이미 끝난 슬롯으로 취소를 보내고 만다.
 */
export function startingRun(): RunTracker {
  return NO_RUN;
}

/** `requestCancel`의 결정. */
export interface CancelDecision {
  readonly next: RunTracker;
  /** 지금 백엔드로 취소를 보낼 수 있는가. false면 보류됐고 예약 직후에 전달된다 */
  readonly deliverNow: boolean;
}

/**
 * 중단을 눌렀다.
 *
 * 따라가는 실행이 있으면 곧바로 전달하고, 아직 예약 전이면 **보류**한다.
 * 보류를 버리면 사용자는 눌렀는데 아무 일도 없는 화면을 본다.
 */
export function requestCancel(state: RunTracker): CancelDecision {
  if (state.activeRunId === "") {
    return {
      next: { ...state, pendingCancel: true },
      deliverNow: false,
    };
  }
  return { next: state, deliverNow: true };
}

/** `runReserved`의 결정. */
export interface ReservedDecision {
  readonly next: RunTracker;
  /**
   * 예약을 기다리는 동안 중단이 들어왔다.
   * 보류된 취소를 전달하고 **사본을 만들기 전에** 멈춰야 한다 —
   * 사용자가 그만두라고 한 작업의 사본을 만들어 둘 이유가 없다.
   */
  readonly abort: boolean;
}

/** `begin_handoff`가 실행 ID를 돌려줬다. */
export function runReserved(
  state: RunTracker,
  runId: string,
): ReservedDecision {
  return {
    next: { activeRunId: runId, pendingCancel: false },
    abort: state.pendingCancel,
  };
}

/**
 * 실행이 끝났다 — 결과가 왔든, 막혔든, 실패했든.
 *
 * 따라갈 대상을 비운다. 남겨 두면 다음 실행의 예약 대기 중에 누른 중단이
 * 이 죽은 ID 때문에 보류되지 못하고, 늦게 도착한 이 실행의 진행 상태가
 * 새 화면을 덮어쓴다.
 */
export function runSettled(): RunTracker {
  return NO_RUN;
}

/**
 * 이 진행 상태 이벤트가 화면이 따라가는 실행의 것인가.
 *
 * 따라가는 실행이 없으면(예약 대기 중이거나 이미 끝났으면) **무엇도 받지 않는다.**
 */
export function ownsStage(state: RunTracker, runId: string): boolean {
  return state.activeRunId !== "" && state.activeRunId === runId;
}
