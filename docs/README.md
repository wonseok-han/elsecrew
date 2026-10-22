# 문서 안내

`Hello, Agent`의 문서는 **생애주기**로 분류한다. 주제가 아니라 "이 문서가 무엇을 주장하는가"가 기준이다.

| 분류 | 위치 | 무엇을 주장하나 |
| --- | --- | --- |
| **작업 기억** | `history.md` | 무엇이 일어났나 (시간순) |
| **현행 사실** | `architecture.md`, `diagrams/` | 지금 시스템이 어떻게 생겼나 |
| **실행 절차** | `release-checklist.md`, `procedures/` | 할 때 따라 하는 순서 |
| **제안 (아직 사실 아님)** | `proposals/` | 앞으로 이럴 수도 있다 |
| **지난 기록** | `archive/` | 당시엔 사실이었다 |
| **2026-09 작업 이력** | [`archive/history-2026-09.md`](archive/history-2026-09.md) | 날짜 단위로 이동한 과거 워크로그; 현재 상태는 `history.md` 우선 |
| **2026-10 작업 이력** | [`archive/history-2026-10.md`](archive/history-2026-10.md) | 날짜 단위로 이동한 워크로그; 현재 결정·미완료 경계는 `history.md` 우선 |
| **M5 잔여 워크트리 대조** | [`archive/m5-worktree-reconciliation-2026-09.md`](archive/m5-worktree-reconciliation-2026-09.md) | 9개 초안의 현행 구현 대응·보존 커밋·이식한 회귀 |
| **M5 변경 반영·복원 계약 초안** | [`proposals/m5-change-application.md`](proposals/m5-change-application.md) | 원본 비교·검토 묶음·복원 지점·적용/복구 구현 순서; 아직 제품 동작 아님 |
| **M5 장시간 단일 실행 출력 계약 초안** | [`proposals/m5-long-running-agent-streams.md`](proposals/m5-long-running-agent-streams.md) | 사용자 승인 방향에 따른 append-only 출력·페이지 조회·재접속·이관·자원 예산과 단계별 구현; 아직 제품 동작 아님 |

> ⚠️ **`proposals/`는 구현된 것이 아니다.** 여기 있는 내용을 현재 동작으로 인용하지 말 것. 이 경계가 이 분류의 존재 이유다.

---

## 어느 문서를 언제 여나

**작업을 시작할 때 → [`history.md`](history.md)**
현재 상태와 인계의 기준. `핵심 결정·발견`, `작업 규칙`, 최신 워크로그, 미완료 `다음 할 일`을 확인한다. 세션 종료 시 새 엔트리를 추가하는 곳도 여기다. 규칙은 루트 `CLAUDE.md` / `AGENTS.md` 참조.

**코드 구조를 알아야 할 때 → [`architecture.md`](architecture.md)**
시스템 구성, 위저드 상태 머신, 에이전트 레시피, 플랫폼별 전략, 검증 현황. 다이어그램 원본은 `diagrams/*.excalidraw`.

**배포할 때 → [`release-checklist.md`](release-checklist.md)**
자동 검증, 깨끗한 기기 확인, 실패 경로, 미서명 베타와 정식 서명 배포 게이트.

**M5 성능을 측정할 때 → [`procedures/m5-performance.md`](procedures/m5-performance.md)**
release sidecar 시작 실측과 전체 측정 JSON 판정의 실행법·입력 형식·남은 G6 수집 범위.

**직접 개발과 위임 방식을 선택할 때 → [`procedures/multi-agent-orchestration.md`](procedures/multi-agent-orchestration.md)**
2026-09-14부터 사용자가 선택한 Astra 메인의 직접 개발을 기본으로 한다. 위임이 더 유리하면 이유와 범위를 설명하고 대기한다. 승인된 병렬 작업에만 worktree·파일 소유권·다중 역할 절차를 적용한다. `.codex/config.toml`과 `.codex/agents/`는 보관된 서브에이전트 설정이며 메인 모델이나 자동 호출을 지정하는 운영 지시가 아니다.

**M4(멀티 에이전트 오케스트레이션)를 다룰 때 → [`proposals/m4-orchestration.md`](proposals/m4-orchestration.md)**
이 주제는 워크로그가 아니라 **이 문서가 단일 기준점**이다. 논의·결정·철회를 전부 여기 쌓는다. **손대기 전 §9 `철회된 주장`을 먼저 읽을 것** — 이미 틀린 것으로 판명된 결론에 다시 도달하는 것을 막는다.

