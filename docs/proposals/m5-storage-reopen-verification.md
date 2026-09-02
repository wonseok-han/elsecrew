# M5 G5D — 압축 기록 재오픈 검증 계약

> **범위: 후속 구현 계약. 제품의 재오픈 제한이 해제됐다는 뜻이 아니다.**
> 현재 동작은 `architecture.md`와 `history.md`를 확인한다. 기준 코드는 `a87b659`의 storage V8이다.

## 1. 목적과 경계

terminal compacted V8의 frozen operation이 100개를 넘더라도, 호출마다 정해진 양만 검증할 수 있게 한다. 기존 immutable provenance·receipt 교차 검증을 유지하며 이전 프로세스의 검증 결과를 믿고 건너뛰지 않는다.

현재 `Storage::open`은 compaction 검증 뒤 전체 integrity/projection replay를 실행하고, sidecar 시작은 곧바로 복구·app-instance 기록을 쓴다. append와 lifetime batch도 기존 compaction 검증기를 호출한다. 따라서 frozen operation 루프 하나를 나눴다고 **전체 시작 비용이나 안전한 실행 허용까지 해결된 것은 아니다.**

이번 R1은 **내부 frozen-operation 재검증기**만 추가한다. DB schema V8, protocol, public bridge, 기존 open/append/lifetime admission은 바꾸지 않는다. 제품의 100개 초과 거부도 유지한다. 새로운 영속 schema나 V9 migration은 필요하지 않다.

## 2. R1: 연결에 귀속된 검증 세션

- 검증기는 같은 SQLite `Connection`을 독점 가변 차용한다. 외부에 connection·임의 cursor·검증 완료 플래그를 넘기지 않으며, 세션을 복제하거나 직렬화하지 않는다.
- cursor는 메모리에만 존재한다. 세션을 버리거나 프로세스를 다시 시작하면 반드시 처음부터 검증한다. DB에 `verified=true`나 재사용 가능한 reopen proof를 저장하지 않는다.
- 각 호출은 짧은 별도 `BEGIN IMMEDIATE` 안에서 동작하고 반환 전에 transaction을 끝낸다. 검증기가 호출 사이에 transaction을 새로 유지하지 않는다. 제품 connection이 기존 `locking_mode=EXCLUSIVE`로 보유하는 수명 전체의 DB 잠금까지 해제한다는 뜻은 아니다. 이미 caller transaction 안이면 세션 시작을 거부한다.
- 최초 transaction에서 exact V8 schema, admissible materialization pair, active compaction/delete authority 부재, 유일한 terminal-success private cutoff proof를 확인한다. schema bootstrap 비용은 호출당 row/byte 예산 밖이다. 이는 frozen source 검증의 선행 조건이지 bounded open이나 전체 reopen 성공 증거가 아니다.
- 최초와 이후 transaction 안에서 **같은 connection**의 `PRAGMA data_version`을 비교한다. 다른 연결이 commit했다면 이미 본 row를 포함한 전체 진행을 무효화한다. 독점 차용은 그 `Connection` 객체를 통한 다른 경로의 사용만 차단한다. 같은 프로세스에서 별도로 연 연결도 다른 연결이며, 그 commit 역시 `data_version`으로 감지한다.
- 취소는 현재 batch의 cursor를 전진시키지 않고 세션을 폐기한다. 검증·SQL·transaction 종료 실패나 commit 결과 불확실성도 해당 세션을 실패 상태로 닫는다. 다음 호출에서 예전 cursor로 자동 재시도하지 않는다.
- 정상 마지막 결과의 이름은 `FrozenOperationsVerified`다. **prefix/message/projection 검증 또는 제품 read/write 허용 token이 아니다.** 마지막 결과 뒤의 호출도 외부 변경을 감지해야 하며, 세션 바깥에 재사용 가능한 권한을 남기지 않는다.

`data_version`은 SQLite를 통하는 다른 연결의 commit 감지 수단이다. 실행 중 DB 파일을 SQLite 바깥에서 덮어쓰는 공격까지 막는 보장으로 설명하지 않는다. 재시작 시에는 새 세션의 처음부터 검증이 다시 필요하다.

## 3. 호출당 예산과 권위

| 항목 | 고정 규칙 |
| --- | --- |
| primary scan | `source_event_rowid > after`의 indexed keyset, `ORDER BY source_event_rowid LIMIT 100` |
| primary payload | frozen operation의 canonical source payload 합계 최대 1 MiB |
| 종속 입력 | frozen envelope·receipt·live event·source index 등 검증에 읽는 UTF-8 입력 합계 최대 8 MiB |
| 완료 판정 | 같은 transaction의 indexed successor 확인. 100개 처리 뒤 EOF까지 반복하지 않음 |
| 다음 row | 남은 호출 예산에 안 맞으면 읽기 검증 완료 row까지만 전진하고 다음 호출로 양보 |
| 단일 초과 row | 단독 호출 예산에도 안 맞으면 실패. 진행 없는 재시도를 반복하지 않음 |

종속 입력 8 MiB는 canonical source 1 MiB와 별개의 내부 allocation guard다. 중복 표현과 JSON envelope를 source payload에 중복 합산해 기존 source 한도를 임의로 줄이지 않는다. 문자열/JSON을 Rust 객체로 읽기 **전에** `length(CAST(column AS BLOB))` 등으로 입력 길이를 확인한다. 이 예산은 SQLite 전체 메모리나 실제 벽시계 지연의 상한을 주장하지 않는다.

검증 권위는 기존 V8 그대로다. canonical payload와 full record의 digest를 재계산하고 immutable receipt와 비교한다. 살아 있는 source는 exact event+source-index와 결속한다. 삭제된 source는 독립 frozen operation과 prefix coverage를 확인한다. receipt만으로 source identity를 복원하지 않는다. 모든 row를 메모리에 모으거나 `OFFSET`, 전체 집계/정렬, 전체-history loop를 추가하지 않는다.

## 4. 회귀·완료 기준

