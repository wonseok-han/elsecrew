# M5.0 — Electron/Rust 사이드카 구현 명세

> **상태: 구현 전 검토용 명세. 아직 현행 동작이 아니다.**
>
> 상위 제품 기준은 [`m5-coding-agent-ide.md`](m5-coding-agent-ide.md)다. 두 문서가 충돌하면 상위 문서의 **확정된 사용자 결정**과 안전 불변식을 우선하며, 이 문서는 첫 구현 단위의 저장소 구조·프로토콜·fixture·합격 수치를 구체화한다.

작성 기준일: 2026-08-24

---

## 1. 목적과 완료 산출물

M5.0의 목적은 Electron 채택을 다시 비교하는 것이 아니라, 이미 확정한 **Electron + React/TypeScript + Monaco renderer + Rust sidecar** 경계를 실제로 한 번 끝까지 연결해 이후 기능을 쌓을 수 있는 기준선을 만드는 것이다.

완료 시 다음 산출물이 있어야 한다.

1. Electron main/preload/sandboxed renderer와 Rust sidecar가 패키징된 앱에서 함께 시작하고 종료된다.
2. 사용자가 명시적으로 고른 Git 프로젝트를 읽고, 앱이 만든 격리 worktree에서 파일 탐색·편집·저장·외부 변경 감지·diff 검토를 수행한다.
3. 서로 다른 `chatId`/`runId`의 모의 스트림과 한 번의 실제 macOS 에이전트 실행이 같은 채팅 원장에 섞이지 않고 기록된다.
4. 실행 중 정상 종료, sidecar 충돌, 앱 강제 종료 뒤의 복구 상태가 SQLite에서 재현된다.
5. 아래 fixture와 성능·메모리·패키지·보안 게이트의 측정 결과가 증거 문서로 남는다.

M5.0은 사용자에게 배포하는 완성 IDE가 아니다. 합격 뒤에도 M5.1의 여러 영속 채팅 제품화와 M5.2의 편집 경험 완성이 남는다.

## 2. 이번 범위와 비목표

### 2.1 구현 범위

- Electron Forge 기반 개발·패키징 골격
- `app://` 로컬 앱 프로토콜, 보안 설정, 최소 typed preload API
- Rust sidecar 시작·handshake·health·정상 종료·비정상 종료 감시
- 버전이 붙은 stdio JSONL 프로토콜과 스트림 격리
- Rust 순수 안전 코어의 최소 추출과 기존 M4 종료 테스트 재사용
- SQLite 스키마 v1: 이벤트 원장, projection, 앱/sidecar 실행 세대
- 프로젝트 명시 선택과 정규화, 격리 worktree 생성·검증
- 가상화된 파일 트리, Monaco 일반 편집기와 diff 편집기, 저장·외부 변경 충돌 보호
- 파일별 diff 요약과 텍스트 diff 열기
- 모의 다중 채팅 스트림, macOS 실제 에이전트 한 경로의 입력·출력·중단 smoke
- 정상 종료·충돌·강제 종료 복구와 `recoveryRequired` 차단
- macOS 패키지와 Windows UI 패키지 smoke

### 2.2 비목표

- Tauri와 Electron에서 같은 M5 기능을 동시에 구현하는 것
- 기존 M0~M4 온보딩 전체를 새 디자인으로 옮기는 것
- 기획·개발·검토 역할 왕복을 제품 수준으로 완성하는 것
- 여러 writer의 동시 실행, 다중 터미널, LSP, 디버거, 확장 마켓
- 파일/줄 단위 적용, commit·push·PR 자동화
- Windows에서 Claude Code/Codex를 실행하는 것
- 리브랜딩, 저장소명·번들 ID·데이터 경로 변경
- 자동 업데이트 서버와 정식 코드 서명 인프라 완성

## 3. 고정 기술 선택

