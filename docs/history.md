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
- **M4 S1 교차 검토 상태**: `PresetOutcome::Applied`는 설정 파일을 새로 썼다는 사실만 나타내며 유효한 안전 정책을 보장하지 않는다. 화면 문구도 파일 작성 사실로 한정하고, 권한 팁은 기본 설정 기준·예외 가능성을 밝힌다. Claude가 Codex 경로의 생성·보존·직렬화 테스트까지 보완했고 2026-08-12 Codex 재검토를 통과했다. **S1은 종결됐고 S2도 통과했다**(아래 항목) — 현재 진행 지점은 §7.10 최소 자동 인계의 Claude 실행 계약이다.
- **M4 Claude 실행 계약(2026-08-19 실측, CLI `2.1.235`)**: 읽기 전용을 강제하는 것은 **`--tools` 허용목록**뿐이다. `--disallowed-tools`는 거부목록이라 `Task`·`ToolSearch` 등 23개를 남기고 **거부 증거도 안 남긴다**. `--setting-sources project`는 **대상 프로젝트 설정을 로드해** 권한을 `bypassPermissions`로 승격시키고 훅·MCP를 실행시키므로 안전 정책이 아니다 → **`--setting-sources ''`** 를 쓴다. `--safe-mode`는 훅·MCP·스킬만 끄고 **권한 승격은 못 막는다**. `--permission-mode dontAsk`는 자동 승인이 아니라 거부 게이트이며 `permission_denials`로 기계 판독 가능한 증거를 남긴다. **작업 폴더 경계와 사후 빈 diff는 예방책이 아니라 사후 확인 수단**이다(경계는 복제본 *안* 쓰기를 허용한다). 실행 형식은 **`--output-format stream-json --verbose` 고정**이고 `system:init`을 fail-closed로 검증한다(`tools` 집합 일치·`permissionMode`·`mcp_servers`·`plugins`·`model`·`cwd`·`session_id`). ⚠️ **`--json-schema`는 요청하지 않은 `StructuredOutput` 도구를 주입**하므로 기대 도구 집합은 4개다. §7.10 파일럿은 **macOS 전용**(Windows 종료 계약 미실측). 상세는 `proposals/m4-orchestration.md` §7.10.1.2
- **Claude CLI 프로세스 트리·종료(2026-08-19 실측, Codex 2차 검토 반영)**: Bash 도구 자식은 **부모 프로세스 그룹에 없고 별도 세션 리더**(`Ss`)다. 그룹 `TERM`으로 죽는 것은 Claude 자체 정리 동작일 뿐이라, **루트에 `SIGKILL`을 보내면 자손이 남는다**(15초까지 생존 확인). **종료 성공은 신호 결과가 아니라 관측으로만 판정한다** — 한 번이라도 관측한 **`(pid, 시작 시각)` 신원을 누적 추적**하고 **누적 집합 전체의 소멸을 확인**해야 성공이다. ⚠️ **루트 기준 ppid 트리 순회는 오판한다** — 부모가 먼저 죽으면 자손이 `ppid=1`로 재부모화돼 트리에서 사라지고, 살아 있는데 빈 집합으로 보인다(C6로 재현). 열거는 **살아 있는 모든 추적 대상**에서 출발한다. **PID 재사용(신원 불일치)과 조회 실패는 `poisoned`**. `killpg`의 **`EPERM`은 성공도 소멸 증거도 아니다**(좀비만 남은 그룹에서도 난다). **확인 불가는 곧 `poisoned`** — 실행과 복제본을 함께 표시하고 자동 인계·재사용을 차단하며 해소는 명시적 복구/폐기뿐. `run_in_background` 때문에 **정상 종료 경로에서도** 회수가 필요하다. 상세는 `proposals/m4-orchestration.md` §7.10.1.2 ⑤
- **M4 §7.10 최소 자동 인계 — 구현 완료(2026-08-20 Codex 4차 반려 반영, 미커밋)**: 모듈 3개가 새로 생겼다. `proctree.rs`(종료 계약, F1~F6 회귀 픽스처), `claude_pilot.rs`(실행 계약 + `system:init` fail-closed 검증), `handoff.rs`(사용자 1회 입력 → 계획 → **앱이 인계** → 구현). 화면은 `App.tsx`의 `HandoffPilot`이고 **macOS에서만 노출**한다(`handoff_available`). 신원 조회는 `libc::proc_pidinfo(PROC_PIDTBSDINFO)`를 쓴다 — `ps` 파싱은 로캘 의존이라 쓰지 않는다. 실사용 통합 테스트 2건(`claude_planning_leg_*`, `handoff_vertical_slice_*`)은 `#[ignore]`이며 **`env -i`로 돌리면 인증이 사라져 실패한다**. 남은 것은 §7.6A UX 판정이며, 구현은 그 판정을 *할 수 있게* 만든 것이지 판정이 아니다
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

### 2026-08-24 · by Claude Opus 5 (§7.6A 통과 — 가설 A 성립)

**사용자가 결함 6건 수정 뒤 Codex 프로젝트로 끝까지 돌려 §7.6A를 통과로 판정했다.** M4의 필수 관문이 열렸다.

관측된 증거(`m4-pilots` 원장에 남아 있음):

| 항목 | 결과 |
| --- | --- |
| 계획 단계 diff | **0바이트** — 읽기 전용이 실물에서 지켜졌다 |
| 구현 단계 | `index.html`·`styles.css`·`script.js`·`README.md` 생성 |
| 진행 상태 | `요청 정리 중 → 만드는 중 → 확인 중 → 완료` |
| 사용자 입력 | 1회. 에이전트 사이 전달 **0회** |

**가설 A가 성립한다** — 사용자가 에이전트 사이의 전달자에서 빠질 수 있다. A2·A3가 이 마일스톤의 존재 이유였고, 그것이 확인됐다.

**가설 B(§7.6B 품질·비용 비교)는 여전히 보류다.** 통과한 것은 A뿐이며, 재작업 루프·다역할 같은 고비용 확장의 게이트는 B다.

