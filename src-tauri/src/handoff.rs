//! §7.10 최소 자동 인계 수직 흐름.
//!
//! ```text
//! 사용자 1회 입력 → Claude 1회 실행 → 앱이 결과를 자동 인계 → Codex 1회 실행 → 결과·증거 표시
//! ```
//!
//! 검증하려는 것은 **가설 A**(§5.2)다 — *사용자가 전달자에서 빠질 수 있는가*.
//! 그래서 사용자는 목표를 한 번 말할 뿐이고, Claude의 계획을 Codex에 넘기는 것은 **앱이 한다**.
//!
//! 세 가지 경계를 지킨다.
//!
//! - **실행 계약**(§7.10.1.2 ④): Claude는 읽기 전용 계획 역할이며 `system:init`을 fail-closed로 검증한다.
//! - **종료 계약**(§7.10.1.2 ⑤): 누적 신원 추적으로 트리 소멸을 확인하고, **확인하지 못하면 `poisoned`** 다.
//!   poisoned가 되면 **Codex 자동 인계를 중단**하고 복제본 재사용도 막는다.
//! - **중단 계약**: 사용자가 누른 중단은 준비·계획·구현 **어느 단계에서도** 닿아야 하고,
//!   실행이 시작된다고 지워지지 않는다. 그래서 취소 깃발은 실행이 아니라 **슬롯**이 소유한다.
//!
//! 플랫폼은 **macOS 전용**이다(§7.10.1.2 ⑥). 종료 계약의 Windows 대응물을 실측하지 않았다.

use crate::claude_pilot::{self, InitFacts, PlanResult};
use crate::codex_pilot;
use crate::orchestration::{self, PilotWorkspace};
use crate::proctree::{
    self, SystemClock, SystemProcTable, TerminationConfig, TerminationReport, TreeTracker,
};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{Emitter, State};

/// 증거는 전부 복제본 **밖**에 둔다 — 에이전트가 자기 증거를 고칠 수 없게 한다.
const PLAN_EVENTS_SUFFIX: &str = ".hello-agent-plan.events.jsonl";
const PLAN_STDERR_SUFFIX: &str = ".hello-agent-plan.stderr.log";
const PLAN_DIFF_SUFFIX: &str = ".hello-agent-plan.diff";
/// 계획 역할의 구조화 출력·스키마. Codex 계획 경로에서만 쓴다
const PLAN_LAST_MESSAGE_SUFFIX: &str = ".hello-agent-plan.last-message.txt";
const PLAN_SCHEMA_SUFFIX: &str = ".hello-agent-plan.schema.json";

/// 실행 중 후손 신원을 누적하는 간격. 짧을수록 §7.10.1.2 ⑤의 잔여 빈틈이 줄어든다.
const OBSERVE_INTERVAL_MS: u64 = 250;
/// 취소를 확인하는 간격. **stdout을 기다리지 않는다** — 조용한 실행에서도 이 안에 종료가 시작된다.
const CANCEL_POLL_MS: u64 = 50;
/// 종료를 확인하지 못했을 때의 제한된 회수 시도.
/// 여기서 blocking `wait()`를 하면 살아남은 프로세스가 파이프를 물고 있는 동안 영영 돌아오지 않는다.
const REAP_TRIES: u32 = 5;
const REAP_INTERVAL_MS: u64 = 20;
/// 사용자에게 보여줄 결과 요약의 상한. 최종 메시지를 통째로 부으면 화면이 로그가 된다.
const RESULT_SUMMARY_LIMIT: usize = 2000;

/// 사용자에게 보여줄 **평이한 진행 상태**(§7.10.2).
/// 역할 이름이 아니라 **일의 이름**이다 — `Claude 실행 중`이 아니라 `요청 정리 중`이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HandoffStage {
    /// 요청 정리 중 (Claude 계획)
    Organizing,
    /// 만드는 중 (Codex 실행)
    Building,
    /// 확인 중 — 무엇이 바뀌었는지 앱이 실제로 확인하는 단계
    Checking,
    /// 완료
    Done,
    /// **무엇을 바꿀지 알아야 한다.** 사고가 아니라 되묻기다 —
    /// 계획 역할이 "할 일이 없다"고 답한 경우(인사말만 넣었을 때 등)
    NeedsGoal,
    /// 사용자 확인 필요 — 안전하게 멈췄다
    NeedsAttention,
}

/// 복제본이 잠긴 방식. **"잠갔다"와 "잠금을 저장했다"는 다른 사실이다.**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkspaceLock {
    /// 잠기지 않았다
    None,
    /// 디스크 표시까지 남았다 — 앱을 재시작해도 재사용이 막힌다
    Persisted,
    /// 표시 저장에 실패했다. **이 세션에서만** 막힌다 — 재시작하면 풀리므로 지금 폐기해야 한다
    SessionOnly,
}

/// 변경된 파일 하나. 표시 문구는 프런트가 만든다 — 백엔드가 언어를 정하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangedFile {
    pub path: String,
    pub kind: ChangeKind,
    /// 이름이 바뀐 경우의 원래 경로
    pub from: Option<String>,
    /// git 원본 상태 코드 (`M`, `D`, `R100` …). 증거로 그대로 남긴다
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    /// 파일 종류 변경 등 위에 없는 상태. **모른다고 목록에서 빼지 않는다**
    Other,
}

impl ChangeKind {
    fn from_status(status: &str) -> Self {
        match status.chars().next() {
            Some('A') => Self::Added,
            Some('M') => Self::Modified,
            Some('D') => Self::Deleted,
            Some('R') => Self::Renamed,
            Some('C') => Self::Copied,
            _ => Self::Other,
        }
    }

    fn takes_two_paths(self) -> bool {
        matches!(self, Self::Renamed | Self::Copied)
    }
}

/// 인계 한 건의 결과. **원시 증거 경로는 화면 기본값이 아니라 상세용이다**(§7.10.2).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffResult {
    pub stage: HandoffStage,
    /// 사용자가 입력한 목표 그대로
    pub goal: String,
    /// **하려던 것** — 계획 역할의 요약이다. 결과가 아니다
    pub plan_summary: String,
    /// **실제로 한 것** — 구현 단계의 최종 메시지다
    pub result_summary: String,
    /// 사람이 읽을 수 있는 변경 내용. 삭제·이름 변경·바이너리도 포함한다
    pub changed_files: Vec<ChangedFile>,
    /// 안전하게 멈춘 이유. 비어 있으면 정상이다
    pub attention: Option<String>,
    /// 복제본이 잠겼는가, 그리고 그 잠금이 디스크에 남았는가
    pub workspace_lock: WorkspaceLock,
    /// 잠금을 저장하지 못한 이유. 있으면 재시작 시 잠금이 풀린다는 뜻이다
    pub lock_error: Option<String>,
    /// **다른 복제본** 때문에 막혔다면 그 복제본. 화면이 그것을 정리할 수 있어야 한다
    pub blocker: Option<orchestration::BlockedWorkspace>,
    pub workspace_path: String,
    /// 고급 상세용 증거. 기본 화면에 붓지 않는다
    pub evidence: HandoffEvidence,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffEvidence {
    pub plan_events_path: String,
    pub plan_stderr_path: String,
    pub plan_diff_path: String,
    pub plan_session_id: String,
    pub plan_cli_version: String,
    pub codex_events_path: String,
    pub codex_last_message_path: String,
    pub codex_stderr_path: String,
    pub codex_diff_path: String,
    pub codex_session_id: String,
    /// 구현 단계 실행 식별자. **증거 파일 이름이 이 값으로 갈리므로** 도움 요청 때 필요하다
    pub codex_run_id: String,
    /// 거부된 도구 호출 (Claude는 기계 판독이 된다)
    pub permission_denials: Vec<String>,
    /// 종료 절차 기록 — 추적한 신원 수와 마지막 생존자
    pub termination: String,
}

