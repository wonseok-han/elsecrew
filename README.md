# Elsecrew

> 개발을 몰라도 Claude Code·Codex 팀에게 일을 맡기고, 결과를 확인해 내 프로젝트에 반영하는 데스크톱 앱

[![CI](https://github.com/wonseok-han/elsecrew/actions/workflows/ci.yml/badge.svg)](https://github.com/wonseok-han/elsecrew/actions/workflows/ci.yml)

Elsecrew(엘스크루)는 비개발자가 GUI에서 코딩 에이전트와 함께 여러 프로젝트를 관리하도록 돕는 작업대입니다. 터미널을 열 필요 없이, 하고 싶은 일을 적으면 총괄 에이전트가 기획·디자인·개발·검증 팀원에게 일을 나눠 맡기고 결과를 정리해 줍니다.

> 현재 개발 중입니다. 배포용 설치 파일과 배포 방식은 아직 준비 중이에요.

## 무엇을 해 주나요

- **팀에게 일 맡기기** — 작업 하나에 목표를 적으면 총괄이 직접 답하거나 필요한 팀원에게 위임합니다. 역할 이름·맡은 일·모델은 프로젝트마다 바꿀 수 있어요.
- **원본은 안전하게** — 모든 작업은 원본과 분리된 작업 공간에서 이뤄집니다. 변경 내용을 비교해 보고, 원할 때만 원본에 반영하고 되돌릴 수 있어요.
- **여러 프로젝트와 작업** — 프로젝트별로 작업 목록과 대화 기록이 남고, 앱을 다시 열어도 이어서 진행합니다.
- **알림** — 작업 완료·확인 필요·작업 중단을 알림 목록과 macOS 알림으로 알려 줍니다.
- **에이전트 설치·연결** — Claude Code와 Codex를 앱 안에서 설치하고 이 앱 전용으로 로그인합니다.

## 지원 범위

| 항목 | 지원 |
|---|---|
| 코딩 에이전트 | Claude Code (Anthropic), Codex (OpenAI) |
| 운영체제 | macOS (Windows는 화면·저장만 지원, 실제 에이전트 실행은 준비 중) |
| 언어 | 한국어, 영어 |

## 개발

Electron + React 19 + TypeScript + Monaco 화면과 Rust 안전 sidecar로 구성됩니다. 패키지 매니저는 pnpm 11, Node.js 22와 Rust 1.85 이상이 필요합니다.

```bash
pnpm install --frozen-lockfile   # 의존성 설치
pnpm m5:dev                      # sidecar 빌드 후 개발 모드 실행
pnpm build                       # 타입 검사 + 화면 빌드
npm test                         # 프런트 단위 테스트
pnpm m5:test:core                # Rust workspace 테스트
pnpm m5:package:smoke            # 패키지 생성 + 기동 확인
```

프로젝트 소개 웹사이트는 `website/`에서 따로 관리합니다(Elsecrew 기준으로 다시 만들 예정).

## 문서

- [문서 안내](docs/README.md) — 문서 분류와 색인
- [기술 설계](docs/architecture.md) — 시스템 구성과 계층별 책임, 구현 기록
- [작업 이력](docs/history.md) — 현재 상태, 주요 결정, 다음 작업
- [릴리스 체크리스트](docs/release-checklist.md) — 배포 전 검증 항목
