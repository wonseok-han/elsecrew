# 작업 이력 (Hello, Agent)

> 여러 코딩 에이전트(Claude Code, Codex)가 **번갈아 작업**할 때 컨텍스트를 이어받기 위한 워크로그다.
>
> **세션 시작**: 아래 "핵심 결정·발견"과 "작업 규칙"을 읽고, 워크로그 맨 위 엔트리로 현재 상태를 파악한다.
> **세션 종료**: 코드 변경이 없어도 프로젝트에 영향을 주는 분석·결정·검증·외부 작업을 했다면 워크로그 맨 위에 새 엔트리를 추가한다(최신이 위). 헤더는 `### YYYY-MM-DD · by <모델명>` 형식으로 **작업한 모델을 구체적으로** 적는다(예: `by Claude Opus 4.8`, `by GPT-5 Codex`). 한 날 여러 모델이 작업했으면 엔트리를 나눈다. 재사용할 만한 새 결정·발견은 "핵심 결정·발견"에도 반영한다. **엔트리는 덮어쓰지 말고 계속 쌓는다.**
>
> **아카이브 롤오버**: 이 파일이 **500줄을 넘으면** 오래된 엔트리부터 `archive/history-YYYY-MM.md`로 옮겨 **300줄 이하로 줄인다.** 날짜 경계에서 자르고(같은 날짜를 쪼개지 않음), 삭제가 아니라 이동이며, 고정 섹션은 옮기지 않는다. **날짜별로 파일을 쪼개지 않는다** — 현재 진실이 흩어진다. 날짜별 조회는 `git log -- docs/history.md`.
>
> **설계 심의는 여기 쌓지 않는다**: 한 주제의 토론 왕복·철회·정정이 여러 엔트리에 얽히기 시작하면 전용 설계 문서로 옮기고 여기에는 결론 요약과 포인터만 남긴다. 워크로그는 *무엇이 일어났나*, 설계 문서는 *무엇이 참인가*를 기록한다. 예: `proposals/m4-orchestration.md`

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 공용 wire 결속 리뷰 수정)

- **accepted 결속**: `AgentRunAcceptedV1`에 chat의 task/project/worktree/workspace identity와 capability fingerprint를 추가했다. 시작 요청·채팅에서 만든 exact context와 accepted 응답을 비교하는 TypeScript/Rust helper와 공용 golden을 두어 chat/task/project/worktree/workspace/role/fingerprint 변조를 모두 거부한다.
- **공개 event 결속**: normalized `AgentPublicEventV1`도 actual run의 full identity와 capability fingerprint를 포함한다. event는 `status/question/final`과 bounded text만 허용하고 reasoning/raw/tool/stderr/debug/env/auth/prompt/token extra를 거부하며, 공용 golden이 chat/task/project/worktree/workspace/run/role/fingerprint 교차 결속을 양언어에서 확인한다.
- **G5/V4 호환 결정**: wire method나 현재 G5 capability 광고는 늘리지 않았다. V4 `getChat`은 기존 G5 detail과 `agentRuns`가 있는 planned G5A detail의 presence-discriminated union을 반환하므로 renderer는 `agentRuns` 존재를 좁힌 뒤에만 actual ledger를 읽는다.
- **검증·경계**: focused G5A Node 7개, 전체 Node, Rust protocol 25개, TypeScript no-emit, production Vite build, scoped clippy와 `git diff --check`가 통과했다. worktree에 의존성이 없어 일반 `pnpm build`는 격리 네트워크 fetch에서 막혔지만 루트의 고정 설치로 동일 `tsc && vite build`는 exit 0이었다. 실제 CLI·인증·model·사용자 프로젝트는 접근하지 않았다. 다음은 이 원자 commit의 독립 재리뷰와 Electron 소비자의 full-identity route 구현이다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A Electron raw chat event 리뷰 수정)

- **수정**: planned G5A 협상 상태에서 `chat.eventAppended`의 검증된 `TeamMessage`라도 `runId`가 `agent_run_` namespace면 listener 호출 전에 session을 poison하도록 보강했다. 아직 normalized actual-agent public event wire가 없으므로 raw stdout/reasoning text가 status/chat timeline으로 우회하는 길은 허용하지 않는다. 기존 raw `run.event`/`run.finished` poison도 유지한다.
- **G5 호환**: current G5 ready handshake의 `chat.get`은 `agentRuns` 없이 기존 detail validator로 계속 통과하는 회귀를 추가했다. V4 bridge getChat의 최종 TypeScript exact shape와 start accepted full identity/fingerprint는 공용 contract owner의 후속 fix를 기다리며 이 커밋에서 임의로 바꾸지 않는다.
- **검증·경계**: root 의존성으로 TypeScript `tsc --noEmit`, Electron bridge/scaffold Node 35개, `git diff --check`를 통과했다. fake G5A sidecar가 `agent_run_` role message에 raw stdout/reasoning text를 넣어도 renderer listener 0회와 client corruption을 확인했다. 실제 CLI/auth/model/user project는 호출하지 않았다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A Electron V4 소비 경계)

- **한 일**: additive V4의 다섯 actual-agent IPC channel(capability 조회, 시작, cancel, recovery inspect/resolve)을 main-frame/origin 검사가 있는 고정 handler와 좁은 preload bridge에 연결했다. renderer 요청은 계약 validator를 preload와 `SidecarClientV1` 양쪽에서 exact 검사하며 executable path·argv·model·env·cwd·prompt·user config·추가 경로가 포함되면 sidecar IPC 전 거부한다. raw `ipcRenderer`나 generic invoke는 노출하지 않았고 context isolation·sandbox·기존 G5 V3 표면은 유지했다.
- **협상·응답 결정**: 현 G5 exact ready set에서는 V4 호출이 `agentAdapterUnavailable`로 끝나며 sidecar에 request를 보내지 않는다. planned G5A exact capability set이 협상된 경우만 V4 request를 허용하고 capability fingerprint/runtime, start chat/role, actual cancel/recovery의 full chat/task/project/worktree/workspace/run identity, V4 `agentRuns` 별도 ledger를 contract validator로 fail-closed 확인한다. fixture `runs`와 actual `agentRuns`는 섞지 않는다.
- **공개 이벤트 경계**: 아직 normalized actual-agent public-event IPC가 계약되어 있지 않으므로 G5A 중 raw `run.event`/`run.finished`를 받으면 session을 poison한다. 따라서 reasoning·raw stdout/stderr·tool args/output·debug·env·auth·prompt·token·evidence body는 renderer로 전달되지 않는다. status/question/final의 normalized event route는 Rust adapter와 renderer 소비자가 추가될 다음 계약/구현 단위에서 함께 열어야 한다.
- **검증·경계**: root 의존성을 재사용한 TypeScript `tsc --noEmit`, Electron bridge/scaffold Node 34개, 전체 Node 132개, direct Vite production build, `git diff --check`를 통과했다. 현 fixture G5 handshake에서 V4 무전송, launch extra pre-IPC 거부, exact G5A handshake, cross-identity response poison, actual-vs-fixture ledger 분리, raw event non-exposure, cancel/recovery namespace를 fake transcript만으로 검증했다. worktree에 자체 `node_modules`가 없어 `pnpm build`는 격리된 네트워크에서 의존성 fetch를 시도하다 중단됐으며, package/lockfile 변경은 없다. 실제 sidecar adapter, Claude/Codex CLI·인증·model 조회·사용자 프로젝트·유료 모델은 호출하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 계약 통합·누적 게이트)

- **통합**: actual-agent adapter 계약 기능·리뷰 수정 세 커밋을 독립 최종 `APPROVE` 뒤 `m5/coding-agent-ide`에 `9008b92`·`aaf614f`·`7903d75`로 순서대로 통합했다. 기존 G5 handshake와 fake-only V3는 그대로이며, planned V4 actual-agent 메서드는 protocol 예약·계약일 뿐 sidecar/Electron이 아직 광고하거나 소비하지 않는다.
- **독립 리뷰 결과**: 최초 리뷰의 process 소멸 증거·capability fingerprint 결속·actual-run 취소/복구 wire·role-stage 결속과 재리뷰의 writer-lock 종료 결속·JSON member-order 양언어 parity 지적을 모두 공용 golden 회귀로 닫았다. 최종 재리뷰는 누적 head `dd92e1f`를 `APPROVE`했고 실제 CLI·인증·model·사용자 프로젝트 호출이 없음을 확인했다.
- **통합 검증**: 통합 HEAD에서 focused G5A Node 6개, Rust protocol 25개, 전체 Node 127개와 production build가 통과했다. G5 누적 하네스도 macOS Electron package/Chromium Monaco, Tauri·core·protocol·sidecar, 실제 sidecar stdio fixture와 clean/uncertain 취소·복구를 포함해 PASS했고 증거는 `.artifacts/m5-harness/g5-run-recovery-2026-08-27T02-04-16-861Z.json`이다. 최초 sandbox 실행은 Electron artifact 네트워크 fetch가 막혀 실패했으나 네트워크 허용 재실행은 전 단계 exit 0이었다.
- **다음 할 일·경계**: 별도 worktree에서 Rust fake adapter/supervisor와 Electron V4 소비자를 구현·독립 리뷰한 뒤 UI와 G5A fixture QA를 연결한다. 실제 CLI capability/auth smoke, 유료 모델, 사용자 프로젝트, Windows 자손 회수는 계속 별도 승인·후속 게이트이며 G6 release는 의도대로 `NOT_READY`다. `main` 병합과 push는 하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 계약 독립 리뷰 2차 수정)

- **writer lock 결속**: actual-agent terminal run과 termination transcript를 함께 검증하는 공용 persistence 계약을 추가하고 cancel result도 반드시 이를 통과하게 했다. clean exit에서 `workspaceWrite` profile은 `writerLockState=released`, `readOnly` profile은 `none`만 허용한다. recoveryRequired는 기존처럼 lock/chat/process가 모두 poisoned `recoveryRequired`여야 하며 preflight failure의 zero run/spawn/lock 의미는 바꾸지 않았다.
- **process parity**: cancel termination과 recovery가 보존하는 process evidence를 TypeScript의 JSON 문자열 순서가 아닌 `pid/startIdentityDigest/executablePathDigest/observedGone` 구조 값으로 비교해 Rust struct equality와 맞췄다. 같은 process object의 JSON member 순서만 다른 공용 valid fixture를 두 언어가 함께 승인하며, writer clean cancel·정상 성공·실행 실패의 잘못된 lock state는 공용 invalid fixture가 거부한다.
- **검증·경계**: focused Node 6개, focused Rust 3개, 전체 Node 127개와 protocol 25개, renderer production build 및 `git diff --check`가 통과했다. 실제 CLI·인증·model 조회·사용자 프로젝트는 접근하지 않았다. 다음은 별도 2차 fix commit을 독립 재리뷰하는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 계약 독립 리뷰 수정)

