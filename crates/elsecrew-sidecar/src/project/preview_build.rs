//! Build-then-preview plan (결과 미리보기 확장 1단계, 2026-11-27;
//! `docs/architecture.md` "결과 미리보기" → "빌드해서 보기").
//!
//! Read-only, like the rest of the preview: this only looks at the task's
//! own workspace and says what a build would need. The app's main process
//! runs the build itself, after the person agrees in main's own box.
//!
//! - The workspace comes from the chat's durable binding (never the page),
//!   checked like `preview.resolve` (binding, manifest, lock, Git link).
//! - `package.json` and the build tool's config are read through the
//!   verified handle without following links, bounded in size.
//! - The build output and installed packages must be ignored by Git in that
//!   workspace (`git check-ignore`): the app records each step with
//!   `git add -A`, so a build that is not ignored would put its output (or
//!   `node_modules`) into the task's history. Not ignored is a refusal
//!   (fail closed), not a warning.
//! - The command lines are not decided here: the facts (package manager,
//!   lockfile, whether packages are installed, output folder) go to main,
//!   whose pure policy (`electron/preview-build-policy.ts`) turns them into
//!   fixed argument lists.

use super::*;
use super::preview::{preview_chat_address, regular_file};
use elsecrew_protocol::preview_v1::preview_path_refusal_v1;

const PACKAGE_JSON_MAX_BYTES: u64 = 256 * 1024;
const BUILD_CONFIG_MAX_BYTES: u64 = 64 * 1024;
/// The build script is shown in the consent box; longer is cut.
const BUILD_SCRIPT_SHOWN_CHARS: usize = 300;
const VITE_CONFIGS: &[&str] = &["vite.config.ts", "vite.config.js", "vite.config.mjs", "vite.config.mts", "vite.config.cjs", "vite.config.cts"];

/// What a workspace's `package.json` says about building it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BuildFacts {
    pub manager: &'static str,
    pub lockfile: bool,
    pub build_script: String,
    pub output_dir: String,
}

/// The facts, or why this workspace cannot be built here. `exists` answers
/// whether a top-level file is a regular file (no links). `vite_config` is
/// the first Vite config's text, if any.
pub(crate) fn build_facts(
    package: &Value,
    exists: &dyn Fn(&str) -> bool,
    vite_config: Option<&str>,
) -> Result<BuildFacts, &'static str> {
    let Some(package) = package.as_object() else { return Err("invalidPackage") };
    let script = match package.get("scripts").and_then(|scripts| scripts.get("build")) {
        Some(Value::String(script)) if !script.trim().is_empty() => script.trim().to_string(),
        Some(_) => return Err("invalidPackage"),
        None => return Err("noBuildScript"),
    };
    let declared = package.get("packageManager").and_then(Value::as_str).unwrap_or("");
    let manager = if declared.starts_with("pnpm@") {
        "pnpm"
    } else if declared.starts_with("yarn@") {
        "yarn"
    } else if declared.starts_with("npm@") {
        "npm"
    } else if !declared.is_empty() || exists("bun.lockb") || exists("bun.lock") {
        return Err("unsupportedManager");
    } else if exists("pnpm-lock.yaml") {
        "pnpm"
    } else if exists("yarn.lock") {
        "yarn"
    } else {
        "npm"
    };
    let lockfile = match manager {
        "pnpm" => exists("pnpm-lock.yaml"),
        "yarn" => exists("yarn.lock"),
        _ => exists("package-lock.json") || exists("npm-shrinkwrap.json"),
    };
    let depends_on = |name: &str| {
        ["dependencies", "devDependencies"].iter().any(|field| {
            package.get(*field).and_then(Value::as_object).is_some_and(|deps| deps.contains_key(name))
        })
    };
    // Where the output goes: an explicit flag in the script, then the Vite
    // config, then the tool's own default.
    let named = flag_value(&script, &["--outDir", "--out-dir"])
        .or_else(|| vite_config.and_then(vite_out_dir));
    let output_dir = match named {
        Some(dir) => safe_output_dir(&dir).ok_or("unsafeOutput")?,
        None if depends_on("react-scripts") => "build".to_string(),
        None => "dist".to_string(),
    };
    Ok(BuildFacts { manager, lockfile, build_script: script, output_dir })
}