1. 100개 초과 정상 frozen operation을 여러 호출로 끝내며 각 호출의 row/byte 예산을 확인한다.
2. 누적 1 MiB 초과 데이터가 여러 호출에 나뉘고, 단일 exact 1 MiB는 허용하며 초과 row는 실패한다. 종속 입력 초과도 decode 전에 거부한다.
3. 첫 batch 이후 row의 payload/record digest, receipt, live source/source index 변조를 거부한다.
4. 다른 connection의 batch 사이 commit과 마지막 검증 뒤 commit이 기존 세션을 무효화한다. 새 세션은 0부터 시작한다. 제품 `configure_connection`의 `locking_mode=EXCLUSIVE`는 Rust 차용과 별개다. 외부 commit 회귀는 정상 잠금 모드의 exact V8 fixture connection으로 수행하고, 제품의 exclusive 잠금은 완화하지 않는다.
5. 취소와 오류에서 cursor가 잘못 전진하지 않고, DB schema·row·file bytes를 바꾸지 않는다.
6. 기존 제품 open/append/lifetime 경로는 그대로이고 100개 초과 전체 reopen은 계속 거부한다.
7. focused Rust, sidecar 전체, protocol/Node lifetime 회귀, scoped clippy, Rust 2021 format, diff check와 독립 리뷰·최신 통합 QA를 통과한다.

## 5. 뒤따를 작업과 남은 게이트

1. **R1 — operation 세션**: 위 내부 재검증기와 실패/예산 회귀. 제품 활성화 없음.
2. **R2 — 전체 검증 closure**: prefix·baseline·retained anchor·frozen message를 bounded 단계로 검사한다. message coverage는 전체 prefix 벡터 대신 indexed lookup을 사용하고 종속 payload 예산도 지킨다. source 누락과 extra derived record를 양방향 검증한다.
3. **R3 — startup/admission**: 기존 전체 integrity/replay, 복구·app-instance 쓰기, append/lifetime의 경계를 함께 다룬다. ready 게시 전 검증 완료와 첫 mutation transaction 사이 외부 변경 경주까지 닫고 나서야 100개 제한을 해제한다. durable cursor가 필요해져도 이전 프로세스의 완료 증거를 자동 신뢰하지 않는다.
4. **C1B2 및 제품 연결**: 실제 bounded compaction writer/delete, scheduler, `storage.status` wire/Electron, 다중 세션 QA를 각각 검토한다.

R1의 통과는 `G5D_READY`가 아니다. 실제 provider·사용자 프로젝트·Windows 시험, 물리 cleanup/보존 정책, `main` 병합·push도 이번 범위에 없다.

## 6. R2 — 압축 관련 데이터의 양방향 검증

### 6.1 범위와 완료 의미

같은 연결·변경 감지 수명을 유지하는 내부 R2 모드를 추가한다. R1의 operations-only 진입점과 기존 제품 경로는 보존한다. 같은 세션의 `start_compaction_closure`는 private immutable 모드와 `operations → prefixes → frozen messages` 진행을 선택하며 caller cursor/proof는 받지 않는다. 최종 결과는 정보성 `CompactionClosureVerified`이며, 전체 DB integrity·projection replay나 read/write admission 증거가 아니다. 새 schema, 영속 cursor, protocol 또는 사용자 설정을 추가하지 않는다.

여기서 양방향이란 **삭제된 frozen operation ↔ 정확한 sequence의 prefix/baseline**, **삭제된 chat introduction ↔ frozen message ↔ 해당 normalized message**다. prefix/message 전체를 각각 keyset으로 검사하여 source 누락과 불필요한 frozen 파생 기록을 거부한다. prefix에서 참조하지 않는 정상 historical baseline은 허용한다. 전체 `ledger_baselines`·`chat_messages`의 source-to-derived/derived-to-source replay는 R3 경계이며, R2에서 이를 완료했다고 주장하지 않는다.

### 6.2 단계와 검증 권위

| 단계 | primary keyset | 확인할 내용 |
| --- | --- | --- |
| operations | `frozen_operation_sources.source_event_rowid` | R1 전체 권위 검증과 cutoff 보존. R2에서 live event/index가 모두 없는 source에는 그 aggregate·sequence의 **정확한 prefix**가 필요. 삭제된 chat source에 한해 introduction은 정확한 frozen/normalized message를 요구하며, 같은 삭제 source의 non-introduction은 frozen message가 없어야 함 |
| prefixes | `ledger_compaction_prefixes.rowid` | prefix 형식·baseline identity/payload/digest·정확한 deleted frozen source·직전 prefix 연결·역사적 anchor frozen tuple을 검사. 가장 최신 prefix의 anchor만 live event/index까지 결속 |
| frozen messages | `frozen_message_sources.rowid` | 정확한 deleted introducing source·predecessor·같은 sequence prefix와 normalized message payload/digest/identity/createdAt을 검사. 전체 prefix 벡터 대신 indexed lookup |

모든 phase는 같은 최초 `data_version` 아래 동작한다. 호출 하나는 phase 하나만 처리하고, exhaustion을 확인한 transaction을 commit한 뒤에만 다음 phase를 게시한다. 빈 phase 전환도 외부 변경·취소 확인을 거친다. prefix는 최소 하나가 있어야 하며 message는 없어도 된다. 암묵적 rowid가 0 이하인 prefix/message가 양수 keyset 밖으로 숨지 않도록 indexed probe로 거부한다.

역사적 anchor는 삭제되어도 exact frozen aggregate·sequence(`deletedThrough + 1`)·event rowid/event ID/operation ID가 일치해야 한다. 최신 retained anchor는 기존 event/source-index/receipt/frozen binding 및 chat transition을 모두 확인한다. 삭제 source를 단지 더 큰 prefix가 덮는다는 이유로 통과시키지 않는다.

chat introduction 여부는 기존 protocol의 transition 규칙을 따른다. durable sequence 증가 시 정확한 새 TeamMessage가 필요하고, 증가가 없으면 snapshot은 null 또는 직전과 같을 수 있으며 introduction은 없다. `A 도입 → A 반복 → null → B 도입`을 허용한다. 현행 `expected_frozen_message`의 더 좁은 equality 조건을 그대로 복사하지 않으며, R2를 위해 기존 제품 동작을 몰래 변경하지 않는다.

### 6.3 호출당 예산

- primary 최대 100행. operations는 source canonical payload, prefixes는 참조 baseline canonical projection, messages는 canonical message를 합해 각각 호출당 최대 1 MiB다.
- 별도 encoded closure 8 MiB에는 해당 primary와 **실제 읽는 모든 종속 UTF-8 입력**을 포함한다. frozen source/envelope, baseline, predecessor, anchor, receipt, event/index, frozen/normalized message와 식별자도 decode 전에 길이를 확인한다. 동일 데이터를 여러 경로에서 읽으면 그 읽기 비용을 보수적으로 계산한다.
- 각 primary의 종속 조회는 고정 수의 indexed exact lookup/`LIMIT 1`이어야 한다. 전역 prefix 수집, `OFFSET`, 전체 집계/정렬, recursive history traversal을 추가하지 않는다.
- 남은 예산만 초과하면 다음 호출로 양보한다. 단일 primary의 closure가 자체 예산을 초과하면 terminal failure이며 zero-progress retry를 만들지 않는다. 각 phase의 successor도 indexed query로 확인한다.
- R1과 동일하게 entry/row/precommit 취소, SQL·COMMIT 오류, 다른 연결의 commit은 세션 전체를 무효화한다. 새 세션은 operations의 0부터 시작한다. 호출 사이 transaction을 보유하지 않는다.

