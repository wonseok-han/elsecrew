# Hello, Agent

터미널을 한 번도 열어보지 않은 비개발자가 Claude Code·Codex를 설치하고, 안전한 첫 프로젝트를 시작한 뒤 계속 관리하도록 돕는 데스크톱 앱. 현재 기본 배포 경로는 Tauri이고, M5는 Electron + React/TypeScript + Monaco renderer와 Rust safety sidecar 전환 골격을 같은 저장소에서 개발한다.

## 가장 먼저 지킬 인계 규칙

`docs/history.md`가 Claude와 Codex가 공유하는 **프로젝트의 단일 작업 기억**이다.

### 작업 시작 전 — 반드시

1. `docs/history.md`를 읽는다. 최소한 `핵심 결정·발견`, `작업 규칙`, 최신 워크로그와 미완료 `다음 할 일`을 확인한다.
2. 문서의 현재 상태를 실제 코드·Git 상태와 대조한다. 오래된 항목을 사실처럼 반복하지 않는다.
3. 사용자가 지정한 범위와 현재 워킹 트리의 기존 변경을 확인한다.

### 작업 중 — 발견 즉시

- 다음 작업자가 재조사하지 않아도 될 결정·함정·외부 변경은 `docs/history.md`의 `핵심 결정·발견`에도 반영한다.
- 과거 워크로그는 덮어쓰거나 삭제하지 않는다. 완료된 예전 TODO는 최신 엔트리에서 완료 사실을 남긴다.
- 코드와 문서가 충돌하면 코드를 확인해 현재 사실로 바로잡고, 확인하지 못한 내용은 추정이라고 표시한다.

### 응답을 마치기 전 — 반드시

코드 변경 여부와 관계없이, **프로젝트에 영향을 주는 작업·분석·결정·검증·외부 변경**을 했다면 `docs/history.md` 맨 위에 새 엔트리를 추가한다.

- 형식: `### YYYY-MM-DD · by <정확한 모델명>`
- 포함: 한 일, 중요한 결정·발견, 검증 결과, 남은 다음 할 일
- 예: 구현, 버그 분석, 코드 리뷰, 우선순위 제안, GitHub 메타데이터 변경, 배포·릴리스 판단
- 단순 사용법 답변처럼 프로젝트 상태나 판단이 전혀 바뀌지 않은 대화만 생략할 수 있다.
- 최종 응답 전에 `git diff -- docs/history.md`로 이번 기록이 실제로 들어갔는지 확인한다.

### 워크로그가 커지면 — 아카이브 롤오버

`docs/history.md`가 **500줄을 넘으면** 오래된 엔트리부터 `docs/archive/history-YYYY-MM.md`로 옮겨 **300줄 이하로 줄인다.**

- **날짜 경계에서 자른다.** 같은 날짜의 엔트리를 쪼개지 않는다.
- **삭제가 아니라 이동이다.** 내용은 그대로 옮기고, `history.md` 맨 아래에 아카이브 포인터 한 줄을 남긴다.
- **고정 섹션**(`프로젝트 요약`·`핵심 결정·발견`·`작업 규칙`)은 옮기지 않는다. 옮기기 전에 재사용할 결정·함정이 `핵심 결정·발견`에 반영돼 있는지 확인한다 — 반영돼 있으면 아카이브를 열 일이 없다.
- 날짜별로 **파일을 쪼개지 않는다.** 현재 진실이 흩어져 조회 비용만 커진다. 날짜별 조회는 `git log -- docs/history.md`로 한다.

**워크로그와 설계 문서의 경계**: 워크로그는 *무엇이 일어났나*, 설계 문서는 *무엇이 참인가*를 기록한다. 한 주제의 설계 심의가 길어지면(토론 왕복·철회·정정이 여러 엔트리에 얽히면) 워크로그에 쌓지 말고 전용 설계 문서로 옮기고, 워크로그에는 결론 요약과 포인터만 남긴다. 예: `docs/proposals/m4-orchestration.md`

### 문서를 새로 만들 때

`docs/`는 **생애주기**로 분류한다. 색인과 규칙은 `docs/README.md`에 있고, 새 문서를 만들면 그 색인 표에 한 줄 추가한다.

