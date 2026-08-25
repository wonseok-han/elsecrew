# 멀티 에이전트 작업 절차

> 이 문서는 제품 기능 제안이 아니라 **저장소에서 여러 구현·검토 에이전트를 안전하게 운영하는 현행 절차**다. M5뿐 아니라 독립적으로 나눌 수 있는 큰 작업에 적용한다.

## 1. 기본 원칙

멀티 에이전트의 목적은 에이전트 수를 늘리는 것이 아니라 **서로 독립적인 작업의 대기 시간을 줄이고 검토 독립성을 확보하는 것**이다.

1. 한 명의 **오케스트레이터/통합 담당**이 전체 결과와 통합 브랜치를 소유한다.
2. 쓰기 권한이 있는 구현 담당은 동시에 **최대 3명**까지만 둔다.
3. 구현 담당은 각자 별도 Git branch와 실제 worktree에서 작업한다. 같은 working directory에서 동시에 수정하지 않는다.
4. 파일·디렉터리 소유권이 겹치는 작업은 병렬화하지 않는다.
5. 공통 계약을 먼저 고정한 뒤 그 계약의 양쪽 구현만 병렬화한다.
6. 구현 담당이 자기 변경을 최종 승인하지 않는다. 독립 코드 리뷰와 통합 QA를 거친다.
7. 판단할 수 없는 안전 상태, 테스트 유실, 계약 불일치는 성공으로 접지 않는다.

단순 문구 수정, 한 파일의 작은 버그처럼 분할 비용이 더 큰 작업에는 멀티 에이전트를 사용하지 않는다.

## 2. 역할과 권한

| 역할 | 책임 | 금지 |
| --- | --- | --- |
| 오케스트레이터/통합 담당 | 범위·의존성·파일 소유권 결정, 작업 배정, 통합, 최종 검증, 사용자 보고 | 검토 없이 작업자 주장을 합격으로 처리하지 않음 |
| 계약 담당 | protocol/schema/public API/fixture의 단일 기준 작성 | 양쪽 구현까지 동시에 소유해 자기 계약을 우회하지 않음 |
| 구현 담당 | 지정된 worktree와 소유 파일 안에서 최소 변경·테스트·증거 기록 | 통합 브랜치 수정, 다른 작업자 파일 수정, 임의 범위 확장 |
| 코드 리뷰 담당 | 특정 commit/diff를 읽기 전용 검토하고 재현 가능한 finding 제시 | 리뷰 대상 수정, 근거 없는 승인 |
| QA 담당 | 최신 통합 commit에서 실제 시나리오와 실패 경로 검증 | 기능 브랜치의 자기 테스트만으로 통합 합격 판정 |
| 최종 게이트 담당 | 구현·리뷰·QA 증거를 다시 대조해 완료선 판정 | 미실행 항목을 추정으로 통과 처리 |

에이전트 모델이나 벤더보다 역할과 권한이 먼저다. 같은 모델을 여러 역할에 쓸 수 있지만, **같은 실행이 구현자와 최종 검토자를 겸하지 않는다.**

### 2.1 Codex 프로젝트 역할 설정

Codex는 저장소의 [`.codex/config.toml`](../../.codex/config.toml)과 [`.codex/agents/`](../../.codex/agents/)에서 M5 전용 역할을 읽는다. 프로젝트를 신뢰한 로컬 Codex에서 적용되며, 동시에 열린 서브에이전트 thread는 최대 3개다. 이 상한은 쓰기 작업자 3명을 모두 투입하라는 뜻이 아니라 충돌 없이 독립적인 작업만 병렬화하라는 안전 한도다.

