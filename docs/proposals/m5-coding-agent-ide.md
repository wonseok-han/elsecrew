# M5 — 코딩 에이전트 IDE (제품·설계 제안)

> **상태: 승인된 제품 방향과 설계 계약, 열린 결정을 관리하는 제안 문서.**
>
> 최초 작성 2026-08-24. 현재 구현 구조와 검증 상태는 [`../architecture.md`](../architecture.md), 작업 경과와 최신 게이트 판정은 [`../history.md`](../history.md)를 본다. 이 문서의 제안·완료 기준을 현재 동작으로 인용하지 않는다.
>
> 인터랙티브 시안: [`m5-guided-start-concept.html`](m5-guided-start-concept.html)은 설치·로그인부터 실제 프로젝트의 첫 변경 검토까지, [`m5-agent-ide-concept.html`](m5-agent-ide-concept.html)은 이후 여러 프로젝트 작업대를 직접 조작해 보는 동반 검토 자료다.

## 0. 이 문서를 읽는 법

이 문서는 결정을 세 종류로 나눈다.

- **확정된 사용자 결정**: 이번 방향을 논의할 때 이미 전제로 삼는다.
- **현재 제안**: 구현 전에 사용자가 검토·수정·승인할 초안이다.
- **열린 질문**: 답에 따라 범위나 구조가 달라져 사용자 결정이 필요하다.

M5의 목적은 기능 목록을 한꺼번에 확정하는 것이 아니라, **여러 영속 채팅·역할별 실행 구성·원본과 분리된 작업 복사본(standalone snapshot)·편집/diff·안전한 검토 루프**를 IDE의 필수 골격으로 정의하고 단계적으로 구현하는 것이다.

---

## 1. 확정된 사용자 결정

1. 기존 M0~M4 화면과 `HandoffPilot`은 버리지 않고 **최초 설정·튜토리얼 흐름**으로 유지한다.
2. 설정을 마친 사용자는 실제 프로젝트를 계속 다루는 **코딩 에이전트 IDE/workbench**로 진입한다.
3. 대상 사용자는 계속 코딩 초보자다. 초보자용이라는 뜻은 기능을 줄이는 것이 아니라 **복잡성을 점진적으로 노출**하는 것이다.
4. 제품 정의는 VS Code 복제가 아니라 **agent-first IDE**다.
5. 핵심 루프는 `프로젝트 선택 → 목표 입력 → 진행 관찰/후속 지시 → diff 검토 → 적용/폐기/복구`다.
6. M4 안전 계약을 약화하지 않는다: 명시적 대상 선택, 격리 작업 공간, 종료 확인, `poisoned` 잠금, 쉬운 오류와 상세 증거 분리.
7. 한 프로젝트에서 **여러 영속 채팅**을 만들고 다시 열 수 있어야 한다. 여러 채팅이 있다는 것과 여러 쓰기 실행을 동시에 허용하는 것은 구분한다.
8. 기획·코딩·검토 같은 **역할마다 에이전트 런타임, 모델, 추론 수준을 설정**할 수 있어야 한다. 예: `기획 = Codex / Sol / medium`, `코딩 = Claude Code / Opus 5`.
9. 에이전트 작업은 원본 Git 저장소를 쓰지 않는 **원본과 분리된 작업 복사본(standalone snapshot)** 으로 격리하고, 변경 파일과 줄 diff를 명확히 보여 준다.
10. IDE이므로 파일 탐색과 **실제 코드 편집**은 M5 완료 범위에 포함한다. 다만 VS Code의 모든 기능을 복제하지는 않는다.
11. `Hello, Agent`라는 제품명은 M5의 성격에 맞는 새 이름으로 바꾼다. 새 이름과 변경 범위는 별도 결정한다.
12. M5 통합 브랜치는 `m5/coding-agent-ide`다. Claude·Codex 기능 브랜치는 이 브랜치로만 합치며, `main` 병합은 M5 전체 완료·검증 뒤 사용자 승인 사항이다.
13. 온보딩의 첫 팀 작업은 별도 샘플이 아니라 **사용자가 명시적으로 고른 실제 프로젝트와 실제 목표**에서 진행한다. 사용자는 격리 작업·팀 대화·diff 검토를 별도 연습이 아니라 실제 일을 하며 자연스럽게 익힌다.
14. 초보자를 위한 **에이전트 선택 → 기기 진단 → 설치 → 로그인 → 연결 확인** 단계를 유지한다. `에이전트 연결`은 설치·로그인을 생략하는 새 단계가 아니라, 이 절차가 끝나 실제로 사용할 수 있음을 확인한 결과다.
15. 새 제품 이름은 계속 찾되 **M5.0 개발 착수 조건에서는 제외**한다. 이름이 정해질 때까지 `M5`와 기존 저장소·번들·데이터 경로를 사용하며 리브랜딩 마이그레이션을 시작하지 않는다.
16. 기술 기준선은 **Electron + React/TypeScript + Monaco + Rust sidecar**다. 기존 Tauri host를 M5 셸로 확장하거나 두 host를 동시에 구현하지 않는다. 검증된 M0~M4 Rust 안전 코어는 sidecar로 재사용한다.
17. 한 프로젝트에는 여러 영속 채팅이 있고, 첫 버전에서 **한 채팅은 하나의 변경 목표와 하나의 격리 작업 복사본을 소유**한다. 기획·개발·검토 역할의 왕복은 별도 채팅이 아니라 그 채팅의 공개 팀 대화에 함께 기록한다.
18. 기본 역할은 `기획/코딩/검토`이며 앱 기본값을 새 프로젝트의 시작값으로 사용한다. 프로젝트별로 재정의하고, 새 채팅은 생성 시 적용 구성을 고정해 기존 채팅이 설정 변경으로 조용히 바뀌지 않게 한다.
19. 첫 영속 저장은 **SQLite 이벤트 원장 + 현재 상태 projection**으로 나눈다. 첫 버전은 앱이 꺼진 뒤 CLI를 무인 관리하지 않고, 앱 종료 전에 안전 중단과 자손 소멸 확인을 요구한다.
20. 첫 적용 단위는 **작업 복사본 전체**다. 적용 직전 앱 전용 복원 지점을 만들며 파일별·줄 단위 반영은 후속으로 미룬다.
21. UI·저장 구조는 macOS와 Windows 공통으로 개발하지만, 에이전트 실행은 검증된 macOS부터 연다. Windows는 M4와 동등한 자손 회수·소멸 확인 계약을 통과하기 전 실행을 노출하지 않는다.
22. G4 작업 복사본은 linked Git worktree가 아니라, 읽기 전용 source capability로 만든 self-contained Git bundle에서 raw committed blob과 mode를 스트리밍 materialize한 **capability-protected standalone snapshot**이다. `project.createWorktree`, `createProjectWorktree`, `worktreeId/path`는 protocol v1 호환 이름으로만 남긴다. Windows G4 안전성은 별도 게이트 전까지 지원으로 광고하지 않는다.
23. G5D 제품 시작은 **3초 이내 shell-ready**와 별도 저장소 admission `verifying → ready | failed`로 나눈다. `verifying`·`failed`에서는 `system.status`와 `app.prepareQuit`만 허용하고, recovery 변경·app instance 등록·append·fixture/actual agent 실행 및 `storage.status` 사실값 조회는 sidecar가 known-no-op로 차단한다. 저장소 검증을 모두 통과하고 `verifiedForStartup` 뒤 app instance 등록까지 끝낸 경우에만 public `ready`로 admission한다.
24. G5D 자동 lossless compaction은 기존 안전 제외·완전 materialization·전후 동등성 검증을 모두 통과한 retained boundary 이후의 exact prefix가 **원장 event 1,000개 이상**일 때만 실행 후보가 된다. 재시작의 빈 authority `pending`만 admission에서 exact 검증한 뒤 ready 및 scheduler 재개를 허용하고, `running` sentinel·non-empty authority·commit 불명 상태는 변경 없이 admission을 실패시킨다. ready 이후 치명적 maintenance 오류는 공개 `ready → failed` 전이를 만들지 않고 추가 변경·실행을 차단한 채 sidecar를 종료한다.

---

## 2. 제품 정의

### 2.1 한 문장

**M5의 새 제품은 코딩 초보자가 여러 에이전트 채팅과 코드를 한 작업 공간에서 다루고, 역할에 맞는 모델에게 일을 맡기고, 원본과 분리된 작업 복사본의 변경을 검토·편집·반영·복구하는 agent-first IDE다.**

### 2.2 M0~M4와의 관계

```
최초 설정 / 다시 고치기                 계속 만드는 일
M0~M3 위저드·홈 + M4 HandoffPilot  →  M5 코딩 에이전트 작업대
설치·로그인·첫 프로젝트·안전 학습        작업 기록·진행·후속 지시·검토·적용·복구
```

- 위저드는 첫 진입과 에이전트·프로젝트가 고장 났을 때 다시 여는 **튜토리얼/정비소**다.
- 작업대는 설정 완료 뒤 매일 돌아오는 **기본 홈**이다.
- `HandoffPilot`의 자동 인계는 M5가 재사용할 수 있는 실행 방식 중 하나지만, M5의 정보 구조를 "Claude → Codex" 파이프라인에 고정하지 않는다. 기본 화면은 **역할과 일의 상태**를 말한다.

### 2.3 초보자용의 의미

처음에는 세 가지 질문에만 답하게 한다.

1. 어느 프로젝트에서 할까요?
2. 무엇을 바꿀까요?
3. 이 변경을 반영할까요?

세션 ID·프로세스·원시 로그·터미널은 기본 화면에서 숨긴다. 반면 채팅, 역할, 선택된 에이전트/모델, 파일, 편집기, diff는 IDE의 핵심이므로 숨기지 않고 쉬운 이름과 기본값을 제공한다. 문제 해결용 내부 정보는 **자세히 보기 → 개발자 도구** 순으로 단계적으로 연다.

### 2.4 이름 변경

