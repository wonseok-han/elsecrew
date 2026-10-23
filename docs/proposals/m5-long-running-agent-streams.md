# M5 장시간 단일 실행 출력 계약

상태: 2026-10-23 사용자 방향 승인 후 작성한 **구현 전 설계안**. 현재 제품 동작이 아니다.

## 1. 확정된 사용자 결정과 범위

- 사용자는 여러 짧은 가짜 실행으로 부하를 나누는 대신 **장시간 단일 실행 계약부터 재설계**하기로 선택했다.
- 목표: 같은 run의 공개 출력이 256개를 넘어도 안전하게 지속되고, 네 채팅의 독립 실행 각각 최소3,000개 공개 이벤트를 저장·표시·다시 조회한다.
- 기존 실행을 자동으로 여러 run으로 분할하거나 3,000개 사용자 메시지 저장으로 대체하지 않는다. 모델 비용이 없는 fake provider가 동일 runtime/저장/IPC/UI 경로를 통과해야 한다.
- 비용·시간·디스크가 무제한이라는 뜻은 아니다. 아래 수치는 설계 제안이며 아직 활성화하지 않는다. 실제 계정 시험, 원본 쓰기, main 병합/push 승인은 별도다.

## 2. 현재 코드에서 바꿔야 할 지점

| 지점 | 현재 제약 | 재설계 책임 |
| --- | --- | --- |
| `crates/hello-agent-protocol/src/lib.rs` | run당 공개256개/본문1MiB/누적 snapshot16MiB, 세션 event ID8,192개 | v1 제한 보존, 새 출력 저장/전송 capability와 별도 예산 |
| `hello-agent-core/src/provider.rs`, `provider/vendor_observation/{codex_events,claude_events}.rs` | 내부1,024·공개256 이벤트 및 누적 observation byte 제한 | 새 capability에 결속된 incremental parser/관측 예산; 실제 두 공급자와 fake 경로를 모두 포함 |
| `agent_runtime.rs`의 `PublicEventBatch::push`, `persist_public_event_record`, budget/projection 검사 | 과거 `public_events` 배열을 복제하고 누적 snapshot 계산 | append-only row와 작은 run head로 분리 |
| `storage.rs` 및 storage admission/replay | 기존 event 원장과 projection 의미 검증 | 새 출력 이벤트 형식·head·페이지의 transaction/replay 검증 추가 |
| `electron/sidecar-client.ts` | 세션 전체 eventId Set, stream/run 신원 보관 | 순서 watermark와 bounded replay 창; 종료된 신원 수명 명시 |
| `electron/event-credit.ts` | stream32/global128 in-flight 초과 시 실패 | bounded page 전달과 ACK; 조용한 drop/무한 queue 금지 |
| TS/Rust adapter 계약, chat 조회/UI | bounded 전체 원장 배열을 예상 | 기존 v1을 유지하고 cursor 기반 새 경로로 분리 |

Rust sidecar가 신원·출력 commit·순서의 권위다. Electron/renderer의 ACK나 화면 표시가 저장 성공 또는 종료 증거가 되어서는 안 된다.

## 3. 기록 모델과 원자성

### 제안 기록

- `run_output_event`: `(run_id, output_sequence)` 유일키, event_id 유일키, 공개 kind/text, 기존 전체 run/chat/project/worktree/profile 결속, UTF-8 byte 길이.
- `run_output_head`: 저장 형식 버전, committed_sequence, event_count, text_bytes, terminal_sequence(없으면 null), 기존 run 신원. 큰 본문 배열은 넣지 않는다.
- 기존 envelope `seq`는 연결별 전송 순서이며 `output_sequence`는 재시작 후에도 유지되는 실행별 영속 순서다. 둘을 혼용하지 않는다.
- 새로운 원장 이벤트가 권위이고 output table/head는 검증 가능한 파생 데이터로 둔다. 기존 projection snapshot 안에 출력 전체를 다시 넣지 않는다. 정확한 SQLite migration 번호·이벤트 이름·bridge 버전은 현재 등록표와 충돌 없이 L1에서 동결한다.
- L2 구현 구체화: 공개 본문은 전용 `run_output_records` 원장에 한 번만 저장하고 `run_output_heads`만 파생 요약으로 둔다. 기존 전체 projection 원장에 본문을 중복 저장하지 않는다. run 시작/폐쇄와 원장의 결속, 두 방향 R3 검증을 새 migration/admission에 포함하기 전에는 활성화하지 않는다. 위 논리 event/head 이름은 실제 SQL 이름과 구별한다.
  - 후속 문맥 예외: 종료 확인 후 `final` 본문 한 건은 기존 `teamMessage` 문맥 스냅샷에도 저장한다. 진행 delta/status 전체를 복제하는 것은 아니며 기존 message ID 선택·보존/압축·문맥 동의 계약을 그대로 재사용하기 위한 bounded 복사다. 출력 final과 chat snapshot을 같은 transaction에서 저장하고 chat sequence 경합 또는 출력 실패 시 모두 rollback한다. 이 복사 비용도 저장 예산에 포함하며 런타임/재시작 통합 회귀가 활성화 선행 조건이다.

