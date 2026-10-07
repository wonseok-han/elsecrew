# M5 성능 측정

합격선은 `../proposals/m5-electron-spike.md` §10이다. 다음 도구는 로컬 개발 측정용이며 전체 G6 완료를 뜻하지 않는다.

## 실제 프로젝트 UI 회귀 (성능 측정과 별도)

macOS에서 현재 checkout의 main/preload/renderer `.vite/build`와 native sidecar를 먼저 빌드한 뒤 `pnpm m5:smoke:project-ui`로 실행한다. 스크립트 자체는 빌드하지 않으므로 오래된 산출물의 PASS를 최신 코드 증거로 쓰지 않는다. 실행할 때마다 임시 Git 프로젝트와 독립 userData를 만들며 사용자 프로젝트·선택된 에이전트·인증 정보를 사용하지 않는다. 실제 Electron/native sidecar를 통해 채팅 생성, Monaco 편집/저장, diff, 원본 불변, 반영 확인 취소, 두 채팅 사본 분리를 확인한다.

- `HELLO_AGENT_SMOKE_APPLY=1 pnpm m5:smoke:project-ui`: 스크립트가 생성한 프로젝트만 명시 확인 UI로 적용 후 복원한다.
- 위 설정에 `HELLO_AGENT_SMOKE_RELOAD=1`을 함께 주면 적용 후 renderer reload→저장 프로젝트/채팅 재선택→복원 경로를 포함한다. 전체 앱 재시작 검증은 아니다.
- PASS/FAIL JSON과 screenshot 경로를 출력하고 실패는 종료 코드1이다. 임시 프로젝트/userData는 성공·실패 모두 증거 확인을 위해 보존하며 자동 삭제하지 않는다. 이 실행은 새 시작/설정 modal·메시지 스크롤 전체 회귀나 실제 공급자·Windows·G6 합격을 대신하지 않는다.

## 실제 sidecar 시작 측정 절차

clean `m5/coding-agent-ide`에서 `pnpm run m5:measure:sidecar-startup`을 실행한다. Cargo가 현재 checkout으로 빌드한 release executable을 사용한다. 생성한 임시 빈 DB의 최초 migration을 제외하고 warm-up 1회 뒤 10회 실행한다. 각 실행은 canonical protocol/status/quit validator로 응답을 검증하고 DB clean marker와 정상 프로세스 종료까지 확인한다. 프로젝트·에이전트 실행 요청은 보내지 않는다.

- `shellReadyMs`: spawn 직전부터 system.ready 검증 완료까지.
- `storageReadyMs`: spawn 직전부터 system.status의 storageAdmission ready 관측까지. 최대 10ms polling 간격이 포함된다.
- 두 값의 nearest-rank p95를 각각 기록하고 1,000ms 이하로 판정한다. shell의 3,000ms hard limit 관측도 따로 기록한다.
- 이 표본은 migration 없는 빈 DB를 반복해서 여는 warm sidecar 측정이다. 앱 전체 cold 시작, 실제 대형 DB, Electron/Chromium UI 성능을 대표하지 않는다.
- 성공 시 생성한 DB만 정리한다. 오류·종료 불확실 시 임시 폴더를 남긴다. JSON은 `.artifacts/m5-performance/`에 저장하며 Git HEAD, binary SHA-256, 기기·Rust·Node 정보, 원시 표본을 기록한다. 서명된 증거나 적대적 동일 사용자 filesystem 변조 방지를 주장하지 않는다.

## 전체 측정값 판정

### Renderer 대형 fixture 실측

clean checkout에서 `pnpm run m5:measure:renderer`를 실행한다. 현재 production Vite renderer를 빌드한 뒤 보이는 Electron 창에서 warm-up 1회·측정 10회를 수행한다. 1,000개 폴더/10,000개 파일의 페이지 탐색, 한국어가 포함된 5 MiB·100,000줄 파일의 실제 Monaco 표시·입력, 500개 파일/20,000 changed lines/총 10 MiB diff의 요약·첫 파일 표시를 측정한다. 시각 완료는 조건 충족 후 두 animation frame으로 관측하며 renderer의 performance.now와 longtask observer를 사용한다. 입력 측정은 Electron의 실제 char event 전달과 dirty 상태·화면 텍스트 관측을 포함하므로 IPC 지연도 포함한 상한이다. 첫 diff는 전체 10 MiB patch 중 한 파일만 요청한다.

데이터는 sidecar 대신 contextIsolation을 유지한 fixture bridge에서 공급한다. fixture preload의 Node require를 위해 테스트 창만 sandbox=false이며 제품 보안 설정을 변경하지 않는다. 이 측정은 실제 renderer와 Monaco의 성능 증거이고 packaged 앱 시작, 파일시스템/sidecar IPC 성능, IME 조합 입력, 실제 agent stream·전체 앱 RSS의 증거는 아니다. 결과와 실패 항목은 `.artifacts/m5-performance/renderer-*.json`에 기록한다. 기준 초과 시 exit 1이며 실패 숫자를 통과로 바꾸지 않는다.

`pnpm run m5:measure:renderer --memory`는 추가로 `/bin/ps`의 KiB RSS를 250ms 간격으로 수집한다. 측정 Electron PID와 현재 자손만 합산하고 수집용 ps 자체와 다른 앱은 제외한다. idle 1초 관측 뒤 기존 대형 fixture를 열고, 같은 빈 작업대로 reload해 문서/Monaco를 해제한 후 강제 GC 없이 60초 기다린다. 프로세스별 원시 표본과 idle/관측 peak/잔류 delta를 저장하며 §10의 400/800/120 MiB 상한을 적용한다. 샘플 사이의 순간 peak, 부모를 이탈한 자손, 실제 sidecar/agent 메모리는 포함하지 않으므로 전체 제품 메모리 gate를 대신하지 않는다. 표시 시간에는 수집기 오버헤드가 포함된다.

`pnpm run m5:measure:performance --input <measurement.json>`은 공급된 측정값을 검증한다. 계측기를 대신하지 않으며 unit fixture를 실측으로 사용하면 안 된다. `PASS`=0, 기준 초과 `FAIL`=1, 누락·잘못된 입력·불일치 HEAD `NOT_READY`=2이다. 알려진 기준 초과는 다른 항목 누락과 함께 있어도 FAIL로 남긴다. 최대 4 MiB regular UTF-8 JSON만 받는다. 출력 JSON은 stdout으로 전달한다.

입력 schemaVersion은 1이며 필수 필드는 다음과 같다.

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