/// 진행 상태 이벤트. **어느 실행의 상태인지 실어 보낸다** —
/// 앞선 실행이 뒤늦게 보낸 상태가 새 실행 화면에 섞이면 사용자가 거짓 진행을 본다.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StageEvent {
    run_id: String,
    stage: HandoffStage,
}

// ───────────────────────── 실행 슬롯과 취소 소유권 ─────────────────────────

/// 인계 전체의 **단일 실행 슬롯**. 동시 실행을 거부하고, 취소 깃발을 소유한다.
#[derive(Default, Clone)]
pub(crate) struct HandoffSession(pub Arc<Mutex<Option<HandoffSlot>>>);

/// 예약된 인계 한 건.
///
/// 취소 깃발이 **실행이 아니라 여기 있는 것이 요점이다.** 실행이 자기 깃발을 만들면
/// 준비 단계(복제본 만들기)에서 누른 중단이 실행 시작과 함께 지워진다.
#[derive(Debug)]
pub(crate) struct HandoffSlot {
    run_id: String,
    cancel: Arc<AtomicBool>,
    /// 실행 소유자가 잡았는가. 준비 단계에서는 아직 아무도 소유하지 않는다
    claimed: bool,
    /// 지금 무슨 일을 하는가. `Building`일 때만 취소를 Codex 실행에도 전달한다
    stage: Option<HandoffStage>,
}

impl HandoffSession {
    /// 락 오염에서 복구해서 쓴다. 다른 스레드가 패닉했다고 **취소가 막히면 안 된다** —
    /// 슬롯이 담는 것은 문자열·깃발뿐이라 중간 상태로 깨질 여지가 없다.
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<HandoffSlot>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 흐름 **시작 전에** 슬롯을 예약한다. 복제본 준비도 이 예약 안에서 일어나야
    /// 그 사이에 누른 중단이 살아남는다.
    fn reserve(&self) -> Result<String, String> {
        let mut slot = self.slot();
        if slot.is_some() {
            return Err("another handoff is already in progress".into());
        }
        let run_id = orchestration::new_run_id();
        *slot = Some(HandoffSlot {
            run_id: run_id.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
            claimed: false,
            stage: None,
        });
        Ok(run_id)
    }

    /// 실행 소유권을 잡고 취소 깃발을 받아 간다.
    /// **깃발을 초기화하지 않는다** — 준비 중에 들어온 취소가 여기서 지워지면 안 된다.
    fn claim(&self, run_id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut slot = self.slot();
        let Some(current) = slot.as_mut() else {
            return Err("this handoff was already finished; start it again".into());
        };
        if current.run_id != run_id {
            return Err("another handoff is already in progress".into());
        }
        if current.claimed {
            return Err("this handoff is already running".into());
        }
        current.claimed = true;
        Ok(current.cancel.clone())
    }

    fn set_stage(&self, run_id: &str, stage: HandoffStage) {
        let mut slot = self.slot();
        if let Some(current) = slot.as_mut() {
            if current.run_id == run_id {
                current.stage = Some(stage);
            }
        }
    }

    /// 취소를 요청한다. 반환값은 **Codex 실행에도 전달해야 하는가**다.
    fn request_cancel(&self) -> bool {
        let mut slot = self.slot();
        match slot.as_mut() {
            Some(current) => {
                current.cancel.store(true, Ordering::Relaxed);
                current.stage == Some(HandoffStage::Building)
            }
            None => false,
        }
    }

    fn release(&self, run_id: &str) {
        let mut slot = self.slot();
        if slot.as_ref().is_some_and(|c| c.run_id == run_id) {
            *slot = None;
        }
    }
}

/// 실행이 어떻게 끝나든 슬롯을 연다. 패닉으로 빠져나가도 다음 실행이 막히지 않게.
struct SlotRelease {
    session: HandoffSession,
    run_id: String,
}

impl Drop for SlotRelease {
    fn drop(&mut self) {
        self.session.release(&self.run_id);
    }
}

// ───────────────────────── 세션 잠금 ─────────────────────────

/// 디스크 표시를 남기지 못한 복제본을 **이 세션에서** 막는 목록.
///
/// `validate_pilot_workspace`는 디스크 표시를 보고 재사용을 막는다. 그런데 그 표시를
/// 쓰지 못했다면(디스크 가득참·권한) 잠금이 존재하지 않는다. 그때도 최소한
/// 지금 실행 중인 앱에서는 같은 복제본이 다시 쓰이지 않아야 한다.
static SESSION_LOCKS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn lock_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn locks() -> std::sync::MutexGuard<'static, BTreeSet<PathBuf>> {
    SESSION_LOCKS.lock().unwrap_or_else(|e| e.into_inner())
}

/// 복제본을 잠근다. **세션 잠금이 먼저다** — 디스크 표시가 실패해도 재사용은 막힌다.
fn lock_workspace(workspace: &Path, reason: &str) -> (WorkspaceLock, Option<String>) {
    locks().insert(lock_key(workspace));
    match orchestration::write_poisoned_marker(workspace, None, reason) {
        Ok(()) => (WorkspaceLock::Persisted, None),
        Err(error) => (WorkspaceLock::SessionOnly, Some(error)),
    }
}

/// 전역 잠금 목록을 테스트 사이에 흘리지 않기 위한 정리 수단.
#[cfg(test)]
fn clear_session_lock(workspace: &Path) {
    locks().remove(&lock_key(workspace));
}

fn ensure_reusable(workspace: &Path) -> Result<(), String> {
    if locks().contains(&lock_key(workspace)) {
        return Err(
            "this pilot copy was locked after a failed run and cannot be reused; \
                    discard it and start again"
                .into(),
        );
    }
    Ok(())
}

// ───────────────────────── 계획 역할 실행 ─────────────────────────

/// 계획 역할 실행의 산출물.
struct PlanRun {
    plan: PlanResult,
    /// 실행 세션 식별자. Claude는 `system:init`, Codex는 `thread.started`에서 온다
    session_id: String,
    /// 원장 기록용 CLI 판올림 정보. Codex는 이벤트로 주지 않아 비어 있을 수 있다
    cli_version: String,
    /// `system:init`으로 검증한 사실들. **Claude에만 있다** —
    /// Codex는 대응 이벤트가 없어 검증 자체가 불가능하다(`codex_pilot` 머리말).
    /// 실행 흐름은 `session_id`·`cli_version`만 쓰고, 이 값은 실사용 검증이 계약 전체를
    /// 확인하는 데 쓴다(도구 집합·권한 모드·모델).
    #[cfg_attr(not(test), allow(dead_code))]
    facts: Option<InitFacts>,
    events_path: PathBuf,
    stderr_path: PathBuf,
    diff_path: PathBuf,
    /// 종료 절차 기록 — 무엇을 몇 개 추적해 어떻게 끝냈는지
    termination: String,
}

