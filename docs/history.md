# 작업 이력 (Hello, Agent)

> 여러 코딩 에이전트(Claude Code, Codex)가 **번갈아 작업**할 때 컨텍스트를 이어받기 위한 워크로그다.
>
> **세션 시작**: 아래 "핵심 결정·발견"과 "작업 규칙"을 읽고, 워크로그 맨 위 엔트리로 현재 상태를 파악한다.
> **세션 종료**: 코드 변경이 없어도 프로젝트에 영향을 주는 분석·결정·검증·외부 작업을 했다면 워크로그 맨 위에 새 엔트리를 추가한다(최신이 위). 헤더는 `### YYYY-MM-DD · by <모델명>` 형식으로 **작업한 모델을 구체적으로** 적는다(예: `by Claude Opus 4.8`, `by GPT-5 Codex`). 한 날 여러 모델이 작업했으면 엔트리를 나눈다. 재사용할 만한 새 결정·발견은 "핵심 결정·발견"에도 반영한다. **엔트리는 덮어쓰지 말고 계속 쌓는다.**
>
> **아카이브 롤오버**: 이 파일이 **500줄을 넘으면** 오래된 엔트리부터 `archive/history-YYYY-MM.md`로 옮겨 **300줄 이하로 줄인다.** 날짜 경계에서 자르고(같은 날짜를 쪼개지 않음), 삭제가 아니라 이동이며, 고정 섹션은 옮기지 않는다. **날짜별로 파일을 쪼개지 않는다** — 현재 진실이 흩어진다. 날짜별 조회는 `git log -- docs/history.md`.
>
> **설계 심의는 여기 쌓지 않는다**: 한 주제의 토론 왕복·철회·정정이 여러 엔트리에 얽히기 시작하면 전용 설계 문서로 옮기고 여기에는 결론 요약과 포인터만 남긴다. 워크로그는 *무엇이 일어났나*, 설계 문서는 *무엇이 참인가*를 기록한다. 예: `proposals/m4-orchestration.md`

---

## 프로젝트 요약 (고정)

터미널을 한 번도 안 열어본 비개발자를 **코딩 에이전트(Claude Code·Codex)가 굴러가는 상태까지** 데려다주는 데스크톱 GUI 앱. Tauri 2 + React 19 + TypeScript + pnpm, Rust는 최소한(설치·감지·로그인·실행)만.

