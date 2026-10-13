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
- 위 설정에 `HELLO_AGENT_SMOKE_RELOAD=1`을 함께 주면 적용 후 renderer reload→저장 프로젝트/채팅 재선택→복원 경로를 포함한다. 전체 앱 재시작 검증은 아니다.
- 시작 화면의 빈 dock 부재, 목표 예시의 입력만 채우기/기존 초안 보호, 역할 설정 modal의 Escape→계속 편집→폐기·닫기/포커스 복원, 저장 후 재열기 및 원래 모델 복원도 같은 실행에서 확인한다. `START_AND_SETTINGS`는 중간 단계이며 마지막 PASS가 전체 시나리오 결과다. 저장 실패 주입과 메시지 스크롤은 별도 회귀가 필요하다.
- 이 시각 검증은 앱 창을 표시하고 포커스를 가져온다. macOS에서는 window가 visible이어도 renderer가 hidden인 상태가 관측돼 Monaco 표시/캡처가 멈춘 적이 있어, 재진입 편집 전 창을 다시 활성화한다. `SECOND_CHAT_WINDOW`의 창/페이지 가시성 진단과 renderer 오류를 함께 기록한다. 제품의 background 정책은 변경하지 않는다.
- PASS/FAIL JSON과 screenshot 경로를 출력하고 실패는 종료 코드1이다. 임시 프로젝트/userData는 성공·실패 모두 증거 확인을 위해 보존하며 자동 삭제하지 않는다. 이 실행은 실제 공급자·Windows·G6 합격을 대신하지 않는다.

## Native UI 작은 프로젝트 메모리 진단

### Native 프로젝트 반복 수집

`pnpm m5:measure:native-project`는 현재 빌드된 native UI 시험을 준비 실행1회＋측정10회 순차 실행한다. 각 실행은 새 임시 프로젝트에1만 파일 트리와5MiB 파일을 함께 생성하며, 편집/저장/diff·반영 확인 취소·두 채팅 흐름까지 수행한다. 원본 적용·reload·메모리 환경 옵션은 상속하지 않는다. 빌드는 자동 수행하지 않으므로 먼저 최신 main/renderer/debug sidecar를 준비한다.

결과는 새 OS 임시 `hello-agent-native-metrics-*`의 measurement.json과 각 trial 로그에 남긴다. 실제 앱 종료를 기다린 뒤 다음 실행을 시작하며 종료 미확정/비정상 종료면 중단한다. 강제 종료나 증거 삭제는 하지 않는다. 정상 종료한 실패 표본도 제외하지 않고10회에 포함하며, 누락 수치의 p95는 null이다. 준비 실행 실패도 전체 FAIL로 남긴다.10개 값의 nearest-rank p95는 최대값이다. 기준 초과/흐름 실패는 exit1, 모두 통과해도 개발 빌드 진단이므로 NOT_READY/exit2다. 이 명령은 packaged·전체 RSS·stream·save race·종료 안전성 측정을 대신하지 않으며 `m5:test:performance` 게이트를 열지 않는다.

`HELLO_AGENT_SMOKE_LARGE_TREE=1 pnpm m5:smoke:project-ui`는 실제 임시 Git 프로젝트에 1,000폴더/10,000파일의 `aaa-tree` 하위 트리를 추가한다. 기존 시험용 hello.txt 및 선택적 large.txt는 이 숫자 밖이며 `controlFiles`로 별도 기록한다. 실제 sidecar→파일 패널에서 하위 트리 첫 페이지와 한 폴더를 열고 두 animation frame까지 관측한다. `largeTree` 결과에 시간·가상 목록 DOM 수를 보존하고 1,000/250ms·500노드 초과를 실패 처리한다. 원본 snapshot 생성 시간은 treeRootMs에 포함하지 않는다. 단일 개발 실행·첫 페이지 진단이며 packaged/10회 p95·전체 탐색/G6 합격을 대체하지 않는다. 큰 파일/메모리 옵션과 결합할 수 있다.

대형 트리 모드의 최초 snapshot 및 새 채팅 생성 관측은 최대65초, 전체 시험은5분으로 제한한다. 이는 시험이15초에 먼저 quit을 요청하던 문제를 피하기 위한 관측 창이며 제품의60초 snapshot timeout이나 트리 성능 기준을 늘리지 않는다. 최초 프로젝트 선택→트리 표시 시간은 `SNAPSHOT_READY` 및 `largeTree.snapshotReadyMs`로 별도 보존한다.