### commit 규칙

1. 공급자 raw 입력을 기존 parser/공개 정보 필터로 검증한다. 비공개 추론·인증정보는 출력 원장에 넣지 않는다.
2. 기존 run 결속, 현재 실행 권한, 다음 순서와 byte/storage 예산을 확인한다.
3. 원장 append + output row + head/counters를 **동일 SQLite transaction**으로 commit한다. 실패 시 전부 rollback한다.
   구현에서 canonical output row 자체가 전용 원장이므로 본문을 이중 insert하지 않는다. 외부 run/fence 갱신과 같은 transaction에 포함하며 helper 오류를 잡고 일부만 commit해서는 안 된다.
4. commit된 항목만 live 전달 대상으로 공지한다. commit 후 전송 실패는 데이터 재조회로 복구하며 모델 작업을 재실행하지 않는다.
5. terminal은 마지막 공개 출력 commit 뒤 같은 순서 체계로 영속화한다. 프로세스 종료 확인 없는 terminal은 성공으로 승격하지 않는다. 실패·취소·불확실 종료의 기존 잠금/복구 계약을 유지한다.

SQLite WAL·원장·파생 데이터로 디스크 사용량이 증폭될 수 있다. 논리 text byte만으로 저장공간 충분을 판단하지 않는다. 디스크 고갈 시 완료로 표시하지 않고 기존 안전 중단/복구 경로로 전환한다.

ENOSPC로 terminal/복구 기록 자체도 실패할 수 있다. 실패를 성공·잠금 해제로 해석하지 않는다. 실행 시작 전 영속 run/fence를 확보하고 저장 실패 시 프로세스 정리를 시도하되 기존 fence를 유지한다. 재시작 시 미종결 run/fence는 종료를 새로 검증하기 전 recoveryRequired로 취급한다. 시작 fence의 commit 실패는 공급자 spawn을 금지한다. L1에서 이 순서와 기존 fence 구현의 부합 여부를 확인하고, L2/L3에서 출력·terminal·복구 기록의 각 commit 전후 ENOSPC/강제 중단 회귀를 고정한다. 여유 공간 검사는 이 계약을 대체하지 않는다.

## 4. 조회·전달·재연결

