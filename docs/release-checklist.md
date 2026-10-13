# 릴리스 체크리스트

Hello, Agent의 설치 파일을 베타 또는 정식 버전으로 배포하기 전에 확인할 항목이다. 자동 테스트가 통과해도 깨끗한 사용자 환경에서만 발견되는 설치·로그인·보안 경고 문제는 별도로 확인한다.

## 1. 자동 검증

- [ ] 루트 `pnpm install --frozen-lockfile`
- [ ] `pnpm build`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml isolated_install -- --ignored --nocapture`
- [ ] `website/`에서 `npm ci`, `npm test`, `npm run lint`
- [ ] GitHub Actions의 macOS·Windows 테스트와 설치 파일 빌드가 모두 통과
- [ ] Actions에서 `macos-installer` DMG와 `windows-installer` EXE를 내려받을 수 있음

격리 설치 E2E는 공식 배포처에서 실제 파일을 내려받으므로 네트워크와 벤더 서비스 상태에 영향을 받는다. 실패하면 앱 회귀인지 외부 장애인지 로그로 구분한다.

## 2. 깨끗한 기기 검증

가능하면 기존 Claude Code·Codex·Node·개발용 PATH 설정이 없는 새 OS 사용자 또는 초기화한 가상 머신에서 진행한다.

### 공통 흐름

- [ ] 앱 첫 실행과 한국어·영어 전환
- [ ] Claude Code와 Codex를 각각 선택해 진단 → 설치 → 로그인 → 프로젝트 생성 → 첫 대화 완료
- [ ] 앱을 종료하고 다시 열었을 때 홈으로 진입하고 프로젝트를 자동 발견
- [ ] 에이전트 설치·로그인·업데이트 상태가 실제 상태와 일치
- [ ] `CLAUDE.md` 또는 `AGENTS.md`와 첫 대화가 선택 언어로 생성
- [ ] 앱 아이콘이 설치 화면, 앱 목록, Dock 또는 시작 메뉴의 작은 크기에서도 식별 가능

### macOS

- [ ] Actions의 DMG로 설치하고 앱 실행
- [ ] 미서명 베타라면 Gatekeeper 경고 후 시스템 보안 설정에서 사용자가 허용해 실행 가능
- [ ] 새 터미널에서 `claude --version`과 `codex --version` 실행 가능
- [ ] Apple Silicon에서 전체 흐름 완료
- [ ] Intel 지원이 필요하면 별도 Intel 또는 universal 번들에서 같은 흐름 완료

### Windows

- [ ] Actions의 NSIS EXE로 설치하고 앱 실행
- [ ] 미서명 베타라면 SmartScreen 경고 후 사용자가 명시적으로 실행 가능
- [ ] 새 PowerShell에서 `claude --version`과 `codex --version` 실행 가능
- [ ] 사용자 PATH 레지스트리 반영 후 로그아웃 없이 새 셸에서 명령 실행 가능
- [ ] 한글과 공백이 포함된 사용자명·프로젝트 경로에서 전체 흐름 완료

## 3. 실패 경로와 닥터

각 항목은 원시 오류만 노출하지 않고 쉬운 원인 설명, 해결 방법, 재시도 또는 폴더 변경 동작을 제공해야 한다.

- [ ] 네트워크를 끊고 설치·로그인·업데이트 시도 → 네트워크 닥터
- [ ] 존재하지 않는 기준 폴더 선택 또는 폴더 삭제 후 재실행 → 폴더 없음 닥터
- [ ] 읽기·쓰기 권한이 없는 기준 폴더에서 스캔·프로젝트 생성 → 권한 닥터
- [ ] 여유 공간이 부족한 환경에서 설치·프로젝트 생성 → 저장 공간 닥터
- [ ] 잘못된 체크섬 또는 손상된 설치 파일을 테스트 환경에서 주입 → 파일 손상 닥터
- [ ] 문제를 해소한 뒤 재시도로 정상 흐름 복귀

## 4. 배포 결정

### 미서명 베타

- [ ] 보안 경고와 실행 허용 절차를 릴리스 노트에 명확히 안내
- [ ] 설치 파일의 SHA-256 체크섬 제공
- [ ] 제한된 테스터에게 먼저 배포하고 OS·아키텍처·앱 버전·재현 절차를 포함해 피드백 수집

### 정식 릴리스

- [ ] macOS Developer ID 서명과 Apple 공증 완료
- [ ] Windows 코드 서명 완료
- [ ] 서명된 최종 설치 파일로 깨끗한 기기 검증을 다시 수행
- [ ] 버전, 변경 사항, 알려진 문제, 지원 OS와 아키텍처를 릴리스 노트에 기록
- [ ] GitHub Release와 소개 웹사이트의 다운로드 링크가 같은 최종 파일을 가리킴

## 5. 검증 기록

릴리스 후보를 확인한 날짜, 커밋 SHA, OS 버전과 아키텍처, 설치 파일 이름, 통과·실패 항목을 `docs/history.md`에 남긴다. 실패를 우회했다면 우회 방법도 함께 기록한다.

## 6. M5 Electron 후보의 추가 게이트

위 Tauri 배포 기록을 Electron 후보의 합격으로 대신하지 않는다. 현재 상태는 `history.md`·`architecture.md`, 수치는 `proposals/m5-electron-spike.md` §9–13과 `procedures/m5-performance.md`를 따른다. 아래는 남은 증거 요구이며 완료 표시가 아니다.

- [ ] 최신 후보의 여러 프로젝트·영속 채팅·역할 설정·코드 편집/diff·반영/복원·재시작을 패키징 앱에서 확인하고 전체 시안 비교 결과를 기록
- [ ] macOS 실제 에이전트 입력·출력·명시 중단·자손 소멸 확인; Codex planning 1회 성공을 쓰기·승인·복구 전체 합격으로 확대하지 않음
- [ ] Claude 앱 전용 로그인과 실제 작업 경로 확인; 계정/유료 모델 시험은 별도 승인 범위만 실행
- [ ] G6 시작·대형 파일/diff·입력·스트림·전체 프로세스 메모리·패키지 크기 측정; 실제 collector를 연결한 `m5:test:performance`와 최종 하네스 증거 확보 (합성 진단 JSON만으로 통과시키지 않음)
- [ ] Windows 패키지의 UI·sidecar·fixture 확인 및 실제 agent 실행 차단 유지; 자손 안전 구현 전 실행 활성화 금지
- [ ] 데이터 손실·교차 채팅 오염·원본 보호·미확인 종료/복구의 최종 실패 주입과 독립 검토
- [ ] 깨끗한 기기·서명/공증·배포 방식 결정과 최종 설치본 확인; main 병합·push·외부 배포는 별도 사용자 승인
