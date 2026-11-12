# M5 채팅 내용 영구 삭제 계약 (제안)

> 이 문서는 **제안**이다. 구현 여부·검증 범위는 `docs/history.md` 최신 엔트리를 따른다. 여기 적힌 동작을 현재 제품 동작으로 인용하지 않는다.

## 1. 목적과 범위

현재 "목록에서 제거"는 프로젝트 원장 확장 `catalog_hidden_chats`로 **숨김만** 한다. 저장공간의 작업 복사본 정리(`project/workspace_cleanup.rs`)는 이미 버린 채팅의 앱 소유 복사본 파일만 지우고 대화 기록은 보존한다. 이 계약은 사용자가 선택한 채팅의 **대화 내용(본문·출력)** 을 앱 DB에서 영구 삭제하는 별도 기능을 정의한다.

- 대상: Elsecrew sidecar SQLite DB(본 파일 + WAL) 안의 해당 채팅 내용.
- 범위 밖(화면에서 명시 고지): ① 공급자(Claude Code·Codex)가 앱 전용 프로필 폴더에 남긴 자체 세션 기록, 공급자 서버 측 기록, ② 작업 복사본 파일(기존 작업 복사본 정리로 별도 삭제), ③ 원본 프로젝트·복구 백업, ④ OS/파일시스템 수준 잔존(APFS 스냅샷·Time Machine·SSD 재할당 블록), ⑤ 이 기능 도입 전 SQLite가 이미 빈 공간으로 돌려놓은 과거 버전 바이트(§6).
- 자동·백그라운드 삭제는 없다. 사용자의 명시 선택과 최종 확인으로만 시작한다.

## 2. 대상 자격(eligibility)

목록 숨김·작업 복사본 정리와 같은 검사를 재사용하고, 하나라도 불충족이면 변경 없이 거절(fail closed)한다.

1. 채팅 projection과 record의 상태가 모두 `discarded`다. (v1은 버린 채팅만. `completed` 등 다른 종료 상태는 후속 범위. 버린 채팅은 후속 입력·실행·작업 복사본 재사용이 이미 차단돼 있다.)
2. 팀 작업(`orchestration_v1`)이 `running`이 아니고, `save_sessions`가 비어 있다.
3. 채팅의 모든 실행이 `succeeded|failed|canceled`이고 미해결 복구가 없다(`resolved`/`resolvedAbandoned`만 허용).
4. 해당 작업 복사본 잠금이 `writer_run_id=null`, `poisoned=false`, `recovery_required=false`다(잠금이 없으면 허용).
5. 프로젝트에 진행 중인 반영(`application_intent`가 `abandoned` 외)·대기 저장이 없다.
6. sidecar가 유휴다: 활성 실행·팀 작업·대기 시작/취소·반영 실행·종료 준비가 없다(작업 복사본 정리와 동일한 IPC 게이트).
7. 저장소가 압축(compaction) 흔적 없이 내용을 다시 쓸 수 있다: 이 채팅·그 실행 aggregate에 대한 `frozen_operation_sources`, `frozen_message_sources`, `ledger_compaction_prefixes`, `storage_compaction_plan_rows`, `ledger_compaction_delete_authority`가 없고, 실행 중인 lossless compaction job이 없다. (1,000 이벤트를 넘겨 압축된 채팅은 v1에서 "아직 삭제할 수 없음"으로 표시한다. 후속 범위.)
8. 지원하는 저장 schema 버전(§7)이고 Windows가 아니다(작업 복사본 정리와 동일하게 Windows에서는 노출하지 않음).

## 3. 지우는 것과 남기는 것

결정적 치환 함수 `redact`가 내용 문자열을 고정 표지 `"[erased]"`로 바꾼다(빈 문자열은 그대로). 같은 원문은 모든 사본에서 같은 결과가 되므로 "최종 출력 = 완료된 항목 본문", "첫 메시지 = goal" 같은 기존 교차 검증이 그대로 성립한다.

| 지움(내용) | 위치 |
| --- | --- |
| 채팅 목표 `record.goal` | 채팅 이벤트 payload 전부, projection, baseline, receipt |
| 사용자·팀·시스템 메시지 `last_message.text` | 위와 동일 + `chat_messages` |
| 팀 작업 지시·보고 `orchestration_v1.instruction`, `reports[].instruction/response` | 채팅 payload 계열 |
| 실행 출력 본문(legacy `public_events[].text`) | 실행 이벤트 payload 계열 |
| 장시간 출력 본문 `run_output_records.event_json.text` | 출력 원장(헤드 `text_bytes` 재계산) |

| 남김(내용 아님) | 이유 |
| --- | --- |
| 채팅·실행·메시지·이벤트 ID, 순서, 시각, 상태, 역할/모델/권한 snapshot, 작업 복사본 manifest·경로, 종료 증거, 실패 단계/코드 | 원장 연속성·복구·감사 |
| 승인 기록의 binding digest(도구 요청 해시) | 원문 아님. 사전 공격 가능성은 위험으로 기록 |
| 프로젝트 원장(반영 이력·백업 등) | 채팅 대화가 아님 |