**M5(코딩 에이전트 IDE)를 검토할 때 → [`proposals/m5-coding-agent-ide.md`](proposals/m5-coding-agent-ide.md)**
여러 영속 채팅, 역할별 에이전트·모델, standalone snapshot, 코드 편집·diff를 포함한 agent-first IDE의 승인된 제품 방향·설계 계약·열린 결정. **현재 구현 구조와 게이트는 `architecture.md`와 `history.md`가 기준**이며 이 제안서의 완료 기준을 현재 동작으로 인용하지 않는다. 새 제품 이름은 개발과 분리해 보류한다.

**M5 기반 스파이크의 원래 구현 계약을 확인할 때 → [`proposals/m5-electron-spike.md`](proposals/m5-electron-spike.md)**
Electron/Rust sidecar 수직 스파이크의 저장소 구조, typed IPC·JSONL protocol, SQLite 복구 계약, 보안 경계, 큰 파일·diff·다중 채팅 fixture와 수치 합격선을 고정한 구현 명세. 초기 구현의 출발 계약이며 **현재 구현 구조와 게이트는 `architecture.md`와 `history.md`를 우선**한다.

**G5D 압축 기록 재검증의 후속 계약을 확인할 때 → [`proposals/m5-storage-reopen-verification.md`](proposals/m5-storage-reopen-verification.md)**
V8을 유지하는 내부 R1 frozen-operation·R2 압축 관련 source/prefix/message·R3A ledger/projection·R3B 파생 기록 검증 계약과 R3C 전체 재오픈 활성화의 남은 게이트. §10은 정상 저장 snapshot의 크기 호환성 진단, §11–12는 큰 기록의 private byte transport, §13은 legacy oracle, §14는 canonical scanner, §15는 한 행 의미/SQLite mirror 비교, §16은 승인된 분할 처리 정책과 전역 검증→catch-up→ready 재검증 계약이다. 구현 사실은 `architecture.md`에서 확인하며 내부 단계의 성공을 제품 ready 또는 100개 초과 재오픈 지원으로 해석하지 않는다.

**M5 작업대 화면을 직접 조작해 볼 때 → [`proposals/m5-agent-ide-concept.html`](proposals/m5-agent-ide-concept.html)**
여러 프로젝트, 재배치 가능한 패널, 파일·변경 탐색, 프로젝트별 역할/모델 설정과 역할 간 팀 대화를 담은 독립 실행형 인터랙티브 시안. 제안서의 동반 검토 자료이며 제품 구현이 아니다.

**M5 처음 시작하기 흐름을 직접 조작해 볼 때 → [`proposals/m5-guided-start-concept.html`](proposals/m5-guided-start-concept.html)**
에이전트 진단·설치·로그인부터 실제 프로젝트 선택, 프로젝트별 역할 배정, 격리 작업, 팀 대화와 첫 diff 검토를 거쳐 작업대로 이어지는 독립 실행형 인터랙티브 시안. 제품 구현이 아니다.

**옛날 기록을 찾을 때 → [`archive/`](archive/)**
`history.md`에서 롤오버된 과거 워크로그. 재사용할 결정·함정은 `history.md`의 `핵심 결정·발견`에 반영돼 있으므로 통상 열 필요가 없다.

---

## 문서를 새로 만들 때

1. **어느 분류인지 먼저 정한다.** 애매하면 "이 문서를 현재 사실로 인용해도 되나?"를 물어본다 — 아니면 `proposals/`.
2. **`docs/` 최상위는 늘어나지 않게 한다.** 최상위는 진입점(`history.md`)과 오래 사는 단일 문서(`architecture.md`·`release-checklist.md`)만 둔다. 나머지는 분류 폴더로.
3. **이 파일의 표에 한 줄 추가한다.** 색인에 없는 문서는 다음 작업자가 못 찾는다.
4. **제안이 채택되면** `proposals/`에서 꺼내 현행 문서(`architecture.md` 등)에 반영하고, 제안 문서에는 반영 위치를 남긴다.

## 롤오버 규칙

`history.md`가 **500줄을 넘으면** 오래된 엔트리부터 `archive/history-YYYY-MM.md`로 옮겨 **300줄 이하로 줄인다.** 날짜 경계에서 자르고, 삭제가 아니라 이동이며, 고정 섹션(`프로젝트 요약`·`핵심 결정·발견`·`작업 규칙`)은 옮기지 않는다.

**날짜별로 파일을 쪼개지 않는다** — 현재 진실이 흩어져 조회 비용만 커진다. 날짜별 조회는 `git log -- docs/history.md`가 한다.