- **리뷰 수정**: 독립 리뷰의 네 차단 사항을 TypeScript/Rust 공용 계약과 같은 golden fixture로 고쳤다. clean exit는 모든 `(pid,startIdentityDigest)`가 중복 없이 `observedGone=true`여야 하며, actual run도 G5와 같은 `planning→planning`, `coding→implementing`, `review→verifying` 역할-stage 결속을 지킨다.
- **fingerprint 결정**: capability fingerprint는 top-level `fingerprint`와 관찰 시각 `observedAt`을 제외한 snapshot을 재귀 ASCII key 순서의 compact JSON으로 직렬화한다. 배열 순서는 보존하고 explicit null은 포함한 UTF-8 bytes의 SHA-256에 실제 결속한다. renderer/Vite 계약에는 Node 전용 모듈을 넣지 않고 기존 순수 TypeScript SHA-256을 사용하며, Node crypto는 테스트 oracle로만 비교한다. 정확한 snapshot이 바뀌었는데 이전 fingerprint를 재사용하면 양 언어가 거부한다.
- **V4 wire 경계**: 기존 G5 handshake·fixture-only V3를 바꾸지 않고 planned V4에 actual-agent 전용 `run.cancelAgent`, `recovery.inspectAgent`, `recovery.resolveAgent`만 예약했다. actual run/recovery는 `agent_run_`·`agent_recovery_` namespace와 별도 `agentRuns` ledger를 사용하고 chat/task/project/worktree/workspace identity를 exact 결속한다. clean cancel과 poisoned recovery/명시적 abandon resolution은 typed response와 validator만 정의했으며 Electron·sidecar 소비자는 아직 구현·광고하지 않는다.
- **검증·경계**: 공용 golden의 clean-exit process proof·중복 identity·fingerprint 재사용·cancel/recovery/chat identity·역할-stage 회귀를 양 언어가 함께 읽는다. focused Node 6개, 전체 Node 127개, focused Rust 3개·전체 protocol 25개, renderer production build와 `git diff --check`가 통과했다. 실제 Claude Code/Codex 실행·인증·model 조회·사용자 프로젝트 접근은 하지 않았다. 다음은 이 fix commit을 같은 독립 리뷰에 다시 제출하는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 실제 agent adapter 계약)

- **한 일**: 현재 G5 handshake와 `run.startFixture`의 fake-only 의미를 바꾸지 않고, protocol v1에 `agent.getCapabilities`·`run.startAgent`를 예약하고 별도 planned `M5BridgeV4`와 TypeScript/Rust 공용 agent adapter v1 계약·golden fixture를 추가했다. 실제 run은 `fixtureScenario`가 있는 G5 fixture run과 섞지 않고 V4 chat detail의 별도 `agentRuns` ledger로 정의했다. renderer start 요청은 `chatId/roleId/inputMessageIds/expectedCapabilityFingerprint`만 허용하며 실행 경로·argv·model·env·cwd·prompt는 서버가 채팅의 프로젝트별 역할 snapshot에서 해석한다.
- **안전 결정**: 실제 실행은 machine-readable runtime 검증 또는 정확한 CLI 버전에서 실측한 manifest만 허용한다. effective model 증거가 `argvOnly`이거나 catalog가 불명확하면 `canStart=false`이며 silent fallback은 없다. v1은 one-shot과 `nextRun` follow-up만 허용하고 active stdin/resume은 금지한다. launcher와 agent identity·process tree를 run/lock과 함께 release 전에 영속화하는 barrier, preflight `spawnCount=0/lockCount=0`, 공개 `status/question/final`만, redacted bounded evidence, 불확실 종료의 poisoned `recoveryRequired`를 양 언어 validator로 고정했다.
- **검증·경계**: Node 전체 126개, TypeScript/Vite build, Rust protocol 25개, scoped clippy와 `git diff --check`를 통과했다. 실제 Claude Code/Codex 실행·인증·model 조회·사용자 프로젝트 접근은 하지 않았다. 다음은 이 계약 commit의 독립 리뷰 후 Rust adapter와 Electron/UI 소비자를 별도 worktree에서 구현하고, 통합 fixture QA를 거치는 것이다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 최종 게이트)

- **판정**: 구현·문서 통합 commit `33f16a009065813bc575b1f528756f661c2ccdeb`을 독립 최종 게이트가 `PASS — APPROVE / G5_READY`로 승인했다. 계약, Rust, Electron, UI, 하네스와 문서의 독립 승인 commit이 모두 해당 통합 commit의 조상임을 확인했다.
- **최종 증거**: exact-head 누적 하네스 `.artifacts/m5-harness/g5-run-recovery-2026-08-26T04-01-24-084Z.json`은 clean 시작과 baseline~G5 명령 12개 전부 exit 0을 기록한다. 독립 QA `.artifacts/m5-qa/g5-integration-014bd987464d39674a5f531765051f60e63b5ecb-2026-08-26T03-45-43Z.json` 이후 변경은 독립 승인된 문서뿐이다.
- **경계·다음 할 일**: G6는 security/package/performance script가 없어 의도대로 `NOT_READY`다. 실제 Claude Code/Codex adapter·인증 smoke, Windows snapshot/process-tree, 전체 OS Quit E2E, Forge prerelease와 macOS 권한 강화는 G5 완료에 포함하지 않았고 후속 게이트로 남긴다. `main` 병합과 push는 하지 않았다.

### 2026-08-26 · by GPT-5.6-Sol (M5 문서 생애주기 경계 재정렬)

- **수정**: 독립 문서 리뷰 지적에 따라 `m5-coding-agent-ide.md`에서 G1~G6 현재 상태표, exact 검증 판정과 구현 완료 서술을 제거하고 제품 방향·설계 계약·계획된 완료 기준·열린 결정만 남겼다. 현행 G5 구조와 게이트 사실은 `architecture.md`와 이 워크로그만 소유한다.
- **색인·규칙**: `docs/README.md`와 byte-identical `AGENTS.md`/`CLAUDE.md`도 제안서를 현재 상태 출처로 안내하지 않도록 고쳤다. 이전 역사적 검토·결정 기록과 2026-08-25 아카이브 내용은 바꾸지 않았다.
- **다음 할 일**: 수정 commit을 독립 재검토한 뒤 G5 최종 게이트에서 문서 생애주기 경계를 확인한다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 누적 하네스·독립 QA와 현행 문서 반영)

- **판정**: exact 통합 commit `014bd987464d39674a5f531765051f60e63b5ecb`의 G5 누적 하네스가 PASS했고 독립 QA도 `G5_READY`로 판정했다. 하네스는 `.artifacts/m5-harness/g5-run-recovery-2026-08-26T03-40-31-061Z.json`, QA는 `.artifacts/m5-qa/g5-integration-014bd987464d39674a5f531765051f60e63b5ecb-2026-08-26T03-45-43Z.json`에 있다.
- **현행 반영**: 프로젝트 역할 설정 CAS와 채팅별 불변 role snapshot, 여러 영속 채팅, 항상 새로 만드는 채팅별 standalone snapshot과 exact prepare/bind/abandon, V3 네 ID workspace/save, 공개 팀 타임라인, fixture 실행의 clean/uncertain 취소·프로세스 트리 복구·poisoned lock·prepareQuit blocker, Electron/preload/UI 경계를 `architecture.md`와 M5 제안서·색인·루트 작업 규칙에 반영했다.
- **검증 범위**: 독립 QA는 계약/UI 22개, 임시 Git 저장소를 쓴 sidecar stdio 7개, Rust 회귀 20개, Electron 29개, Chromium Monaco와 macOS arm64 package smoke를 통과했다. 실제 Claude Code/Codex·인증·사용자 프로젝트와 Windows는 실행하지 않았다.
- **다음 할 일**: 문서 독립 검토와 최종 게이트 뒤 실제 agent adapter 계약, G6 보안·패키징·성능, Windows 안전 게이트를 순서대로 진행한다. push와 `main` 병합은 하지 않는다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 하네스 연결)

- **한 일**: `m5:test:run-recovery`를 추가해 G5 계약, chat-bound workspace preparation/addressing, Electron bridge/scaffold, renderer chat/editor 상태와 Rust sidecar unit·실제 stdio process 검증을 한 명령으로 연결했다. 기존 누적 게이트가 같은 현재 코드의 TypeScript/renderer build·Chromium smoke·Electron package smoke를 이미 실행하므로 G5 명령은 고유 검증을 중복 없이 조합한다. Chromium smoke의 가짜 preload bridge도 V3로 올리고 workspace/chat 구독과 프로젝트 역할·빈 채팅 조회를 제공해 실제 renderer mount 흐름과 맞췄다.
- **하네스 결정**: `pnpm harness:list`에서 G5는 `READY`, 아직 구현되지 않은 G6는 `NOT_READY`로 표시한다. 하네스 자기 검사는 G5 누적 실행 중 재귀하지 않도록 다음 미구현 G6의 exit 2와 세 누락 script를 검증한다.
- **검증**: 의존성 설치를 선행하지 않는 동등한 `CI=true npm run m5:test:run-recovery`에서 Node 51개, sidecar unit 51개 통과·1개 의도한 ignored, 실제 stdio 7개가 통과했다. root의 고정 Vite binary로 renderer 605 module build를 통과한 뒤 승인된 Electron 실행에서 수정된 Chromium Monaco mount/worker smoke 1개도 통과했다. `node scripts/m5-harness.mjs --list`는 G5 `READY`·G6 `NOT_READY`, focused 하네스 자기 테스트 2개와 staged/unstaged `git diff --check`도 통과했다. 실제 유료 agent·사용자 프로젝트·Windows·파괴적 시험은 실행하지 않았다.
- **다음 할 일**: 최신 통합 commit에서 G5 누적 하네스와 package smoke를 실행하고 독립 QA한다. G6 security/package/performance script는 해당 구현 파동에서 별도로 추가한다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 채팅 준비 workspace Rust 소비)

- **한 일**: 승인된 additive `workspace.prepareChat`·`workspace.abandonPrepared`와 `preparationId`가 추가된 `chat.create` 계약을 Rust sidecar에 소비했다. prepare는 기존 active/bound snapshot과 무관하게 새 standalone snapshot과 전역 고유 preparation/worktree/workspace identity를 만들고 `unbound`로 저장한다. create는 exact preparation·active manifest·settings revision·전역 chat/workspace binding을 SQLite `BEGIN IMMEDIATE` 안에서 다시 확인한 뒤 preparation `bound(chatId)`·draft chat·최초 사용자 메시지를 함께 commit한다. G4 `project.createWorktree`의 기존 idempotent 경로는 유지했다.
- **안전 결정**: exact 네 ID가 다른 abandon, bound·unknown·이미 abandoned preparation은 다른 workspace를 바꾸지 않고 거부한다. 성공 abandon은 물리 삭제 없이 manifest와 preparation을 terminal `abandoned`로 만들며, filesystem 전이를 먼저 수행해 이후 SQLite 저장이 실패해도 chat binding이나 G4 작업에 재사용되지 않게 했다. 저장 전후 실패를 위한 exact 재개는 같은 abandoned filesystem identity만 허용한다. prepare 저장 실패도 새 snapshot을 terminal 처리하고, terminal 상태를 증명하지 못하면 outcome-unknown recovery로 승격한다. chat ID 충돌과 binding 도중 crash 주입은 transaction 전체를 rollback해 preparation을 cleanup 가능한 `unbound`로 유지한다.
- **호환·발견**: G5 workspace 작업은 chat/project/worktree/workspace 네 ID와 외부 manifest·marker·lock을 계속 매번 검증한다. 실제 stdio A/B 격리 시험 중 root tree의 유효한 빈 `parentPath`를 공용 파서가 거부하던 오류를 발견해 G4/G5 tree route에서만 빈 문자열을 허용했다. bound A를 유지하면서 fresh B 생성 실패·mismatch 거부·exact/duplicate abandon 뒤 A tree/read/save가 계속 동작하고 원본 Git 저장소는 변하지 않음을 임시 저장소로 검증했다.
- **검증**: `cargo test -p hello-agent-sidecar --locked`는 unit 51 통과·의도한 1 ignored와 실제 stdio 7 통과, `cargo test -p hello-agent-protocol --locked` 22 통과, M4 process-tree F1~F8 회귀 17 통과, Tauri handoff 53 통과·실제 유료 agent 3 ignored를 확인했다. `cargo clippy -p hello-agent-sidecar --locked --no-deps -- -D warnings`, owned Rust 2021 rustfmt, `git diff --check`도 통과했다. Windows/비 macOS spawn은 계약대로 차단 경로만 유지했으며 실제 Windows·유료 agent·사용자 프로젝트는 실행하지 않았다.
- **다음 할 일**: 이 Rust 후속 commit을 독립 코드 리뷰하고 최신 통합 commit에서 G5 누적 하네스와 실제 stdio QA를 재실행한다. Electron/UI는 exact preparation 결과를 받은 뒤에만 chat.create를 호출하고 실패한 unbound preparation을 정확한 네 ID로 명시 폐기해야 한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅 workspace preparation Electron 소비자)