- 기획 원본: `~/Library/CloudStorage/OneDrive-개인/ObsidianVault/오픈소스/아이디어/후보/에이전트-스타터.md`
- 레포: https://github.com/wonseok-han/hello-agent (로컬 폴더도 `~/projects/hello-agent`로 변경 완료 — 2026-08-12 확인)
- 위저드 6단계: 에이전트 선택 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식
- 주요 모듈: `agent.rs`(에이전트 레시피) · `detect.rs` · `install.rs` · `login.rs` · `project.rs` · `editor.rs`

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
- **M5 설계 방향(2026-08-24)**: 기존 M0~M4는 최초 설정·튜토리얼로 유지하고, 이후 여러 영속 채팅, 프로젝트별 역할·에이전트·모델, Git worktree, 명확한 diff와 실제 코드 편집을 갖춘 초보자용 agent-first IDE로 확장한다. 한 채팅은 변경 목표·worktree 하나를 소유하고 역할 왕복을 한 팀 대화에 기록한다. **M5 기술 기준선은 Electron + React/TypeScript + Monaco 셸과 기존 Rust 안전 코어 sidecar**이며 Tauri와 동시 구현하지 않는다. SQLite 이벤트 원장+projection, 앱 종료 시 안전 중단, 첫 적용은 전체 worktree+복원 지점, macOS 우선 실행을 기준으로 한다. 새 이름은 개발과 분리해 보류한다. 통합 브랜치는 `m5/coding-agent-ide`; `main` 병합은 전체 완료 뒤 사용자 승인 사항
- **M5 직접 유사 제품 Cebus 발견(2026-08-24)**: `cebus-ai/cebus`는 여러 AI 모델의 단일 대화, 역할별 모델 배정, 채널·스레드, CLI·웹·VS Code 표면을 이미 제공해 M5 구상과 직접 겹친다. M5 벤치마크에 포함하고, 초보자 안전·격리 worktree·명시적 diff 검토와 복구 루프를 차별점으로 검증한다
- **멀티 에이전트 저장소 운영(2026-08-25)**: 큰 구현은 오케스트레이터 1명이 통합 브랜치를 소유하고, 최대 3명의 쓰기 구현 담당이 별도 branch/worktree와 겹치지 않는 파일 소유권으로 작업한다. 계약을 먼저 고정한 뒤 파동식으로 병렬화하며, 독립 코드 리뷰와 최신 통합 commit QA를 통과한 변경만 통합한다. Codex 프로젝트 역할·모델·sandbox는 `.codex/config.toml`과 `.codex/agents/*.toml`, 상세 실행 절차는 `docs/procedures/multi-agent-orchestration.md`가 기준이다
- **M5 기계 검증 하네스(2026-08-25)**: `scripts/m5-harness.mjs`가 로컬과 macOS/Windows CI의 단일 검증 진입점이다. `baseline`은 기존 회귀를 실제 실행하고 G1~G6는 앞 단계를 누적한다. 필수 script가 없으면 `NOT_READY`/exit 2로 실패하며, versioned JSON 증거를 `.artifacts/m5-harness/`에 남긴다. 예약 script와 판정 규칙은 `docs/procedures/multi-agent-orchestration.md` §13이 기준이다
- **M5 G1 Rust core 추출(2026-08-25)**: 루트 Cargo workspace와 단일 `Cargo.lock`을 기준으로 삼고, `crates/hello-agent-core`에 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약을 Tauri/Electron/Node/SQLite/UI 의존 없이 옮겼다. 기존 Tauri 모듈 경로는 좁은 re-export shim으로 유지해 `handoff.rs`·`orchestration.rs` 동작을 바꾸지 않는다. `crates/hello-agent-sidecar`는 후속 protocol/SQLite 게이트 전까지 compile-only 골격이다
- **M5 커밋·통합 자동 승인(2026-08-25)**: 작업 계약 범위 안의 기능 worktree 원자적 구현·테스트·리뷰 수정 커밋과, 독립 리뷰 및 해당 하네스·QA를 통과한 commit의 `m5/coding-agent-ide` 통합은 사용자에게 매번 다시 묻지 않는다. push, `main` 병합, 범위 밖 변경, 안전 불변식·확정 결정 변경, 실제 유료 에이전트·사용자 프로젝트·파괴적 시험과 외부 게시는 계속 별도 승인 사항이다
- **M5 G1 공급망 차단(2026-08-25)**: Electron Forge stable `7.11.2`가 Git SHA 기반 `@electron/node-gyp`을 전이 의존성으로 요구해 pnpm 11의 기본 `blockExoticSubdeps`에 차단된다. 전역 예외는 안전 계약 변경이므로 자동 적용하지 않았고, Electron scaffold는 별도 worktree의 미커밋 상태로 보존했다
- **M5 G1 Electron Forge 8 스파이크(2026-08-25)**: Forge `8.0.0-alpha.10`과 모든 Forge plugin을 exact pin, Electron `43.4.0`·Vite `8.2.2`·Fuses `2.1.3`을 사용해 strict pnpm lock과 macOS arm64 ASAR package를 확인했다. Forge 호환을 위한 hoisted layout은 `pnpm-workspace.yaml`의 `nodeLinker: hoisted` 단일 설정이며 공급망 예외가 아니다. Electron `44.0.0`은 release-age 예외를 자동 요구해 쓰지 않았다. renderer는 고정 `app://m5`이고 sandboxed/isolated window와 version-only preload를 유지한다
- **M5 G2 protocol v1 계약(2026-08-25)**: `hello-agent-protocol`과 `src/m5/contracts`가 같은 언어 중립 golden fixture를 사용해 UTF-8 JSONL v1, 1 MiB control line, `{ text }`만 허용하는 256 KiB `blob.chunk`, 실행별 request ID 유일성, JS와 호환되는 `seq` 상한 `2^53-1`, `system.ready` 단일 handshake, `app.prepareQuit`, 구조화 오류와 outcome 불명 시 자동 재시도 금지를 고정한다. 세션의 첫 envelope는 반드시 `system.ready`이며 그 전 request/response/event와 중복 ready는 모두 fail-closed다. 공개 `M5BridgeV1`은 system status·prepare quit·sidecar event 구독만 선언하며 범용 IPC/preload wiring은 없다. SQLite·실제 sidecar 전송은 G3 범위다
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

### 2026-08-25 · by GPT-5.6-Terra (M5 G3-C Electron bridge 리뷰 수정)

- **한 일**: 독립 리뷰의 차단점 3건을 닫았다. sidecar transport는 timeout, oversized/invalid stdout, session·identity corruption에서 child `terminate()`를 한 번만 호출하고 모든 pending 요청을 fail-closed로 거부한다. `system.status`의 ready identity·version·capability는 처음 `system.ready`와 정확히 같아야 하며 불일치는 연결 손상이다. stderr는 마지막 32 KiB만 유지하고 제어 문자를 정규화해 unavailable의 기술 detail에만 붙인다. 이는 token/secret redaction이 아니며 기본 UI 문구나 안전 보장으로 쓰지 않는다.
- **종료 결정**: main-owned coordinator는 safe sidecar 응답 뒤에만 clean exit을 허용한다. 반대로 ready 전 sidecar startup이 실패한 경우에는 clean marker나 재시도를 시도하지 않고, 다음 시작의 unclean recovery 의미를 보존한 채 before-quit 재진입 없이 한 번만 unclean exit을 허용한다. ready 뒤 손상/종료는 이 예외에 포함하지 않는다.
- **검증**: fake child timeout/oversize/unknown response/identity mismatch 종료 1회, pending reject, stderr tail truncation·control normalization, pre-ready unclean close single-flight를 포함한 Electron test 10개와 TypeScript no-emit, `git diff --check`를 통과했다. 최종 packaged handshake는 G3-D 통합 QA에서 실제 sidecar와 다시 확인한다.
- **다음 할 일**: 이 fix commit을 독립 재리뷰하고, 승인되면 G3-D sidecar와 함께 package smoke·storage harness를 최신 통합에서 실행한다.

### 2026-08-25 · by GPT-5.6-Terra (M5 G3-C Electron sidecar bridge)