이어서 계획 단계 증거 이름을 역할 기준으로 정리했다(`hello-agent-claude.*` → `hello-agent-plan.*`, `plan_to_codex_prompt` → `plan_to_implementation_prompt`). 내용은 Codex 계획 실행인데 파일 이름이 Claude를 가리켜 로그를 잘못 읽게 만들던 문제다. 모듈 이름 `claude_pilot`·`codex_pilot`은 실제로 각 에이전트의 실행 계약이라 그대로 뒀다.

**커밋**: `50b2c60`(결함 6건) · `1a772ec`(문서) · `d0d1e91`(이름 정리). `m4/orchestration`이 `main`보다 11개 앞서며 **병합·push 없음**.

**다음 할 일**

- 사용자가 제품 방향을 밝혔다 — **지금까지의 Tauri 앱을 "튜토리얼" 개념으로 넘기고, 거기서 IDE로 발전**시킬 계획. 범위가 크게 바뀌므로 `docs/proposals/`에 별도 문서로 심의해야 한다. **현재 제품 범위(`CLAUDE.md`)와 §7.10.2 화면 계약이 전제하는 대상 사용자가 유지되는지부터 확정할 것**
- §7.6B(가설 B)는 보류 유지
- (별건) Windows 게이트·S2 `EPERM` 판정 검토

---

### 2026-08-24 · by Claude Opus 5 (§7.6A 2차 — Codex 경로 결함 4건)

사용자가 Codex로 프로젝트를 만들었는데 파일럿이 Claude 인증을 요구하며 실패했다. 파고 보니 **결함이 네 겹**이었다.

| # | 결함 | 상태 |
| --- | --- | --- |
| 1 | 파일럿이 `approval_policy="untrusted"`를 넘겨 **Codex 실행이 무조건 실패** | 고침 |
| 2 | 설치가 `codex`만 받아 **셸 실행 호스트가 없음** → 명령을 하나도 실행 못 함 | 고침 |
| 3 | 파일럿이 프로젝트 에이전트를 무시하고 **Claude를 하드코딩** | 계약 모듈까지 완료, **배선 남음** |
| 4 | 안전 프리셋이 `~/.codex/config.toml`에 `untrusted`를 써 **신규 사용자의 Codex를 망가뜨림** | 고침 |

**4번이 가장 위험했다.** 사용자 설정 파일이 이미 있으면 안 건드리는 규칙 덕에 이 사용자는 피했지만, 앱으로 처음 시작하는 사람은 Codex가 통째로 안 됐을 것이다.

**2번은 오판을 부르는 종류의 결함이다.** 호스트가 없으면 명령이 실행조차 안 되는데 **파일이 안 생기니 "샌드박스가 막았다"로 읽힌다.** 실제로 첫 실측이 그렇게 나왔고, 호스트를 옆에 두고 다시 재서야 진짜 차단이 확인됐다. 격리 설치 E2E에 호스트 존재 단언을 넣었고, 수정을 되돌리면 정확히 그 자리에서 실패한다(실제 네트워크로 확인).

**Codex CLI 실측 (0.149.1)**

| 항목 | 결과 |
| --- | --- |
| `--sandbox read-only` 쓰기 차단 | **확인** — 샌드박스 안 `EXIT=1`, `operation not permitted`, 파일 미생성 |
| `--output-schema` 구조화 계획 | **동작** — Claude와 같은 스키마로 한국어 계획 산출 |
| `approval_policy` 유효값 | `on-failure`·`on-request`·`granular`·`never`. **`untrusted`는 파싱은 되지만 별도로 차단** |
| `system:init` 대응물 | **없음** — `thread.started{thread_id}`뿐 |
| 릴리스 자산 | `codex-{target}.tar.gz`와 `codex-code-mode-host-{target}.tar.gz`가 **따로** 배포됨 |

**Codex 계획 역할 계약을 만들었다**(`codex_pilot.rs`, 테스트 15건). 계약 강도가 Claude와 **같지 않고, 그 차이를 문서에 적었다** — Codex는 실행 구성을 이벤트로 알려주지 않아 `system:init` 같은 fail-closed 검증이 **불가능**하다. 대신 argv 통제 + 실측된 샌드박스 + 사후 빈 diff에 기대며, 마지막은 예방책이 아니라 사후 확인이다.

**3번 배선 완료 — 두 에이전트가 하나의 감독을 받는다**

`handoff.rs`의 실행 감독부를 공유 구조로 바꿨다. `PlanRole`이 **다른 것 넷만** 담고(인자·프롬프트 전달·stdout 해석·계획 추출), 신원 추적·취소 폴링·종료·증거는 `supervise_plan` 하나가 처리한다. 감독부가 복제되면 종료 계약(F1~F8)이 갈라지므로, **감독부가 하나뿐이고 누적 신원 추적을 쓰는지**를 소스로 검사하는 테스트를 넣었다.

이름도 정리했다: `ClaudeEvidence`/`ClaudeRun`/`ClaudeFailure` → `PlanEvidence`/`PlanRun`/`PlanFailure`. 더는 Claude 전용이 아니다.

`execute_handoff`와 `run_handoff`가 **프로젝트 에이전트**를 받고, 화면이 카드에 보여 준 그 에이전트를 그대로 넘긴다.

**배선하며 나온 5번째 결함**: `--ignore-user-config`를 쓰면 **신뢰 폴더 목록도 비어** codex가 실행을 거부한다 — `Not inside a trusted directory and --skip-git-repo-check was not specified`. **계획·구현 두 경로 모두** 이 플래그가 없어 실행이 시작조차 못 했다. 양쪽에 넣고 검증에도 필수로 걸었다. 대상이 앱이 만든 Git 복제본이라는 것은 `validate_pilot_workspace`가 이미 검증하므로, codex의 중복 확인을 건너뛰는 것뿐이다.

**Codex 계획 구간 실사용 검증 통과**