- **한 일**: 새 `workspace.prepareChat`/`workspace.abandonPrepared` capability를 고정 V3 IPC channel, preload exact validator, main handler, SidecarClient response validator에 연결했다. `createChat`은 이제 `preparationId`와 project/worktree/workspace 세 필드를 모두 포함하고, 현재 client가 정확히 알고 있는 unbound preparation에만 결합한다.
- **안전 결정**: prepare 응답은 새 unbound standalone snapshot 전체 레코드여야 하며 project ID를 exact echo한다. abandon은 준비 ID와 세 workspace ID를 모두 정확히 echo하고 known unbound preparation에만 보낸다. cross-preparation·unknown·late·bound mismatch에는 재시도·projectId close·fallback이 없다. main/window 종료는 명시적 UI 요청 없이 자동 abandon하지 않아 sidecar의 증거를 보존한다.
- **검증**: root 의존성을 재사용한 `tsc --noEmit`, `node --test --experimental-strip-types tests/m5-electron-bridge.test.mts tests/m5-electron-scaffold.test.mts` 29개, `git diff --check`를 통과했다. extra/project-only·UTF-8 ID overflow, preparation A/B 교차, abandon response mismatch, createChat preparation mismatch를 회귀로 추가했다.
- **다음 할 일**: 이 bridge consumer commit을 독립 재검토하고, Rust sidecar 및 renderer 준비 흐름과 통합해 실제 preparation→chat bind→명시적 abandon QA를 수행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅 ID 전역 충돌 리뷰 수정)

- **한 일**: 독립 리뷰 P1을 반영해 chat 생성 후보의 `chatId`도 완료된 전역 binding scan의 모든 기존 chat ID와 비교하도록 TypeScript/Rust `ChatCreationTranscriptV1` validator를 강화했다. fresh preparation이 별도 worktree/workspace를 사용해도 후보 chat ID가 기존 A와 같으면 `bindingConflict`다.
- **안전 결정**: 충돌한 `createdChat`은 커밋 전 후보 증거로만 transcript에 남을 수 있다. 이 경우 transaction은 반드시 미커밋, 최초 사용자 메시지는 없음, preparation은 exact unbound 상태 그대로여야 하며, 성공 또는 binding/write를 주장하면 fail-closed 거부한다.
- **검증**: 공용 fixture가 기존 chat A + fresh preparation B + 생성 후보 chat ID A를 성공으로 주장하는 경우를 거부하고, 같은 충돌을 no-write/no-bind/cleanup-eligible `bindingConflict`로 판정하는 경우만 승인한다. focused Node 10개, focused Rust 5개, 전체 `npm test` 106개, `cargo test -p hello-agent-protocol --locked` 22개, `tsc --noEmit`, `cargo check -p hello-agent-sidecar --locked`, rustfmt/diff 검사를 통과했다.
- **다음 할 일**: 수정 commit을 재검토한 뒤 통합하고, sidecar의 chat ID 생성·전역 uniqueness 확인·insert를 한 transaction 안에서 수행해 같은 판정을 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅별 독립 작업공간 준비 계약)

- **한 일**: 기존 V2와 workspace-addressed V3 API를 변경하지 않고 `workspace.prepareChat`·`workspace.abandonPrepared`와 bridge 메서드를 additive하게 추가했다. prepare는 같은 프로젝트의 active/bound snapshot 존재 여부와 무관하게 전역적으로 고유한 `preparationId/worktreeId/workspaceIdentity`를 가진 새 standalone snapshot을 `unbound`로 저장하며, chat.create는 그 정확한 준비 레코드를 settings·최초 사용자 메시지와 한 transaction에서 `bound(chatId)`로 바꾼다.
- **안전 결정**: abandon은 preparation/project/worktree/workspace 네 ID와 저장 증거가 정확히 일치하는 아직 unbound인 준비만 `abandoned`로 표시하고 물리 삭제하지 않는다. bound·unknown·이미 abandoned·cross-identity 요청은 다른 준비를 변경하지 않는 no-op이며, settings/binding 실패도 해당 준비를 그대로 cleanup-eligible 상태로 남긴다. `ChatRecordV1`에는 preparationId를 중복 저장하지 않고 persisted preparation의 `boundChatId`를 immutable 생성 증거로 사용한다.
- **검증**: 공통 fixture가 bound A가 있어도 B를 새 identity로 준비하고, B 생성 실패 뒤 B만 폐기하며 A는 active인 흐름, 반복 prepare identity 충돌, cross-project/triple/preparation mismatch, bound/late/unknown/duplicate abandon을 TypeScript와 Rust에서 같은 결과로 판정한다. focused Node 18개, 전체 `npm test` 106개, `cargo test -p hello-agent-protocol --locked` 22개, `tsc --noEmit`, `cargo check -p hello-agent-sidecar --locked`, owned Rust rustfmt와 `git diff --check`를 통과했다.
- **다음 할 일**: 이 계약 commit을 독립 리뷰한 뒤 통합하고, sidecar가 새 snapshot 생성과 준비 레코드 저장, chat/settings/message binding, 정확한 준비 abandon을 각각 fail-closed transaction으로 구현한다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 Rust 실행·복구 및 다중 workspace 내부)

- **한 일**: 승인된 G5 run/recovery·workspace 계약을 Rust sidecar에 소비해 프로젝트별 revision CAS 역할 설정과 채팅 생성 시 역할 snapshot, bounded 영속 chat ledger, chat-bound workspace tree/read/save/watch/diff/close를 구현했다. G4 projectId-only 경로는 호환 경로로 유지하고 G5 작업은 저장된 전체 chat binding과 외부 active manifest·marker·lock을 매 요청마다 exact 검증한다. 여러 standalone snapshot과 chat이 동시에 살아 있어도 현재 선택을 추론하지 않으며 cross-chat ref와 saveId continuation은 원본 쓰기 전에 거부한다.
- **실행·복구 결정**: macOS 결정적 self-fixture만 허용하고 실제 Claude/Codex·인증·사용자 프로젝트는 실행하지 않았다. 역할 실행은 chat/workspace별 직렬화하며 coding은 run+writer lock을 spawn 전에 SQLite `BEGIN IMMEDIATE`로 함께 저장하고 planning/review는 writer lock 없이 실행한다. fixture child는 부모가 process identity를 저장하기 전 자손을 만들지 못하도록 stdin release barrier를 두고, 취소는 M4 `TreeTracker`가 관찰한 부모·자손 전체 소멸을 증명한 경우만 clean 처리한다. 불확실하면 chat/run/manifest/writer lock/recovery를 fail-closed로 남기고, 모든 영속 identity가 Gone인 abandon만 허용한다. `prepareQuit`은 active run과 recovery blocker를 정리·보고하며 unsafe 상태에서는 복구 조회/abandon 뒤 재시도할 수 있고, clean checkpoint 전에는 EOF 종료를 성공으로 보지 않는다.
- **저장 안전**: 상수 크기 projection event와 durable event sequence를 wire stream sequence와 분리했다. workspace save begin은 전체 chat/workspace ref를 durable session에 고정하고, commit 결과 뒤 session cleanup 저장이 실패하면 outcome-unknown으로 승격해 자동 재시도를 막는다. 복구 wire payload에서는 내부 마이크로초 process identity 증거를 제거하되 durable projection에는 정확한 probe 값을 보존한다.
- **검증**: 임시 Git 저장소만 사용했다. `cargo test -p hello-agent-sidecar --locked`는 unit 48 통과·의도한 1 ignored와 stdio 6 통과, `cargo test -p hello-agent-protocol --locked`는 21 통과, `cargo clippy -p hello-agent-sidecar --locked --no-deps -- -D warnings`, scoped Rust 2021 format, `git diff --check`를 통과했다. M4 process-tree F1~F8 회귀 17개와 Tauri handoff 53개 통과·실제 유료 agent 3개 ignored도 통과했다.
- **다음 할 일**: 새 chat 전용 preparation 계약 commit을 통합 기준선에 먼저 반영한 뒤 이 branch를 그 exact base로 전진하고, `workspace.prepareChat`·exact `workspace.abandonPrepared`와 preparation을 원자 소비하는 amended `chat.create`를 별도 후속 단위로 구현·검토한다. 이 커밋은 그 미통합 wire를 광고하거나 구현하지 않는다.
### 2026-08-26 · by GPT-5 Codex (M5 G5 Electron Bridge V3 독립 리뷰 수정)

- **한 일**: 독립 리뷰의 차단 사항을 반영했다. recovery blocker가 있는 `ready` status를 유효한 live 상태로 유지해 recovery API 호출을 허용하고, `safeToQuit: false`는 종료 허용으로 바꾸지 않는다.
- **이벤트·ID 결정**: workspace/chat/run protocol event는 generic V2 route에서 제외해 renderer credit가 한 실제 event에 한 번만 소비된다. raw `run.event`·`run.finished` payload는 renderer로 전달하지 않으며, G5 public chat은 검증된 `TeamMessage` 전용 route만 쓴다. bridge-local project/recovery ID 검증도 문자열 길이가 아니라 UTF-8 512 bytes를 적용한다.
- **검증**: root 의존성을 재사용한 `tsc --noEmit`, `node --test --experimental-strip-types tests/m5-electron-bridge.test.mts tests/m5-electron-scaffold.test.mts` 27개, `git diff --check`를 통과했다. recovery-ready 연결성, unsafe quit, four-stream 단일 route, hidden raw run filtering, ASCII/emoji byte-limit pre-IPC 거부를 추가로 검증했다.
- **다음 할 일**: 이 수정 commit을 같은 exact SHA로 재검토하고, G5 Rust sidecar/renderer와 통합한 뒤 package smoke와 누적 QA를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 Electron Bridge V3)

- **한 일**: Electron main/preload와 `SidecarClientV1`이 additive `M5BridgeV3`를 소비하도록 구현했다. workspace tree/close/read/save/watch/diff, 프로젝트 역할 설정, 채팅 페이지, fixture run/cancel, recovery API는 모두 고정 IPC channel·main-frame/app origin 검사·preload exact input validator를 거친다. V2 API와 raw sidecar restart 정책은 유지했다.
- **안전 결정**: G5 ready capability는 canonical 순서를 정확히 비교한다. workspace 응답·event·save continuation은 `chatId/projectId/worktreeId/workspaceIdentity`를 매번 echo하고, begin 뒤 `saveId`를 그 네 ID에 등록한다. cross-chat/unknown/late save와 잘못된 response는 sidecar에 재전송하지 않고 거부한다. 공개 chat stream은 구조화된 `TeamMessage`만 전달하며 workspace/chat event는 별도 bounded-credit stream으로 ACK한다.
- **검증**: root 의존성을 재사용한 `tsc --noEmit`, `node --test --experimental-strip-types tests/m5-electron-bridge.test.mts tests/m5-electron-scaffold.test.mts` 25개, `git diff --check`를 통과했다. G5 package smoke expectation도 canonical fixture로 올렸지만 이 branch에는 G5 Rust sidecar가 아직 없으므로 실제 package smoke는 통합 뒤 실행해야 한다.
- **다음 할 일**: Rust G5 sidecar와 renderer 소비자를 같은 V3 계약으로 통합하고, 최신 통합 commit에서 run/recovery 및 package smoke QA를 실행한다. 실제/유료 agent 실행은 하지 않았다.
### 2026-08-26 · by GPT-5 Codex (M5 G5 renderer bind·폐기 review 수정)

