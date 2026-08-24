//! Codex 계획 역할의 실행 계약.
//!
//! `claude_pilot`의 대응물이다. **다만 계약의 강도가 같지 않고, 그 차이를 숨기지 않는다.**
//!
//! | | Claude | Codex |
//! | --- | --- | --- |
//! | 읽기 전용 강제 | `--tools` 허용목록 | `--sandbox read-only` |
//! | 실측 | 도구 자체가 없음 | 샌드박스 안 `EXIT=1`, `operation not permitted` |
//! | 사용자 설정 배제 | `--setting-sources ''` | `--ignore-user-config --ignore-rules` |
//! | 구조화 계획 | `--json-schema <인라인>` | `--output-schema <파일>` |
//! | **실행 구성 검증** | `system:init`을 fail-closed로 검증 | **불가 — 대응 이벤트가 없다** |
//!
//! 마지막 줄이 핵심이다. Codex는 `thread.started{thread_id}`만 주고, 어떤 샌드박스로 떴는지
//! 어떤 도구를 쥐었는지 **이벤트로 알려주지 않는다.** 그래서 Claude처럼 "실제로 이렇게 떴다"를
//! 확인할 수 없고, 다음 세 가지에 기댄다:
//!
//! 1. **우리가 argv를 통제한다** — 인자를 검증해 승격 플래그가 섞이지 못하게 한다
//! 2. **샌드박스는 실측된 경계다** — 위 표의 실측이 근거다
//! 3. **사후 빈 diff** — 계획 역할이 정말 아무것도 안 바꿨는지 확인한다
//!
//! ⚠️ 3번은 **예방책이 아니라 사후 확인**이다(§7.10.1.2 ④와 같은 규칙). 즉 Codex 계획 역할의
//! 계약은 Claude보다 약하며, 그 사실을 계약서에 적어 둔다.

// 아직 `handoff`가 이 계약을 부르지 않는다 — 실행 감독부를 두 에이전트가 공유하도록
// 바꾸는 작업이 남았다(§7.10.3). 계약과 검증은 먼저 굳혀 두고, 배선만 이어 붙인다.
#![cfg_attr(not(test), allow(dead_code))]

use crate::claude_pilot::{PlanResult, PlanStep};
use std::path::Path;

/// 계획의 구조. Claude 쪽과 **같은 모양**이어야 인계 코드를 하나로 쓸 수 있다.
pub(crate) const PLAN_SCHEMA: &str = crate::claude_pilot::PLAN_SCHEMA;

/// 계획 역할의 고정 실행 인자.
///
/// 스키마는 **파일로만** 받는다(`--output-schema <FILE>`) — Claude처럼 인라인이 안 된다.
/// 파일은 복제본 **밖**에 둬야 한다. 안에 두면 에이전트가 자기 계약을 고칠 수 있다.
pub(crate) fn codex_plan_args(
    workspace: &Path,
    schema: &Path,
    last_message: &Path,
) -> Result<Vec<String>, String> {
    let args = vec![
        "exec".into(),
        // 사용자 설정·규칙이 계획 실행에 끼어들지 못하게 한다
        "--ignore-user-config".into(),
        "--ignore-rules".into(),
        // 사용자 설정을 무시하면 **신뢰 폴더 목록도 비어** codex가 실행을 거부한다(실측:
        // `Not inside a trusted directory and --skip-git-repo-check was not specified`).
        // 대상이 앱이 만든 Git 복제본이라는 것은 **우리가 이미 검증한다**
        // (`validate_pilot_workspace` — 매니페스트·시작 커밋). 그 검사를 대신하는 게 아니라,
        // codex의 중복 확인을 건너뛰는 것뿐이다.
        "--skip-git-repo-check".into(),
        // ★ 유일한 실질 경계. 실측으로 쓰기가 막힘을 확인했다
        "--sandbox".into(),
        "read-only".into(),
        "--output-schema".into(),
        schema.display().to_string(),
        // 세션 ID는 `thread.started`에 실려 온다
        "--json".into(),
        "--output-last-message".into(),
        last_message.display().to_string(),
        "--cd".into(),
        workspace.display().to_string(),
        "--".into(),
        planning_prompt_marker().into(),
    ];
    validate_codex_plan_args(&args)?;
    Ok(args)
}

