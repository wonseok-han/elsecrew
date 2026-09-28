//! 종료 계약의 핵심 — **누적 신원 추적**으로 프로세스 트리 소멸을 확인한다.
//!
//! 설계 근거는 `docs/proposals/m4-orchestration.md` §7.10.1.2 ⑤이다. 요점만 옮기면:
//!
//! - **루트 기준 ppid 트리 순회는 오판한다.** 루트나 중간 부모가 먼저 죽으면 자손이
//!   `launchd`(pid 1)로 재부모화되어 루트 트리에서 사라진다. 살아 있는데 빈 집합으로 보인다.
//! - 그래서 **한 번이라도 관측한 `(pid, 시작 시각)` 신원을 누적**하고, 현재 부모가 누구든
//!   신원별로 생존을 확인한다. 새 후손은 **살아 있는 모든 추적 대상**에서 다시 열거한다.
//! - **누적 집합 전체의 소멸을 확인한 경우에만 성공이다.** 루트가 죽었다는 사실도,
//!   트리가 비었다는 사실도 성공 근거가 아니다.
//! - 신호 결과는 판정에 쓰지 않는다. `EPERM`은 성공도 소멸 증거도 아니다.
//! - **확인 불가·신원 불일치(PID 재사용)는 실패**로 종결하고 호출부가 `poisoned`로 처리한다.

use std::collections::BTreeMap;
use std::time::Duration;

/// 프로세스 신원. **pid만으로는 안 된다** — 재사용되면 다른 프로세스를 원래 것으로 오인한다.
///
/// 누적 집합의 **키가 이 값 전체**여야 한다. pid만 키로 쓰면 소멸을 확인한 pid를
/// 새 자손이 재사용했을 때 "이미 아는 pid"로 보여 편입이 막히고, 살아 있는 자손을 놓친다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcIdentity {
    pub pid: u32,
    /// 시작 시각(마이크로초). macOS는 `proc_pidinfo(PROC_PIDTBSDINFO)`에서 얻는다.
    pub start_us: i64,
}

/// 한 프로세스에 대한 관측.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcEntry {
    pub identity: ProcIdentity,
    pub ppid: u32,
    pub comm: String,
}

/// 신원 확인 결과. **"모름"을 소멸로 접지 않는 것이 이 타입의 존재 이유다.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeResult {
    /// 살아 있고 신원이 기록과 일치한다
    Alive(ProcEntry),
    /// 사라졌음을 확인했다 (`ESRCH`. 좀비도 여기로 온다)
    Gone,
    /// 조회 자체가 실패했다 (`EPERM` 등) — 소멸을 주장할 수 없다
    Unconfirmable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeSignal {
    Term,
    Kill,
}

/// OS 접근을 주입 가능하게 둔다. F1~F6 회귀 픽스처를 실제 프로세스 없이 고정하기 위해서다.
pub trait ProcTable {
    /// 지금 관측 가능한 프로세스 전체. 읽을 수 없는 프로세스(다른 사용자 소유)는 빠진다 —
    /// 우리 자손은 모두 같은 uid이므로 열거에서 누락되지 않는다.
    fn list(&self) -> Result<Vec<ProcEntry>, String>;
    /// 특정 신원의 생존 확인. **현재 부모가 누구인지와 무관하게** 답해야 한다.
    fn probe(&self, identity: ProcIdentity) -> ProbeResult;
    /// 신호 전달. 실패해도 판정에 쓰지 않는다 — 기록만 한다.
    fn signal(&self, pid: u32, signal: TreeSignal) -> Result<(), String>;
}

/// 시간도 주입한다. 테스트가 유예를 실시간으로 기다리지 않게 한다.
pub trait Clock {
    fn elapsed_ms(&self) -> u64;
    fn sleep(&self, duration: Duration);
}