**확정**: `Hello, Agent`는 M5 제품 정의에 맞는 새 이름으로 바꾼다.

**확정**: 이름을 먼저 짓고 제품을 맞추지 않는다. 작명은 M5.0 개발과 분리해 보류하며, 제품 셸과 첫 수직 루프가 실제로 동작한 뒤 다시 검토한다.

- 초보자에게 코딩 에이전트 IDE라는 성격이 전달되는가
- 특정 벤더나 단일 에이전트에 종속되지 않는가
- 한국어·영어에서 발음하고 검색하기 쉬운가
- 저장소명, 앱 표시명, 번들 ID, 설정/데이터 경로, 웹사이트 도메인을 단계적으로 이전할 수 있는가

새 이름이 정해질 때까지 이 문서와 구현에서는 `M5 제품`을 임시 호칭으로 쓴다. 기존 저장소명, 앱 표시명, 번들 ID, 설치 데이터 경로와 프로젝트 표식을 바꾸지 않는다.

**이름 방향 수정**: 1차 설명형 합성어 후보는 사용자 검토에서 전부 폐기했다. 다음 후보는 `code`, `agent`, `work`, `branch` 같은 기능 단어로 아키텍처를 설명하지 않고, Codex·Cursor·Orca처럼 짧고 독립적인 단어로 만든다. 최종 후보는 별도 상표·법인·도메인 가용성 조사를 거친다.

### 2.5 제품 기준점

**사용자 제안 방향**: 제품 형태는 Cursor/VS Code 복제보다 **Codex와 Orca에 가까운 자체 agent-first 작업대**로 잡는다.

**현재 제안**:

- Codex에서 가져올 것: 프로젝트·채팅·worktree·실행·diff 검토가 이어지는 단일 작업 루프, 명시적 적용/폐기/복구.
- Orca에서 가져올 것: 여러 에이전트 런타임과 모델을 한 작업대에서 선택하고, 채팅·파일·편집기·터미널·Git 상태를 함께 관찰하는 셸.
- M5가 다르게 할 것: M0~M4 설정/튜토리얼, 역할별 쉬운 기본값, M4의 종료 확인·`poisoned`·증거 계약으로 초보자에게 복잡성과 위험을 점진적으로 노출.
- 첫 버전에서 가져오지 않을 것: Cursor의 VS Code 전체 표면, Orca의 대규모 병렬 fleet, 원시 터미널 중심 사용법.

---

## 3. 벤치마크 (2026-08-24 재확인)

### 3.1 확인된 제품 패턴

| 제품 | 공식 자료에서 확인한 패턴 | M5가 배우는 점 |
| --- | --- | --- |
| Codex 데스크톱 | Local/Worktree/Cloud 실행 위치, Git worktree의 독립 채팅, Local↔Worktree Handoff, 채팅별 프로젝트 범위 터미널, diff 인라인 의견, chunk/file 단위 stage·revert와 commit·push·PR | 프로젝트·작업·실행 환경을 분리하고, 대화와 변경 검토를 한 흐름에 둔다 |
| Cursor | Agent가 만든 변경의 전체 diff, 파일/줄 단위 수락·거절, Agent 변경 전용 로컬 체크포인트, 중단·방향 전환 | 검토를 별도 개발자 도구가 아니라 작업 완료의 필수 단계로 만든다 |
| Orca (ADE) | agent-first ADE, 작업별 격리 worktree, 에이전트 상태 관찰, diff 줄 주석, 파일·터미널·Git을 한 작업 공간에 결합 | 전통 편집기보다 "에이전트가 하는 일을 지휘·관찰·검토"하는 셸이 우선이다 |
| Cebus | 여러 모델의 단일 대화, 역할별 에이전트·모델 배정, 채널·스레드 UI, CLI·웹·VS Code 표면 | M5의 역할별 모델과 여러 채팅 구상이 이미 가까운 형태로 구현되고 있다. 기능 복제보다 초보자 안전·격리 복사본 검토 루프에서 차별화해야 한다 |

### 3.2 그대로 채택할 패턴

- 프로젝트별 작업 목록과 읽지 않은/주의 필요 상태
- 프로젝트별 여러 영속 채팅과 채팅별 역할/모델 표시
- 한 작업 안에서 목표, 진행, 질문, 결과, 변경 검토를 이어 보는 타임라인
- 원본과 분리된 독립 작업 공간(Codex·Orca의 Git worktree 패턴을 M5에서는 standalone snapshot으로 구현)
- 파일별 변경 요약 → 필요한 경우 줄 diff로 내려가는 점진적 검토
- 같은 파일을 일반 편집과 diff 편집으로 오가는 흐름
- 변경에 의견을 달아 같은 작업에 후속 지시
- 명시적인 적용·폐기·복구 행동
- 앱을 껐다 켜도 남는 작업 기록과 증거 위치

### 3.3 초보자 기본 화면에서 숨길 패턴

| 숨기는 것 | 이유 | 여는 시점 |
| --- | --- | --- |
| 동시에 실행할 채팅 수·fan-out | 여러 채팅과 여러 프로세스 동시 실행은 다른 기능이다 | 단일 writer 계약이 안정된 뒤 고급 기능으로 |
| 다중 터미널·분할 패널 | 명령어와 프로세스 관리를 사용자에게 떠넘긴다 | 문제 해결용 개발자 도구에서 |
| LSP·디버거·확장 마켓 등 VS Code 전체 표면 | IDE 필수 편집과는 별개로 범위가 매우 크다 | 실제 수요와 기술 검증 뒤 |
| 브랜치·detached HEAD 같은 Git 내부 용어 | 안전 경계의 구현 상세다 | 자세히 보기에서 쉬운 설명과 함께 |
| stage·commit·push·PR의 전체 Git 표면 | M5 첫 루프는 로컬 적용/폐기/복구가 우선이다 | Git 경험이 필요한 후속 마일스톤에서 |
| 5개 에이전트 fan-out·승자 병합 | 비용·품질·충돌 모델이 아직 검증되지 않았다 | §7.6B 또는 별도 실험을 통과한 뒤 |

### 3.4 출처

