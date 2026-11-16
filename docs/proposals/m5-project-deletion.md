# M5 프로젝트·작업 삭제 (제안)

> 이 문서는 **제안**이다. 구현 여부와 검증 범위는 `docs/history.md` 최신 엔트리를 따르며, 여기 적힌 동작을 현재 제품 동작으로 인용하지 않는다.

## 1. 사용자 결정 (2026-11-13)

- 목록에서 지우면 숨기기·복원 없이 **삭제**한다. "목록에서 제거"와 "제거한 작업 다시 표시"는 없앤다.
- 숨긴(또는 삭제한) 프로젝트와 같은 경로의 폴더를 다시 추가하면 **항상 새로 시작**한다. 예전 작업·작업 공간·역할/권한 설정·원본 복사본(snapshot)이 보이면 안 된다.
- 원본 프로젝트 폴더는 어떤 경우에도 지우지 않는다.

## 2. 왜 원장 행을 지우지 않는가

앱 기록은 append-only 이벤트 원장과 projection 표로 되어 있고, 열 때마다 원장에서 projection을 다시 유도해 대조한다. 이벤트 행 삭제는 DELETE trigger 6개(`events_*`, `event_source_index_*`), 사본 표의 무조건 거절 trigger, FK 순서, 압축 흔적(append-only)과 같은 aggregate ID 재사용 금지에 막힌다. 따라서 **삭제 표시(tombstone) + 앱 소유 파일 삭제**를 쓰고, 원장 행은 남긴다. 화면에는 다시 나타나지 않는다.

## 3. 프로젝트 삭제

`project.catalog {action:"deleteProject", projectId, confirmed:true}`

1. **거절**: 원본 반영 실행 중, `application_intent.state == "applying"`(반쯤 쓴 원본), 반영 확인창이 열려 있을 때. 그 밖의 실행·복구 상태는 삭제를 막지 않는다(사용자 결정).
2. **삭제 표시**(일반 append, trigger·schema 변경 없음): 프로젝트 record에
   - `canonical_path`를 `deleted:<projectId>`로 바꾼다. `projects.canonical_path`가 UNIQUE라서 같은 폴더가 새 ID로 다시 들어오려면 필요하다. 표지 경로는 `native_path_binding`과 맞지 않으므로 `required_project`가 이 프로젝트의 파일 접근을 자동으로 거절한다(fail-closed).
   - `catalog_hidden: true`로 목록과 종료 차단(blocker) 집계에서 뺀다.
   - `project_deletion_v1 = {state:"pending", worktrees:[…]}`: 지울 작업 공간 목록을 이 시점에 고정한다. 프로젝트 snapshot, 잠금이 있는 모든 작업 공간, 작업의 작업 공간, 기준 기록이 있는 작업 공간이 대상이다.
3. 그 프로젝트의 watcher를 해제하고 `stop_sessions_for_removal`로 실행 중인 세션을 멈춘다(best effort).
4. **파일 삭제**(`finishDeletions`, 재시도 가능): sidecar가 유휴일 때(작업 공간 정리와 같은 게이트) 고정 목록의 작업 공간마다 다음을 확인한 뒤 지운다.
   - 잠금에 writer·poison·recovery가 없어야 한다.
   - 그 작업 공간을 쓴 모든 실행이 종료 상태이고 `processesProvenGone=true`여야 한다. 복구 기록이 있으면 해결된 상태여야 한다.
   - 지우는 순서: `worktrees-v1/<id>`를 `delete_<id>`로 이름을 바꾼 뒤 no-follow walk로 지운다. 이어서 manifest `<id>.json`, 기준 기록 `<id>.source-baseline-v1.json`을 지운다.
   - 모두 끝나면 `project_deletion_v1.state = "deleted"`를 append한다.
5. 하나라도 조건이 안 되면 `pending`으로 남고 다음 호출에서 이어간다. 화면은 삭제 직후 2·8·30초 뒤와 앱 시작 때 `finishDeletions`를 부른다. 복구가 필요한 실행이나 poison 잠금이 있는 작업 공간은 그 복구가 해결되기 전까지 지우지 않는다. (2026-11-16) 삭제된 작업의 미해결 실행 복구는 '작업 공간 버리기'와 같은 경로(`recovery.resolveAgent`)로 자동 해결을 시도한다. 기록된 모든 프로세스가 사라졌음이 증명될 때만 해결하며, 종료 증명은 생략하지 않는다.

## 4. 작업 삭제

`project.catalog {action:"deleteChat", projectId, chatId, confirmed:true}`