| 설정 역할 | 기본 모델 | 권한 | 사용하는 때 |
| --- | --- | --- | --- |
| `m5_architect` | `gpt-5.6-sol` high | 읽기 전용 | 경계·의존성·직렬화 지점 분석 |
| `m5_contract_engineer` | `gpt-5.6-sol` high | 작업 공간 쓰기 | protocol/schema/API/migration/fixture 단일 소유 |
| `m5_electron_engineer` | `gpt-5.6-terra` high | 작업 공간 쓰기 | Electron main/preload/lifecycle/package |
| `m5_rust_safety_engineer` | `gpt-5.6-sol` high | 작업 공간 쓰기 | Rust core/sidecar/process/standalone snapshot/recovery |
| `m5_ui_engineer` | `gpt-5.6-terra` high | 작업 공간 쓰기 | React/TypeScript/Monaco 작업대와 상태 UI |
| `m5_code_reviewer` | `gpt-5.6-terra` high | 읽기 전용 | 정확한 commit/diff의 독립 코드 리뷰 |
| `m5_qa_executor` | `gpt-5.6-terra` high | QA artifact만 쓰기 | 최신 통합 commit의 시나리오·실패 주입 |
| `m5_gate_reviewer` | `gpt-5.6-sol` high | 읽기 전용 | 명세·diff·리뷰·QA의 최종 대조 |

역할 설정은 책임과 기본 성향을 고정할 뿐 작업 범위를 부여하지 않는다. 오케스트레이터는 매번 §4 작업 계약에 base commit, worktree, 소유 파일, 변경 금지 영역, 합격 기준을 넣어야 한다. 모델을 임의로 바꾸기보다 역할 기본값을 쓰고, 비용·속도·난이도 때문에 예외가 필요하면 작업 기록에 이유를 남긴다.

## 3. 시작 전 Gate 0

오케스트레이터는 하위 작업을 만들기 전에 다음을 완료한다.

1. `AGENTS.md`, `CLAUDE.md`, `docs/history.md`, 대상 설계/구현 명세를 읽는다.
2. 실제 branch, HEAD, dirty working tree, 기존 사용자 변경을 확인한다.
3. 기준 검증을 실행해 시작 전부터 실패하던 항목을 기록한다.
4. 사용자에게 허용받은 커밋·병합·push·외부 실행 범위를 확인한다.
5. 작업 의존성 그래프와 직렬화 지점을 정한다.
6. 각 작업의 파일 소유권, 변경 금지 영역, 합격 기준을 적는다.

명세가 미커밋이면 먼저 검토·승인을 받아 기준 커밋으로 만든다. 구현 worktree는 모두 그 정확한 commit에서 분기한다.

## 4. 작업 계약

모든 구현·검토·QA 작업은 시작할 때 아래 계약을 받는다. 빠진 항목이 있으면 오케스트레이터가 보완한 뒤 시작한다.

```yaml
task: m5-electron-scaffold
role: implementation
baseBranch: m5/coding-agent-ide
baseCommit: <full sha>
worktree: .worktrees/m5-electron-scaffold
branch: m5/electron-scaffold
owns:
  - electron/**
  - package.json
  - pnpm-lock.yaml
mustNotTouch:
  - crates/**
  - src-tauri/**
  - src/m5/**
dependsOn:
  - protocol-fixture-v1
acceptance:
  - packaged app loads app:// with sandbox and context isolation
  - preload exposes no generic invoke or raw ipcRenderer
validation:
  - pnpm test
  - pnpm run package
evidence:
  - exact commands and exit codes
  - changed-file list
  - remaining risks
stopWhen:
  - protocol contract must change
  - an unowned shared file must change
```

작업 계약은 프롬프트에 포함해도 되고 별도 이슈/메모에 남겨도 된다. 단, 오케스트레이터가 나중에 **어떤 기준 commit에서 누가 무엇을 소유했는지** 복원할 수 있어야 한다.

## 5. Branch와 worktree 격리

통합 브랜치는 제품 마일스톤 규칙을 따른다. M5에서는 `m5/coding-agent-ide`만 통합 기준선이다.