알 수 없는 최상위 키가 채팅/실행 payload에 있으면 내용 여부를 판단할 수 없으므로 **삭제를 거절**한다(부분 삭제를 완료로 보고하지 않기 위한 fail-closed 규칙). 새 키를 추가하는 개발자는 분류를 이 계약과 코드 목록에 함께 추가해야 한다.

## 4. 원장 일관성 방식 (schema 증가 없음)

G5D 검증기는 원천 이벤트에서 projection·receipt·message·baseline을 재유도해 canonical byte/digest를 비교한다. 따라서 "표지 줄만 허용"하는 예외 규칙을 검증기에 넣는 대신, **모든 사본을 처음부터 표지 내용이었던 것처럼 한 transaction에서 일관되게 다시 쓴다.** 재열기·재생·materialization·compaction 검증 코드는 바꾸지 않는다.

한 `BEGIN IMMEDIATE` transaction 안에서:

1. §2 자격을 저장소 측에서 재검사하고, 대상 행 집합(채팅 aggregate 이벤트, `runs.chat_id`와 payload `chat_id`가 모두 일치하는 실행 aggregate 이벤트)을 고정한다.
2. 수정 금지 trigger 3개(`events_are_append_only_update`, `operation_receipts_compaction_immutable_update`, `ledger_baselines_compaction_immutable_update`)의 `sqlite_schema` SQL을 읽어 기준 SQL과 정확히 같은지 확인한 뒤 DROP한다.
3. 대상 이벤트 payload를 `redact` 후 `canonical_json_v1`로 재인코딩해 UPDATE. 같은 `operation_id`의 receipt는 payload와 `sha256` digest를, 같은 aggregate/sequence의 baseline은 projection과 digest를, `chat_messages`는 메시지 JSON과 digest를, `chats`/`runs` projection은 재생 결과로, 출력 원장은 본문과 `text_bytes`를 다시 쓴다.
4. 프로젝트 원장 확장 `chat_erasure_v1[chatId].state = "redacted"` 이벤트를 일반 append 경로로 같은 transaction에 추가한다(materialization `ready → pending` 무효화 포함).
5. 다시 쓴 모든 행에 원래 내용 문자열(12바이트 이상, 원문·JSON escape 두 형태)이 남지 않았는지 `instr`로 확인한다. 하나라도 남으면 ROLLBACK.
6. trigger를 캡처한 SQL 그대로 재생성하고, 버전별 schema identity 검증기로 전체 schema가 시작 전과 동일함을 확인한 뒤 COMMIT. 어떤 오류든 ROLLBACK되어 trigger·데이터 모두 원상태다.

trigger를 transaction 안에서 잠시 내리는 것은 DB 수준 방어선을 코드 경로 하나에 위임하는 절충이다. 대신 schema 버전·trigger 목록·모든 버전 분기를 건드리지 않아 동시 진행 중인 V13 작업과 충돌하지 않고, 검증기는 "표지 예외"를 몰라도 된다. 검토에서 DB 수준 권한이 필요하다고 판단되면 후속으로 erasure authority 표(V14)를 두고 trigger가 그 표의 정확한 행이 있을 때만 UPDATE를 허용하게 강화한다.

## 5. 중단 안전성·재개·멱등

상태 기계(프로젝트 원장 확장 `chat_erasure_v1`, ID·시각만 저장):

```
(없음) ──A: 의도 기록──▶ pending ──B: 단일 transaction 재작성──▶ redacted ──C: 물리 정리──▶ D: erased
```

- A: 자격 확인 후 `pending`과 요청 시각을 append. 이후 화면은 "중단된 삭제 · 다시 진행"으로 표시한다.
- B: §4 전체가 원자적이다. 중간 종료 시 `pending`으로 남고 원장은 원래 그대로다. 재시도는 자격을 다시 검사한다.
- C: `secure_delete=ON`(§6)로 쓴 뒤 `PRAGMA wal_checkpoint(TRUNCATE)`가 busy 없이 끝나고 WAL 파일 길이가 0인지 확인한다. 실패하면 `redacted`로 남고 완료라고 말하지 않는다.
- D: `erased`와 완료 시각을 append. 응답은 이 commit 뒤에만 `erased:true`를 돌려준다.
- 멱등: `erased`는 즉시 같은 결과를 반환하고, `redacted`는 C부터, `pending`은 B부터 재개한다. `pending`/`redacted` 상태의 재개 판단은 원장 내용이 아니라 상태 필드로 한다.
- 삭제된 채팅은 `chat_erasure_v1`에 기록이 있으면 채팅 목록에서 제외된다. "숨긴 채팅 복원"은 숨김 목록만 비우므로 삭제된 채팅을 되살리지 않는다. `preserve_project_extensions`가 이 필드를 snapshot 전환에서 보존한다.
- 재시작 후: 다음 시작의 기존 admission(`ReopenMaintenanceSession` + 출력 closure)이 다시 쓴 원장을 처음부터 재검증한다. 검증 실패 시 기존 규칙대로 쓰기가 막히며, 삭제 기능이 이를 우회하지 않는다.

