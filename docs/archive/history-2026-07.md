# 작업 이력 아카이브 (Hello, Agent)

> `../history.md`에서 넘어온 **과거 워크로그**다. 최신 상태와 미완료 작업은 `../history.md`를 본다.
>
> 여기 있는 엔트리는 **당시 시점의 기록**이다. 이후 코드·결정이 바뀌었을 수 있으니 현재 사실로 그대로 인용하지 말고, 재사용할 결정·함정은 `../history.md`의 `핵심 결정·발견`에 이미 반영돼 있다.
>
> 아카이브 기준: `../history.md`가 500줄을 넘으면 오래된 엔트리부터 `archive/history-YYYY-MM.md`로 옮긴다. 내용은 삭제하지 않는다.

---

## 워크로그 (최신이 위) — 2026-07-23 및 그 이전

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

### 2026-07-22 · by GPT-5 Codex

**한 일 — 출시 검증 자동화와 체크리스트 보강**
- 실제 공식 배포 파일을 내려받는 격리 설치 E2E를 네트워크 허용 환경에서 실행해 Claude Code와 Codex의 임시 HOME 설치, PATH 반영, 버전 확인까지 검증
- 소개 웹사이트가 데스크톱 앱 CI에서 빠져 있던 공백을 보완: Ubuntu에서 `npm ci` → 빌드·렌더링 테스트 → ESLint를 실행하는 `website` job 추가
- `docs/release-checklist.md` 신설: 자동 검증, 깨끗한 macOS/Windows 계정, 네트워크·권한·잘못된 기준 폴더 등 닥터 실패 경로, 미서명 베타와 정식 서명 배포 게이트를 한곳에 정리
- README 문서 목록에 릴리스 체크리스트 연결

**검증**
- `cargo test --manifest-path src-tauri/Cargo.toml isolated_install -- --ignored --nocapture`: 2 passed — Claude Code 2.1.217, Codex CLI 0.145.0 설치·검증
- `cargo test --manifest-path src-tauri/Cargo.toml`: 15 passed, 0 failed, 6 ignored
- `pnpm build` 통과
- `website/`의 `npm test`: 빌드 및 렌더링 테스트 2 passed
- `website/`의 `npm run lint` 통과

**다음 할 일**
- [ ] `docs/release-checklist.md`에 따라 네트워크 단절·권한 오류·잘못된 기준 폴더 닥터 UI를 실기기에서 확인
- [ ] Actions가 만든 DMG·NSIS 설치 파일을 깨끗한 macOS 계정과 Windows 기기에서 전체 검증
- [ ] 미서명 제한 베타 또는 macOS 공증·Windows 코드 서명을 포함한 정식 배포 방식 결정 후 첫 릴리스

### 2026-07-22 · by GPT-5 Codex

**한 일 — 홈베이스와 오류 복구 흐름 보완**
- 진단·로그인·프로젝트 생성의 원시 오류 표시를 `DoctorCard`로 교체하고 재시도 제공
- 홈의 에이전트 상태 확인·업데이트 실패도 설정 화면 이동 대신 인라인 닥터로 원인과 재시도 표시
- 기준 폴더가 없거나 읽을 수 없을 때 빈 프로젝트 목록으로 숨기지 않고 닥터·재시도·다른 폴더 선택 제공
- 준비된 에이전트가 하나면 홈의 `새 프로젝트`에서 프로젝트 단계로 직행하고, 둘 다 준비됐으면 선택 후 직행
- 선택 언어를 프로젝트 생성과 첫 대화 명령에 전달해 `CLAUDE.md`/`AGENTS.md` 초보자 안내, Codex 안전 설정 주석, 첫 환영 인사를 한·영으로 생성
- `agent.rs`·`login.rs`·`editor.rs`·`project.rs`의 사용자에게 노출될 수 있는 기술 오류를 영어 상세 정보로 통일
- pnpm 11에서 무시되는 `package.json`의 중복 `onlyBuiltDependencies`를 제거하고 기존 `pnpm-workspace.yaml`의 `allowBuilds`만 사용
- README와 기술 설계를 실제 홈 진입·스캔·영속성·i18n 동작에 맞게 갱신

