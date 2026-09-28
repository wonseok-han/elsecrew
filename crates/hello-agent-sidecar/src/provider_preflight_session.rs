//! Model-free preflight on an already gated, owned process. No spawn or prompt API.
use crate::{provider_login_process::LoginProcess, SidecarError};
use hello_agent_core::provider::vendor_observation::codex_preflight::exchange::{
    PreflightExchange, PreflightState,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn failure() -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        "provider settings could not be verified",
    )
}

/// The caller retains ownership on error so uncertain cleanup can be retried.
/// Ok means observations matched AND cleanup was confirmed, never launch readiness.
/// The 30-second budget bounds protocol I/O; observed cleanup has its own deadline.
pub(crate) fn observe_and_close(
    process: &mut LoginProcess<'_>,
    flow: &mut PreflightExchange,
    cancel: &AtomicBool,
) -> Result<(), SidecarError> {
    observe_with_budget(process, flow, cancel, Duration::from_secs(30), None)
}

/// Diagnostic only. Caller must verify managed cwd and integration isolation before
/// launch; creating a thread can initialize integrations even without a model turn.
pub(crate) fn observe_thread_and_close(
    process: &mut LoginProcess<'_>,
    flow: &mut PreflightExchange,
    cancel: &AtomicBool,
    cwd: &std::path::Path,
) -> Result<(), SidecarError> {
    observe_with_budget(process, flow, cancel, Duration::from_secs(30), Some(cwd))
}

fn observe_with_budget(
    process: &mut LoginProcess<'_>,
    flow: &mut PreflightExchange,
    cancel: &AtomicBool,
    budget: Duration,
    thread_cwd: Option<&std::path::Path>,
) -> Result<(), SidecarError> {
    let started = Instant::now();
    // A rejected claim never acquires cleanup authority over another session.
    if !flow.is_fresh() || process.begin_preflight().is_err() {
        flow.invalidate();
        flow.close();
        return Err(failure());
    }
    let result = (|| loop {
        if cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        let frames = flow.next_frames().map_err(|_| failure())?;
        if frames.is_empty() {
            return Err(failure());
        }
        for frame in frames {
            let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
            process.send_preflight(frame, remaining, cancel)?;
        }
        let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
        let reply = process.receive_preflight(remaining, cancel)?;
        if started.elapsed() >= budget {
            return Err(failure());
        }
        if flow.accept(&reply).map_err(|_| failure())? == PreflightState::ObservationsMatched {
            if let Some(cwd) = thread_cwd {
                let request = flow.start_read_only_thread(cwd).map_err(|_| failure())?;
                let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
                process.send_preflight(request, remaining, cancel)?;
                // Exactly one reply and one thread/started, in either order.
                for _ in 0..2 {
                    let remaining = budget.checked_sub(started.elapsed()).ok_or_else(failure)?;
                    let response = process.receive_preflight(remaining, cancel)?;
                    if started.elapsed() >= budget {
                        return Err(failure());
                    }
                    flow.accept(&response).map_err(|_| failure())?;
                }
                if flow.state() != PreflightState::ThreadMatched {
                    return Err(failure());
                }
            }
            return Ok(());
        }
    })();
    // Even a protocol error or cancellation must run the existing observed cleanup.
    let cleanup = process.finish();
    if result.is_err() || cleanup.is_err() || cancel.load(Ordering::Acquire) {
        flow.invalidate();
    }
    flow.close();
    cleanup?;
    result?;
    if cancel.load(Ordering::Acquire) {
        return Err(failure());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_profile::CodexProfileBinding;
    use hello_agent_core::provider::vendor_observation::codex_preflight::ExpectedProfile;
    use serde_json::json;
    use std::process::{Command, Stdio};

    #[test]
    fn real_pipes_match_cleanup_and_fail_closed_on_wrong_config_cancel_or_uncertainty() {
        for case in [
            "success",
            "config",
            "cancel",
            "uncertain",
            "timeout",
            "reused",
            "used-process",
            "polled-process",
            "claimed-process",
            "thread",
            "thread-before",
            "wrong-thread",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let storage = crate::Storage::open(
                &temp
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("hello-agent.sqlite3"),
            )
            .unwrap();
            let profile = CodexProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let mut flow = PreflightExchange::new(
                &ExpectedProfile {
                    model: "test-model",
                    reasoning_effort: "medium",
                },
                home.to_str().unwrap(),
            )
            .unwrap();
            let mut process = LoginProcess::reserve(&profile).unwrap();
            let replies = [
                json!({"id":1,"result":{"userAgent":"test","codexHome":home,"platformFamily":"unix","platformOs":"macos"}}),
                json!({"id":2,"result":{"config":{"model":if case == "config" {"wrong"} else {"test-model"},"model_provider":"openai","model_reasoning_effort":"medium","approval_policy":"on-request","sandbox_mode":"read-only"}}}),
                json!({"id":3,"result":{"account":{"type":"chatgpt"},"requiresOpenaiAuth":true}}),
                json!({"id":4,"result":{"data":[{"id":"test-model","model":"test-model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium"}]}],"nextCursor":null}}),
                json!({"id":5,"result":{"thread":{"id":"thread-test"},"model":"test-model","modelProvider":"openai","cwd":if case == "wrong-thread" {"/wrong"} else {"/managed/snapshot"},"instructionSources":[],"approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly","networkAccess":false},"reasoningEffort":"medium"}}),
            ].map(|value| value.to_string());
            let script = r#"
IFS= read -r line; case "$line" in *'"method":"initialize"'*) ;; *) exit 2;; esac
printf '%s\n' "$1"
IFS= read -r line; test "$line" = '{"method":"initialized","params":{}}' || exit 3
IFS= read -r line; case "$line" in *'"method":"config/read"'*) ;; *) exit 4;; esac
printf '%s\n' "$2"
IFS= read -r line; case "$line" in *'"method":"account/read"'*) ;; *) exit 5;; esac
printf '%s\n' "$3"
IFS= read -r line; case "$line" in *'"method":"model/list"'*) ;; *) exit 6;; esac
printf '%s\n' "$4"
IFS= read -r line || exit 0
case "$line" in *'"method":"thread/start"'*) ;; *) exit 7;; esac
if test "$6" = thread-before; then
  printf '%s\n' '{"method":"thread/started","params":{"thread":{"id":"thread-test"}}}'
  printf '%s\n' "$5"
