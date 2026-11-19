# M5 작업 공간 전환 설계 — 실제 git worktree와 그림자 git

> 제안 문서다. 현재 동작으로 인용하지 않는다. 결정 배경은 `docs/proposals/m5-orca-benchmark.md` §3.2·§3.2.1, 사용자 결정 기록은 `docs/history.md`의 2026-11-18 항목에 있다.

- 작성: 2026-11-18, Claude Opus 5.5
- 근거: 현재 작업 공간 코드 조사(하위 조사 에이전트 보고, 아래 `P`=`crates/elsecrew-sidecar/src/project.rs`, `S`=`crates/elsecrew-sidecar/src/`, `PR`=`crates/elsecrew-protocol/src/`)

## 1. 목표 (사용자 결정 요약)

| | git 저장소 프로젝트 | git이 아닌 폴더 프로젝트 |
| --- | --- | --- |
| 작업 공간 | 사용자 저장소의 실제 `git worktree`, 작업 하나에 브랜치 하나(고정) | 사용자 폴더 그 자리. 앱 데이터의 그림자 git이 기록 |
| 기록 | 팀원 단계마다 자동 커밋 | 작업 시작 커밋(백업·기준점) + 단계마다 자동 커밋, 작업마다 그림자 브랜치 |
| 끝난 뒤 | "main에 합치기" 여부만 결정. 합치기 전에 main 최신을 작업 브랜치로 가져와 충돌은 작업 공간에서 팀이 정리 | 합치기 없음. "이 작업 되돌리기" |
| 동시 작업 | 작업마다 분리 | 같은 폴더는 한 번에 하나 |
| 변경 탭·기록·되돌리기 | git 기반 같은 코드 | 같은 코드 |
| 로그인 | 앱 전용 유지 | 같음 |

### 1.1 기준 모델: 작업 하나 = PR 하나 (사용자 확인 2026-11-18)

| PR | Elsecrew 작업 |
| --- | --- |
| PR 브랜치 | 작업 브랜치 `elsecrew/task-…` |
| PR의 커밋들 | 팀원 단계마다 앱이 만드는 커밋 |
| Files changed | 변경 탭(기준 커밋 대비) |
| update branch | 합치기 직전 main 최신을 작업 브랜치로 가져오기 |
| 충돌 해결 | 작업 공간에서 팀이 정리 |
| Merge | "main에 합치기"(fast-forward) |
| PR 닫기 | 버리기 |

- 합치기 전까지 다른 작업의 변경은 보이지 않는다(열린 PR 두 개와 같음).
- 새 작업은 원본 저장소의 마지막 커밋에서 시작한다. 내 폴더의 커밋하지 않은 변경은 포함되지 않는다("푸시하지 않은 변경은 PR에 없다"와 같음). 새 작업을 만들 때 그런 변경이 있으면 한 줄 안내만 한다.
- 에이전트는 사용자 저장소 `.git`을 읽을 수 있다(사용자 허용 2026-11-18). 쓰기(커밋)는 앱만 한다.

## 2. 지금 구조 (바뀌는 전제)

- 작업 공간은 원본에서 `git bundle`로 HEAD만 받아 앱 데이터에 푼 **독립 저장소**다(`P` `create_worktree_inner` 1361-1708). 원본과 공유하는 것(remote·alternates·ref·원본 경로)이 없어야 한다는 검사가 반복된다(`verify_snapshot_repository`/`_metadata` 4783-4848, `snapshot_invariant`, `activation_evidence` 4877-4892).
- manifest 계약이 `workspaceKind:"standaloneSnapshot"`, `snapshotTransport:"gitBundle"`, `sourceMutationPolicy:"forbidden"`, `headMode:"detached"`로 고정되어 있다. Rust(`PR` project_v1.rs 12-15, 174-261), TS(`src/m5/contracts/project-v1.ts`), SQL 다시 열기(`S` storage/reopen/snapshot/workspace.rs 86-153)가 같은 값을 검사한다.
- 원본 `.git` 다이제스트가 작업 공간을 만들기 전후로 같아야 한다(`P` 1450-1455, 1543-1548). worktree 추가는 원본 refs·`.git/worktrees`를 바꾸므로 이 검사와 정면으로 충돌한다.
- OS 샌드박스는 작업 공간 하위만 읽고 쓴다. `WORKSPACE/.git` 쓰기를 금지한다(`S` provider_runtime_sandbox.rs 17-22, 42-48).
- 저장(파일 쓰기)의 원자 rename은 데이터 루트 FD 기준 `worktrees-v1/<id>/<path>`다(`P` 2579-2584, 2771-2843).
- 반영은 복사본→원본 덮어쓰기다(`latest_apply.rs`, 5단계 반영 `S` project/application/). 원본이 깨끗한 기준점(`source-baselines-v1` receipt)과 같아야 한다.
- git이 아닌 폴더는 프로젝트로 받지 않는다(`P` 4440-4443). 빈 폴더만 `empty_project.rs`가 git을 만든다.

