# 작업 이력 (Hello, Agent)

### 2026-09-14 · by GPT-6 Astra (Codex 실제 로그인 통과)

- **실사용 확인**: clean `8eb6129`에서 사용자가 브라우저 인증 완료를 알렸다. 실행 중인 Electron 앱의 `이 앱에 Codex 로그인이 연결됐어요`·`연결됨` 표시와 프로젝트 선택 활성 상태를 직접 확인했다. 앱 전용 `codex-login.pending` 부재도 확인했다. 인증 파일 내용·토큰·계정 정보는 읽지 않았고 모델 호출은 하지 않았다.
- **판정**: macOS Codex의 앱 전용 로그인 버튼→브라우저 인증→연결 완료 경로는 이번 사용자 실사용을 통과했다. UI의 connected는 현행 구현상 fresh account/read 및 process cleanup 뒤에만 발행한다. 이전 엔트리의 실제 인증 대기는 이 확인으로 해소되며 기존 기록은 보존한다.
- **다음/경계**: 재시작 후 연결 상태 복원, Claude 로그인, 실제 모델 작업/출력은 별도 남는다. 실행 중인 앱과 인증을 보존했으며 재시작·재로그인·유료 실행 시험은 하지 않았다. 문서 diff 검사 후 실사용 결과만 커밋한다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (제품 Codex 로그인 연결)

- **구현**: 내부 helper에서 멈추지 않고 macOS pinned Codex 0.153.4 launcher → 앱 전용 HOME/CODEX_HOME → login session → 고정 provider.login IPC → main의 공식 HTTPS 브라우저 열기 → 작업대 로그인/대기/취소/오류 UI를 연결했다. 개발 실행도 기존 더미 대신 실제 debug sidecar를 사용한다. 실제 모델 작업·Claude 로그인·Windows는 이 기능의 완료 주장이 아니다.
- **안전/검토 수정**: 초기 send에도 취소를 전달하고 준비/실행 실패 때 소유 session을 유지한 bounded cleanup 재시도를 수행한다. 취소 요청은 URL을 제거하고 main도 브라우저를 열지 않는다. worker panic·미확인 종료는 provider-login:codex quit blocker로 남는다. 후속 OS 종료 관측만으로 최초 불확실 fence를 자동 해제하지 않으며 UI에 안전 종료/재로그인 차단을 명시한다. 수동 복구 UI는 후속이다.
- **실제 관측**: 설치된 Codex 및 host 쌍을 strict codesign으로 검증했다. 별도 임시 profile에서 실제 OAuth URL 생성 → 브라우저 없이 즉시 취소 → process 종료/예약 해제 PASS(2.54초). sandbox의 localhost callback 제한은 모델 없는 동일 검사의 허용 실행으로 대조했다. 기존 인증 읽기/복사·유료 모델 호출 없음. sha2만 dev 최적화하여 약 283MB 실행 파일 pin 재검사의 개발 지연을 줄였다.
- **검증**: 전체 protocol 52 PASS, sidecar 377 PASS/기대값 1 FAIL/2 ignored(663.68초)에서 새 capability 기대값을 고친 뒤 해당 검사 PASS. 이후 변경 영역 provider 41 PASS/1 ignored, core login 8 PASS, 실제 설치 CLI의 임시 OAuth URL→취소 검사 1 PASS(2.54초). 전체 Node 335 PASS, 추가 bridge 협상 포함 45 PASS, main 보안/시작 경합 6 PASS, Chromium 준비 중→완료→로그인→취소/Monaco 2 PASS. TypeScript·renderer build·변경 crate rustfmt·diff PASS. sidecar strict clippy `--no-deps` PASS; dependency 포함 strict 검사는 변경하지 않은 core proctree의 기존 2개 lint(unnecessary_lazy_evaluations/map_entry) 때문에 실패했다. 이를 전체 strict PASS로 기록하지 않는다.
- **실제 앱 확인**: 초기 비활성화를 재현했다. loadURL 전 상태 요청 경합을 방지하고, 준비 중 polling의 browser setTimeout/clearTimeout을 메서드로 직접 호출해 발생한 Illegal invocation을 wrapper로 수정했다. bridge 호출도 receiver를 유지한다. 즉시 ready만 반환하던 Chromium fixture에 verifying→ready 전환을 추가했고 새 프로세스로 앱을 다시 열어 프로젝트 선택·Codex 로그인 버튼 활성화를 확인했다. 임시 진단 로그는 제거했다. 독립 읽기 전용 검토에서 cleanup 경로의 차단 결함 해소를 확인했다(검토자는 테스트 미실행).
- **실사용 경계**: browser 인증 완료 후 fresh account/read와 cleanup 성공을 확인하는 흐름은 구현했지만 실제 사용자 계정 로그인 완료는 사용자가 직접 수행해야 한다. 재시작 후 로그인 상태 복원은 아직 없다. 캐시 삭제·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (실행 파일 pin 재검증 연결)