/// 실행이 계약을 지키지 못한 방식.
#[derive(Debug)]
enum PlanFailure {
    /// 복제본을 신뢰할 수 없다 — `poisoned`로 잠근다
    Poisoned(String),
    /// 복제본은 멀쩡하지만 이번 실행은 실패했다
    Failed(String),
}

struct PlanEvidence {
    events: PathBuf,
    stderr: PathBuf,
    diff: PathBuf,
    /// Codex 계획 역할의 **구조화 출력이 담기는 곳**. Claude는 쓰지 않는다
    last_message: PathBuf,
    /// Codex는 스키마를 파일로만 받는다. **복제본 밖**에 둬야 에이전트가 못 고친다
    schema: PathBuf,
}

impl PlanEvidence {
    fn new(root: &Path, workspace: &Path, run_id: &str) -> Result<Self, String> {
        Ok(Self {
            events: orchestration::pilot_run_artifact(root, workspace, run_id, PLAN_EVENTS_SUFFIX)?,
            stderr: orchestration::pilot_run_artifact(root, workspace, run_id, PLAN_STDERR_SUFFIX)?,
            diff: orchestration::pilot_run_artifact(root, workspace, run_id, PLAN_DIFF_SUFFIX)?,
            last_message: orchestration::pilot_run_artifact(
                root,
                workspace,
                run_id,
                PLAN_LAST_MESSAGE_SUFFIX,
            )?,
            schema: orchestration::pilot_run_artifact(root, workspace, run_id, PLAN_SCHEMA_SUFFIX)?,
        })
    }
}