```
session    : 01a031d7-72a2-7451-aa19-4a0f13942af9
steps      : 3
termination: Confirmed after 1 round(s); 2 identity(ies) tracked; survivors: none observed
```

빈 diff까지 확인됐다(계획 역할이 아무것도 안 바꿈). ⚠️ 다만 계획 요약에 **`저장소 조회 도구가 실행 환경 오류로 동작하지 않아 현재 문구는 확인하지 못했으며`** 가 남았다 — 이 기기의 `~/.local/bin/codex`에 아직 셸 호스트가 없어서다. 즉 **Codex가 저장소를 읽지 못한 채 계획했다.** 앱으로 Codex를 다시 설치하면 해소된다.

**검증**: `cargo test --locked` **171 통과 / 0 실패 / 13 무시 · 경고 0**, `npm test` 24 통과, tsc·빌드·rustfmt·`git diff --check` 통과. 격리 설치 E2E와 Codex 계획 구간 E2E 모두 실제로 실행해 통과.

**6번째 결함 — 망가진 설치를 고칠 방법이 없었다**

"앱에서 Codex를 다시 설치하세요"라고 안내했는데, **앱에 그 경로가 없었다.** 홈의 에이전트 버튼은 세 경우에만 뜬다 — 미설치·미로그인·최신 버전 있음. 사용자의 Codex는 **이미 최신**이라 아무 버튼도 없었다. 설치가 망가졌는데 앱은 "설치됨"이라 말하고, 고칠 수단은 없었다.

`AgentStatus.needs_repair`를 추가했다. **설치·로그인·최신이 다 참이어도 실제로 못 쓰는 상태**를 판정한다 — Codex는 `codex` 옆에 `codex-code-mode-host`가 있어야 한다. 그런 상태면 화면이 "고쳐야 해요 / 고치기"를 보여주고 다시 설치한다.

**남은 제한**

- **Codex 계획 계약은 Claude보다 약하다** — `system:init` 대응물이 없어 실행 구성을 검증할 수 없다. argv 통제 + 실측된 샌드박스 + 사후 빈 diff에 기댄다
- `on-request`의 구체적 동작은 미실측. 실제 경계는 `sandbox_mode`다
- **사용자의 기존 `~/.local/bin/codex`에는 여전히 셸 호스트가 없다.** 앱으로 Codex를 다시 설치해야 저장소를 읽고 계획할 수 있다
- 교차 인계(Claude 계획 → Codex 구현)는 지금 구조로 열 수 있지만, 기본은 **프로젝트 에이전트 하나**다. 역할 선택 UI는 §7.10.2대로 기본 화면에 두지 않는다
- **§7.6A 재판정 필요** — 전체 수직 흐름(Codex 계획 → 구현)은 아직 사람이 돌려보지 않았다
- **작업 사본을 화면에서 볼 수 없다**(사용자 관찰, 2026-08-24) — 지금은 "바뀐 파일 목록"과 증거 경로뿐이라, 결과를 보관하기로 해도 그 사본을 열어 확인할 수단이 없다. 파일럿 범위 밖이며 **고도화 단계에서 다룬다**
- 커밋하지 않았다 — 워킹 트리에 있다

---

### 2026-08-24 · by Claude Opus 5 (§7.6A 1차 실사용 — A5 실패, 수정)

**§7.6A 실사용에서 첫 결함이 나왔다.** 사용자가 `pnpm tauri dev`로 앱을 띄우고 파일럿에 `hi`를 넣어 시작하니 `pilot source must be a Git repository`가 떴다.

**원인은 입력값이 아니었다.** 홈 화면이 `projects[0]`을 말없이 파일럿 대상으로 삼았고, `scan_projects`는 `CLAUDE.md`·`AGENTS.md`·`.claude` 같은 **에이전트 표식만** 보고 Git 저장소인지는 보지 않았다. 그래서 비Git 프로젝트에도 실행 버튼이 살아 있었고, 실행 뒤 `prepare_pilot_copy`에서야 거절하며 내부 영문 오류를 그대로 뱉었다.

**A5 실패다** — 비개발자가 전문 영문 오류를 해석해야 했다. **§7.6A는 아직 통과가 아니며 사람이 다시 테스트해야 한다.**

| 고친 것 | 어떻게 |
| --- | --- |
| 암묵적 선택 | `projects[0]` 제거. 프로젝트 카드의 **맡기기** 버튼으로 고른 프로젝트에만 파일럿이 열린다. `projects.find(p => p.pilotEligible)`도 쓰지 않았다 — 여전히 사용자가 고르지 않는다 |
| 사전 판정 | `scan_projects`가 `pilotEligible`을 실어 보낸다. 판정은 **실행과 같은 함수**(`can_copy_pilot_source`)를 쓴다 |
| 비Git 안내 | 카드에 "아직 맡길 수 없어요", 파일럿 안에 "이 프로젝트는 아직 안전한 작업 사본을 만들 수 없어요". **`git init`은 하지 않는다** |
| 입력 문구 | "무엇을 하고 싶으세요?" → **"이 프로젝트에서 무엇을 바꾸고 싶으세요?"** 일반 채팅창처럼 보이던 것을 변경 작업 입력란으로 |
| 오류 노출 | 닥터에 `notgit` 분류 추가. 영문 원문은 "자세히 보기" 안에만 남는다 |

**표시용 판정과 권한 경계를 구분했다.** `can_copy_pilot_source`는 **화면이 거짓말하지 않게 하려는 것**일 뿐이고, 최종 거절은 언제나 `prepare_pilot_copy`가 한다 — 화면을 그린 뒤 `.git`이 사라질 수 있다(TOCTOU). 그 경로도 회귀 테스트로 고정했고, 그때 사용자가 보는 것은 영문 원문이 아니라 지역화된 안내다.

**회귀 테스트 8건 추가** (Rust 137 → 140, npm 9 → 23).