## 3. 새 구조

### 3.1 작업 공간 종류와 기록 파일(manifest v2)

- `workspaceKind`: `gitWorktree` | `shadowFolder`. 기존 `standaloneSnapshot`(v1)은 읽기만 하고 새로 만들지 않는다(§6).
- 새 필드: `branchName`(예: `elsecrew/할-일-앱-3f2a`), `baseRef`(작업 시작 때 원본이 있던 브랜치 이름, 없으면 null), `baseCommit`, `gitDir`(shadow는 앱 데이터의 그림자 git 경로), `workTree`(gitWorktree는 앱 데이터 아래 worktree 경로, shadowFolder는 사용자 폴더).
- `sourceMutationPolicy`: `worktreeRefsOnly`(gitWorktree: 원본 `.git`에서 바뀌어도 되는 것은 `worktrees/<n>`·`refs/heads/elsecrew/*`·objects뿐) | `inPlace`(shadowFolder).
- `headMode`: `branch`.
- Rust·TS·SQL 검사기를 v2로 올린다. 저장 스키마는 V14(파생 테이블 `prepared_workspaces` 확장).

### 3.2 git 저장소 프로젝트: 실제 worktree

- **위치(사용자 결정 2026-11-18)**: Orca처럼 보이는 폴더 `~/elsecrew/workspaces/<프로젝트>/<작업 이름>`이 기본이다. 설정 "작업 공간 폴더"에서 바꿀 수 있다(Orca `workspaceDir`).
  - 바꾼 위치는 새 작업부터 적용한다. 작업마다 실제 경로를 manifest에 기록한다.
  - 거절하는 위치: 원본 안, 앱 데이터 안, 다른 작업 폴더 안, 쓸 수 없는 곳.
  - `worktree_capability`와 `validate_*`의 "데이터 루트/ID 재계산" 전제를 바꾼다. 작업 폴더를 만들 때 연 디렉터리 정체성(dev/ino)을 기록하고, 매번 그 폴더를 다시 열어 대조한다.
  - 저장 rename은 작업 폴더 FD 기준으로, 샌드박스 루트는 기록된 작업 폴더로 맞춘다.
  - 기존 작업을 새 위치로 옮기는 기능(`git worktree move`)은 나중 선택지다.
- **만들기**: 원본에서 `git worktree add -b <branchName> <workTree> <baseCommit>`.
  - 실행 전후로 원본 `.git`의 변화를 측정한다. "허용된 것만 바뀜"을 확인한다. 전후 다이제스트가 같아야 한다는 지금 검사를 대체한다.
  - 실패하면 `git worktree remove --force`, `git branch -D`로 되돌린다. 준비 기록(preparation ledger)에는 "worktree 추가함" 단계를 둔다.
- **git 실행 강화**: 모든 git 호출에 `-c core.hooksPath=/dev/null -c core.fsmonitor=false`를 붙이고, 시스템·전역 config를 끈다(지금 `raw_baseline.rs` 82-89에만 있는 것을 공통으로). 원본 저장소의 로컬 config는 읽히므로, filter·lfs 처리 방식은 §8에서 정한다.
- **검증 교체**: `verify_snapshot_*` 대신 `verify_worktree` 하나로 둔다.
  - worktree의 `.git` 파일이 원본 `.git/worktrees/<n>`을 가리킨다.
  - 그 브랜치가 `branchName`이다.
  - `git worktree list --porcelain`에 등록돼 있다.
  - 원본 경로가 기록과 같다.
  - 큰 저장소에서 매번 `fsck`를 돌리지 않는다.