- **한 일**: Electron main/preload에 G2의 세 내부 channel만 연결했다. main은 `system.ready` first JSONL 세션, 3초 ready 제한, 1 MiB bounded stdout framing, request/response 상관관계와 method별 결과 검증을 가진 단일 sidecar client를 소유한다. 알 수 없거나 중복·불일치인 response와 sidecar 종료는 모든 pending 요청을 fail-closed로 거부하며 재시작·자동 재시도를 하지 않는다. renderer에는 versioned `M5BridgeV1`만 노출하고 정확한 listener wrapper를 해제한다.
- **수명주기·경로 결정**: 내부 IPC는 현재 M5 main window의 top-level frame 및 production `app://m5` 또는 정확한 dev origin만 허용한다. 실제 종료는 main single-flight로 sidecar의 `app.prepareQuit`이 safe 응답을 낸 뒤에만 허용하며 error/unsafe는 앱을 유지한다. production sidecar는 환경 변수 override 없이 `process.resourcesPath/sidecar/hello-agent-sidecar[.exe]`와 `shell:false`, fixed `--stdio`만 사용한다. SQLite path 모호성을 피하기 위해 main이 `app.getPath('userData')/com.wonseokhan.helloagent/m5`를 만들고 정규화한 exact directory를 sidecar `cwd`로 고정한다. Rust sidecar는 이 cwd의 `hello-agent.sqlite3`를 사용한다.
- **패키징·검증**: Forge package-after-copy 단계가 release sidecar를 app.asar 밖의 fixed `Resources/sidecar` 위치에 넣고 smoke가 존재와 ASAR 외부 위치를 확인한다. fake sidecar 실패 주입으로 chunked/oversized JSONL, ready timeout, pre-ready exit, unknown response, pending rejection, no retry, event unsubscribe와 source-level sender/IPC/quit/package 경계를 검사했다. `node --test --experimental-strip-types tests/m5-electron*.test.mts` 8 PASS, `npx tsc --noEmit --pretty false`, `git diff --check`를 통과했다. shared dependency runtime으로 Forge package를 실행해 main/preload/renderer Vite bundle은 성공했지만 Electron arm64 fetch가 격리 네트워크에서 실패해 sidecar 포함 최종 package smoke는 통합 worktree에서 재실행한다.
- **다음 할 일**: 독립 코드 리뷰 후 G3 sidecar 구현과 함께 최신 통합에서 storage harness·실제 packaged sidecar smoke를 실행한다. Forge 8 alpha, Windows/Linux package와 macOS plist hardening은 계속 후속 위험이다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A FK 실제 migration 검증)

- **한 일**: foreign-key connection-local 계약 테스트를 서로 다른 in-memory DB의 generic parent/child 테이블에서, 하나의 고유 OS 임시 SQLite 파일과 실제 `storage-v1/migration.sql`을 쓰는 시나리오로 교체했다. 연결 A가 실제 migration을 적용해 `foreign_keys=1`을 확인하고 닫은 뒤, 같은 파일을 여는 연결 B는 다시 0으로 시작하며 계약 validator가 저장을 거부한다. B에서 ON→1을 검증한 뒤 실제 `runs.chat_id → chats.chat_id`에 필수 열을 모두 채운 orphan insert가 FK 오류로 거부되고 `runs`에 행이 남지 않음을 확인했다.
- **안전·검증**: 테스트 DB와 부속 파일은 `mkdtemp`가 만든 고유 OS temp 디렉터리에만 두고 두 연결을 닫은 뒤 `finally`에서 정리해 저장소 산출물을 남기지 않는다. G2+G3 TypeScript contract 13개, Rust 10개, TypeScript no-emit, fmt/check/clippy, G2 fixture 불변과 diff check를 통과했다.
- **다음 할 일**: 이 최종 P1 fix를 재리뷰하고, sidecar 실제 opener 구현에서도 migration 연결 성공을 후속 runtime 연결의 FK 활성화 증거로 재사용하지 않는지 확인한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A 2차 리뷰 수정)

- **한 일**: 모양만 RFC3339인 불가능한 시각을 통과시키던 predecessor 검증을 양 언어에서 실제 Gregorian UTC 달력 검증으로 교체했다. 계약은 `YYYY-MM-DDTHH:mm:ssZ`, 0001~9999년, 실제 월별 일수와 윤년, 00~23시·00~59분·00~59초만 허용한다. `2026-99-99T99:99:99Z`, 숫자 offset, missing/malformed marker는 공통 fixture에서 `unavailable`로 fail-closed한다.
- **연결별 불변식**: migration 파일 첫 줄의 pragma가 다른 연결에 전파된다고 가정하지 않는다. 모든 sidecar SQLite connection은 open 직후 `PRAGMA foreign_keys = ON;`을 실행하고 `PRAGMA foreign_keys; == 1`을 검증한 뒤에만 migration/read/write와 ready로 진행한다. 실패는 stderr+nonzero exit의 pre-ready `unavailable`이다. lifecycle 시작 순서에도 `storageConnectionVerified`를 migration 앞에 고정했다.
- **실제 SQLite 검증**: 새 in-memory 연결 두 개를 raw foreign-key default로 열어 각각 0임을 확인하고, 한 연결에서만 ON→1 검증 뒤 FK orphan insert가 거부되는 것을 확인했다. 이는 설정이 connection-local이며 migration pragma만으로 충분하지 않음을 실제 엔진으로 증명한다.
- **검증**: G2+G3 TypeScript contract 13개, Rust protocol/storage/lifecycle 10개, TypeScript no-emit, protocol fmt/check, clippy `-D warnings`, SQLite migration/integrity, G2 fixture 불변과 diff check를 통과했다. 새 crate/npm 의존성과 lockfile 변경은 없다.
- **다음 할 일**: 이 2차 fix commit을 재리뷰한 뒤 sidecar의 모든 SQLite open 경로가 공통 opener를 강제하고 ready gate와 실제 failure injection을 통과하는지 구현 단계에서 확인한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A 독립 리뷰 수정)