### 6.4 구현·검토 합격 기준

1. 101개 이상의 prefix와 frozen message를 실제 terminal-compacted 임시 fixture로 검증하고, 각 단계의 row/primary/closure 예산과 단계 순서를 검사한다.
2. baseline payload/digest·삭제 source·prefix chain·역사적/최신 anchor·message identity/digest/normalized payload의 첫 행과 후반부 변조를 거부한다. 필요한 frozen source/message 누락과 non-introduction의 extra frozen message도 거부한다.
3. null/repeat/introduction의 정상 chat 전이, 메시지가 없는 project 압축, 참조되지 않는 정상 historical baseline은 유지한다.
4. primary exact 1 MiB/초과, 종속 UTF-8 초과와 합계에 따른 양보를 검사한다. phase별 주요 SQL의 query plan이 keyset/exact index를 사용함을 확인한다.
5. phase 전환 및 late batch 사이 외부 commit·취소·transaction 실패 뒤 재개 불가와 DB schema/rows/file bytes 무변경을 확인한다. 기존 R1 회귀와 제품 100개 초과 거부도 유지한다.
6. focused tests, full sidecar/stdio, protocol·Node lifetime, scoped clippy, Rust 2021 format, diff check, 독립 코드 리뷰와 최신 통합 QA를 통과한다. 환경 문제로 실행하지 못한 전체 앱/누적 하네스는 별도 미완료로 기록한다.

단일 Rust 담당이 `storage/reopen.rs`와 그 하위 모듈·`storage/tests.rs`의 회귀를 소유한다. 기존 `storage.rs` helper의 추출이 필요하면 동작 보존에 한정한다. schema/protocol/lockfile, startup/append/lifetime admission, writer/delete/scheduler/status, Electron/UI는 변경 금지다. 이 경계를 넘겨야 하면 구현을 멈추고 계약 담당에게 보고한다.

## 7. R3 — 전체 재오픈과 실행 허용 연결

### 7.1 완료선과 구현 순서

R3의 완료선은 내부 검증 결과 추가가 아니라 **100개 초과 압축 기록의 제품 재오픈 → 복구 → app-instance 기록 → 정상 후속 append/lifetime 처리**다. 같은 DB의 손상·검증 후 외부 변경은 첫 쓰기와 `system.ready` 전에 거부해야 한다. 이를 다음 순서로 구현·독립 검토한다. 하위 단계 통과를 R3 또는 G5D 완료로 올리지 않는다.

1. **R3A: 원장과 projection 검증** — 전체 `Vec` replay 대신 indexed predecessor와 마지막 snapshot을 사용해 기존 결정적 replay와 같은 결과를 bounded scan으로 검증한다. R2 압축 closure와 같은 연결·변경 감지 수명에 묶는다. 제품 경로는 아직 바꾸지 않는다.
2. **R3B: 파생 기록 검증** — source index·receipt·normalized message·historical baseline·workspace 등 저장된 파생 기록을 원본과 양방향 대조한다. 기존 materialization 상태에 따라 아직 없는 기록과 손상된 기록을 구분한다.
3. **R3C: startup/admission 통합** — 기존 SQLite integrity/FK 검사와 복구 대상의 읽기 전용 사전 검증을 완료한 뒤, Storage 내부의 같은 연결에만 귀속된 실행 허용 상태를 만든다. open·복구·app-instance·append·lifetime이 이를 실제로 소비하도록 연결한다. 검증 완료와 첫 mutation 사이, 이후 mutation 사이의 변경 경주 및 같은 연결 내부 쓰기도 다룬다.

R3B/C의 구체적인 쓰기 계약은 R3A의 검토 결과를 반영해 별도로 고정한다. R3A에 후속 단계의 공개 API·영속 proof·실행 허용 boolean을 미리 만들지 않는다. 이전 프로세스의 완료 표시나 materialization `ready` 자체는 검증 증거가 아니다.

### 7.2 기존 안전·호환 계약

- `pending + absent` 또는 `pending + pending`은 migration/append 이후 가능한 정상 상태다. append는 event와 source index를 쓰고 기존 `ready`를 `pending`으로 돌리지만 모든 receipt/message/baseline을 즉시 만들지 않는다. 따라서 pending에서 모든 파생 기록의 존재를 무조건 요구하지 않는다. **존재하는 기록은 항상 원본에 결속하고**, ready라면 해당 단계가 약속한 전체 coverage를 검사한다. 압축으로 삭제된 source에 필요한 frozen/prefix/message closure는 pending이어도 생략하지 않는다.
- 현행 startup은 저장소 복구를 먼저 쓰고 actual-run 검증을 뒤에 한다. R3C는 ledger/projection 일치만으로 복구 의미까지 검증했다고 주장하지 않는다. 복구 candidate와 연결된 chat/project, actual persisted evidence를 읽기 전용으로 먼저 검사하고 DB/파일시스템 변경은 그 뒤에 실행한다. poisoned·identityless recovery의 잠금과 fail-closed 동작을 유지한다.
- `PRAGMA integrity_check`와 `foreign_key_check`는 SQLite 전체 엔진 검사이며 100행/1 MiB batch라고 설명하지 않는다. 기존 검사를 생략하거나 `quick_check`로 바꾸지 않는다. 중단 시 완료로 처리하지 않으며 재시도는 처음부터다.
- Electron의 현행 ready hard timeout 3초와 ready 이전 실패 시 중단은 유지한다. 충분한 데이터로 실제 시작 시간을 측정한다. 전체 검증을 지키면서 이 계약을 충족할 수 없다면 별도 시작 진행 표시·시간 제한 변경안을 사용자에게 승인받는다. 검증 전 ready나 임의 timeout 연장으로 우회하지 않는다.
- schema V8·protocol·사용자 설정·exclusive SQLite 잠금·권한은 유지한다. 실제 compaction writer/delete, scheduler/status, 실제 provider·사용자 프로젝트·Windows 시험은 별도 게이트다.

### 7.3 R3A의 원장·projection 동등성

현행 replay는 각 aggregate의 마지막 canonical snapshot에 `last_event_sequence`를 넣는 upsert다. R3A는 이 성질을 사용하되 원장 전체를 읽거나 aggregate별 전체 이력을 재귀 탐색하지 않는다.

