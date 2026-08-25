# Hello, Agent

터미널을 한 번도 열어보지 않은 비개발자가 Claude Code·Codex를 설치하고, 안전한 첫 프로젝트를 시작한 뒤 계속 관리하도록 돕는 Tauri 데스크톱 앱.

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
- M5 방향: Electron + React/TypeScript + Monaco 셸과 기존 Rust 안전 코어 sidecar 위에 여러 프로젝트·영속 채팅·프로젝트별 역할/모델·worktree·코드 편집/diff를 갖춘 agent-first IDE를 만든다. 아직 현행 구현이 아니며 `docs/proposals/m5-coding-agent-ide.md`가 착수 기준
- 홈베이스: 프로젝트 폴더 스캔, 에이전트 설치·로그인 상태, 업데이트 확인
- 상태: M0~M3와 주요 오류 복구, M4 파일럿 완료. M5.0 착수 준비와 실기기 출시 후보 검증·서명/배포 결정이 남음

## 코드 지도

- `src/App.tsx`: 최상위 홈/위저드 라우팅과 화면 컴포넌트
- `src/i18n.tsx`, `src/locales/{ko,en}.ts`: 경량 i18n. 새 키는 ko/en 양쪽에 추가
- `src/doctor.ts`: 구조화 오류를 초보자용 원인·해결책으로 변환
- `src/store.ts`: 기준 폴더와 프로젝트 최근 사용 시각 저장
- `src-tauri/src/agent.rs`: 에이전트별 설치·감지·로그인 레시피
- `src-tauri/src/{detect,install,login,project,status,editor,error}.rs`: 시스템 기능
- `website/`: 별도 Vinext/OpenAI Sites 소개 웹사이트
- `docs/README.md`: 문서 색인과 분류 규칙. 어느 문서를 언제 여는지
- `.codex/config.toml`, `.codex/agents/`: 프로젝트 전용 서브에이전트 동시 실행 상한과 M5 역할·모델·권한 설정
- `scripts/m5-harness.mjs`: 기준선과 M5 G1~G6의 누적 기계 검증·JSON 증거 단일 진입점
- `docs/architecture.md`: 기술 설계
- `docs/history.md`: 현재 상태와 작업 인계의 기준
- `docs/proposals/m4-orchestration.md`: 완료된 M4 파일럿의 설계·검토·철회 기록. M4를 다룰 때 §9 `철회된 주장`을 먼저 확인한다
- `docs/proposals/m5-coding-agent-ide.md`: M5.0 제품·상태·안전·기술 착수 기준. **제안 경계이며 현행 구현으로 인용하지 않는다**
- `docs/proposals/m5-{agent-ide,guided-start}-concept.html`: 작업대와 처음 시작하기 인터랙티브 검토 시안

## 구현 원칙과 함정

- 사용자는 비개발자다. 터미널을 노출하지 않고 문구는 전문용어 없이 쓴다.
- 안전 기본값을 우선하며 기존 사용자 설정 파일을 덮어쓰지 않는다.
- Rust 오류는 `AppError { kind, detail }`로 전달한다. 사용자 설명은 프론트 i18n, `detail`은 도움 요청용 기술 정보로 분리한다.
- OpenAI 앱 번들 경로는 바뀔 수 있다. Codex CLI 감지는 `/Applications/{ChatGPT,Codex}.app/...` 후보 방식을 유지한다.
- 프로젝트 스캔은 현재 기준 폴더의 바로 아래 하위 폴더만 검사한다.
- `scm_breeze`의 `_safe_eval` 오류가 나면 `/usr/bin/git`, `/bin/ls`, `/usr/bin/find`처럼 절대경로를 사용한다.

## 멀티 에이전트 작업 규칙

여러 구현·검토 에이전트를 사용하는 작업은 `docs/procedures/multi-agent-orchestration.md`를 따른다.

- M5 작업은 `.codex/agents/`의 프로젝트 전용 역할(`m5_architect`, `m5_contract_engineer`, `m5_electron_engineer`, `m5_rust_safety_engineer`, `m5_ui_engineer`, `m5_code_reviewer`, `m5_qa_executor`, `m5_gate_reviewer`) 중 책임에 맞는 것을 우선 사용한다.
- 한 명의 오케스트레이터/통합 담당만 통합 브랜치를 수정한다.
- 쓰기 구현 담당은 동시에 최대 3명이며, 각자 별도 Git branch와 `.worktrees/` 아래 실제 worktree를 사용한다. 같은 working directory에서 병렬 수정하지 않는다.
- 작업 시작 전에 base commit, 소유 파일/디렉터리, 변경 금지 영역, 의존 계약, 합격 기준, 검증 명령, 중단 조건을 작업 계약으로 고정한다.
- `package.json`/lockfile, Cargo workspace/lockfile, protocol/schema fixture, CI, `AGENTS.md`/`CLAUDE.md`는 한 파동에서 단일 소유자만 수정한다.
- M5 구현 담당은 작업 계약 범위 안의 구현·테스트·리뷰 수정 결과를 자기 기능 branch에 작업 단위로 자동 커밋할 수 있다. 이 상시 승인은 `m5/coding-agent-ide` 전체 완료 때까지 유효하다.
- 구현 담당은 merge/rebase/cherry-pick/push하지 않는다. 통합 담당은 독립 리뷰와 하네스·QA 게이트를 통과한 기능 commit을 `m5/coding-agent-ide`에 추가 승인 없이 통합할 수 있다.
- 각 기능은 다른 실행의 읽기 전용 코드 리뷰와 최신 통합 commit의 QA를 통과해야 한다. 구현자의 자체 테스트만으로 완료 처리하지 않는다.
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
- **M5 자동 승인 범위(2026-08-25 사용자 승인)**: 작업 계약에 명시된 기능 worktree의 원자적 구현·테스트·리뷰 수정 커밋과, 독립 리뷰 및 해당 하네스·QA를 통과한 commit의 `m5/coding-agent-ide` 통합은 매번 다시 묻지 않고 실행한다. 범위 밖 파일, 안전 불변식·확정 결정 변경, 실제 유료 에이전트·사용자 프로젝트 시험, 외부 게시에는 적용하지 않는다.
- M5 작업 중 `main`에 직접 커밋·병합·push하지 않는다. M5 전체 완료·검증 뒤 **사용자가 명시적으로 승인한 경우에만** `m5/coding-agent-ide`를 `main`에 병합한다.
- 모든 branch의 push는 계속 별도 사용자 승인이 필요하다.
- `m4/orchestration`은 완료된 M4 파일럿 기준선이다. 별도 M4 후속을 명시적으로 수행하지 않는 한 새 M5 작업의 분기·병합 대상으로 사용하지 않는다.
