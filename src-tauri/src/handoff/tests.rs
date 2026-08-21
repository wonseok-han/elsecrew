//! 자동 인계의 **중단·정리·잠금·결과 계약** 검증.
//!
//! 종료 판정 자체(F1~F6)는 `proctree`가, 실행 계약은 `claude_pilot`이 덮는다.
//! 여기서 지키는 것은 그 둘을 엮는 흐름이다 — 취소가 어느 단계에서도 닿는가,
//! 오류 경로가 프로세스를 남기지 않는가, 잠금과 결과를 사실대로 보고하는가.

use super::*;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hello-agent-handoff-{name}-{}-{}",
        std::process::id(),
        orchestration::new_run_id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 제한 시간 안에 끝나야 하는 작업. **돌아오지 않는 구현을 테스트가 붙잡고 있지 않게 한다.**
fn within<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(limit).ok()
}

// ───────────────────────── 세션 ID ─────────────────────────

#[test]
fn session_id_is_a_valid_uuid_v4() {
    let id = new_session_id();
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(parts.len(), 5, "{id}");
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        vec![8, 4, 4, 4, 12],
        "{id}"
    );
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
        "{id}"
    );
    assert!(parts[2].starts_with('4'), "버전 4가 아니다: {id}");
    assert!(
        matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')),
        "변형 비트가 틀렸다: {id}"
    );
}

#[test]
fn session_ids_do_not_repeat() {
    let ids: Vec<String> = (0..64).map(|_| new_session_id()).collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "세션 ID가 겹쳤다");
}

#[test]
fn planning_prompt_carries_the_goal_and_the_read_only_role() {
    let prompt = planning_prompt("인사말을 한국어로 바꿔줘");
    assert!(prompt.contains("인사말을 한국어로 바꿔줘"));
    assert!(prompt.contains("planning role"));
    assert!(prompt.contains("cannot modify files"));
}

// ───────────────────────── 실행 슬롯과 취소 소유권 ─────────────────────────

/// 인계는 한 번에 하나다. 두 실행이 같은 복제본을 건드리면 증거가 서로를 덮는다.
#[test]
fn a_second_handoff_is_refused_while_one_is_in_progress() {
    let session = HandoffSession::default();
    let first = session.reserve().expect("첫 예약이 거부됐다");

    let second = session.reserve();
    assert!(second.is_err(), "동시 실행이 허용됐다");

    session.release(&first);
    assert!(session.reserve().is_ok(), "끝난 뒤에도 슬롯이 잠겨 있다");
}

/// **준비 중에 누른 중단이 실행 시작으로 지워지면 안 된다.**
/// 취소 깃발을 실행이 만들면 이 순간의 취소가 통째로 사라진다.
#[test]
fn cancel_during_preparation_survives_the_run_start() {
    let session = HandoffSession::default();
    let run_id = session.reserve().unwrap();

    // 복제본을 만드는 동안 사용자가 중단을 눌렀다
    let forwarded = session.request_cancel();
    assert!(!forwarded, "아직 구현 중이 아닌데 Codex로 취소를 보냈다");

    // 실행이 소유권을 잡는다
    let cancel = session.claim(&run_id).expect("실행이 슬롯을 잡지 못했다");

    assert!(
        cancel.load(Ordering::Relaxed),
        "준비 중 누른 중단이 실행 시작으로 지워졌다"
    );
}

#[test]
fn a_run_cannot_be_claimed_twice_or_by_another_id() {
    let session = HandoffSession::default();
    let run_id = session.reserve().unwrap();

    assert!(session.claim("someone-else").is_err(), "남의 실행을 잡았다");
    assert!(session.claim(&run_id).is_ok());
    assert!(session.claim(&run_id).is_err(), "같은 실행을 두 번 잡았다");
}

/// 취소를 Codex 실행까지 전달할지는 **지금 무슨 일을 하는가**로 갈린다.
/// 계획 중에 전달하면 이 인계와 무관한 Codex 실행을 끊을 수 있다.
#[test]
fn cancel_reaches_codex_only_while_building() {
    let session = HandoffSession::default();
    let run_id = session.reserve().unwrap();
    let _cancel = session.claim(&run_id).unwrap();

    session.set_stage(&run_id, HandoffStage::Organizing);
    assert!(!session.request_cancel(), "계획 중에 Codex를 끊으려 했다");

    session.set_stage(&run_id, HandoffStage::Building);
    assert!(
        session.request_cancel(),
        "구현 중인데 Codex에 취소가 안 갔다"
    );
}

/// 끝난 실행이 뒤늦게 보낸 상태가 새 실행의 슬롯을 건드리면 안 된다.
#[test]
fn a_finished_runs_stage_does_not_leak_into_the_next_one() {
    let session = HandoffSession::default();
    let old = session.reserve().unwrap();
    session.release(&old);
    let new = session.reserve().unwrap();

    // 지난 실행의 뒤늦은 상태 보고
    session.set_stage(&old, HandoffStage::Building);

    assert!(
        !session.request_cancel(),
        "지난 실행의 진행 상태가 새 실행에 섞였다"
    );
    session.release(&new);
}

#[test]
fn releasing_someone_elses_run_does_not_open_the_slot() {
    let session = HandoffSession::default();
    let run_id = session.reserve().unwrap();
    session.release("some-other-run");
    assert!(session.reserve().is_err(), "남의 이름으로 슬롯이 열렸다");
    session.release(&run_id);
}

// ───────────────────────── Codex 구간 취소 전달 ─────────────────────────

/// 취소는 **Codex 실행이 슬롯에 들어오기 전에** 눌릴 수 있다.
/// 그때 한 번 보내고 마는 구현은 취소를 흘린다 — 닿을 때까지 다시 시도해야 한다.
#[test]
fn the_cancel_bridge_retries_until_the_codex_run_exists() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let forward: CancelForward = {
        let attempts = attempts.clone();
        Arc::new(move || {
            // 처음 두 번은 아직 끊을 대상이 없다 (실행이 슬롯을 예약하기 전)
            Ok(attempts.fetch_add(1, Ordering::SeqCst) >= 2)
        })
    };
    let bridge = CancelBridge::spawn(Arc::new(AtomicBool::new(true)), forward);
    // 실행이 슬롯을 예약할 때까지 걸리는 시간
    std::thread::sleep(Duration::from_millis(400));

    assert!(bridge.finish(), "취소가 Codex 실행에 전달되지 않았다");
    assert!(
        attempts.load(Ordering::SeqCst) >= 3,
        "한 번 보내고 포기했다: {}회",
        attempts.load(Ordering::SeqCst)
    );
}

#[test]
fn the_cancel_bridge_stays_quiet_without_a_cancel() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let forward: CancelForward = {
        let attempts = attempts.clone();
        Arc::new(move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    };
    let bridge = CancelBridge::spawn(Arc::new(AtomicBool::new(false)), forward);
    std::thread::sleep(Duration::from_millis(200));

    assert!(!bridge.finish());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        0,
        "취소하지 않았는데 실행을 끊으려 했다"
    );
}

/// 실행이 끝나기 직전에 들어온 취소를 흘리면 안 된다.
/// 접으라는 신호를 먼저 보는 구현은 이 취소를 한 번도 전달하지 못하고 끝난다.
#[test]
fn a_cancel_arriving_just_before_the_end_is_still_delivered() {
    let cancel = Arc::new(AtomicBool::new(false));
    let forward: CancelForward = Arc::new(|| Ok(true));
    let bridge = CancelBridge::spawn(cancel.clone(), forward);

    cancel.store(true, Ordering::Relaxed);
    assert!(bridge.finish(), "마지막 순간의 취소가 버려졌다");
}