- **한 일**: 독립 리뷰의 차단점 4개를 공통 Rust/TypeScript 계약과 language-neutral fixture에서 닫았다. 직전 계약의 claimed `content_hash` 비교는 신뢰 경계가 아니어서 제거하고, 같은 `operation_id`의 재시도는 `request_id`만 제외한 영속 event 레코드 전체가 같을 때만 no-op으로 인정한다. payload·aggregate·event identity 중 하나라도 달라지면 conflict다.
- **복구 안전**: `previous_app_instance: null`만 이전 실행이 없는 정상 첫 시작이다. 이전 row가 있으면 sidecar identity·pid/platform·시작/heartbeat·event sequence와 explicit nullable `clean_shutdown_at`을 정확히 검증하고 missing/malformed row는 `unavailable`로 fail-closed한다. 정상 종료 row가 있어도 이미 저장된 `poisoned` 또는 `recovery_required` lock은 항상 blocker와 status에 남으며 조용히 정상화하지 않는다.
- **transaction 계약**: writer는 `BEGIN IMMEDIATE` 안에서 operation lookup → event insert → projection update를 수행하고 commit 뒤에만 위험한 외부 작업을 시작한다. event insert 뒤 projection 전 실패 주입은 두 쓰기를 모두 rollback하며 위험 작업을 시작하지 않는다. duplicate no-op과 conflict 판정도 같은 transaction 안에서 이뤄진다.
- **검증**: G2+G3 TypeScript contract 12개, Rust protocol/storage/lifecycle 10개, TypeScript no-emit, protocol fmt/check, clippy `-D warnings`, SQLite 7-table migration과 `integrity_check=ok`, G2 fixture 불변과 diff check를 통과했다.
- **다음 할 일**: 이 fix commit을 독립 재리뷰한 뒤에만 sidecar SQLite writer와 Electron status/quit consumer 구현을 시작한다. 실제 DB writer는 renderer가 제공한 hash나 operation 판정을 신뢰하지 않고 이 transaction/record-equality 계약을 내부에서 수행해야 한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A 저장·수명주기 계약)

- **한 일**: G2 protocol fixture와 공개 bridge 메서드 목록을 그대로 둔 채 G3 구현 착수용 저장·수명주기 계약을 추가했다. SQLite v1은 별도 `userData/com.wonseokhan.helloagent/m5/hello-agent.sqlite3`에 7개 테이블(`schema_meta/app_instances/events/projects/chats/runs/locks`)을 만들며, append-only event 원장은 `request_id`와 별개인 unique `operation_id`·canonical payload·content hash를 영속 키로 쓴다. 같은 operation/content는 재시작 뒤에도 no-op이고 같은 operation의 다른 content는 conflict다. 양 언어 reducer가 공통 replay/projection·unclean recovery fixture를 같은 결과로 만든다.
- **수명주기 결정**: 내부 channel은 `m5:v1:system:get-status`, `m5:v1:app:prepare-quit`, `m5:v1:sidecar:event`로 고정했다. G3 ready capability는 `system.status/app.prepareQuit` 둘뿐이며 migration·DB integrity·projection replay·unclean recovery가 모두 끝나기 전 `system.ready`를 금지한다. pre-ready 실패는 stderr 증거와 nonzero exit로 끝난다. renderer의 `prepareQuit()`은 상태 조회가 아니라 main 소유 single-flight 실제 종료 조정이며, sidecar는 새 요청 차단·작업 중단·DB checkpoint·clean marker 뒤에만 `safeToQuit: true`를 답한다. production 실행은 `process.resourcesPath/sidecar/hello-agent-sidecar[.exe]`, fixed argv, `shell:false`, stdout protocol/stderr log 분리, 환경 override 금지다.
- **검증**: 공통 fixture를 읽는 TypeScript G2+G3 contract test 11개와 Rust protocol/storage/lifecycle test 9개, protocol crate check, TypeScript no-emit 검사를 통과했다. SQLite CLI가 migration으로 정확히 7개 테이블을 만들고 `integrity_check=ok`를 냈으며, G2 fixture 경로는 기준 commit과 byte diff가 없고 protocol fmt·`git diff --check`도 통과했다.
- **다음 할 일**: 독립 계약 리뷰 뒤 sidecar SQLite/stdio 구현과 Electron main/preload 소비자를 각각 별도 소유 worktree에서 구현하고, 최신 통합 commit에서 G3 누적 하네스를 실행한다. G4/G5 project/chat/run 제품 동작은 이 계약에 포함하지 않았다.

### 2026-08-25 · by GPT-5.6-Terra (M5 G2 통합 QA)

- **판정**: 최신 통합 `cd1a9e7`은 `G2_READY`다. TypeScript protocol 4개와 Rust protocol 3개, 프런트 전체 33개, Tauri 회귀 105개/기존 13 ignored를 확인했고, 기존 macOS arm64 패키지에서 ASAR 내용과 V1 Fuse 9개가 모두 통과했다. G2는 `READY`, 아직 구현하지 않은 G3는 성공이나 skip 대신 exit 2 `NOT_READY`로 닫힌다.
- **증거·잔여 위험**: QA 기록은 `.artifacts/m5-qa/g2-integration-cd1a9e7.md`, 누적 하네스는 `.artifacts/m5-harness/g2-contracts-2026-08-25T04-28-46-065Z.json`이다. QA sandbox의 새 Forge package 재생성은 Electron 다운로드 `fetch failed`에서 멈췄지만, 직전 동일 코드 `f3f41df`의 승인 네트워크 누적 실행과 QA의 기존 package 직접 smoke가 통과했다. Forge 8 alpha, Windows/Linux package, macOS plist hardening은 계속 후속 위험이다.
- **다음 할 일**: G3 sidecar transport·SQLite migration/replay·preload 경계를 계약 fixture를 바꾸지 않는 별도 worktree에서 착수한다. push와 `main` 병합은 하지 않는다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G2 계약 통합·누적 하네스)

