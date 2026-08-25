//! 종료 계약 회귀 픽스처 **F1~F6** (`docs/proposals/m4-orchestration.md` §7.10.1.2 ⑤).
//!
//! 전부 통과해야 ⑤가 구현됐다고 본다. 특히 **F1(재부모화 고아)** 은 이 계약이 존재하는 이유이므로
//! 여기서 빠지면 안 된다 — 루트 기준 트리 순회만 하는 구현은 F1에서 빈 집합 오판으로 실패한다.

use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Debug)]
struct FakeProc {
    identity: ProcIdentity,
    ppid: u32,
    comm: String,
    alive: bool,
    /// TERM을 무시한다 (KILL로만 죽는다)
    ignores_term: bool,
}

/// 회차마다 적용되는 시나리오 동작.
#[derive(Clone, Debug)]
enum Step {
    Spawn {
        pid: u32,
        ppid: u32,
        start_us: i64,
        comm: &'static str,
        ignores_term: bool,
    },
    Exit {
        pid: u32,
    },
    Reparent {
        pid: u32,
        ppid: u32,
    },
    /// pid 재사용: 같은 pid가 **다른 시작 시각**으로 존재하게 만든다
    Reuse {
        pid: u32,
        start_us: i64,
    },
}

#[derive(Default)]
struct World {
    procs: Vec<FakeProc>,
    tick: u32,
    signals: Vec<(u32, TreeSignal)>,
    /// `(pid, 오류)` — 해당 pid 조회를 강제로 실패시킨다
    probe_error: Option<(u32, String)>,
    list_error: Option<String>,
    /// 모든 신호 전달을 실패시킨다 (예: `EPERM`)
    signal_error: Option<String>,
    script: Vec<(u32, Step)>,
}

impl World {
    fn spawn_root(pid: u32, start_us: i64, ignores_term: bool) -> Self {
        Self {
            procs: vec![FakeProc {
                identity: ProcIdentity { pid, start_us },
                ppid: 1,
                comm: "root".into(),
                alive: true,
                ignores_term,
            }],
            ..Default::default()
        }
    }

    fn advance(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        let steps: Vec<Step> = self
            .script
            .iter()
            .filter(|(t, _)| *t == tick)
            .map(|(_, s)| s.clone())
            .collect();
        for step in steps {
            self.apply(step);
        }
    }

    fn apply(&mut self, step: Step) {
        match step {
            Step::Spawn {
                pid,
                ppid,
                start_us,
                comm,
                ignores_term,
            } => self.procs.push(FakeProc {
                identity: ProcIdentity { pid, start_us },
                ppid,
                comm: comm.into(),
                alive: true,
                ignores_term,
            }),
            Step::Exit { pid } => {
                if let Some(p) = self
                    .procs
                    .iter_mut()
                    .find(|p| p.alive && p.identity.pid == pid)
                {
                    p.alive = false;
                }
            }
            Step::Reparent { pid, ppid } => {
                if let Some(p) = self
                    .procs
                    .iter_mut()
                    .find(|p| p.alive && p.identity.pid == pid)
                {
                    p.ppid = ppid;
                }
            }
            Step::Reuse { pid, start_us } => {
                self.procs.retain(|p| p.identity.pid != pid);
                self.procs.push(FakeProc {
                    identity: ProcIdentity { pid, start_us },
                    ppid: 1,
                    comm: "recycled".into(),
                    alive: true,
                    ignores_term: true,
                });
            }
        }
    }

    fn live(&self) -> Vec<&FakeProc> {
        self.procs.iter().filter(|p| p.alive).collect()
    }
}

#[derive(Clone)]
struct FakeTable(Rc<RefCell<World>>);

impl ProcTable for FakeTable {
    fn list(&self) -> Result<Vec<ProcEntry>, String> {
        let world = self.0.borrow();
        if let Some(err) = &world.list_error {
            return Err(err.clone());
        }
        Ok(world
            .live()
            .into_iter()
            .map(|p| ProcEntry {
                identity: p.identity,
                ppid: p.ppid,
                comm: p.comm.clone(),
            })
            .collect())
    }

