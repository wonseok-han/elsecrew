use crate::agent::Agent;
use crate::error::AppError;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub path: String,
    /// false면 이미 있던 폴더를 그대로 재사용
    pub created: bool,
    /// 안전 프리셋을 실제로 썼는지. 프론트는 이 값으로 안내 문구를 고른다
    pub safety: SafetyStatus,
}

/// 안전 프리셋 파일 하나의 처리 결과.
/// 기존 파일은 절대 덮어쓰지 않으므로 "이미 있었음"이 정상 경로다.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PresetOutcome {
    /// 앱이 새로 만들었다
    Applied,
    /// 이미 있어서 그대로 뒀다
    Kept,
}

impl PresetOutcome {
    fn from_written(written: bool) -> Self {
        if written {
            PresetOutcome::Applied
        } else {
            PresetOutcome::Kept
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafetyStatus {
    /// 초보자 안내 파일 (CLAUDE.md · AGENTS.md)
    pub guide: PresetOutcome,
    /// 권한·승인 설정 (.claude/settings.json · ~/.codex/config.toml)
    pub config: PresetOutcome,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScannedProject {
    pub path: String,
    pub name: String,
    /// 표식으로 추정한 에이전트 ("claude-code" | "codex")
    pub agent: String,
    /// 자동 인계 파일럿을 쓸 수 있는가 — **안전한 작업 사본을 만들 수 있는지**다.
    ///
    /// 에이전트 표식(`CLAUDE.md` 등)이 있다고 파일럿을 쓸 수 있는 건 아니다.
    /// 복제본은 Git 클론으로 만들기 때문이다. 판정은 실행 쪽과 **같은 함수**를 쓴다 —
    /// 기준이 갈리면 눌러도 되는 것처럼 보여 놓고 실행에서 거절하게 된다.
    pub pilot_eligible: bool,
}

/// 기본 프로젝트 폴더(사용자가 지정 안 하면 Documents).
#[tauri::command]
pub fn default_projects_dir() -> String {
    documents_dir().display().to_string()
}

/// 지정한 폴더의 바로 아래 하위 폴더 중, 에이전트 프로젝트 표식이 있는 것을 찾아 목록화한다.
/// 표식: `.claude/`·`CLAUDE.md`(클로드), `AGENTS.md`(코덱스). 앱 기억(store)과 무관하게
/// 디스크에서 실제 프로젝트를 발견하므로, 다른 데서 만든 프로젝트도 잡힌다.
#[tauri::command]
pub async fn scan_projects(base: String) -> Result<Vec<ScannedProject>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = PathBuf::from(&base);
        let mut found = Vec::new();
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            // 에러 문구는 OS마다 다르므로(Windows "cannot find the path" 등)
            // 텍스트가 아니라 io 에러 종류로 판정한다 — 플랫폼 무관.
            let detail = format!(
                "cannot read project base directory '{}': {e}",
                dir.display()
            );
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    AppError::new(crate::error::ErrorKind::NotFound, detail)
                }
                std::io::ErrorKind::PermissionDenied => {
                    AppError::new(crate::error::ErrorKind::Permission, detail)
                }
                _ => AppError::generic(detail),
            }
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(agent) = detect_project_agent(&path) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                found.push(ScannedProject {
                    pilot_eligible: crate::orchestration::can_copy_pilot_source(&path),
                    path: path.display().to_string(),
                    name,
                    agent: agent.into(),
                });
            }
        }
        found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(found)
    })
    .await
    .map_err(|e| AppError::generic(e.to_string()))?
}

/// 폴더의 표식으로 에이전트를 추정 (클로드 우선 — 둘 다 있으면 클로드).
fn detect_project_agent(dir: &Path) -> Option<&'static str> {
    if dir.join(".claude").is_dir() || dir.join("CLAUDE.md").is_file() {
        Some("claude-code")
    } else if dir.join("AGENTS.md").is_file() {
        Some("codex")
    } else {
        None
    }
}

/// 초보자 안내 프리셋: 프로젝트의 에이전트에게 사용자가 초보자임을 알린다.
const BEGINNER_GUIDE_KO: &str = "# 내 첫 프로젝트

이 폴더의 주인은 코딩을 처음 해 보는 사용자예요. 함께 일할 때:

- 쉬운 한국어로, 전문용어 없이 설명해 주세요.
- 파일을 지우거나 이 폴더 밖의 것을 바꾸기 전에는 반드시 먼저 물어봐 주세요.
- 작업을 마치면 무엇을 했는지 쉬운 말로 한 줄 정리해 주세요.
";