- 새 고정 typed API 제안: `agent.outputPage(run identity, afterSequence, throughSequence, limit)` 및 출력 head 변경 알림. 임의 SQL·파일 경로·범용 IPC는 노출하지 않는다.
- 최초 조회에서 committed high-watermark를 고정하고 그 이하를 keyset pagination한다. cursor는 run/저장 형식/검증된 head 범위에 결속한다. 다른 run의 cursor, 범위를 넘는/음수/unsafe integer 순서는 거절한다.
- live 알림은 **새 저장 head가 있음**을 알리는 힌트다. 힌트 합치기는 허용하되 실제 출력 row 누락은 허용하지 않는다. renderer가 cursor부터 durable page를 읽으므로 알림 횟수를 출력 이벤트 수로 세지 않는다.
- 구독 등록 완료를 확인한 뒤 head 조회 → 연속 page 조회 → head 재조회를 수행한다. 구독 중 dirty watermark를 유지하여 조회 중 새 commit을 놓치지 않는다. 알림 단절은 재구독·head 재조회로 복구하며 연결이 유지된 경우에도 bounded 주기 head reconciliation을 둔다. terminal을 포함한 마지막 commit 이후 추가 알림이 없어도 반드시 따라잡아야 한다. 정확한 구독 handshake·주기·동시 조회 상한은 L1에서 동결하고, 초기 조회/구독/마지막 terminal 경주를 L3에서 시험한다.
- renderer ACK는 받은 page의 마지막 연속 output_sequence와 전송 envelope를 결속한다. 앞선 ACK, 다른 run ACK, 중복 transport ACK로 credit을 늘릴 수 없다.
- 전달 중 renderer가 느려지면 읽기/전송을 bounded하게 제한한다. 기존 공급자 stdin/stdout 소비를 무조건 멈추어 deadlock시키지 않는다. 영속 writer는 자신의 제한 안에서 계속되며 저장/ingress 예산 초과 시 안전 중단한다.
- reconnect 시 새 session identity와 영속 cursor를 검증해 저장 범위를 다시 받는다. 같은 row의 재전달은 idempotent 표시지만 동일 sequence의 다른 내용은 손상으로 거절한다.
- 세션 전체 eventId Set를 무한 확장하지 않는다. SQLite 유일성 + 영속 sequence + bounded 전달 창으로 중복/역전을 검증한다. 오래된 run의 늦은 live 이벤트는 활성 결속 없이는 받지 않는다. 과거 출력은 페이지 API에서만 읽는다.

## 5. 자원 상한 제안 — 승인된 수치 아님

| 경계 | 초기 제안 | 초과 처리 |
| --- | --- | --- |
| 단일 전송 frame | 기존1MiB control line 유지 | protocol 실패 |
| 출력 page | 최대100개, encoded512KiB 이하 | byte 상한에 맞춰 다음 페이지; 한 항목이 안 들어가면 계약 오류 |
| 공급자 ingress RAM | run당4MiB/global16MiB | 예산 종료·기존 프로세스 정리, 불확실하면 recoveryRequired |
| renderer 유지 | 선택 run 최근200개 + 열린 과거 page1개 | 오래된 화면 row만 퇴거, 디스크 기록은 보존 |
| run 공개 본문 |64MiB | 사용자에게 출력 저장 한도 설명 후 안전 중단 |
| 전체 출력 저장 물리 예산 |1GiB, 시작 전 최소256MiB 여유 제안 | 새 실행 거절/활성 실행 안전 중단; 자동 기록 삭제 없음 |

page 직렬화는 항목 수만이 아니라 UTF-8/JSON escaping 후 bytes로 제한한다. 현행 단일 공개 본문은64KiB이며 JSON escaping 시384KiB가 될 수 있어 초기256KiB 제안을512KiB로 조정했다. 전체 transport envelope는 별도로 기존1MiB 경계를 검증해야 한다. 하나의 public event를 UI가 임의 분할해 event 수를 부풀리지 않는다. 시간 제한도 별도다: 현행 run 총10분 제한을 조용히 없애지 않고, 장시간 실행의 사용자 선택 시간 예산·최대값·취소 응답 시간을 L1에서 동결한다.

## 6. UI와 초보자 경험

- 항목별 공개 출력은 run 결속 안의 `itemId`·`channel(message/plan)`·`operation(append/complete)`을 본문과 원자 저장한다. `append`는 해당 항목의 텍스트에 이어 붙이고 `complete`는 누적 텍스트를 완성 본문으로 교체한다. 항목 완성은 프로세스 종료가 아니며 `final` 공개 이벤트는 기존 종료 확인 뒤에만 허용한다. 오래된 page에는 이 선택 정보가 없을 수 있어 독립 메시지로 표시한다. 새 V11은 아직 배포/활성화 전인 초안이며 이 열을 추가한 schema와 과거 개발용 V11 DB를 동일하다고 가정하지 않는다.