- 기능 branch: `<milestone>/<scope>` 예: `m5/electron-scaffold`
- worktree 위치: `.worktrees/<milestone>-<scope>`
- `.worktrees/`는 Git 추적에서 제외한다.
- worktree를 만들기 전 같은 branch/path가 이미 등록됐는지 확인한다.
- 구현 담당은 자기 worktree 밖의 파일을 수정하지 않는다.
- M5 구현 담당은 작업 계약 범위 안의 구현·테스트·리뷰 수정 결과를 자기 기능 branch에 원자적 작업 단위로 자동 커밋한다. 테스트가 실패했거나 미추적 변경이 남은 중간 상태를 완료 커밋으로 만들지 않는다.
- 구현 담당은 merge, rebase, cherry-pick, push를 하지 않는다.
- 통합 담당은 독립 코드 리뷰와 해당 단계 하네스·QA를 통과한 기능 commit을 `m5/coding-agent-ide`에 추가 사용자 승인 없이 통합한다.

위 자동 승인은 2026-08-25 사용자가 M5 작업에 한해 명시했다. 범위 밖 파일이나 확정된 제품·안전 계약 변경, 실제 유료 에이전트·사용자 프로젝트·파괴적 시험, push, 외부 게시, `main` 병합에는 적용되지 않는다. 이런 동작은 §11에 따라 멈추고 별도 승인을 받는다.

worktree 제거는 변경·미추적 파일·필요한 증거가 모두 통합 또는 보존됐음을 확인한 뒤에만 한다. 강제 제거를 기본값으로 쓰지 않는다.

## 6. 파일 소유권과 공유 파일

한 파동에서 같은 파일을 두 구현 담당에게 배정하지 않는다. glob이 겹치면 작업을 다시 나누거나 직렬화한다.

다음은 항상 **단일 소유자**를 지정한다.

- `package.json`, `pnpm-lock.yaml`, 패키지 매니저 설정
- 루트 `Cargo.toml`, `Cargo.lock`, workspace 구성
- protocol/schema/golden fixture
- CI와 release workflow
- `AGENTS.md`, `CLAUDE.md`
- 마일스톤 통합 문서와 최종 증거 색인

`docs/history.md`는 모든 에이전트의 인계 규칙 때문에 기능 branch마다 충돌할 수 있다. 각 구현 담당은 자기 branch에 정확한 기록을 남기고, 통합 담당은 병합할 때 **어느 기록도 삭제하지 않고** 시간순·중복만 정리한다.

작업 도중 소유하지 않은 파일 변경이 필요해지면 직접 고치지 않는다. 필요한 변경과 이유를 오케스트레이터에게 보고하고 작업을 계속할 수 있는 독립 부분만 진행한다.

## 7. 파동식 실행

병렬화는 다음 순서로 연다.

1. **계약 파동**: public API, protocol, schema, fixture, 안전 불변식을 단일 담당이 고정한다.
2. **독립 구현 파동**: 고정 계약의 서로 다른 레이어를 별도 worktree에서 최대 3명이 구현한다.
3. **리뷰 파동**: 각 기능 commit을 독립 리뷰 담당이 읽기 전용으로 검토한다.
4. **재작업 파동**: 원 구현 담당이 finding을 수정하고 회귀 증거를 갱신한다.
5. **통합 파동**: 오케스트레이터가 한 기능씩 통합하며 매번 통합 검증을 실행한다.
6. **QA 파동**: 최신 통합 commit에서 수동·자동 시나리오와 실패 주입을 검증한다.
7. **최종 게이트**: 명세, diff, 리뷰, QA artifact를 다시 대조한다.

의존 계약이 아직 변하는데 양쪽 구현을 동시에 시작하지 않는다. 여러 구현 담당이 빨리 끝났더라도 통합은 한 번에 하나씩 하며 실패가 생기면 마지막으로 들어온 변경부터 원인을 좁힌다.

## 8. 구현 담당 완료 증거