/// UUID v4. `--session-id`가 유효한 UUID를 요구한다.
fn new_session_id() -> String {
    let mut bytes = [0u8; 16];
    // `/dev/urandom`은 끝이 없다. **정확히 16바이트만** 읽는다 —
    // 통째로 읽으면 프로세스가 멈춘다.
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let _ = file.read_exact(&mut bytes);
    }
    // 엔트로피를 못 얻어도 실행별로 갈리도록 시간·pid를 섞는다
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    for (index, byte) in stamp.to_le_bytes().iter().enumerate() {
        bytes[index] ^= byte;
    }
    for (index, byte) in std::process::id().to_le_bytes().iter().enumerate() {
        bytes[8 + index] ^= byte;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// stdout 수집 결과. 읽기는 별도 스레드에서 돌고 여기에 담겨 돌아온다.
#[derive(Default)]
struct PlanOutput {
    facts: Option<InitFacts>,
    result_line: Option<String>,
    /// 실행 세션 식별자 — 원장에 남긴다
    session_id: String,
    /// CLI 판올림 정보. Codex는 이벤트로 주지 않아 비어 있다
    cli_version: String,
    /// 계약 위반 — 복제본을 신뢰할 수 없다
    violation: Option<String>,
    /// 증거를 남기지 못했다. **삼키지 않는다** — 기록 없는 실행은 증명할 수 없다
    evidence_error: Option<String>,
}

/// 종료 절차를 주입 가능하게 둔다. 실제로 안 죽는 트리를 만들 수는 없으므로,
/// **확인 실패 이후의 행동**(제한된 회수 후 즉시 poisoned)은 이 주입으로만 검증된다.
type TerminateFn = fn(&Arc<Mutex<TreeTracker>>) -> TerminationReport;

/// stdout을 읽어 증거로 남기고 계획 재료를 모으는 방식. 에이전트마다 다르다.
type ReadStream = fn(ChildStdout, std::fs::File, PathBuf, String) -> PlanOutput;

/// 계획 역할을 **어느 에이전트가 어떻게** 수행하는가.
///
/// 감독(신원 추적·취소·종료·증거)은 두 에이전트가 **똑같이** 받는다 —
/// 종료 계약(F1~F8)이 갈라지면 한쪽만 고쳐지는 날이 온다.
/// 다른 것은 이 네 가지뿐이다.
struct PlanRole {
    /// 오류 문구에 쓰는 이름
    name: &'static str,
    args: Vec<String>,
    /// stdin으로 보낼 프롬프트. `None`이면 인자에 이미 실려 있다
    stdin_prompt: Option<String>,
    read: ReadStream,
    /// 감독이 끝난 뒤 계획을 뽑는다
    extract: fn(&PlanOutput, &PlanEvidence) -> Result<PlanResult, String>,
}

/// 프로젝트의 에이전트에게 **읽기 전용 계획 역할**을 맡긴다.
///
/// 프로젝트를 만들 때 이미 고른 에이전트를 그대로 쓴다 — Codex로 만든 프로젝트에
/// 말없이 Claude를 끌어오지 않는다(§7.6A 2차 실사용에서 드러난 결함).
fn run_plan(
    agent: crate::agent::Agent,
    workspace: &PilotWorkspace,
    goal: &str,
    evidence: &PlanEvidence,
    cancel: &AtomicBool,
) -> Result<PlanRun, PlanFailure> {
    let program = crate::detect::agent_bin(agent)
        .ok_or_else(|| PlanFailure::Failed(format!("{} is not installed", agent.bin_name())))?;
    let role = plan_role(agent, goal, evidence)?;
    supervise_plan(&program, role, workspace, evidence, cancel, terminate)
}

fn plan_role(
    agent: crate::agent::Agent,
    goal: &str,
    evidence: &PlanEvidence,
) -> Result<PlanRole, PlanFailure> {
    Ok(match agent {
        crate::agent::Agent::ClaudeCode => PlanRole {
            name: "claude",
            // 세션 ID를 우리가 정해 주고 `system:init`에서 되돌아오는지 검증한다
            args: claude_pilot::claude_pilot_args(&new_session_id()),
            // 프롬프트는 **stdin**으로 — 변수 인자 플래그가 뒤따르는 프롬프트를 삼킨다(실측)
            stdin_prompt: Some(planning_prompt(goal)),
            read: read_claude_stream,
            extract: extract_claude_plan,
        },
        crate::agent::Agent::Codex => {
            // 스키마는 파일로만 받는다. 복제본 밖에 미리 써 둔다
            std::fs::write(&evidence.schema, codex_pilot::PLAN_SCHEMA).map_err(|e| {
                PlanFailure::Failed(format!(
                    "cannot write the plan schema '{}': {e}",
                    evidence.schema.display()
                ))
            })?;
            let mut args = codex_pilot::codex_plan_args(
                &evidence.schema, // 자리표는 아래에서 실제 경로로 바뀐다
                &evidence.schema,
                &evidence.last_message,
            )
            .map_err(PlanFailure::Failed)?;
            // 마지막 인자가 프롬프트 자리표다. Codex는 프롬프트를 argv로 받는다
            let last = args.len() - 1;
            args[last] = planning_prompt(goal);
            PlanRole {
                name: "codex",
                args,
                stdin_prompt: None,
                read: read_codex_stream,
                extract: extract_codex_plan,
            }
        }
    })
}

fn supervise_plan(
    program: &Path,
    role: PlanRole,
    workspace: &PilotWorkspace,
    evidence: &PlanEvidence,
    cancel: &AtomicBool,
    terminate: TerminateFn,
) -> Result<PlanRun, PlanFailure> {
    let agent = role.name;
    let args = role.args;
    // ── 증거 파일을 **자식보다 먼저** 만든다 ──
    // 띄운 뒤에 만들다 실패하면 프로세스와 감시 스레드가 붕 뜬다. 순서가 곧 방어다.
    let events = std::fs::File::create(&evidence.events).map_err(|e| {
        PlanFailure::Failed(format!(
            "cannot create the {agent} event log '{}': {e}",
            evidence.events.display()
        ))
    })?;
    let stderr_file = std::fs::File::create(&evidence.stderr).map_err(|e| {
        PlanFailure::Failed(format!(
            "cannot create the {agent} stderr log '{}': {e}",
            evidence.stderr.display()
        ))
    })?;
    // Claude는 우리가 정한 세션 ID가 `init`으로 되돌아오는지 검증한다.
    // Codex는 세션 ID를 스스로 발급해 `thread.started`로 알려 준다.
    let expected_session = args
        .iter()
        .position(|a| a == "--session-id")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_default();

    let mut command = crate::detect::command(program);
    command
        .args(&args)
        .current_dir(&workspace.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command
        .spawn()
        .map_err(|e| PlanFailure::Failed(format!("cannot start {agent}: {e}")))?;

    // 신원을 얻지 못하면 추적을 시작할 수 없다 → 복제본을 신뢰할 수 없다.
    let identity = match proctree::identity_of(child.id()) {
        Ok(identity) => identity,
        Err(e) => {
            bounded_reap(&mut child);
            return Err(PlanFailure::Poisoned(format!(
                "cannot track the {agent} process tree: {e}"
            )));
        }
    };
    let tracker = Arc::new(Mutex::new(TreeTracker::new(identity)));

    // 프롬프트를 stdin으로 받는 에이전트만 여기서 쓴다.
    // 쓰기도 별도 스레드다 — 상대가 읽지 않으면 파이프가 차서 여기서 막힌다.
    let stdin_handle = match (role.stdin_prompt, child.stdin.take()) {
        (Some(prompt), Some(mut stdin)) => Some(std::thread::spawn(move || {
            stdin
                .write_all(prompt.as_bytes())
                .map_err(|e| format!("cannot send the prompt to {agent}: {e}"))
        })),
        // 인자로 넘긴 경우엔 stdin을 곧바로 닫는다. 열어 두면 상대가 입력을 기다린다
        _ => None,
    };

    // stderr는 별도 파일로 받는다. 정상 실행에서는 비어 있고, 프로세스 오류가 여기로 온다.
    let stderr_handle = child
        .stderr
        .take()
        .map(|stderr| std::thread::spawn(move || drain_stderr(stderr, stderr_file)));

    // 실행 **중에도** 후손 신원을 누적한다. 종료 시점에만 열거하면 이미 고아가 된
    // 배경 자손을 발견할 경로가 없다(§7.10.1.2 ⑤ F6).
    let watching = Arc::new(AtomicBool::new(true));
    let watcher = {
        let tracker = tracker.clone();
        let watching = watching.clone();
        std::thread::spawn(move || {
            let table = SystemProcTable;
            while watching.load(Ordering::Relaxed) {
                if let Ok(mut tracker) = tracker.lock() {
                    let _ = tracker.observe(&table);
                }
                std::thread::sleep(Duration::from_millis(OBSERVE_INTERVAL_MS));
            }
        })
    };

    // stdout(NDJSON) 읽기도 별도 스레드다. **읽기 루프 안에서 취소를 보면 안 된다** —
    // 아무것도 출력하지 않는 실행에서는 그 검사에 영영 닿지 못한다.
    let workspace_path = workspace.path.clone();
    let read = role.read;
    let reader = child.stdout.take().map(|stdout| {
        std::thread::spawn(move || read(stdout, events, workspace_path, expected_session))
    });

    // 취소는 stdout이 조용해도 이 주기 안에 닿는다.
    let canceled = loop {
        if cancel.load(Ordering::Relaxed) {
            break true;
        }
        match reader.as_ref() {
            Some(handle) if !handle.is_finished() => {}
            _ => break false,
        }
        std::thread::sleep(Duration::from_millis(CANCEL_POLL_MS));
    };

    // ── 종료 — 취소·성공·실패 **모든 경로**에서 트리를 회수한다(§7.10.1.2 ⑤ 6) ──
    let report = terminate(&tracker);
    let termination = termination_text(&report);
    watching.store(false, Ordering::Relaxed);

    if !report.confirmed() {
        // 살아남은 프로세스가 파이프를 물고 있다. **어떤 blocking 대기도 하지 않는다** —
        // `wait()`도 `join()`도 돌아오지 않을 수 있다. 제한된 회수만 하고 즉시 잠근다.
        bounded_reap(&mut child);
        drop(reader);
        drop(watcher);
        drop(stderr_handle);
        drop(stdin_handle);
        return Err(PlanFailure::Poisoned(format!(
            "{agent} process tree could not be confirmed gone: {} ({} tracked, survivors: {})",
            report.detail,
            report.tracked.len(),
            survivor_text(&report)
        )));
    }

    // 트리 소멸을 확인했으므로 여기서부터의 대기는 반드시 끝난다.
    let _ = child.wait();
    let output = reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let stderr_error = stderr_handle
        .and_then(|handle| handle.join().ok())
        .and_then(|result| result.err());
    let stdin_error = stdin_handle
        .and_then(|handle| handle.join().ok())
        .and_then(|result| result.err());
    let _ = watcher.join();

    if let Some(violation) = output.violation {
        // 계약이 깨진 세션은 복제본을 신뢰할 수 없다
        return Err(PlanFailure::Poisoned(format!(
            "{agent} execution contract violated: {violation}"
        )));
    }
    if canceled {
        return Err(PlanFailure::Failed("canceled by the user".into()));
    }
    // 증거를 남기지 못한 실행은 통과시키지 않는다. 기록이 없으면 증명도 없다.
    if let Some(error) = output.evidence_error.clone().or(stderr_error) {
        return Err(PlanFailure::Failed(format!(
            "the run could not be recorded: {error}"
        )));
    }
    if let Some(error) = stdin_error {
        return Err(PlanFailure::Failed(error));
    }

    // 계획 역할이 정말 아무것도 바꾸지 않았는지 **관측으로** 확인한다.
    // 이것은 예방책이 아니라 사후 확인이다(§7.10.1.2 ④).
    orchestration::write_pilot_diff(workspace, &evidence.diff).map_err(PlanFailure::Failed)?;
    if diff_has_changes(&evidence.diff) {
        return Err(PlanFailure::Poisoned(
            "the planning role modified the clone; its contents can no longer be trusted".into(),
        ));
    }

    let plan = (role.extract)(&output, evidence).map_err(PlanFailure::Failed)?;

    Ok(PlanRun {
        plan,
        session_id: output.session_id,
        cli_version: output.cli_version,
        facts: output.facts,
        events_path: evidence.events.clone(),
        stderr_path: evidence.stderr.clone(),
        diff_path: evidence.diff.clone(),
        termination,
    })
}

/// NDJSON을 증거로 남기면서 `system:init` 검증과 결과 수집을 한다.
fn read_claude_stream(
    stdout: ChildStdout,
    mut events: std::fs::File,
    workspace: PathBuf,
    session_id: String,
) -> PlanOutput {
    let mut out = PlanOutput::default();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if let Err(e) = writeln!(events, "{line}") {
            out.evidence_error = Some(format!("cannot write the claude event log: {e}"));
            break;
        }
        if out.facts.is_none() {
            match claude_pilot::parse_init(&line) {
                Ok(parsed) => match claude_pilot::validate_init(&parsed, &workspace, &session_id) {
                    Ok(()) => {
                        out.session_id = parsed.session_id.clone();
                        out.cli_version = parsed.cli_version.clone();
                        out.facts = Some(parsed);
                    }
                    Err(e) => {
                        out.violation = Some(e.0);
                        break;
                    }
                },
                Err(e) => {
                    out.violation = Some(e.0);
                    break;
                }
            }
            continue;
        }
        if line.contains("\"type\":\"result\"") {
            out.result_line = Some(line);
        }
    }
    if let Err(e) = events.flush() {
        out.evidence_error
            .get_or_insert(format!("cannot flush the claude event log: {e}"));
    }
    out
}

/// Codex의 NDJSON을 증거로 남기며 세션 ID만 건진다.
///
/// **검증할 것이 없다.** Codex는 `thread.started{thread_id}`만 주고 어떤 샌드박스·도구로
/// 떴는지 알려주지 않는다 — Claude의 `system:init` fail-closed 검증에 대응하는 것이 없다.
/// 그래서 이 경로의 근거는 argv 통제와 실측된 샌드박스, 그리고 사후 빈 diff다
/// (`codex_pilot` 머리말).
fn read_codex_stream(
    stdout: ChildStdout,
    mut events: std::fs::File,
    _workspace: PathBuf,
    _session_id: String,
) -> PlanOutput {
    let mut out = PlanOutput::default();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if let Err(e) = writeln!(events, "{line}") {
            out.evidence_error = Some(format!("cannot write the codex event log: {e}"));
            break;
        }
        if out.session_id.is_empty() {
            if let Some(id) = codex_session_id(&line) {
                out.session_id = id;
            }
        }
    }
    if let Err(e) = events.flush() {
        out.evidence_error
            .get_or_insert(format!("cannot flush the codex event log: {e}"));
    }
    out
}

