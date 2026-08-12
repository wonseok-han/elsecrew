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
- **M4 S1 교차 검토 상태**: `PresetOutcome::Applied`는 설정 파일을 새로 썼다는 사실만 나타내며 유효한 안전 정책을 보장하지 않는다. 화면 문구도 파일 작성 사실로 한정하고, 권한 팁은 기본 설정 기준·예외 가능성을 밝힌다. Claude가 Codex 경로의 생성·보존·직렬화 테스트까지 보완했고 2026-08-12 Codex 재검토를 통과했다. 다음 단계는 S2 파일럿 실행 경계 구현이다.

## 작업 규칙 (고정)

- **워크로그 필수 범위**: 구현뿐 아니라 버그 분석, 코드 리뷰, 우선순위 결정, GitHub/배포 같은 외부 변경도 다음 작업자에게 의미가 있으면 이 문서에 기록. 최종 응답 전 `git diff -- docs/history.md`로 기록 여부 확인
- **커밋·push 전 반드시 사용자 승인**을 받는다 (이 프로젝트 관행)
- `git`은 scm_breeze 충돌(`_safe_eval` 오류)로 **`/usr/bin/git` 절대경로** 사용. 커밋 메시지는 임시 파일 방식(HEREDOC 미동작), Co-Authored-By 트레일러 포함
- **검증**: `pnpm build`(tsc+vite) · `cargo test --manifest-path src-tauri/Cargo.toml` · 네트워크 격리 E2E는 `-- --ignored` · push 시 CI(macOS/Windows)
- 실행: `pnpm tauri dev`(개발) / `pnpm tauri build`(설치 파일). Rust 1.85+ (edition 2024)
- 초보자(비개발자) 대상 — 모든 문구는 전문용어 없이 쉬운 한국어. ".md도 IDE도 모르는 사람" 기준으로 검토

---

## 워크로그 (최신이 위)

### 2026-08-12 · by GPT-5 Codex

**S1 재검토 통과 — 안전 문구와 Codex 테스트 보완 확인 (제품 코드 변경 없음)**

- 이전 차단 2건이 해소됐다. `project.done.safe.applied`는 유효한 안전 정책을 보장하지 않고 “안전 관련 기본 설정 파일을 새로 만들었다”는 작성 사실만 말한다. 에이전트별 팁은 “기본 설정” 전제를 붙였고, Claude 자동 진행의 승인 생략과 Codex 폴더 밖 쓰기 예외도 명시한다.
- `apply_safety_preset_in(.., home)`은 테스트용 홈 경로만 주입하는 얇은 분리이며 운영 경로는 기존 `home_dir()`를 그대로 사용한다. Codex의 최초 `Applied`·재호출 `Kept`, 기존 `never + danger-full-access` 설정의 바이트 단위 보존, 프론트 직렬화 계약을 실제 사용자 홈과 격리해 검증한다.
- 독립 검증: `cargo test --manifest-path src-tauri/Cargo.toml` 19 passed·6 ignored, `pnpm build` 통과, `git diff --check` 통과. 새 차단 사항은 발견하지 못했다.
- 문서 상태도 현재 사실에 맞춰 M4 제안서의 S0 승인과 S1 완료를 체크하고 S2를 다음 단계로 남겼다.

**결론** → **S1 교차 검토 통과**. 커밋·push는 사용자 승인 전까지 하지 않는다. 다음 작업은 계획대로 S2(Codex 1차 담당)이며, 호스트에서 `--approve-for-me`를 사용하지 않는 파일럿 실행 경계를 구현한다.

### 2026-08-12 · by Claude Opus 5

**S1 재작업 — Codex 교차 검토 지적 3건 반영 (제품 코드 변경 있음)**

