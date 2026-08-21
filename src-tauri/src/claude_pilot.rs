//! Claude 계획 역할의 실행 계약 (`docs/proposals/m4-orchestration.md` §7.10.1.2 ④).
//!
//! S2가 만든 Codex 실행 경계에 대응하는 Claude 쪽 구현이다. 계약의 요점:
//!
//! - **읽기 전용을 강제하는 것은 `--tools` 허용목록뿐이다.** `--disallowed-tools`는
//!   `Task`·`ToolSearch` 등을 남기고 거부 증거도 남기지 않아 쓰지 않는다.
//! - **`--setting-sources ''`** 로 대상 프로젝트의 설정·훅·MCP·스킬·`CLAUDE.md`를 배제한다.
//!   `--setting-sources project`는 권한을 `bypassPermissions`로 승격시킨다(실측).
//! - 형식은 **`stream-json --verbose` 고정**이다. `system:init`이 없으면 아래 검증을 할 수 없고,
//!   **검증할 수 없는 실행은 하지 않는다.**
//! - `system:init`을 **fail-closed로 검증**한다. 하나라도 불일치하면 실행을 중단하고 `poisoned`다.
//! - 작업 폴더 경계와 실행 후 빈 diff는 **예방책이 아니라 사후 확인 수단**이다.

use serde::Serialize;
use std::path::Path;

/// Claude가 Codex에 넘길 계획의 구조. 자유 서술 산문을 그대로 인계하지 않는다(§7.10.1).
pub(crate) const PLAN_SCHEMA: &str = r#"{"type":"object","properties":{"summary":{"type":"string"},"steps":{"type":"array","items":{"type":"object","properties":{"title":{"type":"string"},"detail":{"type":"string"},"files":{"type":"array","items":{"type":"string"}}},"required":["title","detail","files"],"additionalProperties":false}}},"required":["summary","steps"],"additionalProperties":false}"#;

/// 계획 역할에 허용하는 도구. **읽기 전용 3개뿐이다.**
pub(crate) const ALLOWED_TOOLS: &str = "Read,Glob,Grep";

/// `system:init`에 나타나야 하는 도구 집합 — **정확히 이 4개다.**
///
/// ⚠️ `--json-schema`가 요청하지 않은 `StructuredOutput`을 **주입한다**(실측).
/// 기대값을 `{Read,Glob,Grep}`으로 적으면 정상 실행이 전부 poisoned가 되고,
/// 부분집합 검사로 풀면 경계 붕괴를 놓친다. 그래서 **집합 일치**로 고정한다.
pub(crate) const EXPECTED_INIT_TOOLS: [&str; 4] = ["Glob", "Grep", "Read", "StructuredOutput"];

pub(crate) const EXPECTED_PERMISSION_MODE: &str = "dontAsk";

/// 앱이 `--model`로 넘기는 별칭.
pub(crate) const PINNED_MODEL: &str = "sonnet";

/// `system:init`의 `model`로 **허용하는 정확한 값 목록**.
///
/// 부분 문자열 대조(`contains("sonnet")`)를 쓰지 않는다 — `claude-sonnet-5-evil`처럼
/// 별칭을 품은 임의의 이름이 통과한다. 여기 적힌 값은 실측(E2)에서 그대로 관측한 것뿐이며,
/// CLI 판올림으로 이름이 바뀌면 **계약이 fail-closed로 막히는 것이 의도한 동작**이다.
/// 그때 할 일은 목록을 넓히는 것이 아니라 다시 실측해 값을 갱신하는 것이다.
pub(crate) const ALLOWED_INIT_MODELS: [&str; 1] = ["claude-sonnet-5"];

/// 고정 실행 인자. 순서를 바꾸더라도 **`--tools`와 `--setting-sources ''`는 빠질 수 없다.**
///
/// 프롬프트는 인자로 넘기지 않는다 — 변수 인자 플래그가 뒤따르는 프롬프트를 삼킨다(실측).
/// 반드시 **stdin**으로 전달한다.
pub(crate) fn claude_pilot_args(session_id: &str) -> Vec<String> {
    vec![
        "-p".into(),
        // init 검증이 필수라 형식은 선택지가 아니다
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--json-schema".into(),
        PLAN_SCHEMA.into(),
        // ★ 유일한 실질 도구 경계
        "--tools".into(),
        ALLOWED_TOOLS.into(),
        // 2차 방어 + permission_denials 증거
        "--permission-mode".into(),
        EXPECTED_PERMISSION_MODE.into(),
        // ★ 대상 프로젝트 설정·훅·MCP·스킬·CLAUDE.md 배제
        "--setting-sources".into(),
        String::new(),
        "--safe-mode".into(),
        "--strict-mcp-config".into(),
        "--no-session-persistence".into(),
        "--model".into(),
        PINNED_MODEL.into(),
        "--session-id".into(),
        session_id.into(),
    ]
}