**검증**
- `pnpm build` 통과
- `cargo test --manifest-path src-tauri/Cargo.toml`: 15 passed, 0 failed, 6 ignored
- `pnpm tauri build --debug --bundles app` 통과, macOS `Hello Agent.app` 번들 생성
- 언어별 초보자 안내 생성과 존재하지 않는 기준 폴더 오류 분류 테스트 추가

**다음 할 일**
- [ ] 네트워크 단절·권한 오류·잘못된 기준 폴더를 실기기에서 만들어 닥터 UI 확인
- [ ] 깨끗한 macOS 계정과 Windows 기기에서 출시 후보 전체 흐름 검증
- [ ] 코드 서명·배포 방식 결정 후 첫 베타 릴리스

### 2026-07-22 · by GPT-5 Codex

**한 일 — 프로젝트 기억과 에이전트 인계 규칙 보강**
- 최신 코드와 워크로그를 대조해 다음 우선순위를 정리: ① 진단·로그인·홈 전체에 닥터 적용 ② 재방문 새 프로젝트 흐름 단축 ③ 첫 대화/원시 오류 i18n ④ 프로젝트 스캔 실패 구분 ⑤ 출시 후보 실기기 검증
- `CLAUDE.md`와 `AGENTS.md`를 최신 홈베이스 구조·코드 지도·검증 명령·비자명한 함정에 맞게 갱신
- 코드 변경이 없는 분석·결정·검증·외부 작업도 `../history.md` 기록 대상임을 시작/작업 중/종료 체크리스트로 명시
- 두 에이전트 안내 파일을 `.gitignore`에서 제거해 저장소를 통해 공유되도록 변경
- 상단 `핵심 결정·발견`의 오래된 백엔드 오류·닥터 적용 범위를 실제 코드 기준으로 교정

**다음 할 일**
- [ ] 진단·로그인·홈의 설정/업데이트 실패에 `DoctorCard` 적용 및 실제 실패 경로 검증
- [ ] 홈의 `새 프로젝트`에서 준비된 에이전트를 미리 선택하고 완료 단계 단축
- [ ] 첫 대화 프롬프트와 남은 Rust 한국어 상세 오류 i18n 정리
- [ ] macOS/Windows 출시 후보 설치 시나리오 검증

### 2026-07-22 · by GPT-5 Codex

**한 일 — 오늘 앱 고도화 내용을 소개 웹사이트에 반영**
- 메인 앱 미리보기를 에이전트 선택 화면에서 `내 에이전트`·`내 프로젝트` 홈 화면으로 교체
- 프로젝트 자동 발견, 설치·로그인 상태 확인, 에이전트 업데이트를 소개하는 `설치 후에도` 섹션 추가
- 이미 설치한 사용자와 재방문 사용자를 설명하도록 히어로·FAQ·검색/Open Graph 설명 갱신
- 웹사이트 `npm test`(빌드 + 렌더링 테스트)와 `npm run lint` 통과

### 2026-07-22 · by Claude Opus 4.8 (5)

**한 일 — 프로젝트 목록을 앱 기억→디스크 스캔 기반으로 (사용자 지적)**
- 문제: 홈 목록이 store(앱 기억)에서만 왔음 → 디스크에 폴더가 있어도 앱이 안 만든 것이면 "프로젝트 없음"
- `project.rs`: `scan_projects(base)` — 기준 폴더 하위에서 에이전트 표식(`.claude`/`CLAUDE.md`→클로드, `AGENTS.md`→코덱스)이 있는 폴더를 발견해 목록화. `default_projects_dir`(기본 Documents). `create_first_project`에 `base` 인자 추가(새 프로젝트가 기준 폴더에 생성)
- 기준 폴더를 사용자가 지정: `tauri-plugin-dialog` 추가, 홈에 "이 폴더에서 찾는 중 + 폴더 바꾸기"(디렉터리 선택). store에 `baseDir` 저장
- store.ts: `getBaseDir/setBaseDir/lastOpenedMap`. 목록은 스캔 결과 + store의 최근 사용 시각으로 정렬. "목록에서 지우기"는 제거(목록=디스크 실제 폴더라 의미 안 맞음)
- **실기기 검증**: store 비운 상태에서 `~/Documents/my-first-project`(AGENTS.md)가 코덱스 프로젝트로 자동 인식·표시. cargo test 12개 + pnpm build 통과

