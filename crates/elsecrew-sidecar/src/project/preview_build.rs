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
/// What the person agrees to run is fixed by these files (PR #22 review B1):
/// the package, its lockfiles, the package managers' own settings and hooks,
/// and the build tool's config. Their bytes (or absence) go into the plan's
/// fingerprint; main runs only when the fingerprint it showed is still the
/// fingerprint right before starting.
const FINGERPRINT_FILES: &[&str] = &[
    "package.json", "package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml", "yarn.lock",
    ".npmrc", ".yarnrc", ".yarnrc.yml", "pnpm-workspace.yaml", ".pnpmfile.cjs",
    "vite.config.ts", "vite.config.js", "vite.config.mjs", "vite.config.mts", "vite.config.cjs", "vite.config.cts",
];
/// A lockfile can be large; hashing stops being reasonable past this.
const FINGERPRINT_FILE_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Shown in the consent box: the scripts npm runs around `build`.
const BUILD_HOOKS: &[&str] = &["prebuild", "postbuild"];
const DEPENDENCIES_SHOWN: usize = 40;

/// What a workspace's `package.json` says about building it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BuildFacts {
    pub manager: &'static str,
    pub lockfile: bool,
    pub build_script: String,
    pub output_dir: String,
    /// `prebuild`/`postbuild` scripts npm runs with `build`.
    pub hooks: Vec<(String, String)>,
    /// Declared package names (dependencies and devDependencies), sorted.
    pub dependencies: Vec<String>,
    /// Yarn 2+ (its install takes other flags).
    pub yarn_berry: bool,
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
    let hooks = BUILD_HOOKS.iter().filter_map(|name| {
        package.get("scripts").and_then(|scripts| scripts.get(*name)).and_then(Value::as_str)
            .map(|script| ((*name).to_string(), script.trim().to_string()))
    }).collect();
    let mut dependencies: Vec<String> = ["dependencies", "devDependencies"].iter()
        .filter_map(|field| package.get(*field).and_then(Value::as_object))
        .flat_map(|deps| deps.keys().cloned())
        .collect();
    dependencies.sort();
    dependencies.dedup();
    let yarn_berry = manager == "yarn"
        && (exists(".yarnrc.yml") || declared.strip_prefix("yarn@").is_some_and(|version| !version.starts_with('1')));
    Ok(BuildFacts { manager, lockfile, build_script: script, output_dir, hooks, dependencies, yarn_berry })
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

/// The SHA-256 of a regular file read through the verified handle, streamed;
/// `None` when it is absent (or a link: links are not followed and count as absent).
fn file_digest(root: &Dir, path: &str) -> Result<Option<String>, &'static str> {
    let Ok((parent, name)) = open_parent_capability(root, path) else { return Ok(None) };
    let Ok(file) = open_real_file_at(&parent, &name) else { return Ok(None) };
    let mut file = file;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(|_| "invalidPackage")?;
        if read == 0 { break; }
        total += read as u64;
        if total > FINGERPRINT_FILE_MAX_BYTES { return Err("invalidPackage"); }
        hasher.update(&buffer[..read]);
    }
    Ok(Some(format!("{:x}", hasher.finalize())))
}