pub struct SystemClock {
    start: std::time::Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TerminationConfig {
    /// TERM 이후 협조적 정리를 기다리는 시간
    pub term_grace_ms: u64,
    /// KILL 이후 소멸을 확인하는 시간
    pub kill_grace_ms: u64,
    /// 열거 간격. **짧을수록 "두 회차 사이에 생겼다 부모까지 죽는" 빈틈이 줄어든다**(§7.10.1.2 ⑤).
    pub poll_interval_ms: u64,
}

impl Default for TerminationConfig {
    fn default() -> Self {
        Self {
            term_grace_ms: 4_000,
            kill_grace_ms: 4_000,
            poll_interval_ms: 100,
        }
    }
}

/// 종료 판정. **확인하지 못한 것은 전부 실패 쪽으로 접는다.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationOutcome {
    /// 누적 집합 전체의 소멸을 확인했다
    Confirmed,
    /// 확인하지 못했다. 호출부는 실행과 복제본을 `poisoned`로 만들어야 한다
    Unconfirmed(UnconfirmedReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnconfirmedReason {
    /// 상한 안에 소멸을 관측하지 못했다
    Survivors,
    /// pid는 있는데 시작 시각이 기록과 다르다 — 추적 무결성 붕괴
    IdentityMismatch,
    /// 신원 조회나 열거 자체가 실패했다
    ProbeFailed,
}

/// 종료 절차의 전체 기록. 증거로 남긴다.
#[derive(Debug, Clone)]
pub struct TerminationReport {
    pub outcome: TerminationOutcome,
    /// 사람이 읽을 수 있는 사유
    pub detail: String,
    /// 마지막까지 살아 있던 신원 (증거용 — pid와 명령줄)
    pub survivors: Vec<ProcEntry>,
    /// 한 번이라도 관측한 신원 전체
    pub tracked: Vec<ProcIdentity>,
    pub rounds: u32,
    /// 신호 전달 중 발생한 오류. **판정에 쓰지 않고 기록만 한다**
    pub signal_errors: Vec<String>,
}

impl TerminationReport {
    pub fn confirmed(&self) -> bool {
        self.outcome == TerminationOutcome::Confirmed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackedState {
    Live,
    Gone,
}

#[derive(Debug, Clone)]
struct TrackedProc {
    identity: ProcIdentity,
    state: TrackedState,
    comm: String,
}

/// 한 회차 관측 결과.
struct Refresh {
    live: Vec<ProcEntry>,
    discovered: usize,
}

/// **실행 시작부터** 후손 신원을 누적하는 추적기.
///
/// 종료 시점에만 열거하면 늦다 — 루트가 스스로 끝난 뒤에는 이미 고아가 된 배경 자손을
/// 발견할 경로가 없다(§7.10.1.2 ⑤ F6, T2). 그래서 실행 중에도 `observe`로 계속 채운다.
pub struct TreeTracker {
    /// **키는 pid가 아니라 신원 전체다.** pid를 키로 쓰면 소멸을 확인한 pid를 새 자손이
    /// 재사용했을 때 편입이 막히고, 그 자손은 이후 확인 대상에서도 빠져 살아 있는 채로
    /// `Confirmed`가 난다.
    tracked: BTreeMap<ProcIdentity, TrackedProc>,
    /// **실행 중에 한 번이라도 깨진 추적 무결성.** 나중에 정상으로 돌아와도 지우지 않는다 —
    /// 그 순간 놓친 자손이 있었는지 알 수 없으므로 소멸을 주장할 근거가 사라진 것이다.
    integrity_error: Option<(UnconfirmedReason, String)>,
    signal_errors: Vec<String>,
    termination: Option<TerminationProgress>,
}

struct TerminationProgress {
    started_ms: u64,
    next_poll_ms: u64,
    config: TerminationConfig,
    rounds: u32,
    report: Option<TerminationReport>,
}

impl TreeTracker {
    pub fn new(root: ProcIdentity) -> Self {
        let mut tracked = BTreeMap::new();
        tracked.insert(
            root,
            TrackedProc {
                identity: root,
                state: TrackedState::Live,
                comm: String::new(),
            },
        );
        Self {
            tracked,
            integrity_error: None,
            signal_errors: Vec::new(),
            termination: None,
        }
    }

    /// 실행 중 호출한다. 신원을 누적하기만 하고 판정은 하지 않는다.
    ///
    /// **실패는 기억된다.** 호출부는 실행을 계속해도 되지만, 한 번 깨진 무결성은
    /// `terminate`가 `Confirmed`를 내지 못하게 만든다 — 그 회차에 놓친 자손이 있었는지
    /// 확인할 방법이 없기 때문이다.
    pub fn observe<T: ProcTable>(&mut self, table: &T) -> Result<(), String> {
        self.invalidate_termination_report();
        match self.refresh(table) {
            Ok(_) => Ok(()),
            Err((reason, detail)) => {
                let remembered = detail.clone();
                self.integrity_error
                    .get_or_insert_with(|| (reason, remembered));
                Err(detail)
            }
        }
    }

    /// 지금까지 관측한 신원 수. F6 회귀 테스트가 "실행 중 관측"의 효과를 확인하는 데 쓴다.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tracked_len(&self) -> usize {
        self.tracked.len()
    }

    /// Durable supervisors persist this cumulative identity set after every observation.
    /// Returning identities only avoids exposing mutable tracker internals or process metadata.
    pub fn tracked_identities(&self) -> Vec<ProcIdentity> {
        self.identities()
    }

    /// Add a directly handshaken identity before descendant enumeration begins.
    pub fn track_identity(&mut self, identity: ProcIdentity) {
        self.invalidate_termination_report();
        self.tracked.entry(identity).or_insert(TrackedProc {
            identity,
            state: TrackedState::Live,
            comm: String::new(),
        });
    }

    fn invalidate_termination_report(&mut self) {
        if let Some(progress) = &mut self.termination {
            progress.report = None;
            progress.next_poll_ms = 0;
        }
    }

    fn refresh<T: ProcTable>(&mut self, table: &T) -> Result<Refresh, (UnconfirmedReason, String)> {
        // ── 1) 누적 신원 전체를 개별 확인한다. 현재 부모가 누구든 상관없다 ──
        let mut live: Vec<ProcEntry> = Vec::new();
        for proc in self.tracked.values_mut() {
            if proc.state == TrackedState::Gone {
                continue;
            }
            match table.probe(proc.identity) {
                ProbeResult::Alive(entry) => {
                    if !entry.comm.is_empty() {
                        proc.comm = entry.comm.clone();
                    }
                    live.push(entry);
                }
                ProbeResult::Gone => proc.state = TrackedState::Gone,
                ProbeResult::Unconfirmable(reason) => {
                    let mismatch = reason.contains("identity mismatch");
                    return Err((
                        if mismatch {
                            UnconfirmedReason::IdentityMismatch
                        } else {
                            UnconfirmedReason::ProbeFailed
                        },
                        format!(
                            "cannot confirm process {} is gone: {reason}",
                            proc.identity.pid
                        ),
                    ));
                }
            }
        }

        // ── 2) 살아 있는 **모든** 추적 대상에서 새 후손을 발견한다 ──
        //     루트에서만 열거하면 재부모화된 고아를 놓친다(§7.10.1.2 ⑤ C6).
        let mut discovered = 0usize;
        if !live.is_empty() {
            let snapshot = table.list().map_err(|e| {
                (
                    UnconfirmedReason::ProbeFailed,
                    format!("cannot enumerate processes: {e}"),
                )
            })?;
            let live_pids: Vec<u32> = live.iter().map(|e| e.identity.pid).collect();
            for entry in descendants_of(&snapshot, &live_pids) {
                // 신원 전체로 조회한다. 같은 pid라도 시작 시각이 다르면 **다른 프로세스**이며
                // 새 신원으로 편입돼야 한다.
                if !self.tracked.contains_key(&entry.identity) {
                    self.tracked.insert(
                        entry.identity,
                        TrackedProc {
                            identity: entry.identity,
                            state: TrackedState::Live,
                            comm: entry.comm.clone(),
                        },
                    );
                    discovered += 1;
                }
            }
        }

        Ok(Refresh { live, discovered })
    }

    /// 트리를 종료하고 **누적 집합 전체의 소멸을 확인**한다.
    ///
    /// `Confirmed`가 아니면 호출부는 반드시 실행과 복제본을 `poisoned`로 만들어야 한다.
    pub fn terminate<T: ProcTable, C: Clock>(
        &mut self,
        table: &T,
        clock: &C,
        config: TerminationConfig,
    ) -> TerminationReport {
        // A synchronous retry starts a fresh observation attempt, but never
        // clears tracked identities or a remembered integrity failure.
        self.termination = None;
        loop {
            if let Some(report) = self.poll_termination(table, clock.elapsed_ms(), config) {
                return report;
            }
            clock.sleep(Duration::from_millis(config.poll_interval_ms.max(1)));
        }
    }

    /// One termination observation/signaling round, without sleeping. The first
    /// call fixes the monotonic clock origin and config. Subsequent calls retain
    /// deadlines and return the same terminal report without further signals.
    /// The progress belongs to this tracker, not a transferable process token.
    pub fn poll_termination<T: ProcTable>(
        &mut self,
        table: &T,
        now_ms: u64,
        config: TerminationConfig,
    ) -> Option<TerminationReport> {
        let mut progress = self.termination.take().unwrap_or(TerminationProgress {
            started_ms: now_ms,
            next_poll_ms: now_ms,
            config,
            rounds: 0,
            report: None,
        });
        if progress.report.is_none() && now_ms >= progress.next_poll_ms {
            progress.rounds = progress.rounds.saturating_add(1);
            let term_end = progress
                .started_ms
                .saturating_add(progress.config.term_grace_ms);
            let deadline_ms = term_end.saturating_add(progress.config.kill_grace_ms);
            progress.report = match self.refresh(table) {
                Err((reason, detail)) => {
                    self.integrity_error
                        .get_or_insert((reason.clone(), detail.clone()));
                    Some(self.report(reason, detail, Vec::new(), progress.rounds))
                }
                Ok(refreshed) if refreshed.live.is_empty() && refreshed.discovered == 0 => {
                    Some(self.report_final(progress.rounds))
                }
                Ok(refreshed) if now_ms >= deadline_ms => Some(self.report(
                    UnconfirmedReason::Survivors,
                    format!(
                        "{} process(es) still alive after TERM and KILL",
                        refreshed.live.len()
                    ),
                    refreshed.live,
                    progress.rounds,
                )),
                Ok(refreshed) => {
                    let signal = if now_ms < term_end {
                        TreeSignal::Term
                    } else {
                        TreeSignal::Kill
                    };
                    for entry in &refreshed.live {
                        if let Err(error) = table.signal(entry.identity.pid, signal) {
                            let line = format!("{signal:?} -> pid {}: {error}", entry.identity.pid);
                            if !self.signal_errors.contains(&line) {
                                self.signal_errors.push(line);
                            }
                        }
                    }
                    progress.next_poll_ms =
                        now_ms.saturating_add(progress.config.poll_interval_ms.max(1));
                    None
                }
            };
        }
        let report = progress.report.clone();
        self.termination = Some(progress);
        report
    }
    fn identities(&self) -> Vec<ProcIdentity> {
        self.tracked.values().map(|p| p.identity).collect()
    }

    /// 관측된 소멸을 최종 판정으로 바꾼다.
    ///
    /// **여기가 무결성 실패를 걸러내는 유일한 지점이다.** 실행 중 한 번이라도 열거·조회가
    /// 깨졌다면, 지금 아무도 살아 있지 않다는 관측만으로는 소멸을 주장할 수 없다 —
    /// 그 회차에 있었을지 모를 자손이 추적 집합에 들어오지 못했기 때문이다.
    fn report_final(&self, rounds: u32) -> TerminationReport {
        if let Some((reason, detail)) = self.integrity_error.clone() {
            return self.report(
                reason,
                format!(
                    "the process tree looks gone, but tracking was broken earlier in the run \
                     so its disappearance cannot be claimed: {detail}"
                ),
                Vec::new(),
                rounds,
            );
        }
        TerminationReport {
            outcome: TerminationOutcome::Confirmed,
            detail: format!(
                "all {} tracked process(es) confirmed gone",
                self.tracked.len()
            ),
            survivors: Vec::new(),
            tracked: self.identities(),
            rounds,
            signal_errors: self.signal_errors.clone(),
        }
    }

    fn report(
        &self,
        reason: UnconfirmedReason,
        detail: String,
        survivors: Vec<ProcEntry>,
        rounds: u32,
    ) -> TerminationReport {
        TerminationReport {
            outcome: TerminationOutcome::Unconfirmed(reason),
            detail,
            survivors,
            tracked: self.identities(),
            rounds,
            signal_errors: self.signal_errors.clone(),
        }
    }
}

/// 주어진 pid 집합에서 출발한 후손 폐포. 시작점 자신은 포함하지 않는다.
fn descendants_of(snapshot: &[ProcEntry], roots: &[u32]) -> Vec<ProcEntry> {
    let mut children: BTreeMap<u32, Vec<&ProcEntry>> = BTreeMap::new();
    for entry in snapshot {
        children.entry(entry.ppid).or_default().push(entry);
    }
    let mut seen: Vec<u32> = roots.to_vec();
    let mut stack: Vec<u32> = roots.to_vec();
    let mut out: Vec<ProcEntry> = Vec::new();
    while let Some(pid) = stack.pop() {
        let Some(kids) = children.get(&pid) else {
            continue;
        };
        for kid in kids {
            if seen.contains(&kid.identity.pid) {
                continue;
            }
            seen.push(kid.identity.pid);
            stack.push(kid.identity.pid);
            out.push((*kid).clone());
        }
    }
    out
}

// ───────────────────────────── 실제 OS 구현 ─────────────────────────────

#[derive(Default)]
pub struct SystemProcTable;

#[cfg(unix)]
mod sys {
    use super::{ProbeResult, ProcEntry, ProcIdentity, TreeSignal};

    const PROC_ALL_PIDS: u32 = 1;

    /// `proc_pidinfo(PROC_PIDTBSDINFO)` 한 건. errno로 소멸과 확인 불가를 **구분**한다.
    /// - 성공 → 살아 있음 (마이크로초 시작 시각 + ppid)
    /// - `ESRCH` → 사라짐. **좀비도 여기로 온다** (`killpg`의 `EPERM` 모호성을 겪지 않는 이유)
    /// - 그 외(`EPERM` 등) → 확인 불가
    pub(super) fn bsdinfo(pid: u32) -> Result<Option<ProcEntry>, String> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        unsafe { *libc::__error() = 0 };
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut libc::c_void,
                size,
            )
        };
        if written == size {
            let comm: Vec<u8> = info
                .pbi_comm
                .iter()
                .map(|&c| c as u8)
                .take_while(|&c| c != 0)
                .collect();
            return Ok(Some(ProcEntry {
                identity: ProcIdentity {
                    pid: info.pbi_pid,
                    start_us: info.pbi_start_tvsec as i64 * 1_000_000
                        + info.pbi_start_tvusec as i64,
                },
                ppid: info.pbi_ppid,
                comm: String::from_utf8_lossy(&comm).into_owned(),
            }));
        }
        let errno = unsafe { *libc::__error() };
        if errno == libc::ESRCH {
            return Ok(None);
        }
        Err(std::io::Error::from_raw_os_error(errno).to_string())
    }