- 거절 조건은 프로젝트 삭제와 같다.
- 프로젝트 확장 `chat_deletion_v1[chatId] = {state:"pending", worktreeId}`를 append한다. 작업 목록(`list_chats`)과 blocker 집계에서 즉시 빠진다.
- 실행 중인 세션을 멈추고, 같은 종료 증명 조건으로 그 작업 공간 파일을 지운 뒤 `state:"deleted"`를 남긴다.
- (2026-11-16 이후 새 작업은 자기 복사본만 쓰고 프로젝트 지정을 바꾸지 않는다. 아래 규칙은 예전 데이터에만 해당한다.) 가장 최근 작업은 프로젝트의 현재 snapshot(원본 복사본)을 그대로 쓴다(독립 검토 B1·R3). 그대로 지우면 프로젝트가 열리지 않고, 남겨 두면 지운 작업의 파일이 프로젝트 화면에 계속 보인다. 그래서 다음 규칙을 둔다.
  - 그 작업 공간의 실행이 모두 끝났으면 `close_project`로 그 복사본을 `abandoned`로 닫는다. 응답 `projectClosed:true`를 받은 화면은 프로젝트를 다시 열고, 다음 열기는 원본에서 새 복사본을 만든다. 파일은 곧바로 지울 수 있다.
  - 아직 실행이 남아 있으면 닫지 않는다. 실행 마무리가 복구 필요로 꼬일 수 있기 때문이다. 나중에 `finish_deletions`가 실행 종료를 확인했을 때 닫는데, 그 프로젝트를 보고 있는 화면(watcher)이 없을 때만 닫는다.
  - 삭제되지 않은 다른 작업이나 묶이지 않은 prepared workspace(`manifest.worktreeId` 기준)가 같은 작업 공간을 쓰면 참조가 사라질 때까지 `pending`으로 둔다.
  - 프로젝트가 삭제되면 이런 미룸 없이 프로젝트 삭제가 모든 작업 공간을 지운다.
- 열려 있던 작업을 지우면, 화면은 그 프로젝트를 다시 열어 프로젝트 자체 파일로 돌아간다.
- 삭제된 작업에는 기존 작업 공간 정리(`cleanupWorkspace`)를 직접 호출해도 거절한다. 파일은 삭제 흐름이 책임진다.

## 5. 같은 폴더 다시 추가

- `open_verified_source`는 경로로 살아 있는 프로젝트를 찾는다(`project_id_by_path`). 삭제된 프로젝트는 표지 경로라 찾히지 않는다.
- 찾은 프로젝트가 예전 "목록에서 제거"로 숨겨진 것이면, 그 자리에서 삭제 표시를 한 뒤 새로 시작한다. 이로써 기존 데이터에도 사용자 결정 ②가 적용된다.
- 새 ID는 처음에는 경로 해시(`project_<sha256(path)>`)를 쓴다. 이미 쓰인 경우 `sha256(path + "\0" + 세대)`의 첫 빈 값을 쓴다. 역할 기본값의 고정 operation ID 같은 ID별 상태가 새 프로젝트에 섞이지 않는다.
- 카탈로그 재열기(`reopen_project`)는 경로로 다시 찾은 ID가 요청 ID와 같아야 하고, 표지 경로면 거절한다.

## 6. 지우지 않는 것 (한계)

- 원장 안의 예전 작업 기록 행. 화면에는 보이지 않는다.
- 삭제된 작업의 대화·실행 본문은 원장에 남는다. 설정의 정리·내용 삭제 목록에서도 빠지므로 지금은 화면에서 지울 방법이 없다. 삭제할 때 본문까지 지우는 것(기존 chat_erasure 재사용, 자격을 '삭제된 작업'으로 넓힘)이 다음 단계다.
- 반영 백업(`application-backups-v1`)은 반영 검토 기록과 소유권 검사가 얽혀 있어 1단계에서 지우지 않는다.
- 공급자(Claude·Codex)가 앱 전용 프로필 폴더에 남긴 자체 세션 파일.
- Windows는 파일 삭제가 아직 없다. 삭제 표시만 하고 pending으로 남는다.
- (2026-11-16) 공급자 로그인 잠금이 가리키는 작업 공간이라도 삭제는 기다리지 않는다. 잠금 해제 검증이 원장의 삭제 완료 기록(`deleted`)과 디스크 부재를 manifest 대신 증거로 받기 때문이다. 예전 흐름은 파일을 먼저 지운 뒤 풀 방법이 없어 잠금이 영구히 남았다.
- 종료를 증명할 수 없는 실행(신원 없음·프로세스 남음)이나 poison 잠금이 있는 작업 공간은 pending으로 남는다. 삭제된 항목은 목록에서 빠지므로 이 상태를 설정에 보여 주는 것은 후속 과제다.
- 재시도는 sidecar 전역 유휴 게이트를 따른다. 다른 프로젝트의 실행이 길면 다음 앱 시작 때 지워진다.
- 예전 "목록에서 제거"로 숨긴 작업(`catalog_hidden_chats`)은 삭제로 옮기지 않았다. 목록에서는 계속 빠지고, 파일은 프로젝트를 삭제할 때 함께 지워진다.
- 중단된 기존 작업 공간 정리가 남긴 `cleanup_<id>`, 생성 중이던 staging, 저장 staging 같은 잔여물.

## 7. 검증

- Rust:
  - 작업 삭제 → 작업 공간 제거 → 다시 지우기·다시 끝내기는 무변경
  - 프로젝트 삭제 → 모든 작업 공간 제거 → 저장소 admission 재검증(`open_for_admission`) 통과 → 같은 폴더가 새 ID로 열림
  - 예전 숨김 프로젝트를 다시 추가하면 새로 시작
  - 반쯤 쓴 원본·반영 확인창이 열려 있으면 거절
  - 실행 중이거나 poison 잠금이 있으면 파일이 pending으로 남음
  - 삭제한 작업의 남은 복구는 종료 차단에서 빠지고, 살아 있는 실행은 계속 차단함
- TS: 요청·응답 validator(확인 필수, 옛 동작 거절), 화면 smoke(작업·프로젝트 삭제, 복원 버튼 없음).