- **한 일**: B chat workspace bind를 tree → diff → watch 순차 준비로 바꿔, 세 단계가 모두 성공하기 전 B editor ref/tree/tabs를 publish하지 않도록 했다. 어느 단계든 실패하면 epoch을 무효화하고 A editor snapshot/ref를 복원한 채 action gate를 유지한다. discarded chat도 recovery와 같은 위험 동작 차단 대상으로 두며, pending switch event는 detail hydration 전 raw buffer에 보관해 detail과 merge할 때만 event sequence를 판정한다.
- **검증**: B tree bind 실패가 A ref/tabs를 보존하고 B diff/watch RPC를 시작하지 않는 focused regression을 추가했다. focused chat/workspace editor Node와 TypeScript `tsc --noEmit`, `git diff --check`를 통과했다.
- **다음 할 일**: independent reviewer가 deferred detail/event hydration과 watch-failure restore 경로를 재확인하고 Electron 통합 뒤 Chromium smoke를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 새 채팅 준비 API renderer 연결)

- **한 일**: 승인된 `workspace.prepareChat` → `chat.create(preparationId, projectId, worktreeId, workspaceIdentity, goal, expectedSettingsRevision)` 계약을 renderer에 연결했다. 매 새 chat은 bridge가 만든 독립 prepared workspace의 exact ID를 쓰며, `chat.create` 실패 시에만 그 준비 작업의 exact four-ID를 `workspace.abandonPrepared`에 전달한다. 기존 V2 `createProjectWorktree`/`project.close`는 새 chat 흐름에서 호출하지 않는다.
- **검증**: focused chat/workspace editor Node 7 PASS에서 A/B의 서로 다른 workspace identity·preparationId, B create 실패의 B-only abandon과 A 지속 사용을 확인했다. TypeScript `tsc --noEmit`과 `git diff --check`도 통과했다.
- **다음 할 일**: 독립 renderer review와 Electron V3 preparation bridge 통합 후 Chromium smoke·G5 누적 harness를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 renderer review 수정)

- **한 일**: 독립 UI review의 P1을 반영했다. chat hydration/bind 동안 `switching`을 먼저 공개하고, 기존 A 화면은 유지한 채 모든 editor/run action을 막으며 B의 detail·timeline은 exact editor bind가 성공한 뒤에만 한 번에 publish한다. hydration 중 수신한 event는 durable `eventSequence`로 merge/dedupe하며 충돌·gap은 fail-closed 오류가 된다. cancel 늦은 결과도 chatId/runId/generation/switching을 재검증해 다른 활성 chat을 바꾸지 않는다. recovery 상태는 editor save/close/abandon과 run/switch를 UI/controller 수준에서 RPC 전 차단한다.
- **새 채팅 결정**: 안전한 새 snapshot 준비 API가 아직 통합되지 않았으므로 renderer는 기존 V2 project snapshot 재사용 또는 `project.close` cleanup을 하지 않고 `preparationApiUnavailable` 안내와 비활성화 버튼을 표시한다.
- **검증**: focused chat/workspace editor Node 5 PASS, TypeScript `tsc --noEmit` PASS. 다음은 deferred switch/cancel/hydration/recovery regression을 보강하고 Electron 통합 뒤 Chromium smoke를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 V3 채팅 작업 복사본 Monaco 포트)

- **한 일**: `ProjectEditorController`가 채팅 binding 뒤 immutable `ChatWorkspaceRefV1 { chatId, projectId, worktreeId, workspaceIdentity }`만 사용하도록 포트했다. tree/read/save begin·append·commit/watch/diff/close와 blob/watch event는 각각의 V3 workspace API로 전환되며, V2 project API는 채팅 생성 전 picker·snapshot 준비에만 남는다. clean chat switch는 새 workspace를 다시 load하고, dirty/conflicted 탭은 switch·새 chat을 막으며, late response/event는 epoch과 exact four-ID 비교를 통과해야만 surface에 반영된다.
- **검증**: V3 editor/chat focused Node 5 PASS, 기존 editor 포함 focused Node 30 PASS, root 설치 TypeScript `tsc --noEmit -p tsconfig.json` PASS, `git diff --check` PASS. Chromium smoke와 worktree `pnpm build`는 로컬 의존성 부재 및 network DNS `ENOTFOUND` 때문에 이 worktree에서 실행하지 못했다.
- **다음 할 일**: Electron preload/main V3 port와 통합해 실제 renderer Chromium smoke·G5 cumulative harness를 실행하고, 독립 UI review에서 save session와 dispose/race 경계를 재확인한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅·역할 설정 renderer)

- **한 일**: V3 bridge만 소비하는 renderer 채팅 상태 컨트롤러와 작업대 UI를 추가했다. 프로젝트별 기획·코딩·검토 역할의 runtime/model/reasoning/CAS 설정, 기존 채팅의 불변 role snapshot 안내, 영속 채팅 목록·전환, 새 채팅마다 별도 standalone snapshot triple을 만든 뒤 exact revision으로 `chat.create` 하는 흐름, 공개 팀 타임라인과 명시적 `가짜 에이전트 데모` 시작·취소 UI를 구현했다. 교차 채팅 triple event는 renderer에서 다시 거부하고, recoverable 상태는 실행과 위험 동작을 막고 명시적 abandon confirmation만 보인다.
- **중요한 경계**: 이 UI는 실제 모델을 설치·로그인·실행하거나 chain-of-thought/raw stderr를 표시하지 않는다. 모델을 사용할 수 없을 때는 자동 fallback하지 않으며, 런타임 V3 workspace-addressed 파일 저장/닫기 연결은 Electron bridge 포트와 함께 완성해야 한다.
- **검증**: `node --test --experimental-strip-types tests/m5-g5-chat-ui-state.test.mts` 3 PASS, 루트 설치 TypeScript를 이용한 `tsc --noEmit -p tsconfig.json` PASS, `git diff --check` PASS. worktree에 `node_modules`가 없어 `pnpm build`는 network DNS `ENOTFOUND` 의존성 다운로드에서 중단됐다.
- **다음 할 일**: 독립 UI 리뷰에서 V3-only action·recovery/dirty-switch 경계를 확인하고, Electron V3 bridge와 workspace-addressed Monaco controller를 통합한 뒤 renderer Chromium smoke와 G5 누적 harness를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 protocol envelope 호환·binding 증거 강화)

- **한 일**: 최종 리뷰 P1에 따라 protocol v1 `EventV1`의 Rust/TypeScript 상위 필드를 기존 `chatId`·`taskId`·`runId`까지만 유지하고, G5 workspace event의 `ChatWorkspaceRefV1` 네 ID는 전부 event `payload` 안으로 이동했다. canonical JSONL이 payload 주소를 byte-match하고, 기존 G4 sidecar의 `EventV1` 초기화 코드를 수정하지 않은 채 소비자 빌드 호환을 증명한다.
- **binding 결정**: `WorkspaceResolvedBindingV1`의 요청 유래 `storedBinding` echo를 제거하고, run/recovery validator를 통과한 전체 persisted `ChatRecordV1`과 verified active standalone snapshot을 증거로 요구한다. 모든 operation/save/event route는 저장 chat의 `chatId/projectId/worktreeId/workspaceIdentity`와 요청·snapshot을 exact 비교하며, 다른 저장 chat을 가짜 binding으로 제시하는 공통 TS/Rust fixture를 거부한다.
- **검증**: focused G5/workspace/protocol Node 14개, 전체 `npm test` 102개, `cargo test -p hello-agent-protocol --locked` 21개, `tsc --noEmit`, owned Rust rustfmt/diff 검사를 통과했다. 또한 `cargo check -p hello-agent-sidecar --locked`와 `cargo test -p hello-agent-sidecar --locked`(unit 43 통과·1 ignored, stdio 3 통과)로 기존 소비자 호환을 확인했다.
- **다음 할 일**: 수정 exact SHA를 독립 재검토한 뒤 통합하고, 이후 sidecar G5 소비자는 DB에서 읽은 immutable chat record와 active manifest를 workspace 작업 시작 전에 결합해 같은 validator 순서를 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 workspace chat/save 상관관계 리뷰 수정)

- **한 일**: 독립 리뷰 P1 두 건을 반영해 V3 workspace 요청·응답·descriptor·event의 주소를 `ChatWorkspaceRefV1 { chatId, projectId, worktreeId, workspaceIdentity }`로 강화했다. 각 tree/close/read/watch/diff 작업은 sidecar에 저장된 chat binding과 verified active standalone snapshot을 정확히 resolve한 뒤에만 허용하며, createChat으로 고정된 chat/workspace 결합과 같은 네 ID를 사용한다. V2 메서드·타입은 변경하지 않았다.
- **save 결정**: begin 시 persisted save session에 네 ID와 `saveId`·path·expected version·byte/chunk 조건을 함께 고정한다. append와 commit 요청·응답도 네 ID를 반복하고 `saveId`로 찾은 정확한 persisted session과 일치해야 한다. `saveId`만 있는 V3 continuation은 허용하지 않으며 unknown/종료된 session의 늦은 결과도 fail-closed다.
- **검증**: 공통 fixture가 chat A + workspace B triple을 tree/close/read/watch/diff에서 거부하고, 동시 save A/B에서 A `saveId` + B ref append·commit, unknown/late response·event를 양언어에서 같은 결과로 거부한다. focused Node 14개, 전체 `npm test` 102개, `cargo test -p hello-agent-protocol` 21개, `tsc --noEmit`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다.
- **다음 할 일**: 수정 commit을 동일 exact SHA로 독립 재검토한 뒤 통합하며, sidecar/Electron 소비자는 stored chat binding·active snapshot·persisted save session 조회를 IPC 처리 전에 같은 순서로 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 workspace 명시 주소 계약)

- **한 일**: 기존 projectId-only `M5BridgeV2`를 변경하지 않고 `M5BridgeV3`에 workspace 전용 tree/close/file read·save·watch/diff/event API를 별도 추가했다. 모든 요청과 상관관계가 있는 응답·descriptor·event는 `WorkspaceRefV1 { projectId, worktreeId, workspaceIdentity }`를 명시적으로 전달하고 exact echo하며, save chunk·commit은 begin에서 서버에 결합된 `saveId`로만 이어지도록 TypeScript/Rust validator를 고정했다.
- **호환·안전 결정**: protocol framing/schema는 v1을 유지하고 G5 capability/event allowlist만 additive 확장했다. 기존 V2 메서드 순서·타입과 canonical project.getTree JSONL bytes는 공통 fixture에서 그대로 보존한다. chat·verified active standalone snapshot·workspace API와 event route는 같은 triple이어야 하며 cross-chat worktree, workspace alias, 잘못된 response/path/event/save 상관관계를 fail-closed 거부한다. 선택된 프로젝트·채팅 같은 암시적 전역 상태는 V3 계약에 두지 않는다.
- **검증**: 공통 `g5-workspace-v1` fixture를 TypeScript와 Rust가 같은 결과로 판정한다. focused Node 4개, 전체 `npm test` 102개, `cargo test -p hello-agent-protocol` 21개, root 설치를 재사용한 `tsc --noEmit`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다. 실제 sidecar/Electron/UI 소비자와 사용자 프로젝트·유료 에이전트는 실행하지 않았다.
- **다음 할 일**: 이 계약 commit을 독립 리뷰한 뒤 통합하고, sidecar와 Electron/preload 구현이 모든 workspace 응답·stream·event에서 같은 triple/saveId 상관관계를 검증하도록 소비한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅 상태·단일 run 직렬화 계약)