## 6. 물리적 잔존(SQLite 빈 공간·WAL)

- 행을 다시 쓰면 이전 cell은 빈 공간이 되고 과거 page 이미지는 WAL에 남을 수 있다. 연결 설정에 `PRAGMA secure_delete=ON`을 켜 해제되는 cell·page를 0으로 덮고, 완료 전 WAL을 TRUNCATE 한다.
- `VACUUM`은 쓰지 않는다. `events`는 INTEGER PRIMARY KEY가 없어 VACUUM이 rowid를 바꿀 수 있고(SQLite 문서상 보장 없음), `event_source_index.event_rowid` 결속을 깨뜨릴 수 있다.
- 한계: 이 설정 도입 **전에** 해제된 과거 projection 버전 바이트는 SQLite 빈 공간에 남아 있을 수 있다(재사용 전까지). 화면 문구는 "앱 기록에서 내용을 지운다"로 하고 포렌식 수준 삭제를 주장하지 않는다. rowid를 보존하는 검증된 재구성은 후속 과제다.

## 7. schema 버전과 V13 통합

- schema 버전을 올리지 않는다. 지원 버전은 한 곳의 상수(`ERASURE_SCHEMA_VERSIONS`)와 버전별 identity 검증 함수 선택(`verify_schema_identity_for_erasure`)에만 있다. V13(사용자 정의 역할) 통합 시 13과 그 검증기를 이 두 곳에 추가하면 된다. V13이 다시 쓰는 `event_source_index_requires_exact_event`는 erasure가 내리는 trigger가 아니다.

## 8. 화면 흐름 (설정 → 저장공간)

1. "대화 내용 영구 삭제" 섹션에서 "삭제할 대화 찾기"로 후보를 조회한다(버린 채팅만; 내용 미리보기 1줄, 자격 여부, 중단 상태).
2. 자격 있는 항목만 체크할 수 있다. 자격 없는 항목은 이유 요약(실행·복구 확인 필요/압축된 긴 대화/지원 안 함)을 보여준다.
3. "선택한 N개 영구 삭제" → 확인 대화상자: 지우는 것(목표·메시지·팀 지시/보고·출력 본문), 남는 것(ID·시각·상태·실행 증거, 작업 복사본은 별도 정리), 범위 밖(공급자 자체 세션 기록·백업·OS 스냅샷)을 표시하고 "되돌릴 수 없음"을 명시한다.
4. 확인 시 항목을 하나씩 순서대로 요청하고 진행 수를 표시한다. 각 항목은 응답 `erased:true`를 받은 뒤에만 완료로 표시한다. 오류가 나면 중단하고 목록을 다시 조회해 상태(중단/미완료)를 보여준다.

## 9. 계약(IPC)

기존 `project.catalog` 채널에 두 동작을 추가한다(sidecar `require_exact_params` + TS 양방향 validator).

- `{action:"chatErasureList"}` → `{schemaVersion:1, action, entries:[{chatId, projectId, preview, eligible, state:"none"|"pending"|"redacted"}]}` (최대 200개, `erased` 제외, preview ≤ 160자)
- `{action:"eraseChatContent", chatId, confirmed:true}` → `{schemaVersion:1, action, chatId, erased:true}`; sidecar 유휴 게이트와 `confirmed===true` 필수.

## 10. 검증 계획

임시 DB Rust 테스트: 실제 fake 실행 출력·후속 메시지가 있는 채팅을 버린 뒤 삭제 → 다시 열기 admission 통과 → 표식 문자열이 DB·WAL 바이트 어디에도 없음 → 재시작 후에도 없음/목록 제외; 단계별 중단(A 이후·B 이전, B 이후·D 이전) 재개; 부적격(버리지 않음·미해결 복구·압축 흔적·확인 누락) 무변경 거절; 삭제 후 다른 채팅 내용 보존; 삭제 후 compaction 경로 회귀. TS: 요청/응답 validator와 화면 상호작용.

## 11. 남는 위험·후속

- 공급자 자체 세션 기록 삭제(앱 전용 프로필 내 Claude/Codex 세션 파일)는 미구현. 진짜 "모든 사본 삭제"로 표시하려면 필요.
- 압축된 긴 대화(frozen/prefix/plan 흔적) 삭제, `completed` 상태 채팅, 기능 도입 전 SQLite 빈 공간 잔존 정리.
- trigger 일시 제거 대신 DB 수준 erasure authority(V14) 강화 여부는 독립 검토로 결정.