    pub(super) fn list() -> Result<Vec<ProcEntry>, String> {
        let needed = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
        if needed <= 0 {
            return Err("proc_listpids reported no processes".into());
        }
        let capacity = needed as usize / std::mem::size_of::<libc::c_int>() + 64;
        let mut buffer = vec![0 as libc::c_int; capacity];
        let written = unsafe {
            libc::proc_listpids(
                PROC_ALL_PIDS,
                0,
                buffer.as_mut_ptr() as *mut libc::c_void,
                (capacity * std::mem::size_of::<libc::c_int>()) as libc::c_int,
            )
        };
        if written <= 0 {
            return Err("proc_listpids failed".into());
        }
        buffer.truncate(written as usize / std::mem::size_of::<libc::c_int>());

        let mut out = Vec::new();
        for pid in buffer {
            if pid <= 0 {
                continue;
            }
            // 읽을 수 없는 프로세스는 다른 사용자 소유다 — 우리 자손일 수 없으므로 건너뛴다.
            // 열거 실패를 여기서 오류로 올리면 시스템 프로세스 때문에 매번 poisoned가 된다.
            if let Ok(Some(entry)) = bsdinfo(pid as u32) {
                out.push(entry);
            }
        }
        Ok(out)
    }

    pub(super) fn probe(identity: ProcIdentity) -> ProbeResult {
        match bsdinfo(identity.pid) {
            Ok(None) => ProbeResult::Gone,
            Ok(Some(entry)) => {
                if entry.identity.start_us == identity.start_us {
                    ProbeResult::Alive(entry)
                } else {
                    // pid가 재사용됐다. 원래 프로세스가 끝난 것인지 대체된 것인지
                    // **우리 기록으로는 구분되지 않는다** → 확인 불가로 접는다.
                    ProbeResult::Unconfirmable(format!(
                        "identity mismatch on pid {} (expected start {}us, found {}us)",
                        identity.pid, identity.start_us, entry.identity.start_us
                    ))
                }
            }
            Err(e) => ProbeResult::Unconfirmable(e),
        }
    }