- R2의 exact schema/control·압축 closure를 같은 private 세션에서 검증한 뒤 live events와 다섯 projection table을 각각 keyset으로 스캔한다. 정상 uncompacted/empty V8에는 terminal-compaction proof를 요구하지 않으며, 압축 관련 table의 bounded exact-empty 확인 없이 R2를 건너뛰지 않는다. app-instance는 frozen source/prefix를 허용하지 않는다. 기존 R1/R2의 진입점·결과·제품 동작은 보존한다.
- live event마다 기존 event 형식·canonical payload·aggregate ID를 검사한다. sequence 1부터 시작하거나 검증된 최신 compacted prefix 바로 다음 retained anchor여야 한다. 이후 source는 같은 aggregate의 정확한 sequence-1 predecessor와 이전 rowid 순서로 결속한다. gap·역순·prefix 아래 남은 live source·anchor 누락을 거부한다. prefix/baseline의 신뢰는 R2가 같은 세션에서 검사한 사실에만 의존한다.
- 각 aggregate의 마지막 live event는 반드시 해당 projection과 일치해야 한다. projection 쪽도 마지막 source를 역조회해 extra projection·누락·stale sequence를 거부한다. 프로젝트/채팅/실행/잠금의 JSON과 별도 indexed column, app-instance의 모든 typed column을 현재 replay/column 검사와 같은 의미로 비교한다. JSON의 공백이나 키 순서 차이를 새로운 손상 기준으로 만들지 않는다.
- source index가 존재하면 exact source 및 chat transition과 결속하고, ready의 누락 index는 거부한다. pending의 미생성 index는 허용한다. predecessor index가 없으면 exact live predecessor에서 비교에 필요한 사실을 구성하며 receipt나 현재 projection을 predecessor 권위로 사용하지 않는다. 전체 index의 orphan/역방향 coverage와 나머지 파생 table 검증은 R3B다. chat의 null-or-same snapshot 규칙을 유지한다.
- 모든 primary scan은 rowid keyset 최대 100행이며 0 이하 rowid를 숨길 수 없어야 한다. primary canonical 입력 합계 1 MiB, 별도 실제 읽는 종속 UTF-8 합계 8 MiB를 **decode 전** 검사한다. projection phase는 저장 snapshot(typed app-instance는 해당 필드 입력)을 primary로 계산한다. exact/최신 indexed lookup과 고정 수의 종속 입력만 허용하며 `OFFSET`, 전체-history `Vec`/map/set, 전역 정렬/집계를 추가하지 않는다.
- 같은 연결 독점 차용, 매 batch `BEGIN IMMEDIATE` 안의 최초 `data_version` 대조, commit 후에만 cursor/phase publish, 취소/오류/COMMIT 불확실성 후 전체 세션 폐기를 유지한다. 마지막 결과도 정보성이며 Storage가 받는 admission token이 아니다.

### 7.4 R3A 합격 기준과 소유권

1. 빈/미압축/pending/ready V8, aggregate가 섞인 100행 초과 live ledger와 다섯 projection, 100개 초과 prefix/message의 정상 압축 fixture를 여러 호출로 완료한다. 작은 정상 fixture의 판정은 기존 deterministic replay와 대조한다.
2. 첫 행과 후반부의 payload/ID·sequence gap·역순 rowid·누락/extra/stale projection·indexed column/evidence 불일치·source index/chat transition 변조를 거부한다. 삭제 source와 retained anchor 경계를 검사한다.
3. 100행·exact 1 MiB/+1·8 MiB/+1·다중 바이트 입력·남은 예산 양보를 검증하고, 주요 SQL query plan의 keyset/exact index 사용을 확인한다. 예산을 넘긴 row는 decode하거나 cursor를 전진시키지 않는다.
4. phase 전환·late batch·마지막 완료 뒤 외부 commit, entry/mid/precommit 취소, 실제 COMMIT 거부 후 재사용 불가와 DB schema/rows/file bytes 무변경을 확인한다.
5. 기존 R1/R2, focused Rust, full sidecar/stdio, protocol/Node lifetime, scoped clippy·Rust 2021 format·diff check, 독립 exact-commit 리뷰와 최신 통합 QA를 통과한다. 제품의 100개 초과 거부와 기존 startup/append/lifetime 경로는 그대로임을 확인한다.

단일 Rust 구현 담당이 `storage/reopen.rs` 및 하위 모듈, `storage/tests.rs` 및 하위 회귀, 자기 branch의 `docs/history.md`를 소유한다. `storage.rs`는 module 등록·설명 또는 동작 보존 helper 추출만 허용한다. schema/protocol/golden/의존성/lockfile, 실제 open/append/lifetime·복구 admission, Electron/UI, scheduler/writer/status는 변경 금지다. 통합 담당은 이 문서·현행 구조·색인·통합 워크로그를 소유한다. 경계를 넘겨야 하면 중단하고 보고한다.

### 7.5 R3B 후속 조사 — 아직 구현 계약 아님

R3A 통합 `63ab510`에서 아래 조건을 조사했다. 다음 파동은 이를 포함한 구체 계약을 독립 검토·커밋한 뒤 구현한다. R3B 구현이나 제품 실행 허용을 이 절로 승인하지 않는다.

- pending의 source index와 normalized message가 모두 없어도 **메시지 도입 ID의 전역 유일성**은 기존 요구다. protocol의 `introducedMessageIdentityConflict=rollbackWholeAppend`, V6 unique index와 서로 다른 chat의 ID 재사용 거부 회귀가 근거다. 반복 snapshot은 새 도입이 아니다. 각 source/predecessor만 보는 R3A는 이 전역 검증을 대신하지 않는다.
- 없는 index를 찾는 경로는 raw source의 명시적 중첩 keyset과 종료 조건이 필요하다. JSON 조건으로 전체 이력을 탐색하면서 `LIMIT`만 붙이는 방식은 bounded scan이 아니다. 유일성 확인 후 message association은 정확한 도입 source를 찾거나 keyset 끝까지 검사해야 한다. 한 outer record를 호출 사이 유지한다면 retained encoded bytes도 매 호출 종속 예산에 포함하며, 외부 commit·취소·오류 때 모두 폐기한다. 최악의 유일성 탐색은 `미색인 도입 수 × live event 수`일 수 있어 호출당 예산을 전체 시작 시간 보장으로 해석하지 않는다.
- pending workspace는 최신 project append보다 오래된 정상 파생 기록일 수 있다. exact historical live/frozen source를 bounded하게 찾는 규칙과, bound chat의 **최신 raw live source**를 쓰는 composite identity 검증을 명시해야 한다. ready는 최신 project member→row와 row→최신 member를 모두 확인해 누락·stale·extra를 거부한다. 100개 초과 workspace member도 내부 cursor·예산 검증 대상이다.
- **별도 lifecycle 결함**: 현행 `storage_lifetime.rs`의 bounded `materialize_workspace_event`는 기존 row를 갱신하기 전에 latest-only `source_workspace_matches`를 호출한다. 따라서 R3B가 정상 historical pending row를 받아들여도 이후 catch-up이 실패할 수 있다. 이는 source 대조로 확인한 경로이며 이번에 재현 테스트나 수정을 수행하지 않았다. R3B의 읽기 검증 성공을 catch-up 호환성으로 주장하지 않고, R3C/lifecycle 계약에서 재현·수정·검증한 뒤 제품 활성화를 판단한다.