/// `--outDir x`, `--outDir=x` in a script line.
fn flag_value(script: &str, flags: &[&str]) -> Option<String> {
    let words: Vec<&str> = script.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        for flag in flags {
            if let Some(value) = word.strip_prefix(&format!("{flag}=")) {
                return Some(value.trim_matches(['"', '\'']).to_string());
            }
            if word == flag {
                return words.get(index + 1).map(|value| value.trim_matches(['"', '\'']).to_string());
            }
        }
    }
    None
}

/// `outDir: 'x'` in a Vite config (a plain string only; anything computed is
/// not followed and the default stays).
fn vite_out_dir(config: &str) -> Option<String> {
    let at = config.find("outDir")?;
    let rest = config[at + "outDir".len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"' | '`'))?;
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// A folder inside the workspace the preview can serve: relative, plain
/// segments, never a dot folder, the workspace itself or its sources.
pub(crate) fn safe_output_dir(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches("./").trim_end_matches('/');
    if trimmed.is_empty() || trimmed.len() > 120 || trimmed.split('/').count() > 4 {
        return None;
    }
    if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/')) {
        return None;
    }
    if ["src", "public", "node_modules"].contains(&trimmed.split('/').next().unwrap_or("")) {
        return None;
    }
    preview_path_refusal_v1(&format!("{trimmed}/index.html")).is_none().then(|| trimmed.to_string())
}

fn read_small_file(root: &Dir, path: &str, limit: u64) -> Result<Option<Vec<u8>>, &'static str> {
    let Ok((parent, name)) = open_parent_capability(root, path) else { return Ok(None) };
    let Ok(file) = open_real_file_at(&parent, &name) else { return Ok(None) };
    let declared = file.metadata().map(|metadata| metadata.len()).unwrap_or(u64::MAX);
    if declared > limit {
        return Err("invalidPackage");
    }
    let mut file = file;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file).take(limit + 1).read_to_end(&mut bytes).map_err(|_| "invalidPackage")?;
    if bytes.len() as u64 > limit {
        return Err("invalidPackage");
    }
    Ok(Some(bytes))
}

