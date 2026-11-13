# 릴리스 체크리스트

Elsecrew의 설치 파일을 베타 또는 정식 버전으로 배포하기 전에 확인할 항목이다. 자동 테스트가 통과해도 깨끗한 사용자 환경에서만 발견되는 설치·로그인·보안 경고 문제는 별도로 확인한다.

## 1. 자동 검증

- [ ] 루트 `pnpm install --frozen-lockfile`
- [ ] `pnpm build` (타입 검사 + 화면 빌드)
- [ ] `npm test` (프런트 단위)
- [ ] `pnpm m5:test:core` (Rust workspace 전체)
- [ ] `pnpm m5:package:smoke` (패키지 생성 + 기동 smoke)
- [ ] `website/`에서 `npm ci`, `npm test`, `npm run lint`
- [ ] GitHub Actions의 macOS·Windows 기준선 하네스 통과
- [ ] Release 초안의 ZIP 3개와 `latest.json`이 모두 올라왔고, 이전 버전 앱에서 '업데이트 확인 → 재시작'으로 새 버전이 열림

### 배포 방식 (코드 서명·공증 없음)

- 앱은 유료 코드 서명·공증 없이 배포한다. 처음 설치할 때만 macOS Gatekeeper·Windows SmartScreen 허용이 필요하다.
- 업데이트는 프로젝트 Ed25519 키로 서명한 `latest.json`으로 검증한다. 공개키는 `electron/update-manifest.ts`에 들어 있고, 비밀키는 저장소 밖에 둔다(최초 생성 위치: 관리자 컴퓨터 `~/.config/elsecrew/update-signing-key.pem`).
- 최초 1회: GitHub 저장소 Settings → Environments에 `release` 환경을 만들고 필수 승인자(Required reviewers)와 배포 규칙(`v*` 태그만)을 설정한다. 그 환경의 Environment secrets에 `ELSECREW_UPDATE_PRIVATE_KEY`로 비밀키 PEM 전체를 등록한다(저장소 전체 비밀값으로 두지 않는다). 환경을 만들지 않으면 보호 규칙 없이 자동 생성되어 승인 없이 서명된다. 키를 바꾸면 이미 설치된 앱은 새 업데이트를 받지 못하므로 공개키를 바꾼 버전을 수동 설치로 먼저 배포해야 한다.
- 배포: `package.json`의 `version`을 올리고 커밋한 뒤 같은 값의 태그(`vX.Y.Z`)를 push한다. `.github/workflows/release.yml`이 macOS arm64·x64와 Windows x64 ZIP을 만들고, `latest.json`에 서명한 뒤 GitHub Release **초안**을 만든다.
- 초안 내용과 파일을 확인한 뒤 '게시'를 누르면 그때부터 설치된 앱이 업데이트를 받는다(`releases/latest/download/latest.json`). 태그 push와 게시는 사용자 승인 대상이다.
- 앱이 다운로드 폴더에서 바로 실행되는 macOS 임시 격리(App Translocation) 상태나 앱 폴더에 쓸 수 없는 위치에서는 업데이트를 받지 않는다. 사용자는 앱을 응용 프로그램 폴더로 옮겨 실행해야 한다.

## 2. 깨끗한 기기 검증

가능하면 기존 Claude Code·Codex 로그인과 개발용 설정이 없는 새 OS 사용자 또는 초기화한 가상 머신에서 진행한다.

### 공통 흐름

- [ ] 앱 첫 실행과 한국어·영어 전환
- [ ] 설정 › 에이전트 설치·연결에서 Codex와 Claude Code를 각각 설치·연결(로그인)
- [ ] 프로젝트 추가 → 역할 모델 선택 → 첫 작업 보내기 → 팀 대화와 결과 확인
- [ ] 변경 비교 확인 후 원본 반영과 되돌리기
- [ ] 앱을 종료하고 다시 열었을 때 프로젝트·작업·역할 설정이 그대로 남음
- [ ] 알림(작업 완료·확인 필요·작업 중단)과 앱이 뒤에 있을 때 macOS 배너
- [ ] 앱 아이콘이 앱 목록, Dock 또는 시작 메뉴의 작은 크기에서도 식별 가능