| 항목 | M5.0 선택 | 계약 |
| --- | --- | --- |
| 데스크톱 호스트 | Electron stable | 구현 착수 시 현재 stable을 정확한 버전으로 고정하고 임의 자동 상승을 막는다 |
| 패키징 | Electron Forge | 공식 권장 도구를 사용한다. `package`와 `make`를 CI에서 별도 검증한다 |
| 번들러 | Forge Vite plugin | 기존 React/Vite 자산을 잇되, 플러그인의 실험적 상태를 인정해 Electron/Forge/Vite 관련 버전을 exact pin하고 업그레이드를 별도 변경으로만 수행한다 |
| renderer | React + TypeScript + Monaco ESM | Monaco worker를 Vite worker entry로 로컬 번들링한다. 원격 코드·CDN을 쓰지 않는다 |
| 시스템 코어 | Rust workspace의 library + sidecar binary | 안전 판단과 에이전트 argv는 Rust가 소유한다 |
| sidecar 전송 | stdin/stdout JSON Lines v1 | stdout은 프로토콜 전용, 진단 로그는 stderr 전용이다 |
| 영속 저장 | SQLite, Rust 소유 | bundled SQLite를 사용해 사용자 기기의 외부 런타임에 의존하지 않는다 |
| 프로젝트 격리 | Git worktree | 한 `Chat`은 한 worktree를 소유하며 M5.0에서는 writer 1개만 허용한다 |

`pnpm-lock.yaml`과 `Cargo.lock`은 함께 버전 관리한다. `package.json`의 Electron·Forge·Vite·Monaco 관련 새 의존성에는 `^`나 `~`를 쓰지 않는다. 판올림은 패키징, CSP, preload API, sidecar handshake, fixture를 다시 통과한 독립 커밋으로만 한다.

## 4. 저장소 구조와 전환 순서

```text
electron/
  forge.config.ts
  vite.main.config.ts
  vite.preload.config.ts
  vite.renderer.config.ts
  main/                 # 창, app://, sidecar 감독, IPC sender 검증
  preload/              # window.m5의 최소 typed bridge
src/
  m5/                   # 새 작업대 renderer entry와 화면
  ...                   # 기존 M0~M4 React 화면; M5.0 동안 동결
crates/
  hello-agent-core/     # Tauri/Electron을 모르는 안전·도메인 코어
  hello-agent-sidecar/  # stdio protocol, SQLite, adapter 조합
src-tauri/              # 현재 앱 host; M5.0 동안 회귀 수정 외 기능 추가 금지
tests/fixtures/m5/      # 생성기와 작은 고정 fixture
```

루트에 Cargo workspace를 만들고 `src-tauri`, `hello-agent-core`, `hello-agent-sidecar`를 구성원으로 둔다. 전환 중 기존 Tauri 앱이 사라지지는 않지만 **새 M5 기능은 Electron 경로에만 구현**한다. Electron 기준선이 합격하면 별도 사용자 승인 아래 기본 개발/배포 명령을 Electron으로 전환하고, 그 뒤 Tauri host 제거 계획을 세운다.

### 4.1 Rust 추출 경계

현재 `orchestration.rs`와 `handoff.rs`에는 순수 안전 로직과 `tauri::AppHandle`, `State`, `Emitter`가 섞여 있다. 파일 전체를 복사하지 않는다.

1. `error`, `agent`, `proctree`, Claude/Codex 실행 계약처럼 Tauri를 모르는 코드를 core로 이동한다.
2. 경로, 이벤트 전달, 시간, 프로세스 표, 증거 저장은 작은 trait/입력값으로 주입한다.
3. 기존 Tauri command는 core를 호출하는 얇은 adapter로 유지해 기존 테스트가 계속 통과하게 한다.
4. sidecar는 같은 core를 호출하고 JSONL 이벤트로 변환한다.
5. `handoff`/`orchestration`의 Tauri 의존 구간은 수직 슬라이스에 필요한 순서대로만 추출한다. 대규모 선행 리팩터링은 하지 않는다.

core crate에는 `tauri`, Electron/Node 개념, SQLite UI projection을 넣지 않는다. `hello-agent-sidecar`만 프로토콜과 SQLite를 소유한다.

## 5. 프로세스와 신뢰 경계