- 현재 진행 중인 출력과 완료/검토 상태를 구분하고, 긴 기록은 ‘이전 내용 보기’로 가져온다. 화면 row 가상화와 bounded 텍스트 모델을 사용한다.
- 프로젝트/채팅 전환은 실행을 다시 시작하지 않는다. 새 화면은 durable cursor부터 따라잡고, 따라잡기 중 상태를 표시한다.
- 취소·승인 질문·오류·복구 안내는 일반 텍스트와 분리해 숨겨지지 않게 한다. 페이지 fetch가 늦어도 취소 요청 경로를 막지 않는다.
- ‘출력 저장 중’, ‘이전 기록 불러오는 중’, ‘저장공간 부족으로 중단’은 ko/en으로 제공한다. 조용한 잘림과 자동 삭제는 없다.
- UI의 과거 출력과 다음 모델 입력 문맥은 별개다. 저장한 모든 출력을 다음 요청에 자동 재전송하지 않으며 기존 문맥 선택/사용자 확인 계약을 유지한다.

## 7. 기존 데이터·재시작 안전

- v1의 최대256개 배열 기록은 원래 검증기를 유지해 읽는다. 새 validator로 과거 손상/상한 초과 기록을 합법화하지 않는다.
- 명시 버전/capability 협상 전에는 새 runtime을 시작하지 않는다. 구버전 앱이 새 저장소를 만났을 때 fail-closed하는 최소 독자 버전도 필요하다.
- 이관은 재시작 가능한 operation/checkpoint로 진행하고 v1 event 순서·ID·내용·결속과 새 row/head를 대조한다. 부분 이관을 ready로 공개하지 않는다. 원본 기록은 승인된 별도 보존 정책 전까지 삭제하지 않는다.
- R3 storage admission의 source/replay/mirror 검증에 새 원장 유형과 output table/head를 포함한다. 큰 기록을 단일 JSON으로 다시 모아 검증하지 않고 페이지 단위 검증 후 전역 완료 fence를 세운다.
- migration/replay 중인 DB에서 실행·반영·복원 쓰기를 활성화하지 않는다. 기존 poisoned 잠금, legacy pending, 종료 신원 확인을 우회하지 않는다.

## 8. 단계별 구현·합격 증거

| 단계 | 소유 변경 | 반드시 남길 증거 |
| --- | --- | --- |
| L1 계약 동결 | protocol/TS golden fixture, capability, schema 및 수치 | 정상3,000·잘못된 cursor/결속/oversized 거절; 기존 v1 fixture 유지; 독립 검토 |
| L2 저장·replay | SQLite append/head/page, 이관·R3 admission | commit 경계 실패 주입, 중복/누락/손상/디스크 고갈, 재오픈 일치; 본문 길이에 선형인 기록량 |
| L3 runtime·전송 | output commit→notice/page, credit, 종료 | 단일 fake run3,000개; 중단·단절·늦은 출력·느린 소비자·재접속에서도 권한/종료 보존 |
| L4 UI | 페이지/가상화·tail·재조회·취소·ko/en | 긴 출력 중 채팅 전환/이전 내용 보기/취소, 메모리 bound, 원문 유실 없음 |
| L5 통합 | 실제 packaged4chat/4run 부하와 기존 회귀 | 각 단일 run3,000개·50events/s·60초, 저장/수신/paint·재조회 ID와 순서 대조, 종료/메모리·독립 안전 검토 |

구현은 메인이 진행하며 공용 계약과 저장 admission부터 순차 연결한다. helper마다 전체 테스트/빌드를 반복하지 않고 위험 부분 회귀 뒤 파동 전체를 검증한다. 기존 제품 경로는 새 경로의 누적 검증과 독립 안전 검토 전 교체하지 않는다.

L1/L3의 공급자 범위에는 core parser와 Codex/Claude observation adapter를 포함한다. 기존 v1의256/1,024 경계 거절을 보존하고, 새 capability에서3,000개·byte overflow 안전 종료를 각각 시험한다. 누적 배열 상한만 올리지 않고 bounded incremental 관측으로 전환하며 실행권한·공개 정보 정화 검증은 유지한다. fake 성공만으로 실제 공급자 parser 호환을 선언하지 않는다.

## 9. 성능 시험 의미