fn codex_session_id(line: &str) -> Option<String> {
    let event: serde_json::Value = serde_json::from_str(line).ok()?;
    if event.get("type")?.as_str()? != "thread.started" {
        return None;
    }
    Some(event.get("thread_id")?.as_str()?.to_string())
}

/// Claude는 `result` 이벤트의 구조화 출력이 계획이다.
fn extract_claude_plan(
    output: &PlanOutput,
    _evidence: &PlanEvidence,
) -> Result<PlanResult, String> {
    if output.facts.is_none() {
        return Err("claude produced no init event".into());
    }
    let line = output
        .result_line
        .as_deref()
        .ok_or("claude produced no result")?;
    claude_pilot::parse_result(line)
}

/// Codex는 **최종 메시지 파일이 곧 구조화 출력**이다(실측).
fn extract_codex_plan(_output: &PlanOutput, evidence: &PlanEvidence) -> Result<PlanResult, String> {
    let text = std::fs::read_to_string(&evidence.last_message).map_err(|e| {
        format!(
            "cannot read the codex plan '{}': {e}",
            evidence.last_message.display()
        )
    })?;
    codex_pilot::parse_codex_plan(&text)
}

fn drain_stderr(stderr: ChildStderr, mut file: std::fs::File) -> Result<(), String> {
    let mut buffer = Vec::new();
    BufReader::new(stderr)
        .read_to_end(&mut buffer)
        .map_err(|e| format!("cannot read the claude stderr stream: {e}"))?;
    file.write_all(&buffer)
        .map_err(|e| format!("cannot write the claude stderr log: {e}"))
}

/// 종료를 확인하지 못한 뒤의 정리. **blocking `wait()`를 하지 않는다.**
fn bounded_reap(child: &mut Child) {
    let _ = child.kill();
    for _ in 0..REAP_TRIES {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(REAP_INTERVAL_MS));
    }
}

fn terminate(tracker: &Arc<Mutex<TreeTracker>>) -> TerminationReport {
    let table = SystemProcTable;
    let clock = SystemClock::default();
    match tracker.lock() {
        Ok(mut tracker) => tracker.terminate(&table, &clock, TerminationConfig::default()),
        // 락이 깨졌다면 소멸을 주장할 수 없다
        Err(_) => TerminationReport {
            outcome: proctree::TerminationOutcome::Unconfirmed(
                proctree::UnconfirmedReason::ProbeFailed,
            ),
            detail: "termination tracker lock was poisoned".into(),
            survivors: Vec::new(),
            tracked: Vec::new(),
            rounds: 0,
            signal_errors: Vec::new(),
        },
    }
}

/// 종료 절차 기록을 증거 한 줄로 만든다. 회차·신호 오류까지 남긴다 —
/// `EPERM`은 판정에 쓰지 않지만 **왜 그렇게 판정했는지**를 설명하려면 필요하다.
fn termination_text(report: &TerminationReport) -> String {
    let mut text = format!(
        "{:?} after {} round(s); {} identity(ies) tracked; survivors: {}",
        report.outcome,
        report.rounds,
        report.tracked.len(),
        survivor_text(report)
    );
    if !report.signal_errors.is_empty() {
        text.push_str(&format!(
            "; signal errors (not used for the verdict): {}",
            report.signal_errors.join(" | ")
        ));
    }
    text
}

fn survivor_text(report: &TerminationReport) -> String {
    if report.survivors.is_empty() {
        return "none observed".into();
    }
    report
        .survivors
        .iter()
        .map(|s| format!("{} ({})", s.identity.pid, s.comm))
        .collect::<Vec<_>>()
        .join(", ")
}

fn diff_has_changes(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() > 0)
        .unwrap_or(false)
}

/// 계획 역할에게 주는 지시. **파일을 바꾸지 말라고 말하되, 그것에 기대지 않는다** —
/// 실제 경계는 `--tools` 허용목록이다.
fn planning_prompt(goal: &str) -> String {
    format!(
        "You are the planning role. Inspect this repository and produce an implementation plan \
         for the user's goal. You cannot modify files and must not try. Another agent will \
         implement your plan.\n\nUser goal: {goal}\n"
    )
}

// ───────────────────────── 결과 수집 ─────────────────────────

/// `git diff --name-status -z -M` 출력을 사람이 볼 목록으로 바꾼다.
///
/// 레코드는 `상태\0경로\0`이고, 이름 변경·복사만 `상태\0원본\0대상\0`으로 하나 더 온다.
fn parse_name_status(raw: &str) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    let mut fields = raw.split('\0').filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let kind = ChangeKind::from_status(status);
        let entry = if kind.takes_two_paths() {
            match (fields.next(), fields.next()) {
                (Some(from), Some(to)) => ChangedFile {
                    path: to.to_string(),
                    kind,
                    from: Some(from.to_string()),
                    status: status.to_string(),
                },
                // 잘린 출력을 추측으로 메우지 않는다
                _ => break,
            }
        } else {
            let Some(path) = fields.next() else { break };
            ChangedFile {
                path: path.to_string(),
                kind,
                from: None,
                status: status.to_string(),
            }
        };
        out.push(entry);
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn changed_files(workspace: &PilotWorkspace) -> Result<Vec<ChangedFile>, String> {
    Ok(parse_name_status(&orchestration::pilot_name_status(
        workspace,
    )?))
}

/// 구현 단계의 **실제 결과**. 계획 요약으로 대신하면 "하려던 것"을 "한 것"이라 말하게 된다.
fn read_final_message(path: &str) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let text = text.trim();
    if text.chars().count() <= RESULT_SUMMARY_LIMIT {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(RESULT_SUMMARY_LIMIT).collect();
    cut.push('…');
    cut
}

// ───────────────────────── 취소 전달 ─────────────────────────