- **샌드박스**: 에이전트는 worktree 하위를 읽고 쓴다.
  - 원본 `.git` 전체는 **읽기만** 허용한다. 에이전트의 `git status`·`git diff`·`git log`가 동작하게 하려는 것이다.
  - `.git` 쓰기는 계속 막는다. 커밋은 앱만 한다. 에이전트 지시문에 "커밋하지 마세요, 앱이 단계마다 기록합니다"를 넣는다.
- **자동 커밋**: 팀원 실행이 끝나면(성공·실패 모두, 변경이 있을 때만) 앱이 `git add -A && git commit`을 한다.
  - 작성자는 고정한다(`Elsecrew <elsecrew@local>`). 사용자의 전역 git 설정은 읽지 않는다.
  - 메시지는 "역할: 보고 첫 줄"이다.
- **runtime_isolation·watcher**: 실제 저장소에는 무시된 파일(`node_modules` 등)과 symlink가 있을 수 있다.
  - 검사 대상을 "추적 파일 + 추적 안 된 비무시 파일"로 좁힌다.
  - 추적 파일의 symlink는 허용하되, worktree 밖을 가리키면 거부한다.
  - watcher 한도는 무시 경로를 뺀 수로 계산한다.

### 3.3 합치기("main에 합치기")

1. 조건: 그 작업의 실행이 모두 끝났고 복구 대기가 없다.
2. **main 최신을 작업 브랜치로 가져온다**: 작업 worktree에서 `git merge <baseRef의 현재 커밋>`.
   - 충돌이 없으면 앱이 커밋한다.
   - 충돌이 있으면 충돌 표시가 남은 채로 팀(개발 담당)에게 "충돌 정리" 단계를 맡긴다. 정리·테스트 뒤 앱이 커밋한다. 정리하지 못하면 `git merge --abort`로 되돌리고 사용자에게 알린다.
3. **대상 브랜치를 앞으로 당기기(fast-forward만)**:
   - 원본 폴더가 그 브랜치를 체크아웃하고 있으면 `git -C <원본> merge --ff-only <branchName>`이다. 커밋하지 않은 수정과 겹쳐 실패하면 원본은 그대로 두고 "내 폴더의 수정 중인 파일과 겹쳐요"를 알린다.
   - 원본이 다른 브랜치에 있으면 `git update-ref refs/heads/<baseRef> <새 커밋> <이전 커밋>`로 원자적으로 옮긴다.
   - 어느 경우든 사용자 폴더에서 충돌 해결이 일어나지 않는다.
4. 합친 뒤 작업은 "합침" 상태가 된다. worktree는 정리 대상이고, 브랜치는 이미 합쳐졌으므로 지워도 된다.
- 지금의 `latest_apply.rs`, 5단계 반영, `source-baselines-v1` receipt, `capture_clean_baseline` 요구는 새 작업에 쓰지 않는다(§6).

### 3.4 버리기·정리·삭제

- 버리기: `git worktree remove --force` 뒤 브랜치를 처리한다. 합쳐지지 않은 브랜치는 기본으로 남기고(Orca 규칙), 사용자가 확인하면 지운다.
- 정리·프로젝트 삭제(`S` project/workspace_cleanup.rs, project_deletion.rs)는 재귀 삭제 대신 `worktree remove`와 `worktree prune`을 쓴다.

### 3.5 git이 아닌 폴더: 그림자 git