- **직접 구현**: clean `18bc985`에서 내부 PinnedExecutable과 gated process 연결을 구현했다. backend가 별도로 제공하는 SHA-256과 no-follow 경로/파일 handle을 대조하며 regular·root/현재 UID·단일 링크·실행 권한·setuid/setgid 및 group/other write 금지·512MiB 상한을 요구한다. 64KiB streaming hash 전후 길이/mtime/ctime을 비교하고 보존 inode와 새 경로 신원을 재검증한다. gate 생성 전과 release 직전에 다시 검사한다. expected digest를 후보 파일로부터 자동 신뢰하는 경로는 없다.
- **검증**: pin 변경/동일 경로 교체·링크/권한 거절 2/2 PASS. provider 관련 33검사에서 32 PASS, 새 gate 테스트가 failed launch의 기존 fence 보존을 잘못 기대해 assertion만 수정했다. 제품 복구 정책은 그대로 유지한다. strict sidecar clippy PASS. 기존 승인된 읽기 전용 검토자에게 독립 검토를 요청했다. Karpathy 지침으로 기존 no-follow 디렉터리 검사를 재사용했고 OpenAI Docs [환경 변수](https://learn.chatgpt.com/docs/config-file/environment-variables)를 확인했다.
- **경계·다음**: point-in-time 내용/신원 일치이며 서명/버전/helper closure admission이나 hostile same-UID 후속 변경 방지·fd-exec 보장이 아니다. 실제 Codex를 실행하거나 사용자 인증을 읽지 않았다. 전용 HOME/CODEX_HOME/runtime artifact 정책과 실제 로그인 IPC/UI는 여전히 남는다. 캐시 삭제·main 병합·push 없음. 집중 재검증·독립 검토 마감 후 이 단위를 커밋한다.
- **마감**: 수정한 pin gate 집중 1/1 PASS. 독립 검토의 추가 코드 결함 없음과 비차단 회귀 권고를 확인했다. gate spawn 후 파일을 변경하는 결정적 callback 테스트를 보완해 release 무호출·Drop 종료·fence 보존 1/1 PASS다. 기존 32개 성공 증거와 strict clippy/포맷 증거는 영향 범위를 구분해 재사용하며 전체 제품 gate로 주장하지 않는다.

### 2026-09-14 · by GPT-6 Astra (대기형 로그인 런처 연결)

- **직접 구현**: `5a3e39b`에서 이전 현황 답변의 미커밋 작업 기록을 보존하고 gate/process/lib만 변경했다. Karpathy 지침에 따라 기존 launch_with_barrier·Child·TreeTracker를 재사용했다. 내부 gate는 고정 `/bin/sh` read→정확한 허용 토큰→exec 순서로 vendor 이전에 대기하며, 절대 경로 program/cwd·별도 positional args·빈 상속 환경·stdio pipe만 사용한다. 프로세스 소유자의 durable 기록 뒤 단일 nonblocking write로 허용한다.
- **검증**: 로그인 집중 20/20·strict sidecar clippy·rustfmt·diff PASS. 실제 자체 생성 cat 송수신/추적 종료, printf 특수문자 인수 무해석, touch 대상의 EOF/잘못된 토큰 무실행, 상대 실행 경로 거절을 검사했다. 사용자 프로젝트/인증·Codex·유료 모델은 실행하지 않았다. 기존 승인된 읽기 전용 검토자가 좁은 안전 경계를 검토 중이다.
- **남은 경계**: 이 gate는 검증된 backend 대상에 사용할 내부 실행 제어이며 실제 Codex 실행 파일 admission·전용 HOME/CODEX_HOME/runtime artifact 정책·제품 로그인 IPC/UI는 여전히 미연결이다. 빈 환경 자체를 완전한 vendor 설정 격리로 주장하지 않는다. 캐시 삭제·main 병합·push 없음. 독립 검토 마감 후 현재 단위를 커밋한다.
- **마감**: 독립 검토에서 인자 처리·release·cleanup 연결의 추가 차단 결함 없음을 확인했다. 검토자는 테스트를 재실행하지 않았으며 위 메인 실행 증거를 구분해 보존한다. 이전 현황 기록을 포함해 이 단위를 커밋한다.

### 2026-09-14 · by GPT-6 Astra (잔여 작업 현황 정리)

- **확인**: clean `5a3e39b`와 현행 architecture·최신 기록을 대조했다. 최근 로그인 관련 커밋은 내부 준비/통신/종료/실행 허용 연결이며 실제 Codex·Claude 제품 실행 완료가 아니다.
- **남은 순서**: 실제 실행기·전용 환경/로그인/복구 화면 → 실제 작업 실행·출력/취소/재시작 연결 → 역할별 Codex/Claude 인계 → diff 검토 후 원본 적용/폐기/복구 → 실제 프로젝트 기반 guided start와 남은 시안/상하 도킹 → G6 통합 보안/성능/패키징 및 Windows·출시 후보 실기기 검증이다. 기존 프로젝트/채팅 저장·편집/diff·가로 패널 배치·fixture 검증을 미구현으로 되돌리지 않는다. 각 범주는 크기가 달라 완료율을 단순 계산하지 않는다.
- **경계**: 현황 답변만 수행했으며 테스트 재실행·제품 변경·캐시 삭제·main 병합·push 없음. 다음 개발 우선순위는 실제 provider의 사용자 사용 가능 경로를 닫는 것이다.

### 2026-09-14 · by GPT-6 Astra (로그인 실행 허용의 두 단계 연결)

- **직접 구현**: clean `e46bddc`의 메인 checkout에서 login process/session을 변경했다. Karpathy 지침에 따라 기존 Child·TreeTracker·durable reservation을 재사용했다. `launch_with_barrier(spawn, release)`는 대기 child를 소유하고 신원 기록·관찰·profile/marker 재검사·pipe 존재 확인 후 release를 한 번 호출한다. session도 이 API로 연결하고 기존 무조건 launch/start shortcut은 테스트 전용으로 제한했다.
- **실패 경계**: 준비 실패는 release를 건너뛰며 release 오류/이후 I/O 오류에도 Child를 owner에 남겨 기존 cleanup을 수행한다. 중복 launch는 거절한다. callback은 신뢰하는 backend가 제공해야 하며 bounded 실행·실제 executable 검증·환경 격리·vendor가 release 전 실행되지 않는 보장은 여전히 launcher의 책임이다. 이번 API만으로 그 보장을 증명하거나 실제 provider를 활성화하지 않는다.
- **검증·검토**: 자체 생성 sh read→exec cat peer로 release 시점에 PID/시작시각이 이미 durable marker에 있음을 확인하고 이후 프로토콜 송수신·정상 종료를 검사했다. pipe 누락 시 무허용·release 실패 뒤 보존/정리·재허용 차단을 추가했다. 로그인 관련 집중 18/18·strict sidecar clippy·rustfmt·diff PASS. 기존 승인된 읽기 전용 검토자는 추가 actionable 없음으로 확인했다(검토자는 테스트 미실행). 실제 로그인·사용자 인증·유료 호출은 하지 않았다.
- **다음**: 검증한 이 단위를 커밋한다. 실제 gated launcher와 실행 파일/환경 정책·로그인/복구 IPC/UI가 남으며 전체 제품 완료가 아니다. 사용자는 캐시 위치 확인 후 개발 재개를 요청했으므로 빌드 캐시는 삭제하지 않았다. main 미병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (테스트 설치본·저장공간 점검)

- **현재 상태**: clean `0df309c`·`m5/coding-agent-ide`에서 로그인 실행 연결의 미구현 경계를 재확인하며 사용자가 추가 요청한 저장공간 점검을 우선 수행했다. 이번에는 제품 코드를 변경하지 않았다. Karpathy 지침에 따라 임의 launcher나 인증 설정 우회는 추가하지 않았다.
- **설치본 확인**: `/Applications`·`~/Applications`의 깊이 3 이내 이름 검색 및 Spotlight에서 별도 Hello Agent 설치본은 발견되지 않았다(검색 범위 밖/미색인 위치까지 부재를 보장하지 않음). 발견된 앱은 저장소 `out/hello-agent-darwin-arm64/hello-agent.app` 약 285MiB다. `out` 전체는 430736KiB(약 421MiB)이며 배포 ZIP 등이 포함된다. 프로세스 목록 읽기를 허용된 방식으로 재시도한 시점에는 hello-agent/cargo/rustc 실행 항목이 없었다. 다른 제품 astxAgent는 이 앱과 무관하므로 정리 대상이 아니다.
- **주요 용량**: root `target` 15681036KiB(약 15.0GiB), `src-tauri/target` 5080492KiB(약 4.8GiB), node_modules 약 536MiB, `.vite` 약 2.9MiB다. Git worktree는 현재 checkout 하나뿐이고 `.worktrees` 약 8KiB다. 설치본보다 빌드 캐시가 주원인이다. 두 debug/incremental만 합쳐 6162728KiB(약 5.9GiB)이며 삭제 후 다시 생성 가능하지만 다음 빌드 비용이 생긴다.
- **보존·다음**: 이번 요청은 확인이므로 파일을 삭제하지 않았다. 소스·최신 테스트 앱·사용자 데이터를 보존하고 위 두 incremental 디렉터리만 정리할지 비동기 질문했다. 실제 launcher/barrier·환경/runtime artifact 정책·로그인/복구 IPC/UI는 여전히 남으며 이번 점검을 구현 완료로 세지 않는다. 문서 diff 검사 후 기록만 커밋한다. main 미병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (설치 CLI 변경과 로그인 스키마 대조)

- **직접 작업**: clean `d4ee925`·`m5/coding-agent-ide`에서 실제 launcher 준비 중 설치 CLI가 0.149.1→0.153.4로 변경됐음을 확인했다. 실행 위치는 `/Applications/ChatGPT.app/Contents/Resources/codex`이며 Codex.app 후보는 현재 없다. 기존 exact-version 허용 정책이나 제품 capability는 변경하지 않았다.
- **관측 범위**: env를 비운 버전·app-server 도움말·generate-ts만 실행했다. 사용자 인증 조회/복사·로그인·모델 호출은 하지 않았다. CLI SHA-256 `87a08119b8effa519f0ecb552dc98043f58a8200bf2ec5da60f76890c33e9c3a`, sibling code-mode-host `038b9c6b60baacbfacba1fe81cf603c22b73697810e013d4efda040f1a7294e7`. codesign 표시의 TeamIdentifier는 2DC432GLL2였으나 표시 명령은 서명 검증/실행 승인이 아니다. PATH alias 생성 권한 경고는 기존 환경을 수정해 우회하지 않았다.
- **검사 구현·결과**: `scripts/m5-codex-login-schema.mjs`는 두 생성 schema 폴더의 로그인 루트 10개와 재귀 type import를 SHA-256 비교한다. 동일/검토 필요/읽기 실패를 종료 코드 0/2/1로 구분하며 vendor 실행이나 호환 버전 등록은 하지 않는다. 보존 0.149.1과 새 0.153.4에서 20개 중 18개 동일, AuthMode의 bedrockAccessKeys 및 LoginAccountParams의 amazonBedrockAccessKeys 추가 2개로 REVIEW_REQUIRED(예상 종료 2)다. 현재 사용하는 chatgpt 요청 분기와 완료/계정 응답은 동일하지만 전체 runtime 호환 증거가 아니다.
- **검증·다음**: 도구 테스트 3/3 PASS(동일·의존 타입 변경/순환·누락/경로 이탈), diff 검사 PASS. OpenAI 공식 [app-server](https://learn.chatgpt.com/docs/app-server)·[환경 변수](https://learn.chatgpt.com/docs/config-file/environment-variables)를 재확인했다. 실제 launcher/barrier·환경/runtime artifact 정책·로그인/복구 IPC/UI는 이번 작업에서 구현하지 않았다. 다음은 이 실행 연결이며 기존 인증/유료 실행 승인을 추정하지 않는다. main 미병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (초기화부터 로그인 재확인까지 연결)

- **직접 구현**: clean `b1535de`에서 core login connection·sidecar login session과 기존 process/I/O 연결을 소유했다. Karpathy 지침에 따라 ModelFreeExchange 초기화와 LoginExchange·TreeTracker를 재사용했다. OpenAI Docs의 [초기화](https://learn.chatgpt.com/docs/app-server#initialization)·[인증](https://learn.chatgpt.com/docs/app-server#auth-endpoints)과 보존 0.149.1 생성 schema를 대조했다. 새 연결은 전용 home/macOS 응답을 비교하고 initialize→initialized→명시적 로그인→브라우저 완료→fresh account/read를 고정 ID·단일 사용 요청으로 보낸다.
- **관측·종료 경계**: 계정 갱신 알림은 bounded 검증만 하고 성공 근거로 쓰지 않는다. 마지막 계정 응답이 chatgpt이고 process.finish가 성공한 뒤에만 CredentialsObserved를 반환한다. 이는 모델 가용성/설정 격리 증명이 아니다. 취소·잘못된 ID/계정 종류·알림 폭주·프로세스 정리 실패는 성공으로 접지 않는다. raw account/OAuth/오류 내용은 저장하지 않는다.
- **대기 방식**: nonblocking poll은 데이터가 아직 없음을 None으로 구분하며 EOF/잘린 입력은 계속 실패다. 기존 줄/누적 예산과 추적 callback을 유지한다. 내부 기본 대기 한도는 RPC 30초·브라우저 5분이며 caller는 browser 대기 중에도 100ms 이내 간격으로 advance해야 한다. 취소는 즉시 로컬 성공 자격을 철회하고 프로세스 종료를 확인한다.
- **검증 진행·다음**: core 로그인 8/8·sidecar 로그인 14/14 PASS 후 cleanup 실패/브라우저 취소 회귀를 보완했다. 실제 pipe의 고정 sh peer로 요청 순서·업데이트 알림 교차·계정 재확인·종료 뒤 성공을 검사하며 실제 Codex/사용자 인증/브라우저/유료 모델은 실행하지 않았다. 독립 검토·정적 검사를 마무리 중이다. 검증된 실제 launcher/barrier/환경 정책·로그인/복구 IPC·UI와 Claude 연결은 남고 제품 미활성·main 미병합·push 없음이다.
- **검토 보완·검증**: 외부 세션 closed와 내부 cleanup_required를 분리했다. 첫 종료 미확인 뒤 session을 보관해도 다음 advance/cancel 또는 명시 retry_cleanup이 정리를 재시도하고, 처음 불확실했던 fence는 해제하지 않는다. toy peer는 EOF 후 같은 PID의 sleep으로 생존하게 해 root Alive→후속 tick Gone 검사의 타이밍 의존성을 제거했다. 최신 session 5/5·strict sidecar clippy·scoped rustfmt·diff PASS, opaque 요청 doctest 2/2 PASS다. core clippy는 기존 proctree 3경고 외 신규 없이 종료 0이며, core 8/8와 영향 없는 I/O/owner 11/11 증거를 재사용한다. 전체 제품 누적 gate와 실제 인증 완료 증거는 아니다.
- **마감**: 독립 재검토에서 cleanup 재시도 P1 해소와 추가 차단 결함 없음을 확인해 이 연결 단위를 커밋한다. 실제 launcher·환경 격리 검증과 제품 IPC/UI 활성화는 여전히 다음 작업이며 이번 toy peer 검증으로 완료 처리하지 않는다.

### 2026-09-14 · by GPT-6 Astra (로그인 수명·재시작 잠금 연결)

- **직접 구현**: clean `3d5ee64`에서 sidecar의 로그인 process owner·기존 I/O observer hook·profile capability와 현행 문서를 소유했다. Karpathy 지침에 따라 기존 TreeTracker와 bounded I/O를 재사용했다. app-data의 고정 `codex-login.pending`을 create-new 0600·파일/디렉터리 sync한 뒤 backend launcher callback을 호출하고, 반환된 Child를 즉시 소유해 PID+시작 시각·자손 신원을 추적/저장한다. OAuth URL·계정·명령줄은 기록하지 않는다.
- **종료·재시작**: 송수신 대기 중에도 추적하며 browser 대기는 caller의 100ms tick을 요구한다. 명시적 finish는 누적 tree 종료 확인·root try_wait·증거 저장과 profile/marker 신원 확인 뒤에만 자기 잠금을 제거한다. 불확실한 종료 뒤 blocking wait는 하지 않는다. Drop/불완전 기록/추적·증거 실패는 잠금을 보존하며 새 owner는 기존 파일을 읽거나 수정하거나 저장 PID에 자동 신호를 보내지 않고 거부한다.
- **검증 진행**: 최초 실패 주입 테스트가 probe보다 list가 먼저라고 잘못 가정한 것을 수정했다. 최신 owner 집중 7/7·I/O 3/3 PASS이며 자체 생성 cat/sh/sleep만 사용했다. 부모 선행 종료 후 추적 자손 회수·timeout/cleanup 구분·중복 launch·Drop/재시작·마커 교체 무변경·증거 실패 sticky를 포함한다. clippy의 test-module 위치 지적을 수정했고 정적 검사·독립 검토를 마무리 중이다.
- **독립 검토 수정**: 미확인 cleanup에도 finished를 고정해 재시도/Drop 정리를 건너뛰던 결함을 수정했다. 첫 종료 실패 후 실제 root가 살아 있음을 확인하고 명시적 재시도 또는 Drop이 종료를 다시 시도하되 fence를 보존하는 회귀를 추가했다. marker 대조→이름 기반 삭제는 원자적 unlink-if-inode가 아니므로 기존 profile과 동일한 point-in-time 경계로 주석/현행 문서를 명확히 했다. hostile same-UID 후속 교체 보호를 새 보장으로 주장하지 않는다.
- **마감 검증**: 수정 후 owner 8/8·strict sidecar clippy·scoped rustfmt·diff 검사 PASS다. 변경 영향이 없는 I/O 3/3 증거는 재사용했다. 독립 재검토에서 cleanup 지적 해소·추가 actionable finding 없음으로 확인해 이 단위를 커밋한다. 전체 sidecar/패키지/제품 누적 gate는 재실행하지 않았다.
- **경계·다음**: 이 내부 owner의 backend callback은 검증된 executable·별도 환경/cwd·실행 전 barrier를 별도로 제공해야 한다. 현재 제품에는 실제 vendor launcher·Codex 초기화/로그인/계정 재확인·취소 UI·미확인 로그인 복구 UI가 없으며 이 테스트를 실제 인증이나 제품 활성화 증거로 쓰지 않는다. OpenAI Docs [initialization/auth](https://learn.chatgpt.com/docs/app-server#initialization)를 재확인했으며 초기화 교환이 선행해야 한다. 실제 사용자 인증·유료 모델 호출·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (로그인 프로세스의 bounded stdio)

- **직접 구현**: clean `14aed10`에서 `provider_login_io.rs`와 lib module export를 소유했다. Karpathy 지침에 따라 기존 BoundedObservation을 재사용했다. caller가 Child와 종료 tracker를 유지한 채 pipe만 nonblocking으로 연결한다. 단일 줄·누적 송수신량·대기시간·취소를 제한하며 부분 쓰기/EOF/잘린 줄/오류 뒤 재사용하지 않는다. 원문 버퍼는 Debug로 노출하지 않는다.
- **실제 검사**: 자체 생성 cat/sh/exec-sleep 테스트 프로세스로 두 응답의 왕복·순서를 기존 auth 파서로 확인했다. 시간 초과·취소·부분 EOF·빈 줄·큰 줄·읽지 않는 stdin backpressure를 검사하고 test guard가 자신이 만든 단일 프로세스를 kill/wait한다. 실제 사용자 프로세스·인증·모델은 사용하지 않았다. 이 검사는 제품 자손 회수 gate나 로그인 완료가 아니다.
- **검토 수정**: 임의 raw frame을 받을 수 있다는 독립 검토 지적에 따라 core의 private-field `LoginRequest`만 송신하도록 제한했다. 생성자는 기존 로그인 시작/취소 exchange에만 있고 임의 thread/start 생성은 compile-fail 회귀로 검사한다. 읽지 않는 stdin에는 typed 요청을 반복해 실제 포화 뒤 timeout/poison을 검사한다.
- **최종 검증**: 같은 요청 객체의 재전송 지적도 non-Clone 값 소비 방식으로 수정했다. 고유 ID 요청을 사용하는 통신 집중 3/3, 로그인 상태 5/5, 임의 생성/동일 객체 재사용 compile-fail 2/2, strict sidecar clippy·scoped rustfmt·diff 검사 PASS. 독립 재검토에서 두 지적 해소·추가 차단 결함 없음으로 확인해 이 단위를 커밋한다. 전체 sidecar/제품 누적 게이트는 이번 작은 변경에서 재실행하지 않았다. 요청 ID의 연결 내 유일성은 owner가 보장해야 한다.
- **경계·다음**: 이번에는 I/O까지 구현했다. 처음 알린 프로세스 owner 전체는 완료하지 않았으며 기존 TreeTracker를 사용하는 준비/종료·불확실 cleanup 영속 기록·전용 환경 spawn·초기 handshake/계정 재확인·로그인 UI 연결이 남는다. 실제 vendor 응답과 연결한 검사는 아니며 main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (로그인 profile 경로·handle 결속)

- **직접 구현**: clean `cb1b12d`에서 sidecar profile 준비 모듈을 소유했다. Karpathy 지침에 따라 기존 설정/권한 검사를 재사용하고 생성과 읽기 전용 재검사를 분리했다. startup DB 경로의 고정 파일명·절대 경로를 요구하고 각 디렉터리를 no-follow로 열어 app-data/profile handle의 dev/ino를 보존한다. 재검사는 현재 경로의 identity와 기존 profile 정책을 대조하고, 삭제된 설정이나 폴더를 다시 만들지 않는다.
- **검증**: profile focused 6/6 PASS. 부모/profile 교체·상위 symlink·잘못된 DB 파일명·삭제 config 무복원 거부를 포함한다. 실제 사용자 설정/인증은 접근하지 않고 임시 폴더만 검사했다. 독립 검토 요청 중이다.
- **검토 보완·마감**: 임의 basename 경로만으로 profile 생성을 허용하는 지적을 수정해 `from_storage(&Storage)`로 한정했다. renderer 경로 문자열 대신 이미 열린 sidecar 저장소 객체가 필요하다. 실제 Storage를 열어 수행한 최신 focused 6/6·strict sidecar clippy·rustfmt·diff PASS이며 기존 무생성 재검사·identity 비교는 유지한다. 독립 재검토에서 helper 경계의 지적 해소·추가 지적 없음을 확인해 이 단위를 커밋한다.
- **경계·다음**: owner는 자신의 startup Storage를 전달해야 하며, 반환 경로는 같은 UID의 후속 변경까지 막는 실행 권위가 아니다. 실제 로그인 프로세스 owner·취소/종료·고정 IPC/UI 연결과 Claude는 아직 남는다. 처음 알린 실제 연결 작업의 선행 경계를 보완한 것이며 로그인 UI 완료로 보고하지 않는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (Codex 로그인 요청·완료 결속)

- **직접 구현**: clean `220245a`에서 core vendor login exchange와 현행 문서를 소유했다. OpenAI Docs의 공식 app-server Auth endpoints와 생성 schema를 확인했다. Karpathy 지침에 따라 기존 RawValue/BoundedObservation을 재사용하고 browser 로그인 시작·취소 요청만 표현한다. 로그인 URL은 HTTPS 공식 origin으로 한정하고 Debug/Serialize가 없는 transient 값으로만 반환한다.
- **상태 경계**: request ID·login ID·중복 필드를 대조하며 완료 성공은 사용 가능이 아니라 fresh account/read가 필요한 상태다. 취소 후 늦은 성공·다른 시도 응답·시작 전 완료는 거부한다. EOF/timeout 시 owner가 invalidate해야 하며 실제 login transport/프로세스 종료·브라우저 열기는 아직 미연결이다. 실제 인증·유료 모델·사용자 프로젝트 실행은 하지 않았다.
- **검증**: provider 회귀 40/40과 로그인 거절/모순 성공 보완 후 focused 4/4 PASS. core clippy 종료 0(기존 proctree 3개 경고 외 신규 없음), rustfmt·diff PASS. 독립 검토를 요청했다.
- **검토 수정**: 실제 생성 schema와 비교한 독립 검토에서 emittedAtMs·필수 onboardingEntrypoint 누락을 발견했다. 이전 합성 성공 예제의 PASS는 현 wire 호환 증거가 아니었다. bounded timestamp를 허용하고 추가 온보딩은 null일 때만 진행하도록 보완했다. 같은 timestamp 누락을 기존 공개 이벤트 envelope에서도 수정했다. 전체 필드 성공·누락/중복/잘못된 타입 회귀를 포함한 최신 vendor observation 29/29 PASS다.
- **마감·다음**: 독립 재검토에서 두 스키마 결함의 해소·추가 지적 없음을 확인했다. 최신 로그인 5/5·정적/포맷/diff 검사도 통과해 이 단위를 커밋한다. 앱 데이터 path/handle 결속, 로그인 통신 owner/UI, Claude 분리 검증·실제 실행 연결은 남는다. 새 로그인 방식 승인을 다시 요구하지 않는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (전용 로그인 승인·Codex profile 준비)

- **사용자 결정**: 전용 환경에서 별도 로그인하는 구현을 승인받아 앞선 결정 대기가 해소됐다. 기존 터미널 설정·인증정보 복사나 유료 모델 실행은 포함하지 않는다. 이전 미커밋 대기 기록은 삭제하지 않고 보존한다.
- **직접 구현**: `ae5af65`에서 sidecar의 고정 Codex profile 준비 함수를 추가했다. trusted app-data 디렉터리 capability 아래 0700 폴더·0600 config를 create-new로 만들고 링크/소유자/권한/하드링크/정확한 bounded 내용을 검사한다. 기존 설정이 다르면 덮어쓰지 않고 거부하며 인증 파일은 읽거나 복사하지 않는다. 공식 Codex 인증 저장 문서를 OpenAI Docs로 확인했고 Karpathy 지침에 따라 기존 fake 실행/권한 계약은 바꾸지 않았다.
- **검증·경계**: 임시 폴더 기반 fresh/reopen·기존 synthetic 인증 보존·변조 config/넓은 권한/링크 거부 3/3, sidecar strict clippy all-targets/no-deps PASS. 실제 사용자 인증이나 모델을 실행하지 않았다. 준비 helper는 아직 제품 login transport에 연결되지 않았고 Unix 한정이다. 실제 launch 전 경로/handle 신원 대조와 전체 설정 격리는 여전히 필요하다.
- **검토 보완**: 예상 밖 profile 파일이 허용되는 지적을 수정해 로그인 준비 시 config/auth 외 항목을 거부한다. auth는 내용 없이 파일 종류·소유자·권한·링크 수만 확인한다. 추가 AGENTS.md 무변경 거부 포함 최신 focused 4/4 PASS. vendor runtime artifact 허용 정책과 generic Dir의 app-data 출처 강제는 제품 연결 전에 별도로 필요하며 현재 함수 성공을 전체 격리 증명으로 사용하지 않는다.
- **마감·다음**: 독립 재검토에서 지적 닫힘·현재 준비 helper 범위 추가 결함 없음을 확인했다. 수정 후 strict clippy·rustfmt·diff PASS. [Claude 공식 인증 문서](https://code.claude.com/docs/en/authentication#credential-management)는 CLAUDE_CONFIG_DIR별 macOS Keychain 구분과 파일 fallback을 명시한다. 이는 후보 근거이며 설치 버전에서의 실제 분리 검증은 남는다. 로그인 통신/UI·설치 진단·실제 adapter 연결을 이어간다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (자동 목표를 사용자 결정 대기로 전환)

- **세 번째 연속 점검**: HEAD `ae5af65`와 미커밋 작업 기록 외 변화가 없고 로그인 방식에 대한 사용자 답변도 없다. 앞선 턴은 기능 진전이나 살아 있는 작업의 검증 대기가 아니었다. 동일한 의존 결정을 우회하지 않고 자동 목표를 blocked로 전환한다. 개발 완료 판정이 아니다.
- **재개 조건**: 기존 설정을 보존하는 앱 전용 환경에서 별도 로그인하도록 구현할지 사용자가 결정하면 실제 연결을 이어간다. 인증정보 복사·유료 호출·Windows/출시 검증 승인을 함께 받은 것으로 해석하지 않는다. 미커밋은 이 대기 기록을 포함한 `docs/history.md`뿐이며 코드 변경·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (로그인 결정 대기 재확인)

- **두 번째 점검**: HEAD `ae5af65`와 브랜치는 그대로이며 미커밋은 앞선 작업 기록뿐이다. 새 사용자 결정이나 외부 완료 증거가 없어 실제 연결·처음 시작하기의 동일한 결정 대기가 유지된다. 이전 점검은 기능 진전이나 실행 중인 검증 대기가 아니며, 이번에도 반복 테스트·추가 미연결 모듈로 진전을 대신하지 않는다.
- **다음**: 앱 전용 별도 로그인 방식의 구현 허용 여부를 사용자에게 확인한 뒤 진행한다. 제품 완료를 선언하지 않고 목표는 유지한다. 코드 변경·모델 호출·인증 접근·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (실제 연결·처음 시작하기 의존 결정 확인)

- **현재 확인**: clean `ae5af65`, `m5/coding-agent-ide`, Darwin에서 잔여 흐름을 대조했다. 이전 turn은 실제 코드 커밋으로 진전이 있었지만 전체 개발 완료는 아니다. 작업대는 프로젝트 선택·채팅으로 바로 진입하며 Electron preload에 설치/로그인 경로는 없다. 제안서 §5.1은 설치·로그인·연결 확인 뒤 실제 프로젝트를 요구하므로 미연결 안내 화면을 완료로 대체하지 않는다.
- **진행 경계**: §14는 실제 adapter 안전 검증 뒤 원본 적용/복원을 열도록 한다. 전용 환경의 별도 로그인 구현 여부는 아직 사용자 답변이 없으며 자동 목표 재개를 승인으로 해석하지 않는다. 기존 인증 복사·사용자 설정 재사용·유료 실행을 임의로 선택하지 않는다. 이번에는 읽기 전용 코드/문서/OS·Git 대조만 했고 새 테스트 PASS나 새 제품 기능을 주장하지 않는다.
- **다음**: 사용자에게 전용 환경 별도 로그인 구현 결정을 받은 뒤 설치/인증→실제 adapter→첫 프로젝트 작업의 연결을 진행한다. 실제 유료 smoke·Windows/출시 증거도 별도 남는다. 현재는 첫 명시적 결정 대기 점검이며 목표 완료/blocked를 선언하지 않는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (Codex turn/item 응답 순서 검사)

- **직접 구현**: clean `9ad3094`에서 `codex_events/stream.rs`와 module export·현행 문서를 소유했다. OpenAI Docs의 공식 events와 로컬 생성 schema를 대조했다. Karpathy 지침에 따라 기존 projection/예산을 재사용하고 turn/item 시작·완료·delta의 소속/종류/순서를 확인한다. 공개 본문은 순서 확인 뒤 반환하고, 원시 실패 메시지는 반환하지 않는다.
- **경계·검증**: provider 회귀 36/36 PASS. 정상 완료는 열린 item이 없어야 하며 중단·실패를 성공과 구분한다. 시작 전/중복 시작/종류 변경/완료 후 내용/terminal 없는 EOF는 실패 후 재사용을 막는다. private/미지원 알림은 owner의 별도 처리를 요구하며 허가나 무시할 권위가 아니다. 실제 transport·권한·자손 종료는 미연결이고 모델·인증·사용자 프로젝트 실행은 없었다.
- **검토 수정**: 독립 검토에서 private/unknown item 시작도 owner의 처리가 필요하다고 알려야 한다는 누락을 지적해 agentMessage/plan 이외에는 시작부터 RequiresHandling을 반환하도록 고쳤다. reasoning/command/fileChange/unknown 시작·완료와 reasoning delta 회귀를 추가해 focused 4/4 PASS. core clippy 종료 0(기존 proctree 경고 3개 외 신규 없음), rustfmt·diff PASS다.
- **마감·다음**: 독립 재검토에서 지적이 닫힌 것을 확인했고 이 단위를 커밋한다. 실제 provider 로그인 환경 선택은 답변 대기다. 전체 목표는 계속 유지하며 실제 실행 연결·Claude·역할 인계·원본 적용/복원·guided start·G6/Windows/출시 완료를 이번 오프라인 검증으로 주장하지 않는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (Codex 공개 이벤트 추출 준비)

- **범위·직접 구현**: clean `54d9bb3`에서 core vendor observation과 현행 문서를 소유했다. 원본 적용은 제안서 §14의 실제 adapter 안전 검증 뒤 순서를 유지한다. OpenAI Docs의 [공식 app-server events](https://learn.chatgpt.com/docs/app-server#events)와 보존된 로컬 생성 TS schema의 threadId/turnId/item 구조를 확인하고, agentMessage/plan delta·최종 본문 추출만 구현했다. Karpathy 지침에 따라 transport·로그인·권한 계약으로 범위를 넓히지 않았다.
- **계약**: 공개 본문은 명시적 thread/turn에 결속하며 최종 item은 append가 아니라 교체값이다. raw serde로 인식 필드 중복을 거부하고, 모델 reroute·개수/바이트 예산 위반은 실패 뒤 재사용하지 못한다. 추론·도구·미지원 알림은 본문을 반환하지 않고 owner의 별도 처리를 요구한다. 이 모듈은 순서/terminal 검증·프로세스 종료 증명이 아니며 runtime에 미연결이다.
- **검증**: provider 회귀 33/33 PASS. core all-targets/no-deps clippy는 종료 0, 기존 proctree 경고 3개만 있으며 신규 경고 없음이다. 공개/비공개 stream 개수와 단일 본문 크기 검사에 더해 비공개 누적 byte 초과 회귀를 보완했다. 실제 모델 호출·인증 조회/변경·사용자 프로젝트 변경 없음.
- **검토 마감·다음**: 기존 승인된 읽기 전용 검토자는 수정할 결함 없음을 확인했다. 누적 byte 회귀 추가 후 focused 3/3·rustfmt·diff 검사도 PASS이며 이 단위만 커밋한다. 실제 provider 환경/로그인 선택은 답변 대기이며 요청 응답·item/turn 순서·종료 연결, Claude, 역할 인계·원본 적용/복구·guided start·G6/Windows/출시는 남는다. 전체 개발 완료가 아니며 main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (후속 메시지 파동 macOS 패키지 검증)

- **수정·빌드**: clean `3e57b1f`에서 package smoke의 구형 D1 기대값 뒤에 `chat.appendMessage` 하나를 exact 비교하도록 갱신했다. Karpathy 지침에 따라 baseline fixture나 보안 검증은 완화하지 않았다. 첫 package 실행은 외부 파일 fetch 제한으로 실패했고 허용된 네트워크 실행으로 같은 작업을 재시도해 main/preload/renderer와 release Rust sidecar를 새로 빌드했다.
- **검증**: darwin-arm64 Forge package PASS, 실제 포함 sidecar의 shell ready→storage admission ready→prepareQuit→exit 0/clean marker 및 app.asar/Monaco worker·Electron fuses 검사 PASS. 파동 마감의 run-recovery Node 누적 124/124와 scaffold 5/5·diff 검사 PASS. 패키지 경로는 `out/hello-agent-darwin-arm64/hello-agent.app`이며 SHA-256은 sidecar `e05c0e637a4d19e574e3a540559f224433786d36b89403bdf888f35e0fd70bf6`, app.asar `8a9c261aa750516d87d098f425daaeeb2b582aee223707ae1f904557f711afb1`이다. 패키지 전체 GUI/실제 모델/Windows/서명·배포 승인이나 전체 G6 완료를 뜻하지 않는다.
- **다음**: 실제 provider를 앱 전용 환경의 별도 로그인 방식으로 구현해도 되는지 사용자 선택을 다시 비동기로 요청했다. 유료 모델 실행·기존 인증정보 복사는 승인 범위에 넣지 않았다. 답변 대기 중에도 guided start 등 허용된 개발은 진행할 수 있다. provider·인계·원본 적용/복구·guided start·G6/Windows/출시는 남고 main 병합·push는 하지 않았다.

### 2026-09-14 · by GPT-6 Astra (V7 후속 메시지 입력창 연결)

- **직접 구현**: clean `ee9dd0d`에서 V7의 고정 append IPC·preload·renderer 경계와 팀 대화 입력창을 연결했다. V4~V6는 유지하고 V7도 storage admission 완료 전 쓰기를 차단한다. 채팅별 초안·불확실한 요청을 분리해 전송 중 새 입력과 채팅 전환 중 늦은 응답을 보존하고, 응답/event 중복은 기존 merge로 처리한다. 명시적 무기록 순서 충돌만 새로 읽은 순서로 같은 메시지 ID를 재사용하며 불확실한 결과는 원래 요청 그대로 재시도한다.
- **검증·검토**: 채팅/공개 이벤트/admission 39/39, bridge 44/44, scaffold 5/5, Chromium renderer smoke 2/2, TypeScript noEmit·renderer build PASS. 실제 Chromium에서 채팅 전환→본문 입력→추가→한 번 표시·입력 지우기를 확인했다. 격리 환경의 Electron 실행 실패는 허용된 GUI 실행으로 재검증했고 초기 TS/구버전 scaffold 기대값 오류는 수정했다. 독립 검토에서 추가 지적 없음이다. Karpathy 지침에 따라 저장·메시지 병합 계약을 재사용했다.
- **경계·다음**: 후속 내용은 대화에 저장되며 진행 중인 에이전트 stdin으로 보내지 않는다. 초안은 controller 수명 안에서만 보존된다. 실제 provider 연결·로그인 선택, 작업 간 인계, 원본 적용/복구·guided start·G6/Windows/출시는 여전히 남는다. 패키지 smoke의 ready 기대값이 구형 exact D1에 고정된 것을 확인해 다음 통합 QA에서 새 명시적 suffix에 맞춰 갱신해야 한다. 전체 패키지/누적 게이트를 이번 검사로 대신하지 않는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (후속 메시지 sidecar 통신 연결)

- **직접 구현**: clean `a26faec`에서 기존 저장 writer를 `chat.appendMessage`로 dispatch하고 Electron client 요청/결과 검증을 연결했다. exact G5D baseline은 보존하며 마지막 선택적 capability만 협상한다. 이전 sidecar에는 요청하지 않고 잘못된 순서·중복·미지원 baseline을 거부한다. current Rust/TS 상태 파서는 이 확장과 기존 형식을 지원하되 최초 ready와 status의 신원/기능 목록 일치는 유지한다. storage admission·종료 차단을 우회하지 않으며 모델을 실행하지 않는다.
- **검증**: Electron bridge 44/44, 실제 sidecar stdio 11/11(후속 저장·동일 요청 재시도·기존 준비/취소/복구 포함), Rust protocol 52/52, TypeScript noEmit·sidecar/protocol 자체 strict clippy PASS. 초기 protocol manifest method 누락과 renderer target의 Array.at 오류를 수정한 뒤 재검증했다. Karpathy 지침에 따라 기존 저장 및 transcript 계약을 재사용했다. 전체 제품 누적 gate는 이번 범위가 아니다.
- **검토 마감·다음**: 독립 검토의 기존 baseline 단위 테스트 지적을 수정하고, 저장소 verifying 및 prepareQuit 뒤 요청 차단을 추가했다. 최신 admission 3/3·TS protocol 4/4·포맷/diff 검사 PASS이고 재검토 지적 없음이다. 이 통신 단위를 커밋한다. preload/renderer 입력창은 아직 연결되지 않아 화면에서 후속 지시를 보낼 수 있는 완료 상태가 아니다. 그 연결과 제출 중 추가 입력/재시도 UX, 실제 provider 로그인 선택·실행 인계·원본 적용/복구·guided start·G6/Windows/출시가 남는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (압축 후 chat 재개방 경계 수정)

- **원인·직접 수정**: `83de9e1` 위에서 기존 실패 회귀를 이어 조사했다. v9 계획의 완료/부분 release는 이전 plan row를 의도적으로 삭제하지만 재개방 chat transition 검사가 그 행을 계속 요구했다. `plan_v9.rs`에서 ReleasePlan/Finalize의 cursor가 이미 해제한 predecessor만 exact frozen source로 대조하고, 그 외 단계는 plan predecessor 필수 검증을 유지했다. 저장 형식·삭제 정책·검증 예산은 변경하지 않았다.
- **증거·검토**: 보존 실패 DB의 복사본에서 수정 전 plan_v9.rs:1172 실패→수정 후 product admission 성공을 확인했다. Karpathy 지침에 따라 임시 진단 출력과 로컬 경로 하드코딩 검사는 제거했다. 실제 1,000행 압축→원본 후속 event 부재→재개방→동일 메시지 재시도 무이벤트/무변경 및 frozen predecessor 누락 거부를 포함한 집중 4/4이 66.78초에 통과했다. 기존 읽기 전용 검토자는 코드 결함 없음으로 확인하고 부분 ReleasePlan 경계 검사 추가를 요청했다.
- **최종 검증·다음**: 부분 release 재개방 성공·해제된 frozen predecessor 누락·아직 남아야 하는 plan predecessor 누락 거부를 추가한 최신 focused 4/4(66.89초), 관련 reopen_plan_v9 21/21(109.46초), sidecar 자체 clippy(all-targets/no-deps, warnings denied)·scoped rustfmt·diff 검사 PASS. 독립 검토자가 추가 경계 검사까지 지적 없음으로 확인했다. 전체 sidecar/제품 누적 검증을 이번에 실행한 것은 아니다. 수정·회귀·이 기록만 작업 단위 커밋한다. 후속 메시지 wire/UI·실제 provider 로그인 선택·원본 적용/복구·guided start·G6/Windows/출시는 계속 남는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (후속 메시지 압축 후 재시도 검증)

- **검사 추가**: clean `83de9e1`에서 후속 메시지 저장→동일 chat의 1,001 source rows→제품 배치 정규화→검증 재개방→실제 1,000행 압축→원본 메시지 event 부재 확인→재시작/재시도 시 동일 메시지·무이벤트·동일 projection을 확인하는 회귀를 추가했다. 제품 코드와 임계값은 바꾸지 않았고 테스트용 임시 DB만 사용했다.
- **테스트 구성 수정**: 최초 타입과 준비 절차를 바로잡았다. 압축 전후 모두 v10 product admission으로 열어야 하며, 검사용 별도 SQLite 연결은 기존 저장소를 닫은 뒤 사용해야 한다. v10을 일반 Storage::open으로 재개방하거나 살아 있는 exclusive 연결 옆에서 읽는 실패를 제품 결함으로 기록하지 않는다. Karpathy 지침에 따라 기존 제품 경로로 검증하고 우회/새 저장 구조를 만들지 않았다.
- **새 실패 증거**: 올바른 product admission 경로에서도 압축 후 재시작이 `storageSchemaMismatch: frozen operation verification session is invalid`로 거부됐다. 마지막 session `34683`은 exit 101, 57.64초로 종료했다. `/var/folders/fm/mxcpps3x7f5_w2gp3g5r668c0000gn/T/.tmpNbjzRi/data/hello-agent.sqlite3`를 보존했다. read-only SQL로 chat 원본은 seq1001 한 행만 남고 frozen_operation_sources 1001개, frozen_message_sources/chat_messages 양쪽에 초기 seq1·후속 seq2가 있음을 확인했다. 단순 fresh-row 중복 문제가 아니며 정확한 원인은 조사 중이다.
- **다음**: 실패 회귀와 기록은 미커밋으로 유지한다. 기존 승인된 읽기 전용 검토자에게 코드/보존 DB의 압축-재시작 불일치를 조사 요청했다. 새 test의 잘못된 구성인지 실제 product 결함인지 증거로 구분하고 해결 전 압축 후 재시도 PASS로 표시하지 않는다. 후속 메시지의 동일 요청 강제 interleaving·실패 주입·wire/bridge·전송 UI와 실제 provider/적용/복구·guided start·G6/Windows/출시 작업은 남는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (후속 저장 검토 수정 검증)

- **검증 재개**: 이전 broad chat 검사 session `83144`의 PID 56585가 6분 이상 CPU를 사용하며 101-snapshot을 실행 중인 것을 확인했다. 검토 전 코드의 검사이므로 의도적으로 SIGINT로 중단하고 종료 코드 101을 확인했다. 이 실행은 미완료이며 통과로 사용하지 않는다. 최신 수정 집중 검사로 전환했다.
- **수정·증거**: 테스트의 폐기된 구형 materialize 호출 실패를 현재 product 배치 scheduler로 고쳤다. 최신 focused 3/3 PASS: fresh 재시도·stale projection CAS 거부·정규화 완료 후 재시도·DB 재시작·소속/원문/sequence 충돌·save_sessions 보존·복구 상태 신규 제출 거부. IMMEDIATE transaction의 전체 projection 비교와 CAS 실패 뒤 동일 ID 재조회는 유지한다. Karpathy 지침에 따라 기존 정책을 우회하거나 새 저장 테이블을 만들지 않았다.
- **마감 검증·검토**: sidecar 자체 `clippy --all-targets --no-deps --locked -- -D warnings`, scoped rustfmt·diff 검사 PASS. 의존 core를 포함한 strict 검사 실패는 앞선 기록대로 유지한다. 읽기 전용 검토자는 두 correctness 지적의 코드 수정을 확인하고 현재 내부 구현 차단 결함 없음으로 재검토했다. CAS-false/동일 요청 재조회 분기의 강제 interleaving 검사는 미완료이며 활성화 전 보완 대상이다.
- **남은 범위**: 이 코드는 아직 내부 단계다. 실제 source compaction 후 재시도·실패 주입·새 wire/bridge 협상·전송 UI 검증이 남고, 실제 에이전트 로그인 방식의 사용자 선택 및 전체 M5 잔여도 유지한다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (후속 메시지 영속 저장 경로)

- **직접 구현**: clean `a35f0cc`에서 chat writer·storage 조회·회귀를 소유했다. 후속 메시지를 기존 chat upsert transaction에 저장하고 save_sessions와 기존 projection 필드를 보존한다. 정확한 소속/예상 sequence를 확인하며 같은 ID·내용·순서의 재시도는 기존 메시지만 반환하고 event를 다시 보내지 않는다. 새 제출은 복구/폐기/완료와 종료 준비 중 차단하며 모델을 실행하지 않는다. 제품 wire/bridge는 아직 미연결이다.
- **검증 중 발견**: 정규화 chat_messages만 조회하면 materialization 전의 새 메시지를 찾지 못하는 실패를 재현했다. operation_id unique index로 fresh source도 조회하도록 수정해 최초 저장·재시도·재시작·원문/소속/순서 충돌·저장 세션 보존·복구 차단 focused 3/3을 통과했다. source compaction 뒤 검증은 아직 별도 미완료다. scoped rustfmt·diff 검사 PASS. strict clippy는 기존 core/proctree의 unnecessary_lazy_evaluations·map_entry에서 실패해 전체 PASS로 주장하지 않는다.
- **독립 검토·수정 진행**: 검토자가 transaction 밖 순서 검사와 동일 요청 동시 재시도 경합을 지적했다. IMMEDIATE transaction 안에서 원본 chat projection 전체를 대조하고, 불일치 후에도 같은 ID의 저장 결과가 생겼는지 재조회하도록 수정했다. stale projection 검사·materialize 후 재시도 테스트를 추가했으며 이 최신 변경의 실행 검증은 아직 대기다. 앞선 3/3을 수정 후 PASS로 대신하지 않는다.
- **진행 중 검증·다음**: 이전 코드의 `cargo test -p hello-agent-sidecar chat::tests --locked`는 exec session `83144`, PID 56574/56585에서 101개 snapshot 검사 중이며 2026-09-14 관측 시 약 5분 경과한 live 상태였다. 종료/실패로 추정하지 않고 같은 handle로 확인한 뒤 최신 focused 검사를 수행한다. 현재 변경은 미커밋이다. 압축 후 재시도/실패 주입 검증·wire/bridge·전송 UI와 실제 provider 로그인 선택, 원본 적용/복구·guided start·G6/Windows/출시는 남는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (후속 지시 요청·결과 계약 시작)

- **직접 구현**: clean `da82a07`에서 메인이 TS/Rust protocol validator·공용 후속 메시지 fixture와 회귀를 소유했다. exact 소속·reserved user UUID·UTF-8 16KiB·안전한 예상 순서를 검사하고 durable user 메시지의 ID/원문/소속/다음 sequence를 대조한다. 제안서의 후속 지시 연결 계약 초안에 동일 요청 재시도·충돌·실행 중 입력 분리와 구현 잔여를 기록했다.
- **검증**: TS focused 2/2와 Rust protocol 전체 52/52 PASS. 초기 Rust 구문 오류는 수정했다. scoped rustfmt·TypeScript·diff 검사 PASS이며 전체 workspace fmt에서 발견한 기존 Tauri detect/login 포맷 차이는 무관하여 변경하지 않았다. 저장/전송 API와 capability는 아직 열지 않았고 실제 모델 호출·로그인 변경·main 병합·push는 없다.
- **다음**: 후속 지시는 계약 구현 단계일 뿐 제품 기능 완료가 아니다. durable ID 재시도/충돌 처리와 보존 원장 연결→bridge→화면→독립 검토를 진행한다. 실제 provider 별도 로그인 선택은 계속 사용자 답변 대기이고 적용/복구·guided start·G6/Windows/출시도 남는다.

### 2026-09-14 · by GPT-6 Astra (채팅 목록 추가 페이지 연결)

- **발견·직접 구현**: clean `feeaa45`에서 후속 메시지 흐름을 조사했다. 현재 bridge/sidecar에는 chat.create/list/get만 있고 사용자 후속 메시지 저장 API가 없으므로 별도 영속 계약 연결이 필요하다. 함께 확인한 목록 결함은 첫 100개 뒤 nextCursor를 버려 나머지 채팅에 접근할 수 없는 것이었다. controller/작업대에 `채팅 더 보기`를 연결했다. 기존 목록과 최신 객체는 유지하고 중복 ID를 제거하며 한 번에 100개만 추가 요청한다.
- **검증**: 채팅·public-event 29/29, TypeScript noEmit·diff 검사 PASS. 추가 페이지 실패/재시도·끝 페이지·중복 클릭·프로젝트 변경 뒤 늦은 응답을 확인했다. 파일/실행/저장 권한은 변경하지 않았고 일반 목록 UI는 직접 검토했다. 서브에이전트·worktree 추가 없음.
- **다음**: 후속 지시 전송은 아직 구현되지 않았다. 사용자 메시지의 영속 순서·재시도 중복 방지·정확한 채팅 소속과 실행 중 제출 정책을 새 통신 계약에서 결속해야 한다. 실제 provider의 별도 로그인 방식 답변 대기와 원본 적용/복구·guided start·G6/Windows/출시 범위도 남는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (새 diff 화면 성능 실측)

- **실측**: clean `3f621732df1e1834fe2528964d94320221c84776`의 production renderer를 실제 Electron/Chromium에서 warm-up 1회 뒤 10회 측정해 PASS했다. p95는 tree root 76ms, 폴더 확장 17.1ms, 5MiB 파일 표시 94.1ms, 입력 24.8ms, 500파일 변경 요약 77.4ms, 첫 파일 diff 17.5ms다. 측정 회차의 longtask 관측은 0ms, tree DOM은 500개다. 전후 clean/same HEAD이며 `.artifacts/m5-performance/renderer-2026-09-14T04-46-30.281Z-33604.json`에 표본을 보존했다.
- **범위·다음 결정**: 합성 bridge의 renderer 부분 측정이며 실제 provider·sidecar IPC·메모리·전체 G6 검증이 아니다. 실제 연결의 핵심 대기 사항은 앱 전용 환경에서 별도 로그인하는 방식의 허용 여부다. 기존 사용자 인증/설정을 복사하거나 바꾸지 않고 이 선택을 다시 명확히 질문한다. 실제 실행·역할 인계/후속 지시·원본 적용/복구·guided start·Windows/출시는 계속 미완료다. 이번에는 코드 수정·서브에이전트·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (줄 단위 변경 검토 화면)

- **직접 구현**: clean `bd8dea3`에서 diff 표시 모듈·작업대/CSS·관련 테스트를 소유했다. 원시 patch 대신 변경 전후 줄 번호와 추가/삭제 색상·기호를 표시하고 200줄씩 전체 내용을 탐색한다. 여러 hunk·0줄 범위·파일 끝 개행 없음 표시를 처리하며 불완전/미지원 hunk는 줄 번호를 추측하지 않고 원문 그대로 보여 준다. 렌더링은 텍스트 노드만 사용하고 파일 반영·실행 계약은 변경하지 않았다.
- **검증**: 파서 4/4, 실제 Electron/Chromium smoke 2/2 PASS. 403줄 fixture에서 페이지당 200행 이하·마지막 줄 도달·재열기 시 첫 페이지 복귀와 기존 도킹/초안 복구를 함께 확인했다. TypeScript noEmit·renderer build·diff 검사 PASS. 기존 성능 수집기는 새 DOM으로 갱신했지만 이전 성능 PASS를 새 화면의 실측으로 주장하지 않는다. 일반 표시 변경은 직접 검토했고 서브에이전트/worktree는 추가하지 않았다.
- **다음**: 전체 완료가 아니다. 실제 provider 설정/인증 격리 결정, 실제 역할 실행·후속 지시, 원본 적용/복구, guided start, G6와 Windows/출시 검증이 남는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (편집 초안 재읽기 복구)

- **직접 구현**: `aa51aac` 기준 editor controller·작업대·관련 회귀를 소유했다. dirty/충돌 파일에서 사용자가 확인한 정확한 초안만 버리고 저장된 파일을 다시 읽는다. 실패·추가 입력·프로젝트/작업 복사본 변경·차단 상태에서는 초안을 보존하며 파일 쓰기는 하지 않는다. 충돌 화면은 보존된 초안을 복사할 수 있다. 원본 프로젝트 적용/되돌리기 기능과는 별개다.
- **검증·수정**: 기존 승인된 읽기 전용 검토자의 검토에서 지적 없음. 실제 Chromium 검증으로 재읽기 후 Monaco의 내용 동기화 알림이 dirty를 다시 만드는 결함을 발견해 동일 텍스트 알림을 무시하도록 수정했다. editor/workspace 36/36, Chromium smoke 2/2(취소 시 보존·확인 후 clean), TypeScript noEmit·renderer build·diff 검사 PASS. 초기 숨긴 창의 입력 실패와 수정 전 dirty 회귀 실패를 통과로 취급하지 않는다. 번들 크기 경고는 남는다.
- **다음**: 실제 provider의 별도 로그인 환경 선택은 답변 대기다. 실제 실행·역할 인계/후속 메시지·원본 적용/복원·guided start·G6/Windows/출시는 미완료이며 개발 완료를 선언하지 않는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (저장 중 추가 입력 경합 수정)

- **재현·직접 수정**: clean `dfe261d`에서 메인이 editor-state와 해당 회귀를 소유했다. 저장 중 입력하면 saveState가 dirty로 바뀌어 기존 commit 응답을 무시하고 중복 저장을 허용하는 결함을 실패 테스트로 재현했다. 입력은 유지하되 진행 중 상태를 보존하고, 성공 응답의 새 baseVersion을 반영한 뒤 제출 내용과 최신 초안이 다르면 dirty로 남긴다. 후속 저장은 갱신된 버전을 사용한다. backend 쓰기·격리·권한·충돌 계약은 변경하지 않았다.
- **검증**: 수정 뒤 editor 전체 29/29, 추가 failure/conflict 회귀와 기존 watcher 우선 처리 포함 focused 3/3 PASS. 채팅 workspace editor 4/4, TypeScript noEmit·diff 검사 PASS. 성공·실패·외부 충돌 모두 추가 입력을 보존하며 진행 중 중복 저장을 막는다. 일반 controller 수정은 직접 검토했고 새 검토자/worktree는 만들지 않았다.
- **다음·목표**: 이전 패널 기능은 진전이며 전체 개발 완료가 아니다. 실제 provider의 별도 로그인 환경 선택은 아직 사용자 답변 대기이며, 실제 실행·역할 인계/후속 메시지·원본 적용/복원·guided start·G6/Windows/출시가 남는다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (작업대 패널 접기·집중 모드)

- **직접 구현**: clean `08dbc7e`에서 메인이 dock 상태/UI/CSS·관련 회귀를 수정했다. 파일·편집기·변경·대화 패널을 접고 다시 열거나 하나에 집중할 수 있다. 순서·폭과 함께 표시 상태를 저장하며 예전 v1 배치는 호환한다. 마지막 패널은 접을 수 없고 대화만 보다가 채팅을 벗어나면 편집기를 표시한다. 숨긴 패널은 unmount하지 않아 Monaco 인스턴스를 유지한다. 실행·원본·저장 계약은 변경하지 않았다.
- **검증**: dock 회귀 6/6, 실제 Chromium smoke 2/2(접기·복원·집중·이동에서 같은 Monaco DOM 유지), TypeScript noEmit·production renderer build·diff 검사 PASS. 일반 UI 변경은 직접 검토했으며 새 worktree·서브에이전트 호출은 없다. renderer 번들의 기존 크기 경고는 남는다.
- **전체 목표·다음**: 이전 RPC commit은 진전이며 개발 완료가 아니다. 실제 연결의 전용 환경 로그인 선택은 답변 대기다. 코드 편집의 저장 중 추가 입력 처리도 다음 확인 대상으로 발견했다. 실제 provider/인계·후속 지시·적용/복원·guided start·G6/Windows/출시는 남고 목표를 유지한다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (완료 목표 지속·Codex 요청 응답 결속)

- **목표·기준**: 사용자 `개발 완료까지 계속` 목표를 유지한다. 이전 응답 파서 통합은 진전이지만 전체 완료가 아니다. clean `7a156d6`에서 메인이 core Cargo feature·vendor_observation 모듈·관련 문서를 소유했다.
- **구현**: `codex_rpc.rs`는 한 번에 한 요청만 발행하고 initialize→initialized 알림/config→account(refreshToken:false)→model 목록 순서를 고정한다. 정확한 숫자 ID·중복 envelope·오류/예상 밖 frame을 검사하고 실패 뒤 재사용을 막는다. 임의 메서드·thread/turn 생성·인증 변경·prompt 전달은 표현할 수 없다. serde_json raw_value 기능으로 중첩 원문을 보존해 기존 typed 파서의 중복 필드 거부가 재직렬화로 사라지지 않게 했다. 수신 성공은 필드 검증·실행 권위가 아니며 실제 process/runtime에는 아직 미연결이다.
- **검증**: provider 회귀 30/30 PASS. Clippy 일반 실행은 기존 proctree 경고 3개 외 신규 경고 없음. lock 변경·실제 모델 호출·인증 조회/변경·사용자 프로젝트 변경 없음. 같은 승인된 읽기 전용 검토자에게 이 변경만 검토 요청했다.
- **검토 종결**: 독립 검토에서 actionable defect 없음을 확인했다. 권고한 initialized 전체 객체·요청 ID assertion을 추가하고 RPC 회귀를 재확인했다. runtime 연결 또는 제품 활성화 승인으로 해석하지 않는다.
- **다음 결정**: 공식 인증/환경 문서와 local app-server help를 확인했지만 기존 구독 인증을 재사용하면서 모든 사용자 설정을 분리하는 지원 경로는 아직 확정하지 못했다. 사용자에게 앱 전용 환경의 별도 로그인 허용 여부를 비동기로 질문했다. 기존 HOME/인증 파일 복사·설정 거부 해제는 하지 않는다. 이 선택, 실제 transport/종료 연결·Claude·원본 적용/복원·guided start·G6/Windows/출시는 미완료이며 목표를 유지한다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (Codex 설정·모델·thread 응답 검증)

- **직접 작업 계약**: clean `0a7e9ab`에서 메인이 core의 `vendor_observation/codex_preflight.rs`와 모듈 선언·관련 기록을 소유했다. 기존 승인된 읽기 전용 검토자 1명만 같은 연결 파동의 변경을 검토하고 새 worktree/구현자는 만들지 않는다.
- **구현**: Codex 0.149.1의 config/read는 명시적 model/effort·openai·on-request·read-only와 대조하고, model/list는 완전한 목록의 정확한 model/effort만 받는다. 숨긴 모델·기본값/upgrade 대체·불완전 페이지·중복 필드는 거부한다. thread/start 응답은 정확한 model/effort/cwd·user 승인·network 없는 readOnly·빈 instructionSources를 대조한다. 결과는 unit뿐이며 실행 권위나 파일시스템 신원/설정 격리 증명이 아니다. runtime/capability/기존 fake 계약은 변경하지 않았다.
- **출처·관측 범위**: 공식 app-server 문서를 재확인하고 로컬 0.149.1의 generate-ts로 모델 호출 없이 schema를 재생성했다. 예전 임시 schema 경로는 없어 새 `/private/tmp/hello-agent-schema-gM1m6L`을 사용했다. schema 구조와 합성 wire 테스트이며 실제 config/model/thread 요청 성공이나 모델 가용성 실측을 주장하지 않는다.
- **검증·다음**: 기존 provider 포함 27/27 PASS, rustfmt·diff 검사 PASS. Clippy는 변경 없는 proctree의 기존 경고 3개와 함께 성공했고 새 모듈 경고는 없다. 설정/인증 격리·응답 소속 검증/입력 보류·실제 process 연결·Claude 검증과 승인 모델 smoke는 남는다. 제품 활성화·사용자 프로젝트 변경·main 병합·push 없음.
- **독립 검토 종결**: 동일 검토자가 config/model과 thread 응답 대조의 최종 변경을 확인해 actionable defect 없음으로 판정했다. runtime 비연결·legacy sandbox 응답과 실제 권한/격리의 구분을 재확인했으며 제품 활성화 판정은 아니다.

### 2026-09-14 · by GPT-6 Astra (실제 provider 독립 검토 승인)

- **사용자 승인·작업 계약**: `bbb03f08cdeced97bddf53f792b2edc84ccfa2a7`의 clean `m5/coding-agent-ide`에서 실제 provider 연결의 읽기 전용 검토자 1명을 호출했다. 메인이 모든 구현·문서·커밋을 소유하며 새 worktree·병렬 구현자는 만들지 않는다. 승인 범위는 독립 안전 검토이며 실제 유료 모델 실행·사용자 프로젝트 변경·서명/배포·main/push 승인은 아니다.
- **직접 대조**: 일반 runtime은 아직 차단 모드이고 start·provider 관측·launcher는 fake executable에 결합돼 있다. 이전 로그인/초기화 관측을 제품 실행 권위로 올리지 않는다. 기존 fake 버전을 실제 CLI 버전으로 치환하거나 LaunchSpec의 설정/환경 거부를 해제하는 방식으로 연결하지 않는다.
- **독립 결과·직접 구현**: 검토자는 일반 제품의 차단 상태와 fake-only discovery/launcher/Claude handshake를 확인했다. 실제 활성화 승인이 아니라 미구현 경계 확인이다. 메인은 기존 provider와 분리된 `vendor_observation.rs`에 Codex 0.149.1·Claude 2.1.269 exact version/auth 파서를 구현했다. bounded transient 입력과 redacted enum 출력만 다루고 runtime/capability·공개 계약·설정 격리·원본/복구는 변경하지 않는다. 같은 검토자에게 이 변경만 재확인 요청했다.
- **검증·다음**: 새 회귀 4/4와 기존 provider 포함 18/18, rustfmt·diff 검사 PASS. Clippy 일반 실행은 성공했지만 `-D warnings`는 변경 없는 proctree.rs/tests.rs의 기존 lint 3개로 실패했다(새 모듈 경고 없음). AGENTS.md/CLAUDE.md 동일성 확인. config/model/handshake·입력 보류·실제 프로세스 연결 및 승인 smoke가 다음이다. 실제 CLI 재실행·모델 호출·main/push는 없다.
- **변경 재검토 완료**: 같은 독립 검토자는 파서 diff에 actionable defect 없음을 확인했다. 권고한 Codex 중복 필드·버전 공백·Claude null/unknown auth 경계 회귀 2개를 더해 최종 provider 검사 20/20 PASS했다. 신규 파서 검토 통과는 실제 provider 활성화 승인과 구분한다.

### 2026-09-14 · by GPT-6 Astra (패널 배치 구현·모델 없는 실제 CLI 관측)

- **직접 구현**: 파일·편집기·변경·팀 대화를 같은 작업대로 연결하고 drag/drop 순서 변경, 경계선 드래그·키보드 폭 조절, 로컬 배치 저장·검증·기본값 복원을 추가했다. 패널 key를 유지해 이동 중 Monaco를 재생성하지 않는다. 현재는 가로 열 재배치이며 자유로운 상하 분할·탭 도킹이나 전체 시안 적용 완료를 뜻하지 않는다.
- **검증**: 배치 파서/순서/폭 경계 회귀 3/3, 실제 Chromium smoke 2/2(이동·키보드 resize·저장·Monaco DOM 유지), TypeScript noEmit·production renderer build PASS. 제품 실행/원본/저장 계약은 변경하지 않았다.
- **실제 관측**: 사용자 잔여 작업 진행 요청 범위에서 모델 없는 관측을 수행했다. 생성한 빈 임시 폴더에서 Codex 0.149.1 app-server의 initialize→initialized→config/read→account/read(refreshToken:false)가 성공하고 stdin 종료 후 exit 0이었다. 계정은 chatgpt/credentials present 분류만 남겼다. Claude 2.1.269 auth status는 exit 0/loggedIn true/claude.ai였다. thread/turn/model 요청 0회이며 계정 식별자·토큰·원문 설정은 기록하지 않았다. 이는 설정 격리·모델 가용성·제품 활성화 증거가 아니다.
- **다음·경계**: 실제 adapter/인계·원본 적용/복원·guided start·전체 G6·Windows·출시는 남는다. 제품 활성화 전 읽기 전용 독립 검토자 1명 승인을 요청했으며 답변 전 호출하지 않는다. 유료 smoke·사용자 프로젝트 변경·서명/배포도 별도 승인 경계다. 500줄 초과를 막기 위해 9월 8~9일 워크로그를 내용 변경 없이 월별 아카이브로 이동했다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (잔여 제품 작업 현황 확인)

- `e412225`와 현행 M5 화면·계약을 대조했다. 워크트리 정리는 종결했지만 실제 CLI adapter/역할 인계·후속 지시, 원본 적용/복원, 승인 시안/설치·로그인 guided start의 Electron 연결, 전체 G6·Windows·출시 검증은 남는다. G1~G5D fixture 기반 성공과 제품 M5.0~M5.5 완료를 혼동하지 않는다. 다음 핵심 구현은 실제 CLI 연결이며 모델 없는 startup/config 관측도 기존 별도 승인 대기다. 코드 수정·테스트 재실행·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (잔여 미커밋 작업 9개 대조·종결)

- **사용자 요청·결론**: `3790832`에서 남은 9개를 전수 대조했다. 4개는 검토 자료뿐이고 5개 코드 초안은 후속 통합 구현으로 대체돼 있었다. V6→V10·version-only Electron→V6 bridge·linked worktree→standalone snapshot을 옛 코드로 되돌리지 않았다. 누락된 managed-worktree symlink swap 회귀만 현재 저장 syscall hook으로 이식했고 제품 코드는 변경하지 않았다. 원본 파일·메모·ignored 검증 로그는 기존 기능 브랜치의 보존 커밋 9개에 남겼다.
- **검증·정리**: 새 Rust 회귀 1/1, 기존 syscall 회귀 2/2, Electron/하네스 Node 14/14, rustfmt·diff 검사 PASS. 보존 SHA·브랜치·clean 상태 확인 후 9개 worktree를 force 없이 제거했다. 현재 개발 checkout만 남고 약 8.2 GiB를 추가 정리했다. QA JSON은 `.artifacts/retired-worktrees-2026-09-14/qa-low-impact`에도 복사·대조했다. 원본 복원 SHA와 항목별 대응은 `archive/m5-worktree-reconciliation-2026-09.md`다.
- **다음 경계**: 잔여 워크트리 작업 대조/통합은 종결했다. 실제 CLI 연결 승인·전체 G6·적용/복원·Windows 검증은 별개로 남는다. main 병합·push·서브에이전트 호출 없음.

### 2026-09-14 · by GPT-6 Astra (완료된 워크트리 정리)

- **사용자 요청·처리**: 저장공간 정리 대상이 workflow가 아니라 worktree임을 확인했다. 등록된 보조 worktree 30개 중 HEAD가 통합 브랜치의 조상이고 tracked/untracked 변경이 없는 21개를 재검증한 뒤 `git worktree remove`로 제거했다. force·브랜치 삭제·main 병합·push는 하지 않았다. 현재 checkout과 미커밋 변경이 있는 9개(그중 `m5-g4-project-safety`는 HEAD도 미통합)는 보존했다.
- **보존·검증**: 제거 대상의 사용 중 cwd가 관측되지 않음을 확인했다. `g5d-r3c-snapshot-qa`와 `g5d-storage-harness`의 검증 증거는 `.artifacts/retired-worktrees-2026-09-14/<원래 이름>/`으로 복사하고 recursive diff로 일치를 확인했다. 브랜치/커밋에서 소스 worktree는 재생성 가능하며 제거된 빌드 캐시는 재빌드가 필요하다. `.worktrees`는 약 24 GiB에서 8,595,224 KiB(약 8.2 GiB)로 감소했다. root `target`·node_modules·배포 산출물은 이번 삭제 대상이 아니다.
- **다음**: 남은 9개는 미커밋 코드·작업 메모 보존/중복 확인 전 삭제하지 않는다. 실제 에이전트 연결 승인 대기는 그대로이며 개발 완료를 뜻하지 않는다.

### 2026-09-14 · by GPT-6 Astra (부분 메모리 재측정 PASS·실제 연결 승인 대기)

- **결과**: clean `dfe12bd980ba076b4fe14717a3f477345b447e1d`의 lazy fixture로 같은 GUI 10회+60초 잔류를 재측정해 PASS했다. Electron subtree idle 356.703125 MiB, peak 761.734375 MiB, 잔류 110.125 MiB다. tree p95 30.5ms, 파일 표시 93.2ms, 입력 24.8ms, diff 요약 48.3ms, 첫 diff 17.5ms이며 `.artifacts/m5-performance/renderer-2026-09-14T02-42-47.203Z-1604.json`에 clean/same HEAD·원시 표본을 남겼다. 이전 FAIL은 보존하며 그 회차 지연 원인은 확정하지 않는다.
- **완료·다음**: `affd08d`의 프로젝트 설정 경합 수정(관련 27/27)과 메모리 수집/fixture(5/5)를 커밋했다. 전체 G6·실제 provider·적용/복원·Windows·출시는 미완료다. 실제 CLI startup/config/login 최소 관측은 사용자에게 승인 요청한 상태이며 모델 호출·인증 변경·사용자 프로젝트 실행은 하지 않았다. 승인 전 실제 연결 단계는 대기한다. 서브에이전트·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (idle 측정 fixture 할당 분리)

- **발견·수정**: `affd08d`에서 메인이 renderer 측정용 fixture/preload를 수정했다. 측정 fixture가 파일을 열기 전부터 5 MiB 텍스트 생성·UTF-8 SHA 계산을 수행해 empty idle에 테스트 데이터 비용을 포함했다. 파일을 실제 요청할 때만 생성/hash하도록 바꿨다. 제품 코드 최적화나 앞선 FAIL의 원인 확정 주장이 아니다.
- **검증·다음**: fixture의 exact bytes/lines/diff/tree 및 RSS helper 5/5, preload syntax PASS. clean commit 뒤 같은 메모리 실측을 다시 수행하며 기존 실패 표본은 보존한다.

### 2026-09-14 · by GPT-6 Astra (RSS 실패 증거·프로젝트 설정 응답 격리)

- **실측**: clean `f4def74e2a9b4f683271535839b2f2094400da4c`에서 `m5:measure:renderer --memory`를 실행했다. idle 429,637,632 bytes(약 409.73 MiB), 관측 peak 798,261,248 bytes(약 761.28 MiB), 60초 잔류 delta 0 bytes다. idle 400 MiB 기준과 한 회차 tree 3383.7ms/diff 요약 3387.7ms가 초과해 **FAIL**을 유지한다. `.artifacts/m5-performance/renderer-2026-09-14T02-37-44.611Z-91340.json`에 동일 HEAD·clean·전체 표본이 있다. 합성 bridge Electron 측정이므로 실제 제품 sidecar 포함 RSS나 원인 확정으로 해석하지 않는다.
- **결함·직접 수정**: 여러 프로젝트 이동 중 이전 프로젝트의 settings가 남고, 늦은 저장 성공이 같은 revision의 새 프로젝트 설정을 덮거나 늦은 실패가 이전 프로젝트를 다시 여는 UI 경합을 확인했다. 프로젝트 진입 때 settings를 비우고 저장 성공/실패 모두 generation·projectId를 대조한다. 제품 권한·원본·실행 경계는 변경하지 않는다.
- **검증·다음**: 새 성공/실패/로딩 회귀 포함 chat/public-event 27/27, TypeScript noEmit·diff 검사 PASS. 실제 adapter startup/config 관측은 §6.7의 별도 사용자 승인 전 실행하지 않는다. 모델 호출 없는 임시 폴더 연결 확인 승인을 요청했다. 이후 실제 adapter 연결·적용/복원, 전체 성능·Windows·출시 경계가 남아 있어 개발 완료가 아니다. main 병합·push·서브에이전트 호출 없음.

### 2026-09-14 · by GPT-6 Astra (Electron 부분 RSS 수집)

- **범위·구현**: `2e200d2`에서 메인이 기존 renderer collector와 RSS helper/회귀/절차를 직접 소유했다. 선택 PID와 자손의 실제 RSS를 250ms마다 수집하고, 문서 닫기 후 60초 잔류를 관측하는 `--memory` 옵션을 추가했다. 제품 실행·권한·저장 경계는 변경하지 않는다.
- **검증·다음**: PID subtree·누락/중복/비정상 값 거부·합계/잔류/상한 회귀 3/3, syntax·diff 검사 PASS. clean commit 뒤 GUI 실측한다. 실제 sidecar/agent·이탈 자손은 포함하지 않아 전체 RSS/G6 완료로 쓰지 않는다. 서브에이전트·main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (renderer 대형 fixture 실측 완료)

- **완료·증거**: clean `78323488f7df93e0e92a136347aa0b6b3a15a9ea`의 production renderer를 실제 Electron/Chromium 창에서 warm-up 1회 뒤 10회 측정해 PASS했다. p95는 tree root 21.3ms, 폴더 확장 18.3ms, 5MiB 파일 표시 75.9ms, 입력→화면 32.3ms, 변경 요약 38.7ms, 첫 diff 25.0ms다. 두 번째 500개 페이지를 탐색했고 tree DOM 500개를 유지했다. 측정 회차에서 50ms 이상 longtask는 관측되지 않았다. 전후 HEAD 동일·clean이며 `.artifacts/m5-performance/renderer-2026-09-14T02-27-28.543Z-72171.json`에 표본·renderer hash·환경을 보존했다.
- **검증·범위**: Apple M4 Pro/24GiB, Electron 43.4.0에서 합성 bridge fixture와 실제 Monaco를 사용했다. editor/fixture 30/30 및 채팅 workspace editor 4/4, TypeScript·renderer build·syntax·diff 검사 PASS. 실제 provider·사용자 프로젝트·인증은 사용하지 않았으며 무관한 누적 G5D 검사와 서브에이전트 호출은 생략했다.
- **다음**: packaged interactive startup, 실제 stream paint·전체 RSS·저장 경합/종료 수집이 남아 있다. 이번 결과는 renderer 부분 성능 증거이며 전체 G6 완료가 아니다. main 병합·push 없음.

### 2026-09-14 · by GPT-6 Astra (대형 파일 목록·renderer 성능 계측)

- **기준·직접 구현**: `3691e57`에서 메인이 editor-state/workbench/styles·renderer 측정 fixture/runner·package script·관련 문서를 소유했다. 큰 프로젝트에서 nextCursor를 버려 첫 500개 이후 접근할 수 없던 결함에 페이지 탐색을 추가했다. 한 번에 500개만 표시하고 폴더 전환/늦은 응답/실패 재시도와 중복 클릭을 처리한다. tree 행의 장식은 CSS로 옮겨 페이지 내부 element를 500개 이하로 유지한다.
- **계측**: production renderer·실제 Monaco에 context-isolated fixture bridge로 1,000폴더/10,000파일, 한국어 5MiB/100,000줄, 500파일/20,000변경줄/10MiB 합계 patch를 공급한다. 보이는 Chromium 창에서 warm-up 1회·10회 표시/입력/longtask를 관측한다. sidecar·실제 사용자 프로젝트·제품 sandbox는 변경하지 않는다.
- **검증·다음**: editor/fixture Node 30/30, TypeScript noEmit, renderer build 통과. clean commit 뒤 실제 GUI 측정한다. 누적 G6와 실제 stream/RSS/packaged startup은 아직 완료가 아니다. 서브에이전트 호출 없음.

### 2026-09-14 · by GPT-6 Astra (sidecar 시작 성능 실측)

- **완료·증거**: clean `d97ee562c0e32efea36ea4d6116a63bfa12f4d84`의 release sidecar를 생성한 임시 빈 DB에서 migration bootstrap·warm-up 1회 뒤 10회 측정했다. shell-ready p95 3.863ms, storage-ready p95 52.596ms로 1,000ms 기준 PASS이며 모든 회차의 protocol/status/quit 응답, 정상 종료와 DB clean marker를 확인했다. 전후 HEAD 동일·clean. `.artifacts/m5-performance/sidecar-startup-2026-09-14T02-18-19.162Z-56332.json`에 표본·binary SHA·기기 정보를 보존했다. 정상 측정 뒤 해당 임시 DB 폴더는 정리했다.
- **환경·검증**: 첫 sandbox 실행은 기기 정보 조회 실패로 표본 없이 FAIL을 기록했고, 권한을 허용한 실행에서 위 실측을 완료했다. 성능 판정기 회귀 8/8, syntax·diff 검사 통과. 제품 코드 변경이 없어 기존 16분 누적 G5D 검사를 반복하지 않았다. 서브에이전트 호출·main 병합·push는 없다.
- **다음**: 패키징 앱 interactive·큰 파일/diff·stream paint·전체 RSS·저장 경합/종료 계측을 연결해야 전체 `m5:test:performance`를 열 수 있다. 현재 도구는 개별 성능 관측/입력 판정이며 G6 완료 또는 실제 provider·Windows 승인 증거가 아니다.

### 2026-09-14 · by GPT-6 Astra (G6 성능 측정 도구 직접 구현)

- **기준·범위**: 사용자 커밋 승인으로 Astra 운영 규칙을 `e97d0ee1d6a7bcbc060358b594583755c105a8e5`에 기록했다. 같은 기준에서 메인이 package scripts·성능 측정 도구 2개·회귀·문서를 직접 소유해 구현했다. 서브에이전트 호출은 없었다.
- **구현**: §10의 표본·fixture·시간·메모리·오류 상한을 공급된 JSON에 대해 판정한다. 누락/비정상/다른 HEAD는 NOT_READY, 초과는 FAIL이며 합성 fixture를 실측으로 쓰지 않는다. 별도 sidecar collector는 current release Cargo executable, 생성된 빈 DB, migration 제외·warm-up 1회·10회, canonical 응답 검증·정상 종료/clean marker와 shell/storage 준비 시간을 기록한다.
- **검증·다음**: 판정기 Node 회귀 8/8, 두 스크립트 syntax와 diff 검사 통과. 최초 회귀에서 발견한 CLI의 macOS 임시 경로 alias 진입점 판정을 realpath 비교로 고쳤다. clean commit 뒤 sidecar 실측을 수행한다. 전체 UI/메모리/경합/종료 계측 수집은 후속이며 `m5:test:performance`는 아직 열지 않아 G6 NOT_READY를 유지한다. 사용법은 `procedures/m5-performance.md`다.

### 2026-09-14 · by GPT-6 Astra

- **사용자 결정·반영**: Sol 기반 의무 위임 운영에서 현재 Astra 메인의 직접 개발을 기본으로 전환했다. AGENTS.md/CLAUDE.md를 동일하게 갱신하고 실행 절차·문서 색인에 우선 규칙을 추가했다. 단일 기능의 의무 worktree·역할별 인계·반복 검사를 줄이고 서브에이전트가 더 유리하면 이유·범위를 설명한 뒤 호출 전에 대기한다.
- **범위**: 기존 역할별 Sol/Terra 설정은 대기용으로 보관하며 모델·추론 수준을 일괄 변경하지 않았다. 중요 안전 경계와 제품 활성화·출시의 독립 검토, M5 자동 커밋·main/push 승인 경계를 명시했다. 이번 규칙 변경은 미커밋으로 남긴다.
- **검증·다음**: 두 루트 지침의 동일성, 관련 문서 우선순위와 diff 검사를 확인한다. 제품 코드·하네스는 변경하지 않았으며 누적 테스트나 서브에이전트 호출은 하지 않는다. 다음 구현은 최신 코드·명세를 확인해 메인이 직접 진행하는 것을 우선한다.


## 프로젝트 요약 (고정)

터미널을 한 번도 안 열어본 비개발자를 **코딩 에이전트(Claude Code·Codex)가 굴러가는 상태까지** 데려다주는 데스크톱 GUI 앱. 현재 배포 경로는 Tauri 2 + React 19이고, M5는 Electron + React/TypeScript + Monaco와 Rust sidecar의 별도 작업대 골격으로 전환 중이다.

- 기획 원본: `~/Library/CloudStorage/OneDrive-개인/ObsidianVault/오픈소스/아이디어/후보/에이전트-스타터.md`
- 레포: https://github.com/wonseok-han/hello-agent (로컬 폴더도 `~/projects/hello-agent`로 변경 완료 — 2026-08-12 확인)
- 위저드 6단계: 에이전트 선택 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식
- 주요 모듈: 기존 Tauri `src-tauri/src/`, 공용 Rust `crates/hello-agent-{core,protocol,sidecar}`, M5 셸 `electron/`과 작업대 `src/m5/`

## 핵심 결정·발견 (고정 — 재조사 방지)

- **제품 로그인 활성화(2026-09-14)**: macOS Codex 0.153.4 exact pinned 쌍의 앱 전용 로그인은 사용자 브라우저 인증과 앱 `연결됨` 표시, pending marker 부재로 실사용 통과했다(`8eb6129`). 원래 터미널 인증은 보존한다. 로그인은 모델 실행 준비 판정이나 재시작 후 상태 복원 증거가 아니다. 일반 개발 실행도 실제 debug sidecar를 필요로 한다. 초기/실행 cleanup 불확실성은 bounded 재시도 후에도 자동 해제하지 않고 안전 종료와 재로그인을 차단한다.

- **CLI 버전 드리프트(2026-09-14 후속)**: 현재 ChatGPT.app 번들의 Codex는 0.153.4다. 앞선 0.149.1 관측은 당시 증거이며 현 설치 버전으로 재인용하지 않는다. 로그인 schema 20파일 비교에서 Bedrock 인증 추가 2파일 차이가 있고 chatgpt 분기는 동일하다. `scripts/m5-codex-login-schema.mjs`로 생성 schema와 의존 타입을 오프라인 대조할 수 있지만 결과는 실제 provider/version 활성화 권위가 아니다.

- **로그인 불확실 종료 fence(2026-09-14, 내부 owner 미활성)**: app-data의 `codex-login.pending` 존재는 내용이 비었거나 손상돼도 새 내부 로그인 예약을 차단한다. 자동 삭제/기록 PID 재사용은 하지 않는다. 파일에는 프로세스 신원만 남기며 explicit cleanup의 tree 종료·root 회수·증거 저장 확인 전에는 해제하지 않는다. 제품 launcher/초기화/IPC·수동 복구 연결은 별도 남는다.

- **실제 agent 별도 로그인 승인(2026-09-14)**: 기존 사용자 터미널 설정/인증을 보존하는 앱 전용 환경에 별도 로그인하도록 구현한다. 인증정보 복사·공유 keyring 자동 fallback·유료 모델 smoke는 이 승인에 포함하지 않는다. 로그인 방식 선택은 더 이상 답변 대기가 아니며 실제 구현/검증과 구분한다.

- **압축 계획 해제 후 chat 검증(2026-09-14)**: plan ordinals 1..1000은 release 중 삭제되고 완료 후 anchor 1001만 남는다. release cursor 이하의 직전 행은 exact frozen source에서 검증해야 한다. plan 행 부재를 모든 단계에서 허용하는 fallback은 금지한다. 실제 product admission 실패는 plan_v9 transition 조회였고 구형 closure의 prefix 조회라는 초기 정적 추정은 이 실패 원인으로 확인되지 않았다.
- **편집 초안 복구(2026-09-14)**: 재읽기는 사용자 확인 시의 정확한 tab 객체와 완료 시 프로젝트/작업 복사본·차단 상태를 대조한 뒤에만 초안을 교체한다. 실패하면 보존하고 파일은 쓰지 않는다. Monaco의 프로그램적 내용 동기화도 change 알림을 내므로 같은 텍스트는 dirty로 바꾸지 않는다.
- **2026-09-14 실제 CLI 관측 경계**: Codex 0.149.1의 모델 없는 app-server 초기화/config/account 조회와 Claude 2.1.269 인증 상태 조회는 성공했다. 설정 격리·모델 가용성·actual provider 실행 권위는 아직 증명하지 못했다. 원문 인증/설정은 보존하지 않는다. 최신 후보 계약은 제안서 §6.7이다.
- **2026-09-14 작업대 배치**: 파일·편집기·변경·팀 대화의 가로 순서/폭은 사용자 조절 및 로컬 저장 가능하다. 전체 시안/상하 분할 도킹 완료는 아니며 실행 안전 계약은 그대로다.

- **G6 성능 도구와 실제 완료 경계(2026-09-14)**: `m5:measure:performance`는 공급된 측정 JSON만 판정하고 `m5:measure:sidecar-startup`은 현재 release sidecar의 생성된 빈 DB warm 시작을 직접 측정한다. `d97ee56`에서 10회 shell/storage p95 3.863/52.596ms PASS. `7832348`의 `m5:measure:renderer`는 합성 bridge·실제 Monaco로 대형 파일/diff·500개 페이지 탐색의 10회 측정을 통과했다(파일 표시 p95 75.9ms, 입력 32.3ms). 추가 `f4def74`의 `--memory` 실측은 Electron subtree idle 약 409.73 MiB와 한 회차 tree/diff 약 3.38초로 FAIL이며 원인 미확정이다. packaged startup·실제 stream·전체 제품 메모리·경합·종료 수집이 남아 있어 `m5:test:performance`와 G6는 미완료다. 입력 형식·측정 의미는 `procedures/m5-performance.md`를 따른다.

- **Astra 직접 개발 우선(2026-09-14 사용자 지시)**: 메인은 현재 사용자 선택 `gpt-6-astra`와 추론 설정을 따른다. 단일 기능은 직접 구현·검증하며 의무 역할 분할/worktree를 생략할 수 있다. 위임이 더 유리하거나 중요 안전 경계의 독립 검토가 필요하면 구현·관련 검증을 준비하고 이유·범위를 설명한 뒤 호출 전에 대기한다. 과거 의무 위임 규칙보다 AGENTS.md의 `개발 방식 선택`과 운영 절차 §1이 우선한다. 기존 서브에이전트 모델 설정은 보관 상태다.

- **부분 RSS 재측정(2026-09-14)**: idle에서 테스트용 5 MiB 텍스트 생성을 분리한 `dfe12bd`의 Electron subtree 측정은 idle/peak/60초 잔류 356.70/761.73/110.125 MiB와 GUI 시간 기준 PASS다. 실제 sidecar/agent는 제외하므로 전체 제품 RSS/G6 완료로 사용하지 않는다. 앞선 `f4def74` FAIL 증거도 유지한다.

- **G5D storage lifetime activation 정책(2026-09-08 사용자 승인)**: automatic lossless compaction의 유일한 trigger는 retained boundary 이후 exact safe prefix 1,000 ledger events다. 기존 안전 제외·완전 materialization·exact 전후 동등성이 선행하며 source event/index만 논리 삭제하고 사용자 chat/message/evidence, recovery, workspace, snapshot, baseline, projection, receipt와 물리 cleanup은 보존한다. empty-authority pending만 admission exact 검증 뒤 재개하고 running/non-empty/commit 불명은 변경 없이 fail-closed한다. ready 이후 치명 maintenance 오류는 wire 상태를 추가하지 않고 sidecar 종료로 다음 verifying admission에 넘긴다. 단일 event-loop SQLite writer, ready-only bounded tick, work 100/primary 1 MiB/retained+dependency 8 MiB를 유지한다. V10은 `authorizeDelete`를 read-only witness로 두고 `deletePrefix`의 exact armed interval에서만 완전 authority batch와 source index/event 삭제·authority cleanup·cursor 전진을 한 transaction으로 결속한다. V10 contract fixture만으로 runtime writer 활성화나 readiness를 주장하지 않는다.

- **큰 snapshot 처리와 제품 준비 시간은 별개(2026-09-03)**: 개발 중인 새 전역 경로는 실제 생산 101-workspace DB의 보존 복사본(607 source rows)을 172,685 calls/약 98.288초에 복원·ready 재검증했다. 30,000-call 테스트 guard 실패는 무진행이 아니라 예상 횟수 부족이었다. 이 내부 정확성 관측은 3초 제품 ready 충족이나 startup/admission 활성화가 아니다. 제품 시간·1 MiB fresh/8 MiB retained+fresh/100 work·100 frozen-operation fence는 유지한다. 실패한 생성 테스트와 보존본 replay 성공을 구분하고 원본 DB는 `/tmp/g5d-production-evidence.NX3udd/manifest.md`의 SHA로 고정한다.

- **큰 snapshot 새 경로와 제품 경계(2026-09-03)**: §16 private 동일 연결 전역 검증→index 선행 catch-up→ready coverage는 기존 V1/제품 verifier와 별도다. historical live/frozen workspace를 검증한 뒤 member별 COMMIT하며 durable partial offset 없이 source 처음부터 멱등 재시작한다. 실제 생성101의 보존 DB는 172,685 calls/98.288초에 완료해 3초 제품 목표를 충족하지 못한다. `ReadyVerified`를 제품 admission으로 인용하지 않으며 독립 리뷰·정상 생산 통합 QA와 제품 연결/성능은 별도다.
- **pnpm worktree 설치 재사용 함정(2026-09-03, 09-09 config surface 정정)**: bundled pnpm은 script 실행 전 기본 자동 install을 수행한다. 동일 lockfile이어도 workspace-state의 절대 project 경로가 다르면 shared node_modules symlink를 따라 root 설치본을 재생성할 수 있다. 이를 명시적으로 끌 때 repository `.npmrc`의 kebab-case key는 pnpm 11.25에서 무효였고, `pnpm-workspace.yaml`의 `verifyDepsBeforeRun: false`와 child `pnpm_config_verify_deps_before_run=false`를 함께 쓴다. 손상 시 잠금 파일 그대로 offline frozen install과 필요한 Electron 캐시 복구를 사용하며 네트워크 실패 반복·lock 변경으로 우회하지 않는다.
- **G5D QA evidence의 신뢰 경계(2026-09-09)**: harness evidence의 atomic leaf/inode checkpoint는 trusted local checkout 또는 isolated CI에서 accidental/substitution failure를 fail-closed로 기록하는 보강일 뿐 제품 저장 경계가 아니다. same-UID hostile parent rename race, publication 뒤 변조, signing/remote provenance는 이 helper가 보장하지 않으며 별도 보안/CI provenance 범위다.
- **큰 source catch-up 정책 경계(2026-09-03 승인)**: 기존 protocol batch/workspace source 검사·exact `single_source_payload_oversize == "contractError"`·storage-v2/v3 golden은 개별 source 1 MiB 초과 거부를 유지한다. 사용자는 §16의 새 분할 경로에 한해 호출당 1 MiB fresh/8 MiB retained+fresh/100 work로 큰 기록을 처리하는 확장을 승인했다. 기존 V1 계약을 덮어쓰지 않고 새 경로·중단/재시작 회귀로 구분하며 제품 활성화는 구현·검증 뒤 별도 완료한다.
- **G6 staging 경로(2026-09-03)**: Cargo target 위치는 환경·config·target triple에 따라 달라진다. staging은 build JSON의 해당 manifest/bin executable을 쓰며 root/target 또는 PATH fallback을 하지 않는다. 실제 artifact 없는 성공 exit만으로 이전 binary를 복사하지 않는다.
- **M5 G6 Electron main boundary regression(2026-09-03)**: `main.ts`의 registered callback은 VM-transpiled test로 검증한다. IPC는 current `BrowserWindow.webContents`의 exact main frame와 credential-free configured app/dev origin만 허용하며, rejected sender/frame/origin은 sidecar action 전에 실패한다. `app://m5` asset은 credential/port/query/fragment, encoded dot/slash/backslash, malformed percent, POSIX·Windows absolute path와 missing/non-file를 404로 거부하고 external fetch를 하지 않는다. 이 Node fake 증거는 실제 Electron/packaged renderer security, signing/fuses, Windows device 또는 전체 G6 gate가 아니다.
- **S3 한 행 호환성의 범위(2026-09-03)**: projection은 losing duplicate와 overlay 이전 값까지 문법 검증한 뒤 serde의 마지막 duplicate·루트 typed sequence와 exact-row SQLite의 첫 duplicate/evidence 표현을 각각 보존해야 한다. 긴 projection 숫자의 입력 길이를 S2 canonical 24-byte 출력 한계로 제한하지 않는다. S3의 재개 가능한 숫자 state/최대 33-byte 내부 Number 재구성 및 보존 parsed Number 예산은 이 구분을 지킨다. 한 행 의미/mirror 결과는 글로벌 source/provenance/admission 권위가 아니다.
- **실제 provider wire 재확인(2026-09-03)**: 로컬 Codex 0.149.1/Claude 2.1.258은 현재 fake exact manifest와 다르다. Codex app-server schema의 thread-start 유효 설정은 후보 증거이며 실제 runtime/설정 격리 검증이 아니다. Claude init의 MCP/plugin 객체와 fake `configIsolation` 차이를 별도 adapter에서 해결해야 한다. `--bare`는 구독 OAuth를 사용하지 않으므로 인증 재사용 대체로 임의 적용하지 않는다. 공식 출처·후속 경계는 `proposals/m5-coding-agent-ide.md` §6.7.
- **M5 G6 package-size 증거 도구(2026-09-03)**: `m5-package-metrics.mjs`는 macOS arm64의 unpacked ≤350 MiB와 compressed ≤200 MiB를 supplied directory/regular archive의 exact logical bytes로만 측정한다. symlink target은 세지거나 따라가지 않으며 Electron executable·`app.asar`·ASAR 밖 staged sidecar layout을 요구한다. Windows x64/arm64는 측정값을 내도 threshold 없는 `NOT_READY`다. filesystem walk는 atomic snapshot이 아니므로 metadata change를 발견하면 `NOT_READY`; report도 local-only이며 signing/freshness/runtime/G6 완료 증거가 아니다.
- **S2 canonical source는 의미/admission 증거가 아니다(2026-09-03)**: 새 scanner는 schema bootstrap을 제외한 호출당 fresh main+key 재읽기 1 MiB, retained+fresh 8 MiB, work 100을 유지한다. 전체 key를 보관하지 않고 adjacent source span을 decoded UTF-8 byte로 비교한다. pending decoded byte 뒤의 다음 조각도 함께 읽되 작은 잔여 예산을 필요한 반대편에 예약해야 교대 대량 재읽기/무진전 오류를 피한다. 숫자 24-byte는 현재 serde/zmij canonical 출력 image 근거이며 관대한 projection 입력 상한으로 재사용하면 안 된다. 관련 회귀/실측은 `storage/tests/reopen_canonical.rs`와 S2 evidence manifest에 있다.
- **S2 legacy oracle은 R3A 단독이 아니다(2026-09-03)**: 실제 정상 V8 fixture에서 indexed JSON duplicate의 첫 값이 틀리고 마지막만 맞거나 lock evidence key순서/escape가 한쪽만 다르면 private R3A는 허용해도 legacy SQLite mirror·Storage reopen은 거부한다. 반대로 동일 duplicate/정상 마지막 nonindexed 값/공백/typed sequence overlay는 수락된다. `storage::tests::reopen_oracles::s2_legacy_oracle*` 6개가 실제 DB 43사례를 고정하므로 S2 의미 동등성은 serde map과 SQLite mirror를 함께 보존해야 한다.
- **R3C-S1 구현 통합(2026-09-03)**: `00f54d9`의 private `RawColumnSession`을 관련 검증·단일 독립 리뷰 뒤 반영했다. 호출당 조각 1 MiB/retained+fresh encoded 입력 8 MiB로 읽고 UTF-8·길이·SHA만 완료한다. canonical/의미/provenance/admission 증거가 아니며 S2 이후 검증과 제품 연결·파동 누적 QA는 남는다. 현행 구조는 `architecture.md`, 원본 검증·소스 동일성은 `/tmp/g5d-r3cs1-evidence.ympdQJ/`를 확인한다.
- **간소화 운영 확정(2026-09-03 사용자 승인)**: 기능/결함 단위의 관련 검사와 독립 리뷰 1명을 기본으로 하고 누적 QA·최종 gate는 최신 통합 파동 마감에 수행한다. 수정 후에는 영향 범위만 재검증하며 신규 공용 계약 없는 helper마다 별도 설계 심의·전체 검증을 요구하지 않는다. 기존 계약의 중복 빈도/담당 배치만 대체하며 필수 안전 시나리오와 제품 활성화/출시 게이트는 유지한다. 상세는 `procedures/multi-agent-orchestration.md` §1.1이다.
- **Cargo 테스트 실행 파일 경로(2026-09-03)**: shared `CARGO_TARGET_DIR`에서 repo-relative `target/debug` 가정은 process-gate 19개를 실행 전 실패시켰다. test executable의 `<target>/<profile>/deps` 부모에서 sidecar를 찾고 `--lib` 테스트 전 같은 worktree/target/profile의 명시적 bin build로 freshness를 확인한다. 경로 수리 뒤 원래 실패 19개와 추가 locator 회귀가 통과했다. 이는 S1·전체 G5D 완료와 구분한다.
- **QA 저부하 즉시 적용(2026-09-02 사용자 승인)**: 고부하 Cargo/sidecar 테스트를 사용자 요청으로 종료했고 다음 실행은 하네스 및 직접 Cargo 모두 compile/test 병렬 최대 4로 제한한다. 무거운 검증은 컴퓨터당 하나만 실행한다. focused 회귀와 통합 마감 누적 gate를 구분하되 필수 안전 검증은 생략하지 않는다. 낮은 우선순위는 best-effort이며 CPU 절대 상한은 아니다. 자세한 실행 규칙은 `procedures/multi-agent-orchestration.md` §13.1이다.
- **S2 oracle 분리(2026-09-02)**: legacy product는 canonical source/replay, serde projection 의미 비교, SQLite mirror를 모두 요구한다. R3A는 parsed final map/evidence를 비교해 첫 duplicate를 보는 SQLite 추출과 evidence `json(...)` 표현 비교를 그대로 재현하지 않는다. primitive 차이와 source 대조는 했지만 정상 V8 fixture의 전체 acceptance 재현은 아직이다. 기존 제품 검사는 유지하며 §13의 세 oracle·후속 회귀를 닫기 전 R3A 단독 결과를 제품 동등성으로 인용하지 않는다.
- **Cargo test fixture binary 경로(2026-09-03)**: `chat::tests`가 repo-relative `target/debug`를 가정하면 shared `CARGO_TARGET_DIR`에서 process-gate 19개가 실행 전 실패한다. test executable의 `<target>/<profile>/deps` 부모에서 동일 profile의 sidecar만 찾고, `--lib` 검증 전 같은 worktree/target/profile의 명시적 bin build로 freshness를 확인한다. 관련 수리는 테스트 전용이며 기존 S1 미완료/full exit 143과 구분한다.
- **R3C-S1 raw bytes는 의미 검증이 아니다(2026-09-02)**: 새 private bytes session은 고정 source 주소/expected identity/epoch/byte 길이 아래 fragment별 UTF-8와 SHA를 끝까지 계산한다. ≤1 MiB fresh fragment + ≤8 MiB retained/fresh encoded 입력이며 start source metadata도 예산에 포함한다. schema bootstrap은 같은 BEGIN 안의 별도 비용이고 SQLite slicing 내부 메모리/전체 ready 시간 상한은 미입증이다. 빈 TEXT의 SQLite substr NULL과 retained UTF-8 carry 때문에 다음 호출이 무진행이 되는 경계를 회귀로 고정했다. §10 큰 정상 run source의 raw transport 성공은 기존 R3B 거부 해제나 canonical/admission 성공이 아니므로 S2 및 제품 연결 계약을 생략하지 않는다.
- **R3C 큰 기록 처리 순서(2026-09-02)**: V8 byte transport → canonical oracle 동등성 → projection/파생 의미 비교 → 동일 연결의 쓰기 권위 순서로 나눈다. §12 S1은 fragment 1 MiB/retained+fresh 8 MiB 내 UTF-8·길이·해시 정보만 제공하며 기존 verifier·제품 fence·입력 정책을 바꾸지 않는다. historical workspace catch-up은 source sequence 없는 row 때문에 단순 latest-only 조건 변경으로 해결하지 않는다. 상세 계약은 `proposals/m5-storage-reopen-verification.md` §11–12다.
- **개발 worktree 정리(2026-09-02)**: 완료 기능의 통합·최종 QA/gate 직후, 다음 worktree 생성 전에 정리를 점검한다. branch SHA/통합 동등성·dirty/untracked/ignored 증거·사용 process를 확인하고 필요한 증거를 외부 보존한 뒤 승인 범위에서 비강제 제거한다. branch/commit은 보존하며 진행·미확인 작업은 유지한다. 최초 정리에서 58개를 제거하고 18개를 유지했다. 상세는 `procedures/multi-agent-orchestration.md` §5.1과 `.artifacts/worktree-cleanup/2026-09-02/approved-plan.json`이다.
- **R3C snapshot 호환성 재현(2026-09-02, 테스트 전용)**: 정상 public text/event/누적 snapshot 예산을 통과한 ASCII 16 × 64 KiB 및 역슬래시 9 × 64 KiB의 일반 run+chat atomic writes와 Storage reopen은 성공하지만, 저장된 run canonical BLOB이 1 MiB를 초과해 전체 private R3B ledger 검증은 거부한다. 작은 16-event 대조군은 전체 완료한다. exact test `agent_runtime::budget_tests::budget_valid_actual_run_snapshot_exceeds_reopen_primary`가 양 경로와 exact source row byte preflight/무변경을 구분해 고정한다. provider-less pre-G5C readable shape일 뿐 현재 provider 검증이나 제품 admission 증거가 아니며, §9/§10의 저장 표현·검증 단위·기존 기록 호환 정책을 정하기 전 제품 연결/예산 변경은 하지 않는다.
- **R3C 제품 연결 전 호환성(2026-09-02)**: 기존 actual-agent의 정상 public text 최대량과 metadata를 담은 단일 run snapshot은 private verifier의 1 MiB primary cap을 넘을 수 있다. project의 prepared-workspace map도 abandoned를 보존하며 전체 개수/byte cap 없이 누적되는 별도 정상 생산 경로다. 또한 pending의 raw-source 읽기 검증은 chat mutation의 indexed predecessor 요구를 대체하지 않는다. snapshot/이전 기록 호환과 검증 후 index catch-up·복구 순서를 별도 계약/fixture로 닫기 전 제품 admission을 활성화하거나 기존 100-operation fence를 해제하지 않는다. §10의 run persistence/Storage reopen/private R3B 재현은 `73b2d61`의 독립 통합 QA와 최종 gate까지 통과했다. project 경계 재현·Sidecar startup/recovery·제품 활성화는 미실행이다. 상세는 `proposals/m5-storage-reopen-verification.md` §9다.
- **R3B 후속 호환·유일성(2026-09-02)**: pending에서 index/normalized message가 없더라도 메시지 도입 ID 전역 유일성은 기존 protocol 요구다. 이를 포함한 §8 내부 R3B는 `e5f9f01`에서 독립 통합 QA·최종 gate를 완료했다. historical pending workspace의 읽기 검증과 실제 catch-up은 별개이며 현행 bounded workspace materializer의 갱신 전 latest-only source 대조는 정상 과거 row도 거부할 수 있다. 이 미해결 catch-up과 R3C 호환성은 `proposals/m5-storage-reopen-verification.md` §7.5·§9, 테스트 전용 재현 계약은 §10을 따른다.
- **pnpm 복원과 누적 검증 환경(2026-09-02)**: stale node_modules metadata만으로 설치 성공을 판단하지 않는다. frozen lockfile/ignore-scripts/공급망 정책을 유지한 복원 뒤 하네스와 동일한 `CI=1`로 확인한 build·누적 `g5d-bounded-storage`가 `d4c3751`·`63ab510`에서 PASS했다. 격리 환경의 Electron runtime fetch 실패와 더 넓은 core/protocol lint 실패는 scoped 성공과 별도 기록한다. 자동 install/CI 정규화 차이는 관찰했지만 package 소실 원인을 전부 입증한 것은 아니므로 임의 설정 변경이나 정책 완화로 해결했다고 주장하지 않는다.
- **R3B 내부 derived-record 경계(2026-09-02)**: private 정보성 mode는 R2/R3A 뒤 전체 derived table과 ready 양방향 coverage를 검증한다. index-empty pending receipt/baseline은 raw tuple을 먼저 찾고, indexed 여부와 무관하게 모든 live introduction을 frozen identity와 비교한다. 추가 global raw scan은 unindexed introduction에만 필요하다. historical pending workspace 수락은 기존 catch-up 결함 해결이 아니다. retained outer+fresh dependency가 새 호출에도 8 MiB를 넘으면 terminal failure지만 이번 호출의 이미 소비한 입력 때문에 넘는 경우는 Pending일 수 있다. 실패 batch 안에서 검증한 행과 COMMIT된 진행을 테스트에서 혼동하지 않는다.
- **R3A 내부 ledger/projection 경계(2026-09-02)**: compacted DB는 R2 closure와 R3A를 같은 세션/변경 감지 epoch에서 검증한다. pending의 누락 source index는 exact live predecessor에서 필요한 사실을 구성해 허용하지만 존재하는 index는 항상 결속하며 ready의 누락은 거부한다. latest source와 다섯 projection의 양방향 일치는 global derived-record/R3B 또는 startup/admission/R3C 완료 증거가 아니다. phase-local typed app-instance 1 MiB는 canonical JSON source 1 MiB와 동일한 크기 정의가 아니므로 용량 positive-control을 혼동하지 않는다.
- **R3 startup/admission의 추가 경계(2026-09-02)**: pending materialization은 일부 파생 기록 미생성이 정상이며 ready는 검증 의무이지 proof가 아니다. 현행 startup의 복구 쓰기보다 앞선 read-only semantic preflight가 필요하다. SQLite 전체 integrity/FK 검사와 Electron ready 3초는 batch 예산과 별개다. 둘을 몰래 완화하지 않고 R3A/B/C와 사용자 결정이 필요한 변경을 `proposals/m5-storage-reopen-verification.md` §7에 분리한다.
- **R2 내부 closure 구현 경계(2026-09-02)**: `start_compaction_closure`의 3-phase 결과는 admission이 아니며 product >100 거부를 해제하지 않는다. 각 행의 고정 dependency graph 전체를 먼저 byte-preflight하므로 뒤늦은 oversized dependency도 무한 Pending으로 남지 않는다. TeamMessage 64 KiB text cap과 메시지 단계의 1 MiB 합계 예산을 혼동하지 않는다. 후기 corruption fixture는 frozen message 삽입 순서를 source rowid로 고정해야 실제 100행 이후 위치를 보장할 수 있다.
- **R2 내부 검증 구현·범위(2026-09-02)**: `9327bcd`의 같은 connection 세션이 operations→prefixes→frozen messages를 다회 처리한다. 삭제 frozen source↔정확한 sequence prefix/baseline, 삭제 chat introduction↔frozen message↔해당 normalized message가 대상이다. 참조되지 않는 정상 historical baseline을 금지하지 않으며 전체 derived-table replay와 제품 admission은 R3에 남긴다. chat의 동일 durable sequence는 null-or-same snapshot이 허용된다는 protocol 규칙을 보존한다. 상세 계약은 `proposals/m5-storage-reopen-verification.md` §6, 현행 구현은 `architecture.md`를 확인한다.
- **V8 operation 검증 전구체는 admission이 아니다(2026-09-02)**: `storage::reopen` 내부 세션은 mutable connection borrow + 매 batch의 transaction-under-lock `data_version` 비교로 frozen operation만 다회 검증한다(100행·canonical 1 MiB·별도 raw closure 8 MiB). 완료·취소·실패는 재사용하지 않으며 durable cursor/proof를 쓰지 않는다. prefix/message/replay/integrity와 product activation은 별도 게이트이고 제품의 100개 초과 재오픈 거부는 유지한다. 같은 프로세스라도 별도 SQLite 연결의 commit은 외부 변경이며 production exclusive-lock 정책을 느슨하게 바꾸지 않는다.
- **M5 G5D storage lifetime(2026-09-01, exact schema v8)**: 실제 provider 활성화 전 SQLite 전체 수명 stop condition을 G6에서 분리해 G5D로 진행한다. exact predecessor schema를 mutation 전에 검증하는 additive v1→v8 chain은 원장과 기존 compaction history를 보존한다. V6는 source event에 exact 결속된 append-only `event_source_index`와 16단계 lifetime cursor를 두고, V7/V8은 future compaction prefix·message·operation provenance와 sidecar-private completion proof를 추가한다. 명시 호출형 worker는 모든 source/table scan을 rowid keyset `LIMIT 100`, canonical payload 합계 1 MiB로 제한하며 cancellation·commit 불확실성을 fail-closed로 보존한다. terminal compacted V8 read/replay는 frozen operation과 receipt·baseline·retained anchor를 교차 검증해 duplicate/conflict·chat predecessor·후속 append/materializer 의미를 유지하지만, 현재 frozen operation 100개까지만 재오픈하고 101번째는 무변경 거부한다. 실제 compaction writer/delete, 100개 초과 durable reopen proof, startup/background scheduling, 제품 wire/status, checkpoint/VACUUM, cleanup·retention threshold는 아직 비활성이다
- **M5 G5C actual recovery 원자성(2026-08-27)**: recovery ID는 run 준비 시 writer lock과 같은 transaction에 random UUIDv4로 예약해 모든 후속 recovery가 재사용하며 run ID에서 파생하지 않는다. process identity 없는 재시작은 `processIdentityUnavailable` unresolved recovery와 poisoned lock을 공개하되 resolve로 폐기할 수 없다. actual abandon은 SQLite intent → exact/idempotent manifest abandon → manifest 재검증 뒤 atomic DB finalize의 restart-reconcilable saga이고, 어느 단계든 identity/digest가 다르거나 상태가 불명확하면 writer를 해제하지 않는다. SQLite와 filesystem의 원자성을 주장하지 않는다
- **M5 G5C provider 활성화 전 계약(2026-08-27)**: provider manifest는 executable binary만이 아니라 필수 helper까지 포함한 closure digest, exact model/reasoning/permission tuple, provider별 auth/event parser ID, managed cwd·no-secret env·no-fallback policy를 canonical digest로 묶는다. persisted discovery에는 raw stdout/stderr·token·prompt·email·org·raw auth hash를 넣지 않으며, attestation은 accepted run/profile의 run·fingerprint·workspace·runtime·model·reasoning·permission을 exact 결속하고 derivation이 정의되지 않은 session digest는 저장하지 않는다. Claude는 `system:init`의 version/model/cwd/permission/tools/MCP/plugins/config를 전부 handshake·runtime attestation으로 검증한 뒤에만 public event를 허용하고, internal Public도 기존 G5B 256-event·1 MiB text budget 안에서만 허용한다. Codex가 argv 또는 unknown catalog만 제공하면 `canStart=false`다. 이 계약은 fake-only internal boundary이고 공개 G5/G5A/G5B wire·V4/V5를 바꾸지 않는다. SQLite 전체 lifetime/retention/compaction은 실제 provider 활성화 전 별도 stop condition이다
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
- **M5 G5A actual-agent 경계(2026-08-27)**: 기존 G5 fixture ledger/handshake는 불변이고 actual 실행은 additive Bridge V4의 별도 `agentRuns` ledger와 typed start/cancel/recovery 계약을 사용한다. renderer는 경로·argv·model·env·cwd·prompt를 주입할 수 없고 sidecar가 immutable chat role snapshot과 SHA-256 capability fingerprint로 exact profile을 해석한다. machine-readable 또는 exact-version 실측 capability가 아니거나 effective model 증거가 `argvOnly`이면 `canStart=false`다. v1은 one-shot/`nextRun`만 허용하며 process identity release barrier, 모든 자손 소멸, permission별 writer-lock 종료, 공개 `status/question/final`, 불확실 종료의 poisoned recovery를 TypeScript/Rust 공용 golden이 fail-closed 검증한다. 전용 `--stdio-agent-fixture`의 fake launcher→normalized `agent.publicEvent`→Electron fixed V4 IPC→renderer durable `chat.get` hydration 수직 흐름은 구현됐고 raw/private carrier와 legacy/actual recovery 원장은 분리된다. 일반 제품 `--stdio`는 exact G5를 유지하므로 실제 Claude Code/Codex CLI·인증·모델 출력은 아직 미연결·미검증이다
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

### 2026-09-01 기존 엔트리 → `docs/archive/history-2026-09.md`

2026-09-01 엔트리를 날짜 경계로 아카이브했다. V8 provenance·append fence·제한된 재오픈과 남은 제품 활성화 게이트는 위 `핵심 결정·발견`에 유지했다.

### 2026-08-28 및 그 이전 → `docs/archive/history-2026-08.md`

2026-08-12 ~ 2026-08-28의 기존 엔트리를 날짜 경계로 아카이브했다. G5C recovery 원자성·G5D 저장소 계약과 Electron 시작 복구 등 재사용할 결정은 위 `핵심 결정·발견`에 반영했고, M4의 현재 결론은 `docs/proposals/m4-orchestration.md`가 기준이다.

### 2026-07-23 및 그 이전 → `docs/archive/history-2026-07.md`

17개 엔트리(2026-07-16 ~ 2026-07-23)를 아카이브로 옮겼다. 재사용할 결정·함정은 위 `핵심 결정·발견`에 반영돼 있으므로, 통상 아카이브를 열 필요는 없다.

2026-09-02 워크로그 전체는 [월별 아카이브](archive/history-2026-09.md)에 보존했다.

2026-09-03 워크로그와 2026-09-08의 오래된 일부 엔트리는 [월별 아카이브](archive/history-2026-09.md)에 보존했다.

2026-09-08~09의 나머지 워크로그 전체도 [월별 아카이브](archive/history-2026-09.md)에 보존했다.