- **한 일**: protocol v1 구현 `bff9b47`과 독립 리뷰 수정 `f949cca`를 재리뷰 승인 뒤 `m5/coding-agent-ide`의 `f3f41df`로 병합했다. 현재 구조를 `architecture.md`에 반영하고, 작업 중 생성될 수 있는 저장소-local `.pnpm-store/`를 Git 추적에서 제외했다. 작업자의 격리 worktree 의존성 재사용 시도가 루트 `node_modules`를 부분 재생성한 사실을 확인해 즉시 중단시켰고, 추적 파일 손상 없이 기존 디렉터리를 `/private/tmp`에 보존한 뒤 frozen install로 깨끗하게 복구했다.
- **검증**: 독립 재리뷰가 `f949cca`를 승인했다. 최신 통합에서 `node scripts/m5-harness.mjs --gate g2-contracts`가 PASS했고 프런트 33개, Tauri 105개/기존 13 ignored, core 66개, protocol Rust 3개·TypeScript 4개, macOS arm64 Electron package·ASAR·Fuse smoke를 누적 통과했다. 증거는 `.artifacts/m5-harness/g2-contracts-2026-08-25T04-25-54-493Z.json`이다. 일반 sandbox의 두 선행 실행은 의존성/Electron 다운로드 `ENOTFOUND`로 실패했으며 승인된 네트워크 실행에서 같은 게이트가 통과했다.
- **다음 할 일**: 이 기록 commit의 최신 통합 SHA에서 독립 G2 QA를 수행한다. 통과하면 G3 sidecar transport·SQLite migration/replay·좁은 preload 연결을 새 작업 계약과 별도 worktree로 시작한다. 실제 유료 에이전트·사용자 프로젝트, push와 `main` 병합은 계속 하지 않는다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G2 독립 리뷰 수정)

- **한 일**: 독립 리뷰의 차단 finding 3건을 별도 수정했다. `ProtocolSessionV1` 양쪽 구현은 첫 envelope 전체를 `system.ready`로 고정해 ready 전 request/response/event와 중복 ready를 거부하고, 그 ready의 `sidecarInstanceId`로 세션 신원을 bootstrap한다. Rust `seq`도 JavaScript safe integer 상한 `2^53-1`을 적용했으며, `blob.chunk` payload는 우회 필드 없이 `{ text: string }` 하나만 허용한다. 공통 fixture에 ready 전 세 방향, 중복 ready, 최대 seq/초과 seq, `data` 추가 chunk를 각각 추가하고 기존 valid transcript를 ready-first로 정렬했다.
- **검증**: `cargo fmt -p hello-agent-protocol -- --check`, locked protocol check/test(3 PASS), `npm run m5:test:contracts`(TypeScript 4 PASS + Rust 3 PASS), 전체 Node unit(33 PASS), locked Cargo workspace(Tauri 105 PASS/13 기존 ignored, core 66 PASS, protocol 3 PASS), `git diff --check`를 통과했다. 공유 `node_modules`와 `.pnpm-store`는 건드리지 않았다.
- **다음 할 일**: 새 fix commit을 독립 재리뷰하고, 통합 담당이 dependency 복구 뒤 최신 통합 commit에서 G2 누적 harness를 실행한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G2 protocol·typed bridge 계약)

- **한 일**: Tauri/Electron/SQLite 비의존 `hello-agent-protocol` crate와 동형 TypeScript runtime validator를 추가했다. 언어 중립 manifest와 JSONL fixture가 protocol/schema 버전, method/event 목록, 한계값, canonical encoding, valid/invalid transcript를 단일 기준으로 소유한다. 요청 ID 중복, 응답 상관관계, sidecar instance 변경, stream seq 누락·역전, 필수 run ID 누락, unknown kind/method/event/version, malformed JSON, outcome 불명 오류의 retryable 설정을 fail-closed로 거부한다. `system.ready` identity/capability와 `app.prepareQuit`의 `safeToQuit`/blocker 계약을 고정하고, renderer 공개 타입은 versioned system status·prepare quit·event 구독만 선언했다. 실제 preload wiring, sidecar I/O, SQLite/migration은 만들지 않았다.
- **검증**: `cargo fmt -p hello-agent-protocol -- --check`, `cargo check -p hello-agent-protocol`, `cargo test -p hello-agent-protocol`(3 PASS), `node --test --experimental-strip-types tests/m5-protocol-contract.test.mts`(4 PASS), 전체 Node unit(33 PASS), `cargo test --workspace --manifest-path Cargo.toml`(Tauri 105 PASS/13 기존 ignored, core 66 PASS, protocol 3 PASS), direct `tsc`+`vite build`, `git diff --check`를 통과했다. `node scripts/m5-harness.mjs --list`는 G2 READY/G3 NOT_READY를 보고했다. 별도 worktree의 `pnpm build`와 누적 G2 harness는 로컬 `node_modules` 부재 뒤 registry `ENOTFOUND`로 자동 설치가 막혀 완료하지 못했으며, 통합 worktree에서 frozen install 복구 후 다시 실행해야 한다.
- **다음 할 일**: 이 commit의 독립 계약 리뷰 뒤 통합 branch에서 frozen dependency 설치와 G2 누적 harness를 통과시키고, fixture를 바꾸지 않는 G3 sidecar transport/SQLite/preload 구현을 분리 착수한다.

