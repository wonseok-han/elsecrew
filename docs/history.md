# 작업 이력 (Elsecrew)

최신 macOS visible combined 측정(2026-11-10, HEAD1ebf8383)은 유휴441.58MiB만400MiB 기준을 초과했다. 최대746.36MiB·잔류증분33.34MiB·입력29.7ms·main/renderer long-task35.653/55ms는 이번 단회에서 기준 이내다. 진단 FAIL이며 전체G6/10회 반복 통과는 아니다. 시험 앱/임시 프로젝트는 정리했다. 유휴 예산은 사용자 승인으로512MiB로 변경했으며 과거 FAIL은 보존한다. Windows 후보076e23a0 추가CI 승인·실제 재검증 및 최종 통합·출시 검증이 남는다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #18 재확인(B1·B2 부분 해결) 반영 (`orca/ade-multiproject-a`)
재확인 `~/Documents/elsecrew-ade-multiproject-2026-11-26/REVIEW-pr18-2.md`(대상 13081fd3).

- **B1**
  - 겹침 검사를 모든 초기화보다 앞으로 옮겼다. `project.open`은 빈 폴더 Git 기록이나 일반 폴더 링크를 만들기 전에 `refuse_workspace_overlap`을 한다. 새 작업 폴더는 `create_dir_all` 전에 `refuse_new_task_folder`를 한다.
  - 선택 폴더는 실제 경로로 풀리지 않으면 거절한다. 비교는 구성 요소 단위이고, macOS에서는 대소문자를 무시한다.
  - `.git/commondir`가 있는 저장소는 등록에서 거절한다. 쓰기 범위의 git 폴더는 `commondir`을 따라간다.
  - 시험
    - `nothing_is_written_before_an_overlapping_folder_is_refused`(빈 폴더·일반 폴더·작업 공간 안을 가리키는 옛 작업 공간 폴더 설정 → 폴더에 아무것도 생기지 않음)
    - `a_shared_common_git_folder_is_refused_and_case_does_not_split_a_folder`
  - 실행 중 링크·디렉터리 신원이 바뀌는 경합(같은 사용자가 의도적으로 바꾸는 경우)은 기존 위협 모형처럼 보장 범위 밖이다. 저장·합치기의 하위 경로는 링크를 따라가지 않는 열기·핸들 검사를 그대로 쓴다.
- **B2**: 저장을 전부 아니면 무변경으로 바꿨다. 새 초안이 통째로 들어가지 않으면 다른 작업의 초안도, 이 작업 공간의 이전 초안(여러 파일)도 그대로다.
  - 시험
    - "a save over the bound leaves this workspace's earlier drafts of several files whole"
    - 속성 시험 "property: whatever the order, a draft store only ever changes by a whole save or a forget"(무작위 40판 × 80단계: 저장·잊기·떠나기·한도 초과·쓰기 거절)
    - 보관소 한도 시험 갱신
- **검증**
  - `cargo test -p elsecrew-sidecar` 단위 939, 통합 16 통과.
  - `npm test` 1008/1008, build, 렌더러 스모크 69/69.
  - CI(d9fc2071) macOS·Windows·website·GitGuardian 모두 통과.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #18 독립 검토(차단 2·중요 3·경미 1) 반영 (`orca/ade-multiproject-a`)
검토 `~/Documents/elsecrew-ade-multiproject-2026-11-26/REVIEW-pr18.md`(대상 82284ace). PR-B(`orca/ade-multiproject-b`, 로컬 WIP 커밋)는 이 수정이 머지될 때까지 멈췄다.

- **B1 경로 겹침**
  - 차단 판단을 작업 ID에서 실제 쓰기 경로로 바꿨다(`project/write_scope.rs` `WriteScope`: 작업 공간·프로젝트 폴더·저장소 git 폴더, 링크 해석). git 폴더가 같거나 폴더가 서로 안에 있으면 같은 잠금이다. 같은 저장소의 작업끼리는 이제 서로 기다린다. 경로를 구하지 못하면 fail closed.
  - 등록에서도 거절한다: 작업 공간 안이나 작업 공간을 담은 프로젝트, 작업 공간 안의 작업 공간 폴더, 기존 작업 공간과 겹치는 새 작업 폴더.
  - 시험
    - `a_project_or_workspace_folder_overlapping_a_tasks_workspace_is_refused`
    - `write_scopes_meet_on_a_shared_repository_or_nested_folders`
    - 경합 시험을 고침: 같은 저장소 C는 거절. 다른 저장소 B는 실제 저장(시작·조각·확정)과 합치기, D는 실제 버리기. A의 쓰기 회차가 그 사이에 늘었음을 확인한다.
- **B2 초안 유실**: 보관소가 다른 작업의 초안을 지우지 않고, 읽을 수 없는 옛 항목을 그대로 두며, 쓰기 거절 때 있던 것을 지우지 않게 바꿨다. 같은 작업의 다른 작업 공간 항목도 덮지 않는다. 못 넣는 새 초안은 보관하지 않고 `save`가 false를 돌려준다.
  - 시험: "kept drafts survive other saves: …", "leaving keeps another task's earlier draft; when the store is full the view stays …", 보관소 한도 시험 갱신.
- **I1**: 초안 보관 확인을 `releaseView` 앞으로 옮겼다(`prepareLeaveKeepingDrafts`). 실패하면 감시를 떼지 않고 그대로 머물러 이유를 보인다. 시험 "the switch keeps drafts before it lets go; …"
- **I2**: 작업 읽기에 실패한 승인 요청은 다음 목록에서 다시 다룬다. 질문 알림은 몇 번 다시 읽는다. 시험 "an approval whose task read failed once is announced on the next list"
- **I3**: 엔진 `workStatus`(모든 프로젝트의 바쁜 작업)로 업데이트 다시 시작과 배너를 판단한다. 읽지 못하면 일하는 중으로 본다.
- **M1**: 카드가 지금 대화 페이지에 없으면 그 페이지를 열고 카드로 간다(`revealStage`).
- **실제 앱(네이티브 스모크)**: 앱을 닫고 다시 여는 단계를 더했다. 두 프로젝트가 일하는 중에 닫고 같은 앱 데이터로 열면, 두 실행 모두 `canceled`로 정리되고 두 작업을 오갈 수 있었다. 두 단계 모두 PASS.
  - 발견: 다시 열 때 main이 엔진에 `--storage-seal`을 넘긴다. fixture 스모크의 spawn 대체가 이를 그대로 넘겨야 한다.
- **검증**
  - `npm test` 1000/1000, build.
  - 렌더러 스모크 69/69. 가짜 bridge에 `workStatus` 답을 추가했다. 없으면 '일하는 중'으로 판단해 업데이트 준비 검사가 실패한다.
  - `cargo test -p elsecrew-sidecar` 단위 928 통과(무시 45), 통합 16 통과.
  - feature/ade(PR #17 병합 f2d50132)를 병합했다(history 맨 위 엔트리 충돌만 있었고 양쪽 모두 남김, `m5:check:history` 누락 0). 병합 뒤 관련 sidecar 31개와 `npm test` 1006/1006을 다시 돌려 통과했다. CI(12f8d429) macOS·Windows·website·GitGuardian 모두 통과.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — 여러 프로젝트 동시 작업 PR-A: 단계 막대 전체 보기·실행 중 전환·작업 단위 쓰기 차단 (`orca/ade-multiproject-a`, 기준 feature/ade 331a206d)
계획·조사 `~/Documents/elsecrew-ade-multiproject-2026-11-26/PLAN.md`(본문 + 부록 "Orca 방식과 비교"). 현행 설명은 architecture "여러 프로젝트 동시 작업".

- **조사 결과(코드 변경 전, 같은 날 두 차례)**
  - 실행은 이미 화면과 무관하게 sidecar에서 돌았다. 프로젝트를 바꾸려면 멈춰야 했던 것은 renderer 규칙(`canLeaveProjectView`가 실행 중이면 거짓) 때문이었다.
  - sidecar의 진짜 제약은 둘이었다.
    - 공급자 잠금 파일이 공급자당 하나(`*-login.pending`)라 앱 전체에서 Codex 1·Claude 1개만 돈다.
    - 어느 팀이든 일하면 모든 프로젝트의 저장·합치기·버리기가 막혔다.
  - 둘 다 설계 선택이었다. 설치본 Claude Code 2.1.328은 같은 설정 폴더의 프로세스끼리 `.oauth_refresh.lock`(+ `.owner`, 죽은 소유자 인수)으로 토큰 갱신을 잠그고, Codex 0.167.2는 `auth.lock`·"guarded reload"를 쓴다(실행 파일 문자열 근거, 실제 동시 시험은 PR-B). 실행과 겹치면 안 되는 것은 연결 확인 때 sidecar가 직접 하는 Claude 갱신(`provider_claude_refresh.rs` `refresh_if_expired`)과 로그인·설치뿐이다.
- **단계 막대 전체 보기**(`stage-bar.tsx`)
  - '+N'은 버튼이다. 누르면 모든 단계를 가로로 스크롤되는 한 줄(`.m5-stage-bar-all`, 칸 148px)로 펼치고 가장 최근 단계에서 끝난다.
  - 펼친 단계를 누르면 그 팀원 카드로 이동한다. '접기'로 되돌리고, 다른 작업은 접힌 채 시작한다.
  - 예전 `m5.stage.hidden` 키는 `showAll`·`fold`·`jump`로 바꿨다.
- **P1 실행 중 전환**
  - `canLeaveProjectView`는 오는 중인 요청만 보고, 팀이 일하는지는 새 `isTeamWorking`이 답한다. 업데이트 다시 시작·업데이트 배너·작업 버리기 버튼은 `isTeamWorking`을 쓴다.
  - 저장하지 않은 편집은 `leaveProjectView(false, true)`가 초안 보관소에 넣고, 읽어 확인한 뒤에만 떠난다. 다시 열면 되살린다.
  - 전환 이유 문구를 `m5.switch.busy`·`unsavedHeld`로 바꾸고 `m5.switch.teamWorking`은 지웠다.
- **배지**
  - `project-activity.ts`가 실행 상태 알림으로 프로젝트별 '작업 중'을 셈한다(실행 종료 뒤 20초는 다음 담당을 고르는 중으로 봄, 프로젝트를 열 때 목록으로 맞춤).
  - 전환 팝오버·홈 줄에 '확인 필요 N'·'작업 중', 전환 단추에 다른 프로젝트 상태 점을 단다. 알림 패널(U6)·다른 프로젝트 알림 열기(PR #14 I4)는 그대로 재사용한다.
- **P2′ 작업 단위 쓰기 차단**(`work_scope.rs`)
  - 저장·합치기·버리기·정리·내용 삭제는 그 작업만 기다린다. 사본을 사용자 폴더에 넣는 6개 동작과 `application_running` 슬롯은 전역 그대로다.
  - 작업에 묶을 수 없는 일이 있으면 전역 규칙으로 돌아간다. 거절 코드는 예전 그대로다.
  - `AgentRuntime::run_chats`(`discovery_cleanup`이 있으면 `None`), `G5Manager::active_chat_ids`, `ProjectManager::pending_save_target`을 더했다. 안 쓰게 된 `runs_in_flight`는 지웠다.
  - fake 시나리오 `busy-writer`(작업 공간에 15ms 간격 200번 쓰기)를 더했다.
- **시험**
  - 경합 시험 3개(`orchestration_dispatch.rs`):
    - `other_tasks_save_merge_and_discard_while_a_team_writes_its_own_workspace`: A가 쓰는 동안 같은 저장소 C·다른 프로젝트 B를 실제로 합침. A 자신의 것과 B의 사본 반영은 거절. 끝난 뒤 `git fsck`·깨끗한 상태·자기 파일만.
    - `work_in_flight_without_a_task_blocks_every_task`
    - `a_save_to_a_project_view_waits_only_for_that_projects_tasks`
  - 단위:
    - `tests/m5-multiproject.test.mts`(5)
    - chat-state "the project view can be left while the open task's team works…"
    - editor "leaving a project keeps the open task's unsaved text…", "text that cannot be kept holds the project…"
  - 렌더러 스모크 지점 33(접힌 단계)·34(펼친 단계)을 더하고, 25(전환 팝오버)에 다른 프로젝트 '작업 중' 배지와 상태 점 검사를 더했다.
  - 네이티브 스모크 `pnpm m5:smoke:multiproject`(`scripts/m5-multiproject-ui-run.mjs` → `m5-multiproject-ui-smoke.cjs`): 실제 sidecar, 임시 앱 데이터, fixture `cancel-clean`(30초 동안 일함).
    - A 작업의 에이전트가 일하는 중에 한 번에 B로 바꿨다. A 줄에 '작업 중', 전환 단추에 점이 보였다.
    - B도 실행을 시작했고, 엔진 기록에서 두 실행이 동시에 `running`이었다.
    - A로 돌아오니 그 작업이 진행 중 그대로 보였다. 검사 19개 PASS.
    - 이 시험이 찾아 고친 것: 작업 화면의 '일하는 중'(`teamWorking`)은 막 만든 초안(draft)도 포함한다. 그래서 시작하지 않은 B 작업이 배지에서 '작업 중'으로 보였다. 목록 배지는 `listedWorking`(preparing·running·stopping·applying)을 쓴다.
    - 함정: fixture 엔진의 저장소는 첫 역할 구성(총괄·V3 역할 없음)이라 새 작업 창과 팀 진행은 여기서 못 돈다. 작업·실행은 bridge로 만들고, 팀 진행은 sidecar 경합 시험이 다룬다.
- **검증**
  - `cargo test -p elsecrew-sidecar`: 924 통과, 실패 2. 두 실패는 같은 작업 버리기 거절의 오류 설명 문구가 바뀌어서였고, 문구에 기존 표현("idle native execution")을 남겨 고친 뒤 두 시험만 다시 돌려 통과했다.
  - `npm test` 988 → 새 시험 포함 통과, `pnpm build`.
  - `pnpm m5:test:editor` 69/69(다크·라이트, 렌더 대비 라이트 1479곳 실패 0).
  - 캡처는 `~/Documents/elsecrew-ade-multiproject-2026-11-26/pr-a/`에 있다(렌더러 `dark/`, 네이티브 `native/`).
- **PR**: https://github.com/wonseok-han/elsecrew/pull/18 (`--base feature/ade`). 본문 캡처는 39cccc59에 올리고 2a42e728에서 지웠다. CI(2a42e728) macOS·Windows(기준선 하네스 포함)·website·GitGuardian 모두 통과.
- **남은 것(PR-B)**
  - P5 공급자 병렬 실행: 실행별 잠금 파일, 로그인·설치만 배타, 실행 중 sidecar Claude 갱신 생략.
  - P3′ 상한과 '차례 기다리는 중', P4 전체 수신함(다시 불러오기 전에 시작된 다른 프로젝트 실행도 잡기).
  - 실제 계정 동시 실행 시험.
  - history.md가 500줄을 넘었지만 엔트리가 모두 같은 날짜이고 고정 절만 약 400줄이라, 날짜를 쪼개지 않는 규칙으로는 300줄 이하로 롤오버할 수 없어 두었다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #17 재확인(B1 해소, 새 중요 1건) 반영: 큰 줄 자리 기한 (`orca/ade-stability`)
재확인 `~/Documents/elsecrew-ade-stability-2026-11-26/REVIEW-pr17-2.md`: B1 해소. 새 중요: 끝나지 않는 줄 두 개가 큰 줄 자리 2개를 기한 없이 잡으면 다른 실행도 막힐 수 있다.

- **수정** (`provider_login_io.rs` `LargeLineLimits`):
  - 자리를 잡은 줄은 총 30초(`hold`) 안에 끝나야 한다.
  - 10초 동안 늘지 않으면(`stall`) 그 줄을 거부한다. 무진행은 파이프가 비었을 때만 판정한다. 그래서 sidecar 폴링이 늦어진 것을 멈춤으로 오인하지 않는다.
  - 자리를 기다리는 연결은 90초(`wait`, `hold`보다 길게)를 넘으면 그 세션만 실패한다.
  - 어느 경우든 해당 연결만 실패하고 버퍼와 자리를 돌려준다.
  - 거부 이유는 고정 진단 이름이다("a large line stopped arriving", "a large line took too long", "waited too long for a large-line slot").
- **시험**: `stuck_large_lines_release_their_slots_and_another_run_proceeds`, `a_session_waiting_too_long_for_a_slot_fails_alone`.
  - 첫째: 미완성 줄 두 개가 자리를 잡고 멈추면 무진행 기한에 거부되고 자리가 돌아온다. 세 번째 실행은 5초 안에 큰 줄 3개를 모두 받는다.
  - 둘째: 대기 상한을 넘은 연결만 실패한다. 자리 보유자는 계속 진행한다.
  - 시험 기한은 연결별로 줄여 쓴다(`set_large_line_limits`, 시험 전용). 자리를 쓰는 시험들은 `LARGE_LINE_TEST_LOCK`으로 서로 겹치지 않게 했다(Claude 세션 시험 포함).
- **검증**: sidecar 관련 묶음 15 통과. 전체 Rust·CI는 다음 줄에 덧붙인다.
- **전체 검증(bd9e9b5c)**: `cargo test --workspace` core 259·protocol 75·sidecar lib 932·stdio_process 16 실패 0. CI(run 45833521943) macOS·Windows·website·GitGuardian 모두 통과. TS 변경 없음. 남은 것: 머지. 큰 Write 권한 요청(128KiB 초과)은 별도 설계.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #17 Codex 검토(차단 1·중요 4·경미 2) 반영 (`orca/ade-stability`)
검토 `~/Documents/elsecrew-ade-stability-2026-11-26/REVIEW-pr17.md`(대상 d9562788). 원칙(조정자): 승인에 쓰는 값(화면 표시·지문·되돌려 보내는 입력)은 원문만 쓰고, 축약은 관측·표시 전용으로 둔다.

| 지적 | 처리 | 시험 |
| --- | --- | --- |
| B1 Codex 승인 명령·폴더 축약 | Codex는 포괄 축약을 버리고 **메서드별로 버려도 되는 필드만** 줄인다: `item/started`·`item/completed`의 `commandExecution.aggregatedOutput`·`fileChange.changes[].diff`, `turn/diff/updated`의 `diff`, 도구 출력 delta. `command`·`cwd`·id·상태와 그 밖의 필드·메서드·요청/응답은 그대로다. 그래서 한도를 못 맞추면 예전처럼 실패한다. Claude는 승인이 `control_request`에서만 오고 그 줄은 줄이지 않는다(기존 그대로). | core `codex_approval_and_completion_use_the_original_command_after_shortening`(시작·승인·완료 모두 큰 줄, 명령 10KB·폴더 6KB 원문 유지, 턴 완료), `codex_approval_filled_from_the_start_keeps_the_original_command`, `claude_allow_echoes_the_original_input_after_shortened_frames`(축약된 assistant 뒤 허용 응답 `updatedInput` = 원문), `codex_notifications_shrink_but_requests_and_public_text_do_not`(모르는 종류·메서드, 버릴 수 없는 필드 초과는 거부) |
| I1 자원 | 프로세스 전체 **큰 줄 자리 2개**(`LARGE_LINE_SLOTS`). 128KiB를 넘는 줄은 자리를 얻어야 더 읽고, 자리가 없으면 읽기를 멈춘다(공급자는 쓰기에서 기다리며 손실 없음). 자리 보유자는 반드시 끝나거나 실패하므로 교착이 없다. 줄당 JSON 값 개수 상한 64K를 파싱 전에 센다(`count_values`). 줄은 복사 없이 떼어 내고(`split_off`), 실패 때 버퍼 메모리와 자리를 돌려준다. 상한: 자리당 최악 약 24MiB(원문 8MiB + 읽기 64KiB + 트리 + 결과), 두 자리 48MiB, 자리 없는 연결은 192KiB까지. 7.8MB 줄 축약 14ms(디버그). | sidecar `concurrent_large_lines_share_the_process_wide_slots`(4세션 동시, 보유 ≤2, 자리 없는 연결 버퍼 ≤192KiB, 전부 완료), `a_refused_large_line_returns_its_slot`, core `many_small_values_are_refused_before_parsing`, `a_line_near_the_raw_bound_is_shortened_quickly` |
| I2 거부된 `blob.chunk`가 일반 구독자로 감 | 거부 스트림 청크는 검사 뒤 바로 반환해 어떤 구독자·renderer에도 가지 않는다. | `PR #17 review I2: … reach no listener …`(수정 전 실패 확인) |
| I3 거부 스트림 검증 누락 | 거부 스트림도 같은 맵에서 같은 검사(크기·개수·**해시**)를 거친다. 모든 스트림에 청크 종류(`blob.chunk`/`workspace.blob.chunk`)와 작업 공간 주소를 묶는다. 작업 공간 청크는 거부 여부와 무관하게 `validateWorkspaceEventV1`을 먼저 통과해야 한다. | `PR #17 review I3: … hash, kind, address and workspace contract`(수정 전 실패 확인) |
| I4 Codex 도구 출력 delta | `item/commandExecution/outputDelta`·`item/fileChange/outputDelta`의 delta는 줄이고, 메시지·계획 delta(공개 문장)는 줄이지 않는다. | `codex_tool_output_deltas_shrink_and_message_deltas_do_not` |
| M1 줄 수 상한 초과 | 수를 지킬 수 없으면 원문을 빼서 "모름"으로 둔다. Claude 새 파일은 `content`를 빼고(`lines` None), Codex는 `diff`를 뺀다. Codex `FileChange`의 줄 수를 `lines: Option<(u64,u64)>`로 바꿔, diff 없는 변경은 0/0이 아니라 표시하지 않는다. | `a_new_file_over_the_count_cap_has_no_line_count`, Codex 시험의 4만 줄 diff, `tool_details_are_read_leniently_and_bounded` |
| M2 진단 | 거부 이유를 고정 이름(`OversizedLineRefusal`)으로 돌려주고 stderr에 `provider session line refused: <이유>`만 남긴다(공급자 원문 없음). | 각 거부 시험이 이유를 단정 |

- **history 롤오버**: 새 기준(엔트리 부분만 계산, 500줄 초과 시)으로 엔트리 부분 약 170줄이라 대상이 아니다.
- **검증**: core 259(축약기 14), sidecar 영향 묶음 22, `npm test` 994/994, `pnpm build`. 전체 Rust·CI 결과는 아래 줄에 덧붙인다.
- **전체 검증(f193d7c2)**: `cargo test --workspace` core 259·protocol 75·sidecar lib 930·stdio_process 16 실패 0. CI(run 45830531151) macOS·Windows·website·GitGuardian 모두 통과. 남은 것: 노예의 B1 재확인, 머지. 큰 Write 권한 요청(128KiB 초과)은 여전히 세션을 끊는다(별도 설계).

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — history 롤오버 기준 변경 (`orca/ade-stability`, 조정자 지시)
- **결정(사용자 결정, 조정자 전달)**: 롤오버 줄 수는 **워크로그 엔트리 부분만** 센다(고정 섹션·머리말·아카이브 포인터 제외). 엔트리 부분이 500줄을 넘으면 **날짜와 상관없이 가장 오래된 엔트리부터** `docs/archive/`로 옮겨 300줄 이하로 줄인다. 같은 날짜가 나뉘어도 되지만 엔트리 하나는 쪼개지 않는다. 옮긴 뒤 `pnpm m5:check:history`는 반드시 통과해야 한다. `CLAUDE.md`·`AGENTS.md`(같은 내용)·`docs/README.md`에 반영했다.
- **현재 상태**: 파일 전체 550줄 중 엔트리 부분은 152줄이라 새 기준으로는 롤오버 대상이 아니다. 앞 엔트리의 "롤오버 보류"는 이 결정으로 해소됐다. 고정 섹션만 약 400줄로 큰 것은 별개 문제다(정리는 이번 범위 밖).
- **검증**: `pnpm m5:check:history`(기준 origin/feature/ade) 통과, `CLAUDE.md`·`AGENTS.md` 동일 확인.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — 실행 안정성 D1 보강·D2 큰 줄 (`orca/ade-stability`, 기준 feature/ade 8c036abd)
근거: 1차 통합 검증 보고서 `~/Documents/elsecrew-ade-v-2026-11-26/REPORT.md`의 D1·D2.

- **D1 원인(재확인)**: `#registerStream`의 한도 초과 throw가 stdout 처리(`#onEnvelope`) 안에서 나서 그대로 `protocolInvalid` → 엔진 재시작이 됐다. 한도를 채운 누수(`workspace.blob.chunk` 미정리)는 PR #15가 고쳤지만, 그 뒤에도 동시 17개면 여전히 엔진 실패였다(새 시험 4개 중 3개가 #15 머지본에서 실패 확인).
- **D1 보강**: 한도 초과는 `SidecarStreamCapacityError`로 그 요청만 실패시킨다. 거부한 스트림은 크기·청크 수만 기억해 남은 청크를 조용히 버리고 화면에도 보내지 않는다(최대 64개). 알 수 없는·중복·크기 초과·완료 뒤 청크는 계속 프로토콜 위반이다. 빈 스트림은 자리를 차지하지 않는다. 대기열은 쓰지 않았다: sidecar는 응답 직후 청크를 같은 순서로 보내므로 정상이면 스트림이 동시에 쌓이지 않고, 자리가 새는 경우 대기열은 영원히 멈출 수 있다. 시험 `tests/m5-sidecar-stream-capacity.test.mts`(4개).
- **D2 원인**: Claude·Codex 세션 출력은 모두 `LoginIo::poll_inner`를 거친다. 한 줄 128KiB 초과뿐 아니라 **연결당 누적 수신 1MiB(`PROVIDER_OBSERVATION_TOTAL_MAX_BYTES`) 초과도 실패**시켰다. 줄 한도만 고치면 큰 이미지 7장쯤에서 다시 끊긴다. 보내는 쪽 누적(승인 응답·후속 입력) 1MiB도 같은 문제다. Codex 경로도 같은 한도다(큰 명령 출력·`turn/diff/updated`·fileChange diff).
- **D2 수정**: 세션이 묶인 뒤(Claude 세션 채널 생성, Codex 스레드 일치 후 턴)만 `admit_session_lines`로 세션 모드를 켠다. 한 줄을 8MiB까지 읽고(`PROVIDER_SESSION_LINE_RAW_MAX_BYTES`), 128KiB를 넘으면 `elsecrew_core::provider::oversized_line`이 긴 문자열(4KiB 초과)을 앞 256바이트 + 생략 표시로 줄여 관측한다. 중복 키 검사는 줄이기 전에 한다. 세션 모드에서는 64KiB씩 읽고 줄바꿈 탐색 위치를 기억한다(긴 줄 반복 스캔 방지).
  - 줄이는 것: Claude 도구 결과(`user`)와 무시되는 프레임, assistant의 비공개 블록(공개 text는 그대로). Codex는 id 없는 알림만(delta·agentMessage/plan text 제외).
  - **절대 줄이지 않는 것**: `control_request`·`control_cancel_request`·`control_response`·`result`·`system/init`, Codex의 모든 요청·응답. 허용 응답이 입력을 `updatedInput`으로 그대로 되돌려 보내므로 줄이면 사용자 파일이 잘린 내용으로 써진다(데이터 손상 경계). 이런 줄은 지금처럼 한도 초과 시 실패한다.
  - 작업 기록이 읽는 값은 보존한다: 새 파일 줄 수(`tool_use_result.content` → 같은 줄 수의 빈 줄), Codex fileChange의 추가/삭제 줄 수(`diff_counts`가 같은 값을 내는 짧은 diff), 종료 코드 앞부분.
  - **안전 경계 변경(검토 대상)**: 세션 모드의 연결 누적 한도(받기·보내기)를 1MiB → 256MiB(DurableV1 세션 예산과 같음)로 올렸다. 바이트 수를 셀 뿐이며 메모리에 동시에 드는 것은 한 줄(최대 8MiB)이다. 세션 파서의 자체 예산은 그대로다. 로그인·사전 점검 교환은 엄격한 128KiB/1MiB를 유지한다.
- **재현 시험**: core `provider::oversized_line::tests`(7, 148KB 이미지·공개 text 보존·권한 요청 거부·중복 키·Codex·작업 기록 수 보존·이미지 10장 세션), sidecar `provider_login_io` 실제 파이프(이미지 줄 10개 + 누적 1MiB 초과 통과, 세션 모드 없으면 실패, 권한 요청 거부, 9MB 무종단 줄 거부), `provider_claude_session::tests::a_session_keeps_going_after_reading_ten_large_images`(가짜 Claude 프로세스 → 세션 채널, 수정 없는 대조 시험은 실패).
- **검증**: `cargo test -p elsecrew-core` 252, protocol 75, `-p elsecrew-sidecar` lib 928·stdio_process 16(실패 0), `npm test` 963/963, `pnpm build`.
- **함정**: scm_breeze 환경에서 `cd <폴더> && ...`의 `cd`가 조용히 실패해 뒤 명령이 현재 워크트리에서 돈다. 시험 폴더용 `git add -A && git commit`이 이 워크트리 변경을 엉뚱한 커밋으로 묶은 일이 있었다(곧바로 `reset --soft`로 되돌림, 손실 없음). 다른 폴더 작업은 `git -C <절대경로>`와 절대경로만 쓴다.
- **실제 앱 확인(기본 프로필, 개발 앱 PID 15327만 띄우고 종료)**: 새 시험 폴더 `~/Downloads/elsecrew-d2-image-test`(86KB PNG 3장, git)를 프로젝트로 추가하고 팀 역할 기본값을 저장(버전 2, 총괄 Claude default)했다. 작업 "screens 폴더의 PNG 세 장을 Read로 직접 열어 설명"을 보냈다. 앱 전용 Claude 프로필의 세션 기록에서 도구 결과 줄이 232,168·232,168·232,128바이트(한도 131,072 초과)였고, 세션은 끊기지 않고 27초 만에 "답변 완료"로 끝났다(에이전트가 세 그림을 바르게 설명). 앱 로그 오류 0, 정상 종료(engine exit 0). 증거 `~/Documents/elsecrew-ade-stability-2026-11-26/`. 앱 데이터 변화: 프로젝트 1개·작업 1개 추가, 그 프로젝트 팀 역할 저장. `todo-test-1126g` 프로젝트 화면은 닫기만 했다(작업 보존).
- **PR**: https://github.com/wonseok-han/elsecrew/pull/17 (`--base feature/ade`). feature/ade(PR #14·#16) 병합 뒤 `npm test` 992/992·`pnpm build`, CI(run 45826807799) macOS·Windows·website·GitGuardian 모두 통과.
- **history 롤오버 보류**: 이 파일이 500줄을 넘었지만 작업 엔트리가 모두 11-26 같은 날짜이고 고정 섹션만 약 400줄이라, 날짜를 쪼개지 않고는 300줄 이하로 줄일 수 없다. 조정자 판단 필요.
- **남은 일**: Codex 검토 대응. 큰 Write의 권한 요청(입력 128KiB 초과)은 여전히 세션을 끊는다 — 허용 응답 프레임 자체도 128KiB 한도라 별도 설계가 필요하다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #16 독립 검토(병합 차단 1·중요 4·경미 3) 반영과 새 #14 위로 맞춤 (`orca/ade-screens-u7-u8`)
검토 `~/Documents/elsecrew-ade-screens-u5-u8-2026-11-26/REVIEW-pr16.md`(대상 f06a9e8b). PR #14(검토 반영본, 42e59ad7)를 이 브랜치에 병합했다.
- U5·U6 엔트리는 #14의 원문 그대로 남겼다. 앞서 이 브랜치에서 그 엔트리에 덧붙였던 PR 링크는 #14 쪽 엔트리로 옮겨졌다.
- 작업 화면의 `story` 줄은 PR #11의 새 속성과 U7 빈 상태를 합쳤다.

- **B1 병합 충돌**: #14가 feature/ade(PR #11·#13)를 병합해 충돌을 풀었고, 이 브랜치는 그 #14를 병합했다.
- **I1 하네스**: 업데이트 시험이 main의 실제 다시 시작 콜백을 쓴다. 가짜 종료 조정자는 실제처럼 안전 결과에서 깨끗한 종료를 허락한다.
  - 시험: 보안 하네스 "update restart: main's yes, then the screen's readiness and the quit preparation decide; only a clean result installs"
    - 기본 대화상자에서 '다시 시작' 뒤에 화면 준비 거절·무응답(시간 초과)·안전 종료 불가이면 설치하지 않는다.
    - 준비됨 + 안전이면 설치한다.
    - 요청하지 않은 준비 답은 거절한다.
  - 신뢰 경계는 architecture에 따로 적었다. 기본 동의는 위조할 수 없고, 화면 준비의 참·거짓은 화면 콜백이 준다.
- **I2 반복 실패**: main이 알리는 모든 상태에서 닫힘을 푼다. 확인을 요청할 때도 푼다.
  - 시험: 단위 "PR #16 I2", 렌더러 스모크(실패 → 닫기 → 확인 중 → 실패 → '다시 확인' 다시 보임).
- **I3 문구 검사 범위**: en 사전, `src`·`electron` 모든 TS/TSX 코드 글자, 하위 폴더, 따옴표 종류를 넓혔다. 기록 탭 원문 실패 코드를 '자세히'로 옮겼고, 팀에게 보내는 메시지와 en의 recovery point·snapshot 문구를 바꿨다.
  - 시험: 단위 "U7: no on-screen string uses developer words…", "details-only strings … (any quote, any folder)".
- **I4 Q7 진입점**: 메뉴의 확인·받기는 앱 안 배너로, 쓸 수 없는 실행은 설정 › 일반으로 보낸다. 설정의 확인은 `request("check")`다. 배너에 확인 중·최신 버전을 더했다. 기본 대화상자는 다시 시작 확인만 남았다.
  - 시험: 단위 "PR #16 I4".
- **경미**
  - M1: 작업 버리기 검사 중에도 취소·Esc로 나갈 수 있고, 검사가 끝나면 취소에 초점을 되돌린다. 시험 "PR #16 M1".
  - M2: CSS 검사가 '클래스가 코드에 있는지'만 본다는 한계를 시험 주석에 적었다. 렌더러 스모크에 빈 프로젝트 틀·주 행동·예시 줄·작업 목록 안내의 computed style 비교를 더했다.
  - M3: hex 검사를 `src`·`electron` 모든 TS/TSX의 문자열 안 어디든, 그리고 JSX 글자의 6·8자리 hex까지 넓혔다. 예외 셋은 그대로다.
  - PR #11이 남긴 `m5-activity-original`·`m5-activity-code`는 쓰이지 않는다. 실행 출력 작업자 영역이라 지우지 않고, 시험 예외에 이유와 함께 적었다.
- **검증**: `npm test` 982/982, build, 렌더러 스모크 다크·라이트(32화면 1265개 실패 0).
- **병합·CI**: feature/ade에 PR #15가 들어와 #14(71369834)와 #16 모두 다시 병합했다(누락 0). 병합 뒤 `npm test`는 #14 975, #16 988 모두 통과했다. 렌더러 스모크 두 테마, 미리보기 스모크 각 2회도 PASS다. #14 CI는 macOS·Windows·website·GitGuardian 모두 통과했다. #16 Windows CI에서 새 hex 시험이 경로 구분자(`electron\\appearance.ts`) 때문에 실패해, 경로를 `/`로 맞췄다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — 화면 통일 U7(문구·F11·빈 상태·확인)·U8(죽은 CSS·색·업데이트 배너) (`orca/ade-screens-u7-u8`, PR #14 위)
설계 DESIGN 4.1 U7·U8, 목업 13·15·16. PR #14(U5·U6) 위에 쌓았다. 현행 설명은 architecture "오류·빈 상태·업데이트 배너와 정리".

- **U7**
  - 금지어 시험(`tests/m5-ade-screens-u7-u8.test.mts`): ko 문자열과 main 기본 대화상자에 커밋·브랜치·복구점·식별자·준비 기록·worktree·스냅샷이 0건이다. '자세히' 전용 키 3개는 `<details>` 안에서만 쓰이는지 확인한다.
  - 1차 통합 검증 기준 6에 남아 있던 합치기 차단·충돌·다른 상태 안내와 작업 폴더 도움말의 '브랜치'·'(커밋)'을 없앴다. 준비 기록·복구점·저장 식별자 문구도 바꿨다(스모크·정리 프로브 문구 함께 갱신).
  - F3(종 안 작업 상태)를 `RecoveryGuide`로 바꿨다. 빈 상태 공용 틀 `EmptyState`를 만들어 빈 프로젝트 가운데·변경 없음·검색 결과 없음·기록·작업 목록에 썼다. 작업 버리기를 공용 확인 대화상자로 맞췄다(첫 초점 취소).
- **U8**
  - 죽은 CSS 규칙 약 270개를 지웠다(약 30KB). 지운 뒤 다크·라이트 렌더러 스모크 캡처를 지우기 전과 픽셀 비교했고, 의도한 문구·빈 상태·F3 변화 말고 차이는 없었다.
  - electron 색 hex는 창 배경 둘(토큰 값과 같은지 시험)과 미리보기 흰색만 남았다.
  - 업데이트 배너
    - main이 상태를 갖고 IPC `m5:app-update:state/changed/request`로 알린다.
    - 화면은 check·restart 요청만 보낸다. restart는 main의 기본 대화상자 확인과 기존 안전 종료 확인을 거친다.
    - 보안 하네스가 지키는 것: 다른 프레임·잘못된 요청은 거절, 확인에서 취소하면 다시 시작 없음, 받은 업데이트가 없으면 대화상자도 없음, 상태 변화가 화면에 전달됨.
  - 메뉴의 '저장했어요 — 재시작' 문구를 '다시 시작'으로 바꿨다.
- **시험·검증**
  - 새 `tests/m5-ade-screens-u7-u8.test.mts`(8)과 보안 하네스 1개를 더했다.
  - 렌더러 스모크 지점 31 빈 프로젝트, 32 업데이트 배너를 더해 32곳이다.
  - `npm test` 966/966, build 통과. 렌더 대비 다크·라이트 각 32화면 실패 0.
  - 미리보기 UI 스모크 다크·라이트 각 2회.
- **남은 것**
  - B5·F5·F6 배너는 이미 상태 토큰 면이라 구조는 그대로 두었다.
  - G7(원본 반영 미리보기)은 사본 방식 작업에만 남은 화면이라 문구만 검사 대상이다.
- **PR**: #16(https://github.com/wonseok-han/elsecrew/pull/16, base feature/ade, #14 위). 0e3b8af9 CI macOS·Windows·website·GitGuardian 통과. #14를 먼저 머지해야 diff가 U7·U8만 남는다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #14 독립 검토(차단 1·중요 4·경미 3) 반영과 feature/ade 병합 (`orca/ade-screens-u5-u8`)
검토 `~/Documents/elsecrew-ade-screens-u5-u8-2026-11-26/REVIEW-pr14.md`(대상 dfad3793). 먼저 `origin/feature/ade`(PR #11·#13 포함)를 병합했다. history.md·styles.css 끝에 덧붙인 충돌이라 양쪽을 모두 남겼고, `m5:check:history`에서 누락 0이다.

- **B1 (차단) 설치 동의**: main의 설치 IPC가 `start`를 받으면 다음 순서로 처리한다.
  - sidecar에서 동의를 기다리는 plan을 직접 읽어, 공급자·planId가 맞지 않으면 거절한다.
  - main의 기본 대화상자에 쓰기 위치를 적어 묻는다. '설치·업데이트'일 때만 시작을 보내고, 취소하면 plan이 그대로 기다린다.
  - 시험: 보안 하네스 "installing starts only after main's own box: a page with a valid plan cannot give consent".
- **I1 재로그인 오안내**: 요청 오류(전송·차단·거절·응답 이상)는 `unreachable`(다시 확인만)로 묶는다. 카드는 `null` 대신 고정 진단 코드를 넘긴다. 일반 실패는 두 에이전트 모두 다시 확인이 주 행동이고, 다시 로그인은 대안이다. 시험: 단위 "PR #14 I1", U5 매핑 시험 갱신.
- **I2 배지**: 새 실행이 시작되면 그 작업의 이전 질문을 해결로 바꾼다. 50줄로 줄일 때 열린 요청은 남긴다(최대 200). 시간이 잘못된 저장 줄은 복원하지 않는다. 시험: 단위 "PR #14 I2"(실제 피드 함수로 질문 → 60줄 추가 → 새 실행).
- **I3 이름 조회**: 쪽마다 반영하고, 필요한 이름을 다 찾으면 멈춘다(Map). 시험: "PR #14 I3·minor 1".
- **I4 이동**: 다른 프로젝트 알림은 전환과 같은 한 동작으로 연다. 막히면 이유를 오류 안내로 보이고, 열리지 않으면 대기를 지운다. 읽음은 실제로 열렸을 때만 한다. 시험: "PR #14 I4".
- **경미**
  - 1) 저장 데이터 방어: Map, 잘못된 시각, 이름 `<bdi>`와 80자 상한.
  - 2) 바로가기 문구를 '팀 역할 열기'로 바꾸고 읽기용 이름에 대상을 넣었다. 프로젝트 팀을 읽기 전에는 내놓지 않는다.
  - 3) 안내의 확인 버튼을 카드의 '연결 상태 다시 확인'과 같은 것으로 하고, 안내가 보이는 동안 카드 버튼을 숨긴다. 본문 없는 알림은 종류를 한 번만 보인다.
  - 시험: "PR #14 minor 2·3".
