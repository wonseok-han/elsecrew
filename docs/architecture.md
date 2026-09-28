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

후속 메시지 저장 통신은 `chat.appendMessage`를 지원한다. 현행 sidecar는 기존 exact G5D 제품/agent-fixture capability 배열의 마지막에 이 capability만 추가하며, Electron client는 해당 확장을 협상한 경우에만 요청한다. 기존 baseline 상수와 검증기는 그대로이고 current 상태 검증기는 정확한 baseline+선택적 suffix만 허용한다. V7 preload의 고정 `appendChatMessage` 채널과 팀 대화 입력창이 연결됐고 V4~V6는 유지된다. 요청의 전체 채팅/작업 공간 소속·client message ID·예상 durable 순서·본문을 저장 결과와 대조하며, 동일 요청 재시도는 같은 메시지를 반환하고 이벤트를 중복 발행하지 않는다. 초안과 불확실한 요청은 controller 수명 내에서 채팅별로 보존하며 앱 재시작까지의 초안 영속 저장은 아직 없다. 명시적 무기록 순서 충돌만 기록을 새로 읽고 같은 ID의 예상 순서를 갱신한다. 저장된 메시지는 다음 실제 실행의 inputMessageIds에 포함되지만, 실제 provider 활성화는 별도 미완료이고 메시지 저장 자체는 실행/실행 중 stdin 전달을 의미하지 않는다.

Electron main은 현재 main window의 main frame과 정확한 허용 origin만 IPC에 접근시키고 popup·webview·외부 navigation을 차단한다. `app://m5` 자산은 credential/port/query/fragment 없는 authority와 renderer 내부의 기존 regular file만 허용하며 잘못된 escape·경로 이탈·Windows 절대경로도 거부한다. `02dbaeb`의 VM 실행 회귀는 실제 main의 등록 callback을 호출해 이 경계를 검증한다. 이는 Electron/OS 자체의 sandbox·CSP·fuse 강제나 실제 패키지/Windows 검증을 대신하지 않는다.

M5는 `Electron + React/TypeScript + Monaco + Rust sidecar` 골격, 언어 중립 protocol v1, SQLite 수명주기, macOS standalone snapshot 작업대, 기존 G5 fixture 실행·복구와 테스트 전용 G5A~G5C fake actual-agent의 normalized public/terminal event·자원 예산·provider 발견/실행 전 검증·재시작 안전 복구 수직 흐름까지 구현됐다. G5D는 기존 v1 원장을 보존하면서 제품 admission의 exact storage schema v10 전환, append-only source index와 bounded lifetime materialization, V9 resume witness, V10 lossless compaction writer와 ready-only cooperative scheduler까지 연결했다. 기존 Tauri 위저드와 M4 파일럿은 계속 동작하며 Electron은 아직 기본 실행·배포 경로를 대체하지 않는다.

- `crates/hello-agent-core`: Tauri/Electron을 모르는 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약. 기존 Tauri 모듈은 좁은 re-export shim으로 이 코어를 사용한다
- `crates/hello-agent-sidecar`: 번들 SQLite의 단일 owner인 `--stdio` Rust 프로세스. G5는 프로젝트별 역할 설정을 revision CAS로 저장하고, 채팅 생성 시 역할·런타임·모델·추론 수준·권한 snapshot을 불변 기록으로 고정한다. 여러 채팅과 공개 `TeamMessage`·실행·복구 원장은 재시작 가능한 projection으로 남는다. 제품 admission은 exact predecessor schema를 mutation 전에 검증하고 v1→v10 additive migration과 V9 witness 기반 V10 plan-aware 재오픈을 수행한다. V6 append는 event·source index·projection·ready 무효화를 한 transaction으로 묶고, V7/V8 provenance, V9 immutable plan/resume witness, V10 atomic authority/delete guard를 보존한다. ready/non-quitting event loop의 cooperative scheduler는 V9/V10 materializer 한 step, persisted pending compaction, `project → chat → run → lock` keyset 후보 순으로 tick하며, exact safe prefix 1,000개일 때만 V10 writer가 source `events`와 대응 index를 bounded transaction으로 논리 삭제한다. 한 step은 100 work·primary 1 MiB·retained/dependency 8 MiB이고 별도 writer thread/connection은 없다. D1 `storage.status` wire는 기존 shape와 capability 순서 그대로 이미 연결됐고, user record와 물리 cleanup은 삭제하지 않는다
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
- 패키지는 ASAR와 V1 Fuse 9개를 고정하고 release sidecar를 ASAR 밖 `Resources/sidecar`에 둔다. staging은 같은 Cargo build가 보고한 exact sidecar manifest/bin/non-test executable artifact 하나를 사용하므로 shared/configured target 위치를 추측하지 않으며 실패·없음·모호한 artifact를 복사 전에 거부한다. pnpm은 Forge 요구에 맞춘 hoisted `node_modules`를 쓰되 전이 Git/tarball 차단 정책은 완화하지 않는다