    fn probe(&self, identity: ProcIdentity) -> ProbeResult {
        let world = self.0.borrow();
        if let Some((pid, err)) = &world.probe_error {
            if *pid == identity.pid {
                return ProbeResult::Unconfirmable(err.clone());
            }
        }
        match world
            .live()
            .into_iter()
            .find(|p| p.identity.pid == identity.pid)
        {
            None => ProbeResult::Gone,
            Some(p) if p.identity.start_us == identity.start_us => ProbeResult::Alive(ProcEntry {
                identity: p.identity,
                ppid: p.ppid,
                comm: p.comm.clone(),
            }),
            Some(p) => ProbeResult::Unconfirmable(format!(
                "identity mismatch on pid {} (expected start {}us, found {}us)",
                identity.pid, identity.start_us, p.identity.start_us
            )),
        }
    }

    fn signal(&self, pid: u32, signal: TreeSignal) -> Result<(), String> {
        let mut world = self.0.borrow_mut();
        world.signals.push((pid, signal));
        if let Some(err) = &world.signal_error {
            return Err(err.clone());
        }
        // **살아 있는** 프로세스를 찾는다. pid만으로 첫 항목을 잡으면 같은 pid를 쓰던
        // 죽은 프로세스에 신호가 가고, 실제 대상은 멀쩡히 살아남는다.
        if let Some(p) = world
            .procs
            .iter_mut()
            .find(|p| p.alive && p.identity.pid == pid)
        {
            let dies = match signal {
                TreeSignal::Term => !p.ignores_term,
                TreeSignal::Kill => true,
            };
            if dies {
                p.alive = false;
            }
        }
        Ok(())
    }
}

/// 잠들 때마다 시나리오를 한 칸 진행시킨다. 실시간을 기다리지 않는다.
struct FakeClock {
    world: Rc<RefCell<World>>,
    ms: Cell<u64>,
}

impl Clock for FakeClock {
    fn elapsed_ms(&self) -> u64 {
        self.ms.get()
    }
    fn sleep(&self, duration: Duration) {
        self.ms.set(self.ms.get() + duration.as_millis() as u64);
        self.world.borrow_mut().advance();
    }
}

fn config() -> TerminationConfig {
    TerminationConfig {
        term_grace_ms: 300,
        kill_grace_ms: 300,
        poll_interval_ms: 100,
    }
}

fn harness(world: World) -> (FakeTable, FakeClock) {
    let shared = Rc::new(RefCell::new(world));
    (
        FakeTable(shared.clone()),
        FakeClock {
            world: shared,
            ms: Cell::new(0),
        },
    )
}

fn signaled(table: &FakeTable, pid: u32) -> bool {
    table.0.borrow().signals.iter().any(|(p, _)| *p == pid)
}

// ───────────────────────── F1 재부모화 고아 ─────────────────────────

/// **F1** — 유예 중 자손이 생기고, **부모가 먼저 종료**되어 자손만 `ppid=1`로 살아남는다.
/// 누적 신원 추적이 그 자손을 잡아 회수해야 한다.
#[test]
fn f1_reparented_orphan_is_still_terminated() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.script = vec![
        // 유예 중 자손 생성 (부모는 아직 살아 있다)
        (
            1,
            Step::Spawn {
                pid: 200,
                ppid: 100,
                start_us: 2_000,
                comm: "orphan-to-be",
                ignores_term: true,
            },
        ),
        // 부모가 먼저 죽고 자손은 launchd로 재부모화된다
        (3, Step::Exit { pid: 100 }),
        (3, Step::Reparent { pid: 200, ppid: 1 }),
    ];
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert!(
        report.confirmed(),
        "재부모화된 고아를 회수하지 못했다: {report:?}"
    );
    assert_eq!(report.tracked.len(), 2, "고아가 누적 집합에 없다");
    assert!(
        signaled(&table, 200),
        "고아에게 신호가 직접 가지 않았다 — 그룹에 의존하고 있다"
    );
    assert!(
        table.0.borrow().live().is_empty(),
        "살아남은 프로세스가 있다"
    );
}

