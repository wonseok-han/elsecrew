# Hello, Agent 기술 설계

> 대상 독자: 이 프로젝트에 기여하는 개발자. 제품 배경·시장 조사는 기획 문서(Obsidian) 참고.
> 다이어그램: [시스템 구성](diagrams/architecture.excalidraw) · [위저드 플로우](diagrams/wizard-flow.excalidraw)

> 내부 실행 연결: `RunReservation::launch_proxied_codex_candidate`는 예약 역할/스냅샷에서만 실행 설정을 유도하며 원장에 process identity를 저장한 뒤 opaque proxy command의 gate를 해제한다. 해제 후 원장 저장 실패는 transport/broker를 중단하고 cleanup owner를 유지한다. 기존 offline candidate는 그대로 유지되며 어느 경로도 제품 실행 입장 권한을 제공하지 않는다.

> 2026-09-29 진단 상태: 내부 `inspect_proxied_preflight_candidate`의 실제 설치 CLI 격리 조회는 고정 OS CA 파일을 명시한 뒤 5 RPC·종료/cleanup을 통과했다. 모델 중계 경로만 root 소유·일반 파일·그룹/타인 쓰기 불가·1MiB 이하의 `/private/etc/ssl/cert.pem`을 확인하고 exact read 허용 및 `CODEX_CA_CERTIFICATE`로 전달한다. TLS 검증 비활성화나 키체인/Mach 접근 추가는 없다. 사용자 키체인의 별도 사설 CA 지원은 이 증거에 포함하지 않는다. WorkspaceAccess::ReadOnly는 workspace 쓰기 제한이지 profile 인증/캐시 불변 보장이 아니다. 실제 유료 turn·TLS/domain-fronting 경계·제품 활성화는 별도 미완료다.

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

- **진행/복구 상태 안내**: WorkStatusNotice는 기존 system.status의 blockerIds를 admission 후 및 작업 오류/채팅 상태 변화 때 읽고 명시 재확인을 제공한다. run과 lock은 진행 중에도 존재하므로 종료 실패로 단정하지 않는다. 채팅 생성 전 잠금도 표시하며 도움 요청 ID는20개·각160자로 제한하고 제어문자를 가시화한다. 실패한 재조회는 이전 정보임을 표시하고 늦은 응답을 버린다. 추가 실행·잠금 해제·파일 삭제 권한은 없으며 snapshot 준비 실패 전용 폐기 API는 아직 미구현이다.

- **Unix Git 실행 시간 제한**: 프로젝트 Git stream은 `project/git_process.rs`의 nonblocking stdin/stdout/stderr와 동일 명령별120초 deadline을 사용한다. stderr tail은8KiB로 유지하고 직접 child를 reap하기 전 stderr 완료를 기다린다. 오류/Drop은 reader를 취소하고 아직 reap하지 않은 leader의 그룹·직접 child에 종료를 시도하며 direct reap 대기는2초로 제한한다. 정상 EOF/status와 시간 초과를 구분한다. 이는 Unix 파이프/exit 대기의 상한이며 새 세션으로 이탈한 자손·파이프를 닫은 자손·커널 비중단 상태까지 전체 종료를 증명하는 계약은 아니다. Windows fallback은 종전 구현이며 해당 플랫폼 검증은 별도다.

- **프로젝트 화면 닫기**: toolbar의 나가기는 project.catalog releaseView로 해당 프로젝트의 standalone/workspace watcher만 해제한 뒤 renderer 선택을 비우고 영속 snapshot·채팅·후속 메시지 초안을 보존한다. 이는 manifest를 abandoned로 바꾸는 workspace.close/폐기와 다르다. 감시 해제 실패·dirty/저장·실행·채팅 전환·복구·snapshot 생성·열린 역할 설정창은 나가기를 막는다. 다시 프로젝트와 채팅을 선택해 기존 작업을 이어간다. 닫힌 화면으로 오는 이전 응답은 generation으로 무효화한다.

- **저장된 프로젝트 목록/재열기 계약**: `storage/catalog.rs`는 admission 이후 기존 projects table의 ID/경로만 keyset 페이지 조회한다. 50행·원시 UTF-8 합계 512KiB/경로 128KiB와 wire JSON 768KiB 상한이며 목록 조회는 파일 시스템 접근·활성화를 하지 않는다. `project.catalog` capability와 고정 `m5:project:catalog` IPC(`window.m5Projects.request`)의 list/reopen 요청은 양방향 검증한다. reopen은 renderer 경로가 아닌 저장 ID로 조회하고 ID hash·현재 canonical 경로·실제 Git root와 기존 recovery/active snapshot 경계를 검증한다. 작업대의 저장된 프로젝트 목록은 ID 순 페이지를 교체 표시하고 선택 시 기존 snapshot-before-tree 흐름을 사용한다. dirty/열린 채팅 및 조회 중 새 편집은 전환을 막는다. 목록 반환 자체는 파일 접근 권한이 아니다.

- **탐색과 패널 표시**: 폴더 경로 버튼으로 상위 폴더에 돌아갈 수 있다. 명시 파일/변경 열기는 controller의 panel 요청으로 해당 도킹 패널을 표시하고 기존 그룹의 탭을 활성화한다. 일반 상태 갱신은 사용자 배치를 바꾸지 않으며 같은 파일 재선택도 새 표시 요청으로 처리한다. Monaco 부모와 분할 구조는 유지한다.

- **파일 이름/경로 검색**: 파일 패널의 검색은 기존 작업 복사본 tree API를 순차 페이지 조회해 하위 폴더까지 찾는다. symlink·파일 내용은 읽지 않고 대소문자 무관/NFC 비교를 사용한다. 100페이지·100결과·1000폴더 또는 다음 페이지 시작 전 10초 한도에 도달하면 부분 결과임을 표시한다. 취소·프로젝트/채팅 변경·복구 잠금 뒤 늦은 결과는 폐기하며 기존 폴더 탐색 상태는 유지한다. 결과 열기는 기존 검증된 파일 읽기 경로를 사용한다.

- **연결 잠금 복구**: 로그인 UI의 명시적 `provider.login`/`recover`는 종료 증거가 있는 locked-v2 pending만 처리한다. 생성 owner와 복구자는 같은 파일의 exclusive flock을 사용한다. 복구자는 고정 이름·nofollow·16KiB·canonical 기록·소유권/권한/단일 링크·inode·모든 기록 신원의 Gone·프로필/부모 capability를 검증한 뒤 해제한다. 진행 중 worker, legacy/v1/불완전 기록, native run의 terminal 보류는 거절한다. 인증 정보 삭제나 재로그인을 하지 않으며 증거 없는 기존 실제 pending은 이 경로로 복구되지 않는다. UI는 잠금 중 start/check를 비활성화하고 복구 성공을 인증 성공으로 표시하지 않는다. 복구 후 사용자가 `연결 상태 다시 확인`으로 기존 자격 증명을 검사할 수 있다. Electron Chromium에서 가짜 bridge의 실패/성공·처리 중 버튼·명시적 재확인 클릭을 검증했으며 실제 프로필 복구 E2E는 미완료다.

