//! Incremental Claude control handshake on an already owned, separately admitted process.
//! Errors leave cleanup responsibility with the caller; no spawn or prompt API here.
use crate::{provider_login_io::NativeFrame, provider_login_process::LoginProcess, SidecarError};
use hello_agent_core::provider::vendor_observation::claude_control::{InitializeExchange, State};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(crate) mod launch;
#[allow(dead_code)] // Native Claude discovery dispatcher is not activated yet.
pub(crate) mod discovery;
pub(crate) mod admission;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolAccess {
    ReadOnly,
    Edit,
}

pub(crate) fn expected_tools(access: ToolAccess) -> &'static [&'static str] {
    match access {
        ToolAccess::ReadOnly => &["Read", "Glob", "Grep"],
        ToolAccess::Edit => &["Read", "Glob", "Grep", "Edit", "Write", "Bash"],
    }
}

/// Shell-tool protection only. File tools need separate permission rules and
/// managed settings still require admission; this is not whole-process isolation.
/// https://code.claude.com/docs/en/sandboxing
fn shell_settings() -> serde_json::Value {
    serde_json::json!({"permissions":{"blockReadsOutsideWorkingDirectories":true,"additionalDirectories":[]},"sandbox":{
        "enabled":true,
        "failIfUnavailable":true,
        "allowUnsandboxedCommands":false,
        "autoAllowBashIfSandboxed":false,
        "excludedCommands":[],
        "filesystem":{"disabled":false},
        "network":{"allowedDomains":[],"allowLocalBinding":false,"allowAllUnixSockets":false,"allowUnixSockets":[]}
    }})
}

