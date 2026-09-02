# Hello, Agent 기술 설계

> 대상 독자: 이 프로젝트에 기여하는 개발자. 제품 배경·시장 조사는 기획 문서(Obsidian) 참고.
> 다이어그램: [시스템 구성](diagrams/architecture.excalidraw) · [위저드 플로우](diagrams/wizard-flow.excalidraw)

## 1. 목표와 비목표

**목표**: 터미널을 열어본 적 없는 사용자가 앱 더블클릭 → 안내를 따라가기만 하면 Claude Code 또는 Codex가 동작하는 상태(첫 대화 성공)까지 도달하고, 이후 프로젝트와 에이전트 상태를 한곳에서 관리한다.

**비목표 (MVP 기준)**:
- ~~크로스 에이전트 지원 — 2단계~~ → **2026-07-16 스코프 변경**: Codex를 조기 편입 (위저드 0단계에서 에이전트 선택). Gemini CLI 등 추가 확장은 여전히 2단계
- ~~영어 UI — 2단계~~ → 한국어·영어 전환 구현 완료
- 임베디드 터미널 — 터미널 노출 0이 원칙, 닥터 기능도 해석된 한국어로만 표시
- Linux 지원

## 2. 시스템 구성

현재 배포 앱은 Tauri이고, M5 전환 골격은 같은 저장소에서 별도 Electron 진입점과 공용 Rust 안전 코어로 구성된다. 아직 Electron이 기본 실행·배포 경로를 대체하지는 않았다.

```
[React 위저드 UI (WebView)] ←invoke/이벤트→ [Tauri Core (Rust)] → [OS 셸·파일시스템·PATH]
                                    ↑
                          [설치 레시피 (JSON 데이터)]

[M5 React + Monaco] → [typed preload] → [Electron main] ⇄ JSONL v1 ⇄ [Rust sidecar]
       app://m5              sandboxed        │                              ├─ SQLite v1
                                               └─ 앱 수명주기                 ├─ standalone snapshot
                                                                              └─ hello-agent-core
```

| 레이어 | 역할 | 기술 |
|---|---|---|
| 위저드 UI | 단계 표시, 쉬운 한국어 안내, 선택 분기 | React 19 + TS, 상태 머신 |
| Tauri Core | 환경 감지, 명령 실행, PATH 조작, 진행 이벤트 발행 | Rust + Tauri 2 플러그인(shell, os, fs) |
| 레시피 | "무엇을 어떻게 설치·검증하는가"를 코드가 아닌 데이터로 정의 | JSON (앱 동봉, 추후 원격 갱신) |

**Rust 최소화 원칙**: 설치·감지 로직의 오케스트레이션은 프론트(TS)에서 레시피를 해석하며 진행하고, Rust는 Tauri 플러그인이 못 하는 것(레지스트리 조작, PATH 파일 수정 등)에만 커스텀 커맨드를 만든다. M0에서 이 경계를 확정한다.

## 3. 앱 구조: 홈베이스 + 위저드 상태 머신

**"일회성 도구" 탈출 (2026-07-22 도입)**: 앱은 단일 선형 위저드가 아니라 **뷰가 여럿인 홈베이스**다. 편집기가 못 하는 것(고장 시 재진단, 안전한 새 프로젝트 시작, 업데이트 알림)을 제공해 재방문 이유를 만든다.

```
앱 실행
  ├─ 프로젝트 발견 또는 준비된 에이전트 있음 → 홈
  │                                           ├─ 프로젝트 카드 → 폴더 열기
  │                                           ├─ "새 프로젝트" → 준비된 단계 건너뛰기
  │                                           └─ 상태·업데이트·스캔 실패 → 인라인 닥터
  ├─ 기준 폴더 스캔 실패 → 홈에서 원인·재시도·다른 폴더 선택
  └─ 프로젝트와 준비된 에이전트 모두 없음 → 위저드(첫 온보딩)
```

최상위 뷰 라우팅: `home | wizard | ...`. `App.tsx`가 `view` 상태로 분기하고, 위저드는 지금의 6단계 컴포넌트를 그대로 재사용한다.