- 생성 시각·native 수신·durable commit·renderer 수신·화면 표시를 분리한다. 이벤트 수는 fake provider가 만든 canonical public event 수이며 IPC page/알림 수가 아니다.
- ‘50events/s 예정’만으로 달성을 선언하지 않는다. 실제 발행 간격·최대 지연·60초 구간 내 건수·backlog를 기록한다. 저장 후 늦게 따라잡은 결과를60초 성공으로 바꾸지 않는다.
- 4chat 각각 단일 run이며 run 재시작으로 상한을 우회하지 않는다. 네 채팅의 순서·중복·유실·격리를 전체 확인하고 선택 화면의 paint와 백그라운드 수신을 구분한다.
- 실패 자료를 보존하며 기존 사용자 메시지 부하 probe는 그 이름과 한계를 유지한다. 기존 부분 수집 JSON을 canonical G6 합격 파일로 합성하지 않는다.

## 10. 다음에 동결할 열린 사항

1. 저장 schema/bridge capability 이름과 migration 최소 독자 버전: 기존 V8 storage 경계와 public bridge 번호는 별개이므로 이름만 보고 증가시키지 않는다.
2. §5 수치와 한 항목 최대 크기, run 시간 예산. 기존 제한보다 완화되는 항목은 명시 계약과 독립 검토 뒤 활성화한다.
3. cursor/checkpoint digest의 정확한 canonical 표현과 오래된 cursor 만료/재조회 규칙.
4. 보존/사용자 선택 삭제 UI와 물리 디스크 quota 산정: 이번 설계는 자동 삭제를 허용하지 않는다.

이 문서의 채택은 Windows 실행 활성화·실제 계정 시험·출시 승인이 아니다. 시작 메모리400MiB 기준 실패와 다른 M5 잔여도 그대로 남는다.

## 11. 구현 체크포인트