G5 기준선은 exact 통합 commit `014bd987464d39674a5f531765051f60e63b5ecb`에서 누적 하네스 PASS와 독립 QA `G5_READY`를 받았다. G5A fake adapter/public-event 추가와 hydration tail-race 보정 뒤 exact commit `24a8c712e6b5a57e69c9e852333c46bfab536dc7`도 `G5A_READY`를 받았다. G5B terminal notification·resource budget 통합 exact `022b79517462a1f9339c8cabb68b5fafd65eae0b`은 누적 G5 하네스와 macOS arm64 package smoke를 PASS했고, 독립 gate는 exact docs commit `3dcf75ab1c12cf4a21b05406f4fc814e42d485f7`에서 `G5B_READY`로 판정했다. G5C provider discovery·launch attestation과 restart-atomic recovery 통합 exact `42dcbfad2444eebbebaeef4e4f64f614a6de6264`도 누적 G5 하네스와 macOS arm64 package smoke를 PASS했다(`.artifacts/m5-harness/g5-run-recovery-2026-08-28T01-03-49-390Z.json`). 독립 QA는 no-fallback 발견, handshake mismatch 비공개, text/event 경계, exit-0 poison, random recovery ID·legacy namespace 분리, 재시작 saga, 이전 recovery ID 보존 백필과 identityless UI 비행동 상태를 재검증해 `G5C_QA_PASS`로 판정했다(`.artifacts/m5-qa/g5c-integration-42dcbfad2444eebbebaeef4e4f64f614a6de6264-2026-08-28T01-09-53Z.json`). 독립 gate는 exact docs commit `cdb3e1b49de5f0c6541ff007f007bcb65b7db122`에서 `G5C_READY`로 승인했다. 이 결과는 계약·SQLite·실제 sidecar stdio fixture 프로세스·Electron package를 검증하지만 **일반 `--stdio` 제품 경로의 실제 Claude Code/Codex adapter, 인증 세션, 실제 모델 출력은 검증하지 않는다**. G6 보안·패키징·성능, Windows snapshot/자손 회수, 전체 OS Quit E2E, Forge prerelease와 macOS 권한 문자열 hardening, 실제 에이전트 smoke는 후속 게이트다.

G5D R1의 `storage/reopen.rs`는 제품 시작 경로와 분리된 내부 frozen-operation 검증 세션이다. 같은 connection을 독점 차용하고 매 호출의 `BEGIN IMMEDIATE` 안에서 `data_version`을 대조한다. 호출당 최대 100개·canonical source 1 MiB·종속 UTF-8 입력 8 MiB를 decode 전에 확인하며 외부 commit·취소·오류는 volatile 진행을 폐기한다. 새 세션은 처음부터 검사하고 `FrozenOperationsVerified`는 실행 허용 증거로 소비되지 않는다. V8 schema와 기존 open/append/lifetime의 100개 초과 거부는 그대로다.

R2의 `start_compaction_closure`는 같은 세션 관리 아래 `operations → prefixes → frozen messages`를 호출당 한 단계씩 처리한다. `storage/reopen/closure.rs`는 삭제 source와 정확한 sequence의 prefix/baseline, historical frozen anchor와 최신 live anchor, 삭제된 chat introduction과 frozen/normalized message를 양방향으로 결속한다. null-or-same snapshot과 live anchor의 새 메시지를 허용하며 정상 unreferenced baseline을 거부하지 않는다. 단계별 primary 100행·canonical 합계 1 MiB와 고정 종속 조회 전체의 raw UTF-8 8 MiB를 읽기 전에 검사한다. prefix 벡터나 영속 cursor/proof는 만들지 않고 `CompactionClosureVerified`도 정보성 결과다.

R3A의 `start_ledger_projection`과 `storage/reopen/projection.rs`는 compacted V8의 R2 검증 뒤 같은 연결·변경 감지 수명에서 `ledger → app instances → projects → chats → runs → locks`를 처리한다. 정상 uncompacted/empty V8는 압축 table의 exact-empty 확인 뒤 ledger부터 시작한다. exact predecessor·최신 prefix/retained anchor·rowid 순서와 source index를 검증하고, latest source와 다섯 projection의 JSON/typed/indexed column을 양방향으로 대조한다. pending의 미생성 index는 source에서 비교 사실을 구성해 허용하지만 존재하는 index와 ready coverage는 검사한다. 여섯 단계의 primary 100행·1 MiB 및 모든 실제 종속 UTF-8 입력 8 MiB를 decode 전에 제한하고, commit 후에만 진행을 공개한다. `LedgerProjectionVerified`도 정보성이며 뒤의 R3B나 제품 admission을 대신하지 않는다.

