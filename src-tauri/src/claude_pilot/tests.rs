//! 실행 계약 검증 테스트.
//!
//! `system:init` 픽스처는 **실측(E2)에서 그대로 가져온 CLI 출력**이다
//! (`docs/proposals/m4-orchestration.md` §7.10.1.2 ④). 손으로 지어낸 모양이 아니다.

use super::*;
use std::path::PathBuf;

/// E2 실측의 실제 `system:init` — 적대적 픽스처 위에서 계약을 걸고 얻은 것.
const REAL_INIT: &str = r#"{"type":"system","subtype":"init","tools":["Glob","Grep","Read","StructuredOutput"],"permissionMode":"dontAsk","model":"claude-sonnet-5","mcp_servers":[],"plugins":[],"cwd":"CWD_PLACEHOLDER","session_id":"3f2b1c40-0000-4000-8000-000000000002","claude_code_version":"2.1.235"}"#;

/// B4 실측의 `system:init` — `--setting-sources project`로 프로젝트 설정이 개입한 경우.
/// 권한이 `bypassPermissions`로 승격되고 모델이 탈취되고 MCP가 떴다.
const HOSTILE_INIT: &str = r#"{"type":"system","subtype":"init","tools":["Bash","Edit","Read","Write"],"permissionMode":"bypassPermissions","model":"claude-opus-5","mcp_servers":[{"name":"evilmcp","status":"pending"}],"plugins":[],"cwd":"CWD_PLACEHOLDER","session_id":"3f2b1c40-0000-4000-8000-000000000002","claude_code_version":"2.1.235"}"#;

const SESSION: &str = "3f2b1c40-0000-4000-8000-000000000002";

fn workspace() -> PathBuf {
    std::env::temp_dir()
}

fn init_line(template: &str) -> String {
    template.replace(
        "CWD_PLACEHOLDER",
        &workspace().display().to_string().replace('\\', "\\\\"),
    )
}

// ───────────────────────── 실행 인자 ─────────────────────────

#[test]
fn args_pin_the_measured_contract() {
    let args = claude_pilot_args(SESSION);
    let joined = args.join(" ");

    // 형식은 선택지가 아니다 — init 검증이 필수라 stream-json으로 고정한다
    assert!(joined.contains("--output-format stream-json"));
    assert!(args.iter().any(|a| a == "--verbose"));
    // 유일한 실질 도구 경계
    assert!(joined.contains("--tools Read,Glob,Grep"));
    // 대상 프로젝트 설정 배제 — 값은 빈 문자열이어야 한다
    let sources = args.iter().position(|a| a == "--setting-sources").unwrap();
    assert_eq!(
        args[sources + 1],
        "",
        "--setting-sources 값이 비어 있어야 한다"
    );
    for flag in [
        "--safe-mode",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--json-schema",
    ] {
        assert!(args.iter().any(|a| a == flag), "{flag}가 빠졌다");
    }
    assert!(joined.contains("--permission-mode dontAsk"));
    assert!(joined.contains(&format!("--session-id {SESSION}")));
}

/// 계약에서 **제거된** 플래그가 되살아나지 않도록 고정한다(§9 철회된 주장).
#[test]
fn args_never_use_retracted_flags() {
    let args = claude_pilot_args(SESSION);
    for banned in [
        "--disallowed-tools",
        "--allowed-tools",
        "--allowedTools",
        "--disallowedTools",
        "--dangerously-skip-permissions",
        "--permission-mode bypassPermissions",
    ] {
        assert!(
            !args.join(" ").contains(banned),
            "{banned}는 계약에서 제거됐다"
        );
    }
    // `--setting-sources project`는 프로젝트 설정을 로드한다 — 안전 정책이 아니다
    let sources = args.iter().position(|a| a == "--setting-sources").unwrap();
    assert_ne!(args[sources + 1], "project");
}

/// 프롬프트는 인자로 넘기지 않는다 — 변수 인자 플래그가 삼킨다(실측).
#[test]
fn args_carry_no_prompt() {
    let args = claude_pilot_args(SESSION);
    assert!(
        !args
            .iter()
            .any(|a| a.contains("Implement") || a.contains("goal")),
        "프롬프트는 stdin으로만 전달한다"
    );
}

// ───────────────────────── init 검증 ─────────────────────────