```text
사용자 입력
   ↓
Sandboxed Renderer ── typed window.m5 API ──▶ Preload
   ▲                                             │
   │ projection/event                            │ 허용된 IPC만
   │                                             ▼
   └──────────────────────────────────── Electron Main
                                                   │ spawn(shell:false)
                                                   │ stdin/stdout JSONL
                                                   ▼
                                             Rust Sidecar
                                             ├─ SQLite
                                             ├─ Git/worktree/files
                                             └─ agent process tree
```

- renderer는 Node, Electron IPC, 파일시스템, 셸, child process를 직접 볼 수 없다.
- main은 창과 sidecar의 **생존**을 감독하지만, worktree가 안전한지·종료가 확인됐는지·적용 가능한지를 판정하지 않는다.
- sidecar는 renderer/main이 준 경로와 상태를 신뢰하지 않고 canonical path, manifest, 시작 commit, 실행 세대, 현재 잠금을 다시 검증한다.
- renderer가 렌더링하는 에이전트 출력은 일반 문자열이다. HTML로 삽입하지 않으며 링크·외부 열기는 별도 허용 정책을 거친다.

## 6. Electron 보안 계약

`BrowserWindow`는 다음을 명시적으로 고정한다.

- `nodeIntegration: false`
- `contextIsolation: true`
- `sandbox: true`
- `webSecurity: true`
- 임의 원격 navigation, 새 창, `<webview>` 금지
- 로컬 renderer는 표준·보안 custom scheme인 `app://`로만 로드

초기 CSP는 다음 능력만 허용한다.

```text
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';
img-src 'self' data:;
font-src 'self';
worker-src 'self' blob:;
connect-src 'self';
object-src 'none';
base-uri 'none';
frame-ancestors 'none'
```

Monaco ESM worker는 로컬 Vite worker bundle에서만 만든다. `unsafe-eval`을 추가하지 않는다. inline style 허용은 Monaco/React의 현재 렌더링 요구에만 한정하고, 스크립트·네트워크 권한으로 확대하지 않는다.

preload는 `invoke(channel, payload)`나 `ipcRenderer` 원형을 노출하지 않는다. `selectProject()`, `createChat()`, `readFile()`, `saveFile()`, `startRun()`, `cancelRun()`, `prepareQuit()`처럼 한 동작당 한 메서드를 노출하고, 입력/출력 TypeScript 타입과 런타임 검증을 함께 둔다. main은 모든 호출에서 main frame과 `app://` origin을 검증한다.

패키지에서는 ASAR와 무결성 검증을 켜고, Forge Fuse plugin으로 최소한 다음을 잠근다.

- `RunAsNode` 비활성화
- `NODE_OPTIONS`와 Node inspector CLI 인자 비활성화
- 앱 코드는 ASAR에서만 로드
- 지원 플랫폼의 embedded ASAR integrity 검증 활성화

Rust sidecar는 `process.resourcesPath` 아래의 정해진 절대경로만 `spawn(command, args, { shell: false })`로 실행한다. stdout/stderr pipe를 분리하고, 사용자 입력을 실행 파일명이나 shell 문자열로 사용하지 않는다. 개발용 임의 sidecar 경로 override는 production build에 포함하지 않는다.

## 7. Sidecar protocol v1

### 7.1 framing과 공통 envelope

- UTF-8 JSON 한 객체를 한 줄로 전송한다. 줄바꿈 문자를 포함한 값은 JSON escape한다.
- stdout에는 프로토콜 객체 외 어떤 텍스트도 쓰지 않는다. 로그·panic hook은 stderr로 보낸다.
- control message 한 줄 상한은 **1 MiB**, text chunk는 **256 KiB**다. 초과·잘못된 JSON·알 수 없는 필수 버전은 연결을 오염된 것으로 보고 종료한다.
- 모든 객체는 `protocolVersion: 1`, `kind`, `sidecarInstanceId`를 가진다.

```json
{"protocolVersion":1,"kind":"request","requestId":"req_01","method":"file.read","params":{"projectId":"p1","path":"src/App.tsx"}}
{"protocolVersion":1,"kind":"response","requestId":"req_01","ok":true,"result":{"streamId":"s1"}}
{"protocolVersion":1,"kind":"event","streamId":"s1","seq":1,"event":"blob.chunk","payload":{"text":"..."}}
```