### 위저드 상태 머신

6단계 선형 흐름(에이전트 선택 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식) + 실패 시 닥터 분기. 각 단계는 `idle → running → success | failed`.

```
에이전트 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식
   └─(실패)→ 닥터(kind별 원인·해결) → 해당 단계 재시도
```

**재방문 단축**: 설치·로그인된 에이전트가 하나면 "새 프로젝트"에서 바로 프로젝트 단계로 직행한다. 둘 다 준비됐으면 에이전트만 고른 뒤 프로젝트 단계로 이동한다.

### 영속성 (홈베이스의 전제)

프로젝트 목록의 진실은 사용자가 고른 기준 폴더의 **실제 디스크 상태**다. `scan_projects`가 바로 아래 하위 폴더의 `.claude`/`CLAUDE.md`/`AGENTS.md` 표식을 찾는다. `tauri-plugin-store`는 다음 보조 정보만 앱 설정 디렉터리 JSON에 저장한다.

- `baseDir`: 프로젝트를 찾을 기준 폴더
- `projects[]`: 경로별 생성·최근 사용 시각 — 스캔 결과 정렬용이며 목록 원본이 아님
- 언어 선택은 `localStorage`의 `hello-agent.lang`에 저장

에이전트 설치·로그인 상태는 저장하지 않고 **매번 실시간 감지**한다(사용자가 앱 밖에서 바꿀 수 있으므로 — 진실은 항상 시스템).

- **진단**: OS/아키텍처, Node 유무(참고용), 기존 Claude Code 설치·버전, PATH 상태 감지. 이미 설치된 사용자는 설치 단계를 건너뛴다.
- **설치**: 네이티브 인스톨러 실행(macOS: `curl | sh` 스크립트, Windows: PowerShell 스크립트)을 앱이 대행. 출력을 스트리밍으로 받아 진행률·상태 문구로 변환해 표시.
- **로그인**: `claude auth login --claudeai`를 백그라운드 실행 → CLI가 브라우저를 열고 대비용 URL도 출력 → 사용자가 브라우저 로그인 후 받은 확인 코드를 앱 입력창에 붙여넣으면 stdin으로 전달 → `claude auth status --json`으로 완료 확인 (M0에서 검증됨, §7).
- **첫 프로젝트**: `~/Documents/내-첫-프로젝트` 등 안전한 위치에 폴더 생성 → 안전 프리셋 적용 → 에이전트 실행 확인.
- **졸업식**: 첫 대화 1회 성공을 확인시키고 "이제 혼자 쓸 수 있다" 상태로 종료.

각 단계의 실행 내용은 레시피가 정의하므로, UI는 레시피 스텝의 종류(check/run/verify/choice)만 알면 된다.

## 4. 레시피 스키마 (초안)

설치 방식 변경(리스크: 벤더의 잦은 변경)에 대응하기 위한 구조.

**현 구현 (2026-07-16)**: `src-tauri/src/agent.rs`의 타입化 레시피 — 에이전트별 설치 방법(클로드: 공식 스크립트 / 코덱스: GitHub 릴리즈 바이너리), 감지 후보 경로, 로그인·상태·첫 대화 명령을 enum으로 정의. 새 에이전트는 이 파일 확장으로 추가한다. 아래 JSON 외부화는 원격 갱신(앱 업데이트 없이 레시피 수정)이 필요해지는 시점에 진행.

```jsonc
{
  "id": "claude-code",
  "version": 1,
  "platforms": {
    "darwin": {
      "detect": [ { "type": "command", "run": "claude --version", "parse": "semver" } ],
      "install": [
        { "type": "download-script", "url": "https://claude.ai/install.sh", "checksum": null },
        { "type": "run", "script": "install.sh", "progressPatterns": { "다운로드": "Downloading" } }
      ],
      "path": { "candidates": ["~/.local/bin"], "profile": "~/.zprofile" },
      "verify": [ { "type": "command", "run": "claude --version" } ],
      "errors": [
        { "match": "command not found", "explain": "설치는 됐지만 컴퓨터가 위치를 몰라요", "fix": "path-refresh" }
      ]
    },
    "win32": { /* PowerShell 기반 동형 구조 */ }
  }
}
```