R3B의 `start_derived_records`와 `storage/reopen/derived/`는 같은 연결·최초 변경 감지 epoch에서 R2/R3A 뒤 다섯 파생 table을 역검증하고 ready 상태의 정방향 coverage도 확인한다. index-empty pending receipt/baseline은 raw live tuple을 먼저 찾고 없을 때만 검증된 frozen source와 결속한다. 모든 live introduction을 indexed/frozen identity와 대조하며 unindexed introduction에는 전역 raw keyset 탐색을 추가한다. historical pending workspace의 exact source와 bound chat의 최신 raw source를 검사하고 100개 초과 member도 나눠 처리한다. outer/candidate/member를 합쳐 호출당 100 work units·fresh primary 1 MiB, retained outer와 실제 읽는 UTF-8 입력을 합쳐 8 MiB로 제한하며 실패·취소·외부 commit은 모든 진행을 폐기한다. `DerivedRecordsVerified`는 정보성 결과다. exact 통합 `e5f9f01c397f3482975a231621ee5b16aa4381e8`에서 누적 13단계·R3B 21개 회귀·독립 QA와 최종 gate를 통과했다. **이 기존 경로의 개별 기록 크기 제한과 제품의 frozen operation 100개 초과 재오픈 제한은 유지한다.** 큰 기록의 내부 연결은 아래 private maintenance 경로로 분리되며 제품 integrity/FK·복구 사전 검증·startup/admission은 미완료다.

R3C-S1의 `storage/reopen/bytes.rs`는 제품 경로와 분리된 `RawColumnSession`이다. live event payload와 frozen payload/envelope의 고정 주소를 10필드 source identity·최초 byte 길이·동일 연결의 변경 감지 epoch에 결속한다. 호출당 `BEGIN IMMEDIATE` 안에서 최대 1 MiB BLOB 조각을 읽고 retained identity/UTF-8 carry와 실제 새 입력을 합쳐 8 MiB 이내로 제한한다. UTF-8 carry는 최대 3바이트이며 hash/cursor는 COMMIT 성공 뒤에만 보존하고 오류·취소·외부 commit·완료 뒤에는 재사용을 거부한다. 결과는 UTF-8/길이/SHA 정보일 뿐 JSON canonical·의미·provenance 검증이나 admission 증거가 아니다. 기능 `00f54d9`의 집중 회귀 12개·기존 snapshot 대조 1개와 독립 리뷰를 통과해 통합했다. SQLite 내부 메모리·전체 ready 시간 상한을 보장하지 않는다.

R3C-S2의 `storage/reopen/canonical.rs`는 같은 고정 주소·연결·epoch에 결속된 private canonical scanner다. 전체 source/객체/키를 보유하지 않고 canonical 문자열·깊이 127·현재 serde Number 출력 및 인접 decoded key의 엄격한 정렬을 검증한다. 긴 키는 source span을 재탐색하며 main/key 읽기를 합친 호출당 1 MiB fresh·8 MiB retained+fresh·100 work units를 지키고 COMMIT 뒤에만 진행을 보존한다. 기능 `7eeca1d`·`5c7758e`의 실제 V8 oracle 43사례와 잘못된 write 대조 3사례, scanner 10개·S1 12개·snapshot 1개 및 단일 독립 리뷰가 완료됐다. canonical 완료를 제품 실행 허가나 frozen envelope 내부 payload/provenance 증명으로 사용하지 않는다.

R3C-S3의 `storage/reopen/semantic.rs`는 live/frozen payload와 projects/chats/runs/locks의 정확한 한 행을 같은 연결·epoch에서 canonical→projection 전체 문법→의미→SQLite mirror 순서로 대조한다. 전체 JSON/키를 보관하지 않고 재탐색하며 duplicate의 마지막 값·루트 typed sequence·숫자 종류를 보존하고, 덮이는 값도 먼저 문법 검증한다. 기존 SQLite의 첫 duplicate 및 evidence 표현 비교는 typed primary-key 한 행 조회로 별도 유지한다. S1/S2의 예산·COMMIT 뒤 진행 보존·terminal 폐기 규칙은 그대로다. `5ee0726`의 S3 18개·공유 회귀 23개 및 단일 독립 리뷰가 완료됐다. 한 행 비교를 제품 실행 허가로 사용하지 않으며 SQLite 내부 메모리·전체 ready 시간도 이 결과로 보장하지 않는다.