- **검증**: `npm test` 969/969, build, 렌더러 스모크 다크·라이트(30화면 실패 0).

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — 화면 통일 U5(에이전트 연결·F11 안내)·U6(알림 패널) (`orca/ade-screens-u5-u8`, 기준 feature/ade e7965d97)
설계 DESIGN 4.1 U5·U6, 목업 08·12. 범위가 커서 U5+U6(이 PR)과 U7+U8(다음 PR)로 나눈다. 현행 설명은 architecture "에이전트 연결·알림 패널".

- **U5**: `login-guide.ts`(실패 7종 → 어조·주 행동·대안), `recovery-guide.tsx`(F11 공용 틀, U7에서도 씀), 연결 카드(배지·'연결하기/연결 끊기'·'연결 상태 다시 확인'·실패 시 안내), 설치 동의를 확인 대화상자로(첫 초점 취소·주 행동 하나), 역할 바꾸기 바로가기(팀 역할 화면 열기).
  - 옛 `.m5-provider-login`·`.m5-connection-heading`·`.m5-provider-install button` 규칙(주 행동 색을 덮던 것)을 지우고 U5 구역을 새로 썼다.
- **U6**: `notification-groups.ts`(`needsAttention`·`attentionCount`·`groupNotifications`), 패널에 '확인 필요/전체' 탭·프로젝트 이름·작업 묶음·해결된 줄 접기, 종 배지는 확인 요청만 셈.
- **시험**
  - 새 `tests/m5-ade-screens-u5-u6.test.mts`(10)
    - 실패 7종 매핑과 다시 로그인 금지 종류, 원문 키는 접힌 정보에만, 버튼 문구 규칙
    - 설치 동의(첫 초점·주 행동, 안내는 준비만), 역할 바꾸기
    - 배지 셈, 묶음·접기·최신순, 패널 구조, ko/en 키
  - 렌더러 스모크 캡처·대비 지점 3곳 추가(28 로그인 안내, 29 설치 동의, 30 알림 패널 → 30곳). 알림은 다시 불러오기 전에 `localStorage`로 두 프로젝트 6줄을 넣어 배지 2·탭·묶음·접기를 확인한다.
- **검증**
  - `npm test` 957/957, `pnpm build`, `pnpm m5:test:editor` 69/69
  - 렌더 대비 다크·라이트 각 30화면 1208개 실패 0
  - 미리보기 UI 스모크 다크·라이트 각 2회 연속 PASS
  - 캡처 `~/Documents/elsecrew-ade-screens-u5-u8-2026-11-26/u5-u6/`(actual, compare)
- **다음**: U7(오류·복구·확인·빈 상태 문구와 금지어 검사)·U8(죽은 CSS, electron 색 2곳, 업데이트 배너)을 이 PR 뒤 다음 PR로 한다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #11 재확인 3차 B2: 명령 행은 요약만 저장 (`orca/ade-activity-detail`)
재확인 `~/Documents/elsecrew-ade-activity-detail-2026-11-26/REVIEW-pr11-3.md`: 허용 토큰 방식에서도 짧은 붙임값(`-pabc`), `--pin` 뒤 숫자, 리디렉션 뒤 비밀번호, 파일·호스트·스킴 모양 비밀, 하위 명령 자리의 임의 단어가 남았고, 따옴표 안 공백이 있는 대입으로 시작하는 여러 줄 명령에서 비밀 일부를 프로그램으로 골랐다. 지시(조정자): 저장할 명령 정보를 프로그램 이름·고정 하위 명령·가린 인수 수로 좁힌다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — PR #15 독립 검토(차단 1·중요 3·경미 2) 반영 (`orca/ade-latency`)
검토서 `~/Documents/elsecrew-ade-v-2026-11-26/REVIEW-pr15.md`(대상 60e4e0d1). 무결성(projection 손상 검출·승인 전체 재검사·D1 정리)은 유지 판정.

