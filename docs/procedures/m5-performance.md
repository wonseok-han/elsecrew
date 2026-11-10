# M5 성능 측정

**잔류 native 메모리 진단:** 명시 QA 앱의 `--combined-workload --retained-native-memory`는 RSS·입력·long-task 수집과 판정을 끝낸 뒤 main/renderer/GPU에만 `vmmap -wide`를 순차 실행한다(각5초/4MiB 상한). 결과에는 주소·파일경로 없이 종류별 virtual/resident/dirty/swapped 합과 main V8 통계를 남긴다. 실패는 고정 오류로 보존하고 제품 PASS 근거로 쓰지 않는다. 영역 resident 합은 RSS나 물리메모리 합계가 아니며 기존400/800/120MiB 판정에서 차감하지 않는다. 강제GC·heap snapshot은 실행하지 않는다.

`--file-retention-only --file-retention-with-output-diff`는 같은 비시각 진단에500개 변경 요약·첫 파일diff와 가짜 실행1개를 추가한다. 출력 원장3000개·sealed와 정상 종료는 별도 확인한다. UI에 출력이 나타난 관측과 전체3000개가 화면에서 소비된 증거는 다르며, 이 모드는 모든 출력 표시·4개 스트림·전체combined 성능 시험을 대체하지 않는다.

**실제 파일 수명 진단:** QA 패키지의 `--file-retention-only`는 combined와 함께 사용할 수 없다. 전체 성능 시험과 동일한 5MiB·100,000줄 임시 파일과 채팅1개를 만들고 실제 bridge/sidecar·Monaco로 연 뒤 정상 프로젝트 화면 해제와 heap snapshot을 수행한다. 파일은 별도 생성 프로세스에서 만들어 QA main에 본문 캐시를 남기지 않는다. 이전 단일행 진단 결과와 구분하며 report.retentionFixture에 크기·줄 수를 기록한다. 모델 호출·사용자 인증은 없고 창은 숨긴다. 기능 성공도 전체 status는 `NOT_READY`, gateEligible는 false다. 실제 표시·입력·RSS·출력/diff 공존 검증으로 인용하지 않는다. 덤프에 원문이 없다는 관측은 snapshot의 GC 가능성·단일 시나리오 범위 안에서만 해석한다.