/// **F1의 반증** — 같은 상황에서 *루트 기준 트리 순회*는 빈 집합을 돌려준다.
/// 이 테스트가 §7.10.1.2 ⑤의 C6 관측을 코드로 고정한다.
#[test]
fn f1_root_tree_walk_would_report_empty_while_orphan_lives() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.apply(Step::Spawn {
        pid: 200,
        ppid: 100,
        start_us: 2_000,
        comm: "orphan",
        ignores_term: true,
    });
    world.apply(Step::Exit { pid: 100 });
    world.apply(Step::Reparent { pid: 200, ppid: 1 });
    let (table, _clock) = harness(world);

    let snapshot = table.list().unwrap();
    let from_root = descendants_of(&snapshot, &[100]);

    assert!(
        from_root.is_empty(),
        "루트 기준 순회가 비어 있어야 오판이 재현된다"
    );
    assert!(
        snapshot.iter().any(|e| e.identity.pid == 200),
        "고아는 실제로 살아 있어야 한다"
    );
}

// ───────────────────── F2 유예 중 신규 자손 ─────────────────────

/// **F2** — TERM 이후 유예 창 안에서 새 자손이 생긴다. 회차마다 다시 열거해 편입해야 한다.
#[test]
fn f2_descendant_spawned_during_grace_is_tracked() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.script = vec![(
        2,
        Step::Spawn {
            pid: 300,
            ppid: 100,
            start_us: 3_000,
            comm: "late-child",
            ignores_term: true,
        },
    )];
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert!(report.confirmed(), "{report:?}");
    assert_eq!(report.tracked.len(), 2, "유예 중 생긴 자손을 놓쳤다");
    assert!(signaled(&table, 300));
}

// ────────────── F3 별도 프로세스 그룹·세션 자손 ──────────────

/// **F3** — 실제 Claude 배치처럼 자손이 부모와 다른 그룹·세션에 있다.
/// 구현이 그룹 신호에 의존하지 않고 **신원별로** 신호를 보내야 통과한다.
#[test]
fn f3_descendant_in_separate_group_is_signaled_individually() {
    let mut world = World::spawn_root(100, 1_000, false);
    // 별도 세션 리더로 뜬 도구 자식과 그 손자
    world.apply(Step::Spawn {
        pid: 400,
        ppid: 100,
        start_us: 4_000,
        comm: "tool-shell",
        ignores_term: true,
    });
    world.apply(Step::Spawn {
        pid: 401,
        ppid: 400,
        start_us: 4_100,
        comm: "tail",
        ignores_term: true,
    });
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert!(report.confirmed(), "{report:?}");
    assert_eq!(report.tracked.len(), 3);
    for pid in [400, 401] {
        assert!(signaled(&table, pid), "pid {pid}에 직접 신호가 가지 않았다");
    }
}

// ─────────────────── F4 EPERM(좀비만 남은 그룹) ───────────────────

/// **F4** — 신호가 계속 `EPERM`을 돌려준다. **성공도 실패도 아니며 판정에 쓰지 않는다.**
/// 소멸은 오직 재열거로 확인한다.
#[test]
fn f4_eperm_signal_errors_do_not_decide_the_verdict() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.signal_error = Some("Operation not permitted (os error 1)".into());
    // 신호와 무관하게 프로세스는 스스로 끝난다
    world.script = vec![(1, Step::Exit { pid: 100 })];
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert!(report.confirmed(), "EPERM 때문에 실패로 접었다: {report:?}");
    assert!(
        !report.signal_errors.is_empty(),
        "EPERM은 판정에 쓰지 않되 증거로는 남겨야 한다"
    );
}