후속 메시지 저장 통신은 `chat.appendMessage`를 지원한다. 현행 sidecar는 기존 exact G5D 제품/agent-fixture capability 배열의 마지막에 이 capability만 추가하며, Electron client는 해당 확장을 협상한 경우에만 요청한다. 기존 baseline 상수와 검증기는 그대로이고 current 상태 검증기는 정확한 baseline+선택적 suffix만 허용한다. V7 preload의 고정 `appendChatMessage` 채널과 팀 대화 입력창이 연결됐고 V4~V6는 유지된다. 요청의 전체 채팅/작업 공간 소속·client message ID·예상 durable 순서·본문을 저장 결과와 대조하며, 동일 요청 재시도는 같은 메시지를 반환하고 이벤트를 중복 발행하지 않는다. 초안과 불확실한 요청은 controller 수명 내에서 채팅별로 보존하며 앱 재시작까지의 초안 영속 저장은 아직 없다. 명시적 무기록 순서 충돌만 기록을 새로 읽고 같은 ID의 예상 순서를 갱신한다. 저장된 메시지는 다음 실제 실행의 inputMessageIds에 포함되지만, 실제 provider 활성화는 별도 미완료이고 메시지 저장 자체는 실행/실행 중 stdin 전달을 의미하지 않는다.

Electron main은 현재 main window의 main frame과 정확한 허용 origin만 IPC에 접근시키고 popup·webview·외부 navigation을 차단한다. `app://m5` 자산은 credential/port/query/fragment 없는 authority와 renderer 내부의 기존 regular file만 허용하며 잘못된 escape·경로 이탈·Windows 절대경로도 거부한다. `02dbaeb`의 VM 실행 회귀는 실제 main의 등록 callback을 호출해 이 경계를 검증한다. 이는 Electron/OS 자체의 sandbox·CSP·fuse 강제나 실제 패키지/Windows 검증을 대신하지 않는다.

M5는 `Electron + React/TypeScript + Monaco + Rust sidecar` 골격, 언어 중립 protocol v1, SQLite 수명주기, macOS standalone snapshot 작업대, 기존 G5 fixture 실행·복구와 테스트 전용 G5A~G5C fake actual-agent의 normalized public/terminal event·자원 예산·provider 발견/실행 전 검증·재시작 안전 복구 수직 흐름까지 구현됐다. G5D는 기존 v1 원장을 보존하면서 제품 admission의 exact storage schema v10 전환, append-only source index와 bounded lifetime materialization, V9 resume witness, V10 lossless compaction writer와 ready-only cooperative scheduler까지 연결했다. 기존 Tauri 위저드와 M4 파일럿은 계속 동작하며 Electron은 아직 기본 실행·배포 경로를 대체하지 않는다.

- `crates/hello-agent-core`: Tauri/Electron을 모르는 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약. 기존 Tauri 모듈은 좁은 re-export shim으로 이 코어를 사용한다
- **stdio 진행 공정성**: 입력이 없으면 100ms timeout에서 runtime/watch를 진행하고, 요청이 계속 오더라도 마지막 poll 이후 100ms 이상이면 응답·직접 이벤트 뒤에 한 번 진행한다. accepted/응답 선행 순서를 유지하며 요청 경로에서는 safeToQuit 이후 새 poll을 하지 않는다. 요청 handler·cleanup 자체의 비차단 보장은 별도다.
- **취소 완료 지연 응답**: `run.cancelAgent`는 정확한 6필드 신원을 검증한 뒤 요청 ID를 run별 한 개만 보관하고 즉시 성공 응답을 만들지 않는다. runtime poll이 누적 신원을 저장하며 종료를 단계 진행하고 terminal transaction이 끝난 뒤 기존 runCancelResult를 한 번 보낸다. 해당 poll의 terminal 이벤트보다 응답을 먼저 전송한다. 상태 조회는 대기 중에도 처리하며, 중복/다른 대상 취소는 거절한다. 보류 응답이 있으면 clean shutdown을 표시하지 않는다. 현재 실행 검증 대상은 actual-agent fixture이며 native OwnedTextRun 등록과 실제 공급자 admission을 대신하지 않는다. 기존 정상 종료/오류 fallback 및 별도 prepareQuit 취소의 동기 경로는 남는다.
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

- **승인 직전 경계 후보**: 예약 owner의 `verify_approval_boundary`가 취소·현재 불변 role/input·writer 소유권·snapshot/파일 분리·exact 격리 launch scope·앱 profile·프로세스 관측 원장을 함께 검사한다. 파일 분리 스캔을 Git 검증보다 먼저 수행한다. TextSession의 AcceptOnce 제안은 이 callback을 거쳐야 하며 이후 전송마다 재검증하도록 묶었다. 일반 text-only 경로는 거절한다. Core의 실제 AcceptOnce 직렬화는 아직 차단되어 있어 이 전제 연결을 허용 기능 완료로 보지 않는다.

- **격리 실행 출처(승인 권위 아님)**: fixed `ModelProxyCommand` builder만 생성하는 비직렬화 `RuntimeScope`가 동일 명령의 identity-barrier launch 성공 후 process owner로 이동한다. exact 작업 폴더/ReadOnly·Write를 보존하며 일반 proxy/raw launch는 이 출처 정보를 만들지 않는다. failed/finished/cleanup/no-I/O/no-child 상태에서는 일치하지 않는다. 실제 프로세스 생존·현재 역할/작업 폴더/잠금·사용자 일회 결정 검증을 대체하지 않으며 AcceptOnce와 제품 활성화는 아직 차단된다.

- **모델 중계(내부 후보)**: `provider_model_proxy`의 Broker가 실행별 Basic credential·정확한 HTTPS authority/Host·8KiB/5초 단회 CONNECT 검사와 연결 작업·Relay를 묶는다. 동시8연결·poll당4accept·서비스 총600초/128MiB payload 예산을 적용한다. DNS는 고정 목적지만 한 번 조회하고 최대16개 결과 전체의443/public IP 여부를 검사한 뒤 숫자 주소에 연결한다. 전역8개 작업자 슬롯·10초 연결 예산·시도당최대2초이며 OS DNS가 멈추면 취소 후에도 실제 작업자 종료까지 슬롯을 보유한다. 이는 DNS 스레드를 강제 종료한다는 보장이 아니다. Relay는 방향별8KiB 버퍼, 200응답 배출 후 전달, EOF half-close와 취소/오류/Drop 종료를 유지한다. Broker stop은 연결을 닫되 listener는 drop까지 보유한다. 검사 증거는 합성 DNS 목록·가짜 지연 작업자·로컬 소켓 통합뿐이며 실제 DNS/목적지 연결은 실행하지 않았다. LoginProcess 수명에는 내부 연결했지만 native CLI 호환성과 제품 admission은 아직 없다. 자손도 credential을 사용할 수 있으며 TLS peer/SNI/HTTP 경로를 검사하지 않아 공유 IP의 domain fronting까지 차단한다고 주장하지 않는다.

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