새 private `storage/reopen/snapshot.rs`의 `ReopenMaintenanceSession`은 승인된 분할 정책 아래 전역 source/frozen envelope·5 projection·5 derived table의 양방향 검증, index 선행 catch-up, ready 전체 재검증을 같은 연결에서 수행한다. `snapshot/workspace.rs`는 길이 제한 없는 manifest 경로와 historical live/frozen 출처를 조각으로 검사하고, `snapshot/catchup.rs`는 검증한 원본에서 파생 기록만 저장한다. COMMIT 뒤에만 진행을 보존하며 member 중간 재시작은 durable offset 없이 source 처음부터 멱등 처리한다. 기존 V1의 개별 기록 1 MiB 거부와 제품 진입점은 그대로다. 기능 `5ad7162`의 focused 회귀·독립 리뷰 뒤 통합 `5658480`에서 정상 production101을 포함한 workspace Rust 529 passed/14 ignored/0 failed, exit 0을 확인했다. 실제 생산 101-workspace DB 보존본의 별도 완료는 172,685 calls/98.288초로 **3초 제품 목표를 충족하지 않는다**. 이 `ReadyVerified`는 저장 정합성 완료이지 제품 실행 허가가 아니며 startup/복구/admission·성능은 여전히 미완료다.

## 10. 미해결 사항

- **변경 검토 현행**: 텍스트 diff는 변경 전후 줄 번호·추가/삭제 색상과 기호를 표시하며 페이지당 200행으로 전체 내용을 탐색한다. 불완전/미지원 hunk는 원문 표시로 보존하고 이 파서는 파일 적용 권위로 사용하지 않는다. 파일 반영·복원 루프는 여전히 후속이다.

- **편집 초안 재읽기**: dirty/충돌 탭은 명시적 확인 후 저장된 파일을 다시 읽을 수 있다. 읽기 완료까지 초안을 보존하고 확인 이후 변경된 탭·프로젝트/작업 복사본·차단 상태에서는 교체하지 않는다. 충돌 초안은 복사 가능하다. 파일 쓰기나 원본 프로젝트 적용/되돌리기가 아니며 Monaco의 동일 텍스트 동기화는 dirty를 만들지 않는다.

- **저장 중 입력 보존**: editor controller는 저장 중 추가 입력을 허용하되 saving 상태로 중복 저장을 막는다. 성공 시 baseVersion을 갱신하고 이후 입력은 dirty로 남겨 다음 저장에 사용한다. 외부 watcher 충돌이 관측되면 늦은 성공보다 conflicted 상태를 우선한다.

- **패널 접기와 집중**: dock은 순서/폭 외에 숨김 상태를 저장한다. 접힌 패널은 DOM을 유지해 Monaco를 폐기하지 않으며 최소 한 패널을 표시한다. 대화 패널만 표시하다 채팅을 벗어나면 편집기를 노출한다. 전체 시안 적용·상하 분할 도킹 완료를 뜻하지 않는다.

- **모델 없는 Codex 통신 순서**: `vendor_observation/codex_rpc.rs`는 initialize/config/account/model 4개 요청의 single-flight 순서와 응답 ID를 결속하며 예상 밖 frame은 실패로 고정한다. RawValue로 중첩 중복 키를 보존해 vendor 파서에 전달한다. 알림 일반 처리·process runner·timeout·설정 격리·실제 입력 전달/실행 권위는 이 모듈의 기능이 아니다.