| 분류 | 위치 | 무엇을 주장하나 |
| --- | --- | --- |
| 작업 기억 | `docs/history.md` | 무엇이 일어났나 |
| 현행 사실 | `docs/architecture.md`, `docs/diagrams/` | 지금 시스템이 어떻게 생겼나 |
| 실행 절차 | `docs/release-checklist.md` | 할 때 따라 하는 순서 |
| **제안(아직 사실 아님)** | `docs/proposals/` | 앞으로 이럴 수도 있다 |
| 지난 기록 | `docs/archive/` | 당시엔 사실이었다 |

- **`docs/proposals/`의 내용을 현재 동작으로 인용하지 않는다.** 이 경계가 분류의 존재 이유다.
- 애매하면 "이 문서를 현재 사실로 인용해도 되나?"를 묻는다 — 아니면 `proposals/`.
- `docs/` 최상위는 늘리지 않는다. 진입점(`history.md`)과 오래 사는 단일 문서만 두고 나머지는 분류 폴더로.
- **`CLAUDE.md`와 `AGENTS.md`는 같은 내용을 유지한다.** 한쪽만 고치면 다른 에이전트가 규칙을 모른 채 작업한다.

## 현재 제품 범위

- 에이전트: Claude Code + Codex
- 플랫폼: macOS + Windows
- UI: 한국어·영어 전환. 현재 구현 이름은 `Hello, Agent`, M5 새 이름은 개발과 분리해 보류
- 현행 구조: 재방문용 홈베이스 + 6단계 위저드(에이전트 → 진단 → 설치 → 로그인 → 프로젝트 → 첫 대화) + macOS M4 `HandoffPilot`
- M5 현행 구현: Electron + React/TypeScript + Monaco와 Rust sidecar에 여러 프로젝트·영속 채팅·프로젝트별 역할/모델·채팅별 standalone snapshot·편집/diff·명시 반영/복원 흐름을 연결했다. macOS Codex·Claude Code의 앱 전용 로그인·모델 조회·실제 실행 adapter와 설치/업데이트 경로가 있으며 기존 터미널 인증을 자동 재사용하지 않는다. fixture 실행과 실제 공급자 실행은 분리한다. 구현 존재와 실제 계정/플랫폼 검증 완료는 구분하며 최신 증거는 `docs/history.md`에서 확인한다.
- 홈베이스: 프로젝트 폴더 스캔, 에이전트 설치·로그인 상태, 업데이트 확인
- 상태: M0~M3와 주요 오류 복구, M4 파일럿 완료. M5 G1~G5 macOS fixture gate `G5_READY`, G5A `G5A_READY`, G5B `G5B_READY`, G5C `G5C_READY` 기록은 유지한다. macOS Codex의 작은 planning 응답은 확인했으나 Claude 실제 작업 성공·두 공급자의 도구 쓰기/중단/복구 전체는 미검증이다. G6 성능 수집 누락·메모리 기준 초과, Windows 안전/패키지 검증, 출시 후보 검증과 서명/배포 결정이 남는다. Windows 실제 에이전트 실행은 비활성이다.

## 코드 지도