const BEGINNER_GUIDE_EN: &str = "# My first project

The owner of this folder is new to coding. When working together:

- Explain things in plain English without jargon.
- Ask before deleting files or changing anything outside this folder.
- When a task is complete, summarize what you changed in one simple sentence.
";

/// 클로드 코드 프로젝트 안전 프리셋 — 민감 파일 읽기와 위험 명령을 기본 차단
/// (초보자의 "11GB 삭제 사건" 류 사고 방지, docs/architecture.md §6)
const CLAUDE_SAFE_SETTINGS: &str = r#"{
  "permissions": {
    "deny": [
      "Read(**/.env)",
      "Read(**/.env.*)",
      "Read(~/.ssh/**)",
      "Bash(rm -rf:*)"
    ]
  }
}
"#;

/// 코덱스 전역 안전 프리셋 — 작업 폴더 밖 쓰기 차단 + 승격 시 사람에게 확인.
/// 사용자가 이미 설정 파일을 갖고 있으면 절대 건드리지 않는다.
///
/// ⚠️ **`approval_policy = "untrusted"`를 쓰면 안 된다.** codex-cli 0.149.1이
/// `approval_policy = "untrusted" is no longer supported; remove this setting`으로
/// **거부하며, 그 설정이 있으면 codex 실행 자체가 실패한다**(실측). 즉 앱이 그 값을 써 두면
/// 신규 사용자의 Codex를 통째로 망가뜨린다. 유효한 값은
/// `on-failure`·`on-request`·`granular`·`never`이며, 승격 시 확인이라는 원래 의도에
/// 가장 가까운 `on-request`를 쓴다.
///
/// **실제 경계는 `sandbox_mode`다**(§7.10.1.2와 같은 원칙 — 실측된 것만 경계로 센다).
/// `on-request`의 구체적 동작은 아직 실측하지 않았다.
const CODEX_SAFE_CONFIG_KO: &str = "# Elsecrew가 만든 초보자 안전 설정
approval_policy = \"on-request\"
sandbox_mode = \"workspace-write\"
";

const CODEX_SAFE_CONFIG_EN: &str = "# Beginner-safe defaults created by Elsecrew
approval_policy = \"on-request\"
sandbox_mode = \"workspace-write\"
";

#[tauri::command]
pub async fn create_first_project(
    agent: String,
    name: Option<String>,
    base: Option<String>,
    language: Option<String>,
) -> Result<ProjectInfo, AppError> {
    let agent = Agent::from_id(&agent)?;
    tauri::async_runtime::spawn_blocking(move || {
        create(agent, name, base, language).map_err(AppError::classify)
    })
    .await
    .map_err(|e| AppError::generic(e.to_string()))?
}

fn create(
    agent: Agent,
    name: Option<String>,
    base: Option<String>,
    language: Option<String>,
) -> Result<ProjectInfo, String> {
    let name = sanitize_name(name.as_deref().unwrap_or("my-first-project"));
    if name.is_empty() {
        return Err("project folder name has no usable characters".into());
    }
    let base_dir = base.map(PathBuf::from).unwrap_or_else(documents_dir);
    let dir = base_dir.join(&name);
    let created = !dir.exists();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create project folder '{}': {e}", dir.display()))?;

    let safety = apply_safety_preset(agent, &dir, language.as_deref() == Some("en"))?;

    // 처음부터 **되돌릴 수 있는 상태**로 만든다. 앱이 만든 프로젝트가 나중에
    // "아직 맡길 수 없어요"에 걸리면, 사용자는 자기가 만들지도 않은 문제를 풀어야 한다.
    // 앱이 방금 만든 폴더이므로 여기서는 동의가 이미 성립한다.
    //
    // **실패해도 프로젝트 생성 자체는 성공이다.** 준비는 파일럿을 쓰기 위한 편의일 뿐이고,
    // 이것 때문에 첫 프로젝트 만들기가 막히면 안 된다. 나중에 카드에서 다시 준비할 수 있다.
    let _ = crate::orchestration::prepare_project_for_pilot(
        &dir,
        language.as_deref() != Some("en"),
        true,
    );

    Ok(ProjectInfo {
        path: dir.display().to_string(),
        created,
        safety,
    })
}

/// 에이전트별 안전 프리셋. 이미 있는 파일은 절대 덮어쓰지 않는다.
/// 무엇을 실제로 썼는지 돌려주므로, 앱이 적용하지 않은 것을 적용했다고 말하지 않는다.
fn apply_safety_preset(agent: Agent, dir: &Path, english: bool) -> Result<SafetyStatus, String> {
    apply_safety_preset_in(agent, dir, english, &crate::detect::home_dir())
}

