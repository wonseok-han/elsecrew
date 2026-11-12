/**
 * 자동 인계를 **어느 프로젝트에 맡길 것인가**.
 *
 * 처음 구현은 `projects[0]`을 말없이 대상으로 삼았다. 그래서 §7.6A 실사용에서
 * Git 저장소가 아닌 프로젝트에 작업을 맡기게 됐고, 사용자는 시작 버튼을 누른 **뒤에야**
 * `pilot source must be a Git repository`라는 영문 오류를 봤다. A5 실패다.
 *
 * 두 가지가 함께 틀렸다.
 *
 * - **암묵적 선택**: 사용자가 고르지 않은 프로젝트에 작업이 갔다.
 *   `projects.find(p => p.pilotEligible)`로 바꾸는 것도 같은 잘못이다 —
 *   여전히 사용자가 고르지 않는다.
 * - **사후 판정**: 쓸 수 있는지를 실행해 보고 알았다.
 *
 * 그래서 대상은 **항상 사용자가 명시적으로 고른 것**이고, 고를 수 있는지는
 * 화면을 그리는 시점에 이미 정해져 있다.
 */

/** 목록에 보이는 프로젝트 중 파일럿 판정에 필요한 부분. */
export interface PilotCandidate {
  readonly path: string;
  /**
   * 안전한 작업 사본을 만들 수 있는가. **백엔드가 실행과 같은 기준으로 채워 준다.**
   * 화면이 스스로 추측하지 않는다 — 추측하면 두 기준이 갈린다.
   */
  readonly pilotEligible: boolean;
}

/**
 * 화면이 처음 뜰 때의 대상. **언제나 "없음"이다.**
 *
 * 목록에 무엇이 있든 자동으로 고르지 않는다. 사용자가 어느 프로젝트에 작업을
 * 맡기는지 모른 채 시작 버튼을 누르는 일이 없어야 한다.
 */
export function initialTarget(): string | null {
  return null;
}

/**
 * 사용자가 프로젝트를 골랐다. 고를 수 없는 것은 **선택 자체가 되지 않는다.**
 *
 * 화면은 자격 없는 프로젝트의 버튼을 비활성으로 그리지만, 여기서 한 번 더 막는다 —
 * 목록이 새로고침되는 사이에 자격이 바뀔 수 있다.
 */
export function chooseTarget(
  projects: readonly PilotCandidate[],
  path: string,
): string | null {
  const picked = projects.find((project) => project.path === path);
  if (!picked || !picked.pilotEligible) return null;
  return picked.path;
}

/**
 * 목록이 새로고침됐다. 고른 프로젝트가 사라졌거나 자격을 잃었으면 선택을 놓는다.
 *
 * 붙잡고 있으면 화면에 없는 프로젝트에 작업을 맡기게 된다.
 */
export function retainTarget(
  projects: readonly PilotCandidate[],
  current: string | null,
): string | null {
  if (current === null) return null;
  return chooseTarget(projects, current);
}

/** 지금 시작 버튼을 누를 수 있는가. */
export function canStartPilot(
  target: PilotCandidate | null,
  goal: string,
): boolean {
  if (target === null || !target.pilotEligible) return false;
  return goal.trim() !== "";
}