- 프로젝트를 추가할 때 git이 없고 비어 있지 않아도 받는다. 빈 폴더 git 만들기(`empty_project.rs`)는 "사용자 git으로 시작할지" 선택지로만 남긴다.
- 그림자 저장소: `m5/shadow-git-v1/<project_id>.git`(bare). 모든 git 호출을 `--git-dir=<그림자> --work-tree=<사용자 폴더>`로 한다. 사용자 폴더에는 아무것도 만들지 않는다.
- 기본 제외 목록(`info/exclude`): `.DS_Store`, 큰 의존성 폴더(`node_modules` 등)와 앱이 쓰는 숨김 파일. 사용자 폴더의 `.gitignore`가 있으면 따른다.
- 작업 시작: 작업 브랜치 `elsecrew/<작업>`을 만들고 지금 폴더 상태를 커밋한다(백업). 단계마다 자동 커밋한다.
- 되돌리기:
  - 작업 시작 커밋 기준으로 바뀐 파일은 되살리고, 작업에서 새로 생긴 파일은 지운다. 그림자 기록에 있는 것만 다룬다.
  - 되돌리기 직전 상태도 커밋해 두어 "되돌리기 취소"가 가능하게 한다.
- 같은 폴더의 작업은 한 번에 하나만 실행한다. 다른 작업은 "앞 작업이 끝나면 시작" 대기다.
- 샌드박스: 작업 폴더는 사용자 폴더다(읽기·쓰기). 그림자 git dir은 에이전트에게 보이지 않는다.

### 3.6 파일 탭·변경 탭·화면

- 파일 탭은 열린 작업의 worktree(폴더 프로젝트는 그 폴더)를 보여 준다. 위에 작업 이름·브랜치를 표시하고, git 상태 색을 입힌다.
- 작업 없이 프로젝트만 열었을 때 보던 "프로젝트 전용 복사본"(`keeps_project_copy`)은 없앤다. 대신 원본 폴더를 읽기 전용으로 보여 준다.
- 변경 탭은 기준 커밋 대비다. 지금과 같지만 커밋된 변경(단계별)과 작업 중 변경을 나눠 볼 수 있게 한다.
- 결과 막대:
  - git: `✓ 팀 작업 완료 · 브랜치 … · 변경 N [변경 보기][main에 합치기] 버리기`
  - 폴더: `✓ 팀 작업 완료 · 변경 N [변경 보기] 되돌리기`

### 3.7 재시작 복구

- 새 중간 상태: "worktree 추가함·기록 전", "자동 커밋 중", "합치기 중(작업 브랜치 병합 / 대상 이동)", "되돌리기 중".
- 시작할 때 `git worktree list --porcelain`·브랜치·DB를 대조해 끝내거나 되돌린다.
- 대상 브랜치 이동은 `update-ref <새> <이전>`의 원자성에 기대므로 반쯤 된 상태가 없다.

## 4. 바뀌는 안전 원칙과 지키는 것

- **바뀜**:
  - "원본 `.git`을 절대 바꾸지 않는다" → "원본 `.git`에는 worktree 등록·`elsecrew/*` 브랜치·커밋 객체만 생긴다".
  - "반영 전 원본 무변경" → "합치기 전 원본 작업 폴더 무변경".
  - CLAUDE.md·AGENTS.md의 "채팅별 standalone snapshot" 서술을 고친다.
- **지킴**:
  - 사용자 작업 폴더의 파일은 합치기(fast-forward) 때만 바뀐다.
  - 충돌 해결은 사용자 폴더에서 하지 않는다.
  - 에이전트는 `.git`에 쓰지 않는다.
  - 훅·fsmonitor는 실행하지 않는다.
  - 앱 전용 로그인을 유지한다.
  - 모든 단계는 원장에 기록하고 재시작 시 복구한다.
- 이 경계 변경은 단계마다 상시 검토자의 독립 검토를 받는다.

## 5. 구현 순서

1. **기반(동작 변화 없음)**: git 실행 강화 공통화, manifest v2·저장 V14 계약(Rust/TS/SQL), v1 병행 판독.
2. **git worktree 작업 공간**: 만들기·검증·샌드박스(원본 `.git` 읽기)·실행 연결·자동 커밋·변경 탭. 새 작업만 v2로 만든다.
3. **합치기·버리기·정리·복구**: §3.3·3.4·3.7.
4. **그림자 git 폴더 프로젝트**: 프로젝트 추가, 그림자 저장소, 되돌리기, 한 번에 하나.
5. **화면·정리**: 결과 막대, 파일 탭 표시·색, 프로젝트 전용 복사본 제거, 문서(CLAUDE.md/AGENTS.md/architecture) 갱신.

단계마다 관련 테스트와 실제 앱 todo 시험을 하고, 독립 검토를 받는다.

