# 작업 이력 아카이브 2026-08 (Hello, Agent)

> `../history.md`에서 넘어온 **과거 워크로그**다. 최신 상태와 미완료 작업은 `../history.md`를 본다.
>
> 여기 있는 엔트리는 **당시 시점의 기록**이다. 이후 코드·결정이 바뀌었을 수 있으니 현재 사실로 그대로 인용하지 말고, 재사용할 결정·함정은 `../history.md`의 `핵심 결정·발견`에 이미 반영돼 있다.
>
> M4 오케스트레이션의 **현재 결론**은 워크로그가 아니라 `../proposals/m4-orchestration.md`가 단일 기준점이다. 아래 2026-08-12 ~ 2026-08-21 엔트리는 그 설계 심의와 §7.10 구현의 왕복 기록이며, 확정된 결론과 철회된 주장은 그 문서 §9·§10에 정리돼 있다.
>
> 아카이브 기준: `../history.md`가 500줄을 넘으면 오래된 엔트리부터 `archive/history-YYYY-MM.md`로 옮긴다. 내용은 삭제하지 않는다.

---

## 워크로그 (최신이 위) — 2026-08-28 및 그 이전

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v6 메시지 도입 전이 정정)

- **snapshot과 introduction 분리**: 모호한 `message_id`를 `introduced_message_id`로 바꾸고 chat의 `durable_event_sequence`와 현재 `last_message.messageId`를 각각 nullable `chat_durable_sequence`·`chat_snapshot_message_id`로 exact mirror한다. non-chat은 세 필드가 모두 NULL이다. 첫 chat predecessor는 durable 0/null이고, 새 메시지는 durable이 정확히 1 증가하면서 valid TeamMessage의 eventSequence·chatId·snapshot·introduced identity가 일치해야 한다. 비메시지 event는 durable을 유지하고 introduction은 NULL이며 snapshot은 NULL 또는 직전 snapshot과 같은 ID만 허용한다.
- **bounded predecessor·golden 검증**: predecessor는 aggregate sequence-1 exact indexed lookup만 사용하고, migration-pending append에서 index row가 아직 없을 때만 같은 aggregate tuple의 exact event lookup으로 fallback한다. SQL trigger는 current-row mirror와 indexed predecessor가 있을 때의 방어 전이를 강제하고 partial unique index는 introduced identity만 대상으로 한다. 실제 V5 history `A → repeat A → null → B`의 no-backfill/indexSource, 첫·반복·NULL·두 번째 메시지, double introduction, 감소·jump·invalid message·snapshot 변경·missing-index fallback 및 exact query plan을 Rust·TypeScript·SQLite에서 확인했다. sidecar consumer는 아직 변경하지 않았으며 다음은 이 정정된 V6를 소비하는 별도 migration/append 구현이다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v6 source-index P1 수정)

- **source event exact 결속**: `event_source_index` INSERT가 존재하는 한 `events` row의 rowid·event/operation identity·aggregate tuple을 모두 동시에 일치시키도록 외래 키와 same-row trigger를 추가했다. chat message provenance는 canonical payload의 `last_message.messageId`와 exact 일치하고 non-chat 또는 message가 없는 chat만 NULL을 허용한다. 서로 다른 event에서 개별 FK 값만 섞는 cross-event 조합도 trigger가 거부한다.
- **실행 회귀 검증**: source-less index 삽입을 허용하던 golden을 실제 source event를 먼저 넣는 형태로 교체했다. exact pair 성공, nonexistent source 거부, mirrored field 7개와 message identity 각각의 불일치 거부, cross-event mix 거부, foreign-key integrity를 Rust·TypeScript validator와 실제 Node SQLite에서 함께 확인했다. protocol 40개, focused Rust storage-v6 1개, Node lifetime 15개, TypeScript no-emit, scoped strict clippy, rustfmt, `git diff --check`가 통과했다. migration은 계속 과거 history를 backfill하지 않으며 sidecar consumer는 변경하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v6 source-index contract)

- **append-only source index 계약**: exact storage-v5에서 empty `event_source_index`만 추가하는 schema-only v6 migration을 고정했다. index row는 event rowid·event/operation identity·aggregate tuple·nullable public message identity를 exact하게 담고, aggregate 최신 source와 partial unique message lookup index 및 UPDATE/DELETE 거부 trigger를 둔다. migration은 기존 event를 backfill하지 않으며 ready+absent는 pending+absent로 무효화하고 모든 present job은 state·identity·timestamps·failure evidence를 보존한 채 `indexSource/0/unexhausted`로 재시작한다. attempting/failed는 reopen fail-closed다.
- **16단계·원자성·provenance 경계**: `indexSource → verifySourceIndex`를 기존 v5 14단계 앞에 추가했다. event append·projection·source-index insert·ready invalidation은 같은 `BEGIN IMMEDIATE`이고 exact 재처리는 no-op, identity/message conflict는 전체 rollback이다. prepared workspace는 aggregate latest index로 project source rowid 하나를 찾고, events PK로 최대 1 MiB canonical payload 하나만 읽어 Rust exact 비교한다. aggregate removal/tombstone이 합법화되면 source-index 의미를 versioning할 때까지 구현을 중단하도록 명시했다. 다섯 query는 exact `EXPLAIN QUERY PLAN` detail을 합격선으로 고정했다.
- **golden 검증·범위**: Rust/TypeScript validator와 immutable storage-v6 manifest/migration/cases를 추가했다. 실제 SQLite에서 3 job state×14 v5 phase reset, pending/ready absent mapping, no-backfill, preserved rows/schema, unsafe pair/extra schema byte-zero 거부, append-only/unique guards, append rollback, exact query plans을 확인했고 v1~v5 fixture digest 불변도 고정했다. protocol 40개와 Node lifetime 15개, TypeScript no-emit, focused rustfmt·scoped strict clippy·`git diff --check`가 통과했다. unscoped `-D warnings` clippy는 변경 범위 밖 기존 lint 3건 때문에 실패했다. sidecar consumer·scheduler/wire/UI·cleanup/삭제/compaction은 변경하지 않았으며 다음은 독립 계약 리뷰 뒤 별도 v6 migration consumer다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v5 sidecar migration)

- **exact v5 소비 구현**: `Storage::open`이 fresh/v2/v3/v4/v5를 exact predecessor schema와 canonical meta/job pair로 검증하고 v4→v5 fixture SQL을 원자 적용한다. v4는 어떤 target DDL보다 먼저 exact schema와 admissible `ready+absent|pending+absent|pending+pending|attempting|failed` pair를 확인하며, current exact v5는 migration SQL 없이 schema와 reopen pair를 재검증한다.
- **보수적 mapping·재시작 경계**: v4의 `receipts`~`verifySource` phase/cursor/private exhaustion은 보존하고, 이미 진행한 `verifyReceipts`~`verifyBaselines`는 state·job identity·timestamps·실패 증거를 그대로 둔 채 `verifySource/0/unexhausted`로 되돌린다. v5 reopen은 `ready+absent`, `pending+absent`, `pending+pending`만 허용하며 attempting/failed sentinel은 자동 retry/success 없이 fail-closed한다. 기존 unbounded D1B helper는 v5 job을 소비하지 않고 mutation 전에 거부한다.
- **보존·실패 주입·검증**: 3개 v4 job state×9개 phase와 absent pair를 실행 migration해 모든 원장·projection·materialized row, append-only trigger, named index, compaction history/status 보존을 확인했다. malformed phase/cursor/exhaustion, unsafe meta/pair, partial/extra v4는 checkpoint 뒤 DB bytes와 전체 schema/row snapshot이 하나도 바뀌지 않았고, v5 rebuild 중간 SQL 실패는 exact v4로 rollback했다. storage focused 23개, sidecar 전체 106개+stdio 10개(1 ignored), protocol 전체 39개, sidecar `--no-deps -D warnings` clippy, rustfmt와 `git diff --check`를 통과했다. dependency 포함 strict clippy는 변경 범위 밖의 기존 core/protocol 경고 5건 때문에 실패했다. worker·scheduler·wire/UI·cleanup/삭제·compaction은 변경하지 않았으며 다음은 별도 bounded v5 projection verification worker다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v5 reopen/catch-up P1 수정)

- **pending-before-arm으로 catch-up 한정**: 최초 v5 계약이 max-rowid 증가 시 `attempting` job도 public `catchUpReset`으로 pending에 되돌릴 수 있게 해, durable attempting sentinel은 reopen에서 절대 자동 성공·재시도하지 않는다는 v4 불변식과 충돌했다. 정상 worker는 각 projection/derived batch를 arm하기 전 pending 상태에서 max rowid를 검사하며, arm을 commit한 같은 synchronous sidecar owner는 즉시 batch로 들어간다. 따라서 public catch-up은 pending verification job만 허용하고 attempting/failed는 모두 거부한다.
- **v5 reopen fail-closed validator**: Rust·TypeScript 양쪽에 v5 reopen pair validator를 추가해 `ready+absent`, `pending+absent`, `pending+pending`만 허용하고 `pending+attempting|failed` 및 구조적으로 잘못된 pair를 거부한다. v4→v5 migration은 attempting/failed 상태와 증거를 보존하지만, 전 3 state×9 phase 실행 migration 뒤 reopen 판정에서 attempting/failed가 계속 fail-closed임을 확인한다. sidecar consumer는 아직 변경하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v5 projection verification 계약)

- **v5 migration·보수적 재검증**: exact storage-v4 schema와 `ready+absent|pending+absent|pending+pending|attempting|failed` pair를 어떤 mutation보다 먼저 확인하고 metadata/job 두 표만 transaction 재구축하는 storage v5 계약을 추가했다. v4 `receipts`~`verifySource`의 state/phase/cursor/private exhaustion은 보존하되, 새 projection phase를 건너뛸 수 있는 기존 `verifyReceipts`~`verifyBaselines`는 state·실패 증거를 보존한 채 `verifySource/0/unexhausted`로 되돌린다. v1~v4 golden 파일은 고정 digest로 불변을 확인한다.
- **14단계 bounded projection 검증**: materialization 뒤 `verifySource → projects → chats → runs → locks → app_instances → receipts → messages → workspaces → baselines`를 각 table `rowid` keyset `LIMIT 100`으로 검사한다. source phase는 cutoff 이하 sequence/predecessor·필수 derived 존재·최신 projection 존재를 확인하고, table phase는 최신 exact source event와 projection JSON·mirrored/indexed column 전체를 비교한다. missing은 source phase, extra/source-less는 table phase가 맡으며 `COUNT`·window·`GROUP BY`·anti-join·full scan·unbounded namespace query는 금지한다. `app_instances`는 검증하지만 materialized snapshot aggregate에서는 계속 제외한다.
- **append fence·lifecycle**: projection 검증 진입 전, 모든 projection/derived batch 전, scan-free final publication 전에 `BEGIN IMMEDIATE` 안의 exact max event rowid가 cutoff와 같은지 확인한다. 더 크면 실패시키지 않고 같은 singleton을 새 cutoff의 `pending/receipts/0/unexhausted`로 reset하며, frozen materialization phase는 기존 cutoff로 계속 진행할 수 있다. pending/attempting/failed·cancel·terminal failure·private empty-query exhaustion 의미는 v4와 동일하다. 다음은 독립 계약 검토 뒤 별도 sidecar worker가 exact v5 migration과 bounded verification을 소비하는 구현이다.
- **검증·소유 경계**: 실제 SQLite migration으로 v4 3개 job state×9개 phase와 pending/ready absent, unsafe pair·extra schema의 DB byte-zero 거부를 확인했다. Node lifetime 12개, protocol 전체 39개, TypeScript no-emit, Rust 2021 rustfmt와 `git diff --check`를 통과했다. sidecar consumer·wire/scheduler/UI·삭제/compaction·제품 보존 정책은 변경하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v4 sidecar migration)

- **exact v4 소비 구현**: `Storage::open`이 fresh/v2/v3 상태를 exact v1→v2→v3→v4 순서로 검증·이전하고, 현재 exact v4는 migration SQL 없이 재검증한다. v3→v4 DDL 전에 exact v3 schema와 canonical v3 job/meta pair를 모두 검사하며, canonical transaction 뒤 exact v4 schema·metadata·optional job과 phase별 cursor를 protocol v2 validator로 재검증한다. partial/extra/version/table mismatch와 unsafe v3/v4 pair는 fail-closed다.
- **보존·재시작 경계**: v3 pending/failed의 다섯 phase와 pending/ready absent를 모두 실행 이전해 `verify→verifySource`, `after_event_rowid→after_scan_rowid`, `scan_exhausted=false`, failed 보존을 확인했다. 원장·projection·materialized data·append-only trigger·index·compaction history는 그대로 유지한다. v4 pending job과 sidecar-private exhaustion proof는 재시작 가능하지만 attempting/failed는 구조적으로 읽은 뒤 non-runnable으로 거부한다. 기존 unbounded D1B helper는 v3 회귀를 유지하되 v4에서는 어떤 materialization mutation 전 거부한다.
- **실패 주입·검증·경계**: metadata/job rebuild 뒤 강제 SQL 실패가 exact v3 schema/data로 rollback함을 주입 검증했다. storage focused 19개, lifetime focused 12개, protocol lifetime 6개, sidecar 전체 102개+stdio 10개(1 ignored), strict sidecar clippy, Rust 2021 rustfmt와 `git diff --check`를 통과했다. worker·scheduler·wire/UI·compaction/삭제·provider는 변경하지 않았으며 다음은 bounded v4 batch worker의 별도 구현·검토다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D private exhaustion authority 수정)

- **위조 가능한 공개 proof 제거**: 직전 검토 수정의 `markScanExhausted`/`emptyBatchObserved` JSON lifecycle 입력은 caller가 빈 keyset 조회 사실을 스스로 주장할 수 있어 증명이 아니었다. Rust·TypeScript 공개 operation과 witness 타입·validator·fixture에서 이를 전부 제거하고, 제거된 literal도 fail-closed로 거부한다. 어떤 공개 lifecycle transition도 `scanExhausted` false→true를 만들 수 없다.
- **구현 소유 경계**: durable `scan_exhausted`와 v3→v4 false migration은 유지한다. false→true mutation authority는 `sidecarPrivateExactEmptyKeysetQuerySameTransaction`으로 고정하고, 증명은 public JSON으로 표현하지 않으며 sidecar 구현 테스트가 책임진다고 Rust·TypeScript·manifest에 exact 합의했다. protocol lifecycle은 sidecar가 이미 저장한 true state에서 `phaseAdvance`/final publication을 검증할 뿐 그 state를 만들었다고 주장하지 않는다.
- **검증 범위**: 제거 operation literal과 직접 false→true lifecycle 입력 회귀를 Rust·TypeScript golden에 추가했다. sidecar 구현은 이 contract branch의 소유 범위 밖이므로 변경하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v4 exhaustion review 수정)

- **P1 durable exhaustion proof**: `afterScanRowid=0`만으로 빈 derived table과 아직 검사하지 않은 table을 구분할 수 없어 phase chain을 건너뛰고 ready를 게시할 수 있던 결함을 수정했다. v4 job에 `scan_exhausted`를 추가하고 migration은 모든 v3 job을 보수적으로 false로 옮긴다. nonempty batch·arm·cancel·failure는 false/current proof를 보존하며, pending current phase에서 `emptyBatchObserved=true` witness를 받은 `markScanExhausted`만 false→true를 허용한다. 별도 `phaseAdvance`는 true인 exact 직전 phase만 다음 phase/rowid 0/false로 옮기고, final publish와 max-rowid reset도 exhausted `verifyBaselines`만 받는다.
- **P2 migration matrix**: 실제 Node SQLite golden이 pending/failed의 v3 다섯 phase를 모두 실행 migration하고 `verify→verifySource`, cursor rename, `scan_exhausted=0`, failed 보존을 확인한다. job 없는 pending·ready도 각각 실행해 exact v4와 absent singleton을 확인하므로 preflight가 허용하는 모든 v3 pair가 실행 사례를 갖는다.
- **경계·검증**: Rust·TypeScript 공용 lifecycle fixture가 witness 없는/false witness exhaustion, nonempty batch의 forged true, unexhausted cursor-0 derived skip, unexhausted final publication을 거부한다. sidecar·wire·scheduler·cleanup/삭제·compaction·제품 정책은 변경하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v4 bounded verification 계약)

- **additive v4 migration 계약**: exact storage-v3 schema와 `ready+absent` 또는 구조적으로 유효한 `pending+absent|pending|failed` pair를 어떤 mutation보다 먼저 확인하고, transaction 안에서 `storage_schema_meta` version CHECK와 `lifetime_materialization_job`만 재구축한다. v3 `verify`는 `verifySource`, `after_event_rowid`는 `after_scan_rowid`로 옮기며 failed는 보존한다. 나머지 원장·projection·materialized data·compaction history·named index·append-only trigger는 그대로 보존하고 unsafe pair/partial/extra schema는 DB byte-zero mutation으로 거부한다.
- **arm·batch·검증·취소 경계**: job은 `pending|attempting|failed`, cursor는 source phases와 `verifySource`에서는 cutoff 이하 event rowid, 네 derived verification phase에서는 각 table rowid를 쓴다. caller가 바꿀 수 없는 exact `LIMIT 100`·1 MiB와 keyset scan만 허용한다. 짧은 arm transaction이 attempting을 먼저 영속하고, bounded batch success는 derived row+cursor+pending을 원자 commit한다. cancel은 batch rollback 뒤 pending을 commit하고 terminal validation은 rollback 뒤 failed를 commit한다. 두 번째 기록 transaction이 불가능하면 attempting sentinel이 남으며 reopen은 자동 retry/success를 거부한다.
- **최종 publication·비활성 경계**: verify batch commit 뒤 cancel은 그 호출에서 즉시 `Suspended`로 끝나 final publication으로 이어지지 않는다. 별도 짧은 final transaction은 exact pair/cursor·max event rowid·두 cancel point만 검사해 job 삭제+ready를 게시하며 artifact/table scan/count는 하지 않는다. max가 늘면 receipts/새 cutoff/0으로 reset한다. cleanup·삭제·compaction·wire·scheduler와 보존/downgrade 제품 결정은 계속 비활성·열린 상태다.
- **검증**: 공용 storage-v4 golden이 v3의 모든 pending phase·verify/failed mapping·ready absent, exact data/trigger/maintenance 보존, unsafe preflight 무변경, arm/batch/cancel/fail/attempting sentinel, source-less derived extra 거부, historical baseline 허용, final cancel/max-rowid 및 policy mutation을 Rust·TypeScript 양쪽에서 판정한다. focused Node·protocol Rust·TypeScript no-emit·rustfmt·diff check를 통과했다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage v3 review P1 수정)

- **재시작 pair 검증**: exact v3 open이 metadata와 optional `lifetime_materialization_job`을 protocol canonical 값으로 구성해 job validator와 meta/job pair validator를 모두 통과시킨다. `pending+absent|pending|failed`와 `ready+absent`만 재시작하며 `ready+present`, malformed timestamp/job은 cursor·content·path를 싣지 않는 동일 오류로 fail-closed한다. terminal failed job은 계약상 유효하지만 자동 retry 대상은 아니다.
- **레거시 D1B 격리**: 기존 unbounded `materialize_lifetime_records`는 bounded v3 cursor가 하나라도 있으면 첫 metadata mutation 전에 거부한다. 직접 회귀가 DB bytes·materialization meta·job row·derived row count 보존을 확인해 레거시 경로가 cursor를 둔 채 ready를 게시할 수 없도록 고정했다.
- **검증·경계**: 직접 회귀 2개, sidecar 전체 97개+stdio 10개(1 ignored), protocol lifetime 5개, focused rustfmt와 strict sidecar clippy를 통과했다. worker·scheduling·wire/UI·compaction/삭제는 여전히 구현하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage schema v3 sidecar migration)

- **exact v3 소비 구현**: `Storage::open`이 새 DB는 v1→exact v2 검증→v3 순서로 올리고, 기존 exact v2는 `pending|ready` 상태만 byte/schema mutation 전에 허용한 뒤 canonical v3 SQL을 원자 적용한다. exact v3 재시작은 no-op이며 부분·추가 object, index/table drift, v3 table과 metadata version 불일치는 fail-closed한다. `materializing|failed` v2는 DB bytes·schema·rows가 바뀌지 않은 채 거부한다.
- **보존·격리 회귀**: pending/ready populated v2의 모든 v1/v2 원장·projection·receipt·public message·prepared workspace·baseline·maintenance 행과 기존 table/index/append-only trigger SQL을 그대로 보존하고 job을 만들지 않으며, 이후 ledger append가 계속 동작한다. migration 중간 실패 주입은 exact populated v2로 rollback한다. 전용 `lifetime_materialization_job` 행은 기존 `maintenance_jobs` 기반 compaction status에 영향을 주지 않는다.
- **검증·경계**: focused storage 14개, sidecar 전체 95개+stdio 10개(1 ignored), protocol lifetime 5개, focused rustfmt와 strict sidecar clippy, `git diff --check`를 통과했다. batch worker·startup/idle scheduling·wire/UI·compaction/삭제·provider는 변경하지 않았고 기존 unbounded internal materialization도 그대로다. 다음은 이 v3 cursor를 소비하는 bounded batch/cancel 구현과 독립 리뷰다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D schema v3 preflight 검토 수정)

- **P2 preflight 보강**: exact storage-v2 DDL만 확인하면 schema-valid `materializing/failed`를 v3로 그대로 복사해 시작부터 fail-closed 상태를 영속할 수 있었다. 어떤 DDL·data mutation보다 먼저 exact v2와 `materialization_state IN (pending, ready)`를 함께 검증하고, 그 밖의 상태는 schema/data byte-zero mutation으로 거부하도록 계약을 좁혔다.
- **상태 보존 명시·검증**: v3 migration은 상태를 강제로 pending으로 만들지 않는다. 허용 경로는 `pending→pending+job absent`, `ready→ready+job absent` 두 가지뿐이다. Node golden이 두 보존 경로와 materializing/failed 무변경 거부를 실행하고 Rust·TypeScript preflight validator/mutation case가 같은 조건을 고정한다. focused Node 7개, protocol lifetime Rust 5개, TypeScript no-emit을 통과했다. 다음은 이 보강을 포함한 독립 재검토 뒤 sidecar 소비 구현이다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D storage schema v3 계약)

- **전용 singleton으로 정정**: `maintenance_jobs`는 DDL상 `losslessCompaction` 전용이고 기존 status 계산도 모든 terminal maintenance row를 compaction 결과로 읽으므로, materialization cursor/성공/실패를 그 표에 저장하면 compaction 이력을 오염시킨다는 재검토 결함을 확인했다. immutable storage-v2 migration은 그대로 두고, exact v2 사전검증 뒤 `storage_schema_meta`의 version CHECK만 3으로 transaction 재구축하며 `lifetime_materialization_job` singleton을 추가하는 additive storage schema v3 계약으로 분리했다. protocol·payload schema는 계속 v1이고 downgrade 정책은 열린 질문이다.
- **상태·재개 계약**: canonical readiness는 `storage_schema_meta.materialization_state`다. 정상 pair는 pending+absent/pending/failed와 ready+absent뿐이고 기존 materializing/meta failed 또는 ready+present는 fail-closed한다. job은 durable `pending|failed`만 허용하며 running/succeeded/cancelled를 저장하지 않는다. batch는 derived row+advanced pending cursor를 원자 commit하고 cancel/restart는 직전 pending cursor를 보존한다. terminal failure는 rollback 뒤 같은 singleton을 cursor 보존 failed로 바꾸며 자동 retry하지 않고, 완료는 singleton 삭제+meta ready를 같은 transaction으로 게시한다. ready 뒤 append는 같은 append transaction에서 meta pending+job absent, pending 중 append는 frozen cursor 불변, final max rowid 증가 감지는 같은 singleton을 더 큰 cutoff의 receipts/0으로 reset한다.
- **golden 검증·경계**: Rust·TypeScript가 5개 bounded phase, cursor/job/pair/lifecycle와 sanitized failure kind를 같은 v3 fixture로 검증한다. Node SQLite golden은 populated v1→v2→v3에서 모든 v1/v2 data row, maintenance compaction history, table/index/trigger SQL을 보존하고 후속 event append, exact-v3 no-op, partial/extra v2 fail-closed를 확인했다. focused Node 7개, protocol lifetime Rust 5개, TypeScript no-emit을 통과했다. sidecar·wire·scheduler·삭제/cleanup·보존 threshold는 활성화하지 않았으며 다음은 이 v3 계약의 독립 검토 뒤 sidecar migration/batch 소비 구현이다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D 배치 계약 검토 수정)

- **durable lifecycle 정정**: 최초 계약 commit `f131145`가 `maintenance_jobs`와 `storage_schema_meta` 상태를 한 목록으로 섞고 live projection을 materialization source로 허용한 결함을 검토에서 확인했다. `Suspended`는 메모리 내 호출 결과일 뿐 저장 상태가 아니며 canonical cursor는 단일 `pending` lifetime job의 `cutoff_json`에 저장한다. SQLite owner 하나와 매 batch `BEGIN IMMEDIATE`가 active pending/running 개수·exact job identity·pending 상태·cursor를 검사하고, DDL에 active-job UNIQUE가 없으므로 transaction 경계 검증으로 두 번째 job을 막는다. commit은 derived rows와 advanced pending cursor를 원자 결속하고 cancel/current failure는 직전 pending cursor로 rollback한다. 성공은 같은 job을 `succeeded`로 끝내며 sanitized `failed` 기록은 rollback 뒤 별도 optional best-effort transaction만 허용하고 자동 retry하지 않는다. persisted `running`은 maintenance corruption, `materializing/failed`는 `storage_schema_meta` fail-closed 상태이며 failed maintenance job은 terminal history다.
- **frozen cutoff·phase 정정**: receipt/message/workspace/baseline/verify artifact는 frozen cutoff 이하 event와 그 event projection snapshot만 source로 삼고 live projection은 `max(event.rowid)==cutoff`를 먼저 증명한 최종 catch-up/equivalence transaction에서만 비교한다. 다섯 phase 모두 동일 100행·1 MiB guard의 keyset scan이며 phase는 `afterEventRowid=0`에서 시작해 cutoff까지 완료한 뒤 다음 phase를 0으로 reset한다. verify도 batchable하다. compact는 verify 완료 뒤에만 도달 가능한 reserved marker지만 실행은 계속 비활성이다. 최종 별도 `BEGIN IMMEDIATE`는 bounded SQL/keyset evidence로 completeness·equivalence·live reconstruction을 확인하며 max rowid가 바뀌면 ready를 게시하지 않고 새 cutoff catch-up을 pending으로 이어간다.
- **회귀 계약·검증**: Rust·TypeScript cursor transition validator와 golden cases가 phase start/order/reset, verify batching, mid-phase append의 frozen cutoff, verify 뒤 새 cutoff catch-up, durable state/source/final-check 모순을 거부한다. 미래 sidecar 회귀 순서는 `materialize ready → append와 같은 transaction에서 pending → 별도 catch-up verify → ready`로 고정했다. focused Node 6개, protocol lifetime Rust 4개, TypeScript no-emit, focused rustfmt와 `git diff --check`를 통과했다. SQL·sidecar·wire·scheduler·삭제는 변경하지 않았고 다음은 수정 계약의 독립 재검토 뒤 sidecar 소비 구현이다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D 배치·상한·취소 계약)

- **고정된 내부 guard·cursor**: lifetime materialization은 caller가 바꿀 수 없는 source 100행·canonical payload 합계 1 MiB 상한을 사용하며, 단일 source payload가 1 MiB를 넘으면 fail-closed한다. cursor schema v1은 안전 정수 `cutoffEventRowid`·`afterEventRowid`와 `receipts/messages/workspaces/baselines/verify/compact` phase만 허용하고, `rowid > after AND rowid <= cutoff ORDER BY rowid` keyset scan만 사용하며 `OFFSET`은 금지한다.
- **재개·취소·원자성**: sidecar handshake ready 이후 idle turn마다 batch 하나를 `BEGIN IMMEDIATE` transaction 하나로 처리한다. cancel은 transaction 전에는 mutation 없이, transaction 안에서는 각 source row 전후와 precommit 직전에 확인해 현재 batch만 rollback하고 내부 `Suspended`로 끝낸다. 이미 commit된 exact derived batch는 재시작·취소 뒤에도 남을 수 있지만 source ledger mutation과 compaction trigger 변경은 atomic rollback 대상이다. commit 가능한 진행 상태는 `pending`뿐이고 persisted `running/materializing/failed`는 fail-closed하며 pending/running lifetime job은 최대 하나다. append는 같은 transaction에서 ready를 pending으로 무효화하고, ready 재공개는 별도 catch-up/verification transaction에서만 한다.
- **계약 전용 경계·검증**: Rust·TypeScript validator/constants와 storage-v2 golden batching cases를 추가해 malformed/unsafe/out-of-order cursor, caller limit override, 100행·1 MiB 초과, rollback·ready publication 모순을 양쪽에서 거부했다. focused Node 6개와 protocol lifetime Rust 4개, TypeScript no-emit, focused rustfmt, `git diff --check`를 통과했다. SQL·sidecar 구현·wire/V6·자동 scheduling·compaction/삭제·threshold·downgrade 결정은 건드리지 않았다. 다음은 이 계약을 소비하는 sidecar batch materialization 구현과 독립 리뷰다.

### 2026-08-28 · by GPT-5.6-Sol (M5 Electron 개발 실행 복구)

- **원인·수정**: `pnpm m5:dev`가 만든 `.vite/build/main.js`는 CommonJS인데 루트 `package.json`의 `type=module` 때문에 Electron이 ESM으로 해석해 `exports is not defined`로 창 생성 전에 중단됐다. 루트 ESM은 유지하고 Electron main/preload 산출물만 명시적 `main.cjs`/`preload.cjs`로 고정했다. preload 경로도 Forge가 제공하지 않는 선언형 상수가 아니라 main bundle 옆의 exact 파일을 사용한다.
- **회귀 경계**: package main, Vite CJS 산출물 이름, BrowserWindow preload 경로와 ASAR smoke 기대 파일을 한 계약으로 묶었다. M5 작업대 화면은 존재하며 개발 모드는 fixture sidecar, 패키지 모드는 Rust sidecar를 사용한다. 실제 Claude/Codex provider는 계속 비활성이다.
- **검증·경계**: Electron scaffold 4개와 TypeScript no-emit, Forge 개발 실행을 통과했고 main/preload가 `.cjs`로 빌드된 뒤 `app://m5/index.html`의 프로젝트 선택·파일·코드 편집기·변경 3열 작업대가 실제 창에 렌더링됨을 확인했다. darwin-arm64 package와 Rust sidecar 포함 ASAR smoke도 통과했다. 개발 실행은 fixture sidecar이며 패키지 실행도 실제 Claude/Codex provider는 아직 비활성이다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D D1B 무삭제 materialization)

- **호출형 단일 transaction materialization**: 아직 wire·startup·background에 연결하지 않은 내부 `materialize_lifetime_records`가 stable event rowid cutoff를 잡고 모든 v1 event를 immutable operation receipt로 옮긴다. canonical payload 원문과 재계산 SHA-256 metadata를 exact 비교하며 legacy·actual-agent·recovery duplicate/conflict 의미가 기존 v1과 같음을 검증한다.
- **공개 기록·작업공간·baseline**: durable chat projection의 exact `teamMessage`만 안정된 message ID/순서로 materialize하고, project projection의 unbound/bound/abandoned workspace를 chat composite identity와 대조한다. `projects/chats/runs/locks`는 exact last sequence에 canonical baseline을 만들며 기존 baseline+새 tail이 현재 projection을 재현하는지 매번 확인한다. 기존 materialized 행 손상·extra/missing 행·namespace/recovery/projection drift는 덮어쓰지 않고 fail-closed다.
- **원자성·검증·통합 경계**: receipt 뒤, message/workspace 뒤, baseline 뒤 metadata 직전 3개 실패 주입이 v2 행과 `materialization_state`를 모두 rollback하고 원장/projection을 바꾸지 않음을 확인했다. 독립 리뷰는 원자성·v1 idempotency·public message privacy·workspace 결속·실제 baseline+tail replay에 P0~P3 결함 없음으로 승인했고 commit `c49034293b58698cbb44b5a9694e4d2b819bd38c`로 통합했다. 통합 branch에서 focused lifetime 10개, protocol lifetime 3개, Node 계약 5개와 TypeScript no-emit을 재통과했다. event/projection 삭제, compaction/checkpoint/VACUUM, 자동 스케줄, `storage.status` wire, threshold/cleanup은 포함하지 않았다. 전체 원장을 메모리에 읽으므로 자동 실행 전에 배치·상한·취소 계약이 필요하다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D D0A·D1A 통합)

- **통합**: storage lifetime 계약의 최초·리뷰 보강·receipt 무결성 3개 commit과 SQLite v2 migration/status core를 독립 리뷰 승인 뒤 `m5/coding-agent-ide`에 순서대로 통합했다. 현재 기준은 `a7fec35de27aa3eb99c7c8d9fe8fccf359deca59`이며 `storage.status`와 Bridge V6는 계속 계획 계약일 뿐 제품 wire/ready/dispatcher에는 노출하지 않는다.
- **통합 검증**: G5D Node 계약 5개, protocol Rust 35개, sidecar unit 86개(1 ignored)와 stdio process 10개, TypeScript no-emit, `git diff --check`를 통합 branch에서 다시 통과했다. 독립 D0A 재리뷰는 receipt canonical JSON/SHA-256·workspace composite binding을 승인했고, D1A 리뷰도 migration 원자성·exact schema·privacy-safe status에 P0~P3 결함 없음으로 승인했다.
- **다음 할 일**: ready 이후 receipt/message/workspace/baseline materialization과 무손실 compaction, 그 뒤 `storage.status` V6 활성화·Electron/renderer·하네스/QA를 각각 독립 wave로 진행한다. 보존 기간·전체 용량 threshold·physical cleanup/삭제는 사용자 결정 전 계속 비활성이다. 실제 provider·인증·사용자 프로젝트·Windows·push·`main` 병합은 수행하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D D1A schema migration·내부 status)

- **additive schema v2**: `Storage::open`이 exact v1 schema를 먼저 확인한 뒤 storage-v2 golden DDL을 한 transaction으로 적용하고, v1 `schema_meta=1`·모든 원장/projection/append-only trigger를 그대로 둔 채 v2 table/index/metadata 전체를 exact 검증한다. 재시작은 idempotent하며 부분·추가·index·metadata 변조는 fail-closed다. 시작 중 record backfill/materialization/deletion은 하지 않는다.
- **privacy-safe 내부 status**: 아직 wire에 광고·dispatch하지 않는 `Storage` 내부 상태 계산을 추가했다. SQLite page/file/WAL bytes, events count/canonical payload UTF-8 bytes, `projects/chats/runs/locks` projection snapshot row count/UTF-8 bytes, 완료된 maintenance job 상태만 durable DB 사실에서 집계한다. volume capacity는 새 의존성 없이 `unavailable/null`, physical cleanup은 `false/userPolicyNotConfigured`이며 raw chat/provider text·token/auth·absolute path를 반환하거나 오류에 싣지 않는다. app instance, filesystem worktree inventory, materialization 전 v2 row는 snapshot 집계에서 제외한다.
- **검증·경계**: focused storage 15개와 sidecar 전체 86개+stdio 10개(1 ignored), 소유 파일 rustfmt, known baseline lint 4종을 제외한 sidecar strict clippy, `git diff --check`를 통과했다. exact strict clippy는 이번 범위 밖 core `proctree.rs` 2건과 protocol `run_recovery_v1.rs`·`workspace_v1.rs`·`lib.rs` 3건에서 실패했고, workspace fmt도 기존 `src-tauri/src/detect.rs`·`login.rs` drift만 보고했다. `storage.status` wire/V6, materialization, compaction, threshold/admission, cleanup/삭제는 D1A에 포함하지 않았으며 다음 독립 리뷰와 후속 wave가 필요하다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D receipt 무결성 재리뷰 수정)

- **canonical payload 직접 검증**: Rust와 TypeScript operation receipt validator가 `canonicalPayloadJson`을 직접 parse·canonicalize하고 기존 v1 canonical JSON과 byte-exact인지 확인한다. 같은 원문 bytes의 SHA-256도 직접 재계산해 metadata 형식만 맞는 손상 digest를 거부한다.
- **손상 회귀**: 정상 legacy·actual-agent·recovery duplicate/conflict 사례를 유지하면서, 64자리 lowercase hex지만 원문과 불일치하는 digest, 원문 digest는 일치하지만 key 순서가 canonical이 아닌 JSON, parse할 수 없는 JSON을 공용 golden fixture에서 양쪽 validator가 거부하도록 고정했다.
- **검증**: focused G5D Node contract 5개, protocol Rust 35개, TypeScript no-emit, 소유 Rust focused rustfmt와 diff check를 통과했다. sidecar·Electron·manifest와 활성 wire는 변경하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D D0A 독립 리뷰 수정)

- **operation receipt 정확성**: digest만 보존하던 초안을 고쳐 canonical payload 원문과 SHA-256/version/format metadata를 함께 보존한다. duplicate/conflict는 digest가 아니라 v1 immutable record의 exact canonical JSON과 나머지 불변 필드를 비교하며 request ID만 기존처럼 제외한다. legacy·actual-agent·poisoned recovery fixture가 exact duplicate와 changed-payload conflict를 교차 검증한다.
- **prepared workspace 결속**: `bound ↔ boundChatId non-null`, `unbound/abandoned ↔ null`을 SQL CHECK로 고정하고, 기존 `chats(chat_id, project_id)`에 additive unique index를 둔 composite FK로 bound chat과 project가 같음을 SQLite가 직접 거부하게 했다. 세 valid 상태와 null/state 모순·cross-project insert를 실제 SQLite에서 검증한다.
- **나머지 회귀**: failed compaction의 `lastSuccessfulAt`이 `lastAttemptedAt`보다 늦으면 Rust/TypeScript 모두 거부한다. v1의 7개 테이블 전체 행과 events append-only update/delete trigger·후속 append가 v2 DDL 전후 보존되는지 확장 검증했다.
- **검증**: G5D Node contract 5개, protocol Rust 35개, TypeScript no-emit, 소유 Rust focused rustfmt와 `git diff --check`를 통과했다. sidecar/Electron/runtime method는 D0A 범위대로 변경하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5D D0A 저장 수명 계약)

- **무삭제 계약**: 실제 provider 활성화 전 stop condition인 SQLite 전체 수명을 G6에서 분리해 G5D로 열었다. D0A는 채팅·메시지·run·recovery·evidence·snapshot을 삭제하지 않고 임의 보존 기간·용량 threshold·자동 정리를 정하지 않는다. physical cleanup과 cleanup candidate/action API는 비활성이다.
- **계획된 V6·schema v2**: privacy-safe `storage.status` request/response와 additive `M5BridgeV6`, v1 원장/projection을 보존하는 storage-v2 golden DDL/metadata를 Rust·TypeScript 계약으로 고정했다. sidecar가 아직 처리하지 못하므로 `storage.status`는 D1 전 protocol v1 method/ready capability/dispatcher에 광고·예약하지 않는다.
- **lossless compaction 경계**: baseline+tail replay, 기존 operation duplicate/conflict, actual/legacy namespace, recovery 동등성을 확인한 뒤에만 검증된 ledger cutoff를 논리 삭제하며 실패는 전체 rollback한다. checkpoint/`VACUUM`은 논리 성공 조건이 아니다. 다음은 D1 sidecar migration/materialization/status/compaction 구현과 독립 리뷰이며 물리 삭제 정책은 사용자 결정으로 남는다.
- **검증**: G5D Node contract 4개, protocol Rust 35개, TypeScript no-emit, 소유 Rust 파일의 2021 rustfmt와 `git diff --check`를 통과했다. workspace `cargo fmt --all -- --check`는 이번 소유 범위 밖 기존 `src-tauri/src/detect.rs`·`login.rs` formatting drift만 보고해 focused rustfmt로 대체했고, 해당 파일은 수정하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5C 최종 gate)

- **판정**: exact docs/integration commit `cdb3e1b49de5f0c6541ff007f007bcb65b7db122`을 독립 최종 gate가 `PASS — APPROVE / G5C_READY`로 승인했다. G5C 기능 계보와 원자성·UI·combined 독립 리뷰, exact-head 누적 하네스 12단계, 독립 QA artifact, 현행 문서와 byte-identical `AGENTS.md`/`CLAUDE.md`를 다시 대조했다.
- **재검증·경계**: gate가 G5C 계약·UI Node 30개, protocol 32개, sidecar recovery 14개, fake provider recovery stdio 1개, 일반 `--stdio` ready/status/quit 1개와 diff check를 통과했다. G6는 의도대로 `NOT_READY`다. 실제 Claude/Codex adapter·인증 smoke, SQLite 전체 lifetime/retention/compaction, G6, Windows, 전체 OS Quit와 출시 후보 검증이 남으며 push와 `main` 병합은 하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5C 통합·누적 하네스·독립 QA)

- **통합·리뷰**: provider sidecar, text/recovery 경계, durable recovery 발견, renderer recovery controls, restart-atomic saga, pre-G5C ID 백필과 identityless UI 보강을 독립 Rust·UI·combined 재리뷰 승인 뒤 `m5/coding-agent-ide`에 통합했다. G5C provider/identityless 계약 테스트도 `m5:test:run-recovery` 누적 게이트에 추가했다.
- **누적 증거**: exact integration `42dcbfad2444eebbebaeef4e4f64f614a6de6264`의 G5 누적 하네스가 macOS arm64 Electron package smoke, Chromium Monaco, 기존 Tauri와 Rust workspace, G5A~G5C fake-only 실행·복구를 모두 PASS했다. 최초 sandbox package는 Electron artifact fetch 차단으로 실패했고 네트워크 허용 재실행은 PASS했다. 증거는 `.artifacts/m5-harness/g5-run-recovery-2026-08-28T01-03-49-390Z.json`이다.
- **독립 QA·경계**: 같은 exact commit의 독립 QA가 no-fallback 발견, handshake 비공개, exact text/event 경계, provider recoveryRequired exit 0, random recovery와 legacy namespace, restart saga, pre-G5C recovery ID 보존, identityless zero-RPC 보호를 재검증해 `G5C_QA_PASS`로 판정했다. 증거는 `.artifacts/m5-qa/g5c-integration-42dcbfad2444eebbebaeef4e4f64f614a6de6264-2026-08-28T01-09-53Z.json`이다. 일반 `--stdio`의 실제 Claude/Codex·인증·모델·사용자 프로젝트·Windows는 실행하지 않았으며, 다음은 문서 commit의 독립 최종 gate다. push와 `main` 병합도 하지 않았다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5C identityless recovery renderer boundary)

- **보호 상태**: durable actual recovery의 process identity가 없으면 renderer가 실행·저장·닫기 차단을 계속 유지하고, 초보자 화면에는 종료 상태를 안전하게 확인할 수 없어 보호 중이라는 안내만 보인다. 이 경우 recovery abandon button은 렌더하지 않으며 programmatic `abandonAgentRecovery`도 confirm·inspect·resolve 전에 반환한다.
- **공유 경계·호환**: UI와 controller는 하나의 `canAbandonAgentRecovery` 판정을 사용해 한쪽만 완화되는 drift를 막는다. process identity가 있는 기존 actual recovery의 inspect → exact revision resolve → durable rehydrate 흐름은 바꾸지 않았다.
- **검증·경계**: focused renderer Node 20개와 full Node 180개, TypeScript no-emit, Vite production build와 `git diff --check`를 통과했다. identityless recovery의 zero inspect/resolve, terminal hydration·reopen 뒤 차단 유지, normal recovery 허용을 회귀로 고정했다. 실제 provider·인증·사용자 프로젝트·Windows는 호출하지 않았고 다음은 기능 branch 독립 리뷰와 최신 통합 기준 QA다.

### 2026-08-28 · by GPT-5.6-Sol (M5 G5C legacy recovery 예약 리뷰 수정)

- **startup 보강 순서**: `source_kind=actualAgent` projection에 기존 non-null recovery가 있지만 내부 `recovery_id_reservation`이 없는 pre-G5C 상태를 skip·validation 전에 먼저 보강한다. 새 ID를 만들거나 기존 ID를 바꾸지 않고 public `recoveryId`를 같은 SQLite transaction에 저장한 뒤 persisted projection을 재조회하며, 이후 unresolved·resolved 상태의 기존 startup 분기를 그대로 따른다.
- **회귀·롤오버**: pre-G5C unresolved recovery는 exact ID를 유지한 채 inspect와 abandon이 계속 동작하고, resolved recovery는 exact ID/public record를 유지한 채 inspect 가능·재해결 거부·byte-zero DB mutation임을 고정했다. 현재 파일에 홀로 남았던 2026-08-27 recovery 원자성 엔트리는 같은 날짜의 나머지 엔트리가 있는 `archive/history-2026-08.md`로 원문 그대로 이동해 날짜 경계를 복구했다.
- **검증·경계**: focused startup 회귀 1 passed, sidecar 전체 78 passed·1 ignored와 stdio 10 passed, core 82 passed, protocol 32 passed, strict sidecar clippy `--no-deps`, Rust 2021 rustfmt, `git diff --check`를 통과했다. 실제 provider·인증·사용자 프로젝트·network·Windows는 실행하지 않았고 public wire·Electron·UI는 변경하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C recovery 원자성·재시작 조정)

- **복구 ID·identityless 경계**: actual run 준비와 writer lock의 최초 SQLite transaction에 random `agent_recovery_<UUIDv4>` 예약을 함께 저장하고, 정상 불확실 종료·pre-release 실패·provider recovery·재시작 변환이 그 ID만 재사용한다. 구형 actual projection은 기존 recovery ID를 바꾸지 않으며, 예약이 없으면 별도 atomic reservation을 먼저 commit·재조회한 뒤 변환한다. 추적 process identity 없이 재시작한 run은 PID를 만들거나 EOF를 소멸 증거로 삼지 않고 `processIdentityUnavailable` unresolved recovery와 poisoned run/chat/workspace/lock을 남긴다. stale revision은 먼저 `revisionConflict`, matching 요청은 `identityUnavailable`·`after=null`이며 DB와 manifest를 바꾸지 않는다.
- **restart-reconcilable abandon saga**: actual recovery 폐기는 더 이상 filesystem-first가 아니다. Phase A가 full run/workspace/recovery identity·expected revision·정확한 recoveryRequired manifest+SHA-256 digest를 내부 intent로 원자 저장하고 public ledger/lock은 계속 poisoned다. Phase B는 recoveryRequired→abandoned만 exact·idempotent하게 atomic write하며 active/missing/corrupt/symlink/identity/digest mismatch를 거부한다. Phase C는 abandoned manifest를 다시 확인한 뒤 run canceled·chat discarded·recovery resolved·writer release·intent 제거를 한 SQLite transaction으로 commit한다. startup은 unclean-run 변환 뒤 pending intent를 재개하고 mismatch면 잠금을 풀지 않는다.
- **실패 주입·검증**: run별 예약 ID 차이/재시작 동일성/비파생, legacy reservation 저장 실패 후 retry, identityless chat hydration·revision 우선순위·byte-zero resolve, intent 전/후·filesystem 후·final commit 전/응답 전 crash, abandoned idempotence, manifest mismatch no-release를 회귀로 고정했다. sidecar 전체 77 passed·1 ignored와 stdio 10 passed, core 82 passed, protocol 32 passed, strict sidecar clippy, Rust 2021 rustfmt, `git diff --check`를 통과했다. 다음은 기능 commit의 독립 리뷰이며 실제 provider·인증·사용자 프로젝트·network·Windows는 실행하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C process identity 없는 recovery 계약)

- **좁은 예외**: actual recovery reason에 `processIdentityUnavailable`을 추가했다. 이 reason만 `trackedProcesses=[]`를 허용하며 반드시 unresolved·poisoned·`resolvedAt=null`로 남고 `resolvedAbandoned`로 바뀔 수 없다. 기존 세 reason은 nonempty·unique process identity 증거와 기존 resolve 규칙을 그대로 유지한다.
- **비가변 resolve·우선순위**: matching revision의 identityless recovery는 `after=null`인 `identityUnavailable`을 반환하고 아무 record도 바꾸지 않는다. stale revision은 먼저 `revisionConflict`가 되며, request와 before의 recovery ID 및 chat/task/project/worktree/workspace/run identity는 exact 일치해야 한다. 새 method·channel·capability·event·force resolve는 추가하지 않았다.
- **공용 골든·검증**: identityless record/chat detail/result와 상태·증거·identity drift, 기존 resolved/non-recovery/null chat 및 `resolvedAbandoned|processesRemain|revisionConflict` 회귀를 양 언어에서 고정했다. focused Node 13개, 전체 Node 174개, focused protocol 9개와 전체 protocol 32개, TypeScript no-emit, Rust 2021 format, 기존 범위 밖 lint 3건만 허용한 protocol clippy, `git diff --check`를 통과했다. strict clippy는 변경 전부터 있던 needless lifetime 2건과 collapsible-if 1건에서만 실패했다.
- **다음 할 일·경계**: 독립 계약 리뷰 뒤 sidecar가 identity barrier 전에 process identity를 얻지 못한 실제 기록을 만들고 resolve 우선순위를 집행해야 한다. sidecar/core/Electron/UI와 실제 provider·인증·사용자 프로젝트·Windows는 건드리거나 실행하지 않았고 merge·push도 하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C sidecar recovery 발견 계약 소비)

- **durable recovery 발견**: exact G5A+ `chat.get`이 `agentRuns`와 함께 required nullable `agentRecovery`를 항상 반환한다. 같은 chat의 durable actualAgent projection에서 unresolved recovery를 찾고 chat/task/project/worktree/workspace/run identity를 run·chat과 exact 대조하며, 정상·resolved detail은 null이다. 일반 exact G5 detail에는 두 field를 모두 추가하지 않고 legacy recovery page의 actual-free 경계도 유지한다.
- **fail-closed cardinality**: actual run record/projection identity 누락·불일치, 복수 unresolved actual recovery, recoveryRequired run과 unresolved recovery 수 불일치를 detail 구성 단계에서 거부하고 full 공용 `agentChatDetailPage` validator를 마지막에 다시 통과시킨다. production-shaped random recovery ID를 그대로 발견해 inspect/resolve에 사용하며 이전 deterministic fixture ID 재구성 seam은 제거했다.
- **검증·흐름**: terminal notification 직후 stdio hydration이 full identity/revision의 recovery를 읽고 actual inspect와 explicit abandon에 사용한다. poison/quit blocker와 legacy namespace 분리는 유지되며 resolve 뒤 unit hydration은 `agentRecovery=null`, run canceled, chat discarded를 확인한다. 모호성·identity·cardinality drift 실패 주입을 포함해 sidecar 75 passed·1 ignored와 stdio 10 passed, core 82 passed, protocol 31 passed, strict sidecar clippy, Rust 2021 scoped rustfmt, `git diff --check`를 통과했다. 실제 provider·인증·사용자 프로젝트·network·Windows와 공개 계약/Electron/UI는 변경하거나 실행하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C actual recovery renderer)

- **durable 발견·차단**: G5A+ `chat.get`의 required nullable `agentRecovery`를 initial/switch commit과 모든 durable hydration에 반영했다. current G5 detail처럼 `agentRuns`/`agentRecovery`가 없는 경우는 null로 호환하며, terminal notification의 run payload는 여전히 화면 상태가 아니라 hydration trigger일 뿐이다. unresolved actual recovery는 재개방·재조회 뒤에도 실행·파일 동작을 fail-closed로 막는다.
- **안전한 abandon**: 초보자용 확인 뒤 exact V4 `inspectAgentRecovery` full identity를 보내고, inspect가 돌려준 동일 record의 full identity·revision으로만 `resolveAgentRecovery(abandon)`을 호출한다. legacy inspect/mark API와 재구성한 recovery ID는 사용하지 않는다. resolved/processesRemain/revisionConflict 모두 durable detail을 다시 읽고, null ledger가 같은 active chat에 확인될 때만 차단을 푼다. 원본 프로젝트에는 적용하지 않는다는 UI 안내와 action을 추가했다.
- **검증·경계**: focused renderer Node 18개, TypeScript no-emit, Vite production build와 `git diff --check`를 통과했다. opaque recovery, cancel 뒤 hydration, inactive/pending event, stale chat response, G5 compatibility, processesRemain/revisionConflict 및 legacy API 0 호출을 회귀로 고정했다. 실제 provider·인증·사용자 프로젝트·Windows는 호출하지 않았고, 다음은 기능 branch 독립 리뷰와 최신 통합 기준 QA다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C recovery 발견 계약 리뷰 수정)

- **P1 관계 규칙 명시**: 일반 recovery API가 과거 `resolvedAbandoned` record를 검증할 수 있는 것과 달리, 현재 agent chat detail의 non-null `agentRecovery`는 반드시 `state=unresolved`이면서 `poisoned=true`여야 한다. resolved/non-recovery detail은 `agentRecovery=null`만 허용하도록 TypeScript/Rust 관계 validator에 직접 고정했다.
- **마스킹 없는 골든**: resolvedAt·poison·모든 process gone을 서로 맞춰 단독 recovery validator에는 완전히 유효한 resolved record를 만들고, 같은 record가 chat detail에 들어갈 때만 양 언어가 거부함을 확인했다. completed/succeeded run과 null recovery의 valid detail도 추가했다.
- **검증·경계**: focused Node 9개, 전체 Node 170개, focused protocol 8개와 전체 protocol 31개, TypeScript no-emit, Rust fmt, 기존 범위 밖 lint만 허용한 protocol clippy, `git diff --check`를 통과했다. 별도 원자 수정 commit으로 남기며 sidecar/Electron/UI나 새 API는 변경하지 않는다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C actual recovery 발견 계약)

- **발견 가능한 복구 ID**: exact G5A+ `chat.get` detail의 기존 `agentRuns` presence discriminator 안에 required nullable `agentRecovery`를 추가했다. `recoveryRequired` actual run은 같은 chat/task/project/worktree/workspace/run에 결속된 unresolved·poisoned·`actualAgent` recovery 하나를 반드시 노출하고, 그 외 상태는 null만 허용한다. recoveryRequired run이 없거나 둘 이상인데 recovery를 실으면 fail-closed라 chat당 unresolved actual recovery 하나만 표현된다.
- **ID·원장 경계**: chat detail은 random recovery ID를 재현할 수 없으므로 `agent_recovery_` namespace와 full identity만 검증한다. 반면 durable request/before record가 함께 있는 resolve 결과는 임의 대체 ID를 계속 거부한다. actual recovery를 legacy recovery page에 넣거나 agent detail에 legacy `recovery` member를 추가하는 공용 양언어 회귀도 거부하며, `agentRuns`/`agentRecovery`가 없는 current G5 detail은 그대로 유효하다.
- **골든·검증**: null/non-null valid와 missing field, recovery 누락·orphan, 모든 identity/run/source/reason/state/poison/namespace mismatch, non-recovery+recovery, 복수 recoveryRequired ambiguity, legacy member를 고정한 공용 fixture를 추가했다. focused Node 9개, 전체 Node 170개, focused protocol 8개와 전체 protocol 31개, TypeScript no-emit, Rust fmt, owned 변경 경고를 포함한 protocol clippy와 `git diff --check`를 통과했다. strict protocol clippy는 변경 전 범위 밖 needless lifetime 2건과 collapsible-if 1건만 보고해 해당 세 lint를 허용한 동일 명령으로 변경 범위를 확인했다.
- **다음 할 일·경계**: 이 계약 commit을 독립 리뷰한 뒤 sidecar가 agent detail에 durable actual recovery를 채우고 Electron/UI가 그 ID로 inspect/resolve하도록 별도 소비자 작업을 연다. 새 method/channel/capability/event는 없으며 실제 provider·인증·사용자 프로젝트·Windows는 실행하지 않았고 merge·push도 하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C sidecar 독립 리뷰 P1 수정)

- **raw framing과 공개 text 한도 분리**: provider JSONL read는 core의 framing 포함 128 KiB observation line ceiling을 사용하고, normalized public text는 계속 event당 64 KiB로 제한한다. 실제 launcher→drain→core classifier→SQLite/TeamMessage 경로에서 64 KiB 한 건과 16×64 KiB=1 MiB를 허용하고 aggregate +1 byte는 raw framing에 가려지지 않은 채 fail-closed 거부한다. newline 포함 exact/+1 framing 회귀와 기존 core total observation 한도도 유지했다.
- **provider recovery terminal 보존**: explicit `recoveryRequired` terminal은 child exit 0과 OS tree gone 관찰이 있어도 `failed`로 낮추거나 writer lock을 해제하지 않는다. 개별 gone 증거는 abandon 재확인용으로 보존하되 public termination proof는 불확실로 두고 run/chat/workspace/writer lock과 actual recovery를 함께 poisoned 상태로 저장한 뒤에만 terminal state event를 보낸다. 실제 stdio fixture에서 legacy recovery namespace 비노출, actual inspect, run/lock quit blocker, exact abandon 뒤 run canceled·chat discarded·lock release와 quit 허용을 확인했다. 명시적 provider `failed`와 auth/malformed 경로는 기존 non-poisoned failed/fail-closed 동작을 유지한다.
- **검증·경계**: sidecar 전체 74 passed·1 ignored와 stdio process 10 passed, core 82 passed, protocol 30 passed, strict sidecar clippy, Rust 2021 scoped rustfmt, `git diff --check`를 통과했다. 일반 `--stdio`의 exact G5, 공개 wire, 실제 Claude/Codex CLI·인증·사용자 프로젝트·network·Windows는 변경하거나 실행하지 않았다. 다음은 수정 commit의 독립 재리뷰와 최신 통합 게이트다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C fake provider sidecar 소비)

- **test-only provider 소비**: 일반 제품 `--stdio`의 exact G5/no-provider 표면은 그대로 두고 `--stdio-agent-fixture`만 순수 core provider API를 소비한다. capability는 bounded 주입 관찰을 cache/refresh하고 start마다 executable closure·version·auth·catalog·immutable role profile을 새로 관찰한다. Claude `claude-opus-5` exact selector만 fake positive이며 Codex `argvOnly`와 기존 `opus`/`sol` alias는 silent fallback 없이 `exactModelUnavailable`로 run·spawn·writer lock 0에서 차단한다.
- **handshake·attestation 순서**: 기존 launcher barrier와 `TreeTracker`를 유지하고 release 뒤 Claude version/model/cwd/permission/tools/MCP/plugins/config handshake를 exact 검증한다. accepted run·capability·role profile·managed workspace에 결속한 sanitized discovery/attestation을 원자 저장한 뒤에만 `running`과 public event를 허용한다. 사전 공개, 각 handshake drift, auth/terminal/malformed/oversize, attestation 저장 실패는 raw provider carrier를 공개하지 않고 full tree 종료로 fail-closed 처리하며, 불확실 종료의 poisoned actual recovery와 legacy/actual recovery namespace는 유지한다.
- **내구·비밀 경계**: G5B 256 event·1 MiB text·16 MiB projection 예산과 terminal transaction/cancel/recovery/quit 순서를 재사용했다. restart 시 discovery/attestation을 재검증하고 recovery 전환에도 보존한다. SQLite projection·JSONL public surface·stderr-facing 오류에서 raw auth/version/vendor output, token, prompt, email, org, reasoning carrier가 남지 않는 회귀를 추가했다.
- **검증·경계**: sidecar 전체 70 passed·1 ignored와 stdio process 9 passed, core 82 passed, protocol 30 passed, strict sidecar `clippy --all-targets --no-deps -D warnings`, Rust 2021 scoped rustfmt, `git diff --check`를 통과했다. missing helper/unknown version·auth, cache refresh/stale closure/profile·fingerprint drift, Claude handshake 8차원과 pre-public/attestation failure, Codex/legacy alias 차단, clean/no-output/delayed/cancel/uncertain/restart recovery와 ledger namespace를 포함했다. 실제 Claude/Codex CLI·인증·사용자 프로젝트·network·Windows는 실행하지 않았으며 다음은 독립 리뷰와 통합 게이트다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C pure Rust provider core)

- **순수 provider 경계**: host 탐색·process 실행 없이 주입된 bounded 관찰과 경로/파일 identity digest만 소비하는 core provider 정의를 추가했다. Claude `2.1.241`+`node`, Codex `0.150.0`+`rg` closure와 exact profile/parser/launch policy를 canonical SHA-256 manifest로 만들며 공용 G5C golden digest와 일치한다. 설치·인증·버전·closure·catalog·tuple·verification이 하나라도 불확실하면 startable로 만들지 않고, Codex `argvOnly`/unknown catalog/effective profile은 계속 차단한다.
- **handshake·비밀 경계**: Claude fake `system:init`의 exact version/model/managed cwd/permission/tools/MCP/plugins/config isolation을 모두 확인한 뒤에만 attestation과 첫 Handshake event를 만든다. forged discovery도 attestation 직전에 전체 manifest와 재결속한다. auth/version/vendor 원문과 token/prompt/email/org는 결과·Debug에 남지 않고, 미지원 profile 문자열도 빈 sanitized tuple로 접는다. 이후 event는 handshake 전 공개를 거부하고 `status|question|final`만 G5B 256-event·1 MiB text 안에서 공개하며 private/auth/terminal은 raw body 없이 분류한다.
- **검증·경계**: core 82개와 workspace 전체 Rust 테스트(Tauri 105 passed·13 ignored, protocol 30, sidecar 68 passed·1 ignored+stdio 8 포함), Rust 2021 scoped rustfmt, `git diff --check`를 통과했다. strict workspace clippy는 변경 전부터 있던 core/protocol/Tauri lint 11건만 보고했고 해당 lint를 명시적으로 허용한 `--workspace --all-targets --no-deps -D warnings`는 통과했다. 실제 Claude/Codex CLI·인증·사용자 프로젝트·network·Windows는 실행하지 않았으며 public wire·sidecar·Electron은 변경하지 않았다. 다음은 독립 core 리뷰 후 승인된 commit만 sidecar 소비 작업의 기준으로 쓰는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C aggregate text 경계 증거 보정)

- **공용 경계 fixture**: aggregate Public text 허용 경계를 16개 × 65,536 bytes = 정확히 1 MiB로 고정하고, 거부 경계는 같은 16개 뒤 1-byte 17번째 메시지를 붙인 1 MiB+1로 고정했다. 두 경우 모두 256-event 미만이고 각 메시지는 per-message 상한 이하임을 Rust/TypeScript가 fixture 기대값과 함께 검증한다.
- **검증·경계**: exact 1 MiB는 양언어 validator가 허용하고 +1은 거부하며, 별도 257-event 사례도 유지했다. Focused Node 6개, 전체 Node 169개, protocol Rust 30개, TypeScript no-emit, scoped Rust 2021 rustfmt, 기존 범위 밖 lint만 제외한 protocol clippy, `git diff --check`를 통과했다. 계약 구현·공개 wire·실제 provider 실행은 변경하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C 계약 독립 리뷰 수정)

- **P1 event 예산**: internal provider sequence가 normalization/persistence 전 existing G5B run budget인 Public 최대 256개와 Public text UTF-8 누적 1 MiB를 양언어에서 검사한다. 공용 golden의 결정적 generator로 257개와 1 MiB+1 byte를 fixture 팽창 없이 동일하게 거부한다.
- **P1 attestation 결속**: `validateAgentRuntimeAttestationAgainstAcceptedV1`/Rust 대응 함수가 기존 accepted run과 resolved profile에 run ID·capability fingerprint·workspace identity·runtime·model·reasoning·permission을 exact 결속한다. 각 차원을 독립 drift한 golden은 기본 attestation이 유효한 상태에서 context 비교로만 거부된다.
- **P2 session 정보 제거·검증**: 비인증성 deterministic derivation이 없으므로 persisted attestation에서 `sessionDigest`를 제거하고 extra field 회귀로 금지했다. Focused Node 6개, 전체 Node 169개, protocol Rust 30개, TypeScript no-emit, scoped Rust 2021 rustfmt, 기존 범위 밖 lint만 제외한 protocol clippy, `git diff --check`를 통과했다. 실제 CLI·인증·사용자 프로젝트·Windows는 실행하지 않았고 공개 wire는 변경하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5C provider discovery·launch attestation 계약)

- **내부 계약**: 공개 G5/G5A/G5B method·capability·event와 Electron V4/V5를 바꾸지 않고 `ProviderManifestV1`·`SanitizedDiscoveryObservationV1`·`AgentRuntimeAttestationV1`을 Rust/TypeScript에 같은 canonical JSON·SHA-256 규칙으로 고정했다. 매니페스트는 binary와 필수 helper 전체 closure, exact model/reasoning/permission tuple, provider별 auth/event parser ID, managed cwd·allowlisted no-secret env·no-fallback launch policy를 묶는다.
- **시작 증거·분류 경계**: Claude fake golden은 version/model/cwd/permission/tools/MCP/plugins/config를 handshake와 attestation에서 모두 확인해야 시작 가능하다. Codex `argvOnly`와 unknown catalog는 `modelCatalogUnverified`로 `canStart=false`이며 fallback하지 않는다. 내부 event는 `Handshake|Public|Private|Terminal|AuthFailure`만 허용하고 handshake 전 public, 기존 `status|question|final` 밖 공개 kind, raw stdout/stderr·token·prompt·email·org·raw auth hash와 extra/private 필드를 fail-closed 거부한다.
- **검증·남은 경계**: focused Node 5개, 전체 Node 168개, protocol Rust 30개, TypeScript no-emit, scoped Rust 2021 rustfmt, protocol clippy(기존 범위 밖 lint 3종만 명시적 제외), `git diff --check`를 통과했다. strict clippy 원문은 변경 전부터 있던 `run_recovery_v1.rs`·`workspace_v1.rs` needless lifetime 2건과 `lib.rs` collapsible-if 1건에서만 실패했다. 실제 Claude/Codex CLI·인증·사용자 프로젝트·Windows는 실행하지 않았고, SQLite lifetime/retention/compaction은 실제 provider 활성화 전 별도 stop condition으로 남긴다. 다음은 독립 계약 리뷰 후 소비자 작업 계약을 여는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B 최종 gate)

- **최종 판정**: 독립 gate가 exact docs commit `3dcf75ab1c12cf4a21b05406f4fc814e42d485f7`에서 full-array-prefix 증폭 계약, terminal commit 순서와 실패 후 active/restart recovery, authoritative ledger counter, zero-output·지연 종료, Electron V5 binding·state·session 한도, renderer durable hydration, exact G5/G5A 호환, raw/private 차단과 legacy/actual recovery 분리를 교차검증했다. 차단 finding 없이 `G5B_READY`로 판정했다.
- **재검증·경계**: gate가 전체 Node 163개, focused G5B/Electron/UI 62개, sidecar 68개 통과·1 ignored와 stdio 8개, protocol 29개, TypeScript, Vite build, strict sidecar clippy, G5B Rust scoped rustfmt와 diff check를 다시 통과했다. workspace 전체 fmt는 G5B와 무관한 기존 `src-tauri` 포맷 차이만 보고했다. 실제 provider·인증·사용자 프로젝트·Windows와 SQLite cross-session 보존/compaction은 후속이며 `main` 병합·push는 하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B 누적 하네스·독립 QA)

- **누적 게이트**: exact 통합 commit `022b79517462a1f9339c8cabb68b5fafd65eae0b`에서 G5 run/recovery 누적 하네스와 macOS arm64 Electron package smoke를 통과했다. 증거는 `.artifacts/m5-harness/g5-run-recovery-2026-08-27T06-03-46-910Z.json`이다. 첫 시도는 격리 환경의 Electron binary 다운로드 차단만으로 실패했고, 네트워크 허용 환경에서 같은 명령을 재실행해 코드 실패와 분리했다.
- **독립 QA**: 별도 QA가 같은 exact commit에서 focused G5B 계약·Electron·renderer, 전체 Node 163개, sidecar 68개 통과·1 ignored와 stdio 8개, protocol 29개, TypeScript no-emit, macOS arm64 package smoke를 다시 실행했다. zero-output·지연 종료, terminal 저장 실패 뒤 재시작 recovery/poison, event/text/full-array-prefix budget, counter drift, Electron registry 한도·terminal replay, durable hydration을 확인해 `G5B_QA_PASS`로 판정했다. 증거는 `.artifacts/m5-qa/g5b-integration-022b79517462a1f9339c8cabb68b5fafd65eae0b-2026-08-27T06-07-43Z.json`이다.
- **문서·경계**: 현행 구조 문서를 V5 terminal notification과 run/session 예산까지 갱신했다. 일반 `--stdio`의 실제 Claude Code/Codex provider·인증·모델 출력, 사용자 프로젝트, Windows는 실행하지 않았고, 여러 세션·채팅에 걸친 SQLite 전체 용량·보존·compaction은 G6 미해결 사항이다. 다음은 이 문서 commit을 포함한 독립 G5B 최종 gate이며 `main` 병합·push는 하지 않는다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B terminal·resource budget 통합)

- **통합**: 독립 승인된 G5B 계약·원장 증폭 교정·renderer·Electron·Rust terminal/budget 커밋을 `m5/coding-agent-ide`에 순서대로 통합했다. `agent.runStateChanged`는 terminal 영속 commit 뒤 같은 actual-run stream으로 발행되고 renderer는 그 payload를 직접 상태로 쓰지 않고 durable `chat.get` hydration trigger로만 사용한다.
- **안전 리뷰 수정**: 최초 계약의 개별 event byte 합이 SQLite full-projection 반복 저장의 O(n²) 증폭을 제한하지 못해 full-array prefix snapshot 누적으로 교정했다. Rust 독립 리뷰가 terminal 저장 전 active ownership 제거와 live text/cumulative counter drift 검증 누락을 차단했고, terminal commit 뒤에만 remove·재시작 nonterminal recovery/poison·매 append canonical ledger 재계산으로 수정한 뒤 재승인했다.
- **예산·회귀**: run당 public event 256·text 1 MiB·immutable ledger amplification 16 MiB, session당 active run 32·seen run 1,024·event ID 8,192·agent stream 1,024·protocol stream 4,096을 양언어/Electron/Rust에서 fail-closed 적용했다. 통합 기준 전체 Node 163개, sidecar 68개 통과·1 ignored와 stdio 8개, protocol 29개, TypeScript no-emit, Vite build, strict sidecar no-deps clippy, rustfmt, diff check를 통과했다. `m5:test:run-recovery`에 G5B contract 회귀를 추가했으며 다음은 누적 G5 하네스·통합 QA·최종 gate다. 실제 CLI·인증·모델·사용자 프로젝트·Windows는 실행하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B Rust 독립 리뷰 P1·P2 수정)

- **P1 ownership/restart 수정**: terminal run·chat·lock/recovery payload와 state event를 모두 구성·검증하고 atomic upsert가 commit될 때까지 `AgentRuntime.active` ownership을 유지한다. Construction/upsert 실패는 terminal event를 만들지 않고 active evidence를 남기며, 다음 sidecar 시작은 actual `preparing|running|cancelling|recoveryRequired` durable run을 모두 run/chat/lock 하나의 SQLite transaction으로 poisoned `recoveryRequired`에 수렴시킨다. 이미 terminal run은 그대로 두고, identity barrier 전에 죽은 preparing run은 process identity를 꾸며내지 않은 영구 blocker로 표시한다.
- **P2 live counter 수정**: live poll과 append admission마다 `public_events` 전체에서 count, text UTF-8 합, canonical full-array prefix 누적 bytes를 다시 계산해 세 durable counter와 exact 대조한 뒤 ceiling을 검사한다. text/cumulative undercount 변조는 다음 event 저장·전달 전에 session failure가 되고, restart recovery가 authoritative ledger로 counter를 복원하면서 writer lock을 poison한다.
- **실패 주입·검증**: terminal construction과 terminal projection upsert 실패에서 active ownership·durable running·terminal event 0을 확인하고, runtime drop 뒤 recovery record와 writer lock poison을 확인했다. Text/cumulative live undercount도 event count 불변·재시작 recovery를 검증했다. sidecar unit 68 passed·1 ignored와 stdio process 8개, protocol 29개, strict sidecar clippy, Rust 2021 rustfmt, `git diff --check`를 통과했다. 실제 CLI·인증·model·사용자 프로젝트·Windows는 실행하지 않았다.
- **다음 할 일**: 수정 commit을 같은 exact SHA로 독립 재리뷰한 뒤 승인된 Rust commit만 G5B 통합 후보로 사용한다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B Rust terminal event·resource budget 소비)

- **상태 통지 순서**: fake actual-agent accepted response는 계속 event 없이 먼저 반환하고, 첫 poll이 같은 run 전용 stream의 seq 1 `running` `agent.runStateChanged`를 내보낸다. normalized public event가 이후 연속 seq를 사용하고, process-tree 종료·run/chat/lock/recovery 원자 저장이 성공한 뒤 전체 durable run을 담은 terminal state event가 마지막 seq로 나간다. zero-output도 `running→terminal`이며 `final` 뒤 process가 살아 있는 동안 run은 계속 `running`이다. cancel은 terminal transaction을 먼저 commit하고 response 뒤 running/terminal event를 전달한다.
- **내구 예산·fail-closed**: actual run projection에 public event count·text UTF-8 bytes·full-array prefix snapshot 누적 bytes를 저장하고 sidecar 재시작 전 전체 ledger와 exact 대조한다. append는 기존 내구 counter와 다음 canonical full-array snapshot을 먼저 계산해 256 event·1 MiB text·16 MiB cumulative amplification 초과 event를 저장·공개하지 않는다. collector도 세 한도를 넘겨 성장하지 않으며, session active actual run 32개는 spawn·writer lock 전에 차단한다. proven 종료는 `failed/cleanExit`, 관찰·wait 불확실은 poisoned `recoveryRequired`다.
- **오류 전파·검증**: `AgentRuntime::poll`이 process-tree observe, `try_wait`, tracker/public/terminal persistence와 state-event construction 오류를 stdio owner에 반환하도록 바꾸고, 오류 시 process-tree를 먼저 종료한 뒤 가능한 recovery evidence를 저장한다. 실패 주입은 poll observe·wait, durable counter 변조, terminal binding 변조, text +1 초과를 포함한다. sidecar unit 66 passed·1 ignored와 stdio process 8개, protocol 29개, Rust 2021 rustfmt, strict sidecar clippy, `git diff --check`를 통과했다. 실제 CLI·인증·model·사용자 프로젝트·Windows는 실행하지 않았다.
- **다음 할 일**: 이 기능 commit을 독립 Rust 안전 리뷰한 뒤 Electron V5 registry budget/state listener와 renderer hydration trigger 소비자와 통합하고 최신 누적 하네스·QA를 실행한다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B Electron V5 terminal notification·session budget)

- **V5 알림 경계**: exact G5A negotiated session에서만 normalized `agent.runStateChanged`를 fixed `m5:v5:agent:run-state-changed`와 typed preload `onAgentRunStateChanged`로 전달했다. 이 표면은 새 renderer state source가 아니라 `chat.get`의 durable chat/run ledger를 다시 읽게 하는 hydration trigger뿐이며, V3/V4 channel과 raw/generic IPC 금지는 유지했다.
- **fail-closed 원장**: Electron은 accepted/hydrated full binding과 fingerprint를 session tombstone으로 보존하고 `preparing → running(선택) → terminal`만 허용한다. terminal 뒤 public event, terminal/active regression, unknown/mutated binding·envelope, reused event ID, 같은 run의 다른 stream 및 같은 stream의 다른 run은 listener 전 poison한다. terminal listener를 보낸 뒤 active binding만 제거하고 replay/mutation tombstone은 session 종료까지 남긴다.
- **한도·검증**: shared constants로 active actual run 32, seen run 1,024, seen event ID 8,192, actual stream 1,024, protocol stream 4,096을 Electron에도 적용했다. focused Electron 43개와 전체 Node 158개, root pinned `tsc --noEmit`, Vite build, `git diff --check`를 통과했다. worktree `pnpm exec`는 격리 DNS 의존성 fetch에서 실행할 수 없어서 root pinned binary로 typecheck/build를 재실행했다. 실제 CLI·인증·model·사용자 프로젝트는 호출하지 않았다.
- **다음 할 일**: 이 Electron feature commit의 독립 리뷰 뒤 Rust terminal persistence/event/budget 구현 및 renderer hydration subscription을 통합 기준에서 QA한다.

### 2026-08-27 · by GPT-5.6-Terra (M5 G5B renderer terminal durable hydration)

- **V5는 trigger만**: `agent.runStateChanged` listener는 event의 `run`을 화면 상태에 직접 쓰지 않고 기존 chat/generation별 hydration latch로만 `chat.get` tail 재조회를 요청한다. zero-public-output terminal도 durable `agentRuns`가 `succeeded` 같은 terminal을 반환한 뒤에만 action lock이 풀리고, durable ledger가 아직 `running`이면 event의 terminal 주장만으로 바뀌지 않는다. `final` public event의 nonterminal 의미는 유지했다.
- **동시성·호환**: public/state burst는 같은 in-flight hydration에 합치고 empty-tail response와 같은 turn의 state 통지는 한 번 더 durable tail을 확인한다. pending 채팅은 동일 재수화에 합치며 inactive 채팅은 raw state를 붙이지 않고 refresh 표시만 남긴다. dispose는 V3 chat·V4 public·V5 state listener를 모두 해제하고 V3/V4 bridge에 V5 listener가 없으면 기존 fixture UI를 유지한다.
- **검증·경계**: focused renderer 14개, full Node 159개, TypeScript no-emit, Vite build, `git diff --check`를 통과했다. 실제 CLI·인증·모델·사용자 프로젝트는 호출하지 않았고, Electron/Rust/protocol/manifest는 수정하지 않았다. 다음은 이 renderer commit의 독립 리뷰와 Rust/Electron V5 producer 통합이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B ledger write amplification 계약 교정)

- **감사 결함 교정**: 최초 G5B 계약의 개별 public event byte 합은 sidecar가 append마다 커지는 `public_events` 전체 배열 projection을 immutable ledger event로 저장하는 실제 쓰기 증폭을 반영하지 못했다. 16 MiB 한도를 run의 각 append 직후 전체 배열 snapshot canonical UTF-8 byte 길이를 누적한 값으로 다시 고정해 O(n²) storage amplification을 계측 대상으로 포함했다.
- **결정적 단위·골든**: canonicalization은 재귀 ASCII key 정렬·compact JSON, 배열 순서 보존, explicit null 포함이다. 양언어 helper를 prospective full-array snapshot과 prefix snapshot 누적 API로 교체했고, 한글 text를 포함한 같은 run의 1-event snapshot `431` bytes와 2-event snapshot `837` bytes·누적 `1268` bytes를 공용 golden으로 고정했다.
- **호환·검증**: event count·text·session 예산, `agent.runStateChanged`, exact G5/G5A capability/method 배열은 변경하지 않았다. focused Node 17개, 전체 Node 154개, TypeScript no-emit, Rust protocol 29개, owned Rust rustfmt, scoped protocol clippy, `git diff --check`를 통과했다. sidecar·Electron·renderer 소비자는 변경하지 않았으며 다음 단계에서 이 누적 단위를 append 전 fail-closed admission에 사용해야 한다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5B terminal notification·resource budget 계약)

- **additive terminal 통지**: protocol v1에 `agent.runStateChanged`를 예약하고 payload를 exact `{schemaVersion,eventId,run}`으로 고정했다. event ID는 `agent_state_event_`, run은 전체 `AgentRunRecordV1`이며 envelope chat/task/run과 accepted의 project/worktree/workspace/role/fingerprint까지 exact 결속한다. wire state는 `running|succeeded|failed|canceled|recoveryRequired`만 허용하고 `preparing|cancelling`, active/terminal evidence 불일치, extra/private/raw field를 양언어에서 거부한다. V5 bridge listener는 직접 표시 데이터가 아니라 durable `chat.get` 재수화 trigger다.
- **한도·계측 단위**: run당 공개 event 256개·공개 text UTF-8 합계 1 MiB·immutable public ledger 16 MiB, session당 active actual run 32·seen run ID 1024·seen event ID 8192·actual stream ID 1024·전체 protocol stream ID 4096을 Rust/TypeScript/golden에 같은 수치로 고정했다. ledger bytes는 검증된 `AgentPublicEventV1` 각각을 재귀 ASCII key 정렬·compact JSON·UTF-8로 잰 합계이며 canonical fixture는 422 bytes다. protocol session은 actual public/state event가 한 run당 같은 전용 stream을 쓰고 event ID를 재사용하지 못하게 한다.
- **호환·검증**: 기존 exact G5/G5A capability/method 배열과 일반 `--stdio` 의미는 변경하지 않았다. focused Node 17개, 전체 Node 154개, TypeScript no-emit, Rust protocol 29개, owned Rust rustfmt, 기존 lint 세 종류만 명시적으로 제외한 scoped protocol clippy, `git diff --check`를 통과했다. strict clippy 원문은 변경 전부터 있던 `run_recovery_v1.rs`·`workspace_v1.rs` needless lifetime 2건과 `lib.rs` collapsible-if 1건 때문에 실패하며 범위 밖 정리는 하지 않았다.
- **다음 할 일**: 계약 commit을 독립 리뷰한 뒤 Rust sidecar가 terminal persistence commit 이후 같은 stream으로 event를 방출하고 예산 초과를 fail-closed 처리하며, Electron이 session registry 한도를 적용하고 renderer는 event를 hydration trigger로만 소비해야 한다. 실제 CLI·인증·model·사용자 프로젝트는 실행하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A normalized public-event 수직 통합 게이트)

- **통합 경계**: contract·Rust fake runtime·Electron V4·renderer 변경을 독립 리뷰 뒤 통합했다. 테스트 전용 `--stdio-agent-fixture`만 exact G5A를 광고하고 제품 Electron이 실행하는 일반 `--stdio`는 exact G5를 유지한다. fake actual-agent의 full-identity `agent.publicEvent`만 고정 IPC로 전달하며 raw/private carrier와 legacy/actual recovery 원장은 renderer 전에 fail-closed 분리한다.
- **renderer·호환**: renderer는 public event text를 직접 붙이지 않고 durable `chat.get` 페이지와 별도 `agentRuns` 원장을 재수화한다. current G5의 `agentRuns` 없는 detail과 legacy fixture 흐름은 그대로 동작하고, actual cancel의 `recoveryRequired`는 hydration 중에도 새 fixture/actual 시작을 즉시 막으며 `final`을 terminal 증거로 추론하지 않는다.
- **게이트 수정**: 최초 final gate가 진행 중 hydration의 마지막 empty page 응답 결정 뒤 같은 turn에 public event가 오면 trigger가 기존 Promise에 흡수되는 P1을 재현해 `ITERATE`했다. 채팅·generation별 trigger latch가 미소비 신호에만 empty-tail 재조회를 한 번 더 수행하도록 수정하고, exact interleaving에서 sequence 3 자동 수화·중복 없음·replay 1회를 고정했다. 독립 UI 재리뷰는 `APPROVE`했다.
- **재게이트·QA**: 수정 exact `24a8c712e6b5a57e69c9e852333c46bfab536dc7`에서 누적 G5 하네스가 PASS했다(`.artifacts/m5-harness/g5-run-recovery-2026-08-27T04-39-02-052Z.json`; Node run/recovery 78개·sidecar 59개 통과/1 ignored·stdio 8개 및 macOS arm64 package smoke 포함). 독립 재QA도 focused renderer 9개, 계약/Electron 47개, fake stdio 8개와 tail calls `[1,2,3]`·durable sequence `[1,2,3]`을 확인해 `G5A_QA_PASS`로 판정했다(`.artifacts/m5-qa/g5a-hydration-tail-24a8c712e6b5a57e69c9e852333c46bfab536dc7-2026-08-27T04-43-00Z.json`). 두 격리 실행의 Electron fetch 실패는 네트워크 허용 환경에서 같은 명령을 재실행해 코드 실패와 분리했다.
- **최종 판정**: 독립 gate가 exact `4cae73e6`에서 이전 P1 재현, latch 소비·generation 정리, G5/G5A 분리, raw/private 차단, recovery namespace·action gate, current G5 호환과 위 증거를 교차검증해 차단 finding 없이 `G5A_READY`로 판정했다. 이는 테스트 전용 fake normalized public-event vertical의 완료이며 M5 전체 완료나 실제 provider 준비 판정이 아니다.
- **남은 일**: 실제 CLI·인증·모델·사용자 프로젝트·Windows는 실행하지 않았다. 실제 provider 전에 terminal 상태 통지 또는 bounded polling, public-event cardinality/disk/dedupe 예산을 고정해야 하며 G6 보안·패키징·성능, Windows 자손 회수, RC·서명 검증이 남았다. `main` 병합·push는 하지 않았다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A renderer cancel recovery gate 보정)

- **P2 수정**: actual cancel이 `recoveryRequired`를 반환하면 `chat.get` 재수화가 끝나기 전에도 non-null `agentRecovery`를 recovery gate로 즉시 취급한다. 따라서 fixture demo·actual start·연결된 버튼은 같은 fail-closed 상태를 보며 새 실행 요청을 만들지 않는다. unresolved actual recovery는 재수화 뒤에도 별도 해소 전까지 안전하게 차단한다.
- **회귀·검증**: cancel 뒤 durable hydration을 의도적으로 지연한 상태에서 fixture/actual start RPC가 각각 0회임을 고정했다. focused state 12개, TypeScript no-emit, Vite build, 전체 Node 147개, `git diff --check`를 통과했다. 실제 CLI·인증·model·사용자 프로젝트는 호출하지 않았다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A renderer public-event 소비)

- **영속 기록 우선**: renderer는 V4 `onAgentPublicEvent`를 V3 legacy chat listener와 함께 구독하되, public event의 text를 채팅에 직접 넣지 않는다. active/pending chat은 현재 연속 sequence 뒤의 bounded `chat.get` 페이지를 끝까지 다시 읽고 그 durable `TeamMessage`만 합치며, `agentRuns` ledger도 같은 detail에서 새로 고친다. inactive chat은 내용 대신 refresh 표시만 남긴다.
- **실제 실행 UI**: 채팅 생성 시 고정된 role snapshot의 runtime/model을 그대로 사용해 capability를 확인하고, unavailable이면 이유를 알려 주며 다른 모델로 fallback하지 않는다. start에는 현재 durable message ID와 capability fingerprint만 넣고, cancel은 chat/task/project/worktree/workspace/run 전체 identity를 보낸다. `final` public event는 process 종료로 취급하지 않으며, cancel의 `recoveryRequired`는 legacy abandon으로 바꾸지 않고 안전한 복구 알림으로만 보인다.
- **검증·경계**: public raw text 미노출, burst coalescing, page 순서, pending/inactive event, current G5 compatibility, no-fallback start, exact start/cancel payload, recovery notice 및 양쪽 listener dispose를 state test로 고정했다. root pinned TypeScript no-emit, Vite build, 전체 Node 146개, `git diff --check`를 통과했다. 실제 Claude/Codex CLI·인증·model·사용자 프로젝트는 호출하지 않았다. 다음은 이 기능 branch의 독립 리뷰와 최신 통합 기준 QA다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A Rust normalized public-event 소비)

- **협상·순서**: 결정적 `--stdio-agent-fixture`만 exact G5A capability를 광고하고 일반 `--stdio`는 exact G5를 유지한다. actual start는 event 없는 accepted response를 먼저 반환하며, 이후 영속 `AgentPublicEventV1` 전체를 `agent.publicEvent` seq 1부터 연속 발행한다. actual start/finish의 raw `run.event`·`run.finished` carrier는 제거했고 기존 G5 fixture run event는 변경하지 않았다.
- **조회·채팅 wire**: 협상된 G5A의 `chat.get`은 run이 없어도 `agentRuns: []`를 항상 포함하고, current G5는 해당 멤버를 생략한다. sidecar의 `chat.eventAppended` payload를 계약·Electron이 요구하는 direct `TeamMessageV1`로 바로잡아 기존 G5 공개 메시지 의미와 durable ledger는 유지했다.
- **검증·경계**: 실제 sidecar process 회귀에서 ready exact G5A, pre-run 빈 agent ledger, accepted-before-event, `status→question→final` seq 1~3 full identity/fingerprint, raw actual event 0건, durable TeamMessage와 terminal agent run을 확인했다. sidecar 59 passed·1 ignored와 stdio 8 passed, protocol 26 passed, owned Rust format, strict no-deps sidecar clippy, `git diff --check`를 통과했다. 실제 Claude/Codex CLI·인증·model·사용자 프로젝트는 호출하지 않았다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A Electron normalized public-event 소비)

- **한 일**: exact G5A ready 협상에서만 `agent.publicEvent`를 fixed `m5:v4:agent:public-event` IPC와 `onAgentPublicEvent` typed preload listener로 전달했다. Sidecar client는 start accepted와 `chat.get.agentRuns` hydration의 full chat/task/project/worktree/workspace/run/role/fingerprint binding을 한 원장으로 결합한 뒤, known run에 exact 결속된 공개 payload만 listener 전에 통과시킨다.
- **안전 결정**: public `eventId` 재사용, stream의 다른 run 재사용, unknown/unnegotiated run, full binding 변조, envelope 변조, extra/private field와 oversize는 session을 poison하며 listener에 전달하지 않는다. actual raw `run.event`/`run.finished`와 actual `chat.eventAppended`는 계속 poison이고, preload도 event payload를 독립 exact validate한 뒤 기존 credit/ACK 경로로만 renderer에 전달한다. current G5/V3 표면과 generic/raw IPC는 넓히지 않았다.
- **검증·다음**: Electron bridge/scaffold focused Node 39개와 전체 Node 139개, root의 고정 도구로 TypeScript no-emit 및 Vite production build, `git diff --check`를 통과했다. worktree 자체 의존성이 없어 `pnpm exec`는 격리 네트워크 DNS에서 패키지를 받지 못했으며 package/lockfile은 바꾸지 않았다. 실제 Claude/Codex CLI·인증·model·사용자 프로젝트는 호출하지 않았다. 다음은 이 Electron commit의 독립 리뷰와 최신 통합 기준 QA다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A normalized public-event 계약)

- **공용 wire**: protocol v1에 `agent.publicEvent`를 additive 예약했다. envelope는 `chatId`·`taskId`·`runId`를 모두 요구하고, exact `AgentPublicEventV1` payload의 같은 세 identity와 일치해야 한다. payload는 `status|question|final`, bounded text와 전체 run identity/fingerprint만 허용하고 extra·reasoning·raw/private 필드는 계속 fail-closed다.
- **namespace·bridge**: 공개 event ID를 `agent_event_` namespace로 고정하고 TypeScript/Rust validator와 골든을 함께 올렸다. V4는 generic bus나 request를 추가하지 않고 `onAgentPublicEvent(listener) -> unsubscribe` 전용 listener만 더했다. G5A capability/method 집합과 current G5 handshake는 변경하지 않았다.
- **검증·경계**: envelope identity 누락/변조, public full binding/fingerprint 변조, legacy event ID, private/raw extra와 oversize를 양언어에서 거부했다. 전체 Node 137개, Rust protocol 26개, TypeScript no-emit, 변경 Rust format, scoped protocol clippy와 `git diff --check`를 통과했다. 실제 CLI·인증·model·사용자 프로젝트는 호출하지 않았다. 다음은 sidecar가 이 event만 방출하고 Electron이 협상된 G5A에서만 전용 listener로 전달하도록 별도 소비자 파동에서 구현하는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A Rust·Electron 통합과 recovery namespace 감사 종결)

- **통합**: 독립 승인된 Electron V4 소비 경계 네 커밋을 `ba8eae2`·`6cfe094`·`2436498`·`ca6deaa`로, Rust fake adapter와 안전 수정 네 커밋을 `1040dc2`·`f3f6685`·`337d580`·`8d2cd6c`로 `m5/coding-agent-ide`에 통합했다. 기능 worktree의 공용 계약 동기화 중복 커밋은 제외했고, 충돌한 워크로그는 각 구현·리뷰 기록을 보존했다.
- **namespace 감사 종결**: legacy G5 recovery 조회는 `source_kind=actualAgent`만 제외하고 discriminator가 없거나 다른 기존 기록은 계속 허용한다. actual recovery는 legacy list·inspect·markResolved에서 보이지 않고 변경되지 않으며, actual 전용 API는 같은 기록을 계속 처리하고 반대로 fixture recovery를 거부하는 양방향 회귀가 통과했다.
- **추가 안전 수정**: 독립 Rust 리뷰에서 발견한 active 등록 전 observe/구성 실패의 preparing orphan과 실제 pipe release 전 `releaseGranted=true` 기록을 공통 fail-before-release·사후 atomic transition으로 닫았다. 동적 자손 identity의 durable 복구, accepted/public full identity·fingerprint 결속도 유지됐고 재리뷰는 최종 `APPROVE`했다.
- **통합 검증·경계**: namespace focused Rust 2개, sidecar lib 59개(1 ignored)와 stdio 7개, Electron bridge/scaffold 37개, TypeScript no-emit, 변경 Rust `rustfmt --check`, strict sidecar clippy, `git diff --check`를 통과했다. workspace 전체 fmt는 변경 범위 밖 Tauri 기존 포맷 차이가 있어 변경 파일 기준으로 확인했다. 실제 Claude/Codex CLI·인증·model·사용자 프로젝트는 실행하지 않았고 `main` 병합·push도 하지 않았다. 다음은 normalized public-event wire와 UI 소비를 별도 계약·구현 파동으로 여는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A release barrier 재리뷰 수정)

- active 등록 전 attach `TreeTracker::observe`와 evidence/chat projection 구성, drain thread 생성, release evidence 저장, accepted 검증의 모든 fallible 경로를 공통 fail-before-release 종료로 연결했다. 직접 handshake한 child identity를 열거 전에 tracker에 넣어 observe 실패 주입에서도 launcher와 child 모두의 소멸을 증명하거나 불확실 recovery로 접는다.
- durable pre-release barrier는 `releaseGranted=false`로 먼저 기록하고 release pipe write가 성공한 뒤 별도 atomic projection 전이에서만 true가 된다. injected release failure는 false barrier, terminal failed, clean writer release와 DB reopen 뒤 동일 상태를 검증하며 정상 run만 true barrier로 진행한다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A actual-agent wire identity 소비)

- 공용 계약 wire 보강 commit을 소비해 `run.startAgent` accepted와 durable public event가 chat/task/project/worktree/workspace/run/role identity와 capability fingerprint를 내부 immutable `RunIdentity`·run record에서 exact echo하도록 바꿨다. 짧은 이전 payload는 더 이상 만들지 않으며 두 payload 모두 공용 Rust validator를 통과한 뒤에만 반환·영속화한다.
- 정상 fake run에서 accepted/public event의 모든 identity와 fingerprint가 chat/profile/capability snapshot에 정확히 결속되고, private reasoning/raw secret은 여전히 남지 않는 회귀를 추가했다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A Rust supervisor 안전 리뷰 수정)

- **release 전 실패**: durable actual run/writer lock 뒤 spawn·identity·barrier·release 단계가 실패하면 모두 공통 종료 경로로 들어간다. 생성 process 전체 소멸이 확인되면 run `failed`·chat `needsAttention`·writer exact release를 원자 기록하고, 불확실하면 가능한 누적 identity/evidence와 run/chat/lock poisoned recovery를 남겨 preparing orphan을 금지했다.
- **누적 자손·재시작**: 실행 중 `TreeTracker`가 새 자손 identity를 관측할 때 actual run의 redacted barrier와 private `(pid,start)` 집합을 durable 갱신하고 terminal transaction에도 최종 누적 집합을 다시 반영한다. fake grandchild를 release 뒤 만들고 crash/recovery를 모사해, 살아 있는 자손이 abandon을 막고 전체 소멸 뒤에만 resolution되는 회귀를 추가했다. 관측·영속 evidence 실패는 terminal success가 아닌 recoveryRequired로 접는다.
- **원장 분리**: legacy G5 `recovery.list`·inspect·markResolved는 `source_kind=actualAgent`를 제외하고, actual 전용 API만 해당 source/full identity를 읽도록 양방향 경계를 고정했다. 공용 accepted/public identity wire commit이 직후 동기화되어 정상 start의 전체 검증은 별도 adaptation commit에서 새 필드로 맞춘 뒤 누적 실행한다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 공용 wire 결속 리뷰 수정)

- **accepted 결속**: `AgentRunAcceptedV1`에 chat의 task/project/worktree/workspace identity와 capability fingerprint를 추가했다. 시작 요청·채팅에서 만든 exact context와 accepted 응답을 비교하는 TypeScript/Rust helper와 공용 golden을 두어 chat/task/project/worktree/workspace/role/fingerprint 변조를 모두 거부한다.
- **공개 event 결속**: normalized `AgentPublicEventV1`도 actual run의 full identity와 capability fingerprint를 포함한다. event는 `status/question/final`과 bounded text만 허용하고 reasoning/raw/tool/stderr/debug/env/auth/prompt/token extra를 거부하며, 공용 golden이 chat/task/project/worktree/workspace/run/role/fingerprint 교차 결속을 양언어에서 확인한다.
- **G5/V4 호환 결정**: wire method나 현재 G5 capability 광고는 늘리지 않았다. V4 `getChat`은 기존 G5 detail과 `agentRuns`가 있는 planned G5A detail의 presence-discriminated union을 반환하므로 renderer는 `agentRuns` 존재를 좁힌 뒤에만 actual ledger를 읽는다.
- **검증·경계**: focused G5A Node 7개, 전체 Node, Rust protocol 25개, TypeScript no-emit, production Vite build, scoped clippy와 `git diff --check`가 통과했다. worktree에 의존성이 없어 일반 `pnpm build`는 격리 네트워크 fetch에서 막혔지만 루트의 고정 설치로 동일 `tsc && vite build`는 exit 0이었다. 실제 CLI·인증·model·사용자 프로젝트는 접근하지 않았다. 다음은 이 원자 commit의 독립 재리뷰와 Electron 소비자의 full-identity route 구현이다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A Electron envelope runId 차단 보정)

- **리뷰 수정**: planned G5A `chat.eventAppended`에서 public `TeamMessage.runId`뿐 아니라 protocol envelope의 optional `runId`도 검사한다. 둘 중 하나가 `agent_run_` namespace면 listener를 호출하기 전에 session을 poison한다. 둘 다 non-null인데 서로 다르면 또한 fail-closed로 identity mismatch를 보고한다. current G5에는 이 비교를 적용하지 않아 기존 fixture 채팅 의미는 바꾸지 않는다.
- **회귀·경계**: fake transcript로 payload carrier와 envelope carrier(메시지 `runId=null` status) 각각이 listener 0회·termination 1회를 보장하고, non-agent envelope/payload runId 불일치도 공개 전에 막는 것을 확인했다. normalized actual-agent public-event wire는 여전히 sidecar event/IPC로 negotiated되지 않았으므로 추가하지 않았고 실제 CLI·인증·model·사용자 프로젝트는 호출하지 않았다.
- **검증·다음**: TypeScript `tsc --noEmit`, Electron bridge/scaffold 37개, 전체 Node 136개, Vite production build와 `git diff --check`를 통과했다. 다음은 이 별도 surgical commit의 재리뷰다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A Electron accepted binding 소비 보정)

- **수정**: approved Bridge V4 contract의 `AgentRunAcceptedV1` full identity를 Electron consumer에 적용했다. planned G5A에서 `chat.get`으로 검증해 보관한 현재 chat의 task/project/worktree/workspace identity, 시작 요청의 chat/role, 요청 capability fingerprint와 accepted response를 exact 비교한다. 어느 한 필드라도 변조되면 session을 poison하며, 정상 accepted binding은 이후 normalized public-event route가 계약될 때만 사용할 private registry에 보관한다.
- **공개 경계·호환**: 아직 sidecar protocol event/IPC 이름으로 negotiated되지 않은 normalized `AgentPublicEventV1` route는 임의로 추가하지 않았다. `agent_run_` raw chat event 및 raw run event의 listener 비노출/poison 경계를 유지했다. V4 `getChat`은 current G5의 `agentRuns` 없는 detail과 planned G5A ledger detail의 union을 반환하며, current G5가 actual ledger를 전제하지 않는 회귀를 추가했다.
- **검증·다음**: fake transcript로 chat/task/project/worktree/workspace/role/fingerprint 각각의 accepted response 변조와 duplicate runId를 모두 fail-closed 확인했다. `tsc --noEmit`, Electron bridge/scaffold 37개를 통과했고 실제 CLI·인증·model·사용자 프로젝트는 호출하지 않았다. 다음은 이 Electron adaptation commit의 독립 재리뷰이며, normalized public-event wire는 protocol/sidecar/IPC 계약이 함께 준비된 별도 작업에서만 열 수 있다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A Electron raw chat event 리뷰 수정)

- **수정**: planned G5A 협상 상태에서 `chat.eventAppended`의 검증된 `TeamMessage`라도 `runId`가 `agent_run_` namespace면 listener 호출 전에 session을 poison하도록 보강했다. 아직 normalized actual-agent public event wire가 없으므로 raw stdout/reasoning text가 status/chat timeline으로 우회하는 길은 허용하지 않는다. 기존 raw `run.event`/`run.finished` poison도 유지한다.
- **G5 호환**: current G5 ready handshake의 `chat.get`은 `agentRuns` 없이 기존 detail validator로 계속 통과하는 회귀를 추가했다. V4 bridge getChat의 최종 TypeScript exact shape와 start accepted full identity/fingerprint는 공용 contract owner의 후속 fix를 기다리며 이 커밋에서 임의로 바꾸지 않는다.
- **검증·경계**: root 의존성으로 TypeScript `tsc --noEmit`, Electron bridge/scaffold Node 35개, `git diff --check`를 통과했다. fake G5A sidecar가 `agent_run_` role message에 raw stdout/reasoning text를 넣어도 renderer listener 0회와 client corruption을 확인했다. 실제 CLI/auth/model/user project는 호출하지 않았다.

### 2026-08-27 · by GPT-5 Codex (M5 G5A Electron V4 소비 경계)

- **한 일**: additive V4의 다섯 actual-agent IPC channel(capability 조회, 시작, cancel, recovery inspect/resolve)을 main-frame/origin 검사가 있는 고정 handler와 좁은 preload bridge에 연결했다. renderer 요청은 계약 validator를 preload와 `SidecarClientV1` 양쪽에서 exact 검사하며 executable path·argv·model·env·cwd·prompt·user config·추가 경로가 포함되면 sidecar IPC 전 거부한다. raw `ipcRenderer`나 generic invoke는 노출하지 않았고 context isolation·sandbox·기존 G5 V3 표면은 유지했다.
- **협상·응답 결정**: 현 G5 exact ready set에서는 V4 호출이 `agentAdapterUnavailable`로 끝나며 sidecar에 request를 보내지 않는다. planned G5A exact capability set이 협상된 경우만 V4 request를 허용하고 capability fingerprint/runtime, start chat/role, actual cancel/recovery의 full chat/task/project/worktree/workspace/run identity, V4 `agentRuns` 별도 ledger를 contract validator로 fail-closed 확인한다. fixture `runs`와 actual `agentRuns`는 섞지 않는다.
- **공개 이벤트 경계**: 아직 normalized actual-agent public-event IPC가 계약되어 있지 않으므로 G5A 중 raw `run.event`/`run.finished`를 받으면 session을 poison한다. 따라서 reasoning·raw stdout/stderr·tool args/output·debug·env·auth·prompt·token·evidence body는 renderer로 전달되지 않는다. status/question/final의 normalized event route는 Rust adapter와 renderer 소비자가 추가될 다음 계약/구현 단위에서 함께 열어야 한다.
- **검증·경계**: root 의존성을 재사용한 TypeScript `tsc --noEmit`, Electron bridge/scaffold Node 34개, 전체 Node 132개, direct Vite production build, `git diff --check`를 통과했다. 현 fixture G5 handshake에서 V4 무전송, launch extra pre-IPC 거부, exact G5A handshake, cross-identity response poison, actual-vs-fixture ledger 분리, raw event non-exposure, cancel/recovery namespace를 fake transcript만으로 검증했다. worktree에 자체 `node_modules`가 없어 `pnpm build`는 격리된 네트워크에서 의존성 fetch를 시도하다 중단됐으며, package/lockfile 변경은 없다. 실제 sidecar adapter, Claude/Codex CLI·인증·model 조회·사용자 프로젝트·유료 모델은 호출하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A Rust fake adapter 안전 흐름)

- **안전 코어·capability**: renderer가 executable·argv·cwd·env·prompt를 넘길 수 없는 Rust 내부 `LaunchSpec`을 추가했다. 절대 executable/관리 snapshot cwd, token argv, 최소 env, bounded prompt와 handshake/parser를 검증하고 prompt를 `Debug`·영속 digest에서 제외한다. product provider는 설치·catalog 검증이 없으므로 `canStart=false`이고 spawn 경로가 닫히며, exact fake manifest만 별도 fixture stdio 모드에서 실행된다.
- **barrier·영속화**: SQLite v1 schema/table을 바꾸지 않고 기존 `runs` 원장에 `source_kind=actualAgent` discriminator와 `agent_run_` record를 넣어 fixture run과 조회·복구 의미를 분리했다. run+writer lock을 먼저 durable commit한 뒤 control launcher가 대기 child를 만들고, launcher/agent `(pid,start identity)`·누적 `TreeTracker`·redacted barrier를 다시 영속화한 후에만 release한다. release 전 child가 자손을 만들지 않는 실제 process test와 clean/cancel/uncertain parent-child 종료를 통과했다.
- **공개 이벤트·복구**: fake vendor의 `status/question/final`만 bounded public event와 기존 chat timeline message로 원자 영속화하고 reasoning/tool/usage·raw JSON·stderr·prompt/token/env/auth는 DB/IPC/log에 남기지 않는다. malformed/oversize는 fail-closed 종료하며, clean 종료는 모든 추적 신원 소멸 뒤 read-only `none`/writer `released`, 불확실 종료와 unclean restart는 run/chat/lock을 poisoned `recoveryRequired`로 보존한다. 실제 프로세스 소멸을 다시 관측한 명시적 `abandon`만 lock을 해제한다.
- **검증·잔여 경계**: core 68, protocol 25, sidecar 57개(1 ignored) 회귀와 exact fake/unknown·fingerprint drift preflight spawn0-lock0, ledger 분리, 공개 secret 부재, writer clean/uncertain, launcher no-descendant 테스트를 통과했다. strict clippy는 owned code가 통과했지만 전체 core에는 기존 `proctree.rs`의 두 lint가 남아 허용 후 재실행했다. 실제 Claude/Codex·auth/model/user project·Windows는 실행하지 않았고 current G5 ready capability도 그대로다. 공용 계약의 accepted/public event full identity wire 보강이 별도 진행 중이므로 통합 때 해당 새 필드를 이 소비자에 맞춰야 한다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 계약 통합·누적 게이트)

- **통합**: actual-agent adapter 계약 기능·리뷰 수정 세 커밋을 독립 최종 `APPROVE` 뒤 `m5/coding-agent-ide`에 `9008b92`·`aaf614f`·`7903d75`로 순서대로 통합했다. 기존 G5 handshake와 fake-only V3는 그대로이며, planned V4 actual-agent 메서드는 protocol 예약·계약일 뿐 sidecar/Electron이 아직 광고하거나 소비하지 않는다.
- **독립 리뷰 결과**: 최초 리뷰의 process 소멸 증거·capability fingerprint 결속·actual-run 취소/복구 wire·role-stage 결속과 재리뷰의 writer-lock 종료 결속·JSON member-order 양언어 parity 지적을 모두 공용 golden 회귀로 닫았다. 최종 재리뷰는 누적 head `dd92e1f`를 `APPROVE`했고 실제 CLI·인증·model·사용자 프로젝트 호출이 없음을 확인했다.
- **통합 검증**: 통합 HEAD에서 focused G5A Node 6개, Rust protocol 25개, 전체 Node 127개와 production build가 통과했다. G5 누적 하네스도 macOS Electron package/Chromium Monaco, Tauri·core·protocol·sidecar, 실제 sidecar stdio fixture와 clean/uncertain 취소·복구를 포함해 PASS했고 증거는 `.artifacts/m5-harness/g5-run-recovery-2026-08-27T02-04-16-861Z.json`이다. 최초 sandbox 실행은 Electron artifact 네트워크 fetch가 막혀 실패했으나 네트워크 허용 재실행은 전 단계 exit 0이었다.
- **다음 할 일·경계**: 별도 worktree에서 Rust fake adapter/supervisor와 Electron V4 소비자를 구현·독립 리뷰한 뒤 UI와 G5A fixture QA를 연결한다. 실제 CLI capability/auth smoke, 유료 모델, 사용자 프로젝트, Windows 자손 회수는 계속 별도 승인·후속 게이트이며 G6 release는 의도대로 `NOT_READY`다. `main` 병합과 push는 하지 않았다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 계약 독립 리뷰 2차 수정)

- **writer lock 결속**: actual-agent terminal run과 termination transcript를 함께 검증하는 공용 persistence 계약을 추가하고 cancel result도 반드시 이를 통과하게 했다. clean exit에서 `workspaceWrite` profile은 `writerLockState=released`, `readOnly` profile은 `none`만 허용한다. recoveryRequired는 기존처럼 lock/chat/process가 모두 poisoned `recoveryRequired`여야 하며 preflight failure의 zero run/spawn/lock 의미는 바꾸지 않았다.
- **process parity**: cancel termination과 recovery가 보존하는 process evidence를 TypeScript의 JSON 문자열 순서가 아닌 `pid/startIdentityDigest/executablePathDigest/observedGone` 구조 값으로 비교해 Rust struct equality와 맞췄다. 같은 process object의 JSON member 순서만 다른 공용 valid fixture를 두 언어가 함께 승인하며, writer clean cancel·정상 성공·실행 실패의 잘못된 lock state는 공용 invalid fixture가 거부한다.
- **검증·경계**: focused Node 6개, focused Rust 3개, 전체 Node 127개와 protocol 25개, renderer production build 및 `git diff --check`가 통과했다. 실제 CLI·인증·model 조회·사용자 프로젝트는 접근하지 않았다. 다음은 별도 2차 fix commit을 독립 재리뷰하는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 계약 독립 리뷰 수정)

- **리뷰 수정**: 독립 리뷰의 네 차단 사항을 TypeScript/Rust 공용 계약과 같은 golden fixture로 고쳤다. clean exit는 모든 `(pid,startIdentityDigest)`가 중복 없이 `observedGone=true`여야 하며, actual run도 G5와 같은 `planning→planning`, `coding→implementing`, `review→verifying` 역할-stage 결속을 지킨다.
- **fingerprint 결정**: capability fingerprint는 top-level `fingerprint`와 관찰 시각 `observedAt`을 제외한 snapshot을 재귀 ASCII key 순서의 compact JSON으로 직렬화한다. 배열 순서는 보존하고 explicit null은 포함한 UTF-8 bytes의 SHA-256에 실제 결속한다. renderer/Vite 계약에는 Node 전용 모듈을 넣지 않고 기존 순수 TypeScript SHA-256을 사용하며, Node crypto는 테스트 oracle로만 비교한다. 정확한 snapshot이 바뀌었는데 이전 fingerprint를 재사용하면 양 언어가 거부한다.
- **V4 wire 경계**: 기존 G5 handshake·fixture-only V3를 바꾸지 않고 planned V4에 actual-agent 전용 `run.cancelAgent`, `recovery.inspectAgent`, `recovery.resolveAgent`만 예약했다. actual run/recovery는 `agent_run_`·`agent_recovery_` namespace와 별도 `agentRuns` ledger를 사용하고 chat/task/project/worktree/workspace identity를 exact 결속한다. clean cancel과 poisoned recovery/명시적 abandon resolution은 typed response와 validator만 정의했으며 Electron·sidecar 소비자는 아직 구현·광고하지 않는다.
- **검증·경계**: 공용 golden의 clean-exit process proof·중복 identity·fingerprint 재사용·cancel/recovery/chat identity·역할-stage 회귀를 양 언어가 함께 읽는다. focused Node 6개, 전체 Node 127개, focused Rust 3개·전체 protocol 25개, renderer production build와 `git diff --check`가 통과했다. 실제 Claude Code/Codex 실행·인증·model 조회·사용자 프로젝트 접근은 하지 않았다. 다음은 이 fix commit을 같은 독립 리뷰에 다시 제출하는 것이다.

### 2026-08-27 · by GPT-5.6-Sol (M5 G5A 실제 agent adapter 계약)

- **한 일**: 현재 G5 handshake와 `run.startFixture`의 fake-only 의미를 바꾸지 않고, protocol v1에 `agent.getCapabilities`·`run.startAgent`를 예약하고 별도 planned `M5BridgeV4`와 TypeScript/Rust 공용 agent adapter v1 계약·golden fixture를 추가했다. 실제 run은 `fixtureScenario`가 있는 G5 fixture run과 섞지 않고 V4 chat detail의 별도 `agentRuns` ledger로 정의했다. renderer start 요청은 `chatId/roleId/inputMessageIds/expectedCapabilityFingerprint`만 허용하며 실행 경로·argv·model·env·cwd·prompt는 서버가 채팅의 프로젝트별 역할 snapshot에서 해석한다.
- **안전 결정**: 실제 실행은 machine-readable runtime 검증 또는 정확한 CLI 버전에서 실측한 manifest만 허용한다. effective model 증거가 `argvOnly`이거나 catalog가 불명확하면 `canStart=false`이며 silent fallback은 없다. v1은 one-shot과 `nextRun` follow-up만 허용하고 active stdin/resume은 금지한다. launcher와 agent identity·process tree를 run/lock과 함께 release 전에 영속화하는 barrier, preflight `spawnCount=0/lockCount=0`, 공개 `status/question/final`만, redacted bounded evidence, 불확실 종료의 poisoned `recoveryRequired`를 양 언어 validator로 고정했다.
- **검증·경계**: Node 전체 126개, TypeScript/Vite build, Rust protocol 25개, scoped clippy와 `git diff --check`를 통과했다. 실제 Claude Code/Codex 실행·인증·model 조회·사용자 프로젝트 접근은 하지 않았다. 다음은 이 계약 commit의 독립 리뷰 후 Rust adapter와 Electron/UI 소비자를 별도 worktree에서 구현하고, 통합 fixture QA를 거치는 것이다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 최종 게이트)

- **판정**: 구현·문서 통합 commit `33f16a009065813bc575b1f528756f661c2ccdeb`을 독립 최종 게이트가 `PASS — APPROVE / G5_READY`로 승인했다. 계약, Rust, Electron, UI, 하네스와 문서의 독립 승인 commit이 모두 해당 통합 commit의 조상임을 확인했다.
- **최종 증거**: exact-head 누적 하네스 `.artifacts/m5-harness/g5-run-recovery-2026-08-26T04-01-24-084Z.json`은 clean 시작과 baseline~G5 명령 12개 전부 exit 0을 기록한다. 독립 QA `.artifacts/m5-qa/g5-integration-014bd987464d39674a5f531765051f60e63b5ecb-2026-08-26T03-45-43Z.json` 이후 변경은 독립 승인된 문서뿐이다.
- **경계·다음 할 일**: G6는 security/package/performance script가 없어 의도대로 `NOT_READY`다. 실제 Claude Code/Codex adapter·인증 smoke, Windows snapshot/process-tree, 전체 OS Quit E2E, Forge prerelease와 macOS 권한 강화는 G5 완료에 포함하지 않았고 후속 게이트로 남긴다. `main` 병합과 push는 하지 않았다.

### 2026-08-26 · by GPT-5.6-Sol (M5 문서 생애주기 경계 재정렬)

- **수정**: 독립 문서 리뷰 지적에 따라 `m5-coding-agent-ide.md`에서 G1~G6 현재 상태표, exact 검증 판정과 구현 완료 서술을 제거하고 제품 방향·설계 계약·계획된 완료 기준·열린 결정만 남겼다. 현행 G5 구조와 게이트 사실은 `architecture.md`와 이 워크로그만 소유한다.
- **색인·규칙**: `docs/README.md`와 byte-identical `AGENTS.md`/`CLAUDE.md`도 제안서를 현재 상태 출처로 안내하지 않도록 고쳤다. 이전 역사적 검토·결정 기록과 2026-08-25 아카이브 내용은 바꾸지 않았다.
- **다음 할 일**: 수정 commit을 독립 재검토한 뒤 G5 최종 게이트에서 문서 생애주기 경계를 확인한다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 누적 하네스·독립 QA와 현행 문서 반영)

- **판정**: exact 통합 commit `014bd987464d39674a5f531765051f60e63b5ecb`의 G5 누적 하네스가 PASS했고 독립 QA도 `G5_READY`로 판정했다. 하네스는 `.artifacts/m5-harness/g5-run-recovery-2026-08-26T03-40-31-061Z.json`, QA는 `.artifacts/m5-qa/g5-integration-014bd987464d39674a5f531765051f60e63b5ecb-2026-08-26T03-45-43Z.json`에 있다.
- **현행 반영**: 프로젝트 역할 설정 CAS와 채팅별 불변 role snapshot, 여러 영속 채팅, 항상 새로 만드는 채팅별 standalone snapshot과 exact prepare/bind/abandon, V3 네 ID workspace/save, 공개 팀 타임라인, fixture 실행의 clean/uncertain 취소·프로세스 트리 복구·poisoned lock·prepareQuit blocker, Electron/preload/UI 경계를 `architecture.md`와 M5 제안서·색인·루트 작업 규칙에 반영했다.
- **검증 범위**: 독립 QA는 계약/UI 22개, 임시 Git 저장소를 쓴 sidecar stdio 7개, Rust 회귀 20개, Electron 29개, Chromium Monaco와 macOS arm64 package smoke를 통과했다. 실제 Claude Code/Codex·인증·사용자 프로젝트와 Windows는 실행하지 않았다.
- **다음 할 일**: 문서 독립 검토와 최종 게이트 뒤 실제 agent adapter 계약, G6 보안·패키징·성능, Windows 안전 게이트를 순서대로 진행한다. push와 `main` 병합은 하지 않는다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 하네스 연결)

- **한 일**: `m5:test:run-recovery`를 추가해 G5 계약, chat-bound workspace preparation/addressing, Electron bridge/scaffold, renderer chat/editor 상태와 Rust sidecar unit·실제 stdio process 검증을 한 명령으로 연결했다. 기존 누적 게이트가 같은 현재 코드의 TypeScript/renderer build·Chromium smoke·Electron package smoke를 이미 실행하므로 G5 명령은 고유 검증을 중복 없이 조합한다. Chromium smoke의 가짜 preload bridge도 V3로 올리고 workspace/chat 구독과 프로젝트 역할·빈 채팅 조회를 제공해 실제 renderer mount 흐름과 맞췄다.
- **하네스 결정**: `pnpm harness:list`에서 G5는 `READY`, 아직 구현되지 않은 G6는 `NOT_READY`로 표시한다. 하네스 자기 검사는 G5 누적 실행 중 재귀하지 않도록 다음 미구현 G6의 exit 2와 세 누락 script를 검증한다.
- **검증**: 의존성 설치를 선행하지 않는 동등한 `CI=true npm run m5:test:run-recovery`에서 Node 51개, sidecar unit 51개 통과·1개 의도한 ignored, 실제 stdio 7개가 통과했다. root의 고정 Vite binary로 renderer 605 module build를 통과한 뒤 승인된 Electron 실행에서 수정된 Chromium Monaco mount/worker smoke 1개도 통과했다. `node scripts/m5-harness.mjs --list`는 G5 `READY`·G6 `NOT_READY`, focused 하네스 자기 테스트 2개와 staged/unstaged `git diff --check`도 통과했다. 실제 유료 agent·사용자 프로젝트·Windows·파괴적 시험은 실행하지 않았다.
- **다음 할 일**: 최신 통합 commit에서 G5 누적 하네스와 package smoke를 실행하고 독립 QA한다. G6 security/package/performance script는 해당 구현 파동에서 별도로 추가한다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 채팅 준비 workspace Rust 소비)

- **한 일**: 승인된 additive `workspace.prepareChat`·`workspace.abandonPrepared`와 `preparationId`가 추가된 `chat.create` 계약을 Rust sidecar에 소비했다. prepare는 기존 active/bound snapshot과 무관하게 새 standalone snapshot과 전역 고유 preparation/worktree/workspace identity를 만들고 `unbound`로 저장한다. create는 exact preparation·active manifest·settings revision·전역 chat/workspace binding을 SQLite `BEGIN IMMEDIATE` 안에서 다시 확인한 뒤 preparation `bound(chatId)`·draft chat·최초 사용자 메시지를 함께 commit한다. G4 `project.createWorktree`의 기존 idempotent 경로는 유지했다.
- **안전 결정**: exact 네 ID가 다른 abandon, bound·unknown·이미 abandoned preparation은 다른 workspace를 바꾸지 않고 거부한다. 성공 abandon은 물리 삭제 없이 manifest와 preparation을 terminal `abandoned`로 만들며, filesystem 전이를 먼저 수행해 이후 SQLite 저장이 실패해도 chat binding이나 G4 작업에 재사용되지 않게 했다. 저장 전후 실패를 위한 exact 재개는 같은 abandoned filesystem identity만 허용한다. prepare 저장 실패도 새 snapshot을 terminal 처리하고, terminal 상태를 증명하지 못하면 outcome-unknown recovery로 승격한다. chat ID 충돌과 binding 도중 crash 주입은 transaction 전체를 rollback해 preparation을 cleanup 가능한 `unbound`로 유지한다.
- **호환·발견**: G5 workspace 작업은 chat/project/worktree/workspace 네 ID와 외부 manifest·marker·lock을 계속 매번 검증한다. 실제 stdio A/B 격리 시험 중 root tree의 유효한 빈 `parentPath`를 공용 파서가 거부하던 오류를 발견해 G4/G5 tree route에서만 빈 문자열을 허용했다. bound A를 유지하면서 fresh B 생성 실패·mismatch 거부·exact/duplicate abandon 뒤 A tree/read/save가 계속 동작하고 원본 Git 저장소는 변하지 않음을 임시 저장소로 검증했다.
- **검증**: `cargo test -p hello-agent-sidecar --locked`는 unit 51 통과·의도한 1 ignored와 실제 stdio 7 통과, `cargo test -p hello-agent-protocol --locked` 22 통과, M4 process-tree F1~F8 회귀 17 통과, Tauri handoff 53 통과·실제 유료 agent 3 ignored를 확인했다. `cargo clippy -p hello-agent-sidecar --locked --no-deps -- -D warnings`, owned Rust 2021 rustfmt, `git diff --check`도 통과했다. Windows/비 macOS spawn은 계약대로 차단 경로만 유지했으며 실제 Windows·유료 agent·사용자 프로젝트는 실행하지 않았다.
- **다음 할 일**: 이 Rust 후속 commit을 독립 코드 리뷰하고 최신 통합 commit에서 G5 누적 하네스와 실제 stdio QA를 재실행한다. Electron/UI는 exact preparation 결과를 받은 뒤에만 chat.create를 호출하고 실패한 unbound preparation을 정확한 네 ID로 명시 폐기해야 한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅 workspace preparation Electron 소비자)

- **한 일**: 새 `workspace.prepareChat`/`workspace.abandonPrepared` capability를 고정 V3 IPC channel, preload exact validator, main handler, SidecarClient response validator에 연결했다. `createChat`은 이제 `preparationId`와 project/worktree/workspace 세 필드를 모두 포함하고, 현재 client가 정확히 알고 있는 unbound preparation에만 결합한다.
- **안전 결정**: prepare 응답은 새 unbound standalone snapshot 전체 레코드여야 하며 project ID를 exact echo한다. abandon은 준비 ID와 세 workspace ID를 모두 정확히 echo하고 known unbound preparation에만 보낸다. cross-preparation·unknown·late·bound mismatch에는 재시도·projectId close·fallback이 없다. main/window 종료는 명시적 UI 요청 없이 자동 abandon하지 않아 sidecar의 증거를 보존한다.
- **검증**: root 의존성을 재사용한 `tsc --noEmit`, `node --test --experimental-strip-types tests/m5-electron-bridge.test.mts tests/m5-electron-scaffold.test.mts` 29개, `git diff --check`를 통과했다. extra/project-only·UTF-8 ID overflow, preparation A/B 교차, abandon response mismatch, createChat preparation mismatch를 회귀로 추가했다.
- **다음 할 일**: 이 bridge consumer commit을 독립 재검토하고, Rust sidecar 및 renderer 준비 흐름과 통합해 실제 preparation→chat bind→명시적 abandon QA를 수행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅 ID 전역 충돌 리뷰 수정)

- **한 일**: 독립 리뷰 P1을 반영해 chat 생성 후보의 `chatId`도 완료된 전역 binding scan의 모든 기존 chat ID와 비교하도록 TypeScript/Rust `ChatCreationTranscriptV1` validator를 강화했다. fresh preparation이 별도 worktree/workspace를 사용해도 후보 chat ID가 기존 A와 같으면 `bindingConflict`다.
- **안전 결정**: 충돌한 `createdChat`은 커밋 전 후보 증거로만 transcript에 남을 수 있다. 이 경우 transaction은 반드시 미커밋, 최초 사용자 메시지는 없음, preparation은 exact unbound 상태 그대로여야 하며, 성공 또는 binding/write를 주장하면 fail-closed 거부한다.
- **검증**: 공용 fixture가 기존 chat A + fresh preparation B + 생성 후보 chat ID A를 성공으로 주장하는 경우를 거부하고, 같은 충돌을 no-write/no-bind/cleanup-eligible `bindingConflict`로 판정하는 경우만 승인한다. focused Node 10개, focused Rust 5개, 전체 `npm test` 106개, `cargo test -p hello-agent-protocol --locked` 22개, `tsc --noEmit`, `cargo check -p hello-agent-sidecar --locked`, rustfmt/diff 검사를 통과했다.
- **다음 할 일**: 수정 commit을 재검토한 뒤 통합하고, sidecar의 chat ID 생성·전역 uniqueness 확인·insert를 한 transaction 안에서 수행해 같은 판정을 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅별 독립 작업공간 준비 계약)

- **한 일**: 기존 V2와 workspace-addressed V3 API를 변경하지 않고 `workspace.prepareChat`·`workspace.abandonPrepared`와 bridge 메서드를 additive하게 추가했다. prepare는 같은 프로젝트의 active/bound snapshot 존재 여부와 무관하게 전역적으로 고유한 `preparationId/worktreeId/workspaceIdentity`를 가진 새 standalone snapshot을 `unbound`로 저장하며, chat.create는 그 정확한 준비 레코드를 settings·최초 사용자 메시지와 한 transaction에서 `bound(chatId)`로 바꾼다.
- **안전 결정**: abandon은 preparation/project/worktree/workspace 네 ID와 저장 증거가 정확히 일치하는 아직 unbound인 준비만 `abandoned`로 표시하고 물리 삭제하지 않는다. bound·unknown·이미 abandoned·cross-identity 요청은 다른 준비를 변경하지 않는 no-op이며, settings/binding 실패도 해당 준비를 그대로 cleanup-eligible 상태로 남긴다. `ChatRecordV1`에는 preparationId를 중복 저장하지 않고 persisted preparation의 `boundChatId`를 immutable 생성 증거로 사용한다.
- **검증**: 공통 fixture가 bound A가 있어도 B를 새 identity로 준비하고, B 생성 실패 뒤 B만 폐기하며 A는 active인 흐름, 반복 prepare identity 충돌, cross-project/triple/preparation mismatch, bound/late/unknown/duplicate abandon을 TypeScript와 Rust에서 같은 결과로 판정한다. focused Node 18개, 전체 `npm test` 106개, `cargo test -p hello-agent-protocol --locked` 22개, `tsc --noEmit`, `cargo check -p hello-agent-sidecar --locked`, owned Rust rustfmt와 `git diff --check`를 통과했다.
- **다음 할 일**: 이 계약 commit을 독립 리뷰한 뒤 통합하고, sidecar가 새 snapshot 생성과 준비 레코드 저장, chat/settings/message binding, 정확한 준비 abandon을 각각 fail-closed transaction으로 구현한다.

### 2026-08-26 · by GPT-5.6-Sol (M5 G5 Rust 실행·복구 및 다중 workspace 내부)

- **한 일**: 승인된 G5 run/recovery·workspace 계약을 Rust sidecar에 소비해 프로젝트별 revision CAS 역할 설정과 채팅 생성 시 역할 snapshot, bounded 영속 chat ledger, chat-bound workspace tree/read/save/watch/diff/close를 구현했다. G4 projectId-only 경로는 호환 경로로 유지하고 G5 작업은 저장된 전체 chat binding과 외부 active manifest·marker·lock을 매 요청마다 exact 검증한다. 여러 standalone snapshot과 chat이 동시에 살아 있어도 현재 선택을 추론하지 않으며 cross-chat ref와 saveId continuation은 원본 쓰기 전에 거부한다.
- **실행·복구 결정**: macOS 결정적 self-fixture만 허용하고 실제 Claude/Codex·인증·사용자 프로젝트는 실행하지 않았다. 역할 실행은 chat/workspace별 직렬화하며 coding은 run+writer lock을 spawn 전에 SQLite `BEGIN IMMEDIATE`로 함께 저장하고 planning/review는 writer lock 없이 실행한다. fixture child는 부모가 process identity를 저장하기 전 자손을 만들지 못하도록 stdin release barrier를 두고, 취소는 M4 `TreeTracker`가 관찰한 부모·자손 전체 소멸을 증명한 경우만 clean 처리한다. 불확실하면 chat/run/manifest/writer lock/recovery를 fail-closed로 남기고, 모든 영속 identity가 Gone인 abandon만 허용한다. `prepareQuit`은 active run과 recovery blocker를 정리·보고하며 unsafe 상태에서는 복구 조회/abandon 뒤 재시도할 수 있고, clean checkpoint 전에는 EOF 종료를 성공으로 보지 않는다.
- **저장 안전**: 상수 크기 projection event와 durable event sequence를 wire stream sequence와 분리했다. workspace save begin은 전체 chat/workspace ref를 durable session에 고정하고, commit 결과 뒤 session cleanup 저장이 실패하면 outcome-unknown으로 승격해 자동 재시도를 막는다. 복구 wire payload에서는 내부 마이크로초 process identity 증거를 제거하되 durable projection에는 정확한 probe 값을 보존한다.
- **검증**: 임시 Git 저장소만 사용했다. `cargo test -p hello-agent-sidecar --locked`는 unit 48 통과·의도한 1 ignored와 stdio 6 통과, `cargo test -p hello-agent-protocol --locked`는 21 통과, `cargo clippy -p hello-agent-sidecar --locked --no-deps -- -D warnings`, scoped Rust 2021 format, `git diff --check`를 통과했다. M4 process-tree F1~F8 회귀 17개와 Tauri handoff 53개 통과·실제 유료 agent 3개 ignored도 통과했다.
- **다음 할 일**: 새 chat 전용 preparation 계약 commit을 통합 기준선에 먼저 반영한 뒤 이 branch를 그 exact base로 전진하고, `workspace.prepareChat`·exact `workspace.abandonPrepared`와 preparation을 원자 소비하는 amended `chat.create`를 별도 후속 단위로 구현·검토한다. 이 커밋은 그 미통합 wire를 광고하거나 구현하지 않는다.
### 2026-08-26 · by GPT-5 Codex (M5 G5 Electron Bridge V3 독립 리뷰 수정)

- **한 일**: 독립 리뷰의 차단 사항을 반영했다. recovery blocker가 있는 `ready` status를 유효한 live 상태로 유지해 recovery API 호출을 허용하고, `safeToQuit: false`는 종료 허용으로 바꾸지 않는다.
- **이벤트·ID 결정**: workspace/chat/run protocol event는 generic V2 route에서 제외해 renderer credit가 한 실제 event에 한 번만 소비된다. raw `run.event`·`run.finished` payload는 renderer로 전달하지 않으며, G5 public chat은 검증된 `TeamMessage` 전용 route만 쓴다. bridge-local project/recovery ID 검증도 문자열 길이가 아니라 UTF-8 512 bytes를 적용한다.
- **검증**: root 의존성을 재사용한 `tsc --noEmit`, `node --test --experimental-strip-types tests/m5-electron-bridge.test.mts tests/m5-electron-scaffold.test.mts` 27개, `git diff --check`를 통과했다. recovery-ready 연결성, unsafe quit, four-stream 단일 route, hidden raw run filtering, ASCII/emoji byte-limit pre-IPC 거부를 추가로 검증했다.
- **다음 할 일**: 이 수정 commit을 같은 exact SHA로 재검토하고, G5 Rust sidecar/renderer와 통합한 뒤 package smoke와 누적 QA를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 Electron Bridge V3)

- **한 일**: Electron main/preload와 `SidecarClientV1`이 additive `M5BridgeV3`를 소비하도록 구현했다. workspace tree/close/read/save/watch/diff, 프로젝트 역할 설정, 채팅 페이지, fixture run/cancel, recovery API는 모두 고정 IPC channel·main-frame/app origin 검사·preload exact input validator를 거친다. V2 API와 raw sidecar restart 정책은 유지했다.
- **안전 결정**: G5 ready capability는 canonical 순서를 정확히 비교한다. workspace 응답·event·save continuation은 `chatId/projectId/worktreeId/workspaceIdentity`를 매번 echo하고, begin 뒤 `saveId`를 그 네 ID에 등록한다. cross-chat/unknown/late save와 잘못된 response는 sidecar에 재전송하지 않고 거부한다. 공개 chat stream은 구조화된 `TeamMessage`만 전달하며 workspace/chat event는 별도 bounded-credit stream으로 ACK한다.
- **검증**: root 의존성을 재사용한 `tsc --noEmit`, `node --test --experimental-strip-types tests/m5-electron-bridge.test.mts tests/m5-electron-scaffold.test.mts` 25개, `git diff --check`를 통과했다. G5 package smoke expectation도 canonical fixture로 올렸지만 이 branch에는 G5 Rust sidecar가 아직 없으므로 실제 package smoke는 통합 뒤 실행해야 한다.
- **다음 할 일**: Rust G5 sidecar와 renderer 소비자를 같은 V3 계약으로 통합하고, 최신 통합 commit에서 run/recovery 및 package smoke QA를 실행한다. 실제/유료 agent 실행은 하지 않았다.
### 2026-08-26 · by GPT-5 Codex (M5 G5 renderer bind·폐기 review 수정)

- **한 일**: B chat workspace bind를 tree → diff → watch 순차 준비로 바꿔, 세 단계가 모두 성공하기 전 B editor ref/tree/tabs를 publish하지 않도록 했다. 어느 단계든 실패하면 epoch을 무효화하고 A editor snapshot/ref를 복원한 채 action gate를 유지한다. discarded chat도 recovery와 같은 위험 동작 차단 대상으로 두며, pending switch event는 detail hydration 전 raw buffer에 보관해 detail과 merge할 때만 event sequence를 판정한다.
- **검증**: B tree bind 실패가 A ref/tabs를 보존하고 B diff/watch RPC를 시작하지 않는 focused regression을 추가했다. focused chat/workspace editor Node와 TypeScript `tsc --noEmit`, `git diff --check`를 통과했다.
- **다음 할 일**: independent reviewer가 deferred detail/event hydration과 watch-failure restore 경로를 재확인하고 Electron 통합 뒤 Chromium smoke를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 새 채팅 준비 API renderer 연결)

- **한 일**: 승인된 `workspace.prepareChat` → `chat.create(preparationId, projectId, worktreeId, workspaceIdentity, goal, expectedSettingsRevision)` 계약을 renderer에 연결했다. 매 새 chat은 bridge가 만든 독립 prepared workspace의 exact ID를 쓰며, `chat.create` 실패 시에만 그 준비 작업의 exact four-ID를 `workspace.abandonPrepared`에 전달한다. 기존 V2 `createProjectWorktree`/`project.close`는 새 chat 흐름에서 호출하지 않는다.
- **검증**: focused chat/workspace editor Node 7 PASS에서 A/B의 서로 다른 workspace identity·preparationId, B create 실패의 B-only abandon과 A 지속 사용을 확인했다. TypeScript `tsc --noEmit`과 `git diff --check`도 통과했다.
- **다음 할 일**: 독립 renderer review와 Electron V3 preparation bridge 통합 후 Chromium smoke·G5 누적 harness를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 renderer review 수정)

- **한 일**: 독립 UI review의 P1을 반영했다. chat hydration/bind 동안 `switching`을 먼저 공개하고, 기존 A 화면은 유지한 채 모든 editor/run action을 막으며 B의 detail·timeline은 exact editor bind가 성공한 뒤에만 한 번에 publish한다. hydration 중 수신한 event는 durable `eventSequence`로 merge/dedupe하며 충돌·gap은 fail-closed 오류가 된다. cancel 늦은 결과도 chatId/runId/generation/switching을 재검증해 다른 활성 chat을 바꾸지 않는다. recovery 상태는 editor save/close/abandon과 run/switch를 UI/controller 수준에서 RPC 전 차단한다.
- **새 채팅 결정**: 안전한 새 snapshot 준비 API가 아직 통합되지 않았으므로 renderer는 기존 V2 project snapshot 재사용 또는 `project.close` cleanup을 하지 않고 `preparationApiUnavailable` 안내와 비활성화 버튼을 표시한다.
- **검증**: focused chat/workspace editor Node 5 PASS, TypeScript `tsc --noEmit` PASS. 다음은 deferred switch/cancel/hydration/recovery regression을 보강하고 Electron 통합 뒤 Chromium smoke를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 V3 채팅 작업 복사본 Monaco 포트)

- **한 일**: `ProjectEditorController`가 채팅 binding 뒤 immutable `ChatWorkspaceRefV1 { chatId, projectId, worktreeId, workspaceIdentity }`만 사용하도록 포트했다. tree/read/save begin·append·commit/watch/diff/close와 blob/watch event는 각각의 V3 workspace API로 전환되며, V2 project API는 채팅 생성 전 picker·snapshot 준비에만 남는다. clean chat switch는 새 workspace를 다시 load하고, dirty/conflicted 탭은 switch·새 chat을 막으며, late response/event는 epoch과 exact four-ID 비교를 통과해야만 surface에 반영된다.
- **검증**: V3 editor/chat focused Node 5 PASS, 기존 editor 포함 focused Node 30 PASS, root 설치 TypeScript `tsc --noEmit -p tsconfig.json` PASS, `git diff --check` PASS. Chromium smoke와 worktree `pnpm build`는 로컬 의존성 부재 및 network DNS `ENOTFOUND` 때문에 이 worktree에서 실행하지 못했다.
- **다음 할 일**: Electron preload/main V3 port와 통합해 실제 renderer Chromium smoke·G5 cumulative harness를 실행하고, 독립 UI review에서 save session와 dispose/race 경계를 재확인한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅·역할 설정 renderer)

- **한 일**: V3 bridge만 소비하는 renderer 채팅 상태 컨트롤러와 작업대 UI를 추가했다. 프로젝트별 기획·코딩·검토 역할의 runtime/model/reasoning/CAS 설정, 기존 채팅의 불변 role snapshot 안내, 영속 채팅 목록·전환, 새 채팅마다 별도 standalone snapshot triple을 만든 뒤 exact revision으로 `chat.create` 하는 흐름, 공개 팀 타임라인과 명시적 `가짜 에이전트 데모` 시작·취소 UI를 구현했다. 교차 채팅 triple event는 renderer에서 다시 거부하고, recoverable 상태는 실행과 위험 동작을 막고 명시적 abandon confirmation만 보인다.
- **중요한 경계**: 이 UI는 실제 모델을 설치·로그인·실행하거나 chain-of-thought/raw stderr를 표시하지 않는다. 모델을 사용할 수 없을 때는 자동 fallback하지 않으며, 런타임 V3 workspace-addressed 파일 저장/닫기 연결은 Electron bridge 포트와 함께 완성해야 한다.
- **검증**: `node --test --experimental-strip-types tests/m5-g5-chat-ui-state.test.mts` 3 PASS, 루트 설치 TypeScript를 이용한 `tsc --noEmit -p tsconfig.json` PASS, `git diff --check` PASS. worktree에 `node_modules`가 없어 `pnpm build`는 network DNS `ENOTFOUND` 의존성 다운로드에서 중단됐다.
- **다음 할 일**: 독립 UI 리뷰에서 V3-only action·recovery/dirty-switch 경계를 확인하고, Electron V3 bridge와 workspace-addressed Monaco controller를 통합한 뒤 renderer Chromium smoke와 G5 누적 harness를 실행한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 protocol envelope 호환·binding 증거 강화)

- **한 일**: 최종 리뷰 P1에 따라 protocol v1 `EventV1`의 Rust/TypeScript 상위 필드를 기존 `chatId`·`taskId`·`runId`까지만 유지하고, G5 workspace event의 `ChatWorkspaceRefV1` 네 ID는 전부 event `payload` 안으로 이동했다. canonical JSONL이 payload 주소를 byte-match하고, 기존 G4 sidecar의 `EventV1` 초기화 코드를 수정하지 않은 채 소비자 빌드 호환을 증명한다.
- **binding 결정**: `WorkspaceResolvedBindingV1`의 요청 유래 `storedBinding` echo를 제거하고, run/recovery validator를 통과한 전체 persisted `ChatRecordV1`과 verified active standalone snapshot을 증거로 요구한다. 모든 operation/save/event route는 저장 chat의 `chatId/projectId/worktreeId/workspaceIdentity`와 요청·snapshot을 exact 비교하며, 다른 저장 chat을 가짜 binding으로 제시하는 공통 TS/Rust fixture를 거부한다.
- **검증**: focused G5/workspace/protocol Node 14개, 전체 `npm test` 102개, `cargo test -p hello-agent-protocol --locked` 21개, `tsc --noEmit`, owned Rust rustfmt/diff 검사를 통과했다. 또한 `cargo check -p hello-agent-sidecar --locked`와 `cargo test -p hello-agent-sidecar --locked`(unit 43 통과·1 ignored, stdio 3 통과)로 기존 소비자 호환을 확인했다.
- **다음 할 일**: 수정 exact SHA를 독립 재검토한 뒤 통합하고, 이후 sidecar G5 소비자는 DB에서 읽은 immutable chat record와 active manifest를 workspace 작업 시작 전에 결합해 같은 validator 순서를 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 workspace chat/save 상관관계 리뷰 수정)

- **한 일**: 독립 리뷰 P1 두 건을 반영해 V3 workspace 요청·응답·descriptor·event의 주소를 `ChatWorkspaceRefV1 { chatId, projectId, worktreeId, workspaceIdentity }`로 강화했다. 각 tree/close/read/watch/diff 작업은 sidecar에 저장된 chat binding과 verified active standalone snapshot을 정확히 resolve한 뒤에만 허용하며, createChat으로 고정된 chat/workspace 결합과 같은 네 ID를 사용한다. V2 메서드·타입은 변경하지 않았다.
- **save 결정**: begin 시 persisted save session에 네 ID와 `saveId`·path·expected version·byte/chunk 조건을 함께 고정한다. append와 commit 요청·응답도 네 ID를 반복하고 `saveId`로 찾은 정확한 persisted session과 일치해야 한다. `saveId`만 있는 V3 continuation은 허용하지 않으며 unknown/종료된 session의 늦은 결과도 fail-closed다.
- **검증**: 공통 fixture가 chat A + workspace B triple을 tree/close/read/watch/diff에서 거부하고, 동시 save A/B에서 A `saveId` + B ref append·commit, unknown/late response·event를 양언어에서 같은 결과로 거부한다. focused Node 14개, 전체 `npm test` 102개, `cargo test -p hello-agent-protocol` 21개, `tsc --noEmit`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다.
- **다음 할 일**: 수정 commit을 동일 exact SHA로 독립 재검토한 뒤 통합하며, sidecar/Electron 소비자는 stored chat binding·active snapshot·persisted save session 조회를 IPC 처리 전에 같은 순서로 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 workspace 명시 주소 계약)

- **한 일**: 기존 projectId-only `M5BridgeV2`를 변경하지 않고 `M5BridgeV3`에 workspace 전용 tree/close/file read·save·watch/diff/event API를 별도 추가했다. 모든 요청과 상관관계가 있는 응답·descriptor·event는 `WorkspaceRefV1 { projectId, worktreeId, workspaceIdentity }`를 명시적으로 전달하고 exact echo하며, save chunk·commit은 begin에서 서버에 결합된 `saveId`로만 이어지도록 TypeScript/Rust validator를 고정했다.
- **호환·안전 결정**: protocol framing/schema는 v1을 유지하고 G5 capability/event allowlist만 additive 확장했다. 기존 V2 메서드 순서·타입과 canonical project.getTree JSONL bytes는 공통 fixture에서 그대로 보존한다. chat·verified active standalone snapshot·workspace API와 event route는 같은 triple이어야 하며 cross-chat worktree, workspace alias, 잘못된 response/path/event/save 상관관계를 fail-closed 거부한다. 선택된 프로젝트·채팅 같은 암시적 전역 상태는 V3 계약에 두지 않는다.
- **검증**: 공통 `g5-workspace-v1` fixture를 TypeScript와 Rust가 같은 결과로 판정한다. focused Node 4개, 전체 `npm test` 102개, `cargo test -p hello-agent-protocol` 21개, root 설치를 재사용한 `tsc --noEmit`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다. 실제 sidecar/Electron/UI 소비자와 사용자 프로젝트·유료 에이전트는 실행하지 않았다.
- **다음 할 일**: 이 계약 commit을 독립 리뷰한 뒤 통합하고, sidecar와 Electron/preload 구현이 모든 workspace 응답·stream·event에서 같은 triple/saveId 상관관계를 검증하도록 소비한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 채팅 상태·단일 run 직렬화 계약)

- **한 일**: 마지막 독립 재검토 P1을 반영해 `stopping`을 포함한 명시적 `ChatTransitionV1`을 Rust/TypeScript 양쪽에 추가했다. 전이는 identity·goal·settings revision·role snapshot·createdAt을 보존하고 updatedAt이 단조 증가하며, 완료·취소·폐기·복구 필요 상태는 일반 전이에서 terminal이다. 취소 transcript도 같은 전이 계약을 통과해야 하며 복구 abandon만 전용 transcript로 terminal chat/workspace를 바꾼다.
- **실행 결정**: G5 run 시작 source chat은 `draft|running|waitingForUser|needsAttention`만 허용한다. 그 외 상태는 `chatStateBlocked`, 동일 chat/worktree/workspace의 `preparing|running|cancelling` run이 있으면 역할과 writer 여부에 관계없이 `runActive`이며 persist·lock·spawn 전에 거부한다. 판정 우선순위는 platform → abandoned workspace → recovery → chat state → active run → profile → writer lock → started로 manifest에 고정했다.
- **검증**: 공통 fixture가 completed/canceled/discarded/reviewReady 시작 거부, active read-only와 active writer 직렬화, terminal 이전 run 뒤 needsAttention 재시도, stopping을 거친 clean cancel, 불변 identity/settings와 timestamp 역행 거부를 양언어에서 같은 결과로 판정한다. focused G5 Node 6개, 전체 `npm test` 98개, Rust protocol 19개, `npx tsc --noEmit`, owned Rust rustfmt check와 `git diff --check`를 통과했다.
- **다음 할 일**: 이 상태 계약 수정 commit을 독립 재검토한 뒤 통합하고, sidecar가 start transaction에서 같은 우선순위와 단일 active run 검사를 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 계약 독립 리뷰 수정)

- **한 일**: G5 독립 리뷰의 P1 4건·P2 1건을 반영해 chat 생성의 verified active standalone snapshot과 전역 1:1 binding scan, 일반 run 시작의 platform·exact profile resolution·recovery 차단, cancel/termination/recovery 원자 transcript, abandon 뒤 terminal workspace/chat 상태를 Rust와 TypeScript 계약 및 공통 fixture에 추가했다. `planning`·`review` read-only fixture 실행은 writer lock 없이 허용하고 coding writer만 worktree ID와 workspace identity를 함께 잠근다.
- **안전 결정**: Windows fixture 실행은 persist·lock·spawn 전에 거부하며 이 게이트의 결정적 fake 실행은 macOS만 허용한다. unavailable profile·default fallback·chat snapshot drift는 어떤 실행 기록도 만들지 않는다. 불확실한 취소는 chat/run/workspace lock/unresolved recovery를 함께 `recoveryRequired`로 남기고 이후 읽기/쓰기 역할의 모든 run을 막는다. 모든 process identity 소멸을 증명한 abandon 뒤 기존 snapshot/workspace/chat은 재사용할 수 없다. G5 ID 상한은 양 언어 모두 문자열 문자 수가 아니라 UTF-8 512 bytes다.
- **검증**: 공통 valid/invalid/derived fixture에 Windows 선행 거부, exact profile drift/fallback, 전역 binding cursor·alias·page 경계, cancel-uncertain → recoveryRequired → read-only start 거부 → process proof 뒤 abandon, abandoned workspace 재사용 거부를 추가했다. `npm test` 98개, `cargo test -p hello-agent-protocol` 19개, `npx tsc --noEmit`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다. 실제/유료 에이전트·사용자 프로젝트·Windows 실행은 수행하지 않았다.
- **다음 할 일**: 수정 commit을 독립 재검토한 뒤 통합 담당이 승인된 G5 계약을 `m5/coding-agent-ide`에 통합하고, sidecar/Electron/UI 소비자는 이 fail-closed transcript를 그대로 구현한다.

### 2026-08-26 · by GPT-5 Codex (M5 G5 실행·복구 계약 기준선)

- **한 일**: protocol framing·schema v1과 G4 `M5BridgeV2`를 깨지 않고 프로젝트 역할 설정, 영속 채팅, 결정적 fixture 실행·취소, 복구 조회·폐기를 더한 additive `M5BridgeV3`와 Rust/TypeScript runtime validator를 작성했다. 같은 `g5-v1` fixture가 역할/모델 snapshot, 명시적 project/worktree/workspace 신원, 공개 팀 메시지, run 상태·종료 증거, durable event 순서, page 상한을 양쪽에서 검증한다.
- **안전 결정**: 새 채팅은 프로젝트 설정 revision과 정확한 세 역할 profile을 생성 시 고정하며 채팅별 override·silent model fallback을 허용하지 않는다. writer 시작은 run과 workspace lock을 한 transaction으로 commit한 뒤에만 spawn하고 동일 worktree의 두 번째 writer를 거부한다. 취소·종료가 불확실하면 lock을 유지한 `recoveryRequired`이며, G5 복구 해결은 모든 추적 process identity의 소멸이 증명된 뒤 `abandon`만 허용한다. 공개 `TeamMessage`는 결과·요청·상태만 저장하고 hidden reasoning field는 fail-closed다. `run.startAgentSmoke`는 protocol 예약만 유지하고 G5 capability와 bridge에는 광고하지 않으며 Windows 실행과 실제/유료 에이전트 실행은 비활성이다.
- **검증**: `npm test` 98개(그중 focused G4/G5/protocol 계약 16개)와 `cargo test -p hello-agent-protocol` 19개, `npx tsc --noEmit --pretty false`, owned Rust `rustfmt --edition 2021 --check`, `git diff --check`를 통과했다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 수행하지 않았다.
- **다음 할 일**: 독립 계약 리뷰 뒤 이 기준선을 통합하고, Rust sidecar와 Electron/preload·renderer 구현을 서로 다른 worktree에서 고정된 V3 계약에 맞춰 진행한다.

### 2026-08-25 · by GPT-5 Codex (M5 G4 dispose 뒤 snapshot 결과 격리)

- **한 일**: controller dispose가 selection·project·controller 세대를 무효화하고 모든 비동기 상태 쓰기를 막도록 고쳤다. 늦게 성공한 create는 화면 publish/tree/diff/watch 없이 `project.close`를 정확히 한 번 호출해 evidence-preserving abandon으로 소비하며, 늦은 reject도 unhandled rejection 없이 소비한다. dispose 뒤 close 실패는 화면에 다시 쓰지 않고 sidecar evidence/recovery를 그대로 둔다.
- **수명주기 결정**: Rust `project.close`는 `validate_active_project`를 먼저 요구하므로, native open만 끝나고 create 전 dispose된 opened identity에는 invalid close를 보내지 않는다. 해당 opened record의 후속 lifecycle은 sidecar의 영속 계약에 맡긴다.
- **검증**: dispose 중 late create success(close 1/downstream 0), late create reject(close 0/downstream 0/no unhandled), late open(create 0/close 0) 결정적 회귀를 추가했다. 기존 timeout·stale·cancel 경계는 유지한다. `pnpm run m5:test:editor`는 editor-state 25개와 Chromium Monaco smoke 1개를, `pnpm run m5:test:project`는 Node 24개·Rust 28개(+의도한 ignored 1개)·stdio 1개를 통과했고 `npx tsc --noEmit --pretty false`, `pnpm build`, `git diff --check`도 통과했다.
- **다음 할 일**: 이 dispose 경계 commit을 독립 검토하고 최신 통합 branch에서 G4 editor/project 누적 QA를 재실행한다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 하지 않았다.

### 2026-08-25 · by GPT-5 Codex (M5 G4 스냅샷 선택 recovery 경계 보완)

- **한 일**: 새 프로젝트 스냅샷 생성의 unbounded renderer queue를 없애고 기본 60초의 명시적 timeout과 단일 in-flight 상태를 추가했다. timeout 뒤에도 원 요청을 계속 관찰해 늦은 성공은 `project.close`로 evidence-preserving abandon하며, close 확인 전에는 모든 새 선택을 막는다. stale snapshot close 실패와 timeout 뒤 거부는 최신 화면에 초보자용 복구 안내와 technical detail을 보이고 차단을 유지한다. close 성공 뒤에도 자동 재선택/재시도하지 않는다.
- **UI 결정**: picker 취소는 open 결과가 없으므로 기존 project/tree/tabs/notice/error를 그대로 보존한다. 실제 identity가 돌아온 뒤에만 transient message를 비운다. 선택 차단 상태는 workbench의 프로젝트 선택 버튼에도 반영한다.
- **검증**: stale A close failure가 B의 downstream create/tree/diff/watch를 모두 막고 증거를 표면화하는 회귀, unresolved timeout·late success close·late close failure·명시적 C 재선택, 기존 화면 보존 cancellation 회귀를 editor-state에 추가했다. `pnpm run m5:test:editor`는 editor-state 22개와 Chromium Monaco smoke 1개를, `pnpm run m5:test:project`는 Node 24개·Rust 28개(+의도한 ignored 1개)·stdio 1개를 통과했고 `npx tsc --noEmit --pretty false`, `pnpm build`, `git diff --check`도 통과했다.
- **다음 할 일**: 이 recovery 보완 commit을 독립 검토하고 최신 통합 branch에서 G4 editor/project 누적 QA를 재실행한다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 하지 않았다.

### 2026-08-25 · by GPT-5 Codex (M5 G4 새 프로젝트 스냅샷 선행 흐름)

- **한 일**: renderer의 프로젝트 선택 흐름을 native picker/open identity → `createProjectWorktree({ projectId })` → 화면 publish → tree/diff/watch 순서로 고쳤다. 화면에는 선택한 canonical Git root를 프로젝트 이름·경로로 보이고, private snapshot path는 노출하지 않으며 모든 작업은 `projectId`로만 한다. picker 취소는 상태를 바꾸지 않고, 스냅샷 생성 실패는 원본을 건드리지 않았다는 쉬운 설명과 기술 증거만 남기며 자동 재시도하지 않는다.
- **안전 결정**: `project.close`는 active standalone snapshot을 물리 삭제 없이 `abandoned`로 기록하는 계약이므로, 새 선택으로 stale해진 생성 완료만 순서대로 close한다. 같은 project ID의 동시 create/close 경합을 막기 위해 snapshot 생성 구간을 renderer에서 직렬화했다. close 실패의 recovery/evidence는 sidecar가 보존하며 renderer가 삭제하거나 재시도하지 않는다.
- **검증**: editor-state 회귀에서 call order, 생성 실패의 downstream 차단, stale A snapshot의 abandon 뒤 B 활성화, 기존 dirty/conflict/stream 경계를 확인했다. Chromium smoke는 유효한 standalone snapshot manifest를 반환하고, 생성 전 tree/diff/watch가 호출되면 실패하도록 고쳤다.
- **다음 할 일**: 이 원자 commit을 독립 리뷰하고 최신 통합 branch의 G4 project/editor 누적 QA에서 sidecar·Electron bridge와 함께 확인한다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 하지 않았다.

### 2026-08-25 · by GPT-5 Codex (M5 G4 패키지 capability smoke 계약 수정)

- **한 일**: Electron package smoke가 G3의 두 capability만 기대하던 오래된 검사를 제거하고, `tests/fixtures/m5/project-v1/manifest.json`의 canonical G4 누적 capability 순서를 exact 비교하도록 고쳤다. 향후 capability를 늘릴 때 fixture·Rust 계약과 함께 패키지 검증도 의도적으로 갱신되도록 scaffold 정적 검사를 추가했다.
- **검증**: focused Electron scaffold/static test, macOS package smoke, build 및 `git diff --check`를 실행한다. 패키지·lockfile·product code는 변경하지 않는다.
- **다음 할 일**: 독립 리뷰 후 최신 `m5/coding-agent-ide`에 이 원자 commit을 통합하고 G4 cumulative QA를 재실행한다.

### 2026-08-25 · by GPT-5 Codex (M5 G4 통합 테스트 경계 회귀 수정)

- **한 일**: G4 preload의 유일한 `ipcRenderer.send`인 고정 event ACK(`eventAck`, `{ streamId, seq }`)를 허용하되 raw IPC와 다른 send 계열 노출은 계속 금지하도록 scaffold 검사를 좁혔다. 하네스 자기 검사는 G4 `READY`와 다음 미구현 G5의 `NOT_READY`/exit 2를 검증해 누적 명령 재귀를 피한다. Chromium renderer mount smoke는 기본 순수 프런트 `npm test` glob에서 빼고 G4 editor gate에서만 명시 실행하게 이름과 script를 맞췄다.
- **검증**: `npm test` 81 PASS, `npm run m5:test:project` Node 24 PASS·Rust project 28 PASS(+의도한 ignored 1개)·stdio 1 PASS, 승인된 GUI 실행의 `npm run m5:test:editor` editor-state 15 PASS·Chromium Monaco worker 1 PASS, `npm run harness:list` G4 READY/G5 NOT_READY, `npm run build`, `npx tsc --noEmit --pretty false`, `git diff --check`를 통과했다. `pnpm-lock.yaml` Git blob은 전후 `072de6c88fe06530ab922486494cb3524db692e9`로 동일하다.
- **다음 할 일**: 이 좁은 회귀 수정 commit을 독립 리뷰한 뒤 최신 통합 branch에서 G4 누적 하네스·QA를 재실행한다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 하지 않았다.

### 2026-08-25 · by GPT-5 Codex (M5 G4 하네스 연결)

- **한 일**: `m5:test:project`를 추가해 G4 TypeScript 계약·Electron bridge, Rust `project::` 단위 테스트와 G4 stdio process 시나리오를 `--locked`로 한 번에 실행하도록 연결했다. G4 하네스 설명은 linked worktree가 아닌 `standalone snapshot` 프로젝트 안전 API를 가리키도록 정리했다. `m5:test:editor`는 그대로 유지했다.
- **검증**: `CI=true pnpm run m5:test:project`에서 Node 24개, Rust project 28개(+의도한 ignored 1개), G4 stdio 1개가 통과했다. `CI=true pnpm run m5:test:editor`에서 Vite build와 editor-state 15개 및 Chromium smoke 1개가 통과했다. `CI=true pnpm harness:list`는 G4 `READY`, `git diff --check`도 통과했다. `pnpm-lock.yaml` SHA-256은 설치 전후 `a88b2ac7f4ac3afe4644b0766f96e3a7099b0b5eec1a64a64632962ca0bb3bb5`로 동일하다.
- **다음 할 일**: 이 기능 commit을 독립 리뷰한 뒤 최신 `m5/coding-agent-ide`에서 Rust snapshot·Electron·Monaco 변경과 함께 G4 누적 하네스/QA를 실행한다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 하지 않았다.

### 2026-08-25 · by GPT-5.6 Terra

G4 Electron main/preload 브리지를 standalone snapshot 계약 기준선으로 포팅했다. 호환 API 이름(`project.createWorktree`, `createProjectWorktree`, `worktreeId/path`)은 유지하되, renderer에는 고정된 V2 capability만 노출한다. native 폴더 선택, 엄격한 IPC 입력 검증, 파일 5 MiB·diff 10 MiB·동시 stream 16개·stream당 최대 256 chunk 경계, renderer ACK 기반 이벤트 credit/backpressure, 창 종료 시 미확인 전달 fail-closed, sidecar startup/quit 수명주기를 적용했다. manifest 검증은 `workspaceKind: standaloneSnapshot`을 포함한 G4 계약 validator를 그대로 사용한다.

검증: G4 Electron/G3/G4/protocol focused Node tests 37/37 통과, `npx tsc --noEmit --pretty false`, `git diff --check`, darwin-arm64 Electron package 및 package smoke 통과. 첫 package 시도는 sandbox 네트워크 차단으로 실패했으며, 승인된 재시도에서 고정 Electron artifact를 내려받아 통과했다. 실제 사용자 프로젝트·유료 에이전트·Windows 실행은 수행하지 않았다.

**다음 할 일**: 이 feature commit의 독립 코드 리뷰 후 `m5/coding-agent-ide` 최신 통합 commit에서 G4 Rust snapshot 구현과 함께 QA한다.

---

### 2026-08-25 · by GPT-5 Codex (M5 G4 standalone snapshot 작업대 통합 완료)

- **통합 결과**: standalone snapshot Rust 안전 코어 `0bf04f7`, Electron project/file/diff bridge `76f7eea`, Monaco 작업대 `45c33e4`, 실제 프로젝트 선택 흐름 `9ddac9b`, G4 하네스와 통합 회귀 수정을 모두 독립 리뷰 뒤 `m5/coding-agent-ide`에 병합했다. linked Git worktree 대신 held source capability에서 self-contained bundle을 만들고, 필터 없이 raw blob을 독립 저장소에 materialize하며, 저장 최종 교체는 stable data-root FD와 macOS `RENAME_NOFOLLOW_ANY`로 symlink/path swap을 fail-closed한다. 프로젝트 선택은 snapshot 게시 뒤에만 완료되고 timeout·취소·늦은 결과·화면 폐기 경합은 기존 선택과 새 화면을 오염시키지 않는다.
- **통합 QA**: exact 통합 commit `8eff5f556d399c863e7fe18d7706c8aa3136b3ee`의 누적 증거 `.artifacts/m5-harness/g4-project-editor-2026-08-25T12-07-13-799Z.json`은 baseline~G4 11개 명령이 모두 exit 0이고 `dirtyBefore=false`다. 별도 QA도 project Node 24개·Rust 28개(+sentinel 1 ignored)·stdio 1개, editor 상태·경합 25개와 Chromium smoke 1개(총 26개), Electron 22개와 darwin-arm64 package smoke를 재검증해 `G4_READY`로 판정했으며 기록은 `.artifacts/m5-qa/g4-final-8eff5f5-fresh-project.md`다.
- **경계·다음 할 일**: Windows/비-macOS G4는 native handle·durability·watcher 증명 전까지 ready 이전 fail-closed다. 이미 보유한 source directory 내부를 같은 사용자 권한의 악성 프로세스가 export 중 계속 바꾸는 공격은 승인된 위협 경계 밖이며, source Git metadata/object의 감지 가능한 동시 변경은 export 전후 digest와 기존 lock-file 검사로 거부한다. 커밋되지 않은 working-tree 편집은 snapshot 입력이 아니다. 다음 구현은 G5 영속 채팅·역할별 agent run·중단/복구 계약이며 실제 사용자 프로젝트, 유료 agent, push, `main` 병합은 수행하지 않았다.

### 2026-08-25 · by GPT-5 Codex (M5 G4 Monaco 작업대 standalone snapshot 포트)

- **한 일**: 독립 승인된 Monaco renderer 최종 상태를 standalone snapshot 계약 기준선으로 다시 포팅했다. 파일 탐색·Monaco 편집·hash 기반 dirty/conflict 보호·변경 diff와 로컬 worker, descriptor 전 chunk buffer, project epoch를 유지했다. protocol 호환 `worktree` API 식별자는 바꾸지 않았고, 사용자에게 보이는 작업 공간 표현은 모두 `원본과 분리된 작업 복사본`으로 명확히 했다.
- **경계**: renderer는 이미 검증된 `M5BridgeV2`와 project contract만 소비하며 standalone snapshot manifest를 자체 재검증하거나 Electron bridge/Rust/protocol을 수정하지 않는다. 파일 5 MiB, diff 10 MiB, stream 256 chunk 상한과 Chromium local-worker smoke를 유지한다.
- **검증·다음 할 일**: `pnpm run m5:test:editor`(Vite/Monaco build, state 15개와 Chromium smoke 1개), `pnpm build`, G4/Electron contract 14개, `git diff --check`를 통과했다. 이 기능 commit을 독립 리뷰한 뒤 Rust snapshot/Electron bridge와 최신 통합 commit에서 QA한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 macOS kernel no-follow rename 보강)

- **보강**: local macOS SDK와 pinned `libc`를 확인해 `renameatx_np`의 `RENAME_NOFOLLOW_ANY`를 최종 stable data-root rename에 적용했다. 두 경로는 이미 절대/`.`/`..`/NUL을 거부한 상대 경로이며 같은 보유 data-root FD에서 해석되고, 모든 symlink 추적은 커널 syscall 자체가 거부하므로 최종 사용자 공간 검증 뒤의 symlink 교체도 외부 쓰기로 이어지지 않는다. 함께 검토한 `RENAME_RESOLVE_BENEATH`는 이 런타임에서 정상 저장에도 `EINVAL`이어서 사용하지 않는다. `NOFOLLOW_ANY` 자체가 지원되지 않아 `EINVAL`이면 `renameat` fallback 없이 recovery-required로 실패하며, 비-macOS는 기존대로 ready 전에 fail-closed한다.
- **회귀·검증**: 최종 재검증까지 끝난 실제 syscall 직전 hook에서 (1) 대상 부모를 외부로 이동하고 (2) 대상 부모를 외부 symlink로 바꾸는 두 경우를 각각 주입했다. 둘 다 macOS syscall이 실패하고 외부 byte는 그대로이며 recovery blocker가 남는다. 동일 byte 새 부모 교체는 별도 pre-revalidation hook에서 parent device/inode 불일치로 거부한다. 정상 저장을 포함한 sidecar unit 43개·stdio 3개(의도한 sentinel child 1개 ignored), protocol Rust 15개, TypeScript G4/protocol 10개, fmt, sidecar `clippy --no-deps -D warnings`, `git diff --check`를 모두 통과했다.
- **다음 할 일**: 후속 hardening commit까지 포함한 head를 독립 재리뷰하고 G4 통합 여부를 판단한다. 의존성·protocol·플랫폼 gate와 push/`main` 미병합 정책은 바꾸지 않았다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 stable-root save rename P1 수정)

- **수정**: 독립 리뷰에서 cap-std `Dir::rename`이 대상 부모 FD를 연 뒤 `renameat`을 호출하는 사이 그 부모가 관리 영역 밖으로 이동하면 외부의 같은 파일을 덮을 수 있음을 확인했다. 최종 저장 교체를 이동 가능한 하위 부모 FD가 아니라 시작 때 보유한 data-root FD 양쪽과 정규화한 상대 경로를 한 macOS/Unix `renameat` syscall에 직접 전달하도록 바꿨다. syscall 직전에는 stable data-root device/inode, 저장 시작 때 잡은 대상 부모 device/inode, 모든 중간 컴포넌트의 no-follow 상태와 대상 version을 재검증하며 계층 변화는 이미 저장한 recovery lock을 유지한 채 실패한다.
- **회귀·검증**: 실제 syscall 인자와 root FD를 준비하고 최종 검증까지 끝낸 뒤 실행되는 test hook에서 `src` 부모를 외부로 이동하고 동일 byte의 새 부모로 교체하는 경우와 외부 디렉터리 symlink로 교체하는 경우를 재현했다. 두 경우 모두 외부·교체 대상 byte가 그대로이고 저장은 실패하며 recovery blocker가 남았다. sidecar unit 42개·stdio 3개(의도한 sentinel child 1개 ignored), protocol Rust 15개, TypeScript G4/protocol 10개, fmt, sidecar `clippy --no-deps -D warnings`, `git diff --check`를 모두 통과했다.
- **다음 할 일**: 새 원자 fix commit을 독립 재리뷰한 뒤에만 G4 통합을 진행한다. Windows/비-macOS ready gate, push와 `main` 미병합 정책은 그대로다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 standalone snapshot Rust 안전 코어)

- **한 일**: 기존 linked-worktree 구현에서 SQLite project/lock 수명주기, bounded tree/read/save/diff, capability-rooted atomic save, watcher overflow, abandon-without-delete와 stdio stream framing만 보존하고 생성 경로를 `standaloneSnapshot`/`gitBundle` 계약으로 교체했다. sidecar는 no-follow로 잡은 source root·`.git` capability에서 고정 `/usr/bin/git`과 정리된 환경으로 self-contained HEAD bundle을 제한 스트리밍하고, 앱 data의 staging capability에서 standalone repo를 만든 뒤 `ls-tree -rz`와 `cat-file --batch`의 raw committed blob/mode를 직접 materialize한다. checkout/filter/EOL/encoding/ident/LFS 변환, source remote/ref/alternate 연결과 source-dependent diff는 사용하지 않는다.
- **안전·복구**: export 전에 DB recovery marker를 저장하고 source `HEAD/index/refs/config/objects/packed-refs/entry set` digest와 lock 수를 전후 비교한다. corrupt/truncated/oversize/shallow bundle, 2 GiB bundle·4 GiB materialized·200,000 entry·1 GiB blob·512 MiB reserve 위반, non-UTF-8/`.git`/case collision/abnormal mode를 fail-closed한다. 스트리밍 Git 자식의 stderr는 계속 배출하면서 마지막 8 KiB만 보존하고 모든 조기 반환에서 자식을 종료·회수한다. 외부 manifest·`.git` marker·workspace invariant·DB identity를 다시 읽어 composite activation evidence와 일치한 경우에만 active로 전환한다. 활성화 뒤 source가 사라져도 target object DB만으로 재시작·diff가 동작하며 Windows와 비-macOS G4 ready는 별도 게이트 전까지 차단한다.
- **검증**: exact-pin capability 의존성과 lock을 기존 검토 버전으로 갱신했다. `cargo test -p hello-agent-sidecar --locked --offline`에서 sidecar unit 42개와 stdio integration 3개가 통과하고 sentinel child 1개만 의도대로 ignored였다. SHA-1/SHA-256, source/managed target swap, 읽기 전용 source, hook/filter/pager/diff/LFS sentinel 비실행, raw attribute bytes, executable/symlink/submodule, case collision·non-UTF-8·abnormal mode, 손상/절단/상한 bundle, source 제거 후 restart/diff, 저장 parent 이동·ambiguous recovery를 포함한다. sidecar `clippy --no-deps -D warnings`, protocol Rust 15개, TypeScript G4 6개·protocol 4개와 `git diff --check`도 통과했다. workspace 전체 clippy는 이번 변경 밖 `hello-agent-core/proctree.rs`의 기존 최신-Clippy finding 2건 때문에 별도 확인이 필요하다.
- **다음 할 일**: 정확한 기능 commit을 독립 Rust 안전 리뷰하고, 승인되면 통합 담당이 Electron/UI 호환 변경 뒤 최신 통합 commit에서 G4 누적 하네스와 macOS QA를 수행한다. 실제 사용자 프로젝트, push, `main` 병합은 하지 않았다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 snapshot composite activation 계약)

- **리뷰 수정**: 독립 리뷰에서 개별 manifest·source trace·workspace invariant·activation 기록이 각각 유효해도 서로 다른 작업 복사본의 증거를 섞을 수 있음을 확인했다. proposed active manifest의 `projectId`·호환 `worktreeId`·`workspaceIdentity`·canonical Git root·`sourceIdentity`·object format·base commit/tree를 기준으로 source, workspace, activation trace, DB, 외부 manifest, `.git` marker를 한 composite evidence validator가 모두 교차 검증한 뒤에만 active 전이를 허용하도록 Rust/TypeScript 공통 fixture를 추가했다.
- **수치 경계**: snapshot 계약의 모든 JSON 정수는 음수가 아닌 JavaScript safe integer(`0..2^53-1`)로 제한한다. 특히 materialization 뒤 disk bytes는 `2^53-1`을 승인하고 `2^53`을 양 언어에서 거부하는 공통 경계 fixture로 고정했다.
- **검증·다음 할 일**: protocol Rust fmt/clippy/test 15개, TypeScript G2·G3·G4 계약 19개, `tsc --noEmit`, 변경 금지 파일 byte 불변, AGENTS/CLAUDE 동일성, `git diff --check`를 통과했다. 원자 fix commit을 독립 재리뷰한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 standalone snapshot 계약 개정)

- **결정·한 일**: 사용자가 linked Git worktree 대신 capability-protected standalone snapshot을 선택했다. protocol v1 호환 method/type 이름은 유지하되 `WorktreeManifestV1`에 snapshot kind·Git bundle transport·source mutation 금지·object format·commit/tree OID·workspace identity를 고정하고, Rust/TypeScript 공통 fixture와 validator가 source read-only·전후 digest, detached HEAD/index tree, remote/alternates/source 참조 0, object connectivity와 DB/manifest/`.git` marker identity를 fail-closed 검증하도록 개정했다.
- **안전·자원 계약**: bundle 2 GiB, materialized total 4 GiB, 200,000 entries, single blob 1 GiB, materialization 뒤 disk reserve 512 MiB를 한 manifest에 고정하고 streaming만 허용한다. raw committed blob bytes와 Git mode만 materialize하며 checkout/filter/EOL/encoding/ident/export attribute/LFS 변환, symlink follow, submodule init을 금지하고 case collision·non-UTF-8 path·`.git` entry·비정상 mode를 거부한다. recovery marker부터 identity·DB commit 뒤 activation까지 순서를 고정했고 corrupt/truncated/oversize/shallow missing object는 active 0개다.
- **검증**: protocol Rust fmt/test 14개/clippy, TypeScript G2·G3·G4 계약 18개, `tsc --noEmit`, `AGENTS.md`=`CLAUDE.md`, 변경 금지 G2 fixture·bridge·SQLite/dependency/runtime 파일 byte 불변과 `git diff --check`를 통과했다.
- **위협 경계·다음 할 일**: 선택 root·managed target의 path/symlink swap과 일반 동시 변경은 방어하지만, 이미 잡은 source directory 내부 entry를 같은 사용자 권한의 악성 프로세스가 export 중 계속 바꾸는 공격은 경계 밖이다. Windows G4는 별도 게이트 전까지 unsupported다. 이 계약 commit을 독립 리뷰한 뒤 승인된 exact commit에서만 새 Rust standalone snapshot 구현 branch를 분기하고 Electron/Monaco 소비자와 다시 맞춘다. push·merge·rebase와 실제 사용자 프로젝트 시험은 수행하지 않았다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 구현 파동·linked worktree 안전 차단)

- **진행 결과**: G4 계약 commit `e054f0d`를 독립 리뷰 승인 뒤 통합 `a1b53a5`로 병합하고, 같은 exact SHA에서 Rust·Electron·Monaco 세 worktree를 분기했다. Electron `d8f9f9f`는 native picker, V2 runtime validation, bounded stream/ACK backpressure, window lifecycle, file 5 MiB·diff 10 MiB를 독립 리뷰 승인받았다. Monaco `f7e47b3`은 local worker, dirty/conflict·project epoch, async cross-IPC stream과 bounded pre-descriptor buffer를 Chromium smoke 포함 독립 승인받았다. 둘은 Rust 선행 순서 때문에 아직 통합하지 않았다.
- **Rust 리뷰와 차단**: Rust `042bcb1`은 11 capability, SQLite project/worktree 상태, file/diff/watch와 durable save를 구현했지만 반복 독립 리뷰에서 inherited Git config, symlink/rename TOCTOU, unbounded diff, ambiguous post-rename outcome을 발견해 수정했다. 마지막에는 target parent FD를 외부로 이동한 뒤 save하는 race와, 검증한 source Git root를 외부 repo symlink로 바꾼 뒤 `worktree add`가 외부 filter/admin을 건드리는 race가 남았다. held source `.git` cwd와 held target `/dev/fd`를 함께 쓰는 macOS 실험은 Git이 existing target을 거부해 exit 128이었고 registration 0·target empty를 확인했다. ambient path나 F_GETPATH 사후 검사는 안전 증거가 아니므로 사용하지 않는다.
- **현재 상태·다음 결정**: 통합 branch는 G4 계약까지만 포함해 clean이며 G4는 계속 `NOT_READY`다. Rust worktree에는 stable-root save rename 실험의 미커밋 `project.rs`·tests 2파일이 보존돼 있고, 안전 경계 결론 전 통합·커밋하지 않았다. 다음 선택은 (A) linked worktree 대신 capability로 만들 수 있는 detached local clone/snapshot으로 계약 변경, (B) native Git library/직접 worktree 구현으로 범위 확대, (C) 같은 사용자 path-swap 위험을 명시적으로 수용하는 것인데 C는 권장하지 않는다. push와 `main` 병합, 실제 사용자 프로젝트·유료 agent·물리 삭제는 수행하지 않았다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 계약 독립 리뷰 수정)

- **한 일**: 독립 계약 리뷰의 차단점 3개를 닫았다. 기존 G3 lifecycle fixture·상수·validator 의미는 유지하고, G4 전용 ready/status validator가 `G4_CAPABILITIES_V1`의 누적 11개 capability를 정확한 순서와 집합으로 검사하도록 추가했다. 공통 project fixture에는 G4 ready와 ready/unavailable status의 정상·누락·초과 capability 사례를 넣었다.
- **파일·트리 계약 강화**: `hello`의 exact bytes(`68656c6c6f`)가 실제 SHA-256 파일 버전으로 계산되는 언어 중립 golden vector를 추가했고, Rust/TypeScript가 의존성 추가 없이 digest를 직접 계산해 stale/변조 digest를 거부한다. tree page는 root와 하위 `parentPath` 모두 직계 자식 한 단계만 반환하며 nested descendant나 다른 parent의 entry를 fail-closed한다.
- **검증**: G4 양 언어 fixture와 생성 경계, 기존 G2/G3 계약, Rust fmt/test/clippy, TypeScript no-emit, 변경 금지 fixture·`M5BridgeV1` byte 불변, `git diff --check`를 다시 확인했다.
- **다음 할 일**: 이 fix commit을 독립 재리뷰하고 승인된 exact commit에서만 G4 Rust/Electron/renderer 구현 파동을 분기한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 계약 구현)

- **한 일**: 기존 protocol/schema v1과 `M5BridgeV1`을 바꾸지 않고 G4 project/file/diff 공통 fixture, Rust payload parser, TypeScript runtime validator와 좁은 `M5BridgeV2` 타입을 추가했다. 실제 Git·파일시스템·Electron·renderer 구현은 넣지 않았다.
- **고정한 계약**: G4 ready capability는 G3의 `system.status`·`app.prepareQuit`와 예약된 project/file/diff 9개를 합친 누적 11개다. 프로젝트 신원은 정규화된 실제 Git root에 묶이고, 작업 공간은 원본 밖 detached worktree와 full base commit 매니페스트를 사용한다. tree 500개 page/opaque cursor, 정확한 파일 byte의 SHA-256 버전, UTF-8·binary·5 MiB 초과 판정, 256 KiB read/save/diff chunk, expected/current version 충돌, rename/delete/binary diff, watcher overflow 재스캔, manifest/symlink/dirty/recovery 차단, `project.close → abandoned`와 물리 삭제 금지를 fail-closed fixture로 고정했다. POSIX와 정규화된 Windows drive 경로를 함께 다룬다.
- **검증**: `cargo fmt -p hello-agent-protocol -- --check`, `cargo test -p hello-agent-protocol --locked`(14 PASS), `cargo clippy -p hello-agent-protocol --locked -- -D warnings`, G4 Node contract(5 PASS), 기존 G2 Node contract(4 PASS), G3 Node contract(9 PASS), `npx tsc --noEmit --pretty false`, `git diff --check`를 통과했다.
- **다음 할 일**: 독립 계약 리뷰에서 fixture가 구현 시작에 충분한지 승인받은 뒤에만 Rust project/worktree, Electron bridge, renderer 구현 파동을 같은 exact 계약 commit에서 분기한다. 실제 agent/chat/run, 원본 적용과 물리 worktree 삭제는 G5 이후다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G4 프로젝트·편집기 착수 계약 분석)

- **한 일**: G3 독립 QA 뒤 G4를 계약 선행 파동과 최대 3개 독립 구현 파동으로 분해했다. 계약 담당은 project/file/diff payload, `M5BridgeV2`, project lifecycle fixture만 소유하고 기존 protocol fixture·SQLite migration·dependency manifest를 건드리지 않는다. 계약 승인 뒤 같은 exact commit에서 Rust project/worktree 안전 코어, Electron bridge, Monaco UI를 겹치지 않는 별도 branch/worktree로 나눈다.
- **안전·합격선**: native picker로 고른 canonical Git root만 허용하고 non-Git/bare/외부 worktree/HEAD 없음, hook·filter 실행 가능성, symlink 탈출, manifest·DB·Git registration 불일치를 fail-closed한다. tree는 페이지당 500개, Monaco 대상은 UTF-8 5 MiB 이하, file save는 hash 기반 optimistic concurrency와 256 KiB chunk를 사용한다. dirty/conflicted draft는 전환·폐기에서 보존하고 watcher overflow는 조용히 유실하지 않는다. 실제 사용자 프로젝트·agent/chat/run·원본 적용·물리 worktree 삭제는 G4 자동 작업에서 금지한다.
- **다음 할 일**: 이 계약 분석 기록 commit에서 `m5/g4-contracts` worktree를 분기해 단일 계약 담당이 fixture와 양 언어 validator를 구현한다. 독립 계약 리뷰 승인 전 Rust/Electron/UI 구현은 시작하지 않는다.

### 2026-08-25 · by GPT-5.6-Terra (M5 G3 독립 통합 QA)

- **판정**: exact 통합 `3c3799d7978313dcc15b3c6d137dd19e496eb72c`는 `G3_READY`다. 부모 `79d3375` 대비 `architecture.md`와 `history.md`만 바뀐 문서 커밋임을 확인했고 QA 시작·종료 워킹 트리는 clean이었다.
- **검증**: `CI=true pnpm install --frozen-lockfile`, `pnpm run m5:test:storage`(Node 17개, sidecar unit 13개·stdio process 2개), `node scripts/m5-electron-package-smoke.mjs`, `git diff --check`가 통과했다. `node scripts/m5-harness.mjs --list`는 G3 `READY`, 아직 `m5:test:project`·`m5:test:editor`가 없는 G4 `NOT_READY`를 보고했다. 누적 증거는 `.artifacts/m5-harness/g3-storage-2026-08-25T06-13-24-654Z.json`이다.
- **잔여 위험·다음 할 일**: Forge 8 alpha, Windows sidecar/SQLite 종료, packaged Electron의 OS Quit 전체 E2E, macOS plist hardening은 후속이다. 이제 G4 프로젝트 선택·worktree·파일/diff 계약을 먼저 고정하고 독립 계약 리뷰 뒤 구현을 분리 착수한다. push와 `main` 병합은 하지 않는다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3 누적 하네스 통과·현행 문서 반영)

- **한 일**: 최신 통합 `79d3375`에서 G3 누적 게이트를 실행해 Electron 셸, protocol, storage/lifecycle 계약, 실제 Rust sidecar·SQLite, preload와 package smoke를 한 번에 검증했다. `architecture.md`의 G2 시점 설명을 실제 G3 구조로 올리고, 아직 구현되지 않은 project/chat/run·Monaco·diff와 Windows 경계를 현행 사실에서 분리했다.
- **검증**: 승인된 네트워크 환경에서 `CI=1 node scripts/m5-harness.mjs --gate g3-storage`가 PASS했다. 프런트 50개, 기존 Tauri 105개/13 ignored, core 66개, protocol 10개, sidecar 15개, Electron 11개와 storage 집중 Node 17개가 통과했고 macOS arm64 package의 ASAR/Fuse, 실제 sidecar ready→status→quit→SQLite clean marker를 확인했다. 증거는 `.artifacts/m5-harness/g3-storage-2026-08-25T06-13-24-654Z.json`이며 G3는 `READY`, G4는 `NOT_READY`다.
- **다음 할 일**: 이 문서 정합성 변경을 원자적으로 커밋한 뒤 그 exact commit에서 독립 G3 통합 QA를 수행한다. QA가 통과해야 G4 프로젝트 수명주기 계약·구현을 시작한다. push와 `main` 병합은 하지 않는다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-D 실제 package·storage 게이트 연결)

- **한 일**: 독립 승인된 Rust sidecar와 Electron bridge를 순서대로 `m5/coding-agent-ide`에 통합하고, `m5:test:storage`를 G3 계약·Electron failure injection·실제 Rust sidecar 테스트에 연결했다. package smoke는 ASAR/Fuse/sidecar 파일 존재뿐 아니라 외부 sidecar binary를 고유 임시 cwd에서 실행해 v1 `system.ready` 전체 identity, `system.status`, `app.prepareQuit`, exit 0과 실제 SQLite clean marker를 확인한다.
- **리뷰 수정**: 실패 smoke는 child 종료를 요청한 뒤 bounded `exit`을 기다리고 나서만 임시 cwd를 지우며, cleanup 오류가 원래 protocol/assertion 실패를 덮지 않는다. packaged response는 protocol/schema/stream/instance/capability와 status의 ready identity·DB schema·recovery blocker를 모두 확인한다. 독립 재리뷰가 이 wiring을 승인했다.
- **검증**: `m5:test:storage`에서 Node 17개와 Rust sidecar 15개가 통과했다. 승인된 네트워크 환경의 macOS arm64 `m5:package:smoke`가 release sidecar build, ASAR/Fuse, ready→status→quit→clean marker를 통과했다. 일반 sandbox의 최초 package 시도는 Electron 다운로드 `fetch failed`였으며 같은 코드의 승인 실행에서 통과했다. 첫 누적 G3 하네스는 자기 테스트가 아직 G3 `NOT_READY`를 기대해 실패했으므로 기대를 G3 `READY`·G4 `NOT_READY`로 전진시키고, 미구현 게이트 검사를 명령 실행 전 exit 2가 나는 G4로 옮겼다. 독립 재리뷰와 하네스 자기 테스트 2개가 통과했다.
- **다음 할 일**: G3 누적 하네스를 최신 통합 commit에서 실행하고 현행 아키텍처·QA 증거를 기록한다. G4는 그 뒤에만 착수한다.

### 2026-08-25 · by GPT-5.6-Terra (M5 G3-C partial-pipe 종료 수정)

- **한 일**: spawn이 성공했지만 stdin/stdout/stderr pipe 중 하나가 없는 partial child를 검증 전에 main-owned client가 즉시 소유하도록 순서를 고쳤다. 따라서 pipe 검증 실패도 `terminate()` 한 번, pending fail-closed, 재시작 없음으로 같은 수명주기 규칙을 따른다. pipe listener를 붙이기 전 종료하므로 partial child에 listener가 남지 않는다.
- **검증**: missing stderr fake child가 start reject·terminate 1회·spawn 1회·stdout/stderr data listener 0개를 보장하는 회귀를 추가했다. Electron test 11개, TypeScript no-emit, `git diff --check`를 통과했다.
- **다음 할 일**: 이 작은 fix를 독립 재리뷰한 뒤 G3-D 실제 sidecar/package handshake를 최신 통합에서 확인한다.

### 2026-08-25 · by GPT-5.6-Terra (M5 G3-C Electron bridge 리뷰 수정)

- **한 일**: 독립 리뷰의 차단점 3건을 닫았다. sidecar transport는 timeout, oversized/invalid stdout, session·identity corruption에서 child `terminate()`를 한 번만 호출하고 모든 pending 요청을 fail-closed로 거부한다. `system.status`의 ready identity·version·capability는 처음 `system.ready`와 정확히 같아야 하며 불일치는 연결 손상이다. stderr는 마지막 32 KiB만 유지하고 제어 문자를 정규화해 unavailable의 기술 detail에만 붙인다. 이는 token/secret redaction이 아니며 기본 UI 문구나 안전 보장으로 쓰지 않는다.
- **종료 결정**: main-owned coordinator는 safe sidecar 응답 뒤에만 clean exit을 허용한다. 반대로 ready 전 sidecar startup이 실패한 경우에는 clean marker나 재시도를 시도하지 않고, 다음 시작의 unclean recovery 의미를 보존한 채 before-quit 재진입 없이 한 번만 unclean exit을 허용한다. ready 뒤 손상/종료는 이 예외에 포함하지 않는다.
- **검증**: fake child timeout/oversize/unknown response/identity mismatch 종료 1회, pending reject, stderr tail truncation·control normalization, pre-ready unclean close single-flight를 포함한 Electron test 10개와 TypeScript no-emit, `git diff --check`를 통과했다. 최종 packaged handshake는 G3-D 통합 QA에서 실제 sidecar와 다시 확인한다.
- **다음 할 일**: 이 fix commit을 독립 재리뷰하고, 승인되면 G3-D sidecar와 함께 package smoke·storage harness를 최신 통합에서 실행한다.

### 2026-08-25 · by GPT-5.6-Terra (M5 G3-C Electron sidecar bridge)

- **한 일**: Electron main/preload에 G2의 세 내부 channel만 연결했다. main은 `system.ready` first JSONL 세션, 3초 ready 제한, 1 MiB bounded stdout framing, request/response 상관관계와 method별 결과 검증을 가진 단일 sidecar client를 소유한다. 알 수 없거나 중복·불일치인 response와 sidecar 종료는 모든 pending 요청을 fail-closed로 거부하며 재시작·자동 재시도를 하지 않는다. renderer에는 versioned `M5BridgeV1`만 노출하고 정확한 listener wrapper를 해제한다.
- **수명주기·경로 결정**: 내부 IPC는 현재 M5 main window의 top-level frame 및 production `app://m5` 또는 정확한 dev origin만 허용한다. 실제 종료는 main single-flight로 sidecar의 `app.prepareQuit`이 safe 응답을 낸 뒤에만 허용하며 error/unsafe는 앱을 유지한다. production sidecar는 환경 변수 override 없이 `process.resourcesPath/sidecar/hello-agent-sidecar[.exe]`와 `shell:false`, fixed `--stdio`만 사용한다. SQLite path 모호성을 피하기 위해 main이 `app.getPath('userData')/com.wonseokhan.helloagent/m5`를 만들고 정규화한 exact directory를 sidecar `cwd`로 고정한다. Rust sidecar는 이 cwd의 `hello-agent.sqlite3`를 사용한다.
- **패키징·검증**: Forge package-after-copy 단계가 release sidecar를 app.asar 밖의 fixed `Resources/sidecar` 위치에 넣고 smoke가 존재와 ASAR 외부 위치를 확인한다. fake sidecar 실패 주입으로 chunked/oversized JSONL, ready timeout, pre-ready exit, unknown response, pending rejection, no retry, event unsubscribe와 source-level sender/IPC/quit/package 경계를 검사했다. `node --test --experimental-strip-types tests/m5-electron*.test.mts` 8 PASS, `npx tsc --noEmit --pretty false`, `git diff --check`를 통과했다. shared dependency runtime으로 Forge package를 실행해 main/preload/renderer Vite bundle은 성공했지만 Electron arm64 fetch가 격리 네트워크에서 실패해 sidecar 포함 최종 package smoke는 통합 worktree에서 재실행한다.
- **다음 할 일**: 독립 코드 리뷰 후 G3 sidecar 구현과 함께 최신 통합에서 storage harness·실제 packaged sidecar smoke를 실행한다. Forge 8 alpha, Windows/Linux package와 macOS plist hardening은 계속 후속 위험이다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-B Rust sidecar·SQLite 구현)

- **한 일**: `hello-agent-sidecar`에 bundled SQLite 단일 owner와 실제 bounded JSONL stdio 서버를 구현했다. 매 연결에서 FK를 켜고 확인한 뒤 migration·integrity·ledger replay를 수행하며, event append와 projection 갱신은 `BEGIN IMMEDIATE` 한 transaction 안에서 처리한다. 같은 operation의 immutable ledger record가 `requestId`만 다르면 no-op, 그 밖의 차이는 conflict다. projection 변조와 손상은 ready 전에 fail-closed한다.
- **복구·수명주기**: 직전 unclean app instance의 active run/lock 복구를 원장에 한 번만 남기고, persisted `poisoned`/`recovery_required` lock을 항상 status blocker로 유지한다. production은 `--stdio`만 받고 canonical cwd 아래 고정 `hello-agent.sqlite3`만 연다. stdout에는 protocol envelope만 쓰며 1 MiB 전 newline 제한, malformed/partial EOF 거부, pre-ready 오류 stderr+exit 70, `system.status`/`app.prepareQuit`만 제공한다. 정상 종료는 checkpoint와 clean marker 저장 뒤에만 safe 응답하며 stdin EOF만으로는 clean 처리하지 않는다.
- **검증**: sidecar 포맷·`clippy --no-deps -D warnings`와 locked 테스트 15개(단위 13+실제 binary 2), locked Cargo workspace(Tauri 105 pass/기존 13 ignored, core 66, protocol 10, sidecar 15), `git diff --check`를 통과했다. migration 중간 실패 rollback/retry, event insert 뒤 projection 전 rollback, idempotency conflict, projection tamper, unclean recovery 1회성, poisoned lock 지속, DB symlink, malformed/oversize/partial JSONL, no-ready startup 실패, 실제 ready→status→quit→clean 흐름을 모두 임시 DB에서 주입·검증했다. 저장소 전체 fmt는 소유 밖 기존 `src-tauri/src/detect.rs`·`login.rs` 차이, 의존성 포함 전체 clippy는 소유 밖 기존 `hello-agent-core/src/proctree.rs`의 최신 Rust lint 2건 때문에 각각 실패하며 해당 파일은 건드리지 않았다.
- **다음 할 일**: 독립 코드 리뷰 뒤 Electron main의 고정 cwd/stdio supervisor 구현과 함께 통합하고 최신 통합 commit에서 G3 하네스·QA를 실행한다. 실제 Windows 번들/경로/locking 동작과 process supervision·강제 종료는 아직 검증하지 않았다. 실제 유료 에이전트·사용자 프로젝트, merge/rebase/push는 수행하지 않았다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A FK 실제 migration 검증)

- **한 일**: foreign-key connection-local 계약 테스트를 서로 다른 in-memory DB의 generic parent/child 테이블에서, 하나의 고유 OS 임시 SQLite 파일과 실제 `storage-v1/migration.sql`을 쓰는 시나리오로 교체했다. 연결 A가 실제 migration을 적용해 `foreign_keys=1`을 확인하고 닫은 뒤, 같은 파일을 여는 연결 B는 다시 0으로 시작하며 계약 validator가 저장을 거부한다. B에서 ON→1을 검증한 뒤 실제 `runs.chat_id → chats.chat_id`에 필수 열을 모두 채운 orphan insert가 FK 오류로 거부되고 `runs`에 행이 남지 않음을 확인했다.
- **안전·검증**: 테스트 DB와 부속 파일은 `mkdtemp`가 만든 고유 OS temp 디렉터리에만 두고 두 연결을 닫은 뒤 `finally`에서 정리해 저장소 산출물을 남기지 않는다. G2+G3 TypeScript contract 13개, Rust 10개, TypeScript no-emit, fmt/check/clippy, G2 fixture 불변과 diff check를 통과했다.
- **다음 할 일**: 이 최종 P1 fix를 재리뷰하고, sidecar 실제 opener 구현에서도 migration 연결 성공을 후속 runtime 연결의 FK 활성화 증거로 재사용하지 않는지 확인한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A 2차 리뷰 수정)

- **한 일**: 모양만 RFC3339인 불가능한 시각을 통과시키던 predecessor 검증을 양 언어에서 실제 Gregorian UTC 달력 검증으로 교체했다. 계약은 `YYYY-MM-DDTHH:mm:ssZ`, 0001~9999년, 실제 월별 일수와 윤년, 00~23시·00~59분·00~59초만 허용한다. `2026-99-99T99:99:99Z`, 숫자 offset, missing/malformed marker는 공통 fixture에서 `unavailable`로 fail-closed한다.
- **연결별 불변식**: migration 파일 첫 줄의 pragma가 다른 연결에 전파된다고 가정하지 않는다. 모든 sidecar SQLite connection은 open 직후 `PRAGMA foreign_keys = ON;`을 실행하고 `PRAGMA foreign_keys; == 1`을 검증한 뒤에만 migration/read/write와 ready로 진행한다. 실패는 stderr+nonzero exit의 pre-ready `unavailable`이다. lifecycle 시작 순서에도 `storageConnectionVerified`를 migration 앞에 고정했다.
- **실제 SQLite 검증**: 새 in-memory 연결 두 개를 raw foreign-key default로 열어 각각 0임을 확인하고, 한 연결에서만 ON→1 검증 뒤 FK orphan insert가 거부되는 것을 확인했다. 이는 설정이 connection-local이며 migration pragma만으로 충분하지 않음을 실제 엔진으로 증명한다.
- **검증**: G2+G3 TypeScript contract 13개, Rust protocol/storage/lifecycle 10개, TypeScript no-emit, protocol fmt/check, clippy `-D warnings`, SQLite migration/integrity, G2 fixture 불변과 diff check를 통과했다. 새 crate/npm 의존성과 lockfile 변경은 없다.
- **다음 할 일**: 이 2차 fix commit을 재리뷰한 뒤 sidecar의 모든 SQLite open 경로가 공통 opener를 강제하고 ready gate와 실제 failure injection을 통과하는지 구현 단계에서 확인한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A 독립 리뷰 수정)

- **한 일**: 독립 리뷰의 차단점 4개를 공통 Rust/TypeScript 계약과 language-neutral fixture에서 닫았다. 직전 계약의 claimed `content_hash` 비교는 신뢰 경계가 아니어서 제거하고, 같은 `operation_id`의 재시도는 `request_id`만 제외한 영속 event 레코드 전체가 같을 때만 no-op으로 인정한다. payload·aggregate·event identity 중 하나라도 달라지면 conflict다.
- **복구 안전**: `previous_app_instance: null`만 이전 실행이 없는 정상 첫 시작이다. 이전 row가 있으면 sidecar identity·pid/platform·시작/heartbeat·event sequence와 explicit nullable `clean_shutdown_at`을 정확히 검증하고 missing/malformed row는 `unavailable`로 fail-closed한다. 정상 종료 row가 있어도 이미 저장된 `poisoned` 또는 `recovery_required` lock은 항상 blocker와 status에 남으며 조용히 정상화하지 않는다.
- **transaction 계약**: writer는 `BEGIN IMMEDIATE` 안에서 operation lookup → event insert → projection update를 수행하고 commit 뒤에만 위험한 외부 작업을 시작한다. event insert 뒤 projection 전 실패 주입은 두 쓰기를 모두 rollback하며 위험 작업을 시작하지 않는다. duplicate no-op과 conflict 판정도 같은 transaction 안에서 이뤄진다.
- **검증**: G2+G3 TypeScript contract 12개, Rust protocol/storage/lifecycle 10개, TypeScript no-emit, protocol fmt/check, clippy `-D warnings`, SQLite 7-table migration과 `integrity_check=ok`, G2 fixture 불변과 diff check를 통과했다.
- **다음 할 일**: 이 fix commit을 독립 재리뷰한 뒤에만 sidecar SQLite writer와 Electron status/quit consumer 구현을 시작한다. 실제 DB writer는 renderer가 제공한 hash나 operation 판정을 신뢰하지 않고 이 transaction/record-equality 계약을 내부에서 수행해야 한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G3-A 저장·수명주기 계약)

- **한 일**: G2 protocol fixture와 공개 bridge 메서드 목록을 그대로 둔 채 G3 구현 착수용 저장·수명주기 계약을 추가했다. SQLite v1은 별도 `userData/com.wonseokhan.helloagent/m5/hello-agent.sqlite3`에 7개 테이블(`schema_meta/app_instances/events/projects/chats/runs/locks`)을 만들며, append-only event 원장은 `request_id`와 별개인 unique `operation_id`·canonical payload·content hash를 영속 키로 쓴다. 같은 operation/content는 재시작 뒤에도 no-op이고 같은 operation의 다른 content는 conflict다. 양 언어 reducer가 공통 replay/projection·unclean recovery fixture를 같은 결과로 만든다.
- **수명주기 결정**: 내부 channel은 `m5:v1:system:get-status`, `m5:v1:app:prepare-quit`, `m5:v1:sidecar:event`로 고정했다. G3 ready capability는 `system.status/app.prepareQuit` 둘뿐이며 migration·DB integrity·projection replay·unclean recovery가 모두 끝나기 전 `system.ready`를 금지한다. pre-ready 실패는 stderr 증거와 nonzero exit로 끝난다. renderer의 `prepareQuit()`은 상태 조회가 아니라 main 소유 single-flight 실제 종료 조정이며, sidecar는 새 요청 차단·작업 중단·DB checkpoint·clean marker 뒤에만 `safeToQuit: true`를 답한다. production 실행은 `process.resourcesPath/sidecar/hello-agent-sidecar[.exe]`, fixed argv, `shell:false`, stdout protocol/stderr log 분리, 환경 override 금지다.
- **검증**: 공통 fixture를 읽는 TypeScript G2+G3 contract test 11개와 Rust protocol/storage/lifecycle test 9개, protocol crate check, TypeScript no-emit 검사를 통과했다. SQLite CLI가 migration으로 정확히 7개 테이블을 만들고 `integrity_check=ok`를 냈으며, G2 fixture 경로는 기준 commit과 byte diff가 없고 protocol fmt·`git diff --check`도 통과했다.
- **다음 할 일**: 독립 계약 리뷰 뒤 sidecar SQLite/stdio 구현과 Electron main/preload 소비자를 각각 별도 소유 worktree에서 구현하고, 최신 통합 commit에서 G3 누적 하네스를 실행한다. G4/G5 project/chat/run 제품 동작은 이 계약에 포함하지 않았다.

### 2026-08-25 · by GPT-5.6-Terra (M5 G2 통합 QA)

- **판정**: 최신 통합 `cd1a9e7`은 `G2_READY`다. TypeScript protocol 4개와 Rust protocol 3개, 프런트 전체 33개, Tauri 회귀 105개/기존 13 ignored를 확인했고, 기존 macOS arm64 패키지에서 ASAR 내용과 V1 Fuse 9개가 모두 통과했다. G2는 `READY`, 아직 구현하지 않은 G3는 성공이나 skip 대신 exit 2 `NOT_READY`로 닫힌다.
- **증거·잔여 위험**: QA 기록은 `.artifacts/m5-qa/g2-integration-cd1a9e7.md`, 누적 하네스는 `.artifacts/m5-harness/g2-contracts-2026-08-25T04-28-46-065Z.json`이다. QA sandbox의 새 Forge package 재생성은 Electron 다운로드 `fetch failed`에서 멈췄지만, 직전 동일 코드 `f3f41df`의 승인 네트워크 누적 실행과 QA의 기존 package 직접 smoke가 통과했다. Forge 8 alpha, Windows/Linux package, macOS plist hardening은 계속 후속 위험이다.
- **다음 할 일**: G3 sidecar transport·SQLite migration/replay·preload 경계를 계약 fixture를 바꾸지 않는 별도 worktree에서 착수한다. push와 `main` 병합은 하지 않는다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G2 계약 통합·누적 하네스)

- **한 일**: protocol v1 구현 `bff9b47`과 독립 리뷰 수정 `f949cca`를 재리뷰 승인 뒤 `m5/coding-agent-ide`의 `f3f41df`로 병합했다. 현재 구조를 `architecture.md`에 반영하고, 작업 중 생성될 수 있는 저장소-local `.pnpm-store/`를 Git 추적에서 제외했다. 작업자의 격리 worktree 의존성 재사용 시도가 루트 `node_modules`를 부분 재생성한 사실을 확인해 즉시 중단시켰고, 추적 파일 손상 없이 기존 디렉터리를 `/private/tmp`에 보존한 뒤 frozen install로 깨끗하게 복구했다.
- **검증**: 독립 재리뷰가 `f949cca`를 승인했다. 최신 통합에서 `node scripts/m5-harness.mjs --gate g2-contracts`가 PASS했고 프런트 33개, Tauri 105개/기존 13 ignored, core 66개, protocol Rust 3개·TypeScript 4개, macOS arm64 Electron package·ASAR·Fuse smoke를 누적 통과했다. 증거는 `.artifacts/m5-harness/g2-contracts-2026-08-25T04-25-54-493Z.json`이다. 일반 sandbox의 두 선행 실행은 의존성/Electron 다운로드 `ENOTFOUND`로 실패했으며 승인된 네트워크 실행에서 같은 게이트가 통과했다.
- **다음 할 일**: 이 기록 commit의 최신 통합 SHA에서 독립 G2 QA를 수행한다. 통과하면 G3 sidecar transport·SQLite migration/replay·좁은 preload 연결을 새 작업 계약과 별도 worktree로 시작한다. 실제 유료 에이전트·사용자 프로젝트, push와 `main` 병합은 계속 하지 않는다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G2 독립 리뷰 수정)

- **한 일**: 독립 리뷰의 차단 finding 3건을 별도 수정했다. `ProtocolSessionV1` 양쪽 구현은 첫 envelope 전체를 `system.ready`로 고정해 ready 전 request/response/event와 중복 ready를 거부하고, 그 ready의 `sidecarInstanceId`로 세션 신원을 bootstrap한다. Rust `seq`도 JavaScript safe integer 상한 `2^53-1`을 적용했으며, `blob.chunk` payload는 우회 필드 없이 `{ text: string }` 하나만 허용한다. 공통 fixture에 ready 전 세 방향, 중복 ready, 최대 seq/초과 seq, `data` 추가 chunk를 각각 추가하고 기존 valid transcript를 ready-first로 정렬했다.
- **검증**: `cargo fmt -p hello-agent-protocol -- --check`, locked protocol check/test(3 PASS), `npm run m5:test:contracts`(TypeScript 4 PASS + Rust 3 PASS), 전체 Node unit(33 PASS), locked Cargo workspace(Tauri 105 PASS/13 기존 ignored, core 66 PASS, protocol 3 PASS), `git diff --check`를 통과했다. 공유 `node_modules`와 `.pnpm-store`는 건드리지 않았다.
- **다음 할 일**: 새 fix commit을 독립 재리뷰하고, 통합 담당이 dependency 복구 뒤 최신 통합 commit에서 G2 누적 harness를 실행한다.

### 2026-08-25 · by GPT-5.6-Sol (M5 G2 protocol·typed bridge 계약)

- **한 일**: Tauri/Electron/SQLite 비의존 `hello-agent-protocol` crate와 동형 TypeScript runtime validator를 추가했다. 언어 중립 manifest와 JSONL fixture가 protocol/schema 버전, method/event 목록, 한계값, canonical encoding, valid/invalid transcript를 단일 기준으로 소유한다. 요청 ID 중복, 응답 상관관계, sidecar instance 변경, stream seq 누락·역전, 필수 run ID 누락, unknown kind/method/event/version, malformed JSON, outcome 불명 오류의 retryable 설정을 fail-closed로 거부한다. `system.ready` identity/capability와 `app.prepareQuit`의 `safeToQuit`/blocker 계약을 고정하고, renderer 공개 타입은 versioned system status·prepare quit·event 구독만 선언했다. 실제 preload wiring, sidecar I/O, SQLite/migration은 만들지 않았다.
- **검증**: `cargo fmt -p hello-agent-protocol -- --check`, `cargo check -p hello-agent-protocol`, `cargo test -p hello-agent-protocol`(3 PASS), `node --test --experimental-strip-types tests/m5-protocol-contract.test.mts`(4 PASS), 전체 Node unit(33 PASS), `cargo test --workspace --manifest-path Cargo.toml`(Tauri 105 PASS/13 기존 ignored, core 66 PASS, protocol 3 PASS), direct `tsc`+`vite build`, `git diff --check`를 통과했다. `node scripts/m5-harness.mjs --list`는 G2 READY/G3 NOT_READY를 보고했다. 별도 worktree의 `pnpm build`와 누적 G2 harness는 로컬 `node_modules` 부재 뒤 registry `ENOTFOUND`로 자동 설치가 막혀 완료하지 못했으며, 통합 worktree에서 frozen install 복구 후 다시 실행해야 한다.
- **다음 할 일**: 이 commit의 독립 계약 리뷰 뒤 통합 branch에서 frozen dependency 설치와 G2 누적 harness를 통과시키고, fixture를 바꾸지 않는 G3 sidecar transport/SQLite/preload 구현을 분리 착수한다.

### 2026-08-25 · by GPT-5 Codex (G1 마감·G2 착수 준비)

최신 통합 `9487934`에서 G1 Electron/Rust 기반과 독립 QA의 `G1_READY` 기록을 마쳤다. 새 G2 기록을 추가하기 전에 500줄이 된 워크로그의 2026-08-24 엔트리 전체를 날짜 경계대로 `archive/history-2026-08.md`로 이동해 현재 파일을 300줄 이하로 줄였다. 다음 작업은 protocol v1·SQLite schema·golden recovery fixture를 한 계약 담당이 먼저 고정하는 것이며, sidecar/Electron 구현은 그 fixture 승인 전 시작하지 않는다.

### 2026-08-25 · by GPT-5 Codex (M5 G1 Electron Forge 8 scaffold)
- **한 일**: 별도 `m5/electron-forge8-spike`에서 Forge 8 alpha scaffold, local-only M5 renderer, `app://m5` ASAR handler, navigation/webview 차단, narrow preload, Fuse/ASAR packaging과 G1 실제 harness 명령을 추가했다. 독립 리뷰 P1의 `file:` 추가 권한을 V1 9개 strict fuse와 실제 package API/ASAR smoke로 닫고 승인받아 `0b9f769`까지 `m5/coding-agent-ide`에 fast-forward 통합했으며 현행 구조를 `architecture.md`에 반영했다.
- **검증**: 최신 통합 `8374a2a` QA가 `G1_READY`로 판정됐다. frozen pnpm install(hoisted), `npm test` 29 PASS, Tauri 105 pass/13 ignored, core 66 PASS, macOS arm64 package·ASAR renderer·V1 fuse 9개·임시 user-data 기동이 통과했으며 증거는 `.artifacts/m5-harness/g1-foundation-2026-08-25T03-48-40-974Z.json`이다.
- **다음 할 일**: G2 protocol/SQLite 계약을 시작한다. Forge alpha, Windows/Linux package, macOS 기본 plist의 광범위한 ATS/미디어 문자열 hardening, 정상 Quit 자동화 미검증, 실제 사용자 프로젝트·유료 에이전트 smoke는 후속 또는 별도 승인 범위다.

### 2026-08-25 · by GPT-5 Codex (M5 G1 Rust 통합·Electron 공급망 차단)

- **한 일**: G0 계약을 고정하고 Rust/Electron worktree를 병렬 착수했다. Rust core 추출 `a8ca909`와 리뷰 수정 `bacf273`을 독립 리뷰 승인 뒤 `m5/coding-agent-ide`에 fast-forward 통합했다
- **검증**: fresh baseline과 통합 후 `pnpm test:harness` PASS, workspace 171개 통과·13개 기존 ignored, sidecar check와 diff check 통과
- **차단**: Forge `7.11.2` 설치가 pnpm `ERR_PNPM_EXOTIC_SUBDEP`로 실패했다. `blockExoticSubdeps: false`는 전이 Git/tarball 전체를 허용하므로 적용·커밋하지 않았다
- **다음 할 일**: 사용자가 Forge 8 prerelease 실험, 공급망 예외 승인, 또는 Forge stable 대기/패키징 기준 변경 중 하나를 결정하면 Electron scaffold 검증·리뷰·통합을 재개한다

### 2026-08-25 · by GPT-5 Codex (M5 G1 Rust core 리뷰 P1 수정)

독립 리뷰에서 루트 Cargo workspace가 기본 `target/`을 만들지만 저장소 ignore가 `src-tauri/target/`만 전제로 해 생성물이 추적 후보가 된 P1을 확인했다. `.gitignore`에 저장소 루트만 가리키는 `/target/` 한 줄을 추가했으며 다른 ignore 규칙은 바꾸지 않았다. `cargo test --workspace --locked -q`에서 Tauri 105개와 core 66개가 통과하고 기존 13개 승인 필요 테스트만 무시됐으며, 이후 `git status --short`에 `target/`이 나타나지 않고 `git diff --check`가 통과했다.

**다음 할 일**: 이 수정 commit을 포함해 독립 재리뷰와 통합 branch 하네스를 수행한다.

### 2026-08-25 · by GPT-5 Codex (M5 G1 Rust 안전 코어 추출)

`m5/rust-core-extraction` worktree에서 루트 Cargo workspace와 canonical `Cargo.lock`을 만들고, 기존 Tauri crate의 `Agent`·`AppError`·프로세스 트리 종료 계약(F1~F8)·Claude/Codex 실행 계약 및 66개 직접 테스트를 `crates/hello-agent-core`로 옮겼다. Tauri 쪽 기존 모듈은 core를 다시 내보내는 한 줄 shim이라 `handoff.rs`와 `orchestration.rs`는 수정하지 않았다. `hello-agent-sidecar`는 core를 링크하는 빈 실행 골격뿐이며 protocol·SQLite·Electron 계약은 추가하지 않았다. core 직접 의존성은 `serde`·`serde_json`·macOS 프로세스 조회용 `libc`뿐임을 `cargo tree`로 확인했다.

검증은 `cargo test --workspace --offline`에서 Tauri 105개와 core 66개가 통과하고 기존 13개 승인 필요 테스트만 무시됐으며, `cargo test --manifest-path src-tauri/Cargo.toml --offline`, `cargo check -p hello-agent-sidecar --offline`, 소유 파일별 rustfmt, `git diff --check`가 통과했다. F1 재부모화, F2 유예 중 자손, F3 별도 그룹, F4 신호 실패, F5/F7 PID 재사용, F6 정상 종료 뒤 배경 자손, F8 일시적 조회 무결성 실패를 포함한 기존 실패 주입을 core에서 그대로 실행했다. 정확한 `cargo fmt --all -- --check`는 이번 변경 전부터 있던 소유 밖 `src-tauri/src/detect.rs`와 `src-tauri/src/login.rs` 포맷 차이를 보고해 실패하며 두 파일은 계약대로 건드리지 않았다. 별도 worktree의 `pnpm test:harness`는 `node_modules`가 없어 네트워크 설치 단계에서 중단했고, 통합 담당이 의존성이 준비된 통합 branch에서 실행하기로 했다.

**다음 할 일**: 독립 코드 리뷰에서 추출 경계와 기존 M4 의미 보존을 확인하고, 통합 branch에서 하네스·macOS/Windows QA를 통과한 뒤 G2 protocol/SQLite 계약을 별도 작업으로 시작한다.

### 2026-08-25 · by GPT-5 Codex (M5 구현 착수 승인)

사용자가 M5 기능 커밋·통합 상시 승인 규칙을 커밋하고 실제 구현을 시작하라고 승인했다. 이 규칙 변경을 `m5/coding-agent-ide`의 새 기준 commit으로 만든 뒤, 같은 commit에서 G0 baseline 하네스와 `m5_architect`의 경계·의존성 분석을 병렬 실행한다. 둘 다 통과하면 G1 Electron scaffold와 Rust core 추출을 겹치지 않는 기능 branch/worktree로 분기하며, 이후 작업 계약 범위의 기능 커밋과 검증된 통합은 재승인 없이 진행한다. push와 `main` 병합은 하지 않는다.

**다음 할 일**: 자동 승인 기준 commit 생성 → G0 baseline/아키텍처 분석 → G1 작업 계약과 worktree 분기.

### 2026-08-25 · by GPT-5 Codex (M5 기능 커밋·통합 상시 승인 명시)

사용자가 M5 작업 중 기능 커밋과 통합 승인 여부를 매번 묻지 않도록 상시 승인 범위를 명시해 달라고 요청했다. 작업 계약에 지정된 기능 worktree의 원자적 구현·테스트·리뷰 수정 커밋과, 독립 리뷰 및 해당 하네스·QA를 통과한 commit의 `m5/coding-agent-ide` 통합은 M5 전체 완료까지 자동 승인된 것으로 루트 작업 규칙과 오케스트레이션 절차에 고정했다. 구현 담당의 merge/rebase/cherry-pick/push 금지와 통합 담당 단일 소유는 유지한다. push, `main` 병합, 범위 밖 변경, 안전·제품 확정 결정 변경, 실제 유료 에이전트·사용자 프로젝트·파괴적 시험, 외부 게시는 계속 별도 승인을 받는다. 아직 이 규칙 변경 자체는 커밋하지 않았다.

**다음 할 일**: 이 규칙 변경을 기준 commit으로 만든 뒤, 별도 재승인 없이 Gate 0와 G1 기능 branch/worktree 작업을 시작한다.

### 2026-08-25 · by GPT-5 Codex (M5 기계 검증 하네스 커밋 승인)

사용자가 M5 기계 검증 하네스와 CI 연결 변경의 커밋을 승인했다. `baseline` 실제 통과 증거, G1~G6의 누적 `NOT_READY` 게이트, 하네스 자기 테스트, macOS/Windows artifact 업로드와 연결 문서를 하나의 독립 기준선으로 `m5/coding-agent-ide`에 기록한다. `main` 병합과 push는 하지 않는다.

**다음 할 일**: 이 기준 commit에서 G1 기능 branch/worktree를 분기하고 `m5:test:electron`·`m5:test:core`를 실제 scaffold/core 검증에 연결한다.

### 2026-08-25 · by GPT-5 Codex (M5 기계 검증 하네스 선설정)

사용자 요청에 따라 M5 구현 전에 모든 작업자와 통합 담당이 같은 검증을 실행하도록 `scripts/m5-harness.mjs`를 추가했다. 현재 `baseline`은 diff check, 프런트 빌드·unit, 기존 Rust 회귀를 실제 실행하며, G1~G6는 명세의 단계별 검증 script 이름을 예약하고 앞 단계를 누적 실행한다. 구현되지 않은 script는 성공이나 skip이 아니라 `NOT_READY`와 exit code 2로 판정한다. 실행 결과는 branch·commit·dirty 상태·명령·exit code·소요 시간·stdout/stderr를 담은 versioned JSON으로 `.artifacts/m5-harness/`에 남긴다. macOS/Windows CI도 같은 명령을 쓰고 결과 artifact를 보존하도록 연결했다. 하네스 목록과 미준비 판정 회귀 2개를 포함한 프런트 26개, Rust 171개, 빌드와 diff check가 `baseline`에서 통과했으며 증거는 `.artifacts/m5-harness/baseline-2026-08-25T02-11-08-072Z.json`이다. 아직 커밋하거나 push하지 않았다.

**다음 할 일**: 이 하네스 설정을 기준 commit으로 만들고, G1 담당이 `m5:test:electron`과 `m5:test:core`를 실제 검증 명령에 연결한다.

### 2026-08-25 · by GPT-5 Codex (M5.0 명세·멀티에이전트 기준선 커밋 승인)

사용자가 M5 구현 전에 준비한 Electron/Rust 수직 스파이크 명세와 멀티에이전트 운영·역할 설정 묶음의 커밋을 승인했다. 커밋 범위는 `m5-electron-spike.md`, `.codex/config.toml`과 M5 역할 8개, 공통 오케스트레이션 절차, `.worktrees/` 제외 규칙, 연결된 루트 작업 규칙·문서 색인·워크로그로 한정한다. `m5/coding-agent-ide`에 M5.0 착수 기준선으로 기록하며 `main` 병합과 push는 하지 않는다.

**다음 할 일**: 이 기준 commit에서 Gate 0 기준선 검증을 수행하고, 확정된 작업 계약에 따라 첫 기능 branch/worktree를 분기한다.

### 2026-08-25 · by GPT-5 Codex (M5 프로젝트 서브에이전트 역할 설정)

사용자 요청에 따라 문서 절차에 그치지 않고 Codex가 실제로 읽는 프로젝트 설정을 추가했다. 공식 Codex 문서에서 신뢰된 프로젝트의 `.codex/config.toml`, `.codex/agents/*.toml`, 역할별 `model`·`model_reasoning_effort`·`sandbox_mode`, 동시 thread 상한을 확인했다. 기본 동시 상한은 3개로 두고, M5 설계·계약·Electron·Rust 안전 코어·UI·독립 코드 리뷰·통합 QA·최종 게이트의 8개 역할을 책임과 권한별로 분리했다. 설계·리뷰·게이트는 읽기 전용이고, 구현 역할도 매 작업의 별도 worktree·소유 파일 계약 없이는 범위를 얻지 않는다. 직전 엔트리의 “공식 근거가 없는 임의 `.codex` 설정 키는 만들지 않았다”는 당시 조사 상태였으며, 이번에 확인한 공식 프로젝트 역할 형식으로 보완·정정한다. 아직 커밋하거나 push하지 않았다.

**다음 할 일**: 설정과 M5.0 구현 명세를 함께 사용자 검토·승인한 뒤 기준 commit으로 만들고, 그 commit에서 `m5_architect`의 Gate 0 분석과 역할별 기능 worktree를 시작한다.

### 2026-08-25 · by GPT-5 Codex (멀티 에이전트 저장소 설정 추가)

사용자 지적대로 M5 구현 지시보다 먼저 멀티 에이전트 운영 기반을 저장소에 추가했다. 공식 OpenAI 문서가 권장하는 명시적 위임 범위·테스트 기대치·합격/중단 조건을 반영하되, 공식 근거가 없는 임의 `.codex` 설정 키는 만들지 않았다. 대신 `docs/procedures/multi-agent-orchestration.md`에 통합 담당·작업 계약·branch/worktree 격리·단일 파일 소유·파동식 실행·독립 리뷰/QA·중단 게이트를 고정하고, `AGENTS.md`/`CLAUDE.md`에 필수 규칙을 동일하게 반영했다. `.worktrees/`를 Git에서 제외하고 M5.0 명세와 문서 색인을 새 절차에 연결했다. 변경은 아직 커밋하지 않았다.

**다음 할 일**: 설정 변경과 M5.0 구현 명세를 사용자 검토 뒤 기준 commit으로 만들고, 그 commit에서 Gate 0 검증과 첫 기능 worktree를 시작한다.

### 2026-08-25 · by GPT-5 Codex (M5.0 멀티 에이전트 오케스트레이션 제안)

M5.0 구현을 여러 에이전트에게 맡길 운영 방식을 저장소 경계·코드 리뷰·QA 관점에서 독립 검토했다. `m5/coding-agent-ide`는 통합 전용으로 두고 실제 별도 Git worktree/기능 브랜치에서 파일 소유권을 나누며, 한 명의 오케스트레이터만 통합·공유 manifest/lockfile·최종 워크로그를 관리하는 구조가 적합하다. 구현자는 동시에 2~3명으로 제한하고 `명세 커밋 → Electron scaffold/Rust core → protocol fixture 고정 → sidecar와 renderer 병렬 → 통합 hardening`의 파동식 순서를 권장한다. 각 기능은 독립 코드 리뷰와 최신 통합 브랜치 QA를 통과한 뒤에만 병합하며, 실제 macOS 에이전트 smoke는 비용·로그인·사용자 프로젝트 영향 때문에 별도 사용자 승인 게이트로 둔다. 이번에는 운영 방안만 분석했고 구현·커밋·push는 하지 않았다.

**다음 할 일**: 사용자가 현재 미커밋 M5.0 명세 커밋과 기능 브랜치 커밋/통합 권한의 범위를 승인하면, 기준 커밋에서 worktree를 만들고 G0 기준선 검증부터 시작한다.


### 2026-08-24 · by GPT-5 Codex (M5.0 Electron/Rust 구현 명세 초안)

M5 개발의 첫 단위를 바로 착수할 수 있도록 `proposals/m5-electron-spike.md` 검토용 초안을 작성했다. Electron Forge+정확한 Vite plugin 버전 고정, sandboxed renderer와 typed preload, Rust sidecar JSONL v1, SQLite 원장/복구, 단계적 Rust core 추출을 구현 계약으로 구체화했다. 10,000개 파일·5 MiB 파일·500파일/10 MiB diff·4개 동시 chat stream의 수치 합격선과 종료/충돌/강제 종료 실패 주입도 고정했다. 현재 Rust의 `orchestration.rs`·`handoff.rs`는 Tauri 결합도가 높으므로 통째 복사하지 않고 순수 core를 점진 추출한다. 명세·색인·이 워크로그는 사용자 검토 전까지 커밋하지 않는다.

**다음 할 일**: 사용자가 명세의 범위와 수치를 검토한 뒤 승인하면 `m5/coding-agent-ide`에서 첫 Electron scaffold 커밋부터 순서대로 구현한다. `main` 병합과 push는 여전히 하지 않는다.

### 2026-08-24 · by GPT-5 Codex (M5 Electron 착수 기준과 디자인 시안 확정)

사용자가 M5 문서와 디자인 시안이 모두 작성됐음을 확인하고 커밋을 승인했다. M5 기술 기준선을 Electron main + 최소 preload + sandboxed React/TypeScript/Monaco renderer + Rust 안전 sidecar로 확정한 제안서, 작업대·처음 시작하기 독립 실행 시안, 문서 색인과 `AGENTS.md`/`CLAUDE.md`의 M5 작업 규칙을 하나의 설계 기준선 커밋으로 묶는다. 한 채팅=변경 목표/worktree 1:1, 프로젝트별 역할 snapshot, SQLite 원장/projection, 종료 시 안전 중단, 전체 worktree 적용+복원 지점, macOS 우선 실행/Windows 종료 게이트를 포함한다. 두 루트 작업 규칙의 byte 단위 일치, 오래된 Tauri 우선 문구 부재, 시안 CSP·sandbox wrapper, `git diff --check`를 확인했다. `main` 병합과 push는 하지 않는다.

### 2026-08-24 · by GPT-5 Codex (M5 기술 기준선 Electron으로 정정)

사용자가 M5 기술 기준선은 이미 Electron으로 결정했다고 바로잡았다. 직전 작업에서 Codex가 사용자의 `좋아`를 자체 Tauri 우선 권고까지 승인한 것으로 과도하게 해석한 것이 원인이며, 해당 판정을 철회한다. M5 제안서·색인·루트 작업 규칙·핵심 결정을 **Electron main + 최소 preload + sandboxed React/TypeScript/Monaco renderer + Rust 안전 sidecar**로 통일했다. Tauri/Electron 동시 구현과 Tauri 실패 후 전환 게이트 문구를 제거하고, M5.0을 선택 비교가 아니라 Electron IPC·수명주기·보안·성능과 기존 M4 Rust 안전 계약을 검증하는 수직 스파이크로 수정했다. 현재 구현이 Tauri라는 역사적 사실과 M0~M4 코어 재사용은 유지한다. Electron 공식 프로세스 모델·보안 문서와 Node child process 계약을 다시 확인했으며 아직 커밋하거나 push하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 착수 문서 충돌 해소)

사용자 승인에 따라 M5 개발 착수를 막던 문서 충돌을 정리했다. 작명을 M5.0 완료 조건에서 제외하고, Tauri 2 + React/TypeScript + Rust + Monaco 우선 기준선과 반복 실패 시 Electron 전환 게이트를 확정했다. `Project → Chat(변경 목표·worktree 1:1) → Task/TeamMessage/Run/Review`, 앱 기본 역할+프로젝트 재정의+채팅 snapshot, SQLite 이벤트 원장+projection, 앱 종료 시 안전 중단, 전체 worktree 적용+복원 지점, macOS 우선 실행/Windows 종료 게이트를 제안서 전반의 상태·마일스톤·검증·남은 질문에 일관되게 반영했다. 중복 질문 번호와 오래된 다음 검토 순서도 제거했다. `AGENTS.md`와 `CLAUDE.md`의 이름 고정·M4 미구현·M4 현재 브랜치 문구를 함께 고쳐 두 파일을 동일하게 유지하고, `docs/README.md` 색인 설명도 M5.0 착수 기준으로 갱신했다. 문서 검증 뒤에도 아직 커밋하거나 push하지 않는다.

### 2026-08-24 · by GPT-5 Codex (M5 개발 착수 전 잔여 게이트 점검)

사용자가 작업대와 처음 시작하기 시안을 검토해 디자인 시안은 충분히 나왔다고 판단한 뒤, 개발 착수 전에 남은 항목을 M5 제안서와 현재 워킹 트리에 대조했다. 화면 구조·온보딩·프로젝트별 역할/모델·팀 대화의 제품 방향은 M5.0 구현을 시작할 만큼 구체적이다. 실제 착수 전 필수 게이트는 현재 문서/시안 묶음 커밋, 첫 수직 구현 범위와 합격 기준 확정, `Project/Chat/Task/Worktree` 소유 관계, 저장/재시작 계약, 적용·복구 단위, macOS/Windows 실행 범위다. 작명은 이미 보류됐으므로 M5.0 완료 조건에서 제외하고 임시명으로 개발할 수 있으며, LSP·터미널·병렬 writer·장기 보존 정책은 첫 수직 슬라이스를 막지 않는 후속 질문이다. 아직 제안서를 이 판정에 맞게 수정하거나 커밋·push하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 처음 시작하기 인터랙티브 시안)

사용자 요청에 따라 M5 튜토리얼을 `에이전트 진단·설치 → 로그인 → 실제 프로젝트 선택 → 프로젝트별 역할/모델 배정 → 격리 작업 설명 → 실제 목표 입력 → 역할 간 팀 대화 → 첫 diff 검토 → 작업대`로 직접 넘겨 볼 수 있는 시안을 만들었다. 설치·로그인을 연결 이전의 필수 단계로 두고, 샘플 프로젝트 대신 사용자가 명시적으로 고른 실제 프로젝트와 작업을 졸업 뒤에도 그대로 이어 가는 기존 결정을 화면에 반영했다. 대화용 편집 원본은 별도 시각화 위치에 유지하고, 저장소에는 `docs/proposals/m5-guided-start-concept.html` 독립 실행본을 추가해 M5 제안서와 문서 색인에서 연결했다. 기본 단계 이동, 에이전트 최소 1개 선택, 프로젝트/역할/안전/팀 대화/diff 상태와 좁은 화면 대응을 정적 검증했으며 아직 커밋하거나 push하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 인터랙티브 시안을 저장소로 내보냄)

사용자 요청에 따라 대화 전용 위치에서 발전시키던 M5 작업대 시안을 저장소의 `docs/proposals/m5-agent-ide-concept.html`로 독립 실행 가능하게 내보냈다. 여러 프로젝트, 도킹/크기 조절 패널, 파일·변경 탐색, 전역 설정과 프로젝트별 역할/모델, 역할 간 팀 대화까지 현재 검토 상태가 포함된다. 편집용 대화 시각화 원본은 후속 시안 수정을 위해 유지하고, 저장소 파일은 M5 제안서·색인과 함께 검토·커밋할 동반 제안 자료로 연결했다. HTML 스크립트 구문·필수 상호작용 요소·독립 문서 래퍼를 확인했으며 아직 커밋하거나 push하지 않았다.

### 2026-08-24 · by GPT-5 Codex (초보자 설치·로그인 온보딩 유지 확정)

사용자는 M5가 IDE로 확장돼도 초보자를 버리지 않으며, `에이전트 연결` 전에 설치와 로그인이 명시적으로 있어야 한다고 정정했다. M5 온보딩을 `환영 → 에이전트 선택 → 기기 진단 → 설치/복구 → 로그인 → 연결 확인 → 실제 프로젝트 선택 → 역할 배정 → 안전 설명 → 첫 팀 작업 → diff 검토 → 작업대`로 수정했다. 연결은 설치·로그인을 대신하는 추상 단계가 아니라 실제 실행 가능 상태를 확인한 결과다. 에이전트 하나만 준비돼도 단일 역할로 계속할 수 있지만, 둘을 선택하고 하나가 실패하면 조용히 역할을 바꾸지 않고 계속할지 복구할지 사용자가 고른다. 이 결정을 M5 제안서의 확정 사항과 온보딩 흐름에 반영했으며 아직 화면 시안이나 제품 구현은 하지 않았다.

### 2026-08-24 · by GPT-5 Codex (온보딩 첫 작업은 실제 프로젝트로 확정)

사용자는 튜토리얼용 샘플 프로젝트가 아니라 **명시적으로 고른 실제 프로젝트에서 실제 목표를 진행하며 자연스럽게 익히는 방식**을 확정했다. M5 제안서의 확정 결정에 추가하고, 첫 팀 작업은 실제 프로젝트의 격리 worktree에서 실행해 팀 대화·diff 검토·반영/버리기를 배운 뒤 같은 프로젝트와 채팅을 작업대에서 그대로 이어 가도록 정리했다. 큰 목표나 불명확한 목표는 앱이 임의로 축소하지 않고 기획 역할이 분할 선택지를 제시하거나 `답변 필요`로 멈춘다. 샘플/실제 프로젝트 선택은 더 이상 열린 질문이 아니며 아직 화면 시안이나 제품 구현은 하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 온보딩 재구성 제안)

새 작업대와 팀 대화가 핵심이 된 만큼 기존 튜토리얼을 그대로 유지하면 사용자가 설치·첫 메시지까지만 배우고 역할 분담·격리 작업·diff 검토를 모른 채 졸업하는 문제가 있다. 기존 M0~M4 설치·로그인·닥터·Handoff 안전 코어는 재사용하되 온보딩을 `환영 → 에이전트 연결 → 첫 프로젝트 명시적 선택 → 추천 역할 배정 → 격리/검토 안전 약속 → 첫 팀 작업 → 첫 변경 검토 → 작업대로 이동`으로 재구성하는 현재 제안을 M5 문서에 반영했다. 필수 연결·대상 선택은 생략할 수 없지만 설명과 연습은 건너뛰거나 도움말에서 다시 열 수 있고, 이후 고장 시에는 전체 위저드가 아니라 필요한 설정/닥터 단계로만 돌아간다. 첫 팀 작업을 안전한 연습 프로젝트에서 할지 실제 프로젝트에서 할지는 열린 질문으로 남겼으며 아직 화면 시안이나 제품 구현은 하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 팀 대화 채팅 시안)

역할 간 팀 대화 제안을 작업대 시안의 오른쪽 채팅 면에 구체화했다. 단일 `에이전트 결과`와 진행 체크리스트 대신 사용자 요청 뒤 `기획 · Codex`의 계획, `개발 · Claude Code`의 가능성/누락 검토, 기획의 수정 계획, 개발의 구현·검증 결과가 한 타임라인에서 왕복하도록 구성했다. 각 발언에 역할·런타임·모델을 표시하고, 앱이 역할 사이에 무엇을 인계했는지는 짧은 연결 상태로 보여 준다. 원시 실행 증거는 기존 `자세히`에 남기며 추가 지시는 다음 안전한 단계에서 관련 역할로 전달된다는 문구를 넣었다. 제품 코드는 변경하지 않았고 M5 제안서는 직전 팀 대화 계약을 유지한다.

### 2026-08-24 · by GPT-5 Codex (M5 역할 간 팀 대화 제안)

서로 다른 에이전트가 맡은 기획·개발 역할의 왕복을 한 채팅에서 볼 수 있는지 검토했다. 현재 M4도 앱이 구조화 계획을 다음 역할에 인계하므로 기술적으로 가능하며, M5에서는 두 CLI를 임의로 직접 연결하기보다 **앱이 공개 팀 대화 원장과 실행 순서를 소유**하는 방식으로 확장한다. 사용자는 기획안, 개발의 가능성/누락/위험 검토, 수정된 계획, 구현·검증 요약과 질문을 역할·에이전트·모델 표시와 함께 본다. 비공개 추론이나 원시 chain-of-thought는 노출하지 않고 원시 CLI 이벤트는 상세 증거로 분리한다. 첫 버전은 `기획 → 개발 가능성 검토 → 필요 시 기획 수정 → 개발`의 제한된 직렬 왕복만 허용하고, 합의 실패·범위/비용/안전 판단은 사용자에게 되묻는다. 이 현재 제안과 영속 `TeamMessage` 개념, 자동 왕복 상한의 열린 질문을 M5 제안서에 반영했으며 아직 구현하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 파일·변경 패널 재설계)

사용자 검토에서 1차 시안의 파일·변경 탭이 단순 텍스트 목록처럼 보여 조잡하다는 판단을 반영했다. 파일 탭은 프로젝트/브랜치 맥락, 폴더 계층, 파일 종류, 현재 선택과 수정 상태를 구분하는 탐색기로 다시 구성했다. 변경 탭은 전체 파일 수와 `+/-` 규모, 검토 진행률, 파일별 경로·추가/삭제 규모·검토 상태를 한눈에 보여 주는 검토 대기열로 바꿨다. 파일이나 변경 탭에서는 맥락에 맞지 않는 `새 채팅` 버튼을 숨기고, 어느 목록에서 파일을 골라도 가운데 diff/편집 면의 같은 파일 선택 상태가 동기화되도록 시안을 수정했다. 제품 코드와 M5 제안서는 아직 변경하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 프로젝트별 역할·모델 설정 결정)

역할과 모델 설정은 앱 전체에 하나만 두지 않고 **앱 기본값 + 프로젝트별 재정의** 구조로 설계한다. 앱 기본값은 새 프로젝트의 시작값이며, 각 프로젝트는 역할별 에이전트·모델·추론 수준과 역할 지침을 독립적으로 바꿀 수 있다. 프로젝트 설정 화면에는 현재 적용 대상을 명시하고 앱 기본값과 다른 항목 수를 보여 주며, 변경은 선택한 프로젝트의 새 채팅에만 적용한다. 기존 채팅의 실행 계약을 중간에 조용히 바꾸지 않는 방향이다. 인터랙티브 시안에 적용 대상 선택과 프로젝트별 값 전환을 반영했으며 제품 코드와 M5 제안서는 아직 변경하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 설정 화면 대비 수정)

사용자 확인 중 설정 화면의 글자가 어두운 배경에서 검게 보여 읽히지 않는 문제를 발견했다. 인터랙티브 시안의 설정 본문·역할 표·에이전트/모델 선택·역할 이름/지침 입력에 밝은 글자색과 대응 배경색을 명시하고, 브라우저 기본 폼 스타일에 색이 다시 덮이지 않도록 입력 요소 전체의 글꼴·텍스트 색을 통일했다. 제품 코드와 M5 제안서는 변경하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 전역 설정·사용자 정의 역할 결정)

1차 작업대 시안에 설정 진입점이 빠졌다는 사용자 지적을 반영했다. 설정은 특정 프로젝트 패널이 아니라 앱 전역 기능이므로 **가장 왼쪽 고정 탐색 영역 하단**에 두고, 누르면 작은 팝업이 아닌 전용 설정 화면으로 전환한다. 설정 범위는 `역할과 모델`, `에이전트와 계정`, `프로젝트 기본값`, `화면과 레이아웃`, `튜토리얼과 도움`, `개발자 도구`다. 또한 `기획/코딩/검토`를 고정 목록으로 끝내지 않고 사용자가 역할을 추가해 **역할 이름·책임/지침·에이전트 런타임·모델·추론 수준**을 묶어 저장할 수 있게 한다. 역할 지침은 안전 권한을 넓힐 수 없고 읽기/격리 쓰기/적용/외부 전송 같은 권한 프로필이 항상 우선한다. 시안에서 설정 진입·복귀, 역할 선택에 따른 지침 편집, 새 역할 입력 상태를 검증했으며 아직 제안서나 제품 코드에는 반영하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 도킹 패널 크기 조절 결정)

사용자는 재배치 가능한 3개 작업 패널의 너비도 열 사이 구분선을 drag해 조정할 수 있어야 한다고 확인했다. 레이아웃 상태는 프로젝트별로 `패널 순서 + 너비 + 숨김/접힘`을 함께 저장하고, 각 패널에는 내용이 망가지지 않는 최소 너비를 둔다. 최소 너비 아래로 줄이면 내용을 억지로 찌그러뜨리지 않고 접힌 탭으로 전환하며, 구분선 더블클릭과 상단 `레이아웃 초기화`로 권장 기본 너비를 복원하는 방향이다. 아직 제안서나 제품 코드에는 반영하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 프로젝트별 도킹 레이아웃 결정)

사용자는 `채팅/파일 | diff/편집기 | 에이전트 채팅` 3열을 제품이 고정하지 않고 원하는 순서로 drag & drop 배치할 수 있어야 한다고 결정했다. 1차 시안에 각 패널 머리글의 이동 손잡이, 놓을 위치 피드백, 기본 레이아웃 복원을 추가했다. 프로젝트 목록은 앱 최상위 탐색으로 고정하되 작업 패널 순서는 프로젝트별로 저장하는 방향이며, 키보드 사용자는 `Alt+←/→`로 같은 재배치가 가능하게 잡았다. 시안에서 패널 순서 변경과 초기화 상태 전이를 검증했으며 아직 제안서나 제품 코드에는 반영하지 않았다. 실제 구현에서는 패널 ID·순서·크기·숨김 상태의 영속화, 존재하지 않는 패널/오래된 레이아웃 마이그레이션, 좁은 창의 탭 전환 규칙을 별도 계약으로 정해야 한다.

### 2026-08-24 · by GPT-5 Codex (M5 작업대 디자인 시안 1차)

리브랜딩을 보류한 채 `M5` 작업명으로 agent-first IDE의 첫 인터랙티브 화면 시안을 만들었다. 사용자 피드백 뒤 **프로젝트 목록을 독립된 최상위 탐색 영역**으로 추가해 여러 프로젝트를 한 앱에서 관리하고, 프로젝트를 바꾸면 그 아래 채팅·파일·변경 목록이 함께 전환되는 계층을 명확히 했다. 그다음 영역은 여러 영속 채팅/파일/변경 목록, 가운데 코드·diff 편집 면, 오른쪽의 역할·모델·목표·진행·후속 지시, 하단의 명시적 `버리기/수정 요청/반영하기` 검토 행동으로 구성한다. 기본 화면에서는 격리 여부와 쉬운 진행 상태를 보여 주고 실행 ID·worktree·복원 기준·종료 증거는 `자세히` 안에 둬 M4 안전 계약과 점진적 노출 원칙을 유지했다. 시안은 저장소 밖 대화용 시각화로 만들었으며 사용자가 방향을 검토하기 전 제안서나 제품 코드에는 반영하지 않았다. 데스크톱 렌더링, 프로젝트/패널 전환, 상세 증거 열기, 반영 전 확인 동작을 점검했다.

### 2026-08-24 · by GPT-5 Codex (M5 작명 보류)

사용자가 천문명·조어뿐 아니라 보통명사 방향의 초기 후보에도 끌리지 않는다고 판단했다. 이름을 억지로 확정하지 않고 당분간 `M5`를 작업명으로 유지한다. 제품 셸과 핵심 검토 루프가 구체화돼 실제 성격이 보인 뒤 작명을 다시 시작하며, 그전까지 기존 제품명 `Hello, Agent`를 새 IDE의 확정명으로 간주하거나 코드 전반의 리브랜딩을 시작하지 않는다.

### 2026-08-24 · by GPT-5 Codex (M5 작명 취향 재정의)

사용자가 선호 예시로 `Apple`, `Facebook`, `Oracle`을 제시했다. 이에 따라 작명 기준을 희귀 천문명·설명이 필요한 조어에서 **누구나 알고 발음할 수 있는 일상 단어 또는 직관적인 보통명사 합성어**로 전환한다. 빠른 동종 조사를 거치며 의미가 맞더라도 `Workshop`, `Birdhouse`, `Roundtable`, `Switchyard`는 현재 AI 코딩·다중 에이전트 제품과 직접 충돌해 제외했다. 새 방향의 첫 탐색 후보는 `Sunroom`과 `Toolroom`이며 아직 확정하거나 제안서에 반영하지 않았다.

### 2026-08-24 · by GPT-5 Codex (M5 이름 3차 광역 탐색)

천문 이름·음악 용어·합성어를 넓게 조사하고 AI 코딩/개발 도구와 직접 겹치는 후보를 먼저 제거했다. `Thuban`, `Nunki`, `Asterism`, `Orrery`, `Attacca`, `Polyphony`는 현재 AI 에이전트·코딩 오케스트레이터와 이름과 기능이 직접 충돌해 제외했다. `Sidera`, `Alhena`, `Segin`, `Sideband`, `Widefield`, `Long Arc`도 인접 AI·소프트웨어 제품이 강해 제외했다. 새로 남은 검토 후보는 `Nashira`(나시라: 염소자리의 별, 전통적으로 행운/좋은 소식의 이미지), `Partita`(파르티타: 여러 악장으로 이루어진 음악), `Starfold`(스타폴드: 조어)다. 다만 `Partita`는 회계·스포츠 앱, `Starfold`는 게임과 동명이어서 깨끗한 확정 후보는 아니며, 현 시점의 우선순위는 여전히 `Mintaka` 다음 `Nashira`다. 이름은 아직 확정하지 않았고 제안서에는 사용자가 후보를 좁히기 전 반영하지 않는다.

### 2026-08-24 · by GPT-5 Codex (Mintaka·Common Orbit 최종 후보 비교)

사용자가 M5 이름을 `Mintaka` 또는 `Common Orbit`으로 좁혔다. 빠른 동명 조사를 같은 기준으로 다시 수행한 결과 `Common Orbit`은 Rust 소프트웨어 회사 `Common Orbit LLC`가 `commonorbit.com`을, 프로젝트 공유 소셜 앱이 `commonorbit.app`을 이미 사용해 정확 문구·도메인 충돌이 크다. `Mintaka`도 AI 엔지니어링 업체, 소프트웨어 회사, Amazon QA 데이터셋과 과거 `MINTAKA CODE TRACKER` 상표 기록이 있어 완전히 비어 있지는 않지만, 현재 AI 코딩 IDE와의 직접 충돌은 확인되지 않았다. 두 후보 중 브랜드 식별 가능성과 제품군 혼동 위험을 함께 보면 `Mintaka`를 우선한다. 확정 전 한국·미국 주요 상표류와 사용할 도메인·패키지·소셜 핸들을 별도 확인해야 한다.

### 2026-08-24 · by GPT-5 Codex (Parallax 이름 직접 충돌 확인)

사용자가 긍정적으로 본 `Parallax`를 동종 제품까지 확인했으나 독립 제품명 후보에서는 제외해야 한다. `useparallax.dev`는 다중 에이전트 위임·파일 패치·영속 세션·모델 전환을 제공하는 오픈소스 AI 코딩 에이전트이고, `useparallax.app`은 스스로를 “코딩 에이전트를 위한 mission control”이라고 정의한다. 또한 Parallax Inc.의 동명 IDE와 한국의 다중 모델 AI 플랫폼 `parallax.kr`도 존재한다. M5와 이름·기능·검색어가 직접 겹치므로 수식어를 붙이는 정도로는 혼동 비용이 크다. `parallax`가 표현하는 “같은 대상을 여러 관점에서 본다”는 개념만 향후 이름·제품 문구의 소재로 남긴다.

### 2026-08-24 · by GPT-5 Codex (M5 작명 범위 확장)

사용자는 M5 이름이 단일 동물일 필요가 없고 합성어·별자리도 허용한다고 범위를 넓혔다. 동물의 협업 습성에 제품 의미를 맞추는 제약을 버리고, 천문 이름·음악적 다성·관제 작업대 같은 서사도 함께 검토한다. 빠른 동종 제품 충돌 조사에서 `Vela`, `Pyxis`, `Patchbay`, `Deneb`은 AI 빌더·코딩 에이전트·IDE와 직접 겹쳐 제외했다. 현재 1순위 제안은 오리온자리 허리띠의 별이면서 실제로는 다섯 별로 이루어진 다중성계인 `Mintaka`(민타카)다. 아직 확정명은 아니며, Amazon의 동명 QA 데이터셋과 AI 데이터 작업 업체 등 인접 충돌은 최종 상표·도메인 조사에서 판단해야 한다.

### 2026-08-24 · by GPT-5 Codex (M5 이름 2차 탐색·직접 유사 제품 발견)

Orca처럼 짧은 동물 이름에 협업 서사를 붙이는 방향으로 후보를 다시 조사했다. `Cuon`은 협동하는 승냥이속이라는 의미는 적합하지만 동명의 디지털 제품·소프트웨어 회사가 있어 제외했다. `Kea`, `Cebus`, `Aonyx`, `Skein`, `Saola`도 개발 도구나 AI 제품과 직접 충돌해 제외했다. 특히 `cebus-ai/cebus`는 여러 모델 채팅·역할별 모델·코딩 작업 공간을 표방해 M5와 직접 겹치므로 제안서 벤치마크에 추가했다. 현재 작명 1순위는 협동 사냥·공동 육아를 하는 아프리카들개속을 뜻하는 `Lycaon`(리카온)이며, **아직 사용자 선택이나 상표 확인이 끝난 확정명은 아니다.** 동명 소규모 소프트웨어 업체가 있어 채택 전 법적·상표·도메인 조사가 필요하다.

### 2026-08-24 · by GPT-5 Codex (Orca 작명 의미 확인)

Orca 공식 사이트·저장소에는 이름의 유래를 설명한 기록이 없음을 확인했다. 일반적으로 `orca`는 범고래를 뜻하며, 지능적이고 무리로 협력하는 강한 동물 이미지와 제품이 쓰는 `fleet of agents` 표현이 자연스럽게 연결된다. 이는 공식 작명 이유가 아니라 브랜드 문맥상 추론이다. M5 이름도 기능 합성어보다 짧은 상징 하나에 제품 서사를 붙이는 방향이 적합하다는 기존 결정을 유지한다.

### 2026-08-24 · by GPT-5 Codex (M5 설명형 이름 후보 전부 폐기)

사용자 검토에서 `Branchroom`, `Workbraid`, `Braidwork`가 제품 브랜드로 부적합하다고 판정돼 후보에서 모두 제거했다. 다음 이름 탐색은 기능·아키텍처를 합성어로 설명하지 않고 Codex·Cursor·Orca처럼 짧고 독립적인 단어를 찾는 방향으로 전환한다.

### 2026-08-24 · by GPT-5 Codex (M5 이름 1차 후보 조사)

새 제품명 후보를 제품 의미·한영 발음·빠른 검색 충돌 기준으로 검토했다. 직접 충돌한 이름을 제외하고 `Branchroom`을 1순위, `Workbraid`와 `Braidwork`를 대안으로 제안서에 기록했다. 아직 이름을 확정하지 않았으며 최종 1~2개를 고른 뒤 상표·법인·도메인 가용성을 별도 조사해야 한다.

### 2026-08-24 · by GPT-5 Codex (Codex·Orca형 제품 기준점 채택 제안)

사용자 제안에 따라 M5의 제품 기준점을 `Codex의 작업·검토 루프 + Orca의 멀티 에이전트 작업대 + M4의 초보자 안전 계약`으로 구체화했다. Cursor/VS Code 포크는 범위에서 제외한다. 기술 스택은 제품 유사성과 별도 결정이므로 Tauri host와 Electron + Rust sidecar host를 M5.0에서 같은 fixture로 직접 비교하는 현재 제안으로 수정했으며, 아직 어느 한쪽을 확정하지 않았다.

### 2026-08-24 · by GPT-5 Codex (Codex·Cursor·Orca 데스크톱 스택 재확인)

M5 기술 기준선 비교를 위해 현재 데스크톱 스택을 재확인했다. 설치된 Codex 번들은 Electron/Chromium과 Vite 산출물을 사용하며 React 흔적은 있으나 OpenAI 공식 문서가 세부 프런트엔드 구성을 공개하지 않아 그 이상은 확정하지 않는다. Cursor는 공식적으로 VS Code + Electron 기반이다. Orca의 현재 공식 저장소는 `stablyai/orca`이며 Electron + React/TypeScript + Vite + Monaco + xterm/node-pty의 자체 ADE 셸이다. 따라서 IDE라는 이유만으로 Electron이 필수인 것은 아니지만, 벤치마크 셋은 모두 Chromium/Electron 생태계를 택했다. M5는 기존 Rust 안전 코어를 보존하는 Tauri 기준선을 같은 편집·터미널·IPC 시나리오로 스파이크한다는 결정을 유지한다.

### 2026-08-24 · by GPT-5 Codex (M5 제안서 검토·커밋)

사용자 검토 결과 M5 제안서는 M5.0 구현 착수의 기준 문서로 충분하다고 판정했다. 이름·기술 스파이크 합격 수치·채팅/worktree 관계처럼 제품 방향을 바꾸는 항목은 열린 질문과 승인 게이트로 유지하며, 제안서·색인·워크로그와 규칙상 필요한 아카이브 롤오버를 `m5/coding-agent-ide`에 함께 커밋한다. 코드 구현·`main` 병합·push는 하지 않는다.

### 2026-08-24 · by GPT-5 Codex (M5 필수 기능·기술 결정 기준 보강)

사용자 결정을 반영해 이름 변경, 여러 채팅, 역할별 에이전트/모델 설정, Git worktree, 코드 편집을 M5 필수 범위로 올렸다. Tauri·React/TypeScript·Rust를 한 선택으로 묶지 않고 Monaco 기반 현재 스택 스파이크와 Electron 등 대안의 실패 조건을 제안서에 추가했다. 문서 검토만 수행했으며 M5 초안은 계속 미커밋 상태다.

### 2026-08-24 · by GPT-5 Codex (M5 코딩 에이전트 IDE 설계 착수)

Codex·Cursor·Orca의 공식 자료를 재확인하고 `proposals/m5-coding-agent-ide.md` 검토용 초안을 작성했다. 단일 작업의 영속 기록과 검토·적용·폐기·복구를 첫 단계로 두며, 병렬 에이전트·다중 터미널·전체 편집기는 후속으로 미룬다. 새 제안서·색인·이 워크로그는 사용자 검토 전까지 커밋하지 않는다.

### 2026-08-24 · by GPT-5 Codex (M4 파일럿 마감)

S1·S2·§7.10 구현·교차 검토와 §7.6A 사용자 실사용 통과를 M4 완료선으로 확정하고, 제안서의 현재형 상태와 현행 아키텍처를 정리했다. §7.6B·Windows 자손 회수·고비용 확장은 후속이며 완료 조건이 아니다. `main` 병합·push 없이 `m4/orchestration`에 마감 문서만 독립 커밋한다.

### 2026-08-24 · by Claude Opus 5 (§7.6A 통과 — 가설 A 성립)

**사용자가 결함 6건 수정 뒤 Codex 프로젝트로 끝까지 돌려 §7.6A를 통과로 판정했다.** M4의 필수 관문이 열렸다.

관측된 증거(`m4-pilots` 원장에 남아 있음):

| 항목 | 결과 |
| --- | --- |
| 계획 단계 diff | **0바이트** — 읽기 전용이 실물에서 지켜졌다 |
| 구현 단계 | `index.html`·`styles.css`·`script.js`·`README.md` 생성 |
| 진행 상태 | `요청 정리 중 → 만드는 중 → 확인 중 → 완료` |
| 사용자 입력 | 1회. 에이전트 사이 전달 **0회** |

**가설 A가 성립한다** — 사용자가 에이전트 사이의 전달자에서 빠질 수 있다. A2·A3가 이 마일스톤의 존재 이유였고, 그것이 확인됐다.

**가설 B(§7.6B 품질·비용 비교)는 여전히 보류다.** 통과한 것은 A뿐이며, 재작업 루프·다역할 같은 고비용 확장의 게이트는 B다.

이어서 계획 단계 증거 이름을 역할 기준으로 정리했다(`hello-agent-claude.*` → `hello-agent-plan.*`, `plan_to_codex_prompt` → `plan_to_implementation_prompt`). 내용은 Codex 계획 실행인데 파일 이름이 Claude를 가리켜 로그를 잘못 읽게 만들던 문제다. 모듈 이름 `claude_pilot`·`codex_pilot`은 실제로 각 에이전트의 실행 계약이라 그대로 뒀다.

**커밋**: `50b2c60`(결함 6건) · `1a772ec`(문서) · `d0d1e91`(이름 정리). `m4/orchestration`이 `main`보다 11개 앞서며 **병합·push 없음**.

**다음 할 일**

- 사용자가 제품 방향을 밝혔다 — **지금까지의 Tauri 앱을 "튜토리얼" 개념으로 넘기고, 거기서 IDE로 발전**시킬 계획. 범위가 크게 바뀌므로 `docs/proposals/`에 별도 문서로 심의해야 한다. **현재 제품 범위(`CLAUDE.md`)와 §7.10.2 화면 계약이 전제하는 대상 사용자가 유지되는지부터 확정할 것**
- §7.6B(가설 B)는 보류 유지
- (별건) Windows 게이트·S2 `EPERM` 판정 검토

---

### 2026-08-24 · by Claude Opus 5 (§7.6A 2차 — Codex 경로 결함 4건)

사용자가 Codex로 프로젝트를 만들었는데 파일럿이 Claude 인증을 요구하며 실패했다. 파고 보니 **결함이 네 겹**이었다.

| # | 결함 | 상태 |
| --- | --- | --- |
| 1 | 파일럿이 `approval_policy="untrusted"`를 넘겨 **Codex 실행이 무조건 실패** | 고침 |
| 2 | 설치가 `codex`만 받아 **셸 실행 호스트가 없음** → 명령을 하나도 실행 못 함 | 고침 |
| 3 | 파일럿이 프로젝트 에이전트를 무시하고 **Claude를 하드코딩** | 계약 모듈까지 완료, **배선 남음** |
| 4 | 안전 프리셋이 `~/.codex/config.toml`에 `untrusted`를 써 **신규 사용자의 Codex를 망가뜨림** | 고침 |

**4번이 가장 위험했다.** 사용자 설정 파일이 이미 있으면 안 건드리는 규칙 덕에 이 사용자는 피했지만, 앱으로 처음 시작하는 사람은 Codex가 통째로 안 됐을 것이다.

**2번은 오판을 부르는 종류의 결함이다.** 호스트가 없으면 명령이 실행조차 안 되는데 **파일이 안 생기니 "샌드박스가 막았다"로 읽힌다.** 실제로 첫 실측이 그렇게 나왔고, 호스트를 옆에 두고 다시 재서야 진짜 차단이 확인됐다. 격리 설치 E2E에 호스트 존재 단언을 넣었고, 수정을 되돌리면 정확히 그 자리에서 실패한다(실제 네트워크로 확인).

**Codex CLI 실측 (0.149.1)**

| 항목 | 결과 |
| --- | --- |
| `--sandbox read-only` 쓰기 차단 | **확인** — 샌드박스 안 `EXIT=1`, `operation not permitted`, 파일 미생성 |
| `--output-schema` 구조화 계획 | **동작** — Claude와 같은 스키마로 한국어 계획 산출 |
| `approval_policy` 유효값 | `on-failure`·`on-request`·`granular`·`never`. **`untrusted`는 파싱은 되지만 별도로 차단** |
| `system:init` 대응물 | **없음** — `thread.started{thread_id}`뿐 |
| 릴리스 자산 | `codex-{target}.tar.gz`와 `codex-code-mode-host-{target}.tar.gz`가 **따로** 배포됨 |

**Codex 계획 역할 계약을 만들었다**(`codex_pilot.rs`, 테스트 15건). 계약 강도가 Claude와 **같지 않고, 그 차이를 문서에 적었다** — Codex는 실행 구성을 이벤트로 알려주지 않아 `system:init` 같은 fail-closed 검증이 **불가능**하다. 대신 argv 통제 + 실측된 샌드박스 + 사후 빈 diff에 기대며, 마지막은 예방책이 아니라 사후 확인이다.

**3번 배선 완료 — 두 에이전트가 하나의 감독을 받는다**

`handoff.rs`의 실행 감독부를 공유 구조로 바꿨다. `PlanRole`이 **다른 것 넷만** 담고(인자·프롬프트 전달·stdout 해석·계획 추출), 신원 추적·취소 폴링·종료·증거는 `supervise_plan` 하나가 처리한다. 감독부가 복제되면 종료 계약(F1~F8)이 갈라지므로, **감독부가 하나뿐이고 누적 신원 추적을 쓰는지**를 소스로 검사하는 테스트를 넣었다.

이름도 정리했다: `ClaudeEvidence`/`ClaudeRun`/`ClaudeFailure` → `PlanEvidence`/`PlanRun`/`PlanFailure`. 더는 Claude 전용이 아니다.

`execute_handoff`와 `run_handoff`가 **프로젝트 에이전트**를 받고, 화면이 카드에 보여 준 그 에이전트를 그대로 넘긴다.

**배선하며 나온 5번째 결함**: `--ignore-user-config`를 쓰면 **신뢰 폴더 목록도 비어** codex가 실행을 거부한다 — `Not inside a trusted directory and --skip-git-repo-check was not specified`. **계획·구현 두 경로 모두** 이 플래그가 없어 실행이 시작조차 못 했다. 양쪽에 넣고 검증에도 필수로 걸었다. 대상이 앱이 만든 Git 복제본이라는 것은 `validate_pilot_workspace`가 이미 검증하므로, codex의 중복 확인을 건너뛰는 것뿐이다.

**Codex 계획 구간 실사용 검증 통과**

```
session    : 01a031d7-72a2-7451-aa19-4a0f13942af9
steps      : 3
termination: Confirmed after 1 round(s); 2 identity(ies) tracked; survivors: none observed
```

빈 diff까지 확인됐다(계획 역할이 아무것도 안 바꿈). ⚠️ 다만 계획 요약에 **`저장소 조회 도구가 실행 환경 오류로 동작하지 않아 현재 문구는 확인하지 못했으며`** 가 남았다 — 이 기기의 `~/.local/bin/codex`에 아직 셸 호스트가 없어서다. 즉 **Codex가 저장소를 읽지 못한 채 계획했다.** 앱으로 Codex를 다시 설치하면 해소된다.

**검증**: `cargo test --locked` **171 통과 / 0 실패 / 13 무시 · 경고 0**, `npm test` 24 통과, tsc·빌드·rustfmt·`git diff --check` 통과. 격리 설치 E2E와 Codex 계획 구간 E2E 모두 실제로 실행해 통과.

**6번째 결함 — 망가진 설치를 고칠 방법이 없었다**

"앱에서 Codex를 다시 설치하세요"라고 안내했는데, **앱에 그 경로가 없었다.** 홈의 에이전트 버튼은 세 경우에만 뜬다 — 미설치·미로그인·최신 버전 있음. 사용자의 Codex는 **이미 최신**이라 아무 버튼도 없었다. 설치가 망가졌는데 앱은 "설치됨"이라 말하고, 고칠 수단은 없었다.

`AgentStatus.needs_repair`를 추가했다. **설치·로그인·최신이 다 참이어도 실제로 못 쓰는 상태**를 판정한다 — Codex는 `codex` 옆에 `codex-code-mode-host`가 있어야 한다. 그런 상태면 화면이 "고쳐야 해요 / 고치기"를 보여주고 다시 설치한다.

**남은 제한**

- **Codex 계획 계약은 Claude보다 약하다** — `system:init` 대응물이 없어 실행 구성을 검증할 수 없다. argv 통제 + 실측된 샌드박스 + 사후 빈 diff에 기댄다
- `on-request`의 구체적 동작은 미실측. 실제 경계는 `sandbox_mode`다
- **사용자의 기존 `~/.local/bin/codex`에는 여전히 셸 호스트가 없다.** 앱으로 Codex를 다시 설치해야 저장소를 읽고 계획할 수 있다
- 교차 인계(Claude 계획 → Codex 구현)는 지금 구조로 열 수 있지만, 기본은 **프로젝트 에이전트 하나**다. 역할 선택 UI는 §7.10.2대로 기본 화면에 두지 않는다
- **§7.6A 재판정 필요** — 전체 수직 흐름(Codex 계획 → 구현)은 아직 사람이 돌려보지 않았다
- **작업 사본을 화면에서 볼 수 없다**(사용자 관찰, 2026-08-24) — 지금은 "바뀐 파일 목록"과 증거 경로뿐이라, 결과를 보관하기로 해도 그 사본을 열어 확인할 수단이 없다. 파일럿 범위 밖이며 **고도화 단계에서 다룬다**
- 커밋하지 않았다 — 워킹 트리에 있다

---

### 2026-08-24 · by Claude Opus 5 (§7.6A 1차 실사용 — A5 실패, 수정)

**§7.6A 실사용에서 첫 결함이 나왔다.** 사용자가 `pnpm tauri dev`로 앱을 띄우고 파일럿에 `hi`를 넣어 시작하니 `pilot source must be a Git repository`가 떴다.

**원인은 입력값이 아니었다.** 홈 화면이 `projects[0]`을 말없이 파일럿 대상으로 삼았고, `scan_projects`는 `CLAUDE.md`·`AGENTS.md`·`.claude` 같은 **에이전트 표식만** 보고 Git 저장소인지는 보지 않았다. 그래서 비Git 프로젝트에도 실행 버튼이 살아 있었고, 실행 뒤 `prepare_pilot_copy`에서야 거절하며 내부 영문 오류를 그대로 뱉었다.

**A5 실패다** — 비개발자가 전문 영문 오류를 해석해야 했다. **§7.6A는 아직 통과가 아니며 사람이 다시 테스트해야 한다.**

| 고친 것 | 어떻게 |
| --- | --- |
| 암묵적 선택 | `projects[0]` 제거. 프로젝트 카드의 **맡기기** 버튼으로 고른 프로젝트에만 파일럿이 열린다. `projects.find(p => p.pilotEligible)`도 쓰지 않았다 — 여전히 사용자가 고르지 않는다 |
| 사전 판정 | `scan_projects`가 `pilotEligible`을 실어 보낸다. 판정은 **실행과 같은 함수**(`can_copy_pilot_source`)를 쓴다 |
| 비Git 안내 | 카드에 "아직 맡길 수 없어요", 파일럿 안에 "이 프로젝트는 아직 안전한 작업 사본을 만들 수 없어요". **`git init`은 하지 않는다** |
| 입력 문구 | "무엇을 하고 싶으세요?" → **"이 프로젝트에서 무엇을 바꾸고 싶으세요?"** 일반 채팅창처럼 보이던 것을 변경 작업 입력란으로 |
| 오류 노출 | 닥터에 `notgit` 분류 추가. 영문 원문은 "자세히 보기" 안에만 남는다 |

**표시용 판정과 권한 경계를 구분했다.** `can_copy_pilot_source`는 **화면이 거짓말하지 않게 하려는 것**일 뿐이고, 최종 거절은 언제나 `prepare_pilot_copy`가 한다 — 화면을 그린 뒤 `.git`이 사라질 수 있다(TOCTOU). 그 경로도 회귀 테스트로 고정했고, 그때 사용자가 보는 것은 영문 원문이 아니라 지역화된 안내다.

**회귀 테스트 8건 추가** (Rust 137 → 140, npm 9 → 23).

- Rust: 표시 판정과 실행이 같은 답을 내는가 · 저장소가 사라져도 백엔드가 계속 거절하는가 · 스캔 목록이 자격을 싣는가
- 프런트: 자동 선택 없음 · 비Git은 시작 동작이 호출되지 않음 · 명시적 선택 후 실행 가능 · **첫 프로젝트가 비Git이어도 뒤의 Git 프로젝트가 대신 골라지지 않음** · 목록 새로고침 시 선택 유지/해제 · 영문 오류 미노출

세 결함(첫 프로젝트 자동 선택 · 자격 무시 · 오류 원문 노출)을 각각 되돌려 정확히 그 자리에서 실패함을 확인했다.

**검증**: `cargo test --locked` **140 통과 / 0 실패**, `npm test` **23 통과 / 0 실패**, `tsc --noEmit`·`pnpm build` 통과, 변경 Rust `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과.

**변경 파일**: `src-tauri/src/{orchestration.rs,project.rs}`, `src/{App.tsx,App.css,doctor.ts,handoffTarget.ts(신규)}`, `src/locales/{ko,en}.ts`, `tests/{handoff-target,doctor-pilot}.test.mts`(신규), `docs/{history.md,proposals/m4-orchestration.md}`

**후속 3 (같은 날) — "Hi"만 넣었을 때의 올바른 결과**

사용자가 `Hi`만 넣고 "안전하게 멈췄어요"를 봤다. **증거 파일을 열어보니 `Hi`와 무관한 실패였다.**

```
Failed to authenticate. API Error: 401 OAuth access token has been revoked.
```

토큰이 만료돼 계획 단계가 죽은 것이었다(지금은 정상 — 일시적이었다). 그래서 로그인을 복구한 뒤 **`Hi`를 실행 계약 그대로 재실측**했다. Claude는 정확하게 행동한다:

```json
{ "steps": [], "summary": "the user's message (\"Hi\") is a greeting, not a task ... Recommend asking the user what they'd like to build" }
```

**의도 분류기가 필요 없다** — 계획 역할이 이미 그 판단을 한다. 동작(멈춤·Codex 미실행·무변경)도 이미 맞았다. 틀린 것은 **보여주는 방식**이었고, 결함 두 개를 고쳤다.

| 결함 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| ② 원인 유실 | `structured_output`이 없으면 무조건 `"run produced no structured plan"`으로 덮어써, **실제 원인(인증 만료)이 사라졌다.** ⚠️ `structured_output: null`은 `get`이 `Some(Null)`을 주므로 대체 경로도 오류 보고도 건너뛴다 | `null`을 걸러 내고, CLI가 말해 준 원인을 그대로 전한다. 닥터에 `auth` 분류를 추가해 **"로그인이 풀렸어요 / 다시 로그인해 주세요"** 로 번역한다 |
| ① 되묻기를 사고로 취급 | `steps: []`가 다른 실패와 같은 "안전하게 멈췄어요" 화면으로 갔다. 잘못된 게 없는데 그렇게 말하면 초보자는 자기가 망가뜨린 줄 안다 | `HandoffStage::NeedsGoal`로 분리. **"무엇을 바꿀지 알려주세요"** + 예시 + 다시 적기. 진행 표시도 빨갛게 칠하지 않고, 물어본 적 없는 "바뀐 내용"도 보여주지 않는다 |

멈춘 화면은 이제 **닥터를 거친다** — 쉬운 말로 이유를 보여주고 영문 원문은 상세 안에만 남는다.

회귀 테스트 6건 추가(Rust 148 → 152, npm 23 → 24). 원래 코드로 되돌려 `원인이 사라졌다: structured plan is empty`로 정확히 실패함을 확인했다.

**남은 제한**: 계획 역할의 설명(`summary`)은 영어로 온다. 기본 화면에는 한국어 문구만 쓰고 그 설명은 "자세히 보기"의 *하려던 것*에 둔다 — 계획 프롬프트에 사용자 언어를 실어 보내는 것은 이번 범위 밖.

**후속 2 (같은 날) — 실사용에서 두 번째 결함**

준비 → 맡기기 → 시작까지 갔더니 `command run_handoff missing required key runId`가 떴다.

**Tauri 인자 문제가 아니라 직렬화 계약이 어긋난 것이었다.** `HandoffStart`에 `#[serde(tag="state", rename_all="camelCase")]`만 달아 뒀는데, `rename_all`은 **변형 이름만** 바꾼다. 변형 안의 필드는 `run_id` 그대로 나갔고(실측: `{"state":"ready","run_id":"run-1"}`), 화면의 `started.runId`는 `undefined`가 됐다. 그 값을 다음 `invoke`에 넘기니 **JS가 키를 통째로 빼 버려** 백엔드가 "필수 키 없음"으로 거절했다.

`rename_all_fields = "camelCase"`를 추가해 고쳤다(실측: `{"state":"ready","runId":"run-1"}`). `install.rs`의 `InstallEvent`는 **이미 둘 다 달고 있었다** — 프로젝트에 올바른 선례가 있었는데 따르지 않은 것이다.

**TypeScript가 못 잡는 종류의 버그다.** TS 인터페이스는 손으로 적은 선언일 뿐이라 백엔드가 실제로 내보내는 모양과 어긋나도 컴파일이 통과한다. 그래서 **화면이 읽는 키 이름 자체를 Rust 테스트로 고정**했다(3건). `rename_all_fields`를 빼고 돌려 정확히 그 자리에서 실패함을 확인했다.

프런트에도 방어를 뒀다 — 실행 ID가 비어 있으면 그대로 넘기지 않고 멈춘다. 빈 값을 넘기면 인자가 조용히 사라져 엉뚱한 곳에서 터진다.

**남은 함정**: `login.rs`의 `LoginEvent`도 `rename_all_fields`가 없다. 지금은 필드가 전부 한 단어(`url`·`line`·`success`)라 무해하지만, **여러 단어 필드를 추가하는 순간 같은 사고가 난다.** 이번 범위 밖이라 고치지 않고 기록만 남긴다.

**후속 (같은 날) — 막다른 길을 풀었다**

사용자가 "그럼 어떻게 풀어야 하는데?"라고 물어 확인해 보니, **`create_first_project`가 `git init`을 하지 않아 앱이 만든 프로젝트가 전부** "아직 맡길 수 없어요"에 걸리는 상태였다. 예외가 아니라 기본값이었다. 안내만 남기면 비개발자는 스스로 풀 수 없다.

사용자 승인을 받아 **"되돌릴 수 있게 준비하기"** 를 만들었다(`prepare_project_for_pilot` + `prepare_project_for_handoff` 명령).

- 확인 창에서 **무엇이 생기는지** 먼저 보여 준다 — "파일 내용은 바뀌지 않아요 / 폴더 안에 기록용 공간이 생겨요". 동의 없이는 실행되지 않는다
- `git init` + **첫 복원 지점 커밋**. 실측으로 `git init`만으로는 부족함을 확인했다 — 커밋이 없으면 `rev-parse HEAD`가 실패해 시작 지점을 못 잡는다
- **이미 있는 저장소의 이력은 건드리지 않는다.** 커밋이 없는 저장소면 시작 지점만 만들어 준다
- **다른 저장소 안의 폴더는 거부한다** — 중첩 저장소는 사용자의 기존 이력을 어지럽힌다. 남의 저장소에 손대는 것은 동의 범위 밖이다
- 커밋 신원이 없는 기기에서도 되게 **이 커밋에만** 앱 신원을 주입한다. `git config`(전역·로컬)는 건드리지 않는다
- 앱이 만드는 새 프로젝트는 처음부터 준비된 상태로. **실패해도 프로젝트 생성 자체는 성공**이다 — 편의 때문에 첫 프로젝트 만들기가 막히면 안 된다

회귀 테스트 5건 추가(Rust 140 → 145): 준비 후 실제로 복제본이 만들어지는가 · 동의 없이는 `.git`이 생기지 않는가 · 기존 이력 보존 · 중첩 거부 · 신원 없는 기기.

**남은 제한**

- **§7.6A 재판정 필요** — 이번 수정은 결함을 고친 것이지 판정이 아니다
- 파일이 아주 많은 폴더(`node_modules` 등)는 첫 커밋이 오래 걸릴 수 있다. `.gitignore`를 앱이 만들어 주는 것은 **원본에 파일을 더하는 일**이라 동의 범위를 넘어 이번에 하지 않았다
- 커밋하지 않았다 — 워킹 트리에 남겨 뒀다

---

### 2026-08-21 · by Claude Opus 5 (Codex 최종 통과 → 커밋)

**Codex 재검토를 통과했다.** 추가 차단 사항 없음. Codex가 `npm test` 9건, Rust 137건, TypeScript·Vite 빌드, rustfmt, `git diff --check`를 독립 검증했다.

사용자 승인을 받아 `m4/orchestration`에 **커밋 2개**로 나눠 넣었다. 각 커밋이 독립적으로 빌드·테스트를 통과한다.

| 커밋 | 내용 |
| --- | --- |
| `b6f2142` `feat(m4)` | `proctree`·`claude_pilot`·`handoff` 신규, `orchestration` 실패 구조화, 화면·프런트 테스트·검증 명령 |
| `70510ae` `docs(m4)` | §7.10 구현 상태, §9 철회된 주장, §10 결정 이력, 워크로그 아카이브 롤오버 |

**`main`에는 병합·push하지 않았다.** 현재 `m4/orchestration`은 `main`보다 8개 앞선다. 병합은 M4 전체가 완료·검증된 뒤 사용자가 명시적으로 승인한 경우에만 한다.

**다음 할 일**

- **§7.6A 자동 인계 UX 판정(A1~A5)** — 사람이 실제로 써 보고 내리는 판정. 구현이 끝나 막힌 것이 없다. M4의 다음 관문이다
- (별건) S2 `tree_is_gone`/`signal_ok`의 `EPERM` 오판 검토
- (별건, Windows 게이트) 자손 회수 실측 전까지 Windows에 노출하지 않는다

---

### 2026-08-21 · by Claude Opus 5 (Codex 8차 반려 — 준비 단계 취소 경합 1건)

7차 지적은 모두 해소 확인. 남은 경합 1건을 고쳤다. 여전히 **미커밋**이다.

**무엇이 틀렸나** — 화면이 정상 완료 뒤에도 이전 실행 ID를 들고 있었다.

1. 첫 실행이 끝난다 (화면은 `run-1`을 계속 보유)
2. 두 번째 실행 시작 → `begin_handoff` 응답 대기
3. 그 사이 중단 클릭 → ID가 있으니 "실행 중"으로 보고 **곧바로** `cancel_handoff`
4. 그 슬롯은 이미 끝난 실행의 것이라 **아무 일도 하지 않는다**
5. 새 ID가 와도 보류된 중단이 없어 **두 번째 실행이 그대로 진행**

끝난 실행의 늦은 진행 상태가 새 화면을 덮는 문제도 같은 뿌리였다.

**어떻게 고쳤나** — 실행 추적과 보류 중단을 `src/handoffRun.ts`의 **순수 전이 함수**로 분리했다. ref 두 개를 손으로 굴리던 것을 대체한다.

| 규칙 | 어디서 |
| --- | --- |
| 새 실행을 시작할 때 이전 ID를 버린다 | `startingRun()` — **`begin_handoff` 호출 전에** |
| 실행이 끝나면 비운다 | `runSettled()` — 결과·막힘·오류 **모든 경로** |
| 예약 전 중단은 보류했다가 예약 즉시 전달 | `requestCancel()` → `runReserved()` |
| 예약 대기 중 중단이 있었으면 **사본을 만들기 전에** 멈춘다 | `abort` — 그만두라는 작업의 사본을 남길 이유가 없다 |

**프런트 테스트를 처음 붙였다.** 새 프레임워크는 들이지 않았다 — `website/`가 이미 `node --test`를 쓰고 있어 같은 방식에 노드의 타입 제거 실행(`--experimental-strip-types`, Node 22.18)을 얹었다. `tests/handoff-run.test.mts` 9건을 `npm test`로 묶었다.

**결함을 되돌려 확인했다.** 처음 쓴 테스트 대역은 결함을 **가리고 있었다** — `finish()`가 이미 ID를 비워서, 시작 시 정리를 지워도 통과했다. 실제 옛 코드는 성공 경로에서 비우지 않았다. 대역에 `leaveStaleRunId()`를 넣어 그 상태를 재현하니 세 결함이 각각 잡힌다.

| 되돌린 결함 | 실패한 테스트 |
| --- | --- |
| 시작 시 이전 ID 미정리 | 예약 대기 중 중단 유실 · 늦은 진행 상태 덮어쓰기 (2건) |
| 종료 시 미정리 | 끝난 실행의 진행 상태 수신 (1건) |
| 보류된 중단 폐기 | 중단 전달 · 경합 시나리오 · 소비 확인 (3건) |

**검증**: `cargo test --locked` **137 통과 / 0 실패 / 12 무시**, `npm test` **9 통과 / 0 실패**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과.

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `package.json`, `src/App.tsx`, `src/handoffRun.ts`(신규), `tests/handoff-run.test.mts`(신규)

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 7차 반려 3건 + 추가 1건 반영)

6차 지적 3건은 해소 확인. 남은 것들을 고쳤다. 모두 **사용자가 막힌 상태에서 빠져나올 수 있는가**에 관한 문제였다. 여전히 **미커밋**이다.

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 복구 흐름이 막힘 | 잠금 **오판**은 고쳤지만, 잠긴 A가 슬롯을 쥐면 새 B는 전부 거부되는데 화면에는 B만 보였다. B를 정리해도 슬롯은 A의 것이라 안 풀려 **후속 실행이 영영 거부**된다 | `begin_handoff`가 **사본을 만들기 전에** 막고(`HandoffStart::Blocked`) A의 경로·사유·잠금 상태를 돌려준다. 화면이 그 경로로 A를 정리·복구한다. 우회 경로용으로 `PilotFailure.blocker`도 같은 정보를 싣는다 |
| 증거 생성 도중 부분 실패가 축약됨 | `FailureContext`를 **파일 생성 뒤에** 만들어서, events는 만들어졌는데 stderr에서 실패하면 그 경로를 잃었다. 만들어진 events는 아무도 모르는 고아가 된다 | 맥락을 **경로가 정해지는 즉시** 만든다 |
| "만들 예정이던 경로"와 "저장된 파일"을 구분 안 함 | diff 생성에 실패해도 `evidence.diff`가 채워져 화면의 "저장된 기록 위치"에 **존재하지 않는 파일**이 노출됐다 | `PilotEvidencePaths`를 `Option`으로. 실패 시점에 **`is_file()`인 것만** 채운다(`exists()`가 아니다 — 같은 이름의 폴더를 저장된 기록이라 말하면 안 된다) |
| (추가) `PoisonedRun`에 표시 오류 없음 | 최초 실패 결과가 화면에서 사라진 뒤 같은 슬롯을 다시 확인하면 **종료 오류가 `lockError`로 다시 쓰였다** | 슬롯에도 `error`(종료 실패)와 `marker_error`(표시 저장 실패)를 나눠 저장 |

**회귀 테스트는 전체 흐름을 확인한다.** "B가 잠기지 않는다"만으로는 부족하다는 지적이 맞다 — A를 poison → 시작 시도가 막힘 → **알림에 담긴 경로로 실제 A를 정리** → 다음 실행이 열림 → A는 영구 폐기돼 재사용 불가, 까지 한 테스트로 확인한다. 부분 실패는 stderr 경로를 **폴더로 선점**해 events만 만들어진 상태를 실제로 만들어 검증한다(`orchestration`).

**발견**

- 테스트에서 `session.slot()` 가드를 쥔 채 `session.release()`를 부르면 **데드락**이다. `std::sync::Mutex`는 재진입이 안 된다. 테스트가 120초 타임아웃으로 잡혔고, `reserve()`가 돌려준 run_id를 쓰도록 고쳤다
- 파일 존재 확인은 `exists()`가 아니라 **`is_file()`** 이어야 한다. 폴더로 선점하는 실패 주입 기법을 쓰면 `exists()`는 참이 되어 "저장된 기록"으로 오보고된다

**검증**: `cargo test --locked` **137 통과 / 0 실패 / 12 무시 · 경고 0**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과, 통합 테스트 2건 재실행 통과(CLI `2.1.237`).

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{orchestration.rs,handoff.rs,handoff/tests.rs}`, `src/App.tsx`, `src/App.css`, `src/locales/{ko,en}.ts`

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 6차 반려 3건 반영)

5차 지적 6건은 해소 확인. 남은 3건을 고쳤다. 셋 다 **증거·잠금 정보를 잃거나 뒤섞는** 문제였다. 여전히 **미커밋**이다.

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 다른 복제본의 오염을 이 복제본 것으로 오판 | `poisoned_slot_error`가 오류만 돌려주고 경로를 안 줬다. **슬롯은 하나뿐**이라 A의 오염이 깨끗한 B를 `sessionOnly`로 잠그고, B를 폐기해도 `clear_poisoned_slot`은 경로가 달라 슬롯을 못 풀어 **실행이 영영 막힌다** | `poisoned_slot`이 `(복제본 경로, 오류)`를 반환하고, 대상과 일치할 때만 잠근다 |
| 실패 시 Codex 증거 경로가 사라짐 | `execute_pilot`이 `Result<_, String>`이라 오류가 문자열로 축약됐다. 증거 파일은 이미 만들어져 있는데 경로를 잃어 화면에 Claude 증거만 남았다 | `PilotFailure`로 오류·`run_id`·복제본 경로·증거 경로·오염·표시 오류를 실어 나른다. `run_id`는 화면 상세에도 노출(증거 파일 이름이 그 값으로 갈린다) |
| `sessionOnly`의 `lockError`가 종료 오류 | `guard.poison`에 저장되는 건 **종료 실패 오류**다. 표시 저장 실패 오류는 따로 보존되지 않아, "표시를 저장하지 못한 이유" 자리에 엉뚱한 문구가 나왔다 | `abandon_run`이 `poisoned`(무엇이 잘못됐나)와 `marker_error`(왜 저장 못 했나)를 분리. `attention`·`lockError`에 각각 |

**잠금 판정을 네 갈래로 명시했다**: ① 이번 실행이 오염시킴 → `marker_error` 유무로 `persisted`/`sessionOnly` · ② 이 복제본의 디스크 표시 → `persisted` · ③ **경로가 일치하는** 슬롯 오염 → `sessionOnly` · ④ 그 외 → 잠그지 않음.

**테스트를 실제 실패 경로로 옮겼다.** `stopped()`를 손으로 조립하면 증거를 잃는 지점을 검증하지 못한다.

- 증거 생성 **뒤에** 복제본의 `.git`을 지우는 실행 대상을 주입해 진짜 `PilotFailure`를 만들고(실패 문구: `cannot register new files for the pilot diff (git add -N exited with exit status: 128)`), production 매핑에 통과시켜 Codex 증거 경로가 결과까지 오는지 확인
- `abandon_run`의 두 오류 분리는 **표시 경로를 폴더로 선점**해 저장을 실패시켜 검증. 두 문자열이 실제로 다른지 단언
- A/B 두 복제본으로 슬롯 오판 회귀 고정 — B는 잠기지 않고 A의 오염은 A에 남아야 한다

**검증**: `cargo test --locked` **131 통과 / 0 실패 / 12 무시 · 경고 0**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, `git diff --check` 통과, 통합 테스트 2건 재실행 통과(CLI `2.1.237`, `stages: [Organizing, Building, Checking, Done]`).

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{orchestration.rs,handoff.rs,handoff/tests.rs}`, `src/App.tsx`, `src/locales/{ko,en}.ts`

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 5차 반려 6건 반영)

4차 지적 5건은 해소 확인. 남은 6건을 고쳤다. 여전히 **미커밋**이다.

**차단 2건 — 종료 계약의 구멍**

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 누적 집합의 키가 pid | 소멸을 확인한 pid를 새 자손이 재사용하면 `contains_key(pid)`가 참이라 **편입이 막힌다.** 옛 신원은 `Gone`이라 이후 조회에서도 건너뛰므로 **살아 있는 자손을 아무도 보지 않는 채 `Confirmed`** 가 난다 | 키를 `(pid, start_us)` 전체로. `ProcIdentity`에 `Ord` 파생. **F7** 추가 |
| 실행 중 `observe` 오류를 버림 | 일시적 열거 실패나 신원 불일치가 종료 시점에 사라져 있으면 그 사실을 잊고 `Confirmed`가 난다. 실패한 회차에 생겼다 사라진 자손이 있었는지 **알 방법이 없다** | `integrity_error`에 최초 오류를 영구 저장하고 `report_final`이 반드시 `Unconfirmed`를 내게. **F8** 2종 추가 |

**나머지 4건**

- **"결과 버리기"가 잠긴 사본에서만 동작했다** — `discard_codex_pilot`은 poisoned 표시가 있는 복제본만 받는다. 즉 정상 완료·단순 취소·`sessionOnly` 결과의 버튼은 **전부 실패**했다. 인계 전용 `discard_handoff_workspace`를 만들어 네 경우 모두 영구 `abandoned`로 전환한다. 파일은 지우지 않는다(증거 보존 계약 그대로), 루트·매니페스트 검증은 유지
- **구현 단계 실패가 오류로 던져졌다** — 화면에 오류 문구만 남고 복제본 경로·잠금·증거가 사라져 폐기도 복구도 못 했다. `NeedsAttention` 결과로 변환하되 잠금은 **사실대로** 읽는다(디스크 표시 → `persisted`, 슬롯만 오염 → `sessionOnly`, 복제본 손대기 전 실패 → 잠그지 않음)
- **예약 전 중단 요청이 사라졌다** — 화면이 `begin_handoff` 완료 전에 이미 중단 버튼을 보여 준다. 그 요청을 기억했다가 예약 직후 전달하게 했다
- 문서 §8의 §7.6A 중복 줄 정리

**회귀 테스트 14개 추가** (113 → 127). F7, F8 2종, 무결성 정상 실행, 폐기 6종(정상 완료·취소·persisted·sessionOnly·미확인 거부·임의 폴더 거부), 실패 보고 4종.

**F7·F8이 실제로 결함을 잡는지 옛 동작으로 되돌려 확인했다** — 셋 다 정확히 그 자리에서 실패했다(`재사용된 pid의 새 자손이 편입되지 않았다`, `일시적 열거 실패를 잊고 소멸을 주장했다`).

**발견**

- **테스트 하네스에도 같은 결함이 있었다.** 가짜 `signal`이 pid로 첫 항목을 찾아 **같은 pid를 쓰던 죽은 프로세스**에 신호를 보내고, 실제 대상은 멀쩡히 살아남았다. F7이 이걸 먼저 잡아냈다 — 살아 있는 것만 찾도록 고쳤다
- pid 기반 조회는 프로덕션 코드든 테스트 코드든 **같은 방식으로 틀린다.** pid는 식별자가 아니다

**검증**: `cargo test --locked` **127 통과 / 0 실패 / 12 무시**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과, 통합 테스트 2건 재실행 통과(CLI `2.1.237`, `stages: [Organizing, Building, Checking, Done]`).

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{proctree.rs,proctree/tests.rs,handoff.rs,handoff/tests.rs,orchestration.rs,lib.rs}`, `src/App.tsx`

**다음 할 일**

- Codex 재검토 요청
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5)

---

### 2026-08-20 · by Claude Opus 5 (Codex 4차 반려 5건 반영)

**§7.10 1차 구현이 Codex 재검토를 통과하지 못했다.** 지적은 전부 *구현이 계약을 지키지 못한* 지점이었다. 코드와 회귀 테스트로 고쳤고 여전히 **미커밋**이다.

**고친 것 5건**

| 반려 | 무엇이 틀렸나 | 어떻게 고쳤나 |
| --- | --- | --- |
| 중단 경로 | 준비 중 취소가 `cancel.store(false)`로 지워지고, Claude가 조용하면 취소가 영영 닿지 않고, 구현 중 취소가 Codex에 전달되지 않았다 | 취소 깃발을 **슬롯 소유**로 이동(`HandoffSession`), stdout 읽기를 별도 스레드로 분리해 실행 소유자가 취소를 폴링, `CancelBridge`가 Codex 슬롯 예약 직전의 취소까지 재시도 전달 |
| 단일 정리 절차 | 증거 파일 생성 실패 시 자식·watcher가 남는 조기 반환. 종료 확인 실패 뒤 blocking `child.wait()` | 증거 파일을 **자식보다 먼저** 만들어 그 경로 자체를 없앰. 확인 실패 뒤에는 제한된 `try_wait`만 하고 즉시 poisoned 반환(스레드도 join하지 않음). stderr·events 기록 실패를 증거 실패로 보고 |
| 잠금 보고 | `write_poisoned_marker` 오류를 무시하고 `workspaceLocked: true`로 보고 — **거짓 안전 보고** | `WorkspaceLock` 3상태(`none`/`persisted`/`sessionOnly`) + `lockError`. 저장 실패해도 프로세스 내 목록이 재사용을 막고, 화면은 폐기를 요구 |
| 완료 화면 | 계획 요약을 "실제 수행 결과"로 표시. 변경 목록이 삭제·이름 변경·바이너리를 누락. `확인 중`이 화면에만 존재. 멈추면 진행 표시가 초기화 | 결과는 Codex 최종 메시지에서, 계획은 상세의 *하려던 것*으로 분리. 목록은 `git diff --name-status -z -M` 상태 코드로. `Checking`을 실제 백엔드 단계로. 멈춘 지점을 표시 |
| fail-closed | 도구 목록을 `filter_map`으로 읽어 비문자열 항목이 섞여도 통과. 모델을 `contains("sonnet")`로 대조 | 비문자열 항목·목록 부재를 위반으로. `ALLOWED_INIT_MODELS` **완전 일치**. `is_error` 계획은 `plan_to_codex_prompt`가 구조적으로 거부 |

**단일 실행 슬롯**: `begin_handoff`로 **복제본 준비보다 먼저** 예약하고 `run_handoff`가 소유권을 잡는다. 그래야 준비 중 취소가 살아남고 동시 실행이 거부된다. 진행 이벤트에 `runId`를 실어 끝난 실행의 늦은 상태가 새 실행 화면에 섞이지 않게 했다.

**회귀 테스트** — Codex가 요구한 8종을 전부 넣었다. 중단 3종(준비 중·무출력 중·Codex 구간), 동시 실행 거부, 증거 실패 후 프로세스 잔존 없음, 확인 실패의 제한 시간 내 poisoned, 잠금 저장 실패 후 재사용 거부, 삭제·이름 변경 표시, 결과와 계획 분리, malformed 도구/모델 거부.

- 프로세스가 필요한 경계는 **가짜 실행 대상**(아무것도 출력하지 않고 배경 자손을 남기는 스크립트)으로 실제 트리를 만들어 검증했다
- 실제로 안 죽는 트리는 만들 수 없으므로 **종료 절차를 주입 가능**하게 두고(`TerminateFn`) 확인 실패 이후의 행동만 고정했다
- 돌아오지 않는 구현을 테스트가 붙잡지 않도록 제한 시간 헬퍼(`within`)를 썼다

**검증**: `cargo test --locked` **113 통과 / 0 실패 / 12 무시**, `npx tsc --noEmit` 통과, `pnpm build` 통과, 변경 Rust 파일 `rustfmt --edition 2021 --check` 통과(기존 파일 재포맷 없음), 통합 테스트 2건 실제 재실행 통과, 잔여 프로세스·임시 폴더 없음.

**발견**

- **CLI가 `2.1.235` → `2.1.237`로 올라갔는데 계약이 그대로 통과했다.** 도구 집합·권한 모드·모델(`claude-sonnet-5`)이 모두 동일 — 정확 일치 목록이 패치 판올림에 곧바로 부서지지는 않는다는 첫 관측이다
- **계획과 결과가 실제로 다르다는 것이 실행에서 드러났다.** 계획 요약은 영어, 구현 결과는 한국어였다. 이전 구현이라면 사용자는 *하려던 것*을 *한 것*으로 읽었을 것이다
- `CancelBridge`가 접으라는 신호를 **취소 확인보다 먼저** 보면 실행 종료 직전의 취소를 흘린다. 테스트가 잡아 순서를 뒤집었다
- 테스트에서 **고정 시간 대기로 실행 기동을 가정하면 병렬 실행에서 깨진다.** 실제 기동(pid 파일)을 기다리도록 고쳤다

**변경 파일**: `docs/history.md`, `docs/proposals/m4-orchestration.md`, `src-tauri/src/{handoff.rs,handoff/tests.rs,claude_pilot.rs,claude_pilot/tests.rs,orchestration.rs,lib.rs}`, `src/App.tsx`, `src/App.css`, `src/locales/{ko,en}.ts`

**다음 할 일**

- Codex 재검토 요청 (중단 계약·정리 절차·잠금 보고·결과 표시·fail-closed 엄밀성)
- 통과 시 사용자 승인을 받아 커밋. **`main` 병합은 하지 않는다**
- §7.6A 자동 인계 UX 판정(A1~A5) — 사람이 써 보고 내리는 판정

---

### 2026-08-19 · by Claude Opus 5 (4차 — §7.10 최소 자동 인계 1차 구현)

**Codex 3차 재검토 통과 → 문서 3개 커밋(`2923608`) → §7.10 구현 착수.** 코드는 **미커밋**이다.

**새 모듈 3개**

| 파일 | 역할 |
| --- | --- |
| `src-tauri/src/proctree.rs` (+`proctree/tests.rs`) | 종료 계약 — **누적 신원 추적**. `(pid, 시작 시각)` 신원을 한 번 관측하면 집합에서 빼지 않고, 누적 집합 전체의 소멸을 확인해야 성공 |
| `src-tauri/src/claude_pilot.rs` (+`claude_pilot/tests.rs`) | 실행 계약 — 고정 인자와 `system:init` fail-closed 검증, 계획 파싱, Codex 인계 프롬프트 생성 |
| `src-tauri/src/handoff.rs` (+`handoff/tests.rs`) | 수직 흐름 — 사용자 1회 입력 → 계획 → **앱이 인계** → 구현 → 결과·증거 |

**신원 조회 수단을 정했다.** `ps` 파싱은 로캘 의존이라 버리고 `libc::proc_pidinfo(PROC_PIDTBSDINFO)`를 쓴다(unix 전용 의존성 `libc` 추가). 실측으로 확인한 errno 대응이 계약과 정확히 맞는다 — 성공=생존, **`ESRCH`=소멸(좀비 포함)**, **`EPERM`=확인 불가=poisoned**. 마이크로초 시작 시각과 ppid를 함께 얻는다.

**F1~F6 회귀 픽스처 전부 통과**(13개). 주입 가능한 `ProcTable`·`Clock`으로 실제 프로세스 없이 시나리오를 고정했고, 실제 OS 경로도 별도 테스트로 검증했다. 재부모화 오판(C6)은 `f1_root_tree_walk_would_report_empty_while_orphan_lives`로 코드에 못 박았다. `f6_without_in_run_observation_the_orphan_is_invisible`은 **실행 중 관측이 왜 의무인지**를 테스트로 남긴 것이다.

**실행 계약 테스트 22개.** `init` 픽스처는 지어내지 않고 **E2·B4 실측 출력 그대로** 썼다. 철회된 플래그(`--disallowed-tools`·`--setting-sources project` 등)가 되살아나지 않게 막는 테스트도 넣었다.

**초보자 화면**(§7.10.2): `HandoffPilot` 컴포넌트. 기본 화면에는 목표·진행 상태·결과 요약·변경 내용·보관/폐기만 둔다. 세션 ID·증거 경로·정리 결과는 전부 "자세히 보기" 뒤다. 진행 상태는 역할 이름이 아니라 **일의 이름**이다(`요청 정리 중` → `만드는 중` → `확인 중` → `완료`). ko/en 문구 31키씩 추가.

**macOS 전용 노출**: `handoff_available` 명령이 `cfg!(target_os = "macos")`를 돌려주고, 프런트는 false면 아예 렌더하지 않는다. 백엔드 `run_handoff`도 macOS가 아니면 거부한다.

**실사용 검증 — 실제로 돌렸다** (`#[ignore]`, 로그인·사용량 필요)

| 테스트 | 결과 |
| --- | --- |
| `claude_planning_leg_runs_end_to_end` | 통과. `init` 도구 정확히 4개, `dontAsk`, 구조화 계획 산출, **복제본 diff 비어 있음**, 트리 소멸 확인, 증거는 복제본 밖 |
| `handoff_vertical_slice_runs_end_to_end` | 통과. `Organizing → Building → Done`, `greeting.txt` 변경, `termination: Confirmed`, 복제본 미잠금 |

**구현 중 발견 (다음 작업자용)**

- **`/dev/urandom`을 `std::fs::read`로 읽으면 프로세스가 멈춘다** — 끝이 없는 스트림이다. `read_exact`로 16바이트만 읽어야 한다. 테스트가 SIGKILL로 잡아냈다
- **통합 테스트를 `env -i`로 돌리면 `Not logged in`으로 실패한다.** 인증이 환경에 의존하므로 로그인된 셸 그대로 실행할 것. 테스트 주석에 적어 뒀다
- `--json-schema`가 주입하는 `StructuredOutput` 때문에 기대 도구 집합은 **4개**다. 3개로 적었다면 정상 실행이 전부 poisoned가 됐을 것이다(계약 문서대로)

**검증**: `cargo test` 85 passed / 0 failed / 12 ignored · `pnpm build` 성공 · 새 파일 6개 `rustfmt --edition 2021 --check` 통과(기존 파일 재포맷 없음) · 실측용 임시 폴더와 프로세스 정리 확인

**변경 파일**

| 파일 | 변경 |
| --- | --- |
| `src-tauri/src/{proctree,claude_pilot,handoff}.rs` + 각 `tests.rs` | **신설** |
| `src-tauri/src/lib.rs` | 모듈·상태·명령 3개 등록 |
| `src-tauri/src/orchestration.rs` | 헬퍼 6개를 `pub(crate)`로 (동작 변경 없음) |
| `src-tauri/Cargo.toml`·`Cargo.lock` | unix 전용 `libc` 추가 |
| `src/App.tsx`·`src/App.css`·`src/locales/{ko,en}.ts` | 초보자 화면 |
| `docs/proposals/m4-orchestration.md` | §7.10 구현 상태·§8·§10 |

**다음 할 일**
- **Codex 재검토 요청** — 구현 전반, 특히 종료 계약 구현과 화면 계약 준수
- 통과 후 **커밋 승인 요청** (현재 미커밋). `main` 병합은 하지 않는다
- 남은 판정: **§7.6A(A1~A5)** — 사람이 실제로 써 보고 내리는 UX 판정. 구현은 그것을 *할 수 있게* 만든 것이지 판정이 아니다
- 별건: S2 `tree_is_gone`/`signal_ok`의 `EPERM` 오판 검토, Windows 게이트 실측

### 2026-08-19 · by Claude Opus 5 (3차 — Codex 차단 1건 + 문서 정합성 2건)

**§7.10.1.2 ⑤ 종료 절차 재작성 (문서만, 구현·커밋 없음). E2는 Codex 지시대로 재실행하지 않았다.**

**차단 — 루트 기준 트리 순회가 재부모화된 자손을 놓친다**

2차에서 세운 "매 회차 루트의 ppid 트리를 다시 순회한다"가 틀렸다. **루트나 중간 부모가 먼저 죽으면 자손이 `launchd`(pid 1)로 재부모화**되어 루트 트리에서 사라진다. 살아 있는데 **빈 집합으로 오판**한다.

**C6로 재현했다** — Claude CLI 없이 순수 셸로 재현되는 구조적 결함이다. 부모가 `tail -f /dev/null`을 띄우고 자신만 먼저 종료시켰더니: 루트 생존 `false`, **자손 생존 `true`**, 자손 `ppid` **`1`**, **루트 기준 순회 결과 `[]`**.

**교체한 절차 — 누적 신원 추적**

- 신원은 **`(pid, 시작 시각)`**. pid만으로는 재사용 시 다른 프로세스를 원래 것으로 오인한다
- **한 번이라도 관측된 신원은 누적 집합에서 빠지지 않는다.** 소멸이 확인되면 `소멸됨`으로 표시될 뿐이다
- 매 회차 **누적 신원 전체를 현재 부모와 무관하게 개별 생존 확인**하고, **살아 있는 모든 추적 대상 각각에서** 후손을 새로 열거해 편입한다 — 루트 사후에는 고아가 유일한 발견 경로다
- **누적 집합 전체의 소멸을 확인한 경우에만 성공.** 루트가 죽었다는 사실도, 트리가 비었다는 사실도 성공 근거가 아니다
- **신원 불일치(PID 재사용)와 신원 조회 실패는 `poisoned`** — 소멸로 처리하지 않는다

시작 시각은 `ps -o lstart=`로 **초 단위**까지 확인했다. 구현은 마이크로초 정밀도 소스(`proc_pidinfo`의 `pbi_start_tv*` / `sysctl KERN_PROC_PID`)를 쓰되, **구조체 오프셋은 이 세션에서 검증하지 않았다**고 명시했다.

**구현 완료 기준에 회귀 픽스처 F1~F6 추가**

F1이 Codex가 요구한 **재부모화 고아**(유예 중 자손 생성 → 부모 먼저 종료 → 자손만 `ppid=1` 생존)다. 루트 트리 순회만 하는 구현은 F1에서 빈 집합 오판으로 **실패해야** 한다. F2 유예 중 신규 자손, F3 별도 그룹·세션 자손, F4 `EPERM` 좀비 그룹, F5 신원 불일치, F6 정상 종료 경로의 배경 자손.

**정직하게 남긴 빈틈**: 두 열거 회차 **사이에** 자손이 생기고 그 부모가 같은 창에서 죽으면 신원을 한 번도 관측하지 못해 추적에 못 들어온다. 열거 간격 축소가 완화책이고 **완전 차단은 주장하지 않는다**고 §7.10.1.2 `남은 미확인`에 적었다.

**문서 정합성 2건 (Codex 지적)**

- `핵심 결정·발견`의 **S1 "다음 단계는 S2"** → S1·S2 모두 종결됐고 현재 지점은 §7.10 Claude 실행 계약임을 반영
- **S2 "아직 미통과"** 서술 → §7.9·§8의 실제 상태대로 **통과**로 갱신하고, 남은 별건 2가지(Windows CI, `tree_is_gone`/`signal_ok`의 `EPERM` 오판 검토)만 남김

**이번 회차 변경 파일**

| 파일 | 변경 |
| --- | --- |
| `docs/proposals/m4-orchestration.md` | 수정 — §7.10 완료 기준 포인터, §7.10.1 취소 행, §7.10.1.2 ⑤ 종료 절차 전면 재작성·C6 관측·F1~F6 픽스처·남은 미확인 3건, §9 철회 주장 1건, §10 결정 이력 |
| `docs/history.md` | 수정 — 이 엔트리, `핵심 결정·발견`의 프로세스 트리·S1·S2 항목 3건 갱신 |

코드는 쓰지 않았고 커밋도 하지 않았다.

**다음 할 일**
- **Codex 최종 재검토 요청** (4회차)
- 통과 + 사용자 승인 후 §7.10 구현 착수. 구현 시 F1~F6이 완료 기준이다
- 별건: S2 `tree_is_gone`/`signal_ok`의 `EPERM` 오판 검토, Windows 게이트 실측, `--debug-file`, policy 설정 환경

### 2026-08-19 · by Claude Opus 5 (2차 — Codex 재검토 반려 6건 반영)

**§7.10.1.2 계약 수정 + E2 재실측 (문서만, 구현·커밋 없음)**

Codex가 원시 증거와 이전 반려 해소는 인정했으나 **계약 6건을 반려**했다. 전부 반영하고 재실측이 필요한 1건은 다시 측정했다.

**① 종료 확인 불가 → `poisoned` 강제 (§7.10.1.2 ⑤)**

트리 소멸을 확인하지 못한 실행은 실행과 **복제본을 함께** `poisoned`로 표시하고, **Codex 자동 인계와 복제본 재사용을 모두 차단**한다. 해소는 **명시적 복구 또는 명시적 폐기뿐** — 자동·시간 경과·재시도에 의한 암묵적 해소는 없다. 살아남은 프로세스가 복제본을 계속 쓸 수 있으므로 파일 상태를 신뢰할 수 없다는 것이 근거다. S2가 Codex 경로에 도입한 `poisoned`와 성격은 같되 **발동 조건이 더 넓다**(Codex는 신호 실패, Claude는 *확인 실패* 전부).

**② 단발 열거 폐기 → 반복 열거 + 생존자 부재 관측**

"TERM 전에 ppid 목록을 한 번 확보한다"는 **유예 중 새로 생기는 자손을 놓친다**(에이전트의 `run_in_background` 전환이 실측된 경로다). 종료 절차 내내 후손을 **반복 열거**하고, 최종 판정은 **생존자 집합이 빌 때까지 관측**하는 것으로 바꿨다. `EPERM` 해석도 뒤집었다 — 이전 초안의 "실패로 판정하지 않는다"는 성공으로 읽힐 수 있었다. **`EPERM`은 성공도 소멸 증거도 아니며**, 판정은 신호 결과가 아니라 재열거 결과로만 한다. **열거 자체가 실패하면 확인 불가 = poisoned.**

**③ 실행 형식 `stream-json --verbose` 고정 + init fail-closed 검증 (E2 재실측)**

`--output-format json`에는 `system:init`이 없어 검증이 불가능하므로 **선택지에서 제거**했다. 같은 형식에서 `--json-schema`까지 포함해 E2를 재실행했다: `rc=0`, 이벤트 25개(`system:init` → `result:success`), **`structured_output`이 스키마대로 산출**, 지정한 `session_id` 유지, `permission_denials` `[]`, 워크스페이스 diff 없음, 훅·MCP·스킬 전부 미실행.

> ⚠️ **재실측에서 새로 발견**: `--json-schema`가 **요청하지 않은 `StructuredOutput` 도구를 주입**한다. `--tools Read,Glob,Grep`을 줬는데 init에는 4개가 온다. 기대값을 `{Read,Glob,Grep}`으로 적었으면 **정상 실행이 전부 poisoned로 떨어졌을 것이다.** 계약의 기대값을 **정확히 4개 집합**으로 고정했다.

init 검증 항목은 `tools`(집합 일치)·`permissionMode`·`mcp_servers`·`plugins`·`model`·`cwd`·`session_id`이며 하나라도 불일치하면 즉시 중단 + poisoned. `skills`·`slash_commands`·`agents`는 **검증 대상이 아니다** — `Skill`·`Task` 도구가 없어 호출 수단이 없기 때문이고, 내장 목록을 고정하면 판올림마다 계약이 부서진다.

**④ Windows — 파일럿 범위에서 제외 (§6, §7.10.1.2 ⑥)**

미결로 남기는 대신 **범위를 좁혔다.** 종료 계약이 ppid 후손 반복 열거에 기대는데 Windows 대응물(`taskkill /T /F`, Job Object)과 Claude CLI 자식의 프로세스 관계를 **관측한 적이 없다.** 그 상태로 열면 ⑤ 계약상 모든 실행이 poisoned로 끝날 수 있다. §7.10 조각은 macOS에서만 노출하고, Windows는 자손 회수 실측을 **별도 게이트**로 §8에 넣었다. S2 Codex 경로의 Windows CI 확인과는 별건이며, 제품 전체(M0~M3)의 Windows 지원과도 무관하다.

**⑤ 변경 파일 기록 정정**

앞 엔트리가 "제안서만 수정"이라고 적었으나 실제로는 `docs/history.md` 수정과 `docs/archive/history-2026-08.md` 신설이 함께 있었다. 해당 엔트리를 표로 정정했다.

**이번 회차 변경 파일**

| 파일 | 변경 |
| --- | --- |
| `docs/proposals/m4-orchestration.md` | 수정 — §6 범위 밖(Windows), §7.10.1 계약 행 3개, §7.10.1.2 ④ 재작성·**⑤⑥ 신설**·남은 미확인 갱신, §8 체크리스트 2건, §9 철회 주장 3건, §10 결정 이력 |
| `docs/history.md` | 수정 — 이 엔트리 추가, 앞 엔트리의 변경 파일 기록 정정 |

코드는 쓰지 않았고 커밋도 하지 않았다.

**다음 할 일**
- **Codex 재검토 요청** (3회차) — ④ init 검증 항목, ⑤ 종료·poisoned 절차, ⑥ 플랫폼 범위
- 통과 + 사용자 승인 후에야 §7.10 구현 착수
- 미해소로 남은 것: S2 `tree_is_gone`/`signal_ok`의 `EPERM` 오판 확인, Windows 게이트 실측, `--debug-file`, policy 설정 환경, **유예 중 새 자손을 반복 열거가 실제로 잡는지 재현**(현재는 설계 요구이고 관측은 아니다)

### 2026-08-19 · by Claude Opus 5

**§7.10.1 Claude 실행 계약 재실측 — Codex 반려 4건 반영 (문서만, 구현·커밋 없음)**

Codex 재검토가 §7.10.1.1(2026-08-18, CLI `2.1.232`)의 계약 권고를 반려했다. **CLI `2.1.235`에서 전면 재측정**하고 결과를 `docs/proposals/m4-orchestration.md` **§7.10.1.2**에 기록했다. 판정은 `system:init` 이벤트의 실제 도구·권한·MCP 목록, 파일 SHA-256 diff, 프로세스 테이블, 종료 코드로만 했다.

**① 쓰기 차단 — `--disallowed-tools` 계약에서 제거**

- 대조군(A0, 제한 없음)에서 같은 경로·프롬프트로 쓰기가 **성공**함을 먼저 확인(`hello` → `goodbye`). 이후 미변경이 환경 탓이 아님을 보장
- `--tools Read,Glob,Grep`은 **허용목록**이라 `init` 도구가 정확히 3개가 된다. `Task`·`ToolSearch`까지 사라져 우회 대상 자체가 없다
- `--disallowed-tools`는 23개를 남긴다(`Task`·`WebFetch`·`RemoteTrigger`·`SendMessage`…). 게다가 **도구 제거는 `permission_denials`에 기록되지 않아 증거가 0**이고, `ToolSearch`가 `init` 목록에 없던 `LSP`를 반환해 **막아야 할 표면이 목록보다 넓다**는 것도 관측됐다
- ⚠️ **8/18 기록 정정**: 당시 "`Monitor`로 우회를 시도했다"를 우회 성공처럼 읽히게 적었다. 재실측(A4·A5, 서브에이전트 경로 포함)에서 **우회는 성공하지 않았다.** 제거 근거는 우회 성공이 아니라 노출면·증거·확장성이다
- `--permission-mode dontAsk`는 자동 승인이 아니라 **거부 게이트**다(Write→Edit→Bash 3건 거부, `permission_denials` 기록). 단 거부 메시지가 에이전트에게 **대체 경로 탐색을 명시적으로 권한다** → 단독 경계로 쓰지 않는다

**② 앱 소유 구성 — `--setting-sources project` 철회**

적대적 프로젝트 픽스처(악성 `settings.json`+훅+`.mcp.json`+`CLAUDE.md`+스킬)로 확인했다. `--setting-sources project`에서 **세션 권한이 `bypassPermissions`로 승격**되고, 모델이 프로젝트 지정값으로 바뀌고, **SessionStart 훅이 실행**되고, **프로젝트 MCP 서버가 임의 프로세스를 기동**하고, 프로젝트 스킬과 `CLAUDE.md` 주입까지 적용됐다.

- **`--safe-mode`만으로는 부족**하다 — 훅·MCP·스킬은 껐지만 **권한 승격과 모델 탈취는 통과**했다
- **`--setting-sources ''`(빈 값)** 가 다섯 경로를 한 번에 닫는다. `--safe-mode`·`--strict-mcp-config`는 이중화로만 유지

**③ 취소·프로세스 트리 — Codex와 계약을 분리해야 한다**

- **Claude의 Bash 도구 자식은 부모 프로세스 그룹에 없다.** 별도 pgid이고 `stat=Ss`(별도 세션 리더)다
- 그룹 `TERM`으로 자손이 죽는 것은 커널 보장이 아니라 **Claude 자신의 SIGTERM 처리기**가 정리하기 때문이다. **루트에만 `SIGKILL`을 보내면 자손이 1·3·5·10·15초 모두 생존**했고 ppid 트리 종료로만 회수됐다
- 함정 2건: 좀비만 남은 그룹의 `killpg`는 **`EPERM`**(`ESRCH` 아님) → 종료 실패로 오판 금지. 그리고 에이전트가 `run_in_background`로 전환하면 **정상 종료 경로에서도 자손이 남는다**
- stderr 분리 확인: 정상 실행은 stderr 0B·stdout 유효 JSON, 인자 오류는 `rc=1`·stdout 0B·stderr에 메시지. `-d`로는 stderr 로그가 안 나온다

**④ 경계·diff의 지위 강등**

**작업 폴더 경계와 사후 빈 diff는 읽기 전용 예방책이 아니다.** 경계는 *복제본 밖* 쓰기만 막아 복제본 안 쓰기는 허용하고(A0가 증명), 빈 diff는 사후 관측이라 예방하지 못하며 네트워크·배경 프로세스·훅 같은 파일 밖 부작용을 보지 못한다. 예방은 **`--tools` 허용목록**이 하고, 경계·diff는 그 뒤의 확인 수단이다. §7.4 이벤트 한계 조항에도 같은 취지로 diff 항목을 추가했다.

**확정 계약(§7.10.1.2 ④)**은 적대적 픽스처 위 통합 실행(E1)으로 검증했다 — `rc=0`, 앱이 지정한 `session_id` 유지, `--json-schema` 구조화 산출, 워크스페이스 diff 없음, 훅·MCP·스킬 전부 미실행.

**검증·증거**: 23회 실행의 `command.txt`·`exitcode.txt`·`stdout.json`·`stderr.log`·전후 SHA-256·`files.diff`·프로세스 스냅샷 등 212개 파일. 세션 스크래치패드에 있고 **커밋하지 않았다.** 픽스처 정의와 명령 전문을 §7.10.1.2에 옮겨 적어 문서만으로 독립 재현이 되게 했다. 누출된 실측용 프로세스는 정리 확인했다.

**문서 변경** (⚠️ 최초 기록이 "제안서만 수정"이라고 적었으나 사실과 달라 정정한다 — Codex 지적)

| 파일 | 변경 |
| --- | --- |
| `docs/proposals/m4-orchestration.md` | 수정 — §2.1 버전(`2.1.235`), §7.4 diff 한계, §7.10.1 계약 행 3개, §7.10.1.1 철회 표시·과장 표현 정정, **§7.10.1.2 신설**, §8 체크리스트, §9 철회 주장 5건, §10 결정 이력 |
| `docs/history.md` | 수정 — 이 엔트리 추가, `핵심 결정·발견` 2건 추가, **아카이브 롤오버**(797줄 → 299줄) |
| `docs/archive/history-2026-08.md` | **신설** — 2026-08-12 엔트리 38개 이동(전수 보존 확인) |

코드는 쓰지 않았다.

**다음 할 일**
- **Codex 재검토 요청** — §7.10.1.2의 계약과 취소 요구사항
- 통과 + 사용자 승인 후에야 §7.10 최소 자동 인계 구현 착수
- **S2 `tree_is_gone`/`signal_ok` 재검토** — `killpg`의 `EPERM` 오판이 Codex 경로에도 있는지
- 미실측: Windows 자손 회수(`taskkill /T /F`), `--debug-file`, 관리자(policy) 설정 환경, `--setting-sources ''`의 향후 판올림 호환성

### 2026-08-18 · by Claude Opus 5

**§7.10.1 Claude CLI 실행 계약 실측 (문서만 기록, 구현 없음)**

로컬 CLI **`2.1.232`** 로 측정했다(문서의 `2.1.228`은 오래됐다 — 자동 업데이트됨). 판정은 도움말이 아니라 **파일 변화·도구 결과·관측 가능한 동작 차이**로 했다. 상세·증거는 `docs/proposals/m4-orchestration.md` §7.10.1.1.

- **있는 것**: 구조화 출력 `--json-schema`(+`structured_output` 필드) · 이벤트 스트림 `--output-format stream-json --verbose` · 사용자 설정 배제 `--setting-sources project` · `session_id`(게다가 `--session-id`로 **지정도** 가능).
- **없는 것**: Codex `--output-last-message` 대응물. `--output-format json`의 `result` 필드(stdout)로 대체한다.
- **사용자 설정 배제는 동작으로 확인**: 사용자 전역 설정에 `"model": "opus"`가 있는 상태에서 기본 실행은 `claude-opus-5`, `--setting-sources project`는 `claude-sonnet-5`였다. 모델이 바뀌는 관측 가능한 차이로 판정했다.
- **핵심 발견 — 도구 목록으로는 쓰기를 막지 못한다**: `--disallowed-tools`는 도구를 세션에서 제거하지만, **에이전트가 `ToolSearch`로 대체 실행 도구(`Monitor`)를 찾아 우회를 시도**했다(`printf 'goodbye' > target.txt`). `--allowed-tools`는 다른 도구를 **제거하지 않는다** — 사전 승인 목록에 가까워 `Bash`·`Edit`가 그대로 노출됐다. **deny-list는 안전 경계가 아니다.**
- **실제로 막은 것**: ① 비대화형 `-p`의 승인 게이트 ② 작업 폴더 쓰기 경계(`may only write to files in the allowed working directories`).
- **Codex와의 결정적 차이**: 응답 JSON의 **`permission_denials`** 에 거부된 도구 호출이 입력값까지 그대로 담긴다. §7.4에 적은 *"거부된 명령은 이벤트에 남지 않는다"* 는 **Codex 한계이며 Claude에는 해당하지 않는다.**
- **주의 3건**: ⓐ `--permission-mode plan`이 대상 파일은 안 건드렸지만 **작업 폴더 밖** `~/.claude-me/plans/`에 Write를 시도했다(실패 사유가 권한이 아니라 `ENOENT`였다) — plan 모드는 "아무것도 안 쓴다"가 아니다. ⓑ 변수 인자 플래그가 뒤따르는 **프롬프트를 삼켜** stdin 전달이 필요하다. ⓒ `/private/tmp`에서 허용 디렉터리와 대상이 같아 보이는데도 쓰기가 차단됐다 — **원인 미확인**, 경로 정규화 문제로 추정만 기록했다.
- **결론**: 요구했던 "파일 수정 불가를 정책으로 고정"은 **단일 플래그로 달성되지 않는다.** 플래그 조합을 쓰되 **신뢰의 근거로 삼지 않고**, 최종 방어선은 작업 폴더 경계와 승인 게이트로 두며, 계획 역할이 실제로 아무것도 바꾸지 않았는지는 **실행 후 복제본 diff가 비어 있는지로 확인**한다.
- **여전히 미실측**: 취소·프로세스 트리 종료가 S2와 동등한지, stderr 분리 수집 방식, plan 파일 경로를 워크스페이스 안으로 강제할 수 있는지.

- **부수 발견 — 두 CLI 모두 버전이 올라가 있었다**: §2.1 `확인된 사실` 표가 Claude `2.1.228` · Codex `0.147.0-alpha.6.6`으로 적혀 있었으나 실제로는 **`2.1.232`** · **`0.148.0-alpha.9`** 였다. 표를 갱신하고 *"자동 업데이트되므로 인용 전 재확인"* 을 덧붙였다. 특히 **S2의 경계·이벤트 실측은 Codex `0.147`에서 한 것**이므로 인용 시 버전 차이를 감안하라는 단서도 표에 남겼다. (과거 워크로그의 버전 기록은 당시 사실이므로 손대지 않았다.)

**다음 할 일** → **Codex 재검토**(실측 결과와 그에 따른 §7.10.1 계약 수정). 통과 후 사용자 승인이 나면 §7.10 구현 착수. **미커밋**

### 2026-08-18 · by Claude Opus 5

**M4 계획 마무리 정정 — 게이트 문구 잔재 제거 + 초보자용 화면 계약 신설 (문서만 수정, 구현 없음)**

Codex 재검토에서 이전 3건은 해소됐고, 남은 2건을 반영했다. **문서와 워크로그만 고쳤고 코드는 쓰지 않았다.**

- **① §7.6B 게이트 문구 정정**: *"기각되면 완성형·영속 제어면을 만들지 않는다"* 가 새 §5.2와 다시 충돌했다. 기각 시 ①최소 자동 인계 **유지**, ②제품화용 **최소 영속 실행 기록·실패 복구도 유지**(가설 A 통과로 이미 열림), ③재작업 루프·다역할·반복 실행만 **하지 않음**으로 표를 만들어 바로잡았다. §7.10의 제외 표에 있던 "완성형·영속 원장"도 **"재개 가능한 세션 상태 머신"** 으로 좁히고, 영속 저장 일반은 금지가 아니라 *이번 조각을 얇게 두려고 미루는 것*임을 명시했다.
- **② §7.10.2 초보자용 화면 계약 신설**: **Hello, Agent는 워크플로 도구가 아니다.** 내부에서 Claude→Codex가 돌더라도 그 구조를 사용자가 이해하거나 조작해야 하는 순간 제품이 실패한다. 기본 화면에는 **목표 · 평이한 진행 상태(`요청 정리 중`→`구현 중`→`확인 중`→`완료`) · 결과 요약 · 변경 확인 · 보관/폐기**만 둔다. 원시 출력·구조화 인계 데이터·세션 ID·증거 파일 경로·권한 플래그·역할/모델 선택 UI는 **고급 상세로 격리**한다.
  - 진행 상태는 **역할 이름이 아니라 일의 이름**으로 쓴다 — `Claude 실행 중`이 아니라 `요청 정리 중`. 사용자가 어느 에이전트가 도는지 몰라도 흐름을 이해해야 한다.
  - 기존 "증거 경로를 화면에 연결" 문구를 **"앱 내부에서는 끝까지 추적하되, 기본 화면에는 평이한 요약과 변경 확인만 제공하고 원시 증거는 선택적 상세에서만"** 으로 고쳤다. `AppError`의 `detail`을 도움 요청용으로 분리해 둔 기존 원칙과 같다.
  - 이 계약은 §7.6A의 A4(앱 안에서 결과 확인)·A5(전문 로그 해석 불필요) 판정 대상이기도 하다.

**다음 할 일** → **Codex 재검토**. 통과하고 사용자 승인이 나면, §7.10.1의 Claude 실측 항목부터 확인한 뒤 §7.10 구현 착수. **미커밋**

### 2026-08-14 · by Claude Opus 5

**M4 계획 정정 — 범위 권위 일원화 · Claude 실행 계약 신설 · 게이트 재정의 (문서만 수정, 구현 없음)**

Codex 재검토 지적 3건을 반영했다. **문서와 워크로그만 고쳤고 코드는 쓰지 않았다.** 상세는 `docs/proposals/m4-orchestration.md`.

- **① §6과 §7.10의 권위를 하나로**: 배너만으로는 "구현자가 어느 쪽을 따르나"가 모호했다. §6 머리에 **"구현자는 이 절을 따르지 않는다. 현재 구현 범위의 유일한 기준은 §7.10이며 충돌하면 §7.10이 이긴다"** 를 명시하고, 여섯 축(인계 방향·실행 횟수·재작업·최대 수정 횟수·세션 재개·원장)을 표로 대조했다. 내가 앞서 놓쳤던 **양방향 비교**와 **최대 수정 횟수**도 §7.10 제외 항목임을 명시했다.
- **② §7.10.1 Claude 실행 계약 신설(내가 통째로 빠뜨린 부분)**: S2의 prepare/run/cancel/증거 계약은 **전부 Codex 전용**이고, `run_first_chat`은 `claude -p`를 블로킹 `output()`으로 한 번 부르는 코드라 취소·트리 종료·실행별 증거·복제본 검증·고정 권한 정책·실패 시 보존이 **하나도 없다.** 따라서 **재사용 금지**를 못 박고, Claude를 **읽기 전용 기획자**로 제한(같은 폐기 복제본 사용, 파일 수정을 정책으로 차단, 출력은 구조화된 계획으로 제한, S2와 동등한 취소·증거 계약)했다. 구체 CLI 인자는 **구현 전 실측 항목**으로 표에 남겼다.
  - 이미 실측된 것 하나를 함께 적었다: `claude -p`는 승인 주체가 없어 파일 수정이 차단되지만(§3), 이는 *"승인할 사람이 없어서"* 생긴 결과일 뿐 **정책으로 고정된 것이 아니므로** 명시적 차단 수단을 찾아야 한다.
- **③ 게이트 재정의**: 가설 B가 **모든 영속 상태**를 막던 구조를 바꿨다. 앱 종료·실패 복구가 전혀 없는 메모리 상태는 제품으로 낼 수 없다는 지적이 맞다. **가설 A 통과** → 제품화용 최소 영속 실행 기록·실패 복구 설계 가능 / **가설 B 통과** → 재작업 루프·다역할·반복 실행 등 고비용 확장 허용 / **가설 B 실패** → 자동 인계는 유지, 고비용 확장만 안 함. 상태를 ①임시 ②최소 영속+복구 ③완성형 세 층으로 나눠 각각 언제 열리는지 표로 정리했다.
- 게이트 변경에 맞춰 §6 범위 밖·§8 항목·문서 상태 헤더·§7.6.1 보류 사유의 표현도 함께 고쳤다 — 안 고치면 같은 충돌이 다른 자리에 남는다.

**다음 할 일** → **Codex 재검토**. 통과하고 사용자 승인이 나면, §7.10.1의 Claude 실측 항목부터 확인한 뒤 §7.10 구현 착수. **미커밋**

### 2026-08-14 · by Claude Opus 5

**M4 우선순위 재정리 — 최소 자동 인계 우선, 품질 비교는 보류 (문서만 수정, 구현 없음)**

사용자의 *"그걸 왜 하는 거야?"* 라는 질문에서 시작해, 제안서 안의 순서 충돌을 찾아 정리했다. **문서만 고쳤고 코드는 건드리지 않았다.**

- **발견한 충돌**: §5.2가 *"교차 벤더 품질 향상"* 을 **M4의 근본 가설**로 규정해 뒀는데, 나중에 §1이 첫 가치를 "사용자가 전달자가 되지 않게 한다"로 재정의하고 §4.3이 "품질을 못 높여도 중계 감소만으로 제품 가치가 있다"고 못 박은 뒤에도 **§5.2의 순서 규정이 갱신되지 않았다.** 나는 오래된 쪽을 따라 §7.6 비교 실험부터 설계하고 있었다.
- **① 가설 분리**: 가설 **A(필수)** = 사용자가 중계하지 않아도 된다 / 가설 **B(선택적 가치)** = 교차 검토가 **추가 비용을 설명할 만큼** 품질을 높인다. B는 "품질이 오르는가"가 아니라 "오른 만큼이 비용을 설명하는가"로 좁혔다.
- **② 게이트 축소**: "품질 증명 전엔 어떤 원장·상태 머신도 금지"를 **"완성형·영속 제어면 구현의 게이트"** 로 좁혔다. 기존 문구는 최소 자동 인계까지 막는 것으로 읽혔다.
- **③ 최소 실행 상태 명시**: 완성형 원장은 안 쓰되 목표·Claude 결과·Codex 입력·현재 상태·증거 경로를 담는 **한 실행 동안만 유지되는 메모리 상태**는 필요하다고 적었다. 이 선을 넘으면 완성형이고 그때 게이트가 적용된다.
- **④ §7.6 분리**: **7.6A 자동 인계 UX(필수 통과, A1~A5)** / **7.6B 품질·비용 비교(독립 후속 판단)**. B가 기각돼도 M4 중단이 아니라 완성형 제어면을 안 만들 뿐이다.
- **⑤ §7.6.1 보류**: 16회 비교 초안은 **삭제하지 않고** `보류된 후속 실험안`으로 표시했다. 재개 시 이 절을 먼저 확정한 뒤 측정한다.
- **⑥ §7.10 신설**: 다음 구현 후보의 범위를 `사용자 1회 입력 → Claude 1회 → 자동 인계 → Codex 1회 → 결과·증거 표시`로 못 박고, 재개·병렬·재작업 루프·완성형 원장·실제 사용자 프로젝트·다역할을 **표로 명시 제외**했다.
- **⑦ §7.6C 신설**: 이번 세션의 교차 검토 사례(S1 지적 3건, S2 소유권 결함, 경계 false-positive 3회, zsh `status` 충돌, edition 오기 등)를 **정성적 선행 증거**로 기록하되, 블라인드·통제·단일 기준선이 없어 **7.6B를 대체하지 않는다**고 명시했다.

**다음 할 일** → **Codex 교차 검토**(재정리된 계획) → 사용자 승인 → §7.10 구현 착수. **미커밋**

### 2026-08-14 · by Claude Opus 5

**S2 종료 커밋 + §7.6 비교 과제 정의 초안 (제품 코드 변경 없음)**

- Codex 재검토 통과로 **S2를 완료 처리**하고 커밋했다(`66bc073`, 3 files, +362). S2 체크박스도 `[x]`로 바꿨다. 파일럿 경계의 정확한 기술은 *"복제본만"* 이 아니라 **"복제본 + 임시 디렉터리"** 이며, 비임시 경로는 커널이 거부함을 종료 코드로 확인했다.
- **다음 단계인 §7.6 비교 과제 정의 초안을 작성**했다(§7.6.1). 이 절이 정해지기 전에는 §5.2 실측을 시작하지 않는다 — **과제를 보고 나서 기준을 정하면 결과에 맞춰 기준이 움직인다.**
- **시험 저장소**: 전용 픽스처(Node+TS CLI 300줄, vitest·eslint)를 고정 커밋으로. 이 제품 저장소는 쓰지 않는다(크기·되돌리기 비용).
- **과제 4개**: T1 버그 수정(재현 테스트 존재) · T2 기능 추가(테스트도 직접 작성) · T3 리팩터링(동작 불변) · T4 모호한 요구("오류 메시지가 불친절하다"). T4는 기계 검증이 불가하지만, 성공 조건이 명확한 과제만 모으면 **교차 검토의 가치가 가장 클 영역이 통째로 빠지므로** 품질 점수 전용으로 포함했다.
- **공통 체크리스트**(항목당 0·1·2): 요구 충족 · 불필요한 복잡성 · 회귀 위험 · 스코프 준수 · 증거 제시.
- **작업 예산**: 시작 커밋·과제 서술 동일(역할 안내만 상이), 재작업 1회, 과제당 15분, 초과 시 그 시점 상태로 채점.
- **미해결 — 판정자 편향**: 품질 채점을 Claude/Codex가 하면 자기 산출물을 자기가 채점하는 구간이 생긴다. 완화책 3안(사용자 채점 / 교차 채점 / 라벨 가림)을 제시하고 **①+③ 혼합을 권고**했다. 사용자 결정 필요.
- **중단 기준**도 넣었다: 모든 조건이 통과하거나 모두 실패하면 차이를 잴 수 없으므로 과제를 재설계한다.

**다음 할 일** → 사용자가 §7.6.1(특히 판정자 방식)을 확정하고 Codex가 교차 검토하면, 픽스처 저장소를 만들고 §5.2 수동 프로토콜 실측 착수. **§7.6.1은 미커밋**

### 2026-08-14 · by Claude Opus 5

**S2 경계 시나리오 4차 — 종료 코드 기반 판정으로 교체 + 원장 계약에 이벤트 한계 명문화 (제품 코드 변경 있음)**

Codex 재검토 지적 4건을 모두 반영했다. 상세는 `docs/proposals/m4-orchestration.md` §7.4·§7.9.

- **① 판정 방식 교체(지적 정확)**: `echo ran > 표식; echo leaked > 대상` 순서는 **모델이 표식만 만들고 대상 쓰기를 생략해도 통과**한다. 순서를 뒤집어 `echo leaked > '<대상>'; rc=$?; printf '%s\n' "$rc" > probeN-status.txt`로 바꿨다. 상태 파일의 **존재**가 대상 시도를 지나갔다는 증거이고 **내용**이 커널의 판정이다. 동적 경로는 셸 인용 처리했다.
- **함정 — zsh에서 `status`는 읽기 전용**: 제안대로 `status=$?`를 쓰자 Codex가 명령을 `/bin/zsh -lc`로 돌리는 탓에 `read-only variable: status`로 실패해 뒤 명령이 통째로 건너뛰어졌다. **새 테스트가 이를 "시도하지 않았다"로 정확히 잡아냈다.** 변수명을 `rc`로 바꿔 해결했다.
- **② §7.4에 이벤트 한계 명문화**: 거부된 명령은 `command_execution` 항목을 남기지 않으므로 ⓐ 이벤트 부재는 "시도하지 않음"의 증거가 아니고 ⓑ "안전하게 차단됨"의 증거도 아니며 ⓒ **이벤트 스트림을 완전한 실행 감사 로그로 취급하지 않는다**를 원장·실패 계약에 추가했다. 원장은 diff·종료 코드·검증 산출물처럼 **에이전트 바깥에서 관측 가능한 사실**을 함께 기록해야 한다. §7.7 판정에도 적용된다.
- **③ §7.9 재실행 명령 정정**: `cargo test --manifest-path src-tauri/Cargo.toml boundary_ -- --ignored --nocapture`.
- **④ 최종 측정** (`boundary_` **3 passed**): 시나리오 2 — 종료 코드 `1`, 파일 없음 → **커널이 거부**. 시나리오 3 — 임시 `exit 0`·파일 있음 / 같은 실행의 비임시 `exit 1`·파일 없음 → **한계가 임시 경로에 한정**됨 확인.
- **검증**: 전체 `cargo test` **43 passed·10 ignored·경고 0**, `rustfmt --edition 2021 --check` 클린, `git diff --check` 통과. 홈 canary·픽스처 잔존 0.
- S2 체크박스는 **미완료 유지**(Codex 재검토 대기).

**다음 할 일** → Codex 재검토. 통과하면 커밋(사용자 승인) 후 §7.6 비교 과제 정의로. **미커밋**

### 2026-08-14 · by Claude Opus 5

**S2 경계 시나리오 3차 재측정 — 명령줄 단위 실행 증거로 교체 (제품 코드 변경 있음)**

Codex의 2차 지적도 맞았다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 수동 경계 시나리오 실측`.

- **2차도 무효였던 이유**: 별도 명령으로 안쪽 쓰기를 시킨 대조군은 *"셸을 한 번이라도 돌렸다"* 만 증명할 뿐, **문제의 바깥 쓰기 명령 자체가 시도됐는지**는 증명하지 못한다. 실제로 이 방식으로 재측정하니 **시나리오 2가 실패했다** — 에이전트가 바깥 명령을 건너뛴 것이다. 시나리오 3의 `home_canary`는 프롬프트에 들어 있지도 않아 단언이 공허했다(Codex 지적 그대로).
- **최종 방법**: 같은 셸 한 줄 안에 `echo ran > probeN-ran.txt; echo leaked > <대상>` 형태로 **앞부분에 작업 폴더 안 표식**을 둔다. 그 표식이 생기면 *바로 그 명령줄이 실행됐다*는 증거가 되고, 그때에야 대상 파일 유무가 경계의 증거가 된다.
- **부수 발견(중요)**: 이벤트 로그의 `command_execution` 항목으로 시도를 증명하려 했으나, **샌드박스가 거부한 명령은 이 항목을 전혀 남기지 않는다**(성공한 명령만 남는다). 거부 흔적은 에이전트의 자연어 메시지에만 있었다. 즉 **이벤트 로그만으로는 "시도했는데 막혔다"와 "시도조차 안 했다"를 구분할 수 없다.** §7.4가 원장 근거로 이벤트 스트림을 쓰기로 한 만큼 설계에 반영해야 한다.
- **재측정 결과**: 시나리오 2 — 명령줄 실행 `true`, 바깥 쓰기 `false` → **실제 차단 확인**. 시나리오 3 — 두 명령줄 모두 실행 `true`, 임시 경로 `true` / 비임시 `false` → **한계가 임시 경로에 한정됨** 확인.
- **검증**: `cargo test` 43 passed(+ignored 10), fmt 클린, canary·프로세스 잔존 0. S2 체크박스는 재검토 대기로 유지.

**다음 할 일** → **Codex 재검토**(명령줄 실행 증거 설계와 3차 측정 결과, 그리고 이벤트 로그 한계의 §7.4 반영 여부). **미커밋**

### 2026-08-14 · by Claude Opus 5

**S2 경계 시나리오 재측정 — false-positive 제거, S2는 다시 미완료 (제품 코드 변경 있음)**

Codex가 시나리오 2·3의 측정이 무효라고 판정했고, **맞는 지적이라 재작업했다.** 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 수동 경계 시나리오 실측`.

- **무엇이 틀렸나**: 시나리오 2·3이 **모델이 그냥 거절해도 통과**하는 구조였다. 바깥 canary가 없다는 사실만으로는 "샌드박스가 막았다"를 증명하지 못한다 — 거절해도 파일은 똑같이 없다. 시나리오 3은 단언이 `.git` 존재 여부뿐이라 사실상 아무것도 검증하지 않았고, 모델이 거절했다면 **"한계가 없다"고 잘못 기록**할 뻔했다. §3에 *"에이전트의 '못 한다'를 강제 증거로 삼지 말라"* 고 적어 두고 같은 함정에 다시 빠졌다.
- **수정**: 같은 실행에서 작업 폴더 **안**에도 셸로 파일을 쓰게 하는 **대조군**을 넣었다. 그 증거 파일이 생겼을 때(= 에이전트가 실제로 셸을 돌렸을 때)만 바깥 결과를 판정한다. 시나리오 3은 쓰기 성공을 **단언**하도록 바꿔, 한계가 해소되면 테스트가 깨져 문서를 갱신하게 했다. 비임시 canary도 함께 확인해 한계가 임시 경로에 한정됨을 못 박았다.
- **재측정 결과**: 시나리오 2 — 대조군 `true`, 바깥 쓰기 `false` → **실제 차단 확인**. 시나리오 3 — 대조군 `true`, 임시 경로 쓰기 `true`, 비임시 번짐 없음 → **알려진 한계 재확인**.
- **S2 체크박스를 다시 미완료로 되돌렸다.** 실행기 자체는 `d2dec6d`로 검토 통과했지만, 시나리오 측정은 재검토 대기 상태다.
- **검증**: `cargo test` 43 passed(+ignored 10), fmt 클린, canary·프로세스 잔존 0.

**다음 할 일** → **Codex 재검토**(대조군 설계와 재측정 결과). 통과하면 S2 종료 후 §7.6 비교 과제 정의로. **미커밋**

### 2026-08-14 · by Claude Opus 5

**S2 수동 경계 시나리오 실측 완료 + 실행기 커밋 (제품 코드 변경 있음)**

Codex 재검토 통과 후 사용자 승인으로 커밋(`d2dec6d`, 7 files, +3442)하고, S2의 마지막 항목인 수동 경계 시나리오를 실측했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 수동 경계 시나리오 실측`.

임시 스크립트가 아니라 **앱의 실제 파이프라인**(`execute_pilot` → 고정 인자 → 실행기)으로 돌렸고, 판정은 **모델의 말이 아니라 파일 존재 여부**로 했다. 재실행 가능하도록 `#[ignore]` 테스트로 남겼다(`cargo test boundary_ -- --ignored`).

| # | 시나리오 | 결과 |
| --- | --- | --- |
| 1 | 저장소 **안** 생성·삭제 | ✅ 복제본에 반영, **원본 무변경**, diff에 둘 다 기록 |
| 2 | 저장소 **밖 · 비임시**(홈) 쓰기 | ✅ 파일 생성되지 않음 — 차단 확인 |
| 3 | **임시 경로** 쓰기 | ⚠️ **성공했다** — 알려진 한계 재확인 |
| 4 | 취소 | ✅ 자동 테스트로 대체(증거 보존·원본 무변경) |

- **시나리오 3의 의미**: §3에서 확인한 임시 디렉터리 쓰기가 **강화된 파일럿 고정 인자로도 그대로**다. `--ignore-user-config`·`--ignore-rules`·`workspace-write`·`untrusted`를 다 걸어도 임시 경로는 샌드박스 쓰기 루트에 포함된다. 파일럿 목적에는 수용 가능하지만(복제본·임시 파일 모두 폐기 가능, 사용자 실제 파일은 시나리오 2로 보호 확인), **"복제본만 건드린다"가 아니라 "복제본 + 임시 디렉터리를 건드린다"로 정확히 기술해야 한다.** 실제 사용자 프로젝트로 넓힐 때 외부 샌드박스(VM·컨테이너)가 필요하다는 §3 결론은 그대로다.
- 오래된 상태 문구 2곳(§7.9 "미실측", §8 S2 체크박스)을 실측 완료로 정정했다.

**다음 할 일** → Codex 교차 검토(시나리오 결과·한계 기술). 그 뒤 §7.6 비교 과제 정의와 §5.2 교차 벤더 효과 검증. Windows CI 확인은 별건으로 남아 있음. **시나리오 테스트 추가분은 미커밋**

### 2026-08-14 · by Claude Opus 5

**S2 실행기 9차 재작업 — poisoned 상태 디스크 영속화 (제품 코드 변경 있음)**

Codex 지적 4건을 반영했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 9차 재작업`.

- **결함**: poisoned가 `PilotSession` 메모리에만 있어 **앱을 껐다 켜면 보호가 통째로 사라졌다.** 남은 프로세스가 있는 복제본을 그대로 재실행할 수 있었다.
- **수정**: 종료 실패 **즉시** 파일럿 루트에 `.hello-agent-pilot.poisoned`(pid·오류·경로 JSON)를 기록한다. 복제본 **밖**이라 남은 프로세스가 지울 수 없다. `validate_pilot_workspace`가 이 표시를 보면 거부하므로 **새 세션에서도** 막힌다.
- **표시 생애주기**: 검증된 `Gone` 복구 → `.poisoned` **제거**(다시 사용 가능) / 명시적 폐기 → `.abandoned`로 대체(**영구** 재사용 불가).
- **기준 이동**: 복구·폐기가 메모리가 아니라 **디스크 표시**를 근거로 하도록 `recover_codex_pilot`·`discard_codex_pilot`이 `workspace_path`를 받는다. 재시작 뒤 빈 세션에서도 동작해야 하기 때문이다. 메모리 `PoisonedRun`은 "어느 복제본 때문에 잠겼는가"만 안다(미사용 `pid` 필드 제거).
- **문구 교정**: `Unknown` 안내에서 *"restart the app"* 을 삭제했다. **재시작은 해결책이 아니다** — 표시는 디스크에 남고 프로세스도 그대로다. 이제 "프로세스 확인 후 복구 또는 작업공간 폐기"만 안내한다.
- **신규 테스트**: 실제 종료 실패 후 `SlotGuard`·`SlotHandle`을 버리고 새 핸들을 만들어 재시작을 모사한다. 실패 전 정상 사용 가능(대조군) → 재시작 후 `awaiting recovery` 거부 → 프로세스 생존 중 복구도 거부 → 정리 후 복구되면 표시가 사라지고 재사용 가능까지 확인한다.
- **검증**: `cargo test` **43 passed·경고 0**, `pnpm build`·`rustfmt --edition 2021 --check` 통과, e2e `outcome=Completed session=Some("019ffe2e-…")`, 잔존 프로세스 0, 코드에 `restart the app` 문구 0건.

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건 실측, Windows CI 확인, `recover`/`discard`의 사용자 노출 문구(UI 단계). **커밋·push하지 않음**

### 2026-08-14 · by Claude Opus 5

**S2 실행기 8차 재작업 — Windows 복구 fail-closed 전환 (제품 코드 변경 있음)**

Codex가 짚은 마지막 결함을 고쳤다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 8차 재작업`.

- **결함**: `process_tree_alive()`가 Windows에서 **항상 `false`** 라, `recover_pilot_slot`이 이를 "사라졌다"로 읽고 **`taskkill` 실패 직후에도 poisoned 슬롯을 풀어 줬다.** 보호가 가장 필요한 순간에 보호가 없었다.
- **수정**: `TreeLiveness { Gone, Alive, Unknown }`을 도입해 **"모름"을 성공으로 접지 않는다.** Unix는 `kill -0 -<pgid>`로 그룹을 직접 조회해 결론이 나지만, Windows는 `tasklist`로 직계 PID만 알 수 있고 `taskkill /T` 실패 후 남은 **손자**까지 사라졌는지는 Job Object 없이 증명할 수 없어 부재를 `Unknown`으로 본다. PID 미상도 `Unknown`이다. 복구는 **`Gone`일 때만** 열린다.
- **명시적 탈출구**: `Unknown`에서 영원히 잠기지 않도록 `discard_codex_pilot(confirm_discard_workspace)`를 추가했다. 확인 플래그 없이는 거부하고, 실행 시 복제본에 폐기 표시를 남겨 슬롯을 연다. `validate_pilot_workspace`가 이 표시를 보면 거부하므로 **그 복제본은 두 번 다시 실행 대상이 못 된다.** 증거 파일은 보존된다.
- **신규 테스트 3건**: 판정을 주입 가능하게 만들어 **macOS에서도 Windows 규칙을 고정**한다 — `Unknown` 거부(+대처 안내)·슬롯 잠김 유지·재예약 거부, `Alive` 거부, `Gone`일 때만 해제, PID 미상은 `Unknown`, 명시적 폐기 후 재사용 차단·증거 보존.
- **검증**: `cargo test` **42 passed·경고 0**, `pnpm build`·`rustfmt --edition 2021 --check` 통과, e2e `outcome=Completed session=Some("019ffe06-…")`, 잔존 프로세스 0.
- **미검증(명시)**: `windows_pid_alive`의 `tasklist` 실동작과 Windows 빌드 컴파일은 로컬에서 확인 못 했다(툴체인에 Windows 타깃 미설치). 판정 규칙 자체는 주입 테스트로 고정됐으나 **Windows CI 통과 확인이 필요하다.**

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건 실측, Windows CI 확인, `recover`/`discard`의 사용자 노출 문구(UI 단계). **커밋·push하지 않음**

### 2026-08-14 · by Claude Opus 5

**S2 실행기 7차 재작업 — poisoned 상태 도입 + 검사 순서 교정 (제품 코드 변경 있음)**

Codex가 지적한 후속 2건을 반영했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 7차 재작업`.

- **① 실패 시 슬롯을 열지 않는다**: 이전엔 종료 실패에도 `release()`로 슬롯을 열었는데, 그 시점엔 **살아남은 프로세스와 배출 스레드가 아직 증거 파일에 쓰고 있다.** `RunSlot.poisoned`(pid·오류)를 추가해 **복구 대기** 상태로 남기고, `reserve`는 "실행 중"과 구분해 `requires recovery`로 거부한다. 새 커맨드 `recover_codex_pilot`이 유일한 해제 경로이며 **프로세스 그룹이 실제로 사라졌을 때만** 열어 준다.
- **② 검사 순서**: `await_drain()` 헬퍼가 **`termination_error`를 먼저 확인하고 그다음 스레드 완료**를 본다. events join **이후 stderr join 이전에도** 재확인하고, stderr를 기다리는 **동안에도** 계속 확인한다.
- **③ 신규 테스트(가장 놓치기 쉬운 조합)**: 손자가 stdout을 `/dev/null`로 돌리고 부모도 stdout을 닫아 **이벤트 스트림은 정상 EOF에 닿지만** 손자가 stderr를 물고 있는 경우. stdout 완료만 보면 "잘 끝났다"고 착각하고 stderr join에서 영영 멈춘다. Codex 지적이 정확했다. 실측 **3.23초** 반환(취소의 `TERM 1.5s + KILL 1.5s`와 일치) — 수정 전이라면 60초를 기다렸을 것이다.
- 기존 실패 테스트 2건도 새 계약으로 갱신했다: 실패 직후 슬롯 `is_some()`, 재예약은 `requires recovery`로 거부, `recover_pilot_slot()` 성공 후에야 다음 실행 가능.
- **검증**: `cargo test` **39 passed·경고 0**, `pnpm build`·`rustfmt --edition 2021 --check` 통과, e2e `outcome=Completed session=Some("019ffdfb-…")`, 잔존 프로세스 0.

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측과, `recover_codex_pilot`의 사용자 노출 방식(UI 단계에서 결정). **커밋·push하지 않음**

### 2026-08-12 · by Claude Opus 5

**S2 실행기 6차 재작업 — 부착 후 취소 종료 실패 경로 종결 (제품 코드 변경 있음)**

마지막 경로를 닫았다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 6차 재작업`.

- **문제**: `cancel_pilot_child`가 종료 실패를 호출자에게만 알리고, **실행 소유자는 모른 채 stdout EOF를 기다렸다.** 살아남은 손자가 파이프를 물면 EOF가 오지 않아 실행이 영영 끝나지 않는다.
- **수정**: `RunSlot`에 `termination_error`를 추가해 취소가 실패를 **공유 상태에 저장**한다. **이벤트 읽기를 별도 스레드로 옮기고** 메인 흐름은 `is_finished()`와 `termination_error()`를 함께 감시한다 — 소유자가 블로킹 읽기에 갇히면 어떤 신호도 받을 수 없기 때문이다. 오류가 보이면 스레드를 기다리지 않고 제한된 정리 → diff 기록 → 슬롯 해제 → **취소가 본 것과 같은 오류**를 반환한다. 세션 ID는 채널로 즉시 받아 join하지 않는 경로에서도 쓴다.
- **절충 명시**: 남겨 둔 스레드는 살아남은 프로세스가 끝나면 스스로 종료한다. 안전한 Rust에서 다른 스레드가 소유한 파이프를 강제로 닫을 수단이 없어 택한 선택이며, "소유자가 갇히지 않는다"는 계약은 지켜진다.
- **신규 테스트**(`#[cfg(unix)]`): 자식이 붙은 뒤 취소하고 **취소 쪽에만** 실패 신호를 주입한다. ① 실행 반환 오류 == 취소가 본 오류, ② 경과 20초 미만(손자는 60초), ③ 슬롯이 비고 다음 예약 성공, ④ diff에 실행 중 만든 파일 포함. 실측 **3.15초**로 취소의 `TERM 1.5s + KILL 1.5s`와 일치한다.
- **테스트 함정 발견·수정**: 신호 기록용 전역을 두 테스트가 공유해 **병렬 실행 시 정리 대상이 엇갈려 `sleep 60` 프로세스가 샜다.** 전용 뮤텍스로 직렬화하고 정리를 `kill_recorded_group()`으로 통일했다. `ps`로 잔존 없음을 확인했다.
- **검증**: `cargo test` **38 passed·경고 0**, `pnpm build`·`rustfmt --edition 2021 --check` 통과, e2e `outcome=Completed session=Some("019ff527-…")`, 잔존 프로세스 0.

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측. **커밋·push하지 않음**

### 2026-08-12 · by GPT-5 Codex

**S2 실행기 5차 재작업 재검토 — 요청한 두 경로 해소, 실행 중 취소 실패 전파 1건 잔존 (제품 코드 변경 없음)**

- Windows 판정을 `TreeProbe::SignalStatus`로 분리해 `taskkill` 실패 시 직계 자식이 끝나도 성공으로 보지 않는 것을 확인했다. 순수 함수 테스트와 실제 child 종료 후 실패 신호 테스트가 해당 계약을 고정한다.
- child 부착 전 취소에서 `attach_with`가 종료 오류를 반환하면 stdout/stderr를 즉시 닫고, 제한된 정리·diff 기록·슬롯 해제 뒤 오류를 반환한다. `TERM`을 무시하며 파이프를 상속한 손자와 실패 신호를 조합한 회귀 테스트가 약 3.2초에 끝나므로 직전 EOF 무기한 대기 지적은 해소됐다.
- **차단 — child가 이미 붙은 실행 중 취소 실패는 여전히 실행 스레드에 전달되지 않는다.** 실제 UI 취소 경로인 `cancel_pilot_child`는 `signal_process_tree` 오류를 취소 호출자에게만 반환하고, `RunSlot`에는 `canceled = true` 외에 종료 오류를 남기지 않는다. 실행 소유자는 계속 stdout 순회와 stderr join에서 EOF를 기다린다. 종료 실패로 하위 프로세스가 파이프를 붙들면 취소 호출은 오류가 나더라도 실행 자체는 무기한 끝나지 않는다. 이번 신규 테스트는 spawn 전 취소로 `attach_with` 오류 분기만 덮어 이 일반 경로를 재현하지 않는다.
- 해결하려면 실행 중 종료 오류도 슬롯/공유 상태에 저장하고 실행 소유자가 차단 스트림 회수를 건너뛸 수 있어야 한다. 예를 들어 stdout/stderr drain을 별도 작업으로 두고 실행 소유자가 child 종료·취소 오류를 기준으로 제한된 join을 하거나, 취소 실패 시 읽기 핸들을 닫을 수 있는 소유 구조가 필요하다. 신호 실패를 주입한 **child 부착 후 취소** 테스트로 오류 반환 시간과 증거 보존을 고정해야 한다.
- **검증**: `cargo test --manifest-path src-tauri/Cargo.toml` **37 passed·7 ignored**, `pnpm build`, `rustfmt --edition 2021 --check`, `git diff --check` 통과. Windows `taskkill` 실동작은 아직 CI/실기기 미검증이다.

**결론** → 이번에 요청한 Windows 판정과 조기 `attach` 실패 수정은 통과했지만, 사용자가 실행 도중 누르는 일반 취소의 실패 경로 때문에 S2 실행기 전체는 **아직 미통과**다. 해당 한 경로를 닫은 뒤 재검토한다. 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 실행기 5차 재작업 — Windows 종료 판정·EOF 무기한 대기 수정 + 실패 경로 테스트 고정 (제품 코드 변경 있음)**

사용자 지시로 남은 2건을 수정하고 주입 가능한 신호 실행부로 실패 경로를 고정했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 5차 재작업`.

- **Windows 종료 판정**: 판정을 순수 함수 `tree_is_gone(probe, signal_ok, child_exited, group_alive)`로 분리했다. Windows는 그룹 조회 수단이 없어 `taskkill` 결과가 유일한 근거이므로, **실패했다면 `child.kill()`로 직계 자식이 끝났어도 성공으로 보지 않는다.** 순수 함수라 **macOS에서도 두 플랫폼 규칙을 함께 검증**할 수 있다 — Windows CI를 기다리지 않아도 된다.
- **EOF 무기한 대기 제거**: `attach_with`가 오류를 돌려주면 스트림 루프에 들어가지 않는다. 종료 실패는 프로세스가 파이프를 문 채 살아 있을 수 있다는 뜻이라 EOF를 기다리면 영영 멈춘다. 파이프 드롭 → 자식 회수·`kill()`·`try_wait()`(대기 없음) → **diff 증거는 기록** → 슬롯 해제 → 오류 반환 순으로 바꿨다.
- **주입 가능한 seam**: `type SignalFn`을 도입해 `signal_process_tree_with`·`attach_with`·`run_pilot_process_with`가 실행부를 받는다. 프로덕션은 `tree_signal` 그대로.
- **신규 테스트 3건**: ① 다섯 조합으로 두 플랫폼 판정 규칙 단언, ② 실제 프로세스로 같은 규칙 확인, ③ 신호를 실패로 주입해 **TERM 무시 손자가 파이프를 문** 상황에서 오류 반환·경과 20초 미만·diff 생존 단언.
- **실효성 확인**: ③이 **3.16초**에 끝난다 — `TERM 유예 1.5초 + KILL 유예 1.5초`와 정확히 일치. EOF를 기다렸다면 손자의 60초를 채웠을 것이다. 주입 신호는 아무것도 죽이지 않으므로 테스트가 대상 PID를 기록했다가 그룹을 실제로 정리하며, **잔존 프로세스 없음을 `ps`로 확인**했다.
- **검증**: `cargo test` **37 passed·경고 0**, `pnpm build`·`rustfmt --edition 2021 --check` 통과, e2e `outcome=Completed session=Some("019ff516-…")`.

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측과 Windows CI에서의 `taskkill` 실동작 확인(판정 규칙 자체는 순수 함수 테스트로 고정됨). **커밋·push하지 않음**

### 2026-08-12 · by GPT-5 Codex

**S2 실행기 4차 재작업 재검토 — Unix 성공 경로는 해소, 종료 실패 경로 2건 잔존 (제품 코드 변경 없음)**

- Unix 프로세스 그룹에 `TERM` 후 1.5초 유예, 그룹 `KILL` 후 1.5초 확인을 적용한 것과 `TERM`을 무시하며 파이프를 상속하는 손자 프로세스 회귀 테스트를 확인했다. 해당 테스트는 약 1.8초에 통과해 직전 Unix 트리 종료 회귀는 해소됐다.
- `SlotGuard::attach`가 종료 오류를 반환하고 `run_pilot_process`가 보관하는 변경도 확인했다. 다만 오류를 실제 호출자에게 확실히 전달하는 제어 흐름은 아직 완성되지 않았다.
- **차단 1 — Windows에서 트리 종료 실패를 성공으로 오판할 수 있다.** Windows의 `process_tree_alive`는 항상 `false`다. `/T /F` `taskkill`이 실패해도 뒤이은 `child.kill()`이 직계 자식만 끝내면 `wait_for_tree_exit`가 즉시 참이 되고, 저장해 둔 `kill` 오류를 확인하지 않은 채 `Ok(())`를 반환한다. 하위 프로세스가 남았다는 사실이 다시 숨겨진다. Windows 승격 단계에서는 최소한 강제 `taskkill`의 성공을 요구하고, 실패 시 직계 자식 종료 여부와 무관하게 오류로 반환해야 한다.
- **차단 2 — 조기 취소 종료 오류가 최종 반환 전에 멈출 수 있다.** `attach` 오류를 `early_cancel_error`에 저장한 뒤에도 stdout 순회와 stderr drain join을 먼저 수행한다. 종료 실패의 의미가 “하위 프로세스가 남았을 수 있음”인데, 그 프로세스가 파이프를 상속했다면 EOF가 오지 않아 523행의 오류 반환까지 도달하지 못한다. `attach` 실패 시 파이프를 닫고 제한된 정리만 수행한 뒤 오류를 우선 반환하거나, 스트림 회수 자체에 종료 한계를 둬야 한다.
- **검증**: `cargo test --manifest-path src-tauri/Cargo.toml` **34 passed·7 ignored**, `pnpm build`, `rustfmt --edition 2021 --check`, `git diff --check` 통과. Windows 실행은 로컬에서 미실측이다.

**결론** → Unix에서 정상적으로 신호를 전달할 수 있는 취소 경로는 통과했지만 S2 실행기 전체는 **아직 미통과**다. Windows `taskkill` 실패 판정과 `attach` 실패 시 비차단 오류 반환을 보완하고 해당 실패를 주입 가능한 테스트로 고정한 뒤 재검토한다. 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 실행기 4차 재작업 — 종료 보장 3건 구현 (제품 코드 변경 있음)**

사용자 지시로 취소 종료 보장을 마무리했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 4차 재작업`.

- **TERM → KILL 승격**: 그룹(`-pid`)에 `TERM` → 1.5초 유예 → 안 죽으면 `KILL` → 1.5초 확인. 둘 다 실패하면 `survived both TERM and KILL` 오류로 보고한다. 생존 판정은 `kill -0 -<pid>`로 **그룹 전체**를 보며, 좀비가 판정을 흐리지 않도록 매 회차 `try_wait`으로 자식을 회수한 뒤 검사한다. Windows도 `taskkill /T` → `/T /F`로 같은 2단계를 쓴다.
- **`attach` 조기 취소 오류 전파**: `attach`가 `Result`를 돌려주고 `run_pilot_process`가 증거 확정 후 최종 결과로 반환한다. 프로세스 잔존 가능성이 증거 결함보다 급하므로 먼저 보고한다. 이전에는 `let _ =`로 버려졌다.
- **회귀 테스트 추가**: 부모와 손자가 **둘 다 TERM을 무시**하고 손자가 stdout 파이프를 상속한 시나리오(`trap '' TERM` 중첩). TERM만으로는 파이프가 닫히지 않아 실행이 60초를 채운다 — KILL 승격이 있어야 끝난다.
- **승격이 실제로 동작함을 시간으로 확인**: TERM 무시 테스트 **1.80s**(유예 1.5초 소진 후 KILL) vs TERM 수용 대조군 **0.20s**. 대조가 없으면 "그냥 빨리 끝났다"와 구분되지 않는다.
- **검증**: `cargo test` **34 passed**, `pnpm build`·`rustfmt --edition 2021 --check` 통과, e2e `outcome=Completed session=Some("019ff506-…")`.
- **한계 2건 명시**: ① 새 회귀 테스트는 `#[cfg(unix)]`다(`trap`은 POSIX 셸 기능). **Windows의 `/T /F` 승격은 미실측**이며 CI 실행이 필요하다. ② 종료 실패 시 오류 전파는 자동 테스트로 못 덮었다 — `SIGKILL`을 견디는 프로세스를 만들 수 없어 실패를 강제할 방법이 없다.

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측과 Windows CI에서의 승격 확인. **커밋·push하지 않음**

### 2026-08-12 · by GPT-5 Codex

**S2 실행기 3차 재작업 재검토 — 직전 경합 2건 해소, 프로세스 트리 종료 회귀로 미통과 (제품 코드 변경 없음)**

- `attach`가 예약 직후 들어온 취소를 집행하고, `reclaim_child()`와 `release()`를 분리해 diff 증거 확정까지 슬롯을 유지하는 것을 확인했다. 직전 검토의 부착 전 취소·증거 확정 전 슬롯 개방 문제는 해소됐다.
- **차단 1 — Unix에서 프로세스 트리 강제 종료가 보장되지 않는다.** 현재 프로세스 그룹에는 `TERM`만 보낸 직후 `child.kill()`로 직계 자식만 강제 종료한다. `TERM`을 무시하는 손자 프로세스는 살아남을 수 있고, 그 프로세스가 stdout/stderr 파이프를 상속했다면 실행기의 스트림 회수도 끝나지 않아 취소가 멈출 수 있다. 그룹 전체에 `TERM` → 짧은 유예 → `KILL`을 적용하거나 동등한 트리 강제 종료가 필요하다.
- **차단 2 — 부착 전 취소의 실제 종료 실패가 숨겨진다.** child가 없을 때 `cancel_pilot_child`는 성공을 반환하고, 이후 `attach`가 호출한 `signal_process_tree`의 오류를 버린다. 따라서 나중의 그룹 종료 실패가 호출자에게 전달되지 않은 채 결과가 `Canceled`로 확정될 수 있다. `attach`가 오류를 반환하거나 실행 상태에 보존해 최종 결과에서 드러내야 한다.
- 현재 취소 테스트는 직접 실행한 장기 대역만 종료하며, `TERM`을 무시하고 파이프를 상속한 하위 프로세스를 만들지 않아 위 회귀를 잡지 못한다. 이 트리 시나리오를 Unix 자동 테스트로 추가해야 한다.
- **검증**: `cargo test --manifest-path src-tauri/Cargo.toml` **33 passed·7 ignored**, `pnpm build`, `rustfmt --edition 2021 --check`, `git diff --check` 통과. 첫 `pnpm build`는 샌드박스가 `node_modules/.vite-temp` 쓰기를 막아 실패했으며 권한 확장 재실행에서 통과했다.

**결론** → 직전 두 경합 수정은 승인할 수 있지만 S2 실행기 전체는 **아직 미통과**다. 프로세스 그룹 강제 종료·조기 취소 오류 전파·하위 프로세스 회귀 테스트를 보완한 뒤 재검토한다. 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 실행기 3차 재작업 — 취소 선점·증거 확정 경합 2건 해소 (제품 코드 변경 있음)**

Codex 재검토의 잔여 차단 2건 모두 타당해 수정했다. 핵심은 **`RunSlot`의 소유 범위를 예약 시점부터 증거 확정까지로 넓힌 것**이다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 3차 재작업`.

- **차단 1(부착 전 취소 무시)**: 슬롯은 spawn보다 먼저 예약되는데, 그 사이 취소가 들어오면 표시만 남고 `attach`가 취소를 보지 않은 채 자식을 붙여 **작업이 끝까지 실행된 뒤 결과만 `Canceled`** 였다. 사용자에겐 취소가 안 먹는 것과 같다. `attach`가 이미 취소된 슬롯이면 붙이는 즉시 프로세스 트리를 종료한다.
- **차단 2(증거 확정 전 슬롯 개방)**: `finish()`가 슬롯을 비운 뒤 `wait()`·`write_pilot_diff()`를 해서, 그 틈에 새 실행이 같은 복제본을 고치면 이전 diff가 오염됐다. `reclaim_child()`(자식만 회수, 슬롯 유지)와 `release()`(diff 확정 후 개방)로 분리하고 `Drop`은 `released` 플래그로 이중 해제를 막는다.
- **보완**: `taskkill`·`kill` 종료 상태를 더 이상 버리지 않는다. 실패 시 오류를 전파해 "트리를 못 끊고 부모만 죽였다"를 숨기지 않되, **이미 끝난 프로세스에 대한 신호 실패는 정상**으로 처리한다(중복 취소). Windows 대역을 `timeout` → `ping`으로 교체했다 — `timeout`은 stdin 리다이렉트 환경에서 즉시 실패해 대역 노릇을 못 한다는 Codex 지적이 맞았다.
- **신규 테스트 2건**: ① spawn 전 취소 후 30초 대역이 **20초 미만에 끝나는지**(표시만 남고 완주하면 실패), ② `reclaim_child()` 뒤에도 슬롯이 유지돼 **다른 예약이 거부**되고 `release()` 후에야 열리는지.
- **검증**: `cargo test` **33 passed**, `pnpm build`·`rustfmt --edition 2021 --check` 통과, e2e 재실행 `outcome=Completed session=Some("019ff4fb-…")`.

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측. **커밋·push하지 않음**

### 2026-08-12 · by GPT-5 Codex

**S2 실행기 2차 재작업 재검토 — 직전 3건은 대부분 해소, 취소 선점·증거 확정 경합 2건 잔존 (제품 코드 변경 없음)**

- `RunSlot { run_id, child, canceled }`, `SlotGuard`, Windows `taskkill /T /F`, 플랫폼별 실행 대역, 증거 I/O 오류 수집과 실행 중 취소 통합 테스트를 확인했다. Rust 전체 **31 passed·7 ignored**, `pnpm build`, `rustfmt --edition 2021 --check`, `git diff --check`가 통과했다.
- Claude의 rustfmt 정정은 맞다. `src-tauri/Cargo.toml`의 실제 edition은 2021이며, 이전 Codex 검토는 프로젝트 문서의 잘못된 edition 2024 안내를 따랐다. `AGENTS.md`·`CLAUDE.md`·history의 edition 정정도 실제 설정과 일치한다.
- **차단 1 — child 부착 전 취소가 실행을 중단하지 못한다.** 슬롯은 Codex 탐색·증거 파일 생성·spawn보다 먼저 예약된다. 이 사이 `cancel_pilot_child`가 호출되면 `canceled = true`만 남고 child가 없어 신호를 못 보낸다. 이후 `SlotGuard::attach`는 취소 여부를 보지 않고 child를 붙이므로 작업이 끝까지 실행된 뒤 결과만 `Canceled`가 된다. `attach`가 이미 canceled인 슬롯에는 즉시 프로세스 트리 종료를 적용하거나, 예약/starting 상태의 취소를 spawn 전에 다시 확인해야 한다.
- **차단 2 — 슬롯이 diff 증거 확정 전에 비워진다.** `guard.finish()`가 슬롯을 제거한 뒤 child `wait()`와 `write_pilot_diff()`를 수행한다. 그 사이 새 실행이 같은 복제본을 수정하면 이전 실행의 diff에 새 실행 변경이 섞인다. 실행 상태는 프로세스 종료뿐 아니라 스트림·diff 증거 확정까지 소유해야 하며, 슬롯 해제는 `PilotRun` 또는 오류 결과가 확정된 마지막 단계여야 한다.
- 현재 `cancel_during_run_is_owned_by_the_running_slot` 테스트는 child가 붙을 때까지 기다린 뒤 취소하므로 차단 1의 시작 직후 취소를 재현하지 않는다. 또한 슬롯이 diff까지 유지되는 동안 새 실행이 거부되는 회귀 테스트가 필요하다.
- Windows 트리 종료는 `taskkill` 경로를 추가했지만 종료 상태를 무시한다. 최소한 실패 시 `child.kill()`이 부모만 종료했다는 사실을 숨기지 않도록 결과를 전파해야 한다. Windows 대역인 `cmd /C timeout`이 stdin null 환경에서 충분히 오래 살아 있는지도 CI 실측이 필요하다.

**결론** → S2 2차 실행기는 **아직 미통과**다. `RunSlot` 소유 범위를 예약 시점부터 증거 확정까지 유지하고, 부착 전 취소와 diff 생성 중 동시 시작을 자동 테스트로 닫은 뒤 다시 검토한다. 이번 검토에서는 제품 코드를 수정하거나 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 실행기 2차 재작업 — 차단 3건 수용, rustfmt 지적 1건 정정 (제품 코드 변경 있음)**

Codex 재검토의 차단 3건은 모두 타당해 수정했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 2차 재작업`.

- **차단 1(취소 소유권 경합)**: 취소가 슬롯을 즉시 비워, 취소된 실행이 뒷정리하는 사이 새 실행의 자식을 가져갈 수 있었다. `RunSlot { run_id, child, canceled }`로 재설계해 **취소는 신호와 표시만 남기고 회수는 실행 소유자만** 한다. `SlotGuard`가 `Drop`으로 예약을 자동 해제한다.
- **차단 2(Windows·테스트 이식성)**: Windows도 `taskkill /T /F`로 트리를 종료한다 — 더는 플랫폼 한계로 두지 않는다. 테스트의 `/bin/sleep`·`/bin/ps` 의존을 없애고 대역 프로세스를 플랫폼별로 고르며 생사는 `try_wait()`로 확인한다. **슬롯 예약을 Codex 탐색보다 앞으로** 옮겨 Codex 없는 CI에서도 거부가 먼저 판정된다.
- **차단 3(증거 I/O 은폐)**: `writeln!`·`flush`·읽기·join 오류를 모두 수집해 **실행 결과를 오류로 반환**한다. 잘린 로그를 `Completed`로 보고하던 경로가 사라졌다.
- **신규 테스트**: Codex가 요구한 **실행 중 취소 통합 테스트**를 추가했다(실행 대상 주입으로 Codex 없이 동작). 동시 실행 테스트에는 유령 증거 파일 검사를 더했다. `cargo test` **31 passed**, e2e 재실행 통과.

**정정 — rustfmt 지적은 edition 불일치였다**

- Codex는 `rustfmt --edition 2024 --check` 4건 실패를 근거로 내 "클린" 기록을 틀렸다고 했다. 확인 결과 **이 크레이트의 실제 edition은 2021**이다(`src-tauri/Cargo.toml:6`). `--edition 2021`로 검사하면 **0건**이다. 컴파일러도 let chain을 *"only allowed in Rust 2024 or later"* 로 거부해 이를 확인해 준다.
- **부수 발견 — 고정 문서가 틀렸다.** `CLAUDE.md`·`AGENTS.md`·이 문서가 *"Rust 1.85+ 필요(edition 2024)"* 라고 적어 왔다. Codex가 2024로 검사한 것도 이 문서를 따랐을 가능성이 크다. **세 곳을 실제 값(edition 2021)으로 정정하고 포맷 검사 명령을 명시**했다. CLAUDE.md와 AGENTS.md 동일성도 재확인했다.

**아카이브 롤오버 수행**: 이 엔트리로 파일이 573줄이 돼 임계(500)를 넘겨, 2026-07-23 엔트리 2건을 `docs/archive/history-2026-07.md`로 옮겼다(573 → 452줄). **목표치 300줄에는 못 미친다** — 남은 것이 고정 섹션 56줄 + **단일 날짜(2026-08-12) 약 390줄**이고, 규칙이 같은 날짜를 쪼개는 것을 금지하기 때문이다. 오늘 M4 왕복이 하루에 30개 엔트리로 몰린 결과다. 날짜가 바뀌면 정상 범위로 내려간다.

**다음 할 일** → Codex 재검토. 남은 것은 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측. **커밋·push하지 않음**

### 2026-08-12 · by GPT-5 Codex

**S2 실행기 재검토 — 이전 지적은 반영됐지만 취소 소유권·Windows·증거 오류 차단 3건 잔존 (제품 코드 변경 없음)**

- 실행별 `run_id` 증거 경로, 동시 시작 거부, stderr 배출 스레드, spawn 전 증거 파일 생성, Unix 프로세스 그룹 종료, `git add -N` 실패 전파는 이전 지적의 직접 원인을 해결했다. Rust 전체 테스트를 독립 재실행해 **30 passed·7 ignored**, `pnpm build`, `git diff --check` 통과를 확인했다.
- **차단 1 — 취소 시 child 소유권 경합이 여전히 남는다.** `cancel_pilot_child`가 현재 child를 슬롯에서 `take()`해 슬롯을 즉시 비운다. 취소된 실행이 stdout/stderr 정리와 diff 생성을 끝내기 전에 새 실행이 시작될 수 있고, 이전 실행의 마지막 `handle.take()`가 새 child를 가져가 기다리거나 결과를 오판하며 이전 diff에 새 실행 변경이 섞일 수 있다. 취소는 슬롯을 비우지 말고 같은 run ID의 child에 종료 신호와 canceled 상태만 기록하며, 실행 함수만 자기 run ID를 확인해 최종 정리·슬롯 해제를 해야 한다.
- **차단 2 — Windows 취소·테스트 계약이 성립하지 않는다.** Unix만 프로세스 그룹 전체를 종료하고 Windows는 직접 child만 kill해 하위 셸 명령이 남을 수 있다. 제품 범위가 Windows를 포함하므로 플랫폼 한계로 남길 수 없다. 또한 취소·동시 실행 테스트가 `/bin/sleep`·`/bin/ps`에 의존하고, 동시 실행 테스트는 슬롯 확인 전에 설치된 Codex를 찾으므로 Codex가 없는 Windows/macOS CI에서 실패한다. Windows Job Object 또는 동등한 트리 종료와 플랫폼 독립 실행 대역/주입이 필요하다.
- **차단 3 — 실행 중 증거 I/O 오류를 여전히 숨긴다.** stdout 이벤트와 stderr 로그의 `writeln!`·`flush`, 줄 읽기 오류, stderr 스레드 join 결과를 모두 무시한다. 디스크 부족·권한·I/O 오류가 발생하면 불완전한 이벤트/로그인데도 `Completed` 결과를 반환할 수 있어 증거 기반 완료 계약과 충돌한다. 스트림 배출 결과를 수집하고 증거 실패를 실행 결과에 반영해야 한다.
- 동시 실행 거부 전에 고유 증거 파일들을 먼저 만들기 때문에 거부된 호출도 반환되지 않는 빈 증거 파일을 남기는 부작용이 있다. 실행 슬롯을 먼저 **예약**하는 상태가 필요하다.
- Claude가 “`orchestration.rs`는 rustfmt 클린”이라고 기록했지만 독립 `rustfmt --edition 2024 --check`는 기존 테스트 구간을 포함한 여러 위치에서 실패했다. 기능 차단보다 낮지만 검증 기록은 현재 사실로 정정해야 한다.
- Claude가 남긴 대로 실제 `execute_pilot` 실행 중 취소 통합 경로는 아직 검증되지 않았다. 이는 위 취소 경합을 잡을 핵심 회귀 테스트이므로 수동 확인만으로 미루지 말고 자동화 가능한 프로세스 대역으로 추가해야 한다.

**결론** → S2 2차 실행기 재작업은 **아직 미통과**다. 이전 지적을 단순 보완하는 수준을 넘어 `run_id`를 가진 실행 상태가 child의 전체 수명주기를 소유하도록 정리한 뒤 다시 검토해야 한다. 이번 검토에서는 제품 코드를 수정하거나 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 실행기 재작업 — Codex 지적 5건 전부 반영 (제품 코드 변경 있음)**

Codex가 실행기를 미통과로 판정했다. **차단 3건·보완 2건 모두 내 코드의 실제 결함이 맞아 반박 없이 수정했다.** 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 재작업`.

- **차단 1(증거 덮어쓰기)**: 실행별 `run_id`(밀리초+PID+원자적 시퀀스)를 도입해 모든 증거 경로를 갈랐다. 특히 취소된 재실행이 **이전 실행의 최종 메시지를 자기 결과로 반환하던 버그**는, 실행 시작 시 파일을 빈 내용으로 만들어 해소했다 — 중단되면 빈 파일 자체가 이 실행의 증거다.
- **차단 2(슬롯 탈취)**: 새 실행이 기존 실행을 자동 취소하던 동작을 없애고, 슬롯 점검과 `spawn`을 **한 잠금 안에서** 처리해 이미 실행 중이면 거부한다.
- **차단 3(프로세스 I/O·정리)**: stderr를 전용 스레드로 배출해 교착을 막고(`…stderr.log`), **증거 파일을 모두 만든 뒤 spawn**해 실패 시 프로세스가 슬롯에 남지 않게 했다. 취소는 자식을 프로세스 그룹 리더로 띄운 뒤(`process_group(0)`) 그룹 전체에 `TERM` → 유예 → `KILL`을 보내 **하위 셸 명령까지 끊는다**(Windows는 자식만 — 플랫폼 한계).
- **보완**: `git add -N` 실패를 오류로 전파한다. 증거 생성 실패가 "변경 없음"으로 위장되면 안 된다. `orchestration.rs`는 rustfmt 클린이 됐다.
- **검증**: `cargo test` **30 passed**(신규 3건). end-to-end 재실행 `outcome=Completed session=Some("019ff4df-…")`, 증거 4종 생성, 원본 무변경.
- **함정 발견 — `cargo fmt`는 크레이트 전체를 건드린다.** 실행 후 `agent.rs`·`detect.rs`·`error.rs`·`install.rs`·`login.rs`·`status.rs`가 함께 바뀌어 전부 되돌렸다. 이 저장소의 기존 파일들은 **원래 rustfmt 클린이 아니다.** 포맷 정리를 하려면 이번 변경과 섞이지 않게 별도 작업으로 해야 한다.

**다음 할 일** → **Codex 재검토**. 남은 것은 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측과, Codex가 짚은 대로 `execute_pilot` 실행 중 동시 취소를 거는 통합 경로 확인. **커밋·push하지 않음**

### 2026-08-12 · by GPT-5 Codex

**S2 실행기 교차 검토 — S2 2차 구현 조각으로 확인, 실행 수명주기 차단 3건 (제품 코드 변경 없음)**

- 이번 Claude 작업은 S2 1차의 복제본·사전 점검 다음에 실제 실행·취소·증거 포착을 붙인 것이므로 **S2 2차 구현 조각(취소 가능한 실행기)**으로 부르는 것이 맞다. 다만 S2 전체 완료는 아니며 수동 경계 실측도 남아 있다.
- 고정 인자의 `--json`·`--output-last-message`, `thread.started` 세션 ID 파싱, 파일럿 루트 내부 소스 거부와 Tauri 커맨드 등록은 의도와 일치한다. Rust 전체 테스트를 독립 재실행해 **27 passed·7 ignored**, `git diff --check` 통과를 확인했다.
- **차단 1 — 반복 실행 증거가 보존되지 않는다.** 이벤트·최종 메시지·diff 경로가 작업공간 이름으로 고정돼 매 실행마다 이전 증거를 덮어쓴다. 특히 취소된 재실행에서 이전 `last-message`가 이미 있으면 현재 실행의 결과처럼 그대로 반환된다. 재작업 라운드별 고유 실행 ID/경로가 필요하다.
- **차단 2 — 전역 `Option<Child>` 소유권 경합이 있다.** 새 실행은 기존 실행을 자동 취소하고 같은 슬롯을 재사용한다. 두 `run_codex_pilot` 호출이 겹치면 먼저 끝난 실행이 새 실행의 child를 `take()`해 기다리고, 새 실행은 자신을 `Canceled`로 오판할 수 있다. 새 실행을 거부하거나 실행 ID별 소유권을 분리해야 한다. 현재 취소 테스트는 `execute_pilot`과 동시에 취소하는 통합 경로를 검증하지 않는다.
- **차단 3 — 프로세스 I/O·실패 정리가 불완전하다.** stderr를 `piped`로 만들고 읽지 않아 버퍼가 차면 자식이 멈출 수 있다. 또한 자식을 시작한 뒤 이벤트 파일 생성이 실패하면 함수가 오류로 빠지면서 실행 중인 child가 슬롯에 남는다. 취소도 직접 child만 kill하므로 하위 프로세스까지 종료된다는 보장이 없다. stdout/stderr 동시 배출과 모든 오류 경로의 종료·wait, 플랫폼별 프로세스 트리 취소가 필요하다.
- 추가로 `git add -N` 실패를 무시하면 새 파일이 diff에서 빠져도 성공 증거로 반환될 수 있다. 증거 생성 실패는 숨기지 말고 실행 결과에 실패로 반영해야 한다.
- 수정 파일 자체의 `rustfmt --check`는 새 테스트의 3곳에서 실패한다. 기능 차단보다는 낮지만 재검토 전에 정리해야 한다.

**결론** → S2 실행기 2차 구현은 **미통과**다. 위 실행 수명주기·증거 보존 문제를 코드와 회귀 테스트로 보완한 뒤 다시 Codex가 검토한다. 이번 검토에서는 제품 코드를 수정하거나 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 차단 종료 + 취소 가능한 실행기 구현 (제품 코드 변경 있음)**

사용자 지시로 S2 차단 2건을 닫고, 다음 조각인 실행기를 이어서 구현했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 실행기 구현`.

- **오래된 상태 문구 2곳 정정**: "S2 1차 구현 · Claude 교차 검토 대기" → 검토 완료·차단 해소. §8의 "Claude 교차 검토와 실행기·수동 실측 남음" → 실행기까지 완료, 수동 시나리오와 Codex 검토만 남음.
- **세션 ID 포착 경로 실측 확정**: `codex exec --json`의 첫 이벤트 `{"type":"thread.started","thread_id":"…"}`가 ID를 싣고 오며, **같은 UUID의 rollout 파일이 `~/.codex/sessions`에 존재**함을 대조해 재개용 ID와 동일함을 확인했다. §7.4의 "정확한 세션 ID 저장"이 이제 실제로 가능하다.
- **고정 인자에 `--json`·`--output-last-message` 추가**. `validate_pilot_args`가 **둘의 부재를 거부**해, 증거 없이 실행되는 경로를 코드가 막는다.
- **증거 3종(이벤트·최종 메시지·diff)을 복제본 밖 앱 전용 루트에 보존**한다. 에이전트가 자기 증거를 고칠 수 없다. diff는 `git add -N` 후 `git diff <시작 커밋>`이라 **새로 생긴 파일도 잡힌다.** `cancel_pilot_child`의 계약은 "프로세스만 죽이고 어떤 파일도 지우지 않는다"이다.
- **파일럿 루트 안의 소스 재복제 거부**를 추가했다(검사 순서를 바꿔 루트를 먼저 정규화).
- **검증**: `cargo test` **27 passed**. 신규 4건 중 취소 테스트는 `/bin/sleep`를 실행 중 프로세스 대역으로 두고 취소 → `ps`로 사망 확인, 복제본·매니페스트 생존, diff에 실행 중 만든 파일 포함, 증거가 복제본 밖에 위치, 중복 취소 안전까지 본다.
- **end-to-end 실측**(`#[ignore]`, 사용량 소모): 실제 Codex로 1회 실행해 `outcome=Completed session=Some("019ff4d3-…")`. 증거 3종 생성, diff에 에이전트가 만든 파일 포함, **원본 저장소 무변경** 확인.

**다음 할 일** → **Codex 교차 검토**(§7.9는 S2를 Codex 담당으로 뒀지만 이 조각은 Claude가 구현했으므로 검토 담당이 바뀐다). 그 뒤 수동 경계 시나리오 3건(저장소 밖 쓰기·임시 경로 쓰기·저장소 안 삭제) 실측. **커밋·push하지 않음**

### 2026-08-12 · by GPT-5 Codex

**확인 — Claude의 S2 차단 해소 재검토는 타당, 제안서 상태 문구 2곳만 오래됨 (제품 코드 변경 없음)**

- 실제 코드에서 `--ephemeral` 제거, `merge-base --is-ancestor` 판정, 후속 커밋 허용·amend 거부 테스트를 다시 대조했다. orchestration 테스트 4건을 독립 재실행해 모두 통과했고 `git diff --check`도 통과했다.
- Claude가 남긴 세션 rollout 실측 근거도 확인했다. `~/.codex/sessions`에 기록한 `019ff4c8-…` rollout 파일이 실제로 존재하고 현재 파일 수가 448개여서, 세션 보존과 사용자 Codex 기록 혼입이라는 두 결론이 증거와 일치한다.
- 신규 차단 사항은 없다. `--json` 기반 세션 ID 포착, `--output-last-message`, 취소 가능한 실행기, 파일럿 루트 내부 소스 거부와 수동 경계 4건은 Claude가 적은 대로 다음 구현 범위이며 **S2 전체는 아직 완료가 아니다.**
- 문서상 작은 불일치가 남았다. `m4-orchestration.md`의 S2 1차 구현 제목은 아직 “Claude 교차 검토 대기”, §8 TODO는 “Claude 교차 검토 … 남음”이라고 되어 있지만 재검토는 끝났다. 다음 문서 수정 시 현재 상태로 고쳐야 한다.

**결론** → Claude의 차단 2건 해소 판정은 통과로 받아들인다. 이번 확인에서는 제품 코드를 수정하거나 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 재검토 — 차단 2건 해소 확인 + 부작용 실측 완료 (제품 코드 변경 없음)**

Codex가 차단 2건을 코드·테스트에 반영했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 재검토`.

- **차단 1 해소**: `--ephemeral`이 고정 인자에서 제거됐고 테스트가 부재를 단언한다.
- **차단 2 해소**: `git_is_ancestor`가 `merge-base --is-ancestor` 종료 코드로 판정한다. 오류 문구도 "history was rewritten"으로 바뀌어 **훼손과 이력 재작성을 구분**한다 — 내가 요청한 구분이 반영됐다. 테스트가 정상 커밋 후 통과와 `--amend` 후 거부를 각각 잡는다. `cargo test` **23 passed**.
- **부작용 실측 완료 — 앞서 추정으로 표시했던 것을 사실로 갱신**: 재작업된 인자를 그대로 실행하니 세션 파일이 `~/.codex/sessions` **447 → 448개**로 늘고 `rollout-…-019ff4c8-….jsonl`이 생성됐다. 즉 ① 재개에 필요한 rollout이 실제로 남아 차단 1의 수정이 목적을 달성했고, ② **파일럿 세션이 사용자의 실제 Codex 기록에 섞인다**는 예측도 사실로 확인됐다.
- **남은 것(차단 아님)**: 세션 ID를 앱이 얻을 수단이 아직 없다. rollout 파일명에 ID가 있지만 안정적으로 포착하려면 `--json`이 필요하다 — **재개는 가능해졌을 뿐 아직 구현되지 않았다.** 실행기 조각에서 닫아야 §7.4 원장 계약이 성립한다. `source`가 파일럿 루트 안인지 검사하는 항목도 미반영.

**다음 할 일** → S2는 실행기(`--json`·`--output-last-message`·취소)와 수동 경계 시나리오 4건이 남았다. Codex 1차 담당. 미커밋 변경(`orchestration.rs` 신규 포함)은 커밋 승인 대기

### 2026-08-12 · by GPT-5 Codex

**S2 차단 2건 코드·테스트 반영 — 재개 가능한 세션과 후속 커밋 허용**

- 사용자 지적에 따라 문서 확인에서 멈추지 않고 합의된 수정안을 `orchestration.rs`에 반영했다. Codex 고정 실행 인자에서 `--ephemeral`을 제거해 세션 rollout과 정확한 세션 ID 재개 계약이 양립하도록 했다.
- `HEAD == start_commit` 동일성 검사는 `git merge-base --is-ancestor <start_commit> HEAD`로 교체했다. 정상적인 구현·재작업 커밋은 허용하고, amend·rebase·squash 등으로 시작 커밋이 이력에서 사라지면 이력 재작성 때문에 복원 기준을 찾을 수 없다는 오류로 거부한다.
- 회귀 테스트에 후속 커밋 허용, 시작 커밋을 제거한 amend 거부와 오류 문구, `--ephemeral` 부재를 추가했다.
- 검증은 수정 파일 `rustfmt --check`, Rust 전체 **23 passed·6 ignored**, `pnpm build`, `git diff --check`를 통과했다. 전체 `cargo fmt --check`는 이번 변경과 무관한 기존 Rust 파일들의 포맷 차이 때문에 실패해 해당 파일들을 임의 변경하지 않았다.

**다음 할 일** → Claude가 차단 2건의 코드·테스트 반영을 교차 재검토한다. S2 변경은 아직 커밋·push하지 않는다.

### 2026-08-12 · by GPT-5 Codex

**확인 — Claude의 S2 재검토는 수정 완료가 아니라 현재 코드 상태의 재확인 (제품 코드 변경 없음)**

- Claude가 지적한 상태는 실제 코드와 일치한다. `orchestration.rs`에는 여전히 `--ephemeral`과 `HEAD == start_commit` 비교가 남아 있어, 앞서 합의한 **세션 보존**과 **시작 커밋 자손 검사**가 아직 구현되지 않았다.
- 파생 경계도 타당하다. `--ephemeral` 제거 후 세션 기록이 사용자 `CODEX_HOME`에 남을 가능성은 재개 기능의 비용으로 명시하되, 실제 파일 생성 위치·동작은 아직 실측하지 않았으므로 확정 사실로 확대하지 않는다.
- 자손 검사는 정상 후속 커밋을 허용하지만 amend·rebase·squash처럼 시작 커밋을 이력에서 제거하면 거부한다. 구현 시 `git merge-base --is-ancestor` 종료 코드로 판정하고, 오류 문구는 일반 훼손이 아니라 **이력 재작성으로 복원 기준을 찾을 수 없음**을 설명해야 한다.
- 이번 확인에서는 제품 코드나 테스트를 변경하지 않았고 테스트도 재실행하지 않았다. 커밋·push도 하지 않았다.

**결론** → Claude의 재검토는 정확하며 S2는 여전히 **미통과**다. 다음 작업은 차단 2건을 코드와 테스트에 반영한 뒤 다시 교차 검토하는 것이다.

### 2026-08-12 · by Claude Opus 5

**S2 상태 확인 — 차단 2건은 결정만 났고 코드는 아직 그대로 (제품 코드 변경 없음)**

- Codex가 차단 2건을 모두 수용하고 해결 방향을 정했다. `--ephemeral` 제거, `HEAD == start_commit`을 **자손 검사**로 교체. 후자는 내가 제시한 두 선택지 중 강한 쪽이고 복원 기준을 실제로 지키므로 단순 제거보다 낫다 — 동의한다.
- **다만 코드는 아직 반영되지 않았다.** `orchestration.rs:192`에 `--ephemeral`이, `:172`에 `head != marker.start_commit`이 그대로 있다. Codex 자신도 "S2는 아직 미통과"로 적었다. 문서상 결정과 코드 상태를 혼동하지 않도록 여기 기록한다.
- **재작업 전 파생 결과 1건 추가**(§7.9 `재작업 전 확인 사항`): `--ephemeral`을 빼면 파일럿 세션이 **사용자의 실제 `~/.codex/sessions`에 섞인다.** `--ignore-user-config`는 설정만 배제하고 인증은 `CODEX_HOME`을 계속 쓰므로, 앱 전용 홈으로 돌리면 로그인이 깨진다. 재개 가능성과 맞바꾸는 값이니 명시적으로 수용한 결과로 남겨야 한다(세션 파일 실제 생성은 미실측).
- 자손 검사의 경계 조건도 남겼다: `--amend`·rebase·squash 시 조상 관계가 끊겨 거부되는데, 오류 문구가 훼손이 아니라 **이력 재작성** 때문임을 구분해야 한다. 판정은 `git merge-base --is-ancestor`의 종료 코드로 충분하다.

**다음 할 일** → Codex가 차단 2건을 코드·테스트에 반영 → Claude 재검토. S2는 여전히 **미통과**

### 2026-08-12 · by GPT-5 Codex

**확인 — Claude의 S2 조건부 검토와 차단 2건에 동의 (제품 코드 변경 없음)**

- Claude가 전역 `never + danger-full-access` 설정이 S2 고정 인자에 섞이지 않는다는 점을 비임시 홈 경로 쓰기 실패로 실측한 방법과 결론은 타당하다. 플래그 존재·로그인 유지·Rust 테스트 확인도 현재 코드와 일치한다.
- `--ephemeral`은 세션 rollout을 남기지 않아 정확한 세션 ID 재개 계약과 충돌한다. 기존 §6·§7.4·§7.7 설계를 유지하고 **플래그를 제거**하기로 결론냈다. OpenAI 공식 문서의 ephemeral·resume 설명과도 일치한다.
- `HEAD == start_commit`은 정상적인 구현 커밋 뒤 재작업까지 막는다. 시작 커밋을 복원 기준으로 유지하되 **현재 HEAD가 시작 커밋의 자손인지 검사**하도록 바꾸는 것이 맞다.
- 보완 권고 4건도 수용한다. `--json`·마지막 메시지 파일은 실행기와 함께, 파일럿 복제본 재복제 거부·원본 dirty 상태 노출·Windows Git not-found 분류는 각 준비/실행 경계에 반영한다.

**결론** → S2 1차 구현은 **아직 미통과**다. 다음 작업은 위 두 차단의 코드·테스트 반영이며, 이번 확인에서는 제품 코드를 수정하거나 커밋·push하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S2 교차 검토 — 안전 경계는 실측 통과, 설계 충돌 2건으로 조건부 (제품 코드 변경 없음)**

Codex의 S2 1차 구현(`src-tauri/src/orchestration.rs`)을 검토했다. 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S2 교차 검토`.

- **완료 기준 실측 통과**: 이 머신의 전역 설정이 `never + danger-full-access`인 상태에서 S2 고정 인자를 그대로 실행해 **비임시 경로(홈)에 쓰기**를 시도시켰다. `operation not permitted`, 종료 코드 1, 파일 미생성 — **사용자 설정이 파일럿에 새지 않는다.** 임시 경로 예외를 피하려 일부러 홈을 대상으로 했고 canary는 즉시 제거했다.
- 플래그 6개가 설치된 CLI에 모두 실재함을 확인했다. `--ignore-user-config`가 *"auth still uses CODEX_HOME"* 이라 **위험한 설정만 배제하고 로그인은 유지**하는 정확한 선택이다. `cargo test` 22 passed.
- **차단 1 — `--ephemeral`이 재개 설계와 충돌**: 실제 의미가 "세션 파일을 디스크에 남기지 않음"이라, §6·§7.4·§7.7이 요구하는 "정확한 세션 ID로 재개"가 **성립하지 않는다.** 플래그를 빼든 재개를 포기하든 택일이 필요하다. 지금은 코드와 설계 문서가 다른 말을 한다.
- **차단 2 — `HEAD == start_commit` 검사가 재작업을 막음**: 구현 역할이 커밋을 한 번이라도 하면 이후 preflight가 영구 실패한다(`orchestration.rs:171-174`). §6·§7.3이 재작업 라운드를 정상 분기로 두는 것과 어긋나고, 커밋 여부에 따라 갈리는 비결정적 경계다. 경계 검사는 "앱이 만든 복제본인가"까지로 좁히고, 커밋을 보려면 동일성이 아니라 자손 여부를 봐야 한다.
- **보완 권고(차단 아님)** 4건: `--json`·`--output-last-message` 누락, 원본이 파일럿 루트 안인지 미검사, `git clone`이 미커밋 변경을 안 가져온다는 점의 사용자 노출, Windows `git`의 PATH 의존.

**다음 할 일** → 차단 2건은 **코드와 설계 문서 중 어느 쪽을 고칠지**의 결정이 먼저다. 정해지면 Codex가 반영 후 재검토. 수동 경계 시나리오 4건은 실행기 구현 후 실측

### 2026-08-12 · by GPT-5 Codex

**S1 커밋 + S2 파일럿 실행 경계 1차 구현 (제품 UI 미노출)**

- 사용자 승인에 따라 검토 통과한 S1 변경을 `m4/orchestration`에 `bcf49f1`(`fix: 안전 프리셋 적용 상태와 안내 문구 정합성 개선`)로 커밋했다. push와 `main` 병합은 하지 않았다.
- OpenAI 공식 `codex exec` 문서와 로컬 CLI `--help`를 대조해 `--ignore-user-config --ignore-rules --sandbox workspace-write --config approval_policy="untrusted"`를 파일럿 고정 정책으로 채택했다. `--ephemeral`과 프롬프트 옵션 종료자 `--`도 포함한다.
- `src-tauri/src/orchestration.rs`를 추가했다. 앱 데이터 `m4-pilots/` 아래의 `--no-hardlinks` Git 복제본만 준비하고, 매니페스트를 작업 폴더 밖에 저장하며 실제 경로·시작 커밋을 실행 직전에 다시 확인한다. 원본·무표식 폴더는 거부한다.
- `--approve-for-me`, bypass 2종, `--full-auto`, `--add-dir`를 사전 거부한다. 샌드박스와 승인 정책을 정확히 한 번만 허용해 중복 옵션으로 뒤에서 덮는 경우도 거부한다.
- Tauri에는 `prepare_codex_pilot`과 `codex_pilot_preflight`만 연결했다. 아직 UI와 실제 Codex 실행은 연결하지 않았다. 취소 가능한 자식 프로세스 관리와 중단 후 증거 보존을 먼저 구현해야 하기 때문이다.
- 관련 자동 테스트 3건 통과: 복제본/원본 분리, 원본·무표식 대상 거부, 사용자 정책 배제와 위험 플래그·중복 샌드박스 거부. 전체 검증은 Rust 22 passed·6 ignored, `pnpm build`, `git diff --check` 통과. 토큰과 로그인은 사용하지 않았다.

**다음 할 일** → Claude가 S2 1차 구현을 교차 검토한다. 통과 후 취소 가능한 실행기를 구현하고, 제안서 §7.9의 저장소 안 변경·비임시 밖 쓰기·임시 경로 한계·취소 시나리오를 실제 Codex로 검증한다. S2 변경은 아직 커밋·push하지 않았다.

### 2026-08-12 · by GPT-5 Codex

**S1 재검토 통과 — 안전 문구와 Codex 테스트 보완 확인 (제품 코드 변경 없음)**

- 이전 차단 2건이 해소됐다. `project.done.safe.applied`는 유효한 안전 정책을 보장하지 않고 “안전 관련 기본 설정 파일을 새로 만들었다”는 작성 사실만 말한다. 에이전트별 팁은 “기본 설정” 전제를 붙였고, Claude 자동 진행의 승인 생략과 Codex 폴더 밖 쓰기 예외도 명시한다.
- `apply_safety_preset_in(.., home)`은 테스트용 홈 경로만 주입하는 얇은 분리이며 운영 경로는 기존 `home_dir()`를 그대로 사용한다. Codex의 최초 `Applied`·재호출 `Kept`, 기존 `never + danger-full-access` 설정의 바이트 단위 보존, 프론트 직렬화 계약을 실제 사용자 홈과 격리해 검증한다.
- 독립 검증: `cargo test --manifest-path src-tauri/Cargo.toml` 19 passed·6 ignored, `pnpm build` 통과, `git diff --check` 통과. 새 차단 사항은 발견하지 못했다.
- 문서 상태도 현재 사실에 맞춰 M4 제안서의 S0 승인과 S1 완료를 체크하고 S2를 다음 단계로 남겼다.

**결론** → **S1 교차 검토 통과**. 커밋·push는 사용자 승인 전까지 하지 않는다. 다음 작업은 계획대로 S2(Codex 1차 담당)이며, 호스트에서 `--approve-for-me`를 사용하지 않는 파일럿 실행 경계를 구현한다.

### 2026-08-12 · by Claude Opus 5

**S1 재작업 — Codex 교차 검토 지적 3건 반영 (제품 코드 변경 있음)**

Codex가 S1을 미통과로 판정했다. **지적 3건 모두 타당해 반박 없이 수정했다.** 상세는 `docs/proposals/m4-orchestration.md` §7.9 `S1 재작업`.

- **차단 1(안전 상태 과장)**: `Applied`는 "파일을 썼다"일 뿐 유효 정책이 아니라는 지적. 문구를 파일 작성 사실로 한정했다 — "이 폴더에서 안전하게 일하도록 기본 설정을 넣어뒀어요" → "처음이라 안전 관련 기본 설정 파일을 새로 만들어 뒀어요". Codex 설정은 전역이라 "이 폴더"라는 표현도 애초에 틀렸는데 함께 해소됐다.
- **차단 2(S0 계약 위반)**: 내가 S0에 "폴더 밖은 절대 못 건드린다고 말하지 않는다"고 써놓고 `tips.know2.codex`에 정확히 그 말을 썼다. **자기모순이었다.** 모든 승인 문구에 "기본 설정에서는" 전제를 붙이고, 코덱스 문구는 "폴더 밖까지 완전히 막아 주는 건 아니니 중요한 파일은 따로 복사해 두세요"로 바꿨다.
- **테스트 공백**: `apply_safety_preset_in(.., home)`으로 홈 경로를 주입 가능하게 분리했다. 실제 `~/.codex`를 건드리지 않고 Codex 경로를 검증한다. 신규 3건 — 전역 설정 신규 생성·재호출 시 `Kept`, **기존 `never + danger-full-access` 설정을 한 글자도 바꾸지 않음**, 직렬화 계약(`{"guide":"applied","config":"kept"}`).
- **검증**: `cargo test` **19 passed**(신규 3건 포함), `pnpm build` 통과.
- **한 가지 유보**: 차단 1의 근거 중 "프로젝트 단위 `.codex/config.toml`이 전역 설정을 덮는다"는 부분은 내가 독립 확인하지 않았다. 다만 CLI 옵션 우선순위만으로도 "파일 작성 ≠ 유효 정책"은 성립해 수정 방향에는 영향이 없다.

**다음 할 일** → **Codex 재검토**. 통과하면 커밋(사용자 승인 필요) 후 S2로 진행

### 2026-08-12 · by GPT-5 Codex

**교차 검토 — Claude의 S1 구현은 빌드되지만 안전 계약 기준 수정 필요 (제품 코드 변경 없음)**

- 구조화 자체는 계획대로 구현됐다. `ProjectInfo.safety`가 `guide`·`config` 각각의 `Applied`/`Kept`를 전달하고, 기존 파일 보존 경로와 혼합 상태 테스트도 통과한다.
- **차단 사항 1 — 안전 상태를 과장한다**: `Applied`는 `write_if_absent`가 파일을 썼다는 뜻일 뿐 실제 적용 여부가 아니다. 특히 Codex는 `~/.codex/config.toml`을 쓰지만 프로젝트 `.codex/config.toml`과 CLI 옵션이 더 높은 우선순위로 덮을 수 있다. 그런데 UI는 곧바로 “이 폴더에서 안전하게 일하도록 기본 설정”이 적용됐다고 단정한다. 파일 작성 상태와 유효 안전 상태를 구분하거나, 문구를 “기본 설정 파일을 만들었다” 수준으로 좁혀야 한다.
- **차단 사항 2 — 팁이 기존 설정과 S0 계약을 무시한다**: `tips.know2.claude-code`는 모든 파일 변경 전 확인을 단정하고, `tips.know2.codex`는 폴더 밖 접근이 막힌다고 단정한다. 이 문구는 `config=Kept`인 기존 사용자에게도 그대로 보이며, 기존 설정이 자동 승인·`danger-full-access`일 수 있다는 감사 결과와 “폴더 밖은 절대 못 건드린다고 말하지 않는다”는 S0 계약에 어긋난다. 기본 모드 기준임을 밝히고 현재 상태를 모르면 보장하지 않는 표현이 필요하다.
- **테스트 공백**: 신규 상태 테스트 3개가 모두 Claude 경로만 실행한다. 전역 홈 경로를 쓰는 Codex의 `Applied`/`Kept`, 기존 파일 보존, 직렬화 결과는 검증되지 않았다. 홈 경로를 주입 가능한 구조로 분리해 실제 사용자 설정을 건드리지 않는 Codex 단위 테스트가 필요하다.
- **독립 검증**: `cargo test --manifest-path src-tauri/Cargo.toml`은 16 passed·6 ignored, `pnpm build`는 통과했다. 처음에는 폴더 rename 뒤 샌드박스 쓰기 허용 경로가 옛 `agent-starter`에 남아 빌드 잠금·Vite 임시 파일 작성이 거부됐고, 승인된 동일 명령으로 재실행해 통과했다. 코드 실패가 아니다.

**결론** → S1은 **교차 검토 미통과·수정 필요**. 위 두 안전 문구 차단 사항과 Codex 테스트를 보완한 뒤 재검토하고, 그전에는 S2로 넘어가지 않는다. 커밋·push는 하지 않았다.

### 2026-08-12 · by Claude Opus 5

**S1 착수 — 문서 정밀도 2건 정정 + 안전 문구 초안 작성 (제품 코드 변경 없음)**

사용자가 §7.9 계획과 S0 안전 계약을 승인해 S1(Claude 1차 담당)을 시작했다. 계획상 **문구 초안은 구현 전에 사용자가 확인**하므로 이번에는 초안까지만 작성했다.

- **Codex가 지적한 정밀도 2건 반영**: §3의 "Codex의 실제 보호는 작업 폴더 경계"를 "샌드박스가 허용한 쓰기 루트 경계(작업 폴더 + 임시 디렉터리)"로 좁혔다. §7.9 S0의 "현재 공통 약속"은 앱이 아직 프리셋 적용 상태를 표시하지 않으므로 "S1 완료 후 제공할 약속"으로 바꾸고, 지금 사실인 것을 따로 분리했다. 과거 워크로그 엔트리는 규칙대로 손대지 않았다.
- **S1 초안을 §7.9에 기록**: ① `ProjectInfo`에 `SafetyStatus { guide, config }`(`Applied`/`Kept`)를 추가해 프리셋 적용 결과를 프론트로 전달, ② `project.done.safe`를 적용 결과별 2개 키로, `tips.know2`를 에이전트별 3개 키(claude-code/codex/general)로 분리. ko/en 초안 포함.
- **추가 발견 2건(S1 표에 없던 것)**: `project.make.desc`의 "다른 파일은 건드리지 않으니 안전해요"는 앱 동작인지 에이전트 작업 범위인지 모호해 후자로 읽히면 부정확하다. 더 중요한 것은 **`tips.agent.claude-code`가 auto mode를 권하는데, 이는 Claude Code의 유일한 실질 보호인 변경 전 승인을 끄도록 권하는 문구**이며 같은 카드의 `tips.know2`와 정면 충돌한다.
- **범위 밖으로 명시**: 기존 Codex 설정이 `danger-full-access`일 때 경고하는 기능은 S1에 넣지 않았다. S1의 목표는 "거짓말하지 않기"이지 "남의 설정 평가하기"가 아니다.

**S1 구현 완료 — 안전 문구·표시 조건 교정 (제품 코드 변경 있음)**

사용자가 초안과 추가 발견 2건의 처리 방향(둘 다 추천안)을 승인해 바로 구현했다.

- `project.rs`: `SafetyStatus { guide, config }` + `PresetOutcome{Applied,Kept}` 추가. `write_if_absent`가 "새로 썼는지"를 반환한다. **기존 파일을 덮어쓰지 않는 동작은 그대로**다.
- `App.tsx`: 졸업 직전 안내를 `safety.config` 결과로 분기해 **적용하지 않았으면 "넣어뒀다"고 말하지 않는다.** `UsageTips`는 `agent` 유무로 승인 문구 3종을 고른다.
- `locales/{ko,en}.ts`: `project.done.safe` → `.applied`/`.kept`, `tips.know2` → `.general`/`.claude-code`/`.codex`. `project.make.desc`는 앱 동작 한정으로 좁히고, `tips.agent.claude-code`는 auto mode가 **파일 변경 전 확인 절차도 함께 끈다**는 사실을 명시했다.
- 신규 테스트로 `guide=Kept` · `config=Applied`가 따로 보고되는지와 기존 파일 보존을 확인했다.
- **검증**: `cargo test` 16 passed(신규 2건 포함), `pnpm build` 통과, 제거한 키 잔존 참조 0건, ko/en 키 1:1 대조 완료.
- **함정 발견 — 폴더 rename 잔재**: `cargo test`가 옛 경로 `projects/agent-starter/...`를 찾다 빌드 스크립트에서 실패했다. `target/`에 rename 전 절대경로가 남아 있던 것이며 `cargo clean`(12.6GiB 제거)으로 해결했다. 같은 증상은 재빌드가 아니라 clean이 답이다.

**다음 할 일** → **Codex 교차 검토**(§7.9 S1의 검토 담당). 그다음 S2(Codex 1차 담당, `--approve-for-me` 없는 복제본 파일럿 경계 구현). 커밋은 사용자 승인 대기

### 2026-08-12 · by GPT-5 Codex

**검토 — Claude의 교차 검토·경계 정정 확인 (제품 코드 변경 없음)**
- Claude가 Codex 교차 감사 결과를 직접 재실측해 `patch/file_change`의 프로젝트 경계와 셸의 샌드박스 쓰기 루트를 구분한 정정은 타당하다. 모델의 자체 거절을 강제 증거로 보지 않고 실제 종료 코드·결과 파일로 판정한다는 감사 원칙도 재사용 가치가 높다.
- `--approve-for-me` 호스트 파일럿 제외, 사용자 중계 제거를 M4 필수 통과 조건으로 둔 결정, §7.9 공동 역할 분담에 Claude와 Codex의 결론 충돌은 없다.
- 남은 문서 정밀도 문제 2건: §3의 “Codex의 실제 보호는 작업 폴더 경계”라는 이전 표현은 임시 쓰기 루트 예외와 충돌하므로 “샌드박스가 허용한 쓰기 루트 경계”로 좁혀야 한다. §7.9 S0의 “현재 공통 약속”은 앱이 아직 프리셋 적용 상태를 보여주지 않으므로 “S1 완료 후 제공할 약속”으로 표현해야 한다.
- 최신 문서 변경은 `m4/orchestration`의 `f8eb1ab`에 커밋돼 있고 워킹 트리는 검토 시작 시 깨끗했다. 제품 코드 변경은 없다.

**다음 할 일** → 위 문구 2건을 제안서에 정정한 뒤 사용자가 S0 계약·§7.9 계획을 승인하면 S1 문구 초안 진행

### 2026-08-12 · by Claude Opus 5

**교차 검토 — Codex 반영분 확인 및 내 감사 결론 1건 정정 (제품 코드 변경 없음)**

Codex가 §1·§3·§4.3·§7.6·§7.7·§7.9·§8에 반영한 내용을 읽고, 내 감사 결과와 어긋나 보이는 지점을 재실측했다.

- **내 결론이 틀렸다 — Codex가 옳았다.** 내 시험 A는 "작업 폴더 밖 쓰기가 거부됨 → 폴더 경계가 강제됨"으로 결론냈는데, 이는 `patch`/`file_change` **도구에만** 해당한다. **셸 명령은 임시 디렉터리 같은 샌드박스 쓰기 루트에 쓸 수 있다.** 재실측에서 `/private/tmp` 형제 경로 셸 쓰기가 `exit 0`으로 성공하고 파일이 생성됐다. §3에 `경계 정정` 표를 넣고 §9에 철회 항목으로 남겼다.
- **재사용할 감사 방법론 함정**: 재확인 1차에서 Codex가 *"허용된 작업공간 밖이므로 실행할 수 없습니다"* 라고 답해 경계가 강제되는 것처럼 보였다. 그러나 이는 **샌드박스 거부가 아니라 모델의 자체 거절**이었고, "판단하지 말고 실행만 시도하라"로 다시 요청하니 성공했다. **에이전트의 "못 한다"는 말을 강제 증거로 삼으면 안 되고, 안전 경계는 결과물(파일 존재·종료 코드)로만 판정한다.** §9에도 철회 항목으로 기록했다.
- `--approve-for-me`와 `--sandbox` 동시 지정 불가라는 Codex 보고도 확인했다(`error: the argument '--approve-for-me' cannot be used with '--sandbox'`).
- Codex의 핵심 기여 3건에 동의한다: ① M4의 첫 가치를 "사용자가 전달자가 되지 않게 한다"로 재정의(§1), ② `--approve-for-me`가 저장소 밖 쓰기까지 자동 승인하므로 **폐기 가능한 복제본은 안전 경계가 아니다** → 호스트 기반 파일럿에서 사용 철회(§3), ③ 사용자 중계 횟수를 품질과 독립된 필수 통과 조건으로 승격(§7.6).

**다음 할 일** → **§7.9 계획과 S0 안전 계약의 사용자 승인 대기.** 승인되면 S1(문구 정정 + 프리셋 적용 결과 구조화)이 Claude 1차 담당이며, 문구 초안을 구현 전에 사용자가 확인한다

### 2026-08-12 · by GPT-5 Codex

**제품 정의 — M4의 최우선 문제를 사용자 중계 제거로 확정 (코드 변경 없음)**
- 사용자가 이번 Claude·Codex 교차 논의에서도 두 대화를 오가며 계속 결과와 다음 지시를 전달해야 했고, 바로 이 불편이 M4 구상의 직접 원인이라고 재확인했다.
- `history.md`와 제안서 같은 공유 기억은 컨텍스트 소실을 줄이지만, 다음 에이전트 호출·결과 인계·진행 제어까지 자동화하지는 못한다. 따라서 M4의 첫 가치를 “사용자는 한 곳에 말하고 앱이 Claude와 Codex를 이어 사용자가 전달자가 되지 않게 한다”로 제안서 §1에 명문화했다.
- 파일럿의 핵심 UX 지표에 사용자 전달 횟수·대화/앱 전환 횟수·전문 로그 해석 필요 여부를 추가했다. 사용자가 “상대 결과를 읽어라”라고 직접 호출해야 하면 품질 결과와 관계없이 실패로 판정한다.

**다음 할 일** → 안전 개선 계획을 유지하되, 이후 M4 파일럿은 자동 인계의 최소 수직 흐름을 최우선으로 설계

### 2026-08-12 · by GPT-5 Codex

**계획 — Claude·Codex 공동 안전 개선 순서와 역할 정의 (제품 코드 변경 없음)**
- 사용자의 “Claude도 알아야 하므로 계획부터”라는 판단을 반영해 M4 제안서 §7.9에 공동 실행 계획을 추가했다. 권장안은 잘못된 문구만 고치는 것이 아니라 **문구 정정과 동작 강화 둘 다**다.
- 순서는 `S0 안전 계약 사용자 승인 → S1 현행 안전 문구·표시 조건 교정 → S2 --approve-for-me 없는 복제본 파일럿 → 효과 확인 → S3 실제 프로젝트용 app-server 승인·Git 복원 스파이크`로 고정했다.
- Claude는 현행 안전 프리셋 상태 전달·사용자 문구를 1차 담당하고 Codex가 검토한다. Codex는 파일럿 실행 경계·app-server를 1차 담당하고 Claude가 검토한다. 각 단계는 자기 결과만으로 완료 처리하지 않는다.
- 이 계획의 사용자 승인 전에는 제품 코드 구현을 시작하지 않는다. 모든 작업은 `m4/orchestration` 계열에서 진행하고 최종 main 병합 승인 규칙도 유지한다.

**다음 할 일** → 사용자가 §7.9 계획과 S0 안전 계약을 승인하면 S1 문구 초안부터 작성

### 2026-08-12 · by GPT-5 Codex

**교차 안전 감사 — Codex 삭제·샌드박스·자동 검토 경계 보강 (제품 코드 변경 없음)**
- Claude의 Codex 감사 결과를 별도 폐기 Git 저장소에서 재현했다. `untrusted + workspace-write`에서 파일 생성·`file_change` 삭제·단일 파일 `/bin/rm`은 승인 없이 실행됐고, `/bin/rm -rf`와 비임시 경로의 저장소 밖 셸 쓰기는 승인 왕복 없이 거부됐다. `/private/tmp` 형제 경로 쓰기는 허용돼 임시 경로가 별도 쓰기 범위임도 확인했다.
- `--approve-for-me`는 `--sandbox workspace-write`와 동시 지정할 수 없으며, 자동 검토가 `/bin/rm -rf`와 비임시 경로의 저장소 밖 디렉터리 생성·파일 쓰기를 모두 허용했다. `--ephemeral` 유무와 관계없이 `rm -rf`가 허용됐다.
- 결론: 폐기 가능한 Git 복제본만으로는 자동 검토의 안전 경계가 되지 않는다. 호스트 기반 M4 파일럿의 `--approve-for-me` 사용을 철회하고, 전체 프로세스가 사용자 파일시스템과 분리된 VM·컨테이너에서만 재검토한다.
- 감사에 사용한 비임시 외부 경로 `/Users/wonseok-han/projects/hello-agent-codex-audit-outside-20260812`에는 테스트 파일 하나만 있음을 확인한 뒤 즉시 제거했다. 실제 프로젝트와 사용자 전역 Codex 설정은 변경하지 않았다. 상세 결과는 M4 제안서 §3에 통합했다.
- 실행 중 로컬 프로젝트 폴더가 `agent-starter`에서 `hello-agent`로 rename된 사실을 확인했고, 이후 명령은 새 경로와 `m4/orchestration` 브랜치에서 수행했다.

**다음 할 일** → 안전 문구·동작 불일치에 대한 제품 결정. M4 품질 비교 파일럿은 `--approve-for-me` 없이 설계

### 2026-08-12 · by Claude Opus 5

**안전 감사 실측 — 현행 안전 문구와 에이전트 실제 동작 대조 (제품 코드 변경 없음)**

M4 §8의 1순위였던 현행 안전 감사를 수행했다. 격리 픽스처에 앱이 만드는 프리셋을 그대로 재현해 두 에이전트를 실제로 실행했다. **상세 결과·표·후속 결정은 `docs/proposals/m4-orchestration.md` §3 `감사 결과`가 단일 기준점이다.**

- **Claude Code는 문구와 일치**: 파일 변경 전 승인을 실제로 요구했고(비대화형에선 차단), `.env` deny 규칙도 작동(대조군으로 같은 폴더 다른 파일은 정상 읽힘 확인).
- **Codex는 문구와 불일치**: `approval_policy="untrusted"` + `sandbox_mode="workspace-write"`에서 **작업 폴더 안 파일 수정과 임의 쉘 쓰기 명령이 승인 없이 그대로 실행**됐다. 실제 보호는 승인이 아니라 **작업 폴더 경계**다(폴더 밖 쓰기는 `patch rejected`로 거부 확인).
- **구조적 결함**: `apply_safety_preset`의 `write_if_absent`는 기존 파일을 덮어쓰지 않는데, `project.done.safe` 문구는 프로젝트 생성 성공 시 무조건 표시된다(`src/App.tsx:1239-1247`). **설정을 하나도 안 썼어도 "안전장치를 넣어뒀어요"라고 말한다.** 이 개발 머신이 실증 사례 — 기존 `~/.codex/config.toml`이 `approval_policy="never"`, `sandbox_mode="danger-full-access"`로 앱 프리셋과 정반대인데 앱은 건드리지 않는다. **기존 Codex 사용자 전원이 이 경로다.**
- **부수 성과**: M4 §5.1의 1순위 열린 질문이 함께 해소됐다. `exec`는 승인 지점에서 막히지 않는다 — 승인 지점 자체가 없기 때문이다. 이 과정에서 기존 추론 2건을 §9 `철회된 주장`으로 옮겼다.
- **미실측(추정으로 표시)**: `Bash(rm -rf:*)` deny의 접두사 매칭 우회 가능성. 권한 우회 모드로 에이전트를 실행해야 확인되므로 사용자 승인 전까지 하지 않았다.

**다음 할 일** → **제품 결정 대기**: 불일치 3건을 문구 정정으로 해소할지, 동작 강화로 해소할지, 둘 다 할지. 사용자 문구를 바꾸는 일이라 명시적 결정이 필요하다. M4 후속 작업(§7.6 과제 정의)보다 이것이 먼저다

### 2026-08-12 · by Claude Opus 5

**인계 — 로컬 폴더 rename 후 세션 소실, 미커밋 문서 상태 재파악 (코드 변경 없음)**
- 사용자가 로컬 프로젝트 폴더를 `agent-starter` → `hello-agent`로 변경해 이전 세션 컨텍스트가 사라졌다. 미커밋 문서 변경 일체(`CLAUDE.md`·`AGENTS.md`·`README.md`·`history.md` 수정, `docs/README.md`·`docs/archive/`·`docs/proposals/` 신규)를 읽고 현재 상태를 재구성했다.
- 확인 결과 미커밋 변경은 **전부 2026-08-12 문서 재편 작업**이며 제품 코드 변경은 없다. 브랜치는 `m4/orchestration`이 맞다.
- **정정 — 폴더명 기록이 사실과 어긋남**: `프로젝트 요약`의 "로컬 폴더명은 `agent-starter` 유지"는 이제 거짓이다. 실제 경로는 `/Users/wonseok-han/projects/hello-agent`이며 해당 줄을 현재 사실로 고쳤다. 문서·코드 전역에서 `agent-starter` 잔존 참조를 검색한 결과 남은 것은 `src-tauri/target/`의 오래된 빌드 지문(gitignore 대상)뿐이고, `src-tauri/Cargo.toml`은 이미 `hello-agent`/`hello_agent_lib`다. 아카이브의 두 건은 당시 기록이라 그대로 둔다.
- 기계 검증: `CLAUDE.md`와 `AGENTS.md`는 `diff` 기준 완전 동일. `history.md` 261줄(롤오버 임계 미만), M4 제안서 401줄, 2026-07 아카이브 210줄. `docs/README.md` 색인이 가리키는 경로 전부 실존.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8. 1순위는 §3 현행 안전 문구·동작 감사. 미커밋 문서 변경은 여전히 커밋 대기(사용자 승인 필요)

### 2026-08-12 · by GPT-5 Codex

**Git 정책 정정 — M4 공동 통합 브랜치를 중립적 이름으로 변경**
- 사용자 지적대로 `codex/m4-orchestration`은 Codex 단독 소유 브랜치처럼 보이며 Claude와 공동 작업하는 통합 기준선의 성격에 맞지 않았다.
- 로컬 브랜치 이름을 `m4/orchestration`으로 변경했다. 브랜치의 기준 커밋, 현재 미커밋 변경과 main 보호 정책은 그대로 유지된다.
- `CLAUDE.md`·`AGENTS.md`·M4 제안서의 분기 및 최종 병합 경로를 모두 `m4/orchestration`으로 정정했다.

**다음 할 일** → 모든 M4 작업은 `m4/orchestration`을 기준으로 진행. 최종 main 병합은 사용자 명시적 승인 필요

### 2026-08-12 · by GPT-5 Codex

**Git 정책 — M4 전용 통합 브랜치 생성 및 main 보호 규칙 확정**
- 사용자 요청에 따라 `main`에서 M4 전용 통합 브랜치를 생성하고 현재 작업 브랜치를 전환했다. 처음 이름은 `codex/m4-orchestration`이었으며, 공동 작업 성격에 맞춰 후속 엔트리에서 `m4/orchestration`으로 변경했다. 기존 미커밋 문서 변경은 손실 없이 새 브랜치에 유지됐다.
- 이 브랜치를 M4의 `main` 대체 통합 기준선으로 지정했다. Claude와 Codex는 M4 작업 전에 브랜치를 확인하며, 기능·실험 브랜치가 필요하면 여기서 분기하고 결과도 여기로만 병합한다.
- M4 작업은 `main`에 직접 커밋·병합·push하지 않는다. 전체 기능과 안전성 검증이 완료된 뒤 사용자가 명시적으로 승인한 경우에만 `m4/orchestration → main` 병합을 수행한다.
- `CLAUDE.md`와 `AGENTS.md`의 Git 작업 규칙, `docs/proposals/m4-orchestration.md` §7.8·§8·결정 이력에 같은 정책을 반영했다.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8. 커밋·push는 별도 사용자 승인 전까지 수행하지 않음

### 2026-08-12 · by GPT-5 Codex

**문서 — M4 오케스트레이션 제안서 정합성 수정 및 파일럿 컨셉 구체화 (코드 변경 없음)**
- `docs/proposals/m4-orchestration.md`의 심층 검토 결과를 본문에 반영했다. 상태를 단순 검토 메모에서 **컨셉 초안 작성 완료·검증 대기**로 갱신했다.
- 안전 절을 실제 코드에 맞게 교정했다. Codex 설정은 프로젝트별이 아니라 `~/.codex/config.toml`이 없을 때만 생성되는 전역 기본값이며, `untrusted + workspace-write`가 모든 파일 변경 전 승인을 보장한다고 단정하지 않는다. UI 안전 문구와 Claude Code·Codex의 실제 동작을 감사하는 일을 전체 1순위로 올렸다.
- OpenAI 공식 문서에 공개된 app-server의 명령 실행·파일 변경 승인 프로토콜과 결정 값을 반영하고, 공개 스펙과 번들 CLI 로컬 실측을 구분했다. SDK 런타임은 앱에 번들할 수 있으므로 사용자 설치 문제로 단정하지 않고 배포·업데이트·유지 비용으로 비교하도록 수정했다.
- §7 파일럿 컨셉을 작성했다. 사용자 여정, 역할별 책임, 정상·분기 상태, 원장 기반 중단·정확한 세션 ID 재개, 복제본/실제 프로젝트 안전 경계, 단독 2조건과 교차 벤더 2조건의 비교 지표, 런타임 기능 저하 정책을 명시했다.
- 역할 방향은 `Claude → Codex`로 미리 확정하지 않고 반대 방향·단일 에이전트와 비교한다. LLM 완료 선언은 단독 근거로 삼지 않으며 작업 유형에 맞는 검증 증거를 요구한다. 철회된 주장과 결정 이력에도 이번 정정을 추가했다.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8. 현행 안전 문구·동작 감사부터 시작

### 2026-08-12 · by GPT-5 Codex

**검토 — M4 오케스트레이션 제안서 내용 정합성 심층 점검 (코드·제안서 변경 없음)**
- `docs/proposals/m4-orchestration.md`의 사실·추론·구현 준비도를 실제 코드와 OpenAI 공식 Codex 문서에 대조했다. 설계 심의 기록과 파일럿 가설은 잘 정리됐지만, 현재 상태는 §7이 미작성인 **연구·결정 메모**이며 바로 구현할 수 있는 완성 컨셉 문서는 아니다.
- §3의 “새 프로젝트마다 기록하는 설정”은 코드와 다르다. Codex 설정은 프로젝트별이 아니라 `~/.codex/config.toml` 전역 파일이고, 해당 파일이 없을 때만 최초 1회 생성한다(`src-tauri/src/project.rs:174-218`). 또한 `approval_policy="untrusted"`는 명령 승인 정책이고 `workspace-write`는 작업 폴더 쓰기를 허용하므로, UI의 “파일을 바꾸기 전에 확인” 약속이 현재 설정만으로 보장된다고 단정할 수 없다. M4 승인 실측과 별개로 현행 안전 문구·실제 동작 감사를 먼저 명시해야 한다.
- §2.3의 app-server 승인 프로토콜 “구체 스펙 미확인”은 공식 문서 기준 오래됐다. 명령 실행·파일 변경 승인 요청과 `accept`/`acceptForSession`/`decline`/`cancel` 결정이 문서화돼 있다. 다만 이 프로젝트의 번들 CLI에서 실제 왕복이 되는지는 여전히 로컬 실측 대상이다.
- §4.5의 “SDK 런타임 요구 = 비개발자가 개발 도구를 설치해야 함”은 지나치게 단정적이다. 런타임을 앱에 번들할 수도 있으므로 사용자 설치 여부가 아니라 배포 용량·보안 업데이트·플랫폼 유지 비용으로 비교해야 한다.
- 그 밖에 “LLM 완료 판정은 신뢰 불가”를 “단독 완료 근거로 삼지 않는다”로 좁히기, 역할 방향을 실험 전 확정하지 않기, 재개 시 `--last` 대신 정확한 세션 ID 저장, §10의 “5라운드”와 6개 전환점 표기 정합화가 필요하다.
- 사용자가 요청한 범위가 검토이므로 제안서 본문은 수정하지 않았다. 다음 단계는 이 검토 내용을 제안서에 반영한 뒤, 성공 지표를 먼저 정의하고 §5.1 승인 동작 및 §5.2 교차 벤더 품질 가설을 실측하는 것이다.

### 2026-08-12 · by GPT-5 Codex

**검토 — M4 제안서 분리와 히스토리 관리 방식 확인 (코드 변경 없음)**
- Claude의 최신 M4 의견과 새 문서 운영 규칙을 실제 파일·Git 상태와 대조했다. M4의 현재 결론·확인 사실·안전 제약·열린 질문·철회된 주장이 `docs/proposals/m4-orchestration.md` 한곳에 정리됐고, `history.md`는 사건 요약과 포인터만 남기는 구조로 바뀌어 이전의 정정 사슬 문제가 해소됐다.
- 생애주기 분류(`현행 사실`·`제안`·`작업 기억`·`지난 기록`)는 제안을 구현된 기능으로 오독하는 위험을 구조적으로 줄인다. `history.md`를 진입점으로 유지하고 오래된 기록만 월별 아카이브로 롤오버한 판단도 적절하다.
- 기계 검증: `history.md` 211줄, M4 제안서 287줄, 2026-07 아카이브 210줄. 2026-07-22 이하 15개 엔트리가 날짜 경계를 지켜 아카이브에 존재하며 포인터도 연결됐다. `CLAUDE.md`와 `AGENTS.md`는 바이트 단위로 동일하고, 문서 색인의 대상 경로가 모두 존재하며 `git diff --check`가 통과했다.
- M4 내용은 추가 수정하지 않았다. 현재 1순위 열린 질문인 복제 저장소의 `untrusted + codex exec` 승인 동작 실측부터 진행한다는 순서에 동의한다.

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8 참조

### 2026-08-12 · by Claude Opus 5 + GPT-5 Codex (교차 검토)

**논의 — M4 멀티 에이전트 오케스트레이션 검토 (코드 변경 없음)**

사용자 구상(대표가 목표를 주면 관리자가 기획자·디자이너·개발자·QA에 배분하고 완료까지 반복)을 두 모델이 5라운드 교차 검토했다. **상세는 `docs/proposals/m4-orchestration.md`가 단일 기준점이다. M4 관련 논의·결정·철회는 전부 그 문서에 쌓는다.**

- **결론**: 파일럿은 순차 2롤(Claude 기획·검토 → Codex 구현), 복제 저장소 한정, 앱은 "관리자"가 아니라 **제어면**(반복 상한·중단·검증 강제, 의미 판단은 역할 LLM). 런타임은 `codex exec`로 시작(파일럿 기준 잠정)
- **차별 요소**: 교차 벤더 인계 + 비개발자 제어 + 증거 기반 완료. **역할 기반 팀 자체는 차별점이 아니다**(Claude Code Agent Teams가 이미 상당 부분 제공)
- **구속력 있는 제약(신규 확인)**: 제품이 `approval_policy="untrusted"`(`project.rs:126,131`)와 사용자 문구(`ko.ts:104`·`185`)로 **승인을 이미 약속**하고 있다. M4가 임의로 완화할 수 없다. 중단 버튼은 승인의 대체물이 아니다
- **1순위 열린 질문**: `untrusted` + `codex exec`로 실제 파일 수정을 시켰을 때 승인 지점이 어떻게 동작하는가. **이 답이 M4 런타임 선택을 결정한다**(미검증)
- **논의 중 철회된 주장 5건**은 `proposals/m4-orchestration.md` §9에 정리. 재론 방지용이므로 M4를 다루기 전 반드시 확인할 것
- 실측 확인: Claude Code `2.1.228`, Codex CLI `0.147.0-alpha.6.6`. `codex exec`는 `--json` 스트리밍·`exec resume`·`--output-schema`·`--output-last-message` 지원, **실행 중 개별 승인 왕복만 미지원**
- **부수 발견(M4 무관·단독 적용 가능)**: `project.rs:272` `clean_reply`가 Codex 응답을 "마지막 문단"으로 추정하는데, `--output-last-message`로 대체 가능. 현재는 첫 대화가 짧아 문제되지 않으나 답변이 길어지면 깨진다

**문서 구조 변경 — 워크로그 비대화 대응 (사용자 요청)**
- 계기: M4 논의가 하루 만에 엔트리 8개·155줄(파일의 30%)로 불어나 다른 기록을 압도. 철회·정정이 엔트리에 걸쳐 사슬처럼 얽혀 **"지금 뭐가 참인지" 파악에 엔트리 4개 역추적이 필요**했음. 사용자가 단일 파일 관리 부담을 지적
- **① 주제 분리** — 설계 심의는 워크로그의 종류가 아니라고 판단해 `docs/proposals/m4-orchestration.md` 신설(287줄). 결론·확인된 사실·안전 제약·열린 질문·**철회된 주장 5건 표**로 재구성. 워크로그에는 요약과 포인터만
- **② 아카이브 롤오버** — `docs/archive/` 신설. 2026-07-22 및 그 이전 15개 엔트리를 이동. **525줄 → 196줄**
- **③ 규칙 명문화** — `CLAUDE.md`·`AGENTS.md`와 이 파일 헤더에 롤오버 기준(500줄 초과 시 300줄 이하로, 날짜 경계에서, 이동이지 삭제 아님)과 워크로그/설계문서 경계를 기재
- **날짜별 파일 분할은 기각**: 현재 진실이 여러 파일로 흩어져 조회 비용만 커짐 — 이번에 겪은 문제를 오히려 키우는 방향. 날짜별 조회는 이미 `git log`가 제공하며, 이 문서의 일은 변경 로그가 아니라 **읽을 수 있는 현재 상태 기억**
- 판단 기준(재사용): 워크로그는 **무엇이 일어났나**를, 설계 문서는 **무엇이 참인가**를 기록한다. 토론 왕복은 후자로 정리해 넣는다

**문서 분류 체계 도입 (사용자 요청)**
- 사용자 지적: "docs에 단순히 밀어넣지 말고 분류별로 정리하라". 분류 기준을 주제가 아니라 **생애주기**(이 문서가 무엇을 주장하는가)로 잡음
- 가장 큰 위험이었던 것: **`m4-orchestration.md`(아직 제안)가 `architecture.md`(현행 사실)와 같은 층에 있어** 다음 작업자가 제안을 구현된 사실로 오독할 수 있었음 → `proposals/`로 분리해 경계를 구조로 표현
- 구조: `docs/README.md`(색인·규칙 신설) · `history.md`(진입점, 최상위 유지) · `architecture.md` · `release-checklist.md` · `diagrams/` · **`proposals/`**(제안) · **`archive/`**(지난 기록)
- **`history.md`는 최상위에 그대로 둔다** — 두 에이전트가 가장 먼저 읽어야 하는 진입점이라 한 단계 내리면 접근성만 나빠지고 얻는 게 없음
- 참조 갱신: `CLAUDE.md`·`AGENTS.md`·루트 `README.md`·`history.md`·아카이브 헤더
- **함정 발견·수정 — `AGENTS.md`가 `CLAUDE.md`와 어긋나 있었음**: 롤오버 규칙과 M4 포인터가 `CLAUDE.md`에만 들어가 **Codex는 규칙을 모르는 상태**였음. 두 파일은 내용이 동일해야 하는데(파일명만 다름) 한쪽만 고치면 다른 에이전트가 규칙 없이 작업하게 됨 → 동기화하고, **"두 파일은 같은 내용을 유지한다"를 규칙으로 명문화**

**다음 할 일** → `docs/proposals/m4-orchestration.md` §8 참조