핵심 아이디어: `errors[]`가 닥터 기능의 지식 베이스를 겸한다. 삽질 사례 분류(기획 문서 A~G)에서 수집한 에러 패턴을 여기에 축적한다.

## 5. 플랫폼별 전략

| 항목 | macOS | Windows |
|---|---|---|
| 설치 실행 | `sh` 스크립트 (사용자 권한) | PowerShell 스크립트, ExecutionPolicy 우회 필요 여부 확인 |
| PATH 반영 | `~/.zprofile`에 추가 + **앱 내 프로세스는 자체 env 갱신** (터미널 재시작 개념 자체를 없앰) | 사용자 환경변수(레지스트리 `HKCU\Environment`) + `WM_SETTINGCHANGE` 브로드캐스트 |
| 셸 선택 혼란 | 해당 없음 (zsh 고정) | 앱이 대행하므로 사용자에게 노출 안 함 |
| 검증 | `claude --version` 절대경로 실행 | 동일 |

원칙: **PATH가 안 잡혀도 앱은 절대경로로 실행할 수 있어야 한다.** PATH 설정은 "나중에 사용자가 직접 터미널을 열 때"를 위한 것이고, 위저드 진행 자체는 PATH에 의존하지 않는다.

## 6. 안전·보안 설계

- **Tauri capabilities 최소화**: shell 실행은 레시피가 정의한 명령 패턴만 허용 목록에 등록. 임의 명령 실행 API를 프론트에 노출하지 않는다.
- **안전 프리셋**: 첫 프로젝트에 적용하는 Claude Code 설정 기본값 — 자동 승인 비활성, 작업 폴더 격리(프로젝트 폴더 밖 접근 시 확인). "11GB 삭제 사건" 류의 사고 방지가 목적.
- **다운로드 무결성**: 인스톨러 스크립트는 공식 URL 고정. 체크섬 제공 시 검증 (M1 검토).
- **개인정보**: 앱은 계정 정보·토큰을 저장하지 않는다. 인증은 전적으로 Claude Code 자체 플로우에 위임.

## 7. M0에서 검증할 가정 (실패 시 설계 변경)

| # | 가정 | 실패 시 |
|---|---|---|
| 1 | Tauri shell 플러그인으로 인스톨러 스크립트를 실행하고 stdout/stderr를 실시간 스트리밍할 수 있다 | Rust 커스텀 커맨드로 대체, 그래도 안 되면 Electron 전환 검토 |
| 2 | 설치 직후 앱 프로세스에서 절대경로로 `claude --version` 실행이 된다 | PATH 반영 방식 재설계 |
| 3 | 로그인을 앱에서 트리거하고 완료를 감지할 수 있다 (`claude setup-token`의 비대화형 활용 또는 설정 파일 폴링) | "로그인은 안내만 하고 완료 확인은 검증 명령으로 대체"로 다운그레이드 |
| 4 | Windows에서 동일 흐름이 PowerShell로 재현된다 | Windows 전용 레시피 분기 확대 |

### 검증 현황 (2026-07-16)

