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

**2026-11-10 핵심 제품 정의 재확인:** 사용자가 “TODO 앱을 만들어줘”처럼 목표를 입력하면 PM 역할의 AI 오케스트레이터(Claude Code 또는 Codex)가 기획·디자인·개발·검토 등 담당 에이전트(역할별 Claude Code 또는 Codex)에 일을 지시하고 결과를 모아 최종 결과물을 사용자에게 제시한다. 초보자용 GUI는 이 과정의 사용자 접점이다. 사용자가 역할별 실행 버튼을 매번 누르는 흐름은 최종 제품 개념이 아니며 현재 중간 구현이다. 현재 3역할 설정은 구현 기준선이지 PM/디자인 역할을 제외하는 제품 범위 확정이 아니다. 세부 단계·재지시·완료 판정 계약은 후속 명세로 구체화하되 이 제품 방향 자체를 미확정 제안으로 취급하지 않는다. 기존 명시 동의·원본 보호·종료/복구 경계는 유지한다.


**2026-11-10 기본 대화 상대 확정:** 모든 새 대화의 기본 담당은 **총괄 에이전트**다. 총괄은 사용자 요청을 이해하고 설명·질문에는 직접 응답하며, 역할 분담이 필요한 작업은 팀 에이전트에 지시하고 결과를 취합한다. 총괄은 팀의 기획 역할과 구분하며, 총괄에 사용할 Claude Code/Codex와 모델도 설정 대상으로 둔다. 사용자에게 매 메시지마다 담당 역할 선택을 요구하지 않는다. 팀의 기본 구성은 **기획·디자인·개발·검증**이며 역할을 추가·제거·재정렬하고 역할별 이름·지침/컨텍스트·런타임/모델을 설정할 수 있도록 한다. 네 역할은 모든 질문에 강제하는 순차 단계가 아니다. 역할 지침은 실행 권한이나 원본 반영 승인 경계를 바꾸지 않는다. 이는 확정된 제품 요구이며 현재 고정3역할 수동 실행 구현과 구분한다. 기존 프로젝트·채팅의 설정과 기록을 묵시적으로 바꾸지 않으며 호환 이관 및 총괄 실행 계약은 후속 구현에서 검증한다.

**2026-11-11 사용자 접점/완료 기준 재확정:** 사용자는 오케스트레이터와 하나의 채팅으로 소통한다. 전송 버튼은 `보내기`이며 담당자나 수신 대상을 고르게 하지 않는다. 오케스트레이터 모델이 직접 답변 가능한 요청인지 판단하고 직접 응답하거나, 필요한 팀원에게 작업을 맡겨 결과를 받은 후 검토·정리해 사용자에게 답한다. 키워드 분기나 고정된 전 역할 순차 실행으로 이 판단을 대신하지 않는다. 팀원과의 지시·질문·보고는 같은 대화에서 관찰할 수 있지만 최종 응답 책임은 오케스트레이터에 있다. 직접 응답만 연결된 상태는 전체 제품 개념의 완료가 아니다. 이 요구를 재확정한 당시에는 직접 응답까지만 연결돼 있었다. 이후 모델의 위임 결정→팀원 실행→결과 회수→총괄 재개/최종 응답의 직렬 실행을 구현하고 가짜 공급자 회귀를 통과했다. 실제 이종 공급자 협업 종단 검증은 아직 완료되지 않았으며 현행 증거는 docs/history.md를 따른다. 위임은 설정된 역할·권한과 격리된 작업 복사본 범위로 제한하고 취소·실패·재시작에서 중복 실행하지 않아야 한다.

**2026-11-10 추가 확정:** Electron 앱 자체의 업데이트 확인·다운로드·적용 기능은 필수 요구사항이다. 네이티브 메뉴·다운로드·안전 종료 후 설치 소스 연결은 구현됐으며 실제 배포는 비활성이다. Codex/Claude Code 업데이트와 별개다. 사용자 승인으로 공개 GitHub Releases에 배포 파일을 보관하고 update.electronjs.org를 통해 앱 안에서 확인·다운로드·적용하는 방식을 선택했다. 브라우저에서 설치 파일을 수동으로 받는 흐름이 아니다. 서명·지원 설치 형식·DB 이관과 실패 복구의 상세는 설계/검증 후 확정한다. 실행/저장 중 안전한 종료 없이 앱을 교체하지 않으며 이 요구사항이 배포·push 승인을 대신하지 않는다.

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
18. 기본 대화 상대는 `총괄`이며 기본 팀 역할은 `기획/디자인/개발/검증`이다(2026-11-10 사용자 결정으로 이전3역할 제품 기본값을 대체). 팀 역할은 추가·제거·순서 변경과 역할별 지침 편집을 허용하며 앱 기본값을 새 프로젝트의 시작값으로 사용한다. 프로젝트별로 재정의하고, 새 채팅은 생성 시 적용 구성을 고정해 기존 채팅이 설정 변경으로 조용히 바뀌지 않게 한다.
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

#### 5.1.1 에이전트 업데이트 호환성 (2026-09-30 사용자 요구 확정, 구현 전)

**확정된 제품 요구**: Codex·Claude Code가 정상 업데이트돼도 기존 설치·로그인·프로젝트 흐름을 계속 사용할 수 있어야 한다. 매 릴리스마다 실행 파일 SHA-256을 수동 등록해야만 연결되는 방식은 제품의 호환성 정책으로 사용하지 않는다. 이 요구를 구버전 고정이나 중복 전용 설치로 대체하지 않는다. 아래 앱 전용 고정 버전 설치 권고와 A/B 승인 요청은 이 결정으로 철회한다. 초보자를 위한 앱 내 설치 지원 요구 자체는 유지한다.

**후속 구현 방향(구체적 API/검증 방식은 조사 필요)**:

- 공식 지원 연동 인터페이스를 사용하고 다른 데스크톱 앱의 비공개 번들 구조·내부 helper 배치 의존을 줄인다. 공급자별 adapter가 업데이트 차이를 흡수한다.
- 실행 파일의 출처/신뢰 검증과 프로토콜/기능 호환 검증을 분리한다. 정상 업데이트의 파일 내용 변화만으로 미설치·로그인 실패로 분류하지 않는다. SHA-256의 다운로드 무결성·실행 중 파일 교체 탐지 용도까지 금지한다는 뜻은 아니다.
- 실제 필수 기능과 응답 계약을 확인하며 지원되는 추가 필드/이벤트와 필수 의미 변경을 구분한다. 알 수 없는 권한 의미를 추측해서 허용하거나 기존 인증/profile·원본 보호 경계를 약화하지 않는다.
- 업데이트 전후 로그인 복원·새 실행·출력·취소/종료·복구를 회귀로 검증한다. 바이너리 해시 목록 교체만으로 새 버전 호환성 검증을 대신하지 않는다.
- 공급자가 연동 계약을 실제로 깨는 변경까지 무조건 자동 호환된다고 보장하지 않는다. 해당 기능의 호환성 문제를 구체적으로 알리고 기존 인증·작업 기록을 보존한다. 평범한 업데이트마다 연결을 차단하는 구조와 구분한다.

**현행 구현과의 차이**: 현재 코드는 아직 exact-pin 목록과 앱 번들 탐색을 사용한다. 위 방향의 구현 완료를 주장하지 않으며, 신뢰/호환 검사 대체와 회귀·독립 검토를 준비하기 전에 기존 검사를 삭제해 활성화하지 않는다.