else
  printf '%s\n' "$5"
  printf '%s\n' '{"method":"thread/started","params":{"thread":{"id":"thread-test"}}}'
fi
IFS= read -r hold
"#;
            process
                .launch(|| {
                    Command::new("/bin/sh")
                        .args(["-c", script, "preflight-peer"])
                        .args(&replies)
                        .arg(case)
                        .env_clear()
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                })
                .unwrap();
            if case == "uncertain" {
                process.inject_uncertain_cleanup_once();
            }
            let cancel = AtomicBool::new(case == "cancel");
            if case == "polled-process" {
                assert!(process.poll(&cancel).unwrap().is_none());
            }
            if case == "used-process" {
                let (_, request) = hello_agent_core::provider::vendor_observation::codex_login::connection::LoginConnection::begin(home.to_str().unwrap()).unwrap();
                process
                    .send(request, Duration::from_secs(1), &cancel)
                    .unwrap();
                // The prior initialize reply remains unread in this transport.
            }
            if case == "claimed-process" {
                process.begin_preflight().unwrap();
                assert!(process.receive(Duration::ZERO, &cancel).is_err());
                assert!(process.poll(&cancel).is_err());
                assert!(process.poll_output(&cancel).is_err());
                let (_, request) = hello_agent_core::provider::vendor_observation::codex_login::connection::LoginConnection::begin(home.to_str().unwrap()).unwrap();
                assert!(process
                    .send(request, Duration::from_secs(1), &cancel)
                    .is_err());
            }
            if case == "reused" {
                flow.next_request().unwrap();
            }
            let budget = if case == "timeout" {
                Duration::ZERO
            } else {
                Duration::from_secs(3)
            };
            let thread_cwd = if matches!(case, "thread" | "thread-before" | "wrong-thread") {
                Some(std::path::Path::new("/managed/snapshot"))
            } else {
                None
            };
            let result = observe_with_budget(&mut process, &mut flow, &cancel, budget, thread_cwd);
            assert_eq!(
                result.is_ok(),
                matches!(case, "success" | "thread" | "thread-before"),
                "{case}"
            );
            assert_eq!(
                flow.state(),
                if matches!(case, "success" | "thread" | "thread-before") {
                    PreflightState::Closed
                } else {
                    PreflightState::Failed
                }
            );
            assert!(flow.next_request().is_err());
            assert!(flow
                .start_read_only_thread(std::path::Path::new("/managed/snapshot"))
                .is_err());
            assert_eq!(flow.matched_thread_id(), None);
            if matches!(
                case,
                "reused" | "used-process" | "polled-process" | "claimed-process"
            ) {
                // Rejection must leave the original owner and its live child untouched.
                assert!(process.exit_status().unwrap().is_none(), "{case}");
                assert!(LoginProcess::reserve(&profile).is_err(), "{case}");
                process.finish().unwrap();
            }
            if case == "uncertain" {
                assert_eq!(result.unwrap_err().kind(), "providerLoginRecoveryRequired");
                // Retain and reap the process, but do not reinterpret the original result.
                let _ = process.finish();
            }
            drop(process);
            let reservation = LoginProcess::reserve(&profile);
            assert_eq!(reservation.is_ok(), case != "uncertain", "{case}");
        }
    }
}