- **가정 1 — 검증됨 (설계 일부 변경)**: shell 플러그인 대신 처음부터 Rust 커스텀 커맨드(`install.rs`)로 구현. HOME 오버라이드·스트리밍·프로파일 수정이 모두 필요해 Rust 쪽이 자연스러웠음. stdout/stderr 실시간 스트리밍(Channel) 동작 확인.
- **가정 2 — 검증됨**: 임시 HOME 격리 설치에서 `~/.local/bin/claude --version` 절대경로 실행 성공 (2.1.211).
- **추가 발견**: 공식 install.sh는 셸 프로파일이 없으면 PATH를 건드리지 않고 안내문만 출력 → PATH 반영은 앱이 직접 수행해야 함 (`ensure_path`가 `~/.zshrc`에 추가, §5 원칙대로 위저드 진행은 PATH 비의존).
- **가정 3 — 검증됨 (예상보다 단순)**: CLI 2.1.x가 `claude auth login/logout/status`를 공식 제공. `auth status --json`이 로그인 여부·이메일·요금제를 구조화해 반환하고, `auth login`은 TTY 없이도 로그인 URL을 출력하고 확인 코드를 stdin으로 받는다. setup-token·설정 파일 폴링 같은 우회가 불필요.
- **추가 발견**: macOS 자격증명은 키체인(사용자 전역)에 저장되어 HOME 격리로는 로그아웃 상태를 재현할 수 없음. 미로그인 → 로그인 완료 플로우의 최종 확인은 별도 macOS 계정 또는 VM에서 필요 (실계정 logout은 사용자 세션을 끊으므로 개발 머신에서 금지).
- **가정 4 — 검증됨 (CI, 2026-07-16)**: GitHub Actions `windows-latest` 격리 E2E에서 install.ps1 무인 실행 → `claude.exe` 절대경로 검증까지 통과. Windows 인스톨러도 macOS처럼 PATH를 등록하지 않아 `ensure_path`가 사용자 PATH(HKCU 레지스트리)에 직접 추가하며, 실제 등록 성공 확인. 함정 하나: 부모 프로세스의 `PSModulePath`가 pwsh 7 경로로 오염되면 PowerShell 5.1이 기본 cmdlet을 못 찾음 → 자식 실행 시 해당 변수 제거로 해결. Windows GUI 위저드는 실기기 수동 테스트로 확인 (2026-07-16): 설치·미로그인→로그인 플로우 동작. 이때 발견 — 브라우저에 기존 세션이 있으면 확인 코드 없이 자동 승인되는 경로가 있어, 로그인 완료를 status 폴링으로 감지하도록 수정.

## 8. M4 자동 인계 파일럿 (macOS)

홈의 `HandoffPilot`은 사용자가 명시적으로 고른 Git 프로젝트를 앱 데이터 아래의 폐기 가능한 복제본으로 준비한 뒤, 읽기 전용 계획 역할과 구현 역할을 순차 실행한다. 사용자는 목표를 한 번 입력하고 앱 안에서 진행 상태·결과·변경 파일을 확인한다.

- `handoff.rs`: 단일 실행 슬롯, 역할 간 자동 인계, 쉬운 결과·복구 상태
- `claude_pilot.rs` / `codex_pilot.rs`: 에이전트별 고정 실행 계약과 증거 보존
- `proctree.rs`: `(pid, 시작 시각)` 누적 신원으로 Claude 자손 소멸을 확인하며, 확인 불가는 실행과 복제본을 `poisoned`로 잠금
- `orchestration.rs`: 매니페스트가 있는 격리 복제본만 준비·실행·폐기

파일럿은 macOS에서만 노출한다. Windows 자손 회수는 별도 게이트이며, §7.6B 품질·비용 비교와 재작업 루프·다역할 확장은 구현되지 않았다. 상세한 결정·철회·실측 근거는 `proposals/m4-orchestration.md`에 있다.

## 9. M5 G1~G5D 전환 골격·채팅 작업대

M5는 `Electron + React/TypeScript + Monaco + Rust sidecar` 골격, 언어 중립 protocol v1, SQLite 수명주기, macOS standalone snapshot 작업대, 기존 G5 fixture 실행·복구와 테스트 전용 G5A~G5C fake actual-agent의 normalized public/terminal event·자원 예산·provider 발견/실행 전 검증·재시작 안전 복구 수직 흐름까지 구현됐다. G5D는 기존 v1 원장을 보존하면서 exact storage schema v8까지 순차 migration하고, append-only source index와 명시 호출형 16단계 bounded lifetime materialization, 삭제된 operation·message의 독립 provenance 계약과 terminal compacted V8 read/replay를 추가했다. 기존 Tauri 위저드와 M4 파일럿은 계속 동작하며 Electron은 아직 기본 실행·배포 경로를 대체하지 않는다.