### macOS

- [ ] 패키지로 설치하고 실행. 미서명 베타라면 Gatekeeper 경고 후 사용자가 허용해 실행 가능
- [ ] Apple Silicon에서 전체 흐름 완료
- [ ] Intel 지원이 필요하면 별도 Intel 또는 universal 패키지에서 같은 흐름 완료

### Windows

- [ ] 패키지로 설치하고 실행. 미서명 베타라면 SmartScreen 경고 후 사용자가 명시적으로 실행 가능
- [ ] 한글과 공백이 포함된 사용자명·프로젝트 경로에서 화면·sidecar 흐름 확인
- [ ] 실제 에이전트 실행은 자손 프로세스 안전 구현 전까지 비활성 상태로 남아 있음

## 3. 실패 경로

각 항목은 원시 오류만 노출하지 않고 쉬운 원인 설명과 다음 행동(재시도, 다른 폴더 선택 등)을 제공해야 한다.

- [ ] 네트워크를 끊고 설치·연결·작업 실행 시도
- [ ] 프로젝트 폴더를 지우거나 옮긴 뒤 다시 열기
- [ ] 쓰기 권한이 없는 폴더를 프로젝트로 추가
- [ ] 여유 공간이 부족한 환경에서 작업 공간 준비
- [ ] 실행 중인 작업이 있는 상태에서 작업·프로젝트 지우기, 앱 종료
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

## 6. 추가 게이트

macOS 패키징 전에는 실행 중인 해당 `out/elsecrew-<platform>-<arch>` 앱을 정상 종료한다. Forge `prePackage`는 해당 경로의 앱/sidecar/helper 실행 파일을 OS 목록에서 발견하거나 목록 확인에 실패하면 교체를 거절한다. 자동 종료는 하지 않으며, 검사 이후 새로 실행되는 경쟁까지 막는 OS 잠금은 아니므로 빌드 동안 그 앱을 다시 열지 않는다.

현재 상태는 `history.md`·`architecture.md`, 수치는 `proposals/m5-electron-spike.md` §9–13과 `procedures/m5-performance.md`를 따른다. 아래는 남은 증거 요구이며 완료 표시가 아니다.

원격 검증 준비: `.github/workflows/m5-package-diagnostic.yml`은 수동 macOS/Windows 진단과 `codex/m5-windows-diagnostic-20261102` 전용 브랜치 push의 Windows 진단을 제공한다. 일반 브랜치/main push는 이 workflow를 실행하지 않는다. 기본 브랜치에 파일이 없는 초기 상태에서는 사용자 승인 후 전용 브랜치에 게시해 Windows 검증을 시작할 수 있다. 현재 저장소는 공개이므로 게시하는 M5 코드/문서와 진단 artifact도 공개 범위임을 승인 전에 알린다. 미서명 진단 ZIP은3일만 보관하며 release 게시·실제 계정/모델 호출은 하지 않는다. push/원격 실행은 사용자 승인 뒤 수행한다. 이 작업의 성공만으로 Windows UI/자손 안전·서명·G6/출시 합격을 주장하지 않는다.