## 8. R3B — 전역 파생 기록 검증 계약

### 8.1 완료선·비활성 경계

R3A의 독립 gate·마감 기준 `5cdd5dd`에서 이어간다. 같은 private 세션에 새로운 immutable mode를 추가해 R2/R3A를 먼저 끝내고, 저장된 source index·receipt·message·baseline·workspace 전체를 source와 양방향으로 결속한다. 최종 결과는 정보성 결과이며 기존 R1/R2/R3A 진입점·결과와 제품 open/복구/append/lifetime 경로는 바꾸지 않는다. R3C admission, 실제 100개 초과 제품 재오픈과 catch-up, compaction writer/delete·scheduler/status는 이번 완료선이 아니다.

### 8.2 파생 기록과 coverage

| 기록 | 존재하는 모든 행의 역방향 검증 | ready에서 추가로 요구하는 정방향 coverage |
| --- | --- | --- |
| `event_source_index` | exact live event rowid·event/operation/aggregate tuple 및 source-backed predecessor/introduction 전이 | 모든 live event의 exact index |
| `operation_receipts` | exact live source 또는 같은 세션에서 R2로 검증한 frozen operation; 기존 request ID 제외·duplicate classifier 유지 | app-instance를 포함한 모든 live event의 exact receipt |
| `chat_messages` | 반복 snapshot이 아닌 exact introduction과 typed fields·canonical JSON·digest 결속 | 모든 live introduction의 exact normalized message |
| `ledger_baselines` | exact historical source tuple·deterministic ID·payload·digest; 정상 unreferenced historical baseline 허용 | latest project/chat/run/lock source의 exact baseline; app-instance baseline 금지 |
| `prepared_workspaces` | exact project source member와 typed fields·digest·composite identity | 모든 latest project member→exact row와 모든 row→latest member; 누락·stale·extra 거부 |

pending의 미생성 기록은 허용하되 존재하는 기록을 receipt 자체·timestamp·현재 projection만으로 신뢰하지 않는다. ready는 검증 의무이지 검증 증거가 아니다. 삭제 source에 필요한 R2 frozen/prefix/message closure는 두 상태 모두 필수다. 위 모든 table의 primary keyset을 끝까지 검사하며 0 이하 rowid가 숨지 않아야 한다.

receipt와 baseline의 live source는 `event_source_index` 존재를 전제로 찾지 않는다. raw `events` unique `(aggregate_type, aggregate_id, aggregate_sequence)`의 exact lookup으로 먼저 결속하고, 해당 live source가 없을 때만 같은 세션의 R2-verified frozen operation으로 대조한다. source-index 자체의 역검증은 raw event rowid와 exact tuple을 함께 확인한다. V5→V6에서 기존 receipt/baseline은 남고 index는 비어 있을 수 있으므로 기존 index-first helper를 그대로 호출해 정상 pending을 거부하지 않는다.

### 8.3 미색인 메시지와 전역 유일성

- 메시지 `event_sequence`는 chat durable sequence이며 aggregate source sequence로 직접 사용하지 않는다. 기존 null-or-same 전이와 반복 snapshot을 보존한다.
- **모든 live introduction**은 index 유무와 관계없이 exact indexed introduction/frozen message identity를 대조하고 같은 ID의 다른 introducing source를 거부한다. 이미 indexed된 live introduction과 frozen source 사이의 충돌도 포함한다. 기존 `event_source_index_rejects_frozen_message_reuse` trigger가 정상 INSERT를 보호하지만, 손상된 저장 상태의 재검증을 대신하지는 않는다.
- index가 없는 introduction은 여기에 더해 **전체 live event의 raw rowid keyset** 중첩 탐색으로 같은 ID를 도입한 다른 source가 없는지 검사한다. index/normalized row가 둘 다 없는 두 introduction도 거부해야 한다. 후보마다 exact source/predecessor 전이를 판정하며, 같은 source와 반복 snapshot은 충돌로 오인하지 않는다. 충돌은 발견 즉시 거부하며, 충돌이 없다는 판정은 keyset exhaustion까지 검사해야 성립한다. 이미 indexed된 introduction끼리는 exact schema의 unique index와 각 index/source 결속을 함께 사용한다.
- 이 유일성 단계가 같은 unchanged 세션에서 완료된 뒤 normalized message 역검증을 진행한다. exact indexed/frozen source로 해결되지 않으면 raw `events`의 `(aggregate_type, aggregate_id, aggregate_sequence)` unique index를 이용한 per-chat sequence keyset으로 정확한 introduction을 찾거나 끝까지 검사한다. 첫 nonmatch/page에서 멈추지 않고, match에서 멈추는 근거도 앞선 전역 유일성 검증이다.
- JSON 조건으로 전체 이력을 탐색하면서 matching row에만 `LIMIT`를 붙이는 쿼리, `OFFSET`, 전체-history `Vec`/map/set, TEMP index/schema 변경은 금지한다. candidate가 실제로 방문되는 raw keyset·exact lookup과 query plan을 증거로 남긴다.

### 8.4 pending workspace와 가변 member

- pending workspace가 최신 project append보다 오래됐다면 **exact historical live 또는 R2-verified frozen project source member**와 결속되는 경우만 허용한다. 이 source 발견은 live project events와 frozen project operations 각각의 indexed aggregate-sequence keyset·별도 private cursor로 처리한다. 후보 source/member를 JSON 조건 뒤에 숨기지 않는다. ready의 역검증은 historical match가 아니라 최신 member와 같아야 한다.
- bound workspace의 `boundChatId`는 raw `events` unique `(aggregate_type, aggregate_id, aggregate_sequence)` index에서 찾은 **최신 live chat source**의 project/worktree/workspace identity와 일치해야 한다. pending source-index row 부재는 허용하지만 projection이나 임의 과거 chat source를 권위로 바꾸지 않는다. compacted aggregate는 retained live anchor/tail을 유지하므로 현재 exact live source가 없으면 거부한다.
- 하나의 정상 project payload에도 100개 초과 workspace member가 들어갈 수 있다. member 검증과 가변 bound-chat lookup은 명시적 inner cursor·공유 작업 예산을 사용한다. 기존 all-members helper를 고정 비용 dependency처럼 호출하지 않는다.
- 이 규칙은 **읽기 검증의 정상 pending 수락**이다. §7.5의 기존 bounded materializer 갱신 전 latest-only 대조 문제를 해결했다고 주장하지 않는다. R3C/lifecycle에서 재현·수정·검증하기 전에는 product catch-up/admission으로 연결하지 않는다.