/// 코덱스 설정은 사용자 전역 홈에 쓰므로, 테스트가 실제 `~/.codex`를 건드리지 않도록
/// 홈 경로를 주입받는다. 실제 실행 경로는 위 `apply_safety_preset`이다.
fn apply_safety_preset_in(
    agent: Agent,
    dir: &Path,
    english: bool,
    home: &Path,
) -> Result<SafetyStatus, String> {
    // 새로 썼으면 true, 이미 있어서 건너뛰었으면 false
    let write_if_absent = |path: PathBuf, content: &str| -> Result<bool, String> {
        if path.exists() {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("cannot create settings folder '{}': {e}", parent.display())
            })?;
        }
        std::fs::write(&path, content)
            .map_err(|e| format!("cannot write settings file '{}': {e}", path.display()))?;
        Ok(true)
    };

    let guide = if english {
        BEGINNER_GUIDE_EN
    } else {
        BEGINNER_GUIDE_KO
    };

    let (guide_written, config_written) = match agent {
        Agent::ClaudeCode => (
            write_if_absent(dir.join("CLAUDE.md"), guide)?,
            write_if_absent(
                dir.join(".claude").join("settings.json"),
                CLAUDE_SAFE_SETTINGS,
            )?,
        ),
        Agent::Codex => {
            let config = if english {
                CODEX_SAFE_CONFIG_EN
            } else {
                CODEX_SAFE_CONFIG_KO
            };
            (
                write_if_absent(dir.join("AGENTS.md"), guide)?,
                // 코덱스의 승인 정책은 전역 설정이므로, 설정 파일이 아예 없을 때만 생성
                write_if_absent(home.join(".codex").join("config.toml"), config)?,
            )
        }
    };

    Ok(SafetyStatus {
        guide: PresetOutcome::from_written(guide_written),
        config: PresetOutcome::from_written(config_written),
    })
}

/// 프로젝트 폴더에서 비대화형으로 첫 인사를 주고받는다.
#[tauri::command]
pub async fn run_first_chat(
    agent: String,
    project_path: String,
    language: Option<String>,
) -> Result<String, AppError> {
    let agent = Agent::from_id(&agent)?;
    tauri::async_runtime::spawn_blocking(move || {
        let dir = PathBuf::from(&project_path);
        if !dir.is_dir() {
            return Err(AppError::not_found("project folder not found"));
        }
        let bin = crate::detect::agent_bin(agent)
            .ok_or_else(|| AppError::not_found(format!("{} is not installed", agent.bin_name())))?;
        let prompt = first_chat_prompt(language.as_deref());
        let out = crate::detect::command(&bin)
            .args(agent.chat_args(prompt))
            .current_dir(&dir)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| AppError::classify(e.to_string()))?;
        let reply = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !reply.is_empty() {
            Ok(clean_reply(agent, &reply))
        } else {
            // 에이전트 stderr에 원인이 담기므로 그대로 분류(네트워크 등)
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(AppError::classify(if err.is_empty() {
                "agent returned no reply".to_string()
            } else {
                err
            }))
        }
    })
    .await
    .map_err(|e| AppError::generic(e.to_string()))?
}

fn first_chat_prompt(language: Option<&str>) -> &'static str {
    if language == Some("en") {
        "Give a new coding-agent user a short, warm welcome in plain English using no more than two sentences."
    } else {
        "코딩 도우미를 처음 만나는 사용자에게 두 문장 이내의 짧고 따뜻한 한국어 환영 인사를 해 주세요."
    }
}

/// codex exec는 응답 앞에 메타 정보 블록을 출력하므로 마지막 문단만 남긴다
fn clean_reply(agent: Agent, raw: &str) -> String {
    match agent {
        Agent::ClaudeCode => raw.to_string(),
        Agent::Codex => raw
            .split("\n\n")
            .filter(|block| !block.trim().is_empty())
            .last()
            .unwrap_or(raw)
            .trim()
            .to_string(),
    }
}

fn documents_dir() -> PathBuf {
    crate::detect::home_dir().join("Documents")
}