- **한 일**: 마지막 독립 재검토 P1을 반영해 `stopping`을 포함한 명시적 `ChatTransitionV1`을 Rust/TypeScript 양쪽에 추가했다. 전이는 identity·goal·settings revision·role snapshot·createdAt을 보존하고 updatedAt이 단조 증가하며, 완료·취소·폐기·복구 필요 상태는 일반 전이에서 terminal이다. 취소 transcript도 같은 전이 계약을 통과해야 하며 복구 abandon만 전용 transcript로 terminal chat/workspace를 바꾼다.
- **실행 결정**: G5 run 시작 source chat은 `draft|running|waitingForUser|needsAttention`만 허용한다. 그 외 상태는 `chatStateBlocked`, 동일 chat/worktree/workspace의 `preparing|running|cancelling` run이 있으면 역할과 writer 여부에 관계없이 `runActive`이며 persist·lock·spawn 전에 거부한다. 판정 우선순위는 platform → abandoned workspace → recovery → chat state → active run → profile → writer lock → started로 manifest에 고정했다.
- **검증**: 공통 fixture가 completed/canceled/discarded/reviewReady 시작 거부, active read-only와 active writer 직렬화, terminal 이전 run 뒤 needsAttention 재시도, stopping을 거친 clean cancel, 불변 identity/settings와 timestamp 역행 거부를 양언어에서 같은 결과로 판정한다. focused G5 Node 6개, 전체 `npm test` 98개, Rust protocol 19개, `npx tsc --noEmit`, owned Rust rustfmt check와 `git diff --check`를 통과했다.
- **다음 할 일**: 이 상태 계약 수정 commit을 독립 재검토한 뒤 통합하고, sidecar가 start transaction에서 같은 우선순위와 단일 active run 검사를 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 계약 독립 리뷰 수정)

- **한 일**: G5 독립 리뷰의 P1 4건·P2 1건을 반영해 chat 생성의 verified active standalone snapshot과 전역 1:1 binding scan, 일반 run 시작의 platform·exact profile resolution·recovery 차단, cancel/termination/recovery 원자 transcript, abandon 뒤 terminal workspace/chat 상태를 Rust와 TypeScript 계약 및 공통 fixture에 추가했다. `planning`·`review` read-only fixture 실행은 writer lock 없이 허용하고 coding writer만 worktree ID와 workspace identity를 함께 잠근다.
- **안전 결정**: Windows fixture 실행은 persist·lock·spawn 전에 거부하며 이 게이트의 결정적 fake 실행은 macOS만 허용한다. unavailable profile·default fallback·chat snapshot drift는 어떤 실행 기록도 만들지 않는다. 불확실한 취소는 chat/run/workspace lock/unresolved recovery를 함께 `recoveryRequired`로 남기고 이후 읽기/쓰기 역할의 모든 run을 막는다. 모든 process identity 소멸을 증명한 abandon 뒤 기존 snapshot/workspace/chat은 재사용할 수 없다. G5 ID 상한은 양 언어 모두 문자열 문자 수가 아니라 UTF-8 512 bytes다.
- **검증**: 공통 valid/invalid/derived fixture에 Windows 선행 거부, exact profile drift/fallback, 전역 binding cursor·alias·page 경계, cancel-uncertain → recoveryRequired → read-only start 거부 → process proof 뒤 abandon, abandoned workspace 재사용 거부를 추가했다. `npm test` 98개, `cargo test -p hello-agent-protocol` 19개, `npx tsc --noEmit`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다. 실제/유료 에이전트·사용자 프로젝트·Windows 실행은 수행하지 않았다.
- **다음 할 일**: 수정 commit을 독립 재검토한 뒤 통합 담당이 승인된 G5 계약을 `m5/coding-agent-ide`에 통합하고, sidecar/Electron/UI 소비자는 이 fail-closed transcript를 그대로 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 실행·복구 계약 기준선)

- **한 일**: protocol framing·schema v1과 G4 `M5BridgeV2`를 깨지 않고 프로젝트 역할 설정, 영속 채팅, 결정적 fixture 실행·취소, 복구 조회·폐기를 더한 additive `M5BridgeV3`와 Rust/TypeScript runtime validator를 작성했다. 같은 `g5-v1` fixture가 역할/모델 snapshot, 명시적 project/worktree/workspace 신원, 공개 팀 메시지, run 상태·종료 증거, durable event 순서, page 상한을 양쪽에서 검증한다.
- **안전 결정**: 새 채팅은 프로젝트 설정 revision과 정확한 세 역할 profile을 생성 시 고정하며 채팅별 override·silent model fallback을 허용하지 않는다. writer 시작은 run과 workspace lock을 한 transaction으로 commit한 뒤에만 spawn하고 동일 worktree의 두 번째 writer를 거부한다. 취소·종료가 불확실하면 lock을 유지한 `recoveryRequired`이며, G5 복구 해결은 모든 추적 process identity의 소멸이 증명된 뒤 `abandon`만 허용한다. 공개 `TeamMessage`는 결과·요청·상태만 저장하고 hidden reasoning field는 fail-closed다. `run.startAgentSmoke`는 protocol 예약만 유지하고 G5 capability와 bridge에는 광고하지 않으며 Windows 실행과 실제/유료 에이전트 실행은 비활성이다.
- **검증**: `npm test` 98개(그중 focused G4/G5/protocol 계약 16개)와 `cargo test -p hello-agent-protocol` 19개, `npx tsc --noEmit --pretty false`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 수행하지 않았다.
- **다음 할 일**: 독립 계약 리뷰 뒤 이 기준선을 통합하고, Rust sidecar와 Electron/preload·renderer 구현을 서로 다른 worktree에서 고정된 V3 계약에 맞춰 진행한다.

## 프로젝트 요약 (고정)

터미널을 한 번도 안 열어본 비개발자를 **코딩 에이전트(Claude Code·Codex)가 굴러가는 상태까지** 데려다주는 데스크톱 GUI 앱. 현재 배포 경로는 Tauri 2 + React 19이고, M5는 Electron + React/TypeScript + Monaco와 Rust sidecar의 별도 작업대 골격으로 전환 중이다.

- 기획 원본: `~/Library/CloudStorage/OneDrive-개인/ObsidianVault/오픈소스/아이디어/후보/에이전트-스타터.md`
- 레포: https://github.com/wonseok-han/hello-agent (로컬 폴더도 `~/projects/hello-agent`로 변경 완료 — 2026-08-12 확인)
- 위저드 6단계: 에이전트 선택 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식
- 주요 모듈: 기존 Tauri `src-tauri/src/`, 공용 Rust `crates/hello-agent-{core,protocol,sidecar}`, M5 셸 `electron/`과 작업대 `src/m5/`

## 핵심 결정·발견 (고정 — 재조사 방지)