#[test]
fn real_init_passes_validation() {
    let facts = parse_init(&init_line(REAL_INIT)).expect("init 파싱 실패");
    assert_eq!(facts.cli_version, "2.1.235");
    validate_init(&facts, &workspace(), SESSION).expect("실측 init이 계약을 통과해야 한다");
}

/// `--json-schema`가 주입하는 `StructuredOutput`을 기대 집합에 넣지 않으면
/// **정상 실행이 전부 막힌다.** 그 회귀를 고정한다.
#[test]
fn structured_output_tool_is_expected_not_a_violation() {
    assert!(EXPECTED_INIT_TOOLS.contains(&"StructuredOutput"));
    let facts = parse_init(&init_line(REAL_INIT)).unwrap();
    assert!(facts.tools.contains(&"StructuredOutput".to_string()));
    assert!(validate_init(&facts, &workspace(), SESSION).is_ok());
}

/// 프로젝트 설정이 개입한 실행은 **거부**돼야 한다.
#[test]
fn hostile_project_settings_are_rejected() {
    let facts = parse_init(&init_line(HOSTILE_INIT)).unwrap();
    let violation =
        validate_init(&facts, &workspace(), SESSION).expect_err("권한이 승격된 실행을 통과시켰다");
    // 도구 집합이 먼저 깨지므로 그 항목이 잡힌다
    assert!(violation.0.contains("tool set mismatch"), "{violation}");
}

#[test]
fn escalated_permission_mode_is_rejected() {
    let facts = InitFacts {
        permission_mode: "bypassPermissions".into(),
        ..parse_init(&init_line(REAL_INIT)).unwrap()
    };
    let violation = validate_init(&facts, &workspace(), SESSION).unwrap_err();
    assert!(violation.0.contains("permission mode"), "{violation}");
}

#[test]
fn extra_tool_is_rejected() {
    let mut facts = parse_init(&init_line(REAL_INIT)).unwrap();
    facts.tools.push("Bash".into());
    facts.tools.sort();
    let violation = validate_init(&facts, &workspace(), SESSION).unwrap_err();
    assert!(violation.0.contains("tool set mismatch"), "{violation}");
}

#[test]
fn loaded_mcp_server_is_rejected() {
    let facts = InitFacts {
        mcp_servers: 1,
        ..parse_init(&init_line(REAL_INIT)).unwrap()
    };
    let violation = validate_init(&facts, &workspace(), SESSION).unwrap_err();
    assert!(violation.0.contains("MCP server"), "{violation}");
}

#[test]
fn loaded_plugin_is_rejected() {
    let facts = InitFacts {
        plugins: 2,
        ..parse_init(&init_line(REAL_INIT)).unwrap()
    };
    let violation = validate_init(&facts, &workspace(), SESSION).unwrap_err();
    assert!(violation.0.contains("plugin"), "{violation}");
}

#[test]
fn hijacked_model_is_rejected() {
    let facts = InitFacts {
        model: "claude-opus-5".into(),
        ..parse_init(&init_line(REAL_INIT)).unwrap()
    };
    let violation = validate_init(&facts, &workspace(), SESSION).unwrap_err();
    assert!(violation.0.contains("model"), "{violation}");
}

/// 부분 문자열 대조였다면 **별칭을 품은 아무 이름이나** 통과한다.
/// 허용값은 실측된 정확한 목록이어야 한다.
#[test]
fn model_name_merely_containing_the_alias_is_rejected() {
    for impostor in [
        "claude-sonnet-5-evil",
        "not-sonnet-at-all",
        "sonnet",
        "SONNET-5",
        "",
    ] {
        let facts = InitFacts {
            model: impostor.into(),
            ..parse_init(&init_line(REAL_INIT)).unwrap()
        };
        assert!(
            validate_init(&facts, &workspace(), SESSION).is_err(),
            "'{impostor}'을(를) 통과시켰다"
        );
    }
    // 실측값만 통과한다
    assert!(ALLOWED_INIT_MODELS.contains(&"claude-sonnet-5"));
}

/// 도구 목록은 유일한 실질 경계다. 읽을 수 없는 항목을 **걸러내며** 읽으면
/// 문자열이 아닌 항목이 섞여도 기대 집합과 일치해 통과한다.
#[test]
fn non_string_tool_entry_is_a_violation() {
    let line = r#"{"type":"system","subtype":"init","tools":["Glob","Grep","Read","StructuredOutput",{"name":"Bash"}],"permissionMode":"dontAsk","model":"claude-sonnet-5","mcp_servers":[],"plugins":[],"cwd":"/tmp","session_id":"s","claude_code_version":"2.1.235"}"#;
    let err = parse_init(line).expect_err("문자열이 아닌 도구 항목을 통과시켰다");
    assert!(err.0.contains("non-string entry"), "{err}");
}

