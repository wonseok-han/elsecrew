# 멀티 에이전트 작업 절차

> 이 문서는 제품 기능 제안이 아니라 **저장소에서 여러 구현·검토 에이전트를 안전하게 운영하는 현행 절차**다. M5뿐 아니라 독립적으로 나눌 수 있는 큰 작업에 적용한다.

## 1. 기본 원칙

### 최우선 검증 시점 (2026-10-01 사용자 지시)

남은 앱 기능을 먼저 연결하고 최종 통합 상태에서 검증·버그 수정을 반복한다. 매 작업/커밋마다 테스트·빌드·독립 검토를 의무화하지 않는다. 필요한 진단만 선택적으로 하고 미검증 범위·알려진 실패를 기록한다. 로컬 구현 체크포인트 커밋은 허용하지만 기능 합격/gate PASS가 아니다. 이 규칙은 아래의 기능별 검사·검토 시점 및 완료 커밋 규칙보다 우선하며, 최종 통합 검증·안전 경계 독립 검토·출시 기준 자체는 유지한다. 설치본 장애와 독립적으로 구현 가능한 작업을 계속한다. 미검증 실제 실행/원본 쓰기 활성화, 계정·유료 모델·사용자 프로젝트 시험, main 병합/push를 새로 허가하지 않는다. 상세 기준은 루트 지침의 `구현 우선·최종 통합 검증` 절이다.

### 현재 운영 우선 규칙 (2026-09-14 사용자 지시)

메인 개발은 사용자가 선택한 GPT-6 Astra (`gpt-6-astra`)가 직접 수행한다. 현재 추론 수준을 임의 변경하지 않는다. 직접 수행이 유리한 단일 기능은 메인 checkout에서 조사·구현·diff 검토·관련 검사까지 진행하며, 별도 역할 호출·기능 worktree·병합을 의무화하지 않는다. 기준 HEAD, 범위, 합격 기준과 검증은 짧은 작업 기록으로 남긴다.

읽기 전용 독립 검토자 1명 호출은 2026-09-28 사용자 지시로 상시 승인됐다. 기능 파동의 범위를 짧게 설명하고 추가 수동 승인 없이 호출하며, 같은 검토자의 수정 재검토도 포함한다. 이는 호출 승인이지 검토 결과의 자동 합격이 아니다. 구현 위임·추가 검토자 병렬 투입은 독립적인 소유 범위와 기대 이득을 설명하고 별도 승인을 기다린다. 일반 변경은 직접 검토로 완료할 수 있다. 데이터·권한·원본 보호·종료/복구 경계 변경 또는 제품 활성화·출시는 구현·관련 검증을 준비한 뒤 상시 승인된 검토자 1명으로 독립 검토한다. 실제 계정·유료 모델·사용자 프로젝트 시험, main 병합·push의 별도 승인 경계는 유지한다.

이 규칙은 아래 §1.1~§12의 의무 담당 배치·독립 승인·worktree·통합 순서보다 우선한다. 아래 다중 역할 절차는 위임이 승인됐을 때 적용한다. 필수 안전 시나리오와 출시 기준, §13의 기계 검증은 유지한다. 문서·개발 설정 변경에는 일치·문법·diff 검사만 실행하고, 코드 변경은 관련 검사와 파동 마감 누적 QA를 사용한다. 새 변경·실패·미해결 우려 없이 통과한 검사를 반복하지 않는다.

기존 M5 자동 커밋 범위는 메인의 직접 구현·검증·기록에도 적용한다. main 병합·push 등 외부 작업의 승인 경계는 유지한다.

### 위임 승인 후의 기본 원칙

멀티 에이전트의 목적은 에이전트 수를 늘리는 것이 아니라 **서로 독립적인 작업의 대기 시간을 줄이고 검토 독립성을 확보하는 것**이다.

1. 한 명의 **오케스트레이터/통합 담당**이 전체 결과와 통합 브랜치를 소유한다.
2. 쓰기 권한이 있는 구현 담당은 동시에 **최대 3명**까지만 둔다.
3. 구현 담당은 각자 별도 Git branch와 실제 worktree에서 작업한다. 같은 working directory에서 동시에 수정하지 않는다.
4. 파일·디렉터리 소유권이 겹치는 작업은 병렬화하지 않는다.
5. 공통 계약을 먼저 고정한 뒤 그 계약의 양쪽 구현만 병렬화한다.
6. 구현 담당이 자기 변경을 최종 승인하지 않는다. 독립 코드 리뷰와 통합 QA를 거친다.
7. 판단할 수 없는 안전 상태, 테스트 유실, 계약 불일치는 성공으로 접지 않는다.