- **마일스톤**: M0~M3, 홈베이스·프로젝트 디스크 스캔·에이전트 상태/업데이트, 주요 실패 경로 닥터, 한·영 콘텐츠까지 구현. 남음: 실제 실패 경로와 macOS/Windows 출시 후보 실기기 검증, 정식 배포(코드 서명)
- **PATH**: macOS/Windows 인스톨러 둘 다 PATH를 스스로 등록 안 함 → `ensure_path`가 직접(`~/.zshrc` / Windows 사용자 PATH 레지스트리). 위저드는 절대경로 실행이라 PATH 비의존
- **로그인 자동 감지**: 브라우저 세션이 있으면 확인 코드 없이 자동 승인으로 끝나는 경로가 있어, 대기 중 `auth status` 폴링으로 완료를 잡는다
- **설치 방식**: 클로드 = 공식 install.sh/ps1, 코덱스 = GitHub 릴리즈 tar.gz
- **Codex 데스크톱 앱**: CLI가 앱 번들 `Contents/Resources/codex`에 번들(셸 PATH에 없음). 앱 로그인 자격증명을 CLI가 공유 → 감지되면 로그인도 건너뜀. **벤더가 앱을 옮김**: OpenAI가 Codex.app을 ChatGPT.app으로 통합(2026-07) → `/Applications/{ChatGPT,Codex}.app/...` 여러 후보로 감지(단일 경로 하드코딩 금지). 앞으로도 바뀔 수 있으니 후보 추가로 대응
- **편집기 확장 ID**: Claude Code = `anthropic.claude-code`, Codex = `openai.chatgpt`. 편집기 CLI는 앱 번들 `Contents/Resources/app/bin/{cursor,code}`
- **Windows PowerShell 함정**: 부모가 pwsh 7이면 `PSModulePath` 오염으로 5.1이 기본 cmdlet을 못 찾음 → 자식 실행 시 `PSModulePath` 제거
- **코드 서명 미결정**: 베타는 미서명 + "확인 없이 열기" 안내, 정식은 서명·공증 필요(연 $250~400). 미서명이어도 설치는 가능(사용자가 보안 허용 한 번)
- **스코프**: Codex는 원래 2단계였으나 조기 편입(2026-07-16). Gemini 등 추가는 여전히 2단계
- **UI**: Pretendard + 중성 배경 + 코랄 액센트. 갈색 종이 콘셉트는 사용자가 반려함
- **소개 웹사이트**: 데스크톱 앱과 배포 단위를 분리해 `website/`에서 관리. Vinext + OpenAI Sites 기반의 한국어 정적 랜딩 페이지. 온보딩 6단계뿐 아니라 프로젝트 자동 발견·에이전트 상태·업데이트를 관리하는 재방문 홈베이스도 핵심 가치로 소개
- **브랜드 마크**: `brand/hello-agent-mark.svg`가 정식 원본. 코랄 타일 + 흰 대화선 형태이며 웹 파비콘과 Tauri macOS·Windows 앱 아이콘을 동일 원본에서 생성
- **i18n**: 라이브러리 없이 경량 자체 방식(`src/i18n.tsx` Context/훅 + `src/locales/{ko,en}.ts`). 헤더 언어 토글, 시스템 언어 초기값 + localStorage. 이름 `Hello, Agent`는 언어 무관 고정, UI 문구만 전환. 새 문구는 ko/en 양쪽에 키를 추가해야 함(en은 `Record<MessageKey,string>`으로 누락 시 컴파일 에러)
- **오류/i18n 경계**: Rust 오류는 `AppError { kind, detail }`로 구조화하고 상세 기술 오류는 영어로 통일. 초보자용 설명은 프론트 i18n에서 표시. 프로젝트 안전 안내와 첫 대화 프롬프트도 선택 언어를 Rust로 전달해 한·영 생성
- **닥터 적용 범위**: 구조화 오류를 network/checksum/notfound/permission/disk/generic으로 해석해 i18n 문구와 재시도를 제공. 진단·설치·로그인·프로젝트 생성·첫 대화와 홈의 상태/업데이트·프로젝트 스캔 실패에 적용
- **홈베이스**: 프로젝트가 있거나 설치·로그인된 에이전트가 있으면 홈으로 진입. 기준 폴더 바로 아래에서 `.claude`/`CLAUDE.md`/`AGENTS.md` 표식을 스캔하고, 에이전트 상태와 업데이트를 표시
- **릴리스 게이트**: `.github/workflows/ci.yml`에서 데스크톱 macOS·Windows 빌드/테스트/격리 설치와 소개 웹사이트 빌드·렌더·린트를 검증. 사람 확인 항목과 미서명 베타/정식 서명 기준은 `docs/release-checklist.md`를 단일 체크리스트로 사용
- **M4 파일럿 완료(2026-08-24)**: S1·S2·§7.10 구현과 교차 검토를 마쳤고, §7.6A 사용자 실사용을 통과해 **가설 A(사용자가 에이전트 사이를 직접 중계하지 않아도 된다)가 성립**했다. §7.6B 품질·비용 비교, Windows 자손 회수, 재작업 루프·다역할 같은 고비용 확장은 보류/후속이며 M4 완료 조건이 아니다. `main`에는 미병합
- **M5 설계 방향(2026-08-24, G4 격리 방식 2026-08-25 갱신)**: 기존 M0~M4는 최초 설정·튜토리얼로 유지하고, 이후 여러 영속 채팅, 프로젝트별 역할·에이전트·모델, 원본과 분리된 standalone snapshot, 명확한 diff와 실제 코드 편집을 갖춘 초보자용 agent-first IDE로 확장한다. 한 채팅은 변경 목표·작업 복사본 하나를 소유하고 역할 왕복을 한 팀 대화에 기록한다. **M5 기술 기준선은 Electron + React/TypeScript + Monaco 셸과 기존 Rust 안전 코어 sidecar**이며 Tauri와 동시 구현하지 않는다. SQLite 이벤트 원장+projection, 앱 종료 시 안전 중단, 첫 적용은 전체 작업 복사본+복원 지점, macOS 우선 실행을 기준으로 한다. 새 이름은 개발과 분리해 보류한다. 통합 브랜치는 `m5/coding-agent-ide`; `main` 병합은 전체 완료 뒤 사용자 승인 사항
- **M5 직접 유사 제품 Cebus 발견(2026-08-24)**: `cebus-ai/cebus`는 여러 AI 모델의 단일 대화, 역할별 모델 배정, 채널·스레드, CLI·웹·VS Code 표면을 이미 제공해 M5 구상과 직접 겹친다. M5 벤치마크에 포함하고, 초보자 안전·원본과 분리된 standalone snapshot·명시적 diff 검토와 복구 루프를 차별점으로 검증한다
- **멀티 에이전트 저장소 운영(2026-08-25)**: 큰 구현은 오케스트레이터 1명이 통합 브랜치를 소유하고, 최대 3명의 쓰기 구현 담당이 별도 branch/worktree와 겹치지 않는 파일 소유권으로 작업한다. 계약을 먼저 고정한 뒤 파동식으로 병렬화하며, 독립 코드 리뷰와 최신 통합 commit QA를 통과한 변경만 통합한다. Codex 프로젝트 역할·모델·sandbox는 `.codex/config.toml`과 `.codex/agents/*.toml`, 상세 실행 절차는 `docs/procedures/multi-agent-orchestration.md`가 기준이다
- **M5 기계 검증 하네스(2026-08-25)**: `scripts/m5-harness.mjs`가 로컬과 macOS/Windows CI의 단일 검증 진입점이다. `baseline`은 기존 회귀를 실제 실행하고 G1~G6는 앞 단계를 누적한다. 필수 script가 없으면 `NOT_READY`/exit 2로 실패하며, versioned JSON 증거를 `.artifacts/m5-harness/`에 남긴다. 예약 script와 판정 규칙은 `docs/procedures/multi-agent-orchestration.md` §13이 기준이다
- **M5 G5 실행·복구 계약(2026-08-26)**: chat 생성은 verified active standalone snapshot과 전역 1:1 worktree/workspace binding을 요구한다. fixture run은 macOS에서만 exact chat role snapshot으로 시작하며 planning/review는 read-only, coding만 writer lock을 갖고, 첫 버전은 동일 chat/worktree/workspace의 역할 run을 하나씩 직렬 실행한다. `stopping`을 포함한 chat 상태는 명시적 전이표를 따르며 completed/canceled/discarded/recoveryRequired는 terminal이다. `recoveryRequired`인 chat/run/workspace lock/unresolved recovery는 모든 새 run을 막고, process identity 소멸 증명 뒤 abandon한 snapshot/workspace/chat은 재사용하지 않는다. protocol framing은 v1을 유지하고 공개 bridge는 additive `M5BridgeV3`다
- **M5 G1 Rust core 추출(2026-08-25)**: 루트 Cargo workspace와 단일 `Cargo.lock`을 기준으로 삼고, `crates/hello-agent-core`에 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약을 Tauri/Electron/Node/SQLite/UI 의존 없이 옮겼다. 기존 Tauri 모듈 경로는 좁은 re-export shim으로 유지해 `handoff.rs`·`orchestration.rs` 동작을 바꾸지 않는다. `crates/hello-agent-sidecar`는 후속 protocol/SQLite 게이트 전까지 compile-only 골격이다
- **M5 커밋·통합 자동 승인(2026-08-25)**: 작업 계약 범위 안의 기능 worktree 원자적 구현·테스트·리뷰 수정 커밋과, 독립 리뷰 및 해당 하네스·QA를 통과한 commit의 `m5/coding-agent-ide` 통합은 사용자에게 매번 다시 묻지 않는다. push, `main` 병합, 범위 밖 변경, 안전 불변식·확정 결정 변경, 실제 유료 에이전트·사용자 프로젝트·파괴적 시험과 외부 게시는 계속 별도 승인 사항이다
- **M5 G1 공급망 차단(2026-08-25)**: Electron Forge stable `7.11.2`가 Git SHA 기반 `@electron/node-gyp`을 전이 의존성으로 요구해 pnpm 11의 기본 `blockExoticSubdeps`에 차단된다. 전역 예외는 안전 계약 변경이므로 자동 적용하지 않았고, Electron scaffold는 별도 worktree의 미커밋 상태로 보존했다
- **M5 G1 Electron Forge 8 스파이크(2026-08-25)**: Forge `8.0.0-alpha.10`과 모든 Forge plugin을 exact pin, Electron `43.4.0`·Vite `8.2.2`·Fuses `2.1.3`을 사용해 strict pnpm lock과 macOS arm64 ASAR package를 확인했다. Forge 호환을 위한 hoisted layout은 `pnpm-workspace.yaml`의 `nodeLinker: hoisted` 단일 설정이며 공급망 예외가 아니다. Electron `44.0.0`은 release-age 예외를 자동 요구해 쓰지 않았다. renderer는 고정 `app://m5`이고 sandboxed/isolated window와 version-only preload를 유지한다
- **M5 G2 protocol v1 계약(2026-08-25)**: `hello-agent-protocol`과 `src/m5/contracts`가 같은 언어 중립 golden fixture를 사용해 UTF-8 JSONL v1, 1 MiB control line, `{ text }`만 허용하는 256 KiB `blob.chunk`, 실행별 request ID 유일성, JS와 호환되는 `seq` 상한 `2^53-1`, `system.ready` 단일 handshake, `app.prepareQuit`, 구조화 오류와 outcome 불명 시 자동 재시도 금지를 고정한다. 세션의 첫 envelope는 반드시 `system.ready`이며 그 전 request/response/event와 중복 ready는 모두 fail-closed다. 공개 `M5BridgeV1`은 system status·prepare quit·sidecar event 구독만 선언하며 범용 IPC/preload wiring은 없다. SQLite·실제 sidecar 전송은 G3 범위다
- **M5 G3 Rust sidecar·SQLite 경계(2026-08-25)**: production sidecar argv는 `--stdio` 하나로 고정하고 DB 경로·환경 override를 받지 않는다. Electron main이 `userData/com.wonseokhan.helloagent/m5`를 생성·정규화해 spawn cwd로 주면 sidecar도 그 cwd를 canonicalize하고 고정 파일명 `hello-agent.sqlite3`만 붙인다. sidecar는 bundled SQLite 단일 owner이며 모든 연결의 FK 활성화·검증, transactional migration, integrity/replay/recovery를 통과한 뒤에만 ready를 보낸다. stdout은 bounded JSONL protocol 전용이고 pre-ready 오류는 stderr+exit 70이다
- **M5 G3 Electron-sidecar 수명주기(2026-08-25)**: Electron main은 packaged sidecar를 ASAR 밖 고정 경로에서 한 번만 실행하고 3초 ready, bounded framing, instance/response identity, pending 요청을 fail-closed로 관리하며 자동 restart/retry하지 않는다. preload는 `system.status`·`app.prepareQuit`·sidecar event의 고정 channel만 top-level 허용 origin에 노출한다. 정상 종료는 single-flight `app.prepareQuit` 안전 응답 뒤에만 허용하고, G3 capability는 이 두 method뿐이다. 프로젝트·채팅·실행·파일 API는 G4 이후다
- **M5 G4 계약 경계(2026-08-25, standalone snapshot으로 갱신)**: protocol v1의 예약 project/file/diff method·event와 SQLite schema v1은 유지하고, G2 golden fixture와 `M5BridgeV1`은 불변으로 둔 채 project lifecycle fixture와 `M5BridgeV2`를 사용한다. 명시적 canonical Git root, 읽기 전용 source capability, self-contained Git bundle과 raw committed bytes로 만드는 앱 data 아래 detached standalone snapshot+manifest, hash 기반 optimistic save, chunked file/diff, watcher overflow의 `rescanRequired`, dirty draft 보존을 계약으로 고정한다. `project.createWorktree`·`createProjectWorktree`·`worktreeId/path`는 protocol v1 호환 이름일 뿐 linked worktree를 뜻하지 않는다. G4 `project.close`는 물리 삭제가 아니라 watcher 중단+영속 `abandoned`이며 실제 agent/chat/run과 원본 적용은 범위 밖이다. Windows G4는 별도 안전 게이트 전까지 지원하지 않는다
- **M5 G5 계약 경계(2026-08-26)**: protocol framing·SQLite schema v1과 G4 `M5BridgeV2`는 유지하고 additive `M5BridgeV3`를 사용한다. 한 Chat은 명시적 `projectId`·`worktreeId`·`workspaceIdentity`와 1:1이며 생성 시 프로젝트 역할 설정 revision과 `planning/coding/review`의 runtime·정확한 model selector·nullable reasoning·고정 permission을 snapshot한다. public TeamMessage는 hidden reasoning을 저장하지 않고 durable chat event sequence를 protocol stream seq와 분리한다. fixture writer는 run+workspace lock commit 뒤 spawn하며 동일 worktree writer를 직렬화하고, 불확실한 종료는 lock을 유지한 `recoveryRequired`, 해결 v1은 자손 소멸 증명 뒤 abandon-only다. `run.startAgentSmoke`는 예약만 유지하고 capability에 광고하지 않으며 Windows/실제 유료 실행은 별도 게이트 전까지 비활성이다
- **M5 G5A actual-agent 계약(2026-08-27)**: 기존 G5 fixture ledger/handshake는 불변이고 실제 실행은 planned Bridge V4의 별도 `agentRuns` ledger와 typed start/cancel/recovery 계약을 사용한다. renderer는 경로·argv·model·env·cwd·prompt를 주입할 수 없고 sidecar가 immutable chat role snapshot과 SHA-256 capability fingerprint로 exact profile을 해석한다. machine-readable 또는 exact-version 실측 capability가 아니거나 effective model 증거가 `argvOnly`이면 `canStart=false`다. v1은 one-shot/`nextRun`만 허용하며 process identity release barrier, 모든 자손 소멸, permission별 writer-lock 종료, 공개 `status/question/final`, 불확실 종료의 poisoned recovery를 TypeScript/Rust 공용 golden이 fail-closed 검증한다. 이 계약은 통합됐지만 실제 launcher·Electron/UI 소비자와 실제 CLI/auth smoke는 아직 구현·검증되지 않았다
- **M5 G5 fixture 수직 흐름 완료(2026-08-26)**: 프로젝트 역할 설정은 revision CAS, 기존 채팅은 생성 당시 role snapshot 불변이다. `workspace.prepareChat`은 매번 새 unbound standalone snapshot을 만들고 `chat.create`가 settings·최초 메시지와 exact preparation을 원자 bind하며, 실패 정리는 네 ID가 모두 맞는 준비만 terminal abandon한다. 모든 채팅 workspace/save는 `chatId/projectId/worktreeId/workspaceIdentity`와 `saveId`를 교차 검증한다. macOS 결정적 fixture의 공개 팀 타임라인, clean/uncertain 취소, process-tree 복구, poisoned lock과 `prepareQuit` blocker가 Electron/preload/UI까지 연결돼 exact `014bd987`의 누적 하네스 PASS·독립 QA `G5_READY`를 받았다. 실제 Claude Code/Codex adapter·인증·사용자 프로젝트·Windows는 검증하지 않았다
- **M5 G4 CLI worktree capability 차단(2026-08-25)**: macOS Git CLI의 `worktree add`는 target이 존재하면 거부하고, held directory의 `/dev/fd/N/.git` 하위 탐색도 `ENOENT`라 검증 뒤 교체 가능한 source·target 경로 문자열 없이 두 filesystem capability를 동시에 묶을 수 없었다. source Git root나 managed target을 ambient path로 넘기면 실행 직전 symlink swap으로 외부 repo filter 실행·admin write 또는 외부 worktree 생성이 가능하다. 이 경계를 수용하거나 사후 검사로 덮지 않는다. 실제 linked worktree를 유지할 native Git 구현과 안전한 detached local clone/snapshot은 제품·안전 계약이 다른 대안이므로 사용자 결정 전 G4 Rust 통합을 중단한다
- **M4 Claude 실행 계약(2026-08-19 실측, CLI `2.1.235`)**: 읽기 전용을 강제하는 것은 **`--tools` 허용목록**뿐이다. `--disallowed-tools`는 거부목록이라 `Task`·`ToolSearch` 등 23개를 남기고 **거부 증거도 안 남긴다**. `--setting-sources project`는 **대상 프로젝트 설정을 로드해** 권한을 `bypassPermissions`로 승격시키고 훅·MCP를 실행시키므로 안전 정책이 아니다 → **`--setting-sources ''`** 를 쓴다. `--safe-mode`는 훅·MCP·스킬만 끄고 **권한 승격은 못 막는다**. `--permission-mode dontAsk`는 자동 승인이 아니라 거부 게이트이며 `permission_denials`로 기계 판독 가능한 증거를 남긴다. **작업 폴더 경계와 사후 빈 diff는 예방책이 아니라 사후 확인 수단**이다(경계는 복제본 *안* 쓰기를 허용한다). 실행 형식은 **`--output-format stream-json --verbose` 고정**이고 `system:init`을 fail-closed로 검증한다(`tools` 집합 일치·`permissionMode`·`mcp_servers`·`plugins`·`model`·`cwd`·`session_id`). ⚠️ **`--json-schema`는 요청하지 않은 `StructuredOutput` 도구를 주입**하므로 기대 도구 집합은 4개다. §7.10 파일럿은 **macOS 전용**(Windows 종료 계약 미실측). 상세는 `proposals/m4-orchestration.md` §7.10.1.2
- **Claude CLI 프로세스 트리·종료(2026-08-19 실측, Codex 2차 검토 반영)**: Bash 도구 자식은 **부모 프로세스 그룹에 없고 별도 세션 리더**(`Ss`)다. 그룹 `TERM`으로 죽는 것은 Claude 자체 정리 동작일 뿐이라, **루트에 `SIGKILL`을 보내면 자손이 남는다**(15초까지 생존 확인). **종료 성공은 신호 결과가 아니라 관측으로만 판정한다** — 한 번이라도 관측한 **`(pid, 시작 시각)` 신원을 누적 추적**하고 **누적 집합 전체의 소멸을 확인**해야 성공이다. ⚠️ **루트 기준 ppid 트리 순회는 오판한다** — 부모가 먼저 죽으면 자손이 `ppid=1`로 재부모화돼 트리에서 사라지고, 살아 있는데 빈 집합으로 보인다(C6로 재현). 열거는 **살아 있는 모든 추적 대상**에서 출발한다. **PID 재사용(신원 불일치)과 조회 실패는 `poisoned`**. `killpg`의 **`EPERM`은 성공도 소멸 증거도 아니다**(좀비만 남은 그룹에서도 난다). **확인 불가는 곧 `poisoned`** — 실행과 복제본을 함께 표시하고 자동 인계·재사용을 차단하며 해소는 명시적 복구/폐기뿐. `run_in_background` 때문에 **정상 종료 경로에서도** 회수가 필요하다. 상세는 `proposals/m4-orchestration.md` §7.10.1.2 ⑤
- **M4 §7.10 최소 자동 인계 — 구현·검토·실사용 완료**: `proctree.rs`(종료 계약, F1~F8), `claude_pilot.rs`(실행 계약 + `system:init` fail-closed), `handoff.rs`(사용자 1회 입력 → 계획 → **앱이 인계** → 구현), `App.tsx`의 macOS 전용 `HandoffPilot`으로 구성된다. 신원 조회는 `libc::proc_pidinfo(PROC_PIDTBSDINFO)`를 쓴다. 통합 테스트는 `#[ignore]`이며 **`env -i`로 돌리면 인증이 사라져 실패**한다
- **M4 자동 인계의 중단·잠금·결과 계약(2026-08-20, Codex 4차 반려로 확정)**: 취소 깃발은 **실행이 아니라 슬롯(`HandoffSession`)이 소유**한다 — 실행이 만들면 준비 중(복제본 만드는 동안) 누른 중단이 실행 시작으로 지워진다. **stdout 읽기 루프 안에서 취소를 확인하면 안 된다** — 한 줄도 출력하지 않는 실행에서는 그 검사에 영영 닿지 못하므로 읽기를 별도 스레드로 옮기고 소유자가 폴링한다. Codex 구간 취소는 `cancel_handoff`가 직접 전달하되 **슬롯 예약 직전의 취소**를 놓치므로 `CancelBridge`가 닿을 때까지 재시도한다(접으라는 신호는 취소 확인 **뒤에** 받아야 마지막 순간의 취소를 흘리지 않는다). 증거 파일은 **자식보다 먼저** 만든다 — 띄운 뒤 실패하면 프로세스와 감시 스레드가 주인 없이 남는다. **종료 확인 실패 뒤에는 어떤 blocking 대기도 하지 않는다**(`wait()`·`join()` 모두). 잠금은 **3상태**다(`none`/`persisted`/`sessionOnly`) — 디스크 표시 저장에 실패했는데 잠갔다고 보고하면 재시작 후 오염된 복제본이 다시 쓰인다. 변경 파일 목록은 **`git diff --name-status -z -M`** 에서 읽는다 — 패치 본문의 `+++ b/…`는 **삭제·이름만 변경·바이너리에 나타나지 않아** "바뀐 게 없다"는 거짓말이 된다. 완료 화면의 결과는 **구현 단계 최종 메시지**이고 계획 요약은 *하려던 것*으로 분리한다. `init` 모델 검증은 부분 문자열이 아니라 **실측 허용값 완전 일치**다(`contains("sonnet")`는 `claude-sonnet-5-evil`을 통과시킨다). 상세는 `proposals/m4-orchestration.md` §7.10·§9
- **pid는 식별자가 아니다 — 조회·집합·신호 어디서든(2026-08-20, Codex 5차 반려)**: 종료 추적의 누적 집합을 pid로 키잡으면 소멸을 확인한 pid를 새 자손이 재사용할 때 **편입이 막히고**, 옛 신원은 `Gone`이라 이후 조회에서도 건너뛰어 **살아 있는 자손을 아무도 보지 않는 채 `Confirmed`** 가 난다. 키는 `(pid, start_us)` 전체여야 한다(F7). 같은 실수가 **테스트 하네스에도** 있었다 — 가짜 `signal`이 pid로 첫 항목을 찾아 같은 pid를 쓰던 죽은 프로세스에 신호를 보냈다. 또한 **실행 중 관측 오류는 영구히 기억해야 한다** — 일시적 열거 실패·신원 불일치가 종료 시점에 사라져 있어도, 그 회차에 생겼다 사라진 자손이 있었는지 알 수 없으므로 소멸을 주장할 근거가 없다(F8). 상세는 `proposals/m4-orchestration.md` §7.10.1.2 ⑤
- **프런트 테스트는 `node --test` + 노드 타입 제거로 돌린다(2026-08-21)**: 앱에는 테스트 프레임워크가 없었지만 `website/`가 이미 `node --test`를 쓴다. 같은 방식에 `--experimental-strip-types`(Node 22.18)를 얹어 `.ts` 순수 모듈을 그대로 임포트한다. `npm test` → `tests/*.test.mts`. **UI 렌더링이 아니라 순수 상태 전이만** 테스트한다 — 그래서 프레임워크가 필요 없다
- **비Git 프로젝트를 푸는 길: "되돌릴 수 있게 준비하기"(2026-08-24)**: `create_first_project`가 `git init`을 하지 않아 **앱이 만든 프로젝트가 전부** 파일럿 대상이 못 됐다. 안내만 남기면 비개발자는 못 푼다. 동의를 받고 `git init` + **첫 복원 지점 커밋**을 한다 — `git init`만으로는 부족하다(커밋이 없으면 `rev-parse HEAD` 실패). 원본을 건드리는 유일한 동작이라 **동의 없이 실행 금지**, 기존 저장소 이력 불변, **다른 저장소 안의 폴더는 거부**(중첩 저장소가 사용자 이력을 어지럽힌다), 커밋 신원은 **그 커밋에만** 주입(`git config` 불변). 새 프로젝트는 처음부터 준비하되 **실패해도 프로젝트 생성은 성공**으로 둔다
- **`serde_json`에서 `null`은 "없음"이 아니다(2026-08-24 실사용 사고)**: `event.get("structured_output")`은 값이 JSON `null`이어도 `Some(Value::Null)`을 준다. `or_else` 대체 경로도, `let Some(..) else` 오류 보고도 **전부 건너뛴다.** 실사용에서 이것 때문에 인증 만료 원인을 잃었다. 없을 수도 있는 필드는 `.filter(|v| !v.is_null())`로 걸러 낼 것
- **에이전트 실패는 CLI가 말해 준 원인을 버리지 않는다(2026-08-24)**: 계획이 없다는 이유로 `"run produced no structured plan"`으로 덮어쓰면 **해결책이 분명한 상황(로그인 만료)에서도 사용자가 막다른 길에 선다.** `result` 텍스트를 그대로 전하고 닥터가 분류한다(`auth` 추가). 멈춘 화면은 닥터를 거쳐 쉬운 말로 이유를 보여주고 영문 원문은 상세 안에만 둔다
- **"할 일 없음"은 사고가 아니라 되묻기다(2026-08-24)**: 인사말만 넣으면 계획 역할이 `steps: []` + 이유를 돌려준다(실측) — **별도 의도 분류기가 필요 없다.** 이걸 실패 화면으로 보내면 초보자는 자기가 망가뜨린 줄 안다. `HandoffStage::NeedsGoal`로 분리해 "무엇을 바꿀지 알려주세요"로 되묻는다
- **serde `rename_all`은 변형 이름만 바꾼다 — 필드는 `rename_all_fields`(2026-08-24 실사용 사고)**: `#[serde(tag="state", rename_all="camelCase")]`를 단 enum의 **struct 변형 안 필드는 그대로 snake_case로 나간다**. `run_id`가 그대로 나가 화면의 `started.runId`가 `undefined`가 됐고, 그 값을 다음 `invoke`에 넘기니 **키가 통째로 사라져** `command run_handoff missing required key runId`로 터졌다. **JS에서 값이 `undefined`면 키 자체가 없어진다** — 이게 조용한 실패의 정체다. `install.rs`의 `InstallEvent`는 이미 `rename_all_fields`를 달고 있었다(올바른 예). ⚠️ `login.rs`의 `LoginEvent`는 아직 안 달았지만 필드가 전부 한 단어(`url`·`line`·`success`)라 지금은 무해하다 — **여러 단어 필드를 추가하는 순간 같은 사고가 난다**. 구조체는 해당 없음(`rename_all`이 필드에 적용된다)
- **TypeScript 인터페이스는 손으로 적은 것이라 백엔드 모양을 검증하지 못한다(2026-08-24)**: 위 사고에서 TS는 `{ state: "ready"; runId: string }`이라고 선언했지만 백엔드는 `run_id`를 보냈고, 컴파일은 통과했다. 그래서 **화면이 읽는 키 이름 자체를 Rust 테스트로 못 박는다**(`the_start_reply_uses_the_key_names_the_screen_reads` 등). 경계를 넘는 새 타입을 만들면 같은 방식으로 키를 고정할 것
- **"설치됨"이 "쓸 수 있음"은 아니다(2026-08-24)**: 버전이 최신이어도 설치가 온전하지 않을 수 있다(Codex 셸 호스트 누락). 그런데 홈의 에이전트 버튼은 **미설치·미로그인·업데이트 있음** 세 경우에만 떠서, 망가진 최신 설치는 **아무 버튼도 없이 막다른 길**이 됐다. `AgentStatus.needs_repair`로 판정해 "고치기"를 노출한다. 새 에이전트를 붙일 때 **그 에이전트가 온전하다는 조건**을 `installation_is_complete`에 적을 것
- **Codex CLI 0.149.1 실측 — 앱이 쓰던 계약이 깨졌다(2026-08-24)**: ⚠️ **`approval_policy = "untrusted"`는 이제 거부된다**(`no longer supported; remove this setting`). 파일럿 인자와 앱의 안전 프리셋 양쪽에 있었고, 프리셋 쪽은 **신규 사용자의 Codex를 통째로 망가뜨린다**(유효값: `on-failure`·`on-request`·`granular`·`never`). `codex exec`에서 승인 정책 자체가 사라져 **경계는 `--sandbox` 하나**다. ⚠️ **릴리스는 `codex`와 `codex-code-mode-host`를 별도 자산으로 배포**하는데 앱은 앞의 것만 받아, 설치된 Codex가 **셸 명령을 하나도 실행하지 못했다**. 이때 파일이 안 생기니 **"샌드박스가 막았다"로 오판하기 쉽다** — 실제로 첫 실측이 그렇게 나왔다. 실측 결과: `--sandbox read-only`는 진짜로 쓰기를 막고(`EXIT=1`, `operation not permitted`), `--output-schema`로 구조화 계획이 나온다
- **Codex 계획 역할 계약은 Claude보다 약하다 — 숨기지 않는다(2026-08-24)**: Codex는 `thread.started{thread_id}`만 주고 **어떤 샌드박스·도구로 떴는지 이벤트로 알려주지 않는다.** 그래서 Claude의 `system:init` fail-closed 검증에 대응하는 것이 **없다**. argv 통제 + 실측된 샌드박스 + 사후 빈 diff에 기대며, 마지막은 예방책이 아니라 사후 확인이다. 거부된 도구 호출(`permission_denials`)도 기계 판독 불가. 상세는 `src-tauri/src/codex_pilot.rs` 머리말
- **파일럿 대상은 사용자가 명시적으로 고른다(2026-08-24, §7.6A 1차 실사용 A5 실패)**: 홈이 `projects[0]`을 말없이 대상으로 삼아 비Git 프로젝트에 작업이 갔고, 사용자는 시작 뒤에야 영문 오류를 봤다. **`projects.find(p => p.pilotEligible)`도 같은 잘못**이다 — 여전히 사용자가 고르지 않는다. 쓸 수 있는지는 **누르기 전에** 알려 준다: `scan_projects`가 `pilotEligible`을 싣고, 그 판정은 실행과 **같은 함수**(`can_copy_pilot_source`)를 쓴다(복제본이 Git 클론이라 비Git은 불가). 다만 그것은 **표시용일 뿐 권한 경계가 아니다** — 최종 거절은 언제나 `prepare_pilot_copy`이고(TOCTOU), 그 실패도 닥터 `notgit`으로 지역화해 보여 준다. 원문은 "자세히 보기" 안에만
- **화면의 실행 추적은 순수 모듈이 정한다(2026-08-21, Codex 8차 반려)**: `src/handoffRun.ts`. 따라가는 실행 ID와 보류된 중단을 ref 두 개로 굴리면 경합이 난다 — 끝난 실행 ID가 남으면 다음 실행의 예약 대기 중 중단이 "실행 중"으로 오인돼 **죽은 슬롯으로 나가고**(무동작) 새 실행은 그대로 진행된다. 규칙 둘: **`begin_handoff` 호출 전에 이전 ID를 버린다**, **실행이 끝나면 비운다**(결과·막힘·오류 모든 경로). 방어가 하나뿐이면 다른 하나가 사라질 때 조용히 뚫리므로 둘 다 테스트로 고정한다
- **잠긴 복제본은 새 실행을 시작하기 전에 치운다(2026-08-20, Codex 7차 반려)**: 파일럿 슬롯은 하나뿐이다. 잠긴 A가 슬롯을 쥔 채 새 B를 만들면 B의 실행은 거부되고 화면에는 B만 보인다 — **B를 아무리 정리해도 슬롯은 A의 것이라 안 풀려 후속 실행이 영영 거부된다.** 그래서 `begin_handoff`가 **사본을 만들기 전에** 막고 A의 경로·사유·잠금 상태를 돌려준다. 증거 경로는 **"저장된 파일"만** 보고한다(`Option` + `is_file()`) — 없는 파일의 경로를 "저장된 기록 위치"로 주면 사용자는 열리지 않는 경로를 들고 도움을 요청하게 된다. `FailureContext`는 **경로가 정해지는 즉시** 만들어야 증거 생성 도중의 부분 실패에서 이미 생긴 파일을 잃지 않는다. `PoisonedRun`도 종료 오류와 표시 저장 오류를 나눠 든다
- **테스트 함정 2개(2026-08-20)**: `std::sync::Mutex` 가드를 쥔 채 같은 락을 다시 잡으면 **데드락**이다(`session.slot()` 후 `session.release()`). 그리고 실패 주입에 "경로를 폴더로 선점"하는 기법을 쓸 때 존재 확인은 `exists()`가 아니라 **`is_file()`** 이어야 한다 — 폴더가 파일로 세어진다
- **실행 실패도 결과와 같은 무게로 실어 나른다(2026-08-20, Codex 6차 반려)**: `execute_pilot`은 `Result<PilotRun, PilotFailure>`다. 오류를 문자열로 줄이면 **이미 만들어진 증거 경로가 그 지점에서 사라져** 화면에 폐기·복구 수단이 남지 않는다. `PilotFailure`는 `error`·`run_id`·`workspace_path`·`evidence`·`poisoned`·`marker_error`를 싣는다. **`poisoned`(무엇이 잘못됐나)와 `marker_error`(왜 잠금을 저장 못 했나)는 다른 사실**이라 절대 합치지 않는다 — 합치면 화면의 "표시를 저장하지 못한 이유"에 종료 오류가 나온다. 그리고 **파일럿 슬롯은 하나뿐이므로 슬롯의 오염 상태를 볼 때는 반드시 복제본 경로를 대조한다** — 대조하지 않으면 남의 오염이 멀쩡한 복제본을 잠그고, 폐기해도 슬롯이 풀리지 않아 실행이 영영 막힌다
- **파일럿 복제본 폐기는 두 경로다(2026-08-20)**: `discard_codex_pilot`은 **`poisoned` 표시가 있는 복제본만** 받는다. 인계 화면의 "결과 버리기"는 정상 완료·단순 취소·`sessionOnly`도 버릴 수 있어야 하므로 전용 `discard_handoff_workspace`를 쓴다. 두 경로 모두 **폐기 = 영구 `abandoned` 표시**이며 파일을 지우지 않는다(증거는 복제본 밖에 있고, 표시가 붙으면 다시 실행 대상이 되지 못한다). 임의 폴더 폐기를 막는 것은 파일럿 루트 포함 여부와 매니페스트 검증이다
- **Claude CLI `2.1.237`에서도 실행 계약이 그대로 통과했다(2026-08-20)**: 계약을 실측한 `2.1.235`에서 패치 판올림이 있었지만 도구 집합·`dontAsk`·`claude-sonnet-5`가 모두 동일했다. 정확 일치 목록이 판올림마다 부서지지는 않는다는 첫 관측 — 다만 깨지면 목록을 넓히는 게 아니라 **재실측**이 답이다
- **함정: `/dev/urandom`을 `std::fs::read`로 읽지 말 것.** 끝없는 스트림이라 프로세스가 멈춘다. `read_exact`로 필요한 바이트만 읽는다
- **M4 S2 (Codex 파일럿 실행 경계) — 통과**: 실행기(`d2dec6d`)와 수동 경계 시나리오 실측 모두 **Codex 교차 검토를 통과**했다(§7.9·§8). 앱 데이터 `m4-pilots/` 아래에 만든 매니페스트 있는 Git 복제본만 대상으로 삼고, Codex 파일럿은 사용자 config·rules를 무시한 `workspace-write + untrusted` 고정 정책을 쓴다. 자동 승인·bypass·추가 쓰기 폴더는 금지한다. 경계는 *"복제본 + 임시 디렉터리"* 이며 비임시 경로 쓰기는 커널이 거부함을 종료 코드로 확인했다. Unix 그룹 `TERM` → `KILL`, Windows `taskkill` 실패 판정, child 부착 전후의 취소 실패 처리와 `poisoned` 영속화까지 자동 테스트를 통과했다. **남은 별건 2가지**: Windows CI 확인, 그리고 좀비만 남은 그룹에서 `killpg`가 `EPERM`을 반환하는 건에 대한 `tree_is_gone`/`signal_ok` 오판 검토(§7.10.1.2 ③ T1). UI는 여전히 열지 않는다.

