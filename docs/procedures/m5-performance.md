# M5 성능 측정

`--diagnostic --memory --heap-sampling`은 합성 renderer에만 CDP 할당 샘플러를 붙인다. 평균64KiB 간격·깊이32로 수집하고 checkpoint마다 살아 있는 할당의 상위50개 호출 위치와 전체 샘플 크기를 기록한다. [공식 HeapProfiler 계약](https://github.com/ChromeDevTools/devtools-protocol/blob/master/pdl/js_protocol.pdl)의 기본 live-object 모드이며 GC 강제·힙 스냅샷은 호출하지 않는다. 샘플 크기는 RSS나 retained size가 아니며 minified 함수명/번들 위치의 추가 분석이 필요하다. 계측 오버헤드가 있으므로 정식 성능 PASS 근거로 사용할 수 없다.

합격선은 `../proposals/m5-electron-spike.md` §10이다. 다음 도구는 로컬 개발 측정용이며 전체 G6 완료를 뜻하지 않는다.

## 실제 프로젝트 UI 회귀 (성능 측정과 별도)

### 앱 전체 재시작 후 저장 채팅 확인

`node_modules/.bin/electron scripts/m5-project-reopen-smoke.cjs <앞선 project-ui 시험 폴더>`는 별도 Electron 프로세스로 이전 합성 시험의 userData를 다시 연다. OS 임시 폴더 바로 아래의 `hello-agent-project-ui-*`이며 앞선 `result.json`이 PASS/종료 accepted이고 원본이 복원된 경우만 받는다. 임의 사용자 프로젝트나 실제 앱 데이터 경로를 주지 않는다.

저장 프로젝트를 선택해 `Improve hello`와 `Second task`의 작업 복사본을 Monaco로 다시 열고 각각 저장된 편집/원래 내용이 유지되는지 확인한다. 새 폴더 선택·에이전트 실행은 하지 않는다. `reopen-result.json`은 정상 종료 수락 뒤에만 PASS가 되며 `reopen.png`를 함께 보존한다. 이는 정상 종료 후 프로젝트/채팅/파일 영속성 시험이며 강제 종료 복구, 실제 계정 로그인 유지, 실행 중인 에이전트 재시작의 증거는 아니다. 최신 `.vite/build`와 debug sidecar를 먼저 준비한다.

### 생성부터 편집·반영·복원까지

macOS에서 현재 checkout의 main/preload/renderer `.vite/build`와 native sidecar를 먼저 빌드한 뒤 `pnpm m5:smoke:project-ui`로 실행한다. 스크립트 자체는 빌드하지 않으므로 오래된 산출물의 PASS를 최신 코드 증거로 쓰지 않는다. 실행할 때마다 임시 Git 프로젝트와 독립 userData를 만들며 사용자 프로젝트·선택된 에이전트·인증 정보를 사용하지 않는다. 실제 Electron/native sidecar를 통해 채팅 생성, Monaco 편집/저장, diff, 원본 불변, 반영 확인 취소, 두 채팅 사본 분리를 확인한다.

- `HELLO_AGENT_SMOKE_APPLY=1 pnpm m5:smoke:project-ui`: 스크립트가 생성한 프로젝트만 명시 확인 UI로 적용 후 복원한다.
- `HELLO_AGENT_SMOKE_NATIVE_INPUT=1 pnpm m5:smoke:project-ui`: 잠금 해제된 Mac에서 시험 창을 직접 활성화하고, `WAITING_FOR_NATIVE_INPUT` 뒤 편집기에 `edited in snapshot`을 입력한다. 포커스/입력을 각각 최대45초 기다리며 자동 insertText를 사용하지 않는다. 결과의 `inputMethod`로 기본 자동 삽입과 구분한다. 화면 잠금/입력 미확인은 실패로 보존하며 시험이 창을 활성화하지 못했다는 사실만으로 제품 입력 결함이라고 단정하지 않는다.
- 위 설정에 `HELLO_AGENT_SMOKE_RELOAD=1`을 함께 주면 적용 후 renderer reload→저장 프로젝트/채팅 재선택→복원 경로를 포함한다. 전체 앱 재시작 검증은 아니다.
- 시작 화면의 빈 dock 부재, 목표 예시의 입력만 채우기/기존 초안 보호, 역할 설정 modal의 Escape→계속 편집→폐기·닫기/포커스 복원, 저장 후 재열기 및 원래 모델 복원도 같은 실행에서 확인한다. `START_AND_SETTINGS`는 중간 단계이며 마지막 PASS가 전체 시나리오 결과다. 저장 실패 주입과 메시지 스크롤은 별도 회귀가 필요하다.
- 이 시각 검증은 앱 창을 표시하고 포커스를 가져온다. macOS에서는 window가 visible이어도 renderer가 hidden인 상태가 관측돼 Monaco 표시/캡처가 멈춘 적이 있어, 재진입 편집 전 창을 다시 활성화한다. `SECOND_CHAT_WINDOW`의 창/페이지 가시성 진단과 renderer 오류를 함께 기록한다. 제품의 background 정책은 변경하지 않는다.
- PASS/FAIL JSON과 screenshot 경로를 출력하고 실패는 종료 코드1이다. 임시 프로젝트/userData는 성공·실패 모두 증거 확인을 위해 보존하며 자동 삭제하지 않는다. 이 실행은 실제 공급자·Windows·G6 합격을 대신하지 않는다.

## Native UI 작은 프로젝트 메모리 진단

### Native 대형 diff 진단

`HELLO_AGENT_SMOKE_LARGE_DIFF=1 pnpm m5:smoke:project-ui`는 별도의500파일/20000변경 줄/10MiB Git diff 시나리오다. 임시 원본을 commit하고 앱이 만든 유일한 격리 snapshot의 fixture 파일만 변경한다. Git patch의 실제 byte/변경 줄/파일 개수를 검증하며 원본 Git 상태가 깨끗한지 확인한다. 실제 sidecar와 renderer를 거친 변경 목록·첫 파일 두 frame 관측을2000/1500ms와 비교한 뒤 정상 종료한다. 이 모드에서는 기존 두 채팅/반영 흐름을 실행하지 않으며 다른 대형/메모리/원본 적용 옵션과 결합하지 않는다. 결과는 `largeDiff`, `scenario:nativeLargeDiffOnly`로 구분한다. 단일 개발 빌드 진단이며 packaged/반복 p95/G6 합격을 뜻하지 않는다.

### Native 프로젝트 반복 수집

대형 diff 진단은 시간 측정 뒤500개 파일을100개씩 끝까지 탐색하고, 대문자 검색·결과 없음·Escape 해제·선택 상세 보존·편집기 진입과 새 diff 응답 뒤 복귀를 확인한다. 결과의 `navigation`에 기록한다. 이 추가 UI 흐름은 앞선 diff 시간 측정 구간 밖이며 실제 계정이나 자동 저장을 사용하지 않는다.

`pnpm m5:measure:native-project --diff`는 별도 대형 diff 시나리오만 준비1회＋측정10회 수집한다. 각 실행의500파일/20000변경 줄/10MiB 증거와 실제 IPC handler 시간(대기 포함)을 보존하며 `diffSummaryMs`·`diffFirstFileMs`의 p95를 계산한다. 트리/큰 파일/메모리 모드와 혼합하지 않는다. 정상 종료한 기준 초과 표본도 버리지 않으며 아래와 같은 diagnostic/NOT_READY 경계를 따른다.

`pnpm m5:measure:native-project`는 현재 빌드된 native UI 시험을 준비 실행1회＋측정10회 순차 실행한다. 각 실행은 새 임시 프로젝트에1만 파일 트리와5MiB 파일을 함께 생성하며, 편집/저장/diff·반영 확인 취소·두 채팅 흐름까지 수행한다. 원본 적용·reload·메모리 환경 옵션은 상속하지 않는다. 빌드는 자동 수행하지 않으므로 먼저 최신 main/renderer/debug sidecar를 준비한다.

저장공간 관리: native 반복 collector는 앱 종료와 `shutdown=accepted`, 저장된 결과 일치를 확인한 뒤 각 합성 시험의 `project`/`userdata`만 정리한다. 첫 실패 데이터 한 건은 재현용으로 남긴다. 모든 회차의 JSON·로그·스크린샷과 FAIL 판정은 보존하며, 삭제를 거절하거나 실패하면 다음 앱을 시작하지 않는다. 이 정책은 실제 계정 시험, 제품 백업, 별도 renderer 측정기에는 적용하지 않는다.

결과는 새 OS 임시 `hello-agent-native-metrics-*`의 measurement.json과 각 trial 로그에 남긴다. 실제 앱 종료를 기다린 뒤 다음 실행을 시작하며 종료 미확정/비정상 종료면 중단한다. 강제 종료나 증거 삭제는 하지 않는다. 정상 종료한 실패 표본도 제외하지 않고10회에 포함하며, 누락 수치의 p95는 null이다. 준비 실행 실패도 전체 FAIL로 남긴다.10개 값의 nearest-rank p95는 최대값이다. 기준 초과/흐름 실패는 exit1, 모두 통과해도 개발 빌드 진단이므로 NOT_READY/exit2다. 이 명령은 packaged·전체 RSS·stream·save race·종료 안전성 측정을 대신하지 않으며 `m5:test:performance` 게이트를 열지 않는다.

`HELLO_AGENT_SMOKE_LARGE_TREE=1 pnpm m5:smoke:project-ui`는 실제 임시 Git 프로젝트에 1,000폴더/10,000파일의 `aaa-tree` 하위 트리를 추가한다. 기존 시험용 hello.txt 및 선택적 large.txt는 이 숫자 밖이며 `controlFiles`로 별도 기록한다. 실제 sidecar→파일 패널에서 하위 트리 첫 페이지와 한 폴더를 열고 두 animation frame까지 관측한다. `largeTree` 결과에 시간·가상 목록 DOM 수를 보존하고 1,000/250ms·500노드 초과를 실패 처리한다. 원본 snapshot 생성 시간은 treeRootMs에 포함하지 않는다. 단일 개발 실행·첫 페이지 진단이며 packaged/10회 p95·전체 탐색/G6 합격을 대체하지 않는다. 큰 파일/메모리 옵션과 결합할 수 있다.

대형 트리 모드의 최초 snapshot 및 새 채팅 생성 관측은 최대65초, 전체 시험은5분으로 제한한다. 이는 시험이15초에 먼저 quit을 요청하던 문제를 피하기 위한 관측 창이며 제품의60초 snapshot timeout이나 트리 성능 기준을 늘리지 않는다. 최초 프로젝트 선택→트리 표시 시간은 `SNAPSHOT_READY` 및 `largeTree.snapshotReadyMs`로 별도 보존한다.

### 패키징 앱 외부 관측

`--native-chat-stream`은 자체 작은 프로젝트의 실제 새 대화 UI로 격리 채팅4개를 만들고, `appendChatMessage`를 채팅별20ms 예정 간격·3000개씩 호출한다. 실제 preload/main/sidecar/DB를 통과한 `onChatEvent`의 ID·순서·채팅 구분과 bounded `getChat` 페이지의3000개 영속 기록을 대조한다. 초기 goal 이벤트는 DB에서 읽은 exact ID/내용만 workload에서 구분한다. CAS 계약 때문에 채팅 안에서는 순차 호출하며 지연을 재시도로 숨기지 않고 실제 dispatch duration/최대 지연을 기록한다. 예정50회/초는 달성률 보장이 아니다. 유료 모델·agent.publicEvent는 사용하지 않으며256개 실행 이벤트 상한을 변경하지 않는다. 기존 정상 종료/DB 표식 확인 경로를 사용하고 실패 시 부분 결과를 보존한다.

첫 채팅을 실제 선택한 상태에서 DOM 본문 확인 뒤 두 RAF를 관측해3000개 event→paint/dispatch→paint 표본을 저장하고 event→paint 최대100ms를 검사한다. 실제 compositor 타임스탬프가 아니며, 나머지 채팅은 전환 후 마지막200개 본문을 대조할 뿐 background paint를 주장하지 않는다. 전체 수명RSS·분리 자손·반복10회가 아직 없어 전체G6 합격에 사용하지 않는다.

`--deferred-cdp`는 디버깅 client 연결 전48개 RSS 표본(250ms 간격)을 별도로 수집한 뒤 기존 관측/정상 종료를 수행하는 원인 분리 진단이다. CDP server 자체는 처음부터 켜져 있으므로 완전한 무계측 비교가 아니다. `preAttachmentMemory`는 UI 준비 상태를 확인하지 않은 시작 표본이며, 연결 후 지연된 idle 값으로 기존 초기 peak나 실패를 대체하지 않는다. 전체G6 합격에 사용하지 않는다.

프로젝트 시나리오는 측정 전에 CDP로 시험 창을 앞으로 가져오고 `visibilityState=visible`을 확인한다. 측정 중 숨김 전환은 관측 실패로 남기며 같은 표본을 재시도하지 않는다. 제품 background throttling이나 프레임 deadline은 변경하지 않는다. 시험 창이 앞으로 표시되므로 사용 중인 다른 창과 포커스가 경쟁하면 실패할 수 있다.

반복 패키지 수집기는 각 trial의 `rendererLongTasks` 원시 기록과 최대값·200ms 초과 개수를 대조하고, 본10회의 `rendererLongTaskMs` 최대값을 집계한다. 관측 미지원·미완료·overflow·요약 불일치는0으로 대체하지 않고 중단한다. 완전 관측한 기준 초과는 실패 표본으로 유지한다. 이 관측은 프로젝트 시나리오 구간만 포함하며 앱 시작·main process long task나 전체G6 증거를 대신하지 않는다.

`pnpm m5:smoke:packaged-ui --startup`은 새 임시 프로필의 최초 실행을 `result.json`에 남기고, 동일 프로필에서 준비1회+측정10회를 `startup-0.json`~`startup-10.json`에 기록한다. 합계는 `startup-series.json`이다. 각 앱은 종료0·DB 종료 표식·관측 PID 부재를 확인한 뒤 다음 앱을 시작한다. HEAD/워킹 트리 상태와 실제 패키지 산출물이 도중에 바뀌면 중단한다. 완전 관측 후 종료가 확인된 자체 userdata만 정리하며 JSON/PNG를 보존한다. 앱당 idle RSS 관측10초가 있어 총2분 이상 필요하다.

`storageObservedMs`/`uiObservedMs`의 본10회p95와 idle RSS 최대값을 기록한다. CDP 연결·poll 지연을 포함한 관측 시각이므로 `sidecarReadyMs`/`appInteractiveMs`로 이름을 바꾸거나 정확한 제품 준비 시간으로 사용하지 않는다. 최초 실행은 migration을 포함하고 반복 실행의 warm 명칭은 OS 캐시 상태를 보장하지 않는다. 한도 초과는 FAIL로 보존하며 이 시작 전용 진단만으로G6 PASS를 내지 않는다.

`node scripts/m5-packaged-performance.mjs --diff` 또는 `--tree-file`는 같은 packaged 관측기를 순차11회 실행한다(준비1회+본10회). HEAD와 실제 asar/sidecar 지문을 대조하고 본 측정10값의 nearest-rank p95(최대값)를 기록한다. 정상 종료와 관측 PID 부재를 확인한 trial만 자체 project/other-project/userdata를 정리하며 JSON/PNG/로그는 보존한다. 불완전 관측·불확실 종료는 중단하고 다음 앱을 시작하지 않는다. 확인된 개별 지연/메모리 초과는 전체 FAIL로 유지하며, 모든 수치가 기준 이내여도 이 부분 수집기는 NOT_READY다. migration·CDP 등 기존 한계와 누락 G6 항목은 그대로다.

`node scripts/m5-packaged-performance.mjs --memory-diff`는 같은 순차11회 방식으로 대형 diff 열기→프로젝트 닫기→60초 대기 후 메모리를 수집한다. 각 trial의 원시 RSS 표본에서 idle/peak/retained를 다시 계산해 보고값과 대조하고, 본 측정10회의 **maximum**을 기록한다(p95 아님). 각 trial의 수명 전체에서 관측한 PID가 모두 종료됐는지 확인한 뒤 자체 fixture를 정리한다. 60초 대기만 총11분 이상 필요하므로 다른 무거운 시험과 동시에 실행하지 않는다. 현재 자손의250ms 표본이며 탈출 프로세스·표본 사이 peak·다른 대형 fixture 동시 부하는 포함하지 않는다. 전체 G6 collector나 합격 증거로 취급하지 않는다.

`--tree-file`의 `inputPaintMs`는 각 앱에서 예열 입력1개를 제외한10개 입력의 최대값을 사용한다. 본10앱의 최대값 배열을 다시 집계하며 누락/비정상 표본이나 요약 불일치는 거절한다. 입력 전 DOM 감시를 설치하고 trusted beforeinput·실제 표시 텍스트 이후 두 RAF를 관측한다. 물리 compositor/키보드/IME 증거는 아니며, 입력 후 감시를 시작하던 과거 측정과 직접 제품 속도 개선으로 비교하지 않는다. 50ms 초과도 정상 종료했다면 실패 표본을 보존하고 다음 회차를 진행한다.

`pnpm m5:smoke:packaged-ui --large-diff`는 기존 native diff fixture의500파일/20,000변경 줄/10MiB patch를 자체 생성 프로젝트의 격리 작업 사본에만 만든다. 실제 변경 패널의 새로 고침→500개 요약→첫 파일 표시를 renderer 시계로 측정한다. 기준2000/1500ms, 실제 Git patch 크기/줄수와 원본 Git 상태 불변도 확인한다. 단일 진단이며 warm-up/10회p95를 대체하지 않는다.

`pnpm m5:smoke:packaged-ui --large-project`는 같은 실제 패키지 경로에10,000파일/1,000디렉터리 subtree와 기존5MiB/100,000줄 파일 fixture를 생성한다. 두 control 파일은 별도다. 프로젝트 snapshot 준비 관측은65초로 제한하고 제품60초 제한은 변경하지 않는다. renderer performance.now로 트리 진입/하위 폴더/파일 입력 요소+본문+두 frame까지 측정하며 각각1000/250/1500ms, 트리 DOM500개 기준을 적용한다. 부모 관측기의 snapshotObservedMs는 별도 값이다. 단일 migration 포함 진단으로 warm-up/10회p95·전체 입력/stream/RSS peak/최종G6를 대체하지 않으며 기준 초과를 FAIL로 보존한다. 이 모드는 편집·반영·재시작을 반복하지 않는다.

프로젝트 모드는 고유 파일을 가진 두 번째 임시 Git도 등록한다. 재시작/반영/복원 뒤 프로젝트 화면을 닫아 두 번째 프로젝트를 열고 파일·채팅 비혼입 및 원본 불변을 확인한 후 첫 채팅으로 돌아온다. 모든 project ID는 실제 API 결과를 사용하며 경로/목록을 mock하지 않는다. 이는 순차 전환 증거이며 동시 에이전트의 교차 프로젝트 안전 시험을 대신하지 않는다.

프로젝트 시나리오는 첫 실행에서 편집/저장/diff·정상 종료가 확인된 경우에만 같은 profile을 재시작한다. 두 번째 실행은 새 sidecar instance와 기존 채팅·저장된 편집기 본문·diff·원본 불변을 확인하고 restart.json/PNG를 따로 남긴다. 정상 종료가 불확실하면 재시작하지 않는다. Monaco view-lines의 NBSP는 표시 공백으로 정규화하되 파일이나 diff 내용을 바꾸지 않는다.

`pnpm m5:smoke:packaged-ui --project`는 자체 생성한 작은 Git 프로젝트를 packaged sidecar의 정상 project.open API로 목록에 등록하고 안전 종료한 뒤 같은 임시 profile로 앱을 시작한다. 실제 목록 다시 열기→채팅 생성→Monaco 편집→저장→diff 표시와 원본 불변을 확인한다. 재시작 후 확인 취소의 원본 불변, 반영/복원 각각의 별도 동의 전 차단과 실행 후 정확한 파일 바이트를 확인한다. bridge/제품 main을 교체하지 않지만 CDP 입력이므로 실제 키보드/OS 포커스 증거가 아니다. 다중 프로젝트·실제 모델·실패 주입 전체 검증을 대신하지 않는다. 측정 RSS는 프로젝트를 열기 전 idle이며 편집 후 peak가 아니다.

RSS 원인 분리용으로 관측 앞뒤의 [CDP 프로세스 종류](https://chromedevtools.github.io/devtools-protocol/tot/SystemInfo/#method-getProcessInfo)와 [renderer heap](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/#method-getHeapUsage)도 기록한다. Chromium 목록에 없는 PID의 역할을 추측하지 않으며 heap은 RSS에 더하지 않는다. resource timing 목록이 비어 있으면 자산 미로딩의 증거가 아니다. 강제 GC나 GPU 비활성화로 기준을 우회하지 않는다.

`pnpm m5:smoke:packaged-ui`는 미리 만든 macOS arm64 패키지의 실제 실행 파일을 별도 OS 임시 userData로 시작한다. 제품 main/preload/renderer를 교체하거나 bridge를 가짜로 덮어쓰지 않는다. [Electron 공식 원격 디버깅 스위치](https://www.electronjs.org/docs/latest/api/command-line-switches/)로 해당 프로세스가 출력한 loopback CDP endpoint에만 연결해 시작 화면·실제 저장소 ready·스크린샷·10초 RSS를 관측한다. 프로젝트 선택·인증·모델 호출은 하지 않는다. 종료는 기존 renderer prepareQuit → main coordinator를 사용하고 실제 exit0 및 해당 sidecar instance의 DB clean marker를 확인한다. 종료 불확실이면 강제 종료/프로필 삭제 없이 실패 기록과 PID를 보존한다.

단일 migration 포함 cold 시작·디버거 계측이며 locked Mac의 물리 키보드/포커스 증거가 아니다. `diagnostic=true/gateEligible=false`, 기준 초과·흐름 오류는 FAIL/1, 관측 완료도 NOT_READY/2다. source HEAD/Git 상태와 실제 asar/sidecar 지문을 따로 기록하므로 오래된 패키지를 현재 소스의 증거로 간주하지 않는다. 현재 앱 산출물을 먼저 갱신하고 동일 패키지의 추가 수집을 진행한다. 이 시작 smoke만으로 전체 성능 게이트를 열지 않는다.

격리 userData로 실행한 시험 앱의 PID를 확인한 뒤 `node scripts/m5-observe-process-memory.cjs PID 40 idle`을 실행하면250ms 간격40표본을 수집한다. `idle|loaded|retained` 표시는 관측자가 확인한 화면 단계이며 자동 판정하지 않는다. 앱을 실행/종료/변경하지 않고 원시 PID별 RSS를 새 OS 임시 폴더의 observation.json에 보존한다. 수집 오류는 부분 표본을 남기고 실패한다. `OBSERVED`/exit0은 수집 완료일 뿐 성능 PASS가 아니며 `diagnostic:true`, `gateEligible:false`다. 현재 자손만 관측하고 PID 재사용/부모 이탈을 검증하지 않으므로 실제 에이전트 전체 프로세스 게이트를 대신하지 않는다.

`HELLO_AGENT_SMOKE_LARGE_FILE=1`을 추가하면 별도 생성 프로세스가 동일한 UTF-8 5MiB/100,000줄 fixture를 실제 Git 프로젝트에 넣는다. 실제 sidecar 읽기→Monaco 본문/입력 요소→두 frame 관측까지 renderer 시계로 재며 결과의 `largeFile.fileEditableMs`에 기록한다. 1,500ms 초과는 진단 실패다. 파일 생성용 캐시를 측정 앱의 main에 남기지 않는다. `HELLO_AGENT_SMOKE_MEMORY=1`과 결합하면 해당 파일 열기와 기존 두 채팅 흐름을 RSS 범위에 포함한다. 단일 관측이며 packaged/10회 p95/실제 입력 paint/4 stream 증거는 아니므로 `diagnostic:true`, `gateEligible:false`를 유지한다.

`HELLO_AGENT_SMOKE_MEMORY=1 HELLO_AGENT_SMOKE_APPLY=1 HELLO_AGENT_SMOKE_RELOAD=1 pnpm m5:smoke:project-ui`는 기존 실제 UI 시나리오에 250ms RSS 수집을 추가한다. 시작 화면에서 1초 idle을 수집한 뒤 생성 프로젝트 편집/반영/reload/복원/두 채팅을 실행하고 프로젝트 화면을 정상적으로 닫아 60초 뒤 잔류량을 기록한다. 제품 GC 설정이나 캐시를 변경하지 않는다.

결과는 해당 시험 임시 폴더의 `memory.json`에 원시 PID별 표본과 함께 저장한다. 현재 Electron root의 자손인 sidecar도 합산하지만, 부모를 이탈한 프로세스와 실제 에이전트는 관측 범위 밖이다. 400/800/120MiB 기준 초과는 FAIL로 실행을 끝내며 기준 이내도 메모리 보고서는 `NOT_READY`, `diagnostic:true`, `gateEligible:false`다. 바깥 UI 시나리오의 PASS와 성능 보고서 판정을 구분한다. 작은 파일·한 차례 UI 흐름이므로 대형 파일/4 stream·전체 G6의 대체 증거로 사용하지 않는다. 샘플러 실패나 시나리오 중단 시 불완전 표본과 오류를 보존한다.

## 실제 sidecar 시작 측정 절차

사용자 변경을 보존한 개발 진단은 `pnpm run m5:measure:sidecar-startup --diagnostic`으로 실행한다. 현재 checkout을 debug로 빌드(CARGO_INCREMENTAL=0)하고 동일한 준비/10회 측정을 수행한다. 전후 HEAD·Git 상태를 비교하고 binary SHA-256/buildProfile을 기록한다. 기준 초과는 FAIL/1, 기준 이내도 NOT_READY/2이며 diagnostic=true/gateEligible=false다. release 성능 또는 G6 합격 자료로 사용하지 않는다. 인자 없는 정식 실행은 계속 clean checkout을 요구한다.

clean `m5/coding-agent-ide`에서 `pnpm run m5:measure:sidecar-startup`을 실행한다. Cargo가 현재 checkout으로 빌드한 release executable을 사용한다. 생성한 임시 빈 DB의 최초 migration을 제외하고 warm-up 1회 뒤 10회 실행한다. 각 실행은 canonical protocol/status/quit validator로 응답을 검증하고 DB clean marker와 정상 프로세스 종료까지 확인한다. 프로젝트·에이전트 실행 요청은 보내지 않는다.

- `shellReadyMs`: spawn 직전부터 system.ready 검증 완료까지.
- `storageReadyMs`: spawn 직전부터 system.status의 storageAdmission ready 관측까지. 최대 10ms polling 간격이 포함된다.
- 두 값의 nearest-rank p95를 각각 기록하고 1,000ms 이하로 판정한다. shell의 3,000ms hard limit 관측도 따로 기록한다.
- 이 표본은 migration 없는 빈 DB를 반복해서 여는 warm sidecar 측정이다. 앱 전체 cold 시작, 실제 대형 DB, Electron/Chromium UI 성능을 대표하지 않는다.
- 성공 시 생성한 DB만 정리한다. 오류·종료 불확실 시 임시 폴더를 남긴다. JSON은 `.artifacts/m5-performance/`에 저장하며 Git HEAD, binary SHA-256, 기기·Rust·Node 정보, 원시 표본을 기록한다. 서명된 증거나 적대적 동일 사용자 filesystem 변조 방지를 주장하지 않는다.

## 전체 측정값 판정

### Native 저장 경합 관측

`CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 CARGO_INCREMENTAL=0 cargo test -p hello-agent-sidecar --locked -j4 --test stdio_process binary_workspace_save_races_preserve_external_bytes_across_two_chats -- --ignored --nocapture`는 생성한 Git 프로젝트와 실제 `--stdio` sidecar에서 두 채팅의 `workspace.file.save` begin/chunk/commit을 실행한다. 100회 동안 두 사본을 번갈아 선택하고 begin 또는 chunk 이후 외부 내용을 써 commit 충돌을 유도한다. 각 충돌의 명시 응답·외부 바이트 보존·다른 채팅/원본 불변을 검사한 뒤, 최신 버전으로 정상 저장되는지 대조한다.

정상 종료 뒤 출력하는 `native-workspace-save-races` JSON에는100개 개별 관측과 overwrite/미보고 충돌/교차 채팅 쓰기 카운터를 남긴다. 실패한 assertion은 테스트 실패로 남으며 완료 JSON을 만들지 않는다. 성공한 임시 프로젝트/DB는 종료 확인 후 정리하고 실패 데이터는 진단용으로 보존한다. 일반 검사에서는 ignored이며 명시 실행한다. 이는 commit 전 제어된 순서의 실제 프로토콜 관측이고, packaged UI·에이전트 동시 쓰기·모든 OS 스케줄링 경합의 증거는 아니므로 diagnostic=true/gateEligible=false다. 전체 G6 수집기의 대체물이 아니다.

### 단일 채팅 renderer 입력 스트림 진단

종료 관측은 `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 CARGO_INCREMENTAL=0 cargo test -p hello-agent-sidecar --locked -j4 --test stdio_process binary_cancel_ -- --nocapture`로 실행한다. 기존 취소 fixture의 leaf는 `setsid`로 별도 OS 세션에 진입한다. 시험 프로세스는 취소 전 부모/leaf의 PID·시작 시각과 leaf의 세션 ID를 확인하고, 취소 후 각 신원 소멸과 sidecar 정상 종료를 검사한다. uncertain 시나리오의 복구 전 quit 차단도 유지한다. 실제 공급자·독립적으로 재부모화된 orphan·packaged 통합은 검증하지 않으며 core OS probe를 공유한다. 결과는 diagnostic=true/gateEligible=false다.

현재 renderer를 빌드한 뒤 `M5_MEASURE_MESSAGE_PAINT=1 M5_RENDERER_DIR="$PWD/.vite/renderer/main_window" node_modules/.bin/electron tests/m5-renderer-smoke-runner.cjs`로 기존 합성 화면 회귀에50회/초·3000메시지 관측을 추가할 수 있다. 관측은 renderer performance.now 발송 시각부터 메시지 DOM이 연속 두 animation frame에 나타날 때까지다. 지연된 발송은 schedulingDelayMs로 보존하며 매 메시지 표본·미관측 ID·최대값을 독립 userData의 message-paint.json에 저장한다.

이 경로는 단일 합성 채팅의 진단 전용(`diagnostic:true`, `gateEligible:false`)이며4개 stream·실제 sidecar/패키징·G6 eventPaintMs를 대신하지 않는다.100ms 초과나 미관측은 실패한다. 기본 화면 회귀에서는 실행되지 않고 실제 계정 호출도 없다. 숨겨진 시험 창의 frame 관측이므로 실제 compositor 표시 시각이라고 주장하지 않는다.

### Renderer 대형 fixture 실측

네 채팅 합성 진단은 같은 명령에서 `M5_MEASURE_PARALLEL_PAINT=1`을 사용한다. 각 채팅에50회/초·3000개씩 총12000개를 보내며 선택한 첫 채팅만 frame 지연을 측정한다. 종료 후 네 채팅을 차례로 선택해 최신200개가 해당 채팅의2801~3000번과 정확히 일치하는지 검사한다. 원장 전체 페이지/순서/ID 격리는 `node --test tests/m5-parallel-chat-fixture.test.cjs`에서 별도로 확인한다. `parallel-message-paint.json`의 sourceCounts는 합성 원장 수이며 background 메시지의 화면 관측 수가 아니다. 이 모드도 diagnostic true/gateEligible false로, native IPC·실제 모델·G6 전체 합격 근거로 사용하지 않는다.

개발 중 기존 변경을 보존하면서 수치만 확인하려면 `pnpm run m5:measure:renderer --diagnostic`을 사용한다. 이 모드는 dirty checkout을 허용하되 `diagnostic:true`, `gateEligible:false`와 전후 Git 상태를 기록하고, 기준 초과는 FAIL/1, 기준 이내도 NOT_READY/2로 반환한다. 정식 기본 모드의 clean 조건은 유지한다. 두 모드 모두 별도 임시 userData를 사용하며 경로를 결과에 남긴다. 임시 userData는 진단용으로 보존되고 자동 삭제하지 않는다.

clean checkout에서 `pnpm run m5:measure:renderer`를 실행한다. 현재 production Vite renderer를 빌드한 뒤 보이는 Electron 창에서 warm-up 1회·측정 10회를 수행한다. 1,000개 폴더/10,000개 파일의 페이지 탐색, 한국어가 포함된 5 MiB·100,000줄 파일의 실제 Monaco 표시·입력, 500개 파일/20,000 changed lines/총 10 MiB diff의 요약·첫 파일 표시를 측정한다. 시각 완료는 조건 충족 후 두 animation frame으로 관측하며 renderer의 performance.now와 longtask observer를 사용한다. 입력 측정은 Electron의 실제 char event 전달과 dirty 상태·화면 텍스트 관측을 포함하므로 IPC 지연도 포함한 상한이다. 첫 diff는 전체 10 MiB patch 중 한 파일만 요청한다.

데이터는 sidecar 대신 contextIsolation을 유지한 fixture bridge에서 공급한다. fixture preload는 별도 빌드 프로세스에서 번들하며 테스트 창도 제품과 같은 sandbox=true다. preload의 process.sandboxed를 실제 확인한다. 이 측정은 실제 renderer와 Monaco의 성능 증거이고 packaged 앱 시작, 파일시스템/sidecar IPC 성능, IME 조합 입력, 실제 agent stream·전체 앱 RSS의 증거는 아니다. 결과와 실패 항목은 `.artifacts/m5-performance/renderer-*.json`에 기록한다. 기준 초과 시 exit 1이며 실패 숫자를 통과로 바꾸지 않는다.

`pnpm run m5:measure:renderer --memory`는 추가로 `/bin/ps`의 KiB RSS를 250ms 간격으로 수집한다. 측정 Electron PID와 현재 자손만 합산하고 수집용 ps 자체와 다른 앱은 제외한다. idle 1초 관측 뒤 기존 대형 fixture를 열고, 같은 빈 작업대로 reload해 문서/Monaco를 해제한 후 강제 GC 없이 60초 기다린다. 프로세스별 원시 표본과 idle/관측 peak/잔류 delta를 저장하며 §10의 400/800/120 MiB 상한을 적용한다. 샘플 사이의 순간 peak, 부모를 이탈한 자손, 실제 sidecar/agent 메모리는 포함하지 않으므로 전체 제품 메모리 gate를 대신하지 않는다. 표시 시간에는 수집기 오버헤드가 포함된다.

원인 분리를 위해 각 RSS 표본에 Electron 프로세스 종류를 붙이고 idle/각 trial/retained checkpoint에 main 메모리, Electron process metrics, renderer DOM 수와 제공되는 경우 근사 V8 heap 크기를 함께 저장한다. 이 값은 RSS에 더하거나 대신 사용하지 않으며, heap 크기만으로 누수 여부를 판정하지 않는다. checkpoint 수집은 개별 시간 측정 구간 밖에서 실행한다.

반복 reload의 DOM 보유를 조사할 때만 `--diagnostic --memory --dom-counters`를 사용한다. 격리 시험 창에 DevTools를 붙여 [Memory.getDOMCounters](https://github.com/ChromeDevTools/devtools-protocol/blob/master/pdl/domains/Memory.pdl)의 documents/nodes/jsEventListeners를 checkpoint마다 기록한다. `prepareForLeakDetection`·강제 GC·cache purge는 호출하지 않는다. 계측 부착 자체가 메모리에 영향을 줄 수 있으므로 일반 run과 동일 조건 또는 release gate 증거로 취급하지 않으며, 숫자 증가만으로 누수를 확정하지 않는다. 정식/비메모리 모드에서는 이 옵션을 거절한다.

`pnpm run m5:measure:performance --input <measurement.json>`은 공급된 측정값을 검증한다. 계측기를 대신하지 않으며 unit fixture를 실측으로 사용하면 안 된다. `PASS`=0, 기준 초과 `FAIL`=1, 누락·잘못된 입력·불일치 HEAD `NOT_READY`=2이다. 알려진 기준 초과는 다른 항목 누락과 함께 있어도 FAIL로 남긴다. 최대 4 MiB regular UTF-8 JSON만 받는다. 출력 JSON은 stdout으로 전달한다.

입력 schemaVersion은 1이며 필수 필드는 다음과 같다.

선택 필드 `diagnostic`을 제공하면 false, `gateEligible`을 제공하면 true여야 정식 판정 후보가 된다. 진단/부적격/잘못된 타입 표시가 있으면 수치가 모두 있어도 NOT_READY이며, 확인된 기준 초과는 계속 FAIL로 남긴다. 이 표시는 자료의 진위를 인증하거나 합성 측정을 실제 측정으로 바꾸지 않는다.

| 필드 | 내용 |
| --- | --- |
| `head`, `dirty`, `warmupRuns` | 현재 clean Git SHA, false, 1 |
| `environment` | `machine`, `cpu`, `ramBytes`, `os`, `electron`, `chromium`, `node`, `rust`, `measuredAt`와 `platform: darwin`, `arch: arm64` |
| `fixture` | 아래 고정 fixture 조건 |
| `timings` | 아래 시간 항목마다 warm-up을 제외한 10개 측정값 배열, 단위 ms |
| `maxima` | 아래 최대값 항목의 실제 관측값 |
| `counters` | 아래 오류 개수 |

`timings`: `appInteractiveMs`, `sidecarReadyMs`, `treeRootMs`, `treeExpandMs`, `fileEditableMs`, `inputPaintMs`, `diffSummaryMs`, `diffFirstFileMs`, `eventPaintMs`. 각 trial의 이벤트 지연은 그 trial의 최대 지연을 기록해 개별 느린 이벤트를 평균으로 숨기지 않는다. 10개 trial의 nearest-rank p95는 정렬 후 마지막 값이다. 이는 §10의 기준을 완화하지 않는 보수적 집계다.

`maxima`: `appColdMs`, `treeDomNodes`, `mainLongTaskMs`, `rendererLongTaskMs`, `idleRssBytes`, `peakRssBytes`, `retainedRssBytes`. RSS는 앱 프로세스 전체 합계다. 잔류량은 fixture 닫기 60초 후 전체 RSS에서 idle을 뺀 값이며 음수는 0으로 기록한다.

`counters`: `lostEvents`, `duplicateEvents`, `crossChatEvents`, `reorderedEvents`, `silentOverwrites`, `unreportedConflicts`, `survivingTrackedIdentities`, `unreportedUncertainExits`. 모두 실제 관측한 정수 0이어야 한다.

`fixture`의 고정 값:

```json
{
  "emptyStartupDatabase": true, "packagedApp": true, "sidecarMigration": false,
  "treeFiles": 10000, "treeDirectories": 1000, "fileBytes": 5242880, "fileLines": 100000,
  "diffFiles": 500, "diffChangedLines": 20000, "diffBytes": 10485760,
  "chatStreams": 4, "eventsPerStreamPerSecond": 50, "streamSeconds": 60, "sentEvents": 12000,
  "saveRaces": 100, "detachedDescendants": true, "memoryCloseWaitSeconds": 60,
  "rendererClock": "performance.now", "rssScope": "allAppProcesses"
}
```

패키지 크기는 기존 `m5:test:package`가 담당한다. 현재 전체 UI·메모리·경합·종료 측정 수집기를 모두 연결하지 않았으므로 `m5:test:performance` 예약 게이트는 아직 열지 않는다. `harness:list`의 G6는 계속 NOT_READY이며 이 두 측정 도구만으로 출시를 승인하지 않는다.