/// 구현 구간에서 취소를 Codex 실행까지 전달하는 다리.
///
/// `cancel_handoff`가 직접 전달하지만, **슬롯이 예약되기 직전에 들어온 취소**는 놓친다 —
/// 그 순간에는 끊을 대상이 아직 없기 때문이다. 그 창을 닫으려고 실행 구간 내내 다시 시도한다.
struct CancelBridge {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<bool>>,
}

type CancelForward = Arc<dyn Fn() -> Result<bool, String> + Send + Sync>;

impl CancelBridge {
    fn spawn(cancel: Arc<AtomicBool>, forward: CancelForward) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let stop = stop.clone();
            std::thread::spawn(move || loop {
                if cancel.load(Ordering::Relaxed) {
                    match forward() {
                        // 끊었거나, 끊다 실패했다. 실패는 실행 소유자가 poisoned로 처리한다
                        Ok(true) | Err(_) => return true,
                        // 아직 끊을 대상이 없다 — 다음 회차에 다시 시도한다
                        Ok(false) => {}
                    }
                }
                // 접으라는 신호는 **취소를 한 번 더 확인한 뒤에** 받는다.
                // 먼저 확인하면 실행이 끝나기 직전에 들어온 취소를 그대로 흘린다.
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(CANCEL_POLL_MS));
            })
        };
        Self {
            stop,
            handle: Some(handle),
        }
    }

    /// 다리를 접고 **취소가 실제로 전달됐는지** 알려 준다.
    fn finish(mut self) -> bool {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or(false)
    }
}

// ───────────────────────── Tauri 명령 ─────────────────────────

/// 흐름을 시작할 수 있는가, 아니면 먼저 치워야 할 것이 있는가.
///
/// ⚠️ `rename_all`은 **변형 이름만** 바꾼다(`Ready` → `"ready"`). 변형 **안의 필드**까지
/// 바꾸려면 `rename_all_fields`가 따로 필요하다 — 빠뜨리면 `run_id`가 그대로 나가고
/// 화면은 `runId`를 읽어 `undefined`가 된다. 그러면 그 인자는 통째로 사라진다.
/// (`InstallEvent`가 같은 이유로 둘 다 달고 있다.)
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum HandoffStart {
    /// 시작해도 된다
    Ready { run_id: String },
    /// **앞선 복제본이 잠겨 있어 시작할 수 없다.** 그것부터 정리해야 한다
    Blocked {
        blocker: orchestration::BlockedWorkspace,
    },
}

/// 흐름을 예약한다. **복제본 준비보다 먼저** 불려야 그 사이에 누른 중단이 살아남는다.
#[tauri::command]
pub(crate) fn begin_handoff(
    session: State<'_, HandoffSession>,
    pilot: State<'_, orchestration::PilotSession>,
) -> Result<HandoffStart, crate::error::AppError> {
    if !handoff_available() {
        return Err(crate::error::AppError::generic(
            "the automatic handoff pilot is available on macOS only",
        ));
    }
    start_handoff(&HandoffSession(session.0.clone()), &pilot.0)
        .map_err(crate::error::AppError::generic)
}

/// 잠긴 복제본이 있으면 **새 사본을 만들기 전에** 막는다.
///
/// 막지 않으면 이렇게 된다: 잠긴 A가 슬롯을 쥔 채로 새 B를 만들고, B의 실행은 거부되고,
/// 화면에는 B만 보인다. B를 아무리 정리해도 슬롯은 A의 것이라 풀리지 않아
/// **모든 후속 실행이 계속 거부된다.** 사용자는 무엇을 치워야 하는지 알 수도 없다.
///
/// 그래서 A를 먼저 보여 주고, A를 정리·복구한 뒤에 새 실행을 시작하게 한다.
fn start_handoff(
    session: &HandoffSession,
    pilot_handle: &Arc<Mutex<Option<orchestration::RunSlot>>>,
) -> Result<HandoffStart, String> {
    if let Some(blocker) = orchestration::blocking_workspace(pilot_handle) {
        return Ok(HandoffStart::Blocked { blocker });
    }
    session
        .reserve()
        .map(|run_id| HandoffStart::Ready { run_id })
}

/// 예약만 하고 시작하지 못한 경우(준비 실패 등) 슬롯을 돌려준다.
#[tauri::command]
pub(crate) fn release_handoff(session: State<'_, HandoffSession>, run_id: String) {
    session.release(&run_id);
}

/// 자동 인계 한 번을 처음부터 끝까지 실행한다.
///
/// **macOS 전용**이다. 다른 플랫폼에서는 종료 계약을 지킬 수 없으므로 실행하지 않는다.
#[tauri::command]
pub(crate) async fn run_handoff(
    app: tauri::AppHandle,
    session: State<'_, HandoffSession>,
    pilot: State<'_, orchestration::PilotSession>,
    run_id: String,
    workspace_path: String,
    // 프로젝트 카드가 보여 준 그 에이전트. 화면과 실행이 갈리면 안 된다
    agent: String,
    goal: String,
) -> Result<HandoffResult, crate::error::AppError> {
    if !handoff_available() {
        return Err(crate::error::AppError::generic(
            "the automatic handoff pilot is available on macOS only",
        ));
    }
    let agent = crate::agent::Agent::from_id(&agent)?;
    let root = orchestration::pilot_root(&app)?;
    let handle = HandoffSession(session.0.clone());
    // 깃발을 **초기화하지 않고** 받아 온다 — 준비 중에 들어온 취소를 지우면 안 된다
    let cancel = handle
        .claim(&run_id)
        .map_err(crate::error::AppError::generic)?;
    let pilot_handle = pilot.0.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _release = SlotRelease {
            session: handle.clone(),
            run_id: run_id.clone(),
        };
        let announce = |stage: HandoffStage| {
            handle.set_stage(&run_id, stage);
            let _ = app.emit(
                "handoff:stage",
                StageEvent {
                    run_id: run_id.clone(),
                    stage,
                },
            );
        };
        execute_handoff(
            &announce,
            &root,
            Path::new(&workspace_path),
            agent,
            &goal,
            &cancel,
            &pilot_handle,
        )
        .map_err(crate::error::AppError::classify)
    })
    .await
    .map_err(|e| crate::error::AppError::generic(e.to_string()))?
}

/// 진행 중인 인계를 중단 요청한다.
///
/// 깃발은 슬롯이 소유하므로 **준비 중에 눌러도 실행 시작으로 지워지지 않는다.**
/// 구현 중이라면 Codex 실행에도 곧바로 전달한다.
#[tauri::command]
pub(crate) fn cancel_handoff(
    session: State<'_, HandoffSession>,
    pilot: State<'_, orchestration::PilotSession>,
) -> Result<(), crate::error::AppError> {
    if session.request_cancel() {
        orchestration::cancel_pilot_child(&pilot.0)
            .map(|_| ())
            .map_err(crate::error::AppError::generic)?;
    }
    Ok(())
}