- `src/App.tsx`: 최상위 홈/위저드 라우팅과 화면 컴포넌트
- `src/i18n.tsx`, `src/locales/{ko,en}.ts`: 경량 i18n. 새 키는 ko/en 양쪽에 추가
- `src/doctor.ts`: 구조화 오류를 초보자용 원인·해결책으로 변환
- `src/store.ts`: 기준 폴더와 프로젝트 최근 사용 시각 저장
- `src-tauri/src/agent.rs`: 에이전트별 설치·감지·로그인 레시피
- `src-tauri/src/{detect,install,login,project,status,editor,error}.rs`: 시스템 기능
- `crates/hello-agent-{core,protocol,sidecar}`: 공용 Rust 안전 코어·계약, SQLite·snapshot·채팅·fixture 및 실제 공급자 adapter, 로그인/설치·실행/종료·반영/복원·백업 관리 sidecar
- `electron/`: Electron main/preload, 고정 typed IPC와 sidecar 수명주기·패키징
- `src/m5/`, `src/m5/contracts/`: Monaco 채팅 작업대와 V1~V7 bridge·프로젝트/채팅/실행/복구·저장 수명·정규화 event·실제 공급자 및 변경 반영 계약
- `website/`: 별도 Vinext/OpenAI Sites 소개 웹사이트
- `docs/README.md`: 문서 색인과 분류 규칙. 어느 문서를 언제 여는지
- `.codex/config.toml`, `.codex/agents/`: 프로젝트 전용 서브에이전트 동시 실행 상한과 M5 역할·모델·권한 설정
- `scripts/m5-harness.mjs`: 기준선과 M5 G1~G6의 누적 기계 검증·JSON 증거 단일 진입점
- `docs/architecture.md`: 기술 설계
- `docs/history.md`: 현재 상태와 작업 인계의 기준
- `docs/proposals/m4-orchestration.md`: 완료된 M4 파일럿의 설계·검토·철회 기록. M4를 다룰 때 §9 `철회된 주장`을 먼저 확인한다
- `docs/proposals/m5-coding-agent-ide.md`: M5 제품 방향·안전 계약·열린 결정의 제안 문서. 현재 구현과 게이트는 `docs/architecture.md`·`docs/history.md`에서 확인한다
- `docs/proposals/m5-{agent-ide,guided-start}-concept.html`: 작업대와 처음 시작하기 인터랙티브 검토 시안

## 구현 원칙과 함정

- 사용자는 비개발자다. 터미널을 노출하지 않고 문구는 전문용어 없이 쓴다.
- 안전 기본값을 우선하며 기존 사용자 설정 파일을 덮어쓰지 않는다.
- Rust 오류는 `AppError { kind, detail }`로 전달한다. 사용자 설명은 프론트 i18n, `detail`은 도움 요청용 기술 정보로 분리한다.
- OpenAI 앱 번들 경로는 바뀔 수 있다. Codex CLI 감지는 `/Applications/{ChatGPT,Codex}.app/...` 후보 방식을 유지한다.
- 프로젝트 스캔은 현재 기준 폴더의 바로 아래 하위 폴더만 검사한다.
- `scm_breeze`의 `_safe_eval` 오류가 나면 `/usr/bin/git`, `/bin/ls`, `/usr/bin/find`처럼 절대경로를 사용한다.

## 구현 우선·최종 통합 검증 (2026-10-01 사용자 지시)

- 남은 앱 기능을 먼저 끝까지 연결하고, 그 상태에서 통합 검증과 버그 수정을 반복한다. 매 helper·작업·커밋마다 테스트/빌드/독립 검토를 의무 실행하지 않는다. 이 절은 아래 및 과거 작업 계약의 검증 빈도·커밋 전 검사 규칙보다 우선한다.
- 구현 중에는 영향 범위와 미검증/알려진 실패를 기록한다. 다음 구현에 꼭 필요한 컴파일·진단만 선택적으로 수행하며, 설치본 문제로 실제 시험이 막혀도 독립적으로 가능한 기능 구현을 계속한다.
- 작업 단위 로컬 커밋은 검증 전 구현 체크포인트로 남길 수 있다. 이를 기능 합격·gate PASS·제품 완료로 표현하지 않는다. 기존 사용자 변경은 선택적으로 보존하며 main 병합/push 승인 경계는 유지한다.
- 기능 연결 뒤 TypeScript/Rust 빌드·테스트·하네스·실제 UI 흐름과 안전 경계 독립 검토를 묶어서 수행한다. 실패 목록을 고치고 영향받은 검사부터 재실행한다. 최종 완료는 미검증 항목·차단 결함이 닫힌 뒤에만 선언한다.
- 검증 시점 변경은 제품의 안전 검사를 제거하거나 미검증 실제 실행/원본 쓰기를 활성화하는 승인이 아니다. 실제 계정·유료 모델·사용자 프로젝트 시험의 별도 승인과 원본 보호·종료/복구 계약은 유지한다.

## 개발 방식 선택 (2026-09-14 사용자 지시)