단순 문구 수정, 한 파일의 작은 버그처럼 분할 비용이 더 큰 작업에는 멀티 에이전트를 사용하지 않는다.

### 1.1 기능 단위의 간소화 운영 (2026-09-03 사용자 승인)

- **완료 단위**는 사용자가 확인할 기능 또는 하나의 명확한 결함 수정이다. 내부 helper마다 별도 계약 심의·최종 gate·전체 검증을 만들지 않는다. 하나의 기능 파동에 포함할 변경, 완료 조건, 후속으로 남길 항목을 시작할 때 짧게 고정한다.
- 기본 구성은 구현 담당과 **독립 리뷰 담당 1명**이다. 설계/계약 전문가는 새로운 공용 계약이나 해결되지 않은 설계 문제가 있을 때, 별도 QA/최종 gate 담당은 파동 마감·제품 활성화·출시 검증 때만 호출한다. 역할이 존재한다고 매 작업에 모두 투입하지 않는다.
- **개발 중**에는 변경 영역의 회귀와 정적 검사, **수정 뒤**에는 지적된 경로와 영향 범위만 다시 검사한다. **독립 리뷰는 기능 단위 1회가 기본**이며 이후에는 수정 diff와 차단 finding을 재확인한다. 새 차단 결함이 발견되면 닫되 동일 범위의 전체 리뷰를 관성적으로 반복하지 않는다.
- **누적 전체 QA는 최신 통합 commit의 파동 마감에 한 번** 실행한다. 실패 수정이 해당 결과를 무효화하면 영향받은 단계만 재실행하고, 원래 결과와 수정 SHA·영향 분석을 함께 남긴다. 공용 계약/의존성/실행 환경 등 영향이 넓거나 불명확한 경우에는 관련 전체 단계로 확대하고 이유를 기록한다. 변경된 코드를 옛 PASS로 덮지 않는다.
- 데이터 손상·권한·원본 보호·종료/복구·요구 기능의 실제 결함과 실패 테스트는 차단한다. 취향·장래 확장·현재 완료 조건과 무관한 개선은 후속 목록에 남기며 작업을 계속 확장하지 않는다. 같은 원인으로 두 번 재작업하면 새 검토자를 더 붙이기 전에 원인과 범위를 다시 좁힌다.

이 절은 기존 작업 계약의 **중복 실행 빈도와 담당 배치**에 우선한다. 필수 안전 시나리오·합격 기준·제품 활성화/출시 전 전체 검증은 삭제하지 않는다. 기능 commit/통합과 파동 QA 완료를 구분해 보고하며, 파동 검증 전 제품 완료나 기존 실행 제한 해제를 선언하지 않는다.

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

다음 표는 **필요 시 사용하도록 보관한 서브에이전트 설정**이며 메인 모델을 지정하지 않는다. 메인은 현재 앱에서 사용자가 선택한 Astra를 따른다. Sol/Terra 역할을 일괄 Astra로 변경하거나 실제 호출하지 않았으며, 위임 재개 시 과제에 맞춰 모델과 추론 수준을 정한다.

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
3. 변경 영역의 기준 검증과 기존 실패를 확인한다. 동일 코드·의존성·환경의 유효한 증거가 있으면 재사용 근거를 남기고, 모든 작업 시작에 전체 기준선을 재실행하지 않는다.
4. 사용자에게 허용받은 커밋·병합·push·외부 실행 범위를 확인한다.
5. 작업 의존성 그래프와 직렬화 지점을 정한다.
6. 각 작업의 파일 소유권, 변경 금지 영역, 합격 기준을 적는다.

새 공용 API/schema/안전 계약을 바꾸면 구현 전 검토된 기준을 고정한다. 기존 계약 안의 내부 구현·버그 수정은 §4의 짧은 작업 계약으로 충분하며 별도 제안서·문서 심의 commit을 요구하지 않는다. 정확한 base commit과 기존 dirty 변경은 항상 기록한다.

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
- 통합 담당은 독립 코드 리뷰와 변경 영역 검증을 통과한 기능 commit을 `m5/coding-agent-ide`에 추가 사용자 승인 없이 통합한다. 누적 QA는 §1.1·§10에 따라 파동 마감에 수행한다.