### 패키징 앱 외부 관측

격리 userData로 실행한 시험 앱의 PID를 확인한 뒤 `node scripts/m5-observe-process-memory.cjs PID 40 idle`을 실행하면250ms 간격40표본을 수집한다. `idle|loaded|retained` 표시는 관측자가 확인한 화면 단계이며 자동 판정하지 않는다. 앱을 실행/종료/변경하지 않고 원시 PID별 RSS를 새 OS 임시 폴더의 observation.json에 보존한다. 수집 오류는 부분 표본을 남기고 실패한다. `OBSERVED`/exit0은 수집 완료일 뿐 성능 PASS가 아니며 `diagnostic:true`, `gateEligible:false`다. 현재 자손만 관측하고 PID 재사용/부모 이탈을 검증하지 않으므로 실제 에이전트 전체 프로세스 게이트를 대신하지 않는다.

`HELLO_AGENT_SMOKE_LARGE_FILE=1`을 추가하면 별도 생성 프로세스가 동일한 UTF-8 5MiB/100,000줄 fixture를 실제 Git 프로젝트에 넣는다. 실제 sidecar 읽기→Monaco 본문/입력 요소→두 frame 관측까지 renderer 시계로 재며 결과의 `largeFile.fileEditableMs`에 기록한다. 1,500ms 초과는 진단 실패다. 파일 생성용 캐시를 측정 앱의 main에 남기지 않는다. `HELLO_AGENT_SMOKE_MEMORY=1`과 결합하면 해당 파일 열기와 기존 두 채팅 흐름을 RSS 범위에 포함한다. 단일 관측이며 packaged/10회 p95/실제 입력 paint/4 stream 증거는 아니므로 `diagnostic:true`, `gateEligible:false`를 유지한다.

`HELLO_AGENT_SMOKE_MEMORY=1 HELLO_AGENT_SMOKE_APPLY=1 HELLO_AGENT_SMOKE_RELOAD=1 pnpm m5:smoke:project-ui`는 기존 실제 UI 시나리오에 250ms RSS 수집을 추가한다. 시작 화면에서 1초 idle을 수집한 뒤 생성 프로젝트 편집/반영/reload/복원/두 채팅을 실행하고 프로젝트 화면을 정상적으로 닫아 60초 뒤 잔류량을 기록한다. 제품 GC 설정이나 캐시를 변경하지 않는다.

결과는 해당 시험 임시 폴더의 `memory.json`에 원시 PID별 표본과 함께 저장한다. 현재 Electron root의 자손인 sidecar도 합산하지만, 부모를 이탈한 프로세스와 실제 에이전트는 관측 범위 밖이다. 400/800/120MiB 기준 초과는 FAIL로 실행을 끝내며 기준 이내도 메모리 보고서는 `NOT_READY`, `diagnostic:true`, `gateEligible:false`다. 바깥 UI 시나리오의 PASS와 성능 보고서 판정을 구분한다. 작은 파일·한 차례 UI 흐름이므로 대형 파일/4 stream·전체 G6의 대체 증거로 사용하지 않는다. 샘플러 실패나 시나리오 중단 시 불완전 표본과 오류를 보존한다.

## 실제 sidecar 시작 측정 절차

clean `m5/coding-agent-ide`에서 `pnpm run m5:measure:sidecar-startup`을 실행한다. Cargo가 현재 checkout으로 빌드한 release executable을 사용한다. 생성한 임시 빈 DB의 최초 migration을 제외하고 warm-up 1회 뒤 10회 실행한다. 각 실행은 canonical protocol/status/quit validator로 응답을 검증하고 DB clean marker와 정상 프로세스 종료까지 확인한다. 프로젝트·에이전트 실행 요청은 보내지 않는다.

- `shellReadyMs`: spawn 직전부터 system.ready 검증 완료까지.
- `storageReadyMs`: spawn 직전부터 system.status의 storageAdmission ready 관측까지. 최대 10ms polling 간격이 포함된다.
- 두 값의 nearest-rank p95를 각각 기록하고 1,000ms 이하로 판정한다. shell의 3,000ms hard limit 관측도 따로 기록한다.
- 이 표본은 migration 없는 빈 DB를 반복해서 여는 warm sidecar 측정이다. 앱 전체 cold 시작, 실제 대형 DB, Electron/Chromium UI 성능을 대표하지 않는다.
- 성공 시 생성한 DB만 정리한다. 오류·종료 불확실 시 임시 폴더를 남긴다. JSON은 `.artifacts/m5-performance/`에 저장하며 Git HEAD, binary SHA-256, 기기·Rust·Node 정보, 원시 표본을 기록한다. 서명된 증거나 적대적 동일 사용자 filesystem 변조 방지를 주장하지 않는다.