### 2026-07-22 · by Claude Opus 4.8 (4)

**한 일 — 홈 진입 조건 개선 + 레이아웃 수정 (사용자 요청)**
- 홈 진입 조건: 저장된 프로젝트가 있거나 **에이전트가 이미 설치·로그인돼 있으면** 홈으로(온보딩 위저드 건너뜀). 둘 다 아니면 위저드. `isAnyAgentReady()`가 agent_status로 판정. 실기기: 프로젝트 0인데 claude 준비돼 있어 홈 직행 확인
- 레이아웃: 헤더 하단 여백이 위저드의 `.steps` margin에만 의존해 홈 뷰에서 패널이 타이틀에 붙던 문제 → `.header{margin-bottom}` + `.steps` top margin 제거로 두 뷰 일관화

### 2026-07-22 · by Claude Opus 4.8 (3)

**한 일 — 고도화: 상주 에이전트 상태 + 업데이트 알림 (홈베이스의 "재방문 이유")**
- `src-tauri/src/status.rs` 신설 — `agent_status`(설치·버전·로그인, 로컬 빠름) + `latest_agent_version`(네트워크). 최신 버전: Claude=`downloads.claude.ai/.../stable`, Codex=`github.com/.../releases/latest` 리다이렉트 최종 URL에서 semver 추출(api.github.com은 60회/시 제한이라 회피)
- 홈에 "내 에이전트" 패널(`AgentRow`): 각 에이전트 설치/로그인/업데이트 상태를 한눈에. 미설치·미로그인→"설정하기"(위저드 진단부터, 자동 스킵), 업데이트 가능→"업데이트"(인라인 install_agent). 최신 확인은 백그라운드(느림/오프라인이면 조용히 무시)
- 프론트 semver 비교(`isNewer`), `detect::agent_version`/`login::is_logged_in` pub(crate) 노출
- **실기기 검증**: 홈에서 클로드="설치됨·2.1.216"(최신) 정상 렌더. cargo test 11개 + pnpm build 통과
- **감지 갭 수정**(사용자 지적): OpenAI가 Codex.app→ChatGPT.app 통합으로 CLI 경로 이동 → 코덱스가 "미설치"로 잘못 표시됨. `agent.rs` Codex 후보를 `/Applications/{ChatGPT,Codex}.app/...` 여러 개로 확장. 실기기에서 `ChatGPT.app/.../codex` 0.145.0 감지 확인. 교훈: 앱 번들 경로 단일 하드코딩은 벤더 변경에 취약

**다음 할 일**
- [ ] 업데이트/설정 실패 시 홈에서 DoctorCard 노출(현재는 설정 화면으로 유도만)
- [ ] 재방문 "새 프로젝트"에서 에이전트 미리 선택
- [ ] 첫 대화 프롬프트 한국어 고정(콘텐츠 i18n)
- [ ] 레시피 외부화(agent.rs → 원격 JSON)

### 2026-07-22 · by Claude Opus 4.8 (2)

**한 일 — 고도화 ②③: 영속성 + 홈베이스("일회성 도구" 탈출)**
- `docs/architecture.md §3` 재설계 — 단일 선형 위저드 → 뷰 라우팅(홈/위저드) 홈베이스. 앱 실행 시 저장된 프로젝트 있으면 홈, 없으면 온보딩 위저드
- 영속성: `tauri-plugin-store` 추가(Rust 등록 + `store:default` capability + JS `@tauri-apps/plugin-store`). `src/store.ts`가 프로젝트 목록을 앱 데이터 디렉터리 JSON에 저장(경로·에이전트·이름·생성/최근사용). 에이전트 설치·로그인 상태는 저장 안 하고 매번 실시간 감지
- `App.tsx`에 `view` 라우팅 + `HomeView`(프로젝트 카드: 열기/목록에서 지우기, 새 프로젝트, 최근순 정렬). `ProjectStep`이 생성 시 `saveProject`, 졸업식에 "내 프로젝트 →" 링크
- **실기기 검증 완료**: store에 샘플 심고 재실행 → 홈 화면·프로젝트 카드 정상 렌더 확인. cargo test 10개 + pnpm build 통과. 테스트 데이터 정리함
- 주의: 앱 바이너리명이 rename 후 **`hello-agent`** (pgrep/스크린샷 시 옛 이름 agent-starter 아님)