위 자동 승인은 2026-08-25 사용자가 M5 작업에 한해 명시했다. 범위 밖 파일이나 확정된 제품·안전 계약 변경, 실제 유료 에이전트·사용자 프로젝트·파괴적 시험, push, 외부 게시, `main` 병합에는 적용되지 않는다. 이런 동작은 §11에 따라 멈추고 별도 승인을 받는다.

worktree 제거는 변경·미추적 파일·필요한 증거가 모두 통합 또는 보존됐음을 확인한 뒤에만 한다. 강제 제거를 기본값으로 쓰지 않는다.

### 5.1 완료 작업의 정리 시점

- 통합 담당은 **기능 통합·최종 QA/gate 완료 직후**, 다음 작업용 worktree를 만들기 전에 정리 여부를 점검한다. 보류한 폴더는 이유와 다음 확인 시점을 인계 기록에 남긴다. 현재 등록 수와 `.worktrees/` 용량도 확인해 완료 폴더가 계속 누적되지 않게 한다.
- 제거 대상은 작업·검토·검증 프로세스가 더 이상 사용하지 않고, tracked 변경·미추적 파일이 없는 exact path/branch/HEAD로 고정한다. 최근 작업, 새로 생성된 폴더, 잠긴 폴더 또는 사용 여부가 불명확한 폴더는 보존한다.
- 통합 여부는 commit ancestry 또는 실제 변경 내용의 동등성으로 확인한다. cherry-pick은 SHA가 달라질 수 있으므로 브랜치 이름이나 commit 제목만으로 판정하지 않는다. 불명확하면 제거하지 않는다.
- `.artifacts`·`.omo`·로그처럼 Git에서 제외된 검증 증거도 별도 점검한다. 필요한 파일은 제거 대상 밖에 보존하고 원본과 대조한다. 빌드 캐시와 검증 증거를 같은 것으로 취급하지 않는다.
- 사용자 승인 범위 안에서 제거 직전 상태·사용 여부를 다시 확인하고 `/usr/bin/git worktree remove <확인한 절대경로>`로 제거한다. `--force`나 상위 `.worktrees/` 전체 삭제를 사용하지 않는다. 브랜치·커밋은 보존하며 별도 삭제 승인을 받은 것으로 간주하지 않는다.
- 제거 후 등록 해제·폴더 부재·보존 branch SHA·남긴 작업의 존재와 용량을 확인하고 대상 목록·증거 위치·재생성 방법을 기록한다. 필요하면 보존한 branch로 worktree를 다시 생성하고 빌드 산출물은 재생성한다.

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

1. **구현**: 필요한 공용 계약만 먼저 고정하고, 기능 범위 안에서 구현·관련 회귀를 진행한다. 독립 레이어만 별도 worktree에서 병렬화한다.
2. **독립 리뷰·통합**: 기능 diff를 한 명이 검토하고 원 구현 담당이 차단 결함을 수정한다. 수정 부분을 재확인한 뒤 순차 통합하고 통합으로 달라진 경계를 확인한다.
3. **파동 마감**: 최신 통합 commit의 누적 QA와 필요한 수동 시나리오를 실행하고 증거를 대조한다. 별도 최종 gate는 파동/마일스톤에 한 번이며 중간 helper마다 만들지 않는다.

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

취향 차이와 실제 결함을 구분한다. 차단 finding이 남은 branch는 통합하지 않는다. 수정은 원 구현 담당이 수행하고 리뷰 담당은 새 commit의 수정 diff·영향 범위만 재확인한다. 동일 코드의 성공 테스트는 근거 없이 다시 실행하지 않으며 재현이나 영향 분석에 필요한 검사만 요청한다.

## 10. 통합과 QA 게이트

통합 담당은 기능마다 다음 순서를 지킨다.