/// 인계 결과의 복제본을 폐기한다 — **"결과 버리기"의 실제 동작**이다.
///
/// `discard_codex_pilot`은 `poisoned` 표시가 있는 복제본만 다루므로 정상 완료·단순 취소·
/// `sessionOnly` 결과에서는 전부 실패한다. 사용자는 어느 결과든 버릴 수 있어야 한다.
#[tauri::command]
pub(crate) async fn discard_handoff_workspace(
    app: tauri::AppHandle,
    pilot: State<'_, orchestration::PilotSession>,
    workspace_path: String,
    confirm_discard_workspace: bool,
) -> Result<String, crate::error::AppError> {
    let root = orchestration::pilot_root(&app)?;
    let pilot_handle = pilot.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        discard_handoff_copy(
            Path::new(&workspace_path),
            &root,
            &pilot_handle,
            confirm_discard_workspace,
        )
        .map_err(crate::error::AppError::classify)
    })
    .await
    .map_err(|e| crate::error::AppError::generic(e.to_string()))?
}

fn discard_handoff_copy(
    workspace: &Path,
    root: &Path,
    pilot_handle: &Arc<Mutex<Option<orchestration::RunSlot>>>,
    confirmed: bool,
) -> Result<String, String> {
    let discarded = orchestration::abandon_pilot_copy(workspace, root, pilot_handle, confirmed)?;
    // 세션 잠금은 이제 **영구 폐기 표시**로 대체됐다. 디스크 표시가 재사용을 막으므로
    // 프로세스 내 목록에 남겨 둘 이유가 없다.
    let mut locks = locks();
    locks.remove(&lock_key(workspace));
    locks.remove(&lock_key(Path::new(&discarded)));
    Ok(discarded)
}

/// 이 기기에서 자동 인계 파일럿을 쓸 수 있는가.
///
/// **macOS 전용**이다(§7.10.1.2 ⑥). 종료 계약이 ppid 후손 반복 열거와 마이크로초 신원에
/// 기대는데 Windows 대응물을 실측하지 않았다. 실측 전까지 화면에도 노출하지 않는다.
#[tauri::command]
pub(crate) fn handoff_available() -> bool {
    cfg!(target_os = "macos")
}

/// 진행 상태를 알리는 통로. **역할 이름이 아니라 일의 이름**을 보낸다(§7.10.2) —
/// 사용자는 어느 에이전트가 도는지 몰라도 흐름을 이해할 수 있어야 한다.
/// 주입 가능하게 둬서 Tauri 창 없이도 전체 흐름을 검증할 수 있다.
type Announce<'a> = &'a dyn Fn(HandoffStage);

fn execute_handoff(
    announce_stage: Announce<'_>,
    root: &Path,
    workspace_path: &Path,
    // 프로젝트를 만들 때 고른 에이전트. **계획도 구현도 이 에이전트가 한다** —
    // Codex로 만든 프로젝트에 말없이 Claude를 끌어오지 않는다.
    agent: crate::agent::Agent,
    goal: &str,
    cancel: &Arc<AtomicBool>,
    pilot_handle: &Arc<Mutex<Option<orchestration::RunSlot>>>,
) -> Result<HandoffResult, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve pilot root '{}': {e}", root.display()))?;
    // 디스크 표시를 남기지 못해 잠긴 복제본도 여기서 걸린다
    ensure_reusable(workspace_path)?;
    let workspace = orchestration::validate_pilot_workspace(workspace_path, &root)?;
    ensure_reusable(&workspace.path)?;
    let run_id = orchestration::new_run_id();
    let evidence = PlanEvidence::new(&root, &workspace.path, &run_id)?;

    let claude_paths = || HandoffEvidence {
        plan_events_path: evidence.events.display().to_string(),
        plan_stderr_path: evidence.stderr.display().to_string(),
        plan_diff_path: evidence.diff.display().to_string(),
        ..Default::default()
    };

    // 준비 중에 누른 중단이 여기 살아 있다. 실행을 시작하지 않는다.
    if cancel.load(Ordering::Relaxed) {
        announce_stage(HandoffStage::NeedsAttention);
        return Ok(stopped(
            goal,
            &workspace,
            "canceled by the user".into(),
            WorkspaceLock::None,
            None,
            claude_paths(),
        ));
    }

    // ── 1단계: 요청 정리 (Claude, 읽기 전용) ──
    announce_stage(HandoffStage::Organizing);
    let planned = match run_plan(agent, &workspace, goal, &evidence, cancel) {
        Ok(run) => run,
        Err(PlanFailure::Poisoned(reason)) => {
            // 복제본을 잠근다. **Codex 자동 인계는 여기서 멈춘다** — 재사용도 막힌다.
            let (lock, lock_error) = lock_workspace(&workspace.path, &reason);
            announce_stage(HandoffStage::NeedsAttention);
            return Ok(stopped(
                goal,
                &workspace,
                reason,
                lock,
                lock_error,
                claude_paths(),
            ));
        }
        Err(PlanFailure::Failed(reason)) => {
            announce_stage(HandoffStage::NeedsAttention);
            return Ok(stopped(
                goal,
                &workspace,
                reason,
                WorkspaceLock::None,
                None,
                claude_paths(),
            ));
        }
    };

    let mut collected = HandoffEvidence {
        plan_events_path: planned.events_path.display().to_string(),
        plan_stderr_path: planned.stderr_path.display().to_string(),
        plan_diff_path: planned.diff_path.display().to_string(),
        plan_session_id: planned.session_id.clone(),
        plan_cli_version: planned.cli_version.clone(),
        permission_denials: planned.plan.permission_denials.clone(),
        termination: planned.termination.clone(),
        ..Default::default()
    };

    if cancel.load(Ordering::Relaxed) {
        announce_stage(HandoffStage::NeedsAttention);
        return Ok(stopped(
            goal,
            &workspace,
            "canceled by the user".into(),
            WorkspaceLock::None,
            None,
            collected,
        ));
    }

    // 계획 역할이 **할 일이 없다**고 답했다. 잘못된 것은 없다 —
    // 무엇을 바꿀지 알아야 할 뿐이다. 이걸 사고와 같은 화면으로 보내면
    // 사용자는 자기가 무언가 망가뜨린 줄 안다.
    if planned.plan.steps.is_empty() && !planned.plan.is_error {
        announce_stage(HandoffStage::NeedsGoal);
        return Ok(HandoffResult {
            stage: HandoffStage::NeedsGoal,
            goal: goal.to_string(),
            // 계획 역할이 왜 할 일이 없다고 보는지 써 줬다. 상세에서 볼 수 있게 남긴다
            plan_summary: planned.plan.summary.clone(),
            result_summary: String::new(),
            changed_files: Vec::new(),
            // **비어 있다.** 확인해야 할 사고가 아니다
            attention: None,
            workspace_lock: WorkspaceLock::None,
            lock_error: None,
            blocker: None,
            workspace_path: workspace.path.display().to_string(),
            evidence: collected,
        });
    }

    // ── 2단계: 앱이 계획을 인계한다. 사용자는 아무것도 옮기지 않는다 ──
    // 실패로 끝난 계획은 여기서 거부된다(`plan_to_implementation_prompt`).
    let prompt = match claude_pilot::plan_to_implementation_prompt(goal, &planned.plan) {
        Ok(prompt) => prompt,
        Err(reason) => {
            announce_stage(HandoffStage::NeedsAttention);
            return Ok(stopped(
                goal,
                &workspace,
                reason,
                WorkspaceLock::None,
                None,
                collected,
            ));
        }
    };

    // ── 3단계: 구현 (Codex, S2 실행 경계 그대로) ──
    announce_stage(HandoffStage::Building);
    let bridge = CancelBridge::spawn(cancel.clone(), {
        let handle = pilot_handle.clone();
        Arc::new(move || orchestration::cancel_pilot_child(&handle))
    });
    let codex = orchestration::execute_pilot(&root, &workspace.path, &prompt, pilot_handle);
    let forwarded_cancel = bridge.finish();

    let codex = match codex {
        Ok(run) => run,
        Err(failure) => {
            announce_stage(HandoffStage::NeedsAttention);
            return Ok(implementation_failure(
                goal,
                &workspace,
                failure,
                collected,
                pilot_handle,
            ));
        }
    };

    collected.codex_events_path = codex.events_path.clone();
    collected.codex_last_message_path = codex.last_message_path.clone();
    collected.codex_stderr_path = codex.stderr_path.clone();
    collected.codex_diff_path = codex.diff_path.clone();
    collected.codex_session_id = codex.session_id.clone().unwrap_or_default();
    collected.codex_run_id = codex.run_id.clone();

    // ── 4단계: 확인 — 무엇이 바뀌었는지 앱이 실제로 확인한다 ──
    announce_stage(HandoffStage::Checking);
    let changed = match changed_files(&workspace) {
        Ok(changed) => changed,
        Err(error) => {
            // 목록을 못 읽었으면서 "바뀐 게 없다"고 말하지 않는다
            announce_stage(HandoffStage::NeedsAttention);
            return Ok(stopped(
                goal,
                &workspace,
                format!("the list of changed files could not be read: {error}"),
                WorkspaceLock::None,
                None,
                collected,
            ));
        }
    };
    let result_summary = read_final_message(&codex.last_message_path);

    let canceled = forwarded_cancel || cancel.load(Ordering::Relaxed);
    let stage = match codex.outcome {
        orchestration::PilotOutcome::Completed if !canceled => HandoffStage::Done,
        _ => HandoffStage::NeedsAttention,
    };
    let attention = match (stage, codex.outcome) {
        (HandoffStage::Done, _) => None,
        (_, orchestration::PilotOutcome::Failed) => {
            Some("the implementation step did not finish".to_string())
        }
        _ => Some("canceled by the user".to_string()),
    };

    announce_stage(stage);
    Ok(HandoffResult {
        stage,
        goal: goal.to_string(),
        plan_summary: planned.plan.summary.clone(),
        result_summary,
        changed_files: changed,
        attention,
        workspace_lock: WorkspaceLock::None,
        lock_error: None,
        blocker: None,
        workspace_path: workspace.path.display().to_string(),
        evidence: collected,
    })
}