/// 끊다 실패한 것도 **전달된 것**이다 — 실행 소유자가 그 실패를 poisoned로 처리한다.
#[test]
fn a_failed_forward_is_not_retried_forever() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let forward: CancelForward = {
        let attempts = attempts.clone();
        Arc::new(move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err("could not terminate the tree".to_string())
        })
    };
    let bridge = CancelBridge::spawn(Arc::new(AtomicBool::new(true)), forward);

    assert!(bridge.finish());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

// ───────────────────────── 잠금 ─────────────────────────

/// 표시를 **저장하지 못했는데 잠갔다고 보고하면** 사용자는 재시작 후 오염된 복제본을 다시 쓴다.
#[cfg(unix)]
#[test]
fn a_lock_that_cannot_be_saved_is_not_reported_as_saved() {
    use std::os::unix::fs::PermissionsExt;

    let base = temp_dir("lock-save-fail");
    let workspace = base.join("clone");
    std::fs::create_dir_all(&workspace).unwrap();
    // 표시는 복제본 **밖**(부모)에 쓰인다. 부모를 읽기 전용으로 만들어 저장을 실패시킨다
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o500)).unwrap();

    let (lock, error) = lock_workspace(&workspace, "tree could not be confirmed gone");

    assert_eq!(
        lock,
        WorkspaceLock::SessionOnly,
        "저장에 실패했는데 잠금이 남았다고 보고했다"
    );
    assert!(error.is_some(), "저장 실패 사실이 결과에서 빠졌다");
    // 저장에 실패해도 **같은 복제본 재사용은 실제로 거부돼야 한다**
    assert!(
        ensure_reusable(&workspace).is_err(),
        "표시를 못 남겼다고 재사용이 열렸다"
    );

    clear_session_lock(&workspace);
    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_saved_lock_is_reported_as_persisted_and_blocks_reuse() {
    let base = temp_dir("lock-saved");
    let workspace = base.join("clone");
    std::fs::create_dir_all(&workspace).unwrap();

    let (lock, error) = lock_workspace(&workspace, "tree could not be confirmed gone");

    assert_eq!(lock, WorkspaceLock::Persisted);
    assert!(error.is_none(), "{error:?}");
    assert!(ensure_reusable(&workspace).is_err());

    clear_session_lock(&workspace);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn an_untouched_workspace_is_reusable() {
    let base = temp_dir("lock-clean");
    let workspace = base.join("clone");
    std::fs::create_dir_all(&workspace).unwrap();
    assert!(ensure_reusable(&workspace).is_ok());
    let _ = std::fs::remove_dir_all(&base);
}

// ───────────────────────── 실행 오류 경로 ─────────────────────────

/// 계약 테스트용 가짜 실행 대상. Claude 인자를 전부 무시하고 지정한 대로만 행동한다.
///
/// **아무것도 출력하지 않고 오래 사는 트리**라는 점이 핵심이다 —
/// stdout 읽기 루프 안에서 취소를 확인하는 구현은 여기서 영영 멈춘다.
#[cfg(unix)]
struct FakeClaude {
    base: PathBuf,
    program: PathBuf,
    /// 스크립트가 남기는 자기 pid — "정말로 떴는가"의 증거이자 뒷정리 대상
    root_pid: PathBuf,
    /// 배경 자손의 pid
    descendant_pid: PathBuf,
}

#[cfg(unix)]
impl FakeClaude {
    fn quiet(name: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let base = temp_dir(name);
        std::fs::create_dir_all(base.join("clone")).unwrap();
        let program = base.join("fake-claude.sh");
        let root_pid = base.join("root.pid");
        let descendant_pid = base.join("descendant.pid");
        let script = format!(
            "#!/bin/sh\n\
             echo $$ > '{root}'\n\
             /usr/bin/tail -f /dev/null &\n\
             echo $! > '{descendant}'\n\
             exec /usr/bin/tail -f /dev/null\n",
            root = root_pid.display(),
            descendant = descendant_pid.display(),
        );
        std::fs::write(&program, script).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            base,
            program,
            root_pid,
            descendant_pid,
        }
    }

    fn workspace(&self) -> PilotWorkspace {
        PilotWorkspace {
            path: self.base.join("clone"),
            start_commit: "HEAD".into(),
        }
    }

    fn evidence(&self) -> ClaudeEvidence {
        ClaudeEvidence {
            events: self.base.join("events.jsonl"),
            stderr: self.base.join("stderr.log"),
            diff: self.base.join("run.diff"),
        }
    }

    fn pid_at(path: &Path) -> Option<u32> {
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    /// 스크립트가 남긴 pid가 하나도 살아 있지 않은가.
    fn tree_is_gone(&self) -> bool {
        [&self.root_pid, &self.descendant_pid]
            .iter()
            .all(|path| match Self::pid_at(path) {
                Some(pid) => proctree::identity_of(pid).is_err(),
                None => true,
            })
    }

    fn started(&self) -> bool {
        self.root_pid.exists()
    }

    /// 살아남은 프로세스는 테스트가 직접 끝낸다.
    fn cleanup(&self) {
        for path in [&self.root_pid, &self.descendant_pid] {
            if let Some(pid) = Self::pid_at(path) {
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// **stdout이 한 줄도 오지 않아도** 취소는 닿아야 한다.
/// 읽기 루프 안에서 취소를 보는 구현이면 이 테스트는 제한 시간을 넘긴다.
#[cfg(unix)]
#[test]
fn cancel_reaches_a_run_that_never_prints() {
    let fake = FakeClaude::quiet("silent-cancel");
    let program = fake.program.clone();
    let workspace = fake.workspace();
    let evidence = fake.evidence();
    let cancel = Arc::new(AtomicBool::new(false));

    // 고정 시간을 기다리지 않는다. 기계가 느려 아직 뜨지도 않은 실행을 취소하면
    // "조용한 실행에 취소가 닿는가"를 검증하지 못한 채 통과한다.
    {
        let cancel = cancel.clone();
        let root_pid = fake.root_pid.clone();
        let descendant_pid = fake.descendant_pid.clone();
        std::thread::spawn(move || {
            for _ in 0..500 {
                if root_pid.exists() && descendant_pid.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            cancel.store(true, Ordering::Relaxed);
        });
    }

    let started = Instant::now();
    let outcome = within(Duration::from_secs(20), move || {
        run_claude_plan_with(&program, &workspace, "목표", &evidence, &cancel, terminate)
            .map(|_| ())
            .map_err(|e| match e {
                ClaudeFailure::Poisoned(reason) => format!("poisoned: {reason}"),
                ClaudeFailure::Failed(reason) => reason,
            })
    });

    let Some(outcome) = outcome else {
        fake.cleanup();
        panic!("조용한 실행에서 취소가 닿지 않았다 (제한 시간 초과)");
    };
    let elapsed = started.elapsed();
    let reason = outcome.expect_err("취소했는데 성공으로 끝났다");

    assert!(reason.contains("canceled"), "{reason}");
    assert!(
        elapsed < Duration::from_secs(15),
        "취소가 stdout을 기다렸다: {elapsed:?}"
    );
    assert!(
        fake.started(),
        "가짜 실행 대상이 뜨지 않아 검증이 무의미하다"
    );
    assert!(fake.tree_is_gone(), "취소했는데 트리가 남았다");

    fake.cleanup();
}

/// 종료를 확인하지 못한 뒤 `wait()`로 기다리면, 파이프를 문 생존자 때문에 영영 돌아오지 않는다.
/// **제한된 회수만 하고 즉시 poisoned로 끝나야 한다.**
#[cfg(unix)]
#[test]
fn an_unconfirmed_termination_returns_poisoned_without_waiting() {
    /// 소멸을 끝내 확인하지 못하는 종료 절차. 실제로 안 죽는 트리는 만들 수 없으므로
    /// **확인 실패 이후의 행동**은 이 주입으로만 검증된다.
    fn never_confirms(_: &Arc<Mutex<TreeTracker>>) -> TerminationReport {
        TerminationReport {
            outcome: proctree::TerminationOutcome::Unconfirmed(
                proctree::UnconfirmedReason::Survivors,
            ),
            detail: "forced for the test".into(),
            survivors: Vec::new(),
            tracked: Vec::new(),
            rounds: 3,
            signal_errors: Vec::new(),
        }
    }

    let fake = FakeClaude::quiet("unconfirmed");
    let program = fake.program.clone();
    let workspace = fake.workspace();
    let evidence = fake.evidence();
    let cancel = Arc::new(AtomicBool::new(false));

    // 종료 절차에 닿으려면 실행을 끝내야 한다. 오래 도는 실행을 제풀에 끊지 않는 것이
    // 정상 동작이므로, 여기서는 취소로 종료 구간에 들어간다.
    // 그 뒤 **poisoned가 취소보다 우선**하는지가 이 테스트의 관심사다.
    {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel.store(true, Ordering::Relaxed);
        });
    }

    let outcome = within(Duration::from_secs(5), move || {
        run_claude_plan_with(
            &program,
            &workspace,
            "목표",
            &evidence,
            &cancel,
            never_confirms,
        )
        .map(|_| String::new())
        .map_err(|e| match e {
            ClaudeFailure::Poisoned(reason) => reason,
            ClaudeFailure::Failed(reason) => panic!("잠기지 않고 실패로만 끝났다: {reason}"),
        })
    });

    let Some(outcome) = outcome else {
        fake.cleanup();
        panic!("종료 확인 실패 뒤에도 돌아오지 않았다 — blocking 대기가 남아 있다");
    };
    let reason = outcome.expect_err("확인하지 못했는데 성공으로 끝났다");
    assert!(reason.contains("could not be confirmed gone"), "{reason}");

    fake.cleanup();
}

/// 증거 파일을 만들지 못하면 **자식을 띄우기 전에** 끝나야 한다.
/// 띄운 뒤 조기 반환하면 프로세스와 감시 스레드가 주인 없이 남는다.
#[cfg(unix)]
#[test]
fn a_run_that_cannot_record_evidence_never_starts_a_process() {
    let fake = FakeClaude::quiet("evidence-fail");
    let mut evidence = fake.evidence();
    // 부모 폴더가 없는 경로 — 생성이 실패한다
    evidence.events = fake.base.join("no-such-dir").join("events.jsonl");

    let result = run_claude_plan_with(
        &fake.program,
        &fake.workspace(),
        "목표",
        &evidence,
        &AtomicBool::new(false),
        terminate,
    );

    match result {
        Err(ClaudeFailure::Failed(reason)) => {
            assert!(
                reason.contains("cannot create the claude event log"),
                "{reason}"
            );
        }
        Err(ClaudeFailure::Poisoned(reason)) => panic!("복제본을 잠글 이유가 없다: {reason}"),
        Ok(_) => panic!("증거를 못 남기는데 성공했다"),
    }
    assert!(
        !fake.started(),
        "증거 생성에 실패했는데도 프로세스가 떴다 — 주인 없는 자식과 감시 스레드가 남는다"
    );

    fake.cleanup();
}

/// stderr 증거 파일도 마찬가지다.
#[cfg(unix)]
#[test]
fn a_run_that_cannot_record_stderr_never_starts_a_process() {
    let fake = FakeClaude::quiet("stderr-fail");
    let mut evidence = fake.evidence();
    evidence.stderr = fake.base.join("no-such-dir").join("stderr.log");

    let result = run_claude_plan_with(
        &fake.program,
        &fake.workspace(),
        "목표",
        &evidence,
        &AtomicBool::new(false),
        terminate,
    );

    assert!(matches!(result, Err(ClaudeFailure::Failed(_))));
    assert!(!fake.started(), "stderr 증거 실패 후에도 프로세스가 떴다");

    fake.cleanup();
}

// ───────────────────────── 변경 파일 목록 ─────────────────────────

/// 패치 본문(`+++ b/…`)으로 목록을 만들면 **삭제·이름만 변경·바이너리가 통째로 사라진다.**
/// 그래서 목록은 git 상태 코드에서 온다.
#[test]
fn changed_files_include_deletions_renames_and_binaries() {
    let raw = "A\0added file.txt\0M\0blob.bin\0D\0gone.txt\0M\0keep.txt\0R100\0old.txt\0new.txt\0";

    let files = parse_name_status(raw);

    assert_eq!(files.len(), 5, "{files:?}");
    let by_path = |path: &str| files.iter().find(|f| f.path == path).cloned();

    let deleted = by_path("gone.txt").expect("삭제된 파일이 목록에서 빠졌다");
    assert_eq!(deleted.kind, ChangeKind::Deleted);

    let renamed = by_path("new.txt").expect("이름만 바뀐 파일이 목록에서 빠졌다");
    assert_eq!(renamed.kind, ChangeKind::Renamed);
    assert_eq!(renamed.from.as_deref(), Some("old.txt"));
    assert_eq!(renamed.status, "R100");

    // 바이너리 변경도 상태 코드로는 평범한 수정이다
    assert_eq!(by_path("blob.bin").unwrap().kind, ChangeKind::Modified);
    // 공백이 든 경로가 쪼개지지 않는다 (`-z`를 쓰는 이유)
    assert_eq!(by_path("added file.txt").unwrap().kind, ChangeKind::Added);
}

/// 알 수 없는 상태 코드라고 목록에서 빼지 않는다 — 빼면 "안 바뀌었다"는 거짓말이 된다.
#[test]
fn an_unknown_status_still_shows_up() {
    let files = parse_name_status("T\0link.txt\0");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].kind, ChangeKind::Other);
    assert_eq!(files[0].status, "T");
}

/// 잘린 출력을 추측으로 메우지 않는다.
#[test]
fn a_truncated_record_is_dropped_not_guessed() {
    assert!(parse_name_status("R100\0old.txt\0").is_empty());
    assert!(parse_name_status("M\0").is_empty());
    assert!(parse_name_status("").is_empty());
}

/// `-z` 형식 가정을 **설치된 git으로** 확인한다. 내 전사(轉寫)가 아니라 실제 출력이 기준이다.
#[test]
fn the_changed_file_list_matches_what_git_reports() {
    use std::process::Command;

    let dir = temp_dir("name-status");
    let git = |args: &[&str]| {
        let status = Command::new("/usr/bin/git")
            .args(args)
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} 실패");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "pilot@example.com"]);
    git(&["config", "user.name", "pilot"]);
    std::fs::write(dir.join("keep.txt"), "a\n").unwrap();
    std::fs::write(dir.join("gone.txt"), "b\n").unwrap();
    std::fs::write(dir.join("old.txt"), "c\n").unwrap();
    std::fs::write(dir.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "init"]);
    let start_commit = String::from_utf8(
        Command::new("/usr/bin/git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&dir)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    // 삭제 · 이름만 변경 · 바이너리 변경 · 새 파일 — 전부 패치 본문으로는 놓치는 것들
    std::fs::remove_file(dir.join("gone.txt")).unwrap();
    git(&["mv", "old.txt", "new.txt"]);
    std::fs::write(dir.join("blob.bin"), [9u8, 9, 9]).unwrap();
    std::fs::write(dir.join("fresh file.txt"), "d\n").unwrap();

    let workspace = PilotWorkspace {
        path: dir.clone(),
        start_commit,
    };
    let files = changed_files(&workspace).expect("변경 목록을 읽지 못했다");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();

    assert!(paths.contains(&"gone.txt"), "삭제가 빠졌다: {paths:?}");
    assert!(paths.contains(&"new.txt"), "이름 변경이 빠졌다: {paths:?}");
    assert!(paths.contains(&"blob.bin"), "바이너리가 빠졌다: {paths:?}");
    assert!(
        paths.contains(&"fresh file.txt"),
        "새 파일이 빠졌다: {paths:?}"
    );

    let renamed = files.iter().find(|f| f.path == "new.txt").unwrap();
    assert_eq!(renamed.kind, ChangeKind::Renamed);
    assert_eq!(renamed.from.as_deref(), Some("old.txt"));
    assert_eq!(
        files.iter().find(|f| f.path == "gone.txt").unwrap().kind,
        ChangeKind::Deleted
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── 계획과 결과의 분리 ─────────────────────────

/// 완료 화면의 "한 일"은 **구현 단계의 최종 메시지**여야 한다.
/// 계획 요약을 결과로 보여 주면 하려던 것을 한 것이라 말하게 된다.
#[test]
fn the_result_summary_comes_from_the_implementation_not_the_plan() {
    let dir = temp_dir("final-message");
    let path = dir.join("last-message.txt");
    std::fs::write(&path, "  greeting.txt의 인사말을 더 친근하게 바꿨습니다.\n").unwrap();

    assert_eq!(
        read_final_message(&path.display().to_string()),
        "greeting.txt의 인사말을 더 친근하게 바꿨습니다."
    );
    // 결과가 없으면 비워 둔다 — 계획 요약으로 몰래 채우지 않는다
    assert_eq!(read_final_message("/nonexistent/last-message.txt"), "");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_very_long_final_message_is_cut_not_dumped() {
    let dir = temp_dir("final-message-long");
    let path = dir.join("last-message.txt");
    std::fs::write(&path, "가".repeat(RESULT_SUMMARY_LIMIT + 500)).unwrap();

    let summary = read_final_message(&path.display().to_string());

    assert_eq!(summary.chars().count(), RESULT_SUMMARY_LIMIT + 1);
    assert!(summary.ends_with('…'));

    let _ = std::fs::remove_dir_all(&dir);
}

/// 멈춘 결과에는 계획도 결과도 없다 — 빈 값을 그럴듯한 요약으로 채우지 않는다.
#[test]
fn a_stopped_run_reports_no_summary_and_needs_attention() {
    let workspace = PilotWorkspace {
        path: std::env::temp_dir().join("ws"),
        start_commit: "abc".into(),
    };
    let result = stopped(
        "목표",
        &workspace,
        "tree could not be confirmed gone".into(),
        WorkspaceLock::SessionOnly,
        Some("cannot persist poisoned marker".into()),
        HandoffEvidence::default(),
    );

    assert_eq!(result.stage, HandoffStage::NeedsAttention);
    assert_eq!(result.workspace_lock, WorkspaceLock::SessionOnly);
    assert!(result.lock_error.is_some());
    assert!(result.plan_summary.is_empty());
    assert!(result.result_summary.is_empty());
    assert!(result.changed_files.is_empty());
    assert_eq!(
        result.attention.as_deref(),
        Some("tree could not be confirmed gone")
    );
}

#[test]
fn empty_diff_is_not_a_change() {
    let dir = temp_dir("empty-diff");
    let empty = dir.join("empty.diff");
    std::fs::write(&empty, "").unwrap();
    assert!(!diff_has_changes(&empty));

    let touched = dir.join("touched.diff");
    std::fs::write(&touched, "diff --git a/x b/x\n").unwrap();
    assert!(diff_has_changes(&touched));

    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── 폐기 경로 ─────────────────────────

/// 폐기 테스트용 실제 파일럿 복제본. `prepare_pilot_copy`가 만든 것이어야
/// 매니페스트 검증을 통과한다 — 임의 폴더를 만들어 흉내 내면 검증을 시험하지 못한다.
struct PilotFixture {
    base: PathBuf,
    root: PathBuf,
    workspace: PilotWorkspace,
}

impl PilotFixture {
    fn new(name: &str) -> Self {
        use std::process::Command;

        let base = temp_dir(name);
        let source = base.join("source");
        let root = base.join("pilots");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(source.join("greeting.txt"), "hello\n").unwrap();

        let git = |args: &[&str]| {
            let status = Command::new("/usr/bin/git")
                .args(args)
                .current_dir(&source)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} 실패");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "pilot@example.com"]);
        git(&["config", "user.name", "pilot"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);

        let workspace =
            orchestration::prepare_pilot_copy(&source, &root).expect("복제본 준비 실패");
        Self {
            root: root.canonicalize().unwrap(),
            base,
            workspace,
        }
    }

    fn discard(&self) -> Result<String, String> {
        discard_handoff_copy(
            &self.workspace.path,
            &self.root,
            &Arc::new(Mutex::new(None)),
            true,
        )
    }

    /// 폐기된 복제본은 **다시는 실행 대상이 되지 못한다.**
    fn is_refused_now(&self) -> bool {
        orchestration::validate_pilot_workspace(&self.workspace.path, &self.root).is_err()
    }

    fn cleanup(&self) {
        clear_session_lock(&self.workspace.path);
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// 정상 완료 결과의 "결과 버리기"가 실제로 동작해야 한다.
/// 잠긴 사본만 다루는 경로를 쓰면 여기서 바로 실패한다.
#[test]
fn a_completed_copy_can_be_discarded() {
    let fixture = PilotFixture::new("discard-completed");

    assert!(!fixture.is_refused_now(), "폐기 전에는 유효한 복제본이다");
    let discarded = fixture.discard().expect("정상 완료 복제본을 버리지 못했다");

    assert_eq!(discarded, fixture.workspace.path.display().to_string());
    assert!(fixture.is_refused_now(), "폐기했는데 다시 쓸 수 있다");
    fixture.cleanup();
}

/// 잠기지 않은 취소 결과도 마찬가지다 — poisoned 표시가 없다.
#[test]
fn a_canceled_but_unlocked_copy_can_be_discarded() {
    let fixture = PilotFixture::new("discard-canceled");
    assert!(
        !orchestration::pilot_is_poisoned(&fixture.workspace.path),
        "이 복제본은 잠기지 않았어야 한다"
    );

    fixture.discard().expect("취소 복제본을 버리지 못했다");

    assert!(fixture.is_refused_now());
    fixture.cleanup();
}

/// 디스크 표시까지 남은(`persisted`) 복제본 폐기.
#[test]
fn a_persisted_locked_copy_can_be_discarded() {
    let fixture = PilotFixture::new("discard-persisted");
    let (lock, error) = lock_workspace(&fixture.workspace.path, "tree could not be confirmed gone");
    assert_eq!(lock, WorkspaceLock::Persisted);
    assert!(error.is_none());

    fixture.discard().expect("잠긴 복제본을 버리지 못했다");

    assert!(fixture.is_refused_now());
    // 세션 잠금은 영구 폐기 표시로 대체됐다
    assert!(
        ensure_reusable(&fixture.workspace.path).is_ok(),
        "폐기 후에도 세션 잠금이 남아 있다"
    );
    fixture.cleanup();
}

/// **표시가 없는 `sessionOnly` 복제본**도 버릴 수 있어야 한다.
/// 디스크 표시를 못 남긴 상태라 잠긴 사본 전용 경로로는 손댈 수 없는 경우다.
#[test]
fn a_session_only_locked_copy_can_be_discarded() {
    let fixture = PilotFixture::new("discard-session-only");
    // 디스크 표시 없이 세션 잠금만 있는 상태를 만든다
    locks().insert(lock_key(&fixture.workspace.path));
    assert!(!orchestration::pilot_is_poisoned(&fixture.workspace.path));
    assert!(ensure_reusable(&fixture.workspace.path).is_err());

    fixture
        .discard()
        .expect("표시 없는 잠긴 복제본을 버리지 못했다");

    // 영구 폐기 표시가 세션 잠금을 대신한다 — 재시작해도 막힌다
    assert!(fixture.is_refused_now(), "폐기했는데 다시 쓸 수 있다");
    assert!(ensure_reusable(&fixture.workspace.path).is_ok());
    fixture.cleanup();
}

/// 확인 없이 버릴 수 없다.
#[test]
fn discarding_without_confirmation_is_refused() {
    let fixture = PilotFixture::new("discard-unconfirmed");

    let refused = discard_handoff_copy(
        &fixture.workspace.path,
        &fixture.root,
        &Arc::new(Mutex::new(None)),
        false,
    );

    assert!(refused.is_err());
    assert!(!fixture.is_refused_now(), "확인 없이 폐기됐다");
    fixture.cleanup();
}

/// **임의 폴더는 폐기 대상이 아니다.** 매니페스트와 파일럿 루트 검증이 살아 있어야 한다.
#[test]
fn an_arbitrary_folder_cannot_be_discarded() {
    let fixture = PilotFixture::new("discard-arbitrary");
    let outsider = fixture.base.join("not-a-pilot");
    std::fs::create_dir_all(&outsider).unwrap();
    let handle = Arc::new(Mutex::new(None));

    // 파일럿 루트 밖
    assert!(discard_handoff_copy(&outsider, &fixture.root, &handle, true).is_err());
    // 루트 자신
    assert!(discard_handoff_copy(&fixture.root, &fixture.root, &handle, true).is_err());
    // 루트 안이지만 매니페스트가 없는 폴더
    let fake_copy = fixture.root.join("looks-like-a-copy");
    std::fs::create_dir_all(&fake_copy).unwrap();
    let refused = discard_handoff_copy(&fake_copy, &fixture.root, &handle, true);
    assert!(refused.is_err(), "매니페스트 없는 폴더가 폐기됐다");
    assert!(refused.unwrap_err().contains("pilot marker"));

    fixture.cleanup();
}

// ───────────────────────── 구현 단계 실패 보고 ─────────────────────────

/// 증거를 만든 **뒤에** 실패하는 실행. 스크립트가 이벤트 한 줄을 낸 뒤 복제본의 `.git`을
/// 지워서, 증거는 남았는데 diff를 뜰 수 없는 상태를 만든다 — 실제 실패 경로 그대로다.
#[cfg(unix)]
fn failing_pilot_program(fixture: &PilotFixture) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let program = fixture.base.join("failing-pilot.sh");
    let script = format!(
        "#!/bin/sh\n\
         echo '{{\"type\":\"thread.started\",\"thread_id\":\"fake-session\"}}'\n\
         rm -rf '{workspace}/.git'\n\
         exit 0\n",
        workspace = fixture.workspace.path.display(),
    );
    std::fs::write(&program, script).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

/// **실제 실행 경로**로 실패를 만들어, 결과에 Codex 증거 경로가 살아남는지 본다.
///
/// `stopped()`를 손으로 조립하면 이 검증이 되지 않는다 — 증거를 잃는 지점은
/// `execute_pilot`의 오류 반환과 그것을 결과로 옮기는 매핑이기 때문이다.
#[cfg(unix)]
#[test]
fn a_failure_after_evidence_exists_keeps_the_codex_evidence_paths() {
    let fixture = PilotFixture::new("codex-evidence-kept");
    let program = failing_pilot_program(&fixture);
    let handle = Arc::new(Mutex::new(None));

    let failure = orchestration::execute_pilot_program(
        &fixture.root,
        &fixture.workspace.path,
        "구현해 줘",
        &handle,
        &program,
    )
    .expect_err("`.git`이 사라졌으므로 실패해야 한다");

    // 실패가 증거를 싣고 왔는가 — 파일도 실제로 존재해야 한다
    let evidence = failure
        .evidence
        .clone()
        .expect("증거 경로가 실패와 함께 사라졌다");
    assert!(failure.run_id.is_some());
    let events = evidence.events.clone().expect("이벤트 증거 경로가 없다");
    let stderr = evidence.stderr.clone().expect("stderr 증거 경로가 없다");
    let last_message = evidence
        .last_message
        .clone()
        .expect("최종 메시지 경로가 없다");
    assert!(Path::new(&events).exists(), "이벤트 증거가 없다: {events}");
    assert!(Path::new(&stderr).exists());
    assert!(Path::new(&last_message).exists());
    // **diff는 만들지 못했다.** 없는 파일의 경로를 "저장된 기록"이라고 말하면 안 된다
    assert_eq!(
        evidence.diff, None,
        "저장되지 않은 diff 경로가 증거로 보고됐다"
    );

    // 그리고 그 증거가 사용자 결과까지 옮겨져야 한다
    let result = implementation_failure(
        "목표",
        &fixture.workspace,
        failure,
        HandoffEvidence {
            claude_events_path: "/pilots/.claude.events.jsonl".into(),
            ..Default::default()
        },
        &handle,
    );

    assert_eq!(result.stage, HandoffStage::NeedsAttention);
    assert_eq!(
        result.evidence.codex_events_path, events,
        "Codex 증거 경로가 결과에서 사라졌다"
    );
    assert_eq!(result.evidence.codex_stderr_path, stderr);
    assert_eq!(result.evidence.codex_last_message_path, last_message);
    assert!(
        result.evidence.codex_diff_path.is_empty(),
        "저장되지 않은 diff가 화면 증거로 노출된다: {}",
        result.evidence.codex_diff_path
    );
    // 앞 단계의 증거도 그대로 남는다
    assert!(!result.evidence.claude_events_path.is_empty());
    // 폐기할 대상을 알 수 있어야 한다
    assert!(!result.workspace_path.is_empty());
    assert!(result.attention.is_some());
    // 복제본을 건드리기 전에 실패한 것이 아니므로 잠금 판정도 확인한다
    assert_eq!(
        result.workspace_lock,
        WorkspaceLock::None,
        "종료 실패가 아닌데 잠갔다: {:?}",
        result.attention
    );

    clear_session_lock(&fixture.workspace.path);
    fixture.cleanup();
}

/// 복제본을 건드리기 전에 실패했다면 **잠그지 않는다** — 멀쩡한 사본을 버리게 만들면 안 된다.
#[test]
fn a_failure_before_the_clone_is_touched_does_not_lock_it() {
    let fixture = PilotFixture::new("codex-failure-clean");
    let failure = orchestration::PilotFailure::plain("codex is not installed");

    let (lock, error) = codex_failure_lock(
        &fixture.workspace.path,
        &failure,
        &Arc::new(Mutex::new(None)),
    );

    assert_eq!(lock, WorkspaceLock::None, "멀쩡한 복제본을 잠갔다");
    assert!(error.is_none());
    assert!(ensure_reusable(&fixture.workspace.path).is_ok());
    fixture.cleanup();
}

/// 디스크 표시가 남은 오염은 `persisted`로 보고하고 세션에서도 막는다.
#[test]
fn a_poisoned_clone_is_reported_as_persisted_after_a_failed_implementation() {
    let fixture = PilotFixture::new("codex-failure-poisoned");
    orchestration::write_poisoned_marker(&fixture.workspace.path, None, "tree survived").unwrap();
    let failure = orchestration::PilotFailure::plain("could not start another run");

    let (lock, error) = codex_failure_lock(
        &fixture.workspace.path,
        &failure,
        &Arc::new(Mutex::new(None)),
    );

    assert_eq!(lock, WorkspaceLock::Persisted);
    assert!(error.is_none());
    assert!(ensure_reusable(&fixture.workspace.path).is_err());
    fixture.cleanup();
}

/// **종료 오류와 표시 저장 오류는 다른 사실이다.**
/// 섞으면 화면의 "표시를 저장하지 못한 이유"에 종료 오류가 나타난다.
#[test]
fn the_termination_error_and_the_marker_error_land_in_different_fields() {
    let fixture = PilotFixture::new("codex-two-errors");
    let handle = Arc::new(Mutex::new(None));
    // 서로 다른 두 문자열을 주입한다
    let failure = orchestration::PilotFailure {
        error: "the pilot tree survived both TERM and KILL".into(),
        run_id: Some("run-1".into()),
        workspace_path: Some(fixture.workspace.path.clone()),
        evidence: Some(orchestration::PilotEvidencePaths {
            events: Some("/pilots/.run.events.jsonl".into()),
            last_message: Some("/pilots/.run.last-message.txt".into()),
            stderr: Some("/pilots/.run.stderr.log".into()),
            diff: None,
        }),
        poisoned: Some("the pilot tree survived both TERM and KILL".into()),
        marker_error: Some("cannot persist poisoned marker: Read-only file system".into()),
        blocker: None,
    };

    let result = implementation_failure(
        "목표",
        &fixture.workspace,
        failure,
        HandoffEvidence::default(),
        &handle,
    );

    // 무엇이 잘못됐나 → attention
    assert_eq!(
        result.attention.as_deref(),
        Some("the pilot tree survived both TERM and KILL")
    );
    // 표시를 왜 저장하지 못했나 → lockError
    assert_eq!(
        result.lock_error.as_deref(),
        Some("cannot persist poisoned marker: Read-only file system"),
        "표시 저장 실패 자리에 종료 오류가 들어갔다"
    );
    assert_eq!(result.workspace_lock, WorkspaceLock::SessionOnly);
    assert!(ensure_reusable(&fixture.workspace.path).is_err());

    clear_session_lock(&fixture.workspace.path);
    fixture.cleanup();
}

/// 표시까지 남았다면 `persisted`이고 `lockError`는 비어 있어야 한다.
#[test]
fn a_poisoned_run_that_saved_its_marker_reports_no_lock_error() {
    let fixture = PilotFixture::new("codex-poisoned-saved");
    let failure = orchestration::PilotFailure {
        poisoned: Some("the pilot tree survived both TERM and KILL".into()),
        marker_error: None,
        ..orchestration::PilotFailure::plain("the pilot tree survived both TERM and KILL")
    };

    let result = implementation_failure(
        "목표",
        &fixture.workspace,
        failure,
        HandoffEvidence::default(),
        &Arc::new(Mutex::new(None)),
    );

    assert_eq!(result.workspace_lock, WorkspaceLock::Persisted);
    assert!(result.lock_error.is_none());

    clear_session_lock(&fixture.workspace.path);
    fixture.cleanup();
}

/// **다른 복제본의 오염을 이 복제본의 것으로 오판하면 안 된다.**
///
/// 슬롯은 하나뿐이다. A가 표시 없이 오염된 상태에서 깨끗한 B가 슬롯 예약에 실패하면,
/// 경로를 대조하지 않는 구현은 B를 잠근다. 그리고 B를 폐기해도 슬롯은 A의 것이라 풀리지 않아
/// **실행이 영영 막힌다.**
#[test]
fn another_workspaces_poisoned_slot_does_not_lock_this_one() {
    let a = PilotFixture::new("slot-owner-a");
    let b = PilotFixture::new("slot-victim-b");
    let handle = Arc::new(Mutex::new(None));
    // A가 표시를 남기지 못한 채 오염됐다
    orchestration::poison_slot_for_test(&handle, &a.workspace.path, "A tree survived", None);
    // B는 슬롯이 잠겨 있어 실행을 시작조차 못 했다
    let failure =
        orchestration::PilotFailure::plain("pilot slot requires recovery before another run");

    let (lock, error) = codex_failure_lock(&b.workspace.path, &failure, &handle);

    assert_eq!(lock, WorkspaceLock::None, "남의 오염으로 B가 잠겼다");
    assert!(error.is_none());
    assert!(
        ensure_reusable(&b.workspace.path).is_ok(),
        "B는 멀쩡한데 재사용이 막혔다"
    );
    // A의 문제는 A의 문제로 남아 있어야 한다
    let (poisoned_workspace, reason, _) =
        orchestration::poisoned_slot(&handle).expect("A의 오염이 사라졌다");
    assert_eq!(lock_key(&poisoned_workspace), lock_key(&a.workspace.path));
    assert_eq!(reason, "A tree survived");

    a.cleanup();
    b.cleanup();
}

/// 같은 복제본이라면 슬롯의 오염을 그대로 반영한다 — 경로 대조가 과잉이 되면 안 된다.
#[test]
fn this_workspaces_poisoned_slot_is_reported_as_session_only() {
    let fixture = PilotFixture::new("slot-same-workspace");
    let handle = Arc::new(Mutex::new(None));
    orchestration::poison_slot_for_test(
        &handle,
        &fixture.workspace.path,
        "tree survived",
        Some("cannot persist poisoned marker: Read-only file system"),
    );
    assert!(
        !orchestration::pilot_is_poisoned(&fixture.workspace.path),
        "이 재현은 디스크 표시가 **없는** 상태여야 한다"
    );
    let failure =
        orchestration::PilotFailure::plain("pilot slot requires recovery before another run");

    let (lock, error) = codex_failure_lock(&fixture.workspace.path, &failure, &handle);

    assert_eq!(lock, WorkspaceLock::SessionOnly);
    // **종료 오류가 아니라 표시 저장 실패 오류가 들어가야 한다**
    assert_eq!(
        error.as_deref(),
        Some("cannot persist poisoned marker: Read-only file system"),
        "슬롯만 보고 판단할 때도 종료 오류가 lockError로 새면 안 된다"
    );
    assert!(ensure_reusable(&fixture.workspace.path).is_err());

    clear_session_lock(&fixture.workspace.path);
    fixture.cleanup();
}

// ───────────────────────── 막힌 복제본에서 빠져나오기 ─────────────────────────

/// **잠긴 복제본 A가 있으면 새 실행을 시작조차 하지 않는다.**
///
/// 막지 않으면 이렇게 된다 — A가 슬롯을 쥔 채 새 B가 만들어지고, B의 실행은 거부되고,
/// 화면에는 B만 보인다. B를 정리해도 슬롯은 A의 것이라 풀리지 않아 **모든 후속 실행이
/// 영영 거부된다.** 사용자는 무엇을 치워야 하는지 알 수도 없다.
///
/// 그래서 이 테스트는 "B가 잠기지 않는다"가 아니라 **막힌 상태에서 실제로 빠져나오는
/// 전체 흐름**을 확인한다: 막힘 알림 → 그 안의 경로로 A 정리 → 다음 실행 가능.
#[test]
fn a_locked_workspace_blocks_new_runs_until_it_is_discarded() {
    let a = PilotFixture::new("blocker-discard-a");
    let pilot = Arc::new(Mutex::new(None));
    let session = HandoffSession::default();
    // A가 표시를 남기지 못한 채 오염됐다 (sessionOnly)
    orchestration::poison_slot_for_test(
        &pilot,
        &a.workspace.path,
        "the pilot tree survived both TERM and KILL",
        Some("cannot persist poisoned marker: Read-only file system"),
    );

    // ① 새 실행을 시작하려 하면 **사본을 만들기 전에** 막힌다
    let blocker = match start_handoff(&session, &pilot).expect("시작 판정이 실패했다") {
        HandoffStart::Blocked { blocker } => blocker,
        HandoffStart::Ready { .. } => panic!("잠긴 복제본이 있는데 새 실행이 시작됐다"),
    };
    // 슬롯을 잡아먹지 않았어야 한다 — A를 치운 뒤 바로 시작할 수 있어야 하므로
    let probe = session.reserve().expect("막힘 판정이 슬롯을 붙잡았다");
    session.release(&probe);

    // ② 알림이 **A의 경로**를 담고 있어야 한다. 이게 없으면 사용자는 손댈 대상이 없다
    assert_eq!(
        lock_key(Path::new(&blocker.workspace_path)),
        lock_key(&a.workspace.path),
        "막고 있는 복제본이 A가 아니다"
    );
    assert!(blocker.reason.contains("survived both TERM and KILL"));
    assert!(!blocker.persisted, "이 재현은 디스크 표시가 없는 상태다");
    assert_eq!(
        blocker.marker_error.as_deref(),
        Some("cannot persist poisoned marker: Read-only file system"),
        "표시 저장 실패 자리에 종료 오류가 들어갔다"
    );

    // ③ 알림에 담긴 경로로 **실제 A를** 정리한다
    discard_handoff_copy(Path::new(&blocker.workspace_path), &a.root, &pilot, true)
        .expect("막고 있는 복제본을 정리하지 못했다");

    // ④ 그러고 나면 다음 실행이 가능해야 한다
    assert!(
        orchestration::blocking_workspace(&pilot).is_none(),
        "정리했는데 슬롯이 여전히 잠겨 있다"
    );
    match start_handoff(&session, &pilot).expect("시작 판정이 실패했다") {
        HandoffStart::Ready { run_id } => session.release(&run_id),
        HandoffStart::Blocked { blocker } => {
            panic!("정리한 뒤에도 막혀 있다: {}", blocker.workspace_path)
        }
    }
    // A는 영구 폐기됐으므로 다시 실행 대상이 되지 못한다
    assert!(a.is_refused_now());

    a.cleanup();
}

/// 디스크 표시가 남은 잠금은 **복구**로도 빠져나올 수 있어야 한다.
#[test]
fn a_locked_workspace_can_also_be_recovered_when_the_marker_survived() {
    let a = PilotFixture::new("blocker-recover-a");
    let pilot = Arc::new(Mutex::new(None));
    let session = HandoffSession::default();
    // 표시까지 남은 오염 (persisted). pid를 모르면 복구 판정이 `Unknown`이 되므로 넣는다
    orchestration::write_poisoned_marker(&a.workspace.path, Some(std::process::id()), "tree check")
        .unwrap();
    orchestration::poison_slot_for_test(&pilot, &a.workspace.path, "tree check", None);

    let blocker = match start_handoff(&session, &pilot).unwrap() {
        HandoffStart::Blocked { blocker } => blocker,
        HandoffStart::Ready { .. } => panic!("잠긴 복제본이 있는데 시작됐다"),
    };
    assert!(
        blocker.persisted,
        "디스크 표시가 있으면 복구를 제안할 수 있다"
    );
    assert!(blocker.marker_error.is_none());

    a.cleanup();
}

/// 막힌 게 없으면 그대로 시작된다 — 막힘 판정이 과잉이 되면 안 된다.
#[test]
fn nothing_blocks_a_clean_start() {
    let pilot = Arc::new(Mutex::new(None));
    let session = HandoffSession::default();

    match start_handoff(&session, &pilot).unwrap() {
        HandoffStart::Ready { run_id } => {
            assert!(!run_id.is_empty());
            session.release(&run_id);
        }
        HandoffStart::Blocked { blocker } => panic!("막힌 게 없는데 막혔다: {blocker:?}"),
    }
}

/// 실행 실패 결과에도 **막고 있는 복제본**이 실려야 한다.
/// 시작 판정을 우회해 들어온 경우에도 사용자가 A에 손댈 수 있어야 하기 때문이다.
#[test]
fn a_blocked_run_result_carries_the_blocking_workspace() {
    let a = PilotFixture::new("blocker-result-a");
    let b = PilotFixture::new("blocker-result-b");
    let pilot = Arc::new(Mutex::new(None));
    orchestration::poison_slot_for_test(&pilot, &a.workspace.path, "A tree survived", None);

    // B를 실행하려 하면 슬롯 예약에서 거부된다
    let failure = orchestration::execute_pilot_program(
        &b.root,
        &b.workspace.path,
        "구현해 줘",
        &pilot,
        Path::new("/bin/echo"),
    )
    .expect_err("잠긴 슬롯에서 실행이 성공했다");

    let blocker = failure.blocker.clone().expect("막고 있는 복제본을 잃었다");
    assert_eq!(
        lock_key(Path::new(&blocker.workspace_path)),
        lock_key(&a.workspace.path)
    );

    let result = implementation_failure(
        "목표",
        &b.workspace,
        failure,
        HandoffEvidence::default(),
        &pilot,
    );

    // B는 멀쩡하므로 잠기지 않는다
    assert_eq!(result.workspace_lock, WorkspaceLock::None);
    // 그런데 화면은 **A를** 정리할 수 있어야 한다
    let shown = result.blocker.expect("결과에 막고 있는 복제본이 없다");
    assert_eq!(
        lock_key(Path::new(&shown.workspace_path)),
        lock_key(&a.workspace.path)
    );

    a.cleanup();
    b.cleanup();
}

// ───────────────────────── 증거를 하나도 만들지 못한 경우 ─────────────────────────

/// 증거를 **하나도** 만들지 못했을 때 없는 경로를 보고하면 안 된다.
///
/// 증거 생성 도중의 부분 실패(events 성공 → stderr 실패)는 `orchestration`의
/// `partial_evidence_creation_keeps_the_path_of_what_was_created`가 덮는다.
#[cfg(unix)]
#[test]
fn a_run_that_stored_nothing_reports_no_evidence_paths() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = PilotFixture::new("evidence-none");
    let handle = Arc::new(Mutex::new(None));
    // 증거는 복제본 **밖**(파일럿 루트)에 쓰인다. 루트를 읽기 전용으로 만들어 전부 막는다
    std::fs::set_permissions(&fixture.root, std::fs::Permissions::from_mode(0o500)).unwrap();

    let failure = orchestration::execute_pilot_program(
        &fixture.root,
        &fixture.workspace.path,
        "구현해 줘",
        &handle,
        Path::new("/bin/echo"),
    )
    .expect_err("증거를 만들 수 없으면 실패해야 한다");

    std::fs::set_permissions(&fixture.root, std::fs::Permissions::from_mode(0o700)).unwrap();

    // 맨몸 오류로 축약되지 않았는가 — 실행 식별자와 복제본을 알아야 한다
    assert!(
        failure.run_id.is_some(),
        "증거 생성 실패가 실행 식별자를 잃었다: {}",
        failure.error
    );
    assert_eq!(
        failure.workspace_path.as_deref(),
        Some(fixture.workspace.path.as_path())
    );
    // 하나도 만들지 못했으므로 **저장된 증거는 없다**
    let evidence = failure.evidence.clone().expect("증거 상태가 없다");
    assert_eq!(evidence.events, None, "만들지 못한 파일이 보고됐다");
    assert_eq!(evidence.stderr, None);
    assert_eq!(evidence.last_message, None);
    assert_eq!(evidence.diff, None);

    // 그리고 화면에도 빈 경로가 노출되지 않아야 한다
    let result = implementation_failure(
        "목표",
        &fixture.workspace,
        failure,
        HandoffEvidence::default(),
        &handle,
    );
    assert!(result.evidence.codex_events_path.is_empty());
    assert!(result.evidence.codex_diff_path.is_empty());
    assert!(
        !result.evidence.codex_run_id.is_empty(),
        "실행 번호는 남아야 한다"
    );

    // 고아 파일이 남지 않았는가
    let leftovers: Vec<String> = std::fs::read_dir(&fixture.root)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.contains("events") || name.contains("stderr"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "고아 증거 파일이 남았다: {leftovers:?}"
    );

    fixture.cleanup();
}

// ───────────────────────── 실사용 검증 ─────────────────────────

/// 계획 역할의 **실제 실행**을 확인한다. Claude 로그인과 사용량을 쓰므로 기본 실행에서 제외한다.
///
/// 확인하는 것: `system:init` fail-closed 검증 통과 · 구조화 계획 산출 ·
/// **복제본 diff가 비어 있음**(읽기 전용 확인) · 종료 시 트리 소멸 확인.
///
/// ⚠️ **환경을 비우고(`env -i`) 돌리면 인증이 사라져 `Not logged in`으로 실패한다.**
/// 로그인된 셸 환경 그대로 실행할 것.
///
/// ```bash
/// cargo test --manifest-path src-tauri/Cargo.toml claude_planning_leg -- --ignored --nocapture
/// ```
#[cfg(target_os = "macos")]
#[test]
#[ignore = "claude 로그인 + 사용량 소모"]
fn claude_planning_leg_runs_end_to_end() {
    use std::process::Command;

    let base = std::env::temp_dir().join(format!("hello-agent-e2e-{}", std::process::id()));
    let source = base.join("source");
    let root = base.join("pilots");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(source.join("greeting.txt"), "hello\n").unwrap();

    let git = |args: &[&str]| {
        let status = Command::new("/usr/bin/git")
            .args(args)
            .current_dir(&source)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} 실패");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "pilot@example.com"]);
    git(&["config", "user.name", "pilot"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);

    let workspace = orchestration::prepare_pilot_copy(&source, &root).expect("복제본 준비 실패");
    let root = root.canonicalize().unwrap();
    let evidence = ClaudeEvidence::new(&root, &workspace.path, "e2e").unwrap();
    let cancel = AtomicBool::new(false);

    let run = match run_claude_plan(
        &workspace,
        "인사말을 더 친근하게 바꿔 줘",
        &evidence,
        &cancel,
    ) {
        Ok(run) => run,
        Err(ClaudeFailure::Poisoned(reason)) => panic!("복제본이 잠겼다: {reason}"),
        Err(ClaudeFailure::Failed(reason)) => panic!("계획 실행 실패: {reason}"),
    };

    println!("session    : {}", run.facts.session_id);
    println!("cli        : {}", run.facts.cli_version);
    println!("model      : {}", run.facts.model);
    println!("tools      : {:?}", run.facts.tools);
    println!("summary    : {}", run.plan.summary);
    println!("steps      : {}", run.plan.steps.len());
    println!("termination: {}", run.termination);

    // 계약대로 도구는 정확히 4개여야 한다
    assert_eq!(run.facts.tools.len(), 4, "{:?}", run.facts.tools);
    assert_eq!(run.facts.permission_mode, "dontAsk");
    // 모델은 실측된 허용값 목록 그대로여야 한다 (부분 문자열 대조가 아니다)
    assert!(
        claude_pilot::ALLOWED_INIT_MODELS.contains(&run.facts.model.as_str()),
        "실측 허용값과 다른 모델이다: {}",
        run.facts.model
    );
    // 계획이 실제로 나왔는가
    assert!(!run.plan.summary.is_empty(), "계획 요약이 비었다");
    assert!(!run.plan.steps.is_empty(), "계획 단계가 비었다");
    assert!(!run.plan.is_error, "실패로 끝난 계획이다");
    // 계획 역할은 아무것도 바꾸지 않았어야 한다 (사후 확인)
    assert!(
        !diff_has_changes(&run.diff_path),
        "계획 역할이 복제본을 수정했다"
    );
    // 증거는 복제본 밖에 남는다
    for path in [&run.events_path, &run.diff_path] {
        assert!(path.exists(), "증거가 없다: {}", path.display());
        assert!(
            !path.starts_with(&workspace.path),
            "증거가 복제본 안에 있다: {}",
            path.display()
        );
    }
    // 인계 프롬프트가 만들어지는가
    let prompt = claude_pilot::plan_to_codex_prompt("인사말을 더 친근하게 바꿔 줘", &run.plan)
        .expect("인계가 막혔다");
    assert!(prompt.contains("Steps:"));

    let _ = std::fs::remove_dir_all(&base);
}

/// 수직 흐름 **전체**(요청 정리 → 자동 인계 → 구현 → 확인)를 한 번 돌린다.
/// Claude·Codex 로그인과 사용량을 모두 쓰므로 기본 실행에서 제외한다.
///
/// 확인하는 것: 사용자가 **아무것도 옮기지 않고**(§5.2 가설 A) 결과를 받는가 ·
/// 진행 상태가 일의 이름으로 순서대로 오는가 · **결과 요약이 계획 요약이 아닌가** ·
/// 증거가 복제본 밖에 남는가.
///
/// ```bash
/// cargo test --manifest-path src-tauri/Cargo.toml handoff_vertical_slice -- --ignored --nocapture
/// ```
#[cfg(target_os = "macos")]
#[test]
#[ignore = "claude + codex 로그인, 사용량 소모"]
fn handoff_vertical_slice_runs_end_to_end() {
    use std::process::Command;
    use std::sync::Mutex as StdMutex;

    let base = std::env::temp_dir().join(format!("hello-agent-slice-{}", std::process::id()));
    let source = base.join("source");
    let root = base.join("pilots");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(source.join("greeting.txt"), "hello\n").unwrap();

    let git = |args: &[&str]| {
        let status = Command::new("/usr/bin/git")
            .args(args)
            .current_dir(&source)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} 실패");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "pilot@example.com"]);
    git(&["config", "user.name", "pilot"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);

    let workspace = orchestration::prepare_pilot_copy(&source, &root).expect("복제본 준비 실패");
    let stages: StdMutex<Vec<HandoffStage>> = StdMutex::new(Vec::new());
    let announce = |stage: HandoffStage| stages.lock().unwrap().push(stage);
    let cancel = Arc::new(AtomicBool::new(false));
    let pilot = Arc::new(Mutex::new(None));

    let result = execute_handoff(
        &announce,
        &root,
        &workspace.path,
        "greeting.txt의 인사말을 더 친근한 문장으로 바꿔 줘",
        &cancel,
        &pilot,
    )
    .expect("자동 인계 실패");

    println!("stage      : {:?}", result.stage);
    println!("plan       : {}", result.plan_summary);
    println!("result     : {}", result.result_summary);
    println!("changed    : {:?}", result.changed_files);
    println!("attention  : {:?}", result.attention);
    println!("termination: {}", result.evidence.termination);
    println!("stages     : {:?}", stages.lock().unwrap());

    // 사용자는 목표를 한 번 말했을 뿐이다
    assert!(result.goal.contains("인사말"));
    // 진행 상태가 일의 순서대로 왔는가 — 확인 단계는 화면에만 있는 것이 아니라 실제 단계다
    let seen = stages.lock().unwrap().clone();
    assert_eq!(seen.first(), Some(&HandoffStage::Organizing));
    assert!(seen.contains(&HandoffStage::Building), "{seen:?}");
    assert!(seen.contains(&HandoffStage::Checking), "{seen:?}");
    // 잠기지 않았어야 한다
    assert_eq!(
        result.workspace_lock,
        WorkspaceLock::None,
        "{:?}",
        result.attention
    );
    assert!(result.lock_error.is_none());
    assert_eq!(result.stage, HandoffStage::Done, "{:?}", result.attention);
    // 구현 단계가 실제로 파일을 바꿨는가
    assert!(!result.changed_files.is_empty(), "바뀐 파일이 없다");
    // **결과는 계획이 아니다** — 완료 화면이 "하려던 것"을 "한 것"이라 말하면 안 된다
    assert!(!result.plan_summary.is_empty(), "계획 요약이 비었다");
    assert!(!result.result_summary.is_empty(), "실제 결과 요약이 비었다");
    assert_ne!(
        result.plan_summary, result.result_summary,
        "결과 자리에 계획 요약이 그대로 들어갔다"
    );
    // 증거는 복제본 밖에 있어야 한다
    for path in [
        &result.evidence.claude_events_path,
        &result.evidence.codex_events_path,
        &result.evidence.codex_diff_path,
    ] {
        assert!(!path.is_empty());
        assert!(
            !Path::new(path).starts_with(&workspace.path),
            "증거가 복제본 안에 있다: {path}"
        );
    }

    let _ = std::fs::remove_dir_all(&base);
}