- Rust: 표시 판정과 실행이 같은 답을 내는가 · 저장소가 사라져도 백엔드가 계속 거절하는가 · 스캔 목록이 자격을 싣는가
- 프런트: 자동 선택 없음 · 비Git은 시작 동작이 호출되지 않음 · 명시적 선택 후 실행 가능 · **첫 프로젝트가 비Git이어도 뒤의 Git 프로젝트가 대신 골라지지 않음** · 목록 새로고침 시 선택 유지/해제 · 영문 오류 미노출

세 결함(첫 프로젝트 자동 선택 · 자격 무시 · 오류 원문 노출)을 각각 되돌려 정확히 그 자리에서 실패함을 확인했다.

**검증**: `cargo test --locked` **140 통과 / 0 실패**, `npm test` **23 통과 / 0 실패**, `tsc --noEmit`·`pnpm build` 통과, 변경 Rust `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과.

**변경 파일**: `src-tauri/src/{orchestration.rs,project.rs}`, `src/{App.tsx,App.css,doctor.ts,handoffTarget.ts(신규)}`, `src/locales/{ko,en}.ts`, `tests/{handoff-target,doctor-pilot}.test.mts`(신규), `docs/{history.md,proposals/m4-orchestration.md}`

**후속 3 (같은 날) — "Hi"만 넣었을 때의 올바른 결과**

사용자가 `Hi`만 넣고 "안전하게 멈췄어요"를 봤다. **증거 파일을 열어보니 `Hi`와 무관한 실패였다.**

```
Failed to authenticate. API Error: 401 OAuth access token has been revoked.
```

토큰이 만료돼 계획 단계가 죽은 것이었다(지금은 정상 — 일시적이었다). 그래서 로그인을 복구한 뒤 **`Hi`를 실행 계약 그대로 재실측**했다. Claude는 정확하게 행동한다:

```json
{ "steps": [], "summary": "the user's message (\"Hi\") is a greeting, not a task ... Recommend asking the user what they'd like to build" }
```

**의도 분류기가 필요 없다** — 계획 역할이 이미 그 판단을 한다. 동작(멈춤·Codex 미실행·무변경)도 이미 맞았다. 틀린 것은 **보여주는 방식**이었고, 결함 두 개를 고쳤다.

| 결함 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| ② 원인 유실 | `structured_output`이 없으면 무조건 `"run produced no structured plan"`으로 덮어써, **실제 원인(인증 만료)이 사라졌다.** ⚠️ `structured_output: null`은 `get`이 `Some(Null)`을 주므로 대체 경로도 오류 보고도 건너뛴다 | `null`을 걸러 내고, CLI가 말해 준 원인을 그대로 전한다. 닥터에 `auth` 분류를 추가해 **"로그인이 풀렸어요 / 다시 로그인해 주세요"** 로 번역한다 |
| ① 되묻기를 사고로 취급 | `steps: []`가 다른 실패와 같은 "안전하게 멈췄어요" 화면으로 갔다. 잘못된 게 없는데 그렇게 말하면 초보자는 자기가 망가뜨린 줄 안다 | `HandoffStage::NeedsGoal`로 분리. **"무엇을 바꿀지 알려주세요"** + 예시 + 다시 적기. 진행 표시도 빨갛게 칠하지 않고, 물어본 적 없는 "바뀐 내용"도 보여주지 않는다 |

멈춘 화면은 이제 **닥터를 거친다** — 쉬운 말로 이유를 보여주고 영문 원문은 상세 안에만 남는다.

회귀 테스트 6건 추가(Rust 148 → 152, npm 23 → 24). 원래 코드로 되돌려 `원인이 사라졌다: structured plan is empty`로 정확히 실패함을 확인했다.

**남은 제한**: 계획 역할의 설명(`summary`)은 영어로 온다. 기본 화면에는 한국어 문구만 쓰고 그 설명은 "자세히 보기"의 *하려던 것*에 둔다 — 계획 프롬프트에 사용자 언어를 실어 보내는 것은 이번 범위 밖.

**후속 2 (같은 날) — 실사용에서 두 번째 결함**

준비 → 맡기기 → 시작까지 갔더니 `command run_handoff missing required key runId`가 떴다.

**Tauri 인자 문제가 아니라 직렬화 계약이 어긋난 것이었다.** `HandoffStart`에 `#[serde(tag="state", rename_all="camelCase")]`만 달아 뒀는데, `rename_all`은 **변형 이름만** 바꾼다. 변형 안의 필드는 `run_id` 그대로 나갔고(실측: `{"state":"ready","run_id":"run-1"}`), 화면의 `started.runId`는 `undefined`가 됐다. 그 값을 다음 `invoke`에 넘기니 **JS가 키를 통째로 빼 버려** 백엔드가 "필수 키 없음"으로 거절했다.

`rename_all_fields = "camelCase"`를 추가해 고쳤다(실측: `{"state":"ready","runId":"run-1"}`). `install.rs`의 `InstallEvent`는 **이미 둘 다 달고 있었다** — 프로젝트에 올바른 선례가 있었는데 따르지 않은 것이다.

**TypeScript가 못 잡는 종류의 버그다.** TS 인터페이스는 손으로 적은 선언일 뿐이라 백엔드가 실제로 내보내는 모양과 어긋나도 컴파일이 통과한다. 그래서 **화면이 읽는 키 이름 자체를 Rust 테스트로 고정**했다(3건). `rename_all_fields`를 빼고 돌려 정확히 그 자리에서 실패함을 확인했다.

프런트에도 방어를 뒀다 — 실행 ID가 비어 있으면 그대로 넘기지 않고 멈춘다. 빈 값을 넘기면 인자가 조용히 사라져 엉뚱한 곳에서 터진다.

**남은 함정**: `login.rs`의 `LoginEvent`도 `rename_all_fields`가 없다. 지금은 필드가 전부 한 단어(`url`·`line`·`success`)라 무해하지만, **여러 단어 필드를 추가하는 순간 같은 사고가 난다.** 이번 범위 밖이라 고치지 않고 기록만 남긴다.