/// 구현 단계 실패를 사용자에게 돌려줄 결과로 바꾼다.
///
/// 오류를 그대로 던지면 화면에는 빨간 문구만 남고 **복제본 경로도 잠금 상태도 증거 경로도
/// 사라진다.** 폐기도 복구도 못 하게 되는 것이 진짜 문제다.
///
/// 실패가 **이미 만들어 둔 Codex 증거 경로**를 싣고 오므로 여기서 그대로 옮긴다.
fn implementation_failure(
    goal: &str,
    workspace: &PilotWorkspace,
    failure: orchestration::PilotFailure,
    mut collected: HandoffEvidence,
    pilot_handle: &Arc<Mutex<Option<orchestration::RunSlot>>>,
) -> HandoffResult {
    if let Some(evidence) = failure.evidence.as_ref() {
        // **저장된 파일만 옮긴다.** 없는 파일의 경로를 "저장된 기록 위치"로 보여 주면
        // 사용자는 열리지 않는 경로를 들고 도움을 요청하게 된다.
        collected.codex_events_path = evidence.events.clone().unwrap_or_default();
        collected.codex_last_message_path = evidence.last_message.clone().unwrap_or_default();
        collected.codex_stderr_path = evidence.stderr.clone().unwrap_or_default();
        collected.codex_diff_path = evidence.diff.clone().unwrap_or_default();
    }
    collected.codex_run_id = failure.run_id.clone().unwrap_or_default();
    let (lock, lock_error) = codex_failure_lock(&workspace.path, &failure, pilot_handle);
    HandoffResult {
        blocker: failure.blocker.clone(),
        ..stopped(goal, workspace, failure.error, lock, lock_error, collected)
    }
}

/// 구현 단계가 실패했을 때의 잠금 상태를 **사실대로** 읽는다.
///
/// 네 경우를 구분해야 한다. 실행 전 실패(Codex 미설치 등)로 복제본이 멀쩡한 경우까지
/// 잠그면 멀쩡한 사본을 버리게 하고, 반대로 오염을 놓치면 오염된 사본을 다시 쓰게 한다.
fn codex_failure_lock(
    workspace: &Path,
    failure: &orchestration::PilotFailure,
    pilot_handle: &Arc<Mutex<Option<orchestration::RunSlot>>>,
) -> (WorkspaceLock, Option<String>) {
    // 실패가 스스로 어느 복제본의 것인지 안다면 그것을 기준으로 삼는다 —
    // 호출부가 넘긴 경로와 어긋나면 잠금이 엉뚱한 복제본에 붙는다.
    let workspace = failure.workspace_path.as_deref().unwrap_or(workspace);
    // ① 이번 실행이 이 복제본을 오염시켰다. 실패 자체가 그 사실을 싣고 온다.
    //    `marker_error`는 **표시를 저장하지 못한 이유**이지 종료 오류가 아니다.
    if failure.poisoned.is_some() {
        locks().insert(lock_key(workspace));
        return match failure.marker_error.clone() {
            Some(marker_error) => (WorkspaceLock::SessionOnly, Some(marker_error)),
            None => (WorkspaceLock::Persisted, None),
        };
    }
    // ② 지난 실행이 남긴 디스크 표시. 경로로 판단하므로 이 복제본의 사실이다.
    if orchestration::pilot_is_poisoned(workspace) {
        locks().insert(lock_key(workspace));
        return (WorkspaceLock::Persisted, None);
    }
    // ③ 표시를 남기지 못한 지난 오염이 슬롯에만 남아 있는 경우.
    //    **슬롯은 하나뿐이라 반드시 경로를 대조해야 한다** — 대조하지 않으면 다른 복제본의
    //    오염 때문에 멀쩡한 이 복제본이 잠기고, 폐기해도 그 슬롯은 풀리지 않아 실행이 계속 막힌다.
    if let Some((poisoned_workspace, _reason, marker_error)) =
        orchestration::poisoned_slot(pilot_handle)
    {
        if lock_key(&poisoned_workspace) == lock_key(workspace) {
            locks().insert(lock_key(workspace));
            // **종료 오류를 "표시를 저장하지 못한 이유"로 쓰지 않는다.**
            // 슬롯이 두 사실을 나눠 들고 있으므로 표시 오류만 여기로 온다.
            return (WorkspaceLock::SessionOnly, marker_error);
        }
    }
    // ④ 복제본을 건드리기 전에 실패했다. 잠글 이유가 없다.
    (WorkspaceLock::None, None)
}

fn stopped(
    goal: &str,
    workspace: &PilotWorkspace,
    reason: String,
    lock: WorkspaceLock,
    lock_error: Option<String>,
    evidence: HandoffEvidence,
) -> HandoffResult {
    HandoffResult {
        stage: HandoffStage::NeedsAttention,
        goal: goal.to_string(),
        plan_summary: String::new(),
        result_summary: String::new(),
        changed_files: Vec::new(),
        attention: Some(reason),
        workspace_lock: lock,
        lock_error,
        blocker: None,
        workspace_path: workspace.path.display().to_string(),
        evidence,
    }
}

#[cfg(test)]
mod tests;
