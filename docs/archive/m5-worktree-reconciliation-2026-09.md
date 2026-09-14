# M5 잔여 워크트리 대조·종결 기록

2026-09-14, GPT-6 Astra. 기준 통합 HEAD: `3790832`.

사용자가 남은 미커밋 작업 전부를 확인·완료·통합하도록 요청했다. 아래는 당시 초안과 현재 구현을 대조한 결과다. 보존 커밋은 원본 복원용이며 빌드 가능 또는 제품 승인 커밋이 아니다. 오래된 구현 전체를 merge하면 현행 안전 계약·저장소 버전·IPC를 되돌리므로, 이미 반영된 기능은 재병합하지 않고 빠진 회귀만 현재 구현에 이식했다.

## 9개 워크트리 처리

| 워크트리 | 확인한 내용과 처리 | 원본 보존 커밋 |
| --- | --- | --- |
| `g5d-package-smoke-fix` | 코드 변경 없음. exact capability/status/poll 검증 JSON 3개를 보존. 현재 package smoke는 D1 exact status와 bounded polling을 사용한다. | `90a41bcc9ca7ec834c3f2015af85113b91105f42` |
| `g5d-r3c-byte-transport` | 101-workspace 생산 fixture와 private maintenance 회귀 초안. 생산 fixture는 `5ad7162`에 통합됐고, 현행 `storage/tests/reopen_snapshot.rs`는 초안의 ready/pending·late corruption·external commit·cancel·READ/COMMIT 실패를 구현한다. 기존 역사 기록도 월별 archive에 존재한다. 옛 API 초안을 재적용하지 않는다. | `8fb78c516a9265c26c89b09117d2cdb015236f71` |
| `g5d-storage-v6-migration` | V6 migration/index/readiness 초안. `e3d04b7`에서 provenance·중복 메시지·predecessor 회귀를 보완해 통합했고 현재 V10까지 발전했다. 초안의 단순 last_message 추출로 현행 source provenance 검증을 덮지 않는다. | `d2baa2f4c0158a86c4acfca70b4325b8e71c45f3` |
| `m5-electron-scaffold` | Forge 7/Electron 44 설치 차단 시점의 version-only bridge 초안. 현재 Forge 8 alpha exact pins, Electron 43.4, CJS entry, V6 bridge, sidecar staging·fuses·CSP·Monaco로 대체됐다. 옛 의존성과 generic scaffold를 복원하지 않는다. 당시 작업 메모와 파일 전체는 보존한다. | `606055a9faf00024ccc8ee415e6278fe3c21005f` |
| `m5-g4-harness-wiring` | 코드 변경 없음. 이미 통합된 `ba15c61`의 검토/실행 보고서 2개를 보존한다. | `667424eaa7dd74b93955eee6e657a69fc926b2af` |
| `m5-g4-integration-regressions` | 코드 변경 없음. 이미 통합된 `50ab6e9`의 검토·요약·lock blob 및 ignored raw log를 보존한다. | `62acf152ca7c57904376f595ca895bff8f6cee18` |
| `m5-g4-package-smoke-contract` | 코드 변경 없음. 이미 통합된 `835a567`의 검토/실행 보고서를 보존한다. 현재 D1 검증이 당시 G4 capability만 비교하던 구현을 대체한다. | `f45a1fc16e575d517948620ad7a9a20bded13915` |
| `m5-g4-project-safety` | linked-worktree 기반 옛 G4 branch의 3개 커밋과 stable-root rename 수정 초안. 현재 `01acee5` standalone snapshot, `21b9152` stable-root 저장, `0bf04f7` syscall symlink 거부로 대체됐다. 미사용 FD 검사 helper를 되살리지 않는다. 작업 복사본 전체를 symlink로 바꾸는 회귀는 현행 pre-syscall hook으로 이식했다. | `9d10be218d5b00558cc47beddddadf09a582cf59` |
| `qa-low-impact` | 저부하 QA 초안은 `62abf69`에서 반영됐다. `scripts/m5-qa-resources.mjs`는 byte-identical이며 현행 하네스의 env/priority 기록·검증과 Astra 운영 규칙을 유지한다. 원본 작업 메모와 검증 JSON을 보존한다. | `7924cf6d99a63ed8e0f222500651937de69cfae6` |

## 실제 통합 차이와 검증

- 제품 코드는 변경하지 않는다. `project/tests.rs`에 `managed_worktree_child_swap_at_save_syscall_never_writes_symlink_target`를 추가했다. 저장 syscall 직전에 관리 worktree를 외부 디렉터리 symlink로 바꾸고, 외부/기존 파일 불변·recoveryRequired·잠금 유지를 확인한다. macOS 회귀이며 Windows 지원 증거가 아니다.
- 새 Rust 회귀 1/1와 기존 stable-root syscall 회귀 2/2 PASS. Electron scaffold/하네스 현재 Node 회귀 14/14 PASS. rustfmt와 diff 검사 PASS.
- 원본 보존 커밋 9개는 기존 기능 브랜치에 남긴다. 보존은 원문 손실 방지이지 오래된 코드의 제품 통합 승인이나 새 기능 완료 선언이 아니다.
- `qa-low-impact/.artifacts`는 root `.artifacts/retired-worktrees-2026-09-14/qa-low-impact`에도 복사하고 recursive diff로 일치를 확인했다. 나머지 검토 보고서/ignored raw log는 보존 커밋에 들어 있다. 재생성 가능한 빌드 캐시는 워크트리 제거 때 함께 정리했다.
- 9개 worktree는 보존 SHA·브랜치 참조·clean 상태·사용 중 cwd 없음 확인 뒤 force 없이 제거했다. 현재 통합 checkout과 브랜치/커밋은 남겼으며 main 병합·push는 하지 않았다.

## 복원

위 보존 SHA를 `git show <SHA>:<원래 파일 경로>`로 조회할 수 있다. 전체 복사본이 필요할 때만 해당 SHA에서 별도 detached worktree를 만든다. 예전 보존 커밋 전체를 현행 통합 브랜치로 cherry-pick하지 않는다.

실제 CLI 연결 승인·전체 G6·제품 적용/복원·Windows 검증은 이번 잔여 초안 종결과 별개로 남는다.
