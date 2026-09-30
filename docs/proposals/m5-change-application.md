# M5 변경 반영·복원 구현 계약 초안

상태: A1 내부 생성 경로 결속·집중 회귀 완료, 제품 입장·원본 writer 구현 전. 2026-09-30, 기준 `375aba8` 이후. 제품 방향은 [M5 §5.5·§6.5·M5.3](m5-coding-agent-ide.md)이 우선한다. 이 문서는 원본 쓰기 권한을 활성화하거나 실제 사용자 프로젝트 시험을 승인하지 않는다.

## 확정 범위와 현재 격차

- 전체 작업 복사본의 명시 반영, 반영 직전 복원 지점, 명시 되돌리기. 부분 반영·자동 충돌 해결·자동 Git commit/push는 하지 않는다.
- 현재 `ProjectManager`는 source를 읽어 standalone snapshot을 만들고, snapshot 파일 편집과 diff를 제공한다. `workspace.close`는 폐기이며 `project.catalog releaseView`는 감시 해제다. 둘 다 적용이나 복원이 아니다.
- `ActiveProject`의 base commit/tree/source identity는 Git 기준만 담는다. 원본 working tree의 현재 bytes나 index의 동등성을 증명하지 않는다. `source_digest`는 Git 메타데이터 변경 감지용이지 원본 파일 baseline이 아니다.
- 현재 diff UI 파서는 적용 권위가 아니다. `SourceRepository`의 capability와 고정 plumbing 실행을 재사용하되, 사용자 filter/fsmonitor/hooks 실행 가능성이 있는 일반 status/diff/checkout 명령을 원본 검증·쓰기 경로에 그대로 추가하지 않는다.

