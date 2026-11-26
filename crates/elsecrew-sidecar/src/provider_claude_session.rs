//! Incremental Claude control handshake on an already owned, separately admitted process.
//! Errors leave cleanup responsibility with the caller; no spawn or prompt API here.
use crate::{provider_login_io::NativeFrame, provider_login_process::LoginProcess, SidecarError};
use elsecrew_core::provider::vendor_observation::claude_control::{InitializeExchange, State};
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
    // Safe mode still reports these built-in plugins in current native Claude.
    // Disable them through the documented per-plugin setting; init validation
    // still rejects other plugins; the native organization guard has a separate
    // exact observation contract and is never disabled here.
    // https://code.claude.com/docs/en/settings-reference#enabledplugins
    serde_json::json!({"enabledPlugins":{
        "cc-plugin-agents-md@builtin":false,
        "cc-plugin-plugin-authoring@builtin":false
    },"permissions":{"blockReadsOutsideWorkingDirectories":true,"additionalDirectories":[]},"sandbox":{
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
pub(crate) fn scoped_arguments(model: &str, effort: Option<&str>, access: ToolAccess, home: &std::path::Path, workspace: &std::path::Path, source: &std::path::Path) -> Result<Vec<std::ffi::OsString>, SidecarError> {
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
    // A task worktree (user decision 2026-11-18): sandboxed commands may read
    // its `.git` link and the repository's Git data so Git works; writes and
    // the file tools stay denied. Only the repository's own `.git` reopens.
    let source_git = elsecrew_core::provider::vendor_observation::codex_preflight::named_config::linked_source_git(workspace)
        .filter(|git| elsecrew_core::provider::vendor_observation::codex_preflight::named_config::source_git_dir(source).as_deref() == Some(git.as_path()));
    let unreadable: Vec<String> = match &source_git {
        Some(_) => { let link = literal(&workspace.join(".git"))?; denied.iter().filter(|path| **path != link).cloned().collect() }
        None => denied.clone(),
    };
    // Claude Code also applies `Read(...)` deny rules to sandboxed commands,
    // so the worktree's `.git` link gets no Read rule (its Edit rule and the
    // sandbox write denial stay); otherwise `git` fails with "error reading
    // .git" even though the sandbox reopened it (found 2026-11-19).
    let readable_link = source_git.as_ref().map(|_| literal(&workspace.join(".git"))).transpose()?;
    let mut rules = Vec::new();
    for path in &denied {
        // File permission rules use // for absolute paths; sandbox paths use /.
        for tool in ["Read", "Edit"] {
            if tool == "Read" && readable_link.as_ref() == Some(path) { continue; }
            rules.push(format!("{tool}(/{path})"));
            rules.push(format!("{tool}(/{path}/**)"));
        }
    }
    if access == ToolAccess::ReadOnly { rules.push("Edit".into()); }
    let mut settings = shell_settings();
    settings["permissions"]["deny"] = serde_json::json!(rules);
    settings["sandbox"]["filesystem"]["denyRead"] = serde_json::json!(unreadable);
    if let Some(git) = &source_git {
        settings["sandbox"]["filesystem"]["allowRead"] = serde_json::json!([literal(git)?]);
    }
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
    effort: Option<&str>,
    access: ToolAccess,
) -> Result<Vec<std::ffi::OsString>, SidecarError> {
    if model.is_empty()
        || model.len() > 128
        || model.starts_with('-')
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        || effort.is_some_and(|effort| !matches!(effort, "low" | "medium" | "high" | "xhigh" | "max"))
    {
        return Err(failure());
    }
    let tools = expected_tools(access).join(",");
    let settings = shell_settings().to_string();
    let mut args: Vec<std::ffi::OsString> = [
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
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    if let Some(effort) = effort { args.extend(["--effort".into(), effort.into()]); }
    Ok(args)
}

pub(crate) struct ControlInitialization {
    exchange: InitializeExchange,
    connection: uuid::Uuid,
    deadline: Instant,
    flushed: bool,
    failed: bool,
}

/// Digest of what the app requests, not evidence of effective managed settings.
pub(crate) fn requested_policy_digest(model: &str, effort: Option<&str>, access: ToolAccess,
    home: &std::path::Path, workspace: &std::path::Path, source: &std::path::Path,
) -> Result<String, SidecarError> {
    use sha2::{Digest, Sha256};
    let args = scoped_arguments(model, effort, access, home, workspace, source)?;
    let args: Vec<_> = args.iter().map(|arg| arg.to_str().ok_or_else(failure)).collect::<Result<_, _>>()?;
    let requested = serde_json::json!({"policy":"claude-request-v1","argv":args,
        "profile":home,"workspace":workspace,"source":source,"environment":"claude-native-allowlist-v2-no-builtin-agents"});
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
    // No wall-clock bound (2026-11-17 user decision): a coding session may run
    // for hours and may wait on the user. Cancel, quit cleanup and the
    // per-session request/event budgets remain the bounds.
    permissions: elsecrew_core::provider::vendor_observation::claude_session::Session,
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
    if wire.len() > elsecrew_core::provider::PROVIDER_OBSERVATION_LINE_MAX_BYTES {
        return Err(failure());
    }
    Ok(wire)
}

pub(crate) struct PermissionDenial {
    connection: uuid::Uuid,
    reply: elsecrew_core::provider::vendor_observation::claude_approval::Denial,
}

pub(crate) struct PermissionAllow {
    connection: uuid::Uuid,
    reply: elsecrew_core::provider::vendor_observation::claude_approval::AllowOnce,
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
    /// Must be selected before initialization observations. No launch authority.
    pub(crate) fn with_output_budget(mut self, budget: elsecrew_core::provider::ProviderEventBudget) -> Result<Self, SidecarError> {
        if self.prompt_attempted || self.transmitting { return Err(failure()); }
        self.permissions = self.permissions.with_output_budget(budget).map_err(|_| failure())?;
        Ok(self)
    }
    pub(crate) fn matches_init_receipt(&self, receipt: &elsecrew_core::provider::vendor_observation::claude_session::InitReceipt) -> bool {
        !self.permissions.is_closed() && self.cwd.to_str().is_some_and(|cwd| receipt.matches(cwd, &self.session_model))
    }

    pub(crate) fn is_permission_pending(&self, request_id: &str) -> bool {
        self.permissions.is_permission_pending(request_id)
    }
    pub(crate) fn can_queue_reply(&self) -> bool { !self.transmitting && !self.permissions.is_closed() }
    pub(crate) fn cwd(&self) -> &std::path::Path { &self.cwd }

    pub(crate) fn matches_role(&self, model: &str, access: ToolAccess) -> bool {
        self.requested_model == model && self.cwd.to_str().is_some_and(|cwd| self.permissions.matches_expected(
            &elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit {
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
        request: &elsecrew_core::provider::vendor_observation::claude_approval::Request,
        access: ToolAccess,
    ) -> Result<(), SidecarError> {
        use elsecrew_core::provider::vendor_observation::claude_approval::ToolTarget;
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
        Option<elsecrew_core::provider::vendor_observation::claude_session::Event>,
        SidecarError,
    > {
        let mut stage = "channel state";
        let result = (|| {
            if self.permissions.is_closed()
                || self.connection != process.connection_id()
                || cancel.load(Ordering::Acquire)
            {
                return Err(failure());
            }
            if self.transmitting {
                stage = "channel send";
                self.transmitting = !process.flush_native_observed(cancel, observer)?;
                return Ok(None);
            }
            stage = "channel receive";
            match process.poll_preflight_observed(cancel, observer)? {
                Some(input) => self.observe(process, &input).map(Some).map_err(|error| {
                    stage = self.permissions.diagnostic_stage();
                    error
                }),
                None => Ok(None),
            }
        })();
        if result.is_err() {
            eprintln!("native Claude transport failed at {stage}");
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
        input: &elsecrew_core::provider::BoundedObservation,
    ) -> Result<elsecrew_core::provider::vendor_observation::claude_session::Event, SidecarError>
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
        request: &elsecrew_core::provider::vendor_observation::claude_approval::Request,
        cancel: &AtomicBool,
        admit: &mut impl FnMut(&mut LoginProcess<'_>, &elsecrew_core::provider::vendor_observation::claude_approval::Request) -> Result<(), SidecarError>,
    ) -> Result<(), SidecarError> {
        if !self.can_queue_reply() || process.connection_id() != self.connection
            || cancel.load(Ordering::Acquire)
            || !self.is_permission_pending(&request.request_id)
        { self.permissions.close(); return Err(failure()); }
        admit(process, request)?;
        if cancel.load(Ordering::Acquire) {
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
        if process.connection_id() != self.connection || self.transmitting {
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

/// `.git`, `.claude`, `.codex`, `.agents` in any letter case (macOS folders
/// ignore case, so `.CLAUDE` is `.claude`), and any dot name with non-ASCII
/// letters, which the file system may fold onto one of them. Not valid UTF-8
/// is refused too.
pub(crate) fn protected_name(name: &std::ffi::OsStr) -> bool {
    let Some(text) = name.to_str() else { return true };
    (text.starts_with('.') && !text.is_ascii())
        || [".git", ".claude", ".codex", ".agents"].iter().any(|blocked| text.eq_ignore_ascii_case(blocked))
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
        if protected_name(name) {
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
            // A new file may also bring new folders (Claude's Write makes
            // them): every missing name is still a plain, unprotected name
            // under a real, link-free folder of the workspace (found
            // 2026-11-19: `docs/plan.md` in an empty project was refused).
            Err(error) if allow_new && error.kind() == std::io::ErrorKind::NotFound => {}
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

// Fixed local stages only: never include provider response text, paths, or credentials.
pub(crate) fn initialization_failure(stage: &'static str) -> SidecarError {
    SidecarError::new(
        "providerPreflightFailed",
        format!("Claude control initialization failed at {stage}; process cleanup required"),
    )
}

impl ControlInitialization {
    /// Resolve only the selected catalog entry on this connection; never fallback.
    pub(crate) fn into_profile_channel(
        self,
        expected: &elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit<'_>,
        effort: Option<&str>,
    ) -> Result<SessionChannel, SidecarError> {
        let resolved = self.exchange.profile_model(expected.model, effort).map_err(|_| failure())?.to_owned();
        let resolved_expected = elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit {
            cwd:expected.cwd, model:&resolved, permission_mode:expected.permission_mode, tools:expected.tools,
        };
        let mut channel = self.into_session_channel(&resolved_expected)?;
        channel.requested_model = expected.model.into();
        Ok(channel)
    }

    pub(crate) fn into_session_channel(
        self,
        expected: &elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit<'_>,
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
            permissions:
                elsecrew_core::provider::vendor_observation::claude_session::Session::for_native_builtin_guards(
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
        if self.failed || self.connection != process.connection_id()
            || self.exchange.state() != State::AwaitingResponse {
            return Err(initialization_failure("connection state"));
        }
        if cancel.load(Ordering::Acquire) {
            return Err(initialization_failure("cancellation"));
        }
        if Instant::now() >= self.deadline {
            return Err(initialization_failure("response deadline"));
        }
        if !self.flushed {
            self.flushed = process.flush_native_observed(cancel, observer)?;
            return Ok(false);
        }
        let Some(input) = process.poll_preflight_observed(cancel, observer)? else {
            return Ok(false);
        };
        if self.exchange.accept_response(&input).is_err() {
            return Err(initialization_failure(self.exchange.diagnostic_stage()));
        }
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
        exchange.accept_response(&elsecrew_core::provider::BoundedObservation::new(response.to_string().into_bytes()).unwrap()).unwrap();
        let initialization = ControlInitialization {exchange,connection:uuid::Uuid::new_v4(),deadline:Instant::now()+Duration::from_secs(15),flushed:true,failed:false};
        let expected = elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit {
            cwd:"/snapshot",model:"opus",permission_mode:"default",tools:expected_tools(ToolAccess::ReadOnly),
        };
        let channel = initialization.into_profile_channel(&expected,Some("high")).unwrap();
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
        // A new file in new folders, as Claude's Write makes them.
        assert!(inspect_snapshot_file(&root, "docs/plan.md", true).is_ok());
        assert!(inspect_snapshot_file(&root, "a/b/c.txt", true).is_ok());
        std::fs::create_dir(root.join("dir")).unwrap();
        std::os::unix::fs::symlink(root.join("dir"), root.join("dirlink")).unwrap();
        for path in [
            "../outside",
            "/outside",
            "link",
            ".git/config",
            ".Git/config",
            ".CLAUDE/settings.json",
            "sub/.GIT/x",
            "missing/.Codex/config.toml",
            ".\u{17F}omething",
            "missing/.git/config",
            "missing/.claude/settings.json",
            "dirlink/child",
            "existing/child",
            "missing/../escape",
        ] {
            assert!(inspect_snapshot_file(&root, path, true).is_err(), "{path}");
        }
        assert!(inspect_snapshot_file(&root, "missing", false).is_err());
        assert!(inspect_snapshot_file(&root, "missing/child", false).is_err());
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
    fn a_task_worktree_reopens_only_the_repository_git_data_for_commands() {
        let (_temp, source, workspace) = crate::provider_preflight_session::named_permissions::tests::real_worktree();
        let home = workspace.parent().unwrap().join("profile");
        let args = scoped_arguments("selected-model", Some("medium"), ToolAccess::Edit, &home, &workspace, &source).unwrap();
        let index = args.iter().position(|arg| arg == "--settings").unwrap();
        let settings: serde_json::Value = serde_json::from_str(args[index+1].to_str().unwrap()).unwrap();
        let filesystem = &settings["sandbox"]["filesystem"];
        let text = |path: &Path| serde_json::json!(path.to_str().unwrap());
        assert_eq!(filesystem["allowRead"], serde_json::json!([source.join(".git").to_str().unwrap()]));
        assert!(filesystem["denyRead"].as_array().unwrap().contains(&text(&source)));
        assert!(!filesystem["denyRead"].as_array().unwrap().contains(&text(&workspace.join(".git"))));
        assert!(filesystem["denyWrite"].as_array().unwrap().contains(&text(&workspace.join(".git"))));
        assert!(filesystem["denyWrite"].as_array().unwrap().contains(&text(&source)));
        // Read deny rules also bind sandboxed commands: none for the link,
        // while editing it and reading the user's folder stay denied.
        let rules = settings["permissions"]["deny"].as_array().unwrap();
        let link = workspace.join(".git");
        assert!(!rules.contains(&serde_json::json!(format!("Read(/{})", link.display()))), "{rules:?}");
        assert!(!rules.contains(&serde_json::json!(format!("Read(/{}/**)", link.display()))), "{rules:?}");
        assert!(rules.contains(&serde_json::json!(format!("Edit(/{})", link.display()))));
        assert!(rules.contains(&serde_json::json!(format!("Read(/{}/**)", source.display()))));
    }

    #[test]
    fn scoped_launch_separates_file_rules_from_shell_paths() {
        let home = Path::new("/app/profile");
        let workspace = Path::new("/app/snapshot");
        let source = Path::new("/projects/user-source");
        let args = scoped_arguments("selected-model", Some("medium"), ToolAccess::Edit, home, workspace, source).unwrap();
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
            assert!(scoped_arguments("selected-model", Some("medium"), ToolAccess::Edit, home, Path::new(path), source).is_err());
        }
        assert!(scoped_arguments("selected-model", Some("medium"), ToolAccess::Edit, home, workspace, Path::new("/app")).is_err());
    }

    #[test]
    fn launch_arguments_preserve_subscription_and_do_not_grant_tool_approval() {
        let defaults = arguments("selected-model", None, ToolAccess::Edit).unwrap();
        assert!(!defaults.iter().any(|arg| arg == "--effort"));
        assert!(arguments("selected-model", Some(""), ToolAccess::Edit).is_err());
        for access in [ToolAccess::ReadOnly, ToolAccess::Edit] {
            let args = arguments("selected-model", Some("medium"), access).unwrap();
            let args: Vec<_> = args.iter().map(|a| a.to_str().unwrap()).collect();
            assert!(args.contains(&"--safe-mode"));
            assert_eq!(args.iter().filter(|a| **a == "--settings").count(), 1);
            let settings = args.windows(2).find(|a| a[0] == "--settings").unwrap()[1];
            let settings: serde_json::Value = serde_json::from_str(settings).unwrap();
            assert_eq!(settings["enabledPlugins"], serde_json::json!({
                "cc-plugin-agents-md@builtin":false,
                "cc-plugin-plugin-authoring@builtin":false
            }));
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
        assert!(arguments("--help", Some("medium"), ToolAccess::ReadOnly).is_err());
        assert!(arguments("selected-model", Some("auto"), ToolAccess::Edit).is_err());
        assert!(arguments("model\nother", Some("high"), ToolAccess::Edit).is_err());
    }

    #[test]
    fn owned_toy_control_exchange_handles_reply_cancel_and_deadline_without_prompt() {
        for case in ["ready", "mismatch", "cancel", "deadline"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let storage = Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
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

    /// D2 (2026-11-26): a fake Claude reads ten large images (each tool result
    /// a ~148 KB stream-json line) and then answers. The session must keep
    /// going; before the fix the first such line ended it (`sessionChannel`).
    fn large_image_session(admit: bool) -> (Result<Vec<&'static str>, SidecarError>, bool) {
        use elsecrew_core::provider::vendor_observation::claude_events::Projection;
        use elsecrew_core::provider::vendor_observation::claude_session::Event;
        let _slots = crate::provider_login_io::LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
        let profile = ClaudeProfileBinding::from_storage(&storage).unwrap();
        let home = profile.revalidate().unwrap();
        let mut process = LoginProcess::reserve_claude_owned(profile).unwrap();
        let id = format!("init_{}", process.connection_id().simple());
        let init = serde_json::json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":{}}});
        let reported = serde_json::json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":"/snapshot","model":"model","permissionMode":"default","tools":["Read"],"mcp_servers":[],"skills":[],"plugins":[]});
        let answer = serde_json::json!({"type":"assistant","uuid":"a1","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"I looked at the screenshots."}]}});
        let script = format!(
            "IFS= read -r first || exit 1\nprintf '%s\\n' '{init}' '{reported}'\n\
             i=0; while [ $i -lt 10 ]; do \
               printf '{{\"type\":\"user\",\"uuid\":\"u%s\",\"session_id\":\"s\",\"parent_tool_use_id\":null,\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_%s\",\"content\":[{{\"type\":\"image\",\"source\":{{\"type\":\"base64\",\"media_type\":\"image/png\",\"data\":\"' $i $i; \
               head -c 151552 /dev/zero | tr '\\0' A; printf '\"}}}}]}}]}}}}\\n'; i=$((i+1)); done\n\
             printf '%s\\n' '{answer}'\nIFS= read -r last\n"
        );
        process.launch_gated_command(Path::new("/bin/sh"), &["-c".as_ref(), script.as_ref()], &home).unwrap();
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(10);
        let result = (|| -> Result<Vec<&'static str>, SidecarError> {
            let mut session = ControlInitialization::start(&mut process, &cancel)?;
            while !session.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                if Instant::now() >= deadline { return Err(failure()); }
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut channel = session.into_session_channel(
                &elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit {
                    cwd: "/snapshot", model: "model", permission_mode: "default", tools: &["Read"],
                },
            )?;
            if admit {
                process.admit_session_lines(elsecrew_core::provider::oversized_line::OversizedLinePolicy::ClaudeStreamJson)?;
            }
            let mut seen = Vec::new();
            while seen.len() < 12 {
                if Instant::now() >= deadline { return Err(failure()); }
                match channel.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                    Some(Event::Initialized { .. }) => seen.push("init"),
                    Some(Event::Output(Projection::ToolResults(_))) => seen.push("tool result"),
                    Some(Event::Output(Projection::Assistant { .. })) => seen.push("answer"),
                    Some(_) => seen.push("other"),
                    None => std::thread::sleep(Duration::from_millis(2)),
                }
            }
            Ok(seen)
        })();
        let cleanup = process.finish();
        (result, cleanup.is_ok() && process.closure_evidence().is_some_and(|e| e.confirmed()))
    }

    /// 2026-11-26 big Write: a fake Claude asks to write a 300 KB file whole
    /// (one permission request over the 128 KiB line bound). The session does
    /// not end; the allow reply it reads is the exact original (checked by
    /// SHA-256 on the provider side), and the session goes on to its answer.
    /// `admit` false: the strict bounds still apply outside a session.
    fn large_write_session(admit: bool) -> (Result<Vec<&'static str>, SidecarError>, bool) {
        use elsecrew_core::provider::vendor_observation::claude_approval::Observation;
        use elsecrew_core::provider::vendor_observation::claude_events::Projection;
        use elsecrew_core::provider::vendor_observation::claude_session::Event;
        use sha2::{Digest, Sha256};
        let _slots = crate::provider_login_io::LARGE_LINE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
        let profile = ClaudeProfileBinding::from_storage(&storage).unwrap();
        let home = profile.revalidate().unwrap();
        let mut process = LoginProcess::reserve_claude_owned(profile).unwrap();
        let id = format!("init_{}", process.connection_id().simple());
        let init = serde_json::json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":{}}});
        let reported = serde_json::json!({"type":"system","subtype":"init","uuid":"init","session_id":"s","cwd":"/snapshot","model":"model","permissionMode":"default","tools":["Write"],"mcp_servers":[],"skills":[],"plugins":[]});
        let answer = serde_json::json!({"type":"assistant","uuid":"a1","session_id":"s","parent_tool_use_id":null,
            "message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"Wrote the file."}]}});
        let content = "A".repeat(300_000);
        // The exact reply the provider must read: the original input echoed whole.
        let expected = serde_json::json!({"type":"control_response","response":{"subtype":"success","request_id":"perm_big",
            "response":{"behavior":"allow","updatedInput":{"content":content,"file_path":"notes.md"}}}}).to_string();
        let expected_sha = format!("{:x}", Sha256::digest(expected.as_bytes()));
        let script = format!(
            "IFS= read -r first || exit 1\nprintf '%s\\n' '{init}' '{reported}'\n\
             printf '{{\"type\":\"control_request\",\"request_id\":\"perm_big\",\"request\":{{\"subtype\":\"can_use_tool\",\"tool_name\":\"Write\",\"input\":{{\"file_path\":\"notes.md\",\"content\":\"'; \
             head -c 300000 /dev/zero | tr '\\0' A; printf '\"}},\"tool_use_id\":\"toolu_big\"}}}}\\n'\n\
             IFS= read -r reply || exit 2\n\
             [ \"$(printf '%s' \"$reply\" | /usr/bin/shasum -a 256 | /usr/bin/cut -d' ' -f1)\" = '{expected_sha}' ] || exit 3\n\
             printf '%s\\n' '{answer}'\nIFS= read -r last\n"
        );
        process.launch_gated_command(Path::new("/bin/sh"), &["-c".as_ref(), script.as_ref()], &home).unwrap();
        let cancel = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(10);
        let result = (|| -> Result<Vec<&'static str>, SidecarError> {
            let mut session = ControlInitialization::start(&mut process, &cancel)?;
            while !session.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                if Instant::now() >= deadline { return Err(failure()); }
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut channel = session.into_session_channel(
                &elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit {
                    cwd: "/snapshot", model: "model", permission_mode: "default", tools: &["Write"],
                },
            )?;
            if admit {
                process.admit_session_lines(elsecrew_core::provider::oversized_line::OversizedLinePolicy::ClaudeStreamJson)?;
            }
            let mut seen = Vec::new();
            while !seen.contains(&"answer") {
                if Instant::now() >= deadline { return Err(failure()); }
                match channel.advance(&mut process, &cancel, &mut |_| Ok(()))? {
                    Some(Event::Initialized { .. }) => seen.push("init"),
                    Some(Event::Permission(Observation::Requested(request))) => {
                        if request.withheld() || request.inspect_input().len() < 300_000 { return Err(failure()); }
                        seen.push("large request");
                        channel.allow_once(&mut process, &request, &cancel, &mut |_, _| Ok(()))?;
                    }
                    Some(Event::Output(Projection::Assistant { .. })) => seen.push("answer"),
                    Some(_) => seen.push("other"),
                    None => std::thread::sleep(Duration::from_millis(2)),
                }
            }
            Ok(seen)
        })();
        let cleanup = process.finish();
        (result, cleanup.is_ok() && process.closure_evidence().is_some_and(|e| e.confirmed()))
    }

    #[test]
    fn a_large_write_request_is_allowed_whole_and_the_session_goes_on() {
        let (seen, cleaned) = large_write_session(true);
        assert_eq!(seen.unwrap(), vec!["init", "large request", "answer"]);
        assert!(cleaned);
    }

    #[test]
    fn without_session_lines_a_large_write_request_ends_the_session() {
        let (seen, cleaned) = large_write_session(false);
        assert!(seen.is_err());
        assert!(cleaned);
    }

    #[test]
    fn a_session_keeps_going_after_reading_ten_large_images() {
        let (seen, cleaned) = large_image_session(true);
        let mut expected = vec!["init"];
        expected.extend(["tool result"; 10]);
        expected.push("answer");
        assert_eq!(seen.unwrap(), expected);
        assert!(cleaned);
    }

    #[test]
    fn without_session_lines_the_first_large_image_ends_the_session() {
        let (seen, cleaned) = large_image_session(false);
        assert!(seen.is_err(), "the strict line bound still applies outside a session");
        assert!(cleaned);
    }

    #[test]
    fn permission_denial_round_trips_through_owned_transport() {
        use elsecrew_core::provider::vendor_observation::claude_approval::Observation;
        use elsecrew_core::provider::vendor_observation::claude_session::Event;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let storage = Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
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
                "behavior":"deny","message":"Tool request not allowed: declined by the application user, or outside the permissions or working directory of this role (stay inside it, read only files that exist, keep out of .git, .claude, .codex and .agents). Continue without it.","interrupt":false
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
                &elsecrew_core::provider::vendor_observation::claude_init::ExpectedInit {
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
            // Cancel closes the channel; clearing the flag cannot reopen it.
            cancel.store(true, Ordering::Release);
            if permissions
                .advance(&mut process, &cancel, &mut |_| Ok(()))
                .is_ok()
            {
                return Err(failure());
            }
            cancel.store(false, Ordering::Release);
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