### 8.5 호출당 예산·volatile 진행

- primary scan은 최대 100행이며, outer row·중첩 source 후보·가변 member 처리도 합쳐 **최대 100 work units**를 소비한다. 고정 개수의 exact dependency는 조회 graph를 명시하고 별도 byte 예산으로 제한한다. 같은 후보의 재조회도 실제 읽기 비용에서 빼지 않는다.
- fresh primary canonical 입력 합계는 호출당 1 MiB다. receipt/message/baseline/workspace는 해당 저장 canonical JSON, source 후보는 canonical event payload를 계산한다. canonical JSON column이 없는 source-index phase는 결속할 exact source의 canonical payload를 primary로 계산하며 index의 모든 UTF-8 column은 별도 closure에 포함한다. 각 SQL String/JSON 입력은 decode 전에 BLOB byte-length로 검사한다.
- 호출 사이에는 **bounded outer record 하나**만 유지할 수 있다. 이를 처음 읽은 호출에 primary 비용을 청구하며, 다음 호출에서 다시 읽지 않은 canonical bytes를 fresh primary로 이중 청구하지 않는다. 대신 retained encoded outer bytes는 **매 호출** fresh candidate/dependency 입력과 함께 8 MiB closure 예산에 포함한다. 중복 decoded tree와 무제한 과거 기록을 보관하지 않는다.
- 다음 후보가 retained outer를 포함한 fresh 호출에는 들어가지만 남은 예산에는 안 들어가면 미소비 상태로 양보한다. retained outer와 합친 intrinsic fixed closure가 8 MiB를 초과하면 terminal failure다. outer 완료 전에도 유효한 inner 진행은 commit 후 유지할 수 있으며 zero-progress retry를 만들지 않는다. 재시작한 호출의 retained outer+후보도 함께 preflight한다.
- 같은 mutable connection 독점 차용·매 `BEGIN IMMEDIATE` 내부 최초 `data_version` 대조·commit 뒤에만 outer/inner cursor publish를 유지한다. 취소·SQL/COMMIT 오류·외부 commit은 retained row와 모든 진행을 폐기하고 재사용을 거부한다. 호출 사이 transaction, 영속 cursor/proof, public admission token을 만들지 않는다.
- 최악의 유일성 탐색 비용은 `미색인 introduction 수 × live event 수`이며 message/workspace resolver 비용도 별도다. 호출당 상한을 전체 시작 시간 상한으로 주장하지 않는다. R3C의 SQLite 전체 integrity/FK와 Electron ready 3초 계약은 그대로이며 early-ready나 임의 timeout 연장은 금지한다.

### 8.6 합격 기준·소유권

1. empty/pending/ready·compacted V8와 다섯 derived table 각각 100행 초과 정상 fixture를 다회 완료한다. 정상 historical pending workspace·unreferenced baseline·null/repeat message를 유지한다. V5→V6 migration처럼 index가 전부 없고 receipt/baseline은 남은 pending에서 raw live source 결속과 필요한 frozen fallback을 검증한다.
2. 첫 행과 후반부의 orphan·typed/indexed/canonical/digest/identity 불일치, ready 누락과 pending 미생성의 차이, live/frozen/anchor 경계를 검사한다. ready workspace는 member 누락·extra/stale row를 양방향으로 거부하고 100개 초과 member를 포함한다.
3. 서로 다른 chat에서 100개 초과 후보 간격으로 떨어진 동일 ID의 unindexed introduction을, 양쪽 normalized row도 없는 상태에서 거부한다. indexed↔unindexed·frozen↔unindexed·indexed-live↔frozen 충돌과 정상 repeated snapshot을 구분한다. indexed-live↔frozen은 기존 INSERT trigger를 우회해 손상 상태를 준비한 뒤 exact schema를 복원한 fixture로 재검증하며, 정상 append가 ID 재사용을 허용한다고 주장하지 않는다. per-chat 100개 이후 match와 source absence의 exact exhaustion, source-index가 전부 없는 pending bound workspace의 최신 raw chat 결속을 검사한다.
4. primary exact 1 MiB/+1·closure exact 8 MiB/+1·멀티바이트·남은 예산 양보·retained outer와 큰 source의 호출 분리·100 work units·no-progress 방지와 실제 query plan을 확인한다. private phase probe와 full-session earliest rejection을 구분한다.
5. 각 phase·nested late/terminal 경계에서 외부 commit·entry/mid/precommit 취소·실제 SQL read/COMMIT 거부·busy BEGIN 뒤 전체 진행 폐기 및 DB schema/rows/file bytes 무변경을 검사한다.
6. 새 focused tests, 기존 R1/R2/R3A, full sidecar/stdio, Node/protocol lifetime, scoped strict clippy·Rust 2021 format·diff check, 독립 exact-commit 코드 리뷰와 최신 통합 QA를 통과한다. 누적 하네스는 `CI=1` 환경을 사용하고 환경 제한·미실행 외부 게이트는 별도 기록한다.

단일 Rust 구현 담당이 별도 `m5/g5d-v8-reopen-derived` branch·`.worktrees/g5d-v8-reopen-derived`에서 `storage/reopen.rs`와 하위 모듈, storage test 하위 모듈 및 module 등록, 자기 branch의 `docs/history.md`를 소유한다. `storage.rs`는 동작 보존 helper 추출·설명만 허용한다. 통합 담당은 계약·현행 구조·색인·통합 기록을 소유한다. schema/protocol/golden/의존성/lockfile·제품 open/복구/append/lifetime·기존 materializer catch-up 수정·scheduler/writer/status·Electron/UI는 변경 금지다. 소유 밖 수정이나 안전 계약 변경이 필요하면 구현을 멈추고 보고한다. 실제 provider·사용자 프로젝트·Windows·파괴적 시험, main 병합·push는 수행하지 않는다.