**후속 (같은 날) — 막다른 길을 풀었다**

사용자가 "그럼 어떻게 풀어야 하는데?"라고 물어 확인해 보니, **`create_first_project`가 `git init`을 하지 않아 앱이 만든 프로젝트가 전부** "아직 맡길 수 없어요"에 걸리는 상태였다. 예외가 아니라 기본값이었다. 안내만 남기면 비개발자는 스스로 풀 수 없다.

사용자 승인을 받아 **"되돌릴 수 있게 준비하기"** 를 만들었다(`prepare_project_for_pilot` + `prepare_project_for_handoff` 명령).

- 확인 창에서 **무엇이 생기는지** 먼저 보여 준다 — "파일 내용은 바뀌지 않아요 / 폴더 안에 기록용 공간이 생겨요". 동의 없이는 실행되지 않는다
- `git init` + **첫 복원 지점 커밋**. 실측으로 `git init`만으로는 부족함을 확인했다 — 커밋이 없으면 `rev-parse HEAD`가 실패해 시작 지점을 못 잡는다
- **이미 있는 저장소의 이력은 건드리지 않는다.** 커밋이 없는 저장소면 시작 지점만 만들어 준다
- **다른 저장소 안의 폴더는 거부한다** — 중첩 저장소는 사용자의 기존 이력을 어지럽힌다. 남의 저장소에 손대는 것은 동의 범위 밖이다
- 커밋 신원이 없는 기기에서도 되게 **이 커밋에만** 앱 신원을 주입한다. `git config`(전역·로컬)는 건드리지 않는다
- 앱이 만드는 새 프로젝트는 처음부터 준비된 상태로. **실패해도 프로젝트 생성 자체는 성공**이다 — 편의 때문에 첫 프로젝트 만들기가 막히면 안 된다

회귀 테스트 5건 추가(Rust 140 → 145): 준비 후 실제로 복제본이 만들어지는가 · 동의 없이는 `.git`이 생기지 않는가 · 기존 이력 보존 · 중첩 거부 · 신원 없는 기기.

**남은 제한**

- **§7.6A 재판정 필요** — 이번 수정은 결함을 고친 것이지 판정이 아니다
- 파일이 아주 많은 폴더(`node_modules` 등)는 첫 커밋이 오래 걸릴 수 있다. `.gitignore`를 앱이 만들어 주는 것은 **원본에 파일을 더하는 일**이라 동의 범위를 넘어 이번에 하지 않았다
- 커밋하지 않았다 — 워킹 트리에 남겨 뒀다

---

### 2026-08-21 · by Claude Opus 5 (Codex 최종 통과 → 커밋)

**Codex 재검토를 통과했다.** 추가 차단 사항 없음. Codex가 `npm test` 9건, Rust 137건, TypeScript·Vite 빌드, rustfmt, `git diff --check`를 독립 검증했다.

사용자 승인을 받아 `m4/orchestration`에 **커밋 2개**로 나눠 넣었다. 각 커밋이 독립적으로 빌드·테스트를 통과한다.

| 커밋 | 내용 |
| --- | --- |
| `b6f2142` `feat(m4)` | `proctree`·`claude_pilot`·`handoff` 신규, `orchestration` 실패 구조화, 화면·프런트 테스트·검증 명령 |
| `70510ae` `docs(m4)` | §7.10 구현 상태, §9 철회된 주장, §10 결정 이력, 워크로그 아카이브 롤오버 |

**`main`에는 병합·push하지 않았다.** 현재 `m4/orchestration`은 `main`보다 8개 앞선다. 병합은 M4 전체가 완료·검증된 뒤 사용자가 명시적으로 승인한 경우에만 한다.

**다음 할 일**

- **§7.6A 자동 인계 UX 판정(A1~A5)** — 사람이 실제로 써 보고 내리는 판정. 구현이 끝나 막힌 것이 없다. M4의 다음 관문이다
- (별건) S2 `tree_is_gone`/`signal_ok`의 `EPERM` 오판 검토
- (별건, Windows 게이트) 자손 회수 실측 전까지 Windows에 노출하지 않는다

---

### 2026-08-21 · by Claude Opus 5 (Codex 8차 반려 — 준비 단계 취소 경합 1건)

7차 지적은 모두 해소 확인. 남은 경합 1건을 고쳤다. 여전히 **미커밋**이다.

**무엇이 틀렸나** — 화면이 정상 완료 뒤에도 이전 실행 ID를 들고 있었다.

1. 첫 실행이 끝난다 (화면은 `run-1`을 계속 보유)
2. 두 번째 실행 시작 → `begin_handoff` 응답 대기
3. 그 사이 중단 클릭 → ID가 있으니 "실행 중"으로 보고 **곧바로** `cancel_handoff`
4. 그 슬롯은 이미 끝난 실행의 것이라 **아무 일도 하지 않는다**
5. 새 ID가 와도 보류된 중단이 없어 **두 번째 실행이 그대로 진행**

끝난 실행의 늦은 진행 상태가 새 화면을 덮는 문제도 같은 뿌리였다.

**어떻게 고쳤나** — 실행 추적과 보류 중단을 `src/handoffRun.ts`의 **순수 전이 함수**로 분리했다. ref 두 개를 손으로 굴리던 것을 대체한다.

| 규칙 | 어디서 |
| --- | --- |
| 새 실행을 시작할 때 이전 ID를 버린다 | `startingRun()` — **`begin_handoff` 호출 전에** |
| 실행이 끝나면 비운다 | `runSettled()` — 결과·막힘·오류 **모든 경로** |
| 예약 전 중단은 보류했다가 예약 즉시 전달 | `requestCancel()` → `runReserved()` |
| 예약 대기 중 중단이 있었으면 **사본을 만들기 전에** 멈춘다 | `abort` — 그만두라는 작업의 사본을 남길 이유가 없다 |

