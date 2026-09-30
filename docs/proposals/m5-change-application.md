# M5 변경 반영·복원 구현 계약 초안

상태: 구현 전. 2026-09-29, 기준 `1517906`. 제품 방향은 [M5 §5.5·§6.5·M5.3](m5-coding-agent-ide.md)이 우선한다. 이 문서는 원본 쓰기 권한을 활성화하거나 실제 사용자 프로젝트 시험을 승인하지 않는다.

## 확정 범위와 현재 격차

- 전체 작업 복사본의 명시 반영, 반영 직전 복원 지점, 명시 되돌리기. 부분 반영·자동 충돌 해결·자동 Git commit/push는 하지 않는다.
- 현재 `ProjectManager`는 source를 읽어 standalone snapshot을 만들고, snapshot 파일 편집과 diff를 제공한다. `workspace.close`는 폐기이며 `project.catalog releaseView`는 감시 해제다. 둘 다 적용이나 복원이 아니다.
- `ActiveProject`의 base commit/tree/source identity는 Git 기준만 담는다. 원본 working tree의 현재 bytes나 index의 동등성을 증명하지 않는다. `source_digest`는 Git 메타데이터 변경 감지용이지 원본 파일 baseline이 아니다.
- 현재 diff UI 파서는 적용 권위가 아니다. `SourceRepository`의 capability와 고정 plumbing 실행을 재사용하되, 사용자 filter/fsmonitor/hooks 실행 가능성이 있는 일반 status/diff/checkout 명령을 원본 검증·쓰기 경로에 그대로 추가하지 않는다.