/// 경로 구분자·제어문자를 제거하고 앞뒤 공백과 점을 정리한다
fn sanitize_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()
        })
        .collect();
    cleaned.trim().trim_matches('.').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 목록에 **자격까지 실어 보내야** 화면이 실행 전에 판정할 수 있다.
    /// 실사용에서 터진 모양 그대로 — 에이전트 표식만 있고 Git 저장소가 아닌 프로젝트다.
    #[test]
    fn scanned_projects_carry_whether_the_pilot_can_use_them() {
        let base = std::env::temp_dir().join(format!(
            "elsecrew-scan-eligible-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&base);

        // ① 표식만 있고 .git 없음 → 파일럿을 쓸 수 없다
        let notes = base.join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("CLAUDE.md"), "# notes\n").unwrap();
        // ② 표식 + Git 저장소 → 쓸 수 있다
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join("AGENTS.md"), "# repo\n").unwrap();

        let found = tauri::async_runtime::block_on(scan_projects(base.display().to_string()))
            .expect("스캔 실패");

        let by_name = |name: &str| {
            found
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name}을(를) 찾지 못했다: {found:?}"))
        };
        // 둘 다 프로젝트로는 잡힌다 — 자격만 다르다
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            !by_name("notes").pilot_eligible,
            "Git 저장소가 아닌데 맡길 수 있다고 표시했다"
        );
        assert!(by_name("repo").pilot_eligible, "Git 저장소를 거절했다");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sanitize_strips_path_separators() {
        assert_eq!(sanitize_name("../../../etc"), "etc");
        assert_eq!(sanitize_name("내 프로젝트"), "내 프로젝트");
        assert_eq!(sanitize_name("a/b\\c:d"), "abcd");
        assert_eq!(sanitize_name("///"), "");
    }

    #[test]
    fn create_claude_project_with_safety_preset() {
        let base = std::env::temp_dir().join(format!("ha-base-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let name = format!("elsecrew-테스트-{}", std::process::id());
        let base_s = Some(base.display().to_string());
        let info = create(
            Agent::ClaudeCode,
            Some(name.clone()),
            base_s.clone(),
            Some("ko".into()),
        )
        .unwrap();
        let dir = std::path::PathBuf::from(&info.path);
        assert!(info.created);
        assert!(dir.starts_with(&base));
        assert!(dir.join("CLAUDE.md").is_file());
        let settings = std::fs::read_to_string(dir.join(".claude").join("settings.json")).unwrap();
        assert!(settings.contains("deny"));
        serde_json::from_str::<serde_json::Value>(&settings).expect("valid json");
        // 처음이므로 둘 다 앱이 새로 쓴 것
        assert_eq!(info.safety.guide, PresetOutcome::Applied);
        assert_eq!(info.safety.config, PresetOutcome::Applied);
        // 두 번째 호출은 재사용으로 판정되고, 기존 파일을 덮어쓰지 않았음이 보고돼야 함
        let again = create(Agent::ClaudeCode, Some(name), base_s, Some("ko".into())).unwrap();
        assert!(!again.created);
        assert_eq!(again.safety.guide, PresetOutcome::Kept);
        assert_eq!(again.safety.config, PresetOutcome::Kept);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// 안내 파일만 있고 권한 설정은 없는 폴더 — 둘을 따로 보고해야 한다.
    #[test]
    fn safety_status_reports_each_preset_separately() {
        let base = std::env::temp_dir().join(format!("ha-mixed-{}", std::process::id()));
        let dir = base.join("mixed-project");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "사용자가 이미 쓰던 안내").unwrap();

        let info = create(
            Agent::ClaudeCode,
            Some("mixed-project".into()),
            Some(base.display().to_string()),
            Some("ko".into()),
        )
        .unwrap();

        assert_eq!(info.safety.guide, PresetOutcome::Kept);
        assert_eq!(info.safety.config, PresetOutcome::Applied);
        // 기존 안내 파일은 그대로여야 한다
        let guide = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert_eq!(guide, "사용자가 이미 쓰던 안내");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// 코덱스는 전역 홈에 설정을 쓴다. 주입한 임시 홈으로 실제 `~/.codex`를 건드리지 않고 검증한다.
    #[test]
    fn codex_preset_reports_global_config_state() {
        let base = std::env::temp_dir().join(format!("ha-codex-{}", std::process::id()));
        let dir = base.join("codex-project");
        let home = base.join("home");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&home).unwrap();

        // 전역 설정이 없는 상태 — 안내와 설정을 모두 새로 쓴다
        let first = apply_safety_preset_in(Agent::Codex, &dir, false, &home).unwrap();
        assert_eq!(first.guide, PresetOutcome::Applied);
        assert_eq!(first.config, PresetOutcome::Applied);
        let config_path = home.join(".codex").join("config.toml");
        assert!(config_path.is_file());
        assert!(dir.join("AGENTS.md").is_file());

        // 두 번째 호출 — 둘 다 기존 것을 그대로 둔다
        let again = apply_safety_preset_in(Agent::Codex, &dir, false, &home).unwrap();
        assert_eq!(again.guide, PresetOutcome::Kept);
        assert_eq!(again.config, PresetOutcome::Kept);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// 사용자가 이미 전역 설정을 갖고 있으면 덮어쓰지 않고 `Kept`로 보고해야 한다.
    /// 실제 개발 머신이 이 경로에 해당한다(`approval_policy = "never"`).
    #[test]
    fn codex_preset_never_overwrites_existing_global_config() {
        let base = std::env::temp_dir().join(format!("ha-codex-keep-{}", std::process::id()));
        let dir = base.join("codex-project");
        let home = base.join("home");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let existing = "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n";
        std::fs::write(home.join(".codex").join("config.toml"), existing).unwrap();

        let status = apply_safety_preset_in(Agent::Codex, &dir, false, &home).unwrap();
        assert_eq!(status.config, PresetOutcome::Kept);
        assert_eq!(status.guide, PresetOutcome::Applied);
        // 사용자 설정은 한 글자도 바뀌지 않아야 한다
        let after = std::fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
        assert_eq!(after, existing);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// 프론트가 문자열로 분기하므로 직렬화 형태가 계약이다.
    #[test]
    fn safety_status_serializes_as_camel_case_strings() {
        let json = serde_json::to_string(&SafetyStatus {
            guide: PresetOutcome::Applied,
            config: PresetOutcome::Kept,
        })
        .unwrap();
        assert_eq!(json, r#"{"guide":"applied","config":"kept"}"#);
    }

    #[test]
    fn create_project_localizes_beginner_guide() {
        let base = std::env::temp_dir().join(format!("ha-en-base-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let info = create(
            Agent::ClaudeCode,
            Some("english-project".into()),
            Some(base.display().to_string()),
            Some("en".into()),
        )
        .unwrap();
        let guide = std::fs::read_to_string(PathBuf::from(info.path).join("CLAUDE.md")).unwrap();
        assert!(guide.contains("plain English"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn scan_finds_projects_by_marker() {
        let base = std::env::temp_dir().join(format!("ha-scan-{}", std::process::id()));
        // 클로드 표식 폴더
        std::fs::create_dir_all(base.join("proj-claude")).unwrap();
        std::fs::write(base.join("proj-claude").join("CLAUDE.md"), "x").unwrap();
        // 코덱스 표식 폴더
        std::fs::create_dir_all(base.join("proj-codex")).unwrap();
        std::fs::write(base.join("proj-codex").join("AGENTS.md"), "x").unwrap();
        // 표식 없는 폴더 — 제외돼야 함
        std::fs::create_dir_all(base.join("just-a-folder")).unwrap();

        let dir = base.clone();
        let mut found: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| super::detect_project_agent(&e.path()).map(|a| (e.file_name(), a)))
            .collect();
        found.sort_by_key(|(n, _)| n.to_string_lossy().to_string());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].1, "claude-code");
        assert_eq!(found[1].1, "codex");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn scan_reports_missing_base_directory() {
        let missing = std::env::temp_dir().join(format!("ha-missing-{}", std::process::id()));
        std::fs::remove_dir_all(&missing).ok();
        let error = tauri::async_runtime::block_on(scan_projects(missing.display().to_string()))
            .expect_err("missing base directory should be reported");
        assert_eq!(error.kind, crate::error::ErrorKind::NotFound);
    }

    #[test]
    fn codex_reply_cleanup_keeps_last_block() {
        let raw = "[2026-07-16] OpenAI Codex v0.144.5\nworkdir: /tmp\n\ntokens used: 123\n\n안녕하세요! 만나서 반가워요.";
        assert_eq!(
            clean_reply(Agent::Codex, raw),
            "안녕하세요! 만나서 반가워요."
        );
    }

    #[test]
    fn first_chat_prompt_follows_language() {
        assert!(first_chat_prompt(Some("ko")).contains("한국어"));
        assert!(first_chat_prompt(Some("en")).contains("English"));
    }

    /// 실제 첫 대화 실행 (사용량 소모). 실행: cargo test -- --ignored --nocapture
    #[test]
    #[ignore = "claude 로그인 + 사용량 소모"]
    fn first_chat_on_this_machine() {
        let home = std::env::temp_dir().join(format!("elsecrew-chat-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let reply = tauri::async_runtime::block_on(run_first_chat(
            "claude-code".into(),
            home.display().to_string(),
            Some("ko".into()),
        ))
        .unwrap();
        println!("응답: {reply}");
        assert!(!reply.is_empty());
        std::fs::remove_dir_all(&home).ok();
    }
}