`requestId`는 한 sidecar 실행에서 유일하다. 스트림의 `seq`는 1부터 단조 증가하며 main과 renderer가 중복·누락·역전을 검사한다. `chatId`, `taskId`, `runId`는 해당 이벤트에 명시하고 현재 선택된 채팅으로 추정하지 않는다.

### 7.2 handshake와 수명주기

1. main이 sidecar를 시작한다.
2. sidecar는 DB migration과 이전 실행 복구 스캔 뒤 `system.ready`를 보낸다.
3. ready에는 protocol/sidecar/app schema 버전, PID, 플랫폼, 지원 capability, `sidecarInstanceId`가 들어간다.
4. main은 protocol과 capability를 검증한 뒤에만 renderer 창을 ready 상태로 바꾼다.
5. 3초 안에 ready가 없거나 버전이 맞지 않으면 자동 재시작을 반복하지 않고 쉬운 시작 오류로 멈춘다.

정상 종료는 `app.prepareQuit` 요청 뒤 sidecar가 새 실행을 막고, 실행 중 agent 자손을 안전 중단하고, 소멸 판정과 DB checkpoint를 마친 다음 `safeToQuit: true`를 응답하는 순서다. Electron은 그 전까지 종료를 보류한다. OS 강제 종료를 안전 성공으로 가장하지 않는다.

### 7.3 M5.0 최소 메서드

| 영역 | 메서드/이벤트 |
| --- | --- |
| system | `system.status`, `app.prepareQuit`, `system.ready`, `system.fatal` |
| project | `project.open`, `project.getTree`, `project.createWorktree`, `project.close` |
| file | `file.read`, `file.save`, `file.watch`, `file.changed`, `file.conflict` |
| diff | `diff.getSummary`, `diff.getFile`, `diff.changed` |
| chat | `chat.create`, `chat.list`, `chat.get`, `chat.eventAppended` |
| run | `run.startFixture`, `run.startAgentSmoke`, `run.cancel`, `run.event`, `run.finished` |
| recovery | `recovery.list`, `recovery.inspect`, `recovery.markResolved` |

프로토콜 오류는 `code`, 초보자 화면이 번역할 `kind`, 기술 증거인 `detail`, 재시도 가능 여부를 분리한다. sidecar가 답을 잃었는지 확실하지 않은 위험 동작은 자동 재시도하지 않는다.

### 7.4 큰 데이터와 backpressure

- 5 MiB 파일과 큰 diff를 단일 IPC 값으로 복제하지 않는다. sidecar가 256 KiB chunk stream 또는 파일/구간 페이지로 보낸다.
- main은 스트림별 최대 8 MiB 또는 256 event의 bounded queue를 둔다.
- renderer가 따라오지 못하면 이벤트를 조용히 버리지 않고 해당 stream을 취소해 `consumerTooSlow` 오류로 끝낸다.
- UI는 chat/diff event를 batch로 projection에 반영하되 SQLite 원장의 순서를 바꾸지 않는다.

## 8. SQLite v1과 복구 계약

DB는 OS의 앱 데이터 디렉터리 아래 기존 제품 데이터와 충돌하지 않는 M5 하위 경로에 둔다. 리브랜딩 전에는 기존 제품 식별자를 유지한다.

최소 테이블은 다음과 같다.

| 테이블 | 역할 |
| --- | --- |
| `schema_meta` | schema/protocol 버전과 migration 상태 |
| `app_instances` | sidecar 실행 세대, 시작·정상 종료·마지막 heartbeat |
| `events` | append-only 원장. event ID, aggregate ID/type, seq, payload, 시각 |
| `projects` | 검증된 프로젝트 신원과 현재 projection |
| `chats` | 목표, 역할 snapshot, worktree, 현재 상태 projection |
| `runs` | adapter, 모델, 실행/종료/회수 판정 projection |
| `locks` | writer, poisoned, recoveryRequired와 근거 |