**QA 창 가림 방지(2026-11-06):** 명시 QA 패키지에서만 `--keep-test-visible`을 지정하면 시험 창에 `setAlwaysOnTop(true, 'floating')`을 적용하고 보고서에 `keepTestVisible:true`를 기록한다. macOS에서는 다른 창에 완전히 가려져도 renderer visibility가 hidden이 된다([Electron 문서](https://www.electronjs.org/docs/latest/api/browser-window#page-visibility)). 이 옵션은 실제 시험 창의 배치만 바꾸며 제품 창 설정·backgroundThrottling·visibility 판정·숨김 실패·외부 입력 감시는 바꾸지 않는다. 잠금/다른 Space/OS 창 등 모든 가림을 막는다고 보장하지 않으며 실제 수집된 가시성 전체를 검증한다. 포커스를 계속 빼앗는 반복 호출은 하지 않는다. `--retention-snapshot`은 combined의 retained 단계까지 완료한 경우에만 성능 수집 종료 뒤 최대128MiB heap dump를 별도로 남긴다. snapshot은 GC를 유발할 수 있으므로 성능/누수 부재 증거가 아니며 정상 종료 확인 뒤 필요한 요약을 보존하고 원시 덤프를 정리한다.

**2026-11-06 측정기 수명 보정:** combined 시험은 최종 출력·이력·스크롤 검증이 끝나면 출력 관찰 결과를 main 보고서로 전달하고 renderer의 output observer 구독·MutationObserver·scroll/change/visibility listener와 전역 참조를 해제한 뒤 프로젝트를 닫는다. `outputObserverStoppedAt <= closedAt`을 집계 때 요구한다. 전체 구간의 화면 가시성·외부 입력 감시와 main tracing은 계속 유지하므로 추적 프로세스 RSS도 포함된다. 또한 main 보고서의 liveWindows·generation·outputObservation 원시 증거도 보존하므로 합계는 측정기 자료까지 포함한 보수적 QA 수치다. renderer 관찰기 해제를 전체 프로세스에서 측정기 비용이 제거됐다는 주장으로 해석하지 않으며, 이 합계만으로 제품 누수나 해소를 확정하지 않는다. 기존 실행은 이 해제 시점 증거가 없으며 과거 실패를 새 합격으로 바꾸지 않는다. 강제 GC·RSS 차감·400/800/120MiB 기준 변경은 없다. 보정 후 실제 재측정은 별도 필요하다.


## 큰 파일·diff·네 실행 출력의 동시 부하 진단

별도 output QA 앱 실행 파일에 `--combined-workload`를 전달하면 기존 단일 실행 3,000개×4채팅 paced fixture에 큰 파일/트리/diff를 결합한다. 생성하는 임시 원본에는 10,000개 트리 파일·1,000개 디렉터리와 5MiB/100,000줄 파일, diff 기준 파일이 들어간다. 첫 채팅의 복사본에서만 500파일/20,000변경줄/10MiB diff를 만든다. 사용자 메시지 append로 에이전트 출력을 대체하지 않는다.

첫 채팅에서 큰 모델과 diff를 연 뒤 네 실행을 시작한다. 첫 관측 구간의 앞뒤 head를 대조해 네 출력이 모두 증가하고 아직 sealed되지 않았는지, 모델과 diff가 함께 존재하는지 확인한다. 이후 기존 채팅별 출력·기록 탐색 검사를 수행한다. 프로젝트 닫기 후 60초를 기다린 뒤 잔류 RSS를 수집한다. 원본 Git 상태 불변을 확인하고 기존 정상 종료 경계를 유지한다. 진단은 일반 제품 `out`이 아닌 별도 QA 앱에서만 실행하며 실제 공급자·인증·모델을 사용하지 않는다.

**2026-11-02 단회 실행은 기능 경로를 완료했지만 idle RSS 초과로 FAIL했다.** 증거는 `.artifacts/m5-combined-output-2026-11-02/second`이며 전체 G6 합격은 아니다. 이후 대형 문서·diff 생성은 별도 프로세스에서 완료하고 종료를 기다리도록 수정했다. 생성용 문자열·patch 버퍼를 측정 대상 Electron main에 직접 할당하지 않으며 강제 GC나 RSS 차감은 사용하지 않는다. 이 수정 후 third 실행은 기능 경로를 완료했지만 idle 기준에서 FAIL했다. 다만 second/third 모두 idle phase에 채팅 작업 복사본 준비가 포함된 분류 오류가 있어 idle/잔류증분은 순수 유휴/회수 근거로 사용하지 않는다. 후속 수정은 최초 유휴 구간의 시작/종료 시각을 기록하고 준비 구간을 preparing으로 분리하며 전체 peak에는 계속 포함한다. 구간 분리 후 fourth 재측정도 완료했으며 `.artifacts/m5-combined-output-2026-11-02/fourth/result.json`의 오류는 `Combined workload limits exceeded: idleRssBytes`다. 순수 유휴 최대455,507,968bytes(434.41MiB)로 FAIL이며 전체 G6 합격이 아니다. QA main의 나머지 비용, 초 단위 표본, 10회 반복 미완료 등의 범위 차이가 남는다. 기존 400/800/120MiB와 파일/diff 시간 기준 초과는 실패로 기록한다. 반복 수집기의 기본값은 출력 전용이다. `node scripts/m5-repeat-output-qa.mjs --combined-workload`는 같은 QA 빌드로 동시 부하 warmup1회+측정10회를 연결하며 실패 시 중단한다. 이 경로는 구현·요약기 단위 검증만 마쳤고 실제10회 수집은 아직 하지 않았다. 기존 출력 전용 결과를 동시 부하 증거로 인용하지 않는다. 생성한 QA 패키지와 fixture는 기존 종료·PID 부재·DB clean 확인 뒤 정리한다.

2026-11-06 후속 측정 준비: 기존 동시 부하 수집기가 cold idle 전에 Chromium main 추적을 시작해, idle RSS에 추적용 Utility 프로세스도 포함했다. 예를 들어 fourteenth의 프로세스 역할 기록과 idle 표본에서 tracing 서비스 약34MiB를 확인했다(해당 실행은 외부 입력으로 중단됐으므로 성능 합격 근거가 아니다). main 추적 시작을3초 idle 종료 및 preparing 전환 뒤, 프로젝트 열기 전으로 옮겼다. `mainTraceTiming`의 요청/준비완료/작업시작 시각으로 이 순서를 남기며 추적 프로세스는 이후 preparing/부하/잔류 RSS에 계속 포함한다. RSS 차감이나 강제 GC는 하지 않고400/800/120MiB 기준을 유지한다. 시작 화면의 main long task 관측은 이 동시 부하 시험 범위에서 제외되며 별도 시작 진단을 사용한다. 기존 실패는 재판정하지 않는다. 반복 요약기는 이 시각의 누락·역전과 idle 표본 수집 완료가 추적 요청 이후인 경우를 거절한다. 변경 후 실제 측정은 아직 하지 않았다.

## 작업 복사본 준비 비용 분리

`CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_TEST_THREADS=4 cargo test --release -p hello-agent-sidecar --lib large_snapshot_phase_diagnostic -- --ignored --nocapture`는 기본 임시 fixture에1만 파일·1천 폴더 및5MiB 파일을 추가하고 source open, baseline capture, bundle export, baseline 재검증, snapshot 초기화, materialize+sync의 시간을 각각 출력한다. 추가 tree 루트까지 새 디렉터리는1001개다. 성공 시 TempDir가 생성 데이터를 정리한다.

이는 private 단계 비용 비교용이며 전체 create 경로의 원장/manifest/receipt 게시·안전 검사 순서나 UI 준비 시간의 대체물이 아니다. `diagnostic=true`, `gateEligible=false`를 유지한다. 기본 테스트에서 ignored이며 실제 계정·사용자 프로젝트·모델 호출은 사용하지 않는다. 단계별 지연을 확인하기 전 안전 검사 생략이나 동기화 완화를 근거 없이 적용하지 않는다.

## 실제 최종 응답의 화면 관측

`m5-live-provider-smoke.cjs`는 승인된 실제 실행 직전에 수동 관측기 `m5-agent-paint-probe.cjs`를 연결한다. 관측기는 모델/원장 API를 호출하지 않고 `onAgentPublicEvent`의 `final`만 수신한다. 동일 chat/run의 durable role 행 순서와 두 renderer frame을 대조하며 `notificationToFrameMs`를 기록한다. 본문·인증정보는 수집하지 않는다. 실행 전부터 구독해야 하며, 행의 chat/run 식별 속성이 없는 이전 패키지는 완전 관측으로 인정하지 않는다.

창 숨김, 중복 알림, 256개 초과, 누락/제거된 행, 종료 시 미완료 frame은 `INCOMPLETE`다. 종료 시 구독·observer·frame을 해제한다. 정상 표본은 `OBSERVED`이지 성능 gate PASS가 아니다. 상태 이벤트, sidecar→renderer 전달 시간, 물리적 화면 표시, 4채팅 부하/10회 반복은 측정하지 않으며 `diagnostic=true`, `gateEligible=false`를 유지한다. 기존 실제 작업의 합격 판정과 별도로 outcome.paint에 저장한다. 이 연결은 새 유료 실행 승인이 아니며 actual 시험은 별도 승인이 필요하다.

## 진단 서버 없는 RSS 비교

`node scripts/m5-packaged-ui-smoke.mjs --memory-file-diff`는 기존 큰 파일/트리와 diff fixture를 같은 임시 프로젝트에 생성한다. 5MiB 파일을 편집기에 연 뒤 입력 없이 깨끗한 모델을 유지하고 10MiB diff를 열어 두 화면의 공존을 확인한다. 기존 250ms RSS 수집과 프로젝트 닫기 후60초 대기·잔류 관측을 재사용한다. 원본 불변·정상 종료 검사와400/800/120MiB 기준을 유지한다. 네 채팅 스트림까지 동시 부하한 시험이나10회 반복/전체G6 증거는 아니며 `diagnostic:true`, `gateEligible:false`다. 종료 확인 후 이번 생성 프로젝트/프로필만 정리하고 결과 JSON은 보존한다.

반복하려면 `node scripts/m5-packaged-performance.mjs --memory-file-diff`를 사용한다. 준비1회와 본10회를 순차 실행하고 각 회차의 파일/diff 크기·공존 관측·원본 불변·60초 대기 및 원시 RSS로 재계산한 요약을 확인한다. 기존 종료·관측PID 부재 검사 뒤 해당 회차의 생성 데이터만 정리한다. 본10회의 메모리는 p95가 아닌 최대값을 집계하며 실패와 부분 진단 상태를 유지한다. 전체 실행에는 최소11분의 잔류 대기와 프로젝트 생성 시간이 필요하므로 반복 검증 파동에서 실행한다. 이 명령의 구현과 실제11회 수집 완료는 별개다.

`node --experimental-strip-types scripts/m5-packaged-ui-smoke.mjs --no-cdp`는 같은 패키지를 새 임시 프로필로 열되 remote-debugging 인자를 넣지 않는다. renderer가 기존 두 frame 준비 검사 뒤 출력한 고정 표식을 main이 허용된 main frame에서만 한 번 전달한다. 수집기는 stdout에서 표식을 최대30초 기다리고 수신 시각을 `startScreenReadyReceivedAtMs`로 기록한다. 수신 후2초 뒤 OS 프로세스 RSS를250ms 간격으로40회 수집하고 `MANUAL_QUIT_REQUIRED`를 출력한다. 이때60초 안에 해당 시험 앱 메뉴에서 정상 종료해야 한다. 종료 코드0과 새 DB의 단일 sidecar clean marker를 확인하며, 불확실하면 강제 종료나 자동 삭제하지 않는다.

`uninstrumentedRssBytes`는 시작 화면 준비 표식 이후의 진단 RSS 최대값이다. 표식 수신 시각은 renderer-local mark 시각과 다르며 이후 지속 가시성·실제 paint 완료를 증명하지 않는다. 최초 마이그레이션·OS 캐시 영향도 남는다. 400MiB 초과는 FAIL로 보존하지만 기준 이내여도 `NOT_READY`, `gateEligible=false`로 전체 G6 합격에 사용하지 않는다. 과거12초 고정 대기 결과와 측정 조건이 다르다. `--deferred-cdp`는 서버는 켜고 클라이언트 연결만 늦추는 별도 대조다.

반복 packaged 수집기는 각 trial 종료 후와 전체 종료 시 Git HEAD 및 porcelain 상태를 시작값과 비교한다. 달라지거나 조회에 실패하면 FAIL로 보존한다. trial 단계에서 발견하면 해당 fixture 정리 전에 중단한다. dirty 진단의 동일 상태는 허용하지만, 동일 porcelain 문자열 안의 파일 내용 변경까지 증명하는 검사는 아니며 패키지 freshness/전체 소스 동일성 증거로 쓰지 않는다.

## 시작 화면 준비 표식

패키지 수집기는 버튼 활성화 직후의 빈 mark 목록을 완료로 받지 않고 기존15초 관측 제한 안에서 표식을 기다린다. 숨김·중복·유효하지 않은 표식은 실패이며, 반복 시작 때 이전 실행의 표식은 지운다. renderer-local 시간은 앱 spawn 기준 시간으로 바꾸지 않는다.

renderer는 저장소 admission ready·빈 시작 화면·선택 차단 해제 뒤 활성 프로젝트 선택 버튼과 visible 상태를 두 animation frame에서 재확인해 `m5:start-screen-ready` performance mark를 한 번 남긴다. 숨김 전환은 두 프레임을 다시 시작하고, unmount/차단 변경은 취소한다. 이 표식은 renderer time origin 기준의 paint opportunity이며 OS 화면에 실제 표시됐다는 증거나 앱 spawn→interactive 시간은 아니다. 패키지 진단은 `rendererStartupMark`에 timeOrigin/marks/visibility를 별도 기록한다. 표식이 없거나 이전 패키지라면 관측 지연값을 표식으로 대체하지 않는다. 정식 시작 시간으로의 clock 결속과 실제 패키지 표식 검증은 후속이다.

## 패키지 메인 스레드 추적

전체 Chromium 이벤트 처리 예산은1,000,000개다. root 스레드는 최대128개이며 각 스레드의 전체 작업 수·200ms 초과 수·최대값 증거1개를 누적한다. metadata가 나중에 도착해도 앞선 main 작업을 포함한다. 최대값의 task 이름은128자까지만 보존하고 args는 제외한다. 이벤트/스레드 한도 초과·모호한 main 식별은 불완전 관측이다. 이전10,000개 원시 task 배열 한도는 폐기했지만 과거 실패 기록은 재판정하지 않는다.

`m5-packaged-ui-smoke.mjs`의 프로젝트 시나리오는 가시성 전제 확인 뒤 CDP `Tracing`의 `toplevel`을 수집한다. 앱 PID와 `CrBrowserMain` metadata가 지정한 스레드의 complete duration만 μs→ms로 변환한다. 다른 스레드·순간/flow 표식은 시간값에 섞지 않으며, 메인 스레드의 미완료/알 수 없는 duration 형식·추적 유실·종료 누락·표본 초과는 불완전한 관측이다. task args와 사용자 payload는 보존하지 않는다. 반복 수집기는 집계 형식의 작업 수/초과 수/최대값 증거와 완료·유실 상태를 검증한다. 이전 원시 timing 형식도 검사하며 idle 진단을 프로젝트 증거로 받지 않는다.

`node --experimental-strip-types scripts/m5-packaged-ui-smoke.mjs --main-trace`는 빈 임시 프로필에서 추적 형식만 확인한다. 가시성이 검증된 프로젝트 성능이나 정식 G6를 대신하지 않는다. 추적 자체가 메모리·시간에 영향을 주며, 시작 이전/추적 밖 작업과 JSON B/E duration 쌍의 완전한 지원은 보장하지 않는다. 메인 스레드 B/E가 관측되면 PASS 대신 불완전한 측정으로 남긴다. 프로토콜 근거: [Chromium Tracing](https://chromedevtools.github.io/devtools-protocol/tot/Tracing/).

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

패키지 RSS 관측은 PID와 `LC_ALL=C ps lstart` 시작 시각을 함께 보존한다. 이미 관측한 자손은 부모가 바뀌어도 다음 표본에 포함하고, 시작 시각이 다른 PID 재사용은 별개로 취급한다. 표본에서 사라진 신원은 추적에서 퇴역시킨다. `lstart`는 초 단위라 같은 초의 PID 재사용은 구별하지 못하며 처음 관측되기 전 분리/종료한 자손과 표본 사이 peak도 누락될 수 있다. 이 제한 때문에 여전히 전체 프로세스 수명 검증이나 G6 PASS 근거가 아니며 종료·정리의 기존 보수적 PID 부재 확인도 완화하지 않는다.

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

종료 관측은 `node scripts/m5-save-race-metrics.mjs --termination`으로 같은 수집기를 사용한다. 기존 `binary_cancel_` 두 시험을 release/locked·직렬 실행해 정상 중단과 불확실 종료/명시 복구의 JSON을 모두 요구한다. 관측 자손2개 이상·별도 세션 자손1개 이상·관측 생존0·불확실 상태 보고·정상 종료를 대조하며 한쪽 누락/중복/값 불일치는 FAIL이다. libtest가 JSON 앞에 붙이는 정확한 두 시험 이름만 허용한다. OS 임시 `hello-agent-termination-metrics-*`에 원시 로그와 관측을 보존하며 정상 자료도 NOT_READY/exit2다. 대상은 시험 전용 프로세스이고 실제 공급자/사용자 앱은 실행하지 않는다. 독립적으로 재부모화된 orphan 전체, 실제 공급자 및 전체 G6 종료 안전성의 증거는 아니며 외부 관측기도 제품과 같은 OS probe 구현을 공유한다.

`node scripts/m5-save-race-metrics.mjs`는 아래 기존 시험 하나를 release/locked로 실행하고 OS 임시 `hello-agent-save-race-metrics-*`의 `cargo.log`와 `measurement.json`에 보존한다. 전후 Git HEAD/status, Cargo 종료, 정확히 하나의 완료 관측,100개 순서·두 채팅 교대·개별 보존/충돌/대조 저장 결과를 검사한다. Rust 동시 실행은4 이하이며 다른 무거운 시험과 동시에 실행하지 않는다. 정상 관측도 partial diagnostic이므로 NOT_READY/exit2이고 실패는 exit1이다. 프로세스를 시간 초과로 강제 종료하거나 실패 fixture를 자동 삭제하지 않는다. 전체 G6용 canonical measurement.json을 생성하지 않는다.

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

`pnpm run m5:test:performance`는 같은 판정기를 `.artifacts/m5-performance/measurement.json`에 연결한 G6 하네스 명령이다. 전체 수집기는 아직 미완성이므로 이 명령 등록 자체가 측정 완료를 뜻하지 않는다. 현재 clean HEAD에서 생성한 전체 실측 증거만 해당 경로로 전달해야 하며, 파일이 없으면 exit2/NOT_READY다. 부분 진단의 diagnostic/gateEligible 표식을 바꾸거나 미측정 값을0으로 채우지 않는다. `pnpm harness:list`의 COMMANDS_READY는 명령 등록 상태일 뿐 합격 상태가 아니다. 전체 하네스는 실제 명령의 exit code를 판정한다.

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

패키지 크기는 `m5:test:package`가 담당한다. `m5:test:performance`는 등록된 측정 판정 명령이지만 전체 UI·메모리·경합·종료 수집기는 아직 모두 연결되지 않았다. `harness:list`의 COMMANDS_READY는 명령 등록만 뜻한다. 실제 게이트는 누락되거나 현재 HEAD와 맞지 않는 증거를 NOT_READY로 거절하며, 부분 측정이나 명령 등록만으로 G6 완료·출시를 승인하지 않는다.

### 장시간 출력의 실행 중 화면 진단 (개발 빌드 전용)

현재 renderer/main/preload와 debug sidecar를 빌드한 뒤 `env -u ELECTRON_RUN_AS_NODE node_modules/.bin/electron scripts/m5-durable-output-ui-smoke.cjs --live`를 실행한다. 실제 계정 대신 임시 프로젝트의 네 합성 실행을 각 3,000개/20ms 간격으로 생성한다. `--live`는 `--paced`를 포함하며 각 채팅을 순서대로 선택해 12초 동안 head와 보이는 행의 두 RAF 관측이 증가하는지 확인한다. 처음 한 번 출력 영역을 화면으로 가져온 뒤 지속 강제 스크롤은 하지 않는다. 이후 전체 기록 조회와 이전/최신 이동을 검사한다.

`liveWindows`에는 선택한 run, 관측 시간, head/화면 표본, 누락 카운터가 남는다. 생성 시각과 화면 시각의 차이에는 채팅 전환 후 과거 기록을 따라잡는 시간이 포함되므로 이를 곧바로 live eventPaint 지연으로 집계하지 않는다. 비선택 채팅의 paint나 물리 compositor 표시를 입증하지 않는다. 숨김 이력이 있으면 기능 성공이어도 전체 상태는 NOT_READY다. 결과는 `diagnostic:true, gateEligible:false`이며 정식 패키지/10회 G6 수집을 대체하지 않는다. 정상 종료와 OS 프로세스 부재를 확인한 뒤 작은 JSON/PNG만 보존하고 해당 시험 root를 정리한다.

`--no-cdp-project`는 `--no-cdp`와 같은 디버깅 서버 없는 관측에 `--project`의 생성된 두 프로젝트 seed를 결합한다. 사용자 데이터는 쓰지 않으며 편집/반영/복원은 실행하지 않는다. 시작 화면 준비 표식 후40개 RSS 표본을 수집하고 별도 정상 종료를 기다린다. `seededProjectBaseline:true`로 빈 프로필과 구별한다. 같은 package의 프로젝트 baseline 비교를 돕지만, 단발 차이를 CDP 비용의 인과적/통계적 증명이나 전체 G6 합격으로 사용하지 않는다.

`m5-durable-output-observer.cjs`의 `liveLatency`는 선택 구간에서 처음 받은 head의 고정 순서까지 따라잡은 뒤 도착한 새 알림만 대상으로 한다. 이후 head가 계속 증가해도 이 준비 기준을 옮기지 않는다. 같은 선택 구간의 두 RAF 뒤 보이는 출력 순서가 알림 순서와 정확히 일치할 때 `notificationToFrameMs`를 기록한다. 채팅 전환·숨김·보이지 않는 출력은 구간을 끊고 다시 따라잡아야 한다. 더 최신 행으로 합쳐져 정확한 행 표시를 확인하지 못한 알림은 `superseded`, 구간 중단은 `interrupted`, 남은 알림은 `unfinished`, 수집 상한은 `dropped`로 남긴다. 이들을 0으로 바꾸거나 측정된 일부 표본만으로 전체 이벤트 paint 성공을 선언하지 않는다. 최대 대기256개·보존1024표본이며 기존 생성→화면 관측과 별도다. 실제 모델/commit 지연이나 전체G6 수집을 대체하지 않는다.

### 최적화된 장시간 출력 QA 빌드 경계

`hello-agent-sidecar`의 명시적 Cargo feature `qa-output-fixtures`는 release 최적화를 유지하면서 `--stdio-agent-fixture long-stream`과 `paced-long-stream`을 시험하기 위한 진입점이다. 기본 feature는 아니며 일반 release의 거절 경계와 `--stdio` 실제 공급자 선택은 유지한다. 제품 staging은 이 feature를 포함한 Cargo 산출물을 거절한다.

후속 수집기는 별도 QA 앱 경로와 임시 userData를 사용하고 제품 `out` 앱을 교체하지 않아야 한다. renderer/main 원본 및 설정 차이를 기록하고, QA 실행 파일을 제품과 동일한 바이너리라고 주장하지 않는다. 단일 run 3,000개·4채팅·실제 발행 간격·전체 기록 대조·보이는 채팅의 paint·숨김/미관측 구분·정상 종료·임시 데이터 정리를 함께 수집해야 한다. `node scripts/m5-package-output-qa.mjs`는 fresh 임시 root에 QA 앱을 제작한다(자동 실행하지 않음). 출력된 build.json의 appDirectory 아래 앱을 실행하면 고정 QA main이 네 채팅 long-stream 진단을 수행한다. 결과와 build 메타데이터를 함께 보존하고 정상 종료/관측 PID 부재/DB clean 확인 후 생성한 QA 앱과 fixture root를 정리한다. 아래 반복 수집기도 전체 G6 수집을 대체하지 않으며 결과는 diagnostic/gateEligible:false다.

QA 출력 결과의 `generationSummary`는4개 run의 전체12,000 persisted row를 대상으로 ID 유일성·출력 sequence와본문 sequence·run 결속을 확인하고 각run의60초 내 timestamp 건수/마지막시각/최대간격/예정대비지연을 요약한다. fixture timestamp는 stdout write 직전이므로 실제 write 완료·commit·수신·paint 시각으로 인용하지 않는다. `allStampedWithin60Seconds`만으로 전체 스트림 성능 합격을 선언하지 않는다.

### QA 출력 반복 수집

clean checkout에서 `node scripts/m5-repeat-output-qa.mjs`를 실행하면 QA 앱을 한 번 빌드하고 준비1회+측정10회를 순차 실행한다. 각 회차의 source HEAD/상태와 app.asar/sidecar 지문을 대조하며 전체 생성 기록 요약을 원시 row에서 다시 계산한다. 실제 관측된 지연 초과는 버리지 않고 측정10회의 최대값에 포함한다. 숨김·잘림·누락은 중단 사유이며 실패 회차를 자동 재시도하거나 제외하지 않는다.

결과는 임시 `hello-agent-output-series-*`의 series.json과 trial별 JSON/PNG/log에 남는다. 각 fixture는 정상 종료 응답, 관측 PID 부재, DB clean 표시를 대조한 뒤에만 삭제한다. 종료가 불명확하면 앱을 강제 종료하지 않고 QA 패키지와 fixture를 보존한다. 확실히 종료한 fixture는 측정 실패에도 정리하고 작은 증거를 남긴다. QA 패키지는 모든 회차와 집계가 NOT_READY로 끝난 경우에만 자동 정리한다. 실패·중단 때는 원인 확인을 위해 패키지와 build.json을 보존한다.

이 수집기는 QA artifact·표본 live latency·생성 timestamp·샘플 RSS 진단이다. 모든 행 receipt/paint·닫기 후 retained memory·다른 G6 workload를 대체하지 않으며 기준 이내여도 NOT_READY/exit2다. 관측 실패나 집계 기준 초과는 FAIL/exit1이다.

QA live 구간의 시작은 배열 길이 cursor만 보관하고 종료 때 해당 run의 새 head/frame만 읽는다. 누적 trace 전체와 latency 표본은 종료 시 원시 결과에 남기며, 읽기 비용 `observationReadMs`와 main의 동기 기록 비용 `mainPersistMs`를 각 live 구간에 별도 기록한다. 이 시간은 원래 latency에서 빼지 않는다. cursor 이후 누락/잘림 카운터도 유지하며 변경 전 초과 결과를 무효화하지 않는다.

### 동시 부하 반복 수집

`--combined-workload`를 지정한 series는 각 QA 실행에 같은 인수를 전달하고 workload 종류를 기록한다. 원래 종료 응답·관측 PID부재·DB clean 확인 및 fixture 정리 경계를 그대로 사용한다. 요약기는 표준 출력과 동시 부하 자료가 섞이는 것을 거절하며 fixture 크기·파일/diff 공존·원본 불변·60초 대기·유휴 구간 경계를 확인한다. 메모리 최대값과 실패 목록은 원시 PID별 RSS 표본에서 다시 계산해 저장된 요약과 대조한다. 최종 series에는 peak뿐 아니라 idle/retained 최대값도 남는다. 실패를 제외해10회를 채우지 않고 원시 결과를 남긴 뒤 중단한다. 이 경로도 `diagnostic:true`, `gateEligible:false`이며 계측 누락·전체G6 완료를 대신하지 않는다.

### 출력 QA의 RSS 수집기 자체 제외 (2026-11-02)

동시 부하를 포함한 출력 QA는 비동기 `execFile`의 ps PID를 검증하고 해당 수집기 행만 프로세스 트리 합계에서 제외한다. 한 번에 한 수집만 허용하고2초/4MiB 상한을 유지한다. 단계 변경·종료 전에 진행 중 수집을 drain하며 단계 교차·중복 tick·오류·자료 잘림은 실패다. 시작/완료/소요 시간을 기록한다. 앱·sidecar·에이전트 자손은 계속 합산한다. 정확한 PID·직계 부모를 확인하지 못하거나 ps가 실패하면 관측 오류를 기록한다. 이 수정 전 fourth 등 결과에는 일시적인 수집기 RSS가 포함돼 있다. 과거 원시 값과 실패 결과를 임의 차감/재판정하지 않으며 최신 실행의 수집으로 교체해야 한다. 수집기 제외는 실제 제품 메모리 최적화 또는400MiB 합격 증거가 아니다.

### 출력·동시 부하의 renderer long task

출력 QA는 visible bridge 준비 이후 기존 `beginLongTaskProbe`를 시작하고 시나리오 종료 때 pending record까지 수집한다. 시작 단계 이전이나 main process의 작업 시간은 포함하지 않는다. 중단 시 windowCompleted=false로 남기며 관측 미지원·overflow·미완료·요약 불일치는0ms로 대체하지 않는다. 200ms 초과는 실패다. 반복 수집기는 본10회 원시 관측을 재검증해 rendererLongTaskMs 최대값을 집계한다. 이전 출력 결과에 이 필드가 없으면 새 요약기는 거절한다. 이 연결은 단위 검증까지 완료됐으며 최신 QA 패키지 실측과 전체G6는 남아 있다.

### 출력·동시 부하의 main task

출력 QA도 기존 `createMainTaskTrace`를 재사용한다. visible bridge 준비 뒤 QA 창의 debugger로 Chromium `toplevel` trace를 시작하고, 원시 이벤트에서 해당 앱 PID의 `CrBrowserMain` 완결 slice만 평가한다. 시나리오 종료 뒤 tracingComplete를 최대8초 기다리고 연결을 해제한다. 미완료·data loss·100만 이벤트/128개 root thread 집계 상한 초과는0ms로 대체하지 않는다. task 원시 배열 대신 thread별 count와 최대 witness를 보존한다. 본10회의 mainLongTaskMs 최대값은200ms 기준과 비교한다. 이전 출력 기록에 mainLongTasks가 없으면 새 요약기는 거절한다.

이 관측은 QA 전용이며 debugger/trace 자체 비용도 측정 프로세스에 포함된다. trace 없는 과거 RSS와 직접 비교해 최적화 효과를 주장하지 않는다. 시작 이전 작업·물리적 화면 표시·전체G6를 증명하지 않는다. 연결 및 파서/요약 회귀까지 확인했고, 완전한 단회 실측은 eighth/tenth에서 수행했으나 main/idle 기준을 넘었고 반복 합격은 남아 있다.

`node_modules/.bin/electron tests/m5-main-task-session-smoke.cjs`는 새 `startMainTaskSession`의 debugger attach/trace completion/detach를 실제 Electron에서 확인하는 별도 진단이다. 자체 임시 userData와 보이지 않는 빈 창만 사용하고 sidecar/공급자를 실행하지 않는다. 2026-11-02 완료/손실 없음/리스너 해제/정상 종료를 확인했지만, 빈 창 시간값은 제품 성능이나 전체G6 근거가 아니다. 결과의 root 아래 기록을 보존하고 종료/PID부재를 확인한 뒤 그 프로필만 정리한다.

### 동시 부하의 편집 입력

combined QA는 첫12초 출력 관측 뒤 네 run이 아직 emitting인 상태에서 5MiB Monaco 편집기에 Electron native char Z를 보낸다. 실제 trusted beforeinput1회·dirty 표시·첫 줄 변화·두 animation frame을 요구하고, 시작/전달/화면 관측 시각을 보존한다. Cmd+Z 뒤 원래 첫 줄과 clean 상태를 확인하며 저장/API model mutation은 하지 않는다. 입력 후에도 네 run이 미봉인 상태여야 한다. 요약기는 원시 시각 차이와 복원/trusted/live 증거를 검사하고 본10회 inputPaintMs 최대값을50ms 기준과 비교한다. 물리 화면 지연 측정은 아니다. 실제 입력/Undo는 seventh 이후 단회 실행에서 확인했고 입력798ms 실패와 후속19.9~25.2ms 관측이 공존하므로 반복 합격으로 주장하지 않는다. measure 진입/DOM 준비/첫 프레임도 별도 기록한다.


### 화면 조작과 단계별 보조 진단

측정 중에는 창을 가리지 않고 휠·클릭·탐색 키를 입력하지 않는다. 계획된 native Z/Undo와 별도로 실제 외부 탐색 입력을 기록하며, 그 증거가 없거나 잘렸거나 외부 입력이 섞인 결과는 자동 합격에서 제외한다. 실패 진단에도 입력 기록을 보존한다. 일반 단회 준비/실행은 약5분이지만 반복은 더 오래 걸리므로 시작 전에 가시성 시간을 확인한다.

Node timer 지연은 starting/idle/preparing/loaded/live-output/history-pages/generation-audit/closing/retained로 나눠 보조 진단한다. 행 수는 최대17개이며 단계 경계는 근사치다. ELU가 모두0이면 utilizationAvailable=false와null을 기록한다. 이 정보는 Chromium task 최대200ms나 RSS 기준을 대체하지 않으며, 관측 종료/trace 처리 비용까지 포함할 수 있다.


### 추적 경계 최소 진단

`node_modules/.bin/electron tests/m5-main-task-session-smoke.cjs --dense-trace`는 숨겨진 빈 창에서25초 timer와 고정10만 MessageChannel 이벤트를 만든다. 별도 임시 프로필을 사용하며 renderer 생성35초 제한이 있다. 시작/끝의 CDP clock_sync 표식을 제한된 메타데이터로 보존한다. 앱의 기능·sidecar·인증·공급자는 사용하지 않는다. 2026-11-02의 대량 관측은 incompleteMain으로 실패했으므로 종료 오버헤드 가설을 입증한 결과가 아니다. 일반 앱의 시간값을 차감하거나 gate 구간을 변경하는 근거로 사용하지 않는다. 결과를 보존하고 정상 종료·관측PID 부재를 확인한 뒤 해당 프로필만 정리한다.


측정 중 동시 부하 diff 생성은 비동기 자식 실행을 기다린다. 별도 프로세스를 쓰더라도 execFileSync 대기는 앱 main을 막으므로 측정 구간에서는 사용하지 않는다. 2026-11-02 twelfth의271ms는 종료 표식 이전에 관측됐으나 입력 단계 실패/포커스 이탈로 전체 시나리오는 미완료다. 비동기 전환은 계측 간섭 수정이며 과거 지연을 차감하거나 합격으로 재분류하지 않는다.


macOS QA는 창 표시 및 native 입력 직전에만 app.focus({steal:true})로 시험 앱을 활성화한다. window.focus만으로 다른 활성 앱에서 전환되지 않는 경우를 다룬다. 사용자가 화면 유지 가능하다고 확인한 시험에서만 사용하고 지속적인 포커스 루프는 만들지 않는다. 숨김/외부 입력/불완전 구간 판정은 유지한다.


### IOSurface 수명 진단

최신 renderer를 빌드한 뒤 `node_modules/.bin/electron tests/m5-idle-memory-runner.cjs product --surface-lifetime`로 실행한다. 다른 옵션과 결합하지 않는다. 합성 preload의 유휴 화면이며 sidecar/실제 공급자는 사용하지 않는다. 기존 초기5개 RSS 표본 뒤 약12초 동안 GPU PID를 다시 확인하며 bounded vmmap과 RSS를 순차 수집한다. 각 표본의 실제 소요시간과 visible/focused를 보존하며 원시 map은 저장하지 않는다. native resident는 RSS나 가산 가능한 물리사용량이 아니며 관측비용/처음부터의 전체 구간을 제외해 합격으로 바꾸지 않는다. 2026-11-02에는 surface resident가 줄어도GPU RSS는거의같아 이 버퍼의존속을 RSS초과원인으로확정하지 못했다.


### 성능 기준 재검토안 (2026-11-10, 미승인)

사용자가 기준 자체가 지나치게 타이트한지도 검토하도록 요청했다. 이는 기준 변경 승인이 아니며 현재 400/800/120MiB 및 입력 50ms 판정은 유지한다. 아래는 새 합격 결과가 아니라 기존 진단 자료에 근거한 제안이다.

11월10일 combined 단회 진단 네 건의 유휴 RSS는 407.41~436.23MiB, 최대 RSS는 725.09~805.05MiB, 닫기 후 잔류증분은 78.70~296.28MiB였다. 입력은 24.3/47.0/161.3/36.5ms였다. 서로 다른 계측 빌드이며 준비1회+본10회 동일 빌드 p95 시험을 대신하지 않는다. 자료는 `.artifacts/m5-{combined-output,native-combined,native-fixed-combined,native-complete-combined}-2026-11-10/result.json`에 있다.

- **유휴 400MiB → 512MiB 제안**: 최신 네 진단에서 400을 약2~9% 초과한다. 512는 관측 최대 대비 약17% 여유를 두는 제품 예산 제안이며 업계 표준이나 플랫폼의 필연적 하한이라는 주장이 아니다. 11월2일 앱 코드 없는 rendered 대조군도 409.59~413.92MiB였지만 sidecar 없는 별도 진단이므로 제품과 직접 차감하거나 400이 불가능하다고 단정하지 않는다. 현재 RSS는 공유 페이지와 추적 프로세스 비용도 합산하므로 실제 독점 물리사용량과 구별한다.
- **부하 최대 800MiB 유지 권고**: 네 건 중 세 건은 기준 이내다. 805.05 초과는 그대로 실패로 보존하며 이 한 건만으로 상한을 올리지 않는다.
- **잔류증분 120MiB는 수명 검사 방식을 먼저 검토**: 첫 Monaco/런타임 로드 뒤 유지되는 캐시와 반복 사용 때 계속 커지는 누수를 구분할 자료가 부족하다. 같은 프로세스에서 동일 작업 열기·닫기를 반복하여 최초 부하 후 증가와 후속 회차 증가를 별도 기록하는 진단을 제안한다. 안정화 여부를 확인하기 전 120을 임의 상향하거나 강제GC/수치 차감으로 통과시키지 않는다. 정식 대체 기준과 반복 횟수는 별도 확정 사항이다.
- **입력 p95 50ms와 안전 기준 유지 권고**: 단회 161.3ms를 제외하지 않으며 같은 빌드의 정해진 표본으로 재평가한다. 출력 누락·순서·원본 보호·충돌·종료 확인 기준은 메모리 예산과 무관하며 완화하지 않는다.

다음 결정은 유휴 예산 512MiB 채택 여부다. 승인 전에는 하네스 상수·제품 gate·과거 실패 판정을 변경하지 않는다. 측정기 수정과 반복 재시험에 시간이 집중된 점을 반영해, 무기한 400MiB 맞추기보다 예산 결정 및 잔류 증가의 반복 사용 특성 확인을 우선하는 것을 권고한다.


### 같은 프로세스의 파일 수명 반복 진단

QA 패키지에서 `--file-retention-only --file-retention-cycles`를 사용하면 숨겨진 창·동일 앱/sidecar/프로젝트/채팅으로 5MiB 파일을 세 번 열고 프로젝트 화면을 닫는다. 초기 상태와 각 닫기 뒤 60초 대기 및 3초 RSS 관측을 기록한다. 매번 snapshot/채팅을 새로 만들지 않으며 실제 공급자는 실행하지 않는다. `fileRetention.cycles`와 원시 `memory.samples`로 최초 증가와 후속 증가를 구분한다. 수집 오류·잘림·관측 누락은 실패이며 강제GC와 heap snapshot은 사용하지 않는다. 이 옵션은 output/diff 혼합 모드와 결합하지 않는다.

2026-11-10 clean 69a42d6e 패키지에서 실행했다. 초기/닫기1·2·3회 RSS 최대는 335.53/382.12/349.05/333.66MiB로, 이 파일 전용 세 회차에서 지속 증가를 관측하지 않았다. 수집 오류·잘림 없이 정상 종료했고 결과는 `.artifacts/m5-file-retention-cycles-2026-11-10/`에 보존했다. 출력·diff·가시성 부하는 포함하지 않으므로 종합 잔류 문제의 해소는 아니다. 숨겨진 파일 전용 부하이므로 visible combined RSS/입력/G6 통과나 누수 부재를 증명하지 않는다. 정상 종료·PID 부재·DB clean 대조와 임시 파일 정리는 기존 QA 절차를 따른다.