- L1 일부: `crates/hello-agent-protocol/src/agent_adapter_v1/output_pages.rs`에 독립 출력 형식 버전1, 요청 결속·연속 순서·페이지100개/512KiB 검증 초안을 추가했다. 기존 public event의 단일 항목 검증을 재사용하며 전체 v1 원장256개 제한은 변경하지 않는다.
- 아직 capability 광고·wire method 등록·저장 schema 변경·runtime 연결은 없다. 페이지 검사기는 요청 watermark가 실제 commit됐다는 증거가 아니며 저장 계층에서 별도로 검증해야 한다. cross-page event ID 유일성은 영속 원장의 책임이다. TS 대응·golden fixture·head/구독 계약·L1 독립 검토가 남아 있다.
- 후속 체크포인트: `src/m5/contracts/agent-output-pages.ts`의 TS 대응과 `tests/fixtures/m5/agent-output-page.json` 공유 fixture를 추가했다. Rust/TS 모두 같은 마지막3,000번째 page를 검증하는 회귀를 작성했으며 이 추가분은 다음 계약 연결과 묶어서 실행한다. head/구독·저장 계약과 L1 독립 검토는 계속 남는다.
- 후속 체크포인트: Rust/TS `OutputHead`는 run 결속·committedSequence·sealed를 포함한다. sealed는 출력 폐쇄이지 종료/성공 증거가 아니다. `src/m5/agent-output-reader.ts`는 구독 완료 후 catch-up, dirty 알림 병합, 단일 page 소비, head 재조회와 stop 경계를 구현했다. source는 구독 등록 완료를 보장하고 abort에 응답해야 하며 호출자가 정기 reconciliation과 새 연결의 reader 재생성을 담당한다. 아직 실제 IPC source/화면/저장소에 연결되지 않았고 관련 회귀는 작성 상태다.
- L2 일부: `storage/agent_output.rs`에 transaction-scoped begin/append/seal/head/page와 전용 원장 schema 초안을 추가했다. 호출자가 write admission과 전체 transaction rollback을 소유한다. schema 등록·기존 데이터 이관·R3 양방향 검증·물리 quota·run/fence 원자 결속은 아직 없고 제품 DB에는 새 테이블을 생성하지 않는다. 3,000개 저장/조회 및 rollback 회귀는 작성 상태이며 실행 전이다.
- L2 검증 primitive: `storage/agent_output/verification.rs`는 connection을 독점 차용한 volatile session으로 한 run을100행/512KiB씩 검사한다. 호출 사이 transaction 없이 data_version을 확인하고 실패·취소·완료 시 재사용을 거절한다. 본문 합계/연속 순서/DB ID 일치/head 밖 row를 검사한다. 아직 전체 head 열거·고아 row·run/fence 양방향 closure가 없으므로 admission 증거가 아니며 관련 회귀는 실행 전이다.
- 후속: `AllOutputVerification`이 전체 head 열거와 record→head 역방향 검사를 추가했다. 각 run의 페이지 검사를 재사용하고 고아/초과 sequence·비양수 rowid·key/binding 불일치를 거절한다. 전체 출력 table closure이며 run/fence·schema·물리 quota와 통합되지 않아 아직 admission 증거가 아니다. 회귀는 작성 상태다.
- 후속: head→actual run의 전체 신원·state·output_format_version=1·legacy 배열 부재 및 존재하는 provider fence 검사를 추가했다. terminal run은 sealed 출력이 필요하지만 sealed 자체는 성공 증거가 아니다. runs→head 역방향 phase는 새 형식 marker에 대응하는 head 존재를 요구한다. 이 형식의 실제 runtime 생성·이관·schema/admission 통합은 아직 없다. 기존 provider 종료/poison 검사를 대체하지 않는다.
- schema 체크포인트: `storage/agent_output/schema.rs`에 V10→V11 원자 이관 준비/정확 schema 확인을 추가했다. 기존 run 배열은 보존하고 신규 형식용 빈 테이블만 만든다. V10 metadata 제약을11로 교체하므로 구버전은 거절한다. product open·기존 replay/compaction/admission의 V11 분기와 연결 전에는 호출하지 않는다. 새 함수/회귀는 아직 실행 검증 전이다.
- 후속: 기존 bounded admission preflight, snapshot schema, 압축 prefix/보존 event/message 조회·replay·append 분기에 V11을 추가했다. `verify_admission`은 기존 원장 검사→출력 closure→같은 data_version/최종 schema 검사를 결합한다. 이관 회귀에 호출을 추가했으며 실행 검증 전이다. 자동 product open은 여전히 V11을 거절하고 V10→V11을 호출하지 않는다. 나머지 lifecycle/compaction 계약과 runtime 연결·독립 검토 후 활성화해야 한다.
- 후속: materializer/compaction writer runtime 분기와 TS 저장버전 상수에11을 추가했다. `Storage` 내부 output append/head/page는 admission과 V11·run 결속을 확인하고 기존 transaction/maintenance epoch에 참여한다. 공개 IPC/provider 호출·신규 run 시작/종료와의 원자 연결·물리 quota는 아직 없으며 V10 정책 golden을 V11 합격으로 재사용하지 않는다.
- 후속: 기존 `write_projection`의 run 갱신 transaction에 output head 초기화/terminal seal을 연결했다. marker가 있는 V11 첫 event만 head를 만들고 중간 누락·legacy 되돌림·sealed 재실행을 거절한다. provider가 새 marker를 생성하도록 바꾸거나 자동 이관을 활성화하지는 않았다. rollback 회귀는 작성 상태다.
- runtime 체크포인트: 명시 marker 경로에서 actual projection/head 검증, 공개 event 직접 append, bounded collector(100개/4MiB), 복구/종료 재작성의 marker 보존을 연결했다. 새 형식은 매 event를 chat message로 중복 저장하지 않는다. 최종 응답의 후속 문맥·UI page 표시와 provider parser/observation 한도 변경이 남아 있어 새 run marker 생성/자동 이관은 아직 켜지 않는다. 이 추가분은 실행 검증 전이다.
- parser 체크포인트: core/Codex/Claude에 명시 `ProviderEventBudget::DurableV1` 선택을 추가했다. 기본은 legacy이며 새 모드는 내부65,536개·공개16,384개·raw256MiB·공개64MiB, 기존 단일 항목 상한을 사용한다. 원문 frame을 누적하지 않으며 Claude delivery-ID 집합도 내부 event 상한에 의해 유한하다. 이 집합의 실제 RAM은 ingress와 별도로 측정해야 한다. 숫자는 비활성 초기 구현값이며 runtime 선택·전체 RAM/시간 예산 동결·독립 검토는 남는다.