- **Codex offline 설정 대조**: `vendor_observation/codex_preflight.rs`는 config/read·완전 model/list·thread/start 응답을 명시적 tuple/정책/cwd와 비교한다. 초기 0.149.1 schema 후보에서 읽기/쓰기 역할 대조로 확장했으며 실제 runtime/capability에는 연결하지 않는다. 보고된 관리/알 수 없는 계층은 거절하지만 upstream에서 누락된 계층은 이 검사만으로 감지할 수 없다. [공식 API](https://learn.chatgpt.com/docs/app-server#api-overview)의 별도 `configRequirements/read` 및 정책 가시성 검증은 아직 연결되지 않았다. raw 입력을 반환하지 않으며 응답 일치와 실제 파일 신원·설정 격리·권한 강제는 다른 증거다. 모델 페이지 수집·Claude init은 아직 지원하지 않는다.

- **Codex 순차 preflight(제품 미연결)**: `codex_preflight/exchange.rs`는 위 검증기를 single-flight RPC에 결속해 앱 profile/macOS 초기화→설정→ChatGPT 계정→완전 모델 목록을 검사한다. `ObservationsMatched` 뒤에는 동일 연결의 명시적 read-only thread/start와 exact 응답 대조를 지원한다. 중간 실패는 영구 차단하며 raw 관측값을 저장하지 않는다. thread ID도 닫힘/실패 뒤 폐기한다. 응답 일치는 turn 전송·설정 격리·프로세스 종료 권위가 아니다.
- **설정 출처/훅 후보 검증**: 내부 preflight만 includeLayers=true로 명시 flags·정확한 profile config·빈 system layer를 각각 한 번 요구하고 미지/관리/프로젝트/비활성 layer를 거절한다. launcher의 features.hooks=false와 보고된 effective 값도 대조한다. 초기화 시 remoteControl/status/changed 하나만 opt-out하며 뜻밖의 frame을 무시하지 않는다. 빈 임시 profile의 실제 thread 생성/EOF 종료는 별도 JS 실측을 통과했지만 Rust 전체 계정/모델 preflight·신뢰 hook/MCP/plugin 격리·제품 활성화 증거는 아니다.
- **목표 입력 후보(제품 미연결)**: `codex_turn.rs`는 live ThreadMatched preflight를 소비해 고정 text turn 요청을 한 번만 구성한다. id=6 응답과 같은 thread/turn의 시작 알림을 양순서로 결속한 뒤 기존 TurnStream으로 넘긴다. 정책 override·임의 method는 없고 오류 뒤 재사용하지 않는다. 아래 내부 세션만 사용하며 제품 원장·실행 권한은 연결하지 않았다.
- **승인 요청 관측(제품 미연결)**: `codex_approval.rs`는 command/writeStdin/network/fileChange 요청을 예상 thread/turn과 대조해 일시적 문맥으로 반환한다. 알림 위장·중복·미지 권한·정책 amendment를 거절한다. TurnStream.pending_approval은 진행 중 tool item ID/종류와 command/cwd를 결속하고 기존 예산에 포함하며, 처리된 요청 ID 재사용/미해소 item 완료를 거절한다. serverRequest/resolved는 같은 thread의 알려진 pending 요청 한 번만 해소한다. 이는 명령/파일 권한·성공 증거가 아니고 사용자 UI·실행 전송은 아직 없다.
- **native 승인 대기와 거절 의도 원장(제품 미활성)**: live stream의 미응답 요청을 전체 실행 신원·dispatcher stream·실제 LoginProcess UUID·thread/turn/request/item/시각·내용 digest에 결속한 일회성 ticket으로 보관한다. 내부 조회/거절은 exact 실행 신원과 예약 입력을 검사하며 명시 거절 접수 뒤 다음 callback에서 원장 저장 후에만 Decline을 보낸다. 자동 기본 결정은 없고 취소/종료/서버 해제/연결 변경 뒤 ticket을 제거하며 재시작 복원은 없다. 기존 run projection에는 최대128개 opaque ID/digest/decline/intentRecorded만 원자 저장한다. 저장 실패는 무전송·owner 정리로 가고 terminal/recovery는 원장 기록을 보존한다. 의도는 전달/수행 성공이 아니며 raw 명령을 저장하지 않는다. `run.approval`의 list/decline은 전체 실행 신원과 opaque ID를 검사하며 queued 접수 결과를 반환한다. Electron 현재 main frame 및 양방향 TS 검증을 거쳐 채팅 카드에 대기/거절/기존 취소를 연결한다. capability는 chat.appendMessage/provider.login 뒤 exact suffix로 협상한다. pending은 미접수 live 요청의 `{approvalId,kind}` 요약이며 command/writeStdin/network/fileChange 고정 유형만 표시한다. 기본 list/decline에는 raw 명령/경로/host/reason이 없다. 사용자가 상세를 펼칠 때만 inspect가 같은 live ticket에 결속된 bounded command/cwd/reason/grantRoot/host/protocol을 일시 반환한다. 상세는 채팅/실행 원장에 저장하지 않고 React 평문으로 표시하며 C0/C1·Unicode Cf 문자는 이스케이프한다. 닫기·만료·오류·실행 신원 변경 시 상세 상태를 비운다(보안 메모리 소거 보장은 아님). AcceptOnce 권위·실제 provider 활성화는 아직 없다.
- **승인 응답 준비·내부 일회 허용 후보**: core의 prepare_approval_reply는 decline/cancel만 구성하며 AcceptOnce는 계속 거절한다. reserve_approval_response는 live 요청의 단회 순서만 예약하고 응답 bytes를 만들지 않는다. sidecar 내부 accept_native_approval_once는 같은 ticket의 명시적 상세 조회·현재 예약/격리 경계를 요구한다. 접수 후 원자 intentRecorded(acceptOnce 또는 decline) 저장, 전송 직전과 부분 전송마다 예약/역할/작업 사본/프로세스/취소 검사 후 private transport만 opaque accept frame을 만든다. 의도 저장은 전달/실행 성공이 아니고 serverRequest/resolved·item 완료를 별도 관측한다. 공개 IPC/UI에는 accept가 없으며 제품 native start도 blocked다. 기존 PROFILE 전체 읽기/쓰기 허용은 vendor 자손에게도 적용되므로 인증/profile을 도구에서 격리했다고 주장할 수 없고 실제 제품 허용 활성화 전에 해결해야 한다. 아래 이전 handler 후보의 accept 차단 설명은 일반 core/text 경로에 해당하며 이 내부 후보를 대체하지 않는다.
- **내부 text 실행 세션(제품 미활성)**: `run_text_and_close`는 임의 cwd 대신 `TextWorkspace`를 받는다. 기존 ProjectManager의 durable chat·manifest·snapshot marker·저장소·poison 상태 검증으로 경로를 얻고, process claim 전·thread 생성 전·목표 전송 전에 재검사한다. 이는 시점별 snapshot 신원 검사이지 독점 실행 잠금이나 OS 쓰기 격리가 아니다. 미사용 process claim→동일 연결 preflight/thread→typed turn 전송→공개 출력 callback→종료 관측을 연결하며 callback은 잠정 출력이다. userMessage(text만)·reasoning/추론 delta는 신원·형식·순서·예산을 검증한 뒤 Private 결과로 버리고 공개 출력에 섞지 않는다. 미지원 도구/기타 이벤트·sink 오류·취소는 종료 절차를 거치며 불확실 종료는 owner/잠금을 보존한다. 연결/thread 준비 예산은 30초이며 turn 요청 queue 생성부터 작업 전체 10분을 별도 계상한다. 도구·승인 대기와 partial write도 포함하고 활동으로 시간을 갱신하지 않는다. 원장·소유권·격리·도구 처리의 후속 구현은 아래 항목을 따르며 제품 caller/실제 공급자 활성화는 아직 미완료다.

- **내부 출력 저장 소유권(제품 미활성)**: `TextWorkspace`가 단일 mutable Storage 참조를 보유하고 검증 시 잠깐 읽기 빌림, 공개 출력 callback 시 쓰기 빌림을 순차 제공한다. 별도 DB writer나 병렬 쓰기를 추가하지 않는다. 실제 임시 snapshot/SQLite와 가짜 vendor pipe로 정상 출력 저장·sink 실패/cleanup을 확인했지만 synthetic sink 검증이지 actual-agent 공개 원장/실행 예약/복구 통합 완료는 아니다.
- **실행 준비 원장 공통 경로**: `agent_runtime/preparation.rs`가 저장된 chat/role·message ID·snapshot을 해석하고 예약 직전 재확인한다. 기존 fixture actual-agent 시작 경로가 이를 사용해 preparing run·recovery ID·쓰기 역할의 writer lock을 원자 저장한다. 중복 실행·변경된 snapshot/입력·다른 capability는 예약하지 않는다. 이는 단일 runtime writer 전제이며 자체 provider admission/OS sandbox 권위가 아니다. 아래 내부 Codex text 후보의 barrier·공개 원장·terminal과 연결했지만 실제 공급자 실행 입장/제품 caller는 미완료다.
- **원장 신원 barrier 후보**: `LoginProcess::launch_with_identity_barrier`가 기존 관측·profile fence 기록 뒤 trusted release callback에 추적 신원을 전달한다. `RunReservation::record_pre_release`는 preparing run·snapshot·writer 일치를 확인하고 그 신원을 원자 저장한다. 내부 barrier는 gate 시점의 관측일 뿐 vendor handshake/실행 권한 증거가 아니다. 가짜 gate와 임시 SQLite에서 저장 성공 후 release 및 저장 실패 시 미실행·기존 복구 경로의 신원 보존을 검증했다. 실행 중 신원 갱신·release 기록·공개 원장·terminal은 아래 내부 후보에 연결했으며 제품 launcher·running attestation은 후속이다.
- **예약 소유자의 gate 연결 후보**: `RunReservation::launch_gated`가 신원 저장→trusted gate release→release 기록 저장을 묶는다. 마지막 저장 실패는 이미 실행됐을 가능성을 유지하며 LoginProcess의 추가 통신을 차단하고 owner를 호출자에게 보존한다. 확인 가능한 프로세스 정리와 별개로 run은 preparing/복구 대상으로 남는다. release 기록 성공도 모델 작업 성공이나 running attestation이 아니다. 통합 가짜 gate 시험의 정상/전 저장 실패/후 저장 실패를 통과했으며 제품 caller는 여전히 없다.
- **실행 중 신원 갱신 후보**: `RunReservation::observe_process`는 같은 owner의 누적 추적 집합이 기존 원장 신원을 모두 포함하는지 대조하고 기존 `persist_tracker_snapshot`으로 갱신한다. 관측 실패여도 알려진 신원 저장을 시도하되 성공으로 바꾸지 않는다. 관측/저장 오류는 통신 차단과 명시적 cleanup 대상으로 처리한다. toy 자식 프로세스 추가·SQLite 갱신 실패를 통과했으며 예약 소유자의 내부 text 통신 루프에서도 호출한다.
- **text 통신 checkpoint**: 내부 text 세션은 claim 이후 preflight 송신 전/수신 후, thread/turn 전, turn 응답·출력 수신 후와 입출력 대기 중에 process callback을 제공한다. 대기 중에는 기존 프로세스 관측이 갱신되거나 실패한 때만 원장 callback을 호출한다. 같은 mutable Storage로 기록하며 callback 오류는 다음 통신/공개 출력 없이 cleanup으로 간다. 순수 workspace 형식 거절은 계속 claim 전이다. 임시 SQLite synthetic sink·지연 응답 중 child 관측/실패로 검증했다. 예약 owner callback은 아래 내부 진입점에 연결했으며 공개 actual event·terminal 연결은 미완료다.
- **예약된 목표·역할로 text 실행(제품 미활성)**: `RunReservation::run_read_only_text`는 별도 목표/모델 인자를 받지 않고 예약된 메시지·역할과 같은 프로세스 소유자의 Codex HOME으로 통신을 구성한다. 실행 전과 통신 checkpoint에서 저장된 run·역할·메시지·snapshot 결속을 확인하고 누적 프로세스 신원을 저장한다. Codex/readOnly 외 역할은 거부하며 workspaceWrite를 조용히 낮추지 않는다. 같은 메시지 ID의 반복 projection은 입력에 한 번만 포함하고 내용 충돌은 거부한다. 합성 capability·toy pipe에서 실제 전송된 목표/모델과 설정 변경 차단을 확인하는 내부 후보이며 provider admission·공개 이벤트 원장·terminal/복구 완료·제품 활성화를 대신하지 않는다.
- **text 공개 원장 연결(제품 미활성)**: 예약 후보와 기존 fixture 실행기가 `persist_public_event_record`를 공유해 run 결속·개별/누적 출력 예산·chat 순번 및 run/chat 원자 저장을 적용한다. delta는 잠정 callback으로만 보내고 completed의 전체 텍스트로 대체한다. 계획과 중간 답변은 status, 마지막 답변은 정상 turn 완료와 프로세스 cleanup 확인 뒤 한 번만 final로 저장한다. 중단/실패 outcome의 텍스트는 status이며 cleanup 불확실 또는 sink/DB 오류는 final을 남기지 않는다. 원장 저장만으로 실행 성공을 확정하지 않으며 아래 종료 증거/terminal 쓰기를 별도로 요구한다. 제품 이벤트 송신은 후속이다.
- **예약 실행 종료/복구(제품 미활성)**: `LoginProcess`가 explicit finish의 누적 PID/start 신원과 종료 확인 증거를 제공한다. 모든 추적 프로세스 종료·root 회수·profile/fence 저장을 통과해야 confirmed이며 caller가 이를 만들 수 없다. text 예약은 자기 run의 기존 신원이 모두 포함된 증거만 받아 run/chat terminal과 writer 잠금 해제/poison을 원자 기록한다. 정상·중단·실패는 succeeded/canceled/failed, 종료 불확실은 같은 예약 recovery ID의 unresolved 상태와 workspace recovery 표식을 유지한다. terminal DB 실패는 preparing을 성공으로 해제하지 않고 다음 재시작 복구로 넘긴다. 실제 DB 재오픈 뒤 정상 terminal 불변·불확실 복구 유지·terminal 저장 실패 시 기존 출력/복구 ID 보존을 toy peer로 검증한다. 이는 native provider admission·쓰기 격리·제품 실행 UI의 완료가 아니다.
- **실행 종료 시 profile fence 순서**: 일반 로그인은 기존처럼 finish에서 해제한다. 예약 text 실행은 통신 claim 전에 지연 해제를 설정하고, finish에서는 종료 확인과 fence 신원 확인만 수행한다. run/chat terminal 쓰기가 성공하고 closure가 confirmed인 경우에만 명시적으로 fence를 해제한다. terminal 저장 실패·불확실 종료·drop·일반 finish 재호출은 fence를 해제하지 않으며 DB 재오픈도 자동 삭제하지 않는다. 종료 기록 뒤 fence 해제 실패/충돌 시에도 자동 삭제나 재실행으로 우회하지 않는다.
- **예약 기반 Codex launch 후보(제품 미활성)**: `RunReservation::launch_codex_candidate`는 원장의 Codex 역할에서 model/effort/permission을 해석하고 기존 discovery 명령과 inert gate를 사용한다. profile 소유자가 HOME을 제공하며 별도 prompt/model/cwd 명령 인자는 받지 않는다. 실행 전 취소·CLI/helper pin 불일치는 spawn 전에 거절하고, 프로세스 신원을 원장에 저장한 뒤 payload release 직전에 두 pin과 취소를 재검사한다. 실패한 owner의 명시 cleanup 책임과 preparing 원장은 보존한다. 가짜 pinned CLI로 같은 원장의 입력/출력/terminal까지 연결했으나 pin은 서명/version·설정 격리·native capability 입장 증명을 대신하지 않으며 제품 caller는 여전히 없다.
- **vendor gate의 상속 fd 차단**: 기존 고정 `/bin/sh` gate가 release를 읽기 전에 `/dev/fd`의 숫자 fd를 순회해 0/1/2 외 핸들을 닫는다. 부모 핸들/flags는 변경하지 않고 Rust spawn error pipe는 셸 exec에서 이미 닫힌 뒤다. 순회/닫기 실패는 종료 125로 payload를 실행하지 않는다. protocol stdio 외 descriptor를 넘기는 용도로 이 gate를 사용하지 않는다. 이는 fd 상속 경계이며 파일 hardlink·IPC·전체 sandbox 보장은 아니다.
- **예약 launch의 snapshot 파일 입장 검사**: `project/runtime_isolation.rs`가 capability directory에서 숨김/`.git` 포함 모든 항목을 검사한다. nlink=1 regular file과 실제 directory만 허용하며 symlink·특수 파일 및 128단계/200,000항목 초과는 거절한다. native Codex spawn 전과 예약 gate 신원 저장 전에 수행하며 output polling에는 추가하지 않는다. 새 검사 함수 내부의 Git 신원 대조보다 파일 검사가 먼저다. 이는 검사 시점 snapshot의 공유 inode 거절이며 적대적 same-UID 교체에 대한 원자 보호나 profile/tmp를 포함한 전체 sandbox 정책은 아니다.
- **모델 중계 endpoint 후보(미활성)**: 기존 offline 정책과 별도로 IPv4 loopback 포트 하나만 허용하는 tcp4 정책을 적용한다. opaque ModelProxyCommand가 Broker를 기존 LoginProcess에 옮기고 인증 proxy URL은 HTTP(S)/ALL_PROXY 대소문자 환경에만 전달하며 NO_PROXY는 비운다. owner tick은 중계를 진행하고 통신 무효화·종료 시작·Drop 때 stop한다. 종료 확인까지 listener를 소유하고 불확실 Drop은 연결/작업/인증 객체를 정리한 뒤 listener descriptor만 sidecar 종료까지 보존한다(가용성 비용, crash-survival 보장 없음). 중계 만료는 통신 실패이지 프로세스 종료 증거가 아니므로 별도 cleanup 전 fence를 해제하지 않는다. 실제 native CLI가 이 환경을 따르는지와 제품 admission은 아직 미검증이다. 자손도 proxy credential을 상속할 수 있고 TLS/domain-fronting 한계는 위 모델 중계 항목과 같다.
- **옛 helper의 명시 보존 복구(macOS)**: `provider_profile/stale_helper.rs`는 고정 옛 Codex 경로를 가리키는 단일 helper 링크만 들어 있는 exact 임시 디렉터리를 다룬다. 기존 로그인 예약, nofollow capability·dev/inode·대상 부재 재검증 뒤 profile 밖 app-data UUID 이름으로 exclusive rename하고 양쪽 디렉터리를 fsync한다. 기존 인증·vendor를 호출하지 않고 scanner도 완화하지 않는다. 제품 자동 호출/IPC는 없으며 로컬 exact artifact의 opt-in 복구와 재검사는 통과했다. same-UID 경합의 원자 방어나 일반 캐시 청소 기능은 아니다.
- **예약 Codex profile 파일 입장 검사**: 같은 scanner를 앱 전용 HOME 전체(`skills`·`plugins`·`tmp`·`.tmp`의 숨김/중첩 항목 포함)에 재사용한다. spawn 전과 CLI/helper 재검증 후 payload release 직전에 검사하며 인증 파일 내용은 읽지 않는다. 로그인/상태 조회의 uid·쓰기 권한·타입 검사와 실행 후보의 공유 파일·링크·특수 파일 거절을 유지한다. 안전한 plugins 캐시 폴더가 있다는 이유만으로 거절하지는 않으며, 그 내용도 scanner 검사에서 제외하지 않는다. 대신 native OS 정책은 PROFILE/plugins의 읽기·실행에 필요한 읽기·쓰기를 차단한다. 이는 검사 시점 파일 구조와 해당 정책의 결합이며 same-UID 경합 방어·원격 plugin/관리 설정 전체 격리 증명을 대신하지 않는다.
- **플러그인/앱 기능 고정**: native/discovery argv에 features.apps=false·features.plugins=false를 hooks=false와 함께 고정하고, preflight는 effective 설정과 sessionFlags 양쪽에서 명시적 false를 요구한다. 누락/null/true/중복 키는 거절한다. 사용자 config를 수정하지 않으며 플러그인 캐시 삭제/이동도 없다. [공식 관리 설정 문서](https://learn.chatgpt.com/docs/enterprise/managed-configuration#configure-network-access-requirements)는 지원 client에서 이 기능 차단을 설명하지만 명령용 네트워크 프록시가 Codex 서비스/MCP/앱 트래픽을 모두 통제하지는 않는다고 구분한다. 따라서 이 두 flag를 모델 네트워크/관리 정책 격리의 대체 증거로 사용하지 않는다.
- **예약 후보의 오프라인 OS 격리**: native launch는 이제 fd를 닫고 신원을 기록한 기존 gate에서 `/usr/bin/sandbox-exec`를 exec한 뒤 payload를 실행한다. 기본 거부 정책은 시스템/고정 CLI·helper/작업 사본/profile 읽기와 profile 쓰기를 제한적으로 허용하고 profile config 쓰기는 열지 않고 작업 사본 쓰기는 저장된 역할이 workspaceWrite일 때만 허용한다. 경로는 Scheme 삽입이 아니라 개별 `-D` argv이며 profile/workspace 중첩·비정규 경로를 거절한다. 임시 toy CLI의 실제 OS 실행→공개 답변 저장→종료/DB 재오픈을 opt-in 통합 테스트로 확인했다. 네트워크 권한이 없는 오프라인 후보이며 실제 공급자 호환성·모델 통신·도구/승인·제품 capability는 미완료다. 실패 시 무격리 fallback하지 않으며 기존 로그인/discovery 진단은 이 후보와 별도다.
- **쓰기 정책 기제 후보(저장된 역할 연결, 제품 미활성)**: 같은 builder의 typed `WorkspaceAccess::Write`는 작업 사본 일반 파일 쓰기만 추가하고 `.git`·`.agents`·`.codex`와 profile config의 쓰기를 거절한다. CLI/helper가 쓰기 가능한 HOME 또는 작업 사본 안에 있으면 명령 생성도 거절한다. 임시 셸 시험에서 일반/자식 쓰기 성공과 원본·보호 경로 변경/이동·hardlink/symlink 우회 거절을 확인했다. native 예약의 저장된 permission이 argv·OS 정책·preflight 모드를 함께 결정한다. 실제 공급자 호환성·모델 네트워크·도구/승인 연결은 별도다.
- **쓰기 preflight wire 후보**: core의 `ExpectedPermission`을 생성 시 고정하면 설정 유효값·session flag·thread 요청/응답까지 같은 모드를 대조한다. WorkspaceWrite는 추가 writableRoots 없음·networkAccess=false·TMPDIR 및 /tmp 제외를 요구하고, 두 thread 식별 응답이 맞기 전에는 turn을 준비하지 않는다. 기존 `new`와 `start_read_only_thread`는 읽기 전용 계약을 유지하며 write 후보를 읽기 전용으로 낮추지 않는다. sidecar의 `run_text_candidate`는 저장된 역할에서 이 모드를 생성한다. `run_read_only_text`는 기존 읽기 전용 계약을 유지한다. 합성 wire/오프라인 fake CLI 후보이며 native 응답 호환성·도구 실행 연결은 아직 남는다.
- **도구/승인 stream 분류 후보**: TurnStream은 commandExecution/fileChange의 시작·출력·완료 상태를 Tool metadata로 분류하며 raw stdout/diff는 포함하지 않는다. 명령 완료는 시작 당시 command/cwd와 대조하고 잘못된 상태·범위·늦은 출력은 실패한다. id가 있는 서버 요청은 기존 pending_approval 검증과 동일 turn 예산을 거쳐 Approval로 분류하며 중복·unknown 요청은 거절한다. 이는 도구 권한/파일 변경 내용 검증/사용자 결정이 아니다. sidecar의 기존 text 경로는 Tool/Approval도 여전히 거절하며 handler·UI 연결과 실제 공급자 검증은 남는다.
- **승인 대기/전송 내부 handler 후보**: 같은 claim된 LoginProcess가 typed ApprovalReply를 기존 bounded/nonblocking I/O로 보내고 대기 중에도 process 관측과 입력 poll을 유지한다. TurnStream의 미응답 live 요청만 즉시 반환하는 owner callback에 질의하며 기본 결정은 없다. 서버가 먼저 해제한 요청은 응답 후보에서 제거한다. 결정 직전 workspace·관측·취소를 재검증하고 단회 prepare/send 후 serverRequest/resolved는 별도로 기다린다. send/callback/취소 오류는 같은 owner의 cleanup으로 간다. 기존 text wrapper는 Tool/Approval을 계속 거절하며 새 handler는 제품 caller가 없다. accept는 계속 차단되고 전체 30초 후보 예산·UI/원장/실제 provider 입장 미완료를 유지한다.
- **예약 owner의 상호작용 handler 후보**: RunReservation::run_candidate_with_handler는 기존 권한/입력 검증·공개 원장·terminal/fence를 공유하며 Tool/PollApproval만 입력 재검증 뒤 trusted owner에게 전달한다. process 관측은 예약 원장이 직접 소유하고 workspace 재검증은 transport에서 직접 수행한다. callback이 역할을 변경해도 응답 직전 checkpoint가 이를 거절한다. 기존 readOnly/text wrapper는 상호작용을 계속 거절한다. 실제 사용자 결정/공개 승인 IPC/승인 상태 저장/제품 입장은 별도이며 callback 자체가 실행 권한은 아니다.
- **단계별 text 세션(제품 미연결)**: 기존 동기식 driver는 private `TextSession`을 반복 진행한다. 세션은 preflight/thread/turn handshake·stream·미응답 승인을 보존하고 호출마다 수신 frame 하나 이하를 처리하며 응답 대기 자체는 반환한다. 오류/완료 뒤 재개를 거절하고 cleanup 권위는 기존 process owner에 남긴다. typed 송신도 단일 nonblocking write 후 반환하고 재시도마다 workspace/예약 권위·취소·공유 예산을 확인한다. 부분 frame 완료 전 다음 frame/수신 단계로 넘어가지 않는다. 파일/DB 관측은 동기식이며 제품 runtime/main-loop 통합·공개 승인 IPC는 아직 미완료다.
- **실행 프로필 수명**: `LoginProcess::reserve_codex_owned`는 검증된 CodexProfileBinding의 디렉터리 capability를 이동해 요청 함수 밖에서도 유지한다. 기존 로그인 borrowed API와 같은 reservation/cleanup/fence 경로를 사용하며 같은 경로에 새 프로필이 생겨도 원래 신원 검증을 유지한다. LoginProfile의 Sync 경계는 sidecar 초기화 스레드에서 소유 객체를 안전하게 전달하기 위한 컴파일 시간 제약이며 unsafe 구현은 없다.
- **native 소유 객체의 dispatcher 연결(내부 후보)**: AgentRuntime은 검증된 OwnedTextRun을 별도 목록에 보관하고 기존 poll/request_cancel/cancel_result/prepare_quit에 연결한다. 등록 거절은 소유권을 호출자에게 남기며 fixture와 합산한 실행 상한을 적용한다. 공개 원장에 저장된 event만 단조 증가 seq로 전달하고 내부 preparing을 running으로 위장하지 않는다. 종료 저장이 끝나기 전 취소 응답/소유 객체 제거를 하지 않으며 종료 저장 실패에는 owner와 fence를 유지한다. 요청은 live 대기열에서 명시 거절을 기다리며 도구 알림 자체를 승인으로 쓰지 않는다. 실제 native start/capability admission은 아직 미연결이고 제품 blocked 생성자는 유지된다.
- **예약 실행 단계 상태(내부)**: `begin_text_candidate`/`advance_text_candidate`는 Storage를 빌려 두지 않는 RunTextState에 bound transport·잠정 마지막 답변·run ID·통신 완료 결과를 보관한다. process별 연결 ID가 다른 owner의 진행/cleanup을 거절하고 terminal 이후 재개도 거절한다. 통신이 끝나면 다시 읽거나 보내지 않고 `poll_finish`로 종료를 진행한다. 동기 wrapper와 직접 poll은 같은 공개 원장·종료 확인·terminal/fence 로직을 공유하며 제품 runtime/IPC 연결은 후속이다.
- **단계별 종료 확인(내부)**: TreeTracker의 `poll_termination`은 누적 신원·최초 TERM/KILL 기한·관측 실패를 유지하고, 관측 간격 전에는 신호를 반복하지 않으며 sleep 없이 반환한다. 기존 동기 terminate도 같은 알고리즘을 사용한다. LoginProcess는 종료 단계 동안 프로토콜 I/O를 차단한다. 예약 owner는 새 자손 신원을 profile fence와 실행 원장 양쪽에 저장한 뒤 반환하며 저장 실패는 종료 증거를 무효화한다. 재시작 복구는 profile pending 파일을 읽지 않고 실행 원장의 누적 신원을 사용한다. 종료 확인 전에는 receipt/final/terminal 또는 fence 해제를 만들지 않고 원장 저장 뒤에만 fence를 해제한다. 파일/DB/OS 조회는 여전히 동기식이고 비정상 Drop의 최후 cleanup 및 명시적 실패 재시도는 기존 bounded blocking 경로다. 앱 전체의 비차단 종료 보장이나 제품 활성화 증거는 아니다.
- **계정 조회 알림 호환성**: full preflight는 account/read 응답 대기 중 typed account/updated(authMode=chatgpt) 한 번만 허용한다. emittedAtMs는 기존 로그인 규격의 u64/JS-safe 상한, planType은 제한된 선택 문자열이다. 알림은 pending RPC를 소비하지 않고 동기/단계별 driver 모두 같은 종료 owner·시간 예산에서 실제 id3 응답을 기다린다. 다른 단계/방식·중복·미지 필드·id가 있는 알림은 거절한다. 2026-09-29 고정 CLI/helper·앱 프로필에서 plugins 캐시 호환 수정 후 계정·모델·관리 정책 대조와 종료/잠금 정리를 모두 통과했다. 유료 모델/thread/turn·native 제품 실행의 합격 증거는 아니다.
- **장기 native 실행 owner(내부)**: OwnedTextRun은 준비된 RunReservation·소유형 LoginProcess·RunTextState·취소 표식을 함께 유지한다. Storage는 advance 동안만 빌린다. 기존 RunIdentity 6필드와 live run/process 결속을 확인한 취소만 접수하며 다음 advance가 종료/원장을 처리한다. 제품 active-run 목록과 공개 IPC에 아직 등록되지 않은 내부 연결 객체다.
- **내부 preflight 세션(제품 미활성)**: `provider_preflight_session.rs`는 새로운 exchange의 typed frame만 기존 LoginProcess/LoginIo에 보내며 통신 전체 30초 예산과 취소·자손 관측·별도 종료 확인을 적용한다. 조회 전용 또는 동일 process의 read-only thread 진단 뒤 cleanup을 확인하고 exchange를 닫는다. thread/start 응답과 thread/started 알림을 순서와 무관하게 각각 한 번 받고 동일 ID를 대조한다. 알림 단독은 성공이 아니다. 불확실 종료는 기존 영속 잠금과 호출자의 owner를 유지한다. 가짜 CLI 파이프 검사 단계이며 실제 provider launcher/config 격리·제품 실행 gate는 별도로 남는다. thread 생성도 통합 기능을 초기화할 수 있어 실제 진단 전 격리가 필요하다.
- **관리 정책 조회(내부 후보)**: layered full preflight는 initialize/config/read/account/read/model/list 뒤 고정 `configRequirements/read`(id 50)를 조회하고 정확한 `requirements:null` 보고까지 받아야 ObservationsMatched가 된다. 기존 thread/turn id 5/6은 유지한다. 관리 객체·누락·중복·미지 필드·RPC 오류는 연결 실패로 고정하며 raw 정책은 공개하지 않는다. 현재 후보는 관리 정책을 지원하지 않으므로 객체를 무시해 실행하지 않는다. [공식 API](https://learn.chatgpt.com/docs/app-server#read-admin-requirements-configrequirementsread)의 null은 구성된 정책 없음이라는 vendor 보고이며, 접근 불가 MDM의 신뢰 가능한 가시성까지 증명하지 않는다. 실제 계정 프로필의 5 RPC 성공·제품 활성화는 아직 미검증이다.
- **설정 전용 진단 범위**: `PreflightExchange::configuration_only`는 동일 초기화/설정 layer 검증 후 ConfigurationMatched에서 끝난다. account/read·model/list·configRequirements/read 요청을 만들지 않고 thread/start·turn 준비도 거절하며 전체 preflight의 ObservationsMatched와 구분한다. 기존 세션 driver는 해당 범위만 대조하고 cleanup을 확인한다. 실제 vendor 실행·네트워크 차단·임시 profile 입장은 별도 검증 사항이며 이 진단 성공을 실제 실행 준비 완료로 사용하지 않는다.
- **실제 offline 진단 결과(2026-09-29)**: 기존 정책에서는 관리 설정 동기화 경고로 실패했지만 `com.apple.system.opendirectoryd.libinfo` Mach 조회 한 항목을 허용한 후보에서 현재 pinned native CLI/helper의 initialize/config/read가 PASS했다. 새 임시 profile·빈 작업 폴더를 사용하며 explicit 종료·config 보존·auth 파일 미생성을 확인했다. cfprefsd·설정 도메인·공유 메모리 권한은 추가하지 않았다. 수동 opt-in 테스트 `installed_codex_offline_configuration_only`는 일반 테스트에서 제외된다. 이는 이 호스트의 설정 통신 호환성 증거뿐이다. MDM 설정이 upstream에서 보이지 않아 생략되는 경우를 세 layer 검증만으로 감지할 수 없으므로, 강제 관리 정책의 신뢰 가능한 가시성/명시적 거절을 검증하기 전에는 제품 admission 근거로 사용하지 않는다. 직접 네트워크 허용은 없지만 허용된 OS broker의 간접 동작까지 무통신이라고 증명한 것은 아니다. 제품 runtime은 여전히 blocked다.

- **실제 vendor 관측 파서**: `core/provider/vendor_observation.rs`는 Codex 0.149.1·Claude 2.1.269의 exact version과 모델 없는 인증 상태 응답만 읽는다. transient bounded input에서 계정/토큰을 제거한 분류만 반환하며 runtime·capability·영속 원장에는 아직 연결하지 않는다. config/model/handshake, 설정 격리와 launch authority는 별도 미구현이다. 이후 설치 CLI는 0.153.4로 변경됐으며 기존 exact version 허용을 자동 확대하지 않았다.
- **로그인 schema 드리프트 검사**: `node scripts/m5-codex-login-schema.mjs <기준 생성 폴더> <후보 생성 폴더>`는 초기화·로그인/취소·계정 조회/알림 타입과 재귀 type import의 원문 해시를 비교한다. 종료 0은 이 범위의 원문 동일, 2는 검토 필요, 1은 비교 불가이며 전체 프로토콜/실행 호환 판정이 아니다. 0.149.1→0.153.4 대조는 20파일 중 Bedrock 인증 추가 2파일 변경으로 검토 필요다. 생성 입력은 로컬 임시 증거이며 이 도구가 CLI 실행·인증 조회를 하지는 않는다.

- **작업대 배치 현행**: `src/m5/workbench-dock.tsx`는 파일·편집기·변경·팀 대화를 중첩 가로/세로 분할과 여러 탭 그룹으로 구성한다. 제목/tab을 가장자리에 놓으면 분할하고 가운데 놓으면 탭으로 묶으며 배치 선택 메뉴로도 같은 작업을 할 수 있다. 경계선 pointer/키보드 조절, 탭 좌우/Home/End 및 초점 이동을 지원한다. 검증된 트리를 기존 v1 localStorage에 추가 저장하며 이전 평면 배치를 계속 읽는다. 숨김/재배치 때 평면 keyed DOM을 유지해 Monaco를 재생성하지 않는다. 합성 Chromium smoke는 사용자 키보드와 분리된 숨김 창/CDP 입력으로 편집 복구와 후속 대화를 검사한다. 전체 승인 시안 적용 완료를 뜻하지 않는다.
- **실제 CLI 최소 관측(2026-09-14)**: 빈 임시 폴더에서 Codex 0.149.1 app-server 초기화·config/account 조회·정상 종료와 Claude 2.1.269 인증 상태 조회를 확인했다. 모델 요청과 thread 생성은 없었다. 이 관측만으로 설정 격리·모델 가용성·실행 권위를 보장하지 않으며 제품 actual provider는 활성화하지 않았다.

- **부분 RSS 관측 경계**: `m5:measure:renderer --memory`는 실제 Electron subtree의 RSS를 250ms 간격으로 관측한다. `f4def74`는 idle 약 409.73 MiB 및 한 회차 표시 지연으로 FAIL했다. 테스트 fixture의 idle 선할당을 분리한 `dfe12bd`는 idle/peak/60초 잔류 356.70/761.73/110.125 MiB와 GUI 시간 기준을 통과했다. 합성 bridge를 쓰며 실제 sidecar/agent·이탈 자손은 포함하지 않아 전체 제품 메모리 증거가 아니다. 이전 실패는 삭제하지 않는다.

- **G6 성능 계측**: `scripts/m5-performance-metrics.mjs`는 명세 §10의 공급된 측정값을 판정하고 `scripts/m5-sidecar-startup-metrics.mts`는 현재 Cargo release executable을 생성된 빈 DB에서 migration 제외·warm-up 1회·10회 실행해 shell/storage ready를 구분한다. `d97ee56`에서 p95 3.863/52.596ms와 정상 종료·clean marker를 확인했다. `7832348`의 `scripts/m5-renderer-metrics.mjs`는 production renderer·실제 Monaco와 합성 bridge fixture로 대형 파일/diff·파일 목록의 10회 GUI 측정을 통과했다(파일 표시 p95 75.9ms, 입력 32.3ms, 변경 요약 38.7ms, 첫 diff 25.0ms). 파일 목록은 opaque cursor로 다음 500개를 탐색하고 오래된 응답이 새 폴더를 덮지 못한다. 패키징 interactive startup·실제 스트림 paint·전체 RSS·경합/종료 계측은 아직 남아 있으며 `m5:test:performance`는 미등록 상태다. 절차는 `procedures/m5-performance.md`다.

- **actual-agent 전체 수명 보존의 현재 경계**: G5D의 exact v10 admission, V9/V10 materializer, V10 bounded lossless writer, cooperative scheduler와 기존 D1 `storage.status` wire는 구현됐다. 최신 clean integration commit `6461e67617f4f5e55d7d646034dbb0971f1d845a`에서 macOS fixture 누적 `g5d-storage-lifetime` 16/16과 darwin-arm64 package·Chromium Monaco smoke를 통과했다(`.artifacts/m5-harness/g5d-storage-lifetime-2026-09-09T03-27-46-038Z.json`). 누적 하네스는 standalone wrapper의 범위를 보존하면서 locked workspace Rust를 한 번만 실행하고, 각 명령 전후 clean/same-HEAD 및 focused storage evidence의 동일 HEAD를 schema v2 report에서 재검증한다. 실측 시간은 47분 18초에서 16분 32초로 단축됐다. 이는 출시 승인이 아니며 Windows 안전 게이트, 실제 사용자 DB·강제 종료 검증, 실제 Claude Code/Codex provider·인증, G6 성능·서명·배포 판정이 남아 있다. 보존 기간과 physical cleanup/삭제 정책은 사용자 결정 전 계속 비활성이다.
- **코드 서명**: 미서명 배포 시 SmartScreen/Gatekeeper 경고. 베타는 미서명 + 통과 안내 이미지로 진행, 정식 릴리즈 전 비용(연 $250~400) 결정 — 기획 문서 참고.
- **실기기 출시 검증**: 깨끗한 macOS 계정과 Windows 기기에서 설치·로그인·오프라인·권한 오류·한글 경로 시나리오 확인.
- **업데이트 채널**: 현재 앱은 에이전트 최신 버전만 확인한다. Hello, Agent 자체 업데이트와 레시피 원격 갱신 방식은 첫 베타 이후 결정.