## 작업 규칙 (고정)

- **워크로그 필수 범위**: 구현뿐 아니라 버그 분석, 코드 리뷰, 우선순위 결정, GitHub/배포 같은 외부 변경도 다음 작업자에게 의미가 있으면 이 문서에 기록. 최종 응답 전 `git diff -- docs/history.md`로 기록 여부 확인
- **커밋·push 전 반드시 사용자 승인**을 받는다 (이 프로젝트 관행)
- `git`은 scm_breeze 충돌(`_safe_eval` 오류)로 **`/usr/bin/git` 절대경로** 사용. 커밋 메시지는 임시 파일 방식(HEREDOC 미동작), Co-Authored-By 트레일러 포함
- **검증**: `pnpm build`(tsc+vite) · `cargo test --manifest-path src-tauri/Cargo.toml` · 네트워크 격리 E2E는 `-- --ignored` · push 시 CI(macOS/Windows)
- 실행: `pnpm tauri dev`(개발) / `pnpm tauri build`(설치 파일). Rust 1.85+ 권장, 실제 crate edition은 **2021**
- 초보자(비개발자) 대상 — 모든 문구는 전문용어 없이 쉬운 한국어. ".md도 IDE도 모르는 사람" 기준으로 검토

---

## 워크로그 (최신이 위)

### 2026-08-25 및 그 이전 → `docs/archive/history-2026-08.md`

2026-08-12 ~ 2026-08-25 엔트리를 날짜 경계로 아카이브했다. M4의 현재 결론은 `docs/proposals/m4-orchestration.md`에 있으므로 통상 아카이브를 열 필요는 없다.

### 2026-07-23 및 그 이전 → `docs/archive/history-2026-07.md`

17개 엔트리(2026-07-16 ~ 2026-07-23)를 아카이브로 옮겼다. 재사용할 결정·함정은 위 `핵심 결정·발견`에 반영돼 있으므로, 통상 아카이브를 열 필요는 없다.