- `crates/hello-agent-core`: Tauri/Electron을 모르는 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약. 기존 Tauri 모듈은 좁은 re-export shim으로 이 코어를 사용한다
- `crates/hello-agent-sidecar`: 번들 SQLite의 단일 owner인 `--stdio` Rust 프로세스. G5는 프로젝트별 역할 설정을 revision CAS로 저장하고, 채팅 생성 시 역할·런타임·모델·추론 수준·권한 snapshot을 불변 기록으로 고정한다. 여러 채팅과 공개 `TeamMessage`·실행·복구 원장은 재시작 가능한 projection으로 남는다. 시작 시 exact predecessor schema를 mutation 전에 검증하며 v1→v8 additive migration을 순서대로 적용하고 원장·append-only guard를 보존한다. V6 append는 event·source index·projection·ready 무효화를 한 transaction으로 묶고, V7/V8은 future compaction의 prefix·message·operation provenance와 private proof 경계를 추가한다. 명시적 lifetime worker는 `indexSource`부터 source/projection/derived verification까지 16단계를 rowid keyset `LIMIT 100`, canonical payload 합계 1 MiB로 처리한다. terminal compacted V8은 frozen operation·receipt·prefix·baseline·retained anchor를 교차 검증해 baseline+tail replay, 후속 append와 materializer 의미를 보존하지만, 현재 재오픈 검증은 frozen operation 100개까지만 지원하고 101번째는 무변경 거부한다. 제품 event 삭제·compaction writer는 없으며 startup/background/wire에도 연결하지 않는다
- 채팅 준비는 기존 active/bound snapshot을 재사용하지 않는다. `workspace.prepareChat`은 매번 새 `preparationId/worktreeId/workspaceIdentity`의 unbound standalone snapshot을 만들고, `chat.create`가 설정 revision·최초 사용자 메시지와 함께 한 transaction에서 exact preparation을 bind한다. 실패 정리는 그 준비의 네 ID가 모두 맞을 때만 `workspace.abandonPrepared`로 terminal 표시하며 물리 삭제하지 않는다
- 채팅이 소유한 tree/read/save/watch/diff/close는 V3의 `chatId/projectId/worktreeId/workspaceIdentity` 네 ID를 매 요청·응답·event에서 교차 검증한다. save continuation은 같은 네 ID와 `saveId`가 모두 일치해야 하며, 현재 선택을 추론하거나 다른 채팅의 작업 복사본으로 fallback하지 않는다. G4 project-only V2 경로는 호환용으로 유지한다
- G5 legacy 실행은 macOS 결정적 fixture와 `M5BridgeV3`를 유지한다. 별도 G5A fake actual-agent 실행은 accepted response를 event보다 먼저 반환하고, full chat/task/project/worktree/workspace/run/role/fingerprint에 결속된 `status|question|final`만 `agent.publicEvent`로 발행한다. private vendor event는 sidecar에서 버리고 actual-agent raw `run.event`·`run.finished`·`chat.eventAppended` carrier는 Electron에서 fail-closed 차단한다. 두 실행 원장과 recovery namespace는 서로 섞지 않는다
- 취소는 `(pid, 시작 시각)`의 부모·자손 전체 소멸을 증명한 때만 clean이다. 불확실하면 chat/run/workspace/writer lock을 `recoveryRequired`·`poisoned`로 유지하고 `recovery.list/inspect/markResolved` 뒤 exact abandon만 허용한다. `app.prepareQuit`은 active run과 unresolved recovery를 blocker로 반환하며 안전하지 않으면 종료하지 않는다
- `crates/hello-agent-protocol`과 `src/m5/contracts/`는 같은 golden fixture로 protocol/session, SQLite, snapshot, G5 역할·채팅·실행·복구, preparation, V3 workspace 주소와 G5A/G5B actual-agent public/terminal event·자원 예산 경계를 Rust·TypeScript 양쪽에서 검증한다. `M5BridgeV5`는 V1~V4에 additive하며 범용 IPC를 열지 않는다
- `electron/`: Forge `8.0.0-alpha.10` 기반 main/preload/Vite 패키징과 main-owned sidecar supervisor. production sidecar는 `process.resourcesPath/sidecar/hello-agent-sidecar --stdio`, `shell: false`, 고정 cwd로 한 번만 실행한다. main-frame/app origin과 고정 channel을 검사하고 preload는 exact input/output validator를 통과한 V1~V5 메서드·bounded event 구독만 노출한다. exact G5A 협상 전에는 actual 메서드와 public/state listener를 활성화하지 않는다. 협상 뒤에도 full run binding·단조 상태 전이·전용 stream을 검증하고 active run 32·seen run 1,024·event ID 8,192·agent stream 1,024·protocol stream 4,096 한도를 넘으면 listener 전에 session을 종료한다
- `src/m5/`: 여러 영속 채팅, 프로젝트별 역할/모델 설정, 공개 팀 타임라인, fixture/actual 실행 중단·분리된 복구 gate와 채팅별 Monaco tree/edit/save/diff를 결합한다. 채팅 전환은 detail과 작업 복사본 bind가 모두 성공한 뒤 원자 publish하며 dirty 편집·복구·늦은 결과가 다른 채팅을 덮지 못하게 막는다. UI는 fixture 실행을 **가짜 에이전트 데모**로 명시하고 exact G5에서는 actual 동작을 unavailable로 유지한다. renderer는 normalized public/state event의 payload를 직접 화면 상태로 쓰지 않고 bounded `chat.get` 페이지를 durable sequence 끝까지 다시 읽어 `TeamMessage`와 별도 `agentRuns` 원장만 반영한다. `final` 내용 event를 process terminal 증거로 해석하지 않으며 zero-output 종료도 post-commit state notification 뒤 durable terminal 원장을 읽은 후에만 실행 잠금을 푼다
- fake actual-agent sidecar는 accepted 뒤 같은 run 전용 stream으로 `running`, public event, terminal `agent.runStateChanged`를 연속 발행한다. terminal event는 process-tree 소멸과 run/chat/lock/recovery transaction commit 뒤에만 만들며, terminal 저장 실패는 false event 없이 재시작 때 nonterminal run을 `recoveryRequired`와 poisoned lock으로 수렴시킨다. run당 public event 256·text 1 MiB·각 append의 canonical full-array prefix snapshot 누적 16 MiB를 append 전에 검사하고 초과 event는 저장·전달하지 않는다
- G5C provider core는 executable closure와 exact version/model/reasoning/permission, auth/event parser, managed cwd와 no-fallback 정책을 canonical fingerprint로 묶는다. 테스트 전용 fake Claude provider만 완전한 handshake 뒤 시작할 수 있고 Codex의 `argvOnly` 또는 불명확한 catalog는 계속 `canStart=false`다. raw stdout/stderr·reasoning·auth·prompt는 공개 원장과 renderer에 남기지 않는다
- actual recovery ID는 run 준비 시 writer lock과 함께 무작위로 예약하고 재시작 뒤에도 같은 ID를 쓴다. process identity가 없으면 `processIdentityUnavailable` unresolved recovery와 poisoned lock을 영구 보호 상태로 유지하며 UI도 폐기 RPC를 보내지 않는다. 일반 abandon은 SQLite intent → exact/idempotent manifest abandon → manifest 재검증 뒤 DB finalize 순서로 재조정 가능하고, 이전 G5A/G5B recovery는 공개 ID를 바꾸지 않고 reservation만 백필한다
- 패키지는 ASAR와 V1 Fuse 9개를 고정하고 release sidecar를 ASAR 밖 `Resources/sidecar`에 둔다. pnpm은 Forge 요구에 맞춘 hoisted `node_modules`를 쓰되 전이 Git/tarball 차단 정책은 완화하지 않는다