- 현재 메인 개발 모델은 사용자가 선택한 **GPT-6 Astra (`gpt-6-astra`)**다. 모델명만으로 속도 향상을 가정하지 않으며 추론 수준은 현재 사용자 설정을 따른다.
- **메인이 직접 조사·구현·관련 검증을 수행하는 것이 기본**이다. 단일 기능이나 순서가 밀접한 작업을 역할별로 쪼개 의무 위임하지 않는다. 직접 작업의 범위·기준 HEAD·검증 명령은 짧게 기록한다.
- **읽기 전용 독립 검토자 1명은 상시 호출 승인(2026-09-28 사용자 지시)**을 받았다. 기능 파동의 검토 범위를 짧게 설명한 뒤 추가 수동 승인 없이 호출하고, 같은 검토자의 수정 재검토도 이어간다. 이는 검토자 호출 승인이지 검토 결과의 자동 합격이 아니다. 구현 위임·추가 검토자 병렬 투입은 이유·범위·예상 이득을 설명하고 별도 승인 전에 대기한다.
- 직접 작업은 메인이 소유한 `m5/coding-agent-ide` checkout에서 수행할 수 있다. 동시 작성자가 없으면 기능마다 worktree·병합·별도 계약 문서를 만들 필요가 없다. 실제 병렬 작성에만 별도 branch/worktree와 파일 소유권을 요구한다.
- 일반 변경은 직접 diff 검토와 영향 영역 검사로 완료한다. 데이터 손상·권한·원본 보호·종료/복구 경계 변경과 제품 활성화·출시에는 독립 검토가 필요하다. 필요한 구현·회귀까지 준비한 뒤 상시 승인된 검토자 1명을 호출하며 자체 검증을 독립 승인으로 표현하지 않는다. 실제 계정·유료 모델·사용자 프로젝트 시험, main 병합·push 등 기존 별도 승인 경계는 유지한다.
- 관련 검사가 통과하면 새 실패나 영향 변화 없이 반복하지 않는다. 누적 QA는 기능 파동 마감에 한 번 수행하고, 문서·개발 설정만 바뀐 경우에는 문서 일치·설정 문법·diff 검사로 충분하다.
- 아래의 의무 역할 배치·worktree·독립 리뷰·통합 절차는 **사용자가 위임을 승인한 경우에 적용**한다. 이 절이 기존 운영 절차와 과거 작업 계약의 담당 배치 규칙에 우선하며 제품 안전 계약·출시 합격 기준은 유지한다.
- 기존 M5 작업 단위 자동 커밋 승인은 메인의 직접 구현·검증·작업 기록에도 적용한다. main 병합·push는 계속 별도 승인 대상이다.

## 멀티 에이전트 작업 규칙 (위임 승인 후 적용)

여러 구현·검토 에이전트를 사용하는 작업은 `docs/procedures/multi-agent-orchestration.md`를 따른다.

- M5 작업은 `.codex/agents/`의 프로젝트 전용 역할(`m5_architect`, `m5_contract_engineer`, `m5_electron_engineer`, `m5_rust_safety_engineer`, `m5_ui_engineer`, `m5_code_reviewer`, `m5_qa_executor`, `m5_gate_reviewer`) 중 책임에 맞는 것을 우선 사용한다.
- 한 명의 오케스트레이터/통합 담당만 통합 브랜치를 수정한다.
- 쓰기 구현 담당은 동시에 최대 3명이며, 각자 별도 Git branch와 `.worktrees/` 아래 실제 worktree를 사용한다. 같은 working directory에서 병렬 수정하지 않는다.
- 작업 시작 전에 base commit, 소유 파일/디렉터리, 변경 금지 영역, 의존 계약, 합격 기준, 검증 명령, 중단 조건을 작업 계약으로 고정한다.
- `package.json`/lockfile, Cargo workspace/lockfile, protocol/schema fixture, CI, `AGENTS.md`/`CLAUDE.md`는 한 파동에서 단일 소유자만 수정한다.
- M5 구현 담당은 작업 계약 범위 안의 구현·테스트·리뷰 수정 결과를 자기 기능 branch에 작업 단위로 자동 커밋할 수 있다. 이 상시 승인은 `m5/coding-agent-ide` 전체 완료 때까지 유효하다.
- 구현 담당은 merge/rebase/cherry-pick/push하지 않는다. 통합 담당은 독립 리뷰와 변경 영역 회귀·정적 검사를 통과한 기능 commit을 `m5/coding-agent-ide`에 추가 승인 없이 통합할 수 있다.
- **2026-09-03 간소화 운영**: 사용자 기능/결함 단위로 구현 담당과 독립 리뷰어 1명을 기본 배치한다. 개발 중에는 관련 검사만, 수정 후에는 영향받은 부분만 재검증하며 새 공용 계약이 없으면 별도 문서 심의를 만들지 않는다. 누적 QA·최종 gate는 최신 통합 commit의 파동 마감에 수행하고 helper마다 반복하지 않는다. 상세는 `docs/procedures/multi-agent-orchestration.md` §1.1이다.
- 기능 통합과 파동 QA 완료는 구분한다. 구현자의 자체 테스트만으로 제품 완료를 선언하지 않으며 데이터 손상·권한·원본 보호·종료/복구의 차단 결함은 반드시 닫는다. 후속 개선을 현재 작업의 완료 조건에 계속 추가하지 않는다.
- 하네스와 직접 Cargo 모두 `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4` 이하, 컴퓨터당 무거운 실행 하나를 지킨다. 동일 코드의 검증 증거는 재사용하되 영향받은 변경을 옛 PASS로 덮지 않는다.
- 실제 유료 에이전트, 사용자 인증·프로젝트, 강제 종료, 파괴적 시험은 하위 작업자가 임의 실행하지 않고 별도 사용자 승인을 받는다.
- 각 branch의 `docs/history.md` 기록은 통합 시 삭제하지 않고 모두 보존한다.