**다음 할 일**
- [ ] 상주 닥터(홈에서 재진단) + 에이전트 업데이트 알림 — 홈베이스의 "재방문 이유" 완성
- [ ] 재방문 "새 프로젝트"에서 에이전트 미리 선택(현재는 위저드가 install/login만 자동 스킵)
- [ ] 첫 대화 프롬프트 한국어 고정(콘텐츠 i18n)
- [ ] 레시피 외부화(agent.rs → 원격 JSON)

### 2026-07-22 · by Claude Opus 4.8

**한 일 — 고도화 착수(기술부채 정리 ①): 백엔드 에러 구조화**
- `src-tauri/src/error.rs` 신설 — `AppError { kind, detail }` + `ErrorKind`(network/checksum/not-found/permission/disk/generic). 모든 커맨드가 `Result<T, String>` → `Result<T, AppError>`로 전환
- 원인을 아는 지점은 명시적 kind(예: curl 실패→network, tar 실패→checksum), io 에러는 `AppError::classify`로 원문(영어) 기반 분류. **에러 detail이 더는 한국어 프로즈가 아니라 영어 기술 텍스트** → 영어 모드에서 "자세한 내용"도 영어
- 프론트 `doctor.ts`: `diagnoseError`(kind 우선, generic이면 로그 정규식 보완) + `toAppError`(객체/문자열 수용). `DoctorCard`가 문자열 대신 `AppError`를 받음
- 검증: cargo test 10개 통과(에러 분류 단위테스트 포함), pnpm build 통과. 해피패스(Ok) 구조 불변이라 런타임 리스크 낮음, 실제 에러 경로는 실기기 검증 대기

**다음 할 일**
- [ ] 영속성 계층(tauri-plugin-store) → 홈베이스의 전제
- [ ] 홈 화면 + 재방문 흐름(일회성 탈출) — App.tsx를 뷰 라우팅으로
- [ ] 상주 닥터 + 에이전트 업데이트 알림
- [ ] 첫 대화 프롬프트가 한국어 고정 — 영어 사용자에게 한국어 인사(콘텐츠 i18n, 별건)
- [ ] 레시피 외부화(agent.rs → 원격 JSON)

### 2026-07-21 · by GPT-5 Codex

**한 일**
- GitHub 저장소 About 설명을 비개발자용 Claude Code·Codex 설치 도우미라는 제품 성격이 드러나도록 작성
- 제품·기술 스택·지원 플랫폼 중심의 GitHub 토픽 13개 등록

### 2026-07-21 · by GPT-5 Codex

**한 일**
- 웹사이트 헤더의 코랄 브랜드 마크를 `brand/hello-agent-mark.svg` 정식 원본으로 제작
- Tauri 아이콘 생성기를 사용해 macOS `.icns`, Windows `.ico`, PNG·Appx 아이콘 세트를 새 브랜드 마크로 교체
- 웹사이트에 일반 파비콘(`app/icon.png`)과 Apple 터치 아이콘(`app/apple-icon.png`) 적용

**다음 할 일**
- [ ] 실제 macOS Dock·Windows 시작 메뉴에서 작은 크기 가독성 확인

### 2026-07-21 · by Claude Opus 4.8

**한 일**
- 경량 i18n(한/영) 도입 — react-i18next 없이 자체 Context/훅(`useI18n`), `locales/{ko,en}.ts` 리소스 분리, 헤더 언어 토글, 시스템 언어 초기값 + localStorage 저장. 실기기 영어 렌더링 확인
- M3 닥터 MVP — 에러·로그를 프론트에서 패턴 해석해 초보자용 원인·해결책을 i18n으로 표시 + 재시도(`DoctorCard`, `src/doctor.ts`). network/checksum/notfound/permission/disk 분류, 설치·졸업식 단계 적용, 패턴 매칭 6/6 검증

