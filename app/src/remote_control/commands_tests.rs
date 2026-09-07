use base64::Engine as _;
use remote_control::protocol::{ApprovalModeWire, ClonePhase, ErrorCode, TerminalSignal};
use serde_json::json;

use super::{approval_mode, clone_phase_name, decode_input, params, sanitize_worktree_name};
use crate::settings::AgentApprovalMode;

#[derive(serde::Deserialize, Debug, PartialEq)]
struct Sample {
    terminal_id: String,
    #[serde(default)]
    text: String,
}

#[test]
fn parameters_parse_into_their_typed_form() {
    let parsed: Sample = params(json!({"terminal_id": "12", "text": "ls"})).expect("valid params");
    assert_eq!(
        parsed,
        Sample {
            terminal_id: "12".to_owned(),
            text: "ls".to_owned()
        }
    );
}

#[test]
fn missing_parameters_are_an_invalid_request() {
    let error = params::<Sample>(json!({})).expect_err("terminal_id is required");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("bad parameters"));
}

#[test]
fn input_is_decoded_from_base64_and_size_capped() {
    let encoded = base64::engine::general_purpose::STANDARD.encode("hello");
    assert_eq!(decode_input(&encoded, 16).expect("decodes"), b"hello");

    let error = decode_input(&encoded, 2).expect_err("too large");
    assert_eq!(error.code, ErrorCode::PayloadTooLarge);

    let error = decode_input("not base64!!", 16).expect_err("not base64");
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn approval_modes_map_onto_the_desktop_enum() {
    assert_eq!(
        approval_mode(ApprovalModeWire::Yolo),
        AgentApprovalMode::Yolo
    );
    assert_eq!(
        approval_mode(ApprovalModeWire::Normal),
        AgentApprovalMode::Normal
    );
}

#[test]
fn worktree_names_keep_only_safe_characters() {
    assert_eq!(sanitize_worktree_name("  feature/auth  "), "feature-auth");
    assert_eq!(sanitize_worktree_name("my_branch.v2"), "my_branch.v2");
    assert_eq!(sanitize_worktree_name("a b c"), "a-b-c");
    assert_eq!(sanitize_worktree_name("--edges--"), "edges");
    assert_eq!(sanitize_worktree_name("   "), "");
    assert_eq!(sanitize_worktree_name("***"), "");
}

#[test]
fn signals_write_the_expected_control_bytes() {
    assert_eq!(TerminalSignal::Interrupt.byte(), 0x03);
    assert_eq!(TerminalSignal::Eof.byte(), 0x04);
}

#[test]
fn clone_phases_have_stable_names() {
    let cases = [
        (ClonePhase::Starting, "starting"),
        (ClonePhase::Cloning, "cloning"),
        (ClonePhase::Registering, "registering"),
        (ClonePhase::Done, "done"),
        (ClonePhase::Failed, "failed"),
        (ClonePhase::Cancelled, "cancelled"),
    ];
    for (phase, name) in cases {
        assert_eq!(clone_phase_name(phase), name);
    }
}