#[test]
fn missing_tool_list_is_a_violation() {
    let line = r#"{"type":"system","subtype":"init","permissionMode":"dontAsk","model":"claude-sonnet-5","mcp_servers":[],"plugins":[],"cwd":"/tmp","session_id":"s"}"#;
    let err = parse_init(line).expect_err("도구 목록이 없는 init을 통과시켰다");
    assert!(err.0.contains("no readable tool list"), "{err}");
}

#[test]
fn tool_list_that_is_not_an_array_is_a_violation() {
    let line = r#"{"type":"system","subtype":"init","tools":"Read,Glob,Grep","permissionMode":"dontAsk","model":"claude-sonnet-5","mcp_servers":[],"plugins":[],"cwd":"/tmp","session_id":"s"}"#;
    assert!(parse_init(line).is_err());
}

#[test]
fn different_working_directory_is_rejected() {
    let facts = parse_init(&init_line(REAL_INIT)).unwrap();
    let elsewhere = workspace().join("some-other-clone");
    let violation = validate_init(&facts, &elsewhere, SESSION).unwrap_err();
    assert!(violation.0.contains("working directory"), "{violation}");
}

#[test]
fn different_session_id_is_rejected() {
    let facts = parse_init(&init_line(REAL_INIT)).unwrap();
    let violation =
        validate_init(&facts, &workspace(), "00000000-0000-4000-8000-000000000000").unwrap_err();
    assert!(violation.0.contains("session id"), "{violation}");
}

/// 누락된 배열을 0으로 읽으면 검증이 조용히 통과한다. **없으면 위반이어야 한다.**
#[test]
fn missing_arrays_do_not_silently_pass() {
    let line = r#"{"type":"system","subtype":"init","tools":["Glob","Grep","Read","StructuredOutput"],"permissionMode":"dontAsk","model":"claude-sonnet-5","cwd":"/tmp","session_id":"s","claude_code_version":"2.1.235"}"#;
    let facts = parse_init(line).unwrap();
    assert_eq!(facts.mcp_servers, usize::MAX, "누락은 0이 아니다");
    assert!(validate_init(&facts, Path::new("/tmp"), "s").is_err());
}

/// `system:init`이 아니면 검증 자체를 할 수 없다 — 실행을 계속하면 안 된다.
#[test]
fn non_init_first_event_is_a_violation() {
    let err = parse_init(r#"{"type":"assistant","message":{}}"#).unwrap_err();
    assert!(err.0.contains("not system:init"), "{err}");
}

#[test]
fn non_json_first_event_is_a_violation() {
    let err = parse_init("not json at all").unwrap_err();
    assert!(err.0.contains("not JSON"), "{err}");
}

// ───────────────────────── 결과 파싱 ─────────────────────────

#[test]
fn structured_output_becomes_a_plan() {
    let line = r#"{"type":"result","subtype":"success","is_error":false,"permission_denials":[],"structured_output":{"summary":"greeting 문구 변경","steps":[{"title":"target.txt 수정","detail":"hello를 새 문구로","files":["target.txt"]}]}}"#;
    let plan = parse_result(line).unwrap();
    assert_eq!(plan.summary, "greeting 문구 변경");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].files, vec!["target.txt".to_string()]);
    assert!(!plan.is_error);
}

/// E2 실측처럼 `detail`이 없는 계획도 받아들인다(스키마가 바뀌어도 인계가 끊기지 않게).
#[test]
fn plan_without_detail_is_accepted() {
    let line = r#"{"type":"result","structured_output":{"summary":"s","steps":[{"title":"t","files":["a"]}]}}"#;
    let plan = parse_result(line).unwrap();
    assert_eq!(plan.steps[0].detail, "");
}

/// `structured_output`이 없으면 `result` 텍스트의 JSON을 판다.
#[test]
fn falls_back_to_result_text() {
    let line = r#"{"type":"result","result":"{\"summary\":\"s\",\"steps\":[]}"}"#;
    let plan = parse_result(line).unwrap();
    assert_eq!(plan.summary, "s");
}