**프런트 테스트를 처음 붙였다.** 새 프레임워크는 들이지 않았다 — `website/`가 이미 `node --test`를 쓰고 있어 같은 방식에 노드의 타입 제거 실행(`--experimental-strip-types`, Node 22.18)을 얹었다. `tests/handoff-run.test.mts` 9건을 `npm test`로 묶었다.

**결함을 되돌려 확인했다.** 처음 쓴 테스트 대역은 결함을 **가리고 있었다** — `finish()`가 이미 ID를 비워서, 시작 시 정리를 지워도 통과했다. 실제 옛 코드는 성공 경로에서 비우지 않았다. 대역에 `leaveStaleRunId()`를 넣어 그 상태를 재현하니 세 결함이 각각 잡힌다.

| 되돌린 결함 | 실패한 테스트 |
| --- | --- |
| 시작 시 이전 ID 미정리 | 예약 대기 중 중단 유실 · 늦은 진행 상태 덮어쓰기 (2건) |
| 종료 시 미정리 | 끝난 실행의 진행 상태 수신 (1건) |
| 보류된 중단 폐기 | 중단 전달 · 경합 시나리오 · 소비 확인 (3건) |

**검증**: `cargo test --locked` **137 통과 / 0 실패 / 12 무시**, `npm test` **9 통과 / 0 실패**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과.

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `package.json`, `src/App.tsx`, `src/handoffRun.ts`(신규), `tests/handoff-run.test.mts`(신규)

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 7차 반려 3건 + 추가 1건 반영)

6차 지적 3건은 해소 확인. 남은 것들을 고쳤다. 모두 **사용자가 막힌 상태에서 빠져나올 수 있는가**에 관한 문제였다. 여전히 **미커밋**이다.

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 복구 흐름이 막힘 | 잠금 **오판**은 고쳤지만, 잠긴 A가 슬롯을 쥐면 새 B는 전부 거부되는데 화면에는 B만 보였다. B를 정리해도 슬롯은 A의 것이라 안 풀려 **후속 실행이 영영 거부**된다 | `begin_handoff`가 **사본을 만들기 전에** 막고(`HandoffStart::Blocked`) A의 경로·사유·잠금 상태를 돌려준다. 화면이 그 경로로 A를 정리·복구한다. 우회 경로용으로 `PilotFailure.blocker`도 같은 정보를 싣는다 |
| 증거 생성 도중 부분 실패가 축약됨 | `FailureContext`를 **파일 생성 뒤에** 만들어서, events는 만들어졌는데 stderr에서 실패하면 그 경로를 잃었다. 만들어진 events는 아무도 모르는 고아가 된다 | 맥락을 **경로가 정해지는 즉시** 만든다 |
| "만들 예정이던 경로"와 "저장된 파일"을 구분 안 함 | diff 생성에 실패해도 `evidence.diff`가 채워져 화면의 "저장된 기록 위치"에 **존재하지 않는 파일**이 노출됐다 | `PilotEvidencePaths`를 `Option`으로. 실패 시점에 **`is_file()`인 것만** 채운다(`exists()`가 아니다 — 같은 이름의 폴더를 저장된 기록이라 말하면 안 된다) |
| (추가) `PoisonedRun`에 표시 오류 없음 | 최초 실패 결과가 화면에서 사라진 뒤 같은 슬롯을 다시 확인하면 **종료 오류가 `lockError`로 다시 쓰였다** | 슬롯에도 `error`(종료 실패)와 `marker_error`(표시 저장 실패)를 나눠 저장 |

**회귀 테스트는 전체 흐름을 확인한다.** "B가 잠기지 않는다"만으로는 부족하다는 지적이 맞다 — A를 poison → 시작 시도가 막힘 → **알림에 담긴 경로로 실제 A를 정리** → 다음 실행이 열림 → A는 영구 폐기돼 재사용 불가, 까지 한 테스트로 확인한다. 부분 실패는 stderr 경로를 **폴더로 선점**해 events만 만들어진 상태를 실제로 만들어 검증한다(`orchestration`).

**발견**

- 테스트에서 `session.slot()` 가드를 쥔 채 `session.release()`를 부르면 **데드락**이다. `std::sync::Mutex`는 재진입이 안 된다. 테스트가 120초 타임아웃으로 잡혔고, `reserve()`가 돌려준 run_id를 쓰도록 고쳤다
- 파일 존재 확인은 `exists()`가 아니라 **`is_file()`** 이어야 한다. 폴더로 선점하는 실패 주입 기법을 쓰면 `exists()`는 참이 되어 "저장된 기록"으로 오보고된다

