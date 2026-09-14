# M5 성능 측정

합격선은 `../proposals/m5-electron-spike.md` §10이다. 다음 도구는 로컬 개발 측정용이며 전체 G6 완료를 뜻하지 않는다.

## 실제 sidecar 시작 측정

clean `m5/coding-agent-ide`에서 `pnpm run m5:measure:sidecar-startup`을 실행한다. Cargo가 현재 checkout으로 빌드한 release executable을 사용한다. 생성한 임시 빈 DB의 최초 migration을 제외하고 warm-up 1회 뒤 10회 실행한다. 각 실행은 canonical protocol/status/quit validator로 응답을 검증하고 DB clean marker와 정상 프로세스 종료까지 확인한다. 프로젝트·에이전트 실행 요청은 보내지 않는다.

- `shellReadyMs`: spawn 직전부터 system.ready 검증 완료까지.
- `storageReadyMs`: spawn 직전부터 system.status의 storageAdmission ready 관측까지. 최대 10ms polling 간격이 포함된다.
- 두 값의 nearest-rank p95를 각각 기록하고 1,000ms 이하로 판정한다. shell의 3,000ms hard limit 관측도 따로 기록한다.
- 이 표본은 migration 없는 빈 DB를 반복해서 여는 warm sidecar 측정이다. 앱 전체 cold 시작, 실제 대형 DB, Electron/Chromium UI 성능을 대표하지 않는다.
- 성공 시 생성한 DB만 정리한다. 오류·종료 불확실 시 임시 폴더를 남긴다. JSON은 `.artifacts/m5-performance/`에 저장하며 Git HEAD, binary SHA-256, 기기·Rust·Node 정보, 원시 표본을 기록한다. 서명된 증거나 적대적 동일 사용자 filesystem 변조 방지를 주장하지 않는다.

## 전체 측정값 판정

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