위험 동작은 **원장 append와 projection 갱신을 한 transaction으로 저장한 뒤** 시작한다. 이벤트 payload에는 schema version을 둔다. projection은 원장에서 다시 만들 수 있어야 하며, replay 결과가 저장된 projection과 다른 테스트 fixture를 실패시킨다.

sidecar 시작 시 직전 `app_instances`가 정상 종료되지 않았고 running/preparing/cancelling run이 있으면 해당 run과 worktree를 `recoveryRequired`로 복원한다. 새 실행·적용은 차단한다. 실제 자손과 worktree를 조사해 해결하기 전 단순 `정상` 표시는 허용하지 않는다.

## 9. 구현할 수직 시나리오

M5.0 합격 데모는 다음 한 흐름으로 고정한다.

1. 패키징된 Electron 앱을 시작하고 sidecar handshake를 확인한다.
2. 사용자가 native dialog에서 fixture Git 프로젝트를 명시적으로 고른다.
3. sidecar가 프로젝트를 검증하고 앱 manifest와 격리 worktree를 만든다.
4. renderer가 10,000개 파일 fixture를 가상화된 트리로 보여 준다.
5. 5 MiB 파일을 Monaco에서 열어 한국어 IME로 수정하고 저장한다.
6. fixture agent가 다른 파일을 외부 수정한다. watcher가 이를 감지하고 파일 목록과 diff를 갱신한다.
7. 일반 편집 탭과 side-by-side/inline diff를 오가며 undo와 저장 모델이 섞이지 않음을 확인한다.
8. 네 chat stream을 동시에 재생하고 각 역할/모델/run ID 메시지가 올바른 chat에만 보이는지 확인한다.
9. 사용자 명시 동의 뒤 설치·로그인된 에이전트 하나로 작은 macOS smoke를 실행하고 출력·중단·자손 소멸을 확인한다. 비용이 발생할 수 있으므로 자동 CI에서는 fake adapter로 대체한다.
10. 실행 중 정상 앱 종료를 요청해 안전 중단과 재시작 복원을 확인한다.
11. 별도 회차에서 sidecar와 Electron을 강제 종료해 재시작 뒤 `recoveryRequired` 및 위험 행동 차단을 확인한다.

실제 사용자 프로젝트는 자동 성능 fixture나 강제 종료 fixture에 사용하지 않는다. 생성기가 만든 임시 Git 저장소만 사용하고, 각 테스트가 정리 대상을 manifest로 검증한다.

## 10. Fixture와 수치 합격선

아래 값은 **M5.0의 잠정 합격선**이다. 측정 결과가 다르다는 이유로 같은 변경에서 조용히 낮추지 않는다. 실패 시 병목 증거와 새 수치의 근거를 문서화해 별도 승인을 받는다.

측정 문서에는 기기 모델, CPU, RAM, OS, Electron/Chromium/Node/Rust 버전, cold/warm 여부, 표본 수를 기록한다. 시간 수치는 개발 기준 macOS 기기에서 warm-up 1회 뒤 10회 중 p95로 판정한다.

| 항목 | Fixture | 합격선 |
| --- | --- | --- |
| 앱 시작 | 빈 DB, 패키징 앱 | warm interactive p95 ≤ 3.0초, cold ≤ 5.0초 |
| sidecar ready | migration 없음 | p95 ≤ 1.0초, hard timeout 3.0초 |
| 파일 트리 | 10,000 files / 1,000 dirs | 첫 루트 표시 p95 ≤ 1.0초, 폴더 확장 p95 ≤ 250ms, DOM node 500개 이하 |
| 큰 파일 | UTF-8 5 MiB, 100,000 lines | 첫 편집 가능 p95 ≤ 1.5초, 입력→paint p95 ≤ 50ms |
| 큰 diff | 500 files, 20,000 changed lines, patch 총 10 MiB | 요약 p95 ≤ 2.0초, 첫 파일 diff p95 ≤ 1.5초, main/renderer long task 200ms 초과 0건 |
| 채팅 스트림 | 4 streams × 50 events/s × 60초 = 12,000 events | 유실·중복·cross-chat·순서 역전 0건, event→paint p95 ≤ 100ms |
| 저장/외부 변경 | 저장과 외부 쓰기 100회 경합 | 무음 덮어쓰기 0건, 충돌을 항상 `file.conflict`로 표면화 |
| 종료 | fixture child + 분리된 자손 | 정상 종료 시 추적 신원 생존 0건, 미확인은 항상 `recoveryRequired` |
| 메모리 | cold idle / 모든 큰 fixture 열기 | 전체 RSS idle ≤ 400 MiB, peak ≤ 800 MiB, fixture 닫기 60초 뒤 idle 대비 잔류 ≤ 120 MiB |
| 패키지 | macOS arm64 unpacked / 압축 산출물 | unpacked ≤ 350 MiB, 배포 산출물 ≤ 200 MiB; Windows는 같은 수치를 측정만 하고 기준은 후속 확정 |