**검증**: `cargo test --locked` **137 통과 / 0 실패 / 12 무시 · 경고 0**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과, 통합 테스트 2건 재실행 통과(CLI `2.1.237`).

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{orchestration.rs,handoff.rs,handoff/tests.rs}`, `src/App.tsx`, `src/App.css`, `src/locales/{ko,en}.ts`

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 6차 반려 3건 반영)

5차 지적 6건은 해소 확인. 남은 3건을 고쳤다. 셋 다 **증거·잠금 정보를 잃거나 뒤섞는** 문제였다. 여전히 **미커밋**이다.

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 다른 복제본의 오염을 이 복제본 것으로 오판 | `poisoned_slot_error`가 오류만 돌려주고 경로를 안 줬다. **슬롯은 하나뿐**이라 A의 오염이 깨끗한 B를 `sessionOnly`로 잠그고, B를 폐기해도 `clear_poisoned_slot`은 경로가 달라 슬롯을 못 풀어 **실행이 영영 막힌다** | `poisoned_slot`이 `(복제본 경로, 오류)`를 반환하고, 대상과 일치할 때만 잠근다 |
| 실패 시 Codex 증거 경로가 사라짐 | `execute_pilot`이 `Result<_, String>`이라 오류가 문자열로 축약됐다. 증거 파일은 이미 만들어져 있는데 경로를 잃어 화면에 Claude 증거만 남았다 | `PilotFailure`로 오류·`run_id`·복제본 경로·증거 경로·오염·표시 오류를 실어 나른다. `run_id`는 화면 상세에도 노출(증거 파일 이름이 그 값으로 갈린다) |
| `sessionOnly`의 `lockError`가 종료 오류 | `guard.poison`에 저장되는 건 **종료 실패 오류**다. 표시 저장 실패 오류는 따로 보존되지 않아, "표시를 저장하지 못한 이유" 자리에 엉뚱한 문구가 나왔다 | `abandon_run`이 `poisoned`(무엇이 잘못됐나)와 `marker_error`(왜 저장 못 했나)를 분리. `attention`·`lockError`에 각각 |

**잠금 판정을 네 갈래로 명시했다**: ① 이번 실행이 오염시킴 → `marker_error` 유무로 `persisted`/`sessionOnly` · ② 이 복제본의 디스크 표시 → `persisted` · ③ **경로가 일치하는** 슬롯 오염 → `sessionOnly` · ④ 그 외 → 잠그지 않음.

**테스트를 실제 실패 경로로 옮겼다.** `stopped()`를 손으로 조립하면 증거를 잃는 지점을 검증하지 못한다.

- 증거 생성 **뒤에** 복제본의 `.git`을 지우는 실행 대상을 주입해 진짜 `PilotFailure`를 만들고(실패 문구: `cannot register new files for the pilot diff (git add -N exited with exit status: 128)`), production 매핑에 통과시켜 Codex 증거 경로가 결과까지 오는지 확인
- `abandon_run`의 두 오류 분리는 **표시 경로를 폴더로 선점**해 저장을 실패시켜 검증. 두 문자열이 실제로 다른지 단언
- A/B 두 복제본으로 슬롯 오판 회귀 고정 — B는 잠기지 않고 A의 오염은 A에 남아야 한다

**검증**: `cargo test --locked` **131 통과 / 0 실패 / 12 무시 · 경고 0**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과, 통합 테스트 2건 재실행 통과(CLI `2.1.237`, `stages: [Organizing, Building, Checking, Done]`).

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{orchestration.rs,handoff.rs,handoff/tests.rs}`, `src/App.tsx`, `src/locales/{ko,en}.ts`

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 5차 반려 6건 반영)

4차 지적 5건은 해소 확인. 남은 6건을 고쳤다. 여전히 **미커밋**이다.

**차단 2건 — 종료 계약의 구멍**

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 누적 집합의 키가 pid | 소멸을 확인한 pid를 새 자손이 재사용하면 `contains_key(pid)`가 참이라 **편입이 막힌다.** 옛 신원은 `Gone`이라 이후 조회에서도 건너뛰므로 **살아 있는 자손을 아무도 보지 않는 채 `Confirmed`** 가 난다 | 키를 `(pid, start_us)` 전체로. `ProcIdentity`에 `Ord` 파생. **F7** 추가 |
| 실행 중 `observe` 오류를 버림 | 일시적 열거 실패나 신원 불일치가 종료 시점에 사라져 있으면 그 사실을 잊고 `Confirmed`가 난다. 실패한 회차에 생겼다 사라진 자손이 있었는지 **알 방법이 없다** | `integrity_error`에 최초 오류를 영구 저장하고 `report_final`이 반드시 `Unconfirmed`를 내게. **F8** 2종 추가 |

**나머지 4건**

- **"결과 버리기"가 잠긴 사본에서만 동작했다** — `discard_codex_pilot`은 poisoned 표시가 있는 복제본만 받는다. 즉 정상 완료·단순 취소·`sessionOnly` 결과의 버튼은 **전부 실패**했다. 인계 전용 `discard_handoff_workspace`를 만들어 네 경우 모두 영구 `abandoned`로 전환한다. 파일은 지우지 않는다(증거 보존 계약 그대로), 루트·매니페스트 검증은 유지
- **구현 단계 실패가 오류로 던져졌다** — 화면에 오류 문구만 남고 복제본 경로·잠금·증거가 사라져 폐기도 복구도 못 했다. `NeedsAttention` 결과로 변환하되 잠금은 **사실대로** 읽는다(디스크 표시 → `persisted`, 슬롯만 오염 → `sessionOnly`, 복제본 손대기 전 실패 → 잠그지 않음)
- **예약 전 중단 요청이 사라졌다** — 화면이 `begin_handoff` 완료 전에 이미 중단 버튼을 보여 준다. 그 요청을 기억했다가 예약 직후 전달하게 했다
- 문서 §8의 §7.6A 중복 줄 정리

**회귀 테스트 14개 추가** (113 → 127). F7, F8 2종, 무결성 정상 실행, 폐기 6종(정상 완료·취소·persisted·sessionOnly·미확인 거부·임의 폴더 거부), 실패 보고 4종.

**F7·F8이 실제로 결함을 잡는지 옛 동작으로 되돌려 확인했다** — 셋 다 정확히 그 자리에서 실패했다(`재사용된 pid의 새 자손이 편입되지 않았다`, `일시적 열거 실패를 잊고 소멸을 주장했다`).

**발견**

- **테스트 하네스에도 같은 결함이 있었다.** 가짜 `signal`이 pid로 첫 항목을 찾아 **같은 pid를 쓰던 죽은 프로세스**에 신호를 보내고, 실제 대상은 멀쩡히 살아남았다. F7이 이걸 먼저 잡아냈다 — 살아 있는 것만 찾도록 고쳤다
- pid 기반 조회는 프로덕션 코드든 테스트 코드든 **같은 방식으로 틀린다.** pid는 식별자가 아니다

