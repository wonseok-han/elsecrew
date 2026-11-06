# Hello, Agent 기술 설계

> **Windows 원본 파일 이동 후보(미연결·미검증)**: `application/review/intent/windows_writer.rs`는 archive에 결속된 pending 항목에 private 이름/root·parent ID를 먼저 예약하고 guard/새 내용·owner/group/DACL을 준비한다. 기존 파일은 retained로, staged 파일은 원본 이름으로 각각 비덮어쓰기 이동하며 syscall 전 intent와 sync/신원·내용 재대조 후 receipt를 따로 저장한다. 복원은 원본(after)→detached, retained(before)→원본이며 최종 표현은 원본=before·detached=after·retained 부재다. `restoredEntry`는 `retainedEntry`와 별도이며 macOS swap completion을 그대로 사용하지 않는다. 상대편 파일은 이동 중 write/delete를 막는 handle로 보유한다. 중단 intent는 자동 재시도하지 않으며 partial 파일/잠금을 보존한다. Windows 전용 recovery는 exact pending/archive와 held 양쪽/상대편을 비교해 unmoved/moved/conflict를 분류하고, 명시 acknowledge에서 moved destination을 새 write-through handle로 sync·재관측한 뒤 단일 phase만 CAS 게시한다. phase별 receipt를 archive 및 앞선 receipt에 결속하며 실제 rename·삭제·잠금 해제는 하지 않는다. applied/restored private 이름 배치의 pending-item 읽기 전용 검사도 존재한다. beforeMoved의 명시 부분 취소는 cancelStagedIntent로 staged→detached를 보존한 뒤 retained→원본을 복원한다. 취소 표시가 있는 기록은 installedEntry를 만들지 않고 detached를 staged에 결속한다. installIntent의 unmoved 증거는 별도 cancelReady로 전환할 수 있으며 설치를 재시도하지 않는다. staged 또는 unmoved preserve intent의 취소는 원본 exact before를 확인해 canceledUnwritten/restored로 기록하고 준비 파일을 그대로 보존한다. 해당 완료 배치는 원본+staged, retained/detached 부재로 별도 검증한다. Windows의 항목 전진·미시작 tail 취소·역순 이전 항목 선택은 공유 cursor에서 이 플랫폼별 판정을 사용한다. 공유 batch completion은 전체 archive/항목·미시작 tail과 Windows 파일 배치를 두 차례 대조한 뒤 project/lock CAS로 마감한다. 내부 fresh apply owner와 explicit Windows restore owner도 이 흐름에 연결됐다. prepared handoff 없는 새 반영은 없으며 오류/취소 owner는 재사용하지 않는다. 명시 Windows restore owner는 복원3종 intent의 moved를 확인·동기화하고, unmoved는 rename권한을 보유한 exact source와 목적지 부재를 다시 검증해 기록된 복원 이동만 재개한다. 원본 보존/새 내용 설치 intent의 apply 재개는 거절하며 모든 실제 파일 변경은 applying fence를 먼저 요구한다. 일반 파일의 내부 준비·이동·마감/실행 연결이 있으며 제품 scheduler 연결·reserved 부분 생성 처리·디렉터리·추가 Windows metadata·cleanup/native 검증과 제품 활성화는 남아 있다.