### 2026-08-25 · by GPT-5 Codex (G1 마감·G2 착수 준비)

최신 통합 `9487934`에서 G1 Electron/Rust 기반과 독립 QA의 `G1_READY` 기록을 마쳤다. 새 G2 기록을 추가하기 전에 500줄이 된 워크로그의 2026-08-24 엔트리 전체를 날짜 경계대로 `archive/history-2026-08.md`로 이동해 현재 파일을 300줄 이하로 줄였다. 다음 작업은 protocol v1·SQLite schema·golden recovery fixture를 한 계약 담당이 먼저 고정하는 것이며, sidecar/Electron 구현은 그 fixture 승인 전 시작하지 않는다.

### 2026-08-25 · by GPT-5 Codex (M5 G1 Electron Forge 8 scaffold)
- **한 일**: 별도 `m5/electron-forge8-spike`에서 Forge 8 alpha scaffold, local-only M5 renderer, `app://m5` ASAR handler, navigation/webview 차단, narrow preload, Fuse/ASAR packaging과 G1 실제 harness 명령을 추가했다. 독립 리뷰 P1의 `file:` 추가 권한을 V1 9개 strict fuse와 실제 package API/ASAR smoke로 닫고 승인받아 `0b9f769`까지 `m5/coding-agent-ide`에 fast-forward 통합했으며 현행 구조를 `architecture.md`에 반영했다.
- **검증**: 최신 통합 `8374a2a` QA가 `G1_READY`로 판정됐다. frozen pnpm install(hoisted), `npm test` 29 PASS, Tauri 105 pass/13 ignored, core 66 PASS, macOS arm64 package·ASAR renderer·V1 fuse 9개·임시 user-data 기동이 통과했으며 증거는 `.artifacts/m5-harness/g1-foundation-2026-08-25T03-48-40-974Z.json`이다.
- **다음 할 일**: G2 protocol/SQLite 계약을 시작한다. Forge alpha, Windows/Linux package, macOS 기본 plist의 광범위한 ATS/미디어 문자열 hardening, 정상 Quit 자동화 미검증, 실제 사용자 프로젝트·유료 에이전트 smoke는 후속 또는 별도 승인 범위다.

### 2026-08-25 · by GPT-5 Codex (M5 G1 Rust 통합·Electron 공급망 차단)

- **한 일**: G0 계약을 고정하고 Rust/Electron worktree를 병렬 착수했다. Rust core 추출 `a8ca909`와 리뷰 수정 `bacf273`을 독립 리뷰 승인 뒤 `m5/coding-agent-ide`에 fast-forward 통합했다
- **검증**: fresh baseline과 통합 후 `pnpm test:harness` PASS, workspace 171개 통과·13개 기존 ignored, sidecar check와 diff check 통과
- **차단**: Forge `7.11.2` 설치가 pnpm `ERR_PNPM_EXOTIC_SUBDEP`로 실패했다. `blockExoticSubdeps: false`는 전이 Git/tarball 전체를 허용하므로 적용·커밋하지 않았다
- **다음 할 일**: 사용자가 Forge 8 prerelease 실험, 공급망 예외 승인, 또는 Forge stable 대기/패키징 기준 변경 중 하나를 결정하면 Electron scaffold 검증·리뷰·통합을 재개한다

### 2026-08-25 · by GPT-5 Codex (M5 G1 Rust core 리뷰 P1 수정)

독립 리뷰에서 루트 Cargo workspace가 기본 `target/`을 만들지만 저장소 ignore가 `src-tauri/target/`만 전제로 해 생성물이 추적 후보가 된 P1을 확인했다. `.gitignore`에 저장소 루트만 가리키는 `/target/` 한 줄을 추가했으며 다른 ignore 규칙은 바꾸지 않았다. `cargo test --workspace --locked -q`에서 Tauri 105개와 core 66개가 통과하고 기존 13개 승인 필요 테스트만 무시됐으며, 이후 `git status --short`에 `target/`이 나타나지 않고 `git diff --check`가 통과했다.

**다음 할 일**: 이 수정 commit을 포함해 독립 재리뷰와 통합 branch 하네스를 수행한다.

### 2026-08-25 · by GPT-5 Codex (M5 G1 Rust 안전 코어 추출)

`m5/rust-core-extraction` worktree에서 루트 Cargo workspace와 canonical `Cargo.lock`을 만들고, 기존 Tauri crate의 `Agent`·`AppError`·프로세스 트리 종료 계약(F1~F8)·Claude/Codex 실행 계약 및 66개 직접 테스트를 `crates/hello-agent-core`로 옮겼다. Tauri 쪽 기존 모듈은 core를 다시 내보내는 한 줄 shim이라 `handoff.rs`와 `orchestration.rs`는 수정하지 않았다. `hello-agent-sidecar`는 core를 링크하는 빈 실행 골격뿐이며 protocol·SQLite·Electron 계약은 추가하지 않았다. core 직접 의존성은 `serde`·`serde_json`·macOS 프로세스 조회용 `libc`뿐임을 `cargo tree`로 확인했다.