/// One value for everything the consent box stands for: the workspace
/// folder (its path and identity) and each input file's bytes or absence.
pub(crate) fn plan_fingerprint(path: &str, identity: &str, files: &[(&str, Option<String>)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"elsecrew-preview-build-plan-v1\0");
    hasher.update(path.as_bytes());
    hasher.update(b"\0");
    hasher.update(identity.as_bytes());
    for (name, digest) in files {
        hasher.update(b"\0");
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(digest.as_deref().unwrap_or("absent").as_bytes());
    }
    format!("{:x}", hasher.finalize())
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
        let shown = |text: &str| text.chars().take(BUILD_SCRIPT_SHOWN_CHARS).collect::<String>();
        let answer = |refusal: Option<&str>, plan: Option<(&str, &BuildFacts, bool, &str)>| ProjectAction {
            result: json!({
                "chatId": address.chat_id,
                "projectId": address.project_id,
                "worktreeId": address.worktree_id,
                "workspaceIdentity": address.workspace_identity,
                "refusal": refusal,
                "workspacePath": plan.map(|(path, _, _, _)| path),
                "packageManager": plan.map(|(_, facts, _, _)| facts.manager),
                "lockfile": plan.is_some_and(|(_, facts, _, _)| facts.lockfile),
                "buildScript": plan.map(|(_, facts, _, _)| shown(&facts.build_script)),
                "buildHooks": plan.map(|(_, facts, _, _)| facts.hooks.iter().map(|(name, script)| json!({"name": name, "script": shown(script)})).collect::<Vec<_>>()),
                "dependencies": plan.map(|(_, facts, _, _)| facts.dependencies.iter().take(DEPENDENCIES_SHOWN).cloned().collect::<Vec<_>>()),
                "dependencyCount": plan.map(|(_, facts, _, _)| facts.dependencies.len()),
                "yarnBerry": plan.is_some_and(|(_, facts, _, _)| facts.yarn_berry),
                "outputDir": plan.map(|(_, facts, _, _)| facts.output_dir.as_str()),
                "needsInstall": plan.is_some_and(|(_, _, needs, _)| needs),
                "fingerprint": plan.map(|(_, _, _, fingerprint)| fingerprint),
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
        // The folders themselves (PR #22 review I1: two sample files were not
        // enough), then what is really there now: any file under them that
        // `git add -A` would take (a `!negation` rule, say) refuses as well.
        // Main asks again after every job, so a build that wrote such files
        // never counts as done.
        let ignored = |probe: &str| -> Result<bool, ProjectFailure> {
            let output = self.git_allow_status_at(&root, ["check-ignore", "-q", "--", probe], &[0, 1])?;
            Ok(output.status.code() == Some(0))
        };
        let output_dir = facts.output_dir.clone();
        if !ignored("node_modules/")? || !ignored("node_modules/package.json")? || self.git_would_record(&root, "node_modules")? {
            return Ok(answer(Some("dependenciesNotIgnored"), None));
        }
        if !ignored(&format!("{output_dir}/"))? || !ignored(&format!("{output_dir}/index.html"))? || !ignored(&format!("{output_dir}/assets/index.js"))?
            || self.git_would_record(&root, &output_dir)? {
            return Ok(answer(Some("outputNotIgnored"), None));
        }
        let Some(path) = active.worktree_path.to_str() else {
            return Ok(answer(Some("unsupportedWorkspace"), None));
        };
        let mut digests = Vec::with_capacity(FINGERPRINT_FILES.len());
        for name in FINGERPRINT_FILES {
            match file_digest(&root, name) {
                Ok(digest) => digests.push((*name, digest)),
                Err(refusal) => return Ok(answer(Some(refusal), None)),
            }
        }
        let identity = format!("{:?}", root_identity_for_plan(&root)?);
        let fingerprint = plan_fingerprint(path, &identity, &digests);
        let needs_install = open_real_child_dir(&root, "node_modules").is_err();
        Ok(answer(None, Some((path, &facts, needs_install, &fingerprint))))
    }
}

impl ProjectManager {
    /// Whether `git add -A` would record anything under `folder` now.
    fn git_would_record(&self, root: &Dir, folder: &str) -> Result<bool, ProjectFailure> {
        let output = self.git_at(root, ["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", folder])?;
        Ok(!output.stdout.is_empty())
    }
}

#[cfg(unix)]
fn root_identity_for_plan(root: &Dir) -> Result<(u64, u64), ProjectFailure> { capability_directory_identity(root) }
#[cfg(not(unix))]
fn root_identity_for_plan(root: &Dir) -> Result<String, ProjectFailure> {
    let _ = root;
    Err(ProjectFailure::recovery("build preview is unavailable on this platform"))
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
        let package = json!({"scripts": {"build": "tsc -b && vite build", "prebuild": "node gen.js", "dev": "vite"},
            "dependencies": {"react": "18"}, "devDependencies": {"vite": "^6", "react": "18"}});
        assert_eq!(facts(package.clone(), &["package-lock.json"], None).unwrap(),
            BuildFacts { manager: "npm", lockfile: true, build_script: "tsc -b && vite build".into(), output_dir: "dist".into(),
                hooks: vec![("prebuild".into(), "node gen.js".into())], dependencies: vec!["react".into(), "vite".into()], yarn_berry: false });
        // Yarn 2+ (its own settings file, or the declared version).
        assert!(facts(package.clone(), &["yarn.lock", ".yarnrc.yml"], None).unwrap().yarn_berry);
        let mut berry = package.clone();
        berry["packageManager"] = json!("yarn@4.1.0");
        assert!(facts(berry, &[], None).unwrap().yarn_berry);
        assert!(!facts(package.clone(), &["yarn.lock"], None).unwrap().yarn_berry);
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

    #[test]
    fn the_fingerprint_changes_with_any_input_or_the_folder() {
        let files = |lock: Option<&str>| vec![("package.json", Some("a".to_string())), ("package-lock.json", lock.map(str::to_string))];
        let base = plan_fingerprint("/w/t1", "(1, 2)", &files(Some("b")));
        assert_eq!(base, plan_fingerprint("/w/t1", "(1, 2)", &files(Some("b"))));
        assert_ne!(base, plan_fingerprint("/w/t1", "(1, 2)", &files(Some("c"))), "a changed lockfile");
        assert_ne!(base, plan_fingerprint("/w/t1", "(1, 2)", &files(None)), "a removed lockfile");
        assert_ne!(base, plan_fingerprint("/w/t1", "(1, 3)", &files(Some("b"))), "another folder at the same path");
        assert_ne!(base, plan_fingerprint("/w/t2", "(1, 2)", &files(Some("b"))), "another path");
    }
}
