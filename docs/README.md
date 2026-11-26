# 문서 안내

`Elsecrew`의 문서는 **생애주기**로 분류한다. 주제가 아니라 "이 문서가 무엇을 주장하는가"가 기준이다.

| 분류 | 위치 | 무엇을 주장하나 |
| --- | --- | --- |
| **작업 기억** | `history.md` | 무엇이 일어났나 (시간순) |
| **현행 사실** | `architecture.md` | 지금 시스템이 어떻게 생겼나 |
| **실행 절차** | `release-checklist.md`, `procedures/` | 할 때 따라 하는 순서 |
| **제안 (아직 사실 아님)** | `proposals/` | 앞으로 이럴 수도 있다 |
| **지난 기록** | `archive/` | 당시엔 사실이었다 |
| **2026-11 작업 이력** | [`archive/history-2026-11.md`](archive/history-2026-11.md) | 날짜 전체 블록을 보존한 워크로그; 현재 결정·미완료 경계는 `history.md` 우선 |
| **M5 변경 반영·복원 계약 초안** | [`proposals/m5-change-application.md`](proposals/m5-change-application.md) | 원본 비교·검토 묶음·복원 지점·적용/복구 구현 순서; 아직 제품 동작 아님 |
| **M5 장시간 단일 실행 출력 계약 초안** | [`proposals/m5-long-running-agent-streams.md`](proposals/m5-long-running-agent-streams.md) | 사용자 승인 방향에 따른 append-only 출력·페이지 조회·재접속·이관·자원 예산과 단계별 구현; 아직 제품 동작 아님 |
| **M5 채팅 내용 영구 삭제 계약 초안** | [`proposals/m5-chat-erasure.md`](proposals/m5-chat-erasure.md) | 버린 채팅의 본문·출력 삭제 자격·지우는/남기는 범위·원장 일관 재작성·중단 재개·화면 흐름; 아직 제품 동작 아님 |
| **M5 작업 공간 전환 설계** | [`proposals/m5-real-worktrees.md`](proposals/m5-real-worktrees.md) | 독립 복사본을 실제 git worktree(작업당 브랜치)와 git 없는 폴더의 그림자 git으로 바꾸는 설계·바뀌는 안전 원칙·구현 순서·열린 결정; 아직 제품 동작 아님 |
| **M5 Orca 벤치마킹** | [`proposals/m5-orca-benchmark.md`](proposals/m5-orca-benchmark.md) | 에이전트 실행·권한·실제 git worktree·변경 관리를 Orca와 비교하고 가져올 점·기존 원칙과 충돌·진행 순서 제안; 아직 제품 동작 아님 |
| **M5 프로젝트·작업 삭제** | [`proposals/m5-project-deletion.md`](proposals/m5-project-deletion.md) | 목록 삭제를 숨기기 대신 삭제 표시+앱 소유 파일 삭제로, 같은 폴더 재추가는 새 ID로 시작; 원장 행은 남김 |
| **M5 ADE 고도화 전 진단(2026-11-25)** | [`proposals/m5-ade-diagnosis-2026-11-25.md`](proposals/m5-ade-diagnosis-2026-11-25.md) | 작업대 디자인 문제 D1~D13·기능 공백 F1~F14·방향 A/B/C·1~3차 로드맵; 스크린샷은 원본 폴더 경로만 |
| **M5 ADE 1차-나 레이아웃 계획** | [`proposals/m5-ade-layout-plan.md`](proposals/m5-ade-layout-plan.md) | 방향 A 뼈대 + C 미리보기 + B 상태별 목록의 3영역 레이아웃·결과 미리보기 안전 설계·작업 묶음과 1차-가 충돌 표; 아직 제품 동작 아님 |
| **M5 나머지 화면 디자인 통일 계획** | [`proposals/m5-ade-screens-plan.md`](proposals/m5-ade-screens-plan.md) | 앱 전체 화면 57개 목록·어긋남, 앱 전체 규칙·토큰(다크/라이트)·컴포넌트, 목업 17화면, 작업 묶음 U0~U8. U0·U1 구현 뒤 현행 동작은 architecture "색·간격 토큰과 화면 밝기" 절 |
| **M5 사용자 정의 팀 역할 계획** | [`proposals/m5-custom-roles-plan.md`](proposals/m5-custom-roles-plan.md) | 역할 동적화 조사·설계·구현 순서와 1차-가·1차-나와의 경계. R0·R1·R2·R4 구현 뒤 현행 동작은 architecture "팀 역할 편집" 절 |

> ⚠️ **`proposals/`는 구현된 것이 아니다.** 여기 있는 내용을 현재 동작으로 인용하지 말 것. 이 경계가 이 분류의 존재 이유다.

---

## 어느 문서를 언제 여나

**작업을 시작할 때 → [`history.md`](history.md)**
현재 상태와 인계의 기준. `핵심 결정·발견`, `작업 규칙`, 최신 워크로그, 미완료 `다음 할 일`을 확인한다. 세션 종료 시 새 엔트리를 추가하는 곳도 여기다. 규칙은 루트 `CLAUDE.md` / `AGENTS.md` 참조.

**코드 구조를 알아야 할 때 → [`architecture.md`](architecture.md)**
Electron 화면·main·Rust sidecar·공급자 CLI의 구성과 계층별 책임, M5 작업대 구현 기록과 미해결 사항.

**배포할 때 → [`release-checklist.md`](release-checklist.md)**
자동 검증, 깨끗한 기기 확인, 실패 경로, 미서명 베타와 정식 서명 배포 게이트.