**다음 할 일**
- [ ] 로그인 단계에도 닥터 적용(네트워크 에러) — 현재 설치·졸업식만
- [ ] 백엔드 에러 i18n — Rust 에러 한국어 하드코딩, 영어 모드 "자세한 내용" 원문은 한국어. 완전 대응은 에러 구조화 필요
- [ ] macOS `.dmg` / Windows 실기기 설치 테스트
- [ ] 정식 배포 — 코드 서명, Intel용 universal 빌드

### 2026-07-21 · by GPT-5 Codex

**한 일**
- `website/`에 Hello, Agent 소개용 독립 랜딩 페이지 구축
- 앱의 Pretendard·중성 배경·코랄 포인트를 이어받은 반응형 디자인 구현
- 실제 제품 선택 화면을 HTML/CSS로 재현하고 6단계 흐름·안전 원칙·지원 에이전트·FAQ·베타 CTA 구성
- 링크 공유용 전용 OG 이미지와 요청 호스트 기반 Open Graph·X 메타데이터 추가
- OpenAI Sites/Cloudflare Worker 호환 배포 설정과 서버 렌더링 테스트 정리
- 헤더를 sticky 내비게이션으로 변경하고 모바일에서도 섹션 메뉴가 유지되도록 개선
- `npm test`(배포 빌드 + 렌더링)와 `npm run lint` 통과

**다음 할 일**
- [ ] 사용자 승인 후 Sites에 첫 비공개 버전 배포
- [ ] 실제 베타 릴리스가 나오면 GitHub 진행 상황 CTA를 OS별 다운로드 버튼으로 교체

### 2026-07-21 · by Claude Opus 4.8

**한 일**
- M2: 에이전트 추상화(`agent.rs`) + Codex 조기 편입, 요금제/로그인 방식 안내, 안전 프리셋(설정 파일 수준)
- Codex 데스크톱 앱 번들 CLI 감지 추가 (`/Applications/Codex.app/...`)
- 편집기(커서·VS Code) 감지 → "그 편집기로 폴더 열기" + **에이전트 확장 자동 설치**
- 첫 프로젝트 기본 이름 `my-first-project`로, 졸업식 "다음에 이렇게" 안내를 파일(.md) 대신 앱 UI로 이동
- 프로젝트명 변경: `agent-starter` → **Hello, Agent** (레포 rename `hello-agent`, 번들 `Hello Agent`, 식별자 `com.wonseokhan.helloagent`). 리모트 URL·CI 배지·내부 문자열까지 일괄 반영
- CI에 `build-macos` 잡 추가 → macOS `.dmg` + Windows `.exe` 둘 다 Actions artifact로 생성 (그린 확인)
- 이 워크로그(`../history.md`) 도입

**다음 할 일**
- [ ] macOS `.dmg` 실제 설치 테스트 (Actions `macos-installer` artifact → 설치 → "확인되지 않은 개발자" 흐름 체감)
- [ ] 언어 선택(i18n 한/영) — UI 문구 하드코딩 상태, 이름은 고정하고 텍스트만 리소스 분리
- [ ] Windows 실기기 검증 — Codex 앱 감지·편집기 확장(.cmd 경로)은 macOS로만 검증됨
- [ ] 미로그인→로그인 실플로우(클로드) — 키체인 전역이라 이 머신 재현 불가, 별도 계정/VM 필요
- [ ] M3 닥터 (에러 해석·자동 수정)
- [ ] 정식 배포 — 코드 서명 결정, Intel용 universal 빌드, GitHub Release

### 2026-07-16 · by Claude Fable 5

**한 일**
- Tauri 2 + React 19 스캐폴드, 기술 설계 문서·다이어그램
- M0 검증: 환경 감지 / 무인 설치(+PATH 반영, 격리 E2E) / 브라우저 로그인 — 4가지 가정 전부 통과, Electron 전환 불필요
- M1 위저드 완성: 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식(첫 대화)
- CI 구축(macOS/Windows 테스트 + 격리 E2E + Windows 번들). PSModulePath 함정·로그인 자동 승인 등 실전 버그 수정
- UI 디자인 패스(Pretendard, 코랄 액센트). Windows 실기기 로그인 플로우 확인

**다음 할 일** → 2026-07-21 엔트리로 이어짐