검증은 `cargo test --workspace --offline`에서 Tauri 105개와 core 66개가 통과하고 기존 13개 승인 필요 테스트만 무시됐으며, `cargo test --manifest-path src-tauri/Cargo.toml --offline`, `cargo check -p hello-agent-sidecar --offline`, 소유 파일별 rustfmt, `git diff --check`가 통과했다. F1 재부모화, F2 유예 중 자손, F3 별도 그룹, F4 신호 실패, F5/F7 PID 재사용, F6 정상 종료 뒤 배경 자손, F8 일시적 조회 무결성 실패를 포함한 기존 실패 주입을 core에서 그대로 실행했다. 정확한 `cargo fmt --all -- --check`는 이번 변경 전부터 있던 소유 밖 `src-tauri/src/detect.rs`와 `src-tauri/src/login.rs` 포맷 차이를 보고해 실패하며 두 파일은 계약대로 건드리지 않았다. 별도 worktree의 `pnpm test:harness`는 `node_modules`가 없어 네트워크 설치 단계에서 중단했고, 통합 담당이 의존성이 준비된 통합 branch에서 실행하기로 했다.

**다음 할 일**: 독립 코드 리뷰에서 추출 경계와 기존 M4 의미 보존을 확인하고, 통합 branch에서 하네스·macOS/Windows QA를 통과한 뒤 G2 protocol/SQLite 계약을 별도 작업으로 시작한다.

### 2026-08-25 · by GPT-5 Codex (M5 구현 착수 승인)

사용자가 M5 기능 커밋·통합 상시 승인 규칙을 커밋하고 실제 구현을 시작하라고 승인했다. 이 규칙 변경을 `m5/coding-agent-ide`의 새 기준 commit으로 만든 뒤, 같은 commit에서 G0 baseline 하네스와 `m5_architect`의 경계·의존성 분석을 병렬 실행한다. 둘 다 통과하면 G1 Electron scaffold와 Rust core 추출을 겹치지 않는 기능 branch/worktree로 분기하며, 이후 작업 계약 범위의 기능 커밋과 검증된 통합은 재승인 없이 진행한다. push와 `main` 병합은 하지 않는다.

**다음 할 일**: 자동 승인 기준 commit 생성 → G0 baseline/아키텍처 분석 → G1 작업 계약과 worktree 분기.

### 2026-08-25 · by GPT-5 Codex (M5 기능 커밋·통합 상시 승인 명시)

사용자가 M5 작업 중 기능 커밋과 통합 승인 여부를 매번 묻지 않도록 상시 승인 범위를 명시해 달라고 요청했다. 작업 계약에 지정된 기능 worktree의 원자적 구현·테스트·리뷰 수정 커밋과, 독립 리뷰 및 해당 하네스·QA를 통과한 commit의 `m5/coding-agent-ide` 통합은 M5 전체 완료까지 자동 승인된 것으로 루트 작업 규칙과 오케스트레이션 절차에 고정했다. 구현 담당의 merge/rebase/cherry-pick/push 금지와 통합 담당 단일 소유는 유지한다. push, `main` 병합, 범위 밖 변경, 안전·제품 확정 결정 변경, 실제 유료 에이전트·사용자 프로젝트·파괴적 시험, 외부 게시는 계속 별도 승인을 받는다. 아직 이 규칙 변경 자체는 커밋하지 않았다.

**다음 할 일**: 이 규칙 변경을 기준 commit으로 만든 뒤, 별도 재승인 없이 Gate 0와 G1 기능 branch/worktree 작업을 시작한다.

### 2026-08-25 · by GPT-5 Codex (M5 기계 검증 하네스 커밋 승인)

사용자가 M5 기계 검증 하네스와 CI 연결 변경의 커밋을 승인했다. `baseline` 실제 통과 증거, G1~G6의 누적 `NOT_READY` 게이트, 하네스 자기 테스트, macOS/Windows artifact 업로드와 연결 문서를 하나의 독립 기준선으로 `m5/coding-agent-ide`에 기록한다. `main` 병합과 push는 하지 않는다.

**다음 할 일**: 이 기준 commit에서 G1 기능 branch/worktree를 분기하고 `m5:test:electron`·`m5:test:core`를 실제 scaffold/core 검증에 연결한다.

### 2026-08-25 · by GPT-5 Codex (M5 기계 검증 하네스 선설정)

사용자 요청에 따라 M5 구현 전에 모든 작업자와 통합 담당이 같은 검증을 실행하도록 `scripts/m5-harness.mjs`를 추가했다. 현재 `baseline`은 diff check, 프런트 빌드·unit, 기존 Rust 회귀를 실제 실행하며, G1~G6는 명세의 단계별 검증 script 이름을 예약하고 앞 단계를 누적 실행한다. 구현되지 않은 script는 성공이나 skip이 아니라 `NOT_READY`와 exit code 2로 판정한다. 실행 결과는 branch·commit·dirty 상태·명령·exit code·소요 시간·stdout/stderr를 담은 versioned JSON으로 `.artifacts/m5-harness/`에 남긴다. macOS/Windows CI도 같은 명령을 쓰고 결과 artifact를 보존하도록 연결했다. 하네스 목록과 미준비 판정 회귀 2개를 포함한 프런트 26개, Rust 171개, 빌드와 diff check가 `baseline`에서 통과했으며 증거는 `.artifacts/m5-harness/baseline-2026-08-25T02-11-08-072Z.json`이다. 아직 커밋하거나 push하지 않았다.

**다음 할 일**: 이 하네스 설정을 기준 commit으로 만들고, G1 담당이 `m5:test:electron`과 `m5:test:core`를 실제 검증 명령에 연결한다.