/// Explicit deny paths supplement restricted mode; managed settings still need admission.
pub(crate) fn scoped_arguments(model: &str, effort: &str, access: ToolAccess, home: &std::path::Path, workspace: &std::path::Path, source: &std::path::Path) -> Result<Vec<std::ffi::OsString>, SidecarError> {
    fn literal(path: &std::path::Path) -> Result<String, SidecarError> {
        let value = path.to_str().ok_or_else(failure)?;
        if !path.is_absolute() || value.len() > 16 * 1024 || value.chars().any(char::is_control)
            || value.contains(['*','?','[',']','{','}','(',')','\\'])
            || path.components().any(|part| matches!(part,std::path::Component::ParentDir|std::path::Component::CurDir))
        { return Err(failure()); }
        Ok(value.into())
    }
    if workspace.starts_with(home) || home.starts_with(workspace)
        || workspace.starts_with(source) || source.starts_with(workspace) { return Err(failure()); }
    literal(workspace)?;
    let mut denied = vec![literal(home)?, literal(source)?];
    for name in [".git", ".claude", ".codex", ".agents"] { denied.push(literal(&workspace.join(name))?); }
    let mut rules = Vec::new();
    for path in &denied {
        // File permission rules use // for absolute paths; sandbox paths use /.
        for tool in ["Read", "Edit"] {
            rules.push(format!("{tool}(/{path})"));
            rules.push(format!("{tool}(/{path}/**)"));
        }
    }
    if access == ToolAccess::ReadOnly { rules.push("Edit".into()); }
    let mut settings = shell_settings();
    settings["permissions"]["deny"] = serde_json::json!(rules);
    settings["sandbox"]["filesystem"]["denyRead"] = serde_json::json!(denied);
    settings["sandbox"]["filesystem"]["denyWrite"] = serde_json::json!(denied);
    let mut args = arguments(model, effort, access)?;
    let index = args.iter().position(|arg| arg == "--settings").ok_or_else(failure)?;
    args[index + 1] = settings.to_string().into();
    Ok(args)
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
    let tools = expected_tools(access).join(",");
    let settings = shell_settings().to_string();
    Ok([
        "--print",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--safe-mode",
        "--restricted",
        "--settings",
        &settings,
        "--setting-sources",
        "",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--mcp-config",
        "{\"mcpServers\":{}}",
        "--disallowedTools",
        "mcp__*",
        "--tools",
        &tools,
        "--permission-mode",
        "default",
        "--permission-prompt-tool",
        "stdio",
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

/// Digest of what the app requests, not evidence of effective managed settings.
pub(crate) fn requested_policy_digest(model: &str, effort: &str, access: ToolAccess,
    home: &std::path::Path, workspace: &std::path::Path, source: &std::path::Path,
) -> Result<String, SidecarError> {
    use sha2::{Digest, Sha256};
    let args = scoped_arguments(model, effort, access, home, workspace, source)?;
    let args: Vec<_> = args.iter().map(|arg| arg.to_str().ok_or_else(failure)).collect::<Result<_, _>>()?;
    let requested = serde_json::json!({"policy":"claude-request-v1","argv":args,
        "profile":home,"workspace":workspace,"source":source,"environment":"claude-native-allowlist-v1"});
    let bytes = serde_json::to_vec(&requested).map_err(|_| failure())?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

/// Created only after the handshake on this connection. Not a tool-grant authority.
pub(crate) struct SessionChannel {
    connection: uuid::Uuid,
    cwd: std::path::PathBuf,
    requested_model: String,
    session_model: String,
    prompt_attempted: bool,
    transmitting: bool,
    deadline: Instant,
    permissions: hello_agent_core::provider::vendor_observation::claude_session::Session,
}

/// No public constructor: only the workspace-bound channel may prepare input.
pub(crate) struct InitialPrompt {
    connection: uuid::Uuid,
    wire: String,
}
impl InitialPrompt {
    pub(crate) fn connection(&self) -> uuid::Uuid {
        self.connection
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.wire
    }
}

fn prompt_wire(text: &str) -> Result<String, SidecarError> {
    if text.trim().is_empty() || text.len() > 64 * 1024 || text.contains('\0') {
        return Err(failure());
    }
    // SDK initial user envelope, not a resume request or renderer-controlled session.
    let frame = serde_json::json!({"type":"user","session_id":"",
        "message":{"role":"user","content":text},"parent_tool_use_id":null});
    let wire = format!("{frame}\n");
    // Escaping can expand a valid text substantially; apply the transport bound too.
    if wire.len() > hello_agent_core::provider::PROVIDER_OBSERVATION_LINE_MAX_BYTES {
        return Err(failure());
    }
    Ok(wire)
}

pub(crate) struct PermissionDenial {
    connection: uuid::Uuid,
    reply: hello_agent_core::provider::vendor_observation::claude_approval::Denial,
}

pub(crate) struct PermissionAllow {
    connection: uuid::Uuid,
    reply: hello_agent_core::provider::vendor_observation::claude_approval::AllowOnce,
}
impl PermissionAllow {
    pub(crate) fn connection(&self) -> uuid::Uuid { self.connection }
    pub(crate) fn as_str(&self) -> &str { self.reply.as_str() }
}

impl PermissionDenial {
    pub(crate) fn connection(&self) -> uuid::Uuid {
        self.connection
    }
    pub(crate) fn as_str(&self) -> &str {
        self.reply.as_str()
    }
}

impl SessionChannel {
    pub(crate) fn matches_init_receipt(&self, receipt: &hello_agent_core::provider::vendor_observation::claude_session::InitReceipt) -> bool {
        !self.permissions.is_closed() && self.cwd.to_str().is_some_and(|cwd| receipt.matches(cwd, &self.session_model))
    }

    pub(crate) fn is_permission_pending(&self, request_id: &str) -> bool {
        self.permissions.is_permission_pending(request_id)
    }
    pub(crate) fn can_queue_reply(&self) -> bool { !self.transmitting && !self.permissions.is_closed() }

    pub(crate) fn matches_role(&self, model: &str, access: ToolAccess) -> bool {
        self.requested_model == model && self.cwd.to_str().is_some_and(|cwd| self.permissions.matches_expected(
            &hello_agent_core::provider::vendor_observation::claude_init::ExpectedInit {
                cwd, model: &self.session_model, permission_mode: "default", tools: expected_tools(access),
            }
        ))
    }

    pub(crate) fn matches_process(&self, process: &LoginProcess<'_>, cwd: &std::path::Path) -> bool {
        self.connection == process.connection_id() && self.cwd == cwd
    }

    /// Snapshot path observation only, not a grant: a later provider open can race
    /// this check. Search traversal and shell commands need their own confinement.
    pub(crate) fn inspect_file_target(
        &self,
        workspace: &crate::provider_preflight_session::TextWorkspace<'_>,
        request: &hello_agent_core::provider::vendor_observation::claude_approval::Request,
        access: ToolAccess,
    ) -> Result<(), SidecarError> {
        use hello_agent_core::provider::vendor_observation::claude_approval::ToolTarget;
        if self.permissions.is_closed() || workspace.cwd() != self.cwd {
            return Err(failure());
        }
        workspace.revalidate()?;
        let ToolTarget::File { path, writes } = request.target() else {
            return Err(failure());
        };
        if *writes && matches!(access, ToolAccess::ReadOnly) {
            return Err(failure());
        }
        inspect_snapshot_file(&self.cwd, path, request.tool_name == "Write")
    }

    /// Incremental drive: a result event is still just an observation, never cleanup.
    /// Runtime owner persists process identity through `observer` and must finish on error.
    pub(crate) fn advance<'a>(
        &mut self,
        process: &mut LoginProcess<'a>,
        cancel: &AtomicBool,
        observer: &mut impl FnMut(&mut LoginProcess<'a>) -> Result<(), SidecarError>,
    ) -> Result<
        Option<hello_agent_core::provider::vendor_observation::claude_session::Event>,
        SidecarError,
    > {
        let result = (|| {
            if self.permissions.is_closed()
                || self.connection != process.connection_id()
                || cancel.load(Ordering::Acquire)
                || Instant::now() >= self.deadline
            {
                return Err(failure());
            }
            if self.transmitting {
                self.transmitting = !process.flush_native_observed(cancel, observer)?;
                return Ok(None);
            }
            match process.poll_preflight_observed(cancel, observer)? {
                Some(input) => self.observe(process, &input).map(Some),
                None => Ok(None),
            }
        })();
        if result.is_err() {
            self.permissions.close();
        }
        result
    }

    /// One initial message. The reserved runtime owner must check role, run lock,
    /// launch provenance and provider sandbox in `admit`; this channel owns none.
    /// A queued/partially written message must never be retried as a fresh prompt.
    pub(crate) fn queue_initial_prompt(
        &mut self,
        process: &mut LoginProcess<'_>,
        workspace: &crate::provider_preflight_session::TextWorkspace<'_>,
        text: &str,
        cancel: &AtomicBool,
        admit: &mut impl FnMut(&mut LoginProcess<'_>) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        let result = (|| {
            if self.prompt_attempted
                || self.transmitting
                || Instant::now() >= self.deadline
                || self.permissions.is_closed()
                || process.connection_id() != self.connection
                || cancel.load(Ordering::Acquire)
                || workspace.cwd() != self.cwd
            {
                return Err(failure());
            }
            self.prompt_attempted = true;
            let wire = prompt_wire(text)?;
            workspace.revalidate()?;
            admit(process)?;
            if cancel.load(Ordering::Acquire) {
                return Err(failure());
            }
            workspace.revalidate()?;
            process.queue_native(NativeFrame::ClaudePrompt(InitialPrompt {
                connection: self.connection,
                wire,
            }))?;
            self.transmitting = true;
            Ok(())
        })();
        if result.is_err() {
            self.prompt_attempted = true;
            self.permissions.close();
        }
        result
    }

    fn observe(
        &mut self,
        process: &LoginProcess<'_>,
        input: &hello_agent_core::provider::BoundedObservation,
    ) -> Result<hello_agent_core::provider::vendor_observation::claude_session::Event, SidecarError>
    {
        if process.connection_id() != self.connection {
            self.permissions.close();
            return Err(failure());
        }
        self.permissions.observe(input).map_err(|_| failure())
    }

    /// Trusted caller must recheck confinement and the explicit user decision here.
    /// The run owner supplies its retained admission; product activation is separate.
    pub(crate) fn allow_once(
        &mut self,
        process: &mut LoginProcess<'_>,
        request: &hello_agent_core::provider::vendor_observation::claude_approval::Request,
        cancel: &AtomicBool,
        admit: &mut impl FnMut(&mut LoginProcess<'_>, &hello_agent_core::provider::vendor_observation::claude_approval::Request) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if !self.can_queue_reply() || process.connection_id() != self.connection
            || Instant::now() >= self.deadline || cancel.load(Ordering::Acquire)
            || !self.is_permission_pending(&request.request_id)
        { self.permissions.close(); return Err(failure()); }
        admit(process, request)?;
        if Instant::now() >= self.deadline || cancel.load(Ordering::Acquire) {
            self.permissions.close(); return Err(failure());
        }
        let reply = self.permissions.allow_once(request).map_err(|_| failure())?;
        let result = process.queue_native(NativeFrame::ClaudeAllow(PermissionAllow { connection:self.connection, reply }));
        if result.is_err() { self.permissions.close(); } else { self.transmitting = true; }
        result
    }

    /// `advance` flushes the reply; caller owns cleanup on every failure.
    pub(crate) fn deny(
        &mut self,
        process: &mut LoginProcess<'_>,
        request_id: &str,
        interrupt: bool,
    ) -> Result<(), SidecarError> {
        if process.connection_id() != self.connection
            || self.transmitting
            || Instant::now() >= self.deadline
        {
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
        } else {
            self.transmitting = true;
        }
        result
    }
}

fn inspect_snapshot_file(
    root: &std::path::Path,
    value: &str,
    allow_new: bool,
) -> Result<(), SidecarError> {
    use std::path::{Component, Path};
    let path = Path::new(value);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).map_err(|_| failure())?
    } else {
        path
    };
    if relative.as_os_str().is_empty() {
        return Err(failure());
    }
    let components: Vec<_> = relative.components().collect();
    let mut current = root.to_path_buf();
    for (index, part) in components.iter().enumerate() {
        let Component::Normal(name) = part else {
            return Err(failure());
        };
        if matches!(
            name.to_str(),
            Some(".git" | ".claude" | ".codex" | ".agents")
        ) {
            return Err(failure());
        }
        current.push(name);
        let last = index + 1 == components.len();
        match std::fs::symlink_metadata(&current) {
            Ok(metadata)
                if !metadata.file_type().is_symlink()
                    && (if last {
                        metadata.is_file()
                    } else {
                        metadata.is_dir()
                    }) => {}
            Err(error) if last && allow_new && error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(failure()),
        }
    }
    Ok(())
}

fn failure() -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        "Claude control initialization unavailable; process cleanup required",
    )
}