G5 기준선은 exact 통합 commit `014bd987464d39674a5f531765051f60e63b5ecb`에서 누적 하네스 PASS와 독립 QA `G5_READY`를 받았다. G5A fake adapter/public-event 추가와 hydration tail-race 보정 뒤 exact commit `24a8c712e6b5a57e69c9e852333c46bfab536dc7`도 `G5A_READY`를 받았다. G5B terminal notification·resource budget 통합 exact `022b79517462a1f9339c8cabb68b5fafd65eae0b`은 누적 G5 하네스와 macOS arm64 package smoke를 PASS했고, 독립 gate는 exact docs commit `3dcf75ab1c12cf4a21b05406f4fc814e42d485f7`에서 `G5B_READY`로 판정했다. G5C provider discovery·launch attestation과 restart-atomic recovery 통합 exact `42dcbfad2444eebbebaeef4e4f64f614a6de6264`도 누적 G5 하네스와 macOS arm64 package smoke를 PASS했다(`.artifacts/m5-harness/g5-run-recovery-2026-08-28T01-03-49-390Z.json`). 독립 QA는 no-fallback 발견, handshake mismatch 비공개, text/event 경계, exit-0 poison, random recovery ID·legacy namespace 분리, 재시작 saga, 이전 recovery ID 보존 백필과 identityless UI 비행동 상태를 재검증해 `G5C_QA_PASS`로 판정했다(`.artifacts/m5-qa/g5c-integration-42dcbfad2444eebbebaeef4e4f64f614a6de6264-2026-08-28T01-09-53Z.json`). 독립 gate는 exact docs commit `cdb3e1b49de5f0c6541ff007f007bcb65b7db122`에서 `G5C_READY`로 승인했다. 이 결과는 계약·SQLite·실제 sidecar stdio fixture 프로세스·Electron package를 검증하지만 **일반 `--stdio` 제품 경로의 실제 Claude Code/Codex adapter, 인증 세션, 실제 모델 출력은 검증하지 않는다**. G6 보안·패키징·성능, Windows snapshot/자손 회수, 전체 OS Quit E2E, Forge prerelease와 macOS 권한 문자열 hardening, 실제 에이전트 smoke는 후속 게이트다.