### 2026-08-25 · by GPT-5 Codex (M5.0 명세·멀티에이전트 기준선 커밋 승인)

사용자가 M5 구현 전에 준비한 Electron/Rust 수직 스파이크 명세와 멀티에이전트 운영·역할 설정 묶음의 커밋을 승인했다. 커밋 범위는 `m5-electron-spike.md`, `.codex/config.toml`과 M5 역할 8개, 공통 오케스트레이션 절차, `.worktrees/` 제외 규칙, 연결된 루트 작업 규칙·문서 색인·워크로그로 한정한다. `m5/coding-agent-ide`에 M5.0 착수 기준선으로 기록하며 `main` 병합과 push는 하지 않는다.

**다음 할 일**: 이 기준 commit에서 Gate 0 기준선 검증을 수행하고, 확정된 작업 계약에 따라 첫 기능 branch/worktree를 분기한다.

### 2026-08-25 · by GPT-5 Codex (M5 프로젝트 서브에이전트 역할 설정)

사용자 요청에 따라 문서 절차에 그치지 않고 Codex가 실제로 읽는 프로젝트 설정을 추가했다. 공식 Codex 문서에서 신뢰된 프로젝트의 `.codex/config.toml`, `.codex/agents/*.toml`, 역할별 `model`·`model_reasoning_effort`·`sandbox_mode`, 동시 thread 상한을 확인했다. 기본 동시 상한은 3개로 두고, M5 설계·계약·Electron·Rust 안전 코어·UI·독립 코드 리뷰·통합 QA·최종 게이트의 8개 역할을 책임과 권한별로 분리했다. 설계·리뷰·게이트는 읽기 전용이고, 구현 역할도 매 작업의 별도 worktree·소유 파일 계약 없이는 범위를 얻지 않는다. 직전 엔트리의 “공식 근거가 없는 임의 `.codex` 설정 키는 만들지 않았다”는 당시 조사 상태였으며, 이번에 확인한 공식 프로젝트 역할 형식으로 보완·정정한다. 아직 커밋하거나 push하지 않았다.

**다음 할 일**: 설정과 M5.0 구현 명세를 함께 사용자 검토·승인한 뒤 기준 commit으로 만들고, 그 commit에서 `m5_architect`의 Gate 0 분석과 역할별 기능 worktree를 시작한다.

### 2026-08-25 · by GPT-5 Codex (멀티 에이전트 저장소 설정 추가)

사용자 지적대로 M5 구현 지시보다 먼저 멀티 에이전트 운영 기반을 저장소에 추가했다. 공식 OpenAI 문서가 권장하는 명시적 위임 범위·테스트 기대치·합격/중단 조건을 반영하되, 공식 근거가 없는 임의 `.codex` 설정 키는 만들지 않았다. 대신 `docs/procedures/multi-agent-orchestration.md`에 통합 담당·작업 계약·branch/worktree 격리·단일 파일 소유·파동식 실행·독립 리뷰/QA·중단 게이트를 고정하고, `AGENTS.md`/`CLAUDE.md`에 필수 규칙을 동일하게 반영했다. `.worktrees/`를 Git에서 제외하고 M5.0 명세와 문서 색인을 새 절차에 연결했다. 변경은 아직 커밋하지 않았다.

**다음 할 일**: 설정 변경과 M5.0 구현 명세를 사용자 검토 뒤 기준 commit으로 만들고, 그 commit에서 Gate 0 검증과 첫 기능 worktree를 시작한다.

### 2026-08-25 · by GPT-5 Codex (M5.0 멀티 에이전트 오케스트레이션 제안)

M5.0 구현을 여러 에이전트에게 맡길 운영 방식을 저장소 경계·코드 리뷰·QA 관점에서 독립 검토했다. `m5/coding-agent-ide`는 통합 전용으로 두고 실제 별도 Git worktree/기능 브랜치에서 파일 소유권을 나누며, 한 명의 오케스트레이터만 통합·공유 manifest/lockfile·최종 워크로그를 관리하는 구조가 적합하다. 구현자는 동시에 2~3명으로 제한하고 `명세 커밋 → Electron scaffold/Rust core → protocol fixture 고정 → sidecar와 renderer 병렬 → 통합 hardening`의 파동식 순서를 권장한다. 각 기능은 독립 코드 리뷰와 최신 통합 브랜치 QA를 통과한 뒤에만 병합하며, 실제 macOS 에이전트 smoke는 비용·로그인·사용자 프로젝트 영향 때문에 별도 사용자 승인 게이트로 둔다. 이번에는 운영 방안만 분석했고 구현·커밋·push는 하지 않았다.

**다음 할 일**: 사용자가 현재 미커밋 M5.0 명세 커밋과 기능 브랜치 커밋/통합 권한의 범위를 승인하면, 기준 커밋에서 worktree를 만들고 G0 기준선 검증부터 시작한다.


### 2026-08-24 및 그 이전 → `docs/archive/history-2026-08.md`

2026-08-12 ~ 2026-08-24 엔트리를 날짜 경계로 아카이브했다. M4의 현재 결론은 `docs/proposals/m4-orchestration.md`에 있으므로 통상 아카이브를 열 필요는 없다.

### 2026-07-23 및 그 이전 → `docs/archive/history-2026-07.md`

17개 엔트리(2026-07-16 ~ 2026-07-23)를 아카이브로 옮겼다. 재사용할 결정·함정은 위 `핵심 결정·발견`에 반영돼 있으므로, 통상 아카이브를 열 필요는 없다.