impl ControlInitialization {
    /// Resolve only the selected catalog entry on this connection; never fallback.
    pub(crate) fn into_profile_channel(
        self,
        expected: &hello_agent_core::provider::vendor_observation::claude_init::ExpectedInit<'_>,
        effort: &str,
    ) -> Result<SessionChannel, SidecarError> {
        let resolved = self.exchange.profile_model(expected.model, effort).map_err(|_| failure())?.to_owned();
        let resolved_expected = hello_agent_core::provider::vendor_observation::claude_init::ExpectedInit {
            cwd:expected.cwd, model:&resolved, permission_mode:expected.permission_mode, tools:expected.tools,
        };
        let mut channel = self.into_session_channel(&resolved_expected)?;
        channel.requested_model = expected.model.into();
        Ok(channel)
    }

    pub(crate) fn into_session_channel(
        self,
        expected: &hello_agent_core::provider::vendor_observation::claude_init::ExpectedInit<'_>,
    ) -> Result<SessionChannel, SidecarError> {
        if self.failed || self.exchange.state() != State::Ready {
            return Err(failure());
        }
        Ok(SessionChannel {
            connection: self.connection,
            cwd: expected.cwd.into(),
            requested_model: expected.model.into(),
            session_model: expected.model.into(),
            prompt_attempted: false,
            transmitting: false,
            deadline: Instant::now() + Duration::from_secs(10 * 60),
            permissions:
                hello_agent_core::provider::vendor_observation::claude_session::Session::new(
                    expected,
                ),
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
    fn resolved_session_model_remains_bound_to_the_selected_alias() {
        let mut exchange = InitializeExchange::new("init_test").unwrap();
        exchange.request().unwrap();
        let response = serde_json::json!({"type":"control_response","response":{
            "subtype":"success","request_id":"init_test","response":{"hooks_applied":true,"current_permission_mode":"default",
                "models":[{"value":"opus","resolvedModel":"claude-opus-test","supportsEffort":true,"supportedEffortLevels":["high"]}]}
        }});
        exchange.accept_response(&hello_agent_core::provider::BoundedObservation::new(response.to_string().into_bytes()).unwrap()).unwrap();
        let initialization = ControlInitialization {exchange,connection:uuid::Uuid::new_v4(),deadline:Instant::now()+Duration::from_secs(15),flushed:true,failed:false};
        let expected = hello_agent_core::provider::vendor_observation::claude_init::ExpectedInit {
            cwd:"/snapshot",model:"opus",permission_mode:"default",tools:expected_tools(ToolAccess::ReadOnly),
        };
        let channel = initialization.into_profile_channel(&expected,"high").unwrap();
        assert!(channel.matches_role("opus", ToolAccess::ReadOnly));
        assert!(!channel.matches_role("claude-opus-test", ToolAccess::ReadOnly));
        assert_eq!(channel.session_model,"claude-opus-test");
        assert!(!channel.matches_role("opus", ToolAccess::Edit));
    }

    #[test]
    fn snapshot_file_observation_rejects_escape_links_and_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("existing"), b"test").unwrap();
        std::os::unix::fs::symlink("existing", root.join("link")).unwrap();
        assert!(inspect_snapshot_file(&root, "existing", false).is_ok());
        assert!(inspect_snapshot_file(&root, "new", true).is_ok());
        for path in [
            "../outside",
            "/outside",
            "link",
            ".git/config",
            "missing/child",
        ] {
            assert!(inspect_snapshot_file(&root, path, true).is_err());
        }
        assert!(inspect_snapshot_file(&root, "missing", false).is_err());
    }

    #[test]
    fn initial_prompt_is_a_single_bounded_user_frame_not_a_control_command() {
        let text = "hello\n\"type\":\"control_request\"";
        let wire = prompt_wire(text).unwrap();
        assert_eq!(wire.lines().count(), 1);
        let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(value["type"], "user");
        assert_eq!(value["session_id"], "");
        assert_eq!(value["message"]["content"], text);
        assert!(value["parent_tool_use_id"].is_null());
        for text in [
            " ".into(),
            "\0".into(),
            "a".repeat(65537),
            "\u{1}".repeat(65536),
        ] {
            assert!(prompt_wire(&text).is_err());
        }
    }

    #[test]
    fn scoped_launch_separates_file_rules_from_shell_paths() {
        let home = Path::new("/app/profile");
        let workspace = Path::new("/app/snapshot");
        let source = Path::new("/projects/user-source");
        let args = scoped_arguments("selected-model", "medium", ToolAccess::Edit, home, workspace, source).unwrap();
        assert!(args.iter().any(|arg| arg == "--restricted"));
        let index = args.iter().position(|arg| arg == "--settings").unwrap();
        let settings: serde_json::Value = serde_json::from_str(args[index+1].to_str().unwrap()).unwrap();
        assert_eq!(settings["permissions"]["blockReadsOutsideWorkingDirectories"],true);
        assert!(settings["permissions"]["deny"].as_array().unwrap().contains(&serde_json::json!("Read(//app/profile/**)")));
        for path in ["/app/profile", "/projects/user-source", "/app/snapshot/.git", "/app/snapshot/.claude"] {
            assert!(settings["sandbox"]["filesystem"]["denyRead"].as_array().unwrap().contains(&serde_json::json!(path)));
            assert!(settings["sandbox"]["filesystem"]["denyWrite"].as_array().unwrap().contains(&serde_json::json!(path)));
        }
        assert_eq!(settings["sandbox"]["network"]["allowLocalBinding"],false);
        for path in ["/app/snap*", "/app/snap?", "/app/snap[1]", "/app/snap)extra", "relative"] {
            assert!(scoped_arguments("selected-model", "medium", ToolAccess::Edit, home, Path::new(path), source).is_err());
        }
        assert!(scoped_arguments("selected-model", "medium", ToolAccess::Edit, home, workspace, Path::new("/app")).is_err());
    }

    #[test]
    fn launch_arguments_preserve_subscription_and_do_not_grant_tool_approval() {
        for access in [ToolAccess::ReadOnly, ToolAccess::Edit] {
            let args = arguments("selected-model", "medium", access).unwrap();
            let args: Vec<_> = args.iter().map(|a| a.to_str().unwrap()).collect();
            assert!(args.contains(&"--safe-mode"));
            assert_eq!(args.iter().filter(|a| **a == "--settings").count(), 1);
            let settings = args.windows(2).find(|a| a[0] == "--settings").unwrap()[1];
            let settings: serde_json::Value = serde_json::from_str(settings).unwrap();
            assert_eq!(settings["sandbox"]["enabled"], true);
            assert_eq!(settings["sandbox"]["failIfUnavailable"], true);
            assert_eq!(settings["sandbox"]["allowUnsandboxedCommands"], false);
            assert_eq!(settings["sandbox"]["autoAllowBashIfSandboxed"], false);
            assert_eq!(settings["sandbox"]["filesystem"]["disabled"], false);
            assert_eq!(
                settings["sandbox"]["excludedCommands"],
                serde_json::json!([])
            );
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
                .any(|a| a == ["--permission-prompt-tool", "stdio"]));
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
            let mut process = LoginProcess::reserve_claude_owned(profile).unwrap();
            assert_eq!(process.claude_preflight_home().unwrap(), home);
            assert!(process.codex_preflight_home().is_err());
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

    #[test]
    fn permission_denial_round_trips_through_owned_transport() {
        use hello_agent_core::provider::vendor_observation::claude_approval::Observation;
        use hello_agent_core::provider::vendor_observation::claude_session::Event;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("hello-agent.sqlite3")).unwrap();
        let profile = ClaudeProfileBinding::from_storage(&storage).unwrap();
        let home = profile.revalidate().unwrap();
        let mut process = LoginProcess::reserve_claude_owned(profile).unwrap();
        assert_eq!(process.claude_preflight_home().unwrap(), home);
        let id = format!("init_{}", process.connection_id().simple());
        let init = serde_json::json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":{}}});
        let reported = serde_json::json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":"/snapshot","model":"model","permissionMode":"default","tools":["Bash"],"mcp_servers":[],"skills":[],"plugins":[]});
        let request = serde_json::json!({"type":"control_request","request_id":"permission_a","request":{
            "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"tool_a","input":{"command":"PRIVATE"}
        }});
        let expected = serde_json::json!({"type":"control_response","response":{
            "subtype":"success","request_id":"permission_a","response":{
                "behavior":"deny","message":"Tool request declined by the application user.","interrupt":false
            }
        }});
        let cancelled =
            serde_json::json!({"type":"control_cancel_request","request_id":"permission_a"});
        // No provider/account/model. The toy acknowledges only the exact denial bytes.
        let script = format!(
            "IFS= read -r first || exit 1\nprintf '%s\\n' '{init}' '{reported}' '{request}'\nIFS= read -r denial || exit 2\n[ \"$denial\" = '{expected}' ] || exit 3\nprintf '%s\\n' '{cancelled}'\nIFS= read -r last\n"
        );
        process
            .launch_gated_command(
                Path::new("/bin/sh"),
                &["-c".as_ref(), script.as_ref()],
                &home,
            )
            .unwrap();
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = (|| -> Result<(), SidecarError> {
            let mut session = ControlInitialization::start(&mut process, &cancel)?;
            while !session.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                if Instant::now() >= deadline {
                    return Err(failure());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut permissions = session.into_session_channel(
                &hello_agent_core::provider::vendor_observation::claude_init::ExpectedInit {
                    cwd: "/snapshot",
                    model: "model",
                    permission_mode: "default",
                    tools: &["Bash"],
                },
            )?;
            let incoming = loop {
                if Instant::now() >= deadline {
                    return Err(failure());
                }
                if let Some(input) = permissions.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                    break input;
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            if !matches!(incoming, Event::Initialized { .. }) {
                return Err(failure());
            }
            let incoming = loop {
                if Instant::now() >= deadline {
                    return Err(failure());
                }
                if let Some(input) = permissions.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                    break input;
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            let Event::Permission(Observation::Requested(request)) = incoming else {
                return Err(failure());
            };
            if request.request_id != "permission_a"
                || request.tool_use_id.as_deref() != Some("tool_a")
            {
                return Err(failure());
            }
            permissions.deny(&mut process, &request.request_id, false)?;
            let incoming = loop {
                if Instant::now() >= deadline {
                    return Err(failure());
                }
                if let Some(input) = permissions.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                    break input;
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            if !matches!(incoming, Event::Permission(Observation::Cancelled)) {
                return Err(failure());
            }
            if permissions
                .deny(&mut process, "permission_a", false)
                .is_ok()
            {
                return Err(failure());
            }
            // Expiry closes the channel; moving the clock bound cannot reopen it.
            permissions.deadline = Instant::now();
            if permissions
                .advance(&mut process, &cancel, &mut |_| Ok(()))
                .is_ok()
            {
                return Err(failure());
            }
            permissions.deadline = Instant::now() + Duration::from_secs(60);
            if permissions
                .advance(&mut process, &cancel, &mut |_| Ok(()))
                .is_ok()
            {
                return Err(failure());
            }
            Ok(())
        })();
        let cleanup = process.finish();
        assert!(cleanup.is_ok());
        assert!(process.closure_evidence().is_some_and(|e| e.confirmed()));
        assert!(result.is_ok());
    }
}
