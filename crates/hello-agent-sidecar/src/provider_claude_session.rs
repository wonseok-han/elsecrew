//! Incremental Claude control handshake on an already owned, separately admitted process.
//! Errors leave cleanup responsibility with the caller; no spawn or prompt API here.
use crate::{provider_login_io::NativeFrame, provider_login_process::LoginProcess, SidecarError};
use hello_agent_core::provider::vendor_observation::claude_control::{InitializeExchange, State};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(crate) enum ToolAccess {
    ReadOnly,
    Edit,
}

/// Fixed, noninteractive launch arguments; not an OS sandbox or approval grant.
/// Managed policy can override customization settings and must be admitted before spawn.
/// https://code.claude.com/docs/en/cli-reference
pub(crate) fn arguments(
    model: &str,
    effort: &str,
    access: ToolAccess,
) -> Result<Vec<std::ffi::OsString>, SidecarError> {
    if model.is_empty()
        || model.len() > 128
        || model.starts_with('-')
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || !matches!(effort, "low" | "medium" | "high" | "xhigh" | "max")
    {
        return Err(failure());
    }
    let tools = match access {
        ToolAccess::ReadOnly => "Read,Glob,Grep",
        ToolAccess::Edit => "Read,Glob,Grep,Edit,Write,Bash",
    };
    Ok([
        "--print",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--safe-mode",
        "--setting-sources",
        "",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--mcp-config",
        "{\"mcpServers\":{}}",
        "--disallowedTools",
        "mcp__*",
        "--tools",
        tools,
        "--permission-mode",
        "default",
        "--model",
        model,
        "--effort",
        effort,
    ]
    .into_iter()
    .map(Into::into)
    .collect())
}

pub(crate) struct ControlInitialization {
    exchange: InitializeExchange,
    connection: uuid::Uuid,
    deadline: Instant,
    flushed: bool,
    failed: bool,
}

/// Created only after the handshake on this connection. Not a tool-grant authority.
pub(crate) struct PermissionChannel {
    connection: uuid::Uuid,
    permissions: hello_agent_core::provider::vendor_observation::claude_approval::Permissions,
}

pub(crate) struct PermissionDenial {
    connection: uuid::Uuid,
    reply: hello_agent_core::provider::vendor_observation::claude_approval::Denial,
}

impl PermissionDenial {
    pub(crate) fn connection(&self) -> uuid::Uuid {
        self.connection
    }
    pub(crate) fn as_str(&self) -> &str {
        self.reply.as_str()
    }
}

impl PermissionChannel {
    pub(crate) fn observe(
        &mut self,
        process: &LoginProcess<'_>,
        input: &hello_agent_core::provider::BoundedObservation,
    ) -> Result<
        hello_agent_core::provider::vendor_observation::claude_approval::Observation,
        SidecarError,
    > {
        if process.connection_id() != self.connection {
            self.permissions.close();
            return Err(failure());
        }
        self.permissions.observe(input).map_err(|_| failure())
    }

    /// Caller flushes through the same observed nonblocking transport and owns cleanup.
    pub(crate) fn deny(
        &mut self,
        process: &mut LoginProcess<'_>,
        request_id: &str,
        interrupt: bool,
    ) -> Result<(), SidecarError> {
        if process.connection_id() != self.connection {
            self.permissions.close();
            return Err(failure());
        }
        let reply = self
            .permissions
            .deny(request_id, interrupt)
            .map_err(|_| failure())?;
        let result = process.queue_native(NativeFrame::ClaudeDenial(PermissionDenial {
            connection: self.connection,
            reply,
        }));
        if result.is_err() {
            self.permissions.close();
        }
        result
    }
}

fn failure() -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        "Claude control initialization unavailable; process cleanup required",
    )
}

impl ControlInitialization {
    pub(crate) fn into_permission_channel(self) -> Result<PermissionChannel, SidecarError> {
        if self.failed || self.exchange.state() != State::Ready {
            return Err(failure());
        }
        Ok(PermissionChannel {
            connection: self.connection,
            permissions: Default::default(),
        })
    }

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
    fn launch_arguments_preserve_subscription_and_do_not_grant_tool_approval() {
        for access in [ToolAccess::ReadOnly, ToolAccess::Edit] {
            let args = arguments("selected-model", "medium", access).unwrap();
            let args: Vec<_> = args.iter().map(|a| a.to_str().unwrap()).collect();
            assert!(args.contains(&"--safe-mode"));
            assert!(!args.iter().any(|a| matches!(
                *a,
                "--bare"
                    | "--allowedTools"
                    | "--dangerously-skip-permissions"
                    | "--fallback-model"
                    | "--resume"
            )));
            assert!(args.windows(2).any(|a| a == ["--setting-sources", ""]));
            assert!(args.windows(2).any(|a| a == ["--model", "selected-model"]));
            assert!(args.windows(2).any(|a| a == ["--effort", "medium"]));
            assert!(args
                .windows(2)
                .any(|a| a == ["--permission-mode", "default"]));
        }
        assert!(arguments("--help", "medium", ToolAccess::ReadOnly).is_err());
        assert!(arguments("selected-model", "auto", ToolAccess::Edit).is_err());
        assert!(arguments("model\nother", "high", ToolAccess::Edit).is_err());
    }

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
