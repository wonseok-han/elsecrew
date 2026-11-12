# Elsecrew website

Elsecrew 데스크톱 앱을 소개하는 한국어 랜딩 페이지입니다. 데스크톱 앱과 의존성·빌드·배포가 분리되어 있으며, Vinext와 OpenAI Sites를 통해 Cloudflare Worker 호환 결과물을 만듭니다.

## 로컬 실행

Node.js 22.13 이상과 npm이 필요합니다.

```bash
npm install
npm run dev
```

개발 서버가 출력한 로컬 주소를 브라우저에서 열면 됩니다. 다른 패키지 매니저의 잠금 파일을 추가하지 않고 `package-lock.json`을 기준으로 의존성을 관리합니다.

## 명령

```bash
npm run dev    # 개발 서버
npm run build  # Cloudflare Worker 호환 프로덕션 빌드
npm start      # 빌드 결과 로컬 실행
npm test       # 프로덕션 빌드 + 렌더링 결과 테스트
npm run lint   # ESLint 검사
```

`npm test`는 실제 Worker 진입점을 불러와 응답 상태, 핵심 제품 문구, 메타데이터, 고정 헤더 동작을 확인합니다. 저장소 CI에서도 빌드·렌더링 테스트·린트를 별도 작업으로 실행합니다.

## 구조

| 경로 | 역할 |
|---|---|
| `app/page.tsx` | 랜딩 페이지 콘텐츠와 섹션 |
| `app/globals.css` | 전역 스타일과 반응형 레이아웃 |
| `app/layout.tsx` | 검색·Open Graph 메타데이터 |
| `app/icon.png`, `app/apple-icon.png` | 파비콘과 Apple 터치 아이콘 |
| `public/og.png` | 링크 공유 미리보기 이미지 |
| `tests/rendered-html.test.mjs` | 프로덕션 렌더링 회귀 테스트 |
| `.openai/hosting.json` | OpenAI Sites 호스팅 설정 |
| `../brand/elsecrew-mark.svg` | 웹과 데스크톱 앱이 공유하는 브랜드 마크 원본 |

## 수정할 때

- 제품 기능이나 현재 지원 범위가 바뀌면 루트 `README.md`, `docs/history.md`, 웹사이트 문구를 함께 대조합니다.
- 주요 문구나 섹션 내비게이션을 바꾸면 `tests/rendered-html.test.mjs`의 검증도 갱신합니다.
- 브랜드 마크는 `../brand/elsecrew-mark.svg`를 원본으로 유지하고 파생 아이콘만 각 플랫폼 형식으로 생성합니다.
- 배포 전에는 `npm test`와 `npm run lint`를 모두 통과시킵니다.

## 배포

OpenAI Sites가 `.openai/hosting.json`을 읽어 배포합니다. 현재 사이트는 데이터베이스나 파일 저장소를 사용하지 않아 D1과 R2가 모두 비활성화되어 있습니다. 데스크톱 앱과는 별도 배포 단위이므로 사이트 변경이 앱 설치 파일을 다시 만들지는 않습니다.
