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