/// `system:init`에서 계약 검증에 쓰는 사실들.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitFacts {
    pub tools: Vec<String>,
    pub permission_mode: String,
    pub model: String,
    pub mcp_servers: usize,
    pub plugins: usize,
    pub cwd: String,
    pub session_id: String,
    /// 검증 대상이 아니라 **원장 기록용**. 계약이 판올림에 깨졌는지 사후 추적한다.
    pub cli_version: String,
}

/// 계약 위반. 어느 항목이 깨졌는지 그대로 남긴다 — 증거가 되어야 한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InitViolation(pub String);

impl std::fmt::Display for InitViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 첫 이벤트에서 사실을 뽑는다. `system:init`이 아니면 그 자체가 위반이다.
pub(crate) fn parse_init(line: &str) -> Result<InitFacts, InitViolation> {
    let event: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| InitViolation(format!("first event is not JSON: {e}")))?;
    let is_init = event.get("type").and_then(|v| v.as_str()) == Some("system")
        && event.get("subtype").and_then(|v| v.as_str()) == Some("init");
    if !is_init {
        return Err(InitViolation(
            "first event is not system:init; cannot verify the execution contract".into(),
        ));
    }
    let text = |key: &str| {
        event
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let count = |key: &str| {
        event
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(usize::MAX)
    };
    // 도구 목록은 **유일한 실질 경계**다. 읽을 수 없는 항목을 걸러내며 읽으면
    // `["Read","Glob","Grep","StructuredOutput",{…}]`처럼 문자열이 아닌 항목이 섞여도
    // 기대 집합과 일치해 통과한다. 하나라도 문자열이 아니면 그 자체가 위반이다.
    let raw_tools = event
        .get("tools")
        .and_then(|v| v.as_array())
        .ok_or_else(|| InitViolation("init has no readable tool list".into()))?;
    let mut tools: Vec<String> = Vec::with_capacity(raw_tools.len());
    for item in raw_tools {
        let name = item
            .as_str()
            .ok_or_else(|| InitViolation(format!("tool list has a non-string entry: {item}")))?;
        tools.push(name.to_string());
    }
    tools.sort();

    Ok(InitFacts {
        tools,
        permission_mode: text("permissionMode"),
        model: text("model"),
        mcp_servers: count("mcp_servers"),
        plugins: count("plugins"),
        cwd: text("cwd"),
        session_id: text("session_id"),
        cli_version: text("claude_code_version"),
    })
}

/// 계약을 **fail-closed로** 검증한다. 통과한 항목만 근거로 삼고, 판단할 수 없으면 통과로 치지 않는다.
///
/// `skills`·`slash_commands`·`agents`는 **일부러 검증하지 않는다** — `Skill`·`Task` 도구가
/// 도구 목록에 없어 호출 수단이 없고, CLI 내장 목록을 고정하면 판올림마다 계약이 부서진다.
pub(crate) fn validate_init(
    facts: &InitFacts,
    expected_cwd: &Path,
    expected_session_id: &str,
) -> Result<(), InitViolation> {
    let mut expected_tools: Vec<String> =
        EXPECTED_INIT_TOOLS.iter().map(|s| s.to_string()).collect();
    expected_tools.sort();
    if facts.tools != expected_tools {
        return Err(InitViolation(format!(
            "tool set mismatch: expected {expected_tools:?}, got {:?}",
            facts.tools
        )));
    }
    if facts.permission_mode != EXPECTED_PERMISSION_MODE {
        return Err(InitViolation(format!(
            "permission mode was escalated: expected '{EXPECTED_PERMISSION_MODE}', got '{}'",
            facts.permission_mode
        )));
    }
    if facts.mcp_servers != 0 {
        return Err(InitViolation(format!(
            "{} MCP server(s) were loaded; expected none",
            facts.mcp_servers
        )));
    }
    if facts.plugins != 0 {
        return Err(InitViolation(format!(
            "{} plugin(s) were loaded; expected none",
            facts.plugins
        )));
    }
    if !ALLOWED_INIT_MODELS.contains(&facts.model.as_str()) {
        return Err(InitViolation(format!(
            "model was overridden: expected one of {ALLOWED_INIT_MODELS:?}, got '{}'",
            facts.model
        )));
    }
    if !same_path(&facts.cwd, expected_cwd) {
        return Err(InitViolation(format!(
            "working directory mismatch: expected '{}', got '{}'",
            expected_cwd.display(),
            facts.cwd
        )));
    }
    if facts.session_id != expected_session_id {
        return Err(InitViolation(format!(
            "session id mismatch: expected '{expected_session_id}', got '{}'",
            facts.session_id
        )));
    }
    Ok(())
}