근거: [Git 공식 config 문서의 core.fsmonitor](https://git-scm.com/docs/git-config#Documentation/git-config.txt-corefsmonitor)는 외부 hook 경로 설정과 index 갱신 명령(status 등)의 사용을 설명한다. 저장소 로컬 config를 읽는 현재 고정 Git launcher가 모든 명령의 무부작용을 보장하는 것은 아니라는 설계 판단이다.

## 실행 단위와 권위

### A1 baseline 정책 (구현 제안, 영속 스키마 확장 전)

내부 raw inventory 후보(`project/application.rs`)는 Unix에서 한 pass당 100,000개 항목·파일/링크 내용 합계 4GiB·경로 합계 8MiB·개별 UTF-8 경로 4,096byte·깊이 128을 제한한다. 두 pass의 결과를 대조하며 전체 내용 읽기 상한은 8GiB다. root `.git`만 제외하고 ignored/임시 이름/하위 `.git`을 포함한다. 하드링크·특수 파일·비 UTF-8 경로는 거절하며 링크는 대상이 아니라 링크 문자열을 기록한다. 파일 내용·mode·관측한 identity/ctime 변경을 검사하지만 원자 snapshot이나 동일 UID 외부 작성자 차단을 보장하지 않는다. 이 단독 수집기는 snapshot 생성·원장·제품 API에 연결하지 않았다. 후속 index/raw 통합 비교 후보는 아래에 기록하며 영속 binding·legacy 정책 구현은 남아 있다. 이 내부 수치가 복원 백업 용량 정책을 대신하지 않는다.

새 snapshot 생성 시 no-follow raw source baseline receipt를 원장에 결속한다. receipt는 canonical root/실물 directory identity, HEAD/tree, index의 semantic entry 집합, `.git` 밖 파일의 경로·종류·mode·size·content digest·device/inode/link count를 bounded stream으로 담으며 capture 전후 동일성도 확인한다. 읽기에 따라 OS가 갱신할 수 있는 atime은 비교·무변경 보장에서 제외한다. 앱이 atime을 복원하는 쓰기도 하지 않는다. tracked 파일과 index가 HEAD의 raw tree와 다르면 첫 적용 지원 대상에서 제외한다. 필터/LFS 변환을 clean 명령으로 실행해서 동등성을 만들어내지 않는다.

Untracked/ignored 파일도 receipt 범위에 포함하되 snapshot 변경 집합에 자동 편입하지 않는다. 원본 재검사에서 receipt 범위 변화는 거절하고, snapshot의 새 파일/디렉터리와 그 파일들의 경로가 충돌하면 덮어쓰지 않는다. 이렇게 원래 존재했던 사용자 파일을 삭제 대상이나 소유 파일로 오해하지 않는다. 이 전체 집합의 byte/entry 상한과 초과 안내를 A1 구현에서 고정하며 큰 ignored tree를 조용히 제외하지 않는다.

기존 snapshot에는 receipt가 없으므로 원본 적용에는 사용할 수 없다. 기존 파일/채팅/diff 열기는 유지하지만 과거 baseline을 현재 source에서 소급 생성하지 않는다. 사용자에게 새 기준에서 작업을 시작하도록 안내한다. A1은 receipt 생성·영속 binding·재검증·legacy 거절 회귀까지 통과해야 하며, 그 전 A2 검토 묶음은 writer admission 근거가 아니다.

내부 `application/git_baseline.rs` 후보는 index image 32MiB와 각 Git 출력 24MiB를 제한하고, `ls-files -v --stage --sparse -z`와 `ls-tree -rz`의 경로/mode/OID 집합을 대조한다. staged 변경·unmerged stage·gitlink·split index·sparse directory entry·skip-worktree·assume-unchanged는 거절한다. full index 형태의 sparse checkout도 skip-worktree 표시로 차단한다. 같은 빈 blob으로 보이는 intent-to-add는 cached 전용 `diff-index --ita-invisible-in-index`로 추가 확인한다. 외부 diff/textconv/rename과 fsmonitor/hooks를 명시 끄고 index stamp/hash 및 HEAD를 전후 확인한다. 이 index 단독 검사는 작업 폴더 bytes의 HEAD 동등성을 증명하지 않는다. 후속 raw blob 비교 후보는 아래에 기록하며 영속 receipt 결속은 아직 미완료다. unsupported Git 옵션도 허용으로 해석하지 않고 실패한다. 실제 사용자 저장소·외부 객체 fetch/관리 설정 전체 안전성 증거 없이 제품 입장에 연결하지 않는다.

명령 의미는 [Git ls-files](https://git-scm.com/docs/git-ls-files), [ls-tree](https://git-scm.com/docs/git-ls-tree), [diff-index](https://git-scm.com/docs/git-diff-index)의 공식 문서를 확인했다. `diff-index --cached`는 working-tree 파일을 비교하지 않으며 외부 변환은 별도로 끈다. 일반 `git diff`/`status`를 무설정으로 실행하는 정책이 아니다.

후속 내부 `git_baseline/raw_baseline.rs`가 위의 별도 검사들을 묶는다. index/HEAD → raw inventory → committed blob 비교 → raw/index 재검사 → canonical root 경로 신원 재검사 순서다. raw source 두 pass와 blob 한 pass 각각 4GiB로 제한한다(내용 합계 최대12GiB, Git metadata 읽기는 별도 한도). 일반 파일/링크의 raw SHA-256·길이·owner 실행 bit를 비교하고 Git 객체 header+bytes의 SHA-1 또는 SHA-256도 재계산한다. SHA-1은 Git 호환용이며 receipt 내용 digest의 SHA-256을 대체하지 않는다. untracked/ignored 원본 파일은 기록에 남지만 managed 변경 파일로 승격하지 않는다.

[Git cat-file 공식 문서](https://git-scm.com/docs/git-cat-file)의 batch raw 경로를 사용하며 filters/textconv/follow-symlinks 옵션은 사용하지 않는다. header256byte·본문 합계4GiB를 제한하고 missing/잘못된 OID/type/잘림·추가 stdout을 거절한다. 프로세스 정리는 기존 StreamingGitChild를 재사용한다. **시간 상한·비정상 Git 자손 종료와 Git 객체/로컬 설정 전체 신뢰 경계는 제품 결속 전 추가 검증 대상**이다. 현재 성공은 관측 구간의 비교이지 원자 snapshot이나 동일 UID 동시 작성자 차단이 아니다. 영속 receipt·snapshot 결속·legacy 정책 및 원본 writer/복원은 여전히 미구현이다.

검토 대상은 project/chat/task/worktree/workspace identity, base commit/tree, 검토 시 snapshot 파일 집합의 digest로 고정한다. renderer는 원본 절대 경로·patch bytes·백업 경로를 전달하지 않는다.

### A1 receipt 파일·원장 결속 후보 (제품 생성 연결 전)

내부 `SnapshotCreation::ApplicationCandidate`를 공통 생성 경로에 연결했다. launch fence를 먼저 저장한 뒤 clean baseline을 수집하고 export 후 같은 baseline인지 대조하여 receipt를 게시한다. pre-activation과 activation은 최신 원장 extension을 보존한다. 3개 독립 snapshot 생성·SQLite 재열기·각 기준 검증과 원본 변화 거절을 확인했다. 기존 일반/채팅 생성은 이 admission을 강제하지 않으며 공개 IPC caller는 없다. 이미 dirty이거나 미지원 원본의 편집 기능을 깨뜨리지 않는 제품 안내/입장과 시작 이후 복구·누적 크기/압축은 다음 조건이다. 아래의 생성 미연결 표현은 공개 제품 경로에만 해당한다.

2026-09-30 후속: version1 미시작 준비 복구는 `project.catalog`와 저장 프로젝트 UI에 연결했다. 명시 inspect 뒤 exact inspection으로 폐기하며, 동일한 영속 완료 증거만 응답 유실 재전송을 허용한다. 아래의 공개 연결 미완료 기록은 이 제한된 경로에 대해서 갱신한다. 이미 export를 시작한 실패나 legacy 잠금, 실제 A1 생성 결속은 이 기능으로 해결되지 않는다.

준비 복구의 내부 기반은 `snapshot_recovery.rs`에 구현했다. version1 `beforeSnapshotExport` 및 정확한 신원·파일 부재·미결속을 검사하고 project/lock revision을 같은 트랜잭션에서 대조해 명시 폐기한다. export 직전에는 별도 영속 단계로 넘어가므로 이후 기록과 legacy는 이 경로를 사용할 수 없다. 공개 IPC/UI·응답 유실 처리·시작된 프로세스의 종료 증거 복구는 여전히 후속이다.

Unix Git stream의 stdout/input/stderr/exit 대기에 명령별120초 한도를 연결했다. 시간 초과/오류는 unreaped leader 그룹 종료와 최대2초 direct reap을 시도한다. 파이프를 붙잡은 자손 때문에 무한 join하지 않는 회귀는 통과했지만 새 세션 이탈·파이프를 닫고 남은 자손·종료 실패의 영속 신원 보존은 아직 별도 조건이다. 이 개선만으로 위의 전체 자손 종료/제품 활성화 조건을 닫았다고 해석하지 않는다.

저장 API는 임의 Dir를 받지 않고 ProjectManager의 고정 managed child에서만 생성되는 opaque ReceiptStore를 받는다. 전체 raw/index JSON 트리를 만들지 않으며 정렬된 항목을 하나씩 bounded writer에 직렬화한다. 64MiB는 encoded payload 한도이며 기존 baseline과 검증용 읽기 버퍼를 포함한 프로세스 전체 메모리 한도를 뜻하지 않는다.

`raw_baseline/receipt.rs`는 앱 전용 `source-baselines-v1` 디렉터리 capability 안에 `<worktreeId>.source-baseline-v1.json`을 exclusive-create 0600으로 저장한다. payload는 snapshot_identity의 project/worktree/workspace/source root·identity·object format·base commit/tree와 raw/index 상태를 묶고 canonical JSON 최대64MiB다. file fsync → directory fsync → no-follow 재조회와 payload/digest 확인 뒤에만 `{schemaVersion:1,digest,length}` descriptor를 반환한다. 기존 파일·부분 쓰기 파일은 덮어쓰거나 자동 삭제하지 않는다.

재검증은 신뢰되는 원장으로부터 받은 descriptor와 새로 수집한 baseline이 모두 필요하다. 단순 파일 존재나 renderer 입력은 권위가 아니다. descriptor가 없는 legacy snapshot은 파일이 있더라도 거절한다. 내부 persist_and_bind는 exact project/source/workspace 신원·recoveryRequired·복구 잠금·writer 없음 확인 뒤 파일 durability를 확보하고 기존 project 원장 source_baselines[worktreeId]에 binding/descriptor를 게시한다. verify_bound는 해당 원장의 exact 신원과 파일을 함께 검증한다. 실제 SQLite 재열기 및 프로젝트 close/open에서 extension 보존을 검증한다. **snapshot 생성 시 호출은 아직 없다.** Git 시간/자손 종료 경계와 원장 누적 크기/압축·과거 여러 workspace 수명 검증을 완료한 뒤 제품 생성에 연결한다.

DB 게시 실패 시 부분/완성 파일과 recovery lock을 보존하며 동일 snapshot에 자동 재게시하지 않는다. 현재 source에서 과거 기준을 소급 생성하는 위험을 피하기 위해 미완료 snapshot을 명시 폐기하고 새 snapshot을 만드는 정책이다. **현재 공개 recovery API는 run 중심이며 채팅 생성 전 snapshot 준비 잠금을 폐기하는 경로는 아직 없다.** 기존 workspace recovery 메서드만으로 이 경로가 구현됐다고 보지 않는다. 프로세스 종료 증거·신원·정확한 실패 단계와 결속한 명시 폐기 API 및 재시작 통합 회귀를 먼저 구현해야 한다. 정상 legacy 파일/채팅/diff 열기는 보존하고 적용만 차단한다.

1. **준비**: active workspace·원장·실행 종료 증거·writer 잠금을 확인한다. source의 canonical root 및 실물 디렉터리 capability, HEAD/tree, index, tracked bytes/mode와 대상 경로 충돌을 읽기 전용으로 대조한다.
2. **검토 묶음**: sidecar가 변경 파일의 before/after 존재·종류·mode·size·content digest와 묶음 digest를 만든다. rename은 표시용 관계이며 쓰기는 정확한 old/new path 항목으로 표현한다. binary·삭제·심볼릭 링크·권한 변경도 빠짐없이 요약한다.
3. **명시 확인**: 사용자는 정확한 검토 묶음 ID/digest를 확인한다. 그 뒤 source 또는 snapshot이 달라지면 재검토한다. 확인은 다른 실행/묶음/재시작에 재사용하지 않는다.
4. **적용 준비**: project-wide 앱 내부 writer 배타 잠금과 원본 재검사를 거친다. 원본의 이전 파일·mode·존재 여부, 새로 생성할 디렉터리 목록을 앱 소유 복원 지점에 보존하고 hash 검증·fsync·원장 intent 영속화 후에만 첫 원본 쓰기를 허용한다.
5. **적용**: 고정 capability 아래에서 정렬된 변경 항목을 적용한다. 부모 symlink/alias·hardlink·파일 종류/identity 변경은 거절한다. 각 항목은 durable intent → exact before 검증 → staged rename/삭제 → 디렉터리 동기화 → 완료 기록 순서다. 기존 `.git`·index·refs는 쓰지 않는다.
6. **완료**: 전체 after-image를 다시 확인한 뒤 완료를 기록한다. 파일 여러 개의 변경을 원자적이라고 표현하지 않는다. 중간 실패는 `recoveryRequired`이며 반영 성공으로 표시하지 않는다.
7. **복원**: 명시 선택한 복원 지점과 적용 결과를 대조한다. 현재 원본이 기록된 after-image와 다르면 사용자의 후속 편집으로 보고 자동 덮어쓰지 않는다. 복원도 자체 intent·항목별 진행·fsync·최종 대조를 가지며 완료 뒤에만 잠금을 해제한다.

### A2 읽기 전용 ReviewPlan 후보

`application/review.rs`는 현재 active project의 snapshot identity와 원장 receipt를 재검증하고 tracked 파일의 변경/삭제 및 snapshot의 추가 경로를 정렬된 before/after kind·owner executable·size·SHA-256 묶음으로 만든다. 원본의 untracked/ignored 파일을 삭제로 추론하지 않고 같은 경로 추가는 내용이 같아도 거절한다. 기존 디렉터리는 유지하며 새 디렉터리만 추가로 기록한다. 파일 종류 전환과 중첩 `.git`, regular 0644/0755·directory0755 밖의 snapshot 권한은 현재 명시 미지원이다. 원본의 실행 bit 외 권한은 receipt에서 검증하며 향후 writer가 보존해야 한다.

source/snapshot 전체 경로의 NFKC→upper/lower→NFD 보수적 충돌 key로 case-only/정규화 충돌을 거절한다. 이는 모든 OS의 collation을 재현한다는 보장이 아니다. [unicode-normalization 공식 API](https://docs.rs/unicode-normalization/0.1.25/unicode_normalization/)의 0.1.25를 고정하며 정규화 개별4KiB·합16MiB, 항목별 review 직렬화 hash 입력32MiB를 제한한다. 전체 JSON을 선할당하지 않는다.

생성 전후 source baseline/snapshot inventory를 대조하고 `revalidate`는 같은 binding·digest·변경 집합과 파일 신원까지 재검증한다. receipt 없는 legacy, 외부 원본 변화, 같은 bytes의 snapshot inode 교체도 거절한다. 일반 diff 화면의 표시와 이 내부 apply 후보의 권위를 혼동하지 않는다.

`review/persistence.rs`는 전체 변경 내용 대신 프로젝트의 최신 `application_review` descriptor를 기존 원장에 revision CAS로 저장한다. schemaVersion/reviewId/state:prepared/binding/changesDigest/snapshotDigest/changeCount를 묶으며 snapshot digest는 root stamp와 모든 entry의 identity/kind/mode/size/hash를 bounded32MiB 직렬화로 계산한다. 재열기는 현재 파일에서 plan을 재구성하고 descriptor가 정확히 같은지 확인한다. 과거 파일 내용 복원이나 superseded 검토의 열람 기능은 아니다. DB 게시 실패는 이전 descriptor를 보존하며 lifecycle 갱신은 extension을 보존한다. 승인 표시로 바뀐 descriptor도 거절하고 재시작은 승인을 복원하지 않는다. chat/task별 승인 ticket·공개 IPC/UI·writer 및 재시작 승인 수명은 후속이다.

## 반드시 닫을 안전 조건

검토 대상 연결 후속: `ReviewTarget::Chat`은 정확한 WorkspaceAddress와 task ID를 받되 값만 믿지 않고 durable chat projection/record와 active manifest/marker를 대조한다. draft/waitingForUser/reviewReady/completed/needsAttention/canceled만 후보이며 진행 중 run/recovery 전역 blocker와 writer 소유 lock은 거절한다. binding에 chat revision·state와 exact lock revision을 포함한다. 저장 슬롯은 프로젝트 단위 `application_review`와 별개인 `application_chat_reviews[chatId]`이며 project+chat+lock CAS로 게시한다. 다른 채팅으로 전환해도 해당 snapshot을 검토할 수 있고, 채팅/잠금 변경 뒤 옛 descriptor는 거절한다. 이는 확인 ticket이나 실행 권한이 아니며 기존 descriptor에 없는 새 binding을 소급 보충하지 않는다.

- 첫 구현의 clean-source 조건은 HEAD/tree만이 아니라 staged/unstaged 변경, 추가 파일과 경로 충돌을 다룬다. ignored 파일도 덮어쓸 대상과 충돌하면 거절한다. 시작 시 이미 dirty였던 원본을 HEAD snapshot과 같다고 간주하지 않는다.
- 검증은 raw blob/entry 비교와 no-follow capability 읽기로 구현한다. Git object 읽기는 bounded `ls-tree`/`cat-file` plumbing을 사용하며 external filter·fsmonitor를 실행하지 않았다는 hostile-config fixture를 둔다. index가 HEAD tree와 같은지는 별도 검증하며 파일 내용만 같다고 staged 상태를 무시하지 않는다.
- external editor/다른 프로세스는 앱 잠금을 따르지 않는다. 검사와 rename 사이의 동일 UID 동시 수정에 대한 실제 OS 경계를 증명하기 전에는 제품 적용을 활성화하지 않는다. 이 문제를 단순 digest 재확인으로 해결됐다고 주장하지 않는다.
- 경로 종류 전환·대소문자 충돌·Unicode 정규화·submodule·symlink·실행 권한·binary·대용량·disk-full·부모 디렉터리 교체를 명시 지원하거나 이유와 함께 거절한다. 조용한 누락/부분 성공은 금지한다.
- 복원 지점은 적용 성공 뒤에도 유지한다. 용량 부족이면 원본을 쓰기 전에 거절한다. 정리/보존 기간은 사용자가 복원 가능성을 이해하는 별도 정책으로 결정하며 무기한 보관이나 자동 삭제를 암묵 도입하지 않는다.
- 재시작은 각 항목의 현재 bytes/identity를 before/after/불명 중 하나로 분류한다. 명확한 자체 변경만 복구 대상으로 삼고 불명확하면 잠금·증거를 보존한다. 응답 유실 후 같은 operation ID 재조회는 기존 결과를 반환하며 다른 payload의 재사용은 거절한다.

## 구현 순서와 검증

| 단위 | 소유 위치 | 통과 증거 |
| --- | --- | --- |
| A1 raw source baseline | 새 `project/application.rs`, snapshot 생성/기존 원장, capability/plumbing helper | 새 receipt 결속·legacy receipt 없음 거절, clean/HEAD 변경/index-only/unstaged/untracked·ignored 변경/충돌과 hostile filter·fsmonitor marker 미생성, source bytes/mode/identity/mtime/ctime 무변경(atime 제외) |
| A2 immutable review plan | sidecar + protocol/TS 계약과 golden fixture | 전체 변경 집합/정규화 경로/byte 상한/identity 및 digest 불일치 거절, snapshot 변경 후 이전 review 무효 |
| A3 journal/restore point | 기존 Storage 원장, 앱 전용 capability 디렉터리 | 첫 write 전 intent/backup fsync, byte 검증, disk-full·저장 실패시 source writes 0 |
| A4 writer/restart recovery | 별도 application 모듈 | 각 파일 경계 crash injection, 삭제/추가/mode 변경 및 역복원, 외부 편집 보존, 잠금 재시작 후 유지 |
| A5 UI/IPC 연결 | fixed main/preload 및 검토 화면 | 명시 전체 반영/되돌리기, stale confirmation·중복 클릭·응답 유실·오류 복구, 쉬운 설명/상세 증거 |

메인이 A1부터 직접 구현한다. A2 이전에 byte/파일 개수·백업 용량의 수치 제한과 계약 버전을 고정하고 golden fixture를 추가한다. A4의 source 쓰기·복구 경계는 독립 검토를 통과해야 하며, A1~A4 성공도 A5/실제 제품 완료의 대체 증거가 아니다. 테스트는 합성 임시 저장소만 사용하고 실제 사용자 프로젝트 적용은 별도 승인 대상이다.

## 열린 결정

A3 내부 진전(2026-09-30): `review/backup.rs`의 before-image archive 후보는 변경 대상의 raw bytes/링크 문자열·원래 권한·부재 항목을 담는다. archive 한 개 상한은 raw4GiB+metadata32MiB이며 전체 보존 용량/기간 정책은 아래 열린 결정과 별개다. 정상 실패의 자체 파일 정리·신원 불일치 보존 오류를 지원하지만 crash orphan의 영속 등록, strict framing decoder, DB intent 게시/재시작과 confirmation 소비 연결은 아직 없다. header는 version/reviewId/binding/changesDigest/count JSON+LF, 각 item은 path/before(kind,mode,size,digest,identity 또는 null) JSON+LF 후 정확한 size bytes이며 null payload는0이다. 전체 hash만으로 restore 의미 검증을 대체하지 않는다.

1. 동일 UID 외부 작성자와 경합할 때 원본을 절대 덮어쓰지 않는 보장을 제공할 OS 전략과 지원 플랫폼 범위. 실패 시 수동 전달 경로는 대안이지 전체 적용 기능의 완료가 아니다.
2. 복원 지점의 보존 기간·용량·삭제 확인 UX. 수치 없이 제품 writer를 열지 않는다.
3. clean source의 submodule·LFS·필터·symlink·case-only rename 초기 지원 범위. 미지원은 UI에서 명확히 차단하되 원래 전체 제품 목표를 축소한 완료로 보고하지 않는다.