## 전체 측정값 판정

### 단일 채팅 renderer 입력 스트림 진단

현재 renderer를 빌드한 뒤 `M5_MEASURE_MESSAGE_PAINT=1 M5_RENDERER_DIR="$PWD/.vite/renderer/main_window" node_modules/.bin/electron tests/m5-renderer-smoke-runner.cjs`로 기존 합성 화면 회귀에50회/초·3000메시지 관측을 추가할 수 있다. 관측은 renderer performance.now 발송 시각부터 메시지 DOM이 연속 두 animation frame에 나타날 때까지다. 지연된 발송은 schedulingDelayMs로 보존하며 매 메시지 표본·미관측 ID·최대값을 독립 userData의 message-paint.json에 저장한다.

이 경로는 단일 합성 채팅의 진단 전용(`diagnostic:true`, `gateEligible:false`)이며4개 stream·실제 sidecar/패키징·G6 eventPaintMs를 대신하지 않는다.100ms 초과나 미관측은 실패한다. 기본 화면 회귀에서는 실행되지 않고 실제 계정 호출도 없다. 숨겨진 시험 창의 frame 관측이므로 실제 compositor 표시 시각이라고 주장하지 않는다.

### Renderer 대형 fixture 실측

네 채팅 합성 진단은 같은 명령에서 `M5_MEASURE_PARALLEL_PAINT=1`을 사용한다. 각 채팅에50회/초·3000개씩 총12000개를 보내며 선택한 첫 채팅만 frame 지연을 측정한다. 종료 후 네 채팅을 차례로 선택해 최신200개가 해당 채팅의2801~3000번과 정확히 일치하는지 검사한다. 원장 전체 페이지/순서/ID 격리는 `node --test tests/m5-parallel-chat-fixture.test.cjs`에서 별도로 확인한다. `parallel-message-paint.json`의 sourceCounts는 합성 원장 수이며 background 메시지의 화면 관측 수가 아니다. 이 모드도 diagnostic true/gateEligible false로, native IPC·실제 모델·G6 전체 합격 근거로 사용하지 않는다.

개발 중 기존 변경을 보존하면서 수치만 확인하려면 `pnpm run m5:measure:renderer --diagnostic`을 사용한다. 이 모드는 dirty checkout을 허용하되 `diagnostic:true`, `gateEligible:false`와 전후 Git 상태를 기록하고, 기준 초과는 FAIL/1, 기준 이내도 NOT_READY/2로 반환한다. 정식 기본 모드의 clean 조건은 유지한다. 두 모드 모두 별도 임시 userData를 사용하며 경로를 결과에 남긴다. 임시 userData는 진단용으로 보존되고 자동 삭제하지 않는다.

clean checkout에서 `pnpm run m5:measure:renderer`를 실행한다. 현재 production Vite renderer를 빌드한 뒤 보이는 Electron 창에서 warm-up 1회·측정 10회를 수행한다. 1,000개 폴더/10,000개 파일의 페이지 탐색, 한국어가 포함된 5 MiB·100,000줄 파일의 실제 Monaco 표시·입력, 500개 파일/20,000 changed lines/총 10 MiB diff의 요약·첫 파일 표시를 측정한다. 시각 완료는 조건 충족 후 두 animation frame으로 관측하며 renderer의 performance.now와 longtask observer를 사용한다. 입력 측정은 Electron의 실제 char event 전달과 dirty 상태·화면 텍스트 관측을 포함하므로 IPC 지연도 포함한 상한이다. 첫 diff는 전체 10 MiB patch 중 한 파일만 요청한다.

데이터는 sidecar 대신 contextIsolation을 유지한 fixture bridge에서 공급한다. fixture preload의 Node require를 위해 테스트 창만 sandbox=false이며 제품 보안 설정을 변경하지 않는다. 이 측정은 실제 renderer와 Monaco의 성능 증거이고 packaged 앱 시작, 파일시스템/sidecar IPC 성능, IME 조합 입력, 실제 agent stream·전체 앱 RSS의 증거는 아니다. 결과와 실패 항목은 `.artifacts/m5-performance/renderer-*.json`에 기록한다. 기준 초과 시 exit 1이며 실패 숫자를 통과로 바꾸지 않는다.

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