- [ ] 최신 후보의 여러 프로젝트·영속 채팅·역할 설정·코드 편집/diff·반영/복원·재시작을 패키징 앱에서 확인하고 전체 시안 비교 결과를 기록
- [ ] macOS 실제 에이전트 입력·출력·명시 중단·자손 소멸 확인; Codex planning 1회 성공을 쓰기·승인·복구 전체 합격으로 확대하지 않음
- [ ] Claude 앱 전용 로그인과 실제 작업 경로 확인; 계정/유료 모델 시험은 별도 승인 범위만 실행
- [ ] G6 시작·대형 파일/diff·입력·스트림·전체 프로세스 메모리·패키지 크기 측정; 실제 collector를 연결한 `m5:test:performance`와 최종 하네스 증거 확보 (합성 진단 JSON만으로 통과시키지 않음)
단일 실패 재검증용 `.github/workflows/m5-windows-fixture-followup.yml`은 `codex/m5-windows-fixture-followup-20261102` 전용 push에서 원장 snapshot fixture 1개만 검사한다(20분 상한, 실제 모델·패키징·artifact 업로드 없음). 전체 회귀/Windows 사용 가능/출시 합격 증거가 아니며, 기존 통과 기록과 구분한다. 이 별도 공개 브랜치의 push·CI도 사용자 승인 뒤에만 실행한다.

- [ ] Windows 패키지의 UI·sidecar·fixture 확인 및 실제 agent 실행 차단 유지; 자손 안전 구현 전 실행 활성화 금지
- [ ] 데이터 손실·교차 채팅 오염·원본 보호·미확인 종료/복구의 최종 실패 주입과 독립 검토
- [ ] 깨끗한 기기·서명/공증·배포 방식 결정과 최종 설치본 확인; main 병합·push·외부 배포는 별도 사용자 승인

수동 workflow_dispatch의 기본 브랜치 조건: [GitHub 공식 문서](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch). 전용 브랜치 진단은 main 병합이나 출시 승인이 아니다.

### M5 Windows 설치된 Git 입장·입출력 진단

제품 활성화와 별도로, Windows에서 `ELSECREW_WINDOWS_TEST_GIT`에 시험할 설치본의 `git.exe` 절대 경로를 명시하고 아래 opt-in 검사를 실행한다. 사용자 저장소·앱의 기존 선택/동의 기록은 사용하지 않는다.

```powershell
$env:CARGO_BUILD_JOBS = '4'
$env:RUST_TEST_THREADS = '4'
cargo test --locked --release -p elsecrew-sidecar --lib selected_git_trust_runs_bounded_version_and_stdin_then_revokes -- --ignored --nocapture --test-threads=1
```

자체 임시 앱 데이터에서 선택 → 서명자/설치 폴더 확인 → 미동의 실행 거절 → 명시 동의 → 매 실행 재검사 → `--version` 및 `hash-object --stdin` → 동의 철회 후 실행 거절을 검사한다. 실제 실행은 제품의 실행 파일/의존 DLL 관측·고정 환경·보호된 작업 폴더·프로세스 종료 경로를 사용한다. 내용 해시 명령은 객체를 저장하지 않으며 네트워크·모델 호출도 하지 않는다. 성공 시 임시 자료를 정리한다. 이 검사 후보는 아직 Windows에서 실행하지 않았으며 프로젝트 전체 반영/복원·패키지 UI·출시 합격 증거가 아니다. 검사가 동의를 만들어 사용하는 대상은 임시 데이터뿐이며 일반 앱의 실행 gate는 그대로 비활성이다.

동일한 설치본 환경에서 `confirmed_multi_item_apply_and_restore_through_scheduler_and_ledger -- --ignored --nocapture --test-threads=1` 필터를 사용하면 전체 반영·복원 진단 후보를 실행한다. 앱이 임시 원본을 Git으로 준비한 뒤 실제 프로젝트/snapshot·review·확인 토큰·백업·scheduler·SQLite 원장을 거쳐 파일 수정/추가/삭제와 새 중첩 폴더를 반영하고 역순 복원한다. 오래된 복원 기록 거절과 SQLite 재열기도 검사한다. 테스트 바이너리에서 해당 manager에만 Git 실행을 허용하며 설치본 동의/서명/의존성 검사와 일반 앱의 공개 쓰기 gate는 우회하지 않는다. 이 후보도 Windows 실행 전이며 중간 syscall 강제 중단·전원장애·패키지 UI 시험은 별도로 남는다.
