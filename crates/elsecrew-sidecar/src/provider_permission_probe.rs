//! Test-only, model-free permission experiment. Not compiled into the product.
//! Installed cases require explicit approval for their trust boundary; toy cases do not.
//! The trusted-parent case does not claim OS-level offline confinement.
use crate::provider_login_io::NativeFrame;
use crate::provider_login_process::LoginProcess;
use crate::provider_profile::CodexProfileBinding;
use crate::{SidecarError, Storage};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub(crate) struct ProbeFrame(String);
impl ProbeFrame {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

fn failure() -> SidecarError {
    SidecarError::new(
        "permissionProbeFailed",
        "offline permission probe did not match",
    )
}

fn configuration_arguments(home: &Path, workspace: &Path) -> Vec<std::ffi::OsString> {
    use elsecrew_core::provider::vendor_observation::codex_preflight::{
        ExpectedPermission, ExpectedProfile,
    };
    let mut args = crate::provider_preflight_session::named_discovery_arguments(
        home,
        workspace,
        &ExpectedProfile {
            model: "test-model",
            reasoning_effort: "medium",
        },
        ExpectedPermission::WorkspaceWrite,
    )
    .unwrap();
    // Probe never asks a user/agent to expand permissions, even if enforcement fails.
    for arg in &mut args {
        if arg == "approval_policy=\"on-request\"" {
            *arg = "approval_policy=\"never\"".into();
        }
    }
    args.push("--strict-config".into());
    args
}

use crate::provider_preflight_session::named_permissions::{
    expected_write_policy as expected_policy, matches_write_policy as matches_policy,
};

// Fixed sentinel-only command: output never contains file contents. No model,
// login, user path, credential, or free-form caller-supplied command is accepted.
fn probe_command(home: &Path, workspace: &Path) -> Value {
    json!({"command":["/bin/sh","-c",r#"
if /bin/cat "$1" >/dev/null 2>&1; then exit 21; fi
if (printf changed >> "$1") 2>/dev/null; then exit 22; fi
/bin/sh -c '
  if /bin/cat "$1" >/dev/null 2>&1; then exit 23; fi
  if (printf changed >> "$1") 2>/dev/null; then exit 24; fi
' child "$1" || exit $?
printf workspace-ok > "$2/probe-result"
printf protected
"#, "permission-probe", home.join("skills/credential-sentinel"), workspace],
        "cwd":workspace,"timeoutMs":2000})
}

fn exchange(
    process: &mut LoginProcess<'_>,
    home: &Path,
    workspace: &Path,
) -> Result<(), SidecarError> {
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let remaining = || {
        Duration::from_secs(15)
            .checked_sub(started.elapsed())
            .ok_or_else(failure)
    };
    process.begin_preflight()?;
    let initialize: Value = serde_json::from_str(
        &elsecrew_core::provider::vendor_observation::codex_rpc::ModelFreeExchange::with_named_permission_config(workspace).map_err(|_| failure())?
            .next_request().map_err(|_| failure())?.ok_or_else(failure)?
    ).map_err(|_| failure())?;
    for frame in [
        initialize,
        json!({"method":"initialized"}),
        json!({"id":2,"method":"config/read","params":{"includeLayers":true}}),
        json!({"id":51,"method":"permissionProfile/list","params":{"cwd":workspace,"limit":256}}),
        json!({"id":3,"method":"command/exec","params":probe_command(home, workspace)}),
    ] {
        remaining()?;
        let id = frame.get("id").and_then(Value::as_u64);
        process.queue_native(NativeFrame::PermissionProbe(ProbeFrame(format!(
            "{frame}\n"
        ))))?;
        while !process.flush_native_observed(&cancel, &mut |_| Ok(()))? {
            remaining()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        let Some(id) = id else { continue };
        let reply = process
            .receive_permission_probe(remaining()?, &cancel)
            .map_err(|error| {
                SidecarError::new(
                    "permissionProbeFailed",
                    format!("RPC {id} receive failed: {}", error.detail()),
                )
            })?;
        if reply["id"] != id || reply.get("error").is_some() {
            return Err(SidecarError::new(
                "permissionProbeFailed",
                format!("RPC {id} rejected or mismatched"),
            ));
        }
        let result = &reply["result"];
        match id {
            1 if !result.is_object() => return Err(failure()),
            2 => {
                let config = &result["config"];
                let layers_match = crate::provider_preflight_session::named_permissions::matches_configuration_layers(
                    result, home,
                    &elsecrew_core::provider::vendor_observation::codex_preflight::ExpectedProfile { model:"test-model", reasoning_effort:"medium" },
                    &expected_policy(home, workspace), "never",
                );
                if !layers_match
                    || config["default_permissions"] != "elsecrew-snapshot"
                    || config["approval_policy"] != "never"
                    || !config["sandbox_mode"].is_null()
                    || !config["sandbox_workspace_write"].is_null()
                    || !matches_policy(
                        &config["permissions"]["elsecrew-snapshot"],
                        home,
                        workspace,
                    )
                {
                    return Err(SidecarError::new(
                        "permissionProbeFailed",
                        format!("permission config mismatch: layers={layers_match}, selection={}, approval={}, legacy_mode_absent={}, legacy_write_absent={}, policy={}",
                            config["default_permissions"] == "elsecrew-snapshot",
                            config["approval_policy"] == "never",
                            config["sandbox_mode"].is_null(),
                            config["sandbox_workspace_write"].is_null(),
                            matches_policy(&config["permissions"]["elsecrew-snapshot"], home, workspace)),
                    ));
                }
            }
            51 => {
                let observation = elsecrew_core::provider::BoundedObservation::new(
                    serde_json::to_vec(result).map_err(|_| failure())?,
                )
                .map_err(|_| failure())?;
                elsecrew_core::provider::vendor_observation::codex_preflight::match_named_permission_profile(
                    &observation, "elsecrew-snapshot"
                ).map_err(|_| failure())?;
            }
            3 if result["exitCode"] != 0 || result["stdout"] != "protected" => {
                return Err(SidecarError::new(
                    "permissionProbeFailed",
                    format!(
                        "sentinel command mismatch: exit={:?}, nested_sandbox_denied={}",
                        result["exitCode"].as_i64(),
                        result["stderr"].as_str().is_some_and(|value| value
                            .contains("sandbox-exec: sandbox_apply: Operation not permitted"))
                    ),
                ))
            }
            _ => {}
        }
    }
    Ok(())
}

fn run_probe(installed: bool, rejected: bool, vendor_sandbox: bool) {
    if installed {
        let approval = if vendor_sandbox {
            "ELSECREW_APPROVED_VENDOR_SANDBOX_PROBE"
        } else {
            "ELSECREW_APPROVED_PERMISSION_PROBE"
        };
        assert_eq!(
            std::env::var(approval).as_deref(),
            Ok("1"),
            "explicit approval for the selected command/exec trust boundary required"
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let storage = Storage::open(&root.join("elsecrew.sqlite3")).unwrap();
    let profile = CodexProfileBinding::from_storage(&storage).unwrap();
    let home = profile.revalidate().unwrap();
    let workspace = root.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    // Runtime artifact subdirectory avoids changing the login-only root contract.
    std::fs::create_dir_all(home.join("skills")).unwrap();
    std::fs::write(home.join("skills/credential-sentinel"), b"synthetic-only").unwrap();
    let mut process = LoginProcess::reserve(&profile).unwrap();
    let launch = if installed {
        use crate::provider_executable::PinnedExecutable;
        let cli = PinnedExecutable::open_codex_signed(Path::new("/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex"), false).unwrap();
        let helper = PinnedExecutable::open_codex_signed(
            Path::new(
                "/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex-code-mode-host",
            ),
            true,
        )
        .unwrap();
        process.launch_with_barrier(
            || {
                let arguments = configuration_arguments(&home, &workspace);
                // Separate, explicitly approved experiment. The parent is trusted;
                // only its tools are sandboxed. This is NOT an offline guarantee.
                let mut command = if vendor_sandbox {
                    helper.revalidate().map_err(std::io::Error::other)?;
                    let args: Vec<_> = arguments.iter().map(|arg| arg.as_os_str()).collect();
                    crate::provider_login_gate::command(
                        cli.revalidate().map_err(std::io::Error::other)?,
                        &args,
                        &workspace,
                    )?
                } else {
                    crate::provider_runtime_sandbox::command(
                        cli.revalidate().map_err(std::io::Error::other)?,
                        helper.revalidate().map_err(std::io::Error::other)?,
                        &arguments,
                        &home,
                        &workspace,
                        crate::provider_runtime_sandbox::WorkspaceAccess::Write,
                    )?
                };
                command
                    .env("HOME", &home)
                    .env("CODEX_HOME", &home)
                    .env("TMPDIR", home.join("tmp"))
                    .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
                command.spawn()
            },
            |child| {
                cli.revalidate().map_err(std::io::Error::other)?;
                helper.revalidate().map_err(std::io::Error::other)?;
                crate::provider_login_gate::release(child)
            },
        )
    } else {
        let flags = json!({
            "model":"test-model","model_provider":"openai","model_reasoning_effort":"medium",
            "features":{"hooks":false,"apps":false,"plugins":false},
            "default_permissions":"elsecrew-snapshot",
            "approval_policy":"never",
            "permissions":{"elsecrew-snapshot":expected_policy(&home, &workspace)}
        });
        let config_response = json!({"id":2,"result":{"config":flags,"layers":[
            {"name":{"type":"sessionFlags"},"config":flags},
            {"name":{"type":"user","file":home.join("config.toml")},"config":{"cli_auth_credentials_store":"file"}},
            {"name":{"type":"system","file":"/etc/codex/config.toml"},"config":{}}
        ]}});
        let script = format!(
            r#"
IFS= read -r line || exit 1
printf '%s\n' '{{"id":1,"result":{{}}}}'
IFS= read -r line || exit 1
IFS= read -r line || exit 1
printf '%s\n' '{config_response}'
IFS= read -r line || exit 1
printf '%s\n' '{{"id":51,"result":{{"data":[{{"id":"elsecrew-snapshot","allowed":true}}],"nextCursor":null}}}}'
IFS= read -r line || exit 1
printf '%s\n' '{{"id":3,"result":{{"exitCode":{},"stdout":"protected"}}}}'
"#,
            if rejected { 21 } else { 0 }
        );
        process.launch_gated_command(
            Path::new("/bin/sh"),
            &["-c".as_ref(), script.as_ref()],
            &home,
        )
    };
    let observation = launch.and_then(|()| exchange(&mut process, &home, &workspace));
    let cleanup = process.finish();
    if cleanup.is_err()
        || !process
            .closure_evidence()
            .is_some_and(|value| value.confirmed())
    {
        let retained = temp.keep();
        panic!(
            "probe cleanup unconfirmed; retained fixture at {}",
            retained.display()
        );
    }
    assert_eq!(
        std::fs::read(home.join("skills/credential-sentinel")).unwrap(),
        b"synthetic-only"
    );
    assert!(!home.join("auth.json").exists());
    if installed && observation.is_ok() {
        assert_eq!(
            std::fs::read(workspace.join("probe-result")).unwrap(),
            b"workspace-ok"
        );
    }
    assert_eq!(
        observation.is_err(),
        rejected,
        "cleanup confirmed, sentinel unchanged, auth absent; sanitized probe result: {:?}",
        observation
            .as_ref()
            .err()
            .map(|error| (error.kind(), error.detail()))
    );
}

#[test]
fn toy_permission_probe_uses_owned_cleanup() {
    run_probe(false, false, false);
    run_probe(false, true, false);
}

#[test]
fn normalized_policy_does_not_accept_added_authority() {
    let home = Path::new("/synthetic/profile");
    let workspace = Path::new("/synthetic/workspace");
    let mut valid = expected_policy(home, workspace);
    valid["extends"] = Value::Null;
    valid["network"]["domains"] = Value::Null;
    assert!(matches_policy(&valid, home, workspace));
    for (field, value) in [
        ("extends", json!(":workspace")),
        ("workspace_roots", json!({"/outside":true})),
        ("unknown", Value::Null),
    ] {
        let mut changed = valid.clone();
        changed[field] = value;
        assert!(!matches_policy(&changed, home, workspace));
    }
    let mut changed = valid;
    changed["network"]["enabled"] = json!(true);
    assert!(!matches_policy(&changed, home, workspace));
}

#[test]
fn permission_probe_never_mixes_legacy_sandbox_flags() {
    let arguments = configuration_arguments(
        Path::new("/synthetic/profile"),
        Path::new("/synthetic/workspace"),
    );
    let arguments: Vec<_> = arguments.iter().map(|arg| arg.to_str().unwrap()).collect();
    assert!(arguments.contains(&"default_permissions=\"elsecrew-snapshot\""));
    assert!(arguments.contains(&"approval_policy=\"never\""));
    // Override keys are split by the CLI, not parsed as quoted TOML keys.
    // Keep special filesystem keys inside one TOML value instead.
    assert_eq!(
        arguments
            .iter()
            .filter(|arg| arg.starts_with("permissions="))
            .count(),
        1
    );
    assert!(!arguments.iter().any(|arg| arg.starts_with("permissions.")));
    assert!(!arguments
        .iter()
        .any(|arg| arg.contains("sandbox_mode") || arg.contains("sandbox_workspace_write")));
    let command = probe_command(
        Path::new("/synthetic/profile"),
        Path::new("/synthetic/workspace"),
    );
    assert_eq!(command["command"][0], "/bin/sh");
    assert_eq!(
        command["command"][4],
        "/synthetic/profile/skills/credential-sentinel"
    );
    assert_eq!(command["timeoutMs"], 2000);
    assert!(command.get("sandboxPolicy").is_none());
}

#[test]
#[ignore = "requires explicit installed CLI offline command/exec approval"]
fn installed_permission_profile_blocks_profile_access() {
    run_probe(true, false, false);
}

#[test]
#[ignore = "requires explicit trusted-parent vendor-sandbox command/exec approval; not offline"]
fn installed_vendor_sandbox_blocks_profile_access() {
    run_probe(true, false, true);
}