구현 담당은 완료 시 다음을 모두 보고한다.

- branch, base commit, head commit
- 소유 범위와 실제 변경 파일 목록
- 구현한 동작과 의도적으로 하지 않은 동작
- 실행한 정확한 검증 명령, exit code, 핵심 결과
- 실패했거나 실행하지 못한 검증과 이유
- 보안·데이터·호환성 위험
- 통합 담당이 알아야 할 manifest/lockfile/migration 변경
- `git diff --check` 결과와 working tree 상태

“테스트 통과”만 적고 명령이나 대상 commit이 없으면 증거로 인정하지 않는다. 실제 유료 에이전트, 사용자 인증, 사용자 프로젝트, 강제 종료처럼 영향이 큰 시나리오는 하위 작업자가 임의로 실행하지 않는다.

## 9. 코드 리뷰와 재작업

리뷰 담당은 구현 담당과 다른 실행이어야 하며 특정 head commit을 기준으로 검토한다.

finding에는 다음이 필요하다.

- 우선순위와 차단 여부
- 정확한 파일·줄 또는 재현 지점
- 계약/불변식 중 무엇을 위반하는지
- 재현 방법 또는 실패 가능한 구체적 경로
- 수정 뒤 확인할 회귀 시나리오

취향 차이와 실제 결함을 구분한다. 차단 finding이 남은 branch는 통합하지 않는다. 수정은 원 구현 담당이 수행하고, 리뷰 담당은 새 commit을 다시 확인한다.

## 10. 통합과 QA 게이트

통합 담당은 기능마다 다음 순서를 지킨다.

1. 기능 branch의 기준과 변경 범위가 계약과 맞는지 확인한다.
2. 작업자 테스트와 리뷰 결과를 읽는다.
3. 독립 리뷰와 해당 하네스·QA를 통과한 기능 commit을 M5 자동 승인 범위 안에서 통합한다.
4. shared manifest/lockfile/history 충돌을 보존적으로 해결한다.
5. `git diff --check`와 해당 단계 회귀 테스트를 다시 실행한다.
6. 실패하면 다음 branch를 통합하지 않고 원인을 먼저 닫는다.

QA는 기능 worktree가 아니라 **최신 통합 commit**을 대상으로 한다. UI·패키징·프로세스·복구처럼 표면 증거가 필요한 시나리오는 실행 방법, verdict, artifact 경로를 남긴다. `PASS`는 실제 로그·스크린샷·DB/상태 증거 중 하나 이상을 가리켜야 한다.

## 11. 중단하고 사용자에게 묻는 조건

다음은 하위 에이전트나 오케스트레이터가 임의로 결정하지 않는다.

- M5 자동 승인 범위를 벗어난 commit·merge, 모든 push 또는 외부 게시가 필요함
- 실제 유료 모델 실행이나 새 로그인·자격 증명이 필요함
- 사용자 프로젝트에서 파괴적·강제 종료 시험이 필요함
- 명세의 확정 결정이나 안전 불변식을 바꿔야 함
- 사용자 기존 변경과 소유 파일이 충돌함
- 원본 프로젝트, 넓은 경로, 미확인 worktree를 삭제해야 함
- 동일한 blocker가 반복돼 안전하게 진전할 방법이 없음

범위 안의 일반 구현 선택과 되돌릴 수 있는 진단은 오케스트레이터가 진행한다.

## 12. M5.0 적용 순서

M5.0은 [`../proposals/m5-electron-spike.md`](../proposals/m5-electron-spike.md)의 구현 순서와 아래 소유권을 함께 따른다.