입력→paint와 event→paint는 renderer의 `performance.now()` 표식을 사용한다. 프로세스별 메모리와 전체 RSS를 함께 기록하며, 평균만으로 p95 실패를 숨기지 않는다.

## 11. 테스트와 실패 주입

### 11.1 자동 테스트

- TypeScript unit: bridge runtime validation, stream sequence, projection reducer, dirty editor 전이
- Rust unit: envelope size/version, SQLite append+projection transaction, replay, canonical path, lock 전이
- 기존 Rust 회귀: `proctree` F1~F8, Claude/Codex 실행 계약, poisoned/증거 보존
- contract test: TypeScript와 Rust가 같은 protocol fixture를 각각 encode/decode
- Electron integration: sandbox/CSP/preload surface, app:// origin, sender 거부, Monaco worker
- end-to-end: §9에서 실제 유료 agent를 제외한 전체 흐름
- packaging smoke: unpacked가 아닌 `package`/`make` 산출물에서 sidecar handshake와 Monaco worker 확인

### 11.2 필수 실패 주입

1. 잘못된 protocol version, 1 MiB 초과 line, 중간에 끊긴 JSON
2. sidecar ready 전 종료, 응답 중 crash, stderr 폭주
3. renderer consumer 정지와 bounded queue 초과
4. DB 쓰기 실패, migration 중단, projection 불일치
5. worktree manifest 변조, symlink 경계 이탈, 원본 시작 commit 변경
6. file save 직전 외부 변경, binary/삭제/rename diff
7. agent가 stdout을 닫지 않음, 별도 세션 자손 생성, TERM 무시, PID 재사용 fixture
8. 준비 중/실행 중/취소 중 Electron 정상 종료 요청
9. Electron `SIGKILL`/강제 종료 뒤 재시작
10. IPC를 subframe, 잘못된 origin, 개발자 도구에서 직접 호출

판정할 수 없는 상태는 성공으로 접지 않는다. 특히 sidecar crash 뒤 main이 마지막 응답을 받지 못했으면 위험 요청을 자동 재전송하지 않는다.

## 12. 구현 순서와 커밋 경계

여러 에이전트가 구현할 때는 저장소 공통 절차인 [`../procedures/multi-agent-orchestration.md`](../procedures/multi-agent-orchestration.md)를 함께 따른다. `m5/coding-agent-ide`는 통합 담당만 수정하고, 구현 담당은 별도 worktree와 기능 branch에서 파일 소유권이 겹치지 않게 작업한다.

각 단계는 독립 검토 가능한 커밋으로 만들며, 다음 단계가 앞 단계의 실패를 숨기지 않게 한다.

1. **Scaffold**: Electron Forge/Vite, app://, 보안 BrowserWindow, 빈 typed preload, 패키징 smoke.
2. **Core extraction**: Cargo workspace, Tauri-free core, 기존 M4 테스트를 양 adapter 아래서 통과.
3. **Protocol/storage**: sidecar handshake, JSONL parser, SQLite v1, replay/recovery fixture.
4. **Project/worktree**: 명시 선택, manifest, tree/read/save/watch/diff API와 실패 주입.
5. **Renderer slice**: 승인 시안의 최소 셸, Monaco, 파일·변경 탭, dirty/conflict 보호.
6. **Chat/run slice**: 다중 stream fixture, macOS agent smoke, cancel/termination/recovery.
7. **Hardening/evidence**: CSP/fuses/ASAR, package/make, 수치 fixture, macOS/Windows UI smoke, 증거 문서.