**M5 성능을 측정할 때 → [`procedures/m5-performance.md`](procedures/m5-performance.md)**
release sidecar 시작 실측과 전체 측정 JSON 판정의 실행법·입력 형식·남은 G6 수집 범위.

**직접 개발과 위임 방식을 선택할 때 → [`procedures/multi-agent-orchestration.md`](procedures/multi-agent-orchestration.md)**
메인이 직접 개발하는 것을 기본으로 한다. 위임이 더 유리하면 이유와 범위를 설명하고 대기한다. 승인된 병렬 작업에만 worktree·파일 소유권·다중 역할 절차를 적용한다. 2026-11-12 사용자 결정으로 개발용 커스텀 역할·고정 모델 설정을 제거했다. 필요 시 기본 에이전트 1명에게 읽기 전용 독립 검토를 맡기며 현재 모델/추론 설정을 상속한다.

**M5(코딩 에이전트 IDE)를 검토할 때 → [`proposals/m5-coding-agent-ide.md`](proposals/m5-coding-agent-ide.md)**
여러 영속 채팅, 역할별 에이전트·모델, standalone snapshot, 코드 편집·diff를 포함한 agent-first IDE의 승인된 제품 방향·설계 계약·열린 결정. **현재 구현 구조와 게이트는 `architecture.md`와 `history.md`가 기준**이며 이 제안서의 완료 기준을 현재 동작으로 인용하지 않는다. 새 제품 이름은 개발과 분리해 보류한다.

**M5 성능 합격선과 초기 구현 계약을 확인할 때 → [`proposals/m5-electron-spike.md`](proposals/m5-electron-spike.md)**
Electron/Rust sidecar 수직 스파이크의 구현 명세. 지금도 §9–13의 수치 합격선을 성능 측정(`procedures/m5-performance.md`)과 배포 체크리스트가 기준으로 쓴다. 현재 구현 구조와 게이트는 `architecture.md`와 `history.md`를 우선한다.

**G5D 압축 기록 재검증의 후속 계약을 확인할 때 → [`proposals/m5-storage-reopen-verification.md`](proposals/m5-storage-reopen-verification.md)**
V8을 유지하는 내부 R1 frozen-operation·R2 압축 관련 source/prefix/message·R3A ledger/projection·R3B 파생 기록 검증 계약과 R3C 전체 재오픈 활성화의 남은 게이트. §10은 정상 저장 snapshot의 크기 호환성 진단, §11–12는 큰 기록의 private byte transport, §13은 legacy oracle, §14는 canonical scanner, §15는 한 행 의미/SQLite mirror 비교, §16은 승인된 분할 처리 정책과 전역 검증→catch-up→ready 재검증 계약이다. 구현 사실은 `architecture.md`에서 확인하며 내부 단계의 성공을 제품 ready 또는 100개 초과 재오픈 지원으로 해석하지 않는다.

**M5 작업대 화면을 직접 조작해 볼 때 → [`proposals/m5-agent-ide-concept.html`](proposals/m5-agent-ide-concept.html)**
여러 프로젝트, 재배치 가능한 패널, 파일·변경 탐색, 프로젝트별 역할/모델 설정과 역할 간 팀 대화를 담은 독립 실행형 인터랙티브 시안. 제안서의 동반 검토 자료이며 제품 구현이 아니다.

**M5 처음 시작하기 흐름을 직접 조작해 볼 때 → [`proposals/m5-guided-start-concept.html`](proposals/m5-guided-start-concept.html)**
에이전트 진단·설치·로그인부터 실제 프로젝트 선택, 프로젝트별 역할 배정, 격리 작업, 팀 대화와 첫 diff 검토를 거쳐 작업대로 이어지는 독립 실행형 인터랙티브 시안. 제품 구현이 아니다.

**옛날 기록을 찾을 때 → [`archive/`](archive/)**
`history.md`에서 롤오버된 과거 워크로그(2026-11-13에 Elsecrew 이전 기록은 정리했다). 재사용할 결정·함정은 `history.md`의 `핵심 결정·발견`에 반영돼 있으므로 통상 열 필요가 없다.

---

## 문서를 새로 만들 때

1. **어느 분류인지 먼저 정한다.** 애매하면 "이 문서를 현재 사실로 인용해도 되나?"를 물어본다 — 아니면 `proposals/`.
2. **`docs/` 최상위는 늘어나지 않게 한다.** 최상위는 진입점(`history.md`)과 오래 사는 단일 문서(`architecture.md`·`release-checklist.md`)만 둔다. 나머지는 분류 폴더로.
3. **이 파일의 표에 한 줄 추가한다.** 색인에 없는 문서는 다음 작업자가 못 찾는다.
4. **제안이 채택되면** `proposals/`에서 꺼내 현행 문서(`architecture.md` 등)에 반영하고, 제안 문서에는 반영 위치를 남긴다.

## 롤오버 규칙

`history.md`가 **500줄을 넘으면** 오래된 엔트리부터 `archive/history-YYYY-MM.md`로 옮겨 **300줄 이하로 줄인다.** 날짜 경계에서 자르고, 삭제가 아니라 이동이며, 고정 섹션(`프로젝트 요약`·`핵심 결정·발견`·`작업 규칙`)은 옮기지 않는다.

**날짜별로 파일을 쪼개지 않는다** — 현재 진실이 흩어져 조회 비용만 커진다. 날짜별 조회는 `git log -- docs/history.md`가 한다.