| 파동 | 단일 소유 영역 | 병렬 가능 영역 | 통합 게이트 |
| --- | --- | --- | --- |
| G0 | 명세·protocol fixture | 기준선 조사·테스트 목록 검토 | 사용자 승인과 기준 commit |
| G1 | Electron package manifest / Cargo workspace 각각 한 명 | Electron scaffold ↔ Rust core 추출 | package smoke + 기존 M4 회귀 |
| G2 | protocol v1과 typed bridge 계약 | contract test 검토 | Rust/TypeScript golden fixture 일치 |
| G3 | SQLite migration | sidecar protocol ↔ renderer mock shell ↔ fixture generator | replay·CSP·preload 검증 |
| G4 | project/standalone snapshot 안전 API | sidecar file/diff ↔ Monaco renderer | 원본 무변경·dirty/conflict QA |
| G5 | run/recovery state machine | chat stream UI ↔ fake agent fixture | 순서·취소·종료·복구 QA |
| G6 | 통합 manifest/CI/evidence | 보안 리뷰 ↔ 성능/패키지 QA | M5.0 완료 게이트 |

Windows agent 실행은 별도 종료 계약 전까지 비활성화 상태를 테스트한다. macOS 실제 에이전트 smoke는 자동 병렬 작업에서 제외하고 사용자의 별도 동의를 받은 뒤 통합 commit에서 한 번 실행한다.

## 13. 기계 검증 하네스

[`../../scripts/m5-harness.mjs`](../../scripts/m5-harness.mjs)가 모든 작업자와 통합 담당이 공유하는 검증 진입점이다. 하위 에이전트가 임의의 축약 명령으로 합격을 주장하지 않게 다음 규칙을 적용한다.

- `pnpm test:harness`: 현재 기준선의 diff check, 프런트 빌드·unit, 기존 Rust 회귀를 순서대로 실행한다.
- `pnpm harness:list`: `baseline`, `g1-foundation`부터 `g6-release`까지 준비 상태와 아직 없는 필수 package script를 나열한다.
- `pnpm test:m5`: G6까지 앞선 모든 게이트를 누적 실행한다.
- 구현되지 않은 단계는 `SKIP`이나 `PASS`가 아니라 `NOT_READY`와 exit code `2`다.
- 실제 실행 결과는 `.artifacts/m5-harness/*.json`에 schema version, branch, commit, dirty 상태, 명령, exit code, 소요 시간, stdout/stderr와 함께 남는다. artifact는 Git에 넣지 않고 QA·게이트 보고에서 정확한 경로를 인용한다.
- macOS/Windows CI도 `pnpm test:harness`를 실행하고 성공·실패와 관계없이 JSON artifact를 업로드한다. 로컬 축약 명령과 CI 판정이 갈라지지 않는다.
- 각 단계의 단일 소유자는 아래 예약 script를 실제 검증 명령에 연결한다. 이름을 바꿔 우회하지 않는다.

| 게이트 | 활성화에 필요한 `package.json` script |
| --- | --- |
| G1 foundation | `m5:test:electron`, `m5:test:core` |
| G2 contracts | `m5:test:contracts` |
| G3 storage | `m5:test:storage` |
| G4 project/editor | `m5:test:project`, `m5:test:editor` |
| G5 run/recovery | `m5:test:run-recovery` |
| G6 release | `m5:test:security`, `m5:test:package`, `m5:test:performance` |

게이트 실행은 항상 `baseline`부터 해당 단계까지 누적된다. 필수 script를 빈 명령이나 무조건 성공하는 placeholder에 연결하지 않는다. 실제 유료 에이전트 smoke와 사용자 승인 항목은 이 자동 하네스 바깥의 별도 증거로 남기며, 없으면 최종 게이트 담당이 전체 완료를 승인하지 않는다.

## 참고 근거

- [OpenAI Codex Subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents): 프로젝트 전용 `.codex/agents/*.toml`, 역할별 모델·추론 강도·sandbox, 동시 thread 설정과 독립 작업 위임 방식을 따른다.
- [OpenAI Codex configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference): `.codex/config.toml`의 `[agents]` 설정과 신뢰된 프로젝트 적용 범위를 따른다.