- OpenAI 공식 문서: [Codex 환경](https://learn.chatgpt.com/docs/environments/modes), [Git worktrees와 Handoff](https://learn.chatgpt.com/docs/environments/git-worktrees), [코드 검토](https://learn.chatgpt.com/docs/code-review), [로컬 환경과 Git 도구](https://learn.chatgpt.com/docs/environments/local-environment), [통합 터미널](https://learn.chatgpt.com/docs/integrated-terminal)
- Cursor 공식 문서: [Diffs & Review](https://cursor.com/docs/agent/review), [Checkpoints](https://cursor.com/docs/agent/chat/checkpoints), [Agent security](https://cursor.com/docs/agent/security)
- Orca 공식 자료: [`stablyai/orca` 저장소](https://github.com/stablyai/orca), [Orca ADE 제품 사이트](https://www.onorca.dev/)
- Cebus 공식 자료: [`cebus-ai/cebus` 저장소](https://github.com/cebus-ai/cebus)
- 기술 선택 자료: [Electron 프로세스 모델](https://www.electronjs.org/docs/latest/tutorial/process-model), [Electron 보안 지침](https://www.electronjs.org/docs/latest/tutorial/security), [Node.js child process](https://nodejs.org/api/child_process.html), [Monaco Editor](https://microsoft.github.io/monaco-editor/), [Claude Code CLI 모델 지정](https://docs.anthropic.com/en/docs/claude-code/cli-usage)

---

## 4. 정보 구조 초안

### 4.1 최상위 구조

```
M5 제품 (임시 호칭)
├─ 작업대 (설정 완료 뒤 기본 진입)
│  ├─ 프로젝트 / 작업 복사본
│  ├─ 여러 채팅과 상태
│  ├─ 파일 탐색기 / 코드 편집기 / diff
│  └─ 현재 채팅: 대화·진행·질문·결과
├─ 새 채팅
└─ 설정과 도움
   ├─ 역할별 에이전트·모델
   ├─ 에이전트·로그인·업데이트
   ├─ 프로젝트 준비
   ├─ 튜토리얼 다시 보기 (M0~M4)
   └─ 문제 해결·상세 증거
```

### 4.2 화면 셸

**현재 제안**: IDE 필수 요소를 네 영역으로 나누되, 패널은 접거나 크기를 바꿀 수 있게 한다.

| 영역 | 기본 내용 | 초보자에게 보이는 말 |
| --- | --- | --- |
| 왼쪽 사이드바 | 프로젝트, 작업 복사본, 여러 채팅, 파일 탐색 | `채팅`, `파일`, `변경 5개` |
| 가운데 편집 면 | 코드 편집기와 side-by-side/inline diff 탭 | 파일명, 저장 상태, 변경 전/후 |
| 오른쪽 에이전트 면 | 선택한 채팅의 목표, 진행, 질문, 후속 입력 | 역할·에이전트·모델, `지금 하는 일`, `추가로 알려주기` |
| 아래 보조 면 | 문제, 검증 결과, 선택적 터미널/원시 증거 | 기본은 닫힘, 주의가 필요할 때만 배지 |

- 좁은 창에서는 편집 면과 에이전트 면을 탭으로 전환한다.
- 실행 중에는 에이전트 면의 진행을 강조하고, 변경이 생기면 편집 면에 diff 탭을 연다.
- 원시 로그·증거 경로·세션 ID는 `자세히 보기` 안에서만 표시한다.

### 4.3 점진적 노출 단계

1. **기본**: 여러 채팅, 역할 프리셋, 코드 편집, 목표, 평이한 진행 상태, 변경 파일, 반영/버리기.
2. **자세히**: 줄 diff, 정확한 모델/추론 수준, 실행한 검증, 에이전트의 이유, 복구 지점.
3. **개발자 도구**: 원시 이벤트·stderr·증거 경로·격리 폴더·터미널(후속).

---

## 5. 핵심 사용자 흐름

### 5.1 첫 설정에서 작업대로

기존 M0~M4 화면을 그대로 남기는 것이 아니라 **설치에 필요한 필수 설정**과 **새 작업대 사용법을 익히는 첫 팀 작업**으로 재구성한다. 설치·로그인·오류 복구 코어는 재사용하되, 튜토리얼의 졸업 기준을 "첫 대화를 보냄"에서 "팀 작업의 변경을 이해하고 안전하게 결정함"으로 바꾼다.

**현재 제안 흐름**:

1. **환영**: 이 앱이 사용자의 프로젝트를 대신 소유하는 것이 아니라, 여러 코딩 에이전트가 한 작업 공간에서 협업하도록 돕는다는 제품 약속을 한 문장으로 설명한다.
2. **에이전트 선택**: Codex·Claude Code 중 사용할 대상을 고른다. 둘 다 고르면 역할을 나눈 팀 대화를 쓸 수 있고, 하나만 골라도 단일 에이전트로 시작할 수 있다고 설명한다.
3. **기기 진단**: 운영체제·필수 도구·기존 설치·로그인·설치 온전성을 자동 확인하고, 문제가 있으면 쉬운 원인과 해결 행동을 보여 준다.
4. **설치**: 선택한 에이전트 중 설치되지 않았거나 망가진 항목을 공식 설치 경로로 설치/복구한다. 진행 로그는 쉬운 상태로 요약하고 원문은 자세히에 둔다.
5. **로그인**: 에이전트별 공식 인증 흐름을 안내하고 완료를 자동 감지한다. 로그인 대기·만료·브라우저 자동 승인 같은 기존 복구 흐름을 유지한다.
6. **연결 확인**: 설치·로그인·실행 가능 상태가 모두 확인된 에이전트만 `사용 가능`으로 표시한다. 하나만 준비돼도 계속할 수 있고, 선택한 둘 중 하나가 준비되지 않았다면 단일 역할로 계속할지 다시 고칠지 사용자가 고른다.
7. **첫 프로젝트 선택**: 기존 폴더를 추가하거나 새 프로젝트를 만든다. 대상은 사용자가 명시적으로 고르며 앱이 첫 번째 폴더를 추정하지 않는다.
8. **역할 배정**: 추천 기본값과 함께 `기획`, `개발`, `검토`에 어떤 에이전트·모델을 쓸지 보여 준다. 처음에는 추천값으로 계속 갈 수 있고 자세한 추론 수준·권한 설정은 작업대 설정에서 나중에 바꾼다.
9. **안전한 작업 방식**: 원본과 격리 작업 공간을 쉬운 그림으로 설명하고, `원본은 바로 바뀌지 않음`, `변경을 보고 반영/버리기`, `문제가 생기면 복구` 세 약속만 기본 화면에 남긴다.
10. **첫 팀 작업**: 앞에서 고른 실제 프로젝트에서 사용자가 실제 목표를 한 문장으로 입력한다. 앱은 원본과 분리된 작업 복사본을 준비한 뒤 `기획 → 개발 가능성 검토 → 필요 시 계획 수정 → 개발`이 한 채팅에서 진행되는 모습을 보여 준다. 기존 `HandoffPilot`은 이 수직 흐름의 안전 코어로 재사용하되 역할 조합을 Claude→Codex로 고정하지 않는다.
11. **첫 변경 검토**: 파일별 변경 요약에서 시작해 필요한 줄 diff만 펼치고, `반영하기`, `수정 요청`, `버리기`의 차이를 직접 선택하게 한다. 적용 전에는 원본과 시작 기준이 같은지 다시 확인한다.
12. **작업대로 이동**: 졸업식의 기본 행동을 `작업대로 가기`로 바꾼다. 방금 사용한 프로젝트·채팅·격리 작업 공간·검토 상태를 그대로 이어서 열며, 프로젝트 추가·새 채팅·설정 위치를 짧게 짚는다.

설치/로그인과 대상 프로젝트의 명시적 선택 같은 필수 안전 단계는 건너뛸 수 없다. 반면 역할 설명·팀 대화 연습·diff 안내는 이미 익숙한 사용자가 건너뛰거나 나중에 도움말에서 다시 열 수 있다. 세션 ID·프로세스·원시 로그·Git 내부 용어는 튜토리얼 기본 화면에 넣지 않는다.

앱은 튜토리얼 전용 샘플 프로젝트를 만들지 않는다. 온보딩은 실제 일을 가짜 연습 화면과 분리하지 않고, 사용자가 고른 프로젝트·목표·결과를 졸업 뒤 같은 작업대에서 그대로 이어 간다. 목표가 너무 크거나 불명확하면 임의로 축소해 실행하지 않고 기획 역할이 범위를 나눌 선택지를 제시하거나 `답변 필요`에서 멈춘다.

이후 앱 실행은 작업대로 가며, 에이전트 연결이나 안전 상태가 깨진 경우에만 해당 설정 단계 또는 닥터로 돌아간다. 전체 위저드를 처음부터 반복시키지 않는다.

### 5.2 새 채팅과 역할 선택

1. 사용자가 프로젝트를 **명시적으로 선택**한다.
2. 새 채팅 화면은 프로젝트의 `기획`, `코딩`, `검토` 역할 구성을 보여 준다. 바꾸려면 시작 전에 프로젝트 설정에서 수정한다.
3. 채팅을 만들 때 적용된 에이전트·모델·추론 수준·권한을 snapshot으로 고정한다. 이후 프로젝트 설정 변경은 새 채팅에만 적용하고 기존 채팅을 조용히 바꾸지 않는다.
4. 앱이 실행 가능 여부를 확인하고 원본과 분리된 작업 복사본과 복원 기준을 만든다.
5. 사용자가 목표를 평문으로 입력하면 채팅·작업 ID와 영속 기록을 실행보다 먼저 저장한다.
6. 에이전트가 일을 시작하고, 사용자는 다른 채팅을 열거나 앱을 떠날 수 있다. 첫 단계에서는 **한 작업 복사본당 쓰기 실행 1개**만 허용한다.

### 5.3 진행 관찰과 후속 지시

1. 화면은 `준비 중 → 요청 정리 중 → 만드는 중 → 확인 중`처럼 일의 이름을 표시한다.
2. 에이전트가 판단을 요구하면 작업은 `답변 필요`로 멈추고 질문·선택의 영향·안전한 기본값을 보여 준다.
3. 사용자는 실행 중에도 `중단`하거나 후속 지시를 남길 수 있다.
4. 후속 지시는 현재 실행을 무조건 끊지 않고, **다음 안전한 경계에서 반영**하거나 즉시 중단 후 재개할지를 앱이 명확히 표시한다.

#### 5.3.1 역할 사이의 팀 대화

**현재 제안**: 기획과 코딩에 서로 다른 에이전트를 지정했다면, 한 채팅 안에서 두 에이전트의 명시적 인계와 답변을 **팀 대화**로 보여 준다. 두 CLI가 서로 임의로 직접 연결되는 구조가 아니라 앱이 대화 원장과 실행 순서를 소유하고, 한 역할의 구조화된 산출물을 다음 역할에게 전달한 뒤 그 응답을 같은 타임라인에 기록한다.

예:

```text
사용자       로그인 만료 때 다시 로그인할 수 있게 고쳐줘
기획 · Codex  현재 흐름을 확인해 3단계 계획을 제안했어요
개발 · Claude 이 계획은 가능하지만 App.tsx만 바꾸면 테스트가 빠져요
기획 · Codex  맞아요. doctor 테스트 추가를 포함하도록 계획을 고쳤어요
개발 · Claude 수정과 테스트를 마쳤어요. 3개 파일을 검토해 주세요
```

기본 화면에 보여 주는 것은 다음과 같다.

- 역할이 상대 역할에게 보낸 **명시적 요청·계획·제약 조건**
- 개발 역할의 가능 여부, 빠진 조건, 위험, 대안 같은 **실행 가능성 검토**
- 검토를 반영해 고친 계획과 최종 구현·검증 요약
- 사용자 결정이 필요한 질문과 답변, 중단·재시도·실패 상태
- 각 메시지의 역할, 에이전트, 모델, 실행 시각과 대응 `runId`

모델의 비공개 추론 과정이나 원시 chain-of-thought를 표시하거나 저장 대상으로 요구하지 않는다. 원시 CLI 이벤트·도구 호출·stderr는 사용자 대화와 분리해 안전 필터를 거친 **상세 증거**로만 보존한다. 기본 타임라인에는 에이전트가 외부에 내보내기로 확정한 메시지, 구조화 산출물, 도구 실행 상태와 결과 요약만 투영한다.

첫 버전의 역할 왕복은 무제한 자율 토론이 아니다. `기획 → 개발 가능성 검토 → 필요하면 기획 수정 → 개발`처럼 목적이 분명한 직렬 단계와 작은 상한을 두고, 합의하지 못하거나 범위·비용·안전 경계가 달라지면 `답변 필요`로 멈춘다. 기획은 계속 읽기 전용이고 쓰기 역할은 한 작업 복사본에서 하나만 실행하며, 대화가 추가돼도 M4의 종료 확인·`poisoned` 잠금·명시적 대상 선택 계약은 약해지지 않는다.

### 5.4 변경 검토

1. 실행이 끝나면 `검토할 변경` 상태가 된다. 완료로 바로 확정하지 않는다.
2. 먼저 "무엇을 왜 바꿨는지"와 검증 결과를 쉬운 말로 보여 준다.
3. 다음으로 파일별 `추가됨/바뀜/삭제됨/이름 바뀜`을 보여 준다.
4. 사용자는 파일별 검토 상태를 남기고, 전체 작업 복사본에 대해 `반영하기`, `버리기`, `수정 요청`을 선택한다.
5. 줄 단위 diff는 사용자가 펼칠 때만 보인다.

### 5.5 적용·폐기·복구

- **반영하기**: 원본의 현재 상태가 작업 시작 기준과 맞는지 다시 확인한 뒤, 전체 작업 복사본에서 어떤 파일이 바뀌는지 최종 확인을 받고 적용한다. 자동 충돌 해결과 파일별·줄 단위 부분 반영은 하지 않는다.
- **버리기**: 격리 공간을 `abandoned`로 표시하고 다시 실행하지 않는다. 증거는 정책에 따라 보존한다.
- **되돌리기**: 적용 직전 만든 복원 지점으로 되돌린다. 어떤 사용자 변경이 사라질 수 있는지 먼저 비교하고, 불명확하면 자동 실행하지 않는다.
- **앱 재시작**: 미완료 작업은 목록에 남는다. 실행 생존과 격리 공간 상태를 확인할 수 없으면 `복구 필요`로 두고 새 실행·적용을 막는다.

### 5.6 코드 편집

1. 사용자는 파일 탐색기에서 작업 복사본의 파일을 열고 직접 편집·저장할 수 있다.
2. 에이전트 변경과 사용자 직접 편집은 같은 작업 복사본 diff에 나타나되, 가능하면 변경 주체와 시각을 구분한다.
3. 저장되지 않은 편집이 있으면 에이전트 실행·작업 복사본 폐기·적용 전에 명확히 알린다.
4. diff에서 파일을 열면 같은 편집기 탭으로 이동하고, 편집 뒤 diff가 즉시 갱신된다.
5. M5 필수 편집 범위는 열기·수정·저장·검색·기본 구문 강조·diff다. LSP 자동완성·디버거·확장 생태계는 별도 범위다.

---

## 6. 작업·실행·검토 상태 모델 초안

### 6.0 핵심 엔터티

```
Project
  ├─ Chat 1 (변경 목표 하나의 영속 대화)
  │   ├─ 작업 복사본 (Chat과 1:1인 격리·적용 단위)
  │   ├─ Task (첫 버전에서는 활성 수명주기 하나)
  │   │   ├─ TeamMessage[] (사용자·역할·시스템의 공개 타임라인)
  │   │   ├─ Run[] (역할별 실제 실행과 증거)
  │   │   └─ Review
  │   └─ EditorState (열린 파일, 저장되지 않은 편집)
  └─ Chat 2
```

- **Chat**은 하나의 변경 목표와 작업 복사본을 소유하며 여러 개 만들고 다시 열 수 있는 사용자 대화 단위다.
- **Task**는 그 채팅의 실행·질문·검토 상태 수명주기다. 첫 버전에서는 채팅마다 활성 Task 하나만 두며 새 변경 목표는 새 채팅으로 시작한다.
- **Run**은 특정 역할·에이전트·모델로 실행한 한 회차다.
- 역할별 발언은 별도 채팅을 만들지 않고 같은 Task의 `TeamMessage`에 출처와 함께 쌓는다.
- 여러 채팅을 보존하더라도 첫 단계에서는 한 작업 복사본의 writer 실행을 직렬화한다. 읽기 전용 기획/검토의 병행 허용 여부도 별도 검증 전에는 열지 않는다.

### 6.1 작업 상태

```
draft
  → preparing
  → running
  → waitingForUser ─→ running
  → reviewReady
  → applying → completed
              ↘ needsAttention

어느 실행 상태에서나 → stopping → canceled | recoveryRequired
reviewReady → discarded
```

| 상태 | 사용자에게 보이는 말 | 허용 행동 |
| --- | --- | --- |
| `draft` | 새 작업 | 목표 수정, 시작 |
| `preparing` | 준비 중 | 중단 |
| `running` | 요청 정리/만드는 중/확인 중 | 관찰, 후속 지시, 중단 |
| `waitingForUser` | 답변 필요 | 답변, 중단 |
| `reviewReady` | 검토할 변경 | 반영, 버리기, 수정 요청 |
| `applying` | 반영 중 | 중복 행동 차단 |
| `completed` | 반영 완료 | 결과 보기, 되돌리기, 새 작업 |
| `needsAttention` | 확인이 필요해요 | 쉬운 해결책, 자세한 증거, 폐기/복구 |
| `recoveryRequired` | 안전 확인이 필요해요 | 새 실행·적용 차단, 복구 또는 폐기 |
| `canceled` / `discarded` | 중단됨 / 버림 | 기록 보기, 새 작업 |

### 6.2 실행 단계와 작업 상태를 분리한다

`running`이라는 작업 상태 안에 `planning | implementing | verifying` 실행 단계를 둔다. 에이전트 이름은 증거에 남기되 기본 진행 문구에는 쓰지 않는다. 한 작업은 재시도로 여러 실행을 가질 수 있으며, 이전 실패 증거를 덮어쓰지 않는다.

### 6.3 역할별 에이전트·모델 프로필

역할 설정은 에이전트 이름을 문자열 하나로 합치지 않고 독립 필드로 저장한다.

| 필드 | 예 | 규칙 |
| --- | --- | --- |
| 역할 | `planning`, `coding`, `review` | 사용자가 이해할 표시명과 안정적인 내부 ID 분리 |
| 에이전트 런타임 | `codex`, `claude-code` | 설치·로그인·실행 adapter 선택 |
| 모델 선택자 | `sol`, `opus` 또는 벤더의 전체 모델 ID | 실행 직전 adapter가 실제 지원 목록에서 해석 |
| 추론 수준 | `medium`, `high` 등 | 해당 모델이 지원하는 값만 노출 |
| 권한 프로필 | `readOnly`, `workspaceWrite` | 역할보다 안전 경계가 우선하며 임의 우회 금지 |

예시 표시:

```
기획  Codex · Sol · medium · 읽기 전용
코딩  Claude Code · Opus 5 · 작업 복사본 쓰기
검토  Codex · Sol · high · 읽기 전용
```

모델 이름과 지원 추론 수준은 바뀔 수 있다. 앱은 오래된 목록을 하드코딩해 조용히 다른 모델로 바꾸지 않는다. 선택한 모델을 사용할 수 없으면 실행 전에 쉬운 오류와 대체 가능한 목록을 보여 주고, **사용자 확인 없이 fallback하지 않는다.** Claude Code는 공식 CLI가 alias 또는 전체 모델명을 `--model`로 받을 수 있음을 확인했다. Codex도 실제 설치 버전/런타임이 제공하는 모델 목록을 adapter 계약으로 읽어야 한다.

역할 구성의 우선순위는 `앱 기본값 → 프로젝트 재정의 → 채팅 생성 snapshot`이다. 첫 버전은 채팅별 임시 override UI를 두지 않는다. 프로젝트 설정을 바꿔도 이미 생성된 채팅과 실행 증거의 정확한 구성이 유지돼야 한다.

### 6.4 최소 영속 기록

**확정**: SQLite에 불변 이벤트 원장과 재생성 가능한 현재 상태 projection을 분리해 저장한다. 파일 경로만 모은 JSON을 "원장"이라고 부르지 않는다.

- `chatId`, `taskId`, 프로젝트 신원, 사용자 목표, 생성·수정 시각
- 역할 프로필과 실행 당시의 정확한 에이전트·모델·추론 수준
- 작업 시작 커밋/트리·복원 지점, 작업 복사본 경로와 매니페스트
- 현재 작업 상태와 상태 전이 시각·이유
- 실행별 `runId`, 역할, 세션 ID(있을 때), 종료·자손 회수 판정
- 사용자 질문·답변·후속 지시
- 역할 사이에 전달한 요청·구조화 산출물·가능성 검토·수정 계획과 각 메시지의 역할/실행 출처
- 변경 파일 목록, 검증 요약, 원시 증거 경로
- 검토 결정과 적용/폐기/복구 결과
- `poisoned`/표시 저장 실패를 구분한 잠금 정보
- 열린 편집기 탭과 저장되지 않은 편집 여부(파일 내용 자체의 중복 저장 여부는 스파이크에서 결정)

저장은 **실행 시작 전**, 역할 시작·종료, 사용자 질문 전, 검토 준비, 적용·폐기·복구 직후에 이뤄져야 한다. 저장 실패 시 다음 위험 행동으로 진행하지 않는다.

앱 종료 시 첫 버전은 살아 있는 CLI를 백그라운드 서비스로 넘기지 않는다. 사용자에게 실행 중임을 알리고 안전 중단을 시작하며, 누적 추적한 자손의 소멸과 최종 상태 저장을 확인하기 전에는 정상 종료로 보고하지 않는다. 강제 종료 등으로 확인이 끊기면 다음 실행에서 `recoveryRequired`로 복원해 새 실행과 적용을 막는다.

### 6.5 불변식

1. 한 작업의 대상 프로젝트는 실행 중 조용히 바뀌지 않는다.
2. 종료를 확인하지 못한 실행은 성공·실패가 아니라 `recoveryRequired`다.
3. `poisoned` 격리 공간에서는 새 실행과 적용을 할 수 없다.
4. 검토 결정 없이 원본 프로젝트에 반영하지 않는다.
5. 원본이 시작 기준에서 달라졌으면 자동 적용하지 않는다.
6. 사용자가 보지 못한 삭제·이름 변경·바이너리 변경을 "변경 없음"으로 보고하지 않는다.
7. 쉬운 요약이 원시 증거보다 강한 안전 보장을 주장하지 않는다.
8. 선택한 에이전트·모델을 사용할 수 없을 때 다른 모델로 조용히 바꾸지 않는다.
9. 한 작업 복사본에서 두 writer 실행이 동시에 파일을 바꾸지 않는다.
10. 저장되지 않은 사용자 편집을 실행·적용·폐기가 조용히 덮지 않는다.

### 6.6 G5D 저장 수명 D0A 계약

#### 확정된 범위

- 실제 Claude Code/Codex provider를 제품 경로에 활성화하기 전에 여러 채팅·세션에 걸친 SQLite 전체 수명을 측정하고, 기록을 잃지 않는 내부 정리 경계를 검증한다.
- D0A는 **무삭제 계약**이다. 채팅·메시지·실행·복구·증거·standalone snapshot을 자동 또는 물리 삭제하지 않고, cleanup 후보 조회나 purge action도 공개하지 않는다.
- 보존 기간, 전체 용량 상한, 경고/차단 임계값, 자동 정리, 폐기된 작업 복사본 정리와 downgrade 지원은 D0A가 임의로 정하지 않는다.

#### 확정된 시작·admission 계약

- `system.ready`는 process/shell 사용 가능 신호로 먼저 한 번 내보낸다. additive `system.status`는 최상위 `shellReady`와 저장소 `verifying|ready|failed`를 함께 돌려주며, `verifiedForStartup`은 공개 상태가 아닌 내부 순서 증거다.
- `verifying`은 완료 시각·오류·복구 판정을 아직 주장하지 않는다. `ready`만 healthy schema v1과 기존 `recoveryRequired`/`blockerIds` 불변식을 공개하고, `failed`는 원시 DB 진단 대신 고정된 `storageAdmissionFailed` 공개 오류만 제공한다.
- 저장소가 `verifying`이면 차단 요청은 retryable `storageVerifying`, `failed`이면 non-retryable `storageAdmissionFailed`다. 두 경우 모두 `requestBlocked`, `outcomeUnknown=false`, known-no-op이며 입력 검증이나 부작용을 admission gate보다 먼저 수행하지 않는다.

#### 확정된 scheduler·compaction activation 계약

- 자동 실행 임계값은 retained boundary 이후 exact safe prefix의 원장 event **1,000개** 하나뿐이다. age·byte trigger와 1,000개 미만 compaction은 없으며, 기존 안전 제외·완전 materialization·exact 전후 동등성을 모두 먼저 증명한다.
- user chat/message/evidence, recovery, workspace, snapshot, baseline, projection, receipt는 삭제하지 않는다. 검증된 prefix의 source `events`와 대응 `event_source_index` 행만 논리 삭제할 수 있고 물리 cleanup은 계속 비활성이다.
- sidecar event loop의 단일 SQLite owner만 public ready이며 quitting이 아닐 때 cooperative tick을 수행한다. 우선순위는 (1) pending/active materializer의 bounded work 한 step, (2) exact admission을 통과한 persisted pending compaction 재개, (3) volatile indexed keyset 후보 탐색 순서다. materializer가 ready에 도달한 tick에는 compactor를 실행하지 않고 다음 tick까지 기다리며, 기존 pending compaction은 새 후보보다 항상 먼저 처리한다. 후보 aggregate type은 `project → chat → run → lock` 고정 순서이고 active target은 single-row join으로 찾는다. `OFFSET`·전체 `Vec`·temp table·unbounded `DISTINCT`/scan은 금지한다. 후보 없는 전체 sweep 뒤 dormant가 되고 append commit 또는 materialization pending에서만 깨운다.
- 한 tick은 bounded work commit 최대 하나만 허용한다. compaction work 직전의 committed `running` arm sentinel은 예외적으로 바로 인접할 수 있지만 arm과 work 사이 request/event polling은 없다. candidate job+plan 생성과 `pending → succeeded` finalize는 각각 work가 아닌 administrative commit으로 tick 하나를 소비하고 즉시 반환한다. 생성한 후보를 같은 tick에서 arm/work하지 않고 finalize는 arm하지 않는다. private writer와 scheduler의 compactor 결과는 `created | armed-worked | finalized | no-work`를 구분하고 writer 결과를 그대로 보존한다. 별도 writer connection/thread는 만들지 않고 work 100, primary 1 MiB, retained+dependency 8 MiB 한도를 완화하지 않는다.
- 입력 응답을 flush한 뒤 최대 tick 하나만 수행하고, active scheduler의 timeout tick은 100 ms다. dormant timeout은 DB를 열지 않으며 EOF에는 tick이 없다. `prepareQuit`은 checkpoint와 clean marker보다 먼저 새 tick을 차단한다. V9와 V10 materializer 및 V10 compaction writer의 exact runtime 지원 전에는 scheduler/runtime/product execution을 켜지 않는다.
- 재시작 시 empty-authority `pending`은 admission에서 exact 검증한 뒤 ready가 될 수 있고 scheduler가 이후 재개한다. `running` sentinel, non-empty authority, ambiguous/commit-uncertain state는 자동 reset/resume하지 않고 mutation 없이 admission을 실패시켜 수동 복구를 요구한다.
- 이 `pending` 재개 판정은 V8의 source cursor를 신뢰하지 않는다. additive storage V9는 job에 묶인 immutable plan header와 ordinal `1..1001`의 `WITHOUT ROWID` witness를 만들고, private state schema 3의 `afterPlanOrdinal`까지 source·receipt·message·baseline과 digest를 bounded 재검증한 경우에만 재개한다. 각 arm은 nullable `stepThroughPlanOrdinal`로 다음 최대 100개를 먼저 고정하고 plan INSERT/release DELETE trigger가 같은 transaction에서 `afterPlanOrdinal`을 전진시킨다. ordinal `1..1000`은 exact delete decision, `1001`은 남는 anchor이며 release 완료·finalize도 `afterPlanOrdinal=1000`을 보존해 삭제 진척과 anchor 보존을 함께 증명한다. 기존 active V2 `pending`은 증거가 부족하므로 자동 변환하지 않고 admission을 실패시킨다.
- plan 작성·freeze·검증·authority·삭제·동등성·plan release는 각 100개 이하의 연속 ordinal step과 같은 transaction의 cursor로 결합한다. `maintenance_jobs`의 V3 `state`·private cutoff·`failure_kind`·`completed_at`을 바꾸는 모든 UPDATE는 단일 `BEFORE` guard가 exact no-op·arm·bounded progress·running→pending commit·인접 phase·release/finalize·terminal failure만 허용하고, phase jump/backward·cursor/flag 위조·필수 plan/prefix/anchor 증거 부재를 거부한다. SQL의 NULL은 허용 판정으로 흐르지 않으며 terminal timestamp는 RFC 3339 UTC seconds 형식과 실제 달력을 SQL에서 검사하고 Rust/TypeScript runtime도 다시 검증한다. 성공/실패 뒤 terminal evidence rewrite는 금지한다. V10에서 `authorizeDelete`는 bounded read-only final witness이고 authority INSERT는 금지한다. 실제 authority는 `deletePrefix`의 armed `(afterPlanOrdinal, stepThroughPlanOrdinal]`에 해당하는 exact plan/prefix/live source 전부를 1부터 연속 batch ordinal로 준비한 뒤에만 source index 삭제를 시작할 수 있다. batch 전체 source index가 사라진 뒤에만 event 삭제를 허용하며, 각 event의 `BEFORE DELETE`는 아직 남은 `live event + authority`와 이미 처리한 `event 부재 + authority 부재`가 전체 plan batch를 정확히 분할하는지 다시 검사한다. event 삭제가 authority를 소비하고 마지막 소비가 source/index 완전 부재를 확인한 뒤 같은 transaction에서 cursor를 전진시켜 `pending`으로 돌린다. 잘못되거나 부분적인 batch 또는 event-before-index 순서는 transaction 전체를 rollback한다. append-after-cutoff는 삭제 대상이 아니라 동등성의 retained tail에 포함한다. 성공 뒤 plan row만 동등성 확인 후 bounded release할 수 있으며 immutable header와 성공/실패 terminal evidence는 유지한다. 여러 job은 immediate predecessor 하나를 indexed lookup으로 검증하며 chain traversal을 activation 조건으로 두지 않는다.
- public ready 이후 `running` sentinel·non-empty authority·복수 active job·malformed persisted state·materializer/compactor 오류는 내부 scheduler를 poison하고 이후 mutation/run을 금지한 채 process error를 반환하고 sidecar를 종료한다. 자동 reset이나 공개 `ready → failed` 전이는 만들지 않는다. 공개 G5D·`storage.status` shape와 capability 순서는 그대로이며 다음 process는 `verifying`부터 admission을 다시 수행한다.

#### 현재 제안

- protocol v1과 payload `schemaVersion: 1`을 유지하면서 DB logical/physical bytes, WAL bytes, ledger tail event count/payload bytes, snapshot aggregate count/payload bytes와 볼륨 capacity/available 사실값만 돌려주는 exact `storage.status`를 추가한다. raw chat/provider text, 비공개 추론, prompt, token/auth, absolute path는 응답에 넣지 않는다.
- public bridge는 additive `M5BridgeV6.getStorageStatus`와 새 G5D `getSystemStatus` 응답을 사용한다. protocol method와 D1 exact capability에는 `storage.status`가 예약됐지만 sidecar dispatcher·renderer 활성화는 저장소 admission 구현과 양쪽 검증이 함께 준비된 뒤에만 연다.
- 내부 SQLite schema v2는 v1 원장과 projection을 변경·삭제하지 않는 additive table/index/metadata migration으로 `ledger_baselines`, immutable operation receipt, normalized chat message/prepared workspace, maintenance job을 수용한다. 이후 V3~V8의 bounded 검증·source index·compaction/frozen source 계약을 거쳐 private V9는 V8에 resume decision witness table과 versioned guard를 additive로 더했다. 실제 V8→V9 SQL도 `BEGIN IMMEDIATE` 안에서 persistent DDL보다 먼저 V8의 모든 non-autoindex table/index/trigger에 대해 canonical `sqlite_schema.sql` 정의를 exact 비교하고, active pending/running 부재와 empty delete authority까지 확인한 뒤 하나라도 다르면 persistent schema를 V8 그대로 둔 채 transaction 전체를 rollback한다. prefix 생성은 plan 1,001개가 gap 없이 완성되고 `verifyPrefix` phase의 현재 증거가 갖춰진 때만 허용하되 미래 `equivalenceVerified`를 요구하지 않으며, release는 equivalence 뒤 다음 연속 delete ordinal 최대 100개만 허용한다. 기존 baseline·message·frozen-message digest는 exact `sha256:<64 lowercase hex>`를 유지하고 V9 plan witness만 접두사 없는 64자리 lowercase hex를 저장한다. SQL은 기존 값의 exact `sha256:` 접두사·알고리즘·hex를 확인해 접두사를 제외한 plan 값과 비교하고, operation receipt는 별도 `sha256`/v1/`lowercaseHex64` 열과 raw hex를 유지하며 runtime Rust가 canonical payload·baseline·message를 반드시 다시 hash한다. prepared workspace는 `bound`일 때만 `boundChatId`가 있고 그 chat이 같은 project임을 composite FK로 강제한다. 제품 admission은 V9 migration과 read-only resume witness 검증까지 구현됐지만 scheduler/delete writer는 아직 활성화되지 않았다. additive V10 계약은 이미 제품이 만들 수 있는 V9를 제자리 수정하지 않고 exact idle V9만 선행 검사한 뒤 authority guard를 `deletePrefix` 전용 atomic batch로 교체한다. V10의 모든 non-autoindex object는 `(type, name, tableName, canonical sqlite_schema.sql SHA-256)` golden으로 고정해 같은 이름의 약화 trigger도 identity mismatch로 판별한다. V3 private schema·10개 phase·flags, idle/succeeded 기록과 public D1 wire/capability는 유지하며 V10 fixture 자체는 runtime writer나 product readiness를 뜻하지 않는다.
- lossless compaction 구현은 확정된 activation 계약 아래 `BEGIN IMMEDIATE → cutoff 고정 → receipt/message/workspace/baseline materialize → baseline+tail replay·projection·old operation duplicate/conflict·actual/legacy namespace·recovery 동등성 검증 → 검증된 cutoff만 논리 삭제 → append-only guard 복원 확인 → COMMIT` 순서를 따른다. 어느 검증도 실패하면 전체 rollback하며, checkpoint와 `VACUUM` 성공은 논리 성공 조건이 아니다.
- active/cancelling/recoveryRequired run, unresolved recovery, poisoned/writer lock, pending intent/save, incomplete operation, active/bound workspace, 역할 설정, 사용자 chat/history/evidence, identity mismatch, orphan/symlink 불명 상태는 정리 대상으로 분류하지 않는다.

#### 열린 질문

- 폐기된 standalone snapshot을 사용자가 직접 정리할지, 안전 조건을 통과한 항목만 자동 정리할지 결정하지 않았다.
- 사용자 대화·증거·snapshot의 보존 기간과 전체 용량 상한, 압박 상태에서 새 작업을 막을 기준과 사용자 정리 UI를 결정하지 않았다.
- 채팅 삭제 요청이 들어올 때 message/run/recovery/evidence/workspace를 어떤 순서와 확인 단계로 처리할지 결정하지 않았다.
- 파일 삭제가 secure erase라는 약속을 할 수 있는지와 schema v2 이후 downgrade를 지원할지 결정하지 않았다.

---

### 6.7 실제 provider 연결 준비 — 2026-09-03 조사, 제품 활성화 아님

G5D 완료 뒤 실제 adapter를 연결한다. 기존 `core/provider.rs`의 exact fake manifest/handshake를 실제 vendor wire로 그대로 취급하지 않는다. 로컬 `--version` 확인값은 Codex CLI `0.149.1`, Claude Code `2.1.258`로 현재 fixture의 `0.150.0`/`2.1.241`과 다르다. 버전 범위를 임의 확대하거나 fixture 모델명을 실제 사용 가능 모델로 표시하지 않는다.

- **Codex 후보**: 공식 [non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode)의 `exec --json`은 JSONL 실행 이벤트를 제공한다. [app-server](https://learn.chatgpt.com/docs/app-server)는 초기화·모델 목록·thread/turn·interrupt를 제공한다. 로컬 0.149.1에서 모델 실행 없이 생성한 schema의 `ThreadStartResponse`에는 model/cwd/approvalPolicy/sandbox와 optional reasoningEffort가 있다. 따라서 app-server를 실행 전 유효 설정 대조 후보로 검토하되, 실제 응답·설정 격리·권한 적용/종료를 검증하기 전 startable로 올리지 않는다. 요청 argv나 schema의 존재만으로 실행 권위를 만들지 않는다.
- **Claude 후보**: 공식 [headless](https://code.claude.com/docs/en/headless)는 `-p`/stream-json과 system/init·assistant/result 흐름을 설명한다. [SDKSystemMessage](https://code.claude.com/docs/en/agent-sdk/typescript#sdksystemmessage)의 MCP/plugin 항목은 객체이며 기존 fake의 문자열 배열과 다르다. 기존 fake 전용 `configIsolation: managedOnly`를 실제 wire가 제공한다고 가정하지 않는다. 로컬 help의 `--safe-mode`·`--restricted`·설정/MCP 제한 옵션을 버전별 isolation 후보로 검증해야 한다. `--bare`는 구독 OAuth를 읽지 않으므로 기존 로그인 재사용의 대체로 몰래 선택하지 않는다.
- **공통 연결 순서**: 버전 고정 offline wire 회귀→provider별 실제 설정·모델/권한의 실행 전 검증→공개 메시지만 정규화→같은 snapshot/run 원장·취소/종료/복구 연결→사용자 승인 macOS smoke다. private reasoning/tool raw output·토큰·auth 원문은 채팅이나 durable public evidence에 넣지 않는다. vendor 결과 수신과 자손 소멸 관측을 구분하고 기존 poisoned/복구 계약을 유지한다.

이번 확인은 공식 문서, 로컬 version/help, Codex schema 생성까지만 수행했다. 모델 요청·로그인/인증 조회·사용자 프로젝트 실행은 없었다. local schema는 `/tmp/hello-agent-provider-schema.3kFDdj`에 있으며 이 경로는 제품 의존성이 아니다. CLI 선택·인증 격리의 최종 구현 방식과 실제 smoke 승인은 남아 있다.

**2026-09-03 승인 후 로그인 상태 관측**: 사용자는 모델 호출 없는 상태 조회를 승인했다. `/private/tmp`에서 exact CLI의 `codex login status`는 exit 0/ChatGPT credentials present였고, `claude auth status --json`은 격리 환경 exit 1/false와 자격 증명 접근 허용 환경 exit 0/true·claude.ai로 달랐다. 격리 환경의 false를 전역 미로그인으로 단정하지 않는다. 결과는 boolean/분류/exit만 남겼고 계정 식별자·토큰·원문 출력은 저장하지 않았다. 이는 로그인 정보 관측이며 토큰의 실제 요청 유효성·모델 가용성·설정 격리·제품 연결 성공 증거가 아니다. [Codex 공식 CLI reference](https://developers.openai.com/codex/cli/reference/#codex-login)도 status exit 0을 credentials present로 설명한다. 새 로그인·토큰 발급·모델 요청·사용자 프로젝트 실행은 하지 않았다.

**연결 전 남은 증거와 구현 경계**:

- Codex의 `initialize`/`initialized`→`model/list`→`thread/start` 응답을 검사한 뒤에만 `turn/start`로 목표를 전달하는 내부 상태를 둔다. 로컬 0.149.1 schema의 요청 sandbox 값은 `read-only`/`workspace-write`이며, reasoning은 top-level 필드가 아니라 고정 `config.model_reasoning_effort` 후보와 응답 `reasoningEffort`를 대조해야 한다. 응답의 정확한 모델·cwd 신원·승인 정책·sandbox 세부 권한 및 thread/turn 소속을 생략하지 않는다. 이는 schema 기반 후보 계약이며 실제 응답 성공은 아직 관측하지 않았다.
- [공식 app-server](https://learn.chatgpt.com/docs/app-server)의 `config/read`와 `configRequirements/read`는 유효 설정/관리 요구를 조회하지만, 시작 중 임의 설정 명령이 이미 실행되지 않았다는 증명은 아니다. 사용자 설정과 기존 구독 인증이 함께 있는 home을 임의 교체하거나 인증 파일을 복사하지 않는다. 설정 격리와 인증 재사용을 함께 보장하는 exact-version 시작 경로는 추가 관측이 필요하다.
- [Claude CLI reference](https://code.claude.com/docs/en/cli-reference)의 `--safe-mode`는 인증을 유지하지만 관리 정책의 hook·statusline·file-suggestion 명령은 남길 수 있다. `--restricted`는 user/project 설정과 기본 파일 도구의 접근을 제한하나 실제 init만으로 모든 설정 격리·권한 강제를 증명할 수는 없다. `-p <prompt>` 직후 init 검사만으로 첫 모델 요청 전에 입력을 보류했다고 주장하지 않는다. streaming-input 초기화와 실제 prompt-release 경계를 확인해야 한다.
- 현재 `LaunchSpec`의 config argv·빈 token·환경 allowlist 거부를 alias나 환경 상속으로 우회하지 않는다. 후속 exact vendor 전용 builder와 bounded wire parser는 별도로 구현하고 fake 계약을 보존한다. 누락된 config 증거를 합성하거나 요구를 삭제해서 `canStart`를 올리지 않는다. offline parser 구현과 제품 연결 완료는 구분한다.
- 모델 없는 startup/config 및 최소 인증 상태 관측도 지정 CLI·임시 snapshot·수집 항목에 대한 별도 사용자 승인 뒤 수행한다. 원문 설정·계정 식별자·토큰은 기록하지 않는다. 유료 가능 모델 smoke·쓰기/취소·자손 종료 검증은 그 다음 별도 승인 사항이다.

---

## 7. 안전 경계

M5는 M4의 실행 계약을 제품 셸 아래에 숨길 뿐, 약화하지 않는다.

| 경계 | M5 계약 |
| --- | --- |
| 대상 | 사용자가 프로젝트를 명시적으로 고른다. 첫 항목 자동 선택 금지 |
| 작업 공간 | 앱이 만들고 검증한 격리 공간만 실행 대상으로 사용 |
| 원본 변경 | 실행과 분리된 `반영하기` 단계에서만, 최종 diff 확인 후 |
| 종료 | 신호 성공이 아니라 자손 소멸 관측으로 판정. 미확인은 `recoveryRequired` |
| 잠금 | `poisoned`와 표시 저장 실패를 구분하고 둘 다 위험 행동을 막음 |
| 취소 | 준비·실행·검증 모든 단계에서 닿고, 취소 후 종료 확인까지 진행 중으로 봄 |
| 오류 | 기본 화면은 원인·할 일, 자세히 보기는 원문·run ID·증거 경로 |
| 적용 | 시작 기준과 원본의 현재 상태를 재검증. 충돌·불명확성은 사용자에게 돌려보냄 |
| 복구 | 되돌릴 범위와 사라질 수 있는 변경을 보여 주고 명시적 확인 후 실행 |

Windows에서 M4와 동등한 자손 회수·소멸 확인이 없으면 해당 실행 방식을 노출하지 않는다. M0~M3의 Windows 설치·홈 기능 지원과는 별개다.

---

## 8. 기술 방향과 아키텍처 결정

### 8.1 Electron 셸과 Rust 안전 코어

현재 구현은 **Tauri 2 셸 + React/TypeScript 프런트엔드 + Rust 시스템 코어**지만 M5의 새 agent-first IDE 셸은 **Electron + React/TypeScript + Monaco**로 만든다. 기존 Tauri host를 계속 확장하거나 Tauri/Electron 두 host를 동시에 유지하지 않는다. M0~M4에서 검증한 설치·로그인·격리 작업 공간·프로세스 종료·`poisoned`·증거 계약은 Rust sidecar로 분리해 재사용한다.

| 레이어 | 책임 | 금지 |
| --- | --- | --- |
| Electron main | 앱 수명주기, 창, 메뉴, 업데이트, Rust sidecar 시작·감시 | 프로젝트 안전 판단과 에이전트 argv를 임의로 재구현하지 않음 |
| preload bridge | renderer에 필요한 최소 typed API와 이벤트 채널만 노출 | 범용 IPC·파일시스템·셸·Node API 노출 금지 |
| sandboxed renderer | React/TypeScript 작업대, Monaco, 채팅·diff·설정 UI | Node integration과 직접 프로세스/파일 접근 금지 |
| Rust sidecar | 프로젝트/standalone snapshot 수명주기, SQLite 원장, 에이전트 실행·회수, Git 적용·복구, 안전 판정 | renderer 입력을 검증 없이 신뢰하지 않음 |

Electron renderer는 sandbox와 context isolation을 유지하고 Node integration을 끈다. main/preload는 허용된 명령과 구조화 이벤트만 Rust API에 중계한다. Electron 공식 프로세스 모델의 main/renderer/preload 경계를 따르되, 보안에 중요한 상태·권한·종료 판정은 Rust sidecar가 소유한다.

### 8.2 standalone snapshot 안전 계약

G4 작업 복사본은 linked worktree나 source repository의 alternates·remote·ref에 의존하지 않는다. sidecar는 source를 읽기 전용 capability로 잡고 `HEAD`를 self-contained하게 포함한 Git bundle을 크기 제한 안에서 스트리밍한 뒤, 앱이 관리하는 임시 대상에 raw committed blob bytes와 Git mode를 직접 materialize한다. checkout, filter, EOL·encoding·ident·export attribute·LFS 변환을 실행하지 않는다. symlink는 따라가지 않고 submodule은 초기화하지 않으며, case collision·non-UTF-8 path·`.git` entry·비정상 mode는 fail-closed다.

활성화 전에는 다음을 모두 만족해야 한다.

1. object format(`sha1` 또는 `sha256`)과 길이가 맞는 `baseCommit`·`baseTree`를 bundle에서 다시 검증한다.
2. detached `HEAD == baseCommit`, index tree `== baseTree`, object connectivity를 확인한다.
3. 작업 복사본 내부의 remote·alternates·source ref·source absolute path는 0개다.
4. proposed active manifest, source trace, workspace invariant, activation trace, DB, 외부 manifest, `.git` marker의 `projectId`·호환 `worktreeId`·`workspaceIdentity`·canonical Git root·`sourceIdentity`·object format·base commit/tree가 정확히 같다.
5. source의 `HEAD/index/refs/config/objects/packed-refs/entry set` 전후 SHA-256 digest가 같고 source lock·write가 0개다.
6. recovery marker를 export 전에 저장하고, marker·manifest·rename·identity·DB commit 뒤 activation 직전의 단일 composite evidence가 위 신원 전체를 교차 검증한 경우에만 한 작업 복사본을 active로 전환한다. 서로 따로 유효한 manifest와 다른 workspace 증거를 섞으면 fail-closed한다. corrupt/truncated/oversize bundle과 shallow missing object는 active 0개로 끝난다.

상한은 bundle 2 GiB, materialized total 4 GiB, 200,000 entries, single blob 1 GiB이며 materialization 뒤에도 disk reserve 512 MiB가 남아야 한다. 전체 bundle/blob을 한 번에 메모리에 올리지 않는다. 이 경계는 선택 root와 관리 target의 path/symlink swap 및 일반적인 동시 변경을 방어한다. 이미 잡은 source directory 내부 entry를 같은 사용자 권한의 악성 프로세스가 export 도중 계속 바꾸는 공격은 경계 밖이다. 그래도 export 결과의 commit/tree/object format과 source 전후 digest는 반드시 재검증한다. G4 Windows 안전성은 별도 실기기 게이트 전까지 지원으로 표시하지 않는다.

`project.createWorktree`, `createProjectWorktree`, `WorktreeManifestV1`, `worktreeId/path`는 protocol v1 호환 이름이라 유지하지만 실제 linked worktree를 뜻하지 않는다.

### 8.3 Electron 기준선 스파이크 합격 기준

1. macOS에서 한국어 IME, 복사/붙여넣기, 단축키, 접근성이 정상 동작하고 Windows UI에서도 같은 시나리오를 재현할 fixture가 있다.
2. sandbox·context isolation·Node integration 비활성화 상태에서 Monaco와 필요한 preload API가 동작한다.
3. 작업 복사본 파일 탐색·감시와 외부/에이전트 변경이 편집기와 diff에 안정적으로 반영된다.
4. 일반 편집과 side-by-side/inline diff 사이를 오가며 저장·undo가 섞이지 않는다.
5. 큰 파일과 큰 diff에서 renderer와 main이 멈추지 않는다. 구체적 fixture 크기와 허용 지연은 M5.0 구현 명세에서 수치로 정한다.
6. 여러 영속 채팅의 스트리밍 이벤트가 `Rust sidecar → Electron main/preload → renderer` 경계에서 섞이거나 유실되지 않는다.
7. 기존 M4 프로세스 종료·`poisoned`·증거 보존 테스트를 Rust sidecar에서 재사용하고, sidecar 충돌·Electron 강제 종료 뒤 `recoveryRequired`를 복원한다.
8. 앱 번들 크기·메모리·sidecar 서명·업데이트 경로의 기준값을 측정해 이후 회귀를 비교할 수 있다.
9. macOS 에이전트 실행의 출력·입력·중단과 M4 자손 회수 계약이 Electron 수명주기에서도 유지된다. Windows 에이전트 실행은 별도 종료 게이트 전까지 비활성화한다.

fixture 크기와 허용 지연은 M5.0 첫 구현 명세에서 수치로 고정한다. 이 스파이크는 Electron 채택 여부를 다시 비교하는 단계가 아니라, 선택한 구조의 IPC·수명주기·성능·안전 계약을 구현 전에 고정하는 단계다.

### 8.4 레이어 경계

- Rust sidecar: standalone snapshot 수명주기, 파일시스템 안전 경계, SQLite 원장/projection, 프로세스 실행·회수, Git 적용·복구.
- Electron main: 창과 앱 수명주기, 최소 권한 preload 등록, sidecar 시작·health/종료 조정. 위험 행동의 성공을 스스로 판정하지 않는다.
- preload: 명시적으로 버전된 typed bridge. 임의 채널 이름과 범용 `invoke`를 renderer에 노출하지 않는다.
- TypeScript renderer: 여러 채팅, 역할/모델 설정, 편집기·diff, 상태 projection과 사용자 상호작용.
- 에이전트 adapter: Rust 쪽에서 설치 버전이 실제로 지원하는 모델·추론 수준 조회, argv/세션/이벤트 정규화.
- Monaco: 코드 열기·수정·저장·검색·구문 강조·diff. LSP와 디버거는 별도 adapter로 나중에 붙인다.

### 8.5 나중까지 미룰 경계

- 여러 writer의 동시 실행과 자원 스케줄러
- 다중 터미널·무한 분할
- LSP 전체 언어 지원, 디버거, 확장 마켓
- 임의 에이전트 플러그인 런타임
- 원격/모바일 제어

---

## 9. 단계별 마일스톤

### M5.0 — 정보 구조와 상태 계약 확정

- Electron main/preload/sandboxed renderer와 Rust sidecar 골격 구현
- Monaco·채팅·standalone snapshot·diff·macOS 실행을 잇는 얇은 수직 스파이크 구현
- 스파이크 fixture 크기·허용 지연·메모리/번들 기준값을 구현 전에 수치로 기록
- 승인된 작업대·처음 시작하기 시안을 반응형 화면 계약으로 옮김
- Project/Chat/작업 복사본/Task/Run/Review 상태와 불변식 확정
- SQLite 이벤트 원장·현재 projection·앱 종료 안전 중단 계약 확정
- 저장 중단·앱 재시작·원본 변경 경합 시나리오를 테스트 명세로 작성

**완료 기준**: Electron + Rust sidecar 스파이크가 §8.2를 통과하고, 승인된 화면·핵심 루프와 모든 위험 행동의 사전 조건·실패 상태가 구현 가능한 계약과 테스트로 정의됨. 이름 확정과 Windows 에이전트 실행은 완료 조건이 아니다.

### M5.1 — 작업 복사본과 여러 영속 채팅

- 프로젝트 선택, standalone snapshot 생성·복구·폐기
- 한 프로젝트의 여러 채팅 생성·전환·재열기와 채팅별 1:1 작업 복사본
- 역할별 에이전트·모델·추론 수준 설정과 실행 전 검증
- 앱 기본 역할과 프로젝트별 재정의, 채팅 생성 시 구성 snapshot
- M4 실행을 영속 `Chat`/`Task`/`Run` 기록에 연결
- SQLite 이벤트 원장과 projection으로 앱 재시작 뒤 채팅·진행·증거 복원
- 앱 종료 시 안전 중단과 확인 불가 상태의 `recoveryRequired` 복원
- `waitingForUser`, `needsAttention`, `recoveryRequired` 표시

**완료 기준**: 여러 채팅을 오가고 앱을 닫았다 열어도 각 대화·실행 구성·안전 상태·다음 행동이 복원되며, 한 작업 복사본에서 writer 실행이 겹치지 않고 모델 fallback이 조용히 일어나지 않음.

### M5.2 — 코드 편집기와 작업 복사본 diff

- 파일 탐색, 열기·수정·저장·검색·기본 구문 강조
- 에이전트/사용자 변경의 파일 상태와 줄 diff
- 일반 편집 탭과 diff 탭 연결, 외부 변경 감지
- 저장되지 않은 편집 보호

**완료 기준**: 사용자가 작업 복사본 코드를 직접 편집하고, 에이전트 변경과 자신의 편집을 파일/줄 diff로 확인하며, 저장되지 않은 편집이 어떤 전이에서도 조용히 사라지지 않음.

### M5.3 — 검토·적용·폐기·복구 루프

- 파일 상태와 줄 diff의 점진적 검토
- 수정 요청을 같은 작업에 후속 실행으로 연결
- 시작 기준 재검증 뒤 전체 작업 복사본 명시적 적용
- 적용 직전 앱 전용 복원 지점, 폐기와 적용 후 되돌리기
- 파일별·줄 단위 반영은 후속 범위로 유지

**완료 기준**: 초보 사용자가 터미널 없이 변경을 이해하고 반영 또는 폐기하며, 적용 뒤 한 번의 명시적 복구를 성공시킴.

### M5.4 — 진행 중 협업 품질

- 질문/답변과 안전한 경계의 후속 지시
- 알림·읽지 않음·주의 필요 상태
- 긴 작업의 요약과 검증 증거 정리

**완료 기준**: 사용자가 앱을 계속 보고 있지 않아도 복귀 시 "무슨 일이 있었고 무엇을 해야 하는지"를 1분 안에 파악함.

### M5.5 — 검증된 고급 기능만 확장

- 단일 통합 터미널
- LSP·디버거 같은 편집 고급 기능
- Git 고급 동작
- 병렬 작업/다중 에이전트는 별도 품질·비용·안전 게이트 뒤 검토

**완료 기준**: 각 고급 기능이 기본 화면을 복잡하게 하지 않고, 끌 수 있으며, M5.1~M5.2 안전 불변식을 깨지 않음.

---

## 10. 성공 기준

### 제품 성공

- 첫 설정을 마친 사용자가 별도 편집기·터미널 없이 새 작업을 시작한다.
- 한 프로젝트에서 여러 채팅을 만들고, 각 채팅의 역할·에이전트·모델을 확인하고 다시 연다.
- 작업 복사본 파일을 직접 편집하고 에이전트 변경과 함께 diff로 검토한다.
- 앱을 닫았다 열어도 미완료 작업과 다음 행동을 찾는다.
- 에이전트 사이 결과를 사용자가 복사·붙여넣지 않는다.
- 변경 파일과 삭제 여부, 검증 결과를 앱 안에서 확인한다.
- 적용·폐기·복구를 전문 Git 용어 없이 수행한다.

### 안전 성공

- 명시적으로 고르지 않은 프로젝트에서 실행 0건.
- 종료 미확인 또는 원장 저장 실패 뒤 새 실행·적용 0건.
- 시작 기준이 달라진 원본에 자동 적용 0건.
- `poisoned` 상태가 앱 재시작으로 사라지는 경우 0건.
- 쉬운 오류 화면에서 사용자가 취할 수 있는 다음 행동이 없는 경우 0건.

### 검증 시나리오

1. 정상 작업 → 검토 → 적용 → 앱 재시작 → 기록 확인.
2. 정상 작업 → 폐기 → 같은 격리 공간 재사용 거부.
3. 실행 중 앱 종료 요청 → 안전 중단·자손 소멸·상태 저장 확인 → 재시작 뒤 기록 확인. 강제 종료로 확인이 끊기면 `recoveryRequired` 복원.
4. 검토 중 원본 변경 → 적용 차단 → 쉬운 설명.
5. 삭제·이름 변경·바이너리 변경이 파일 목록에 정확히 표시됨.
6. 원장 저장 실패 → 실행 또는 적용 전 fail-closed.
7. 적용 후 복구 → 복구 범위 확인 → 시작 상태 회복.
8. 기획과 코딩 역할에 서로 다른 에이전트·모델 지정 → 정확한 구성으로 실행되고 기록됨.
9. 선택한 모델 사용 불가 → 실행 전 차단 → 사용자 확인 없는 fallback 없음.
10. 여러 채팅 전환·재시작 → 대화·작업 복사본·편집기 상태·실행 이벤트가 섞이지 않음.
11. 에이전트 실행 중 직접 편집 → diff 즉시 갱신 → 저장되지 않은 편집 보호.

---

## 11. 비목표

- VS Code/Cursor의 편집 기능을 그대로 복제하기
- 여러 채팅을 지원한다는 이유만으로 첫 버전부터 여러 writer를 병렬 실행하기
- 터미널과 Git 지식을 사용자에게 가르쳐야만 쓸 수 있는 제품
- 의미 판단까지 앱 상태 머신이 대신하기
- 검증되지 않은 자동 병합·자동 충돌 해결·자동 push
- M4 §7.6B를 수행한 것처럼 가정해 교차 검토 품질을 제품 약속으로 만들기
- Windows 자손 회수 계약 없이 macOS 파일럿 실행 방식을 그대로 노출하기
- 기존 위저드·닥터·HandoffPilot을 한 번에 새 UI로 다시 쓰기

---

## 12. 남은 질문

아래 항목은 확정된 기준선 안에서 후속 구현·측정 결과로 결정하거나 다음 마일스톤에서 다룬다. 확정된 사용자 제품 방향을 다시 묻기 위한 게이트가 아니다.

### 후속 구현에서 수치화

1. 이미 고정한 파일·diff·snapshot 상한 안에서 실제 agent와 package의 지연·메모리·번들 회귀 기준을 얼마로 고정할 것인가?
2. 후속 지시를 즉시 주입할 수 없는 adapter에서 `다음 안전한 단계에 반영`과 `중단 후 다시 시작`을 어떤 이벤트 계약으로 구분할 것인가?
3. diff가 너무 크거나 생성 파일이 수천 개인 작업을 어떤 기준으로 `답변 필요`에 멈추고 나누도록 제안할 것인가?
4. 역할 사이의 자동 왕복을 작업당 몇 회까지 허용하고 비용·시간·반복 답변을 어떤 기준으로 멈출 것인가?

### 후속 마일스톤에서 결정

5. 새 제품 이름과 저장소·번들 ID·데이터 경로·웹사이트의 이전 범위는 무엇인가?
6. G5D §6.6의 증거·snapshot 보존 기간, 전체 용량 상한, abandoned snapshot 자동/수동 정리, 채팅 삭제 graph와 사용자 정리 UI를 어떻게 결정할 것인가?
7. Monaco 필수 범위 뒤 LSP를 붙일 때 언어 서버 수명과 작업 복사본별 진단을 어디서 관리할 것인가?
8. 터미널은 문제 해결 전용인가, 일상 실행 action의 기반인가?
9. 파일별·줄 단위 반영을 언제 열 것인가?
10. 병렬 writer를 열기 전에 필요한 §7.6B 또는 별도 비용·충돌·자원 게이트는 무엇인가?
11. Windows 실행 지원을 여는 자손 회수·소멸 확인 완료 기준은 무엇인가?

---

## 13. 브랜치와 의사결정 정책

- 통합 기준선: `m5/coding-agent-ide`
- Claude·Codex의 기능/실험 브랜치는 이 브랜치에서 분기하고 이 브랜치로만 병합한다.
- M5 작업 중 `main`에 직접 커밋·병합·push하지 않는다.
- `main` 병합은 M5 전체 완료·검증 뒤 사용자의 명시적 승인 게이트다.
- 이 문서의 **확정된 사용자 결정**과 M5 안전 기준선은 구현 계약이다. **현재 제안**과 §12 후속 질문은 해당 마일스톤에서 검토하며, 변경 시 사용자 결정과 안전 불변식을 명시적으로 갱신한다.

## 14. 다음 실행 순서

1. G5D D1에서 additive storage V10 exact migration과 `deletePrefix` atomic batch 계약을 Rust sidecar admission·scheduler·lossless compaction writer에 구현하고 end-to-end로 검증한다. 물리 cleanup은 계속 열지 않는다.
2. 실제 Claude Code/Codex adapter의 argv·모델 가용성·공개 이벤트 정규화·세션/취소 계약을 fixture와 분리해 고정한다.
3. 실제 유료 실행과 사용자 인증은 별도 승인을 받은 smoke에서만 검증한다.
4. 보안·패키징·성능 하네스와 macOS 전체 OS Quit·서명/권한 문자열 hardening을 구현한다.
5. Windows standalone snapshot과 프로세스 자손 회수·소멸 확인 게이트를 통과하기 전에는 Windows agent 실행을 노출하지 않는다.
6. 실제 adapter가 기존 안전 불변식을 유지한 뒤 적용·폐기·복원 루프를 다음 제품 단위로 연다.