    pub(super) fn signal(pid: u32, signal: TreeSignal) -> Result<(), String> {
        let sig = match signal {
            TreeSignal::Term => libc::SIGTERM,
            TreeSignal::Kill => libc::SIGKILL,
        };
        unsafe { *libc::__error() = 0 };
        let rc = unsafe { libc::kill(pid as libc::pid_t, sig) };
        if rc == 0 {
            return Ok(());
        }
        let errno = unsafe { *libc::__error() };
        Err(std::io::Error::from_raw_os_error(errno).to_string())
    }

    pub(super) fn identity_of(pid: u32) -> Result<ProcIdentity, String> {
        match bsdinfo(pid)? {
            Some(entry) => Ok(entry.identity),
            None => Err(format!("process {pid} already gone before it was tracked")),
        }
    }
}

#[cfg(unix)]
impl ProcTable for SystemProcTable {
    fn list(&self) -> Result<Vec<ProcEntry>, String> {
        sys::list()
    }
    fn probe(&self, identity: ProcIdentity) -> ProbeResult {
        sys::probe(identity)
    }
    fn signal(&self, pid: u32, signal: TreeSignal) -> Result<(), String> {
        sys::signal(pid, signal)
    }
}

/// 실행 직후 자식의 신원을 확보한다. **신원을 얻지 못하면 추적을 시작할 수 없다.**
#[cfg(unix)]
pub fn identity_of(pid: u32) -> Result<ProcIdentity, String> {
    sys::identity_of(pid)
}

// Windows 등 비 unix 플랫폼: §7.10.1.2 ⑥에 따라 **파일럿 범위 밖**이다.
// 여기서 조용히 성공을 반환하면 계약이 무너지므로, 전부 "확인 불가"로 답한다.
#[cfg(not(unix))]
impl ProcTable for SystemProcTable {
    fn list(&self) -> Result<Vec<ProcEntry>, String> {
        Err("process enumeration is not implemented on this platform".into())
    }
    fn probe(&self, _identity: ProcIdentity) -> ProbeResult {
        ProbeResult::Unconfirmable(
            "process identity probing is not implemented on this platform".into(),
        )
    }
    fn signal(&self, _pid: u32, _signal: TreeSignal) -> Result<(), String> {
        Err("process signaling is not implemented on this platform".into())
    }
}

#[cfg(not(unix))]
pub fn identity_of(_pid: u32) -> Result<ProcIdentity, String> {
    Err("process identity is not available on this platform".into())
}

#[cfg(test)]
mod tests;