근거: [Git 공식 config 문서의 core.fsmonitor](https://git-scm.com/docs/git-config#Documentation/git-config.txt-corefsmonitor)는 외부 hook 경로 설정과 index 갱신 명령(status 등)의 사용을 설명한다. 저장소 로컬 config를 읽는 현재 고정 Git launcher가 모든 명령의 무부작용을 보장하는 것은 아니라는 설계 판단이다.

## 실행 단위와 권위

### A1 baseline 정책 (구현 제안, 영속 스키마 확장 전)

내부 raw inventory 후보(`project/application.rs`)는 Unix에서 한 pass당 100,000개 항목·파일/링크 내용 합계 4GiB·경로 합계 8MiB·개별 UTF-8 경로 4,096byte·깊이 128을 제한한다. 두 pass의 결과를 대조하며 전체 내용 읽기 상한은 8GiB다. root `.git`만 제외하고 ignored/임시 이름/하위 `.git`을 포함한다. 하드링크·특수 파일·비 UTF-8 경로는 거절하며 링크는 대상이 아니라 링크 문자열을 기록한다. 파일 내용·mode·관측한 identity/ctime 변경을 검사하지만 원자 snapshot이나 동일 UID 외부 작성자 차단을 보장하지 않는다. 아직 snapshot 생성·원장·제품 API에 연결하지 않았고 HEAD/index 검증·영속 binding·legacy 정책 구현은 남아 있다. 이 내부 수치가 복원 백업 용량 정책을 대신하지 않는다.

새 snapshot 생성 시 no-follow raw source baseline receipt를 원장에 결속한다. receipt는 canonical root/실물 directory identity, HEAD/tree, index의 semantic entry 집합, `.git` 밖 파일의 경로·종류·mode·size·content digest·device/inode/link count를 bounded stream으로 담으며 capture 전후 동일성도 확인한다. 읽기에 따라 OS가 갱신할 수 있는 atime은 비교·무변경 보장에서 제외한다. 앱이 atime을 복원하는 쓰기도 하지 않는다. tracked 파일과 index가 HEAD의 raw tree와 다르면 첫 적용 지원 대상에서 제외한다. 필터/LFS 변환을 clean 명령으로 실행해서 동등성을 만들어내지 않는다.

Untracked/ignored 파일도 receipt 범위에 포함하되 snapshot 변경 집합에 자동 편입하지 않는다. 원본 재검사에서 receipt 범위 변화는 거절하고, snapshot의 새 파일/디렉터리와 그 파일들의 경로가 충돌하면 덮어쓰지 않는다. 이렇게 원래 존재했던 사용자 파일을 삭제 대상이나 소유 파일로 오해하지 않는다. 이 전체 집합의 byte/entry 상한과 초과 안내를 A1 구현에서 고정하며 큰 ignored tree를 조용히 제외하지 않는다.

기존 snapshot에는 receipt가 없으므로 원본 적용에는 사용할 수 없다. 기존 파일/채팅/diff 열기는 유지하지만 과거 baseline을 현재 source에서 소급 생성하지 않는다. 사용자에게 새 기준에서 작업을 시작하도록 안내한다. A1은 receipt 생성·영속 binding·재검증·legacy 거절 회귀까지 통과해야 하며, 그 전 A2 검토 묶음은 writer admission 근거가 아니다.

내부 `application/git_baseline.rs` 후보는 index image 32MiB와 각 Git 출력 24MiB를 제한하고, `ls-files -v --stage --sparse -z`와 `ls-tree -rz`의 경로/mode/OID 집합을 대조한다. staged 변경·unmerged stage·gitlink·split index·sparse directory entry·skip-worktree·assume-unchanged는 거절한다. full index 형태의 sparse checkout도 skip-worktree 표시로 차단한다. 같은 빈 blob으로 보이는 intent-to-add는 cached 전용 `diff-index --ita-invisible-in-index`로 추가 확인한다. 외부 diff/textconv/rename과 fsmonitor/hooks를 명시 끄고 index stamp/hash 및 HEAD를 전후 확인한다. 이 검사는 작업 폴더 bytes의 HEAD 동등성을 증명하지 않으며 raw blob 비교와 영속 receipt 결속은 별도 미완료다. unsupported Git 옵션도 허용으로 해석하지 않고 실패한다. 실제 사용자 저장소·외부 객체 fetch/관리 설정 전체 안전성 증거 없이 제품 입장에 연결하지 않는다.

명령 의미는 [Git ls-files](https://git-scm.com/docs/git-ls-files), [ls-tree](https://git-scm.com/docs/git-ls-tree), [diff-index](https://git-scm.com/docs/git-diff-index)의 공식 문서를 확인했다. `diff-index --cached`는 working-tree 파일을 비교하지 않으며 외부 변환은 별도로 끈다. 일반 `git diff`/`status`를 무설정으로 실행하는 정책이 아니다.

검토 대상은 project/chat/task/worktree/workspace identity, base commit/tree, 검토 시 snapshot 파일 집합의 digest로 고정한다. renderer는 원본 절대 경로·patch bytes·백업 경로를 전달하지 않는다.

1. **준비**: active workspace·원장·실행 종료 증거·writer 잠금을 확인한다. source의 canonical root 및 실물 디렉터리 capability, HEAD/tree, index, tracked bytes/mode와 대상 경로 충돌을 읽기 전용으로 대조한다.
2. **검토 묶음**: sidecar가 변경 파일의 before/after 존재·종류·mode·size·content digest와 묶음 digest를 만든다. rename은 표시용 관계이며 쓰기는 정확한 old/new path 항목으로 표현한다. binary·삭제·심볼릭 링크·권한 변경도 빠짐없이 요약한다.
3. **명시 확인**: 사용자는 정확한 검토 묶음 ID/digest를 확인한다. 그 뒤 source 또는 snapshot이 달라지면 재검토한다. 확인은 다른 실행/묶음/재시작에 재사용하지 않는다.
4. **적용 준비**: project-wide 앱 내부 writer 배타 잠금과 원본 재검사를 거친다. 원본의 이전 파일·mode·존재 여부, 새로 생성할 디렉터리 목록을 앱 소유 복원 지점에 보존하고 hash 검증·fsync·원장 intent 영속화 후에만 첫 원본 쓰기를 허용한다.
5. **적용**: 고정 capability 아래에서 정렬된 변경 항목을 적용한다. 부모 symlink/alias·hardlink·파일 종류/identity 변경은 거절한다. 각 항목은 durable intent → exact before 검증 → staged rename/삭제 → 디렉터리 동기화 → 완료 기록 순서다. 기존 `.git`·index·refs는 쓰지 않는다.
6. **완료**: 전체 after-image를 다시 확인한 뒤 완료를 기록한다. 파일 여러 개의 변경을 원자적이라고 표현하지 않는다. 중간 실패는 `recoveryRequired`이며 반영 성공으로 표시하지 않는다.
7. **복원**: 명시 선택한 복원 지점과 적용 결과를 대조한다. 현재 원본이 기록된 after-image와 다르면 사용자의 후속 편집으로 보고 자동 덮어쓰지 않는다. 복원도 자체 intent·항목별 진행·fsync·최종 대조를 가지며 완료 뒤에만 잠금을 해제한다.

## 반드시 닫을 안전 조건

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

1. 동일 UID 외부 작성자와 경합할 때 원본을 절대 덮어쓰지 않는 보장을 제공할 OS 전략과 지원 플랫폼 범위. 실패 시 수동 전달 경로는 대안이지 전체 적용 기능의 완료가 아니다.
2. 복원 지점의 보존 기간·용량·삭제 확인 UX. 수치 없이 제품 writer를 열지 않는다.
3. clean source의 submodule·LFS·필터·symlink·case-only rename 초기 지원 범위. 미지원은 UI에서 명확히 차단하되 원래 전체 제품 목표를 축소한 완료로 보고하지 않는다.