#[test]
fn empty_plan_is_an_error() {
    let line = r#"{"type":"result","structured_output":{"summary":"","steps":[]}}"#;
    assert!(parse_result(line).is_err());
}

/// 거부된 도구 호출은 기계 판독 가능한 증거다 — Codex와의 결정적 차이(§7.10.1.1).
#[test]
fn permission_denials_are_captured() {
    let line = r#"{"type":"result","permission_denials":[{"tool_name":"Write","tool_input":{}},{"tool_name":"Bash","tool_input":{}}],"structured_output":{"summary":"s","steps":[]}}"#;
    let plan = parse_result(line).unwrap();
    assert_eq!(plan.permission_denials, vec!["Write", "Bash"]);
}

/// **실행이 왜 끝났는지 CLI가 말해 줬다면 그대로 전한다.**
///
/// 실사용에서 토큰이 만료돼 계획 단계가 죽었는데, 원인을 "계획이 없다"로 덮어써서
/// 화면이 "다시 로그인하세요"라고 안내하지 못했다. 원인을 잃으면 사용자는 막다른 길에 선다.
#[test]
fn a_failure_reported_by_the_cli_is_not_replaced_with_a_generic_message() {
    let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"Failed to authenticate. API Error: 401 OAuth access token has been revoked.","structured_output":null}"#;

    let error = parse_result(line).expect_err("실패인데 계획으로 읽었다");

    assert!(error.contains("OAuth"), "원인이 사라졌다: {error}");
    assert!(error.contains("revoked"), "{error}");
}

/// CLI가 아무 말도 없으면 그때만 기본 문구를 쓴다.
#[test]
fn a_silent_failure_still_gets_a_message() {
    let error = parse_result(r#"{"type":"result","is_error":true}"#).unwrap_err();
    assert!(error.contains("no structured plan"), "{error}");
}

// ───────────────────────── 인계 ─────────────────────────

/// 인계는 **앱이 한다.** 사용자가 옮겨 나르지 않는다(§5.2 가설 A).
#[test]
fn plan_becomes_a_codex_prompt() {
    let plan = PlanResult {
        summary: "인사 문구를 바꾼다".into(),
        steps: vec![PlanStep {
            title: "target.txt 수정".into(),
            detail: "hello를 안녕으로".into(),
            files: vec!["target.txt".into()],
        }],
        permission_denials: vec![],
        is_error: false,
    };
    let prompt = plan_to_codex_prompt("인사말을 한국어로 바꿔줘", &plan).expect("인계가 막혔다");

    assert!(prompt.contains("인사말을 한국어로 바꿔줘"));
    assert!(prompt.contains("target.txt 수정"));
    assert!(prompt.contains("hello를 안녕으로"));
    assert!(prompt.contains("Files: target.txt"));
}

/// 실패로 끝난 계획은 **Codex로 넘어가지 않는다.** 호출부 검사에 맡기지 않고
/// 인계 함수 자체가 막아, 경로가 늘어도 다시 잊을 수 없게 한다.
#[test]
fn errored_plan_is_never_handed_off() {
    let plan = PlanResult {
        summary: "그럴듯한 요약".into(),
        steps: vec![PlanStep {
            title: "무언가".into(),
            detail: String::new(),
            files: vec![],
        }],
        permission_denials: vec![],
        is_error: true,
    };
    let err = plan_to_codex_prompt("목표", &plan).expect_err("실패한 계획을 인계했다");
    assert!(err.contains("error"), "{err}");
}

/// `is_error`는 `result` 이벤트에서 실제로 읽혀야 한다 — 읽지 못하면 위 방어가 죽는다.
#[test]
fn is_error_is_read_from_the_result_event() {
    let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"structured_output":{"summary":"s","steps":[{"title":"t","detail":"","files":[]}]}}"#;
    let plan = parse_result(line).unwrap();
    assert!(plan.is_error);
    assert!(plan_to_codex_prompt("목표", &plan).is_err());
}

/// 단계가 없는 계획도 인계 대상이 아니다 — Codex에게 시킬 일이 없다.
#[test]
fn plan_without_steps_is_not_handed_off() {
    let plan = PlanResult {
        summary: "요약만 있다".into(),
        ..PlanResult::default()
    };
    assert!(plan_to_codex_prompt("목표", &plan).is_err());
}