/// `EPERM`의 반대 방향도 고정한다 — 신호가 전부 실패하고 프로세스도 살아 있으면 **실패**다.
#[test]
fn f4_eperm_with_surviving_process_is_unconfirmed() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.signal_error = Some("Operation not permitted (os error 1)".into());
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert_eq!(
        report.outcome,
        TerminationOutcome::Unconfirmed(UnconfirmedReason::Survivors)
    );
    assert_eq!(report.survivors.len(), 1);
    assert_eq!(report.survivors[0].identity.pid, 100);
}

// ─────────────────── F5 신원 불일치 (PID 재사용) ───────────────────

/// **F5** — 추적 중인 pid가 **다른 시작 시각**으로 존재한다. 소멸로 처리하지 않고 `poisoned`다.
#[test]
fn f5_pid_reuse_is_unconfirmed_not_gone() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.script = vec![(
        1,
        Step::Reuse {
            pid: 100,
            start_us: 9_999,
        },
    )];
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert_eq!(
        report.outcome,
        TerminationOutcome::Unconfirmed(UnconfirmedReason::IdentityMismatch),
        "PID 재사용을 소멸로 처리했다: {report:?}"
    );
}

/// 조회 자체가 실패하면(권한 등) 확인 불가로 종결한다.
#[test]
fn probe_failure_is_unconfirmed() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.probe_error = Some((100, "Operation not permitted (os error 1)".into()));
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert_eq!(
        report.outcome,
        TerminationOutcome::Unconfirmed(UnconfirmedReason::ProbeFailed)
    );
}

/// 열거 실패도 마찬가지다. 살아 있는 대상이 있는데 목록을 못 얻으면 소멸을 주장할 수 없다.
#[test]
fn list_failure_is_unconfirmed() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.list_error = Some("proc_listpids failed".into());
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert_eq!(
        report.outcome,
        TerminationOutcome::Unconfirmed(UnconfirmedReason::ProbeFailed)
    );
}

// ───────────── F6 정상 종료 경로에 남은 배경 자손 ─────────────

/// **F6** — 루트가 **스스로 성공 종료**했는데 배경 자손이 남는다(`run_in_background`, T2).
/// 실행 중 `observe`로 신원을 누적해 뒀기 때문에 회수할 수 있다.
#[test]
fn f6_background_descendant_after_successful_exit_is_recovered() {
    let mut world = World::spawn_root(100, 1_000, false);
    world.apply(Step::Spawn {
        pid: 500,
        ppid: 100,
        start_us: 5_000,
        comm: "background-job",
        ignores_term: true,
    });
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    // 실행 중 관측 — 이 한 번이 회수 가능 여부를 가른다
    tracker.observe(&table).unwrap();
    assert_eq!(tracker.tracked_len(), 2);

    // 루트가 스스로 끝나고 배경 자손은 고아로 남는다
    table.0.borrow_mut().apply(Step::Exit { pid: 100 });
    table
        .0
        .borrow_mut()
        .apply(Step::Reparent { pid: 500, ppid: 1 });

    let report = tracker.terminate(&table, &clock, config());

    assert!(report.confirmed(), "{report:?}");
    assert!(signaled(&table, 500), "배경 자손을 회수하지 않았다");
    assert!(table.0.borrow().live().is_empty());
}