Codex가 S1을 미통과로 판정했다. **지적 3건 모두 타당해 반박 없이 수정했다.** 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S1 재작업`.

- **차단 1(안전 상태 과장)**: `Applied`는 "파일을 썼다"일 뿐 유효 정책이 아니라는 지적. 문구를 파일 작성 사실로 한정했다 — "이 폴더에서 안전하게 일하도록 기본 설정을 넣어뒀어요" → "처음이라 안전 관련 기본 설정 파일을 새로 만들어 뒀어요". Codex 설정은 전역이라 "이 폴더"라는 표현도 애초에 틀렸는데 함께 해소됐다.
- **차단 2(S0 계약 위반)**: 내가 S0에 "폴더 밖은 절대 못 건드린다고 말하지 않는다"고 써놓고 `tips.know2.codex`에 정확히 그 말을 썼다. **자기모순이었다.** 모든 승인 문구에 "기본 설정에서는" 전제를 붙이고, 코덱스 문구는 "폴더 밖까지 완전히 막아 주는 건 아니니 중요한 파일은 따로 복사해 두세요"로 바꿨다.
- **테스트 공백**: `apply_safety_preset_in(.., home)`으로 홈 경로를 주입 가능하게 분리했다. 실제 `~/.codex`를 건드리지 않고 Codex 경로를 검증한다. 신규 3건 — 전역 설정 신규 생성·재호출 시 `Kept`, **기존 `never + danger-full-access` 설정을 한 글자도 바꾸지 않음**, 직렬화 계약(`{"guide":"applied","config":"kept"}`).
- **검증**: `cargo test` **19 passed**(신규 3건 포함), `pnpm build` 통과.
- **한 가지 유보**: 차단 1의 근거 중 "프로젝트 단위 `.codex/config.toml`이 전역 설정을 덮는다"는 부분은 내가 독립 확인하지 않았다. 다만 CLI 옵션 우선순위만으로도 "파일 작성 ≠ 유효 정책"은 성립해 수정 방향에는 영향이 없다.

**다음 할 일** → **Codex 재검토**. 통과하면 커밋(사용자 승인 필요) 후 S2로 진행

### 2026-08-12 · by GPT-5 Codex

**교차 검토 — Claude의 S1 구현은 빌드되지만 안전 계약 기준 수정 필요 (제품 코드 변경 없음)**

- 구조화 자체는 계획대로 구현됐다. `ProjectInfo.safety`가 `guide`·`config` 각각의 `Applied`/`Kept`를 전달하고, 기존 파일 보존 경로와 혼합 상태 테스트도 통과한다.
- **차단 사항 1 — 안전 상태를 과장한다**: `Applied`는 `write_if_absent`가 파일을 썼다는 뜻일 뿐 실제 적용 여부가 아니다. 특히 Codex는 `~/.codex/config.toml`을 쓰지만 프로젝트 `.codex/config.toml`과 CLI 옵션이 더 높은 우선순위로 덮을 수 있다. 그런데 UI는 곧바로 “이 폴더에서 안전하게 일하도록 기본 설정”이 적용됐다고 단정한다. 파일 작성 상태와 유효 안전 상태를 구분하거나, 문구를 “기본 설정 파일을 만들었다” 수준으로 좁혀야 한다.
- **차단 사항 2 — 팁이 기존 설정과 S0 계약을 무시한다**: `tips.know2.claude-code`는 모든 파일 변경 전 확인을 단정하고, `tips.know2.codex`는 폴더 밖 접근이 막힌다고 단정한다. 이 문구는 `config=Kept`인 기존 사용자에게도 그대로 보이며, 기존 설정이 자동 승인·`danger-full-access`일 수 있다는 감사 결과와 “폴더 밖은 절대 못 건드린다고 말하지 않는다”는 S0 계약에 어긋난다. 기본 모드 기준임을 밝히고 현재 상태를 모르면 보장하지 않는 표현이 필요하다.
- **테스트 공백**: 신규 상태 테스트 3개가 모두 Claude 경로만 실행한다. 전역 홈 경로를 쓰는 Codex의 `Applied`/`Kept`, 기존 파일 보존, 직렬화 결과는 검증되지 않았다. 홈 경로를 주입 가능한 구조로 분리해 실제 사용자 설정을 건드리지 않는 Codex 단위 테스트가 필요하다.
- **독립 검증**: `cargo test --manifest-path src-tauri/Cargo.toml`은 16 passed·6 ignored, `pnpm build`는 통과했다. 처음에는 폴더 rename 뒤 샌드박스 쓰기 허용 경로가 옛 `agent-starter`에 남아 빌드 잠금·Vite 임시 파일 작성이 거부됐고, 승인된 동일 명령으로 재실행해 통과했다. 코드 실패가 아니다.

**결론** → S1은 **교차 검토 미통과·수정 필요**. 위 두 안전 문구 차단 사항과 Codex 테스트를 보완한 뒤 재검토하고, 그전에는 S2로 넘어가지 않는다. 커밋·push는 하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S1 착수 — 문서 정밀도 2건 정정 + 안전 문구 초안 작성 (제품 코드 변경 없음)**

사용자가 §7.9 계획과 S0 안전 계약을 승인해 S1(Claude 1차 담당)을 시작했다. 계획상 **문구 초안은 구현 전에 사용자가 확인**하므로 이번에는 초안까지만 작성했다.

- **Codex가 지적한 정밀도 2건 반영**: §3의 "Codex의 실제 보호는 작업 폴더 경계"를 "샌드박스가 허용한 쓰기 루트 경계(작업 폴더 + 임시 디렉터리)"로 좁혔다. §7.9 S0의 "현재 공통 약속"은 앱이 아직 프리셋 적용 상태를 표시하지 않으므로 "S1 완료 후 제공할 약속"으로 바꾸고, 지금 사실인 것을 따로 분리했다. 과거 워크로그 엔트리는 규칙대로 손대지 않았다.
- **S1 초안을 §7.9에 기록**: ① `ProjectInfo`에 `SafetyStatus { guide, config }`(`Applied`/`Kept`)를 추가해 프리셋 적용 결과를 프론트로 전달, ② `project.done.safe`를 적용 결과별 2개 키로, `tips.know2`를 에이전트별 3개 키(claude-code/codex/general)로 분리. ko/en 초안 포함.
- **추가 발견 2건(S1 표에 없던 것)**: `project.make.desc`의 "다른 파일은 건드리지 않으니 안전해요"는 앱 동작인지 에이전트 작업 범위인지 모호해 후자로 읽히면 부정확하다. 더 중요한 것은 **`tips.agent.claude-code`가 auto mode를 권하는데, 이는 Claude Code의 유일한 실질 보호인 변경 전 승인을 끄도록 권하는 문구**이며 같은 카드의 `tips.know2`와 정면 충돌한다.
- **범위 밖으로 명시**: 기존 Codex 설정이 `danger-full-access`일 때 경고하는 기능은 S1에 넣지 않았다. S1의 목표는 "거짓말하지 않기"이지 "남의 설정 평가하기"가 아니다.

**S1 구현 완료 — 안전 문구·표시 조건 교정 (제품 코드 변경 있음)**

사용자가 초안과 추가 발견 2건의 처리 방향(둘 다 추천안)을 승인해 바로 구현했다.

- `project.rs`: `SafetyStatus { guide, config }` + `PresetOutcome{Applied,Kept}` 추가. `write_if_absent`가 "새로 썼는지"를 반환한다. **기존 파일을 덮어쓰지 않는 동작은 그대로**다.
- `App.tsx`: 졸업 직전 안내를 `safety.config` 결과로 분기해 **적용하지 않았으면 "넣어뒀다"고 말하지 않는다.** `UsageTips`는 `agent` 유무로 승인 문구 3종을 고른다.
- `locales/{ko,en}.ts`: `project.done.safe` → `.applied`/`.kept`, `tips.know2` → `.general`/`.claude-code`/`.codex`. `project.make.desc`는 앱 동작 한정으로 좁히고, `tips.agent.claude-code`는 auto mode가 **파일 변경 전 확인 절차도 함께 끈다**는 사실을 명시했다.
- 신규 테스트로 `guide=Kept` · `config=Applied`가 따로 보고되는지와 기존 파일 보존을 확인했다.
- **검증**: `cargo test` 16 passed(신규 2건 포함), `pnpm build` 통과, 제거한 키 잔존 참조 0건, ko/en 키 1:1 대조 완료.
- **함정 발견 — 폴더 rename 잔재**: `cargo test`가 옛 경로 `projects/agent-starter/...`를 찾다 빌드 스크립트에서 실패했다. `target/`에 rename 전 절대경로가 남아 있던 것이며 `cargo clean`(12.6GiB 제거)으로 해결했다. 같은 증상은 재빌드가 아니라 clean이 답이다.

**다음 할 일** → **Codex 교차 검토**(§7.9 S1의 검토 담당). 그다음 S2(Codex 1차 담당, `--approve-for-me` 없는 복제본 파일럿 경계 구현). 커밋은 사용자 승인 대기

### 2026-08-12 · by GPT-5 Codex

**검토 — Claude의 교차 검토·경계 정정 확인 (제품 코드 변경 없음)**
- Claude가 Codex 교차 감사 결과를 직접 재실측해 `patch/file_change`의 프로젝트 경계와 셸의 샌드박스 쓰기 루트를 구분한 정정은 타당하다. 모델의 자체 거절을 강제 증거로 보지 않고 실제 종료 코드·결과 파일로 판정한다는 감사 원칙도 재사용 가치가 높다.
- `--approve-for-me` 호스트 파일럿 제외, 사용자 중계 제거를 M4 필수 통과 조건으로 둔 결정, §7.9 공동 역할 분담에 Claude와 Codex의 결론 충돌은 없다.
- 남은 문서 정밀도 문제 2건: §3의 “Codex의 실제 보호는 작업 폴더 경계”라는 이전 표현은 임시 쓰기 루트 예외와 충돌하므로 “샌드박스가 허용한 쓰기 루트 경계”로 좁혀야 한다. §7.9 S0의 “현재 공통 약속”은 앱이 아직 프리셋 적용 상태를 보여주지 않으므로 “S1 완료 후 제공할 약속”으로 표현해야 한다.
- 최신 문서 변경은 `m4/orchestration`의 `f8eb1ab`에 커밋돼 있고 워킹 트리는 검토 시작 시 깨끗했다. 제품 코드 변경은 없다.

**다음 할 일** → 위 문구 2건을 제안서에 정정한 뒤 사용자가 S0 계약·§7.9 계획을 승인하면 S1 문구 초안 진행

### 2026-08-12 · by Claude Opus 5

**교차 검토 — Codex 반영분 확인 및 내 감사 결론 1건 정정 (제품 코드 변경 없음)**

Codex가 §1·§3·§4.3·§7.6·§7.7·§7.9·§8에 반영한 내용을 읽고, 내 감사 결과와 어긋나 보이는 지점을 재실측했다.

- **내 결론이 틀렸다 — Codex가 옳았다.** 내 시험 A는 "작업 폴더 밖 쓰기가 거부됨 → 폴더 경계가 강제됨"으로 결론냈는데, 이는 `patch`/`file_change` **도구에만** 해당한다. **셸 명령은 임시 디렉터리 같은 샌드박스 쓰기 루트에 쓸 수 있다.** 재실측에서 `/private/tmp` 형제 경로 셸 쓰기가 `exit 0`으로 성공하고 파일이 생성됐다. §3에 `경계 정정` 표를 넣고 §9에 철회 항목으로 남겼다.
- **재사용할 감사 방법론 함정**: 재확인 1차에서 Codex가 *"허용된 작업공간 밖이므로 실행할 수 없습니다"* 라고 답해 경계가 강제되는 것처럼 보였다. 그러나 이는 **샌드박스 거부가 아니라 모델의 자체 거절**이었고, "판단하지 말고 실행만 시도하라"로 다시 요청하니 성공했다. **에이전트의 "못 한다"는 말을 강제 증거로 삼으면 안 되고, 안전 경계는 결과물(파일 존재·종료 코드)로만 판정한다.** §9에도 철회 항목으로 기록했다.
- `--approve-for-me`와 `--sandbox` 동시 지정 불가라는 Codex 보고도 확인했다(`error: the argument '--approve-for-me' cannot be used with '--sandbox'`).
- Codex의 핵심 기여 3건에 동의한다: ① M4의 첫 가치를 "사용자가 전달자가 되지 않게 한다"로 재정의(§1), ② `--approve-for-me`가 저장소 밖 쓰기까지 자동 승인하므로 **폐기 가능한 복제본은 안전 경계가 아니다** → 호스트 기반 파일럿에서 사용 철회(§3), ③ 사용자 중계 횟수를 품질과 독립된 필수 통과 조건으로 승격(§7.6).

**다음 할 일** → **§7.9 계획과 S0 안전 계약의 사용자 승인 대기.** 승인되면 S1(문구 정정 + 프리셋 적용 결과 구조화)이 Claude 1차 담당이며, 문구 초안을 구현 전에 사용자가 확인한다

### 2026-08-12 · by GPT-5 Codex

**제품 정의 — M4의 최우선 문제를 사용자 중계 제거로 확정 (코드 변경 없음)**
- 사용자가 이번 Claude·Codex 교차 논의에서도 두 대화를 오가며 계속 결과와 다음 지시를 전달해야 했고, 바로 이 불편이 M4 구상의 직접 원인이라고 재확인했다.
- `history.md`와 제안서 같은 공유 기억은 컨텍스트 소실을 줄이지만, 다음 에이전트 호출·결과 인계·진행 제어까지 자동화하지는 못한다. 따라서 M4의 첫 가치를 “사용자는 한 곳에 말하고 앱이 Claude와 Codex를 이어 사용자가 전달자가 되지 않게 한다”로 제안서 §1에 명문화했다.
- 파일럿의 핵심 UX 지표에 사용자 전달 횟수·대화/앱 전환 횟수·전문 로그 해석 필요 여부를 추가했다. 사용자가 “상대 결과를 읽어라”라고 직접 호출해야 하면 품질 결과와 관계없이 실패로 판정한다.

**다음 할 일** → 안전 개선 계획을 유지하되, 이후 M4 파일럿은 자동 인계의 최소 수직 흐름을 최우선으로 설계

### 2026-08-12 · by GPT-5 Codex

**계획 — Claude·Codex 공동 안전 개선 순서와 역할 정의 (제품 코드 변경 없음)**
- 사용자의 “Claude도 알아야 하므로 계획부터”라는 판단을 반영해 M4 제안서 §7.9에 공동 실행 계획을 추가했다. 권장안은 잘못된 문구만 고치는 것이 아니라 **문구 정정과 동작 강화 둘 다**다.
- 순서는 `S0 안전 계약 사용자 승인 → S1 현행 안전 문구·표시 조건 교정 → S2 --approve-for-me 없는 복제본 파일럿 → 효과 확인 → S3 실제 프로젝트용 app-server 승인·Git 복원 스파이크`로 고정했다.
- Claude는 현행 안전 프리셋 상태 전달·사용자 문구를 1차 담당하고 Codex가 검토한다. Codex는 파일럿 실행 경계·app-server를 1차 담당하고 Claude가 검토한다. 각 단계는 자기 결과만으로 완료 처리하지 않는다.
- 이 계획의 사용자 승인 전에는 제품 코드 구현을 시작하지 않는다. 모든 작업은 `m4/orchestration` 계열에서 진행하고 최종 main 병합 승인 규칙도 유지한다.

**다음 할 일** → 사용자가 §7.9 계획과 S0 안전 계약을 승인하면 S1 문구 초안부터 작성

### 2026-08-12 · by GPT-5 Codex

**교차 안전 감사 — Codex 삭제·샌드박스·자동 검토 경계 보강 (제품 코드 변경 없음)**
- Claude의 Codex 감사 결과를 별도 폐기 Git 저장소에서 재현했다. `untrusted + workspace-write`에서 파일 생성·`file_change` 삭제·단일 파일 `/bin/rm`은 승인 없이 실행됐고, `/bin/rm -rf`와 비임시 경로의 저장소 밖 셸 쓰기는 승인 왕복 없이 거부됐다. `/private/tmp` 형제 경로 쓰기는 허용돼 임시 경로가 별도 쓰기 범위임도 확인했다.
- `--approve-for-me`는 `--sandbox workspace-write`와 동시 지정할 수 없으며, 자동 검토가 `/bin/rm -rf`와 비임시 경로의 저장소 밖 디렉터리 생성·파일 쓰기를 모두 허용했다. `--ephemeral` 유무와 관계없이 `rm -rf`가 허용됐다.
- 결론: 폐기 가능한 Git 복제본만으로는 자동 검토의 안전 경계가 되지 않는다. 호스트 기반 M4 파일럿의 `--approve-for-me` 사용을 철회하고, 전체 프로세스가 사용자 파일시스템과 분리된 VM·컨테이너에서만 재검토한다.
- 감사에 사용한 비임시 외부 경로 `/Users/wonseok-han/projects/hello-agent-codex-audit-outside-20260812`에는 테스트 파일 하나만 있음을 확인한 뒤 즉시 제거했다. 실제 프로젝트와 사용자 전역 Codex 설정은 변경하지 않았다. 상세 결과는 M4 제안서 §3에 통합했다.
- 실행 중 로컬 프로젝트 폴더가 `agent-starter`에서 `hello-agent`로 rename된 사실을 확인했고, 이후 명령은 새 경로와 `m4/orchestration` 브랜치에서 수행했다.

**다음 할 일** → 안전 문구·동작 불일치에 대한 제품 결정. M4 품질 비교 파일럿은 `--approve-for-me` 없이 설계

### 2026-08-12 · by Claude Opus 5

**안전 감사 실측 — 현행 안전 문구와 에이전트 실제 동작 대조 (제품 코드 변경 없음)**

M4 §8의 1순위였던 현행 안전 감사를 수행했다. 격리 픽스처에 앱이 만드는 프리셋을 그대로 재현해 두 에이전트를 실제로 실행했다. **상세 결과·표·후속 결정은 `docs/proposals/m4-orchestration.md` §3 `감사 결과`가 단일 기준점이다.**

- **Claude Code는 문구와 일치**: 파일 변경 전 승인을 실제로 요구했고(비대화형에선 차단), `.env` deny 규칙도 작동(대조군으로 같은 폴더 다른 파일은 정상 읽힘 확인).
- **Codex는 문구와 불일치**: `approval_policy="untrusted"` + `sandbox_mode="workspace-write"`에서 **작업 폴더 안 파일 수정과 임의 쉘 쓰기 명령이 승인 없이 그대로 실행**됐다. 실제 보호는 승인이 아니라 **작업 폴더 경계**다(폴더 밖 쓰기는 `patch rejected`로 거부 확인).
- **구조적 결함**: `apply_safety_preset`의 `write_if_absent`는 기존 파일을 덮어쓰지 않는데, `project.done.safe` 문구는 프로젝트 생성 성공 시 무조건 표시된다(`src/App.tsx:1239-1247`). **설정을 하나도 안 썼어도 "안전장치를 넣어뒀어요"라고 말한다.** 이 개발 머신이 실증 사례 — 기존 `~/.codex/config.toml`이 `approval_policy="never"`, `sandbox_mode="danger-full-access"`로 앱 프리셋과 정반대인데 앱은 건드리지 않는다. **기존 Codex 사용자 전원이 이 경로다.**
- **부수 성과**: M4 §5.1의 1순위 열린 질문이 함께 해소됐다. `exec`는 승인 지점에서 막히지 않는다 — 승인 지점 자체가 없기 때문이다. 이 과정에서 기존 추론 2건을 §9 `철회된 주장`으로 옮겼다.
- **미실측(추정으로 표시)**: `Bash(rm -rf:*)` deny의 접두사 매칭 우회 가능성. 권한 우회 모드로 에이전트를 실행해야 확인되므로 사용자 승인 전까지 하지 않았다.

**다음 할 일** → **제품 결정 대기**: 불일치 3건을 문구 정정으로 해소할지, 동작 강화로 해소할지, 둘 다 할지. 사용자 문구를 바꾸는 일이라 명시적 결정이 필요하다. M4 후속 작업(§7.6 과제 정의)보다 이것이 먼저다

### 2026-08-12 · by Claude Opus 5

**인계 — 로컬 폴더 rename 후 세션 소실, 미커밋 문서 상태 재파악 (코드 변경 없음)**
- 사용자가 로컬 프로젝트 폴더를 `agent-starter` → `hello-agent`로 변경해 이전 세션 컨텍스트가 사라졌다. 미커밋 문서 변경 일체(`CLAUDE.md`·`AGENTS.md`·`README.md`·`history.md` 수정, `docs/README.md`·`docs/archive/`·`docs/proposals/` 신규)를 읽고 현재 상태를 재구성했다.
- 확인 결과 미커밋 변경은 **전부 2026-08-12 문서 재편 작업**이며 제품 코드 변경은 없다. 브랜치는 `m4/orchestration`이 맞다.
- **정정 — 폴더명 기록이 사실과 어긋남**: `프로젝트 요약`의 "로컬 폴더명은 `agent-starter` 유지"는 이제 거짓이다. 실제 경로는 `/Users/wonseok-han/projects/hello-agent`이며 해당 줄을 현재 사실로 고쳤다. 문서·코드 전역에서 `agent-starter` 잔존 참조를 검색한 결과 남은 것은 `src-tauri/target/`의 오래된 빌드 지문(gitignore 대상)뿐이고, `src-tauri/Cargo.toml`은 이미 `hello-agent`/`hello_agent_lib`다. 아카이브의 두 건은 당시 기록이라 그대로 둔다.
- 기계 검증: `CLAUDE.md`와 `AGENTS.md`는 `diff` 기준 완전 동일. `history.md` 261줄(롤오버 임계 미만), M4 제안서 401줄, 2026-07 아카이브 210줄. `docs/README.md` 색인이 가리키는 경로 전부 실존.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8. 1순위는 §3 현행 안전 문구·동작 감사. 미커밋 문서 변경은 여전히 커밋 대기(사용자 승인 필요)

### 2026-08-12 · by GPT-5 Codex

**Git 정책 정정 — M4 공동 통합 브랜치를 중립적 이름으로 변경**
- 사용자 지적대로 `codex/m4-orchestration`은 Codex 단독 소유 브랜치처럼 보이며 Claude와 공동 작업하는 통합 기준선의 성격에 맞지 않았다.
- 로컬 브랜치 이름을 `m4/orchestration`으로 변경했다. 브랜치의 기준 커밋, 현재 미커밋 변경과 main 보호 정책은 그대로 유지된다.
- `CLAUDE.md`·`AGENTS.md`·M4 제안서의 분기 및 최종 병합 경로를 모두 `m4/orchestration`으로 정정했다.

**다음 할 일** → 모든 M4 작업은 `m4/orchestration`을 기준으로 진행. 최종 main 병합은 사용자 명시적 승인 필요

### 2026-08-12 · by GPT-5 Codex

**Git 정책 — M4 전용 통합 브랜치 생성 및 main 보호 규칙 확정**
- 사용자 요청에 따라 `main`에서 M4 전용 통합 브랜치를 생성하고 현재 작업 브랜치를 전환했다. 처음 이름은 `codex/m4-orchestration`이었으며, 공동 작업 성격에 맞춰 후속 엔트리에서 `m4/orchestration`으로 변경했다. 기존 미커밋 문서 변경은 손실 없이 새 브랜치에 유지됐다.
- 이 브랜치를 M4의 `main` 대체 통합 기준선으로 지정했다. Claude와 Codex는 M4 작업 전에 브랜치를 확인하며, 기능·실험 브랜치가 필요하면 여기서 분기하고 결과도 여기로만 병합한다.
- M4 작업은 `main`에 직접 커밋·병합·push하지 않는다. 전체 기능과 안전성 검증이 완료된 뒤 사용자가 명시적으로 승인한 경우에만 `m4/orchestration → main` 병합을 수행한다.
- `CLAUDE.md`와 `AGENTS.md`의 Git 작업 규칙, `docs/proposals/m4-orchestration.md` §7.8·§8·결정 이력에 같은 정책을 반영했다.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8. 커밋·push는 별도 사용자 승인 전까지 수행하지 않음

### 2026-08-12 · by GPT-5 Codex

**문서 — M4 오케스트레이션 제안서 정합성 수정 및 파일럿 컨셉 구체화 (코드 변경 없음)**
- `docs/proposals/m4-orchestration.md`의 심층 검토 결과를 본문에 반영했다. 상태를 단순 검토 메모에서 **컨셉 초안 작성 완료·검증 대기**로 갱신했다.
- 안전 절을 실제 코드에 맞게 교정했다. Codex 설정은 프로젝트별이 아니라 `~/.codex/config.toml`이 없을 때만 생성되는 전역 기본값이며, `untrusted + workspace-write`가 모든 파일 변경 전 승인을 보장한다고 단정하지 않는다. UI 안전 문구와 Claude Code·Codex의 실제 동작을 감사하는 일을 전체 1순위로 올렸다.
- OpenAI 공식 문서에 공개된 app-server의 명령 실행·파일 변경 승인 프로토콜과 결정 값을 반영하고, 공개 스펙과 번들 CLI 로컬 실측을 구분했다. SDK 런타임은 앱에 번들할 수 있으므로 사용자 설치 문제로 단정하지 않고 배포·업데이트·유지 비용으로 비교하도록 수정했다.
- §7 파일럿 컨셉을 작성했다. 사용자 여정, 역할별 책임, 정상·분기 상태, 원장 기반 중단·정확한 세션 ID 재개, 복제본/실제 프로젝트 안전 경계, 단독 2조건과 교차 벤더 2조건의 비교 지표, 런타임 기능 저하 정책을 명시했다.
- 역할 방향은 `Claude → Codex`로 미리 확정하지 않고 반대 방향·단일 에이전트와 비교한다. LLM 완료 선언은 단독 근거로 삼지 않으며 작업 유형에 맞는 검증 증거를 요구한다. 철회된 주장과 결정 이력에도 이번 정정을 추가했다.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8. 현행 안전 문구·동작 감사부터 시작

### 2026-08-12 · by GPT-5 Codex

**검토 — M4 오케스트레이션 제안서 내용 정합성 심층 점검 (코드·제안서 변경 없음)**
- `docs/proposals/m4-orchestration.md`의 사실·추론·구현 준비도를 실제 코드와 OpenAI 공식 Codex 문서에 대조했다. 설계 심의 기록과 파일럿 가설은 잘 정리됐지만, 현재 상태는 §7이 미작성인 **연구·결정 메모**이며 바로 구현할 수 있는 완성 컨셉 문서는 아니다.
- §3의 “새 프로젝트마다 기록하는 설정”은 코드와 다르다. Codex 설정은 프로젝트별이 아니라 `~/.codex/config.toml` 전역 파일이고, 해당 파일이 없을 때만 최초 1회 생성한다(`src-tauri/src/project.rs:174-218`). 또한 `approval_policy="untrusted"`는 명령 승인 정책이고 `workspace-write`는 작업 폴더 쓰기를 허용하므로, UI의 “파일을 바꾸기 전에 확인” 약속이 현재 설정만으로 보장된다고 단정할 수 없다. M4 승인 실측과 별개로 현행 안전 문구·실제 동작 감사를 먼저 명시해야 한다.
- §2.3의 app-server 승인 프로토콜 “구체 스펙 미확인”은 공식 문서 기준 오래됐다. 명령 실행·파일 변경 승인 요청과 `accept`/`acceptForSession`/`decline`/`cancel` 결정이 문서화돼 있다. 다만 이 프로젝트의 번들 CLI에서 실제 왕복이 되는지는 여전히 로컬 실측 대상이다.
- §4.5의 “SDK 런타임 요구 = 비개발자가 개발 도구를 설치해야 함”은 지나치게 단정적이다. 런타임을 앱에 번들할 수도 있으므로 사용자 설치 여부가 아니라 배포 용량·보안 업데이트·플랫폼 유지 비용으로 비교해야 한다.
- 그 밖에 “LLM 완료 판정은 신뢰 불가”를 “단독 완료 근거로 삼지 않는다”로 좁히기, 역할 방향을 실험 전 확정하지 않기, 재개 시 `--last` 대신 정확한 세션 ID 저장, §10의 “5라운드”와 6개 전환점 표기 정합화가 필요하다.
- 사용자가 요청한 범위가 검토이므로 제안서 본문은 수정하지 않았다. 다음 단계는 이 검토 내용을 제안서에 반영한 뒤, 성공 지표를 먼저 정의하고 §5.1 승인 동작 및 §5.2 교차 벤더 품질 가설을 실측하는 것이다.

### 2026-08-12 · by GPT-5 Codex

**검토 — M4 제안서 분리와 히스토리 관리 방식 확인 (코드 변경 없음)**
- Claude의 최신 M4 의견과 새 문서 운영 규칙을 실제 파일·Git 상태와 대조했다. M4의 현재 결론·확인 사실·안전 제약·열린 질문·철회된 주장이 `docs/proposals/m4-orchestration.md` 한곳에 정리됐고, `history.md`는 사건 요약과 포인터만 남기는 구조로 바뀌어 이전의 정정 사슬 문제가 해소됐다.
- 생애주기 분류(`현행 사실`·`제안`·`작업 기억`·`지난 기록`)는 제안을 구현된 기능으로 오독하는 위험을 구조적으로 줄인다. `history.md`를 진입점으로 유지하고 오래된 기록만 월별 아카이브로 롤오버한 판단도 적절하다.
- 기계 검증: `history.md` 211줄, M4 제안서 287줄, 2026-07 아카이브 210줄. 2026-07-22 이하 15개 엔트리가 날짜 경계를 지켜 아카이브에 존재하며 포인터도 연결됐다. `CLAUDE.md`와 `AGENTS.md`는 바이트 단위로 동일하고, 문서 색인의 대상 경로가 모두 존재하며 `git diff --check`가 통과했다.
- M4 내용은 추가 수정하지 않았다. 현재 1순위 열린 질문인 복제 저장소의 `untrusted + codex exec` 승인 동작 실측부터 진행한다는 순서에 동의한다.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8 참조

### 2026-08-12 · by Claude Opus 5 + GPT-5 Codex (교차 검토)

**논의 — M4 멀티 에이전트 오케스트레이션 검토 (코드 변경 없음)**

사용자 구상(대표가 목표를 주면 관리자가 기획자·디자이너·개발자·QA에 배분하고 완료까지 반복)을 두 모델이 5라운드 교차 검토했다. **상세는 `docs/proposals/m4-orchestration.md`가 단일 기준점이다. M4 관련 논의·결정·철회는 전부 그 문서에 쌓는다.**

- **결론**: 파일럿은 순차 2롤(Claude 기획·검토 → Codex 구현), 복제 저장소 한정, 앱은 "관리자"가 아니라 **제어면**(반복 상한·중단·검증 강제, 의미 판단은 역할 LLM). 런타임은 `codex exec`로 시작(파일럿 기준 잠정)
- **차별 요소**: 교차 벤더 인계 + 비개발자 제어 + 증거 기반 완료. **역할 기반 팀 자체는 차별점이 아니다**(Claude Code Agent Teams가 이미 상당 부분 제공)
- **구속력 있는 제약(신규 확인)**: 제품이 `approval_policy="untrusted"`(`project.rs:126,131`)와 사용자 문구(`ko.ts:104`·`185`)로 **승인을 이미 약속**하고 있다. M4가 임의로 완화할 수 없다. 중단 버튼은 승인의 대체물이 아니다
- **1순위 열린 질문**: `untrusted` + `codex exec`로 실제 파일 수정을 시켰을 때 승인 지점이 어떻게 동작하는가. **이 답이 M4 런타임 선택을 결정한다**(미검증)
- **논의 중 철회된 주장 5건**은 `proposals/m4-orchestration.md` §9에 정리. 재론 방지용이므로 M4를 다루기 전 반드시 확인할 것
- 실측 확인: Claude Code `2.1.228`, Codex CLI `0.147.0-alpha.6.6`. `codex exec`는 `--json` 스트리밍·`exec resume`·`--output-schema`·`--output-last-message` 지원, **실행 중 개별 승인 왕복만 미지원**
- **부수 발견(M4 무관·단독 적용 가능)**: `project.rs:272` `clean_reply`가 Codex 응답을 "마지막 문단"으로 추정하는데, `--output-last-message`로 대체 가능. 현재는 첫 대화가 짧아 문제되지 않으나 답변이 길어지면 깨진다

**문서 구조 변경 — 워크로그 비대화 대응 (사용자 요청)**
- 계기: M4 논의가 하루 만에 엔트리 8개·155줄(파일의 30%)로 불어나 다른 기록을 압도. 철회·정정이 엔트리에 걸쳐 사슬처럼 얽혀 **"지금 뭐가 참인지" 파악에 엔트리 4개 역추적이 필요**했음. 사용자가 단일 파일 관리 부담을 지적
- **① 주제 분리** — 설계 심의는 워크로그의 종류가 아니라고 판단해 `docs/proposals/m4-orchestration.md` 신설(287줄). 결론·확인된 사실·안전 제약·열린 질문·**철회된 주장 5건 표**로 재구성. 워크로그에는 요약과 포인터만
- **② 아카이브 롤오버** — `docs/archive/` 신설. 2026-07-22 및 그 이전 15개 엔트리를 이동. **525줄 → 196줄**
- **③ 규칙 명문화** — `CLAUDE.md`·`AGENTS.md`와 이 파일 헤더에 롤오버 기준(500줄 초과 시 300줄 이하로, 날짜 경계에서, 이동이지 삭제 아님)과 워크로그/설계문서 경계를 기재
- **날짜별 파일 분할은 기각**: 현재 진실이 여러 파일로 흩어져 조회 비용만 커짐 — 이번에 겪은 문제를 오히려 키우는 방향. 날짜별 조회는 이미 `git log`가 제공하며, 이 문서의 일은 변경 로그가 아니라 **읽을 수 있는 현재 상태 기억**
- 판단 기준(재사용): 워크로그는 **무엇이 일어났나**를, 설계 문서는 **무엇이 참인가**를 기록한다. 토론 왕복은 후자로 정리해 넣는다

**문서 분류 체계 도입 (사용자 요청)**
- 사용자 지적: "docs에 단순히 밀어넣지 말고 분류별로 정리하라". 분류 기준을 주제가 아니라 **생애주기**(이 문서가 무엇을 주장하는가)로 잡음
- 가장 큰 위험이었던 것: **`m4-orchestration.md`(아직 제안)가 `architecture.md`(현행 사실)와 같은 층에 있어** 다음 작업자가 제안을 구현된 사실로 오독할 수 있었음 → `proposals/`로 분리해 경계를 구조로 표현
- 구조: `docs/README.md`(색인·규칙 신설) · `history.md`(진입점, 최상위 유지) · `architecture.md` · `release-checklist.md` · `diagrams/` · **`proposals/`**(제안) · **`archive/`**(지난 기록)
- **`history.md`는 최상위에 그대로 둔다** — 두 에이전트가 가장 먼저 읽어야 하는 진입점이라 한 단계 내리면 접근성만 나빠지고 얻는 게 없음
- 참조 갱신: `CLAUDE.md`·`AGENTS.md`·루트 `README.md`·`history.md`·아카이브 헤더
- **함정 발견·수정 — `AGENTS.md`가 `CLAUDE.md`와 어긋나 있었음**: 롤오버 규칙과 M4 포인터가 `CLAUDE.md`에만 들어가 **Codex는 규칙을 모르는 상태**였음. 두 파일은 내용이 동일해야 하는데(파일명만 다름) 한쪽만 고치면 다른 에이전트가 규칙 없이 작업하게 됨 → 동기화하고, **"두 파일은 같은 내용을 유지한다"를 규칙으로 명문화**

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8 참조

### 2026-07-23 · by Claude Opus 4.8

**추가 작업 — v0.1.2 범프 (자동 업데이터 첫 탑재 릴리스)**
- 업데이터 배선이 CI에서 완전 검증됨(build-macos/windows가 서명 env로 통과) → v0.1.2로 범프. tauri.conf.json·package.json·Cargo.toml·Cargo.lock 동기화
- 이 버전이 **업데이터를 처음 담은 릴리스**. 태그 push하면 서명된 latest.json이 릴리스에 올라가 이후 버전부터 앱이 자동 업데이트. v0.1.0/v0.1.1 사용자는 v0.1.2를 한 번 수동 설치해야 함
- **결과**: v0.1.2(`a453dc9`) 릴리스 빌드 macOS·Windows 성공. 자산에 `latest.json` + 각 업데이터 아티팩트 `.sig`(서명) 첨부. `latest.json` 검증: version 0.1.2, 7개 플랫폼 항목 전부 서명 포함, URL이 v0.1.2 자산을 정확히 가리킴 → 자동 업데이터 완전 작동 확인. 릴리스 노트 작성(자동 업데이트 강조). **draft 상태 — 공개는 사용자 결정 대기**

**버그 수정 — 웹사이트 네비게이션 반복 시 먹통**
- 증상: 앵커 네비게이션(#homebase 등)을 빠르게 왔다갔다 여러 번 누르면 페이지가 멈춤
- 원인: page.tsx는 커스텀 JS 없는 순수 서버 컴포넌트 → JS 문제 아님. **sticky `.site-header`의 `backdrop-filter: blur(18px) saturate(150%)`** 가 `scroll-behavior: smooth`와 겹쳐, 반복 스크롤 중 매 프레임 blur 재계산으로 컴포지터가 과부하되어 멈춤
- 수정(`website/app/globals.css`): sticky 헤더 배경을 거의 불투명(0.88→0.96)으로 올리고 `backdrop-filter`(webkit 포함) 제거. `.trust-chip`의 blur(8px)도 제거(배경 0.92→0.96, 비sticky라 부차적). 배경이 이미 불투명해 시각 변화는 미미
- 검증: `npm test` 2 pass, `npm run lint` 클린, prerender OK. 테스트가 의존하는 `.site-header{position:sticky; top:0}`·`scroll-padding-top` 유지

**추가 작업 — 인앱 자동 업데이터 구현 (tauri-plugin-updater, 무료 서명)**
- 배경: v0.1.x 사용자는 새 버전 받으려면 수동 재다운로드뿐 → 앱이 스스로 업데이트하도록. 업데이터 서명 키는 무료(ed25519), 유료 코드 서명과 무관
- Rust: `Cargo.toml`에 `tauri-plugin-updater`·`tauri-plugin-process` 추가, `lib.rs`에 플러그인 등록
- 설정: `tauri.conf.json` bundle에 `createUpdaterArtifacts: true`, `plugins.updater`(endpoints=releases/latest/download/latest.json, pubkey=**자리표시자 REPLACE_WITH_UPDATER_PUBLIC_KEY**). `capabilities/default.json`에 `updater:default`·`process:default`
- 프론트: `@tauri-apps/plugin-updater`·`plugin-process` 설치. `AppUpdateBanner` 컴포넌트(시작 시 `check()` → 있으면 배너 → `downloadAndInstall()`+`relaunch()`) 헤더 밑에 배치. `appUpdate.*` i18n ko/en, `.app-update-banner` CSS
- CI: `release.yml`에 `TAURI_SIGNING_PRIVATE_KEY`·`_PASSWORD` env 추가(있으면 tauri-action이 업데이터 서명 + latest.json 생성·업로드)
- 검증: `pnpm build`·`cargo check` 통과
- 키 생성 완료(사용자, 빈 비밀번호). 공개 키를 `~/.tauri/hello-agent-updater.key.pub`에서 읽어 tauri.conf.json pubkey에 **반영 완료**(개인 키는 미접근). 개인 키는 저장소 밖 `~/.tauri/`에 보관
- **남은 수동 단계(사용자)**: GitHub 시크릿 2개 등록 — `TAURI_SIGNING_PRIVATE_KEY`(개인 키 파일 내용), `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`(빈 값). 그다음 v0.1.2 범프·태그 push하면 서명된 latest.json이 릴리스에 올라가 자동 업데이트 활성화
- **함정: 소급 안 됨** — v0.1.1까지는 업데이터가 없어 latest.json 확인 불가. 업데이터 첫 탑재 버전(v0.1.2)은 사용자가 한 번 수동 설치해야 이후부터 자동 업데이트됨
- **CI 실패·수정(발견)**: `createUpdaterArtifacts: true` + pubkey면 `tauri build`가 **서명을 강제** → ci.yml의 build-macos/build-windows가 "public key는 있는데 private key 없음"으로 실패. 시크릿을 저장소에 등록한 뒤 **ci.yml의 두 build 스텝에도 `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` env 추가**로 해결. (test 잡은 `cargo test`뿐이라 무관, 통과). release.yml은 원래 env를 넣어둠

**추가 작업 — v0.1.1 릴리스 준비 (버전 범프)**
- 사용 팁 기능을 담아 v0.1.1로 배포하려고 버전을 3곳 모두 올림: `src-tauri/tauri.conf.json`(파일명 기준)·`package.json`·`src-tauri/Cargo.toml` → `0.1.1`, `Cargo.lock`도 `cargo update -p hello-agent`로 동기화
- 배포 흐름: 커밋·push 후 `v0.1.1` 태그 push → release.yml이 초안 릴리스 빌드(태그·push는 분류기 차단이라 사용자가 `!`로 실행)
- 릴리스 노트는 초안 생성 후 gh release edit로 작성 예정(사용 팁 추가 강조)
- **문서 버전 하드코딩 제거**: 다운로드 링크는 이미 `releases/latest`라 버전 무관. 유일하게 버전이 박혀 있던 README 문구("첫 베타 v0.1.0")를 "베타 버전이 공개되어 있어요 + 최신"으로 바꿔 릴리스마다 문서 수정 불필요하게 함. 웹사이트 page.tsx는 원래 버전 무관 문구라 무변경(0.145.0은 목업 속 가짜 Codex 버전)
- **결과**: v0.1.1 태그(`d086ce4`) 릴리스 빌드 **macOS·Windows 성공**. `0.1.1` .dmg/.exe/.msi + app.tar.gz 첨부. 릴리스 노트 작성(사용 팁 강조, v0.1.1 다운로드 링크). 사용자 승인으로 **공개 완료**
- **함정 발견·수정 — prerelease와 releases/latest**: `releases/latest`는 **정식(비-prerelease) 최신만** 가리킴. v0.1.0·v0.1.1이 모두 prerelease라 문서의 latest 링크가 `/releases` 목록으로 튕겼음 → 두 릴리스 모두 `--prerelease=false`로 끄고 `release.yml`의 `prerelease: true`→`false`로 변경. 확인: API `isLatest=true`(v0.1.1), `releases/latest`→`/tag/v0.1.1` 정상. 베타 표시는 버전·노트로 대체

**논의 — 앱 자동 업데이트(다음 작업 후보)**
- 사용자 질문: v0.1.0 받은 사람이 v0.1.1로 어떻게 업데이트? 현재는 **수동 재다운로드만 가능**(앱 본체는 자기 업데이트 기능 없음. 홈의 "업데이트"는 Claude/Codex 에이전트 대상이지 앱 본체가 아님)
- 해결책: `tauri-plugin-updater`. 업데이터 서명 키는 **무료**(ed25519, 유료 코드 서명과 별개). 릴리스가 이미 `app.tar.gz`(업데이터 번들) 생성 중이라 절반 준비됨. 플러그인+키+`latest.json` 설정+앱 체크 로직만 추가하면 됨
- 함정: (1) 소급 안 됨 — v0.1.0/v0.1.1 사용자는 업데이터 첫 탑재 버전(예 v0.1.2)까지 한 번 수동 설치 필요, 이후 자동. (2) macOS 미서명이면 업데이트 후 첫 실행 경고 재발 가능(기능엔 무관)

**추가 작업 — 초보자용 "코딩 에이전트 사용 팁" 추가 (졸업식 + 홈)**
- 배경: 온보딩 끝난 비개발자가 "그래서 뭘 어떻게 시키지?"에서 다시 막히는 게 제품 최대 공백. 사용자 제안으로 팁 추가, 위치는 두 곳 다 합의
- `src/App.tsx`: 재사용 컴포넌트 `UsageTips` 신설. **졸업식** 성공 화면 next-guide 뒤에 인라인(`agent` prop 전달), **홈** 하단에 접이식 `<details className="tips-card">` 카드로 배치
- 팁 구성: ①예시 프롬프트 3(복사용) ②알아두면 좋은 것 3 ③**에이전트별 특징** ④**컨텍스트 파일**(CLAUDE.md·AGENTS.md에 규칙 적어두면 매번 참고). 졸업식은 해당 에이전트만, 홈은 둘 다 표시
- **에이전트 특징 문구는 실제 화면 용어를 그대로 사용**(Claude=`auto mode`, Codex=`goal`) + 괄호로 한국어 뜻. 사용자 피드백: "자동 승인/목표"로 의역하면 실제 버튼을 못 찾아 애매함
- i18n: `tips.*`·`tips.agent.{claude-code,codex}`·`tips.context.*`·`home.tips.title` 키를 ko/en 양쪽에 추가(tsc가 MessageKey 정합성 검증). 전문용어 없이 비개발자 눈높이
- CSS(`src/App.css`): 팁 카드·예시 칩·접이식 스타일 추가. **예시 프롬프트는 복사해서 쓰는 것이라 `user-select` 예외 목록에 `.tips-examples li` 추가**(전역 user-select:none 때문에 안 그러면 복사 불가)
- 참고: `ProjectInfo`엔 agent 필드가 없음 → 졸업식은 `GraduationStep`의 `agent` prop을 사용
- 검증: `pnpm build`(tsc+vite) 통과. 아직 커밋 안 함(사용자 승인 대기)

**한 일 — README 현행화 커밋·push, 정식 배포(코드 서명) 경로 정리**
- 앞선 Codex의 README 보완 변경(루트·웹사이트 README + 이력)을 한 커밋으로 묶어 커밋·push (`1e59690`). 세 파일을 함께 묶은 이유는 history 최신 엔트리가 바로 그 README 작업 기록이라 코드+이력이 한 단위이기 때문
- push 후 CI 끝까지 감시: website / build-macos / build-windows / test(macOS·Windows) **전부 success**
- 사용자 질문("정식버전은 어떻게? 뭘 등록?")에 정식 배포 경로 분석 제공

**정식 배포(코드 서명) 정리 — 다음 작업자 참고**
- **macOS**: Apple Developer Program 등록($99/년) → Developer ID 서명 → notarization(공증) → staple. 최신 macOS는 우클릭>열기 우회도 조여지는 추세라 사실상 필수
- **Windows**: OV($100~300/년) 또는 EV($300~600+/년) 코드사인 인증서. 2023년부터 키는 하드웨어 토큰/HSM 의무 → CI 자동 서명은 클라우드 HSM(예: Azure Trusted Signing ~$10/월) 필요. EV는 SmartScreen 경고 즉시 제거
- **배포 채널**: GitHub Releases 추천(무료, 현 CI가 이미 .dmg/.exe 아티팩트 생성). `tauri-plugin-updater`로 앱 내 자동 업데이트 가능. **Mac App Store는 샌드박스 필수라 "다른 CLI를 설치하는 앱" 성격상 부적합 — 비추천**
- **제안한 단계**: (1) 무료 미서명 GitHub Releases = 베타/지인용, (2) macOS 서명·공증부터($99), (3) Windows 인증서+Azure Trusted Signing. 서명 자리를 잡아둘 릴리스 워크플로(태그 push→빌드→릴리스 생성)는 미착수(사용자 결정 대기)

**추가 작업 — GitHub Releases 자동 배포 워크플로 신설(`.github/workflows/release.yml`)**
- 사용자가 "GitHub 릴리즈가 맞는 방향" 확정 → `v*` 태그 push 시 빌드하는 릴리스 워크플로 추가
- `tauri-apps/tauri-action`으로 빌드+릴리스 생성/업로드 일원화. macOS는 `--target universal-apple-darwin`(Intel·Apple Silicon 공용 .dmg), Windows는 기본(.exe). 매트릭스 두 잡이 같은 tagName으로 한 릴리스에 아티팩트를 누적
- `releaseDraft: true` + `prerelease: true` → 초안으로 올라가 사람이 검토 후 공개. 미완성 공개 방지
- **서명은 자리만 확보**: Apple 시크릿(APPLE_CERTIFICATE/…/APPLE_TEAM_ID)을 env로 연결해둠. 시크릿이 비면 미서명 빌드, 채우면 코드 변경 없이 서명·공증 켜짐
- 아직 태그 미발행 → 실제 릴리스는 돌지 않음. YAML 문법만 로컬 검증(python yaml.safe_load 통과). 아직 커밋 안 함(사용자 승인 대기)

**추가 작업 — 소개 웹사이트 Vercel 배포 문제 진단·해결(prerender 방식)**
- 증상: 사용자가 Vercel에 배포(프리셋 Vite) 성공했으나 페이지가 안 뜸
- **근본 원인**: `website`는 표준 Next가 아니라 **vinext**(Vite+Cloudflare Worker) 앱. 빌드 산출물 `dist/`에 **`index.html`이 없음** — HTML은 요청 때 `dist/server/index.js`(Worker)가 SSR로 생성. Vercel 정적 서빙엔 렌더할 서버가 없어 빈 화면. (`dist/server/wrangler.json`도 자동 생성돼 Cloudflare 배포는 원래 준비돼 있음)
- 사용자가 "모든 게 Vercel에 있다"며 Vercel 유지 원함 → **prerender 방식** 채택: 페이지가 완전 정적(next/image·데이터패칭 없음, 유일 동적요소는 `layout.tsx`의 `headers()` 기반 og 절대 URL)이라 안전
- 신설 `website/build/prerender.mjs`: 빌드 후 Worker를 1회 `fetch("/")` 실행해 완성 HTML을 `out/index.html`로 저장 + `dist/client/`(assets·og.png) 복사 + `app/{icon,apple-icon}.png`를 정적 경로로 복사. 호스트는 `VERCEL_PROJECT_PRODUCTION_URL`(빌드 시 주입)→`SITE_HOST`→기본값
- 신설 `website/vercel.json`: `framework: null`, installCommand는 CI와 동일한 rolldown optional-deps 우회(`rm -f package-lock.json && npm install`), buildCommand `npm run build && node build/prerender.mjs`, outputDirectory `out`
- **검증(로컬)**: 전체 체인 `npm run build`(exit 0) → prerender(exit 0, 29KB HTML). HTML이 참조하는 `/assets/*.js·css` 5개 전부 `out/`에 실존, og 절대 URL·title·아이콘 정상. `npm run lint` 클린(eslint config가 `out/**`·`build/**` 무시), `npm test` 2 pass. `out/`·`dist/`는 이미 gitignore
- 미검증: 실제 Vercel 빌드 컨테이너에서의 재현(push 후 확인 필요)

**추가 작업 — 첫 릴리스(v0.1.0) 실행, macOS 서명 실패 → 수정**
- 사용자가 Vercel 배포 성공 확인 후 릴리스 진행. `v0.1.0` 태그 push로 release.yml 첫 실행(태그·push는 auto-mode 분류기가 차단 → 사용자가 `!`로 직접 실행)
- 결과: **Windows 성공**(초안 릴리스에 `.exe`·`.msi` 첨부), **macOS 실패**
- **실패 원인**: release.yml에 `APPLE_*` env를 빈 시크릿으로라도 넘겼더니 tauri가 서명을 시도 → `security import: failed to import keychain certificate`. 인증서 없는데 서명 경로가 켜진 것
- **수정**: env 블록에서 `APPLE_*` 6개를 제거(빈 값도 넘기지 않음)해 미서명으로 빌드되게 함. 서명 켜는 방법은 주석으로 명시. YAML 검증 통과
- **주의(핵심 발견)**: 미서명 배포 시 `APPLE_*` env는 아예 넘기지 말 것. 빈 문자열이라도 존재하면 tauri가 서명을 시도하다 실패. Windows는 서명 env가 없어 미서명으로 정상 빌드됨
- 재실행 주의: 워크플로 파일은 **태그가 가리키는 커밋** 기준으로 실행됨 → 수정본을 적용하려면 v0.1.0 태그를 새 커밋으로 재지정(delete+recreate) 후 재push해야 함. 기존 초안 릴리스는 tauri-action이 같은 tagName으로 재사용

**재실행 결과 — 성공 ✅**
- 수정본(`84201b2`)으로 v0.1.0 태그 재지정·push → 워크플로 재실행. **macOS·Windows 둘 다 success**
- 초안(draft·prerelease) 릴리스 `v0.1.0`에 아티팩트 4개 첨부: `Hello.Agent_0.1.0_universal.dmg`(6.6MB)·`Hello.Agent_universal.app.tar.gz`·`Hello.Agent_0.1.0_x64-setup.exe`(2.3MB)·`Hello.Agent_0.1.0_x64_en-US.msi`(3.4MB)
- 릴리스 파이프라인 **첫 통과 검증 완료**. 이후 릴리스는 `vX.Y.Z` 태그 push만 하면 됨(단, tauri.conf.json version과 태그 일치)
- 미공개 상태: 초안이라 GitHub Releases에서 사람이 직접 Publish해야 공개됨

**공개(Publish) 완료 ✅ — v0.1.0 정식 라이브**
- 릴리스 노트 작성: 6단계 온보딩·홈베이스 소개 + 다운로드 표(실제 파일 링크) + 미서명 경고 우회법(macOS 우클릭>열기/시스템 설정, Windows 추가 정보>실행). `gh release edit`로 적용
- **다운로드 링크 주의(발견)**: 초안 상태의 자산 URL은 `.../download/untagged-<hash>/...` 형태 → Publish하면 `.../download/v0.1.0/...`로 바뀜. 그래서 노트 링크는 처음부터 `v0.1.0` 정식 형태로 작성(초안일 땐 404, 공개 후 동작)
- 사용자 승인으로 공개: `gh release edit v0.1.0 --draft=false`. `draft=false, prerelease=true`. dmg 다운로드 링크 HEAD 200 확인
- 공개 URL: https://github.com/wonseok-han/hello-agent/releases/tag/v0.1.0

**다음 할 일**
- [ ] 웹사이트 다운로드 섹션·README의 GitHub 링크를 실제 릴리스(v0.1.0 다운로드)로 반영
- [ ] Apple Developer / Windows 코드사인 인증서 등록 여부 결정 → 시크릿 채우고 release.yml에 APPLE_* env 복원해 서명 활성화
- [ ] (미커밋) 이 이력 업데이트 커밋·push 대기
- [ ] 첫 릴리스 시 `v0.1.0` 태그 push → 초안 릴리스 확인 후 공개. 릴리스 워크플로 실제 통과 검증(현재 미검증, 태그 발행해야 실행됨)
- [ ] Apple Developer / Windows 코드사인 인증서 등록 여부 결정 → 시크릿 채워 서명 활성화
- [ ] (참고) Vercel 대신 Cloudflare Workers도 `npm run build` 후 `wrangler deploy`로 즉시 가능(SSR·이미지 최적화 유지). prerender는 정적화라 `/_vinext/image` 런타임 최적화는 포기(현재 페이지는 미사용이라 무영향)

### 2026-07-23 · by GPT-5 Codex

**한 일 — 프로젝트와 소개 웹사이트 README 보완**
- 루트 README의 오래된 한국어 단일 지원 표기를 한·영 지원으로 바로잡고, 공개 다운로드 전 베타 준비 상태를 명시
- 온보딩 이후 프로젝트 자동 발견, 에이전트 상태·업데이트, 오류 복구를 제품 핵심 기능으로 추가
- 데스크톱 앱의 개발 요구 사항, 실제 설치 E2E, CI 검증 범위와 작업 이력 문서 링크를 보강
- 웹사이트 README에 Node 요구 버전, 명령별 역할, 파일 구조, 수정 시 동기화 원칙, 렌더링 테스트와 OpenAI Sites 배포 구조를 정리

**검증**
- 두 README의 명령·지원 범위·배포 설명을 `package.json`, `website/package.json`, CI, Sites 설정, 최신 워크로그와 대조
- 웹사이트 README에 기록한 페이지·아이콘·공유 이미지·테스트·호스팅 설정 파일의 실제 존재 확인
- `website/`의 `npm test`: 빌드 및 렌더링 테스트 2 passed
- `website/`의 `npm run lint` 통과

**다음 할 일**
- [ ] 깨끗한 macOS·Windows 환경에서 출시 후보를 검증한 뒤 공개 다운로드 링크와 설치 안내 추가
- [ ] 첫 베타 배포 시 웹사이트 README에 실제 배포 주소와 운영 절차 기록

### 2026-07-22 및 그 이전 → `docs/archive/history-2026-07.md`

15개 엔트리(2026-07-16 ~ 2026-07-22)를 아카이브로 옮겼다. 재사용할 결정·함정은 위 `핵심 결정·발견`에 반영돼 있으므로, 통상 아카이브를 열 필요는 없다.
