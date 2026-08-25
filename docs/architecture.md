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

[M5 React renderer] → [typed preload] → [Electron main] ⇄ JSONL v1 ⇄ [Rust sidecar]
       app://m5           sandboxed        │                              ├─ SQLite v1
                                           └─ 앱 수명주기                 └─ hello-agent-core
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

## 9. M5 G1~G3 전환 골격·protocol·저장 수명주기

M5는 `Electron + React/TypeScript + Monaco + Rust sidecar` 방향의 기반, 언어 중립 protocol v1, 실제 SQLite sidecar와 최소 Electron 수명주기 연결까지 구현됐다. 기존 Tauri 위저드와 M4 파일럿은 계속 동작하며, Electron이 아직 기본 실행·배포 경로를 대체하지는 않는다.

- `crates/hello-agent-core`: Tauri/Electron을 모르는 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약. 기존 Tauri 모듈은 좁은 re-export shim으로 이 코어를 사용한다
- `crates/hello-agent-sidecar`: `--stdio`만 받는 실제 Rust 프로세스. 번들 SQLite의 단일 owner로서 모든 연결에서 foreign key를 켜고 확인한 뒤 transactional migration, integrity check, 불변 event ledger 재생과 projection 복구를 수행하고서야 `system.ready`를 보낸다. 현재 공개 method는 `system.status`와 `app.prepareQuit`뿐이다
- `crates/hello-agent-protocol` / `src/m5/contracts/protocol-v1.ts`: 같은 golden JSONL fixture를 읽는 Rust·TypeScript protocol v1 codec과 session validator. 첫 envelope `system.ready`, sidecar instance 고정, request/response 상관관계, stream sequence, 1 MiB control line과 256 KiB text chunk를 fail-closed로 검증한다
- `src/m5/contracts/{storage,lifecycle,bridge}-v1.ts`: SQLite schema/replay/recovery, 고정 실행 경로와 capability, renderer에 공개하는 system status·안전 종료 준비·sidecar event 구독 계약을 선언한다
- `tests/fixtures/m5/{protocol,storage,lifecycle}-v1`: JSONL session 위반, SQLite 7-table migration·operation idempotency·replay/recovery, 고정 capability·경로·startup/quit 실패를 Rust와 TypeScript가 함께 판정하는 계약 원본
- `electron/`: Forge `8.0.0-alpha.10` 기반 main/preload/Vite 패키징과 main-owned sidecar supervisor. production은 `process.resourcesPath/sidecar/hello-agent-sidecar`를 `--stdio`, `shell: false`로 한 번만 실행하며 3초 ready 제한, bounded framing, identity·response 검증, pending 요청 fail-closed, 자동 재시도 금지를 적용한다
- Electron main은 `userData/com.wonseokhan.helloagent/m5`를 생성·정규화해 sidecar cwd로 고정하고, sidecar는 그 안의 `hello-agent.sqlite3`만 연다. DB 경로나 실행 파일의 renderer/env override는 없다
- `electron/preload.ts`는 현재 main window의 top-level 허용 origin에서만 세 개의 고정 내부 channel을 versioned `M5BridgeV1`로 노출한다. 범용 IPC·Node·파일·셸 API는 없다
- 앱 종료는 single-flight coordinator가 `app.prepareQuit`의 안전 응답을 받은 뒤에만 정상 종료를 허용한다. ready 이후 sidecar 손상·종료는 앱을 열린 채 fail-closed하며, ready 전 시작 실패만 clean marker를 위조하지 않고 unclean 종료를 허용한다
- `src/m5/`: 기존 Tauri renderer와 분리된 최소 Electron renderer. 아직 프로젝트·채팅·작업 실행 화면이나 Monaco 편집 기능은 연결하지 않았다
- 패키지는 ASAR와 V1 Fuse 9개를 명시적으로 고정하고 release sidecar를 ASAR 밖 `Resources/sidecar`에 둔다. macOS arm64 smoke가 패키지의 실제 sidecar를 임시 cwd에서 실행해 ready → status → quit → exit 0과 같은 instance의 SQLite clean marker를 확인한다
- pnpm은 Forge의 패키징 요구 때문에 hoisted `node_modules`를 사용하지만, 전이 Git/tarball 차단 정책은 완화하지 않는다. Forge 7의 exotic dependency 문제를 피하려고 Forge 8 alpha와 registry-only `@electron/rebuild 4.2.0`을 exact pin했다

G3의 capability는 의도적으로 status와 quit에 한정된다. schema나 protocol 목록에 필드·method가 있어도 `project.*`, `chat.*`, `run.*`, 파일·diff·Monaco 기능이 구현됐다는 뜻은 아니며 G4 이후 게이트에서 추가한다. Forge 8은 prerelease이고 실제 package/runtime smoke는 현재 macOS arm64만 통과했다. Windows sidecar 경로·패키징·SQLite locking과 M4 동등 자손 회수 계약, packaged Electron 전체의 OS Quit E2E, 생성된 macOS `Info.plist`의 Electron 기본 ATS·미디어 권한 문자열은 후속 검증·hardening 대상이다.

## 10. 미해결 사항

- **코드 서명**: 미서명 배포 시 SmartScreen/Gatekeeper 경고. 베타는 미서명 + 통과 안내 이미지로 진행, 정식 릴리즈 전 비용(연 $250~400) 결정 — 기획 문서 참고.
- **실기기 출시 검증**: 깨끗한 macOS 계정과 Windows 기기에서 설치·로그인·오프라인·권한 오류·한글 경로 시나리오 확인.
- **업데이트 채널**: 현재 앱은 에이전트 최신 버전만 확인한다. Hello, Agent 자체 업데이트와 레시피 원격 갱신 방식은 첫 베타 이후 결정.