/// **F6의 근거** — 실행 중 관측이 없으면 같은 상황에서 고아를 **볼 수 없다**.
/// 그래서 `observe`가 선택이 아니라 실행기의 의무다.
#[test]
fn f6_without_in_run_observation_the_orphan_is_invisible() {
    let mut world = World::spawn_root(100, 1_000, false);
    world.apply(Step::Spawn {
        pid: 500,
        ppid: 100,
        start_us: 5_000,
        comm: "background-job",
        ignores_term: true,
    });
    world.apply(Step::Exit { pid: 100 });
    world.apply(Step::Reparent { pid: 500, ppid: 1 });
    let (table, clock) = harness(world);
    // 실행 중 관측 없이 종료 시점에만 추적을 시작한 경우
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    assert!(report.confirmed(), "루트만 보면 성공으로 보인다");
    assert_eq!(report.tracked.len(), 1, "고아는 애초에 추적되지 않았다");
    assert!(
        table
            .0
            .borrow()
            .live()
            .iter()
            .any(|p| p.identity.pid == 500),
        "그런데 고아는 실제로 살아 있다 — observe가 필요한 이유다"
    );
}

// ────────── F7 소멸한 신원의 PID를 새 자손이 재사용 ──────────

/// **F7** — 자손 A가 사라진 뒤, **살아 있는 부모**가 같은 pid를 가진 새 자손 B를 만든다.
///
/// 누적 집합을 pid로만 키잡으면 `contains_key(200)`이 이미 참이라 B가 편입되지 않는다.
/// A는 이미 `Gone`이라 이후 조회에서도 건너뛰므로, **B는 살아 있는데 아무도 보지 않는다** —
/// 그 상태로 `Confirmed`가 난다. 키는 `(pid, 시작 시각)` 전체여야 한다.
#[test]
fn f7_a_recycled_pid_becomes_a_new_identity_not_a_known_one() {
    let mut world = World::spawn_root(100, 1_000, true);
    world.apply(Step::Spawn {
        pid: 200,
        ppid: 100,
        start_us: 2_000,
        comm: "first-child",
        ignores_term: true,
    });
    world.script = vec![
        // 자손 A가 끝난다
        (1, Step::Exit { pid: 200 }),
        // 살아 있는 부모가 **같은 pid**로 새 자손 B를 만든다
        (
            2,
            Step::Spawn {
                pid: 200,
                ppid: 100,
                start_us: 9_000,
                comm: "recycled-child",
                ignores_term: true,
            },
        ),
    ];
    let (table, clock) = harness(world);
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    let report = tracker.terminate(&table, &clock, config());

    // 같은 pid의 **두 신원**이 모두 추적됐어야 한다
    let recycled = ProcIdentity {
        pid: 200,
        start_us: 9_000,
    };
    assert!(
        report.tracked.contains(&ProcIdentity {
            pid: 200,
            start_us: 2_000
        }),
        "사라진 자손이 추적 기록에서 빠졌다: {:?}",
        report.tracked
    );
    assert!(
        report.tracked.contains(&recycled),
        "재사용된 pid의 새 자손이 편입되지 않았다: {:?}",
        report.tracked
    );
    // 그리고 실제로 회수됐어야 한다
    assert!(report.confirmed(), "{report:?}");
    assert!(
        table.0.borrow().live().is_empty(),
        "살아 있는 프로세스를 남긴 채 소멸을 주장했다: {:?}",
        table.0.borrow().live()
    );
}

// ────────── F8 실행 중 한 번 깨진 추적 무결성 ──────────

/// **F8** — 실행 중 열거가 한 번 실패했다가 회복되고, 종료 시점에는 모두 사라져 있다.
///
/// 지금 보이는 것만 보면 완벽하지만, **실패한 그 회차에 생겼다 사라진 자손이 있었는지 알 수 없다.**
/// 무결성이 한 번 깨지면 소멸을 주장할 근거 자체가 사라진다.
#[test]
fn f8_a_transient_list_failure_still_blocks_confirmation() {
    let (table, clock) = harness(World::spawn_root(100, 1_000, false));
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    // 실행 중 열거가 실패했다
    table.0.borrow_mut().list_error = Some("cannot enumerate processes".into());
    assert!(tracker.observe(&table).is_err());

    // 일시적이었다 — 다음 관측은 정상이고 프로세스도 모두 사라졌다
    {
        let mut world = table.0.borrow_mut();
        world.list_error = None;
        world.procs.clear();
    }
    assert!(tracker.observe(&table).is_ok());

    let report = tracker.terminate(&table, &clock, config());

    assert!(
        !report.confirmed(),
        "일시적 열거 실패를 잊고 소멸을 주장했다: {report:?}"
    );
    assert_eq!(
        report.outcome,
        TerminationOutcome::Unconfirmed(UnconfirmedReason::ProbeFailed)
    );
    assert!(
        report.detail.contains("tracking was broken earlier"),
        "{}",
        report.detail
    );
}