2단계(나) 보이는 위치 구현 계획(2026-11-18):
- 설정 "작업 공간 폴더"(앱 설정, 기본 `~/elsecrew/workspaces`). 새 v2 작업은 `<설정 폴더>/<프로젝트 이름>/<작업 짧은 이름>-<8자>`에 만든다. 바꾼 설정은 새 작업부터 적용한다.
- 거절할 위치: 원본 안, 앱 데이터 안, 다른 작업 폴더 안, symlink를 거치는 경로, 쓸 수 없는 곳.
- 작업 폴더의 근거는 "데이터 루트의 고정 자식"에서 "기록된 경로 + 만들 때 기록한 폴더 신원(dev/ino)"으로 바뀐다. 매번 절대 경로를 링크 없이 다시 열고 신원을 대조한다(`worktree_capability`). v1은 그대로 둔다.
- 저장(rename): 임시 파일을 대상과 같은 작업 폴더 안에 숨김 이름으로 만들고 같은 디렉터리 FD 안에서 rename한다. 다른 볼륨도 지원하기 위해서다.
- 정리: 신원 대조 뒤 `git worktree remove --force`로 지우고, 우리 기록과 브랜치를 정리한다.
- 바뀌는 곳: 만들기, 검증, 샌드박스 WORKSPACE, 실행 cwd(`orchestration_dispatch`), 저장, 감시, 정리, 프로젝트 삭제, 복구 검사.
- 경계 변경이므로 구현 뒤 독립 검토를 받는다.

진행(2026-11-19): 4단계는 사용자 결정으로 §3.5의 "제자리 수정" 대신 **링크 파일 방식**을 택했다. 그림자 저장소는 앱 데이터에 두고, 폴더에는 `.git` 링크 파일 하나만 둔다. 작업은 git 프로젝트와 같은 worktree에서 하고 "main에 합치기"로 폴더에 반영한다. §3.5의 되돌리기와 한 번에 하나 실행은 필요 없어졌다. 현행 사실은 `docs/architecture.md`.

진행(2026-11-18): 1단계 완료. 2단계는 (가) 만들기·검증·단계 커밋·샌드박스 읽기를 앱 데이터 위치(`worktrees-v1/<id>`)로 먼저 구현했다. 기본은 꺼져 있고 `ELSECREW_GIT_WORKTREES=1`로 켠다. (나) 보이는 위치와 설정은 저장 rename(같은 파일 시스템 필요)·정리·감시 기준을 작업 폴더로 옮기는 일이라 따로 한다.

## 6. 기존 작업(v1) 처리

- 이미 있는 standaloneSnapshot 작업은 그대로 열고 이어서 작업할 수 있다(v1 경로 유지). "내 폴더에 반영"도 v1 작업에만 남긴다.
- 새로 만드는 작업만 v2다. v1 작업이 모두 끝나거나 버려진 뒤 v1 경로를 지우는 것은 별도 결정이다.

## 7. 위험

- 실제 저장소 크기: worktree 추가는 빠르지만, untracked 파일 검사·watcher가 큰 저장소에서 느릴 수 있다. 한도와 측정이 필요하다.
- 원본 저장소의 로컬 config(filter·lfs·sparse-checkout)가 worktree 동작에 영향을 준다.
- 사용자가 터미널에서 같은 브랜치를 체크아웃하거나 worktree를 지우는 등 밖에서 바꾸는 경우: 검증에서 감지해 "복구 필요"로 보낸다.
- 그림자 git은 사용자 폴더 전체를 읽으므로 큰 폴더에서 느릴 수 있다. 제외 목록과 한도로 대응한다.

## 8. 결정 (사용자 확인 2026-11-18)

1. worktree 위치: Orca처럼 보이는 폴더 `~/elsecrew/workspaces/<프로젝트>/<작업 이름>`이 기본이고, 설정에서 바꿀 수 있다(§3.2).
2. 자동 커밋 작성자: 고정 `Elsecrew`.
3. 에이전트의 직접 git 커밋: 막고 앱이 커밋한다.
4. git filter(lfs 등)가 있는 저장소: 처음엔 미지원으로 안내한다.