- **B1 거짓 통과 → G6에서 뺌 + 판정 엄격화**: 조정자 지시("빨리 확실하게 못 하면 빼라")대로 `m5:test:latency`를 g6-release·하네스 시험에서 빼고 `pnpm m5:measure:latency`(진단)로 남겼다. fake 공급자에는 승인 큐가 없어 G6 근거로 부족하다. 그래도 `--check`는 `scripts/m5-latency-judge.mjs`로 바꿔 단계별 종료 코드 0·엔진 시작 1회(재시작 없음)·둘째 단계 봉인 시작·시드 완료·요청 표본 수·**표본마다 부하 유지**(실행 중 = 표본 앞뒤로 run이 running, 뒤 검사 중 = 앞뒤로 복사본 검사 중)·클릭 1회를 모두 확인한 뒤에만 목표치를 본다. 시험 `tests/m5-latency-judge.test.mts`(검토서의 거짓 통과 보고서가 FAIL, 조건 하나씩 빠질 때 FAIL).
- **I1 구간 매칭**: 같은 메서드 시간 창 매칭을 없앴다. 라운드마다 sidecar가 본 같은 메서드 요청 전체(화면 폴링 포함)의 대기·처리를 집계한다(`round.engine`). 파일 열기 시계는 행 찾기(폴더 펼침·스크롤) 뒤, 클릭을 보내기 직전에 시작한다(첫 재측정에서 시계를 탐색 앞에 둬 봉인 시작 p95 588ms가 나왔던 것을 바로잡음). 승인 처리 시간은 fake에서 0.018ms로, 승인 경로(5초 Git 재사용) 개선은 측정으로 입증하지 못했다고 문서·PR에 명시.
- **I2 계측**: `latency_log.rs`가 채널로 쓰기 스레드에 넘긴다(루프는 파일 I/O를 기다리지 않음, 가득 차면 버리고 `dropped`). 요청 ID·action은 쓰지 않고 메서드는 프로토콜 목록에 있는 이름만(그 밖 `other`). 요청 복제 제거. 시험 `timing_log_writes_only_known_names_and_numbers_off_the_loop`, `a_full_timing_queue_drops_records_instead_of_waiting`.
- **I3 큰 스트림**: `#acceptChunk`가 청크마다 누적 문자열 전체를 다시 인코딩하던 것을, 그 청크만 인코딩하고 바이트 수·SHA-256(node `createHash`)을 순차 갱신하게 바꿨다(`TextStreamV1.received/digest`). 시험 `task-workspace streams of many chunks settle, each chunk encoded once`(40청크 ~1.9MiB 파일·diff 20쌍, 수정 전 코드에서 실패 확인).
- **M2**: 작업 공간 경로의 해시 불일치·선언 크기 초과·빈 파일(등록 없음) 시험 `task-workspace streams still refuse a wrong hash, and an empty file opens no stream`.
- **M1(코드 변경 없음)**: 5초 안 Git 상태만 바뀌면 보기에서 버튼이 잠시 살아 보일 수 있으나 수락·거절은 전체 검사로 거절되고 카드에 오류가 난다. native owner가 필요한 시험이라 이번에 넣지 않았다.
- **재측정(엄격 판정 PASS, 작업 41·실행 120, 조건별 12회, p50/p95 ms)**: 파일 열기 쉬는 중 66/142·실행 중 67/90·봉인 시작 66/234·뒤 검사 중 69/149, 승인 목록 1.4/19.8, getChat 16~25/≤120. 종료 0/0, 엔진 시작 1/1(둘째 봉인), 부하 유지 12/12·12/12, 계측 버림 0. 증거 `~/Documents/elsecrew-ade-latency-2026-11-26/review-round/`.
- **검증**: `npm test` 953/953, `pnpm build`, 하네스 시험 9/9, `cargo test -p elsecrew-sidecar --test stdio_process` 16, lib(latency_log·projection) 4, `git diff --check`.
- **feature/ade(e641cffc, PR #11) 병합 뒤**: `npm test` 959/959, `pnpm build`, `cargo test -p elsecrew-sidecar --locked` lib 923·stdio_process 16 통과. 엄격 판정 재측정은 병합 전 코드에서 했다. CI(run 45816202040) macOS·Windows·website·GitGuardian 통과. history 539줄 → 머지된 PR #11·#12 엔트리를 아카이브로 옮김(보존 검사 누락 0).
- **ade-stability와 겹치는 곳**: D1 보강(한도 초과 시 그 요청만 실패)은 이번에 바꾼 `electron/sidecar-client.ts`의 `TextStreamV1`(필드 `received`/`digest`로 바뀜)·`#acceptChunk`·`#registerStream`과 `tests/m5-electron-bridge.test.mts`의 작업 공간 스트림 시험 근처다. D2(관측 줄 128KiB)는 겹치지 않는다.

### 2026-11-26 · by Claude Opus 5.5 (claude-opus-5-5) — 응답 지연 조사·수정: 파일 열기·승인 목록·getChat (`orca/ade-latency`, 기준 feature/ade 7378332a)
증상: 주인님 "파일을 열어 보려니까 한참 뒤에 열린다", 11-25 실측 approval list 7초·getChat 10초(권한 카드가 내용을 보이기 전에 사라짐, 통합 검증 D3). 증거·표: `~/Documents/elsecrew-ade-latency-2026-11-26/`.

- **원인 1 — 엔진 재시작(통합 검증 D1)**: main(`electron/sidecar-client.ts`)이 `workspace.file.read`·`workspace.diff.getFile` 응답의 스트림을 등록하고 `blob.chunk`만 정리했다. 작업 공간 청크(`workspace.blob.chunk`)는 정리하지 않아, 작업 파일·diff를 16번쯤 열면 다음 읽기가 `sidecar active stream bound exceeded` → `protocolInvalid` → 엔진 재시작(저장소 확인 포함 약 10초)이 됐다. 재시작은 실행 중인 팀 작업도 끊었다. 7~10초 규모 지연의 정체다. 조정자 결정으로 이 PR에 포함(D1, ade-stability는 하지 않음).
- **원인 2 — projection 전체 파싱**: `Storage::projection_record`가 레코드 하나를 찾을 때도 네 표 전 행 JSON을 파싱했다. 100ms 루프의 감시 검증과 요청마다 여러 번 불렸다(다른 워커의 쉬는 개발 앱 sidecar 표본: 메인 루프 시간의 52%). 실제 데이터 복사본(프로젝트 19·채팅 40·실행 305, projection 2.5MB)에서 조회 1건 28.7ms → 행 기억 뒤 2.3ms.
- **원인 3 — 승인 목록·보기**: 750ms마다 오는 목록과 새 요청 보기가 매번 Git 검사 전체와 projection 조회 묶음을 했다(Codex 경로는 목록 한 번에 두 번).
- **수정**: (1) `workspace.blob.chunk`도 `#acceptChunk`. (2) `ProjectionMemo`: 매 조회마다 모든 행을 읽고 바이트·sequence를 비교해 바뀐 행만 다시 파싱(결과·손상 검출 = 전체 파싱, 변경 토큰 불신). (3) 승인 list/inspect는 `registration_identity_with(viewing)`로 스트리밍과 같은 5초 Git 재사용, 수락·거절은 전체 검사 유지. (4) 계측 `ELSECREW_SIDECAR_TIMING_LOG`, fake 시나리오 `paced-long-stream-read-only`, fake 모드의 봉인 인자 수용(디버그·QA 빌드). (5) `pnpm m5:test:latency`(g6-release, CI 제외).
- **수정 전후(개발 앱, 임시 프로필, fake 공급자, 작업 41·실행 120 시드, 각 12회, ms, p50/p95)**:
  - 쉬는 중: 파일 열기 89/219 → 67/147, getChat 60/99 → 16/47
  - 실행 중: 파일 열기 88/**33,211** → 66/89, 승인 목록 6/**8,895** → 2/29, getChat 58/**16,027** → 18/30 (전: 엔진 재시작 1회, 실행이 끊겨 표본 12개 중 1개만 실행 중 / 후: 재시작 0, 12/12 실행 중)
  - 봉인 시작 쉬는 중: 파일 열기 89/247 → 67/204, getChat 82/153 → 17/121
  - 뒤 검사 중: 파일 열기 89/**18,178** → 67/68, getChat 52/73 → 22/38
  - sidecar 처리 시간 p50: getChat 47.7 → 14.7, 파일 읽기 67 → 42. 남은 파일 읽기 40ms는 git 작업 공간 확인(symbolic-ref·merge-base 프로세스)이다.
- **검증**: `npm test` 920/920, `pnpm build`, `m5:test:security` 96/96, 새 시험(브리지 40회 작업 공간 읽기 — 수정 없이 실패 확인, projection 기억 = 전체 파싱·바뀐 행만 파싱·손상 검출·롤백 반영), `cargo test -p elsecrew-sidecar --locked` lib 916·stdio_process 16 통과(실패 0). 실제 데이터 복사본 조회 벤치 28.7→2.3ms. 측정기 `--check` 수정본 PASS(전부 목표 안).
- **최신 feature/ade(e7965d97, PR #12·#13) 병합 뒤**: `npm test` 948/948, `pnpm build`, `pnpm m5:test:latency` PASS(작업 21·실행 40 시드, 조건별 10회: 파일 열기 p95 쉬는 중 143·실행 중 67·봉인 시작 198·뒤 검사 중 69ms, 승인 목록 p95 27ms, getChat p95 ≤86ms, 재시작 0).
- **위험 경계(독립 검토 대상)**: projection 기억은 저장소 검증 경계다(전 행 비교 유지로 손상 검출 동일). 승인 보기의 Git 재사용은 권한 경계다(보기만, 답은 전체 검사). fake 모드 봉인 인자는 디버그·QA 빌드의 fixture 경로뿐.
- **함정(재조사 방지)**: 측정기에서 다리로 `readWorkspaceFile`을 직접 부르면 스트림을 화면이 소비하지 않아도 main이 정리하지만(수정 후), 수정 전에는 그 자체가 재시작을 일으킨다. 파일 열기는 화면 트리 클릭으로 잰다. 트리는 가상화돼 보이지 않는 행이 DOM에 없고, Monaco는 공백을 NBSP로 그린다. fake 실행은 60초면 끝난다. 저장소는 EXCLUSIVE라 다른 연결이 쓸 수 없다. 대기 루프에 `pgrep -f '...rustc'`를 쓰면 자기 명령줄과 일치해 영원히 기다린다(`pgrep -x`).
- **PR**: https://github.com/wonseok-han/elsecrew/pull/15 (`--base feature/ade`). CI(run 45809641599) macOS·Windows·website·GitGuardian 모두 통과.
- **남은 일**: 실제 Claude 권한 요청으로 D3(카드가 '불러오는 중…' 뒤 2~4초에 사라짐)가 줄었는지 확인(실계정 필요, fake 공급자에는 승인 큐가 없어 승인 경로 처리 시간은 0으로 나옴). 파일 읽기의 git 확인 재사용 검토. Codex 검토 대응.
## 프로젝트 요약 (고정)
터미널을 한 번도 안 열어본 비개발자를 **코딩 에이전트(Claude Code·Codex)가 굴러가는 상태까지** 데려다주는 데스크톱 GUI 앱. 현재 배포 경로는 Tauri 2 + React 19이고, M5는 Electron + React/TypeScript + Monaco와 Rust sidecar의 별도 작업대 골격으로 전환 중이다.
- 기획 원본: `~/Library/CloudStorage/OneDrive-개인/ObsidianVault/오픈소스/아이디어/후보/에이전트-스타터.md`
- 레포: https://github.com/wonseok-han/hello-agent (로컬 폴더도 `~/projects/hello-agent`로 변경 완료 — 2026-08-12 확인)
- 위저드 6단계: 에이전트 선택 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식
- 주요 모듈: 기존 Tauri `src-tauri/src/`, 공용 Rust `crates/elsecrew-{core,protocol,sidecar}`, M5 셸 `electron/`과 작업대 `src/m5/`
## 핵심 결정·발견 (고정 — 재조사 방지)
- **여러 프로젝트 동시 작업의 경계(2026-11-26, multi-project PR-A)**
  - 화면은 한 프로젝트만 보여 주고 실행은 sidecar에서 계속된다. 프로젝트를 떠나도 실행을 멈추지 않는다(`canLeaveProjectView`는 오는 중인 요청만, 팀이 일하는지는 `isTeamWorking`).
  - 쓰기 차단은 실제 쓰기 경로 단위다(`work_scope.rs` `write_refusal` + `project/write_scope.rs`, PR #18 검토 B1). 작업 ID가 아니라 경로로 본다. 같은 저장소 git 폴더이거나 폴더가 서로 안에 있으면 같은 잠금이고, 경로를 못 구하면 fail closed다. 근거는 두 가지다. 에이전트는 자기 작업 공간에만 쓰고, 앱의 git 쓰기는 단일 dispatcher 스레드에서 차례로 일어난다. 그래서 git 쓰기를 dispatcher 밖(작업자 스레드)으로 옮기면 이 근거가 깨지니 이 차단을 다시 봐야 한다. 사본을 사용자 폴더에 넣는 동작만 전역이다. 작업에 묶을 수 없는 진행 중인 일은 전역 차단으로 돌아간다. 작업 공간과 겹치는 프로젝트·작업 공간 폴더는 등록에서 거절한다.
  - 초안 보관소(`editor-drafts.ts`)는 이미 보관한 것을 지우지 않는다(PR #18 검토 B2·재확인). 저장은 전부 아니면 무변경이고, 속성 시험이 이를 지킨다. 새 초안이 못 들어가면 그 초안을 보관하지 않고 알린다. 예전의 "오래된 작업부터 지움" 규칙으로 되돌리지 않는다.
  - 폴더 겹침 검사는 그 폴더에 무엇이든 쓰기 전 첫 단계다(빈 폴더·일반 폴더 초기화, 새 작업 폴더). `.git/commondir` 저장소는 등록하지 않는다.
  - 공급자 동시 실행(PR-B): CLI는 같은 설정 폴더의 프로세스끼리 토큰 갱신을 스스로 잠근다(Claude 2.1.328 `.oauth_refresh.lock`, Codex 0.167.2 `auth.lock`, 문자열 근거). sidecar가 직접 하는 Claude 갱신은 그 잠금을 잡지 않으므로, 실행이 살아 있을 때는 하지 않아야 한다.
- **실제 앱 시험의 함정(2026-11-26, ADE V)**
  - 앱 전용 로그인은 프로필 안에 있다(Codex `…/m5/codex-profile-v1/auth.json`, Claude는 설정 폴더 경로 해시로 구분된 Keychain 항목). 그래서 임시 `--user-data-dir`로는 실제 실행이 안 된다.
  - 개발 앱 창이 다른 창에 완전히 가려지면 macOS Electron이 `hide`를 보내 미리보기가 숨고, rAF도 멈춘다. CDP 측정 중 "뷰가 안 보임"은 이것부터 의심한다(창을 앞으로 가져오면 다시 보인다).
  - main 디버거(`--inspect=<port>`)에서는 `require.cache[".../.vite/build/main.cjs"].exports.previewManager`로 미리보기 상태(`currentLayout`·`visibility`·`refusedMeasurements`)를 읽을 수 있다.
  - 큰 도구 결과(이미지)가 Claude 출력 한 줄 128KiB를 넘으면 실행이 멈췄다(D2). 이제 세션 모드에서 8MiB까지 읽고 줄여 관측한다. **승인에 쓰는 값은 원문만**: Claude 권한 요청 줄은 줄이지 않고(허용 응답이 입력을 되돌려 보냄), Codex는 메서드별로 명시한 폐기 필드만 줄인다(`item/started`의 `command`·`cwd`가 승인 보완·완료 대조에 쓰임). 큰 줄은 프로세스 전체 2자리로 동시 처리 메모리를 묶는다(`provider_login_io.rs` `LARGE_LINE_SLOTS`). 자리 보유는 총 30초·무진행 10초, 대기는 90초가 상한이며 넘으면 그 연결만 실패한다(`LargeLineLimits`).
  - 동시 스트림 16개를 넘으면 엔진이 재시작됐다(D1). 누수는 PR #15, 한도 초과를 그 요청만의 실패로 바꾼 것은 ade-stability.

- **색은 토큰 블록 두 곳에만(2026-11-26, 화면 통일 U0·U1)**: `styles.css`의 다크 `:root`와 라이트 `:root[data-theme="light"]` 밖에는 hex·`rgb()`를 쓰지 않는다. 화면 코드에도 색 리터럴을 쓰지 않는다(Monaco는 `theme.ts`가 토큰에서 읽음). `tests/m5-style-tokens.test.mts`가 대비(다크·라이트 각 95쌍)·블록 밖 색 0·정의되지 않은 토큰 0을 막는다. 라이트 값은 1단 `--p-*`만 바꾼다. 새 화면은 공용 컴포넌트 클래스(`.m5-btn-*`·`.m5-badge`·`.m5-banner`·`.m5-card`·`.m5-group`·`.m5-empty-state`·`.m5-steps`·`.m5-recover`)를 쓴다.
  - 화면 밝기 기록의 기준은 main의 `userData/appearance.json`이다(PR #10 검토 I1로 바꿈). 렌더러 `localStorage`(`elsecrew.theme.v1`)는 첫 페인트용 사본이다. 렌더러는 시작 때 `m5Appearance.get()`으로 기록을 받아 따르고, main에 읽을 수 있는 기록이 없을 때만 올바른 사본으로 복구한다(손상 사본은 보내지 않음). 기록은 `.tmp` + rename으로 바꾸며 실패는 `saved: false`로 화면에 알린다. 시스템 밝기 변경은 `nativeTheme.updated`가 창 배경을, `prefers-color-scheme`이 CSS·Monaco를 바꾼다.
  - 대비는 토큰 쌍만으로 부족하다(검토 I4): 컴포넌트 조합(`scripts/m5-contrast-check.mjs` 조합 표)과 렌더된 DOM(`tests/m5-rendered-contrast-probe.cjs`, 렌더러 스모크 다크·라이트)을 함께 본다. 새 화면·컴포넌트를 넣으면 렌더러 스모크 캡처 지점에서 자동으로 측정되므로, 그 화면이 스모크 경로에 없으면 캡처 지점을 추가한다. `color-mix`·필터로 만든 면은 조합 표에 넣는다(필터 대신 토큰 권장).
  - 앱 문구 규칙(주인님, 2026-11-26): 버튼은 동작만 말한다. 행·카드·제목이 보여 주는 대상을 반복하지 않고('Codex 연결하기' → '연결하기'), 대상은 접근성 이름에 남긴다. 설계 `docs/proposals/m5-ade-screens-plan.md` 2.1의 10번.
- **홈·처음 시작·설정의 규칙(2026-11-26, 화면 통일 U2~U4)**: 현행 설명은 architecture "홈·처음 시작·설정(U2~U4)".
  - **되돌리기 어려운 쓰기의 동의는 main이 직접 받는다**: 폴더 기록(PR #12), 에이전트 설치(PR #14 B1), 업데이트 재시작(PR #16)은 화면의 요청만으로 실행하지 않고 main의 기본 대화상자에서 '예'일 때만 실행한다. 화면 안 확인 창은 검토 단계일 뿐이다.
  - **폴더 확인(빈 폴더·일반 폴더·기록 없음)은 main의 네이티브 대화상자로 남긴다**(PR #12 검토 B1). 앱 화면 안으로 옮기면 메인 렌더러의 응답이 곧 되돌릴 수 없는 쓰기(`.git` 초기화·앱 내 기록)의 허가가 되어, 실제 사용자 확인의 근거가 없다. 디자인 일관성보다 이 경계가 우선이다. 앱 안 모양으로 바꾸려면 렌더러가 결과를 위조할 수 없는 main 소유 경로와 그 증명 시험이 먼저 필요하다.
  - 첫 작업의 권한은 확인 체크 후 보낼 때만 저장한다. 첫 작업 창에 별도 저장 버튼을 두지 않는다(PR #12 검토 I1).
  - `scripts/m5-project-ui-smoke.cjs`의 기본 흐름은 첫 작업 뒤 단계(옛 diff·반영 버튼)가 L1 레이아웃 이전 화면 기준이라 멈춘다(이 PR 이전부터). 프로젝트·첫 작업 회귀는 `ELSECREW_SMOKE_PROJECTS=1` 시나리오로 본다.
  - 프로젝트를 연 시각은 `localStorage` `elsecrew.m5.projectOpenedAt.v1`(편의 기록), 알림 설정은 `elsecrew.m5.notifications.prefs.v1`이다. 꺼진 알림 종류는 저장도 배너도 안 한다. 해결된 알림은 해결 후 하루 지난 것을 앱 시작 때·한 시간마다·설정을 바꿀 때 지운다.
  - 첫 작업의 별도 확인 체크(Q9)는 유지한다. 보내기 버튼이 동의를 대신하게 바꾸지 않는다(`tests/m5-ade-screens-u2-u4.test.mts`가 막음).
  - 화면 캡처는 `pnpm m5:capture`(렌더러 스모크 캡처 지점 = 렌더 대비 측정 지점). 새 화면을 만들면 스모크에 캡처 지점을 더한다.
- **팀 역할 편집의 규칙(2026-11-26, custom roles R0·R1·R2·R4)**: 화면 `role-settings.tsx`, 순수 함수 `team-draft.ts`(insert/move/duplicate/restore/teamChanges/uniqueRoleName/newMemberId), 템플릿 `role-templates.ts`(TS에만). 현행 설명은 architecture "팀 역할 편집".
  - 목록 순서는 보기 순서다. 실행 순서는 총괄이 정한다(Q1). 새 팀원 ID는 무작위 `member-xxxxxx`이고 지운 기본 역할은 원래 ID로 되살린다.
  - 이름 규칙(서식 문자 Cf·총괄 이름·중복)은 **저장 요청에만** 적용하고 저장된 기록 읽기에는 적용하지 않는다. 이름은 화면에 보이는 대로 비교한다(NFC, 시드 이름 그대로인 기본 역할의 번역 이름 포함). 번역 이름을 바꾸면 `G5_LEGACY_ROLE_LABELS_V3`(Rust·TS)와 공용 표도 바꾼다. Rust·TS가 `tests/fixtures/m5/role-names-v3.json` 한 표를 함께 읽는다. 새 규칙을 더할 때 이 표에 줄을 넣는다.
  - 총괄이 팀에 없는 역할로 위임하면 한 번 바로잡는다. "총괄 단계의 instruction이 비어 있지 않음 = 바로잡기 뒤"라는 표시로 쓰므로, 총괄 단계에 다른 지시를 넣게 되면 이 판단을 바꿔야 한다(`orchestration.rs` `finish`).
  - 함정: 프로젝트 설정을 저장하면 `RoleSettings`가 revision 키로 다시 마운트돼 컴포넌트 안 상태(안내 문구 등)가 사라진다. 저장 뒤에도 보여야 할 안내는 작업대(`workbench.tsx`)가 들고 있어야 한다.
  - 함정: sidecar 통합 시험(`orchestration_dispatch`)의 fake 에이전트는 `target/debug/elsecrew-sidecar` 실행 파일을 띄운다. `cargo test --lib`만 돌리면 "fake agent control launcher failed: No such file"로 15개가 실패하니 `cargo build -p elsecrew-sidecar`를 먼저 한다.

- **결과 미리보기의 DNS 미리 찾기(2026-11-26)**: Electron 43에서 `<link rel=dns-prefetch>`는 CSP·`webRequest`·고정 프록시를 모두 거치지 않고 실제 DNS 질의를 보낸다. 미리보기가 켜지면(macOS 기본값, Windows·Linux는 `ELSECREW_PREVIEW=1`일 때만, `=0`으로 끔) `--blink-settings=dnsPrefetchingEnabled=false`(앱 전체)를 유지할 것. 미리보기 뷰는 앱 페이지 위에 그려지므로, 미리보기 영역을 덮는 새 화면 요소는 `previewObscured` 목록에 넣거나 DOM 변화로 나타나야 한다(탭이 그리기 전에 붙잡고 뷰를 숨긴 뒤 보임, PR #9 I1). CSS `:hover`만으로 뜨는 덮개와 `pointer-events: none`인 덮개는 감지되지 않는다. 앱 확대·축소에서 좌표는 main이 `getZoomFactor()`로 바꾸고, 렌더러 측정의 `devicePixelRatio`·페이지 크기가 맞지 않으면 숨긴다. 창 크기·확대를 바꾸는 새 경로는 반드시 그 전에 `previewManager.geometryChanged()`를 부를 것(보기 메뉴 확대는 `electron/zoom-menu.ts`). 숨김·표시 순서는 `src/m5/preview-layout-loop.ts` 한 곳에서만 정한다. 확인은 `pnpm m5:test:preview-isolation`의 net log 판정이며 Electron을 올릴 때 다시 돌린다. 미리보기 토큰은 renderer에 주지 않고, 작업 공간 주소는 sidecar가 채팅 바인딩에서 꺼낸다(renderer 값 불사용).

- **작업 레이아웃의 규칙은 순수 함수에 있다(2026-11-26, ADE 1차-나)**: 창 폭 구간·오른쪽 폭·편집기 요청 해석·기본 탭·미리보기 겹침은 `task-layout-model.ts`, 목록 묶음은 `task-list-model.ts`, 단계 칸·카드 결합은 `task-story-model.ts`, 역할 색은 `role-colors.ts`. 화면 컴포넌트는 그 값을 그리기만 한다. 합치기·반영·버리기 버튼은 결과 상자(`result-box.tsx`) 한 곳에만 둔다 — 다른 곳은 "결과 상자에서 넣기" 링크로 가리킨다.
  - 측정 함정: CDP `Emulation.setDeviceMetricsOverride`로 폭을 바꾸면 CSS는 즉시 바뀌지만, 창이 가려져 있으면 프레임이 없어 `resize`·`matchMedia` 처리기가 다음 캡처 때 돈다. 재기 전에 `Page.captureScreenshot`으로 프레임을 한두 번 그리게 한다.
  - 소스 문자열 검사는 `tests/m5-workbench-source.mts`(작업대의 나뉜 파일 목록)를 읽는다. 블록을 새 파일로 옮기면 그 목록에 파일을 더한다.
- **ADE 1차-나가 지켜야 할 1차-가 규칙(2026-11-26)**: 판단 로직은 `task-status.ts`(목록 묶음 `taskGroup`, 결과 막대 `resultBarView`, 변경 줄 `changesStatus`, `standingRead`)·`settings-draft.ts`·`markdown*.ts`·`approval-display.ts`·`merge-confirmation.ts`·`last-session.ts`에 있고 `workbench.tsx`는 값만 연결한다. 위치를 옮길 때도 지킬 것:
  - 열린 작업의 합침·반영 상태 읽기는 결과 막대와 별개로 작업대가 한다(작업·변경 목록·"다시 확인"마다). 막대가 숨어도(답변 완료) 읽기는 계속돼야 한다.
  - 합치기 확인은 연 순간의 head·base tip을 고정해 그대로 보내고, 이후 읽기에서 바뀌면 확인을 닫는다. 사본 반영 확인도 열린 동안 반영 내역이 바뀌면 닫는다. 반영 확인 요청은 파일 수가 돌아올 때까지 기다린다.
  - 권한 허용은 그 요청의 전체 내용을 이 카드에서 펼친 뒤에만 켜진다(`canAcceptApproval`). 철회 안내 수명은 대화(타임라인)가 관리한다.
  - 작업 전환 중이거나 상태를 아직 못 읽었으면 자리표시, 읽기에 실패하면 "확인하지 못했어요 · 다시 확인"(옛 값을 보이지 않음).

- **작업 상태는 하나만 읽는다(2026-11-25, ADE 1차-가)**: 작업 목록·결과 막대·변경 패널·실행 카드·대화 머리말은 `src/m5/task-status.ts`의 `taskPhase`(초안 → 진행 → 확인 필요 → 검토 대기 → 합침 / 반영 / 반영·합친 뒤 또 바뀜 / 중단 / 버림)만 읽는다. 새 표시를 만들 때 `chat.state`나 merge/apply 값을 직접 해석하지 않는다.
  - merge/apply 읽은 값은 작업 ID로 꼬리표를 달고, 팀이 일하는 동안 비운다. 예전 결함("진행 중인데 main에 합쳐짐")은 화면 전역 `mergeInfo`가 이전 작업 값을 들고 있던 탓이었다.
  - 열려 있지 않은 작업의 합침/반영은 `task-disposition.ts`가 작업 버전(`updatedAt`)별로 한 번 묻고 뷰어별 localStorage에 둔다. 다시 실행된 작업은 새로 묻는다.
  - 상태 색 CSS 변수 이름은 `--status-running/attention/review/done/danger`(1차-나 토큰과 공유).
- **설정의 "저장하지 않은 변경" 규칙(2026-11-25)**: `src/m5/settings-draft.ts`. 화면이 스스로 하는 보정(추천 모델 채우기 등)은 불러온 값에도 같이 적용해 미저장으로 세지 않는다. 사람이 누른 "추천 모델 채우기"는 편집이다. 역할 편집 재작성은 이 함수를 그대로 쓴다.
- **보고 렌더러 안전 규칙(2026-11-25)**: `src/m5/markdown.ts`는 HTML을 해석하지 않고, React 요소로만 그린다. 링크는 자격 증명 없는 http(s)/mailto만, `href` 없이 그리며 누르면 main(`m5:external-link:v1`)이 주소를 다시 검사하고 네이티브 확인 창을 띄운 뒤에만 연다. 이미지는 설명 글자만 보인다.
- **권한 요청 화면과 sidecar 지연(2026-11-25 실측)**: 검증 단계 실행 중 approval `list` 7초, `getChat` 10초가 걸렸다. Claude CLI는 권한 요청을 3초 안에 스스로 거둬들이므로, 카드가 내용을 읽기 전에 요청이 사라진다(진단 보고서의 "누르기 전에 사라짐"의 원인). 화면은 이제 "거둬들였어요"로 남기고 알림도 해소하지만, sidecar 응답 지연 자체는 미조사다. **2026-11-26 원인 확인(응답 지연 작업)**: 7~10초 규모의 대기는 엔진 재시작이었다. 작업 공간 파일·diff를 16번 열면 main이 `workspace.blob.chunk` 스트림을 지우지 않아 17번째에 `sidecar active stream bound exceeded` → `protocolInvalid` → 재시작(저장소 확인 포함 약 10초)이 났다(통합 검증 D1, `sidecar-client.ts`에서 수정). 그 밖에 승인 목록·보기가 매번 Git 검사 전체와 projection 전체 파싱을 두 번씩 했다(viewing은 5초 안 Git 검사 재사용으로 수정). 측정은 `pnpm m5:measure:latency`(진단, G6 아님 — PR #15 검토 B1). 실제 Claude 권한 요청으로 D3가 줄었는지는 실계정 확인이 남았다.
- **개발 앱 시험 함정(2026-11-25)**: `workbench.tsx`에서 컴포넌트가 아닌 것을 export하면 Fast Refresh가 깨져 저장할 때마다 화면 전체가 다시 로딩된다. 실행 중에 다시 로딩되면 팀 작업은 sidecar에서 계속되고, 그 실행이 끝날 때까지 프로젝트 열기가 잠긴다. 셸에서 `cd`는 scm_breeze 때문에 실패해도 다음 명령이 실행된다 — 시험 폴더를 만들 때 `cd` 대신 절대경로와 `git -C`를 쓴다(이번에 시험용 `git init`·커밋이 작업 브랜치에 잘못 들어가 고쳤다).

- **Codex CLI가 바뀌면 사전 확인이 먼저 깨진다(2026-11-25)**: ChatGPT.app 업데이트가 내장 codex-cli를 바꾼다. 앱의 사전 확인(`codex_preflight/exchange.rs`, `named_config.rs`, `codex_login/connection.rs`)은 응답을 엄격하게 비교한다(`deny_unknown_fields`, null 기본값 목록). 그래서 새 필드 하나만 생겨도 "연결을 확인하지 못했어요"(`providerLoginFailed`)가 난다.
  - 진단 순서:
    1. 앱 로그의 `Codex preflight failed at <단계>`로 실패 단계를 찾는다.
    2. `codex app-server generate-ts --out <임시>`로 스키마를 본다.
    3. 실제 직렬화는 인증 없이 임시 CODEX_HOME + 같은 `-c` 설정으로 `config/read`를 호출해 확인한다.
  - 실제 자격 증명 복사본으로는 재현하지 않는다(refresh token 회전 위험).
- **키체인 허용 창과 partition list(2026-11-26, 11-25 서명 판단 정정)**: macOS 키체인 항목은 허용 앱 목록(ACL)과 별개로 partition list를 검사한다. 항목을 만든·허용받은 코드가 Apple Team ID로 서명됐으면 `teamid:`, `/usr/bin/security` 같은 Apple 도구는 `apple-tool:`, **자체 서명·ad-hoc 코드는 `cdhash:`(바이너리 해시)**로 묶인다.
  - 그래서 "Elsecrew Dev" 자체 서명(지정 요구사항이 고정)은 소용이 없었다. 다시 빌드할 때마다 cdhash가 바뀌어 partition 검사에서 다시 묻는다. 11-25의 "한 번 허용하면 다시 묻지 않는다"는 지정 요구사항만 본 잘못된 판단이었고, `m5-dev-sign-sidecar.mjs`는 11-26에 제거했다.
  - 해결: 사이드카는 앱 전용 Claude 로그인을 읽기·쓰기 모두 `/usr/bin/security`로 한다. 쓰기는 `security -i`에 stdin으로 `add-generic-password -U -a … -s … -X <hex>` 한 줄을 넘긴다(Claude Code 자신과 같은 방식). 비밀 값은 argv·로그·파일에 남지 않는다.
  - `security -i`의 한 줄 상한은 개행 포함 4095자(macOS 26 실측)다. 넘으면 남은 부분이 별도 명령으로 실행되고 "unknown command" 오류에 그 내용이 찍힌다. 그래서 4032자로 미리 막는다. native 대체는 없고, 1400바이트 이상인 로그인은 교환 전에 갱신을 거절한다(검토 반영).
  - `find-generic-password -w`는 출력할 수 없는 바이트(비ASCII UTF-8 등)가 있으면 값 전체를 hex로 낸다. 저장은 ASCII JSON으로 하고 읽기는 hex를 푼다.
  - Keychain 호출은 30초 제한 자식 프로세스이고, 코어 덤프를 막고, 죽인 뒤 거둔다. 취소는 토큰 교환 전까지만 받는다. 쓰기 전후로 항목 신원(`cdat`)과 값을 확인한다. 확인과 쓰기 사이 몇 밀리초의 외부 변경은 막을 수 없다(한계).
  - `security create-keychain`으로 만든 임시 키체인은 버전 256이라 partition list가 생기지 않는다. partition 동작은 로그인 키체인에서만 확인된다.
  - `Elsecrew Safe Storage`(Electron safeStorage)는 바꾸지 않는다. 같은 Electron 버전의 Electron.app은 워크스페이스·경로가 달라도 cdhash가 같고(74cc2c91…, 43.4.0), 경로가 다른 워크트리에서도 창 없이 읽혔다. Electron 버전을 올릴 때만 한 번 묻는다.
  - Keychain 항목은 지우지 않는다.
- **에이전트는 실제 브라우저·Node를 실행할 수 없다(2026-11-23 확인)**: 실행 샌드박스(`provider_runtime_sandbox.rs` `POLICY`, `deny default`)는 `/System`·`/usr`·작업 공간·프로필만 읽게 하고 로컬 소켓을 막는다. 그래서 Chrome·Firefox·WebKit(Swift)·Homebrew Node가 모두 실패하고, 에이전트는 macOS `jsc`와 가짜 DOM으로만 웹 앱을 확인한다. 웹 앱 결과의 실제 동작은 사람이(또는 시험자가 별도 Chrome으로) 확인해야 한다. 이 경계를 넓히는 것은 안전 결정이다.
- **"요청 내용 보기"는 사라진 요청에 오류를 내지 않는다(2026-11-23)**: Claude CLI는 권한 요청을 스스로 취소할 수 있다(`control_cancel_request`). 대기하지 않는 요청의 inspect는 `inspection: null`, 허용·거절은 `agentApprovalChanged`. approval 결과는 sidecar client·main·preload·화면에서 네 번 검증되므로 계약 검증은 자기 출력을 다시 통과해야 한다(멱등).
- **셸로 fd가 닫혔는지 검사할 때(2026-11-20)**: `/bin/sh`(bash)는 내장 명령의 리디렉션(`printf … >&N`)을 처리하며 원래 fd를 10번 이상 첫 빈 번호에 보관한다. 그래서 닫힌 10번이 쓰기 가능처럼 보인다. 검사는 외부 명령(`/usr/bin/printf`)으로 한다. 병렬 테스트에서 낮은 번호가 차 있을 때만 드러난다.
- **projection 조회는 계약상 전체를 검증한다(2026-11-20 확인)**: `Storage::projection_record(종류, id)`는 하나를 찾을 때도 projects·chats·runs·locks 전 행의 JSON을 파싱해, 다른 테이블의 손상도 오류로 낸다(`selected_projection_retains_only_requested_rows_without_skipping_validation`). 그래서 반복 호출은 비싸다. 성능 문제는 호출 쪽에서 한 번 읽은 값을 재사용해 푼다(예: `validate_actual_run_projection`). **2026-11-26 갱신**: `Storage`가 행 바이트를 비교해 바뀐 행만 다시 파싱하는 `ProjectionMemo`를 갖는다(결과·손상 검출은 전체 파싱과 같음, 시험 `remembered_projection_rows_equal_a_full_parse_and_parse_only_changed_rows`). 행 읽기·비교 비용은 남는다. 새 조회 경로도 `load_projection_memo`를 쓸 것.
- **2026-11-20 앱 시작 검증 결정(사용자)**: 처음 열 때의 긴 확인은 받아들이지만 두 번째·세 번째 열 때도 오래 걸리면 안 된다. 그래서 (1) 검증 범위를 유지한 채 시작 검증을 빠르게 하고, (2) 정상 종료 때 Keychain 비밀 키로 서명한 "검증 완료 지점"을 남겨 다음 시작에는 서명이 맞으면 그 뒤의 기록만 검증한다(서명 없음·불일치·비정상 종료 뒤에는 전체 검증). 이는 `docs/proposals/m5-storage-reopen-verification.md`의 "이전 프로세스의 검증 결과를 믿지 않는다"를 "서명된 정상 종료 지점은 믿는다"로 바꾸는 사용자 승인이다. Keychain 키까지 가진 공격자는 조작한 DB를 통과시킬 수 있다는 대가를 설명했다. 독립 검토 대상.
- **2026-11-20 후속 결정(사용자)**: 전체 검증을 일정 간격마다 시작 때 하지 않고 앱이 쉬는 동안 뒤에서 한다(`background_check.rs`, 복사본 검사, 통과 시각을 봉인이 이어 받음). 7일은 쉬는 시간이 없었을 때의 대체 기준이다. 같은 날 반영 화면은 반영 전/반영됨(버튼 없음)/반영 뒤 또 바뀜(다시 반영) 3상태로 정했고, 사본 작업은 엔진의 반영 영수증(`apply-receipts-v1/`), git 작업은 `mergedBefore`로 판단한다. 영수증에는 실제로 쓰거나 지운 경로만 넣는다(건너뛴 경로를 넣으면 거짓 "반영됨", 검토 I9).
- **2026-11-19 일반 폴더 = 링크 파일 방식(사용자 선택)**: git이 아닌 폴더는 확인 뒤 앱 데이터 `shadow-git-v1/<uuid>.git`에 기록을 만들고, 폴더에는 `.git` 링크 파일 하나만 둔다. 이후 worktree·단계 커밋·합치기·에이전트 git은 git 프로젝트와 같은 코드 경로다(`ProjectManager::repository_git_path`). 기록 소유 표시(`elsecrew-folder`의 ino 또는 경로)로 복사·이동된 폴더를 구분하고, 다른 폴더·다른 Mac의 기록은 경고 뒤 새로 만든다. 상한 20,000파일·1GiB, 안에 다른 저장소가 있으면 거절한다. 게시 뒤에는 그림자 기록을 지우지 않는다. 에이전트 git은 `task_git_environment`(개발자 폴더·전역 설정 끔)가 있어야 동작한다.
- **2026-11-18 작업 공간 위치·커밋 결정**: worktree는 `~/elsecrew/workspaces/<프로젝트>/<작업 이름>`이 기본이고 설정에서 바꿀 수 있다. 커밋은 앱만 하며 작성자는 `Elsecrew`로 고정한다. LFS 등 필터 저장소는 처음엔 미지원이다. 상세는 `docs/proposals/m5-real-worktrees.md`.
- **2026-11-18 폴더 프로젝트·파일 탭 결정**: git이 아닌 그냥 폴더도 프로젝트로 받는다(필수). 방식은 원본 바로 수정 + 작업 시작 전 자동 백업이다(사용자 선택). "이 작업 되돌리기"를 제공하고, 같은 폴더의 작업은 한 번에 하나씩 돈다. 반영 단계는 없다. 백업은 그림자 git으로 한다(git dir은 앱 데이터, 작업 폴더는 사용자 폴더, 작업 시작 커밋은 기준점, 단계마다 자동 커밋, 작업마다 브랜치). 그래서 모든 프로젝트가 git 기반 같은 코드 경로를 쓴다(사용자 확인). 파일 탭은 열린 작업의 worktree(폴더 프로젝트는 그 폴더) 파일을 보여 주고, 보고 있는 작업 이름과 git 상태 색을 표시한다(Orca 파일 탐색기와 같음).
- **2026-11-18 worktree·합치기 결정**: 작업 하나 = worktree 하나 = 브랜치 하나(작업 안 브랜치 전환 없음). 합치기 전에 내 폴더 최신을 작업 브랜치로 먼저 가져오고, 충돌은 작업 공간에서 팀이 정리한 뒤 합친다(유일한 방식). 파일 통째 최신 우선은 이 전환 때 대체한다.
- **2026-11-18 Orca 방향 전환(사용자 결정)**: 에이전트 실행·워크트리·변경 관리는 Orca를 벤치마킹한다(`docs/proposals/m5-orca-benchmark.md`). 작업 공간은 사용자 저장소의 실제 git worktree·브랜치, 기본 권한은 "전부 허용", 로그인은 앱 전용 유지. 허용된 도구(웹 검색 등)는 끄지 않고, 모르는 이벤트로 실행을 멈추지 않는 방향이다. 기존 "감독 못 하는 도구는 끈다"(144f5601, eb05b6af)는 임시 조치였다.
- **에이전트 CLI 내부 흔적은 원본 반영에서 제외(2026-11-17)**: Claude Code는 작업 복사본에 `.claude/.cc-writes`(0700)를 만든다. 반영 검토(`review.rs` `AGENT_SCRATCH`)는 이 경로를 통째로 빼고, 원본이 그 안의 파일을 추적하면 거절한다. 다른 CLI 폴더를 추가할 때는 CLI만 쓰고 에이전트는 쓸 수 없는 경로인지부터 확인한다. Codex 0.169는 browser/computer use·image generation이 기본으로 켜져 있어 실행 인자로 끈다(`UNSUPERVISED_TOOL_FEATURES`). 새 Codex 버전마다 `codex-profile-v1/logs_2.sqlite`의 feedback_tags `feature.*`를 다시 점검한다.
- **dev 앱 자동 조작(2026-11-17)**: 폴더 선택 창은 Orca로 뒤에서 조작할 수 없다. 개발 빌드에서는 `~/Library/Application Support/Elsecrew/dev-pick-directory.txt`에 경로를 적으면 다음 '프로젝트 추가'가 그 폴더를 고른다(한 번만, 빈 폴더 확인도 생략, 패키지 앱은 무시). 엔진 watcher는 `file.changed`만 보내고 `diff.changed`는 보내지 않는다. 화면이 목록을 갱신하려면 `file.changed`를 받아야 한다. 번역 파일을 바꾸면 dev HMR이 깨지므로 Cmd+R로 다시 불러온다.

- **프로세스 증거는 '살아 있을 수 있는 것'만 기록한다(2026-11-17)**: 실행 기록(DB)은 root와 아직 끝난 것으로 관측되지 않은 신원만 저장한다(`TreeTracker::recorded_identities`, `LoginProcess::recorded_for_ledger`). 끝난 프로세스는 다시 돌 수 없으므로 빼도 안전하다. 로그인 잠금 파일은 추가 전용 그대로이고 상한은 65,536개·4MiB다(`HOLD_MAX_BYTES`). 소유권 검사는 '이전 기록 ⊆ 누적 관측'이다. 엔진이 사라진 뒤의 판정(시작 시 정리·명시 복구·잠금 해제)은 `SettledProcTable`을 쓴다. 같은 pid를 시작 시각이 다른 프로세스가 쓰면 기록된 프로세스는 끝난 것이다(살아 있는 두 프로세스는 pid를 공유하지 않는다). 실행 중인 추적은 계속 이를 무결성 실패로 본다. 앱 데이터 사본을 엔진으로 열 때는 살아 있는 이전 엔진이 있으면 정리를 건너뛴다(`previous_engine_still_runs`).
- **종료는 중단이지 초기화가 아니다(2026-11-17 사용자 결정)**: 중단된 실행은 다음 시작에서 작업 공간을 버리지 않고 canceled로 끝낸다(`settle_interrupted_run`). 남은 에이전트 프로세스는 기록된 신원으로 직접 끝내고, 소멸 확인과 작업 공간 일치가 증명돼야 한다. 예전 unresolved 복구 기록도 같은 증명이 서면 푼다. abandon(작업 공간 포기)은 증명이 안 되는 경우와 삭제에만 남는다. 편집기는 VS Code처럼 저장하지 않은 내용을 종료 뒤에도 보관했다가 되살린다(`editor-drafts.ts`). 같은 run의 lock을 다시 쓸 때는 새 operation id가 필요하다(같은 id에 다른 내용이면 Conflict).
- **앱 종료는 막지 않는다(2026-11-17 사용자 결정)**: 종료하면 실행 중인 작업을 멈추고 최대 10초 기다린 뒤 묻지 않고 끈다(`quit-coordinator.ts`). 다음 시작에서 기록된 프로세스가 모두 사라졌고 작업 공간과 기록이 정상인 실행은 자동으로 canceled가 된다(`interrupted_run_upserts`). 불확실한 경우만 사용자 복구로 남긴다. 사용자 복구(abandon)는 채팅을 discarded로 만들므로 자동 정리에 쓰지 않는다.
- **작업 내역 채널과 이전 빌드(2026-11-17)**: 실행 출력의 `activity` 채널은 이전 빌드가 모르는 값이다. 작업 내역이 한 줄이라도 저장된 앱 데이터를 이전 빌드로 열면 출력 재검증에서 거절돼 영구 admission 실패가 된다(schema 버전은 같다). 개발 빌드끼리 같은 실제 데이터를 오가지 않는다. Claude 실행은 결과에 적힌 거절이 모두 앱이 거절한 tool_use일 때만 정상 종료로 본다.
  - **2026-11-26 변경(주인님 피드백, PR #11 검토 반영)**: 작업 내역은 명령 줄 자체(비밀 가림)·실행 위치·종료 코드(공급자가 밝힌 경우만)·걸린 시간·바뀐 줄 수를 남긴다. "도구 입력 원문은 공개하지 않는다"는 옛 규칙은 명령에 한해 폐지됐다. **명령 출력·도구 결과·파일 내용은 저장하지 않는다**(이름 없는 값은 가릴 수 없음). 형식·규칙은 architecture "실행 출력" 절.
  - 저장은 `record_activity`의 `sanitize` 한 곳만 거친다. 새 필드를 더하면 `sanitize`의 허용 목록과 화면 `KEYS`에 같이 넣는다(빠지면 조용히 버려진다). id는 앱 키(`c1`, `t1`)만 쓴다.
  - **명령 행은 요약만(2026-11-26 검토 3차)**: 명령 줄의 글자는 저장하지 않는다. 프로그램 이름(첫 토큰 규칙), 도구별 고정 목록의 하위 명령 하나, 가린 단어 수만 남긴다(Rust `summarize_command` ↔ TS `summarizeCommand`, 공용 `summary` 사례). 2차의 "허용 토큰만 남기기"도 짧은 붙임값·PIN·경로·호스트 모양 비밀이 남아 폐기했다. 보이는 정보를 늘리려면 고정 목록에 단어를 더하는 쪽으로만 바꾼다. 파일 경로는 명령 줄에서 뽑지 않는다.
  - 비밀 가림 규칙은 Rust `secret_mask.rs`와 TS `secret-shape.ts` `maskSecrets` 두 곳이다(이제 산문용). 규칙을 바꾸면 둘 다 고치고 `tests/fixtures/m5/secret-mask-cases.json`에 사례를 넣는다(양쪽이 같은 결과를 요구, 다시 가려도 같아야 함). 가림은 언제나 자르기보다 먼저다.
  - 함정: 저장소에 GitGuardian PR 검사가 있다. 테스트의 가짜 비밀은 처음부터 `FAKE`/`example`이 들어간 명백한 합성값으로 쓴다(`PGPASSWORD=s3cret`, `--password hunter2`, jwt.io 예시 JWT는 걸렸다). `curl -u 사용자:비밀번호`, JWT, `--token=값`·`--secret 값` 모양은 값이 가짜여도 형태로 걸리므로 공용 사례처럼 `{colon}`/`{dot}`/`{eq}`/`{sp}`(줄바꿈은 `{nl}`) 자리표시로 저장하고 시험에서 푼다. 이후 커밋으로 지워도 PR의 앞 커밋이 계속 걸리므로 커밋을 다시 만들어야 한다.
  - 함정: Write 도구는 내용 속 `\uXXXX`를 실제 문자로 바꿔 쓴다. 정규식·문자열에 보이지 않는 문자(U+2028은 JS 정규식 리터럴을 깨뜨림)가 들어가므로, TS 소스에 이스케이프가 필요하면 파이썬 등으로 `\` + `u…`를 이어 붙여 쓰거나 `String.fromCharCode`를 쓴다. Rust의 `\u{…}`는 영향 없다.
  - 함정: Claude CLI의 `tool_use_result`는 성공한 Bash면 `{stdout, stderr, interrupted}` 객체지만 실패하면 `"Error: Exit code N\n…"` 문자열이다. 성공에는 종료 코드가 없다. Write로 새 파일을 만들면 structuredPatch가 빈 배열이고 `type:"create"`·`content`로 줄 수를 센다.
  - 함정: activity 행은 append할 수 없다(`complete`만). 끝난 결과는 같은 `id`의 두 번째 행으로 쓰고 화면이 합친다.
- **실행에는 전체 시간 제한이 없다(2026-11-17 사용자 결정)**: Claude 세션(`SessionChannel`)과 Codex 턴(`TextSession`)의 10분 고정 마감을 없앴다. 도구 실행과 사용자 승인 대기도 시간 제한 없이 기다린다. 남은 상한은 연결 준비 시간(Claude 15초, Codex 30초), 사용자 취소, 앱 종료 정리, 요청·이벤트 수와 바이트 예산이다. `provider_model_proxy`의 600초는 제품에서 쓰지 않는 진단 후보라 그대로 두었다. 실행 시간 상한을 다시 넣으려면 사용자 결정이 필요하다.
- **정책 밖 도구 요청은 그 요청만 거절한다(2026-11-16)**: Claude 도구 요청이 역할의 경계(작업 공간 안·존재하는 파일·`.git` 등 제외·읽기 전용은 쓰기/명령 불가) 밖이면, `deliver_approval`이 사용자 확인 없이 그 요청만 deny하고 실행은 계속된다. 세션·프로세스·역할 불일치만 실행을 끝낸다. 에이전트용 거절 문구는 `claude_approval::DENIAL_GUIDANCE` 하나다.
- **중단된 정리 작업은 다음 시작에서 이어 간다(2026-11-16)**: materialization의 'attempting'은 arm만 커밋된 상태이고, batch 쓰기와 'pending' 복귀는 한 트랜잭션이다. 그래서 새 엔진의 admission에서는 'pending'으로 되돌린다(`release_interrupted_attempt`). 'failed'만 계속 거부한다. 엔진이 앱 데이터를 열지 못하면 `~/Library/Logs/Elsecrew/last-engine-admission-failure.json`의 `reason`(kind)로 확인한다(멈춘 원인 기록 `last-engine-failure.json`을 덮지 않는다). `Storage::open`(Legacy·옛 스키마 이전 v6/v7)은 여전히 attempting을 거부한다. 자세한 위치는 `ELSECREW_DEBUG_ADMISSION=1`로 엔진을 띄워 복사본에서 재현한다. Rust 실행 기록에 필드를 추가하면 TS의 실행 상태 이벤트 검증기(`protocol-v1.ts`)도 함께 고친다. 빠뜨리면 이벤트 하나가 엔진 연결을 끊는다.
- **엔진은 요청 하나·이벤트 하나 때문에 죽지 않는다(2026-11-16)**: 요청 처리 오류는 그 요청의 오류 응답이다(무결성 계열만 종료). 화면은 이벤트를 받으면 처리 결과와 관계없이 ack한다. 사용 중 멈춘 엔진은 Electron이 다시 띄우고, 화면은 저장 안 된 편집이 없을 때만 스스로 새로 고친다. 원인 진단은 `~/Library/Logs/Elsecrew/last-engine-failure.json`. 개발 중 테스트 빌드는 `target/debug/elsecrew-sidecar`를 교체하므로, 개발 앱은 빌드 뒤 다시 켜야 새 코드를 쓴다.
- **장치 번호는 재부팅 뒤 바뀐다(2026-11-16)**: macOS의 `st_dev`는 마운트할 때 정해진다. 재시작을 넘겨 저장·비교하는 신원에는 inode(와 내용 digest)를 쓰고 장치 번호는 일관성만 본다. 로그인 잠금 결합과 원본 반영 쪽 기록(영수증·journal·archive·정리 job)은 고쳤다. 기록에는 0을 쓰고, 저장된 값과 비교할 때는 `without_device`로 장치 원소를 무시한다. 앱 데이터 조사는 DB를 임시 폴더로 복사해 읽는다(실행 중인 앱의 DB는 잠겨 있다).
- **로그인 잠금 해제의 증거(2026-11-16)**: 해제는 '결합된 실행이 원장에서 끝났음'을 증명해야 하며 프로세스 소멸만으로는 풀지 않는다(기존 불변식 유지). 작업 공간 증거는 manifest 또는 원장의 삭제 완료 기록 중 하나다.
- **작업마다 독립 복사본(2026-11-16)**: 새 작업 복사본은 프로젝트의 현재 복사본 지정을 바꾸지 않는다(`persist_task_copy_active`, 활성화와 한 번에 기록). 작업에 속한 복구는 새 복사본 생성을 막지 않지만, 반영 잠금(`applicationId`)과 진행 중인 `application_intent`는 막는다. 공급자 로그인 잠금은 실행에 묶이며, 해제 증명이 그 작업 공간의 manifest를 읽는다. 그래서 잠금이 남은 작업 공간 파일은 지우면 안 된다(`finish_deletions_keeping`).
- **삭제는 삭제 표지 + 파일 삭제(2026-11-13)**: 원장은 append-only이고 재생 검증을 하므로 이벤트 행은 지우지 않는다(DELETE trigger 6개, 압축 흔적, UNIQUE `canonical_path`). 프로젝트 ID는 더 이상 경로 해시와 같다고 가정하지 않는다(`project_id_by_path`, `fresh_project_id`). 최신 작업의 작업 공간은 프로젝트 현재 복사본과 같은 경우가 많으므로, 그대로 지우면 프로젝트가 깨진다. 설계는 `docs/proposals/m5-project-deletion.md`.
- **설정 구조와 수동 반영 폐지(2026-11-13)**: 프로젝트 단위 수동 반영(`prepareApplicationWorkspace`·`applicationPreview`)은 화면·브리지·sidecar 요청에서 제거됐다. 원본 반영은 작업(채팅) 단위 미리보기·확인 흐름만 쓴다. 복구·진단 도구는 프로젝트 설정의 접힌 '문제 해결'에 있다. 개발 실행은 `target/debug/elsecrew-sidecar`가 필요하다(`cargo build -p elsecrew-sidecar`). `provider_login_gate` descriptor 테스트는 병렬 실행에서 간헐 실패할 수 있다.
- **작업 복사본 검증과 Git 잠금(2026-11-12)**: 실행 중 검증은 `.git/index.lock`을 잡지 않아야 한다(공급자가 자체 git 명령으로 잠금 보유 가능). `attempting` materialization이 남으면 admission이 fail-closed로 거절하며, 복구는 백업 뒤 `pending`으로 되돌려 정밀 재검증을 통과시키는 것이다(2026-11-16부터 admission이 이 되돌림을 스스로 한다 — 위 항목으로 대체). 다른 세션·리뷰어는 이 worktree에서 쓰기 금지, `cd` 대신 `git -C`.
- **공급자 fence 경합(2026-11-12)**: 로그인 확인·모델 목록·실행이 같은 `*-login.pending`을 공유한다. 기존 fence가 live 소유자에게 잠겨 있으면 `providerBusy`(재시도)이고, 잠금 없는 고아만 recovery 증거다. fence는 staging 잠금 후 hard_link로만 게시해 이름이 잠기지 않은 inode를 가리키는 순간이 없다. 사용량 표시는 연결 변경 시 Codex 모델 조회를 즉시 실행하므로 로그인 확인과 동시 실행될 수 있다.
- **2026-11-12 main 통합 이후**: PR #1 병합 b6e2de81 및 GitHub `elsecrew` 이름 변경 완료. 이후 최신 main 기반 기능 브랜치에서 작업하며 추가 main 병합/push는 별도 승인이다. CI43150190399 실패는 제품 완료가 아님; Tauri 빌드 성공과 artifact 업로드 실패를 구분한다.
- **Claude 실패 진단(2026-11-12)**: 새 terminal run.failure는 허용된 단계/원인 코드만 보존하며 기존 실패 원인은 추정하지 않는다. fixture Bash→Read 정상 경로는 통과했고 전달 오류 주입의 재시작 보존도 확인했다. 과거 사용자 실행의 직접 원인과 설치본/실제 공급자 재시험은 별도 미확인이다.

- **채팅 내용 삭제(2026-11-12, 구현·독립 검토 전)**: G5D 검증기가 원천에서 사본을 재유도해 byte/digest를 비교하므로 "표지 행 예외" 대신 모든 사본을 한 transaction에서 같은 결정적 치환으로 다시 쓴다(`storage/chat_erasure.rs`). schema 버전은 그대로이며 지원 버전 목록은 그 파일 한 곳이다. `VACUUM`은 `events` rowid를 바꿀 수 있어 쓰지 않는다. 의도(`pending`) 기록 전 쓰기 없는 dry run이 필수이며, 거절된 `pending`은 철회한다(갇힘 방지). 실행/채팅 payload에 새 최상위 키를 추가하면 `storage/chat_erasure.rs` 분류 목록에도 추가해야 삭제가 거절되지 않는다. 공유 target 디렉터리에서 다른 worktree와 sidecar 산출물이 충돌하므로 병렬 작업 중에는 `--target`으로 분리한다. 계약: `docs/proposals/m5-chat-erasure.md`.

- **사용자 정의 팀(2026-11-12 재확정)**: 기본5역할은 템플릿이다. 총괄은 사용자가 구성한 역할 설명/컨텍스트와 요청의 선행조건으로 배분한다. 고정 순차 pipeline은 제품 방향이 아니며 현재 실행의 고정 role enum은 남은 구현 제약이다.
- **공유 Cargo target 주의(2026-11-12)**: 여러 worktree가 같은 CARGO_TARGET_DIR를 쓰면 `elsecrew-sidecar`/`elsecrew-protocol` artifact hash가 같아 서로의 빌드를 덮어쓸 수 있다(`--target`만으로도 다른 worktree 산출물로 컴파일된 사례 확인). 병렬 worktree 검증은 전용 target dir을 쓴다.
- **사용자 정의 역할 V3·저장 V13(2026-11-12, feature/ade-custom-roles)**: 역할 집합 `schemaVersion:3`은 5키 프로필 배열 + 같은 순서의 `roleDefinitions{roleId,name,responsibility}`이며 총괄(읽기 전용) + 팀원1~7명이다. 이름과 실행 권한은 분리돼 팀원 권한은 자유 선택, stage/채팅 상태/변경 판정은 `permission` 기준(`review`만 표시상 verifying 유지). V1/V2 기록은 고정 ID·권한 그대로 정확히 검증하고 다시 쓰지 않는다. 저장 V13은 V12 답변 작성자 IN 목록을 role ID 형식 검사로 바꾼 것뿐이며 V3 설정은 V13에서만 저장된다. 채팅 내용 영구 삭제 등 다음 schema는 V14로 쌓는다. 상세는 architecture.md “사용자 정의 팀 역할 V3”.

- **Claude 사용량 인증(2026-11-12)**: CLI auth status의 저장 로그인 존재와 OAuth access-token 유효성은 다르다. 만료 시 check가 앱 전용 Keychain의 갱신을 같은 provider pending fence 아래 수행하며 사용량은 갱신 후 다시 읽는다. 단순 인증 존재를 사용량 인증 유효성으로 해석하지 않는다.

- **선택 폴더의 상위 경로 접근(2026-11-12)**: macOS 경로 순회는 ancestor를 검색 전용 O_SEARCH/no-follow handle로 통과하며 선택 leaf만 읽기로 연다. 폴더 선택 동의와 OS 권한, 에이전트 도구 승인은 서로 대체하지 않는다. Downloads/test_todo의 실제 재시도는 미확인이다.
- **2026-11-11 핵심 팀 실제 성공**: Codex→Claude→Codex3회 모두 성공/cleanExit·프로세스 종료 증거 확인, 임시 복사본 hello.txt 변경·원본 보존·재시작 기록 유지. 채팅 제거는 선택 채팅 범위로 검사하되 동일 workspace/unknown lock과 대상 복구 제한 유지. 상세 증거는11월 아카이브. 전체 M5/Windows/출시 완료는 아님.

- 2026-11-11 Claude privacy 등록 거절은 실제 초기 연결 차단점이다. 공식 privacy README가 확인됐으며 모듈을 끄지 않는 정확한 native 관측 후보를 준비했다. 후속 사용자 승인으로 native 생성자를 전환했다. 설치/실제 성공은 최신 기록을 확인하며 정책/증거는 proposals/m5-coding-agent-ide.md privacy 절을 따른다.

- 2026-11-11 모델 설정은 공급자 조회 목록에서 선택한다. 최신 표시가 없는 경우 공급자 기본·추천 모델 사용을 사용자 승인받았다. 저장한 선택은 자동 덮어쓰지 않으며 목록 순서/이름 정렬로 최신을 추정하지 않는다. 조회는 채팅 없이 설정 진입 때 자동 수행하며 세션 캐시를 재사용한다. 원본 프로젝트/실제 prompt를 사용하지 않는다.

- 2026-11-11 사용자 채팅의 상대는 항상 오케스트레이터이며 전송은 단순 ‘보내기’다. 직접 답변 또는 필요한 팀원 위임/결과 회수/최종 정리를 모델이 판단한다. 2026-11-11 임시 실제 Codex→Claude→Codex3회에서 작업 복사본 수정·최종 취합·정상 종료를 확인했다. 원본 반영은 자동 수행하지 않는다. Windows/전체 출시 완료와 구분한다.

- 2026-11-10 총괄 직접 응답은 V2 canonical5 역할 snapshot + 저장 V12가 필요하다. V1 채팅은 변경하지 않으며 새 채팅부터 총괄 실제 실행/후속 전송을 사용한다. V12는 답변 source-index trigger의 역할 허용 목록만 확장하고 metadata/원장 검증·compaction prefix를 함께 적용한다. 이전 앱으로 다운그레이드 불가. 자동 팀 위임은11월11일 구현/검증 진행 중이며 설치본 반영·실제 공급자 증거는 최신 기록을 따른다.

- **2026-11-10 파일 목록 정렬 계약**: 폴더 우선 정렬은 sidecar 생성기와 TS/Rust treePage 검증기·공유 fixture에 함께 적용한다. 불일치는 protocolInvalid로 연결을 끊어 기본값 등 무관 조회도 실패하게 한다. 그룹 순서 외 동일 경로 중복 거부는 유지한다.

- **2026-11-10 설정·정리:** 앱 공통 설정은 언어/기본 역할·모델/에이전트/앱 업데이트로 통합, 프로젝트 역할은 별도다. 더미30개는 활성 목록에서 제거하고 약20MB를 retired-test-projects-20261110-131013에 보관했다. 업데이트 배포는 GitHub Releases+update.electronjs.org로 확정됐으나 서명/실제 배포 미완료로 비활성이다.
- **2026-11-10 제품 요구/경쟁 확인:** Electron 앱 자체 업데이트는 필수이며 네이티브 메뉴·다운로드·안전 종료 후 설치 소스 연결을 구현했다. 서명된 배포 피드 미설정으로 비활성이고 실제 업데이트 검증은 미완료다. Orca 공식 orchestration은 이종 공급자 worker 감독을 문서화하므로 해당 기능 부재를 차별화 근거로 쓰지 않는다. 우리 M5의 사용자 역할별 실행과 제품 AI coordinator 구현은 구분한다. 상세는 M5 제안서 말미.
- **원본/관리폴더 겹침(2026-11-06)**: snapshot 생성은 source 내부 managed root를 준비기록/폴더생성 전에 거절해야 한다. durable Windows 비교는 native verbatim/protocol 혼용을 피하되 원본 삭제 후 standalone view를 위해 원본 재open은 요구하지 않는다. 로컬 회귀/독립 검토를 통과한 별도수정이며 Windows native 추가확인은 남는다.
- **Windows 전체 통합 진단 후보(2026-11-06)**: d1071b95까지 명시 설치Git/임시 프로젝트의 선택·동의·실행·철회와 snapshot→review/token→scheduler 반영/복원→SQLite재열기 opt-in을 연결했다. 테스트 전용 manager Git허용은 제품에 컴파일되지 않으며 독립 검토 차단결함없음. native 실행은 아직 미확인, 새 CI 공개 범위 승인이 필요하다. 이전75 PASS에 포함하지 않는다.
- **Windows 일반 파일 재관측(2026-11-06)**: cap-primitives3.4.6의 maybe_dir는 일반 파일에도 FILE_SHARE_DELETE를 제거한다. retained DELETE handle과 병존하는 writer read-only rebind는 부모 상대 native FILE_OPEN/share7/no-follow 및 열린 객체 종류/신원 검사를 사용한다. mutation 보호 handle은 유지한다. CI42155540808에서 전용 회귀 포함75개 통과; 전체 반영/복원 거래 검증과 구분한다.
- **Windows 생성 폴더 재관측(2026-11-06)**: DELETE-capable creation handle을 유지한 상태의 generic directory reopen은 sharing 위반을 낼 수 있다. 부모handle 상대 read-only FILE_OPEN/share7/no-follow+full identity로 재관측하고 기존 creation pin·lease·flush·receipt 비교는 유지한다. CI42154541724에서 stage/new-directory 회귀 포함74개 진단 통과; 전체 Windows 제품 활성화 근거로 확장하지 않는다.
- **Windows Git 설치 신뢰 정책 승인(2026-11-06)**: 사용자가 선택한 설치 폴더와 OS 검증 서명자를 보여준 뒤 명시 동의하는 방식으로 확정했다. 같은 설치본의 서명 없는 DLL을 수용하되 실행마다 재검사하고 root/서명자 변경은 재동의한다. 버전/파일 digest 고정은 하지 않는다. 이 승인은 Windows 제품 활성화·추가CI/push 승인이 아니다. 후속 Git admission·baseline receipt·review/confirmation·before archive·pre-write intent·취소 가능한 원본 관측은 구현 checkpoint이며 Windows native 검증 전이다. 일반 파일 writer/명시 restore·archive cleanup·제품 scheduler 내부 연결은 구현 checkpoint다. directory/추가 metadata·원본 잔여 파일 처리와 native 검증은 미완료다. 제품 gate는 닫혀 있다. 최신 상세는 현재 워크로그와11월 아카이브에서 확인한다.
- **Windows Git 의존 DLL 서명(2026-11-06)**: [공식 이슈6491](https://github.com/git-for-windows/git/issues/6491)에서 유지보수자는 모든 배포 DLL 서명을 제공하지 않는다고 설명했다. 모든 의존 DLL에 Authenticode를 요구하는 방식은 정상 설치본 호환성을 보장하지 못한다. 주 실행 파일의 검증된 signer 추출을 구현했으나 배포자 정책·설치본/의존성 신뢰·실제 로드 결속은 미완료이고 제품 gate는 유지한다.
- **Windows 저장/복구 native45 PASS(2026-11-06)**: 공개6e079a9/tree77dd9ad3, CI42129091905에서44 native+1 직접 호출 차단 통과. 통합02657f7/9a62344와 같은 소스 수정이다. Win32 상대 rename87은 동기 handle의 NtSetInformationFile class10으로, 이미 동일한 보안 descriptor의 불필요한 재적용은 exact-equal/recheck 경로로 수정했다. 4개 파일/DB 재열기 fixture가 통과했지만 Windows Git admission·실제 프로세스 재시작·제품 활성화·출시는 별도다. 이번 승인3회는 모두 사용했다.
- **Windows Git 제공 확정(2026-11-06)**: 사용자 설치 Git을 사용한다. 앱 포함 MinGit은 채택하지 않았다. 명시 설치 위치·실행 파일/의존성·업데이트 후 재확인과 기존 config/hook/filter 격리가 필요하며 PATH 첫 git.exe 자동 신뢰나 제품 activation 승인이 아니다.
- **Windows replacement 계약 정정(2026-11-05)**: 앞선 expected-ID CAS 부재 blocker는 기존 atomic rename/관측 충돌 검사와 별도 보장을 혼동한 것으로 독립 검토자가 철회했다. macOS workspace save도 leaf CAS가 아니며 hostile same-user 동시 편집의 OS 완전배제 보장이 아니다. Windows save는 경로 결속·전후 ID/내용 검사와 replaceIntent/replaced 기록으로 연결했으나 ACL·namespace durability·cleanup/복구/native가 미완료라 제품 admission은 계속 닫혀 있다. 원본 apply/restore에 새 승인을 추가하거나 보존 계약을 약화하지 않는다.
- **Windows native34 PASS(2026-11-05 최신)**: CI41988529910/0a7efd5에서 lease/image/GUID 실행·공용 reader·종료 supervisor·spawn/watcher34개 통과. 첫41987756587의1FAIL은 fixture의 별도 directory handle 수명으로 수정했다. 통합 수정2812429. 현재 batch3회 중2회 사용·1회 남음. 실제 Git 설치본/저장·복구·background watcher 종단 실행/제품G6 합격은 아니다.
- **Windows save guard 충돌(2026-11-05)**: 기존 DirectoryLease에서 교체 대상 자체를 guard로 잡으면 의도한 replace도 막힌다. 마지막에 pin을 풀어 해결하지 않는다. 별도 guard의 durable intent/ID/cleanup과 handle-relative replace·durability/복구를 묶어야 하며 제안서 §12에 조건을 기록했다. Windows 저장·cleanup·명시 복구 및 재열기 fixture는 구현됐으나 native 검증/제품 admission은 차단 상태다.
- **Watcher 현재 구조(2026-11-05)**: Unix/Windows 모두 한 background 검증 job과 한 bounded scan slice를 순환한다. 완료 뒤 현재 stream/active/root/.git 결속과 metadata/tree 재검증을 유지한다. Windows 연결은 native 미검증·실제 Git 차단 상태이고 과거 동기 Windows 설명보다 이 구현 상태를 우선한다. 실측 UI 지연 PASS는 별도다.
- **Windows 내부 Git 제공 방식(2026-11-06 확정)**: 사용자 설치 Git을 사용한다. MinGit 번들은 채택하지 않았다. 선택한 설치 폴더·OS 검증 서명자를 표시하고 명시 동의하며 매 실행 재검사한다. Codex/Claude 설치 정책과 별개이고 Windows 제품 실행은 아직 비활성이다. 제안·출처는 `docs/proposals/m5-coding-agent-ide.md` §12.
- **Windows 자손 supervisor(2026-11-05, scoped native PASS)**: a7c0674의 상시 Unsupported 봉쇄 뒤 creator-thread DEBUG_PROCESS 추적을 구현했다. 생성 이벤트를 계속하기 전 handle 복제·EXIT 계속 후 모든 signal·root signal·누적 Job 수 일치·active0이 완료 조건이다. 중첩 debugger 누락/상한/오류는 실패이며 Job 밖의 외부 broker는 보장 범위가 아니다. 3a02b76/CI41980701769에서 기존 실제 child signal 회귀와 native20·spawn1·watcher4 합계25개가 PASS했다. 전체 제품 PASS가 아니며 Windows admission은 닫혀 있다.
- **Windows Job 종료 판정 반증(2026-11-05)**: 실제 CI41977646539에서 parent signal+Job active0/terminate(true) 뒤에도 별도 보유 자손 handle은 WAIT_TIMEOUT이었다. 기존 completed()/terminate()를 모든 자손 종료 증거로 인용하지 않는다. 임의 지연·카운트 완화로 해결하지 않으며 native gate는 닫아 둔다. 동일 실행의 disk capacity 회귀는 PASS, 전체16PASS1FAIL·watcher skipped다.
- **Windows CWD lease 초안(2026-11-05, native 미검증)**: 모든 경로 성분과 기존 직접 child를 delete 공유 없이 보유해 nonempty를 유지하고 전체 ID/부모 결속을 검증하는 기반을 구현했다. 일반적인 공유 잠금만으로 속성 변경을 막는다는 주장은 하지 않는다. name-surrogate와 directory-bit tag 구분·실행 파일 권한·GUID CWD native 호환이 남아 있다. typed Family/Transport 연결과 armed pin의 성공/실패 소유권은 구현했으나 native 미검증이며 실제 Git/제품 admission에는 연결하지 않았다.
- **Windows CWD 공유 잠금의 한계(2026-11-05)**: deny-share-write/delete만으로 reparse 불변을 증명하지 않는다. attribute 접근은 공유 제한과 별개이며 MS-FSA는 FILE_WRITE_ATTRIBUTES로 reparse 설정을 허용한다. CWD admission은 미완료이고 Windows gate는 유지한다.
- **Windows 프로세스 연결 조건(2026-11-05)**: Job 생성 시 결속과 제한 시간/취소 가능한 stdio transport 기반은 scoped native 검증을 통과했으나 실제 Git에는 아직 미연결이다. stdio가 없으면 STARTF_USESTDHANDLES를 켜지 않으며, 지정 시 임시 복제 핸들+HANDLE_LIST만 전달한다. 공통 spawn 잠금의 경합 회귀는 통과했다. CWD 권한·Git 호출 연결과 제품 native 묶음 검증 전에는 Windows gate를 열지 않는다. Drop의 Job close는 종료 요청이며 부모 signal과 active-process 0만으로는 전체 종료 증거가 되지 않는다(11/05 native 반증 참조).
- **기존 원본 경로 재확인 (2026-11-02)**: 과거 경로 철자는 source_identity로도 복원할 수 없다. native 폴더 선택으로만 `native_path_binding`을 보강하며 catalog reopen/원본 접근은 없는 표식을 자동 생성하지 않는다. 재선택은 기존 snapshot과 복구 상태를 보존한다.
- **2026-11-02 Windows 경로 fixture**: host 절대경로와 canonical wire 경로는 구분한다. 실행 41394071830에서 core 226·protocol 65·sidecar 313 PASS, 마지막 fixture 1개는 수정 후 focused 실행 41399404281에서 PASS했다. 최신 전체 suite 단일 재실행/Windows 제품 합격은 아니다. 원장 테스트에 native 역슬래시 경로를 넣지 않으며 제품 검증·실제 공급자 차단은 유지한다.
- **Windows 컴파일과 제품 준비 구분 (2026-11-02)**: macOS native owner를 cfg로 분리해도 Windows G4/stdio는 DB·ready 전에 unsupportedPlatform으로 차단된다. 공통 영수증 검증은 플랫폼 독립으로 유지한다. Windows CI를 녹색으로 만들려고 패키지 smoke의 ready 조건을 낮추지 않으며, Windows의 실제 안전 경로 구현·검증·활성화가 별도로 남는다.
- **Windows 체크아웃 바이트 (2026-11-02)**: 첫 원격 CI에서 core.autocrlf 변환으로 golden 해시·SQL manifest·JSONL 동일 바이트 검사가 실패했다. `.gitattributes`는 소스 LF, `tests/fixtures/** -text`로 의도된 원본 바이트를 보존한다. 테스트의 호스트 경로와 macOS 모의 inventory를 구분한다. 로컬 수정 PASS를 Windows 실행 증거로 대체하지 않는다.
- **main 지연 귀속 주의(2026-11-02 eleventh)**: Chromium284ms와 달리 live/history/generation Node timer는42ms 이하다. retained 보조 진단638ms는 trace 종료/보고까지 포함한다. 측정기 종료 비용 가설을 먼저 최소 시험으로 분리하고, 다른 시계/측정의 값을 직접 동일시하거나 기존FAIL을 차감하지 않는다. 출력 재파싱 최적화만으로200ms/400MiB 문제가 해결되지는 않았다.
- **현재 인계(2026-11-02, Undo 수정)**: 저장 내용으로 되돌려도 dirty가 남던 결함을 비동기 파일버전 비교로 수정했다. 원문 추가 보관 없이 경로 대기열·단일 digest·정확한 탭/세대 검사를 사용한다. controller62/작업복사본10/TypeScript/독립 검토 PASS, 2faed62 실제 QA에서도 Undo 복원을 확인했다. 단일 combined 시험은 입력798.1ms와 main trace 불완전으로 FAIL이다. 이후 main 수집기는 제한된 스레드별 전체 집계로 수정하고19개 검사·독립 검토를 통과했으나 eighth 실제 실행에서도 수집 완결성을 확인했다. 다만 main294.538ms·idle448.703MiB 실패이며 입력19.9ms는 단일 관측이다. 이 trial 메모리만 기준 내이며 반복 합격은 아니다. 관련 시험 앱/프로필은 종료 확인 후 모두 삭제했다. 실제 모델 호출 승인 잔여 없음. 유휴400MiB/G6/Windows/출시는 계속 미완료다. 최신 세부 워크로그는 [11월](archive/history-2026-11.md)을 먼저 확인한다.
- **누적 Node 검사 최신 증거(2026-11-02)**: 깨끗한803b937의 npm test635 PASS/실패·스킵0 및 src TypeScript PASS. 이전11개 focused 수정 이후 전체 재검사 공백 해소. Rust/패키지/G6/Windows/출시까지 합격한 결과는 아니다. 원문은 `.artifacts/m5-regression-2026-11-02/consolidated-node/`.
- **통신 버퍼 보유 해소(2026-11-02)**: 완결stdout 뒤 빈 view와 잘린stderr tail의 큰 backing allocation 보유를 제거했다. bridge64회귀·독립 검토PASS. 큰I/O 후 잔류 문제 수정이며cold idle400MiB 달성 증거가 아니다.
- **초기 메모리 조사(2026-11-02)**: 유휴400MiB 기준 유지. 같은 정적 화면도 큰 비용이며 software rendering/spellcheck/ready-to-show는 개선 근거 없음. 레이어5개에 명백한 불필요 승격 없음. 손실 없는 memory-infra 통계에서 renderer tile pool72.4375MiB 중 free55.0625MiB 확인(할당 통계≠RSS 절감량). 초기 paint를 DOM mount 뒤로 늦춘 대조도 RSS 개선이 없어 기각했다. Blink 타일 예산32MiB로 실제 할당을 줄여도 RSS가 개선되지 않아 이 후보도 제품에 미적용이다. 상세·증거는 최신 워크로그.
- **2026-11-02 사용자 확정 — 메모리 기준 유지**: macOS 유휴400MiB와 추가 최적화를 선택했다.512MiB 상향은 승인되지 않았다. 부하800MiB·잔류증분120MiB 및 전체 검증 요건도 유지한다.
- **2026-11-02 통합 QA 메모리 정정**: second/third combined 결과의 idle phase에는 프로젝트 준비 비용이 섞였다. 해당 idle/잔류증분은 순수 유휴/회수 근거가 아니다. 생성 프로세스 분리와 준비 phase 분리는 별개 수정이며 마지막 phase 수정 후 재측정 필요. 일반 제품10/30 RSS 실패는 유지한다.
- **2026-11-02 실제 공급자 상태 갱신**: 0843105 패키지에서 Codex 자체 파일 수정 succeeded·cleanExit 및 실행 중 취소 canceled·cleanExit를 확인했다. 둘 다 processesProvenGone=true이고 원본 불변이다. 앞선 Claude 자체 수정·취소 성공과 함께 기본 실제 실행 증거를 확보했다. 이전 checking UI 정체는 이번에 재현되지 않았으나 원인 미확정이며 전체 복구/G6/Windows/출시 합격과 구분한다. 아래10/30의 미완료 파일 수정·취소 항목은 이 결과로 갱신한다.
- **2026-10-30 실제 공급자 최신 상태**: Claude 자체 파일수정·취소 성공. Codex는 da52055의 text_elements 생략 호환 수정 후에도 수정/중단 시험이 `turn stream item started`에서 failed·cleanExit로 끝났고 중단 클릭에는 도달하지 못했다. 원본/복사본 불변, 승인된 최신2회 소진. 단계명은 Electron 로그 폴더 last-provider-checkpoints.json에 민감 원문 없이 보존된다. 후속 승인 진단1회는 user echo shape 거절로 좁혀졌고 소진됐다. 공통 lifecycle timestamp 수정2c06e79 후 재시험1회에서 실제 응답은 표시됐으나 다음 moderation metadata 알림에서 failed·cleanExit다. 승인 소진이며 82f7c77 후속 시험에서 짧은 응답은 succeeded·cleanExit로 통과했다. 파일 수정도 복사본에서 이뤄졌으나 diff 알림에서 failed·cleanExit여서 취소는 미실행이다. 실패 상태가 UI에 반영되지 않은 별도 결함도 조사 중이다. 추가 모델 호출은 새 승인 대상이다. 증거 .artifacts/m5-codex-two-2026-10-30, 상세 과거 기록은 10월 아카이브. G6/Windows/출시는 미완료다.
- **2026-10-29 일반 discard 연결**: X34law FakeExact actual adapter의 sidecar–작업대 합동 PASS와 독립 검토 뒤 임시 gate를 제거했다. 실제 공급자/일반 설치 패키지/전체G6 합격은 별도다. 기존 gated 체크포인트 기록은 당시 상태로 보존한다.
- **2026-10-29 앱 기본 역할 경계**: 앱 기본값은 프로젝트 role_settings 최초 초기화 때만 복사한다. 기존 프로젝트·chat roleSnapshots에는 소급하지 않으며, 기본값 파일이 손상돼도 이미 초기화한 프로젝트는 독립적으로 사용한다. 실제 모델 가용성은 계속 실행 전 별도 검증한다.
- **2026-10-29 일반 discard 완료 재개**: 응답 유실 후 dialog가 재생성돼도 inspect가 exact discard_receipt를 돌려주어 중복 쓰기 없이 재개한다. workspace watcher 제거만으로 project watcher는 종료되지 않으므로 완료 후 releaseView 응답을 확인한 뒤 화면을 비운다. 현재 기능은 gated checkpoint이며 아직 일반 앱에 활성화하지 않았다.
- **2026-10-29 현재 QA 인계**: watcher 읽기 비동기화79daaa9 후 단회/반복 본1~4 지연은100ms 이내였지만 본5의 latest clipping으로 eSjVnl은FAIL이다. 7db103f 진단3회는 재현되지 않았다. 내부 scroll은bottom/창visible였으며 부모좌표·trusted입력 진단으로 원인 확인이 남는다. 이를 전체G6 합격으로 보지 않는다.
- **2026-10-29 감시 검증 동시성**: Git `write-tree`는 검증에서도 index.lock을 만든다. watcher 백그라운드와 요청이 동시에 실행하면 실제 충돌하므로 dispatcher에서 순차 수행한다. worker는 취소 가능한 고정 읽기 자식만 담당하고 metadata/인덱스/스캔은 완료 후 최신 결속·held FD 신원 재확인 뒤 수행한다. watcher 결과는 실행 권한이 아니다.
- **2026-10-27 현재 인계**: macOS V11 시작 이관은 사용자 승인 후01d2f19로 활성화했고 해당패키지PASS(059a42f 기록). 전체 sidecar 667PASS/9FAIL/43ignored에서 나온 9실패는 관련 focused 재검사로 해소했다. 변경 후 전체 suite 재실행 결과는 아니다. release기준검사는579.71초PASS이며 remote동시검사8baf58e 후 debug3,000출력도413.27초PASS다. 관련6회귀/독립검토PASS를유지하며G6전체성능증거는아니다. 최신상세는현재워크로그와10월아카이브. G6/실계정/Windows/출시는미완료.
- **2026-10-23 장시간 단일 실행 방향 승인**: 4채팅×각3,000개 공개 이벤트를 여러 짧은 run이나 사용자 메시지로 대체하지 않는다. 출력 저장·조회·전송·표시 계약부터 재설계한다. 구현 전 제안과 L1~L5 순서는 `proposals/m5-long-running-agent-streams.md`; 수치·새 버전은 아직 미확정이며 제품 활성화/성능 합격을 뜻하지 않는다.
- **과거 Codex 단건 복구 완료(2026-10-22)**: 사용자 명시 위험 수용 승인 후 test-only ignored 도구로 원장 배타 잠금·완료/manifest·정확 파일 신원·기록 프로세스 Gone을 재검증해107바이트 codex-login.pending1건을 제거했다. 감사 기록은 앱 데이터에 보존한다. 일반 legacy 자동 복구나 소급 binding 생성은 아니며 실제 로그인/모델 성공은 별도 확인해야 한다.
- **공급자 잠금과 원장 복구의 분리(2026-10-21)**: 새 run은 pending 파일과 원장에 run ID/nonce/파일·디렉터리 신원을 결속하고 명시 복구에서 종료 기록·프로세스 소멸·abandoned manifest를 함께 확인한다. 이 구현은64860d2 패키지에 반영됐다. 과거 Codex107바이트 pending은 canceled/resolvedAbandoned/cleanExit인 원장의3개 PID+시작시각과 일치하지만 결속이 없어 자동 해제하지 않는다. 일반 연결 잠금 복구 버튼으로 이 과거 파일이 해결된다고 안내하지 않는다.
- **격리 경고의 수용 범위(2026-10-21)**: configWarning의 알려진 격리 설명은 설정 조회 대기 중에만 보류하고 실제 disabled project layer와 대조한다. initialized 이후 모든 응답 대기를 허용하면 계정/모델 조회 단계에서 대조 없이 수용하는 구멍이 생기므로 config/read 전용 pending 조건을 사용한다. 이 처리와 실제 사용자 프로필 입장 승인은 별개다.
- **Codex hook 대조 보완(2026-10-20)**: 세션 생성만으로는 비교군 hook이 실행되지 않았다. 별도 승인된 무인증·OS 네트워크 차단 첫 작업 요청 뒤 비교군의 hook 시작/완료·표시 파일 생성과 격리군의 hooks 빈 목록·표시 파일 부재를 대조 확인했다. 종료 확인·임시 정리까지 PASS이며 실제 모델 응답 성공은 아니다. 제품 trust 입장/경고 처리 활성화는 별도 미완료다.
- **Codex 격리 시험과 연결 입장의 구분(2026-10-20)**: named config/read는 cwd를 명시해야 해당 프로젝트 설정을 관측한다. 임시 trusted 프로젝트에서 invocation-only untrusted override의 sentinel 미로드를 대조 확인했지만 configWarning도 발생한다. 시험 전용 경고 후 관측을 제품 경고 무시 권한으로 재사용하지 않는다. 실제 trust-bearing 프로필 차단은 유지 중이며 인증 삭제/재로그인 문제가 아니라 설정 호환 후속이다.
- **실제 공급자 최신 상태(2026-10-19)**: Claude 작은 응답·도구 수정·권한 대기 중 취소는 확인했다. Codex gpt-6.1-sol 파일 수정은 recoveryRequired로 끝났으며 사용자 승인 뒤 앱이 종료 확인→취소됨/복사본 abandoned로 복구했다. 해당 복사본108K는 물리적으로 남아 있으며 자동 삭제 완료가 아니다. Codex 수정 재시험은 새 승인 필요, 기존 중단1회 승인은 미사용이다. G6/400MiB 메모리/Windows/출시는 미완료다. 상세 최신 워크로그는 10월 아카이브(정리됨, Git 기록)에 같은 날짜 전체를 보존했다.
- **압축 이후 채팅 읽기(2026-10-16)**: 압축은 source events를 제거해도 chat_messages에 본문을 보존한다. 화면과 실제 에이전트 입력은 보존 table+남은 source를 함께 읽어야 한다. 페이지 필터 전 전체1..durable_event_sequence 연속성과 복사본 일치를 검사하며 source만 읽으면 기존 대화가 누락된다.
- **native 전송 수치의 visibility 한계(2026-10-15)**: mcy4lx은 hiddenTransitions1/final:hidden이었다. 타이머 기반 부하 전송 자체가 숨김 제한을 받을 수 있으므로659건/180초를 제품 단독 처리량으로 인용하지 않는다. hEC8SO는 visible 선행 대기부터 실패해 전송 미시작이다. probe는 이제 숨김을 실패로 중단하며 제품 background throttling을 끄거나 실패 표본을 재활성화해 통과시키지 않는다.
- **follow-up 비용 진단(2026-10-15)**:64건 작은 자체 fixture에서 공개 append60~85ms와 별도 workspace 검사60~88ms, idempotent retry0.1ms 미만을 관측했다. verify_workspace는 매번 Git HEAD/index/fsck/remotes/metadata를 확인한다. 공개 user-message append는 provider stream과 다른 경로이며 이 결과는12000 provider event 처리량이나 패키지 지연 원인 확정이 아니다. 안전 검사 생략/캐시 없이 Git 비용과 IPC/원장 확장 비용을 분리한다.
- **동시 메시지 증거 범위(2026-10-13)**: NNe0ks의 실제 renderer 합성4채팅12000개/60초 시험은 선택 chat0 전체3000개 관측·최대35.4ms 및 네 채팅 최신200개 전환 일치를 확인했다. background3000개 전체를 화면에서 관측하거나 native 원장/공급자/패키지 G6를 검증한 것은 아니다. actual-agent 실행당256개 제한을 부하 시험 편의로 완화하지 않는다.
- **첫 diff stat 초기화(2026-10-13)**: read-tree 뒤 raw materialize한 snapshot은 stat 정보가 비어 첫 diff에 파일 재확인 비용이 몰렸다. unpublished staging에서 고정 update-index -q --ignore-submodules --refresh 후 기준 트리 검증을 유지한다. 원본 인덱스 불변/동일 길이 편집 감지·필터 미실행 회귀와 독립 APPROVE, 단일 packaged firstTree966→272ms 관측을 확보했다. 반복p95·메모리400MiB 기준은 별도 미완료다.
- **분할 감시 핸들 재결속(2026-10-13)**: Unix watcher는128단계씩 진행하지만 다음 회차에 보관된 자식 핸들을 그대로 믿지 않는다. 현재 root에서 모든 보관 prefix를 재열어 dev/ino를 대조하고 교체는 rescan/폐기한다. 초기 다회차 수집 완료도 rescan을 알린다. non-Unix는 종전 동기 스캔 유지. 감시는 stdio 응답 후/idle poll에서만 진행하며 요청 응답 직전 중복 poll은 제거했다. 반복 성능 문제·전체 watcher 합계 시간 상한은 미해결이다.
- **백업 디렉터리 flock 수명(2026-10-13)**: 설치 journal과 마찬가지로 backup Dir close만으로는 concurrent fork/dup의 OFD 잠금 잔류를 막지 못한다. 생성·조회·예산·정리 모두 성공 취득 전용 ArchiveLock의 Drop LOCK_UN으로 소유자 수명을 끝낸다. duplicateFD 수정 전 FAIL/후 PASS·shared/error 경로·backup24·독립 APPROVE를 확보했다. 이는 잠금 수명 수정이며 동의/원장/복구점 보호/삭제 조건은 그대로다.
- **반영 응답 유실/취소 경합 UI(2026-10-13)**: start 응답만으로 ID를 정하면 접수 응답 유실 뒤 조회·취소가 막힌다. 시작 전 chat-scoped baseline과 다른 새 ID만 채택하고 known ID는 exact 유지한다. 준비 취소 성공만으로 화면을 초기화하지 않으며 진행 요청 종료 뒤 같은 scope에 새 intent가 없음을 확인해야 새 검토로 복귀한다. 합성 React/Chromium 회귀로 검증하며 실제 source write 증거와 구분한다.
- **설치 journal 잠금 수명(2026-10-13)**: flock은 descriptor 하나가 아니라 open-file-description에 남는다. concurrent fork/dup의 핸들이 남으면 단순 drop 뒤 inspect가 잠김으로 거부될 수 있어 InstallJournal::Drop에서 명시 LOCK_UN한다. duplicateFD 회귀의 수정 전 FAIL/후 PASS와 독립 APPROVE를 확보했다. 이는 소유자의 잠금 해제일 뿐 journal/fence 삭제나 설치 완료·프로세스 종료 추정이 아니다. 기존 pending/이력과 명시 복구 조건을 유지한다.
- **설치 활성화 경계(2026-10-13)**: deny-default/제한 read·exec·DNS/443와 기존 shared-file admission·새 file-link 거절로 이전 blocker2건은 독립 정적 재검토에서 해소됐다. OS toy probe와 실제 임시 HOME의 Claude2.1.296/Codex0.162.1 신규 설치+재설치도 통과했다. TLS literal 보완의 검토 후 사용자 명시 승인으로 macOS enabled=true를 연결했으며944bba1 활성화 diff 독립 재검토도 APPROVE다. 이 경로는 hostile same-UID 외부 프로세스가 마지막 경로 재검증 뒤 교체하는 공격까지 완전히 보장하지 않는다. 실제 사용자 HOME·다른 OS/버전·로그인/모델 성공으로 확대하지 않는다.
- **설치/기기 진단 연결(2026-10-13)**: d8b131d는 읽기 전용 OS/CPU/RAM/userData 여유 공간 표시만 연결했다(설치 준비/실행 가능 판정 아님). Codex 선택기는 기존 bundle→레거시 ~/.local/bin 직접 pair→공식 ~/.codex/packages/standalone 링크의 concrete pair 순서이며 기존 서명/no-follow/identity 검증을 유지한다. 465901d의 test-only helper 참조는 수정하고 일반 cargo check·독립 정적 검토를 통과했다. 전체 자동 설치 worker/launcher 회귀/실제 설치 및 기기 UI 검증은 미완료다.
- **Claude 실제 시험 선행 조건(2026-10-13 재확인)**: 새 사용자 로그인 확인/Claude1회 승인 뒤 G5cuDY에서도 자기 check의 idle/error null로 종료했다. 모델 실행0회/원본 불변/정상 종료이며 실제 모델1회 승인은 미사용이다. 앱 전용 Claude 재로그인 완료 통지 전 동일 자동 시험을 반복하지 않는다. 자격증명 부재의 근본 원인은 미확정이며 오류 검증을 우회하지 않는다.
- **개발 저장공간 관리(2026-10-13 사용자 지시)**: 시험마다 큰 합성 프로젝트와 userdata를 무기한 남기지 않는다. 프로세스 종료 확인 뒤 JSON/로그/스크린샷을 남기고 재생성 가능한 fixture를 정리한다. 실패 재현 데이터는 필요한 것만 한정 보관한다. Rust 개발 빌드는 CARGO_INCREMENTAL=0 우선, 빌드 중 캐시 삭제 금지. 사용자 프로젝트·인증·제품 복구 백업은 이 정리 대상이 아니다.
- **2026-10-13 native 대형 diff 최신 결과**: 초기46Z4Lw의 목록9892ms·첫 파일9983ms 초과 이후 요청 단위 Git batch 재사용을 적용했다. b6a68d1의 수동 조회 공유는 반복 시험에서236/500개 stale 목록을 만들어 철회했다. 매번 새 조회＋summary generation 적용 후 SOlVal은 준비1회/측정10회 전부 정상 종료, p95 목록1155.3ms·첫 파일816.6ms로 기준 이내다. 개발 빌드 diagnostic/NOT_READY이며 packaged G6 합격은 아니다.
- **2026-10-13 macOS 명시 정리 활성화 승인**: 공개 경로·중단 후 DB 재열기·합성 UI 동의/응답 유실 검증과 독립 APPROVE 후 사용자가 제품 활성화를 별도로 승인했다. application_cleanup_enabled는 macOS에서만 true다. 이전 승인 대기는 해소됐으며 current terminal 복구점 보호·명시 retirement/별도 정리·자동 삭제 없음·원본/잠금/이력 보존 계약은 유지한다. 실제 사용자 백업을 이 개발 작업에서 삭제하지 않는다.
- **2026-10-13 대형 프로젝트 최신 단일 관측**: 084ceae batch sync 반영 debug sidecar의 h5k4N8 native UI는 1만 파일에서 편집/저장/diff·반영 확인 취소·두 채팅·정상 종료까지 PASS했다. snapshot33.4초/treeRoot977ms/expand75ms다. 아래 이전 실패를 최신 실행 결과로 반복하지 않되, 단일 diagnostic을 반복 성능/G6 합격으로 확대하지 않는다.
- **2026-10-13 대형 프로젝트 현황**: Unix Git 파이프는 poll 준비 상태 대기로 변경(b08eddf), 파일/채팅 감시 한도 초과는 exact `watching:false,rescanRequired:true`를 양쪽 UI에 전달(c4fb3bf/60fccd8). 기존 실제10000파일 시험에서 snapshot53초·treeRoot1534ms(1000ms 초과), 편집/저장/diff 성공 뒤 반영 확인15초 대기 실패. 전체 성공 아님. 현재 성능 수집기·RSS 초과·Claude 실제 작업·Windows/출시 증거가 남으며 진단 PASS를 G6로 확대하지 않는다.
- **최신 실제 응답 성공 범위(2026-10-13, c5dde79)**: 974Ab5에서 Codex gpt-6.1-sol medium planning1회 succeeded/cleanExit/응답 확인, 원본 불변/앱 정상 종료. Claude는 요청 ID 결속된 로그인 검사에서 idle로 실행 못 함. 전체 시험 FAIL이며 아래 이전 Codex requirements 차단은 최신 작은 planning 경로에서는 해소됐다. 다른 쓰기/복구/Windows/제품 완료로 확장하지 않는다.
- **최신 실연결 차단(2026-10-13, e89bde5)**: W4SUaF는 두 로그인 유지·Codex 모델 선택 뒤 configRequirements/read 및 Claude start discovery differs에서 차단됐다. RUN_ACCEPTED 없음, 임시 원본 불변/정상 종료. 현재 Claude 링크2.1.296이며 이전2.1.293 관측을 현재 동작으로 가정하지 않는다. 10월8일 account/plugins 실패보다 앞 단계의 최신 증거다.
- **Claude 내장 mod와 safe-mode(2026-10-08, 승인 2026-10-19)**: 공식 mods overview는 safe-mode/bare/disableAllHooks가 내장 mod를 끄지 않는다고 명시한다. e5324bd 패키지 시험에서 init plugins organization guard를 관측했다. 사용자 승인으로 서명 검증 native 연결의 정확한 단일 조직 guard만 수용하는 소스 구현을 진행했다. generic/fixture와 다른 plugin은 계속 거절한다. 조직 정책을 우회하는 임의 disable 또는 builtin 표시만으로 허용 금지. 실제 전체 shape·실계정 성공·최신 패키지는 미검증이다.
- **Claude safe-mode 내장 플러그인(2026-10-08)**: 서명된2.1.293의 빈 임시 프로필/가짜 API/도구 없음에서도 init plugins가 cc-plugin-agents-md@builtin·cc-plugin-plugin-authoring@builtin을 보고했다. 공식 enabledPlugins에서 두 ID를 false로 지정하면 빈 배열이 된다(5ikQWQ→7D0mvz). 제품은 이를 명시 비활성화하며 알 수 없는 플러그인 거절은 유지한다. 실제 계정 성공·다른 버전 보장을 뜻하지 않는다.
- **실제 연결 최신 관측(2026-10-08, 548c4f8)**: 두 로그인/모델 조회 및 실행 접수는 가능하나 실제 응답은 실패한다. 승인된 각1회 WD8GdD 시험에서 Codex turn stream account metadata, Claude init plugins other builtins 경계 실패·둘 다 cleanExit, 임시 원본 불변/앱 정상 종료를 확인했다. 이는 구체 payload 원인 확정이나 제품 완료가 아니다. 추가 실제 실행 승인은 소진됐으며 무계정2.1.293 plugins 제거 결과를 실계정 성공으로 대체하지 않는다.
- **모델 선택과 실행 권한 분리(2026-10-07)**: Codex의 bounded catalog 표시값은 종료 확정·exe/helper/home 재검증 뒤 agent.getModelChoices/m5Models로 전달한다. 채팅/역할/workspace 결속과 단발 소비를 유지하고, 실행 경로는 여전히 선택 tuple의 새 preflight를 요구한다. 역할 화면은 projectId/revision/chatId로 remount해 이전 채팅 응답 유입을 막는다. 기능 코드f1a135c, 실제 목록 조회·안전 회귀는 최신 워크로그 참조.
- **Claude 내부 소켓 호환성(2026-10-07)**: profile/tmp/srt-mux-PID-seq.sock은 실행 중 생성·종료 후 제거되는 unix socket으로 관측됐고 공식 anthropics/sandbox-runtime mux-proxy.ts의 내부 HTTP proxy와 일치한다. 일반 scanner는 계속 거절한다. 추적 Claude parent와 전용 tmp에 결속하는 제한적 예외는 사용자 승인 대기이며 일반 소켓/링크 허용 승인이 아니다.
- **실계정 연결 재검증(2026-10-07, 415a155 시점)**: 두 provider의 앱 전용 로그인은 connected이나 실제 작업 준비는 실패한다. 최신 Codex는 선택 모델의 catalog 부재, Claude는 runtime 특수 파일 거절로 목표 전달 전에 중단된다. login 성공을 실행 성공으로 취급하지 않는다. 승인된 모델 목표는 아직0회이며 임시 원본은 불변이다. 최신 시험 session60083은 shutdown accepted/exit1(연결 FAIL)로 정상 종료했고 main 산출물에도 반복 unavailable 알림 억제가 반영됐다. 이전 초기화·잔류 창 기록은 당시 상태다.
- **현재 M5 마감 상태(2026-10-07, 5209128 이후)**: macOS 합성 실제 UI에서 시작 안내/설정 저장·재열기/두 채팅·편집·diff/원본 반영·renderer reload·복원 PASS. 585bea4에서 별도 앱 프로세스 정상 재시작 후 두 채팅의 저장 파일 분리와 원본 불변도 확인했다. 파일 목록은120행 가상화, 설정 modal/첫 목표/최신 대화 따라가기는 연결됐으나 전체 시안 마감과 실제 모델 종단 검증·Windows/G6/출시는 미완료다. 편집기 dispose는 미완료 stream을 끝내고 늦은 descriptor를 무시한다. 최신07:10 synthetic 메모리 idle406.8/peak884.4MiB는 FAIL,60초 잔류delta13.3MiB는 기준 이내며 heap 감소만으로 누수 여부를 단정하지 않는다. 아래 과거 실측보다 이 최신 상태를 우선한다.
- **macOS 원본 writer 활성화(2026-10-07)**: 신규 archive/after staging의 device별 disk-free reserve, 순차 rename 복구 allowance, 쓰기 직전 재검사 및 archive 취소 검사를 연결했다. 공간·취소 독립 차단은 해소했고 macOS source_writes_enabled=true다. 합성 실제 UI의 편집→diff→원본 반영→복원→두 채팅 전환 PASS. 삭제/기록 정리는 별도 application_cleanup_enabled=false이며 실제 계정/사용자 프로젝트·Windows/출시 합격과 구분한다.
- **재시작·원본 반영 준비 실측(2026-10-07)**: 합성 userData의 앱 재시작/저장 프로젝트·채팅·편집 보존, baseline 포함 채팅의 검토/확인 취소·원본 불변 UI PASS. 원본 writer의 내부 application28/backup10 통합은 통과했지만 제품 source_writes_enabled=false와 활성화 전 독립 검토는 별도다.
- **메시지 snapshot과 새 메시지 구분(2026-10-07)**: metadata-only chat 원장 이벤트는 last_message를 보존한다. get_chat은 연속된 완전 동일 snapshot만 합쳐 공개 메시지 순서를 유지한다. 저장 후 두 채팅 전환/재열기 및 사본 분리 실제 UI PASS로 직전 미완료를 해소했다. 시험 창 background throttling 해제는 제품 변경이 아니다.
- **자신의 저장 알림(2026-10-07)**: file.changed는 외부 변경의 확정 증거가 아니다. saving 중 알림을 저장 결과 뒤 현재 버전과 대조하고 clean 버전 일치도 안내 없이 소비한다. explicit conflict는 즉시 보호한다. 늦은 읽기는 epoch/workspace/baseVersion뿐 아니라 blocked gate도 다시 확인한다. 실제 저장/diff는 진전했으나 두 번째 채팅 편집기 표시 검증은 미완료다.
- **실제 편집 IPC 정합성(2026-10-07)**: tree 루트 빈 경로만 허용하고 파일 경로의 빈 값/탈출은 계속 거절한다. ChatRecord에서 IPC workspace 주소 네 필드만 추출해야 하며 workspace 저장 chunk/commit은 V2와 달리 saveId를 응답해야 한다. 격리 실제 UI에서 편집·저장·diff 목록/원본 불변 확인; 자신의 저장 후 외부 변경 안내가 남는 감시 UX는 후속이다.
- **macOS native 제품 연결(2026-10-07)**: 일반 --stdio의 전역 blocked를 macOS에서 해제하고 scoped native dispatcher를 선택한다. 시작별 입장·관측·도구 승인·종료/복구 검사는 유지하며 비-macOS G4 차단은 유지한다. native capability는 fixture와 독립된 고정 목록이다. 이전 비활성 기록은 당시 상태이며 현재 코드 연결/무계정 stdio 검증과 실제 계정/모델·패키지/UI 합격은 구분한다.
- **native/fixture 기능 분리(2026-10-07)**: native ready/status는 기존 agent baseline에서 run.startFixture를 제외한 별도 exact 계약이다. native 세션은 새 fixture 시작을 sidecar/Electron에서 거절하고 작업대에서 숨긴다. 과거 원장용 cancel/recovery는 유지한다. 이 분리만으로 일반 --stdio 활성화 또는 실제 계정/모델 검증을 완료한 것은 아니다.
- **Claude 추론 기본값(2026-10-07)**: reasoningEffort=null은 공급자 기본값 요청이며 --effort를 생략한다. 실제 적용 수준을 관측한 값이나 medium으로 치환하지 않는다. 명시 수준의 catalog 지원 검사와 null/명시값 역할·범위·요청 정책 결속은 별도로 유지한다.
- **Claude 보장 범위 승인(2026-10-07)**: 공식 공급자 보호를 신뢰하고 요청 정책/실제 세션 관측을 분리한다. 관측 불가능한 config·관리 설정 불변성은 런타임 증거로 만들지 않는다. control 초기화 뒤 목표를 보내고 system/init을 검증한 뒤 도구를 허용한다. 이 설계 결정 대기는 해소됐으나 제품 활성화·계정/유료 시험·전체 완료 승인과 다르다. 구체 계약은 제안서 §6.9의 사용자 확정 항목, 구현은 phased_claude 기록에서 시작한다.
- **실제 실행 연결의 현재 경계(2026-10-07)**: Codex/Claude scoped 조회·fresh 예약·시작 dispatcher와 Claude 정책 Admission·단계별 원장·상세 확인 후 단발 승인 후보를 연결했다. Claude 버전·앱 프로필 인증·모델 관측은 분리하고 scope/역할/전체 30초 수명에 결속한다. 제품은 아직 blocked이며 누적/독립 안전 검증과 활성화가 남는다. 초기화 성공이나 credential 존재를 실제 유료 모델 성공으로 취급하지 않는다. 원문은 월간 아카이브에 보존한다.
- **서명 검사 실행 환경 주의(2026-10-06 정정)**: 제한 환경의 codesign invalid signature만으로 설치본 손상을 판정하지 않는다. 같은 공식 Claude 2.1.291 다운로드가 제한 밖에서 통과했고 기존 Claude 2.1.287·standalone Codex·ChatGPT.app 내장 Codex도 strict/all-architectures 검사를 모두 통과했다. 이전 서명 장애 및 재설치 필요 판단은 철회한다. 공식 복구는 사용자 승인됐지만 불필요한 설치/종료는 수행하지 않았다. 제품 서명 검사는 유지하며 우회·재서명·고정 해시 등록으로 대체하지 않는다.
- **Claude shell sandbox와 파일 도구 구분(2026-10-06)**: 공식 sandboxing 문서상 sandbox는 셸 도구에만 적용되고 Read/Edit/Write는 권한 규칙을 따른다. 기본 셸 읽기는 광범위하며 can_use_tool 콜백이 모든 호출을 가로채지도 않는다. Claude native owner는 연결됐지만 strict shell 인자만으로 전체 target/credential 보호를 주장하지 않는다. 가짜 API/sentinel의 내부 도구 성공·외부 셸 OS 거절 관측은 scripts/m5-claude-tool-probe.mjs로 재현 가능하며 관리 정책/reload/전체 제품 gate를 대체하지 않는다.
- **Claude 권한 통신(2026-10-06)**: SDK can_use_tool은 --permission-prompt-tool stdio로 control 요청을 받는다. 옵션만으로 모든 도구가 콜백을 거친다고 보장하지 않으며 allow 규칙/권한 모드의 우회와 실행 격리 검증은 별도다. 후보 인자와 거절 IO가 연결됐으나 제품 실행/UI 활성화 증거는 아니다.
- **Claude safe-mode 경계(2026-10-06)**: 공식 CLI의 safe-mode는 bare와 달리 구독 인증을 유지하지만 관리 정책 hook 등 일부 동작은 제외하지 않는다. 인자 구성만으로 실행 admission/OS 격리를 입증하지 않는다. 앱 프로필로 실제 실행할 때 생성되는 파일과 권한 응답·종료 검증은 여전히 후속이다.
- **Claude init의 관측 한계(2026-10-06)**: 일반 system/init의 effort 누락은 유지되며 사전 control initialize의 모델별 effort 지원·resolvedModel/default mode/hooks 등록을 별도로 대조한다. 무계정 초기화에도 모델 목록이 나오므로 인증은 고정 auth status 조회로 분리했다. 초기화/첫 프롬프트·실행 owner는 연결했지만 이 관측은 OS 격리·관리 정책 불변성 증거가 아니다.
- **Claude 공개 출력 계약(2026-10-06)**: SDK assistant frame은 message.id가 반복될 수 있어 uuid로 구분한다. main-session text만 공개하고 검증된 init receipt/running 상태를 원자 저장하며 result 이후 실제 종료·terminal 저장까지 owner가 유지된다. 오류를 finished로 오판하지 않도록 native poll을 수정했다. 제품 실제 모델 실행은 아직 비활성이며 fake/parser/컴파일 통과를 실사용 증거로 사용하지 않는다.
- **백업 목록의 정리 상태(2026-10-06)**: backupRetention은 연결 이유와 별도로 cleanupState를 반환한다. deleted는 영속 정리 기록이며 파일 부재의 새 관측이 아니다. 별도 inspectBackupCleanup에서 실제 파일을 대조하고, 정리 후에도 이력은 보존한다. 제품 삭제는 여전히 비활성이다.
- **복구점 종료 후보(2026-10-06)**: 완료된 현재 기록을 project/lock CAS로 과거 이력에 보존하는 내부 경로와 확인 IPC/UI를 연결했다. 원본·백업 파일/잠금은 변경하지 않으며 종료 뒤 별도 확인으로 과거 백업을 정리한다. 제품 기본값은 비활성, 통합·독립 검토 전이다.
- **백업 보존 조회(2026-10-01)**: backupRetention은 현재/과거 기록의 review·파일 이름 연결을 UI에 표시한다. 파일 검증/삭제 권한은 아니며, 삭제는 staging 파일 검증과 intent 복사본을 함께 다루는 영속 삭제 계약이 필요하다. 삭제 기능은 미구현이다.
- **종료된 준비 정리 후보(2026-10-06)**: 참조 없는 abandoned 준비 파일의 pending/deleted 영속 정리와 명시 확인 IPC/UI를 연결했다. 제품 삭제는 검증 전 비활성이다. 현재/과거 복구 백업과 소유 기록 없는 파일은 정리 대상이 아니며 전체 복원 지점 삭제 완료를 뜻하지 않는다.
- **정리 후보 확장(2026-10-06 후속)**: 현재 복구가 참조하지 않는 ready 백업 및 원본 백업이 정확히 일치하는 terminal 과거 이력을 명시 정리 후보에 포함했다. 파일 삭제 시 해당 백업 복원을 포기하고 이력/삭제 기록은 남긴다. 제품 삭제는 아직 비활성이고 현재 복구점/고아 파일은 미지원이다.
- **복원 예산 UI 후보(2026-10-01)**: 앱 전체 사용량 조회와 명시 예산 변경(기본5GiB, 정수1~1024GiB)을 IPC/UI에 연결했다. application-settings-v1의 원자 설정과 archive flock을 공유하며 기존 백업 자동 삭제는 없다. 통합 검증 전 구현이며 원본 writer 활성화·삭제 기능 완료를 뜻하지 않는다.
- **named 권한 조회 계약(2026-10-06)**: 정상 서명된 앱 내장 Codex 0.159.2에서 generate-json-schema --experimental로 상세 계약을 확보했다. permissionProfile/list는 cwd/cursor/limit, 응답은 data[{id,allowed,description?}]/nextCursor다. 기존 named RPC 후보에 cwd와 조회를 연결했고 complete page의 유일한 요청 ID/allowed=true를 대조한다. 선택 허용은 실제 filesystem/network 보호 증거가 아니며 named 설정/thread/turn·owner 연결은 아직 남는다. 제한 환경 서명 오류를 설치 손상 차단으로 반복하지 않는다.
- **구현 우선 운영(2026-10-01 사용자 지시)**: 매 작업 검증 대신 남은 기능 전체 연결 후 통합 검증·순차 버그 수정을 진행한다. 미검증 구현 체크포인트 커밋은 가능하지만 제품 완료/gate PASS가 아니다. 실제 실행 활성화·계정/유료/사용자 프로젝트·출시 경계는 유지하며 설치본 장애와 독립적인 기능 구현은 계속한다. 루트 지침의 같은 제목 절이 과거 검증 빈도 규칙보다 우선한다.
- **실행 격리 전환 승인(2026-10-01, 10-06 정정)**: §6.9의 서명된 공급자 부모 신뢰와 공식 도구 sandbox 분담/무계정 임시 자료 시험은 사용자 승인 완료다. 이전 정책 입력 대기를 반복하지 않는다. 당시 제한 환경의 서명 오류로 시험을 중단했으나 10-06 제한 밖 검사는 통과했다. 설치 손상이라는 판단은 철회하며 제품 활성화·실제 계정/유료 모델 승인과 구분한다. 정상 서명 설치본의 도구 보호/종료 검증은 남는다.
- **Claude 로그인 업데이트 입장(2026-09-30)**: 고정275/270 버전/SHA 목록 대신 공식 native launcher가 가리키는 versions/X.Y.Z 실파일을 Anthropic 서명으로 검증한다. 공식 링크는 bin 밖 절대경로이므로 cap_std의 read_link가 아닌 read_link_contents 후 고정 target 검증을 사용한다. launcher 자체는 실행하지 않고 현재 선택한 실파일을 bind한다. 설치2.1.285 읽기 전용 검증 통과는 계정 인증/모델 실행 완료가 아니다. npm/Homebrew/custom launcher 경로는 별도다.
- **Codex 로그인 업데이트 입장(2026-09-30)**: 로그인·진단은 고정 bundle native 후보의 Apple anchor/OpenAI Team2DC432GLL2/exact CLI·helper identifier 서명으로 입장한다. release SHA 목록은 이 경로에서 제거했고 실행 중 교체 감지용 digest는 유지한다. 새 코드 경로의 설치본 검사는 CLI/helper 수락·잘못된 identifier/다른 공급자 거절을 확인했으나 계정/모델을 호출하지 않았다. Claude와 actual runtime의 pin/호환 전환은 남으며 모든 upstream 파괴적 변경 호환을 보장하지 않는다.
- **복원 예산 내부 연결(2026-09-30)**: 반영 백업 생성은 앱 데이터 루트의 모든 archive 일반 파일을 합산해 기본5GiB 내 write만 허용한다. 고아/abandoned도 포함하고 directory flock으로 협력 작성자를 직렬화한다. 기존 파일 자동 삭제 없음. preparing 예약 뒤 한도 실패 시 명시 복구가 필요할 수 있다. 제품 원본 writer·복원/정리/예산 설정 UI는 별도 미구현이며 이 내부 제한으로 전체 반영 기능 완료를 주장하지 않는다.
- **1·3 승인과 오프라인 실측(2026-09-30)**: 오프라인 설치 CLI/가짜 sentinel 시험과 원본 반영 외부 편집 중지·보존형 충돌 중단/복원5GiB·자동 삭제 없음 정책은 사용자 승인 완료다. 아래 승인 대기는 과거 상태다. 시험은 설정 조회까지 성공했으나 현재 outer sandbox와 vendor 내부 sandbox 조합에서 exit71로 실패해 실제 도구 격리 성공은 미입증이다. 업데이트 대응 adapter와 격리 책임을 함께 해결해야 하며 같은 승인을 반복 요청하지 않는다.
- **업데이트 호환 우선(2026-09-30 사용자 확정)**: 공급자 정상 업데이트를 exact 실행 파일 SHA-256 미등록이라는 이유로 차단하는 정책은 제품 방향에서 제외한다. 구버전 고정/중복 전용 설치로 이를 대체하지 않는다. 이전 앱 전용 설치 A/B 권고·선택 요청은 철회됐으며 제안서 §5.1.1의 신뢰/기능 호환 분리와 공식 adapter 방향으로 재설계한다. 코드의 기존 pin은 아직 남아 있고 검증 없이 제거하지 않는다. 아래 설치 공급 결정 대기 기록은 당시 상태다.
- **M5 설치 공급 불일치(2026-09-30)**: 레거시 Codex standalone 최신 설치와 M5의 pinned 앱 번들 CLI/helper admission은 호환을 보장하지 않는다. 앱 전용 검증 버전 설치 vs 기존 설치 사용 정책은 제안서 §5.1.1의 사용자 결정 대기다. 사전 진단의 후보 부재를 시스템 전체 미설치로 단정하거나 latest 설치 성공을 사용 가능으로 표시하지 않는다.
- **M5 현재 후속(2026-10-01)**: macOS 일반 파일/링크 순차 반영·역복원·완료 잠금 해제·부분 취소·완료 이력 보존은 내부 미검증 구현이다. 제품에는 읽기 전용 반영 상태 조회만 연결했고 쓰기는 열지 않았다. 원본 보호 조건/5GiB 예산은 승인 완료다. 디렉터리·staging 실패 복구·공개 반영/이력 선택·실제 공급자 활성화·Windows/G6/출시와 통합 검증이 남는다. 내부 구현을 앱 완료로 인용하지 않는다.
- **대규모 snapshot 생성 비용 관측(2026-09-30)**: 101개 작업 복사본 회귀의 진행 중 1초 CPU sample에서 append_event의 전체 원장 replay/JSON 검증을 관측했다. storage.rs:append_event_with_failure도 매 append 후 전체 events를 재로드·replay한다. 검사 정지를 의미하지 않으며, 기존 보존/중복/충돌 계약을 유지하는 성능 개선은 별도 설계·검증해야 한다. 단일 debug sample을 배포판 성능 수치로 인용하지 않는다.
- **채팅별 검토 대상(2026-09-30)**: 각 chat descriptor 슬롯과 exact task/workspace/chat revision·lock 신원/revision을 결속한다. 다른 chat의 active 포인터로 바뀌어도 원래 manifest를 검증하며 chat/lock 변경은 옛 검토를 무효화한다. 세 원장 revision CAS와 미소유 lock을 요구하고 아직 승인/쓰기 권위는 없다.
- **A2 ReviewPlan 후보(2026-09-30)**: receipt로 검증된 source/snapshot 변화·신원을 읽기 전용으로 묶는다. 최신 descriptor는 project 원장 CAS로 게시하고 재시작 시 현재 파일에서 재구성/대조한다. untracked·경로 충돌·미지원 권한과 승인 상태/superseded ID는 거절한다. 과거 파일 내용 보관이나 승인/쓰기 권위는 아니며 chat/task 확인 수명과 A3~A5는 남는다.
- **snapshot 준비 복구(2026-09-30)**: snapshot_recovery는 version1 beforeSnapshotExport만 두 revision의 트랜잭션 CAS로 원자 폐기한다. project.catalog와 저장 프로젝트 UI의 명시 확인/종료에 연결했고 exact 영속 완료 evidence에만 응답 유실 재확인을 허용한다. legacy/실행 시작/파일 존재/chat 또는 run 결속은 거절한다. 시작 이후의 불확실 종료 복구는 별도 남는다.
- **Unix Git 대기 한도(2026-09-30)**: fixed Git stream stdin/stdout/stderr/exit에 명령별120초 deadline을 공유한다. stderr 완료까지 unreaped leader를 유지하고 오류는 그룹 종료·최대2초 direct reap을 시도한다. pipe 보유 자손의 무한 대기는 닫았지만 escaped/비중단 전체 tree 종료 증거는 아니며 Windows fallback에도 적용되지 않는다.
- **A1 원장/생성 결속 후보(2026-09-30)**: source_baselines[worktreeId]는 불변 descriptor를 게시·조회하며 close/open에서 보존한다. 내부 ApplicationCandidate 생성3회·재시작 검증을 통과했고 활성화는 최신 원장 extension을 보존한다. DB 게시 실패는 artifact와 잠금을 남긴다. 공개 생성 활성화·시작 이후 복구·누적 크기/압축은 남고 아래 receipt 게시/생성 미구현 기록은 이 내부 후보 추가로 갱신한다.
- **A1 receipt 저장 후보(2026-09-30)**: opaque ReceiptStore만 전용 source-baselines-v1에 불변 파일을 쓸 수 있다. 항목별 streaming과 encoded64MiB 한도·fsync·재검증을 제공하지만 descriptor 원장 게시/실제 snapshot 생성 연결은 미구현이다. 원장 descriptor 없는 legacy를 파일만으로 승격하지 않는다. 이전 raw blob 항목의 영속 receipt 없음은 내부 파일 후보 추가로 갱신하되 writer 권위는 여전히 없다.
- **A1 raw blob 비교 후보(2026-09-30)**: capture_clean_baseline은 index/HEAD와 raw 파일 상태를 묶고 blob SHA-256 및 Git object OID를 직접 대조한다. ordinary byte/mode/link drift·missing object·root 경로 교체와 필터 미실행 회귀를 통과했다. 지금은 내부 반환값만 있고 영속 receipt나 writer 권위는 없으며, 기존 Git helper의 시간/자손 수명 제한을 제품 활성화 전에 닫아야 한다.
- **A1 index 비교 함정(2026-09-30)**: mode/OID/path만으로는 HEAD의 빈 파일에 대한 intent-to-add를 구분하지 못한다. cached-only diff-index와 명시 외부 converter 차단을 더하고 dirty exit1을 dirtyConflict로 분류한다. split index는 별도 shared image가 필요해 거절하고 sparse-index·full-index sparse·assume-unchanged도 현재 내부 후보에서 거절한다. 실제 raw blob 동등성·receipt 결속 전에는 적용 권위가 아니다.
- **원본 baseline 내부 후보(2026-09-30)**: project/application.rs는 root .git만 제외한 raw inventory를 bounded/no-follow로 읽는다. ignored/임시 이름과 nested .git도 포함하며 identity와 내용 변경을 구분한다. 읽기로 바뀔 수 있는 atime은 무변경 보장에서 제외한다. 두 pass 일치·집중 회귀·독립 검토는 HEAD/index/영속 receipt 또는 원본 writer 활성화의 증거가 아니다. 상세와 한도는 proposals/m5-change-application.md A1을 따른다.
- **고정 CLI의 권한 계약(2026-09-29)**: SHA 3e11ccc…aca5a의 일반 SandboxPolicy에는 최신 문서의 restricted read 필드가 없다. `generate-ts --experimental`에서 named permissions와 activePermissionProfile을 확인했으나 runtime 유효 권한/OS 강제는 아직 미검증이다. beta permissions와 기존 sandbox_mode를 혼용하면 안 된다. 별도 도구 scratch의 synthetic OS 격리는 PASS지만 vendor tool routing 분리는 미구현이다.
- **내부 일회 허용과 제품 활성화 경계(2026-09-29)**: core는 accept bytes를 생성하지 않고 순서만 예약한다. private sidecar transport가 inspected live ticket·현재 예약/격리/취소·원자 intent 뒤 일회 accept를 전송하며 부분 전송도 재검증한다. toy 6case와 독립 검토는 통과했으나 공개 accept/native start는 미활성이다. PROFILE 전체 읽기/쓰기가 vendor 자손에 상속되므로 인증/profile 도구 격리가 다음 활성화 차단 조건이며 기존 snapshot 보호와 혼동하지 않는다.
- **승인 상세 조회(2026-09-29)**: 기본 요약과 별도로 명시 inspect만 live ticket/실행/연결에 결속된 상세를 일시 반환한다. 원장/채팅에는 저장하지 않으며 평문 표시 때 C0/C1·전체 Cf를 escape한다. 상세 열람은 추가 권한 허용이 아니며 제품 native 활성화는 별도다. 아래 유형 표시 당시의 상세 미지원 상태를 이 구현으로 갱신한다.
- **권한 요청 요약(2026-09-29)**: 공개 승인 pending은 `{approvalId,kind}`만 반환한다. 고정4유형 표시는 raw 행위/host/명령 원문 공개나 허용 권한이 아니며, 구체적 허용 결정을 위한 정보/경계는 별도 구현 대상이다.
- **승인 UI 경계(2026-09-29)**: run.approval은 opaque 대기 ID 조회와 명시 거절만 제공하며 receipt의 queued는 접수일 뿐 wire 전달 증거가 아니다. exact suffix·실행 신원·현재 frame을 검증하고 만료 요청 오류는 세션 전체를 종료하지 않는다. 카드의 허용 기능/요청 상세/실제 provider 활성화는 아직 없으며 이 부분 구현을 앱 완료로 주장하지 않는다.
- **native 승인 live 대기열(2026-09-29)**: 자동 거절은 명시적 거절 접수 방식으로 교체했다. 조회/응답과 매 advance 뒤 실제 미응답 stream 집합을 대조하며 취소/완료/서버 해제 뒤 ticket을 제거한다. 요청 ID는 프로세스 로컬이며 원장만으로 복원하지 않는다. 내부 조회/거절은 exact 실행 신원과 예약 입력을 재검증한다. 공개 UI/IPC와 허용은 후속이다.
- **승인 연결 신원 보강(2026-09-29)**: 아래 초기 승인 의도 기록의 후속 조건 중 실제 process UUID 결속은 완료했다. dispatcher stream과 LoginProcess UUID를 함께 사용하며 BoundTextSession의 매 advance 신원 검증을 유지한다. UI/직전 권한 재검증/허용 연결은 여전히 남는다.
- **승인 의도 기록(2026-09-29, 내부 후보)**: native 자동 거절은 실행 신원·dispatcher stream·thread/turn/request/item/시각·내용 digest에 묶인 일회성 ticket을 원자 저장한 뒤 전송한다. 원장은 최대128개 opaque 의도이며 전달/수행 성공이나 승인 권한이 아니다. 저장 실패는 무전송·정리로 처리하고 terminal/recovery에서도 기록을 보존한다. 독립 검토 scoped PASS. 실제 허용 경로는 process transport UUID의 추가 결속·직전 권한 재검증·UI 연결이 필요하며 AcceptOnce는 여전히 차단된다.
- **프록시 실행 원장 결합(2026-09-29)**: RunReservation의 내부 proxied launch는 기존 offline 경로와 입력/pin/profile 검증을 공유한다. identity 기록 전에 gate를 열지 않으며 release 저장 실패는 transport/broker 중단 후 owner cleanup으로 처리한다. toy10경로 통과는 제품 admission이나 실제 모델 작업의 증거가 아니다.
- **격리 조회 CA 해결(2026-09-29)**: 기존 account id3 실패는 OS 공개 CA 파일을 fixed env/exact read로 명시한 뒤 실제5 RPC·cleanup 통과했다. CONNECT/auth/upstream은 동작하며 키체인/Mach 권한 또는 TLS검증 해제는 필요하지 않았다. 아래 격리 조회 차이 기록은 수정 전 증거다. 사설 사용자 CA·실제turn·domain-fronting·제품 활성화까지 검증된 것은 아니다.
- **격리 조회 차이(2026-09-29)**: 실제 앱 프로필의 비격리 full preflight는5 RPC 통과하지만 OS sandbox+proxy 후보는id1/2 뒤 account id3 단계에서 거절된다. 두 경로 모두 종료/cleanup 확인이며 기존 로그인 실패로 단정하지 않는다. 다음은 계정 응답의 비밀 없는 구조 진단이고, proxy CONNECT 성공이나 제품 활성화는 아직 증명되지 않았다.
- **중계/실행 소유권 결합(2026-09-29)**: ModelProxyCommand가 Broker를 LoginProcess에 이동하고 proxy secret은 고정 환경으로만 전달한다. owner tick으로 처리하며 종료/무효화 때 stop, 불확실 Drop은 listener만 보존하고 Broker의 연결/작업/인증 객체는 drop한다. 중계 실패는 프로세스 종료 증거가 아니다. native 환경 호환성·제품 admission은 아직 미확인이다.
- **Broker/DNS 연결(2026-09-29, 미활성)**: fixed destination DNS 결과를 모두 검증하고 숫자 주소에만 연결한다. DNS/연결 작업자는 전역8개이며 취소된 정지 resolver도 실제 종료까지 슬롯을 점유한다. Broker는8연결·600초/128MiB aggregate이며 stop 때 listener를 drop하지 않는다. 제품 process lease 결합·native 호환성은 후속, 실제 공급자나 TLS/domain-fronting 검증으로 사용하지 않는다.
- **모델 중계 transport(2026-09-29, 내부)**: Relay는 preconnected socket2개·방향별8KiB·128MiB/600초를 소유하고 EOF half-close/역압·취소·오류·Drop을 처리한다. 로컬 소켓 회귀만 검증했으며 broker/DNS/동시 연결 제한·process lease 결합은 후속이다. TLS peer identity나 경로 접근 권한을 검증한 것으로 사용하지 않는다.
- **모델 중계 입장(2026-09-29, 제품 미연결)**: provider_model_proxy는 per-run Basic credential과 bounded CONNECT framing만 검증한다. 정확 authority/Host·단회 소비·8KiB/5초를 적용하며 실제 curl의 로컬 요청은 통과했다. upstream DNS/public IP·TLS 내부 목적지·relay 예산·native 모델 호환성은 별도다. 이 parser를 실행 권한 또는 완성된 네트워크 격리로 사용하지 않는다.
- **macOS 종료/회수 경합(2026-09-29)**: identity Gone과 Child::try_wait의 종료 결과 준비는 동시에 보장되지 않는다. LoginProcess는 confirmed tree에서 Ok(None)이면 원래 8초 종료 예산 내 pending을 유지하며 root 회수까지 fence/listener를 놓지 않는다. 동기 재시도는 TreeTracker::terminate의 fresh 관측을 유지해야 cached failure로 생존 프로세스 정리를 건너뛰지 않는다. 실패 receipt/integrity 오류를 재시도로 성공 승격하지 않는다.
- **중계 endpoint 소유권(2026-09-29)**: loopback tcp4 단일 포트 정책만으로 listener 신원 수명이 보장되지 않는다. opaque command가 listener를 lifecycle owner에 옮기도록 수정했고, 종료 확인 전 보유·불확실 Drop 시 sidecar 종료까지 descriptor 보존을 검사했다. 상류 broker 인증/목적지/실제 모델 통신은 아직 미완료이며 제품 활성화에 사용할 수 없다. cat 자연 종료 cleanup 실패 1회는 원인 미확정으로 남긴다.
- **native 시간 예산 분리(2026-09-29)**: 이전 전체30초 toy 제한을 연결/thread 준비30초와 turn queue 생성부터 총10분으로 분리했다. 도구·승인 대기 포함, 활동 갱신 없음, 취소·sticky failure·기존 cleanup 경계 유지. 실제 모델 장시간 실측/제품 활성화는 아직 별도다.
- **stale helper 보존 복구 완료(2026-09-29)**: exact 옛 tmp helper 디렉터리를 `codex-stale-helper-preserved-ec5db708-c8c2-4683-8dfd-72c926603381`로 inode 그대로 보존했다. 현재 앱 profile 전체 runtime 파일 검사가 통과하며 기존 stale 링크는 더 이상 차단이 아니다. 명시 복구만 구현했고 자동 허용/링크 scanner 완화는 없다. 정상 vendor helper의 실행 수명과 제품 활성화 검증은 별도 남는다.
- **stale helper 차단 실측(2026-09-29)**: 실제 앱 tmp의 codex-execve-wrapper는 사라진 옛 번들 경로를 가리킨다. 고정 CLI offline 임시 프로필 재현에서 초기화/설정 조회와 cleanup은 성공하지만 dangling helper가 남아 recursive runtime scanner는 계속 거절했다. 로그인 잠금과 별개이며 재로그인/재부팅을 요구하지 않는다. 실제 임시 artifact의 보존 복구는 아직 실행하지 않았다.
- **plugins 실행 입장 갱신(2026-09-29)**: blanket 존재 거절 대신 hooks/apps/plugins=false의 명시 argv·effective/sessionFlags 검사 + 기존 전체 profile 재귀 scanner + macOS PROFILE/plugins read/write deny를 결합했다. OS 캐시 차단 probe·고정 CLI offline config·앱 프로필5 RPC가 통과했다. 실제 tmp helper symlink와 네트워크/도구·관리 설정 전체 입장은 별도다. toy 실행 회귀의 cleanup 실패 2회는 재검사에서 재현되지 않았으나 원인 미확정으로 남긴다. 이전 plugins 존재 거절 설명은 당시 임시 조건이며 현행은 architecture를 따른다.
- **native dispatcher 소유권 연결(2026-09-29)**: 내부 등록 목록을 공통 poll/취소/quit에 연결해 임시 프로세스의 정상·취소·종료·DB 실패 복구를 통과했다. 준비 상태는 wire running으로 바꾸지 않으며 native start/capability는 미활성이다. 이 파동의 sidecar 라이브러리 누적 결과는 421 PASS/9 ignored/729.68초이며 과거 fixture의 id50 누락도 수정됐다. 실제 공급자 활성화·격리·모델 통신/도구 UI와 혼동하지 않는다.
- **앱 프로필 조회 최종 통과(2026-09-29)**: plugins 캐시를 로그인/조회에서 검증해 허용하고 native 실행에서는 명시 거절하도록 분리했다. 기존 v2 종료 증거 API로 복구한 뒤 실제 5 RPC preflight가 matched/termination/cleanup 모두 true로 통과했다. 아래 과거 plugins 잠금/requirements 미확인 상태는 해소됐다. 이 결과는 실제 작업 실행 활성화나 plugin 격리 증명이 아니며 runtime blocked는 유지된다.
- **계정 preflight 진전/새 프로필 경계(2026-09-29)**: 계정 응답 전 account/updated+emittedAtMs를 bounded 단1회 처리해 실제 account/model 대조를 통과했다. 마지막 requirements 단계 뒤 vendor plugins 생성으로 profile cleanup이 실패했다. 새 pending에는 locked-v2 종료 증거가 있으므로 legacy/reboot 이슈로 회귀하지 않는다. 플러그인 자동 생성/격리와 프로필 admission이 다음 조사이며 현재 plugins/pending은 보존한다.
- **legacy 잠금 수동 복구 완료(2026-09-29)**: 사용자 명시 승인으로 개발 앱을 정상 종료하고 exact legacy pending을 원본 inode/hash 그대로 `.preserved-20260929-168613811`로 보존했다. 재부팅은 하지 않았다. 후속 실제 model-free 검사에서는 대조 실패가 남았지만 프로세스 종료·cleanup 모두 confirmed이며 활성 pending은 없다. 과거 재부팅/승인 대기를 현재 차단으로 반복하지 않는다. 다음 원인은 preflight 응답의 단계별 불일치이며 인증 실패라고 아직 단정하지 않는다.
- **관리 정책 조회 후보(2026-09-29)**: full preflight는 고정 configRequirements/read(id 50)의 explicit requirements:null까지 받아야 observations가 일치한다. 객체/누락/중복/미지 응답은 거절하며 configuration-only는 기존 2 RPC를 유지한다. 이는 공식 vendor 보고 검증이며 inaccessible MDM visibility/실제 계정 검사 성공/제품 admission 증거는 아니다. 현행 경계는 architecture의 관리 정책 조회 항목에 기록한다.
- **종료/잠금 진단 분리(2026-09-29)**: ProcessClosureEvidence.termination_confirmed는 추적한 프로세스 종료/reap와 증거 저장까지 확인한 진단값이다. 프로필·예약 검증까지 통과한 confirmed를 대신해 실행/잠금 해제 권한으로 쓰지 않는다. 과거 pending은 이 증거가 없으므로 PID가 없다는 사실로 confirmed를 재구성하지 않는다.
- **로그인 종료 표시(2026-09-29)**: 새 로그인/조회는 clean 종료 뒤 profile 재검증 전에 pending 마지막 줄에 login-processes-terminated-v1을 fsync한다. native run의 terminal 보류에는 쓰지 않으며 표시만으로 자동 해제하지 않는다. legacy pending에 소급해 표시를 붙이지 않는다.
- **명시적 복구 갱신(2026-09-29)**: 후속 구현은 flock owner와 locked-v2 종료 표시만 복구 대상으로 삼는다. v1 표시도 자동 승격하지 않는다. UI recover는 제한된 기록/메타데이터·OS Gone·capability 검증 후만 해제하며, 실제 현재 legacy pending은 여전히 미해결이다.
- **현재 legacy 잠금 복구 체크포인트(2026-09-29)**: 동일 날짜의 재부팅 전 증거 워크로그에 boot UUID와 pending inode/시간/hash를 보존했다. 다른 부팅 세션과 정확한 파일 신원 재확인 전에는 해제하지 않는다. 사용자에게 Mac 재부팅을 요청했으며 아직 완료 증거는 없다.
- **실제 offline 설정 호환성 PASS, MDM 검증은 미완료(2026-09-29)**: 현재 exact CLI/helper의 config-only 검사는 기존 정책에서 `Failed to synchronize managed preferences`로 실패했으나 opendirectoryd.libinfo Mach 조회 한 항목 추가로 PASS/2.49초다. 다른 설정/Mach/공유 메모리 허용은 제거했고 config 원문 불변·auth 미생성·cleanup confirmed를 확인했다. 독립 검토는 이 변경에 scoped PASS이나, inaccessible MDM layer가 upstream에서 생략되는 false PASS 위험은 닫히지 않았다. 관리 정책 가시성/명시적 거절 확인 전 제품 admission 근거로 사용하지 않으며 runtime은 blocked 유지다.
- **종료 대기와 완료 응답 분리(2026-09-28)**: native 예약 실행은 통신 결과를 보관하고 poll_finish로 종료를 단계 진행한다. 대기 중 receipt/final/terminal/fence 해제는 없고 완료 후 기존 원자 저장 순서를 유지한다. run.cancelAgent 결과는 종료 증거를 요구하므로 취소 접수 flag로 대신할 수 없다. 공통 stdio의 지연 완료 응답을 fixture runtime에 연결해 검증했으며 native owner 등록은 남는다. 비정상 Drop fallback은 아직 동기식이다.
- **코딩용 OS 쓰기 경계 후보(2026-09-28)**: 같은 deny-default builder의 Write 모드는 작업 사본 일반 파일을 허용하되 Git/agent 제어 경로와 profile config를 보호한다. CLI/helper는 HOME/작업 사본 밖이어야 한다. 저장된 역할이 native argv·OS 모드·preflight 기대 권한을 함께 정하며 별도 renderer 인자는 없다. writer terminal은 run/chat/잠금을 원자 기록하고 불확실 종료/저장 실패는 잠금을 유지한다. 실제 공급자·도구/승인·네트워크 입장을 대신하지 않는다.
- **Codex 실행 profile 입장(2026-09-28)**: 기존 snapshot scanner를 runtime HOME 전체에 재사용해 spawn 전·release 직전 중첩 파일의 nlink=1·실제 directory 조건을 확인한다. 로그인 규칙 자체를 강화해 기존 연결을 깨뜨리지 않으며 파일/인증 내용은 변경하지 않는다. OS sandbox와 적대적 동시 교체 보호는 별도다.
- **deny-default 로컬 정책 후보(2026-09-28)**: opt-in probe에서 지정 폴더 쓰기/읽기·자식 실행은 통과, 원본/보호 설정 변경과 외부 hardlink 생성·loopback TCP는 거절됐다. 시스템 로더가 루트 디렉터리 자체 읽기를 요구해 `(literal "/")`만 추가했다. broad root 읽기/쓰기 허용이 아니며 실제 vendor/Mach/인증/모델 통신 호환성은 미검증이다. 테스트 정책만 존재하며 제안서 §6.8을 따른다.
- **외부 경로 sandbox만으로 원본 보호 불충분(2026-09-28)**: 임시 fixture의 macOS `macos_sandbox_probe`에서 기존 writable descriptor와 hardlink로 sandbox 밖 원본 변경을 재현했다. 직접/심볼릭 링크/자손 쓰기와 실행 중 hardlink 생성은 차단됐다. probe exit 0은 기제 관측 완료이지 안전 gate PASS가 아니며 제품에 적용된 정책이 아니다. 기존 핸들·inode 공유·IPC 경계가 포함된 입장/실행 정책이 필요하다. 제안서 §6.8 참조.
- **공유 vendor gate의 fd 상속 차단(2026-09-28)**: 고정 셸이 payload release 전 0/1/2 외 inherited fd를 닫는다. 임시 원본의 non-CLOEXEC fd 누출을 먼저 재현하고 낮은/높은 fd 및 부모 보존·기존 로그인/실행 회귀·독립 검토로 수정 확인했다. hardlink 및 전체 sandbox 보장으로 확대 해석하지 않는다.
- **실제 실행 원장 통합 파동 검토 통과(2026-09-28, 기준 `e738340`)**: 공통 RunPreparation이 메시지/역할/snapshot 해석과 run/recovery/writer 예약을 담당한다. RunReservation은 gate 전 신원 저장·release 기록·누적 자손 갱신과 예약 입력의 text 통신·공개 원장·explicit 종료 증거에 따른 terminal/복구 저장을 연결한다. 임시 snapshot/SQLite/toy pipe와 DB 재오픈 회귀 및 독립 정적 검토 PASS다. 누적 라이브러리 증거와 후속 변경의 집중 회귀를 조합하며 전체 최신 소스 누적 재실행으로 주장하지 않는다. native provider 입장·workspace-write OS 격리·도구/승인·제품 caller는 여전히 미완료다. 가짜 공급자 증거로 실제 provider·유료 모델·제품 활성화를 주장하지 않는다. 상세는 현재 및 월간 archive의 2026-09-28 기록이다.
- **승인은 snapshot 보호를 대체하지 않는다(2026-09-28)**: Codex requestApproval에는 sandbox 밖 실행/추가 쓰기가 포함될 수 있다. cwd·item 신원 일치나 사용자 클릭만으로 원본 보호를 증명하지 못한다. 현재 미활성 후보의 AcceptOnce builder는 거절하도록 수정했고 decline/cancel만 준비한다. workspace-write의 독립 권한 검증을 먼저 구현하며 관리 정책 우회/원본 직접 쓰기 경로로 대체하지 않는다.
- **Codex 격리 미입증 조건(2026-09-28)**: HOME/CODEX_HOME 분리와 CLI 모델/권한 override는 시스템/회사 관리 layer 및 전역 hook 배제 증명이 아니다. 내부 full preflight는 includeLayers=true로 정확한 flags/user/빈 system 출처와 hooks=false를 대조한다(default RPC는 false 유지). 조회는 startup 부작용 이후의 관측이다. 빈 임시 환경의 무모델 thread 실측과 미신뢰 hook/MCP 표시 미생성은 통과했지만 신뢰/관리/plugin 및 프로젝트 입장 격리는 별도 남는다. 관리 정책을 우회하지 않는다. remoteControl/status/changed는 exact opt-out으로 억제하고 예상 밖 수신 frame은 계속 거절한다.
- **최신 로그인 복원(2026-09-28)**: Codex 0.158.0-alpha.2.1의 signed nested native CLI와 helper exact pair를 직접 실행해 Claude 2.1.275와 함께 실제 앱 재시작 복원을 확인했다. `codex-cli/bin/codex`는 shell wrapper이므로 native 신뢰 판정과 혼동하지 않는다. 아래 과거 항목의 Claude 복원 대기는 해소됐다. 실제 모델 실행/출력은 여전히 별도다.
- **Claude 사용자 인증 통과(2026-09-15)**: 키체인 HOME 수정 후 사용자가 연결 성공을 확인했다. 아래 이전 항목의 사용자 인증 완료 대기는 해소됐으며 Claude 재시작 복원·독립 안전 검토는 남는다. 사용자 보고와 직접 화면 관찰은 구분한다.
- **로그인 환경·업데이트 함정(2026-09-15)**: Claude 앱 전용 HOME에서는 macOS 기본 키체인 조회가 실패하므로 OS HOME을 유지하고 CLAUDE_CONFIG_DIR로 앱 인증 namespace를 분리하도록 수정했다. Codex SHA-256 드리프트 차단은 새 0.154.0-alpha.6.2 exact pair·동일 로그인 규격 검증으로 대응했고 실제 재시작 연결 복원이 통과했다. 향후 미검증 업데이트는 계속 차단하며 로그인 유지 실패·설치 불일치·복구 잠금을 UI에서 구분한다.
- **Claude 연결 코드 전달(2026-09-15)**: 브라우저 반환 코드의 UI/IPC/stdin 경로를 구현했다. 현재 Claude 로그인 시도에 한 번만 제출하고 verifying 이후 fresh 상태 조회·종료 확인으로 성공을 판단한다. 가짜 코드 회귀는 통과했으나 실제 사용자 인증 완료·복원과 독립 안전 검토는 남는다. 이메일 확인 번호가 아니라 마지막 브라우저 화면의 연결 코드를 사용한다.
- **제품 로그인 연결(2026-09-14)**: Codex 0.153.4 exact pinned 쌍의 앱 전용 사용자 인증(`8eb6129`)과 후속 check-only 실제 재시작 복원을 각각 확인했다. Claude 2.1.270은 별도 config/Keychain namespace·manager·fence·버튼을 구현하고 빈 profile 조회/URL 생성·취소를 확인했으나 사용자 인증 완료와 이번 변경의 독립 검토는 남는다. provider별 종료 잠금과 URL allowlist를 분리하고 원래 터미널 인증은 보존한다. 모델 실행 준비 증거가 아니며 cleanup 불확실성은 자동 해제하지 않는다. 상세 현행 구조는 architecture 로그인 항목을 따른다.
- **CLI 버전 드리프트(2026-09-14 후속)**: 현재 ChatGPT.app 번들의 Codex는 0.153.4다. 앞선 0.149.1 관측은 당시 증거이며 현 설치 버전으로 재인용하지 않는다. 로그인 schema 20파일 비교에서 Bedrock 인증 추가 2파일 차이가 있고 chatgpt 분기는 동일하다. `scripts/m5-codex-login-schema.mjs`로 생성 schema와 의존 타입을 오프라인 대조할 수 있지만 결과는 실제 provider/version 활성화 권위가 아니다.
- **로그인 불확실 종료 fence(2026-09-14, 내부 owner 미활성)**: app-data의 `codex-login.pending` 존재는 내용이 비었거나 손상돼도 새 내부 로그인 예약을 차단한다. 자동 삭제/기록 PID 재사용은 하지 않는다. 파일에는 프로세스 신원만 남기며 explicit cleanup의 tree 종료·root 회수·증거 저장 확인 전에는 해제하지 않는다. 제품 launcher/초기화/IPC·수동 복구 연결은 별도 남는다.
- **실제 agent 별도 로그인 승인(2026-09-14)**: 기존 사용자 터미널 설정/인증을 보존하는 앱 전용 환경에 별도 로그인하도록 구현한다. 인증정보 복사·공유 keyring 자동 fallback·유료 모델 smoke는 이 승인에 포함하지 않는다. 로그인 방식 선택은 더 이상 답변 대기가 아니며 실제 구현/검증과 구분한다.
- **압축 계획 해제 후 chat 검증(2026-09-14)**: plan ordinals 1..1000은 release 중 삭제되고 완료 후 anchor 1001만 남는다. release cursor 이하의 직전 행은 exact frozen source에서 검증해야 한다. plan 행 부재를 모든 단계에서 허용하는 fallback은 금지한다. 실제 product admission 실패는 plan_v9 transition 조회였고 구형 closure의 prefix 조회라는 초기 정적 추정은 이 실패 원인으로 확인되지 않았다.
- **편집 초안 복구(2026-09-14)**: 재읽기는 사용자 확인 시의 정확한 tab 객체와 완료 시 프로젝트/작업 복사본·차단 상태를 대조한 뒤에만 초안을 교체한다. 실패하면 보존하고 파일은 쓰지 않는다. Monaco의 프로그램적 내용 동기화도 change 알림을 내므로 같은 텍스트는 dirty로 바꾸지 않는다.
- **2026-09-14 실제 CLI 관측 경계**: Codex 0.149.1의 모델 없는 app-server 초기화/config/account 조회와 Claude 2.1.269 인증 상태 조회는 성공했다. 설정 격리·모델 가용성·actual provider 실행 권위는 아직 증명하지 못했다. 원문 인증/설정은 보존하지 않는다. 최신 후보 계약은 제안서 §6.7이다.
- **2026-09-14 작업대 배치**: 파일·편집기·변경·팀 대화의 가로 순서/폭은 사용자 조절 및 로컬 저장 가능하다. 전체 시안/상하 분할 도킹 완료는 아니며 실행 안전 계약은 그대로다.
- **G6 성능 도구와 실제 완료 경계(2026-09-14)**: `m5:measure:performance`는 공급된 측정 JSON만 판정하고 `m5:measure:sidecar-startup`은 현재 release sidecar의 생성된 빈 DB warm 시작을 직접 측정한다. `d97ee56`에서 10회 shell/storage p95 3.863/52.596ms PASS. `7832348`의 `m5:measure:renderer`는 합성 bridge·실제 Monaco로 대형 파일/diff·500개 페이지 탐색의 10회 측정을 통과했다(파일 표시 p95 75.9ms, 입력 32.3ms). 추가 `f4def74`의 `--memory` 실측은 Electron subtree idle 약 409.73 MiB와 한 회차 tree/diff 약 3.38초로 FAIL이며 원인 미확정이다. packaged startup·실제 stream·전체 제품 메모리·경합·종료 수집이 남아 있어 `m5:test:performance`와 G6는 미완료다. 입력 형식·측정 의미는 `procedures/m5-performance.md`를 따른다.
- **Astra 직접 개발 우선(2026-09-14, 승인 경계 갱신 2026-09-28)**: 메인은 현재 사용자 선택 `gpt-6-astra`와 추론 설정을 따른다. 단일 기능은 직접 구현·검증하며 의무 역할 분할/worktree를 생략할 수 있다. 읽기 전용 독립 검토자 1명과 같은 검토자의 수정 재검토는 상시 승인돼 범위 설명 뒤 바로 호출한다. 구현 위임·추가 검토자 병렬 투입은 별도 승인 전에 대기한다. 과거 의무 위임 규칙보다 AGENTS.md의 `개발 방식 선택`과 운영 절차 §1이 우선한다. 검토 합격·실제 계정/유료 실행·main 병합/push를 자동 승인한 것은 아니다.
- **부분 RSS 재측정(2026-09-14)**: idle에서 테스트용 5 MiB 텍스트 생성을 분리한 `dfe12bd`의 Electron subtree 측정은 idle/peak/60초 잔류 356.70/761.73/110.125 MiB와 GUI 시간 기준 PASS다. 실제 sidecar/agent는 제외하므로 전체 제품 RSS/G6 완료로 사용하지 않는다. 앞선 `f4def74` FAIL 증거도 유지한다.
- **G5D storage lifetime activation 정책(2026-09-08 사용자 승인)**: automatic lossless compaction의 유일한 trigger는 retained boundary 이후 exact safe prefix 1,000 ledger events다. 기존 안전 제외·완전 materialization·exact 전후 동등성이 선행하며 source event/index만 논리 삭제하고 사용자 chat/message/evidence, recovery, workspace, snapshot, baseline, projection, receipt와 물리 cleanup은 보존한다. empty-authority pending만 admission exact 검증 뒤 재개하고 running/non-empty/commit 불명은 변경 없이 fail-closed한다. ready 이후 치명 maintenance 오류는 wire 상태를 추가하지 않고 sidecar 종료로 다음 verifying admission에 넘긴다. 단일 event-loop SQLite writer, ready-only bounded tick, work 100/primary 1 MiB/retained+dependency 8 MiB를 유지한다. V10은 `authorizeDelete`를 read-only witness로 두고 `deletePrefix`의 exact armed interval에서만 완전 authority batch와 source index/event 삭제·authority cleanup·cursor 전진을 한 transaction으로 결속한다. V10 contract fixture만으로 runtime writer 활성화나 readiness를 주장하지 않는다.
- **큰 snapshot 처리와 제품 준비 시간은 별개(2026-09-03)**: 개발 중인 새 전역 경로는 실제 생산 101-workspace DB의 보존 복사본(607 source rows)을 172,685 calls/약 98.288초에 복원·ready 재검증했다. 30,000-call 테스트 guard 실패는 무진행이 아니라 예상 횟수 부족이었다. 이 내부 정확성 관측은 3초 제품 ready 충족이나 startup/admission 활성화가 아니다. 제품 시간·1 MiB fresh/8 MiB retained+fresh/100 work·100 frozen-operation fence는 유지한다. 실패한 생성 테스트와 보존본 replay 성공을 구분하고 원본 DB는 `/tmp/g5d-production-evidence.NX3udd/manifest.md`의 SHA로 고정한다.
- **큰 snapshot 새 경로와 제품 경계(2026-09-03)**: §16 private 동일 연결 전역 검증→index 선행 catch-up→ready coverage는 기존 V1/제품 verifier와 별도다. historical live/frozen workspace를 검증한 뒤 member별 COMMIT하며 durable partial offset 없이 source 처음부터 멱등 재시작한다. 실제 생성101의 보존 DB는 172,685 calls/98.288초에 완료해 3초 제품 목표를 충족하지 못한다. `ReadyVerified`를 제품 admission으로 인용하지 않으며 독립 리뷰·정상 생산 통합 QA와 제품 연결/성능은 별도다.
- **pnpm worktree 설치 재사용 함정(2026-09-03, 09-09 config surface 정정)**: bundled pnpm은 script 실행 전 기본 자동 install을 수행한다. 동일 lockfile이어도 workspace-state의 절대 project 경로가 다르면 shared node_modules symlink를 따라 root 설치본을 재생성할 수 있다. 이를 명시적으로 끌 때 repository `.npmrc`의 kebab-case key는 pnpm 11.25에서 무효였고, `pnpm-workspace.yaml`의 `verifyDepsBeforeRun: false`와 child `pnpm_config_verify_deps_before_run=false`를 함께 쓴다. 손상 시 잠금 파일 그대로 offline frozen install과 필요한 Electron 캐시 복구를 사용하며 네트워크 실패 반복·lock 변경으로 우회하지 않는다.
- **G5D QA evidence의 신뢰 경계(2026-09-09)**: harness evidence의 atomic leaf/inode checkpoint는 trusted local checkout 또는 isolated CI에서 accidental/substitution failure를 fail-closed로 기록하는 보강일 뿐 제품 저장 경계가 아니다. same-UID hostile parent rename race, publication 뒤 변조, signing/remote provenance는 이 helper가 보장하지 않으며 별도 보안/CI provenance 범위다.
- **큰 source catch-up 정책 경계(2026-09-03 승인)**: 기존 protocol batch/workspace source 검사·exact `single_source_payload_oversize == "contractError"`·storage-v2/v3 golden은 개별 source 1 MiB 초과 거부를 유지한다. 사용자는 §16의 새 분할 경로에 한해 호출당 1 MiB fresh/8 MiB retained+fresh/100 work로 큰 기록을 처리하는 확장을 승인했다. 기존 V1 계약을 덮어쓰지 않고 새 경로·중단/재시작 회귀로 구분하며 제품 활성화는 구현·검증 뒤 별도 완료한다.
- **G6 staging 경로(2026-09-03)**: Cargo target 위치는 환경·config·target triple에 따라 달라진다. staging은 build JSON의 해당 manifest/bin executable을 쓰며 root/target 또는 PATH fallback을 하지 않는다. 실제 artifact 없는 성공 exit만으로 이전 binary를 복사하지 않는다.
- **M5 G6 Electron main boundary regression(2026-09-03)**: `main.ts`의 registered callback은 VM-transpiled test로 검증한다. IPC는 current `BrowserWindow.webContents`의 exact main frame와 credential-free configured app/dev origin만 허용하며, rejected sender/frame/origin은 sidecar action 전에 실패한다. `app://m5` asset은 credential/port/query/fragment, encoded dot/slash/backslash, malformed percent, POSIX·Windows absolute path와 missing/non-file를 404로 거부하고 external fetch를 하지 않는다. 이 Node fake 증거는 실제 Electron/packaged renderer security, signing/fuses, Windows device 또는 전체 G6 gate가 아니다.
- **S3 한 행 호환성의 범위(2026-09-03)**: projection은 losing duplicate와 overlay 이전 값까지 문법 검증한 뒤 serde의 마지막 duplicate·루트 typed sequence와 exact-row SQLite의 첫 duplicate/evidence 표현을 각각 보존해야 한다. 긴 projection 숫자의 입력 길이를 S2 canonical 24-byte 출력 한계로 제한하지 않는다. S3의 재개 가능한 숫자 state/최대 33-byte 내부 Number 재구성 및 보존 parsed Number 예산은 이 구분을 지킨다. 한 행 의미/mirror 결과는 글로벌 source/provenance/admission 권위가 아니다.
- **실제 provider wire 재확인(2026-09-03)**: 로컬 Codex 0.149.1/Claude 2.1.258은 현재 fake exact manifest와 다르다. Codex app-server schema의 thread-start 유효 설정은 후보 증거이며 실제 runtime/설정 격리 검증이 아니다. Claude init의 MCP/plugin 객체와 fake `configIsolation` 차이를 별도 adapter에서 해결해야 한다. `--bare`는 구독 OAuth를 사용하지 않으므로 인증 재사용 대체로 임의 적용하지 않는다. 공식 출처·후속 경계는 `proposals/m5-coding-agent-ide.md` §6.7.
- **M5 G6 package-size 증거 도구(2026-09-03)**: `m5-package-metrics.mjs`는 macOS arm64의 unpacked ≤350 MiB와 compressed ≤200 MiB를 supplied directory/regular archive의 exact logical bytes로만 측정한다. symlink target은 세지거나 따라가지 않으며 Electron executable·`app.asar`·ASAR 밖 staged sidecar layout을 요구한다. Windows x64/arm64는 측정값을 내도 threshold 없는 `NOT_READY`다. filesystem walk는 atomic snapshot이 아니므로 metadata change를 발견하면 `NOT_READY`; report도 local-only이며 signing/freshness/runtime/G6 완료 증거가 아니다.
- **S2 canonical source는 의미/admission 증거가 아니다(2026-09-03)**: 새 scanner는 schema bootstrap을 제외한 호출당 fresh main+key 재읽기 1 MiB, retained+fresh 8 MiB, work 100을 유지한다. 전체 key를 보관하지 않고 adjacent source span을 decoded UTF-8 byte로 비교한다. pending decoded byte 뒤의 다음 조각도 함께 읽되 작은 잔여 예산을 필요한 반대편에 예약해야 교대 대량 재읽기/무진전 오류를 피한다. 숫자 24-byte는 현재 serde/zmij canonical 출력 image 근거이며 관대한 projection 입력 상한으로 재사용하면 안 된다. 관련 회귀/실측은 `storage/tests/reopen_canonical.rs`와 S2 evidence manifest에 있다.
- **S2 legacy oracle은 R3A 단독이 아니다(2026-09-03)**: 실제 정상 V8 fixture에서 indexed JSON duplicate의 첫 값이 틀리고 마지막만 맞거나 lock evidence key순서/escape가 한쪽만 다르면 private R3A는 허용해도 legacy SQLite mirror·Storage reopen은 거부한다. 반대로 동일 duplicate/정상 마지막 nonindexed 값/공백/typed sequence overlay는 수락된다. `storage::tests::reopen_oracles::s2_legacy_oracle*` 6개가 실제 DB 43사례를 고정하므로 S2 의미 동등성은 serde map과 SQLite mirror를 함께 보존해야 한다.
- **R3C-S1 구현 통합(2026-09-03)**: `00f54d9`의 private `RawColumnSession`을 관련 검증·단일 독립 리뷰 뒤 반영했다. 호출당 조각 1 MiB/retained+fresh encoded 입력 8 MiB로 읽고 UTF-8·길이·SHA만 완료한다. canonical/의미/provenance/admission 증거가 아니며 S2 이후 검증과 제품 연결·파동 누적 QA는 남는다. 현행 구조는 `architecture.md`, 원본 검증·소스 동일성은 `/tmp/g5d-r3cs1-evidence.ympdQJ/`를 확인한다.
- **간소화 운영 확정(2026-09-03 사용자 승인)**: 기능/결함 단위의 관련 검사와 독립 리뷰 1명을 기본으로 하고 누적 QA·최종 gate는 최신 통합 파동 마감에 수행한다. 수정 후에는 영향 범위만 재검증하며 신규 공용 계약 없는 helper마다 별도 설계 심의·전체 검증을 요구하지 않는다. 기존 계약의 중복 빈도/담당 배치만 대체하며 필수 안전 시나리오와 제품 활성화/출시 게이트는 유지한다. 상세는 `procedures/multi-agent-orchestration.md` §1.1이다.
- **Cargo 테스트 실행 파일 경로(2026-09-03)**: shared `CARGO_TARGET_DIR`에서 repo-relative `target/debug` 가정은 process-gate 19개를 실행 전 실패시켰다. test executable의 `<target>/<profile>/deps` 부모에서 sidecar를 찾고 `--lib` 테스트 전 같은 worktree/target/profile의 명시적 bin build로 freshness를 확인한다. 경로 수리 뒤 원래 실패 19개와 추가 locator 회귀가 통과했다. 이는 S1·전체 G5D 완료와 구분한다.
- **QA 저부하 즉시 적용(2026-09-02 사용자 승인)**: 고부하 Cargo/sidecar 테스트를 사용자 요청으로 종료했고 다음 실행은 하네스 및 직접 Cargo 모두 compile/test 병렬 최대 4로 제한한다. 무거운 검증은 컴퓨터당 하나만 실행한다. focused 회귀와 통합 마감 누적 gate를 구분하되 필수 안전 검증은 생략하지 않는다. 낮은 우선순위는 best-effort이며 CPU 절대 상한은 아니다. 자세한 실행 규칙은 `procedures/multi-agent-orchestration.md` §13.1이다.
- **S2 oracle 분리(2026-09-02)**: legacy product는 canonical source/replay, serde projection 의미 비교, SQLite mirror를 모두 요구한다. R3A는 parsed final map/evidence를 비교해 첫 duplicate를 보는 SQLite 추출과 evidence `json(...)` 표현 비교를 그대로 재현하지 않는다. primitive 차이와 source 대조는 했지만 정상 V8 fixture의 전체 acceptance 재현은 아직이다. 기존 제품 검사는 유지하며 §13의 세 oracle·후속 회귀를 닫기 전 R3A 단독 결과를 제품 동등성으로 인용하지 않는다.
- **Cargo test fixture binary 경로(2026-09-03)**: `chat::tests`가 repo-relative `target/debug`를 가정하면 shared `CARGO_TARGET_DIR`에서 process-gate 19개가 실행 전 실패한다. test executable의 `<target>/<profile>/deps` 부모에서 동일 profile의 sidecar만 찾고, `--lib` 검증 전 같은 worktree/target/profile의 명시적 bin build로 freshness를 확인한다. 관련 수리는 테스트 전용이며 기존 S1 미완료/full exit 143과 구분한다.
- **R3C-S1 raw bytes는 의미 검증이 아니다(2026-09-02)**: 새 private bytes session은 고정 source 주소/expected identity/epoch/byte 길이 아래 fragment별 UTF-8와 SHA를 끝까지 계산한다. ≤1 MiB fresh fragment + ≤8 MiB retained/fresh encoded 입력이며 start source metadata도 예산에 포함한다. schema bootstrap은 같은 BEGIN 안의 별도 비용이고 SQLite slicing 내부 메모리/전체 ready 시간 상한은 미입증이다. 빈 TEXT의 SQLite substr NULL과 retained UTF-8 carry 때문에 다음 호출이 무진행이 되는 경계를 회귀로 고정했다. §10 큰 정상 run source의 raw transport 성공은 기존 R3B 거부 해제나 canonical/admission 성공이 아니므로 S2 및 제품 연결 계약을 생략하지 않는다.
- **R3C 큰 기록 처리 순서(2026-09-02)**: V8 byte transport → canonical oracle 동등성 → projection/파생 의미 비교 → 동일 연결의 쓰기 권위 순서로 나눈다. §12 S1은 fragment 1 MiB/retained+fresh 8 MiB 내 UTF-8·길이·해시 정보만 제공하며 기존 verifier·제품 fence·입력 정책을 바꾸지 않는다. historical workspace catch-up은 source sequence 없는 row 때문에 단순 latest-only 조건 변경으로 해결하지 않는다. 상세 계약은 `proposals/m5-storage-reopen-verification.md` §11–12다.
- **개발 worktree 정리(2026-09-02)**: 완료 기능의 통합·최종 QA/gate 직후, 다음 worktree 생성 전에 정리를 점검한다. branch SHA/통합 동등성·dirty/untracked/ignored 증거·사용 process를 확인하고 필요한 증거를 외부 보존한 뒤 승인 범위에서 비강제 제거한다. branch/commit은 보존하며 진행·미확인 작업은 유지한다. 최초 정리에서 58개를 제거하고 18개를 유지했다. 상세는 `procedures/multi-agent-orchestration.md` §5.1과 `.artifacts/worktree-cleanup/2026-09-02/approved-plan.json`이다.
- **R3C snapshot 호환성 재현(2026-09-02, 테스트 전용)**: 정상 public text/event/누적 snapshot 예산을 통과한 ASCII 16 × 64 KiB 및 역슬래시 9 × 64 KiB의 일반 run+chat atomic writes와 Storage reopen은 성공하지만, 저장된 run canonical BLOB이 1 MiB를 초과해 전체 private R3B ledger 검증은 거부한다. 작은 16-event 대조군은 전체 완료한다. exact test `agent_runtime::budget_tests::budget_valid_actual_run_snapshot_exceeds_reopen_primary`가 양 경로와 exact source row byte preflight/무변경을 구분해 고정한다. provider-less pre-G5C readable shape일 뿐 현재 provider 검증이나 제품 admission 증거가 아니며, §9/§10의 저장 표현·검증 단위·기존 기록 호환 정책을 정하기 전 제품 연결/예산 변경은 하지 않는다.
- **R3C 제품 연결 전 호환성(2026-09-02)**: 기존 actual-agent의 정상 public text 최대량과 metadata를 담은 단일 run snapshot은 private verifier의 1 MiB primary cap을 넘을 수 있다. project의 prepared-workspace map도 abandoned를 보존하며 전체 개수/byte cap 없이 누적되는 별도 정상 생산 경로다. 또한 pending의 raw-source 읽기 검증은 chat mutation의 indexed predecessor 요구를 대체하지 않는다. snapshot/이전 기록 호환과 검증 후 index catch-up·복구 순서를 별도 계약/fixture로 닫기 전 제품 admission을 활성화하거나 기존 100-operation fence를 해제하지 않는다. §10의 run persistence/Storage reopen/private R3B 재현은 `73b2d61`의 독립 통합 QA와 최종 gate까지 통과했다. project 경계 재현·Sidecar startup/recovery·제품 활성화는 미실행이다. 상세는 `proposals/m5-storage-reopen-verification.md` §9다.
- **R3B 후속 호환·유일성(2026-09-02)**: pending에서 index/normalized message가 없더라도 메시지 도입 ID 전역 유일성은 기존 protocol 요구다. 이를 포함한 §8 내부 R3B는 `e5f9f01`에서 독립 통합 QA·최종 gate를 완료했다. historical pending workspace의 읽기 검증과 실제 catch-up은 별개이며 현행 bounded workspace materializer의 갱신 전 latest-only source 대조는 정상 과거 row도 거부할 수 있다. 이 미해결 catch-up과 R3C 호환성은 `proposals/m5-storage-reopen-verification.md` §7.5·§9, 테스트 전용 재현 계약은 §10을 따른다.
- **pnpm 복원과 누적 검증 환경(2026-09-02)**: stale node_modules metadata만으로 설치 성공을 판단하지 않는다. frozen lockfile/ignore-scripts/공급망 정책을 유지한 복원 뒤 하네스와 동일한 `CI=1`로 확인한 build·누적 `g5d-bounded-storage`가 `d4c3751`·`63ab510`에서 PASS했다. 격리 환경의 Electron runtime fetch 실패와 더 넓은 core/protocol lint 실패는 scoped 성공과 별도 기록한다. 자동 install/CI 정규화 차이는 관찰했지만 package 소실 원인을 전부 입증한 것은 아니므로 임의 설정 변경이나 정책 완화로 해결했다고 주장하지 않는다.
- **R3B 내부 derived-record 경계(2026-09-02)**: private 정보성 mode는 R2/R3A 뒤 전체 derived table과 ready 양방향 coverage를 검증한다. index-empty pending receipt/baseline은 raw tuple을 먼저 찾고, indexed 여부와 무관하게 모든 live introduction을 frozen identity와 비교한다. 추가 global raw scan은 unindexed introduction에만 필요하다. historical pending workspace 수락은 기존 catch-up 결함 해결이 아니다. retained outer+fresh dependency가 새 호출에도 8 MiB를 넘으면 terminal failure지만 이번 호출의 이미 소비한 입력 때문에 넘는 경우는 Pending일 수 있다. 실패 batch 안에서 검증한 행과 COMMIT된 진행을 테스트에서 혼동하지 않는다.
- **R3A 내부 ledger/projection 경계(2026-09-02)**: compacted DB는 R2 closure와 R3A를 같은 세션/변경 감지 epoch에서 검증한다. pending의 누락 source index는 exact live predecessor에서 필요한 사실을 구성해 허용하지만 존재하는 index는 항상 결속하며 ready의 누락은 거부한다. latest source와 다섯 projection의 양방향 일치는 global derived-record/R3B 또는 startup/admission/R3C 완료 증거가 아니다. phase-local typed app-instance 1 MiB는 canonical JSON source 1 MiB와 동일한 크기 정의가 아니므로 용량 positive-control을 혼동하지 않는다.
- **R3 startup/admission의 추가 경계(2026-09-02)**: pending materialization은 일부 파생 기록 미생성이 정상이며 ready는 검증 의무이지 proof가 아니다. 현행 startup의 복구 쓰기보다 앞선 read-only semantic preflight가 필요하다. SQLite 전체 integrity/FK 검사와 Electron ready 3초는 batch 예산과 별개다. 둘을 몰래 완화하지 않고 R3A/B/C와 사용자 결정이 필요한 변경을 `proposals/m5-storage-reopen-verification.md` §7에 분리한다.
- **R2 내부 closure 구현 경계(2026-09-02)**: `start_compaction_closure`의 3-phase 결과는 admission이 아니며 product >100 거부를 해제하지 않는다. 각 행의 고정 dependency graph 전체를 먼저 byte-preflight하므로 뒤늦은 oversized dependency도 무한 Pending으로 남지 않는다. TeamMessage 64 KiB text cap과 메시지 단계의 1 MiB 합계 예산을 혼동하지 않는다. 후기 corruption fixture는 frozen message 삽입 순서를 source rowid로 고정해야 실제 100행 이후 위치를 보장할 수 있다.
- **R2 내부 검증 구현·범위(2026-09-02)**: `9327bcd`의 같은 connection 세션이 operations→prefixes→frozen messages를 다회 처리한다. 삭제 frozen source↔정확한 sequence prefix/baseline, 삭제 chat introduction↔frozen message↔해당 normalized message가 대상이다. 참조되지 않는 정상 historical baseline을 금지하지 않으며 전체 derived-table replay와 제품 admission은 R3에 남긴다. chat의 동일 durable sequence는 null-or-same snapshot이 허용된다는 protocol 규칙을 보존한다. 상세 계약은 `proposals/m5-storage-reopen-verification.md` §6, 현행 구현은 `architecture.md`를 확인한다.
- **V8 operation 검증 전구체는 admission이 아니다(2026-09-02)**: `storage::reopen` 내부 세션은 mutable connection borrow + 매 batch의 transaction-under-lock `data_version` 비교로 frozen operation만 다회 검증한다(100행·canonical 1 MiB·별도 raw closure 8 MiB). 완료·취소·실패는 재사용하지 않으며 durable cursor/proof를 쓰지 않는다. prefix/message/replay/integrity와 product activation은 별도 게이트이고 제품의 100개 초과 재오픈 거부는 유지한다. 같은 프로세스라도 별도 SQLite 연결의 commit은 외부 변경이며 production exclusive-lock 정책을 느슨하게 바꾸지 않는다.
- **M5 G5D storage lifetime(2026-09-01, exact schema v8)**: 실제 provider 활성화 전 SQLite 전체 수명 stop condition을 G6에서 분리해 G5D로 진행한다. exact predecessor schema를 mutation 전에 검증하는 additive v1→v8 chain은 원장과 기존 compaction history를 보존한다. V6는 source event에 exact 결속된 append-only `event_source_index`와 16단계 lifetime cursor를 두고, V7/V8은 future compaction prefix·message·operation provenance와 sidecar-private completion proof를 추가한다. 명시 호출형 worker는 모든 source/table scan을 rowid keyset `LIMIT 100`, canonical payload 합계 1 MiB로 제한하며 cancellation·commit 불확실성을 fail-closed로 보존한다. terminal compacted V8 read/replay는 frozen operation과 receipt·baseline·retained anchor를 교차 검증해 duplicate/conflict·chat predecessor·후속 append/materializer 의미를 유지하지만, 현재 frozen operation 100개까지만 재오픈하고 101번째는 무변경 거부한다. 실제 compaction writer/delete, 100개 초과 durable reopen proof, startup/background scheduling, 제품 wire/status, checkpoint/VACUUM, cleanup·retention threshold는 아직 비활성이다
- **M5 G5C actual recovery 원자성(2026-08-27)**: recovery ID는 run 준비 시 writer lock과 같은 transaction에 random UUIDv4로 예약해 모든 후속 recovery가 재사용하며 run ID에서 파생하지 않는다. process identity 없는 재시작은 `processIdentityUnavailable` unresolved recovery와 poisoned lock을 공개하되 resolve로 폐기할 수 없다. actual abandon은 SQLite intent → exact/idempotent manifest abandon → manifest 재검증 뒤 atomic DB finalize의 restart-reconcilable saga이고, 어느 단계든 identity/digest가 다르거나 상태가 불명확하면 writer를 해제하지 않는다. SQLite와 filesystem의 원자성을 주장하지 않는다
- **M5 G5C provider 활성화 전 계약(2026-08-27)**: provider manifest는 executable binary만이 아니라 필수 helper까지 포함한 closure digest, exact model/reasoning/permission tuple, provider별 auth/event parser ID, managed cwd·no-secret env·no-fallback policy를 canonical digest로 묶는다. persisted discovery에는 raw stdout/stderr·token·prompt·email·org·raw auth hash를 넣지 않으며, attestation은 accepted run/profile의 run·fingerprint·workspace·runtime·model·reasoning·permission을 exact 결속하고 derivation이 정의되지 않은 session digest는 저장하지 않는다. Claude는 `system:init`의 version/model/cwd/permission/tools/MCP/plugins/config를 전부 handshake·runtime attestation으로 검증한 뒤에만 public event를 허용하고, internal Public도 기존 G5B 256-event·1 MiB text budget 안에서만 허용한다. Codex가 argv 또는 unknown catalog만 제공하면 `canStart=false`다. 이 계약은 fake-only internal boundary이고 공개 G5/G5A/G5B wire·V4/V5를 바꾸지 않는다. SQLite 전체 lifetime/retention/compaction은 실제 provider 활성화 전 별도 stop condition이다
- **로그인 자동 감지**: 브라우저 세션이 있으면 확인 코드 없이 자동 승인으로 끝나는 경로가 있어, 대기 중 `auth status` 폴링으로 완료를 잡는다
- **Codex 데스크톱 앱**: CLI가 앱 번들 `Contents/Resources/codex`에 번들(셸 PATH에 없음). 앱 로그인 자격증명을 CLI가 공유 → 감지되면 로그인도 건너뜀. **벤더가 앱을 옮김**: OpenAI가 Codex.app을 ChatGPT.app으로 통합(2026-07) → `/Applications/{ChatGPT,Codex}.app/...` 여러 후보로 감지(단일 경로 하드코딩 금지). 앞으로도 바뀔 수 있으니 후보 추가로 대응
- **편집기 확장 ID**: Claude Code = `anthropic.claude-code`, Codex = `openai.chatgpt`. 편집기 CLI는 앱 번들 `Contents/Resources/app/bin/{cursor,code}`
- **Windows PowerShell 함정**: 부모가 pwsh 7이면 `PSModulePath` 오염으로 5.1이 기본 cmdlet을 못 찾음 → 자식 실행 시 `PSModulePath` 제거
- **코드 서명 미결정**: 베타는 미서명 + "확인 없이 열기" 안내, 정식은 서명·공증 필요(연 $250~400). 미서명이어도 설치는 가능(사용자가 보안 허용 한 번)
- **스코프**: Codex는 원래 2단계였으나 조기 편입(2026-07-16). Gemini 등 추가는 여전히 2단계
- **UI**: Pretendard + 중성 배경 + 코랄 액센트. 갈색 종이 콘셉트는 사용자가 반려함
- **i18n**: 라이브러리 없이 경량 자체 방식(`src/i18n.tsx` Context/훅 + `src/locales/{ko,en}.ts`). 헤더 언어 토글, 시스템 언어 초기값 + localStorage. 이름 `Hello, Agent`는 언어 무관 고정, UI 문구만 전환. 새 문구는 ko/en 양쪽에 키를 추가해야 함(en은 `Record<MessageKey,string>`으로 누락 시 컴파일 에러)
- **오류/i18n 경계**: Rust 오류는 `AppError { kind, detail }`로 구조화하고 상세 기술 오류는 영어로 통일. 초보자용 설명은 프론트 i18n에서 표시. 프로젝트 안전 안내와 첫 대화 프롬프트도 선택 언어를 Rust로 전달해 한·영 생성
- **릴리스 게이트**: `.github/workflows/ci.yml`에서 데스크톱 macOS·Windows 빌드/테스트/격리 설치와 소개 웹사이트 빌드·렌더·린트를 검증. 사람 확인 항목과 미서명 베타/정식 서명 기준은 `docs/release-checklist.md`를 단일 체크리스트로 사용
- **M5 설계 방향(2026-08-24, G4 격리 방식 2026-08-25 갱신)**: 기존 M0~M4는 최초 설정·튜토리얼로 유지하고, 이후 여러 영속 채팅, 프로젝트별 역할·에이전트·모델, 원본과 분리된 standalone snapshot, 명확한 diff와 실제 코드 편집을 갖춘 초보자용 agent-first IDE로 확장한다. 한 채팅은 변경 목표·작업 복사본 하나를 소유하고 역할 왕복을 한 팀 대화에 기록한다. **M5 기술 기준선은 Electron + React/TypeScript + Monaco 셸과 기존 Rust 안전 코어 sidecar**이며 Tauri와 동시 구현하지 않는다. SQLite 이벤트 원장+projection, 앱 종료 시 안전 중단, 첫 적용은 전체 작업 복사본+복원 지점, macOS 우선 실행을 기준으로 한다. 새 이름은 개발과 분리해 보류한다. 통합 브랜치는 `m5/coding-agent-ide`; `main` 병합은 전체 완료 뒤 사용자 승인 사항
- **M5 직접 유사 제품 Cebus 발견(2026-08-24)**: `cebus-ai/cebus`는 여러 AI 모델의 단일 대화, 역할별 모델 배정, 채널·스레드, CLI·웹·VS Code 표면을 이미 제공해 M5 구상과 직접 겹친다. M5 벤치마크에 포함하고, 초보자 안전·원본과 분리된 standalone snapshot·명시적 diff 검토와 복구 루프를 차별점으로 검증한다
- **멀티 에이전트 저장소 운영(2026-08-25)**: 큰 구현은 오케스트레이터 1명이 통합 브랜치를 소유하고, 최대 3명의 쓰기 구현 담당이 별도 branch/worktree와 겹치지 않는 파일 소유권으로 작업한다. 계약을 먼저 고정한 뒤 파동식으로 병렬화하며, 독립 코드 리뷰와 최신 통합 commit QA를 통과한 변경만 통합한다. Codex 프로젝트 역할·모델·sandbox는 `.codex/config.toml`과 `.codex/agents/*.toml`, 상세 실행 절차는 `docs/procedures/multi-agent-orchestration.md`가 기준이다
- **M5 기계 검증 하네스(2026-08-25)**: `scripts/m5-harness.mjs`가 로컬과 macOS/Windows CI의 단일 검증 진입점이다. `baseline`은 기존 회귀를 실제 실행하고 G1~G6는 앞 단계를 누적한다. 필수 script가 없으면 `NOT_READY`/exit 2로 실패하며, versioned JSON 증거를 `.artifacts/m5-harness/`에 남긴다. 예약 script와 판정 규칙은 `docs/procedures/multi-agent-orchestration.md` §13이 기준이다
- **M5 G5 실행·복구 계약(2026-08-26)**: chat 생성은 verified active standalone snapshot과 전역 1:1 worktree/workspace binding을 요구한다. fixture run은 macOS에서만 exact chat role snapshot으로 시작하며 planning/review는 read-only, coding만 writer lock을 갖고, 첫 버전은 동일 chat/worktree/workspace의 역할 run을 하나씩 직렬 실행한다. `stopping`을 포함한 chat 상태는 명시적 전이표를 따르며 completed/canceled/discarded/recoveryRequired는 terminal이다. `recoveryRequired`인 chat/run/workspace lock/unresolved recovery는 모든 새 run을 막고, process identity 소멸 증명 뒤 abandon한 snapshot/workspace/chat은 재사용하지 않는다. protocol framing은 v1을 유지하고 공개 bridge는 additive `M5BridgeV3`다
- **M5 G1 Rust core 추출(2026-08-25)**: 루트 Cargo workspace와 단일 `Cargo.lock`을 기준으로 삼고, `crates/hello-agent-core`에 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약을 Tauri/Electron/Node/SQLite/UI 의존 없이 옮겼다. 기존 Tauri 모듈 경로는 좁은 re-export shim으로 유지해 `handoff.rs`·`orchestration.rs` 동작을 바꾸지 않는다. `crates/hello-agent-sidecar`는 후속 protocol/SQLite 게이트 전까지 compile-only 골격이다
- **M5 커밋·통합 자동 승인(2026-08-25)**: 작업 계약 범위 안의 기능 worktree 원자적 구현·테스트·리뷰 수정 커밋과, 독립 리뷰 및 해당 하네스·QA를 통과한 commit의 `m5/coding-agent-ide` 통합은 사용자에게 매번 다시 묻지 않는다. push, `main` 병합, 범위 밖 변경, 안전 불변식·확정 결정 변경, 실제 유료 에이전트·사용자 프로젝트·파괴적 시험과 외부 게시는 계속 별도 승인 사항이다
- **M5 G1 공급망 차단(2026-08-25)**: Electron Forge stable `7.11.2`가 Git SHA 기반 `@electron/node-gyp`을 전이 의존성으로 요구해 pnpm 11의 기본 `blockExoticSubdeps`에 차단된다. 전역 예외는 안전 계약 변경이므로 자동 적용하지 않았고, Electron scaffold는 별도 worktree의 미커밋 상태로 보존했다
- **M5 G1 Electron Forge 8 스파이크(2026-08-25)**: Forge `8.0.0-alpha.10`과 모든 Forge plugin을 exact pin, Electron `43.4.0`·Vite `8.2.2`·Fuses `2.1.3`을 사용해 strict pnpm lock과 macOS arm64 ASAR package를 확인했다. Forge 호환을 위한 hoisted layout은 `pnpm-workspace.yaml`의 `nodeLinker: hoisted` 단일 설정이며 공급망 예외가 아니다. Electron `44.0.0`은 release-age 예외를 자동 요구해 쓰지 않았다. renderer는 고정 `app://m5`이고 sandboxed/isolated window와 version-only preload를 유지한다
- **M5 G2 protocol v1 계약(2026-08-25)**: `hello-agent-protocol`과 `src/m5/contracts`가 같은 언어 중립 golden fixture를 사용해 UTF-8 JSONL v1, 1 MiB control line, `{ text }`만 허용하는 256 KiB `blob.chunk`, 실행별 request ID 유일성, JS와 호환되는 `seq` 상한 `2^53-1`, `system.ready` 단일 handshake, `app.prepareQuit`, 구조화 오류와 outcome 불명 시 자동 재시도 금지를 고정한다. 세션의 첫 envelope는 반드시 `system.ready`이며 그 전 request/response/event와 중복 ready는 모두 fail-closed다. 공개 `M5BridgeV1`은 system status·prepare quit·sidecar event 구독만 선언하며 범용 IPC/preload wiring은 없다. SQLite·실제 sidecar 전송은 G3 범위다
- **M5 G3 Rust sidecar·SQLite 경계(2026-08-25)**: production sidecar argv는 `--stdio` 하나로 고정하고 DB 경로·환경 override를 받지 않는다. Electron main이 `userData/com.wonseokhan.helloagent/m5`를 생성·정규화해 spawn cwd로 주면 sidecar도 그 cwd를 canonicalize하고 고정 파일명 `hello-agent.sqlite3`만 붙인다. sidecar는 bundled SQLite 단일 owner이며 모든 연결의 FK 활성화·검증, transactional migration, integrity/replay/recovery를 통과한 뒤에만 ready를 보낸다. stdout은 bounded JSONL protocol 전용이고 pre-ready 오류는 stderr+exit 70이다
- **M5 G3 Electron-sidecar 수명주기(2026-08-25)**: Electron main은 packaged sidecar를 ASAR 밖 고정 경로에서 한 번만 실행하고 3초 ready, bounded framing, instance/response identity, pending 요청을 fail-closed로 관리하며 자동 restart/retry하지 않는다. preload는 `system.status`·`app.prepareQuit`·sidecar event의 고정 channel만 top-level 허용 origin에 노출한다. 정상 종료는 single-flight `app.prepareQuit` 안전 응답 뒤에만 허용하고, G3 capability는 이 두 method뿐이다. 프로젝트·채팅·실행·파일 API는 G4 이후다
- **M5 G4 계약 경계(2026-08-25, standalone snapshot으로 갱신)**: protocol v1의 예약 project/file/diff method·event와 SQLite schema v1은 유지하고, G2 golden fixture와 `M5BridgeV1`은 불변으로 둔 채 project lifecycle fixture와 `M5BridgeV2`를 사용한다. 명시적 canonical Git root, 읽기 전용 source capability, self-contained Git bundle과 raw committed bytes로 만드는 앱 data 아래 detached standalone snapshot+manifest, hash 기반 optimistic save, chunked file/diff, watcher overflow의 `rescanRequired`, dirty draft 보존을 계약으로 고정한다. `project.createWorktree`·`createProjectWorktree`·`worktreeId/path`는 protocol v1 호환 이름일 뿐 linked worktree를 뜻하지 않는다. G4 `project.close`는 물리 삭제가 아니라 watcher 중단+영속 `abandoned`이며 실제 agent/chat/run과 원본 적용은 범위 밖이다. Windows G4는 별도 안전 게이트 전까지 지원하지 않는다
- **M5 G5 계약 경계(2026-08-26)**: protocol framing·SQLite schema v1과 G4 `M5BridgeV2`는 유지하고 additive `M5BridgeV3`를 사용한다. 한 Chat은 명시적 `projectId`·`worktreeId`·`workspaceIdentity`와 1:1이며 생성 시 프로젝트 역할 설정 revision과 `planning/coding/review`의 runtime·정확한 model selector·nullable reasoning·고정 permission을 snapshot한다. public TeamMessage는 hidden reasoning을 저장하지 않고 durable chat event sequence를 protocol stream seq와 분리한다. fixture writer는 run+workspace lock commit 뒤 spawn하며 동일 worktree writer를 직렬화하고, 불확실한 종료는 lock을 유지한 `recoveryRequired`, 해결 v1은 자손 소멸 증명 뒤 abandon-only다. `run.startAgentSmoke`는 예약만 유지하고 capability에 광고하지 않으며 Windows/실제 유료 실행은 별도 게이트 전까지 비활성이다
- **M5 G5A actual-agent 경계(2026-08-27)**: 기존 G5 fixture ledger/handshake는 불변이고 actual 실행은 additive Bridge V4의 별도 `agentRuns` ledger와 typed start/cancel/recovery 계약을 사용한다. renderer는 경로·argv·model·env·cwd·prompt를 주입할 수 없고 sidecar가 immutable chat role snapshot과 SHA-256 capability fingerprint로 exact profile을 해석한다. machine-readable 또는 exact-version 실측 capability가 아니거나 effective model 증거가 `argvOnly`이면 `canStart=false`다. v1은 one-shot/`nextRun`만 허용하며 process identity release barrier, 모든 자손 소멸, permission별 writer-lock 종료, 공개 `status/question/final`, 불확실 종료의 poisoned recovery를 TypeScript/Rust 공용 golden이 fail-closed 검증한다. 전용 `--stdio-agent-fixture`의 fake launcher→normalized `agent.publicEvent`→Electron fixed V4 IPC→renderer durable `chat.get` hydration 수직 흐름은 구현됐고 raw/private carrier와 legacy/actual recovery 원장은 분리된다. 일반 제품 `--stdio`는 exact G5를 유지하므로 실제 Claude Code/Codex CLI·인증·모델 출력은 아직 미연결·미검증이다
- **M5 G5 fixture 수직 흐름 완료(2026-08-26)**: 프로젝트 역할 설정은 revision CAS, 기존 채팅은 생성 당시 role snapshot 불변이다. `workspace.prepareChat`은 매번 새 unbound standalone snapshot을 만들고 `chat.create`가 settings·최초 메시지와 exact preparation을 원자 bind하며, 실패 정리는 네 ID가 모두 맞는 준비만 terminal abandon한다. 모든 채팅 workspace/save는 `chatId/projectId/worktreeId/workspaceIdentity`와 `saveId`를 교차 검증한다. macOS 결정적 fixture의 공개 팀 타임라인, clean/uncertain 취소, process-tree 복구, poisoned lock과 `prepareQuit` blocker가 Electron/preload/UI까지 연결돼 exact `014bd987`의 누적 하네스 PASS·독립 QA `G5_READY`를 받았다. 실제 Claude Code/Codex adapter·인증·사용자 프로젝트·Windows는 검증하지 않았다
- **M5 G4 CLI worktree capability 차단(2026-08-25)**: macOS Git CLI의 `worktree add`는 target이 존재하면 거부하고, held directory의 `/dev/fd/N/.git` 하위 탐색도 `ENOENT`라 검증 뒤 교체 가능한 source·target 경로 문자열 없이 두 filesystem capability를 동시에 묶을 수 없었다. source Git root나 managed target을 ambient path로 넘기면 실행 직전 symlink swap으로 외부 repo filter 실행·admin write 또는 외부 worktree 생성이 가능하다. 이 경계를 수용하거나 사후 검사로 덮지 않는다. 실제 linked worktree를 유지할 native Git 구현과 안전한 detached local clone/snapshot은 제품·안전 계약이 다른 대안이므로 사용자 결정 전 G4 Rust 통합을 중단한다
- **M4 Claude 실행 계약(2026-08-19 실측, CLI `2.1.235`)**: 읽기 전용을 강제하는 것은 **`--tools` 허용목록**뿐이다. `--disallowed-tools`는 거부목록이라 `Task`·`ToolSearch` 등 23개를 남기고 **거부 증거도 안 남긴다**. `--setting-sources project`는 **대상 프로젝트 설정을 로드해** 권한을 `bypassPermissions`로 승격시키고 훅·MCP를 실행시키므로 안전 정책이 아니다 → **`--setting-sources ''`** 를 쓴다. `--safe-mode`는 훅·MCP·스킬만 끄고 **권한 승격은 못 막는다**. `--permission-mode dontAsk`는 자동 승인이 아니라 거부 게이트이며 `permission_denials`로 기계 판독 가능한 증거를 남긴다. **작업 폴더 경계와 사후 빈 diff는 예방책이 아니라 사후 확인 수단**이다(경계는 복제본 *안* 쓰기를 허용한다). 실행 형식은 **`--output-format stream-json --verbose` 고정**이고 `system:init`을 fail-closed로 검증한다(`tools` 집합 일치·`permissionMode`·`mcp_servers`·`plugins`·`model`·`cwd`·`session_id`). ⚠️ **`--json-schema`는 요청하지 않은 `StructuredOutput` 도구를 주입**하므로 기대 도구 집합은 4개다. §7.10 파일럿은 **macOS 전용**(Windows 종료 계약 미실측). 상세는 `proposals/m4-orchestration.md` §7.10.1.2
- **Claude CLI 프로세스 트리·종료(2026-08-19 실측, Codex 2차 검토 반영)**: Bash 도구 자식은 **부모 프로세스 그룹에 없고 별도 세션 리더**(`Ss`)다. 그룹 `TERM`으로 죽는 것은 Claude 자체 정리 동작일 뿐이라, **루트에 `SIGKILL`을 보내면 자손이 남는다**(15초까지 생존 확인). **종료 성공은 신호 결과가 아니라 관측으로만 판정한다** — 한 번이라도 관측한 **`(pid, 시작 시각)` 신원을 누적 추적**하고 **누적 집합 전체의 소멸을 확인**해야 성공이다. ⚠️ **루트 기준 ppid 트리 순회는 오판한다** — 부모가 먼저 죽으면 자손이 `ppid=1`로 재부모화돼 트리에서 사라지고, 살아 있는데 빈 집합으로 보인다(C6로 재현). 열거는 **살아 있는 모든 추적 대상**에서 출발한다. **PID 재사용(신원 불일치)과 조회 실패는 `poisoned`**. `killpg`의 **`EPERM`은 성공도 소멸 증거도 아니다**(좀비만 남은 그룹에서도 난다). **확인 불가는 곧 `poisoned`** — 실행과 복제본을 함께 표시하고 자동 인계·재사용을 차단하며 해소는 명시적 복구/폐기뿐. `run_in_background` 때문에 **정상 종료 경로에서도** 회수가 필요하다. 상세는 `proposals/m4-orchestration.md` §7.10.1.2 ⑤
- **pid는 식별자가 아니다 — 조회·집합·신호 어디서든(2026-08-20, Codex 5차 반려)**: 종료 추적의 누적 집합을 pid로 키잡으면 소멸을 확인한 pid를 새 자손이 재사용할 때 **편입이 막히고**, 옛 신원은 `Gone`이라 이후 조회에서도 건너뛰어 **살아 있는 자손을 아무도 보지 않는 채 `Confirmed`** 가 난다. 키는 `(pid, start_us)` 전체여야 한다(F7). 같은 실수가 **테스트 하네스에도** 있었다 — 가짜 `signal`이 pid로 첫 항목을 찾아 같은 pid를 쓰던 죽은 프로세스에 신호를 보냈다. 또한 **실행 중 관측 오류는 영구히 기억해야 한다** — 일시적 열거 실패·신원 불일치가 종료 시점에 사라져 있어도, 그 회차에 생겼다 사라진 자손이 있었는지 알 수 없으므로 소멸을 주장할 근거가 없다(F8). 상세는 `proposals/m4-orchestration.md` §7.10.1.2 ⑤
- **프런트 테스트는 `node --test` + 노드 타입 제거로 돌린다(2026-08-21)**: 앱에는 테스트 프레임워크가 없었지만 `website/`가 이미 `node --test`를 쓴다. 같은 방식에 `--experimental-strip-types`(Node 22.18)를 얹어 `.ts` 순수 모듈을 그대로 임포트한다. `npm test` → `tests/*.test.mts`. **UI 렌더링이 아니라 순수 상태 전이만** 테스트한다 — 그래서 프레임워크가 필요 없다
- **비Git 프로젝트를 푸는 길: "되돌릴 수 있게 준비하기"(2026-08-24)**: `create_first_project`가 `git init`을 하지 않아 **앱이 만든 프로젝트가 전부** 파일럿 대상이 못 됐다. 안내만 남기면 비개발자는 못 푼다. 동의를 받고 `git init` + **첫 복원 지점 커밋**을 한다 — `git init`만으로는 부족하다(커밋이 없으면 `rev-parse HEAD` 실패). 원본을 건드리는 유일한 동작이라 **동의 없이 실행 금지**, 기존 저장소 이력 불변, **다른 저장소 안의 폴더는 거부**(중첩 저장소가 사용자 이력을 어지럽힌다), 커밋 신원은 **그 커밋에만** 주입(`git config` 불변). 새 프로젝트는 처음부터 준비하되 **실패해도 프로젝트 생성은 성공**으로 둔다
- **`serde_json`에서 `null`은 "없음"이 아니다(2026-08-24 실사용 사고)**: `event.get("structured_output")`은 값이 JSON `null`이어도 `Some(Value::Null)`을 준다. `or_else` 대체 경로도, `let Some(..) else` 오류 보고도 **전부 건너뛴다.** 실사용에서 이것 때문에 인증 만료 원인을 잃었다. 없을 수도 있는 필드는 `.filter(|v| !v.is_null())`로 걸러 낼 것
- **에이전트 실패는 CLI가 말해 준 원인을 버리지 않는다(2026-08-24)**: 계획이 없다는 이유로 `"run produced no structured plan"`으로 덮어쓰면 **해결책이 분명한 상황(로그인 만료)에서도 사용자가 막다른 길에 선다.** `result` 텍스트를 그대로 전하고 닥터가 분류한다(`auth` 추가). 멈춘 화면은 닥터를 거쳐 쉬운 말로 이유를 보여주고 영문 원문은 상세 안에만 둔다
- **serde `rename_all`은 변형 이름만 바꾼다 — 필드는 `rename_all_fields`(2026-08-24 실사용 사고)**: `#[serde(tag="state", rename_all="camelCase")]`를 단 enum의 **struct 변형 안 필드는 그대로 snake_case로 나간다**. `run_id`가 그대로 나가 화면의 `started.runId`가 `undefined`가 됐고, 그 값을 다음 `invoke`에 넘기니 **키가 통째로 사라져** `command run_handoff missing required key runId`로 터졌다. **JS에서 값이 `undefined`면 키 자체가 없어진다** — 이게 조용한 실패의 정체다. `install.rs`의 `InstallEvent`는 이미 `rename_all_fields`를 달고 있었다(올바른 예). ⚠️ `login.rs`의 `LoginEvent`는 아직 안 달았지만 필드가 전부 한 단어(`url`·`line`·`success`)라 지금은 무해하다 — **여러 단어 필드를 추가하는 순간 같은 사고가 난다**. 구조체는 해당 없음(`rename_all`이 필드에 적용된다)
- **TypeScript 인터페이스는 손으로 적은 것이라 백엔드 모양을 검증하지 못한다(2026-08-24)**: 위 사고에서 TS는 `{ state: "ready"; runId: string }`이라고 선언했지만 백엔드는 `run_id`를 보냈고, 컴파일은 통과했다. 그래서 **화면이 읽는 키 이름 자체를 Rust 테스트로 못 박는다**(`the_start_reply_uses_the_key_names_the_screen_reads` 등). 경계를 넘는 새 타입을 만들면 같은 방식으로 키를 고정할 것
- **"설치됨"이 "쓸 수 있음"은 아니다(2026-08-24)**: 버전이 최신이어도 설치가 온전하지 않을 수 있다(Codex 셸 호스트 누락). 그런데 홈의 에이전트 버튼은 **미설치·미로그인·업데이트 있음** 세 경우에만 떠서, 망가진 최신 설치는 **아무 버튼도 없이 막다른 길**이 됐다. `AgentStatus.needs_repair`로 판정해 "고치기"를 노출한다. 새 에이전트를 붙일 때 **그 에이전트가 온전하다는 조건**을 `installation_is_complete`에 적을 것
- **Codex CLI 0.149.1 실측 — 앱이 쓰던 계약이 깨졌다(2026-08-24)**: ⚠️ **`approval_policy = "untrusted"`는 이제 거부된다**(`no longer supported; remove this setting`). 파일럿 인자와 앱의 안전 프리셋 양쪽에 있었고, 프리셋 쪽은 **신규 사용자의 Codex를 통째로 망가뜨린다**(유효값: `on-failure`·`on-request`·`granular`·`never`). `codex exec`에서 승인 정책 자체가 사라져 **경계는 `--sandbox` 하나**다. ⚠️ **릴리스는 `codex`와 `codex-code-mode-host`를 별도 자산으로 배포**하는데 앱은 앞의 것만 받아, 설치된 Codex가 **셸 명령을 하나도 실행하지 못했다**. 이때 파일이 안 생기니 **"샌드박스가 막았다"로 오판하기 쉽다** — 실제로 첫 실측이 그렇게 나왔다. 실측 결과: `--sandbox read-only`는 진짜로 쓰기를 막고(`EXIT=1`, `operation not permitted`), `--output-schema`로 구조화 계획이 나온다
- **Codex 계획 역할 계약은 Claude보다 약하다 — 숨기지 않는다(2026-08-24)**: Codex는 `thread.started{thread_id}`만 주고 **어떤 샌드박스·도구로 떴는지 이벤트로 알려주지 않는다.** 그래서 Claude의 `system:init` fail-closed 검증에 대응하는 것이 **없다**. argv 통제 + 실측된 샌드박스 + 사후 빈 diff에 기대며, 마지막은 예방책이 아니라 사후 확인이다. 거부된 도구 호출(`permission_denials`)도 기계 판독 불가. 상세는 `src-tauri/src/codex_pilot.rs` 머리말
- **테스트 함정 2개(2026-08-20)**: `std::sync::Mutex` 가드를 쥔 채 같은 락을 다시 잡으면 **데드락**이다(`session.slot()` 후 `session.release()`). 그리고 실패 주입에 "경로를 폴더로 선점"하는 기법을 쓸 때 존재 확인은 `exists()`가 아니라 **`is_file()`** 이어야 한다 — 폴더가 파일로 세어진다
- **실행 실패도 결과와 같은 무게로 실어 나른다(2026-08-20, Codex 6차 반려)**: `execute_pilot`은 `Result<PilotRun, PilotFailure>`다. 오류를 문자열로 줄이면 **이미 만들어진 증거 경로가 그 지점에서 사라져** 화면에 폐기·복구 수단이 남지 않는다. `PilotFailure`는 `error`·`run_id`·`workspace_path`·`evidence`·`poisoned`·`marker_error`를 싣는다. **`poisoned`(무엇이 잘못됐나)와 `marker_error`(왜 잠금을 저장 못 했나)는 다른 사실**이라 절대 합치지 않는다 — 합치면 화면의 "표시를 저장하지 못한 이유"에 종료 오류가 나온다. 그리고 **파일럿 슬롯은 하나뿐이므로 슬롯의 오염 상태를 볼 때는 반드시 복제본 경로를 대조한다** — 대조하지 않으면 남의 오염이 멀쩡한 복제본을 잠그고, 폐기해도 슬롯이 풀리지 않아 실행이 영영 막힌다
- **Claude CLI `2.1.237`에서도 실행 계약이 그대로 통과했다(2026-08-20)**: 계약을 실측한 `2.1.235`에서 패치 판올림이 있었지만 도구 집합·`dontAsk`·`claude-sonnet-5`가 모두 동일했다. 정확 일치 목록이 판올림마다 부서지지는 않는다는 첫 관측 — 다만 깨지면 목록을 넓히는 게 아니라 **재실측**이 답이다
- **함정: `/dev/urandom`을 `std::fs::read`로 읽지 말 것.** 끝없는 스트림이라 프로세스가 멈춘다. `read_exact`로 필요한 바이트만 읽는다## 작업 규칙 (고정)
- **워크로그 필수 범위**: 구현뿐 아니라 버그 분석, 코드 리뷰, 우선순위 결정, GitHub/배포 같은 외부 변경도 다음 작업자에게 의미가 있으면 이 문서에 기록. 최종 응답 전 `git diff -- docs/history.md`로 기록 여부 확인
- **커밋·push 전 반드시 사용자 승인**을 받는다 (이 프로젝트 관행)
- `git`은 scm_breeze 충돌(`_safe_eval` 오류)로 **`/usr/bin/git` 절대경로** 사용. 커밋 메시지는 임시 파일 방식(HEREDOC 미동작), Co-Authored-By 트레일러 포함
- **검증**: `pnpm build`(tsc+vite) · `cargo test --manifest-path src-tauri/Cargo.toml` · 네트워크 격리 E2E는 `-- --ignored` · push 시 CI(macOS/Windows)
- 실행: `pnpm tauri dev`(개발) / `pnpm tauri build`(설치 파일). Rust 1.85+ 권장, 실제 crate edition은 **2021**
- 초보자(비개발자) 대상 — 모든 문구는 전문용어 없이 쉬운 한국어. ".md도 IDE도 모르는 사람" 기준으로 검토
---
지난 워크로그: [11월](archive/history-2026-11.md) · 그 이전(Elsecrew 이름 변경 전) 기록은 2026-11-13에 정리했다. 필요하면 `git log -- docs/archive/`로 찾는다.

2026-11-11~11-20 워크로그(11-17 엔트리 안의 11-18 오전 후속 포함)는 `docs/archive/history-2026-11.md`로 이동했다.

2026-11-23~11-25 워크로그(실제 todo 시험, ADE 진단·서명·Codex 0.171, ADE 1차-가 구현, PR #3 1차 검토 반영)는 2026-11-26 롤오버로 같은 파일 맨 위로 옮겼다. 고정 섹션만 약 340줄이라 300줄 이하는 맞출 수 없다.

2026-11-26 워크로그 중 이미 main 쪽 `feature/ade`에 머지된 PR #3·#4(Keychain 수정 포함) 엔트리는 같은 날 롤오버로 `docs/archive/history-2026-11.md` 맨 위로 옮겼다. 모든 엔트리가 11-26이라 날짜 경계로는 자를 수 없어, 머지된 PR 단위로 나눴다.

2026-11-26 워크로그 중 `feature/ade`에 머지된 PR #5(레이아웃)·#6(미리보기 백엔드)·#7(스모크 복구)·#8(팀 역할) 엔트리는 같은 날 두 번째 롤오버로 `docs/archive/history-2026-11.md` 맨 위로 옮겼다(머지된 PR 단위).

2026-11-26 워크로그 중 `feature/ade`에 머지된 PR #9(미리보기 화면·검토 반영 두 라운드) 엔트리는 같은 날 세 번째 롤오버로 `docs/archive/history-2026-11.md` 맨 위로 옮겼다(머지된 PR 단위).

2026-11-26 워크로그 중 `feature/ade`에 머지된 PR #10(토큰·화면 밝기, 검토 반영 포함)·#13(ADE 1차 통합 검증 V, 검토 반영 포함) 엔트리는 같은 날 네 번째 롤오버로 `docs/archive/history-2026-11.md` 맨 위로 옮겼다(머지된 PR 단위).

2026-11-26 워크로그 중 `feature/ade`에 머지된 PR #11(활동 줄 자세히)·#12(화면 통일 U2~U4) 엔트리는 같은 날 롤오버로 `docs/archive/history-2026-11.md` 맨 위로 옮겼다(머지된 PR 단위).
