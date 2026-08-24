//! Codex 계획 역할 실행 계약 검증.
//!
//! 계획 픽스처는 **실측에서 그대로 가져온 최종 메시지**다 — `--output-schema`를 걸고
//! 실제로 받은 출력이며, 손으로 지어낸 모양이 아니다.

use super::*;
use std::path::PathBuf;

/// 실측: `codex exec --sandbox read-only --output-schema …`의 최종 메시지 파일 내용.
const REAL_PLAN: &str = r#"{"summary":"`greeting.txt`의 현재 문구 `hello`를 더 친근한 한국어 인사말로 교체하는 단일 파일 변경입니다.","steps":[{"title":"인사말 변경","detail":"기존 `hello`를 `안녕하세요! 만나서 반가워요.`처럼 따뜻하고 자연스러운 문장으로 교체합니다.","files":["greeting.txt"]},{"title":"변경 검증","detail":"파일을 다시 읽어 새 문구가 정확히 저장되었는지 확인합니다.","files":["greeting.txt"]}]}"#;

fn args() -> Vec<String> {
    codex_plan_args(
        Path::new("/pilots/clone"),
        Path::new("/pilots/.run.schema.json"),
        Path::new("/pilots/.run.last-message.txt"),
    )
    .expect("계약 인자가 스스로의 검증을 통과해야 한다")
}

// ───────────────────────── 실행 인자 ─────────────────────────

#[test]
fn the_planning_role_runs_read_only() {
    let joined = args().join(" ");
    assert!(
        joined.contains("--sandbox read-only"),
        "계획 역할이 읽기 전용이 아니다: {joined}"
    );
}

#[test]
fn args_exclude_user_config_and_rules() {
    let a = args();
    assert!(a.iter().any(|x| x == "--ignore-user-config"));
    assert!(a.iter().any(|x| x == "--ignore-rules"));
}

/// 스키마와 최종 메시지는 **복제본 밖**에 둔다 — 안에 두면 에이전트가 자기 계약을 고친다.
#[test]
fn the_schema_and_final_message_live_outside_the_clone() {
    let a = args();
    let value_after = |flag: &str| {
        let i = a.iter().position(|x| x == flag).unwrap();
        a[i + 1].clone()
    };
    for flag in ["--output-schema", "--output-last-message"] {
        let path = value_after(flag);
        assert!(
            !path.starts_with("/pilots/clone"),
            "{flag}가 복제본 안을 가리킨다: {path}"
        );
    }
}

/// 승격 수단은 하나도 섞이면 안 된다.
#[test]
fn escalation_flags_are_refused() {
    for flag in [
        "--approve-for-me",
        "--dangerously-bypass-approvals-and-sandbox",
        "--dangerously-bypass-hook-trust",
        "--full-auto",
        "--add-dir",
    ] {
        let mut a = args();
        a.insert(1, flag.to_string());
        assert!(validate_codex_plan_args(&a).is_err(), "{flag}를 통과시켰다");
    }
}

/// 쓰기 가능한 샌드박스는 계획 역할이 아니다.
#[test]
fn a_writable_sandbox_is_not_a_planning_role() {
    for mode in ["workspace-write", "danger-full-access"] {
        let mut a = args();
        let i = a.iter().position(|x| x == "read-only").unwrap();
        a[i] = mode.to_string();
        let refused = validate_codex_plan_args(&a).expect_err("{mode}를 통과시켰다");
        assert!(refused.contains("read-only"), "{refused}");
    }
}

/// `approval_policy`는 현재 CLI가 거부한다 — 넣으면 **실행 자체가 실패**한다(실측).
/// 되살아나지 않도록 계약에서 막는다.
#[test]
fn approval_policy_is_refused_because_the_cli_rejects_it() {
    let mut a = args();
    a.insert(1, "approval_policy=\"untrusted\"".to_string());
    a.insert(1, "--config".to_string());

    let refused = validate_codex_plan_args(&a).expect_err("실행을 죽이는 설정을 통과시켰다");

    assert!(refused.contains("approval_policy"), "{refused}");
}

#[test]
fn the_plan_schema_matches_the_other_agent() {
    // 두 에이전트의 계획 모양이 다르면 인계 코드를 하나로 쓸 수 없다
    assert_eq!(PLAN_SCHEMA, crate::claude_pilot::PLAN_SCHEMA);
}

// ───────────────────────── 계획 읽기 ─────────────────────────

#[test]
fn the_measured_plan_is_read_back() {
    let plan = parse_codex_plan(REAL_PLAN).expect("실측 계획을 읽지 못했다");

    assert!(plan.summary.contains("greeting.txt"));
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].files, vec!["greeting.txt".to_string()]);
    assert!(!plan.is_error);
}

/// 계획이 실제로 인계까지 이어지는가 — 두 에이전트가 같은 인계 코드를 쓴다.
#[test]
fn the_plan_can_be_handed_to_the_implementation_role() {
    let plan = parse_codex_plan(REAL_PLAN).unwrap();

    let prompt =
        crate::claude_pilot::plan_to_codex_prompt("인사말 바꿔 줘", &plan).expect("인계가 막혔다");

    assert!(prompt.contains("인사말 바꿔 줘"));
    assert!(prompt.contains("인사말 변경"));
    assert!(prompt.contains("Files: greeting.txt"));
}

/// 스키마를 안 지킨 출력은 계획이 아니다. **산문을 계획인 척 넘기지 않는다.**
#[test]
fn prose_is_not_a_plan() {
    let refused = parse_codex_plan("여기 계획이 있습니다: 파일을 고치세요")
        .expect_err("산문을 계획으로 읽었다");
    assert!(refused.contains("not the pinned structure"), "{refused}");
}

/// 오류 문구에 최종 메시지를 통째로 붓지 않는다 — 화면에 로그가 쏟아진다.
#[test]
fn a_huge_malformed_message_does_not_flood_the_error() {
    let refused = parse_codex_plan(&"가".repeat(5000)).unwrap_err();
    assert!(refused.chars().count() < 300, "{}", refused.len());
}

#[test]
fn an_empty_final_message_is_an_error() {
    assert!(parse_codex_plan("   ").is_err());
}

#[test]
fn an_empty_plan_is_an_error() {
    assert!(parse_codex_plan(r#"{"summary":"","steps":[]}"#).is_err());
}

/// 인사말처럼 **할 일이 없는** 요청도 계획으로 읽힌다 — 되묻기 화면이 그 신호를 쓴다.
#[test]
fn a_no_op_plan_is_still_a_plan() {
    let plan =
        parse_codex_plan(r#"{"summary":"인사말이라 바꿀 것이 없습니다.","steps":[]}"#).unwrap();

    assert!(plan.steps.is_empty());
    assert!(!plan.is_error);
    assert!(!plan.summary.is_empty());
}

/// Codex는 거부된 도구 호출을 기계 판독 가능하게 주지 않는다 — 없는 것을 있는 척하지 않는다.
#[test]
fn codex_reports_no_machine_readable_denials() {
    let plan = parse_codex_plan(REAL_PLAN).unwrap();
    assert!(plan.permission_denials.is_empty());
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {}