/// 프롬프트 자리를 표시하는 자리표. 호출부가 마지막 인자를 실제 프롬프트로 바꾼다.
fn planning_prompt_marker() -> &'static str {
    "<prompt>"
}

/// 계획 역할 인자 검증. **승격 수단이 하나라도 섞이면 거부한다.**
pub(crate) fn validate_codex_plan_args(args: &[String]) -> Result<(), String> {
    const FORBIDDEN: &[&str] = &[
        "--approve-for-me",
        "--dangerously-bypass-approvals-and-sandbox",
        "--dangerously-bypass-hook-trust",
        "--full-auto",
        "--add-dir",
    ];
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| "codex plan prompt must be separated from options".to_string())?;
    if separator + 2 != args.len() {
        return Err("codex plan must contain exactly one prompt after --".into());
    }
    let options = &args[..separator];

    if let Some(flag) = options.iter().find(|arg| FORBIDDEN.contains(&arg.as_str())) {
        return Err(format!("forbidden codex plan flag: {flag}"));
    }
    // ★ 계획 역할은 **읽기 전용**이다. 다른 샌드박스는 계획 역할이 아니다
    let sandboxes: Vec<_> = options
        .windows(2)
        .filter(|pair| pair[0] == "--sandbox")
        .map(|pair| pair[1].as_str())
        .collect();
    if sandboxes != ["read-only"] {
        return Err("codex plan must use the read-only sandbox".into());
    }
    // 승인 정책은 현재 CLI가 거부한다 — 넣으면 실행 자체가 실패한다(실측)
    if options
        .windows(2)
        .any(|pair| pair[0] == "--config" && pair[1].starts_with("approval_policy="))
    {
        return Err("codex plan must not set approval_policy; the current CLI rejects it".into());
    }
    if options
        .iter()
        .filter(|arg| *arg == "--ignore-user-config")
        .count()
        != 1
        || options
            .iter()
            .filter(|arg| *arg == "--ignore-rules")
            .count()
            != 1
    {
        return Err("codex plan must ignore user config and rules".into());
    }
    // 이게 빠지면 실행이 시작조차 못 한다(실측)
    if options
        .iter()
        .filter(|arg| *arg == "--skip-git-repo-check")
        .count()
        != 1
    {
        return Err("codex plan must skip the redundant git repo check".into());
    }
    if options
        .iter()
        .filter(|arg| *arg == "--output-schema")
        .count()
        != 1
    {
        return Err("codex plan must pin the plan schema".into());
    }
    if options
        .iter()
        .filter(|arg| *arg == "--output-last-message")
        .count()
        != 1
    {
        return Err("codex plan must capture the final message".into());
    }
    Ok(())
}

/// 최종 메시지에서 구조화 계획을 뽑는다.
///
/// Claude는 `result` 이벤트의 `structured_output`에 실어 주지만, Codex는 **최종 메시지 파일이
/// 곧 구조화 출력**이다(실측). 그래서 파일 내용을 그대로 JSON으로 읽는다.
pub(crate) fn parse_codex_plan(last_message: &str) -> Result<PlanResult, String> {
    let text = last_message.trim();
    if text.is_empty() {
        return Err("codex plan produced no final message".into());
    }
    let structured: serde_json::Value = serde_json::from_str(text).map_err(|_| {
        format!(
            "codex plan is not the pinned structure: {}",
            first_line(text)
        )
    })?;

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
        return Err("codex plan is empty".into());
    }
    Ok(PlanResult {
        summary,
        steps,
        // Codex는 거부된 도구 호출을 기계 판독 가능한 형태로 주지 않는다 —
        // Claude의 `permission_denials`에 대응하는 것이 없다(§7.10.1.1).
        permission_denials: Vec::new(),
        is_error: false,
    })
}

/// 오류 문구에 최종 메시지를 통째로 붓지 않는다 — 화면에 로그가 쏟아진다.
fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    if line.chars().count() <= 200 {
        return line.to_string();
    }
    line.chars().take(200).collect::<String>() + "…"
}

#[cfg(test)]
mod tests;