> **2026-11-06 Windows 저장·복구 검증**: [CI42129091905](https://github.com/wonseok-han/hello-agent/actions/runs/42129091905), 공개6e079a9에서 native44개와 직접 복구 호출 차단1개가 통과했다. guard/권한/handle 교체·정리 및4개 seeded snapshot 파일/SQLite 재열기를 포함한다. 사용자 설치 Git의 admission·원본 반영/복원 포트·실제 프로세스 재시작·전체 제품/성능 검증과 활성화 결정은 남아 있다. 아래 native 미검증 표기는 이 진단 범위 밖 시나리오까지 합격했다는 의미로 읽지 않는다.

> **최근 구현·검증 경계**: Windows 저장 복구의 prepared/replaced/rename 직후 receipt 누락/cleanup 완료 후 원장 미완료4개 상태를 실제 파일·SQLite 재열기 fixture로 작성했다. 2026-11-06 Windows CI42129091905에서4개 모두 통과했다. snapshot manifest는 seed하므로 Git admission·프로세스 강제 종료/startup E2E 증거가 아니다. 편집기는 파일 전환 시 단일 Monaco 화면을 유지하고 이전 model을 해제하도록 변경했으며 실제 GUI·메모리 효과는 미측정이다. 최신 남은 실행/선택은 docs/history.md를 따른다.

> **Windows 저장 flush 정책(scoped native 통과)**: 보유 handle의 filesystem이 NTFS인지 확인하고 source/guard를 WRITE_THROUGH로 생성한다. 새 intent의 ntfsWriteThroughV1 정책과 교체 후 동일 source handle flush 성공을 ntfsWriteThroughFlushed로 기록하며 save는 exact receipt/현재 source ID를 검증한다. [Microsoft의 NTFS rename metadata flush 설명](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)에 근거한 OS acknowledgement이며 모든 하드웨어의 전원 장애 보장은 아니다. ReFS/FAT/미확인 FS와 과거 policy 없는 기록은 자동 허용하지 않는다. 정상 cleanup·명시 재시작 복구는 연결됐으며 native 검증 전에는 Windows 제품 gate가 닫혀 있다.

> **Windows 임시 파일 정리 primitive(정상 저장 연결·scoped native 통과)**: windows_remove는 caller가 결속한 독립 DirectoryLease 아래에서 정상 leaf를 read+write+DELETE/share0/nofollow/WRITE_THROUGH로 열고 기록된 full ID와 일치할 때만 FileDispositionInfo를 호출한다. guard alias/다른 ID/사용 중 파일을 거절하고 명시 CloseHandle 성공 뒤 부재를 확인하며 path-delete fallback은 없다. 정상 save는 cleanupIntent/guardRemoved/stagingRemoved/cleanupComplete를 원장에 순차 기록한다. 단계 실패는 복구 잠금을 유지하며 명시 재개는 구현됐으나 native 검증은 남아 있다. Windows 제품 admission은 계속 닫혀 있다.

> **Windows 정상 저장 정리(구현 체크포인트)**: staging 파일의 임시 pin 아래 고정 staging anchor를 create-new/write-through/flush하며 기존 anchor는 읽기만 해 정확한 표식을 확인한다. 원장에 staging parent/anchor/file ID를 결속한다. 교체 flush 뒤 기존 guard를 유지한 채 source handle을 해제하고 committed target의 lease로 parent pin을 인계한다. cleanupIntent 게시 → guard exact-ID 삭제 → guardRemoved → staging handle 해제/anchor lease 아래 삭제 → stagingRemoved → target ID/내용/권한 확인 → cleanupComplete 순서다. target pin은 최종 잠금 해제까지 유지한다. Windows 채팅 저장의 최종 잠금 해제와 save session 삭제는 lock/chat revision을 확인하는 단일 SQLite transaction으로 게시한다. 이 원자 게시 구현은 save_completion 모듈에서 공용으로 검사하며 실제 SQLite 재열기·두 번째 쓰기 충돌 rollback·다른 workspace 거절의 로컬 통합 회귀를 통과했다. 이는 Windows 파일/세션 전체 재시작 증거를 대신하지 않는다. 실패하면 둘 다 이전 상태를 유지하며 V2의 chat 없는 저장은 기존 lock-only 경로다. 정상 저장 성공 경로의 구현 존재와 Windows 실사용 합격은 구분한다.

> **저장 복구 확인 흐름(연결됨·실행 비활성)**: workspace.resolveSaveRecovery는 workspace 참조/saveId/expectedLockRevision/complete 또는 abort/confirmed:true를 정확히 요구한다. main-frame IPC·preload·client의 요청/결과 검증을 거쳐 내부 재개/취소에 연결했다. 화면은 관찰과 phase가 맞는 동작만 제안하고 한 번 더 확인받으며, 요청 전 관찰을 소모하고 오류는 결과 불확실로 처리해 자동 재시도하지 않는다. `WINDOWS_SAVE_RECOVERY_MUTATIONS_ENABLED=false`가 조회 admission과 sidecar 실행 양쪽을 제어하므로 현재 직접 IPC 호출도 거절된다. native QA와 별도 활성화 결정 전에는 실제 정리를 수행하지 않는다. 검증된 성공 응답은 현재 workspace의 편집기/diff에 전달한다. 열린 초안과 기존 baseVersion은 보존하고 다시 불러오기에 명시 확인을 요구하며 이전 diff는 폐기 후 재조회한다. 다른 workspace나 닫힌 화면의 늦은 응답은 반영하지 않는다.

> **저장 복구 조회 통신(읽기 전용)**: workspace.saveRecovery는 정확한 chat/project/worktree/workspaceIdentity만 받는다. Windows에서 원장과 현재 작업복사본을 검증해 saveId·상대 파일 경로·중단 phase·lock revision·관찰 분류만 반환한다. 정상 Ready 경로만 recovery:null을 반환하며 다른 복구 잠금은 빈 결과로 숨기지 않는다. 다른 플랫폼은 unsupportedCapability다. Electron current-main-frame IPC와 m5SaveRecovery.inspect preload 및 응답 바인딩 검증을 연결했다. actionsAvailable은 닫힌 실행 gate를 반영하며 Windows 활성 채팅 화면의 접힌 저장 복구 조회 영역에 연결했다. 새 파일 관찰/기존 파일 관찰/내용만 일치/충돌·부재를 한국어·영어로 안내하고 기술 식별자는 별도 상세에 둔다. workspace 주소나 admission 변경 때 조회 소유자를 다시 만들어 늦은 응답을 폐기한다. 복구 명령은 gate 뒤에 연결돼 있으며 Windows native/UI 실행 검증과 활성화 결정은 후속이다.

> **Windows 교체 전 저장 취소(내부·미활성)**: 최초 preparing intent는 기존 target ID/내용/접근 권한을 결속한다. 명시 abort는 inspected lock revision/canonical workspace와 기존 파일의 동일성을 확인하고 root/target/staging lease 아래 abortIntent→abortSourceRemoved→abortGuardRemoved→abortStagingRemoved→abortComplete로 정리한다. ID가 기록된 임시 파일만 삭제하고 ID 미기록 이름은 이미 없을 때만 허용한다. 현재 존재하는 미기록 파일이나 외부 수정은 보존하고 잠금을 유지한다. 완료는 saveAborted/abortedWindowsSave 및 preserved_version으로 남겨 새 저장 성공과 구분하며 lock/chat session은 원자 갱신한다. 공개 확인 UI까지 연결됐으나 native 실행 검증과 제품 활성화는 남아 있다.

> **Windows cleanup 명시 재개(내부·미활성)**: resume_windows_save_cleanup은 사용자가 검토한 lock revision과 durable save session/정확한 workspace 결속을 요구한다. replaced 이후 NTFS flush receipt가 있는 단계와, replaceIntent에서 기록된 replacement가 실제 destination에 있는 경우를 지원하며 target/staging anchor lease와 디렉터리 ID·target 내용/권한을 재확인한다. 최초 cleanupIntent 전에는 guard/staging 실재 ID를 요구하고, 기록된 삭제 의도 이후에만 이미 없는 파일을 인정한다. 재사용된 이름은 삭제하지 않는다. cleanupComplete 뒤 lock/chat session 완료를 원자적으로 게시한다. replaceIntent 재개는 source 이름 부재와 target ID/내용/권한 일치 뒤 같은 재개 handle을 NTFS/write-through/read-write로 열어 bounded 검증·flush를 수행한 경우만 replaced 기록으로 넘긴다. rename을 다시 실행하지 않는다. 교체 전 상태는 별도 abort 경로로 연결했고 공개 계약/UI도 존재한다. native 통합 검증은 남아 있다.

> **Windows save 복구 관찰(내부·미활성)**: windows_save_recovery는 정확한 durable save session/lock과 canonical workspace binding을 대조한 뒤 guard/source/staging/destination의 full ID와 5MiB 한도 내용 hash를 읽는다. 이전 target ID+내용, replacement ID+내용, 내용만 일치, 충돌/부재를 구분하며 ID 미기록 파일을 정리 소유권으로 승격하지 않는다. 일반 Ready 잠금 검사는 유지한다. 조회 자체는 cleanup/completion을 허용하지 않는다. 명시 확인 UI와 별도 resolve 명령이 연결됐으며 native 검증/활성화는 후속이다.

> **Windows replacement 접근 권한(구현 중·native 미검증)**: windows_security는 열린 파일의 owner/group/DACL과 protection 상태를 읽는다. source가 이미 정확히 같은 descriptor면 원본 재확인 뒤 유지하고, 다르면 SetSecurityInfo로 복사한다. 교체 전 old/source 정규화 descriptor 일치와 교체 후 destination 일치를 확인하며 원장에는 접근 descriptor digest만 남긴다. 권한 상승은 없고 보존 실패·상속 결과 차이는 거절한다. SACL/감사·integrity label 등은 미구현이며 전체 메타데이터 보존이나 Windows 출시 합격을 뜻하지 않는다.

> **Windows save guard 준비(구현 중·native 미검증)**: 저장 준비는 기존 target pin을 유지한 채 현재 managed root/workspace/parent 신원을 확인하고, lock 원장에 preparing 의도를 기록한 뒤 독립 guard를 생성·flush·pin하여 ready와 전체 file ID를 기록한다. 첫 intent 실패 전에는 교체용 guard/source를 만들지 않으며, intent 성공 이후 실패는 staging과 workspace save_session을 함께 보존한다. save lock 갱신은 cleanupComplete receipt와 save/workspace/version/flush 결속을 확인한 정상 경로에서만 해제하며 완료 증거를 completedWindowsSave로 보존한다. guard Drop 자동 삭제는 없다. replacement source 이름도 생성 전 intent에 포함하며, flush 뒤 full ID/version을 sourceReady로 기록한다. source는 READ|WRITE|DELETE/share-read-only handle을 계속 보유하고 Windows PreparedTarget Drop은 path-only 삭제를 하지 않는다. 준비 단계의 directoryDurability는 unconfirmed이며 NTFS flush receipt 뒤 정상 cleanup을 연결했다. 명시 재시작 복구는 구현됐으나 native 회귀가 남아 있어 Windows 제품 admission은 닫혀 있다.

> **Windows replacement primitive(scoped native 통과·제품 비활성)**: `windows_replace.rs`는 source file handle과 pinned destination Dir handle·단일 leaf로 NtSetInformationFile의 FileRenameInformation(10) 교체를 요청한다. Win32 wrapper의 상대 경로87 오류로 이 경로를 수정했으며 최신 native 결과는 history를 따른다. 독립 guard를 유지하고 비정상 이름·guard 자체 교체를 거절하며 readonly/POSIX 우회·delete/copy fallback은 없다. Windows workspace save는 sourceReady와 현재 경로/target 버전·ID 검증 뒤 replaceIntent → 보유 source handle의 교체 → destination full ID/내용 검증 → replaced를 기록한다. 이는 기존 macOS와 같은 관측 충돌 검사와 atomic rename이며 hostile same-user leaf 변경의 원자 CAS를 보장하지 않는다. CAS를 기존 필수 요구로 본 11/05 검토는 계약 대조 후 철회됐다. 내부 정상 경로는 cleanupComplete 뒤 저장 성공을 반환하도록 연결했다. 재시작 복구는 구현됐으나 추가 메타데이터 정책·native 검증이 미완료여서 Windows 제품 admission은 닫혀 있다. 이 primitive 자체는 save transaction이 아니다. 기존34 PASS는 이 신규 코드를 포함하지 않는다.

> **2026-11-05 이전 Windows 기반 native 증거**: [CI41988529910](https://github.com/wonseok-han/hello-agent/actions/runs/41988529910), 공개0a7efd5(f41eb29+시험 핸들 수명 수정)는 native29·spawn1·watcher4, 합계34 PASS/0 FAIL/0 ignored다. CWD pin/ACL/reparse·실제 GUID image/CWD 실행·공용 control reader EOF·unwind retention을 포함한다. background watcher의 실제 Git 검증·공식 Git 설치본 신뢰·파일 교체/복구·전체 제품/성능 합격은 포함하지 않는다.

> **Windows watcher background 연결(컴파일 확인·종단 실행 미검증)**: Unix와 Windows는 한 번에 하나의 소유형 repository 검증 worker를 사용한다. 결과는 dispatcher에서 현재 stream/active binding 및 보유 root/.git 신원과 다시 대조하며, Windows는 full FILE_ID_INFO를 사용한다. metadata/write-tree 재검증 뒤 bounded scan을 이어간다. 취소 후 worker join을 유지하고 Windows 실제 Git admission은 계속 차단한다. 따라서 연결 구현을 Windows 실사용/성능 합격으로 해석하지 않는다.

> **Windows 공용 Git reader adapter(scoped native 검증)**: `windows_git_process.rs`가 공용 snapshot/diff reader의 StreamingGitChild와 pipe 타입을 제공한다. typed image/CWD로 생성한 Transport만 소유하고 bounded pipe·전체 자손 종료 확인·Windows ExitStatus 변환을 수행한다. 일반 Command 실행은 거절하며 Windows의 이전 parent-only Child fallback은 사용하지 않는다. 실제 Git 설치본 신뢰·admission 연결은 여전히 미완료다.

> **2026-11-06 사용자 설치 Git 연결(구현 중, native 미검증)**: native 파일 선택을 main 소유 설정으로 저장하고 Rust가 동일 앱 데이터 폴더에서 bounded schema/path 검사를 수행한다. 선택한 파일과 동일 설치 root의 실제 runtime을 handle로 결속하고 Authenticode 성공 state에서 주 서명자의 CN/조직을 읽어 대조한다. runtime 폴더 DLL inventory는 파일/부모 pin과 목록 재확인을 보유한다. 선택 파일과 DLL pin은 runtime lease에 함께 실어 기존 family owner가 종료 확인까지 보유할 수 있다. 이는 설치 출처/배포자 허용 정책이나 실제 DLL load 허가가 아니며 전체 의존성·spawn 연결은 남아 있다. 일부 공식 DLL은 서명이 없어 모든 DLL 서명 필수 조건으로 대체하지 않는다. Windows 제품 gate는 비활성이다.

> **Windows 실행 이미지 결속(scoped native 검증)**: `windows_executable.rs`는 명시적으로 선택한 부모 handle 아래 실행 파일과 부모 경로를 보유하고 전체 파일 ID를 기록한다. 쓰기/delete 공유를 허용하지 않는 image handle을 실행 수명 동안 유지한다. `Transport::spawn_bound`는 image와 CWD lease를 creator supervisor로 넘기며 첫 CREATE_PROCESS 이벤트의 실제 hFile을 선택한 파일 ID와 비교한 뒤에만 이벤트를 계속한다. 거절 시 user code를 계속하지 않고 종료를 요청하며 pin은 미확인 종료 정책에 따라 유지한다. 이는 파일 신원 결속이지 공식 Git 설치본·서명·기존 writable mapping·DLL/자손 실행 파일의 신뢰 보장이 아니다. 실제 Git admission은 계속 닫혀 있다. 아래 최신34 PASS의 fixture 범위에서 image 결속을 확인했다.

> **Windows CWD lease 기반(scoped native 검증·제품 미연결)**: `windows_directory_lease.rs`는 이미 보유한 디렉터리의 GUID volume 경로에서 각 성분을 handle-relative로 다시 열어 전체 신원을 대조한다. 명시적인 delete 공유 금지 핸들과 기존 직접 child pin으로 폴더의 nonempty 상태를 유지하고, 모든 pin 확보 뒤 전체 ID·no-reparse·부모/자식 결속을 확인한다. 사용자 파일 생성/수정은 하지 않으며 guard가 없는 빈 폴더는 거부한다. 이 기반을 모든 종류의 reparse 불변·프로세스 CWD 권한의 완성으로 보지 않는다. Family/Transport 내부에는 typed lease 경로를 연결했으며 생성 직전·resume 직전에 재검증한다. creator supervisor가 CreateProcess 성공 후 pin을 armed로 보유하고 전체 종료 확인 뒤 해제한다. 미확인 오류·unwind에서는 마지막 Arc를 sidecar 종료까지 보존하므로 durable fence/자동 재시도 금지와 복구 처리가 필수다. 이는 전역 자원 상한이나 자동 복구 완료 보장이 아니다. GUID CWD fixture 실행은 최신 native 검사에서 통과했다. 공식 설치본/의존성 신뢰와 실제 Git 연결을 마치기 전까지 제품 admission에는 사용하지 않는다.

> **Windows 자손 종료 supervisor(scoped native 검증)**: `project/windows_job.rs`는 별도 생성 스레드의 DEBUG_PROCESS와 Job의 생성 시 결속을 함께 사용한다. `windows_supervisor.rs`가 CREATE_PROCESS 이벤트를 계속하기 전에 process handle을 복제하고, EXIT 이벤트를 계속한 뒤 모든 복제 핸들의 signal·root signal·Job 누적 멤버 수의 일치·active 0을 함께 요구한다. 일반 Job 알림이나 active 0만으로 종료 완료를 반환하지 않는다. 중첩 debugger 등으로 추적이 끊어져 누적 멤버 수가 다르면 실패하며, 4,096개 추적 상한·오류·시간 초과·Drop을 완료로 승격하지 않는다. 이 구현은 Windows Git/제품 경로에 아직 연결하지 않았으며 G4/stdio·실제 공급자 gate는 닫혀 있다. 3a02b76의 [CI41980701769](https://github.com/wonseok-han/hello-agent/actions/runs/41980701769)에서 기존 실제 child signal 회귀와 짧은 자손·중첩 debugger 거절을 포함한 native20개, spawn 잠금1개, watcher4개가 통과했다. 이 결과는 CWD/실행 파일 권한·원본 쓰기·전체 Windows 제품 검증을 포함하지 않는다. 근거: [Microsoft Debugging Events](https://learn.microsoft.com/en-us/windows/win32/debug/debugging-events), [Process Creation Flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags).

> **Windows 파일 접근 기반(scoped native 검증)**: `project/windows_fs.rs`에 열린 핸들의 전체 파일 신원·재분석 속성·local GUID volume 검사와 component 탐색을 구현했다. 디렉터리/파일은 OPEN_REPARSE_POINT로 열어 대상에 접근하기 전에 재분석 지점을 거부한다. `windows_space.rs`는 열린 동기 핸들로 native volume 정보를 조회해 사용자 quota 기준 여유 공간을 계산한다. 2026-11-05 scoped native CI에서 파일 신원·reparse·held-handle quota 회귀는 통과했으나 전체 Windows 제품 검증은 남아 있고 G4/stdio 및 실제 공급자 gate는 비활성이다.

> **저장된 원본 폴더 재확인**: project projection의 비공개 `native_path_binding`은 손실 없는 변환으로 확인한 canonical 원본 경로를 기록한다. 새 프로젝트는 native 폴더 선택으로 기록하며, 이전 기록은 목록/대화 이력을 유지하되 폴더를 다시 선택하기 전에는 catalog reopen과 원본 접근을 거부한다. 명시 재선택은 기존 작업 복사본·편집·복구 상태를 보존한다. marker가 있는데 경로가 불일치하면 자동 수정하지 않는다. 원본을 열지 않는 반영 이력 조회와 시작 전 snapshot 복구는 metadata 전용 조회를 사용해 재선택 전에도 유지한다. 이 표식은 원본 선택만 증명하며 종료·반영·복구 승인을 대체하지 않는다. 과거 손실된 경로 철자는 자동 복원하지 않는다.

> 프로젝트 경로 인코딩: `project/protocol_path.rs`가 native 경로를 기존 canonical wire 계약으로 변환한다. Windows local drive 및 정규화 가능한 verbatim drive 경로만 지원하며 namespace/ADS/예약 장치명/후행 점·공백 등 의미가 달라질 수 있는 입력은 거부한다. Unix 파일명의 역슬래시를 슬래시로 바꾸지 않는다. 이 변환은 파일 신원·핸들 검증이나 IO 권한의 증거가 아니며 Windows G4/stdio 차단은 유지한다. 과거 손실 변환된 기록의 원래 철자는 이 parser로 복구할 수 없으며 별도 재선택/이관 계약이 미완료다. Windows 이름 해석 근거: [Microsoft 파일·경로 규칙](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

> 일반 작업 복사본 버리기: `reviewReady` 대화에서 명시 동의 후 해당 snapshot을 abandoned/chat을 discarded로 마감한다. 원본·대화·복사본 파일은 삭제하지 않는다. 종료 증거, writer/recovery/준비 중 작업, 주소/manifest 신원과 revision을 재검증한다. 완료 receipt로 응답 유실을 재개하고 releaseView를 확인한 뒤 편집/대화 화면을 해제한다.

> 앱 기본 역할: 작업대의 `앱 기본 역할·모델`에서 저장한 세 역할은 앱 데이터의 `application-settings-v1/role-defaults.json`에 revision과 함께 원자 저장한다. 프로젝트 역할 설정을 처음 만들 때만 복사하며, 기존 프로젝트 설정과 채팅 생성 snapshot은 변경하지 않는다. `project.catalog`의 `getAppRoleDefaults`/`updateAppRoleDefaults`는 기존 역할 계약과 expected revision을 검사한다. 기본값 파일이 손상되면 임의의 값으로 덮지 않고 오류를 돌려주며, 이미 설정된 프로젝트는 해당 파일을 읽지 않는다.

> **G6 명령 연결(2026-10-22)**: `m5:test:performance`는 기존 측정 판정기를 `.artifacts/m5-performance/measurement.json`에 연결한다. 아래의 명령 미완성 설명 중 등록 부재는 해소됐지만 전체 실측 collector는 여전히 미완성이다. 누락/부분 진단/불일치 HEAD는 합격시키지 않으며 하네스 목록의 COMMANDS_READY는 명령 등록 상태만 뜻한다.

> **조회 메모리 수명(2026-10-22)**: `Storage::projection_record`와 `projection_records`는 모든 projection 행의 디코딩·오류 검사를 유지하되 요청한 종류/ID의 결과만 보관한다. 시작·재생 검증의 전체 projection 경로는 유지한다. 캐시나 검증 생략이 아니며 전체 행 읽기 비용도 남는다. 패키지 RSS 기준 충족은 별도 실측 대상이다.

> **연결 복구 상태 갱신(2026-10-22)**: 아래 10월21일의 패키지 미반영·과거 잠금 미완료 설명은 후속 작업으로 갱신됐다. 결속 복구는 64860d2 소스 패키지에 반영·smoke 검증했고, 과거 비결속 파일 1건은 명시 승인된 시험 전용 도구로 재검증 후 제거했다(45c28d6). 일반 제품 경로의 legacy 거절은 유지한다. 기존 인증으로 Codex·Claude 연결 표시와 정상 종료를 관측했으나 이것을 실제 도구 작업 성공으로 확대하지 않는다.

> **결속된 실제 작업의 연결 잠금 복구(2026-10-21, 소스 구현)**: 새 native Codex/Claude 실행은 spawn 전에 pending 파일에 run ID·connection UUID·공급자·파일/디렉터리 신원을 동기화하고 동일 receipt를 실행 원장에 저장한다. 명시적 `provider.login recover`는 기존 로그인 종료 marker 경로에 더해 이 결속 경로를 검사한다. 정확한 receipt·프로세스 집합, resolvedAbandoned/cleanExit, 폐기된 채팅·manifest, 해제된 writer lock 및 현재 프로세스 소멸을 모두 확인하고 잠금 파일을 배타적으로 보유한 채 해제한다. 부분 기록·살아 있는 소유자·불일치·결속 없는 과거 파일은 거절한다. 사용자 인증과 원장은 변경하지 않는다. 일반 시작 시 자동 해제하지 않으며 현재 실행 중인 패키지에는 아직 반영하지 않았다. 과거 비결속 잠금 복구는 별도 미완료다.

> **Codex 설정 구조 검사(2026-10-21, 활성화 승인 반영)**: 앱 전용 config.toml은 64KiB 이하 TOML로 읽고 `cli_auth_credentials_store=file`을 의미상 검증한다. 주석/공백 차이는 허용하고, 알 수 없는 설정·중복·잘못된 값은 거절한다. 사용자 승인으로 유효한 `projects` 신뢰 기록이 있다는 이유만의 프로필 입장 차단을 제거했다. 로그인·상태 조회·discovery·실제 실행과 버전 확인 launcher는 저장된 project 키 및 home/cwd 조상 경로를 invocation-only untrusted로 지정하고 hooks/apps/plugins를 끈다. native preflight는 플래그와 effective 설정 일치를 요구하며, 알려진 격리 경고는 config/read 응답 대기 중 1회만 보류해 실제 disabled project layer 경로·사유와 대조한다. 로그인/상태 확인도 계정 조회·OAuth 전에 cwd=home의 config/read(id60)를 거친다. 임시 프로필에서 로그인 설정 응답·버전 확인과 별도 첫 작업 요청 hook 격리 대조를 확인했다. 입장 변경 후 실제 계정 연결 성공·패키지 검증은 별도로 남는다. 설정 파일이나 인증을 자동 변경하지 않는다.

> **Claude 내장 조직 guard(2026-10-19 승인·구현)**: native 소유 연결만 단일 `name=cc-plugin-sec-default`, `path=builtin`, `source=cc-plugin-sec-default@builtin` 등록을 허용한다. 나머지 필드·다른 항목·중복·혼합은 거절하며 generic/fixture 관측은 빈 목록 조건을 유지한다. 이는 서명 확인된 공급자 부모 신뢰 모델 안의 예외이지 등록 문자열 자체의 인증이 아니다. guard를 비활성화하지 않으며 도구/MCP/cwd/권한·종료·원본 보호 조건은 유지한다. 같은 init receipt에서 확인한 guard 존재는 phased session의 `observedBuiltinPlugins`에 기록하며 기존 필드 없는 원장은 빈 목록으로 읽는다. 1cb7843 패키지의 승인된 Claude opus 단독 시험에서 `HELLO_AGENT_SMOKE_OK` 응답·성공·종료 확인 UI와 임시 원본 불변을 확인했다(run `agent_run_8af93d90879f487590f772883880d706`). 원시 init 전체 형태를 별도 추출한 증거는 아니며 도구 쓰기/승인/중단/복구 전체 성공으로 확대하지 않는다.

> **세션 연결 진단(2026-10-19 소스 구현)**: 실패한 Codex·Claude 실행의 정보 영역에서 이번 앱 세션의 알려진 연결 실패 단계명을 조회한다. 별도 입력 없는 `m5:session-diagnostics:v1`은 현재 main frame/origin만 허용하며, main이 sidecar의 bounded stderr tail에서 정확한 허용 목록과 일치하는 단계명만 최대32개 반환한다. 원문·인증정보·경로는 반환하지 않으며 실행 ID 결속·재시작 영속성·원인 확정·합격 증거가 아니다. 알 수 없는 출력/빈 목록은 정상으로 판정하지 않는다. 이 조회는 에이전트 재실행·원본 쓰기·복구 상태 변경을 하지 않는다. 기존 배포 패키지에는 재빌드 전까지 적용되지 않는다.
> **실패 단계 보존(2026-10-30 소스 구현)**: Codex item/started의 거절 위치는 shape·identity·순서·예산·미지원 타입의18개 고정 구분으로 진단하며 원문/임의 타입명은 노출하지 않는다. Electron은 같은 허용 목록의 완료된 stderr 줄만 분류하여 `app.getPath("logs")/last-provider-checkpoints.json` 한 파일에 마지막 비어 있지 않은 snapshot을 비동기로 원자 교체한다. 진행 중 쓰기1건과 최신 대기1건만 유지하며 앱 종료는 저장 완료를 기다리지 않으므로 급작스러운 종료 시 마지막 진단은 유실될 수 있다. 공급자별 최대32개 고정 단계명·포착 시각·로컬 앱 세션 UUID만 저장한다. 원문/대화/인증/경로/실행 ID는 저장하지 않으며 과거 snapshot을 현재 세션 IPC 응답에 섞지 않는다. 빈 새 세션은 이전 증거를 지우지 않고, 저장 오류는 실행/종료 판정에 영향을 주지 않는다. 기존 로그 폴더 symlink는 거절하며 동일 사용자 권한의 악의적 경로 교체 경쟁까지 방어하는 계약은 아니다. 이 파일은 진단용이며 원인 확정이나 실행/복구 권한의 증거가 아니다. 7db00dd macOS 패키지에서 실제 Codex 실패 시 고정 단계 저장과 정상 종료 후 보존을 확인했다. 단계명만으로 상세 원인이나 에이전트 성공을 판단하지 않는다.


> **명시 설치·업데이트(2026-10-13 사용자 활성화 승인)**: macOS `provider.install`은 준비→쓰기 위치 표시→180초 단발 동의→공식 스크립트 다운로드/실행→서명·경로 검증→종료 확인·원장 보관으로 연결된다. 시작/로그인·실행은 상호 배제하며 응답 유실에서 시작을 자동 재전송하지 않는다. 고정 쓰기 폴더와 임시 폴더만 허용하는 deny-default sandbox, 기존 hardlink 거절, 설정 파일 쓰기 금지, 단계별 프로세스 fence·명시 복구를 유지한다. read-only 기기 진단은 OS/CPU/RAM/앱 저장공간을 표시한다. 임시 HOME에서 Claude2.1.296/Codex0.162.1 신규·재설치를 검증했으며 사용자 HOME·다른 버전/Windows·인증 성공을 보장하는 증거는 아니다. 마지막 재검증 뒤 hostile same-UID 외부 교체까지 완전히 방어하는 모델은 아니며 Windows 설치는 비활성이다.

> **현재 검증 경계(2026-10-13)**: macOS 일반 실행 경로에서 Codex `gpt-6.1-sol/medium`의 작은 planning 작업은 실제 응답·cleanExit·임시 원본 불변·앱 정상 종료를 확인했다(c5dde79, 974Ab5). Claude는 같은 시험의 요청 ID에 결속된 로그인 확인이 idle여서 실행하지 못했다. 아래 과거의 내부 후보/미연결 설명을 최신 Codex planning 경로의 상태로 인용하지 않는다. 도구 쓰기·승인·명시 중단·복구 전체와 두 공급자의 완성은 별도 미완료다.
>
> **현재 작업대(2026-10-22, a2deb80)**: 도킹 패널 메뉴·집중/복귀, 파일명 말줄임·키보드 탭 이동, 마지막 diff 선택 보존·목록 갱신 시 이전 본문 무효화, 대화 기록과 후속 입력 영역 분리를 구현했다. 변경 목록에서 선택한 줄 diff는 editor dock에 표시하고, 편집기는 숨김 상태로 모델·미저장 초안을 유지한다. 편집 복귀는 늦은 diff 요청을 무효화한다. 첫 작업 안내는 파일·diff·채팅 작업 진입 때 접히며 수동으로 다시 열 수 있다. Monaco 텍스트 동기화는 읽기 전용 옵션 갱신과 분리한다. 중앙 diff·안내 접기는 c29fa8d 패키지에서 편집/재시작·명시 반영/복원 흐름을 확인했고 a2deb80 옵션 분리는 아직 패키지 실측 전이다. 전체 시안 충실도 합격은 아직 아니다.
>
> **현재 성능 증거(2026-10-22)**: c29fa8d 실제 패키지의 프로젝트/재시작 진단(YcnTnA)은 기능 assertion과 정상 종료는 확인했지만 idle458719232/459456512 bytes로400MiB를 초과해 FAIL이다. dfaf6e3 대형 프로젝트 단회 입력은50.6ms로50ms 기준 초과다. 서로 다른 시나리오의 수치를 개선률로 비교하지 않는다. f168262 무CDP 수집기는 고정12초 대신 visible/enabled 시작 버튼의 두 frame 준비 신호 후2초부터 측정하며, 신호는 허용된 main frame의 고정 문자열만 stdout으로 전달한다. 진단용이며 지속 가시성·실제 paint 완료나 실행 권한을 보장하지 않는다. 이 새 경로의 GUI 실측은 화면 잠금으로 미실행이다. 전체 G6 collector와 `m5:test:performance`는 아직 미완성이며 부분 진단을 release gate 증거로 쓰지 않는다. Windows는 M5.0 명세상 패키지 UI·sidecar·fixture 검증과 실제 에이전트 실행 차단 유지가 필요하며 native 실행 활성화를 자동으로 완료 범위에 추가하지 않는다.

> Claude 내부 소켓 호환(2026-10-08): 실행 전 profile 검사는 여전히 모든 소켓을 거절한다. 서명된 부모를 소유한 live scope에서만 `profile/tmp/srt-mux-<추적된 Child PID>-<base36>.sock` 직계 항목을 uid·single-link·group/other write 금지와 함께 검사한다. 소켓이 존재하면 scan 전후 정확한 PID/start_us 생존을 요구한다. 경로는 retained capability/no-follow로 확인하고 작업 복사본 검사는 변경하지 않는다. 소켓이 사라진 정상 종료 뒤에는 버퍼 출력 수신을 허용한다. 이는 커널 소켓 peer 소유권 증명이나 같은 UID의 적대적 교체 방어가 아니며 기존 signed-parent 신뢰 경계 안의 호환 처리다. 실제 Claude 모델 실행 성공은 아직 미검증이다.

> 프로필 호환 검사 변경(2026-10-07, 독립 검토 완료·실연결은 후속): Codex/Claude 앱 전용 프로필의 top-level 생성물은 owner·mode·real-directory 또는 single-link regular-file로 검사하고 알려진 고정 설정/credential·기존 경로 타입·지시 파일 거절을 유지한다. Codex named-tool 경로는 별도 bounded no-follow scanner로 `tmp/arg0/codex-arg0*/`의 세 도구 alias가 현재 선택·재검증된 실행 파일을 정확히 가리킬 때만 링크를 허용한다. 일반 workspace/Claude/legacy outer scanner는 변경하지 않는다. 실계정 연결 복구는 아직 미검증이다.

> 작업대 표시 수명(2026-10-07): 파일 트리는 페이지500개 중 최대120행만 렌더한다. diff는 페이지별 hunk 위치를 색인해 현재200행만 만들며 전체 탐색·잘못된 hunk의 원문 fallback을 유지한다. 2026-10-29 한 화면/나란히 전환을 추가했으며 나란히 보기도 현재 페이지에서만 삭제/추가를 짝지어 원래 줄 참조와 metadata를 보존한다. 대화는200개 표시와 이전/다음/최신 이동을 제공하고 과거 페이지를 읽을 때 새 이벤트가 화면을 이동시키지 않는다. 원장과 조회 기록을 잘라내는 정책은 아니다. Monaco는 마지막 동기화 문자열을 재사용하고 종료된 editor는 미완료 read를 끝내며 늦은 descriptor를 거절한다.

> 최신 부분 성능 관측(2026-10-07, 2612b74): synthetic renderer10회에서 표시 시간·DOM 기준은 이내지만 전체 Electron subtree RSS는 idle399.94MiB/peak1036.83MiB/60초 잔류delta128.38MiB로 **FAIL**이다. 아래 예전 PASS 수치를 현행 메모리 합격으로 인용하지 않는다. 실제 native sidecar/agent·stream paint·전체 G6 합격 증거는 아니다. 증거와 한계는 `docs/history.md` 및 `procedures/m5-performance.md`에 기록한다.

> 원본 적용·복구 활성화(2026-10-07): macOS의 `source_writes_enabled`를 활성화했다. immutable 검토/일회성 확인, 정확한 복구 fingerprint, 원장 잠금, 디스크 여유 및 취소 검사와 no-follow/atomic rename writer를 그대로 거친다. 새 archive의 before payload와 apply의 after staging만 추가 할당으로 계산하고 restore는 볼륨별 순차 rename 여유를 사용한다. 임시 프로젝트 실제 UI에서 편집·diff·원본 반영·복원·두 채팅 전환을 확인했다. Windows/실제 사용자 프로젝트/출시 합격을 뜻하지 않는다.

> 명시 백업 정리(2026-10-13 사용자 활성화 승인): 독립 검토·관련 회귀 이후 사용자 승인으로 macOS에서만 `application_cleanup_enabled=true`를 사용한다. 현재 복구점은 완료돼도 삭제하지 않으며, 별도의 명시 retirement가 terminal 기록을 이력으로 옮긴 뒤에만 과거 백업 정리 후보가 된다. 정리는 최신 대상/fingerprint·원장 참조·단일 파일 신원·idle 상태를 재검사하고 pending 영속 기록→unlink→디렉터리 fsync→deleted 기록 순서로 진행한다. 응답 유실/중단 뒤에는 자동 재시도하지 않고 재조회 및 새 동의를 요구한다. 원본·잠금·과거 이력은 보존하고 자동 삭제/보존 기간 정책은 추가하지 않았다. Rust 공개 경로/결정적 중단 후 DB 재열기와 합성 Chromium의 동의 흐름 증거는 실제 사용자 자료·OS 전원 장애·패키지 종단 합격과 구분한다.

> macOS native 실행 연결(2026-10-07): 일반 `--stdio`가 native provider runtime을 선택한다. Rust/TypeScript의 독립된 고정 ready/status 목록은 `run.startFixture`를 포함하지 않고 fixture 목록 변경을 상속하지 않는다. sidecar/Electron은 새 fixture 실행을 거절하고 작업대는 데모 시작을 숨긴다. 기존 fixture 취소/복구는 유지한다. 비-macOS는 기존 blocked/G4 gate를 유지한다. 이 연결과 무계정 stdio 회귀는 실제 계정·유료 모델·패키지 UI 합격 증거와 별개다.

> Claude 추론 기본값: 저장된 역할의 `reasoningEffort=null`은 native 조회·예약·실행에서 `--effort` 생략으로 전달한다. 임의 수준으로 대체하지 않으며 실제 적용된 추론 수준을 관측했다고 주장하지 않는다. 명시한 수준은 모델 catalog의 지원 여부를 검사하고, 기본값/명시값 전환도 역할·실행 범위·요청 정책 결속 변경으로 거절한다.

> Claude native 실행: 저장된 채팅 역할로 조회/시작 공급자를 선택하고, `Admission`이 작업 사본·앱 프로필·원본 디렉터리 신원과 요청 모델/추론/권한을 재대조한다. 목표 전송 전에 `claude.stream.v2` preflight를 저장하고 실제 system/init 검증 후 여섯 관측(model/cwd/permission/tools/mcp/plugins)과 running을 원자 저장한다. 종료/복구에서도 이 결속을 검증한다. 파일·검색 대상과 셸 sandbox 해제 요청을 검사하되 공식 CLI 보호를 신뢰하며 관측 불가능한 관리 설정/정책 reload 불변성을 증명했다고 주장하지 않는다. capability는 선택 역할의 시작 시도 자격이며 모델 사용권·실행 성공 증거가 아니다. macOS 연결 후 실제 공급자/UI 및 최종 누적 검증은 별도다.

> Claude 로그인 업데이트 호환: macOS native 설치의 `~/.local/bin/claude` 링크 내용을 no-follow 부모 capability로 읽고, 고정 `~/.local/share/claude/versions/<숫자 X.Y.Z>` 또는 이에 해당하는 `../share/claude/versions/<숫자 X.Y.Z>`만 선택한다. 런처 자체를 실행하거나 PATH/임의 최신 파일로 fallback하지 않는다. 선택한 실파일은 Apple anchor + Anthropic TeamIdentifier `Q6L2SF6YDW` + `com.anthropic.claude-code` 서명과 기존 파일/상위 경로 stamp로 검증한다. 기존 버전/SHA 목록은 제거했고 auth·profile·코드 입력·종료 경계는 유지한다. native 설치 외 npm/Homebrew·custom launcher 지원은 이 경로의 범위가 아니다. [공식 설치/업데이트 문서](https://code.claude.com/docs/en/setup#auto-updates)의 런처 구조와 현재 설치2.1.285의 읽기 전용 선택/서명 검사를 확인했으며 실제 계정 로그인 회귀는 별도다.

> Codex 로그인 업데이트 호환: macOS 로그인·설치 진단은 고정 bundle 후보의 native CLI/helper를 Apple 서명 anchor + OpenAI TeamIdentifier `2DC432GLL2` + 각각 `codex`/`codex-code-mode-host` identifier로 검증한다. 버전별 파일 SHA 허용 목록은 이 경로에서 제거했다. 검증 시점 bytes의 SHA는 실행 중 교체 감지에만 사용하며 기존 로그인 응답·URL·profile·종료 검증을 유지한다. 진단은 공급자를 실행하지 않지만 고정 `/usr/bin/codesign` 검증 프로세스는 실행한다. Claude와 실제 모델 실행의 기존 admission은 별도이며, 업데이트 전후 실제 계정 인증 회귀까지 입증한 것은 아니다. 아래 과거 pinned 로그인 설명은 이 Codex 경로에 한해 갱신한다.

> 내부 반영 백업 용량 제한: `application-backups-v1`의 모든 일반 파일(고아·abandoned 포함)을 합산해 기본 5GiB 내에서만 새 archive를 작성한다. 동일 directory inode의 비차단 flock을 census부터 작성/실패 정리까지 유지해 협력하는 작성자를 직렬화하며 남은 용량을 매 write에 적용한다. 기존 복원 파일은 자동 삭제하지 않는다. 링크·특수 객체는 따라가지 않고 거절한다. 이 경계는 내부 준비 경로이며 제품 원본 writer/복원 UI, 사용자 용량 변경·명시 정리 UI는 아직 없다. 준비 중 용량 부족도 기존 preparing 복구 상태를 유지할 수 있고 원본에는 쓰지 않는다.

> 오프라인 도구 격리 실측(2026-09-30): 사용자 승인된 설치 Codex/임시 profile·가짜 sentinel 시험에서 named permission 설정 조회까지 도달했으나 command/exec가 exit71, `sandbox-exec: sandbox_apply: Operation not permitted`로 실패했다. 현재 외부 deny-default 정책 아래 vendor 내부 sandbox 적용이 거절된 관측이며 인증 파일 차단 성공이나 macOS의 모든 중첩 정책 불가능성을 입증한 것은 아니다. 시험의 inline TOML 전달/JSONL 여러 줄 수신 오류는 수정했고 제품 정책·실제 실행 활성화는 변경하지 않았다. 다음은 공식 실행 경계와 격리 책임을 함께 재설계·검증하는 것이다.

> 연결 설정의 점진적 표시: 공급자 선택/추가 영역은 처음에 펼쳐지고 선택을 저장하면 접힌 요약을 남기며 사용자가 다시 펼칠 수 있다. 선택 저장 실패는 접힌 상태에서도 표시한다. 공급자별 연결 컴포넌트는 이 영역 밖에서 계속 mount/polling한다. 로그인 상태가 오류 없는 connected이면 상세 조작을 접고 연결 요약과 `연결 관리`를 남긴다. 펼친 상태는 반복 status 조회에서 유지하고 새 connected 전환에서 다시 접는다. 진행 중·미연결·복구 필요·조회 오류의 연결 상세는 접을 수 없다. 이는 표시 정책일 뿐 실제 모델 실행 가능을 뜻하지 않는다.

> 실제 실행 capability의 미관측 상태: 비활성 제품 adapter는 설치를 조사하지 않았으므로 `installationState: unknown`, `authState: unknown`, 빈 모델 목록·실행 파일 없음과 `canStart: false`를 반환한다. macOS 사유는 `capabilityUnknown`, 비지원 플랫폼은 `platformUnsupported`다. 로그인 사전 진단 성공으로 실행 capability를 승격하지 않는다. unknown은 공개 capability snapshot에만 추가한 값이며 실제 G5C 공급자 관측의 installation enum은 변경하지 않는다. Rust/TypeScript는 unknown의 인증·discovery 증거 주장을 거부하고 기존 fingerprint 검증을 유지한다. sidecar/renderer는 함께 배포하며 구형 renderer가 새 값을 거부하는 경우 fail-closed한다.

> 처음 시작하기의 에이전트 선택: 저장된 유효 선택이 없으면 Codex/Claude Code를 명시적으로 고르기 전 공급자 진단·계정 확인을 호출하지 않는다. 하나만 선택해도 연결 설정을 진행하며 다른 공급자는 나중에 추가할 수 있다. `hello-agent.m5.providers.v1`은 공급자 이름만 저장하는 UI 선호값이고 인증/실행 권한·프로젝트 역할 snapshot을 대체하지 않는다. 저장 실패는 현재 선택을 유지하되 다음 실행에 재선택이 필요함을 알린다. 선택 추가는 기존 연결 컴포넌트를 재생성하지 않으며 진행 중 연결을 숨기는 해제 기능은 이번에 추가하지 않았다.

> 로그인 사전 진단: `provider.login`의 고정 `diagnose` 요청은 기존 로그인과 같은 pinned 실행 파일 후보만 검사한다. 인증/프로필 생성·CLI 실행·설치·네트워크 요청 없이 `diagnosing`→`installed` 또는 실패를 반환하며 renderer는 성공 뒤 기존 계정 확인을 별도로 수행한다. `installed`는 지원하는 로그인 프로그램의 현재 파일 검사 결과일 뿐 인증·모델 실행 가능 증거가 아니다. 진단은 진행 중 worker와 복구 필요 상태를 덮어쓰지 못한다. Claude 설치 디렉터리 부재와 미지원/변조 후보를 구분한다. 명시 설치·업데이트와 읽기 전용 기기 정보는 별도 경로로 연결되며 이 진단이 자동 설치를 시작하지 않는다.

> 작업대 언어: 기존 I18nProvider/hello-agent.lang 설정으로 앱의 프로젝트·로그인·역할/모델·채팅·복구·권한 요청·파일/검색/diff·편집·패널 배치 안내를 ko/en 전환한다. 대화/코드 원문·경로·모델 ID·기술 상세는 번역하지 않는다. Monaco 자체 내장 메뉴 전체의 번역은 이 범위에 포함되지 않는다.
>
> 언어 전환은 controller/Monaco를 재생성하지 않으며 입력·미저장 초안·확인창·배치를 유지한다. 앱이 등록한 Monaco 저장 명령만 폐기/재등록하고 접근성 이름을 갱신한다. 표시 갱신 실패도 초안을 보존한 명시 재시도 화면으로 전환하며 자동 저장/재시도하지 않는다.
>
> controller 오류/notice는 기존 한국어 문자열·기술 상세와 typed 표시 키를 함께 보관한다. 새 notice/null은 이전 표시 메타데이터를 제거한다. 채팅 notice는 별도 상태 영역에 보이며 역할/공급자 조합은 불변 context로 구성한다. 네이티브 복구 확인은 선택 언어를 받고 기존 확인/거절 조건은 유지한다.

> 편집기 초기화 복구: 로컬 Monaco 로딩/생성 실패는 자동 반복하지 않고 현재 초안의 읽기 전용 복사 화면과 명시 재시도를 제공한다. 실패한 loader cache와 부분 생성 자원을 정리하고 재시도 때 최신 초안·읽기 전용 상태를 사용한다. 앱 재시작까지 초안을 영속 보존한다는 의미는 아니다.

> 편집 되돌리기(2026-11-02): 마지막 확인된 파일 버전과 UTF-8 내용이 같아지면 dirty 표시를 해제한다. 원문 사본은 추가 보관하지 않고 길이/허용 여부만 저장하며, 변경 후보는 경로만 대기열에 넣어 Web Crypto 비교를 한 번에 하나씩 수행한다(입력 최대5MiB, 인코딩/엔진 복사 비용은 존재). 결과는 같은 프로젝트 세대·정확히 같은 탭 객체에만 적용한다. saving/conflicted는 이 비교로 풀지 않고, 불확실한 저장 실패 뒤에는 명시적 재읽기나 성공한 저장까지 비교로 clean 처리하지 않는다. controller 회귀·독립 검토와 2faed62 QA의 실제 Undo 복원을 확인했다. 같은 시험의 입력 지연·main trace 수집 실패는 별도로 남아 있다.


> 편집기 언어: 로컬 Monaco tokenizer로 TS/JS·Markdown·Python·Rust·Go·Shell·YAML·CSS·HTML·SQL·Dockerfile을 지원하고 JSON은 별도 로컬 worker를 선택한다. JSON 원격 schema 요청은 고정 Monaco 0.52 기본값(false)을 유지한다. 미지원 파일은 plaintext이며 외부 언어 서버/실행기 지원을 뜻하지 않는다.

> 편집기 저장: Monaco의 Cmd/Ctrl+S 및 context menu는 현재 editable dirty tab만 기존 controller.saveActive로 전달한다. 최신 active path/storage admission과 controller의 작업 차단/충돌 검증을 유지하며 editor 밖 전역 키를 가로채지 않는다. 원본 적용이 아니라 작업 복사본 저장이다.

> pending 원본 관측(내부): `intent::observe_pending_source`는 결속 archive/pending 검증 후 저장 canonical root·Git source identity/HEAD/tree를 대조하고 기존 두-pass no-follow raw inventory와 root 재열기 stamp를 확인한다. 부모가 현재 real directory인 항목만 before 전체 metadata/inode와 `matchesBeforeEntry`, 또는 정규화 after와 `matchesAfterImage`로 비교하며 나머지는 conflict다. root inode를 포함한 현재 관측만 반환하며 파일/DB/잠금은 변경하지 않는다. after 일치는 앱 작성 소유권 증명이 아니고 before 일치도 중간에 외부 변경이 전혀 없었다는 뜻은 아니다. 과거 root inode 보존·동일 UID 경합 보호·안전한 자동 재시도/복원 권위를 제공하지 않는다. 기존 inventory 한도와 전체 archive 재검증 비용을 유지한다.

> 첫 파일 pending 기록(내부): `intent::record_first_item`은 현재 applying의 결속 archive 첫 항목을 검증한 뒤 `pendingItem {index:0,item}`을 project/lock revision CAS로 저장한다. pending이 있거나 이전 intent를 재전송하면 거절한다. `inspect_pending_item`은 재시작 후에도 실제 archive와 같은 metadata인지 대조하며 기록을 완료/재시도 권위로 해석하지 않는다. 아직 완료 cursor·다음 항목 전진·writer 소유 신원/사후 증거·역복원은 없으므로 첫 항목 기록 이후 자동 실행/잠금 해제는 없다. 원본 쓰기/제품 API는 비활성이다.

> 적용 항목 읽기(내부): `intent::inspect_write_item`은 현재 applying intent/lock·백업 원장의 exact ready 기록에 결속된 archive에서 지정 순번의 path/before/after metadata를 읽는다. 기존 전체 archive hash·canonical decoder·payload 및 변경 digest·EOF·전후 파일 신원 검증을 재사용하고 모두 통과하기 전에는 항목을 반환하지 않는다. metadata 한 프레임(기존64KiB 제한)만 추가 보존하며 payload를 메모리에 모으지 않는다. 호출마다 전체 archive를 읽으므로 전체 적용 루프의 항목별 반복 호출용 성능 경로는 아니며, 향후 순차 writer/journal과 구분한다. 이 읽기는 현재 원본의 before/after 분류나 복원·쓰기 권위가 아니다.

> 쓰기 시작 영속 경계(내부): `intent::mark_write_started`는 exact prepared 기록과 lock을 대조하고 결속된 ready 백업을 실제 archive까지 재검증한 뒤 project `applying`과 lock `applicationWriting`을 같은 CAS transaction으로 게시한다. 중간 DB 실패는 prepared를 보존하고, 성공 뒤에는 재시작해도 recovery 잠금을 유지한다. 재전송은 쓰기 재개 권위가 아니므로 거절하며 applying에 대해 준비 취소도 거절한다. 이는 첫 파일 변경 전에 필요한 영속 fence일 뿐 파일별 journal·현재 원본/작업 복사본 재검증·외부 작성자 경합 보호·source writer나 restore의 구현은 아니다. 제품 caller는 없다.

> 적용 입장 직렬화 전제: fixture 시작과 native `RunPreparation::reserve`는 단일 runtime의 `&mut Storage`를 동기적으로 보유한 동안 project intent를 재검사하고 run을 기록한다. 그 사이 scheduler yield/외부 callback은 없고 SQLite EXCLUSIVE 연결이 별도 연결의 쓰기를 거절한다. 사전 resolve 결과만으로 예약하지 않는다. 따라서 현재 앱 내부에서 intent 게시가 검사와 예약 사이에 끼어들 수 없으며, 다중 DB writer나 비동기 예약으로 바꿀 때에는 project revision을 포함한 atomic admission으로 다시 설계해야 한다. 이는 파일 시스템의 외부 작성자 배제를 뜻하지 않는다.

> 적용 전 원자 잠금(내부): `review/intent.rs::begin`은 세션의 PreparedApplication을 소비하고 project.application_intent(prepared)와 해당 workspace lock(recovery_required=true, applicationPrepared evidence)을 project/lock/chat revision CAS로 함께 저장한다. 기존 blocker 경로가 새 snapshot 생성을 막고, fixture/native 실행 공통 검증은 해당 project의 진행 중 intent가 있으면 다른 chat의 실행도 예약 전에 거절한다. 기존 준비된 run도 reserve/release 시 재검사한다. 다른 프로젝트의 실행이나 모든 snapshot 편집을 전역 차단한다고 주장하지 않는다. 이는 앱 내부 배타 경계이며 다른 snapshot의 모든 편집이나 동일 UID 외부 작성자를 막는 OS 잠금은 아니다. 재시작 inspect는 exact intent/lock 신원·phase·경로·flags를 확인한다. `abandon_unwritten`은 prepared에서만 기록 종료와 같은 잠금 해제를 원자 수행하며 파일은 변경하지 않는다. 실제 writer는 첫 쓰기 전에 영속 phase를 반드시 변경해야 하며 현재 source writer/쓰기 직전 OS 경합 보호·restore/UI는 아직 없다.

> 확인→백업 준비 결속(내부): `Confirmations::prepare_backup`은 일회용 token을 소비하고 exact 검토를 다시 연 뒤 새 백업을 준비하거나 ready 백업을 재검증해 재사용한다. preparing의 자동 복구는 하지 않는다. 결과는 serialize/clone 불가능한 `PreparedApplication`으로 동일 owner generation과 최초 만료 시각에 묶인다. cancel·새 open(실패 포함)·새 owner는 무효이며 consume은 generation을 한 번 소비하고 current review와 ready archive/record를 다시 검사한다. 긴 백업/최종 검증 후에도 만료를 검사한다. 반환은 적용 transaction의 입력 후보이며 project-wide 배타 잠금·적용 intent·쓰기 직전 검증을 대신하지 않는다. 공개 UI/IPC 연결과 source writer는 남는다.

> 미완료 백업 명시 복구(내부): `staging::resume`은 preparing의 예약된 이름/header와 현재 검토·원본 baseline을 다시 대조한다. 완성 파일의 각 before mode/identity까지 baseline과 일치해야 새 descriptor를 계산해 ready를 CAS 게시한다. Git 정규화 검토 digest만으로 전체 복원 권한을 추론하지 않는다. missing/partial/stale은 거절하며 inspect는 여전히 자동 승격하지 않는다. `abandon`은 exact preparing 기록만 abandoned로 CAS 전환하고 파일·원본·잠금을 건드리지 않으며 같은 요청의 응답 유실 재전송을 허용한다. 새 검토/새 백업은 가능하지만 이전 파일과 기록은 보존한다. 디스크 회수·보존 정책과 공개 복구 UI는 아직 없다.

> 백업 준비 원장(내부): `backup/staging.rs`는 archive 생성 전에 project의 `application_backups[reviewId]`에 exact 파일명·검토 header·preparing을 project/lock/chat revision CAS로 저장한다. 검증 완료 후 같은 준비 기록과 최신 검토를 대조해 ready 및 descriptor를 게시한다. 미완료가 있으면 다음 준비를 거절하고 metadata 합계256KiB를 제한한다. 재시작 inspect는 preparing을 파일 존재만으로 승격하지 않으며 ready의 strict descriptor/decoded header를 원장과 대조한다. 프로젝트/채팅 lifecycle은 이 extension을 보존한다. 이는 백업 준비만의 원장이며 source-writer 잠금이나 적용 intent가 아니다. 명시 파일 회수·원장별 보존 용량 정책·confirmation 연결과 source writer는 남는다.

> 반영 전 백업 후보(내부): `review/backup.rs`는 최신 검토와 baseline을 재검증하고 변경 대상의 이전 bytes·mode·종류·존재 여부 및 after image를 앱 전용 `application-backups-v1/backup_<uuid>.bin`에 exclusive 0600으로 스트리밍한다. 링크는 대상 문자열만 보존하고 각 부모는 no-follow로 연다. version2 JSON header/item과 길이가 정해진 raw payload로 구성하며 상한은4GiB+32MiB, payload buffer는64KiB다. file/directory fsync·전체 hash·최종 source/review 대조 뒤 반환한다. `backup/decode.rs`는 canonical JSON·정확한 key/항목수·정렬/안전 경로·kind/mode·내용 길이/hash·기존 검토 digest를 재계산한다. metadata32MiB·한 줄64KiB·항목100,000·경로8MiB 한도를 적용한다. 정상 오류 시 이번 파일 신원을 확인하여 제거/fsync하며 정리 실패는 이름을 포함한 recovery 오류다. strict descriptor 직렬화와 staging의 사전 예약으로 중단된 파일 이름을 보존한다. 파일 회수·적용 intent·확인 ticket 결속과 실제 restore writer는 아직 없으므로 공개 복원 지점으로 사용하지 않는다.

> 검토 확인 수명(내부): `review/confirmation.rs`는 세션 메모리에 한 개의 무작위 ticket만 유지한다. 검토 재열기(실패 포함)·취소·owner 재생성으로 무효화하며 5분이 지나면 거절한다. 확인 시 성공/실패 모두 ticket을 소비하고 exact 저장 descriptor와 현재 파일을 다시 대조한다. 이 반환 plan은 원본 쓰기 권한이 아니다. 적용 transaction/복원 지점·쓰기 직전 재검증과 공개 IPC/UI 연결은 아직 남는다. 아래의 미연결 ticket은 제품 연결을 뜻한다.

> 채팅별 검토 대상(내부): `ReviewTarget::Chat`은 프로젝트의 현재 active 포인터 대신 해당 채팅의 manifest/marker로 exact workspace를 검증한다. chat/task ID·상태·revision과 건강한 미소유 lock의 ID/실제 경로/revision을 binding에 포함한다. `application_chat_reviews[chatId]`에 각 최신 descriptor를 보존하고 project+chat+lock revision을 같은 transaction에서 검사한다. 다른 채팅의 새 검토는 기존 채팅 검토를 덮지 않으며 후속 대화·잠금 변경은 재검토를 요구한다. 명시 확인 ticket·승인/원본 쓰기는 아직 연결하지 않았다.

> 변경 검토 기반(내부): `application/review.rs`가 원장 receipt·현재 원본·현재 snapshot을 재검증하여 정렬된 before/after 변경 집합과 bounded digest를 만든다. untracked 충돌·case/정규화 충돌·미지원 종류/권한을 거절하고 확인 후 원본/복사본의 내용·신원 변화도 거절한다. `review/persistence.rs`는 프로젝트의 최신 prepared descriptor를 revision CAS로 원장에 게시하고 재열기 때 현재 파일에서 plan을 재구성·대조한다. descriptor에는 binding·변경 digest·root/파일 신원 포함 snapshot digest·count가 있고 승인 상태는 없다. 원본을 쓰거나 삭제하지 않으며 chat/task 승인·공개 IPC/UI·writer는 아직 없다. 상세 지원/한도는 `proposals/m5-change-application.md` A2를 따른다.

> 원본 기준 생성 결속(내부): Unix `SnapshotCreation::ApplicationCandidate`는 공통 snapshot 생성 경로에서 launch fence → clean baseline → bundle export → baseline 재대조 → 불변 receipt/원장 게시 → 기존 활성화를 수행한다. 활성화 직전 최신 project 원장을 읽어 새/과거 receipt를 보존한다. 3개 복사본 생성·DB 재열기·각 receipt 검증과 dirty/게시 실패/rename 후 실패 잠금 유지 회귀를 통과했다. 공개 IPC caller는 없고 기존 일반/채팅 생성은 그대로다. 시작 이후 프로세스 복구·원장 누적 용량/압축·지원 불가 원본의 제품 안내 및 원본 writer는 미완료다.

> 준비 복구 공개 연결: `project.catalog`의 `inspectPreparation`/`abandonPreparation`과 main/preload의 기존 exact validator를 통해 저장 프로젝트별 명시 확인·종료를 제공한다. 원본 경로나 patch를 입력받지 않고 inspection의 project/worktree/workspace/lock 및 두 revision을 그대로 대조한다. 응답 유실 재전송은 동일 project가 abandoned이고 동일 lock의 완료 evidence가 exact inspection일 때만 no-op 성공한다. 새 snapshot으로 전환된 뒤 옛 요청은 거절한다. 아래의 공개 연결 미완료 표기는 이 구현으로 갱신하며, legacy/실행 시작 이후의 준비 복구는 여전히 지원하지 않는다.

> 2026-09-30 준비 복구: `project/snapshot_recovery.rs`는 version1 `beforeSnapshotExport` 준비만 내부 inspect/abandon으로 처리한다. project/lock 준비를 원자 저장하고 export 직전 `snapshotExportStarting`을 영속화한다. exact 신원·managed 파일 부재·chat/run 미결속 검사 뒤 Immediate 트랜잭션에서 두 revision을 다시 대조하여 project 폐기와 잠금 해제를 원자 저장한다. 파일은 삭제하지 않으며 legacy/이미 시작된 기록은 거절한다. 공개 IPC/UI와 응답 유실 처리는 아직 연결되지 않았다.

> 대상 독자: 이 프로젝트에 기여하는 개발자. 제품 배경·시장 조사는 기획 문서(Obsidian) 참고.
> 다이어그램: [시스템 구성](diagrams/architecture.excalidraw) · [위저드 플로우](diagrams/wizard-flow.excalidraw)

> 내부 실행 연결: `RunReservation::launch_proxied_codex_candidate`는 예약 역할/스냅샷에서만 실행 설정을 유도하며 원장에 process identity를 저장한 뒤 opaque proxy command의 gate를 해제한다. 해제 후 원장 저장 실패는 transport/broker를 중단하고 cleanup owner를 유지한다. 기존 offline candidate는 그대로 유지되며 어느 경로도 제품 실행 입장 권한을 제공하지 않는다.

> 2026-09-29 진단 상태: 내부 `inspect_proxied_preflight_candidate`의 실제 설치 CLI 격리 조회는 고정 OS CA 파일을 명시한 뒤 5 RPC·종료/cleanup을 통과했다. 모델 중계 경로만 root 소유·일반 파일·그룹/타인 쓰기 불가·1MiB 이하의 `/private/etc/ssl/cert.pem`을 확인하고 exact read 허용 및 `CODEX_CA_CERTIFICATE`로 전달한다. TLS 검증 비활성화나 키체인/Mach 접근 추가는 없다. 사용자 키체인의 별도 사설 CA 지원은 이 증거에 포함하지 않는다. WorkspaceAccess::ReadOnly는 workspace 쓰기 제한이지 profile 인증/캐시 불변 보장이 아니다. 실제 유료 turn·TLS/domain-fronting 경계·제품 활성화는 별도 미완료다.

## 1. 목표와 비목표

**목표**: 터미널을 열어본 적 없는 사용자가 앱 더블클릭 → 안내를 따라가기만 하면 Claude Code 또는 Codex가 동작하는 상태(첫 대화 성공)까지 도달하고, 이후 프로젝트와 에이전트 상태를 한곳에서 관리한다.

**비목표 (MVP 기준)**:
- ~~크로스 에이전트 지원 — 2단계~~ → **2026-07-16 스코프 변경**: Codex를 조기 편입 (위저드 0단계에서 에이전트 선택). Gemini CLI 등 추가 확장은 여전히 2단계
- ~~영어 UI — 2단계~~ → 한국어·영어 전환 구현 완료
- 임베디드 터미널 — 터미널 노출 0이 원칙, 닥터 기능도 해석된 한국어로만 표시
- Linux 지원

## 2. 시스템 구성

현재 배포 앱은 Tauri이고, M5 전환 골격은 같은 저장소에서 별도 Electron 진입점과 공용 Rust 안전 코어로 구성된다. 아직 Electron이 기본 실행·배포 경로를 대체하지는 않았다.

```
[React 위저드 UI (WebView)] ←invoke/이벤트→ [Tauri Core (Rust)] → [OS 셸·파일시스템·PATH]
                                    ↑
                          [설치 레시피 (JSON 데이터)]

[M5 React + Monaco] → [typed preload] → [Electron main] ⇄ JSONL v1 ⇄ [Rust sidecar]
       app://m5              sandboxed        │                              ├─ SQLite v1
                                               └─ 앱 수명주기                 ├─ standalone snapshot
                                                                              └─ hello-agent-core
```

| 레이어 | 역할 | 기술 |
|---|---|---|
| 위저드 UI | 단계 표시, 쉬운 한국어 안내, 선택 분기 | React 19 + TS, 상태 머신 |
| Tauri Core | 환경 감지, 명령 실행, PATH 조작, 진행 이벤트 발행 | Rust + Tauri 2 플러그인(shell, os, fs) |
| 레시피 | "무엇을 어떻게 설치·검증하는가"를 코드가 아닌 데이터로 정의 | JSON (앱 동봉, 추후 원격 갱신) |

**Rust 최소화 원칙**: 설치·감지 로직의 오케스트레이션은 프론트(TS)에서 레시피를 해석하며 진행하고, Rust는 Tauri 플러그인이 못 하는 것(레지스트리 조작, PATH 파일 수정 등)에만 커스텀 커맨드를 만든다. M0에서 이 경계를 확정한다.

## 3. 앱 구조: 홈베이스 + 위저드 상태 머신

**"일회성 도구" 탈출 (2026-07-22 도입)**: 앱은 단일 선형 위저드가 아니라 **뷰가 여럿인 홈베이스**다. 편집기가 못 하는 것(고장 시 재진단, 안전한 새 프로젝트 시작, 업데이트 알림)을 제공해 재방문 이유를 만든다.

```
앱 실행
  ├─ 프로젝트 발견 또는 준비된 에이전트 있음 → 홈
  │                                           ├─ 프로젝트 카드 → 폴더 열기
  │                                           ├─ "새 프로젝트" → 준비된 단계 건너뛰기
  │                                           └─ 상태·업데이트·스캔 실패 → 인라인 닥터
  ├─ 기준 폴더 스캔 실패 → 홈에서 원인·재시도·다른 폴더 선택
  └─ 프로젝트와 준비된 에이전트 모두 없음 → 위저드(첫 온보딩)
```

최상위 뷰 라우팅: `home | wizard | ...`. `App.tsx`가 `view` 상태로 분기하고, 위저드는 지금의 6단계 컴포넌트를 그대로 재사용한다.

### 위저드 상태 머신

6단계 선형 흐름(에이전트 선택 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식) + 실패 시 닥터 분기. 각 단계는 `idle → running → success | failed`.

```
에이전트 → 진단 → 설치 → 로그인 → 첫 프로젝트 → 졸업식
   └─(실패)→ 닥터(kind별 원인·해결) → 해당 단계 재시도
```

**재방문 단축**: 설치·로그인된 에이전트가 하나면 "새 프로젝트"에서 바로 프로젝트 단계로 직행한다. 둘 다 준비됐으면 에이전트만 고른 뒤 프로젝트 단계로 이동한다.

### 영속성 (홈베이스의 전제)

프로젝트 목록의 진실은 사용자가 고른 기준 폴더의 **실제 디스크 상태**다. `scan_projects`가 바로 아래 하위 폴더의 `.claude`/`CLAUDE.md`/`AGENTS.md` 표식을 찾는다. `tauri-plugin-store`는 다음 보조 정보만 앱 설정 디렉터리 JSON에 저장한다.

- `baseDir`: 프로젝트를 찾을 기준 폴더
- `projects[]`: 경로별 생성·최근 사용 시각 — 스캔 결과 정렬용이며 목록 원본이 아님
- 언어 선택은 `localStorage`의 `hello-agent.lang`에 저장

에이전트 설치·로그인 상태는 저장하지 않고 **매번 실시간 감지**한다(사용자가 앱 밖에서 바꿀 수 있으므로 — 진실은 항상 시스템).

- **진단**: OS/아키텍처, Node 유무(참고용), 기존 Claude Code 설치·버전, PATH 상태 감지. 이미 설치된 사용자는 설치 단계를 건너뛴다.
- **설치**: 네이티브 인스톨러 실행(macOS: `curl | sh` 스크립트, Windows: PowerShell 스크립트)을 앱이 대행. 출력을 스트리밍으로 받아 진행률·상태 문구로 변환해 표시.
- **로그인**: `claude auth login --claudeai`를 백그라운드 실행 → CLI가 브라우저를 열고 대비용 URL도 출력 → 사용자가 브라우저 로그인 후 받은 확인 코드를 앱 입력창에 붙여넣으면 stdin으로 전달 → `claude auth status --json`으로 완료 확인 (M0에서 검증됨, §7).
- **첫 프로젝트**: `~/Documents/내-첫-프로젝트` 등 안전한 위치에 폴더 생성 → 안전 프리셋 적용 → 에이전트 실행 확인.
- **졸업식**: 첫 대화 1회 성공을 확인시키고 "이제 혼자 쓸 수 있다" 상태로 종료.

각 단계의 실행 내용은 레시피가 정의하므로, UI는 레시피 스텝의 종류(check/run/verify/choice)만 알면 된다.

## 4. 레시피 스키마 (초안)

설치 방식 변경(리스크: 벤더의 잦은 변경)에 대응하기 위한 구조.

**현 구현 (2026-07-16)**: `src-tauri/src/agent.rs`의 타입化 레시피 — 에이전트별 설치 방법(클로드: 공식 스크립트 / 코덱스: GitHub 릴리즈 바이너리), 감지 후보 경로, 로그인·상태·첫 대화 명령을 enum으로 정의. 새 에이전트는 이 파일 확장으로 추가한다. 아래 JSON 외부화는 원격 갱신(앱 업데이트 없이 레시피 수정)이 필요해지는 시점에 진행.

```jsonc
{
  "id": "claude-code",
  "version": 1,
  "platforms": {
    "darwin": {
      "detect": [ { "type": "command", "run": "claude --version", "parse": "semver" } ],
      "install": [
        { "type": "download-script", "url": "https://claude.ai/install.sh", "checksum": null },
        { "type": "run", "script": "install.sh", "progressPatterns": { "다운로드": "Downloading" } }
      ],
      "path": { "candidates": ["~/.local/bin"], "profile": "~/.zprofile" },
      "verify": [ { "type": "command", "run": "claude --version" } ],
      "errors": [
        { "match": "command not found", "explain": "설치는 됐지만 컴퓨터가 위치를 몰라요", "fix": "path-refresh" }
      ]
    },
    "win32": { /* PowerShell 기반 동형 구조 */ }
  }
}
```

핵심 아이디어: `errors[]`가 닥터 기능의 지식 베이스를 겸한다. 삽질 사례 분류(기획 문서 A~G)에서 수집한 에러 패턴을 여기에 축적한다.

## 5. 플랫폼별 전략

| 항목 | macOS | Windows |
|---|---|---|
| 설치 실행 | `sh` 스크립트 (사용자 권한) | PowerShell 스크립트, ExecutionPolicy 우회 필요 여부 확인 |
| PATH 반영 | `~/.zprofile`에 추가 + **앱 내 프로세스는 자체 env 갱신** (터미널 재시작 개념 자체를 없앰) | 사용자 환경변수(레지스트리 `HKCU\Environment`) + `WM_SETTINGCHANGE` 브로드캐스트 |
| 셸 선택 혼란 | 해당 없음 (zsh 고정) | 앱이 대행하므로 사용자에게 노출 안 함 |
| 검증 | `claude --version` 절대경로 실행 | 동일 |

원칙: **PATH가 안 잡혀도 앱은 절대경로로 실행할 수 있어야 한다.** PATH 설정은 "나중에 사용자가 직접 터미널을 열 때"를 위한 것이고, 위저드 진행 자체는 PATH에 의존하지 않는다.

## 6. 안전·보안 설계

- **Tauri capabilities 최소화**: shell 실행은 레시피가 정의한 명령 패턴만 허용 목록에 등록. 임의 명령 실행 API를 프론트에 노출하지 않는다.
- **안전 프리셋**: 첫 프로젝트에 적용하는 Claude Code 설정 기본값 — 자동 승인 비활성, 작업 폴더 격리(프로젝트 폴더 밖 접근 시 확인). "11GB 삭제 사건" 류의 사고 방지가 목적.
- **다운로드 무결성**: 인스톨러 스크립트는 공식 URL 고정. 체크섬 제공 시 검증 (M1 검토).
- **개인정보**: 앱은 계정 정보·토큰을 저장하지 않는다. 인증은 전적으로 Claude Code 자체 플로우에 위임.

## 7. M0에서 검증할 가정 (실패 시 설계 변경)

| # | 가정 | 실패 시 |
|---|---|---|
| 1 | Tauri shell 플러그인으로 인스톨러 스크립트를 실행하고 stdout/stderr를 실시간 스트리밍할 수 있다 | Rust 커스텀 커맨드로 대체, 그래도 안 되면 Electron 전환 검토 |
| 2 | 설치 직후 앱 프로세스에서 절대경로로 `claude --version` 실행이 된다 | PATH 반영 방식 재설계 |
| 3 | 로그인을 앱에서 트리거하고 완료를 감지할 수 있다 (`claude setup-token`의 비대화형 활용 또는 설정 파일 폴링) | "로그인은 안내만 하고 완료 확인은 검증 명령으로 대체"로 다운그레이드 |
| 4 | Windows에서 동일 흐름이 PowerShell로 재현된다 | Windows 전용 레시피 분기 확대 |

### 검증 현황 (2026-07-16)

- **가정 1 — 검증됨 (설계 일부 변경)**: shell 플러그인 대신 처음부터 Rust 커스텀 커맨드(`install.rs`)로 구현. HOME 오버라이드·스트리밍·프로파일 수정이 모두 필요해 Rust 쪽이 자연스러웠음. stdout/stderr 실시간 스트리밍(Channel) 동작 확인.
- **가정 2 — 검증됨**: 임시 HOME 격리 설치에서 `~/.local/bin/claude --version` 절대경로 실행 성공 (2.1.211).
- **추가 발견**: 공식 install.sh는 셸 프로파일이 없으면 PATH를 건드리지 않고 안내문만 출력 → PATH 반영은 앱이 직접 수행해야 함 (`ensure_path`가 `~/.zshrc`에 추가, §5 원칙대로 위저드 진행은 PATH 비의존).
- **가정 3 — 검증됨 (예상보다 단순)**: CLI 2.1.x가 `claude auth login/logout/status`를 공식 제공. `auth status --json`이 로그인 여부·이메일·요금제를 구조화해 반환하고, `auth login`은 TTY 없이도 로그인 URL을 출력하고 확인 코드를 stdin으로 받는다. setup-token·설정 파일 폴링 같은 우회가 불필요.
- **추가 발견**: macOS 자격증명은 키체인(사용자 전역)에 저장되어 HOME 격리로는 로그아웃 상태를 재현할 수 없음. 미로그인 → 로그인 완료 플로우의 최종 확인은 별도 macOS 계정 또는 VM에서 필요 (실계정 logout은 사용자 세션을 끊으므로 개발 머신에서 금지).
- **가정 4 — 검증됨 (CI, 2026-07-16)**: GitHub Actions `windows-latest` 격리 E2E에서 install.ps1 무인 실행 → `claude.exe` 절대경로 검증까지 통과. Windows 인스톨러도 macOS처럼 PATH를 등록하지 않아 `ensure_path`가 사용자 PATH(HKCU 레지스트리)에 직접 추가하며, 실제 등록 성공 확인. 함정 하나: 부모 프로세스의 `PSModulePath`가 pwsh 7 경로로 오염되면 PowerShell 5.1이 기본 cmdlet을 못 찾음 → 자식 실행 시 해당 변수 제거로 해결. Windows GUI 위저드는 실기기 수동 테스트로 확인 (2026-07-16): 설치·미로그인→로그인 플로우 동작. 이때 발견 — 브라우저에 기존 세션이 있으면 확인 코드 없이 자동 승인되는 경로가 있어, 로그인 완료를 status 폴링으로 감지하도록 수정.

## 8. M4 자동 인계 파일럿 (macOS)

홈의 `HandoffPilot`은 사용자가 명시적으로 고른 Git 프로젝트를 앱 데이터 아래의 폐기 가능한 복제본으로 준비한 뒤, 읽기 전용 계획 역할과 구현 역할을 순차 실행한다. 사용자는 목표를 한 번 입력하고 앱 안에서 진행 상태·결과·변경 파일을 확인한다.

- `handoff.rs`: 단일 실행 슬롯, 역할 간 자동 인계, 쉬운 결과·복구 상태
- `claude_pilot.rs` / `codex_pilot.rs`: 에이전트별 고정 실행 계약과 증거 보존
- `proctree.rs`: `(pid, 시작 시각)` 누적 신원으로 Claude 자손 소멸을 확인하며, 확인 불가는 실행과 복제본을 `poisoned`로 잠금
- `orchestration.rs`: 매니페스트가 있는 격리 복제본만 준비·실행·폐기

파일럿은 macOS에서만 노출한다. Windows 자손 회수는 별도 게이트이며, §7.6B 품질·비용 비교와 재작업 루프·다역할 확장은 구현되지 않았다. 상세한 결정·철회·실측 근거는 `proposals/m4-orchestration.md`에 있다.

## 9. M5 G1~G5D 전환 골격·채팅 작업대

### 현재 검증 경계 (2026-11-02)

- macOS 일반 패키지0843105에서 Codex의 자체 임시 프로젝트 파일 수정과 실행 중 취소가 성공했다. 원본 불변, 영속 결과, 프로세스 종료와 잠금 해제를 대조했다. 이 한정 시험은 모든 도구·복구·플랫폼의 출시 검증을 대체하지 않는다.
- 별도 QA 패키지eeccc82에서 네 채팅의 단일3,000출력 스트림, 대형 파일·diff 공존, 기록 탐색과 원본 불변을 확인했다. 준비와 유휴 구간을 분리한 단회 관측은 idle434.41MiB로400MiB 기준을 초과했다. peak726.56MiB와 닫기60초 후 유휴 대비 잔류증분0MiB는 해당 관측 기준 이내다. QA 비용·1Hz 표본·유휴3초라는 한계가 있으므로 전체 G6 합격이 아니다.
- 아래 초기 단계 및 §10의 미연결/미활성 서술에는 당시 구현 단계의 기록이 남아 있다. 최신 실제 실행·활성화 여부는 현재 코드와 `history.md`의 최신 기록을 우선 확인한다. 전체 누적 회귀·G6 반복/입력·paint, Windows 및 출시/서명 검증은 완료되지 않았다.


- **진행/복구 상태 안내**: WorkStatusNotice는 기존 system.status의 blockerIds를 admission 후 및 작업 오류/채팅 상태 변화 때 읽고 명시 재확인을 제공한다. run과 lock은 진행 중에도 존재하므로 종료 실패로 단정하지 않는다. 채팅 생성 전 잠금도 표시하며 도움 요청 ID는20개·각160자로 제한하고 제어문자를 가시화한다. 실패한 재조회는 이전 정보임을 표시하고 늦은 응답을 버린다. 추가 실행·잠금 해제·파일 삭제 권한은 없으며 snapshot 준비 실패 전용 폐기 API는 아직 미구현이다.

- **Unix Git 실행 시간 제한**: 프로젝트 Git stream은 `project/git_process.rs`의 nonblocking stdin/stdout/stderr와 동일 명령별120초 deadline을 사용한다. stderr tail은8KiB로 유지하고 직접 child를 reap하기 전 stderr 완료를 기다린다. 오류/Drop은 reader를 취소하고 아직 reap하지 않은 leader의 그룹·직접 child에 종료를 시도하며 direct reap 대기는2초로 제한한다. 정상 EOF/status와 시간 초과를 구분한다. 이는 Unix 파이프/exit 대기의 상한이며 새 세션으로 이탈한 자손·파이프를 닫은 자손·커널 비중단 상태까지 전체 종료를 증명하는 계약은 아니다. Windows fallback은 종전 구현이며 해당 플랫폼 검증은 별도다.

- **프로젝트 화면 닫기**: toolbar의 나가기는 project.catalog releaseView로 해당 프로젝트의 standalone/workspace watcher만 해제한 뒤 renderer 선택을 비우고 영속 snapshot·채팅·후속 메시지 초안을 보존한다. 이는 manifest를 abandoned로 바꾸는 workspace.close/폐기와 다르다. 감시 해제 실패·dirty/저장·실행·채팅 전환·복구·snapshot 생성·열린 역할 설정창은 나가기를 막는다. 다시 프로젝트와 채팅을 선택해 기존 작업을 이어간다. 닫힌 화면으로 오는 이전 응답은 generation으로 무효화한다.

- **프로젝트 역할 설정창**: 작업대의 역할·모델 버튼은 native modal dialog를 연다. 배경 작업대는 inert가 되며 기존 editor action 차단도 유지한다. Escape/돌아가기는 같은 미저장 확인을 거치고 처리 중에는 닫지 않는다. 저장 결과·오류/상세·admission 안내를 창 안에서도 표시한다. 닫은 뒤 진입 버튼으로 포커스를 돌린다. 설정 revision/프로젝트 결속과 기존 채팅의 역할 snapshot 정책은 바뀌지 않는다. 이 modal 전환의 실제 UI 회귀 검증은 아직 남아 있다.

- **프로젝트 미선택 시작 화면**: ProviderLogin의 설치·로그인과 저장 프로젝트 catalog를 유지하며 빈 dock 대신 실제 프로젝트 시작 안내/선택 버튼을 표시한다. 연결은 파일 탐색의 새 선행 조건이 아니다. 기존 Git 프로젝트만 지원한다는 제한을 명시하고 선택은 기존 picker/controller 경로를 사용한다. 프로젝트 선택 후 dock을 mount한다. 이는 시작 화면 연결이며 guided-start 전체 시안 및 실제 계정 검증 완료를 뜻하지 않는다.

- **첫 채팅 안내**: 프로젝트 선택 후 active chat이 없으면 대화 패널에 목표 예시와 현재 프로젝트의 역할 설정을 표시한다. 예시는 비어 있는 목표 입력만 채우고 포커스를 옮기며 자동 생성/실행하지 않는다. 기존 목표 초안이 있으면 예시 버튼은 비활성이다. 역할 표시는 설정값이며 로그인/모델 사용권/실행 성공의 증거가 아니다.

- **저장된 프로젝트 목록/재열기 계약**: `storage/catalog.rs`는 admission 이후 기존 projects table의 ID/경로만 keyset 페이지 조회한다. 50행·원시 UTF-8 합계 512KiB/경로 128KiB와 wire JSON 768KiB 상한이며 목록 조회는 파일 시스템 접근·활성화를 하지 않는다. `project.catalog` capability와 고정 `m5:project:catalog` IPC(`window.m5Projects.request`)의 list/reopen 요청은 양방향 검증한다. reopen은 renderer 경로가 아닌 저장 ID로 조회하고 ID hash·현재 canonical 경로·실제 Git root와 기존 recovery/active snapshot 경계를 검증한다. 작업대의 저장된 프로젝트 목록은 ID 순 페이지를 교체 표시하고 선택 시 기존 snapshot-before-tree 흐름을 사용한다. dirty/열린 채팅 및 조회 중 새 편집은 전환을 막는다. 목록 반환 자체는 파일 접근 권한이 아니다.

- **탐색과 패널 표시**: 폴더 경로 버튼으로 상위 폴더에 돌아갈 수 있다. 명시 파일/변경 열기는 controller의 panel 요청으로 해당 도킹 패널을 표시하고 기존 그룹의 탭을 활성화한다. 일반 상태 갱신은 사용자 배치를 바꾸지 않으며 같은 파일 재선택도 새 표시 요청으로 처리한다. Monaco 부모와 분할 구조는 유지한다.

- **파일 목록 가상화**: backend의 최대500개 페이지 중 renderer는 고정32px 행을 최대120개와 앞뒤 spacer로 표시한다. 아이콘/이름을 포함한 행 DOM은 최대362개이며 실제 성능 측정은 별도다. 프로젝트/사본/폴더/페이지 변경 때 범위를 초기화하고 방향키/Home/End로 페이지 내 항목에 접근한다. 서버 페이지 조회/읽기 권한/링크 차단 계약은 변경하지 않는다.

- **파일 이름/경로 검색**: 파일 패널의 검색은 기존 작업 복사본 tree API를 순차 페이지 조회해 하위 폴더까지 찾는다. symlink·파일 내용은 읽지 않고 대소문자 무관/NFC 비교를 사용한다. 100페이지·100결과·1000폴더 또는 다음 페이지 시작 전 10초 한도에 도달하면 부분 결과임을 표시한다. 취소·프로젝트/채팅 변경·복구 잠금 뒤 늦은 결과는 폐기하며 기존 폴더 탐색 상태는 유지한다. 결과 열기는 기존 검증된 파일 읽기 경로를 사용한다.

- **연결 잠금 복구**: 로그인 UI의 명시적 `provider.login`/`recover`는 종료 증거가 있는 locked-v2 pending만 처리한다. 생성 owner와 복구자는 같은 파일의 exclusive flock을 사용한다. 복구자는 고정 이름·nofollow·16KiB·canonical 기록·소유권/권한/단일 링크·inode·모든 기록 신원의 Gone·프로필/부모 capability를 검증한 뒤 해제한다. 진행 중 worker, legacy/v1/불완전 기록, native run의 terminal 보류는 거절한다. 인증 정보 삭제나 재로그인을 하지 않으며 증거 없는 기존 실제 pending은 이 경로로 복구되지 않는다. UI는 잠금 중 start/check를 비활성화하고 복구 성공을 인증 성공으로 표시하지 않는다. 복구 후 사용자가 `연결 상태 다시 확인`으로 기존 자격 증명을 검사할 수 있다. Electron Chromium에서 가짜 bridge의 실패/성공·처리 중 버튼·명시적 재확인 클릭을 검증했으며 실제 프로필 복구 E2E는 미완료다.

후속 메시지 저장 통신은 `chat.appendMessage`를 지원한다. 현행 sidecar는 기존 exact G5D 제품/agent-fixture capability 배열의 마지막에 이 capability만 추가하며, Electron client는 해당 확장을 협상한 경우에만 요청한다. 기존 baseline 상수와 검증기는 그대로이고 current 상태 검증기는 정확한 baseline+선택적 suffix만 허용한다. V7 preload의 고정 `appendChatMessage` 채널과 팀 대화 입력창이 연결됐고 V4~V6는 유지된다. 요청의 전체 채팅/작업 공간 소속·client message ID·예상 durable 순서·본문을 저장 결과와 대조하며, 동일 요청 재시도는 같은 메시지를 반환하고 이벤트를 중복 발행하지 않는다. 초안과 불확실한 요청은 controller 수명 내에서 채팅별로 보존하며 앱 재시작까지의 초안 영속 저장은 아직 없다. 명시적 무기록 순서 충돌만 기록을 새로 읽고 같은 ID의 예상 순서를 갱신한다. 저장된 메시지는 다음 실제 실행의 inputMessageIds에 포함되지만, 실제 provider 활성화는 별도 미완료이고 메시지 저장 자체는 실행/실행 중 stdin 전달을 의미하지 않는다.

Electron main은 현재 main window의 main frame과 정확한 허용 origin만 IPC에 접근시키고 popup·webview·외부 navigation을 차단한다. `app://m5` 자산은 credential/port/query/fragment 없는 authority와 renderer 내부의 기존 regular file만 허용하며 잘못된 escape·경로 이탈·Windows 절대경로도 거부한다. `02dbaeb`의 VM 실행 회귀는 실제 main의 등록 callback을 호출해 이 경계를 검증한다. 이는 Electron/OS 자체의 sandbox·CSP·fuse 강제나 실제 패키지/Windows 검증을 대신하지 않는다.

M5는 `Electron + React/TypeScript + Monaco + Rust sidecar` 골격, 언어 중립 protocol v1, SQLite 수명주기, macOS standalone snapshot 작업대, 기존 G5 fixture 실행·복구와 테스트 전용 G5A~G5C fake actual-agent의 normalized public/terminal event·자원 예산·provider 발견/실행 전 검증·재시작 안전 복구 수직 흐름까지 구현됐다. G5D는 기존 v1 원장을 보존하면서 제품 admission의 exact storage schema v10 전환, append-only source index와 bounded lifetime materialization, V9 resume witness, V10 lossless compaction writer와 ready-only cooperative scheduler까지 연결했다. 기존 Tauri 위저드와 M4 파일럿은 계속 동작하며 Electron은 아직 기본 실행·배포 경로를 대체하지 않는다.

- `crates/hello-agent-core`: Tauri/Electron을 모르는 `Agent`·`AppError`·프로세스 트리 F1~F8·Claude/Codex 실행 계약. 기존 Tauri 모듈은 좁은 re-export shim으로 이 코어를 사용한다
- **stdio 진행 공정성**: 입력이 없으면 100ms timeout에서 runtime/watch를 진행하고, 요청이 계속 오더라도 마지막 poll 이후 100ms 이상이면 응답·직접 이벤트 뒤에 한 번 진행한다. accepted/응답 선행 순서를 유지하며 요청 경로에서는 safeToQuit 이후 새 poll을 하지 않는다. 요청 handler·cleanup 자체의 비차단 보장은 별도다.
- **여러 채팅 감시 공정성**: 한 poll에서는 정렬된 감시 키의 순환 순서로 한 watcher만 처리한다. 마지막 키가 삭제돼도 다음 키로 전진하고 끝에서는 처음으로 돌아간다. 전체 중지 시 순환 커서도 비운다. 매 선택마다 기존 Git/manifest/lock/capability 검증과 스캔 경로 재결속을 수행하며 캐시로 대체하지 않는다. 감시가 많으면 개별 변경 알림 간격은 늘어나지만 한 poll에서 모든 채팅의 Git 검사를 연달아 수행하지 않는다. 단일 Git 검사의 벽시계 상한·정식 메시지 성능 통과를 보장하는 것은 아니다.
- **Unix 감시 검증 작업 분리**: 한 번에 하나의 소유된 worker가 고정 Git 읽기 명령만 실행한다. 완료 후 dispatcher가 현재 manifest/lock/채팅 결속·ActiveProject 값과 보관한 작업 폴더/.git FD의 dev/ino를 다시 대조하고, Git metadata·인덱스 검증과 스캔을 수행한다. `write-tree`는 index.lock을 만들 수 있어 worker에서 실행하지 않는다. 감시 교체/삭제의 이전 결과는 버리며 중지/종료는 취소 flag로 Git pipe/exit 대기를 끊고 worker를 join한다. 다른 파일·실행 요청은 기존 전체 동기 검증을 유지한다. 감시 결과는 실행/쓰기 권한 캐시가 아니다. metadata/스캔과 인덱스 검사는 여전히 dispatcher에서 수행되므로 전체 비차단 또는 성능 합격을 뜻하지 않는다.
- **파일 감시 분할 수집**: Unix 초기 등록과 이후 poll은 watcher별 최대128 iterator 단계씩 진행한다(무시 이름·디렉터리 종료 포함). `watching:true`는 감시 등록이며 전체 초기 수집 완료 보증이 아니다. 여러 회차의 초기 수집을 마치면 빈 paths/rescanRequired 이벤트로 기존 화면 재확인을 요구한다. 기존 기준점과의 비교는 한 스캔이 완성된 뒤에만 게시하고, 실패·10,000항목 초과·변경 경로 overflow는 재확인 알림 후 감시를 폐기한다. 닫기/중지는 진행 중 iterator도 폐기한다. 매 회차 프로젝트/채팅 검증 후 보관한 모든 디렉터리의 현재 경로와 dev/ino를 대조하며 교체는 재확인/중지한다. non-Unix는 같은 신원 검사 구현 전까지 기존 단일 호출 수집을 유지한다. 모든 watcher 합계의 시간 상한 보장은 아니다.
- **레거시 데모 응답**: `run.startFixture`의 wire 응답은 `{run,streamId}`, `run.cancel`은 `{outcome,runId,run}`이다. 취소의 run은 종료 처리 뒤 원장에서 해당 ID로 직접 읽은 기록이며 bounded chat 목록에 의존하지 않는다. Electron은 요청 chat/run과 최종 state·종료 증거를 검증하고 기존 renderer RunRecord 계약으로 반환한다. G5A/G5D agent adapter 모드의 legacy 시작은 전송 전 거절하고 normalized `run.startAgent` 경로를 사용한다.
- **취소 완료 지연 응답**: `run.cancelAgent`는 정확한 6필드 신원을 검증한 뒤 요청 ID를 run별 한 개만 보관하고 즉시 성공 응답을 만들지 않는다. runtime poll이 누적 신원을 저장하며 종료를 단계 진행하고 terminal transaction이 끝난 뒤 기존 runCancelResult를 한 번 보낸다. 해당 poll의 terminal 이벤트보다 응답을 먼저 전송한다. 상태 조회는 대기 중에도 처리하며, 중복/다른 대상 취소는 거절한다. 보류 응답이 있으면 clean shutdown을 표시하지 않는다. 현재 실행 검증 대상은 actual-agent fixture이며 native OwnedTextRun 등록과 실제 공급자 admission을 대신하지 않는다. 기존 정상 종료/오류 fallback 및 별도 prepareQuit 취소의 동기 경로는 남는다.
- `crates/hello-agent-sidecar`: 번들 SQLite의 단일 owner인 `--stdio` Rust 프로세스. G5는 프로젝트별 역할 설정을 revision CAS로 저장하고, 채팅 생성 시 역할·런타임·모델·추론 수준·권한 snapshot을 불변 기록으로 고정한다. 여러 채팅과 공개 `TeamMessage`·실행·복구 원장은 재시작 가능한 projection으로 남는다. 제품 admission은 exact predecessor schema를 mutation 전에 검증하고 v1→v10 additive migration과 V9 witness 기반 V10 plan-aware 재오픈을 수행한다. V6 append는 event·source index·projection·ready 무효화를 한 transaction으로 묶고, V7/V8 provenance, V9 immutable plan/resume witness, V10 atomic authority/delete guard를 보존한다. ready/non-quitting event loop의 cooperative scheduler는 V9/V10 materializer 한 step, persisted pending compaction, `project → chat → run → lock` keyset 후보 순으로 tick하며, exact safe prefix 1,000개일 때만 V10 writer가 source `events`와 대응 index를 bounded transaction으로 논리 삭제한다. 한 step은 100 work·primary 1 MiB·retained/dependency 8 MiB이고 별도 writer thread/connection은 없다. D1 `storage.status` wire는 기존 shape와 capability 순서 그대로 이미 연결됐고, user record와 물리 cleanup은 삭제하지 않는다
- 채팅 준비는 기존 active/bound snapshot을 재사용하지 않는다. `workspace.prepareChat`은 매번 새 `preparationId/worktreeId/workspaceIdentity`의 unbound standalone snapshot을 만들고, `chat.create`가 설정 revision·최초 사용자 메시지와 함께 한 transaction에서 exact preparation을 bind한다. 실패 정리는 그 준비의 네 ID가 모두 맞을 때만 `workspace.abandonPrepared`로 terminal 표시하며 물리 삭제하지 않는다
- 작업 복사본의 raw blob 생성과 Git 자식 종료 뒤, 아직 게시하지 않은 staging에서 고정 `update-index -q --ignore-submodules --refresh`로 stat 정보를 연결한다. 경로/add/remove/assume-unchanged 옵션 없이 기준 object ID·트리를 유지하며, 이후 기준 신원 검증과 매 요청의 새 diff는 생략하지 않는다. 원본 인덱스에는 실행하지 않는다. 이는 첫 사용자 diff에 몰리던 초기 상태 확인을 준비 단계로 옮기는 것이며 전체 복사 준비 시간을 없애는 캐시는 아니다.
- 채팅이 소유한 tree/read/save/watch/diff/close는 V3의 `chatId/projectId/worktreeId/workspaceIdentity` 네 ID를 매 요청·응답·event에서 교차 검증한다. save continuation은 같은 네 ID와 `saveId`가 모두 일치해야 하며, 현재 선택을 추론하거나 다른 채팅의 작업 복사본으로 fallback하지 않는다. G4 project-only V2 경로는 호환용으로 유지한다
- G5 legacy 실행은 macOS 결정적 fixture와 `M5BridgeV3`를 유지한다. 별도 G5A fake actual-agent 실행은 accepted response를 event보다 먼저 반환하고, full chat/task/project/worktree/workspace/run/role/fingerprint에 결속된 `status|question|final`만 `agent.publicEvent`로 발행한다. private vendor event는 sidecar에서 버리고 actual-agent raw `run.event`·`run.finished`·`chat.eventAppended` carrier는 Electron에서 fail-closed 차단한다. 두 실행 원장과 recovery namespace는 서로 섞지 않는다
- 취소는 `(pid, 시작 시각)`의 부모·자손 전체 소멸을 증명한 때만 clean이다. 불확실하면 chat/run/workspace/writer lock을 `recoveryRequired`·`poisoned`로 유지하고 `recovery.list/inspect/markResolved` 뒤 exact abandon만 허용한다. `app.prepareQuit`은 active run과 unresolved recovery를 blocker로 반환하며 안전하지 않으면 종료하지 않는다
- `crates/hello-agent-protocol`과 `src/m5/contracts/`는 같은 golden fixture로 protocol/session, SQLite, snapshot, G5 역할·채팅·실행·복구, preparation, V3 workspace 주소와 G5A/G5B actual-agent public/terminal event·자원 예산 경계를 Rust·TypeScript 양쪽에서 검증한다. `M5BridgeV5`는 V1~V4에 additive하며 범용 IPC를 열지 않는다
- `electron/`: Forge `8.0.0-alpha.10` 기반 main/preload/Vite 패키징과 main-owned sidecar supervisor. production sidecar는 `process.resourcesPath/sidecar/hello-agent-sidecar --stdio`, `shell: false`, 고정 cwd로 한 번만 실행한다. main-frame/app origin과 고정 channel을 검사하고 preload는 exact input/output validator를 통과한 V1~V5 메서드·bounded event 구독만 노출한다. exact G5A 협상 전에는 actual 메서드와 public/state listener를 활성화하지 않는다. 협상 뒤에도 full run binding·단조 상태 전이·전용 stream을 검증하고 active run 32·seen run 1,024·event ID 8,192·agent stream 1,024·protocol stream 4,096 한도를 넘으면 listener 전에 session을 종료한다
- `src/m5/`: 여러 영속 채팅, 프로젝트별 역할/모델 설정, 공개 팀 타임라인, fixture/actual 실행 중단·분리된 복구 gate와 채팅별 Monaco tree/edit/save/diff를 결합한다. 채팅 전환은 detail과 작업 복사본 bind가 모두 성공한 뒤 원자 publish하며 dirty 편집·복구·늦은 결과가 다른 채팅을 덮지 못하게 막는다. UI는 fixture 실행을 **가짜 에이전트 데모**로 명시하고 exact G5에서는 actual 동작을 unavailable로 유지한다. renderer는 normalized public/state event의 payload를 직접 화면 상태로 쓰지 않고 bounded `chat.get` 페이지를 durable sequence 끝까지 다시 읽어 `TeamMessage`와 별도 `agentRuns` 원장만 반영한다. `final` 내용 event를 process terminal 증거로 해석하지 않으며 zero-output 종료도 post-commit state notification 뒤 durable terminal 원장을 읽은 후에만 실행 잠금을 푼다
- fake actual-agent sidecar는 accepted 뒤 같은 run 전용 stream으로 `running`, public event, terminal `agent.runStateChanged`를 연속 발행한다. terminal event는 process-tree 소멸과 run/chat/lock/recovery transaction commit 뒤에만 만들며, terminal 저장 실패는 false event 없이 재시작 때 nonterminal run을 `recoveryRequired`와 poisoned lock으로 수렴시킨다. run당 public event 256·text 1 MiB·각 append의 canonical full-array prefix snapshot 누적 16 MiB를 append 전에 검사하고 초과 event는 저장·전달하지 않는다
- G5C provider core는 executable closure와 exact version/model/reasoning/permission, auth/event parser, managed cwd와 no-fallback 정책을 canonical fingerprint로 묶는다. 테스트 전용 fake Claude provider만 완전한 handshake 뒤 시작할 수 있고 Codex의 `argvOnly` 또는 불명확한 catalog는 계속 `canStart=false`다. raw stdout/stderr·reasoning·auth·prompt는 공개 원장과 renderer에 남기지 않는다
- actual recovery ID는 run 준비 시 writer lock과 함께 무작위로 예약하고 재시작 뒤에도 같은 ID를 쓴다. process identity가 없으면 `processIdentityUnavailable` unresolved recovery와 poisoned lock을 영구 보호 상태로 유지하며 UI도 폐기 RPC를 보내지 않는다. 일반 abandon은 SQLite intent → exact/idempotent manifest abandon → manifest 재검증 뒤 DB finalize 순서로 재조정 가능하고, 이전 G5A/G5B recovery는 공개 ID를 바꾸지 않고 reservation만 백필한다
- 패키지는 ASAR와 V1 Fuse 9개를 고정하고 release sidecar를 ASAR 밖 `Resources/sidecar`에 둔다. staging은 같은 Cargo build가 보고한 exact sidecar manifest/bin/non-test executable artifact 하나를 사용하므로 shared/configured target 위치를 추측하지 않으며 실패·없음·모호한 artifact를 복사 전에 거부한다. pnpm은 Forge 요구에 맞춘 hoisted `node_modules`를 쓰되 전이 Git/tarball 차단 정책은 완화하지 않는다

G5 기준선은 exact 통합 commit `014bd987464d39674a5f531765051f60e63b5ecb`에서 누적 하네스 PASS와 독립 QA `G5_READY`를 받았다. G5A fake adapter/public-event 추가와 hydration tail-race 보정 뒤 exact commit `24a8c712e6b5a57e69c9e852333c46bfab536dc7`도 `G5A_READY`를 받았다. G5B terminal notification·resource budget 통합 exact `022b79517462a1f9339c8cabb68b5fafd65eae0b`은 누적 G5 하네스와 macOS arm64 package smoke를 PASS했고, 독립 gate는 exact docs commit `3dcf75ab1c12cf4a21b05406f4fc814e42d485f7`에서 `G5B_READY`로 판정했다. G5C provider discovery·launch attestation과 restart-atomic recovery 통합 exact `42dcbfad2444eebbebaeef4e4f64f614a6de6264`도 누적 G5 하네스와 macOS arm64 package smoke를 PASS했다(`.artifacts/m5-harness/g5-run-recovery-2026-08-28T01-03-49-390Z.json`). 독립 QA는 no-fallback 발견, handshake mismatch 비공개, text/event 경계, exit-0 poison, random recovery ID·legacy namespace 분리, 재시작 saga, 이전 recovery ID 보존 백필과 identityless UI 비행동 상태를 재검증해 `G5C_QA_PASS`로 판정했다(`.artifacts/m5-qa/g5c-integration-42dcbfad2444eebbebaeef4e4f64f614a6de6264-2026-08-28T01-09-53Z.json`). 독립 gate는 exact docs commit `cdb3e1b49de5f0c6541ff007f007bcb65b7db122`에서 `G5C_READY`로 승인했다. 이 결과는 계약·SQLite·실제 sidecar stdio fixture 프로세스·Electron package를 검증하지만 **일반 `--stdio` 제품 경로의 실제 Claude Code/Codex adapter, 인증 세션, 실제 모델 출력은 검증하지 않는다**. G6 보안·패키징·성능, Windows snapshot/자손 회수, 전체 OS Quit E2E, Forge prerelease와 macOS 권한 문자열 hardening, 실제 에이전트 smoke는 후속 게이트다.

G5D R1의 `storage/reopen.rs`는 제품 시작 경로와 분리된 내부 frozen-operation 검증 세션이다. 같은 connection을 독점 차용하고 매 호출의 `BEGIN IMMEDIATE` 안에서 `data_version`을 대조한다. 호출당 최대 100개·canonical source 1 MiB·종속 UTF-8 입력 8 MiB를 decode 전에 확인하며 외부 commit·취소·오류는 volatile 진행을 폐기한다. 새 세션은 처음부터 검사하고 `FrozenOperationsVerified`는 실행 허용 증거로 소비되지 않는다. V8 schema와 기존 open/append/lifetime의 100개 초과 거부는 그대로다.

R2의 `start_compaction_closure`는 같은 세션 관리 아래 `operations → prefixes → frozen messages`를 호출당 한 단계씩 처리한다. `storage/reopen/closure.rs`는 삭제 source와 정확한 sequence의 prefix/baseline, historical frozen anchor와 최신 live anchor, 삭제된 chat introduction과 frozen/normalized message를 양방향으로 결속한다. null-or-same snapshot과 live anchor의 새 메시지를 허용하며 정상 unreferenced baseline을 거부하지 않는다. 단계별 primary 100행·canonical 합계 1 MiB와 고정 종속 조회 전체의 raw UTF-8 8 MiB를 읽기 전에 검사한다. prefix 벡터나 영속 cursor/proof는 만들지 않고 `CompactionClosureVerified`도 정보성 결과다.

R3A의 `start_ledger_projection`과 `storage/reopen/projection.rs`는 compacted V8의 R2 검증 뒤 같은 연결·변경 감지 수명에서 `ledger → app instances → projects → chats → runs → locks`를 처리한다. 정상 uncompacted/empty V8는 압축 table의 exact-empty 확인 뒤 ledger부터 시작한다. exact predecessor·최신 prefix/retained anchor·rowid 순서와 source index를 검증하고, latest source와 다섯 projection의 JSON/typed/indexed column을 양방향으로 대조한다. pending의 미생성 index는 source에서 비교 사실을 구성해 허용하지만 존재하는 index와 ready coverage는 검사한다. 여섯 단계의 primary 100행·1 MiB 및 모든 실제 종속 UTF-8 입력 8 MiB를 decode 전에 제한하고, commit 후에만 진행을 공개한다. `LedgerProjectionVerified`도 정보성이며 뒤의 R3B나 제품 admission을 대신하지 않는다.

채팅 조회와 실제 에이전트 입력 준비는 공통 `chat_message_history`로 압축 후 `chat_messages`의 보존 본문과 남은 source의 메시지를 합친다. 동일 복사본만 통합하며 chat/ID/순서/본문 충돌과 전체 `1..durable_event_sequence` 범위의 누락을 페이지 필터 전에 거절한다. 시작 admission의 digest·원장 결속 검사를 대체하지 않는다.

실행 요청의 입력 메시지 ID 상한은100개다. 화면은100개 이하를 모두 전달하고, 초과하면 처음 요청+최근99개만 전달한다는 범위와 이전 결정 누락 위험을 사용자에게 확인받는다. 취소 시 실행하지 않으며 전체 대화 원장은 줄이지 않는다. 확인 뒤에도 현재 채팅·실행/복구 차단 상태를 다시 확인한다. 자동 요약이나 무제한 문맥을 제공하는 기능은 아니다.

R3B의 `start_derived_records`와 `storage/reopen/derived/`는 같은 연결·최초 변경 감지 epoch에서 R2/R3A 뒤 다섯 파생 table을 역검증하고 ready 상태의 정방향 coverage도 확인한다. index-empty pending receipt/baseline은 raw live tuple을 먼저 찾고 없을 때만 검증된 frozen source와 결속한다. 모든 live introduction을 indexed/frozen identity와 대조하며 unindexed introduction에는 전역 raw keyset 탐색을 추가한다. historical pending workspace의 exact source와 bound chat의 최신 raw source를 검사하고 100개 초과 member도 나눠 처리한다. outer/candidate/member를 합쳐 호출당 100 work units·fresh primary 1 MiB, retained outer와 실제 읽는 UTF-8 입력을 합쳐 8 MiB로 제한하며 실패·취소·외부 commit은 모든 진행을 폐기한다. `DerivedRecordsVerified`는 정보성 결과다. exact 통합 `e5f9f01c397f3482975a231621ee5b16aa4381e8`에서 누적 13단계·R3B 21개 회귀·독립 QA와 최종 gate를 통과했다. **이 기존 경로의 개별 기록 크기 제한과 제품의 frozen operation 100개 초과 재오픈 제한은 유지한다.** 큰 기록의 내부 연결은 아래 private maintenance 경로로 분리되며 제품 integrity/FK·복구 사전 검증·startup/admission은 미완료다.

R3C-S1의 `storage/reopen/bytes.rs`는 제품 경로와 분리된 `RawColumnSession`이다. live event payload와 frozen payload/envelope의 고정 주소를 10필드 source identity·최초 byte 길이·동일 연결의 변경 감지 epoch에 결속한다. 호출당 `BEGIN IMMEDIATE` 안에서 최대 1 MiB BLOB 조각을 읽고 retained identity/UTF-8 carry와 실제 새 입력을 합쳐 8 MiB 이내로 제한한다. UTF-8 carry는 최대 3바이트이며 hash/cursor는 COMMIT 성공 뒤에만 보존하고 오류·취소·외부 commit·완료 뒤에는 재사용을 거부한다. 결과는 UTF-8/길이/SHA 정보일 뿐 JSON canonical·의미·provenance 검증이나 admission 증거가 아니다. 기능 `00f54d9`의 집중 회귀 12개·기존 snapshot 대조 1개와 독립 리뷰를 통과해 통합했다. SQLite 내부 메모리·전체 ready 시간 상한을 보장하지 않는다.

R3C-S2의 `storage/reopen/canonical.rs`는 같은 고정 주소·연결·epoch에 결속된 private canonical scanner다. 전체 source/객체/키를 보유하지 않고 canonical 문자열·깊이 127·현재 serde Number 출력 및 인접 decoded key의 엄격한 정렬을 검증한다. 긴 키는 source span을 재탐색하며 main/key 읽기를 합친 호출당 1 MiB fresh·8 MiB retained+fresh·100 work units를 지키고 COMMIT 뒤에만 진행을 보존한다. 기능 `7eeca1d`·`5c7758e`의 실제 V8 oracle 43사례와 잘못된 write 대조 3사례, scanner 10개·S1 12개·snapshot 1개 및 단일 독립 리뷰가 완료됐다. canonical 완료를 제품 실행 허가나 frozen envelope 내부 payload/provenance 증명으로 사용하지 않는다.

R3C-S3의 `storage/reopen/semantic.rs`는 live/frozen payload와 projects/chats/runs/locks의 정확한 한 행을 같은 연결·epoch에서 canonical→projection 전체 문법→의미→SQLite mirror 순서로 대조한다. 전체 JSON/키를 보관하지 않고 재탐색하며 duplicate의 마지막 값·루트 typed sequence·숫자 종류를 보존하고, 덮이는 값도 먼저 문법 검증한다. 기존 SQLite의 첫 duplicate 및 evidence 표현 비교는 typed primary-key 한 행 조회로 별도 유지한다. S1/S2의 예산·COMMIT 뒤 진행 보존·terminal 폐기 규칙은 그대로다. `5ee0726`의 S3 18개·공유 회귀 23개 및 단일 독립 리뷰가 완료됐다. 한 행 비교를 제품 실행 허가로 사용하지 않으며 SQLite 내부 메모리·전체 ready 시간도 이 결과로 보장하지 않는다.

새 private `storage/reopen/snapshot.rs`의 `ReopenMaintenanceSession`은 승인된 분할 정책 아래 전역 source/frozen envelope·5 projection·5 derived table의 양방향 검증, index 선행 catch-up, ready 전체 재검증을 같은 연결에서 수행한다. `snapshot/workspace.rs`는 길이 제한 없는 manifest 경로와 historical live/frozen 출처를 조각으로 검사하고, `snapshot/catchup.rs`는 검증한 원본에서 파생 기록만 저장한다. COMMIT 뒤에만 진행을 보존하며 member 중간 재시작은 durable offset 없이 source 처음부터 멱등 처리한다. 기존 V1의 개별 기록 1 MiB 거부와 제품 진입점은 그대로다. 기능 `5ad7162`의 focused 회귀·독립 리뷰 뒤 통합 `5658480`에서 정상 production101을 포함한 workspace Rust 529 passed/14 ignored/0 failed, exit 0을 확인했다. 실제 생산 101-workspace DB 보존본의 별도 완료는 172,685 calls/98.288초로 **3초 제품 목표를 충족하지 않는다**. 이 `ReadyVerified`는 저장 정합성 완료이지 제품 실행 허가가 아니며 startup/복구/admission·성능은 여전히 미완료다.

### 장시간 단일 실행 출력 — macOS 소스 활성화, 패키지 검증 별도

새 출력 형식은 schema V11의 `run_output_records`에 공개 항목을 순서대로 저장하고 `run_output_heads`에 검증된 요약을 유지한다. 2026-10-27 사용자 승인에 따라 macOS NativeProviders의 일반 시작은 `Storage::open_for_durable_admission`을 사용한다. 기존 원장 검증→취소 가능한 원자 이관→기존/출력 원장 합동 검증 후 쓰기를 허용한다. 실제 모델 실행을 기다리지 않고 첫 앱 시작에서 V10→V11로 이관되며, 구버전 앱에서는 이 DB를 열 수 없고 다운그레이드 경로는 없다. 기존 기록을 삭제하지 않는다. 이관 commit 전 취소는 V10으로 롤백하고 commit 뒤 취소/검증 실패는 V11을 남길 수 있지만 입장을 허용하지 않는다. 패키지 반영·실제 계정·G6 성능 검증의 완료 여부는 작업 기록에서 별도로 확인한다.

개발 빌드(debug assertions)의 명시적 `--stdio-agent-fixture long-stream` 및 `paced-long-stream` 모드도 같은 입장 경로를 사용한다. 일반 release 빌드에서는 두 장시간 fixture 인자를 DB open 이전에 거절한다. 명시적 Cargo feature `qa-output-fixtures`를 사용하는 검증용 빌드는 최적화된 코드에서도 이 두 인자를 허용하지만, 일반 `--stdio` 선택은 바꾸지 않는다. 제품 staging은 Cargo artifact의 feature 목록을 확인하여 QA feature가 켜졌거나 목록이 누락된 실행 파일을 복사 전에 거절한다. `scripts/m5-package-output-qa.mjs`는 별도 임시 QA 앱을 만들고 고정된 시험 진입점으로 네 채팅 출력 진단을 실행한다. 사용자/세션/로그 경로를 시험 root로 고정하며 제품 out을 교체하지 않는다. QA 결과는 제품과 바이너리가 다른 진단이고 반복 전체 G6 검증 완료가 아니다. blocked/Windows 및 다른 fixture 모드는 기존 입장을 유지하며 V11을 허용하지 않는다. fixture는 시험용 임시 DB에서 stdio·페이지·화면 통합을 검증하기 위한 것이며 실제 계정 호출을 하지 않는다. V11 시험 DB는 V11 입장을 허용하는 모드로 다시 열어야 한다.

- 형식 marker가 있는 실행은 legacy `public_events` 배열에 본문을 반복 저장하지 않는다. 종료 증거 이후 final 한 건만 후속 대화 문맥과 같은 transaction에 저장한다. 기존 형식의 검증·예산은 유지한다.
- V8 bridge의 `getAgentOutputHead`·`getAgentOutputPage`·`onAgentOutputAvailable`은 읽기/알림 경로다. `agent.outputAvailable`은 결속된 head만 전달하며 본문은 최대100행/512KiB 페이지로 읽는다. wire 순번과 영속 출력 순번은 다르다.
- native/fixture runtime은 변경된 head를 알리고 마지막 sealed head를 terminal 상태보다 먼저 보낸다. sealed는 프로세스 종료 확인이나 성공 판정을 대신하지 않는다. Electron은 기존 이벤트 credit/ACK를 사용하며 renderer는 bounded window와 주기적 head 재조회로 따라잡는다.
- 실행 poll의 후속 실패가 이미 만든 알림을 버리지 않도록 수집 이벤트를 먼저 전달한다. 동시에 발생한 오류는 AgentRuntime/Sidecar 경계에서 보존하고 다음 poll에 전파한다. 모델을 자동 재실행하거나 오류를 성공으로 바꾸지 않는다.

전체 native 장시간 실행, 실제 화면 자원 예산, 기존 DB 이관과 최종 안전 검토를 마쳐야 제품 활성화를 판단할 수 있다. 최신 시험 결과와 남은 게이트는 `history.md`가 기준이다.

## 10. 미해결 사항

- **승인 직전 경계 후보**: 예약 owner의 `verify_approval_boundary`가 취소·현재 불변 role/input·writer 소유권·snapshot/파일 분리·exact 격리 launch scope·앱 profile·프로세스 관측 원장을 함께 검사한다. 파일 분리 스캔을 Git 검증보다 먼저 수행한다. TextSession의 AcceptOnce 제안은 이 callback을 거쳐야 하며 이후 전송마다 재검증하도록 묶었다. 일반 text-only 경로는 거절한다. Core의 실제 AcceptOnce 직렬화는 아직 차단되어 있어 이 전제 연결을 허용 기능 완료로 보지 않는다.

- **격리 실행 출처(승인 권위 아님)**: fixed `ModelProxyCommand` builder만 생성하는 비직렬화 `RuntimeScope`가 동일 명령의 identity-barrier launch 성공 후 process owner로 이동한다. exact 작업 폴더/ReadOnly·Write를 보존하며 일반 proxy/raw launch는 이 출처 정보를 만들지 않는다. failed/finished/cleanup/no-I/O/no-child 상태에서는 일치하지 않는다. 실제 프로세스 생존·현재 역할/작업 폴더/잠금·사용자 일회 결정 검증을 대체하지 않으며 AcceptOnce와 제품 활성화는 아직 차단된다.

- **모델 중계(내부 후보)**: `provider_model_proxy`의 Broker가 실행별 Basic credential·정확한 HTTPS authority/Host·8KiB/5초 단회 CONNECT 검사와 연결 작업·Relay를 묶는다. 동시8연결·poll당4accept·서비스 총600초/128MiB payload 예산을 적용한다. DNS는 고정 목적지만 한 번 조회하고 최대16개 결과 전체의443/public IP 여부를 검사한 뒤 숫자 주소에 연결한다. 전역8개 작업자 슬롯·10초 연결 예산·시도당최대2초이며 OS DNS가 멈추면 취소 후에도 실제 작업자 종료까지 슬롯을 보유한다. 이는 DNS 스레드를 강제 종료한다는 보장이 아니다. Relay는 방향별8KiB 버퍼, 200응답 배출 후 전달, EOF half-close와 취소/오류/Drop 종료를 유지한다. Broker stop은 연결을 닫되 listener는 drop까지 보유한다. 검사 증거는 합성 DNS 목록·가짜 지연 작업자·로컬 소켓 통합뿐이며 실제 DNS/목적지 연결은 실행하지 않았다. LoginProcess 수명에는 내부 연결했지만 native CLI 호환성과 제품 admission은 아직 없다. 자손도 credential을 사용할 수 있으며 TLS peer/SNI/HTTP 경로를 검사하지 않아 공유 IP의 domain fronting까지 차단한다고 주장하지 않는다.

- **변경 검토 현행**: 텍스트 diff는 변경 전후 줄 번호·추가/삭제 색상과 기호를 표시하며 페이지당 200행으로 전체 내용을 탐색한다. 불완전/미지원 hunk는 원문 표시로 보존하고 이 파서는 파일 적용 권위로 사용하지 않는다. 기준 기록을 포함한 채팅은 별도 immutable 변경 검토와 세션 확인/취소를 제공한다. `project/application/review/intent/execution`의 반영·복원 실행, 고정 catalog IPC와 UI를 macOS에서 활성화했으며 Windows는 차단한다. 채팅에서 실행 결과의 정확한 applicationId에 연결된 복구 목록과 별도 복원 확인을 제공하고 복구 진행 중에도 조회 화면을 유지한다. 실제 합성 UI에서 원본 반영·복원 및 채팅 전환을 확인했다. 백업 삭제/기록 정리는 별도 비활성이다.

- **편집 초안 재읽기**: dirty/충돌 탭은 명시적 확인 후 저장된 파일을 다시 읽을 수 있다. 읽기 완료까지 초안을 보존하고 확인 이후 변경된 탭·프로젝트/작업 복사본·차단 상태에서는 교체하지 않는다. 충돌 초안은 복사 가능하다. 파일 쓰기나 원본 프로젝트 적용/되돌리기가 아니며 Monaco의 동일 텍스트 동기화는 dirty를 만들지 않는다.

- **저장 중 입력 보존**: editor controller는 저장 중 추가 입력을 허용하되 saving 상태로 중복 저장을 막는다. 성공 시 baseVersion을 갱신하고 이후 입력은 dirty로 남겨 다음 저장에 사용한다. 외부 watcher 충돌이 관측되면 늦은 성공보다 conflicted 상태를 우선한다.

- **패널 접기와 집중**: dock은 순서/폭 외에 숨김 상태를 저장한다. 접힌 패널은 DOM을 유지해 Monaco를 폐기하지 않으며 최소 한 패널을 표시한다. 대화 패널만 표시하다 채팅을 벗어나면 편집기를 노출한다. 전체 시안 적용·상하 분할 도킹 완료를 뜻하지 않는다.

- **모델 없는 Codex 통신 순서**: `vendor_observation/codex_rpc.rs`는 initialize/config/account/model 4개 요청의 single-flight 순서와 응답 ID를 결속하며 예상 밖 frame은 실패로 고정한다. RawValue로 중첩 중복 키를 보존해 vendor 파서에 전달한다. 알림 일반 처리·process runner·timeout·설정 격리·실제 입력 전달/실행 권위는 이 모듈의 기능이 아니다.

- **제품 로그인 연결(macOS)**: `provider_login_manager.rs` → 고정 `provider.login` → Electron main/preload → 작업대의 Codex·Claude Code 버튼을 연결한다. provider별 manager/attempt/취소를 분리하며 provider 생략은 기존 Codex 요청과 호환된다. start/check/status/cancel과 Claude 전용 submitCode만 허용하고 모델 호출은 하지 않는다. main만 provider와 일치하는 공식 HTTPS URL을 열며 renderer/영속 기록에는 URL·계정·토큰을 전달하지 않는다. `pnpm m5:dev`는 실제 debug sidecar를 사용한다. Windows는 미연결이며 이번 두-provider 변경의 독립 안전 검토는 아직 대기다.
- **Claude 수동 연결 코드**: waiting 화면의 비밀번호형 입력창에서 브라우저 반환 코드 전체를 입력한다. 고정 submitCode 요청은 현재 살아 있는 Claude attempt에만 한 번 허용하고 verifying으로 전환한다. 제한된 문자/4096바이트 검증 후 같은 worker의 stdin에 줄바꿈과 함께 전달한다. 코드가 포함된 요청·출력은 로그/DB/상태 응답에 남기지 않는다. 취소·새 시도·완료·종료 준비에서 대기 코드를 비우고, 제출 후 30초 내 CLI 종료 및 fresh auth status와 process cleanup을 확인해야 connected다. 2026-09-15 사용자가 실제 연결 성공을 보고했다. 2026-09-28 새 앱 실행에서 재로그인 없는 복원을 직접 확인했으며 독립 안전 검토는 남는다.
- **Codex 전용 환경·복원**: `provider_profile.rs`의 private profile을 HOME/CODEX_HOME으로 전달하고 상속 환경을 비운다. 서명 확인 및 SHA-256 pin으로 대조한 Codex 0.153.4, 0.154.0-alpha.6.2, 0.158.0-alpha.2.1의 exact CLI/host 쌍만 실행한다. 최신 번들은 Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex와 codex-cli/bin/codex-code-mode-host를 직접 선택하며 bin/codex shell wrapper는 실행하지 않는다. 쌍 혼합·후보 해시 자동 신뢰는 거절하고 실행 파일 변경/부재는 별도 오류로 표시한다. 원래 터미널 인증은 읽거나 복사하지 않는다. 고정 config/auth 외에 관측한 skills/tmp/.tmp, installation_id 및 고정 SQLite 이름만 소유권/종류/권한 검사 후 허용한다. 앱 진입의 check는 initialize→initialized→account/read(refreshToken:false)→cleanup만 수행하며 OAuth를 시작하지 않는다. 명시적 null 계정은 check에서만 미연결이고 브라우저 인증 후에는 실패다. 2026-09-28 최신 native pair로 실제 앱 재시작 후 재로그인 없이 연결 복원을 확인했다. 향후 미검증 업데이트는 여전히 차단한다.
- **로그인 중 종료**: Electron quit coordinator는 로그인 blocker만 남으면 최대 40회, 250ms 간격으로 종료 준비를 다시 요청한다. 두 provider cleanup과 sidecar의 clean 판정 전에는 종료하지 않는다. 다른 blocker·오류·예산 소진은 fail-closed이며 잠금을 자동 해제하거나 강제 종료하지 않는다.
- **Claude 전용 환경·상태**: `provider_claude_profile.rs`가 `claude-profile-v1`의 신원을 보존하고 `forceLoginMethod:claudeai` 설정을 대조한다. `provider_claude_login.rs`는 strict 서명 검증한 2.1.270/2.1.275의 고정 설치 버전 파일과 SHA-256을 대조해 `auth login --claudeai`/`auth status --json`만 실행한다. HOME은 기본 키체인 탐색을 위해 OS 사용자 경로를 유지하고 CLAUDE_CONFIG_DIR·cwd·tmp는 앱 전용 폴더로 고정한다. 나머지 상속 환경·자동 업데이트·불필요 통신·CLI 브라우저 실행을 제한한다. 공식 [인증 저장 규칙](https://code.claude.com/docs/en/authentication#credential-management)에 따라 별도 config 폴더는 Keychain 항목도 분리한다. 앱이 키체인이나 인증 파일 내용을 직접 읽거나 복사하지 않는다. 로그인 안내 stderr는 bounded 임시 pipe로 합치며 실제 2.1.270에서 관측한 `https://claude.com/cai/oauth/authorize`와 기존 `https://claude.ai/oauth/authorize` 경로만 main에 전달한다. CLI 종료 후 새 status의 종료 코드·claude.ai 인증 종류·firstParty·configDirectory와 cleanup을 확인해야 연결됨을 표시한다. 잘못된 HOME으로 기본 키체인을 못 찾던 문제를 수정했고 새 환경의 실제 상태 조회는 미연결로 정상 처리됐다. 2026-09-15 사용자가 Claude 인증 완료를 보고했으며 2026-09-28 2.1.275에서 재시작 복원을 직접 확인했다. [공식 CLI 명령](https://code.claude.com/docs/en/cli-reference).

- **로그인 profile 신원 재검사**: `CodexProfileBinding::from_storage`는 열린 Storage에서 부모 경로를 받아 no-follow로 app-data/profile handle을 보존한다. 실행 전 현재 dev/ino·정책을 다시 대조하고 삭제된 config/폴더를 복구하지 않는다. 로그인 owner가 사용하지만 이후 동일 UID 경합까지 막는 원자적 실행 권위는 아니다.

- **로그인 stdio**: `provider_login_io.rs`는 caller가 소유한 child의 stdin/stdout만 nonblocking으로 연결하고 줄/누적 byte·timeout·cancel을 제한한다. 불완전 frame·EOF·쓰기 실패는 연결 재사용을 막으며 원문을 공개 오류에 포함하지 않는다. 송신은 core login/connection이 생성한 opaque `LoginRequest`(초기화/초기화 알림/시작/취소/계정 조회)만 받는다. 아래 내부 lifecycle owner가 통신 대기 중 추적 callback을 제공한다. bounded poll의 None은 데이터 대기이며 EOF와 구별한다.
- **로그인 session**: `provider_login_session.rs`가 core `codex_login/connection.rs`의 단일 연결 흐름과 아래 lifecycle owner를 연결한다. 보고된 전용 home/macOS 신원 비교 뒤 initialize→initialized→login/start→브라우저 완료→fresh account/read(chatgpt)→확인된 process cleanup 순서로 처리한다. 알려진 account/updated는 검증 후 성공 권위 없이 처리하며 알림/응답 합계 256개를 넘거나 순서/ID/종류가 다르면 실패한다. 기본 RPC 30초/browser 5분 한도, 100ms 이내 caller advance, 취소 후 늦은 성공 차단을 사용한다. CredentialsObserved는 계정 관측 및 세션 종료 결과일 뿐 실제 모델 준비 판정이 아니다. 위 제품 로그인 manager가 launcher/barrier·전용 환경·브라우저·IPC/UI를 연결한다.
- **로그인 수명 owner(macOS)**: `provider_login_process.rs`는 내부 LoginProfile capability에 따라 app-data의 create-new `codex-login.pending` 또는 `claude-login.pending`을 파일/디렉터리 sync한 후 launcher를 호출한다. Child/TreeTracker/I/O를 소유하고 누적 PID+시작 시각만 bounded append/sync한다. Claude의 비JSONL 출력은 별도 bounded chunk/EOF 경로를 쓰고 deadline·취소·원문 비저장 경계는 유지한다. 정상 finish만 추적 tree 소멸·root 회수·증거 저장·자기 marker 신원 확인 뒤 자기 잠금을 제거한다. 한 provider의 종료는 다른 provider의 잠금을 해제하지 않는다. Drop·불확실 종료·기존 파일은 재시작 fence로 보존하며 자동 삭제/PID 재신호를 하지 않는다. 앱 종료는 두 manager 모두 확인하고 provider별 blocker를 반환한다. 수동 복구 UI는 미구현이다.
- **수명 owner의 확인 한계**: marker 신원 대조와 POSIX 이름 기반 삭제는 한 원자 연산이 아니다. profile binding과 동일하게 검사 뒤 hostile same-UID 경로 교체까지 막는다고 주장하지 않는다. 정상 병렬 owner는 create-new로 배제한다. 종료 미확인 시 finish 재시도/Drop은 살아 있는 프로세스 정리를 계속 시도하되 최초 불확실 상태의 fence는 자동 해제하지 않는다.
- **세션 종료와 정리 상태 분리**: 사용자에게 반환하는 세션 종료는 cleanup 완료와 별도다. cleanup_required가 남은 closed session은 후속 advance/cancel 또는 retry_cleanup으로 underlying owner의 종료를 재시도한다. 이후 프로세스가 사라져도 최초 미확인 fence는 보존하며 인증 성공을 다시 반환하지 않는다.

- **Codex 로그인 교환**: `vendor_observation/codex_login.rs`는 browser 로그인 시작 응답의 request ID와 완료 알림의 login ID를 묶는다. 제한된 HTTPS 로그인 URL만 transient 값으로 반환하며 credentials나 모델 호출은 표현하지 않는다. 완료 알림 성공은 `VerificationRequired`이고 owner의 fresh account/read가 필요하다. 취소/EOF/timeout 뒤 성공을 채택하지 않으며 실제 프로세스 취소·종료와 URL 열기·계정 재확인은 위 제품 owner/main이 맡는다.

- **Codex 공개 본문 추출(미연결)**: `vendor_observation/codex_events.rs`는 [공식 item event](https://learn.chatgpt.com/docs/app-server#events)의 agentMessage/plan delta와 completed 본문만 명시적 thread/turn에 결속해 추출한다. completed는 누적 본문의 교체값이며 비공개 추론·도구 출력·미지원 알림은 공개하지 않고 별도 처리 필요로 반환한다. 중복 필드·모델 reroute·예산 초과는 실패 후 재사용을 막는다. 아직 runtime에 연결하지 않았으며 알림 순서/terminal/권한 검증이나 프로세스 종료 증명이 아니다.

- **Codex 응답 순서 검사(미연결)**: `codex_events/stream.rs`는 위 projection과 같은 예산 안에서 turn 시작→item 시작/종류가 일치하는 delta/완료→turn 완료 순서를 검사한다. 중복·소속/종류 변경·완료 후 frame·terminal 없는 EOF는 실패한다. 정상 completed는 미완료 item이 없어야 하며 interrupted/failed는 성공과 구분한다. private/미지원 알림은 owner의 추가 처리를 요구한다. 이 검사도 실제 transport·권한·자손 종료 증명을 대신하지 않는다.

- **Codex offline 설정 대조**: `vendor_observation/codex_preflight.rs`는 config/read·완전 model/list·thread/start 응답을 명시적 tuple/정책/cwd와 비교한다. 초기 0.149.1 schema 후보에서 읽기/쓰기 역할 대조로 확장했으며 실제 runtime/capability에는 연결하지 않는다. 보고된 관리/알 수 없는 계층은 거절하지만 upstream에서 누락된 계층은 이 검사만으로 감지할 수 없다. [공식 API](https://learn.chatgpt.com/docs/app-server#api-overview)의 별도 `configRequirements/read` 및 정책 가시성 검증은 아직 연결되지 않았다. raw 입력을 반환하지 않으며 응답 일치와 실제 파일 신원·설정 격리·권한 강제는 다른 증거다. 모델 페이지 수집·Claude init은 아직 지원하지 않는다.

- **Codex 순차 preflight(제품 미연결)**: `codex_preflight/exchange.rs`는 위 검증기를 single-flight RPC에 결속해 앱 profile/macOS 초기화→설정→ChatGPT 계정→완전 모델 목록을 검사한다. `ObservationsMatched` 뒤에는 동일 연결의 명시적 read-only thread/start와 exact 응답 대조를 지원한다. 중간 실패는 영구 차단하며 raw 관측값을 저장하지 않는다. thread ID도 닫힘/실패 뒤 폐기한다. 응답 일치는 turn 전송·설정 격리·프로세스 종료 권위가 아니다.
- **설정 출처/훅 후보 검증**: 내부 preflight만 includeLayers=true로 명시 flags·정확한 profile config·빈 system layer를 각각 한 번 요구하고 미지/관리/프로젝트/비활성 layer를 거절한다. launcher의 features.hooks=false와 보고된 effective 값도 대조한다. 초기화 시 remoteControl/status/changed 하나만 opt-out하며 뜻밖의 frame을 무시하지 않는다. 빈 임시 profile의 실제 thread 생성/EOF 종료는 별도 JS 실측을 통과했지만 Rust 전체 계정/모델 preflight·신뢰 hook/MCP/plugin 격리·제품 활성화 증거는 아니다.
- **목표 입력 후보(제품 미연결)**: `codex_turn.rs`는 live ThreadMatched preflight를 소비해 고정 text turn 요청을 한 번만 구성한다. id=6 응답과 같은 thread/turn의 시작 알림을 양순서로 결속한 뒤 기존 TurnStream으로 넘긴다. 정책 override·임의 method는 없고 오류 뒤 재사용하지 않는다. 아래 내부 세션만 사용하며 제품 원장·실행 권한은 연결하지 않았다.
- **승인 요청 관측(제품 미연결)**: `codex_approval.rs`는 command/writeStdin/network/fileChange 요청을 예상 thread/turn과 대조해 일시적 문맥으로 반환한다. 알림 위장·중복·미지 권한·정책 amendment를 거절한다. TurnStream.pending_approval은 진행 중 tool item ID/종류와 command/cwd를 결속하고 기존 예산에 포함하며, 처리된 요청 ID 재사용/미해소 item 완료를 거절한다. serverRequest/resolved는 같은 thread의 알려진 pending 요청 한 번만 해소한다. 이는 명령/파일 권한·성공 증거가 아니고 사용자 UI·실행 전송은 아직 없다.
- **native 승인 대기와 거절 의도 원장(제품 미활성)**: live stream의 미응답 요청을 전체 실행 신원·dispatcher stream·실제 LoginProcess UUID·thread/turn/request/item/시각·내용 digest에 결속한 일회성 ticket으로 보관한다. 내부 조회/거절은 exact 실행 신원과 예약 입력을 검사하며 명시 거절 접수 뒤 다음 callback에서 원장 저장 후에만 Decline을 보낸다. 자동 기본 결정은 없고 취소/종료/서버 해제/연결 변경 뒤 ticket을 제거하며 재시작 복원은 없다. 기존 run projection에는 최대128개 opaque ID/digest/decline/intentRecorded만 원자 저장한다. 저장 실패는 무전송·owner 정리로 가고 terminal/recovery는 원장 기록을 보존한다. 의도는 전달/수행 성공이 아니며 raw 명령을 저장하지 않는다. `run.approval`의 list/decline은 전체 실행 신원과 opaque ID를 검사하며 queued 접수 결과를 반환한다. Electron 현재 main frame 및 양방향 TS 검증을 거쳐 채팅 카드에 대기/거절/기존 취소를 연결한다. capability는 chat.appendMessage/provider.login 뒤 exact suffix로 협상한다. pending은 미접수 live 요청의 `{approvalId,kind}` 요약이며 command/writeStdin/network/fileChange 고정 유형만 표시한다. 기본 list/decline에는 raw 명령/경로/host/reason이 없다. 사용자가 상세를 펼칠 때만 inspect가 같은 live ticket에 결속된 bounded command/cwd/reason/grantRoot/host/protocol을 일시 반환한다. 상세는 채팅/실행 원장에 저장하지 않고 React 평문으로 표시하며 C0/C1·Unicode Cf 문자는 이스케이프한다. 닫기·만료·오류·실행 신원 변경 시 상세 상태를 비운다(보안 메모리 소거 보장은 아님). AcceptOnce 권위·실제 provider 활성화는 아직 없다.
- **승인 응답 준비·내부 일회 허용 후보**: core의 prepare_approval_reply는 decline/cancel만 구성하며 AcceptOnce는 계속 거절한다. reserve_approval_response는 live 요청의 단회 순서만 예약하고 응답 bytes를 만들지 않는다. sidecar 내부 accept_native_approval_once는 같은 ticket의 명시적 상세 조회·현재 예약/격리 경계를 요구한다. 접수 후 원자 intentRecorded(acceptOnce 또는 decline) 저장, 전송 직전과 부분 전송마다 예약/역할/작업 사본/프로세스/취소 검사 후 private transport만 opaque accept frame을 만든다. 의도 저장은 전달/실행 성공이 아니고 serverRequest/resolved·item 완료를 별도 관측한다. 공개 IPC/UI에는 accept가 없으며 제품 native start도 blocked다. 기존 PROFILE 전체 읽기/쓰기 허용은 vendor 자손에게도 적용되므로 인증/profile을 도구에서 격리했다고 주장할 수 없고 실제 제품 허용 활성화 전에 해결해야 한다. 아래 이전 handler 후보의 accept 차단 설명은 일반 core/text 경로에 해당하며 이 내부 후보를 대체하지 않는다.
- **내부 text 실행 세션(제품 미활성)**: `run_text_and_close`는 임의 cwd 대신 `TextWorkspace`를 받는다. 기존 ProjectManager의 durable chat·manifest·snapshot marker·저장소·poison 상태 검증으로 경로를 얻고, process claim 전·thread 생성 전·목표 전송 전에 재검사한다. 이는 시점별 snapshot 신원 검사이지 독점 실행 잠금이나 OS 쓰기 격리가 아니다. 미사용 process claim→동일 연결 preflight/thread→typed turn 전송→공개 출력 callback→종료 관측을 연결하며 callback은 잠정 출력이다. userMessage(text만)·reasoning/추론 delta는 신원·형식·순서·예산을 검증한 뒤 Private 결과로 버리고 공개 출력에 섞지 않는다. 미지원 도구/기타 이벤트·sink 오류·취소는 종료 절차를 거치며 불확실 종료는 owner/잠금을 보존한다. 연결/thread 준비 예산은 30초이며 turn 요청 queue 생성부터 작업 전체 10분을 별도 계상한다. 도구·승인 대기와 partial write도 포함하고 활동으로 시간을 갱신하지 않는다. 원장·소유권·격리·도구 처리의 후속 구현은 아래 항목을 따르며 제품 caller/실제 공급자 활성화는 아직 미완료다.
- **Codex private 항목 시각 호환(2026-10-30)**: userMessage/reasoning은 ThreadItem 공통 선택 속성 `startedAtMs`·`completedAtMs`를 nullable int64로 받고 즉시 폐기한다(실제 CLI0.166.0-alpha.5.1 생성 스키마). 미지 필드·잘못된 타입·중복 키는 계속 거절하며 시각 값은 공개 출력이나 종료/권한 증거로 사용하지 않는다.
- **Codex moderation 알림 호환**: `turn/moderationMetadata`는 시작된 현재 thread/turn과 필수 metadata의 형식을 확인하고 Private로 폐기한다. 공급자 schema의 metadata는 임의 JSON이며 공개 출력·도구 승인·앱 자체의 moderation 판정·완료 증거로 해석하지 않는다. 일반 알림 예산과 종료 후 거절을 공유하며 미지 최상위 속성·누락·중복·다른 scope는 거절한다.
- **Codex 통합 diff 알림 호환**: `turn/diff/updated`는 시작된 현재 thread/turn과 필수 문자열 diff를 검증해 Private로 폐기한다. 공급자 diff는 파일 비교·원본 반영·승인 또는 완료 증거를 대체하지 않는다. notification 예산·종료 후 거절·미지 필드/누락/중복 거절은 유지한다.

- **내부 출력 저장 소유권(제품 미활성)**: `TextWorkspace`가 단일 mutable Storage 참조를 보유하고 검증 시 잠깐 읽기 빌림, 공개 출력 callback 시 쓰기 빌림을 순차 제공한다. 별도 DB writer나 병렬 쓰기를 추가하지 않는다. 실제 임시 snapshot/SQLite와 가짜 vendor pipe로 정상 출력 저장·sink 실패/cleanup을 확인했지만 synthetic sink 검증이지 actual-agent 공개 원장/실행 예약/복구 통합 완료는 아니다.
- **실행 준비 원장 공통 경로**: `agent_runtime/preparation.rs`가 저장된 chat/role·message ID·snapshot을 해석하고 예약 직전 재확인한다. 기존 fixture actual-agent 시작 경로가 이를 사용해 preparing run·recovery ID·쓰기 역할의 writer lock을 원자 저장한다. 중복 실행·변경된 snapshot/입력·다른 capability는 예약하지 않는다. 이는 단일 runtime writer 전제이며 자체 provider admission/OS sandbox 권위가 아니다. 아래 내부 Codex text 후보의 barrier·공개 원장·terminal과 연결했지만 실제 공급자 실행 입장/제품 caller는 미완료다.
- **원장 신원 barrier 후보**: `LoginProcess::launch_with_identity_barrier`가 기존 관측·profile fence 기록 뒤 trusted release callback에 추적 신원을 전달한다. `RunReservation::record_pre_release`는 preparing run·snapshot·writer 일치를 확인하고 그 신원을 원자 저장한다. 내부 barrier는 gate 시점의 관측일 뿐 vendor handshake/실행 권한 증거가 아니다. 가짜 gate와 임시 SQLite에서 저장 성공 후 release 및 저장 실패 시 미실행·기존 복구 경로의 신원 보존을 검증했다. 실행 중 신원 갱신·release 기록·공개 원장·terminal은 아래 내부 후보에 연결했으며 제품 launcher·running attestation은 후속이다.
- **예약 소유자의 gate 연결 후보**: `RunReservation::launch_gated`가 신원 저장→trusted gate release→release 기록 저장을 묶는다. 마지막 저장 실패는 이미 실행됐을 가능성을 유지하며 LoginProcess의 추가 통신을 차단하고 owner를 호출자에게 보존한다. 확인 가능한 프로세스 정리와 별개로 run은 preparing/복구 대상으로 남는다. release 기록 성공도 모델 작업 성공이나 running attestation이 아니다. 통합 가짜 gate 시험의 정상/전 저장 실패/후 저장 실패를 통과했으며 제품 caller는 여전히 없다.
- **실행 중 신원 갱신 후보**: `RunReservation::observe_process`는 같은 owner의 누적 추적 집합이 기존 원장 신원을 모두 포함하는지 대조하고 기존 `persist_tracker_snapshot`으로 갱신한다. 관측 실패여도 알려진 신원 저장을 시도하되 성공으로 바꾸지 않는다. 관측/저장 오류는 통신 차단과 명시적 cleanup 대상으로 처리한다. toy 자식 프로세스 추가·SQLite 갱신 실패를 통과했으며 예약 소유자의 내부 text 통신 루프에서도 호출한다.
- **text 통신 checkpoint**: 내부 text 세션은 claim 이후 preflight 송신 전/수신 후, thread/turn 전, turn 응답·출력 수신 후와 입출력 대기 중에 process callback을 제공한다. 대기 중에는 기존 프로세스 관측이 갱신되거나 실패한 때만 원장 callback을 호출한다. 같은 mutable Storage로 기록하며 callback 오류는 다음 통신/공개 출력 없이 cleanup으로 간다. 순수 workspace 형식 거절은 계속 claim 전이다. 임시 SQLite synthetic sink·지연 응답 중 child 관측/실패로 검증했다. 예약 owner callback은 아래 내부 진입점에 연결했으며 공개 actual event·terminal 연결은 미완료다.
- **예약된 목표·역할로 text 실행(제품 미활성)**: `RunReservation::run_read_only_text`는 별도 목표/모델 인자를 받지 않고 예약된 메시지·역할과 같은 프로세스 소유자의 Codex HOME으로 통신을 구성한다. 실행 전과 통신 checkpoint에서 저장된 run·역할·메시지·snapshot 결속을 확인하고 누적 프로세스 신원을 저장한다. Codex/readOnly 외 역할은 거부하며 workspaceWrite를 조용히 낮추지 않는다. 같은 메시지 ID의 반복 projection은 입력에 한 번만 포함하고 내용 충돌은 거부한다. 합성 capability·toy pipe에서 실제 전송된 목표/모델과 설정 변경 차단을 확인하는 내부 후보이며 provider admission·공개 이벤트 원장·terminal/복구 완료·제품 활성화를 대신하지 않는다.
- **text 공개 원장 연결(제품 미활성)**: 예약 후보와 기존 fixture 실행기가 `persist_public_event_record`를 공유해 run 결속·개별/누적 출력 예산·chat 순번 및 run/chat 원자 저장을 적용한다. delta는 잠정 callback으로만 보내고 completed의 전체 텍스트로 대체한다. 계획과 중간 답변은 status, 마지막 답변은 정상 turn 완료와 프로세스 cleanup 확인 뒤 한 번만 final로 저장한다. 중단/실패 outcome의 텍스트는 status이며 cleanup 불확실 또는 sink/DB 오류는 final을 남기지 않는다. 원장 저장만으로 실행 성공을 확정하지 않으며 아래 종료 증거/terminal 쓰기를 별도로 요구한다. 제품 이벤트 송신은 후속이다.
- **예약 실행 종료/복구(제품 미활성)**: `LoginProcess`가 explicit finish의 누적 PID/start 신원과 종료 확인 증거를 제공한다. 모든 추적 프로세스 종료·root 회수·profile/fence 저장을 통과해야 confirmed이며 caller가 이를 만들 수 없다. text 예약은 자기 run의 기존 신원이 모두 포함된 증거만 받아 run/chat terminal과 writer 잠금 해제/poison을 원자 기록한다. 정상·중단·실패는 succeeded/canceled/failed, 종료 불확실은 같은 예약 recovery ID의 unresolved 상태와 workspace recovery 표식을 유지한다. terminal DB 실패는 preparing을 성공으로 해제하지 않고 다음 재시작 복구로 넘긴다. 실제 DB 재오픈 뒤 정상 terminal 불변·불확실 복구 유지·terminal 저장 실패 시 기존 출력/복구 ID 보존을 toy peer로 검증한다. 이는 native provider admission·쓰기 격리·제품 실행 UI의 완료가 아니다.
- **실행 종료 시 profile fence 순서**: 일반 로그인은 기존처럼 finish에서 해제한다. 예약 text 실행은 통신 claim 전에 지연 해제를 설정하고, finish에서는 종료 확인과 fence 신원 확인만 수행한다. run/chat terminal 쓰기가 성공하고 closure가 confirmed인 경우에만 명시적으로 fence를 해제한다. terminal 저장 실패·불확실 종료·drop·일반 finish 재호출은 fence를 해제하지 않으며 DB 재오픈도 자동 삭제하지 않는다. 종료 기록 뒤 fence 해제 실패/충돌 시에도 자동 삭제나 재실행으로 우회하지 않는다.
- **예약 기반 Codex launch 후보(제품 미활성)**: `RunReservation::launch_codex_candidate`는 원장의 Codex 역할에서 model/effort/permission을 해석하고 기존 discovery 명령과 inert gate를 사용한다. profile 소유자가 HOME을 제공하며 별도 prompt/model/cwd 명령 인자는 받지 않는다. 실행 전 취소·CLI/helper pin 불일치는 spawn 전에 거절하고, 프로세스 신원을 원장에 저장한 뒤 payload release 직전에 두 pin과 취소를 재검사한다. 실패한 owner의 명시 cleanup 책임과 preparing 원장은 보존한다. 가짜 pinned CLI로 같은 원장의 입력/출력/terminal까지 연결했으나 pin은 서명/version·설정 격리·native capability 입장 증명을 대신하지 않으며 제품 caller는 여전히 없다.
- **vendor gate의 상속 fd 차단**: 기존 고정 `/bin/sh` gate가 release를 읽기 전에 `/dev/fd`의 숫자 fd를 순회해 0/1/2 외 핸들을 닫는다. 부모 핸들/flags는 변경하지 않고 Rust spawn error pipe는 셸 exec에서 이미 닫힌 뒤다. 순회/닫기 실패는 종료 125로 payload를 실행하지 않는다. protocol stdio 외 descriptor를 넘기는 용도로 이 gate를 사용하지 않는다. 이는 fd 상속 경계이며 파일 hardlink·IPC·전체 sandbox 보장은 아니다.
- **예약 launch의 snapshot 파일 입장 검사**: `project/runtime_isolation.rs`가 capability directory에서 숨김/`.git` 포함 모든 항목을 검사한다. nlink=1 regular file과 실제 directory만 허용하며 symlink·특수 파일 및 128단계/200,000항목 초과는 거절한다. native Codex spawn 전과 예약 gate 신원 저장 전에 수행하며 output polling에는 추가하지 않는다. 새 검사 함수 내부의 Git 신원 대조보다 파일 검사가 먼저다. 이는 검사 시점 snapshot의 공유 inode 거절이며 적대적 same-UID 교체에 대한 원자 보호나 profile/tmp를 포함한 전체 sandbox 정책은 아니다.
- **모델 중계 endpoint 후보(미활성)**: 기존 offline 정책과 별도로 IPv4 loopback 포트 하나만 허용하는 tcp4 정책을 적용한다. opaque ModelProxyCommand가 Broker를 기존 LoginProcess에 옮기고 인증 proxy URL은 HTTP(S)/ALL_PROXY 대소문자 환경에만 전달하며 NO_PROXY는 비운다. owner tick은 중계를 진행하고 통신 무효화·종료 시작·Drop 때 stop한다. 종료 확인까지 listener를 소유하고 불확실 Drop은 연결/작업/인증 객체를 정리한 뒤 listener descriptor만 sidecar 종료까지 보존한다(가용성 비용, crash-survival 보장 없음). 중계 만료는 통신 실패이지 프로세스 종료 증거가 아니므로 별도 cleanup 전 fence를 해제하지 않는다. 실제 native CLI가 이 환경을 따르는지와 제품 admission은 아직 미검증이다. 자손도 proxy credential을 상속할 수 있고 TLS/domain-fronting 한계는 위 모델 중계 항목과 같다.
- **옛 helper의 명시 보존 복구(macOS)**: `provider_profile/stale_helper.rs`는 고정 옛 Codex 경로를 가리키는 단일 helper 링크만 들어 있는 exact 임시 디렉터리를 다룬다. 기존 로그인 예약, nofollow capability·dev/inode·대상 부재 재검증 뒤 profile 밖 app-data UUID 이름으로 exclusive rename하고 양쪽 디렉터리를 fsync한다. 기존 인증·vendor를 호출하지 않고 scanner도 완화하지 않는다. 제품 자동 호출/IPC는 없으며 로컬 exact artifact의 opt-in 복구와 재검사는 통과했다. same-UID 경합의 원자 방어나 일반 캐시 청소 기능은 아니다.
- **예약 Codex profile 파일 입장 검사**: 같은 scanner를 앱 전용 HOME 전체(`skills`·`plugins`·`tmp`·`.tmp`의 숨김/중첩 항목 포함)에 재사용한다. spawn 전과 CLI/helper 재검증 후 payload release 직전에 검사하며 인증 파일 내용은 읽지 않는다. 로그인/상태 조회의 uid·쓰기 권한·타입 검사와 실행 후보의 공유 파일·링크·특수 파일 거절을 유지한다. 안전한 plugins 캐시 폴더가 있다는 이유만으로 거절하지는 않으며, 그 내용도 scanner 검사에서 제외하지 않는다. 대신 native OS 정책은 PROFILE/plugins의 읽기·실행에 필요한 읽기·쓰기를 차단한다. 이는 검사 시점 파일 구조와 해당 정책의 결합이며 same-UID 경합 방어·원격 plugin/관리 설정 전체 격리 증명을 대신하지 않는다.
- **플러그인/앱 기능 고정**: native/discovery argv에 features.apps=false·features.plugins=false를 hooks=false와 함께 고정하고, preflight는 effective 설정과 sessionFlags 양쪽에서 명시적 false를 요구한다. 누락/null/true/중복 키는 거절한다. 사용자 config를 수정하지 않으며 플러그인 캐시 삭제/이동도 없다. [공식 관리 설정 문서](https://learn.chatgpt.com/docs/enterprise/managed-configuration#configure-network-access-requirements)는 지원 client에서 이 기능 차단을 설명하지만 명령용 네트워크 프록시가 Codex 서비스/MCP/앱 트래픽을 모두 통제하지는 않는다고 구분한다. 따라서 이 두 flag를 모델 네트워크/관리 정책 격리의 대체 증거로 사용하지 않는다.
- **예약 후보의 오프라인 OS 격리**: native launch는 이제 fd를 닫고 신원을 기록한 기존 gate에서 `/usr/bin/sandbox-exec`를 exec한 뒤 payload를 실행한다. 기본 거부 정책은 시스템/고정 CLI·helper/작업 사본/profile 읽기와 profile 쓰기를 제한적으로 허용하고 profile config 쓰기는 열지 않고 작업 사본 쓰기는 저장된 역할이 workspaceWrite일 때만 허용한다. 경로는 Scheme 삽입이 아니라 개별 `-D` argv이며 profile/workspace 중첩·비정규 경로를 거절한다. 임시 toy CLI의 실제 OS 실행→공개 답변 저장→종료/DB 재오픈을 opt-in 통합 테스트로 확인했다. 네트워크 권한이 없는 오프라인 후보이며 실제 공급자 호환성·모델 통신·도구/승인·제품 capability는 미완료다. 실패 시 무격리 fallback하지 않으며 기존 로그인/discovery 진단은 이 후보와 별도다.
- **쓰기 정책 기제 후보(저장된 역할 연결, 제품 미활성)**: 같은 builder의 typed `WorkspaceAccess::Write`는 작업 사본 일반 파일 쓰기만 추가하고 `.git`·`.agents`·`.codex`와 profile config의 쓰기를 거절한다. CLI/helper가 쓰기 가능한 HOME 또는 작업 사본 안에 있으면 명령 생성도 거절한다. 임시 셸 시험에서 일반/자식 쓰기 성공과 원본·보호 경로 변경/이동·hardlink/symlink 우회 거절을 확인했다. native 예약의 저장된 permission이 argv·OS 정책·preflight 모드를 함께 결정한다. 실제 공급자 호환성·모델 네트워크·도구/승인 연결은 별도다.
- **쓰기 preflight wire 후보**: core의 `ExpectedPermission`을 생성 시 고정하면 설정 유효값·session flag·thread 요청/응답까지 같은 모드를 대조한다. WorkspaceWrite는 추가 writableRoots 없음·networkAccess=false·TMPDIR 및 /tmp 제외를 요구하고, 두 thread 식별 응답이 맞기 전에는 turn을 준비하지 않는다. 기존 `new`와 `start_read_only_thread`는 읽기 전용 계약을 유지하며 write 후보를 읽기 전용으로 낮추지 않는다. sidecar의 `run_text_candidate`는 저장된 역할에서 이 모드를 생성한다. `run_read_only_text`는 기존 읽기 전용 계약을 유지한다. 합성 wire/오프라인 fake CLI 후보이며 native 응답 호환성·도구 실행 연결은 아직 남는다.
- **도구/승인 stream 분류 후보**: TurnStream은 commandExecution/fileChange의 시작·출력·완료 상태를 Tool metadata로 분류하며 raw stdout/diff는 포함하지 않는다. 명령 완료는 시작 당시 command/cwd와 대조하고 잘못된 상태·범위·늦은 출력은 실패한다. id가 있는 서버 요청은 기존 pending_approval 검증과 동일 turn 예산을 거쳐 Approval로 분류하며 중복·unknown 요청은 거절한다. 이는 도구 권한/파일 변경 내용 검증/사용자 결정이 아니다. sidecar의 기존 text 경로는 Tool/Approval도 여전히 거절하며 handler·UI 연결과 실제 공급자 검증은 남는다.
- **승인 대기/전송 내부 handler 후보**: 같은 claim된 LoginProcess가 typed ApprovalReply를 기존 bounded/nonblocking I/O로 보내고 대기 중에도 process 관측과 입력 poll을 유지한다. TurnStream의 미응답 live 요청만 즉시 반환하는 owner callback에 질의하며 기본 결정은 없다. 서버가 먼저 해제한 요청은 응답 후보에서 제거한다. 결정 직전 workspace·관측·취소를 재검증하고 단회 prepare/send 후 serverRequest/resolved는 별도로 기다린다. send/callback/취소 오류는 같은 owner의 cleanup으로 간다. 기존 text wrapper는 Tool/Approval을 계속 거절하며 새 handler는 제품 caller가 없다. accept는 계속 차단되고 전체 30초 후보 예산·UI/원장/실제 provider 입장 미완료를 유지한다.
- **예약 owner의 상호작용 handler 후보**: RunReservation::run_candidate_with_handler는 기존 권한/입력 검증·공개 원장·terminal/fence를 공유하며 Tool/PollApproval만 입력 재검증 뒤 trusted owner에게 전달한다. process 관측은 예약 원장이 직접 소유하고 workspace 재검증은 transport에서 직접 수행한다. callback이 역할을 변경해도 응답 직전 checkpoint가 이를 거절한다. 기존 readOnly/text wrapper는 상호작용을 계속 거절한다. 실제 사용자 결정/공개 승인 IPC/승인 상태 저장/제품 입장은 별도이며 callback 자체가 실행 권한은 아니다.
- **단계별 text 세션(제품 미연결)**: 기존 동기식 driver는 private `TextSession`을 반복 진행한다. 세션은 preflight/thread/turn handshake·stream·미응답 승인을 보존하고 호출마다 수신 frame 하나 이하를 처리하며 응답 대기 자체는 반환한다. 오류/완료 뒤 재개를 거절하고 cleanup 권위는 기존 process owner에 남긴다. typed 송신도 단일 nonblocking write 후 반환하고 재시도마다 workspace/예약 권위·취소·공유 예산을 확인한다. 부분 frame 완료 전 다음 frame/수신 단계로 넘어가지 않는다. 파일/DB 관측은 동기식이며 제품 runtime/main-loop 통합·공개 승인 IPC는 아직 미완료다.
- **실행 프로필 수명**: `LoginProcess::reserve_codex_owned`는 검증된 CodexProfileBinding의 디렉터리 capability를 이동해 요청 함수 밖에서도 유지한다. 기존 로그인 borrowed API와 같은 reservation/cleanup/fence 경로를 사용하며 같은 경로에 새 프로필이 생겨도 원래 신원 검증을 유지한다. LoginProfile의 Sync 경계는 sidecar 초기화 스레드에서 소유 객체를 안전하게 전달하기 위한 컴파일 시간 제약이며 unsafe 구현은 없다.
- **native 소유 객체의 dispatcher 연결(내부 후보)**: AgentRuntime은 검증된 OwnedTextRun을 별도 목록에 보관하고 기존 poll/request_cancel/cancel_result/prepare_quit에 연결한다. 등록 거절은 소유권을 호출자에게 남기며 fixture와 합산한 실행 상한을 적용한다. 공개 원장에 저장된 event만 단조 증가 seq로 전달하고 내부 preparing을 running으로 위장하지 않는다. 종료 저장이 끝나기 전 취소 응답/소유 객체 제거를 하지 않으며 종료 저장 실패에는 owner와 fence를 유지한다. 요청은 live 대기열에서 명시 거절을 기다리며 도구 알림 자체를 승인으로 쓰지 않는다. 실제 native start/capability admission은 아직 미연결이고 제품 blocked 생성자는 유지된다.
- **예약 실행 단계 상태(내부)**: `begin_text_candidate`/`advance_text_candidate`는 Storage를 빌려 두지 않는 RunTextState에 bound transport·잠정 마지막 답변·run ID·통신 완료 결과를 보관한다. process별 연결 ID가 다른 owner의 진행/cleanup을 거절하고 terminal 이후 재개도 거절한다. 통신이 끝나면 다시 읽거나 보내지 않고 `poll_finish`로 종료를 진행한다. 동기 wrapper와 직접 poll은 같은 공개 원장·종료 확인·terminal/fence 로직을 공유하며 제품 runtime/IPC 연결은 후속이다.
- **단계별 종료 확인(내부)**: TreeTracker의 `poll_termination`은 누적 신원·최초 TERM/KILL 기한·관측 실패를 유지하고, 관측 간격 전에는 신호를 반복하지 않으며 sleep 없이 반환한다. 기존 동기 terminate도 같은 알고리즘을 사용한다. LoginProcess는 종료 단계 동안 프로토콜 I/O를 차단한다. 예약 owner는 새 자손 신원을 profile fence와 실행 원장 양쪽에 저장한 뒤 반환하며 저장 실패는 종료 증거를 무효화한다. 재시작 복구는 profile pending 파일을 읽지 않고 실행 원장의 누적 신원을 사용한다. 종료 확인 전에는 receipt/final/terminal 또는 fence 해제를 만들지 않고 원장 저장 뒤에만 fence를 해제한다. 파일/DB/OS 조회는 여전히 동기식이고 비정상 Drop의 최후 cleanup 및 명시적 실패 재시도는 기존 bounded blocking 경로다. 앱 전체의 비차단 종료 보장이나 제품 활성화 증거는 아니다.
- **계정 조회 알림 호환성**: full preflight는 account/read 응답 대기 중 typed account/updated(authMode=chatgpt) 한 번만 허용한다. emittedAtMs는 기존 로그인 규격의 u64/JS-safe 상한, planType은 제한된 선택 문자열이다. 알림은 pending RPC를 소비하지 않고 동기/단계별 driver 모두 같은 종료 owner·시간 예산에서 실제 id3 응답을 기다린다. 다른 단계/방식·중복·미지 필드·id가 있는 알림은 거절한다. 2026-09-29 고정 CLI/helper·앱 프로필에서 plugins 캐시 호환 수정 후 계정·모델·관리 정책 대조와 종료/잠금 정리를 모두 통과했다. 유료 모델/thread/turn·native 제품 실행의 합격 증거는 아니다.
- **thread 생성 알림 시각**: `thread/started`도 공급자 `ServerNotification`의 선택 `emittedAtMs`를 같은 u64/JS-safe 범위로 받는다. 시각은 권한이나 순서 판정에 사용하지 않으며, 응답/알림의 동일 thread ID·각1회·설정 대조가 모두 필요하다. 음수·문자열·null·상한 초과·중복 키·미지 envelope 필드는 거절한다. 근거는 Codex0.159.2의 생성 JSON schema와 [공식 App Server 흐름](https://learn.chatgpt.com/docs/app-server)이며 실제 계정 실패 해결 증거와 구분한다.
- **장기 native 실행 owner(내부)**: OwnedTextRun은 준비된 RunReservation·소유형 LoginProcess·RunTextState·취소 표식을 함께 유지한다. Storage는 advance 동안만 빌린다. 기존 RunIdentity 6필드와 live run/process 결속을 확인한 취소만 접수하며 다음 advance가 종료/원장을 처리한다. 제품 active-run 목록과 공개 IPC에 아직 등록되지 않은 내부 연결 객체다.
- **내부 preflight 세션(제품 미활성)**: `provider_preflight_session.rs`는 새로운 exchange의 typed frame만 기존 LoginProcess/LoginIo에 보내며 통신 전체 30초 예산과 취소·자손 관측·별도 종료 확인을 적용한다. 조회 전용 또는 동일 process의 read-only thread 진단 뒤 cleanup을 확인하고 exchange를 닫는다. thread/start 응답과 thread/started 알림을 순서와 무관하게 각각 한 번 받고 동일 ID를 대조한다. 알림 단독은 성공이 아니다. 불확실 종료는 기존 영속 잠금과 호출자의 owner를 유지한다. 가짜 CLI 파이프 검사 단계이며 실제 provider launcher/config 격리·제품 실행 gate는 별도로 남는다. thread 생성도 통합 기능을 초기화할 수 있어 실제 진단 전 격리가 필요하다.
- **관리 정책 조회(내부 후보)**: layered full preflight는 initialize/config/read/account/read/model/list 뒤 고정 `configRequirements/read`(id 50)를 조회하고 정확한 `requirements:null` 보고까지 받아야 ObservationsMatched가 된다. 기존 thread/turn id 5/6은 유지한다. 관리 객체·누락·중복·미지 필드·RPC 오류는 연결 실패로 고정하며 raw 정책은 공개하지 않는다. 현재 후보는 관리 정책을 지원하지 않으므로 객체를 무시해 실행하지 않는다. [공식 API](https://learn.chatgpt.com/docs/app-server#read-admin-requirements-configrequirementsread)의 null은 구성된 정책 없음이라는 vendor 보고이며, 접근 불가 MDM의 신뢰 가능한 가시성까지 증명하지 않는다. 실제 계정 프로필의 5 RPC 성공·제품 활성화는 아직 미검증이다.
- **설정 전용 진단 범위**: `PreflightExchange::configuration_only`는 동일 초기화/설정 layer 검증 후 ConfigurationMatched에서 끝난다. account/read·model/list·configRequirements/read 요청을 만들지 않고 thread/start·turn 준비도 거절하며 전체 preflight의 ObservationsMatched와 구분한다. 기존 세션 driver는 해당 범위만 대조하고 cleanup을 확인한다. 실제 vendor 실행·네트워크 차단·임시 profile 입장은 별도 검증 사항이며 이 진단 성공을 실제 실행 준비 완료로 사용하지 않는다.
- **실제 offline 진단 결과(2026-09-29)**: 기존 정책에서는 관리 설정 동기화 경고로 실패했지만 `com.apple.system.opendirectoryd.libinfo` Mach 조회 한 항목을 허용한 후보에서 현재 pinned native CLI/helper의 initialize/config/read가 PASS했다. 새 임시 profile·빈 작업 폴더를 사용하며 explicit 종료·config 보존·auth 파일 미생성을 확인했다. cfprefsd·설정 도메인·공유 메모리 권한은 추가하지 않았다. 수동 opt-in 테스트 `installed_codex_offline_configuration_only`는 일반 테스트에서 제외된다. 이는 이 호스트의 설정 통신 호환성 증거뿐이다. MDM 설정이 upstream에서 보이지 않아 생략되는 경우를 세 layer 검증만으로 감지할 수 없으므로, 강제 관리 정책의 신뢰 가능한 가시성/명시적 거절을 검증하기 전에는 제품 admission 근거로 사용하지 않는다. 직접 네트워크 허용은 없지만 허용된 OS broker의 간접 동작까지 무통신이라고 증명한 것은 아니다. 제품 runtime은 여전히 blocked다.

- **실제 vendor 관측 파서**: `core/provider/vendor_observation.rs`는 Codex 0.149.1·Claude 2.1.269의 exact version과 모델 없는 인증 상태 응답만 읽는다. transient bounded input에서 계정/토큰을 제거한 분류만 반환하며 runtime·capability·영속 원장에는 아직 연결하지 않는다. config/model/handshake, 설정 격리와 launch authority는 별도 미구현이다. 이후 설치 CLI는 0.153.4로 변경됐으며 기존 exact version 허용을 자동 확대하지 않았다.
- **로그인 schema 드리프트 검사**: `node scripts/m5-codex-login-schema.mjs <기준 생성 폴더> <후보 생성 폴더>`는 초기화·로그인/취소·계정 조회/알림 타입과 재귀 type import의 원문 해시를 비교한다. 종료 0은 이 범위의 원문 동일, 2는 검토 필요, 1은 비교 불가이며 전체 프로토콜/실행 호환 판정이 아니다. 0.149.1→0.153.4 대조는 20파일 중 Bedrock 인증 추가 2파일 변경으로 검토 필요다. 생성 입력은 로컬 임시 증거이며 이 도구가 CLI 실행·인증 조회를 하지는 않는다.

- **작업대 배치 현행**: `src/m5/workbench-dock.tsx`는 파일·편집기·변경·팀 대화를 중첩 가로/세로 분할과 여러 탭 그룹으로 구성한다. 새 기본은 파일/변경 탭·편집기·팀 대화의3열이며 기존 저장 배치는 보존한다. 초기화/프리셋/표시 조작은 펼칠 수 있는 배치 설정에 모았다. 제목/tab을 가장자리에 놓으면 분할하고 가운데 놓으면 탭으로 묶으며 배치 선택 메뉴로도 같은 작업을 할 수 있다. 경계선 pointer/키보드 조절, 탭 좌우/Home/End 및 초점 이동을 지원한다. 검증된 트리를 기존 v1 localStorage에 추가 저장하며 이전 평면 배치를 계속 읽는다. 숨김/재배치 때 평면 keyed DOM을 유지해 Monaco를 재생성하지 않는다. 다크 Monaco와 잔여 창 높이 기반 패널을 사용한다. 전체 승인 시안 적용 완료를 뜻하지 않는다.
- **실제 CLI 최소 관측(2026-09-14)**: 빈 임시 폴더에서 Codex 0.149.1 app-server 초기화·config/account 조회·정상 종료와 Claude 2.1.269 인증 상태 조회를 확인했다. 모델 요청과 thread 생성은 없었다. 이 관측만으로 설정 격리·모델 가용성·실행 권위를 보장하지 않으며 제품 actual provider는 활성화하지 않았다.

- **부분 RSS 관측 경계**: `m5:measure:renderer --memory`는 실제 Electron subtree의 RSS를 250ms 간격으로 관측한다. `f4def74`는 idle 약 409.73 MiB 및 한 회차 표시 지연으로 FAIL했다. 테스트 fixture의 idle 선할당을 분리한 `dfe12bd`는 idle/peak/60초 잔류 356.70/761.73/110.125 MiB와 GUI 시간 기준을 통과했다. 합성 bridge를 쓰며 실제 sidecar/agent·이탈 자손은 포함하지 않아 전체 제품 메모리 증거가 아니다. 이전 실패는 삭제하지 않는다.

- **G6 성능 계측**: `scripts/m5-performance-metrics.mjs`는 명세 §10의 공급된 측정값을 판정하고 `scripts/m5-sidecar-startup-metrics.mts`는 현재 Cargo release executable을 생성된 빈 DB에서 migration 제외·warm-up 1회·10회 실행해 shell/storage ready를 구분한다. `d97ee56`에서 p95 3.863/52.596ms와 정상 종료·clean marker를 확인했다. `7832348`의 `scripts/m5-renderer-metrics.mjs`는 production renderer·실제 Monaco와 합성 bridge fixture로 대형 파일/diff·파일 목록의 10회 GUI 측정을 통과했다(파일 표시 p95 75.9ms, 입력 32.3ms, 변경 요약 38.7ms, 첫 diff 25.0ms). 파일 목록은 opaque cursor로 다음 500개를 탐색하고 오래된 응답이 새 폴더를 덮지 못한다. 패키징 interactive startup·실제 스트림 paint·전체 RSS·경합/종료 계측은 아직 남아 있으며 `m5:test:performance`는 미등록 상태다. 절차는 `procedures/m5-performance.md`다.

- **actual-agent 전체 수명 보존의 현재 경계**: G5D의 exact v10 admission, V9/V10 materializer, V10 bounded lossless writer, cooperative scheduler와 기존 D1 `storage.status` wire는 구현됐다. 최신 clean integration commit `6461e67617f4f5e55d7d646034dbb0971f1d845a`에서 macOS fixture 누적 `g5d-storage-lifetime` 16/16과 darwin-arm64 package·Chromium Monaco smoke를 통과했다(`.artifacts/m5-harness/g5d-storage-lifetime-2026-09-09T03-27-46-038Z.json`). 누적 하네스는 standalone wrapper의 범위를 보존하면서 locked workspace Rust를 한 번만 실행하고, 각 명령 전후 clean/same-HEAD 및 focused storage evidence의 동일 HEAD를 schema v2 report에서 재검증한다. 실측 시간은 47분 18초에서 16분 32초로 단축됐다. 이는 출시 승인이 아니며 Windows 안전 게이트, 실제 사용자 DB·강제 종료 검증, 실제 Claude Code/Codex provider·인증, G6 성능·서명·배포 판정이 남아 있다. 보존 기간과 physical cleanup/삭제 정책은 사용자 결정 전 계속 비활성이다.
- **코드 서명**: 미서명 배포 시 SmartScreen/Gatekeeper 경고. 베타는 미서명 + 통과 안내 이미지로 진행, 정식 릴리즈 전 비용(연 $250~400) 결정 — 기획 문서 참고.
- **실기기 출시 검증**: 깨끗한 macOS 계정과 Windows 기기에서 설치·로그인·오프라인·권한 오류·한글 경로 시나리오 확인.
- **업데이트 채널**: 현재 앱은 에이전트 최신 버전만 확인한다. Hello, Agent 자체 업데이트와 레시피 원격 갱신 방식은 첫 베타 이후 결정.