1. 기능 branch의 기준과 변경 범위가 계약과 맞는지 확인한다.
2. 작업자 테스트와 리뷰 결과를 읽는다.
3. 독립 리뷰와 변경 영역 회귀·정적 검사를 통과한 기능 commit을 M5 자동 승인 범위 안에서 통합한다.
4. shared manifest/lockfile/history 충돌을 보존적으로 해결한다.
5. `git diff --check`, 검토된 diff와의 동일성 및 통합 영향 범위를 확인한다. 코드 충돌 해결·공용 변경이 있으면 해당 회귀를 실행한다. 단순 동일 코드 통합마다 기능 전체 suite와 누적 하네스를 둘 다 반복하지 않는다.
6. 실패하면 다음 branch를 통합하지 않고 원인을 먼저 닫는다.

파동 마감 QA는 기능 worktree가 아니라 **최신 통합 commit**을 대상으로 한다. 기능 통합만 끝났으면 누적 QA 대기라고 기록한다. UI·패키징·프로세스·복구처럼 표면 증거가 필요한 시나리오는 실행 방법, verdict, artifact 경로를 남긴다. `PASS`는 실제 로그·스크린샷·DB/상태 증거 중 하나 이상을 가리켜야 한다.

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

### 13.1 로컬 QA 저부하 기본값 (2026-09-02 사용자 승인)

- 하네스 자식 환경의 `CARGO_BUILD_JOBS`와 `RUST_TEST_THREADS`는 각각 최대 **4**다. 미설정·유효하지 않은 값은 4로 정규화하며 이미 설정한 1~3은 유지한다. package script를 통해 실행되는 Cargo에도 상속된다. 직접 명령으로 실행할 때도 아래와 같이 같은 제한을 명시한다. `--jobs`/`--test-threads`로 제한을 우회하지 않는다.
- 하네스 자신의 실행 우선순위를 nice 값 10 이상으로 낮춘다. 이미 더 낮은 우선순위이면 올리지 않는다. POSIX 자식은 이를 상속한다. 권한/플랫폼 때문에 적용할 수 없으면 경고와 실제 결과를 JSON `resources`에 남기며 병렬 수 제한은 유지한다. Windows 자식의 우선순위 상속은 별도 실측 전까지 보장하지 않는다.
- 이 설정은 Rust 컴파일 작업·테스트 실행 수 제한이지 CPU 사용률의 절대 상한이 아니다. 테스트 내부 thread·Node/Electron/native worker 전체를 4개로 제한하지 않는다. **같은 컴퓨터에서는 무거운 전체 suite/누적 QA를 한 번에 하나만** 실행한다. 다른 worktree의 컴파일·QA와 겹치지 않도록 통합 담당이 조정하며, 자동 전역 잠금은 아직 없다.
- **작업 중**에는 §1.1에 따라 변경 영역의 focused 회귀·정적 검사를 실행한다. **통합 파동 마감/최종 승인 전**에는 최신 통합 commit에서 해당 누적 게이트를 실행한다. 기존 계약에 나열된 전체 suite가 누적 게이트에 포함되면 같은 파동에서 별도로 반복하지 않는다. focused 성공을 누적 `PASS`나 제품 완료로 보고하지 않으며 필수 안전 시나리오와 합격 기준은 유지한다.
- 진행 중인 검증의 우선순위·환경·대상 코드는 바꾸지 않는다. 제한은 다음 실행부터 적용한다. 낮은 병렬 수로 시간이 늘어나더라도 기존 하네스 timeout·제품 ready 시간·합격 기준을 자동 완화하지 않는다.

macOS/Linux에서 작업 계약에 지정된 집중 회귀를 직접 실행하는 예:

```bash
CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 nice -n 10 cargo test -p hello-agent-sidecar --locked <작업_계약의_테스트_필터>
```

PowerShell에서는 `$env:CARGO_BUILD_JOBS='4'; $env:RUST_TEST_THREADS='4'` 설정 뒤 지정된 명령을 실행한다. `nice` 예제를 Windows 지원 증거로 사용하지 않는다.

## 참고 근거

- [OpenAI Codex Subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents): 프로젝트 전용 `.codex/agents/*.toml`, 역할별 모델·추론 강도·sandbox, 동시 thread 설정과 독립 작업 위임 방식을 따른다.
- [OpenAI Codex configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference): `.codex/config.toml`의 `[agents]` 설정과 신뢰된 프로젝트 적용 범위를 따른다.