R3B 구현은 exact 통합 `e5f9f01c397f3482975a231621ee5b16aa4381e8`에서 누적 13단계·독립 QA·최종 gate를 통과했다. 현행 구조는 `../architecture.md` §9, 검증 기록은 `../history.md`와 `.artifacts/m5-qa/e5f9f01c397f3482975a231621ee5b16aa4381e8/manifest.json`에서 확인한다. 이 완료는 내부 정보성 검증기에 한정하며 제품 admission은 승인하지 않는다.

## 9. R3C 활성화 전 조사 — 아직 구현 계약 아님

`da539a6`의 제품 경로를 R3A/B 계약과 대조한 후속 조사다. 아래 호환성·순서 조건은 R3B 구현 범위나 기존 안전 예산을 바꾸지 않는다. 제품 활성화 전에 재현 fixture·별도 계약·독립 검토로 닫아야 한다.

- **기존 허용 입력과 단일 snapshot 크기**: `agent_runtime.rs`의 `valid_public_ledgers_reach_count_and_text_boundaries_but_reject_plus_one`은 16개 × 64 KiB public text를 정상으로 허용한다. `agent_run_projection`과 `persist_public_event`는 그 전체 배열에 run/manifest 등의 metadata를 더해 하나의 run snapshot으로 저장하므로 canonical payload는 1 MiB를 넘는다. `storage_v1.rs::validate_ledger_event`에는 그 canonical byte 상한이 없다. 따라서 R3A/B의 indivisible primary 1 MiB를 그대로 제품 admission에 연결하면 기존 budget-valid 입력을 거부할 수 있다. §10의 test-only 재현은 exact 통합 `73b2d61297b376637f5cc6794057f325652ba450`에서 일반 atomic 저장·Storage reopen 성공과 full private 검증 거부, 작은 대조군의 완료를 독립 QA·최종 gate로 확인했다. 저장 표현·검증 단위·이전 기록 호환 정책을 결정해야 하며, G5B 허용량 축소나 R3A/B 예산 확대를 이번 조사로 승인하지 않는다.
- **읽기 검증과 쓰기 predecessor 권위의 차이**: R3B 계약은 pending의 index 부재를 raw source 검증으로 허용하도록 정했지만, chat append는 exact indexed live predecessor 또는 검증된 deleted frozen predecessor를 요구한다. `storage_lifetime_v1.rs`의 기존 mutation 계약은 raw live fallback을 금지한다. 따라서 읽기 admission만으로 pending chat recovery가 성공한다고 할 수 없다. 필요한 index catch-up을 검증 후 affected write 전에 완료하거나 기존 거부를 유지하는 순서를 명시해야 한다. 읽기 검증에서 합성한 predecessor를 그대로 쓰기 권위로 넘기지 않는다.
- **project snapshot도 별도 누적 경로**: `chat.rs`의 준비 생성은 project의 `prepared_workspaces` map에 계속 항목을 더하고, abandon도 삭제하지 않고 terminal 항목으로 교체·보존한다. 조사한 생산 helper에는 전체 항목 수나 canonical byte 상한이 없어 개별 항목이 정상이어도 단일 project snapshot이 커질 수 있다. R3B의 member cursor는 전체 primary payload의 1 MiB preflight 뒤에 실행되므로 이 문제를 대신 해결하지 않는다. 실제 경계 persistence/reopen fixture는 아직 없으며 §10의 run 진단 범위에 추가하지 않는다. run metadata·manifest·lock의 넓은 validator 허용 범위는 정상 생산값의 초과 증거와 구분한다. 예를 들어 path 타입에 명시적 byte cap이 없더라도 실제 manifest 파일 reader에는 64 KiB cap이 있다. 다음 호환 설계는 public events 배열만 줄이는 것으로 전체 정상 입력 문제를 해결했다고 주장할 수 없다.
- **첫 변경 이전의 복구 검증**: exact V8 open은 integrity/FK/replay 전에 WAL 전환을 요청하고, generic recovery 뒤에도 managed directory 생성·actual reservation/manifest 쓰기·legacy manifest 쓰기가 전체 recovery semantic 검증보다 앞선다. generic recovery는 integrity/replay보다 뒤라는 사실을 구분한다. 순수 preflight는 prospective generic recovery와 정상 older-record reservation 보완을 반영하고, 모든 대상의 검증 뒤 mutation을 허용해야 한다. filesystem preflight는 잠금이 아니므로 mutation 시 identity/lifecycle 재검증도 유지한다. fresh/recognized legacy/exact V8/unsupported 저장소를 구분하며 기존 개별 migration transaction을 하나의 rollback 단위라고 표현하지 않는다. SQLite 자체 crash-recovery 파일 효과의 byte-zero 보장은 아직 조사하지 않았다.
- **전체 시간은 별도 증거**: 미색인 전역 탐색·전체 integrity/FK·catch-up·복구를 포함한 실제 시작 시간을 측정해야 한다. 호출당 예산만으로 현행 ready 3초를 보장하지 않으며 SQLite busy timeout 5초도 성공 보장이 아니다. 검증 전 ready, 임의 timeout 변경, 제품 100-operation fence 해제는 승인되지 않았다.

순서 조사와 코드 위치는 `.artifacts/m5-review/g5d-r3c-startup-discovery.json`에 보조 증거로 남긴다. §7.5의 historical workspace catch-up 재현·수정과 함께 후속 계약의 선행 조건이며, 이 절은 R3C 완료나 구현 착수 승인이 아니다.

## 10. R3C 연결 전 저장 snapshot 호환성 진단 — 테스트 전용 계약

§9의 정상 입력 크기 문제를 재현하는 한정된 후속 작업이다. R3B의 exact 통합 QA·최종 gate 이후 이 계약을 검토·커밋하고 별도 기능 worktree에서 시작한다. 제품 admission·새 저장 표현·용량 정책 변경을 승인하지 않는다.

### 10.1 기준·소유·금지 영역

- 기준 코드는 R3B 통합 `e5f9f01c397f3482975a231621ee5b16aa4381e8`이다. 실제 기능 branch는 이 계약을 포함한 최신 clean 통합 commit에서 생성하며 exact SHA를 작업 계약·검증 manifest에 기록한다.
- 단일 Rust 담당은 `crates/hello-agent-sidecar/src/agent_runtime.rs`의 기존 `#[cfg(test)] mod budget_tests`와 자기 branch의 `docs/history.md`만 소유한다. production helper는 그대로 호출하며 접근 범위를 늘리지 않는다.
- production 코드·visibility·schema/protocol/golden·의존성/lockfile·Storage open/append/recovery·예산·기존 100-operation fence·Electron ready 시간은 변경 금지다. 소유 밖 수정이나 정책 결정이 필요하면 구현을 멈추고 보고한다.
- 임시 DB만 사용한다. 실제 provider 실행·로그인·사용자 DB/프로젝트·Git 작업 복사본·ProjectManager·Windows·파괴적 시험은 하지 않는다.