단계 2에서 기존 테스트가 깨진 상태로 단계 3을 시작하지 않는다. 단계 7의 측정 스크립트와 fixture 생성기는 재현 가능하게 저장소에 남긴다.

## 13. M5.0 완료·중단 판정

### 완료

- §9 흐름과 §10 수치가 패키징 앱에서 통과한다.
- §11 실패 주입에서 무음 데이터 손실, cross-chat 오염, 원본 직접 변경, 종료 미확인 성공 처리가 0건이다.
- macOS 실제 에이전트 한 경로에서 입력·출력·명시 중단·자손 소멸을 확인한다.
- Windows 패키지에서 UI·sidecar·fixture는 동작하고 agent 실행은 명시적으로 비활성화돼 있다.
- M5.1이 사용할 versioned bridge, sidecar protocol, SQLite migration 기준이 문서와 테스트로 고정돼 있다.
- 측정값과 남은 위험을 `docs/history.md`와 M5 증거 문서에 기록한다.

### 즉시 중단하고 설계를 다시 검토할 조건

- sandbox/context isolation을 끄거나 renderer에 Node/범용 IPC를 노출해야만 Monaco가 동작함
- main 또는 renderer가 M4 안전 판단을 복제해야만 수직 흐름이 연결됨
- sidecar crash/앱 강제 종료 뒤 실행·잠금 상태를 fail-closed로 복원할 수 없음
- 5 MiB 파일 또는 10 MiB diff에서 chunk/page 구조로도 main event loop가 반복적으로 멈춤
- 패키징된 sidecar의 경로·무결성·서명을 앱 수명주기와 함께 보장할 수 없음

수치 하나의 실패는 곧 Electron 포기를 뜻하지 않는다. 위 구조적 중단 조건과 단순 최적화 문제를 구분해 증거로 판단한다.

## 14. M5.0 뒤로 넘기는 열린 결정

- 앱 종료 뒤 에이전트를 백그라운드 서비스로 계속 실행할지
- SQLite 원장 payload 보존 기간과 원시 로그 자동 정리 정책
- 저장하지 않은 편집 내용을 DB에 복구용으로 중복 저장할지
- Windows 자손 신원·종료 계약 구현 방식
- 전체 worktree 적용 뒤 파일/줄 단위 적용을 어느 마일스톤에 넣을지
- 앱 기본 역할 프리셋과 공급자별 모델 capability 조회 규격
- Electron auto-update 공급자, 코드 서명·공증, sidecar 서명 CI
- 최종 제품 이름과 번들 ID/데이터 경로 마이그레이션

이 질문은 M5.0 착수를 막지 않는다. M5.0에서 임시로 필요한 경우 안전한 최소값을 쓰고 확정된 제품 결정으로 가장하지 않는다.

## 15. 구현 근거

- [Electron Process Model](https://www.electronjs.org/docs/latest/tutorial/process-model)
- [Electron Security](https://www.electronjs.org/docs/latest/tutorial/security)
- [Electron Context Isolation](https://www.electronjs.org/docs/latest/tutorial/context-isolation)
- [Electron Process Sandboxing](https://www.electronjs.org/docs/latest/tutorial/sandbox)
- [Electron custom protocol API](https://www.electronjs.org/docs/latest/api/protocol)
- [Electron Packaging Tutorial](https://www.electronjs.org/docs/latest/tutorial/tutorial-packaging)
- [Electron Forge Vite plugin](https://www.electronforge.io/config/plugins/vite)
- [Node.js child_process](https://nodejs.org/api/child_process.html)
- [Monaco Editor ESM integration](https://github.com/microsoft/monaco-editor/blob/main/docs/integrate-esm.md)