/// **F8** — 신원 불일치도 마찬가지다. 재사용된 pid가 나중에 사라져도 그 사실은 남는다.
#[test]
fn f8_a_transient_identity_mismatch_still_blocks_confirmation() {
    let (table, clock) = harness(World::spawn_root(100, 1_000, false));
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    // 실행 중 관측에서 pid 재사용이 잡혔다
    table.0.borrow_mut().apply(Step::Reuse {
        pid: 100,
        start_us: 7_000,
    });
    assert!(tracker.observe(&table).is_err());

    // 그 뒤 모두 사라졌다 — 지금 보이는 것만으로는 아무 문제가 없다
    table.0.borrow_mut().procs.clear();

    let report = tracker.terminate(&table, &clock, config());

    assert_eq!(
        report.outcome,
        TerminationOutcome::Unconfirmed(UnconfirmedReason::IdentityMismatch),
        "{report:?}"
    );
}

/// 무결성이 깨지지 않은 실행은 그대로 통과해야 한다 — F8이 모든 성공을 막으면 안 된다.
#[test]
fn a_clean_run_is_still_confirmed() {
    let (table, clock) = harness(World::spawn_root(100, 1_000, false));
    let mut tracker = TreeTracker::new(ProcIdentity {
        pid: 100,
        start_us: 1_000,
    });

    assert!(tracker.observe(&table).is_ok());
    let report = tracker.terminate(&table, &clock, config());

    assert!(report.confirmed(), "{report:?}");
}

// ───────────────────────── 실제 OS 연동 ─────────────────────────

/// FFI 경로를 실제 프로세스로 검증한다. 신원 조회·열거·종료가 진짜로 동작하는지 본다.
#[cfg(unix)]
#[test]
fn system_table_terminates_a_real_process_tree() {
    use std::process::{Command, Stdio};

    let mut child = Command::new("/bin/sh")
        .args(["-c", "/usr/bin/tail -f /dev/null"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot spawn fixture process");

    let identity = super::identity_of(child.id()).expect("cannot read child identity");
    assert_eq!(identity.pid, child.id());
    assert!(
        identity.start_us > 0,
        "시작 시각을 마이크로초로 얻어야 한다"
    );

    let table = SystemProcTable;
    let clock = SystemClock::default();
    let mut tracker = TreeTracker::new(identity);
    // 자손(`tail`)이 뜰 시간을 준 뒤 실행 중 관측을 한 번 한다
    std::thread::sleep(Duration::from_millis(400));
    let _ = tracker.observe(&table);

    let report = tracker.terminate(
        &table,
        &clock,
        TerminationConfig {
            term_grace_ms: 2_000,
            kill_grace_ms: 3_000,
            poll_interval_ms: 50,
        },
    );

    let _ = child.wait();
    assert!(report.confirmed(), "실제 트리 종료 실패: {report:?}");
    assert!(
        report.tracked.len() >= 1,
        "루트 신원은 최소한 추적돼야 한다"
    );
}

/// 이미 사라진 pid의 신원은 얻을 수 없어야 한다 — 추적을 시작하면 안 되는 상황이다.
#[cfg(unix)]
#[test]
fn system_identity_of_missing_process_fails() {
    let err = super::identity_of(u32::MAX - 1).unwrap_err();
    assert!(err.contains("already gone"), "{err}");
}