- **제품 로그인 연결(macOS)**: `provider_login_manager.rs` → 고정 `provider.login` → Electron main/preload → 작업대의 Codex·Claude Code 버튼을 연결한다. provider별 manager/attempt/취소를 분리하며 provider 생략은 기존 Codex 요청과 호환된다. start/check/status/cancel과 Claude 전용 submitCode만 허용하고 모델 호출은 하지 않는다. main만 provider와 일치하는 공식 HTTPS URL을 열며 renderer/영속 기록에는 URL·계정·토큰을 전달하지 않는다. `pnpm m5:dev`는 실제 debug sidecar를 사용한다. Windows는 미연결이며 이번 두-provider 변경의 독립 안전 검토는 아직 대기다.
- **Claude 수동 연결 코드**: waiting 화면의 비밀번호형 입력창에서 브라우저 반환 코드 전체를 입력한다. 고정 submitCode 요청은 현재 살아 있는 Claude attempt에만 한 번 허용하고 verifying으로 전환한다. 제한된 문자/4096바이트 검증 후 같은 worker의 stdin에 줄바꿈과 함께 전달한다. 코드가 포함된 요청·출력은 로그/DB/상태 응답에 남기지 않는다. 취소·새 시도·완료·종료 준비에서 대기 코드를 비우고, 제출 후 30초 내 CLI 종료 및 fresh auth status와 process cleanup을 확인해야 connected다. 2026-09-15 사용자가 실제 연결 성공을 보고했다. 2026-09-28 새 앱 실행에서 재로그인 없는 복원을 직접 확인했으며 독립 안전 검토는 남는다.
- **Codex 전용 환경·복원**: `provider_profile.rs`의 private profile을 HOME/CODEX_HOME으로 전달하고 상속 환경을 비운다. 서명 확인 및 SHA-256 pin으로 대조한 Codex 0.153.4, 0.154.0-alpha.6.2, 0.158.0-alpha.2.1의 exact CLI/host 쌍만 실행한다. 최신 번들은 Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex와 codex-cli/bin/codex-code-mode-host를 직접 선택하며 bin/codex shell wrapper는 실행하지 않는다. 쌍 혼합·후보 해시 자동 신뢰는 거절하고 실행 파일 변경/부재는 별도 오류로 표시한다. 원래 터미널 인증은 읽거나 복사하지 않는다. 고정 config/auth 외에 관측한 skills/tmp/.tmp, installation_id 및 고정 SQLite 이름만 소유권/종류/권한 검사 후 허용한다. 앱 진입의 check는 initialize→initialized→account/read(refreshToken:false)→cleanup만 수행하며 OAuth를 시작하지 않는다. 명시적 null 계정은 check에서만 미연결이고 브라우저 인증 후에는 실패다. 2026-09-28 최신 native pair로 실제 앱 재시작 후 재로그인 없이 연결 복원을 확인했다. 향후 미검증 업데이트는 여전히 차단한다.
- **로그인 중 종료**: Electron quit coordinator는 로그인 blocker만 남으면 최대 40회, 250ms 간격으로 종료 준비를 다시 요청한다. 두 provider cleanup과 sidecar의 clean 판정 전에는 종료하지 않는다. 다른 blocker·오류·예산 소진은 fail-closed이며 잠금을 자동 해제하거나 강제 종료하지 않는다.
- **Claude 전용 환경·상태**: `provider_claude_profile.rs`가 `claude-profile-v1`의 신원을 보존하고 `forceLoginMethod:claudeai` 설정을 대조한다. `provider_claude_login.rs`는 strict 서명 검증한 2.1.270/2.1.275의 고정 설치 버전 파일과 SHA-256을 대조해 `auth login --claudeai`/`auth status --json`만 실행한다. HOME은 기본 키체인 탐색을 위해 OS 사용자 경로를 유지하고 CLAUDE_CONFIG_DIR·cwd·tmp는 앱 전용 폴더로 고정한다. 나머지 상속 환경·자동 업데이트·불필요 통신·CLI 브라우저 실행을 제한한다. 공식 [인증 저장 규칙](https://code.claude.com/docs/en/authentication#credential-management)에 따라 별도 config 폴더는 Keychain 항목도 분리한다. 앱이 키체인이나 인증 파일 내용을 직접 읽거나 복사하지 않는다. 로그인 안내 stderr는 bounded 임시 pipe로 합치며 실제 2.1.270에서 관측한 `https://claude.com/cai/oauth/authorize`와 기존 `https://claude.ai/oauth/authorize` 경로만 main에 전달한다. CLI 종료 후 새 status의 종료 코드·claude.ai 인증 종류·firstParty·configDirectory와 cleanup을 확인해야 연결됨을 표시한다. 잘못된 HOME으로 기본 키체인을 못 찾던 문제를 수정했고 새 환경의 실제 상태 조회는 미연결로 정상 처리됐다. 2026-09-15 사용자가 Claude 인증 완료를 보고했으며 2026-09-28 2.1.275에서 재시작 복원을 직접 확인했다. [공식 CLI 명령](https://code.claude.com/docs/en/cli-reference).

- **로그인 profile 신원 재검사**: `CodexProfileBinding::from_storage`는 열린 Storage에서 부모 경로를 받아 no-follow로 app-data/profile handle을 보존한다. 실행 전 현재 dev/ino·정책을 다시 대조하고 삭제된 config/폴더를 복구하지 않는다. 로그인 owner가 사용하지만 이후 동일 UID 경합까지 막는 원자적 실행 권위는 아니다.

- **로그인 stdio**: `provider_login_io.rs`는 caller가 소유한 child의 stdin/stdout만 nonblocking으로 연결하고 줄/누적 byte·timeout·cancel을 제한한다. 불완전 frame·EOF·쓰기 실패는 연결 재사용을 막으며 원문을 공개 오류에 포함하지 않는다. 송신은 core login/connection이 생성한 opaque `LoginRequest`(초기화/초기화 알림/시작/취소/계정 조회)만 받는다. 아래 내부 lifecycle owner가 통신 대기 중 추적 callback을 제공한다. bounded poll의 None은 데이터 대기이며 EOF와 구별한다.
- **로그인 session**: `provider_login_session.rs`가 core `codex_login/connection.rs`의 단일 연결 흐름과 아래 lifecycle owner를 연결한다. 보고된 전용 home/macOS 신원 비교 뒤 initialize→initialized→login/start→브라우저 완료→fresh account/read(chatgpt)→확인된 process cleanup 순서로 처리한다. 알려진 account/updated는 검증 후 성공 권위 없이 처리하며 알림/응답 합계 256개를 넘거나 순서/ID/종류가 다르면 실패한다. 기본 RPC 30초/browser 5분 한도, 100ms 이내 caller advance, 취소 후 늦은 성공 차단을 사용한다. CredentialsObserved는 계정 관측 및 세션 종료 결과일 뿐 실제 모델 준비 판정이 아니다. 위 제품 로그인 manager가 launcher/barrier·전용 환경·브라우저·IPC/UI를 연결한다.
- **로그인 수명 owner(macOS)**: `provider_login_process.rs`는 내부 LoginProfile capability에 따라 app-data의 create-new `codex-login.pending` 또는 `claude-login.pending`을 파일/디렉터리 sync한 후 launcher를 호출한다. Child/TreeTracker/I/O를 소유하고 누적 PID+시작 시각만 bounded append/sync한다. Claude의 비JSONL 출력은 별도 bounded chunk/EOF 경로를 쓰고 deadline·취소·원문 비저장 경계는 유지한다. 정상 finish만 추적 tree 소멸·root 회수·증거 저장·자기 marker 신원 확인 뒤 자기 잠금을 제거한다. 한 provider의 종료는 다른 provider의 잠금을 해제하지 않는다. Drop·불확실 종료·기존 파일은 재시작 fence로 보존하며 자동 삭제/PID 재신호를 하지 않는다. 앱 종료는 두 manager 모두 확인하고 provider별 blocker를 반환한다. 수동 복구 UI는 미구현이다.
- **수명 owner의 확인 한계**: marker 신원 대조와 POSIX 이름 기반 삭제는 한 원자 연산이 아니다. profile binding과 동일하게 검사 뒤 hostile same-UID 경로 교체까지 막는다고 주장하지 않는다. 정상 병렬 owner는 create-new로 배제한다. 종료 미확인 시 finish 재시도/Drop은 살아 있는 프로세스 정리를 계속 시도하되 최초 불확실 상태의 fence는 자동 해제하지 않는다.
- **세션 종료와 정리 상태 분리**: 사용자에게 반환하는 세션 종료는 cleanup 완료와 별도다. cleanup_required가 남은 closed session은 후속 advance/cancel 또는 retry_cleanup으로 underlying owner의 종료를 재시도한다. 이후 프로세스가 사라져도 최초 미확인 fence는 보존하며 인증 성공을 다시 반환하지 않는다.

- **Codex 로그인 교환**: `vendor_observation/codex_login.rs`는 browser 로그인 시작 응답의 request ID와 완료 알림의 login ID를 묶는다. 제한된 HTTPS 로그인 URL만 transient 값으로 반환하며 credentials나 모델 호출은 표현하지 않는다. 완료 알림 성공은 `VerificationRequired`이고 owner의 fresh account/read가 필요하다. 취소/EOF/timeout 뒤 성공을 채택하지 않으며 실제 프로세스 취소·종료와 URL 열기·계정 재확인은 위 제품 owner/main이 맡는다.

- **Codex 공개 본문 추출(미연결)**: `vendor_observation/codex_events.rs`는 [공식 item event](https://learn.chatgpt.com/docs/app-server#events)의 agentMessage/plan delta와 completed 본문만 명시적 thread/turn에 결속해 추출한다. completed는 누적 본문의 교체값이며 비공개 추론·도구 출력·미지원 알림은 공개하지 않고 별도 처리 필요로 반환한다. 중복 필드·모델 reroute·예산 초과는 실패 후 재사용을 막는다. 아직 runtime에 연결하지 않았으며 알림 순서/terminal/권한 검증이나 프로세스 종료 증명이 아니다.

- **Codex 응답 순서 검사(미연결)**: `codex_events/stream.rs`는 위 projection과 같은 예산 안에서 turn 시작→item 시작/종류가 일치하는 delta/완료→turn 완료 순서를 검사한다. 중복·소속/종류 변경·완료 후 frame·terminal 없는 EOF는 실패한다. 정상 completed는 미완료 item이 없어야 하며 interrupted/failed는 성공과 구분한다. private/미지원 알림은 owner의 추가 처리를 요구한다. 이 검사도 실제 transport·권한·자손 종료 증명을 대신하지 않는다.

- **Codex offline 설정 대조**: `vendor_observation/codex_preflight.rs`는 0.149.1 schema 기반 config/read·완전 model/list·read-only thread/start 응답을 명시적 tuple/정책/cwd와 비교한다. raw 입력을 반환하지 않고 실제 runtime/capability에는 연결하지 않는다. 응답 일치와 실제 파일 신원·설정 격리·권한 강제는 다른 증거다. 쓰기 정책·페이지 수집·Claude init은 아직 지원하지 않는다.

- **Codex 순차 preflight(제품 미연결)**: `codex_preflight/exchange.rs`는 위 검증기를 single-flight RPC에 결속해 앱 profile/macOS 초기화→설정→ChatGPT 계정→완전 모델 목록을 검사한다. `ObservationsMatched` 뒤에는 동일 연결의 명시적 read-only thread/start와 exact 응답 대조를 지원한다. 중간 실패는 영구 차단하며 raw 관측값을 저장하지 않는다. thread ID도 닫힘/실패 뒤 폐기한다. 응답 일치는 turn 전송·설정 격리·프로세스 종료 권위가 아니다.
- **설정 출처/훅 후보 검증**: 내부 preflight만 includeLayers=true로 명시 flags·정확한 profile config·빈 system layer를 각각 한 번 요구하고 미지/관리/프로젝트/비활성 layer를 거절한다. launcher의 features.hooks=false와 보고된 effective 값도 대조한다. 초기화 시 remoteControl/status/changed 하나만 opt-out하며 뜻밖의 frame을 무시하지 않는다. 빈 임시 profile의 실제 thread 생성/EOF 종료는 별도 JS 실측을 통과했지만 Rust 전체 계정/모델 preflight·신뢰 hook/MCP/plugin 격리·제품 활성화 증거는 아니다.
- **목표 입력 후보(제품 미연결)**: `codex_turn.rs`는 live ThreadMatched preflight를 소비해 고정 text turn 요청을 한 번만 구성한다. id=6 응답과 같은 thread/turn의 시작 알림을 양순서로 결속한 뒤 기존 TurnStream으로 넘긴다. 정책 override·임의 method는 없고 오류 뒤 재사용하지 않는다. 아직 process 전송/원장·실행 권한은 연결하지 않았다.

- **내부 preflight 세션(제품 미활성)**: `provider_preflight_session.rs`는 새로운 exchange의 typed frame만 기존 LoginProcess/LoginIo에 보내며 통신 전체 30초 예산과 취소·자손 관측·별도 종료 확인을 적용한다. 조회 전용 또는 동일 process의 read-only thread 진단 뒤 cleanup을 확인하고 exchange를 닫는다. thread/start 응답과 thread/started 알림을 순서와 무관하게 각각 한 번 받고 동일 ID를 대조한다. 알림 단독은 성공이 아니다. 불확실 종료는 기존 영속 잠금과 호출자의 owner를 유지한다. 가짜 CLI 파이프 검사 단계이며 실제 provider launcher/config 격리·제품 실행 gate는 별도로 남는다. thread 생성도 통합 기능을 초기화할 수 있어 실제 진단 전 격리가 필요하다.

- **실제 vendor 관측 파서**: `core/provider/vendor_observation.rs`는 Codex 0.149.1·Claude 2.1.269의 exact version과 모델 없는 인증 상태 응답만 읽는다. transient bounded input에서 계정/토큰을 제거한 분류만 반환하며 runtime·capability·영속 원장에는 아직 연결하지 않는다. config/model/handshake, 설정 격리와 launch authority는 별도 미구현이다. 이후 설치 CLI는 0.153.4로 변경됐으며 기존 exact version 허용을 자동 확대하지 않았다.
- **로그인 schema 드리프트 검사**: `node scripts/m5-codex-login-schema.mjs <기준 생성 폴더> <후보 생성 폴더>`는 초기화·로그인/취소·계정 조회/알림 타입과 재귀 type import의 원문 해시를 비교한다. 종료 0은 이 범위의 원문 동일, 2는 검토 필요, 1은 비교 불가이며 전체 프로토콜/실행 호환 판정이 아니다. 0.149.1→0.153.4 대조는 20파일 중 Bedrock 인증 추가 2파일 변경으로 검토 필요다. 생성 입력은 로컬 임시 증거이며 이 도구가 CLI 실행·인증 조회를 하지는 않는다.

- **작업대 배치 현행**: `src/m5/workbench-dock.tsx`는 파일·편집기·변경·팀 대화를 가로/세로 분할 또는 단일 탭 그룹으로 구성한다. 제목/tab drag/drop·방향에 맞는 이동 버튼과 경계선 pointer/키보드 조절을 지원하며 순서/비율/방향/탭 선택을 검증해 localStorage에 저장한다. 탭은 좌우/Home/End 이동과 roving focus를 지원하고 이동·방향/탭 전환 때 Monaco identity를 유지한다. 여러 split/tab 그룹의 중첩 도킹 및 전체 승인 시안 적용은 아직 남는다.
- **실제 CLI 최소 관측(2026-09-14)**: 빈 임시 폴더에서 Codex 0.149.1 app-server 초기화·config/account 조회·정상 종료와 Claude 2.1.269 인증 상태 조회를 확인했다. 모델 요청과 thread 생성은 없었다. 이 관측만으로 설정 격리·모델 가용성·실행 권위를 보장하지 않으며 제품 actual provider는 활성화하지 않았다.

- **부분 RSS 관측 경계**: `m5:measure:renderer --memory`는 실제 Electron subtree의 RSS를 250ms 간격으로 관측한다. `f4def74`는 idle 약 409.73 MiB 및 한 회차 표시 지연으로 FAIL했다. 테스트 fixture의 idle 선할당을 분리한 `dfe12bd`는 idle/peak/60초 잔류 356.70/761.73/110.125 MiB와 GUI 시간 기준을 통과했다. 합성 bridge를 쓰며 실제 sidecar/agent·이탈 자손은 포함하지 않아 전체 제품 메모리 증거가 아니다. 이전 실패는 삭제하지 않는다.

- **G6 성능 계측**: `scripts/m5-performance-metrics.mjs`는 명세 §10의 공급된 측정값을 판정하고 `scripts/m5-sidecar-startup-metrics.mts`는 현재 Cargo release executable을 생성된 빈 DB에서 migration 제외·warm-up 1회·10회 실행해 shell/storage ready를 구분한다. `d97ee56`에서 p95 3.863/52.596ms와 정상 종료·clean marker를 확인했다. `7832348`의 `scripts/m5-renderer-metrics.mjs`는 production renderer·실제 Monaco와 합성 bridge fixture로 대형 파일/diff·파일 목록의 10회 GUI 측정을 통과했다(파일 표시 p95 75.9ms, 입력 32.3ms, 변경 요약 38.7ms, 첫 diff 25.0ms). 파일 목록은 opaque cursor로 다음 500개를 탐색하고 오래된 응답이 새 폴더를 덮지 못한다. 패키징 interactive startup·실제 스트림 paint·전체 RSS·경합/종료 계측은 아직 남아 있으며 `m5:test:performance`는 미등록 상태다. 절차는 `procedures/m5-performance.md`다.

- **actual-agent 전체 수명 보존의 현재 경계**: G5D의 exact v10 admission, V9/V10 materializer, V10 bounded lossless writer, cooperative scheduler와 기존 D1 `storage.status` wire는 구현됐다. 최신 clean integration commit `6461e67617f4f5e55d7d646034dbb0971f1d845a`에서 macOS fixture 누적 `g5d-storage-lifetime` 16/16과 darwin-arm64 package·Chromium Monaco smoke를 통과했다(`.artifacts/m5-harness/g5d-storage-lifetime-2026-09-09T03-27-46-038Z.json`). 누적 하네스는 standalone wrapper의 범위를 보존하면서 locked workspace Rust를 한 번만 실행하고, 각 명령 전후 clean/same-HEAD 및 focused storage evidence의 동일 HEAD를 schema v2 report에서 재검증한다. 실측 시간은 47분 18초에서 16분 32초로 단축됐다. 이는 출시 승인이 아니며 Windows 안전 게이트, 실제 사용자 DB·강제 종료 검증, 실제 Claude Code/Codex provider·인증, G6 성능·서명·배포 판정이 남아 있다. 보존 기간과 physical cleanup/삭제 정책은 사용자 결정 전 계속 비활성이다.
- **코드 서명**: 미서명 배포 시 SmartScreen/Gatekeeper 경고. 베타는 미서명 + 통과 안내 이미지로 진행, 정식 릴리즈 전 비용(연 $250~400) 결정 — 기획 문서 참고.
- **실기기 출시 검증**: 깨끗한 macOS 계정과 Windows 기기에서 설치·로그인·오프라인·권한 오류·한글 경로 시나리오 확인.
- **업데이트 채널**: 현재 앱은 에이전트 최신 버전만 확인한다. Hello, Agent 자체 업데이트와 레시피 원격 갱신 방식은 첫 베타 이후 결정.