G5D R1의 `storage/reopen.rs`는 제품 시작 경로와 분리된 내부 frozen-operation 검증 세션이다. 같은 connection을 독점 차용하고 매 호출의 `BEGIN IMMEDIATE` 안에서 `data_version`을 대조한다. 호출당 최대 100개·canonical source 1 MiB·종속 UTF-8 입력 8 MiB를 decode 전에 확인하며 외부 commit·취소·오류는 volatile 진행을 폐기한다. 새 세션은 처음부터 검사하고 `FrozenOperationsVerified`는 실행 허용 증거로 소비되지 않는다. V8 schema와 기존 open/append/lifetime의 100개 초과 거부는 그대로다. prefix/message/full-startup 검증과 제품 활성화는 아직 없다.

## 10. 미해결 사항

- **actual-agent 전체 수명 보존**: G5D가 exact storage v8 migration, append-only source provenance, 호출형 bounded 무삭제 materialization과 제한된 terminal compacted V8 read/replay까지 구현했다. 실제 bounded compaction writer/delete, 100개 초과 frozen-operation 재오픈 proof, worker 자동 scheduling, 제품 `storage.status` wire와 여러 세션·채팅의 용량 threshold는 아직 없다. 보존 기간과 physical cleanup/삭제 정책은 사용자 결정 전 비활성이며, 실제 provider 활성화 전에 남은 G5D gate로 닫아야 한다.
- **코드 서명**: 미서명 배포 시 SmartScreen/Gatekeeper 경고. 베타는 미서명 + 통과 안내 이미지로 진행, 정식 릴리즈 전 비용(연 $250~400) 결정 — 기획 문서 참고.
- **실기기 출시 검증**: 깨끗한 macOS 계정과 Windows 기기에서 설치·로그인·오프라인·권한 오류·한글 경로 시나리오 확인.
- **업데이트 채널**: 현재 앱은 에이전트 최신 버전만 확인한다. Hello, Agent 자체 업데이트와 레시피 원격 갱신 방식은 첫 베타 이후 결정.
