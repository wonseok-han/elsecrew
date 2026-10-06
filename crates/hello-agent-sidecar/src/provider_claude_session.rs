//! Incremental Claude control handshake on an already owned, separately admitted process.
//! Errors leave cleanup responsibility with the caller; no spawn or prompt API here.
use crate::{provider_login_io::NativeFrame, provider_login_process::LoginProcess, SidecarError};
use hello_agent_core::provider::vendor_observation::claude_control::{InitializeExchange, State};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(crate) struct ControlInitialization {
    exchange: InitializeExchange,
    connection: uuid::Uuid,
    deadline: Instant,
    flushed: bool,
    failed: bool,
}

fn failure() -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        "Claude control initialization unavailable; process cleanup required",
    )
}

impl ControlInitialization {
    pub(crate) fn start(
        process: &mut LoginProcess<'_>,
        cancel: &AtomicBool,
    ) -> Result<Self, SidecarError> {
        if cancel.load(Ordering::Acquire) {
            return Err(failure());
        }
        let connection = process.connection_id();
        let mut exchange = InitializeExchange::new(&format!("init_{}", connection.simple()))
            .map_err(|_| failure())?;
        process.begin_claude_preflight()?;
        process.queue_native(NativeFrame::ClaudeInitialize(
            exchange.request().map_err(|_| failure())?,
        ))?;
        Ok(Self {
            exchange,
            connection,
            deadline: Instant::now() + Duration::from_secs(15),
            flushed: false,
            failed: false,
        })
    }

    /// true means the control response matched, not that launch settings or tools are safe.
    /// One bounded IO poll per call keeps cancellation and the sidecar scheduler responsive.
    pub(crate) fn advance<'a>(
        &mut self,
        process: &mut LoginProcess<'a>,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut LoginProcess<'a>) -> Result<(), SidecarError>,
    ) -> Result<bool, SidecarError> {
        let result = self.advance_inner(process, cancel, observer);
        if result.is_err() {
            self.failed = true;
            self.exchange.close();
        }
        result
    }

    fn advance_inner<'a>(
        &mut self,
        process: &mut LoginProcess<'a>,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut LoginProcess<'a>) -> Result<(), SidecarError>,
    ) -> Result<bool, SidecarError> {
        if self.failed
            || self.connection != process.connection_id()
            || cancel.load(Ordering::Acquire)
            || Instant::now() >= self.deadline
            || self.exchange.state() != State::AwaitingResponse
        {
            return Err(failure());
        }
        if !self.flushed {
            self.flushed = process.flush_native_observed(cancel, observer)?;
            return Ok(false);
        }
        let Some(input) = process.poll_preflight_observed(cancel, observer)? else {
            return Ok(false);
        };
        self.exchange
            .accept_response(&input)
            .map_err(|_| failure())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider_claude_profile::ClaudeProfileBinding, provider_profile::LoginProfile, Storage,
    };
    use std::path::Path;

    #[test]
    fn owned_toy_control_exchange_handles_reply_cancel_and_deadline_without_prompt() {
        for case in ["ready", "mismatch", "cancel", "deadline"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
            let profile = ClaudeProfileBinding::from_storage(&storage).unwrap();
            let home = profile.revalidate().unwrap();
            let mut process = LoginProcess::reserve(&profile).unwrap();
            let id = if case == "mismatch" {
                "wrong".into()
            } else {
                format!("init_{}", process.connection_id().simple())
            };
            let reply = serde_json::json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":{}}});
            // Keep the toy alive after the response; the owner must explicitly finish it.
            let script = format!(
                "IFS= read -r line || exit 1\nprintf '%s\\n' '{reply}'\nIFS= read -r extra\n"
            );
            process
                .launch_gated_command(
                    Path::new("/bin/sh"),
                    &["-c".as_ref(), script.as_ref()],
                    &home,
                )
                .unwrap();
            assert!(process.begin_preflight().is_err()); // wrong provider namespace
            let cancel = AtomicBool::new(false);
            let mut session = ControlInitialization::start(&mut process, &cancel).unwrap();
            assert!(ControlInitialization::start(&mut process, &cancel).is_err());
            if case == "cancel" {
                cancel.store(true, Ordering::Release);
            }
            if case == "deadline" {
                session.deadline = Instant::now();
            }
            let observed = loop {
                match session.advance(&mut process, &cancel, &mut |_| Ok(())) {
                    Ok(false) => std::thread::sleep(Duration::from_millis(2)),
                    terminal => break terminal,
                }
            };
            let cleanup = process.finish();
            assert!(cleanup.is_ok());
            assert!(process.closure_evidence().is_some_and(|e| e.confirmed()));
            assert_eq!(observed.is_ok(), case == "ready");
            assert!(session
                .advance(&mut process, &cancel, &mut |_| Ok(()))
                .is_err());
        }
    }
}