## 검증 명령

```bash
pnpm build
npm test                       # 프런트 순수 로직 (node --test)
cargo test --manifest-path src-tauri/Cargo.toml
pnpm test:harness              # 위 세 검증 + diff check를 실행하고 JSON 증거 저장
pnpm tauri build

cd website
npm test
npm run lint
```

- 네트워크 격리 설치 E2E: `cargo test --manifest-path src-tauri/Cargo.toml isolated_install -- --ignored --nocapture`
- M5 게이트 목록: `pnpm harness:list`; 최종 누적 게이트: `pnpm test:m5` (`NOT_READY`는 종료 코드 2이며 합격이 아님)
- Rust 1.85+ 권장. `src-tauri/Cargo.toml`의 실제 edition은 **2021**이다(포맷 검사 시 `rustfmt --edition 2021`)

## Git 작업 규칙

- 커밋과 push는 반드시 사용자 승인을 받은 뒤 실행한다. 단, 아래 M5 자동 승인 범위의 기능 커밋·통합 커밋은 이미 사용자 승인을 받았다.
- `git`은 `/usr/bin/git` 절대경로를 사용한다.
- 커밋 메시지는 임시 파일 방식으로 작성하고 `Co-Authored-By` 트레일러를 포함한다.
- 사용자 변경과 무관한 파일을 임의로 스테이징하거나 되돌리지 않는다.
- **M5 통합 브랜치**: `m5/coding-agent-ide`. M5 관련 작업 전 현재 브랜치를 확인하며, 기능·실험 브랜치는 여기서 분기하고 이 브랜치로만 병합한다.
- **M5 자동 승인 범위(2026-08-25 승인, 2026-09-03 검증 시점 간소화)**: 작업 계약에 명시된 기능 worktree의 원자적 구현·테스트·리뷰 수정 커밋과, 독립 리뷰 및 변경 영역 검증을 통과한 commit의 `m5/coding-agent-ide` 통합은 매번 다시 묻지 않고 실행한다. 파동 마감의 최신 통합 QA는 별도로 완료해야 한다. 범위 밖 파일, 안전 불변식·확정 결정 변경, 실제 유료 에이전트·사용자 프로젝트 시험, 외부 게시에는 적용하지 않는다.
- M5 작업 중 `main`에 직접 커밋·병합·push하지 않는다. M5 전체 완료·검증 뒤 **사용자가 명시적으로 승인한 경우에만** `m5/coding-agent-ide`를 `main`에 병합한다.
- 모든 branch의 push는 계속 별도 사용자 승인이 필요하다.
- `m4/orchestration`은 완료된 M4 파일럿 기준선이다. 별도 M4 후속을 명시적으로 수행하지 않는 한 새 M5 작업의 분기·병합 대상으로 사용하지 않는다.