### 10.2 정상 저장 fixture

기존 `public_event`, `public_budget_metrics`, `agent_run_record`, `agent_run_projection`, `chat_projection_with_record`를 사용한다. 기존 golden의 `chat-record`, `prepare-b-is-distinct-from-bound-a` standalone manifest, `actual-run-persists-without-fixture-scenario` readOnly selected profile을 복사·재결속하되 golden 파일은 고치지 않는다.

TempDir의 project/chat/최초 사용자 메시지/readOnly actual run은 public event의 `*_a` 식별자와 일치해야 한다. chat/run/manifest 형식과 profile·workspace·run 결속을 독립 검사한다. provider discovery와 runtime attestation을 생략한 형태는 기존 `validate_provider_projection`이 허용하는 **pre-G5C 저장 기록 호환 형태**다. 이를 현재 실제 provider의 실행 전 검증 증거로 표현하지 않는다.

public event 하나마다 전체 run 배열 snapshot과 대응하는 유효한 TeamMessage를 production helper로 만들고 일반 `Storage::upsert_projection_records_atomic`으로 함께 저장한다. 최초 사용자 메시지 뒤 chat durable/message sequence가 단조 증가해야 한다.

| 사례 | 입력 | 요구 결과 |
| --- | --- | --- |
| ASCII 최대 text | 16개 × 65,536바이트 `x` | raw text 1,048,576바이트, metadata 포함 단일 canonical run snapshot은 1 MiB 초과 |
| JSON escaping | 9개 × 65,536바이트 역슬래시 | raw text 589,824바이트인데 encoded snapshot은 1 MiB 초과 |
| 작은 정상 대조군 | 작은 text의 event 16개 | 모든 run source canonical payload가 1 MiB 이하이고 전체 private R3B 완료 |

모든 event와 모든 prefix가 기존 public validation·1 MiB text·256 event·16 MiB 누적 snapshot 예산을 실제 통과해야 한다. escaping의 encoded 크기를 추정치만으로 판정하거나 16개 escape-heavy event도 허용된다고 가정하지 않는다.

### 10.3 서로 구분해 입증할 결과

1. **기존 저장 허용**: 모든 atomic write가 `Committed`이고 `AgentRuntime::blocked().validate_persisted_runs`가 성공한다. checkpoint/drop/일반 `Storage::open` 뒤 최신 run payload·계수·chat sequence가 같고 persisted-run validation도 다시 통과한다. 이 결과는 **Storage reopen**이며 Sidecar 시작·복구·ready·provider 성공이 아니다.
2. **크기 귀속**: 저장된 exact event rowid의 `length(CAST(canonical_json_payload AS BLOB))`를 측정하고 첫 oversized row를 기록한다. 작은 대조군도 입력 문자열 길이뿐 아니라 가장 큰 persisted run-event payload가 1,048,576바이트 이하임을 확인한다.
3. **전체 private verifier의 호환성 거부**: Storage를 checkpoint/drop한 뒤 같은 임시 DB에 설정된 별도 connection으로 `start_derived_records`를 호출한다. 시작은 성공해야 하며 0회 이상의 Pending 뒤 `storageSchemaMismatch`로 종료하고 재사용을 거부해야 한다. 관측된 모든 Pending은 `DerivedVerificationPhase::Prerequisite(LedgerProjectionPhase::Projection(ProjectionPhase::Ledger))`여야 한다. `DerivedRecordsVerified`나 다른 단계의 오류로 이 크기 문제를 입증하지 않는다.
4. **정확한 실패 원인·대조**: 기존 test-private `projection_batch_for_test`를 oversized source 직전에서 실행해 그 행의 byte preflight 거부를 별도 입증한다. 전체 session의 최초 실패와 phase-local 귀속을 혼동하지 않으며 heap/decode 계측을 했다고 주장하지 않는다. schema·rows·DB file bytes가 변하지 않아야 하고 작은 sibling DB의 동일 full-session 경로는 `DerivedRecordsVerified`까지 완료해야 한다.

호출당 100행 미만이어도 남은 primary/closure byte 예산 때문에 양보할 수 있으므로 첫 호출 실패나 최소 한 번의 Pending을 강제하지 않는다. 제품의 100-operation fence는 frozen operation 수의 제한이며 일반 live event 수 제한과 다르다.

### 10.4 검증·인계

새 진단은 아래 exact test 하나 안에서 세 사례를 구분해 검증한다. 각 focused 명령은 실행한 test가 정확히 하나인지 확인하고 raw log·실제 종료 코드를 남긴다.

```bash
cargo test -p hello-agent-sidecar --locked --lib agent_runtime::budget_tests::budget_valid_actual_run_snapshot_exceeds_reopen_primary -- --exact --nocapture
cargo test -p hello-agent-sidecar --locked --lib agent_runtime::budget_tests::valid_public_ledgers_reach_count_and_text_boundaries_but_reject_plus_one -- --exact --nocapture
cargo clippy -p hello-agent-sidecar --locked --all-targets --no-deps -- -D warnings
rustfmt --edition 2021 --check crates/hello-agent-sidecar/src/agent_runtime.rs
```

diff check·exact feature commit 독립 코드 리뷰와 최신 통합 commit의 독립 focused QA를 수행한다. production diff가 없다는 사실과 상속한 R3B 누적 QA를 명시하며 테스트 전용 변경의 범위를 넘어가면 전체 재검증 계획을 다시 정한다. main 병합·push는 하지 않는다.

합격은 호환성 문제가 재현됐다는 뜻이다. 기존 입력을 안전하게 수락하는 해결책이나 R3C·전체 G5D 완료가 아니다. 결과를 §9와 작업 이력에 반영한 뒤 저장 표현·검증 단위·이전 기록 호환 정책을 별도로 설계한다.

§10 재현은 기능 `5a5858f`의 독립 리뷰와 exact 통합 `73b2d61297b376637f5cc6794057f325652ba450`의 focused QA·최종 gate를 통과했다. 새 exact test 1개(세 사례)와 기존 budget boundary test 1개, scoped clippy/format/diff가 통과했으며 제품 코드는 바뀌지 않았다. 근거는 `.artifacts/m5-qa/73b2d61297b376637f5cc6794057f325652ba450/manifest.json`과 `../history.md`다. 이전 `e5f9f01`의 누적 13단계는 상속한 production 기준선이며 이번 test-only SHA에서 다시 실행한 결과가 아니다.
