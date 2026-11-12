# Elsecrew

> 터미널을 한 번도 안 열어본 사람을 **코딩 에이전트가 굴러가는 상태까지** 데려다주는 데스크톱 앱

[![CI](https://github.com/wonseok-han/hello-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/wonseok-han/hello-agent/actions/workflows/ci.yml)

코딩 에이전트(Claude Code, Codex)를 쓰고 싶지만 **설치·설정 단계에서 막히는** 비개발자를 위한 도우미예요. 검은 터미널 창을 열 필요 없이, 버튼을 따라 누르기만 하면 첫 대화까지 도달합니다. 설치가 끝난 뒤에도 프로젝트와 에이전트 상태를 한곳에서 관리할 수 있습니다.

> **베타 버전이 공개되어 있어요.** [Releases](https://github.com/wonseok-han/hello-agent/releases/latest)에서 최신 macOS(`.dmg`)·Windows(`.exe`/`.msi`) 설치 파일을 받을 수 있습니다. 아직 코드 서명 전이라 처음 실행할 때 한 번만 보안 허용이 필요합니다(자세한 안내는 릴리스 노트 참고).

## 무엇을 해 주나요

더블클릭으로 실행하는 데스크톱 앱이 **6단계**를 대신 처리합니다. 터미널은 한 번도 보이지 않고, 모든 진행 상황은 쉬운 한국어 또는 영어로 안내돼요.

1. **에이전트 선택** — Claude Code / Codex 중 쓰던 구독에 맞게 고르기
2. **진단** — 내 컴퓨터에 이미 설치돼 있는지, Node 등 환경 확인
3. **설치** — 공식 배포처에서 자동 설치 + 터미널 PATH까지 정리
4. **로그인** — 브라우저 로그인 연동 (구독/API 방식 안내 포함)
5. **첫 프로젝트** — 안전장치가 들어간 작업 폴더 생성
6. **졸업식** — 에이전트와 첫 대화를 나누고 마무리

## 설치 후에도

Elsecrew는 한 번 쓰고 지우는 설치 마법사에 머물지 않습니다.

- 기준 폴더에서 Claude Code·Codex 프로젝트를 자동으로 찾습니다.
- 각 에이전트의 설치·로그인 상태와 현재 버전을 보여줍니다.
- 새 버전이 있으면 앱 안에서 업데이트할 수 있습니다.
- 네트워크, 권한, 폴더 문제를 만나면 원인과 해결 방법을 쉬운 말로 안내합니다.

## 지원 범위 (MVP)

| 항목 | 지원 |
|---|---|
| 코딩 에이전트 | Claude Code (Anthropic), Codex (OpenAI) |
| 운영체제 | macOS, Windows |
| 언어 | 한국어, 영어 |

## 왜 필요한가요

초보자의 코딩 에이전트 첫 사용은 실제로 40분 이상 걸리고, 대부분 **터미널 공포·PATH 문제·개념 혼란·에러 복구 불가**에서 이탈합니다. 시중엔 설치 가이드 글은 많아도 *도구*는 없었어요. Elsecrew는 그 공백을 채우는, "이 앱 하나 받아서 실행해"로 끝나는 물건을 목표로 합니다.

## 개발

Tauri 2 + React 19 + TypeScript, 패키지 매니저는 pnpm 11입니다. Rust는 설치·감지·로그인 등 시스템 작업에만 최소한으로 씁니다. Node.js 22와 Rust 1.85 이상이 필요합니다.

```bash
pnpm install --frozen-lockfile                  # 의존성 설치
pnpm tauri dev                                  # 개발 모드 실행
pnpm build                                      # 프론트엔드 빌드 검증 (tsc + vite)
cargo test --manifest-path src-tauri/Cargo.toml # Rust 테스트
pnpm tauri build                                # 현재 OS용 설치 파일 빌드
```

프로젝트 소개 웹사이트는 데스크톱 앱과 분리된 `website/`에서 관리합니다.

```bash
cd website
npm install
npm run dev   # 웹사이트 개발 서버
npm test      # 배포 빌드 + 렌더링 검증
npm run lint  # 코드 검사
```

- 네트워크가 필요한 실제 설치 E2E는 `cargo test --manifest-path src-tauri/Cargo.toml isolated_install -- --ignored --nocapture`로 실행합니다.
- CI는 macOS·Windows 앱 테스트와 설치 파일 빌드, 소개 웹사이트 빌드·렌더링·린트를 검증합니다.

## 문서

- [문서 안내](docs/README.md) — 문서 분류와 색인. 어느 문서를 언제 여는지
- [기술 설계](docs/architecture.md) — 시스템 구성, 위저드 상태 머신, 에이전트 레시피, 플랫폼별 전략, 검증 현황
- [릴리스 체크리스트](docs/release-checklist.md) — 자동 검증, 깨끗한 기기 확인, 실패 경로, 서명·배포 기준
- [소개 웹사이트](website/README.md) — 랜딩 페이지 개발·배포 구조
- [작업 이력](docs/history.md) — 현재 구현 상태, 주요 결정, 다음 작업
- 검토 중인 제안은 `docs/proposals/`, 지난 작업 이력은 `docs/archive/`, 설계 다이어그램은 `docs/diagrams/*.excalidraw`