**검증**: `cargo test --locked` **127 통과 / 0 실패 / 12 무시**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, 통합 테스트 2건 재실행 통과(CLI `2.1.237`, `stages: [Organizing, Building, Checking, Done]`).

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{proctree.rs,proctree/tests.rs,handoff.rs,handoff/tests.rs,orchestration.rs,lib.rs}`, `src/App.tsx`

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 4차 반려 5건 반영)

**§7.10 1차 구현이 Codex 재검토를 통과하지 못했다.** 지적은 전부 *구현이 계약을 지키지 못한* 지점이었다. 코드와 회귀 테스트로 고쳤고 여전히 **미커밋**이다.

**고친 것 5건**

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 중단 경로 | 준비 중 취소가 `cancel.store(false)`로 지워지고, Claude가 조용하면 취소가 영영 닿지 않고, 구현 중 취소가 Codex에 전달되지 않았다 | 취소 깃발을 **슬롯 소유**로 이동(`HandoffSession`), stdout 읽기를 별도 스레드로 분리해 실행 소유자가 취소를 폴링, `CancelBridge`가 Codex 슬롯 예약 직전의 취소까지 재시도 전달 |
| 단일 정리 절차 | 증거 파일 생성 실패 시 자식·watcher가 남는 조기 반환. 종료 확인 실패 뒤 blocking `child.wait()` | 증거 파일을 **자식보다 먼저** 만들어 그 경로 자체를 없앰. 확인 실패 뒤에는 제한된 `try_wait`만 하고 즉시 poisoned 반환(스레드도 join하지 않음). stderr·events 기록 실패를 증거 실패로 보고 |
| 잠금 보고 | `write_poisoned_marker` 오류를 무시하고 `workspaceLocked: true`로 보고 — **거짓 안전 보고** | `WorkspaceLock` 3상태(`none`/`persisted`/`sessionOnly`) + `lockError`. 저장 실패해도 프로세스 내 목록이 재사용을 막고, 화면은 폐기를 요구 |
| 완료 화면 | 계획 요약을 "실제 수행 결과"로 표시. 변경 목록이 삭제·이름 변경·바이너리를 누락. `확인 중`이 화면에만 존재. 멈추면 진행 표시가 초기화 | 결과는 Codex 최종 메시지에서, 계획은 상세의 *하려던 것*으로 분리. 목록은 `git diff --name-status -z -M` 상태 코드로. `Checking`을 실제 백엔드 단계로. 멈춘 지점을 표시 |
| fail-closed | 도구 목록을 `filter_map`으로 읽어 비문자열 항목이 섞여도 통과. 모델을 `contains("sonnet")`로 대조 | 비문자열 항목·목록 부재를 위반으로. `ALLOWED_INIT_MODELS` **완전 일치**. `is_error` 계획은 `plan_to_codex_prompt`가 구조적으로 거부 |

**단일 실행 슬롯**: `begin_handoff`로 **복제본 준비보다 먼저** 예약하고 `run_handoff`가 소유권을 잡는다. 그래야 준비 중 취소가 살아남고 동시 실행이 거부된다. 진행 이벤트에 `runId`를 실어 끝난 실행의 늦은 상태가 새 실행 화면에 섞이지 않게 했다.

**회귀 테스트** — Codex가 요구한 8종을 전부 넣었다. 중단 3종(준비 중·무출력 중·Codex 구간), 동시 실행 거부, 증거 실패 후 프로세스 잔존 없음, 확인 실패의 제한 시간 내 poisoned, 잠금 저장 실패 후 재사용 거부, 삭제·이름 변경 표시, 결과와 계획 분리, malformed 도구/모델 거부.

- 프로세스가 필요한 경계는 **가짜 실행 대상**(아무것도 출력하지 않고 배경 자손을 남기는 스크립트)으로 실제 트리를 만들어 검증했다
- 실제로 안 죽는 트리는 만들 수 없으므로 **종료 절차를 주입 가능**하게 두고(`TerminateFn`) 확인 실패 이후의 행동만 고정했다
- 돌아오지 않는 구현을 테스트가 붙잡지 않도록 제한 시간 헬퍼(`within`)를 썼다

**검증**: `cargo test --locked` **113 통과 / 0 실패 / 12 무시**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과(기존 파일 재포맷 없음), 통합 테스트 2건 실제 재실행 통과, 잔여 프로세스·임시 폴더 없음.

**발견**

- **CLI가 `2.1.235` → `2.1.237`로 올라갔는데 계약이 그대로 통과했다.** 도구 집합·권한 모드·모델(`claude-sonnet-5`)이 모두 동일 — 정확 일치 목록이 패치 판올림에 곧바로 부서지지는 않는다는 첫 관측이다
- **계획과 결과가 실제로 다르다는 것이 실행에서 드러났다.** 계획 요약은 영어, 구현 결과는 한국어였다. 이전 구현이라면 사용자는 *하려던 것*을 *한 것*으로 읽었을 것이다
- `CancelBridge`가 접으라는 신호를 **취소 확인보다 먼저** 보면 실행 종료 직전의 취소를 흘린다. 테스트가 잡아 순서를 뒤집었다
- 테스트에서 **고정 시간 대기로 실행 기동을 가정하면 병렬 실행에서 깨진다.** 실제 기동(pid 파일)을 기다리도록 고쳤다

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{handoff.rs,handoff/tests.rs,claude_pilot.rs,claude_pilot/tests.rs,orchestration.rs,lib.rs}`, `src/App.tsx`, `src/App.css`, `src/locales/{ko,en}.ts`

**다음 할 일**

- Codex 재검토 요청 (중단 계약·정리 절차·잠금 보고·결과 표시·fail-closed 엄밀성)
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5) — 사람이 써 보고 내리는 판정

---

### 2026-08-19 및 그 이전 → `docs/archive/history-2026-08.md`

2026-08-12 ~ 2026-08-19 엔트리(초기 교차 검토 왕복 38개 + M4 실행 계약 실측·§7.10 1차 구현 12개)를 아카이브로 옮겼다. M4의 현재 결론은 `docs/proposals/m4-orchestration.md`에 있으므로 통상 아카이브를 열 필요는 없다.

### 2026-07-23 및 그 이전 → `docs/archive/history-2026-07.md`

17개 엔트리(2026-07-16 ~ 2026-07-23)를 아카이브로 옮겼다. 재사용할 결정·함정은 위 `핵심 결정·발견`에 반영돼 있으므로, 통상 아카이브를 열 필요는 없다.