/// `/tmp`와 `/private/tmp`처럼 심링크로 갈리는 경로를 정규화해 비교한다.
fn same_path(actual: &str, expected: &Path) -> bool {
    let normalize = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    normalize(Path::new(actual)) == normalize(expected)
}

/// 실행 결과에서 뽑아내는 인계 자료.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanResult {
    pub summary: String,
    pub steps: Vec<PlanStep>,
    /// 거부된 도구 호출. **Codex와 달리 기계 판독이 된다** — 증거의 1급 근거로 쓴다.
    pub permission_denials: Vec<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanStep {
    pub title: String,
    pub detail: String,
    pub files: Vec<String>,
}

/// `result` 이벤트에서 구조화 계획을 뽑는다.
/// `--json-schema`를 걸었으므로 `structured_output`이 1차 근거이고, 없으면 `result` 텍스트를 판다.
pub(crate) fn parse_result(line: &str) -> Result<PlanResult, String> {
    let event: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("result event is not JSON: {e}"))?;

    let denials = event
        .get("permission_denials")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|d| d.get("tool_name").and_then(|v| v.as_str()))
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();
    let is_error = event
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let structured = event.get("structured_output").cloned().or_else(|| {
        event
            .get("result")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
    });
    let Some(structured) = structured else {
        return Err("run produced no structured plan".into());
    };

    let summary = structured
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let steps: Vec<PlanStep> = structured
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| PlanStep {
                    title: item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    detail: item
                        .get("detail")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    files: item
                        .get("files")
                        .and_then(|v| v.as_array())
                        .map(|f| {
                            f.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();

    if summary.is_empty() && steps.is_empty() {
        return Err("structured plan is empty".into());
    }
    Ok(PlanResult {
        summary,
        steps,
        permission_denials: denials,
        is_error,
    })
}

/// 계획을 Codex가 실행할 지시문으로 바꾼다. **앱이 인계한다 — 사용자가 옮겨 나르지 않는다.**
///
/// 실패로 끝난 실행(`is_error`)은 **여기서 거부한다.** 호출부의 검사에 맡기면
/// 경로가 하나 늘 때마다 다시 잊을 수 있다. 인계 함수 자체가 막으면 구조적으로 불가능해진다.
pub(crate) fn plan_to_codex_prompt(goal: &str, plan: &PlanResult) -> Result<String, String> {
    if plan.is_error {
        return Err("the planning step ended in an error; its plan is not handed off".into());
    }
    if plan.steps.is_empty() {
        return Err("the planning step produced no steps to implement".into());
    }
    let mut out = String::new();
    out.push_str("Implement the following plan in this repository.\n\n");
    out.push_str(&format!("User goal: {goal}\n\n"));
    if !plan.summary.is_empty() {
        out.push_str(&format!("Plan summary: {}\n\n", plan.summary));
    }
    out.push_str("Steps:\n");
    for (index, step) in plan.steps.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", index + 1, step.title));
        if !step.detail.is_empty() {
            out.push_str(&format!("   {}\n", step.detail));
        }
        if !step.files.is_empty() {
            out.push_str(&format!("   Files: {}\n", step.files.join(", ")));
        }
    }
    out.push_str("\nMake the changes on disk. Do not ask follow-up questions.\n");
    Ok(out)
}

#[cfg(test)]
mod tests;