**이하 이전 공급 경로 조사와 철회된 선택지 기록**: 당시의 권장/미승인/A·B/후속 표기는 과거 기록이며 현재 승인 요청이 아니다.

**확인한 현재 코드**: `b5bd325`의 M5 사전 진단/로그인은 Codex의 ChatGPT/Codex 앱 번들 CLI+helper pair와 Claude의 일부 고정 버전/해시만 허용한다. 기존 Tauri 설치기는 Codex standalone 최신 tarball과 Claude vendor 설치 스크립트를 사용한다. 따라서 레거시 설치기를 UI에 연결하는 것만으로 M5 사용 가능 상태가 되지 않는다. `providerExecutableMissing`은 지원 후보가 없다는 뜻이지 컴퓨터 전체에 설치가 없다는 증명이 아니다.

**공식 문서 확인**: [Codex CLI](https://developers.openai.com/codex/cli)는 standalone installer/npm/Homebrew 설치를 안내한다. [App Server](https://developers.openai.com/codex/app-server)는 버전별 schema 생성과 앱 통합 인터페이스를 설명한다. 문서가 특정 다운로드의 해시·M5 격리/종료 계약 통과를 증명하지는 않는다. [Claude 설치 문서](https://code.claude.com/docs/en/setup)는 native launcher가 버전 디렉터리를 가리키며 자동 업데이트될 수 있다고 설명한다. 기존의 관측된 실행 파일을 최신으로 무조건 교체하거나 후보 파일 자체에서 신뢰 해시를 학습하지 않는다.

**현재 제안 A (권장, 미승인)**: 이 앱 전용 도구 저장소에 공식 출처의 검증된 정확한 버전/필수 helper를 설치하고, 전역 CLI·PATH·다른 앱 번들은 변경하지 않는다. 설치할 항목·버전·저장 위치·용량을 먼저 표시하며 사용자 선택으로 시작한다. 전송/검증 중 파일은 실행 후보에 포함하지 않고 검증 완료 후 게시한다. 실패/취소는 기존 사용 가능 버전을 유지하며 불확실 종료는 복구 대상으로 남긴다. 업데이트도 같은 검증을 거치며 실행 중인 버전을 교체하지 않는다. 인증은 기존 앱 전용 namespace를 유지하고 재설치 시 초기화하지 않는다.

**대안 B (미승인)**: 기존 설치만 사용하고 공식 설치를 안내한다. 중복 도구 저장은 줄지만 외부 업데이트에 영향을 받으며 지원하지 않는 버전에서는 호환성 확인 전 사용을 막아야 한다. 초보자가 앱 안에서 설치를 끝낸다는 기존 목표와 차이가 있으므로 이 대안은 명시적인 사용자 결정 없이 채택하지 않는다.

**합격 증거/후속**: 선택된 정책 아래 공급자·OS/arch별 공식 배포 artifact/서명 또는 승인 해시와 필수 helper 목록, 다운로드 예산/취소·중단 복구, 게시/재검증, 실제 진단·로그인 연결까지 테스트한다. 설치·인증·모델 실행은 별도 결과로 표시한다. A/B는 비동기 질문으로 요청했으며 응답 전 확정하지 않는다. 이번은 설치 실행/제품 활성화 승인이 아니며 실제 사용자 환경 변경·계정/유료 모델 검증은 별도 권한 경계를 따른다.

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

#### 후속 지시 연결 계약 초안 (2026-09-14, 아직 전송 기능 아님)

- 요청은 `projectId/chatId/taskId/worktreeId/workspaceIdentity`, `clientMessageId`, `expectedEventSequence`, `text`를 고정한다. `clientMessageId`는 `user_`+소문자 UUID 32자리이고 renderer가 제출 시 한 번 생성해 응답 불명확 재시도에 그대로 사용한다. text는 기존 목표와 같은 UTF-8 16KiB 상한이며 서버가 임의 수정하지 않는다.
- sidecar는 소속과 보존된 채팅/작업 복사본 상태를 검증한다. 아직 저장되지 않은 요청은 예상 durable sequence가 같을 때만 정확히 다음 sequence의 `kind:user`, roleId/runId:null 메시지로 저장한다. chat 상태와 설정 snapshot을 바꾸거나 모델을 자동 실행하지 않는다.
- 메시지 ID와 원문/소속/최초 sequence가 같은 재시도는 기존 저장 결과를 반환한다. 같은 ID의 다른 내용은 거부한다. 타임아웃만으로 UI가 새 ID를 발급하면 안 된다. 순서 충돌이 확정되면 최신 기록을 읽고 초안을 보존하며 사용자가 다시 제출하도록 한다.
- 입력 보존과 원장 커밋/재시작/압축 후 동일 ID 검증이 선행한다. 복구 필요·폐기/완료 상태의 제출은 차단하고, 실행 중 입력은 현재 실행의 고정 입력 목록에 끼워 넣지 않는다. 중간 지시를 즉시 주입하는 기능과 durable 후속 메시지 저장은 구분한다.
- 현재 양쪽 언어의 요청/결과 대조 validator·공용 fixture와 내부 영속 writer를 추가했다. writer는 트랜잭션 안의 전체 chat projection 비교, fresh/정규화 메시지 재조회로 중복을 막으며 모델을 실행하지 않는다. wire capability/bridge 버전·압축 후 재시도/실패 테스트·전송 UI를 연결하기 전 제품 기능으로 표시하지 않는다.

### 5.5 적용·폐기·복구

**2026-09-30 사용자 승인, 구현 전**: 반영 중에는 다른 편집기/프로그램의 원본 수정을 중지하도록 안내하고, 전후 충돌 검사·기존 내용 보존·불확실 시 중단으로 보호한다. 동일 UID 외부 프로세스의 동시 수정을 OS 차원에서 완전히 배제한다는 보장은 하지 않는다. 앱 전체 복원 데이터 기본 한도는5GiB, 자동 삭제는 없으며 한도 초과 전에 새 반영을 차단한다. 사용자가 복원 지점을 확인해 명시 삭제하거나 예산을 변경하도록 한다. 구체적 구현/검증은 [변경 반영 계약](m5-change-application.md)을 따르며 승인 자체가 적용/복원 구현 완료나 실제 사용자 프로젝트 시험 허가는 아니다.

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

#### 확정 계약의 구현 기준

이 절은 목표 계약을 기록하며 현행 구현 여부를 주장하지 않는다. 현재 구현과 검증 상태는 `docs/architecture.md`와 `docs/history.md`에서 확인한다.

- protocol v1과 payload `schemaVersion: 1`을 유지하면서 DB logical/physical bytes, WAL bytes, ledger tail event count/payload bytes, snapshot aggregate count/payload bytes와 볼륨 capacity/available 사실값만 돌려주는 exact `storage.status`를 추가한다. raw chat/provider text, 비공개 추론, prompt, token/auth, absolute path는 응답에 넣지 않는다.
- public bridge는 additive `M5BridgeV6.getStorageStatus`와 G5D `getSystemStatus` 응답을 사용한다. `storage.status`는 기존 D1 exact shape와 capability 순서를 유지하고 scheduler activation을 이유로 새 공개 wire를 추가하지 않는다.
- 내부 SQLite schema v2는 v1 원장과 projection을 변경·삭제하지 않는 additive table/index/metadata migration으로 `ledger_baselines`, immutable operation receipt, normalized chat message/prepared workspace, maintenance job을 수용한다. 이후 V3~V8의 bounded 검증·source index·compaction/frozen source 계약을 거쳐 private V9는 V8에 resume decision witness table과 versioned guard를 additive로 더한다. V8→V9 SQL은 `BEGIN IMMEDIATE` 안에서 persistent DDL보다 먼저 V8의 모든 non-autoindex table/index/trigger에 대해 canonical `sqlite_schema.sql` 정의를 exact 비교하고, active pending/running 부재와 empty delete authority까지 확인한 뒤 하나라도 다르면 persistent schema를 V8 그대로 둔 채 transaction 전체를 rollback해야 한다. prefix 생성은 plan 1,001개가 gap 없이 완성되고 `verifyPrefix` phase의 현재 증거가 갖춰진 때만 허용하되 미래 `equivalenceVerified`를 요구하지 않으며, release는 equivalence 뒤 다음 연속 delete ordinal 최대 100개만 허용한다. 기존 baseline·message·frozen-message digest는 exact `sha256:<64 lowercase hex>`를 유지하고 V9 plan witness만 접두사 없는 64자리 lowercase hex를 저장한다. SQL은 기존 값의 exact `sha256:` 접두사·알고리즘·hex를 확인해 접두사를 제외한 plan 값과 비교하고, operation receipt는 별도 `sha256`/v1/`lowercaseHex64` 열과 raw hex를 유지하며 runtime Rust가 canonical payload·baseline·message를 반드시 다시 hash해야 한다. prepared workspace는 `bound`일 때만 `boundChatId`가 있고 그 chat이 같은 project임을 composite FK로 강제한다. 제품 admission은 exact V10 migration과 V9 witness 기반 V10 resume 검증을 수행하고 V9/V10 materializer·V10 writer·cooperative scheduler는 위 확정 계약을 따라야 한다. V10의 모든 non-autoindex object는 `(type, name, tableName, canonical sqlite_schema.sql SHA-256)` golden으로 고정해 같은 이름의 약화 trigger도 identity mismatch로 판별한다. V3 private schema·10개 phase·flags, idle/succeeded 기록과 public D1 wire/capability는 유지한다.
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

**2026-09-28 승인 실측 결과**: 사용자 승인 뒤 pinned 0.158 native CLI를 빈 임시 profile에서 initialize/config-read-only로 시험했다. 호출별 명시 설정 5개는 모두 일치했고 설정 출처는 sessionFlags/user/빈 system, disabledReason은 생략됐다. stdin EOF 뒤 root exit 0이다. 로그인·account/model/thread/turn 요청은 없었다. 이는 아래 config-only 실측 범위에 한정되며, `inspect_full_preflight_candidate`의 전체 조회(account/model 포함)나 signed-in 관리 설정·thread startup 격리 성공은 아니다. 독립 검토에서 내부 후보의 새 차단 결함은 없었고 gate 대기 중 CLI/helper 변경·취소 회귀는 활성화 전 후속으로 남겼다.

**2026-09-28 설정 격리 재확인 — 다음 실측 범위**: 공식 [설정 우선순위](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence)는 CLI override 외에도 system·cloud-managed·project layer를 명시한다. [훅 설정](https://learn.chatgpt.com/docs/config-file/config-advanced#hooks)은 project trust가 user/global hook을 끄지 않는다고 설명한다. 따라서 앱 전용 HOME, 빈 환경, 모델/정책 override만으로 hooks/MCP/plugins 격리를 선언하지 않는다. 현재 config/read는 includeLayers=false이고 유효 모델/권한만 비교하므로 설정 출처 증명도 없다. 로컬 0.158 생성 schema의 ConfigLayerSource에는 packagedDefaults/mdm/system/enterpriseManaged/user/project/sessionFlags/legacy managed 종류가 있다. 모르는 종류를 앱 소유 설정으로 간주하면 안 된다.

- 다음 실측은 사용자 승인 후, 고정 CLI/helper·새 빈 임시 profile·앱 소유 가짜 프로젝트만 사용한다. 기존 앱/터미널 인증 파일은 읽거나 복사하지 않는다. initialize/initialized/config/read(includeLayers:true)까지만 수행하고 thread/start·turn/start·로그인·model/list는 보내지 않는다.
- 반환값은 layer 종류/활성 여부와 허용한 모델·정책의 일치 여부만 기록한다. 원문 config·계정·환경 비밀을 로그나 증거로 저장하지 않는다. 실제 CLI는 승인 없이 시작하지 않는다.
- 시작/종료·예상 밖 활성 layer를 확인하고, 관리 설정을 무시하는 우회가 아니라 지원 불가를 명시적으로 거절할 조건을 정한다. config/read는 이미 일어난 startup 명령의 무실행 증명이 아니므로 소스 검토 및 통제된 훅/외부 도구 실행 여부 시험도 별도로 필요하다.
- 이번 내부 launcher는 검토 전 미커밋이며 제품 IPC/actual-agent capability를 열지 않는다. 독립 검토는 launcher·profile·preflight·취소/종료 소유권을 함께 한 번에 다룬다. 실측 및 독립 검토 완료 전 사용자 목표를 전달하는 turn 경로를 활성화하지 않는다.

**2026-09-14 사용자 확정 — 앱 전용 별도 로그인**: 기존 터미널 설정·인증을 보존하고 앱 전용 환경에서 Codex·Claude에 별도 로그인하도록 구현하는 방향을 사용자가 승인했다. 기존 인증정보 복사나 유료 모델 실행은 승인에 포함하지 않는다. [공식 인증 저장 문서](https://learn.chatgpt.com/docs/auth#credential-storage)의 Codex `file` 방식은 전용 `CODEX_HOME` 아래 auth.json에 저장한다. 앱 전용 접근 제한 폴더만 사용하고 기존 설정 덮어쓰기·자동 keyring fallback·공유 인증 재사용을 하지 않는다. Claude의 macOS 인증 저장 분리는 별도 확인하며 Codex와 같다고 추정하지 않는다. 환경 준비·모델 없는 파서와 실제 격리 프로세스/로그인 UI 연결 완료는 구분한다.

**2026-09-14 Codex offline 대조 구현**: `vendor_observation/codex_preflight.rs`는 명시적 model/effort와 config/read 및 완전 model/list를 비교한다. thread/start에는 readOnly/networkAccess=false, on-request/user, exact cwd 및 빈 instructionSources만 허용한다. 이 초기 읽기 전용 후보의 성공은 쓰기 역할 지원·파일 신원·설정 격리·실행 권위가 아니다. 기존 실행/공개 capability는 차단 상태다. 로컬 0.149.1 generate-ts 출력의 SHA-256은 Config.ts `f3fce144710f8972c83c229345611038066b67bd1777a9f859c696545c4bc5df`, Model.ts `ff56f09e9b9f301f1c6ecc565e2bcf6355c6ea66880f3ac963b7754e74cbe0a3`, ThreadStartResponse.ts `efc3d5df82b8f6f7d320fc97e2c51e8f25dc38195ae49af55f37b2ead8a05464`다. 실제 CLI 응답 관측을 대신하지 않으며 합성 테스트 모델명을 실제 카탈로그로 표시하지 않는다.

**2026-09-14 독립 검토 후 구현 순서**: 일반 제품의 차단 모드는 유지한다. 현재 start의 fake executable/launcher 및 Claude-only handshake를 실제 CLI로 단순 치환하지 않는다. 첫 offline 버전/인증 파서는 `core/provider/vendor_observation.rs`에 분리했으며 실행 권위를 반환하지 않는다. 다음은 provider별 config/model/handshake 검증과 입력 보류 경계다. 이후에만 기존 snapshot 검증→run/쓰기 잠금 영속화→프로세스 신원/시작 증거→입력 전달→공개 event→종료/poison/recovery에 연결한다. 버전/auth 성공만으로 제품 capability를 광고하거나 `canStart`를 올리지 않는다.

**2026-09-28 다음 실행 통합 단위 — 제안, 아직 미구현**

기준 `f913af6`에서 내부 Codex text 후보는 채팅 snapshot을 확인하지만 실행 원장을 소유하지 않는다. 기존 `AgentRuntime::start_agent`는 preparing run·recovery ID·쓰기 잠금을 함께 저장하고, 프로세스 신원을 저장한 뒤 launcher를 release한다. 반면 후보가 사용하는 `LoginProcess`의 profile fence는 로그인/프로세스 보호이며 chat/run 복구 원장과 별개다. 두 소멸 증거를 서로 대신 사용하지 않는다.

다음 구현은 helper마다 별도 완료시키지 않고 다음을 포함하는 하나의 **실제 실행 소유권 연결 파동**으로 묶는다.

1. 기존 actual-agent namespace와 preparing/recovery 예약을 재사용한다. 목표는 지정 채팅의 저장된 메시지 ID에서, 모델·추론·권한은 해당 채팅의 role snapshot에서 해석한다. 임의 prompt/model을 원장과 따로 받는 제품 진입점은 만들지 않는다. fixture provider 증거를 실제 provider 증거로 재표시하지 않는다.
2. 단일 owner가 profile fence·run ID·작업 공간을 함께 소유한다. launch 전 run 예약, payload release 전 프로세스 신원 영속화, thread/turn 전 같은 run/역할/snapshot 재검사를 연결한다. 초기 설정 관측용 프로세스와 작업 실행 프로세스의 재사용 여부는 명시적으로 정하고, 이미 닫힌 관측 세션을 실행 권위로 사용하지 않는다.
3. 공개 출력은 기존 원장의 순서/용량/신원 검증으로 저장한다. 현재 `TextWorkspace`의 장기 `&Storage` 참조와 public event 저장의 `&mut Storage` 요구를 함께 정리한다. 빌림 문제를 별도 DB writer나 병렬 owner로 우회하지 않고, 상태 소유자가 검증·통신·저장을 순차 수행하도록 만든다.
4. 종료 확인 뒤에만 terminal 상태와 잠금 해제를 확정한다. run 기록 실패·프로세스 종료 불확실·앱 재시작 시 profile fence만 지우거나 run만 완료 처리하지 않는다. 기존 actual recovery 경로가 같은 실행을 찾고 보수적으로 복구할 수 있어야 한다.
5. 가짜 vendor pipe와 실제 임시 snapshot/SQLite를 함께 사용하는 통합 회귀로 원장→입력→출력→종료 및 각 저장 실패를 확인한다. 잘못된 역할/채팅, 중복 시작, 입력 전 취소, 출력 저장 실패, 종료 불확실, 재시작 복구를 포함한다. 순수 파서·mock callback PASS만으로 이 파동을 완료하지 않는다.

이 파동은 workspace-write OS 격리·도구 lifecycle/승인 UI·Claude adapter 및 사용자 승인 실제 모델 smoke를 생략하는 대안이 아니다. 해당 실행 정책과 사용자 화면까지 검증하기 전 제품 capability는 계속 닫는다. 이는 현재 제안이며 확정된 제품 범위·원본 보호 규칙을 변경하지 않는다.

**2026-09-14 추가 관측**: 사용자 잔여 작업 진행 요청 후 생성한 빈 임시 폴더에서 Codex 0.149.1의 `initialize`→`initialized`→`config/read`→`account/read(refreshToken:false)`를 확인했다. 초기화/config 응답 성공, chatgpt credentials present 분류와 stdin 종료 후 exit 0만 기록했다. Claude 2.1.269의 `auth status --json`은 exit 0/loggedIn true/claude.ai였다. thread/turn/model 요청은 0회이고 원문 설정·계정 식별자·토큰은 보존하지 않았다. 이전 startup/config 관측 대기는 이 범위에서 해소했지만 설정 격리·실제 모델 smoke·제품 활성화는 여전히 미완료다.

- Codex의 `initialize`/`initialized`→`model/list`→`thread/start` 응답을 검사한 뒤에만 `turn/start`로 목표를 전달하는 내부 상태를 둔다. 로컬 0.149.1 schema의 요청 sandbox 값은 `read-only`/`workspace-write`이며, reasoning은 top-level 필드가 아니라 고정 `config.model_reasoning_effort` 후보와 응답 `reasoningEffort`를 대조해야 한다. 응답의 정확한 모델·cwd 신원·승인 정책·sandbox 세부 권한 및 thread/turn 소속을 생략하지 않는다. 이는 schema 기반 후보 계약이며 실제 응답 성공은 아직 관측하지 않았다.
- [공식 app-server](https://learn.chatgpt.com/docs/app-server)의 `config/read`와 `configRequirements/read`는 유효 설정/관리 요구를 조회하지만, 시작 중 임의 설정 명령이 이미 실행되지 않았다는 증명은 아니다. 사용자 설정과 기존 구독 인증이 함께 있는 home을 임의 교체하거나 인증 파일을 복사하지 않는다. 설정 격리와 인증 재사용을 함께 보장하는 exact-version 시작 경로는 추가 관측이 필요하다.
- [Claude CLI reference](https://code.claude.com/docs/en/cli-reference)의 `--safe-mode`는 인증을 유지하지만 관리 정책의 hook·statusline·file-suggestion 명령은 남길 수 있다. `--restricted`는 user/project 설정과 기본 파일 도구의 접근을 제한하나 실제 init만으로 모든 설정 격리·권한 강제를 증명할 수는 없다. `-p <prompt>` 직후 init 검사만으로 첫 모델 요청 전에 입력을 보류했다고 주장하지 않는다. streaming-input 초기화와 실제 prompt-release 경계를 확인해야 한다.
- 현재 `LaunchSpec`의 config argv·빈 token·환경 allowlist 거부를 alias나 환경 상속으로 우회하지 않는다. 후속 exact vendor 전용 builder와 bounded wire parser는 별도로 구현하고 fake 계약을 보존한다. 누락된 config 증거를 합성하거나 요구를 삭제해서 `canStart`를 올리지 않는다. offline parser 구현과 제품 연결 완료는 구분한다.
- 모델 없는 startup/config 및 최소 인증 상태 관측도 지정 CLI·임시 snapshot·수집 항목에 대한 별도 사용자 승인 뒤 수행한다. 원문 설정·계정 식별자·토큰은 기록하지 않는다. 유료 가능 모델 smoke·쓰기/취소·자손 종료 검증은 그 다음 별도 승인 사항이다.

---

### 6.8 외부 프로세스 격리 검증 조건 (2026-09-28, 제안)

실제 쓰기 역할을 열기 전, 공급자 내부 sandbox 설정 대조와 앱이 강제하는 원본 보호를 구분한다. [OpenAI 공식 보안 문서](https://learn.chatgpt.com/docs/agent-approvals-security)는 macOS 명령 실행에 Seatbelt를 사용하며 승인 정책과 sandbox를 별개로 설명한다. 이는 우리 앱의 app-server 전체 startup·상속 자원·원본 보호 증명은 아니다.

`tests/macos_sandbox_probe.rs`는 임시 파일만 사용하는 opt-in OS 기제 실험이다. 실행: `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 cargo test -p hello-agent-sidecar --test macos_sandbox_probe -- --ignored --nocapture`. 이미 sandbox 안인 환경에서는 `sandbox_apply: Operation not permitted`로 실패할 수 있으며 이를 성공이나 제품 fallback 조건으로 처리하지 않는다.

2026-09-28 로컬 관측은 다음과 같다. broad allow/default + 경로별 write 제한은 작업 폴더 쓰기를 허용하고 외부 직접/심볼릭 링크/부모 경로/자식 프로세스 쓰기·외부 삭제/이동·실행 중 외부 hardlink 생성을 거절했다. 그러나 **sandbox 진입 전 열린 writable descriptor와 기존 hardlink로 원본 파일을 변경할 수 있었다.** 출력 `standalone_source_protection_candidate=false`가 판정이며 테스트 프로세스 exit 0은 이 정책의 제품 합격이 아니다. 외부 네트워크·Mach IPC·실제 vendor 호환성은 검증하지 않았다. 현재 제품에 이 정책을 적용하지 않는다.

따라서 후속 구현/입장 검증에는 다음이 필요하다.

1. 원본 파일의 열린 핸들이 child에 전달되지 않음을 명시적으로 확인한다. stdio는 pipe이고 그 외 descriptor는 close-on-exec 또는 명시 닫기 대상이며, 원본 파일을 stdout/stderr로 전달하지 않는다.
2. snapshot과 쓰기 허용 디렉터리의 기존 inode 공유/hardlink를 입장 단계에서 거절한다. 현재 snapshot 생성 방식만 믿고 후속 경로 교체를 허용하지 않는다. 준비 시점 검증과 실행 직전 재검증, 검사 비용을 함께 설계한다.
3. 기제 실험의 `(allow default)`를 생산 정책으로 채택하지 않는다. 파일 정책 외에도 IPC를 통한 다른 프로세스의 대리 쓰기·network·자손 프로세스·권한 확대와 설정 실행을 다뤄야 한다. 기존 사용자/회사 관리 정책을 우회하지 않는다.
4. 제품 runtime은 위 검증·실제 공급자 격리 관측·독립 검토를 통과한 실행 경로만 허용한다. 거절 시 무격리 fallback하지 않는다. Windows는 별도 OS 증거가 필요하다.

이는 새 사용자 권한 승인이 아니라 기존 원본 보호 계약을 구현하기 위한 검증 조건이다. 읽기 전용 후보나 이 실험을 코딩 역할·승인 UI의 대체 완료선으로 사용하지 않는다.

**기본 거부 정책 실측(2026-09-28, 아직 테스트 전용)**: 같은 probe의 `default_deny_outer_policy_enforces_writes_and_network_boundary`는 `(deny default)`에서 지정된 시스템/작업 복사본/profile/scratch 읽기, 세 쓰기 폴더, 프로세스 생성/실행·자기 signal·sysctl-read만 명시 허용한다. `.git`·`.agents`·`.codex` 및 profile config 쓰기는 별도 거절한다. 시스템 셸의 쓰기/자식 실행·지정 파일 읽기는 성공하고, 임시 원본 읽기/쓰기·symlink 우회·보호 설정 변경/이동·외부 hardlink 생성·localhost TCP 연결은 거절됐다. 네트워크 대조군은 같은 임시 listener에 sandbox 밖에서 연결 성공을 확인한다. Mach IPC 및 실제 provider 호환성·인증 갱신·모델 네트워크 연결은 별도 미검증이다.

초기 읽기 제한은 시스템 로더의 SIGABRT를 일으켰다. 해당 진단 자식 PID의 kernel deny 로그가 `file-read-data /`를 가리켰고, `(literal "/")`의 디렉터리 자체 읽기를 추가해 해결했다. `(subpath "/")`나 file-read 전체 허용으로 우회하지 않았다. 로컬 시스템 경로 허용은 다른 macOS 버전의 호환성 보장이 아니다. 실제 실행에 사용하기 전 trusted CLI/helper 읽기 경로·profile/scratch의 inode 입장·명시적 모델 통신 경계를 조합하고, fd 차단 gate 및 프로세스 소유권과 통합 검증해야 한다. 이 정책을 제품 capability에 연결하거나 네트워크 허용 fallback으로 사용하지 않는다.

### 6.9 실행 격리 책임 재선택 (2026-10-01 사용자 승인, 제품 연결 전)

**2026-10-07 사용자 확정 — 업데이트와 프로필 생성물**: 정상 공급자 생성물은 고정 파일명 허용 목록 대신 소유권·권한·파일 구조로 검사한다. 앱 고정 설정·credential 파일의 기존 제약과 프로필 디렉터리 신원 검사는 유지한다. 알려진 설정/지시 파일을 임의 허용하거나 읽어 실행 권한을 얻는 정책이 아니다. Codex 임시 도구 링크는 선택한 공식 서명 실행 파일에 결속된 전용 검사에서만 허용하며 일반 프로젝트/작업 사본과 Claude의 링크 거절은 유지한다. 이 결정은 실제 연결 성공·모든 미래 CLI 호환성을 보장하지 않으며 구체 패치의 독립 검토·회귀가 필요하다.

**공식 계약 재확인(2026-10-01)**: [App Server thread 시작](https://learn.chatgpt.com/docs/app-server#start-or-resume-a-thread)은 named profile 사용에 experimentalApi=true와 permissions/sandbox 상호 배타를 요구한다. 실제 cwd로 permissionProfile/list를 조회해 관리 정책상 허용을 확인해야 한다. 별도 beta handshake 후보는 이 후속 검증을 대체하지 않으며 기존 stable thread 응답을 새 정책 보호 증거로 사용하지 않는다.

현재 두 공급자의 로그인 설치 입장은 공급자 서명 검증으로 전환했지만 실제 모델 실행은 여전히 비활성이다. 기존 outer 정책은 인증 PROFILE 읽기/쓰기를 모든 자손에게 상속한다. 승인된 임시 sentinel 시험에서 이 정책 아래 Codex 내부 도구 sandbox 적용은 exit71로 실패했다. 이는 해당 조합의 실패이며 모든 macOS 중첩 방식의 불가능 증거가 아니다.

공식 [App Server 문서](https://learn.chatgpt.com/docs/app-server)의 command execution은 이미 서버를 격리한 경우 `externalSandbox`로 Codex 자체 격리를 생략할 수 있다고 설명한다. 그러나 현재 outer PROFILE 권한을 그대로 둔 채 이 값만 바꾸면 도구와 인증 파일의 분리가 해결되지 않으므로 채택하지 않는다. 같은 문서의 외부 `chatgptAuthTokens`는 호스트가 인증 갱신 수명을 소유하는 experimental 방식이다. 기존 OAuth 파일을 임의로 읽어 이 경로로 바꾸거나 원격 Code Mode host를 검증 없이 제품 대안으로 삼지 않는다.

**승인된 설계 방향(제품 활성화 아님)**: 서명 검증된 공급자 부모 프로세스는 신뢰하고, 모델이 요청하는 도구의 파일/명령 실행은 공급자 공식 sandbox가 담당하도록 전환한다. 우리 앱은 standalone snapshot·명시적 프로젝트 선택·상속 descriptor 차단·설정/권한 대조·확장 권한 거절·종료 확인·poisoned 잠금·diff 후 별도 원본 반영을 계속 소유한다. 바뀌는 경계는 공급자 부모 자체에 우리 custom outer sandbox를 추가하지 않는다는 점이다. 따라서 부모의 시작 동작/모델 통신 전체를 우리 OS 정책으로 차단한다는 기존 후보의 보장은 포기하며 이를 단순 호환 수정으로 숨기지 않는다.

승인 후 첫 검증은 실제 계정·유료 모델·사용자 프로젝트 없이 새 임시 profile/snapshot과 가짜 sentinel로 수행한다. 공급자가 공식 sandbox를 적용해 원본/인증 가짜 파일 접근과 권한 확대를 거절하는지, 취소·실패 때 기존 소유권/종료 기록을 보존하는지 검사한다. 부모 자체에 OS network 차단이 없으므로 이전의 완전 오프라인 시험과 동일하다고 표현하지 않는다. 이 검증 승인에는 실제 인증 토큰 읽기/갱신·계정 조회·유료 작업 승인이 포함되지 않는다. 공급자 sandbox를 우회하는 도구/설정이 있거나 보호를 입증하지 못하면 제품을 활성화하지 않는다.

대안은 별도 사용자/VM 등의 독립 실행 환경으로 공급자 전체를 격리하는 것이다. 설치·인증 공유·자원/패키징 비용이 커져 별도 제품 결정과 구현이 필요하다. 자동 목표 진행만으로 두 신뢰 모델 중 하나를 확정하지 않는다.

#### Claude 정책 관측의 남은 연결 조건 (2026-10-06 조사)

**2026-10-07 사용자 확정 — 보장 범위와 단계 분리**: 공식 CLI의 보호 기능을 신뢰하되 공급자가 보고하지 않는 관리 설정·실행 중 설정 불변성까지 검증했다고 주장하지 않는 방향을 승인했다. 이 결정은 별도 VM 전환, sandbox 우회, 원본 직접 쓰기 또는 실제 계정/유료 모델 시험 승인이 아니다. standalone snapshot·명시적 대상·도구 승인·종료 확인·poisoned 잠금은 유지한다.

- 실행 전 기록은 서명 확인된 실행 파일의 버전/신원, 앱 프로필의 credential 존재, 선택 역할/모델/effort, 앱이 **요청한** 실행 설정의 결속이다. credential 존재는 모델 사용 권한이나 실제 실행 성공이 아니다. 정책 digest는 기록의 결속용이며 고정 실행 파일 해시 허용 목록으로 업데이트를 막는 수단이 아니다.
- 세션 기록은 같은 연결에서 실제 검증한 model/cwd/permission/tools/mcp/plugins만 포함한다. config·sandbox 실효 상태·관리 설정의 부재는 관측 목록에 넣지 않는다. 기존 G5C V1 전체 관측 계약은 유지하고 native Claude의 단계별 기록을 별도 버전 계약으로 다룬다.
- 순서는 control 초기화와 실행 전 검사 → 목표 전송 → system/init 검증 → 도구 허용이다. system/init 이전 도구 요청·불일치 init·증거 누락은 거절하고 종료/복구한다. 목표 전송 전 system/init이 온다고 가정하지 않는다. 부모의 모델 통신 전체를 앱의 OS 격리로 차단한다는 보장은 하지 않는다.
- 공식 도구 보호의 임시 sentinel 시험은 호환성/출시 검증이지 매 실행의 실효 정책 증명이 아니다. 알려진 보호 실패는 차단한다. 이 승인만으로 제품 gate를 PASS로 바꾸지 않고 실제 연결·누적 회귀·독립 검토를 마친다.

확인된 공식 계약과 현재 제안을 구분한다. [SDK `resolveSettings()`](https://code.claude.com/docs/en/agent-sdk/typescript#resolvesettings)는 CLI를 띄우지 않고 같은 merge engine으로 설정과 출처를 조회하지만 `policyHelper`를 실행하지 않고 서버 관리 설정을 가져오지도 않는다. `settingSources: []`도 endpoint 관리 정책을 제외하지 않는다. 따라서 이 함수의 snapshot만으로 live session의 격리/관리 정책 합격을 만들지 않는다. 새 SDK 의존성을 추가하는 것만으로 현재 admission 공백은 해소되지 않는다.

[관리 설정 문서](https://code.claude.com/docs/en/managed-settings)는 서버·MDM·파일·Windows HKCU, 파일의 `managed-settings.d/`와 실행 중 reload를 설명한다. 한 파일의 부재, 초기화 모델 목록, `hooks_applied`만으로 정책 부재나 불변성을 추론하지 않는다. 기존 `--safe-mode`/`--restricted`도 관리 정책 전체를 제거하는 옵션이 아니다.

다음 구현의 근거 수집 순서는 (1) 승인된 무계정 임시 profile에서 초기화 전후 관측 가능한 정보 확인, (2) 실제 도구 경계의 가짜 sentinel 시험, (3) 관측 가능한 조건을 세션/실행 대상에 결속하고 알 수 없는 조건은 명시적 불가 사유로 반환하는 것이다. 계정/정책 원문을 renderer나 공개 원장에 저장하지 않는다. 정책 우회나 VM 전환을 이 조사만으로 승인한 것으로 해석하지 않는다.

도구 목록도 실행 surface를 구분한다. [EndConversation](https://code.claude.com/docs/en/tools-reference#endconversation-tool-behavior)은 일반 CLI 설명상 자동 포함되지만 `-p`/Agent SDK에서는 제외된다. 현재 stream-json print 실행의 expected tools에 이를 임의 추가하거나 알려지지 않은 도구 전부를 허용하지 않는다.

**임시 자료 관측(2026-10-06, 제품 gate 아님)**: 로컬 가짜 API가 반환한 tool_use를 실제 Claude 2.1.287이 실행했을 때 Read 내부 성공/외부 거절과 Bash 내부 읽기·쓰기 성공/외부 거절을 확인했다. 후자는 Read deny 또는 blockReadsOutsideWorkingDirectories의 사전 승인 거절이므로 OS sandbox 자체 증거와 분리한다. [SDK readFile](https://code.claude.com/docs/en/agent-sdk/typescript#what-readfile-can-read)은 Read보다 좁은 범위라 대체 증거로 사용하지 않는다. 개별 도구 거절 뒤에도 최종 result success가 나왔으므로 result success만으로 앱 작업을 성공 처리하지 않는다. 실제 계정·유료 모델·사용자 프로젝트는 이 관측에 사용하지 않았다.

#### Claude 내장 조직 guard 호환 방향 (2026-10-19 사용자 승인)

사용자가 공식 내장 guard만 수용하는 호환 구현을 승인했다. 조직 정책 비활성화·유료 재시험은 포함하지 않는다. 아래 후보의 구현/검토를 진행하며 실제 등록 전체 shape와 실제 계정 성공은 별도 검증한다.

최신 패키지의 단일 Claude 시험에서 `init plugins organization guard`를 관측했다. 현재 `claude_init.rs`는 모든 nonempty plugins를 거절한다. 따라서 로그인 재시도나 CLI 재설치만으로 해결된다고 안내하지 않는다.

공식 [mods overview](https://code.claude.com/docs/en/plugins/mods/overview#mods-built-into-claude-code)는 `cc-plugin-sec-default`를 사용자 비활성화 불가인 내장 조직 보호 기능으로 구분한다. 일반 mod는 Bash sandbox 밖에서 동작할 수 있다. 공식 [sec-default 설명](https://github.com/anthropics/claude-code/blob/main/mods/sec-default/README.md)은 이 guard가 기존 조직 정책을 사용자 mod의 변경으로부터 보호한다고 설명한다. 이 자료는 공급자 설계 설명이며 현재 설치본과 동일한 코드라는 증명은 아니다.

**제안**: 이미 승인된 서명 검증 공급자 부모 신뢰 모델 안에서, native Claude에 한해 공식 내장 guard의 정확한 등록 형태를 별도 관측으로 수용한다. 빈 목록은 계속 허용하되 guard 이외 항목·혼합·중복·잘못된 형태는 거절한다. `builtin` 문자열 자체를 신뢰 근거로 삼지 않고, 기존 서명/실행 파일 결속 및 같은 소유 세션 경로를 필수로 유지한다. fixture/G5C 관측 계약 전체를 일괄 완화하지 않는다. 관측 기록은 ‘플러그인 없음’ 대신 guard 존재를 정확히 표시해야 한다.

변경하지 않는 경계는 MCP/도구/모델/cwd/permission 검사, 관리 정책 우회 금지, snapshot과 원본 반영 분리, 종료 확인/poisoned 잠금이다. 조직 guard를 끄거나 관리 정책의 부재/불변성을 보장하지 않는다. 설치본 해시 고정으로 업데이트를 막지 않는다.

이는 기존 확장 거절 계약의 예외이며 위 사용자 승인 범위에서만 구현한다. 실제 등록 shape와 저장 관측 경로를 확인하고 최소 변경·혼합/위조 형태 회귀·읽기 전용 독립 검토를 수행한다. 실제 유료 재시험은 별도 승인 대상이며 이 승인은 전체 gate 합격이 아니다.

## 7. 안전 경계

셸 OS 경계 후속 관측에서는 fixture의 사전 외부 읽기 제한을 제거하고 sandbox denyRead/denyWrite만 유지했을 때 내부 쓰기 성공/외부 읽기·쓰기 Operation not permitted를 확인했다(2026-10-06, Claude 2.1.287). 재현은 별도 승인 후 `node scripts/m5-claude-tool-probe.mjs --run /absolute/signed/claude`로 수행한다. 일반 테스트 자동 실행과 제품 활성화 근거로 자동 승격하지 않는다. 스크립트는 새 임시 자료를 보존하고 요약에 위치를 표시한다. 원본 probe의 성공과 저장소 스크립트/전체 제품 검증은 구분한다.

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

### Windows 내부 Git 제공 방식 — 사용자 설치 Git 사용 확정 (2026-11-06)

**설치 신뢰 정책 승인(2026-11-06)**: 앱은 선택한 설치 폴더와 Windows가 검증한 실행 파일 서명자를 표시하고 명시 동의를 받는다. 동의한 설치본의 서명 없는 runtime DLL도 포함하며 실행마다 설치본을 재검사한다. 폴더의 실제 식별 정보 또는 서명자가 바뀌면 재동의한다. 정상 업데이트를 특정 버전·파일 해시 allowlist로 제한하지 않는다. Windows 제품 활성화는 별도 검증/승인 경계를 유지한다.

앱 내부 snapshot/diff에 사용하는 Git은 Codex·Claude Code 설치/업데이트와 별도다. **2026-11-06 사용자 결정: 사용자가 설치한 Git을 사용한다.** 공식 MinGit 앱 포함 권고는 채택하지 않았다. 앱은 별도 Git 묶음을 다운로드하거나 배포하지 않으며, 선택한 설치 위치의 실행 파일·의존성과 업데이트 후 재확인을 연결해야 한다. 이 결정은 임의 PATH 실행이나 Windows 제품 활성화 승인이 아니다.

- 채택하지 않은 대안: 공식 [MinGit 설명](https://gitforwindows.org/mingit.html)은 타사 앱용 비대화형 배포판이며 git.exe와 의존성을 함께 제공한다고 명시한다. GUI/Bash 전체 배포보다 필요한 구성이 작지만 실제 앱 용량은 패키지 측정 전 수치로 단정하지 않는다.
- MinGit도 alias/hook 실행을 지원한다. 번들 채택이 기존 config/hook/filter/외부 명령 격리를 대체하지 않는다.
- 앱 포함 방식을 선택하면 설치 위치는 renderer/프로젝트/PATH에서 받지 않고 앱의 배포 자원 경로에서 결정한다. 전체 의존성 묶음과 업데이트의 신뢰를 확인하며, 파일 ID 결속만으로 출처를 증명했다고 보지 않는다. 실행 중 묶음 교체 금지와 실패 시 이전 정상 묶음 보존이 필요하다.
- 기존 PC Git을 선택하면 명시적 설치 위치·실제 image/의존성·업데이트 호환을 별도로 검증한다. PATH의 첫 git.exe 또는 특정 내부 mingw 디렉터리 이름만으로 신뢰하지 않는다.
- 선택과 무관하게 공통 호출 경계는 인수 배열·입력 필요 여부·취소 토큰을 받고, Windows에서 image/CWD capability와 전체 자손 종료 Transport로 연결한다. bundle export·tree/blob materialization·diff batch reader가 별도 일반 프로세스 경로를 갖지 않는다.

---

### Windows 편집 저장 포트 — 구현 전 확인한 조건 (2026-11-05)

아래 조건을 정리한 당시에는 `commit_pending_save`의 최종 교체가 macOS에만 구현되어 있었다. 이후 Windows handle 교체·cleanup·명시 복구와 공개 확인 흐름을 구현했으나 native 검증/제품 활성화는 남아 있다. 현행 구조는 [architecture](../architecture.md)를 따른다. `PreparedTarget`의 실패 시 temp 정리와 이후 directory sync/파일 재열기/원장 해제까지 함께 이식해야 한다. syscall 하나의 교체로 끝내지 않는다.

1. **경로 보호와 교체 대상 분리**: `DirectoryLease::acquire(parent, target)`의 guard는 delete 공유를 거부하므로 의도한 target 교체도 막는다. syscall 직전에 guard를 drop하는 방식은 보호 공백을 만든다. 대상/이동할 temp와 다른 guard로 parent의 nonempty 상태를 유지해야 한다. 사용할 sibling이 없는 디렉터리는 앱 소유 snapshot 내부에만 새 보호 파일을 만들고, 생성 전 복구 intent·정확한 파일 ID·cleanup 상태를 기록하는 절차가 필요하다. 사용자 원본에는 만들지 않는다. 임의의 기존 파일을 guard로 선택하거나 이름만으로 삭제하지 않는다.
2. **핸들 기반 source와 destination**: Microsoft [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)는 상대 이름과 RootDirectory handle을 지원한다고 명시한다. source는 DELETE 접근으로 연 실제 temp handle, destination은 검증한 parent handle과 정상 단일 이름을 사용한다. Windows API native 실행 증거 없이 이 형태가 현재 filesystem/handle sharing 조합에서 작동한다고 선언하지 않는다.
3. **실제 buffer/권한 계약**: `FileNameLength`는 UTF-16 바이트 수이며 구조체 정렬과 유연 배열 길이를 검증한다. [FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)의 같은 volume·DELETE 접근·열린 target/readonly 제한을 반영한다. POSIX/ignore-readonly 옵션으로 사용자 권한을 무조건 우회하지 않는다. 파일 내용뿐 아니라 필요한 접근 권한 보존도 native 확인 대상이다.
4. **교체 전후**: stable managed root·parent chain·temp ID·target 현재 버전을 재확인하고 기존 두 failure hook의 의미를 유지한다. 교체 뒤 destination을 다시 열어 staged bytes/ID와 비교한다. directory durability가 확인되지 않거나 결과가 불명확하면 save lock/증거를 보존하고 성공으로 반환하지 않는다. `PreparedTarget::Drop`이 불명확한 교체의 유일한 복구 증거를 삭제하지 않도록 소유권도 변경해야 한다.
5. **묶음 native 회귀**: 정상 교체, 외부 수정 충돌, ancestor rename/reparse 교체, target handle sharing 충돌, temp 교체, 다른 volume 거절, readonly 대상, 교체 직전/직후 중단 및 재시작 복구를 자체 temp snapshot으로 확인한다. 사용자 프로젝트·실제 모델을 사용하지 않는다. 로컬 macOS 검사나 단순 API 성공을 Windows 저장/복구 합격으로 대체하지 않는다.

이 절은 구현 조건이며 저장 활성화 승인이 아니다. 현재 Windows 원본 쓰기·편집 저장 gate는 유지한다.

---

## 13. 브랜치와 의사결정 정책

- 통합 기준선: `m5/coding-agent-ide`
- Claude·Codex의 기능/실험 브랜치는 이 브랜치에서 분기하고 이 브랜치로만 병합한다.
- M5 작업 중 `main`에 직접 커밋·병합·push하지 않는다.
- `main` 병합은 M5 전체 완료·검증 뒤 사용자의 명시적 승인 게이트다.
- 이 문서의 **확정된 사용자 결정**과 M5 안전 기준선은 구현 계약이다. **현재 제안**과 §12 후속 질문은 해당 마일스톤에서 검토하며, 변경 시 사용자 결정과 안전 불변식을 명시적으로 갱신한다.

## 14. 다음 실행 순서

1. `storage-scheduler`·`storage-status`·`storage-compaction` 하네스 스크립트와 증거 gate를 먼저 구현한다. 세 gate가 모두 존재해 통과하기 전까지 누적 G5D gate는 fail-closed `NOT_READY`를 유지하고, 구현 뒤 최신 통합 commit에서 세 gate와 누적 G5D gate를 실행해 activation V4의 통합 상태를 판정한다. 물리 cleanup은 계속 열지 않는다.
2. Windows standalone snapshot·프로세스 자손 회수 안전 게이트와, 별도 승인된 실제 사용자 DB·강제 종료 복구 검증을 완료한다.
3. 패키징·성능·전체 OS Quit·서명/권한 문자열 hardening을 검증하고 릴리스 판정을 받는다.
4. 실제 Claude Code/Codex adapter의 argv·모델 가용성·공개 이벤트 정규화·세션/취소 계약을 fixture와 분리해 고정한다.
5. 실제 유료 실행과 사용자 인증은 별도 승인을 받은 smoke에서만 검증한다.
6. 실제 adapter가 기존 안전 불변식을 유지한 뒤 적용·폐기·복원 루프를 다음 제품 단위로 연다.


### 2026-11-10 UI 완성도 재검토 (현재 구현 아님)

사용자는 최신 Orca와 비교해 현재 UI가 조잡하다고 지적했다. 실제 최신 QA 시작 화면(output.png, m5-visible-resume-2026-11-10)을 확인하면 큰 안내 카드/반복 설명, 프로젝트를 닫은 재방문 상태의 튜토리얼과 기기 정보, 잘리는 좁은 프로젝트 목록이 동시에 노출된다. 이번 관측은 시작 화면이며 모든 작업대 화면의 시각 검증을 마쳤다는 주장은 아니다.

Orca 공식 사이트(https://www.onorca.dev/, 확인2026-11-10)는 프로젝트/작업 탐색, 작업별 분할 패널, 채팅/편집/diff와 빠른 전환을 소개한다. 공식 저장소 https://github.com/saiichi/orca 도 확인했다. 기능 목록의 확장을 그대로 M5 완료 조건에 추가하지 않는다.

다음 UI 파동의 제안은 기존 기능의 계층과 흐름을 정돈하는 것이다. ① 재방문 홈은 최근 프로젝트·대화 재개를 우선하고 최초 설치/로그인은 준비가 필요한 경우에 보여준다. ② 공통 글자 크기·간격·버튼 높이·색상·경계선과 패널 헤더를 정리하고 프로젝트/채팅 탐색에 읽을 수 있는 너비를 준다. ③ 작업 중 화면은 목표 입력·에이전트 진행·코드/diff 검토를 중심으로 구성하며 일반 설명/기기 진단은 도움말·설정으로 이동한다. 필수 동의와 복구 제한은 숨기거나 생략하지 않는다.

기존 시안의 다중 프로젝트/프로젝트별 역할·모델/역할 대화/편집/diff/드래그 배치·크기 조정은 유지한다. 새 시안을 반복 작성하는 대신 실제 React 화면에 적용하고 첫 방문·재방문·진행·검토·복구 대표 상태를 확인하는 것을 권고한다. 이 항목은 개선 방향이며 이번에 UI를 변경했다는 뜻은 아니다.

후속 구현 기록(2026-11-10): 위 UI 개선 방향은 실제 renderer에 반영하고 대표 상태를 확인했다. 당시 제안/관측은 보존하며 현재 구성은 `../architecture.md`의 “M5 화면 구성 갱신”, 검증 범위와 미완료 출시 항목은 `../history.md`를 참조한다.

### 2026-11-10 Orca 오케스트레이션 재확인

공식 [Orchestration 문서](https://www.onorca.dev/docs/cli/orchestration)에는 작업 의존성, coordinator inbox, 서로 다른 공급자의 worker-start, worker별 model/effort, 결과 보고·질문·결정 게이트가 명시돼 있다. 공식 [orchestration guide](https://github.com/stablyai/orca/blob/main/skill-guides/orchestration.md)는 Claude에서 Codex를 worker로 쓰는 경우를 명시한다. 따라서 단순 병렬 실행만 지원한다거나 이종 에이전트 감독이 없다는 가정으로 차별성을 주장하지 않는다. 문서 확인이며 설치 버전에서 실제 동작을 시험한 것은 아니다. 공식 문서는 실험적 기능과 패널/스위치의 일시적 비가용도 명시한다.

현재 우리 M5는 사용자 역할 선택으로 실행하는 `ChatUiController.startAgent`와 역할별 대화/설정/작업 복사본을 제공한다. 앱 개발에 사용한 Codex 서브에이전트 운영과 제품 자체의 AI coordinator 구현을 혼동하지 않는다. AI가 목표를 분해하고 worker 선택·결과 판정·재지시·완료 판단을 수행하는 제품 기능의 완성은 이 코드/화면만으로 주장할 수 없다.

**현재 제안(미확정):** 차별화는 초보자의 목표 하나를 받아 계획·위임·검토·재작업을 운영하는 기본 흐름, 공개된 역할 간 요청/결과의 통합 표시, 예산·완료 기준·원본 반영/복원의 이해 쉬운 제어로 검증한다. 이종 모델 조합 자체의 독점성이나 제품 경쟁력은 보장하지 않는다.