impl ProjectManager {
    pub fn preview_build_plan(
        &mut self,
        storage: &Storage,
        project_id: &str,
        chat_id: &str,
    ) -> Result<ProjectAction, ProjectFailure> {
        let address = preview_chat_address(storage, project_id, chat_id)?;
        let (active, root) = self.validate_workspace_binding_with_lock_handle(
            storage, &address, "active", WorkspaceLockAdmission::Ready,
        )?;
        let answer = |refusal: Option<&str>, plan: Option<(&str, &BuildFacts, bool)>| ProjectAction {
            result: json!({
                "chatId": address.chat_id,
                "projectId": address.project_id,
                "worktreeId": address.worktree_id,
                "workspaceIdentity": address.workspace_identity,
                "refusal": refusal,
                "workspacePath": plan.map(|(path, _, _)| path),
                "packageManager": plan.map(|(_, facts, _)| facts.manager),
                "lockfile": plan.is_some_and(|(_, facts, _)| facts.lockfile),
                "buildScript": plan.map(|(_, facts, _)| facts.build_script.chars().take(BUILD_SCRIPT_SHOWN_CHARS).collect::<String>()),
                "outputDir": plan.map(|(_, facts, _)| facts.output_dir.as_str()),
                "needsInstall": plan.is_some_and(|(_, _, needs)| needs),
            }),
            ..ProjectAction::default()
        };
        // An old standalone copy has no Git ignore rules of the task's own.
        if active.kind == WorkspaceKind::Snapshot {
            return Ok(answer(Some("unsupportedWorkspace"), None));
        }
        self.verify_snapshot_repository(&root, &active)?;
        let package = match read_small_file(&root, "package.json", PACKAGE_JSON_MAX_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(answer(Some("noPackage"), None)),
            Err(refusal) => return Ok(answer(Some(refusal), None)),
        };
        let Ok(package) = serde_json::from_slice::<Value>(&package) else {
            return Ok(answer(Some("invalidPackage"), None));
        };
        let vite_config = VITE_CONFIGS
            .iter()
            .find_map(|name| read_small_file(&root, name, BUILD_CONFIG_MAX_BYTES).ok().flatten())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let facts = match build_facts(&package, &|name| regular_file(&root, name), vite_config.as_deref()) {
            Ok(facts) => facts,
            Err(refusal) => return Ok(answer(Some(refusal), None)),
        };
        // The app records each step with `git add -A`: what the build and the
        // installation write must be ignored, or they would join the task.
        let ignored = |probe: &str| -> Result<bool, ProjectFailure> {
            let output = self.git_allow_status_at(&root, ["check-ignore", "-q", "--", probe], &[0, 1])?;
            Ok(output.status.code() == Some(0))
        };
        if !ignored("node_modules/package.json")? {
            return Ok(answer(Some("dependenciesNotIgnored"), None));
        }
        if !ignored(&format!("{}/index.html", facts.output_dir))? {
            return Ok(answer(Some("outputNotIgnored"), None));
        }
        let Some(path) = active.worktree_path.to_str() else {
            return Ok(answer(Some("unsupportedWorkspace"), None));
        };
        let needs_install = open_real_child_dir(&root, "node_modules").is_err();
        Ok(answer(None, Some((path, &facts, needs_install))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(package: Value, files: &[&str], vite: Option<&str>) -> Result<BuildFacts, &'static str> {
        let files: Vec<String> = files.iter().map(|file| file.to_string()).collect();
        build_facts(&package, &|name| files.iter().any(|file| file == name), vite)
    }

    #[test]
    fn a_vite_app_builds_with_its_own_manager_into_dist() {
        let package = json!({"scripts": {"build": "tsc -b && vite build"}, "devDependencies": {"vite": "^6"}});
        assert_eq!(facts(package.clone(), &["package-lock.json"], None).unwrap(),
            BuildFacts { manager: "npm", lockfile: true, build_script: "tsc -b && vite build".into(), output_dir: "dist".into() });
        assert_eq!(facts(package.clone(), &["pnpm-lock.yaml"], None).unwrap().manager, "pnpm");
        assert_eq!(facts(package.clone(), &["yarn.lock"], None).unwrap().manager, "yarn");
        let none = facts(package.clone(), &[], None).unwrap();
        assert_eq!((none.manager, none.lockfile), ("npm", false));
        // The declared manager wins over the lockfiles.
        let mut declared = package.clone();
        declared["packageManager"] = json!("pnpm@9.1.0");
        let pnpm = facts(declared, &["package-lock.json"], None).unwrap();
        assert_eq!((pnpm.manager, pnpm.lockfile), ("pnpm", false));
        // The config's outDir, then the script's flag (which wins).
        assert_eq!(facts(package.clone(), &[], Some("export default { build: { outDir: 'web-out' } }")).unwrap().output_dir, "web-out");
        let flagged = json!({"scripts": {"build": "vite build --outDir=site"}});
        assert_eq!(facts(flagged, &[], Some("build: { outDir: \"other\" }")).unwrap().output_dir, "site");
        // Create React App builds into build/.
        let cra = json!({"scripts": {"build": "react-scripts build"}, "dependencies": {"react-scripts": "5"}});
        assert_eq!(facts(cra, &[], None).unwrap().output_dir, "build");
    }

    #[test]
    fn what_cannot_be_built_says_why() {
        assert_eq!(facts(json!({"scripts": {"dev": "vite"}}), &[], None), Err("noBuildScript"));
        assert_eq!(facts(json!({"scripts": {"build": 1}}), &[], None), Err("invalidPackage"));
        assert_eq!(facts(json!({"scripts": {"build": "  "}}), &[], None), Err("invalidPackage"));
        assert_eq!(facts(json!([]), &[], None), Err("invalidPackage"));
        assert_eq!(facts(json!({"scripts": {"build": "b"}}), &["bun.lockb"], None), Err("unsupportedManager"));
        assert_eq!(facts(json!({"scripts": {"build": "b"}, "packageManager": "bun@1"}), &[], None), Err("unsupportedManager"));
        // An output folder that is not a plain folder inside the workspace.
        for dir in ["../out", "/tmp/out", ".", "./", ".hidden", "src", "public/x", "node_modules/x", "a/../b", "C:\\out", "a/b/c/d/e"] {
            let package = json!({"scripts": {"build": format!("vite build --outDir {dir}")}});
            assert_eq!(facts(package, &[], None), Err("unsafeOutput"), "{dir}");
        }
        assert_eq!(safe_output_dir("./dist/"), Some("dist".into()));
        assert_eq!(safe_output_dir("out/web"), Some("out/web".into()));
    }
}
