use serde_json::{Value, json};

use super::{
    AgentCounts, ApiError, AppSnapshot, ClientMessage, CommandError, CommandName, ErrorCode,
    Features, LimitEntry, ServerEvent, ServerInfo, ServerMessage, TerminalMode,
};

fn empty_snapshot() -> AppSnapshot {
    AppSnapshot {
        version: 7,
        instance_id: "inst_1".to_owned(),
        active_window_id: Some("1".to_owned()),
        windows: Vec::new(),
        projects: Vec::new(),
        sessions: Vec::new(),
        agents: Vec::new(),
        server: ServerInfo {
            connected_clients: 1,
            lan_access: false,
        },
        features: Features {
            ade_workspaces: true,
            session_history: false,
        },
    }
}

#[test]
fn hello_serializes_with_the_documented_shape() {
    let hello = ServerMessage::Hello {
        instance_id: "inst_1".to_owned(),
        protocol: crate::PROTOCOL_VERSION,
        app_version: "0.1.0".to_owned(),
        client_id: "c1".to_owned(),
        capabilities: vec!["app.ping".to_owned()],
        limits: vec![LimitEntry {
            name: "max_clients".to_owned(),
            value: 8,
        }],
    };
    assert_eq!(
        serde_json::to_value(&hello).unwrap(),
        json!({
            "type": "hello",
            "instance_id": "inst_1",
            "protocol": 1,
            "app_version": "0.1.0",
            "client_id": "c1",
            "capabilities": ["app.ping"],
            "limits": [{"name": "max_clients", "value": 8}]
        })
    );
}

#[test]
fn state_wraps_the_snapshot() {
    let message = ServerMessage::State {
        version: 7,
        snapshot: Box::new(empty_snapshot()),
    };
    let value = serde_json::to_value(&message).unwrap();
    assert_eq!(value["type"], "state");
    assert_eq!(value["version"], 7);
    assert_eq!(value["snapshot"]["instance_id"], "inst_1");
    assert_eq!(value["snapshot"]["features"]["ade_workspaces"], true);
}

#[test]
fn results_carry_either_data_or_an_error() {
    let ok = ServerMessage::ok_result("1", json!({"instance_id": "inst_1"}));
    assert_eq!(
        serde_json::to_value(&ok).unwrap(),
        json!({"type": "result", "id": "1", "ok": true, "data": {"instance_id": "inst_1"}})
    );

    let failed = ServerMessage::error_result(
        "2",
        CommandError::new(ErrorCode::NotFound, "tab no longer exists"),
    );
    assert_eq!(
        serde_json::to_value(&failed).unwrap(),
        json!({
            "type": "result",
            "id": "2",
            "ok": false,
            "error": {"code": "not_found", "message": "tab no longer exists"}
        })
    );
}

#[test]
fn events_flatten_their_name() {
    let event = ServerMessage::Event {
        event: ServerEvent::TerminalResync {
            attach_id: 3,
            cols: 80,
            rows: 24,
            mode: TerminalMode::Running,
            snapshot: "AAA".to_owned(),
        },
    };
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({
            "type": "event",
            "name": "terminal.resync",
            "attach_id": 3,
            "cols": 80,
            "rows": 24,
            "mode": "running",
            "snapshot": "AAA"
        })
    );

    let shutdown = ServerMessage::Event {
        event: ServerEvent::ServerShuttingDown,
    };
    assert_eq!(
        serde_json::to_value(&shutdown).unwrap(),
        json!({"type": "event", "name": "server.shutting_down"})
    );
}

#[test]
fn client_commands_parse_with_their_dotted_names() {
    let raw = r#"{"type":"command","id":"1","name":"tab.activate","params":{"tab_id":"812"}}"#;
    let parsed: ClientMessage = serde_json::from_str(raw).unwrap();
    assert_eq!(
        parsed,
        ClientMessage::Command {
            id: "1".to_owned(),
            name: CommandName::TabActivate,
            params: json!({"tab_id": "812"}),
        }
    );
}

#[test]
fn ping_round_trips() {
    let parsed: ClientMessage = serde_json::from_str(r#"{"type":"ping","ts":12}"#).unwrap();
    assert_eq!(parsed, ClientMessage::Ping { ts: 12 });
}

#[test]
fn unknown_fields_are_ignored() {
    let raw = r#"{"type":"ping","ts":12,"future_field":true}"#;
    assert_eq!(
        serde_json::from_str::<ClientMessage>(raw).unwrap(),
        ClientMessage::Ping { ts: 12 }
    );
}

#[test]
fn unknown_command_names_are_rejected() {
    let raw = r#"{"type":"command","id":"1","name":"tab.explode","params":{}}"#;
    assert!(serde_json::from_str::<ClientMessage>(raw).is_err());
}

#[test]
fn command_names_round_trip_through_their_wire_form() {
    for name in CommandName::all() {
        let encoded = serde_json::to_value(name).unwrap();
        assert_eq!(encoded, Value::String(name.as_str().to_owned()));
        assert_eq!(
            serde_json::from_value::<CommandName>(encoded).unwrap(),
            *name
        );
    }
}

#[test]
fn command_name_table_has_no_duplicates() {
    let mut names: Vec<&str> = CommandName::all().iter().map(|n| n.as_str()).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total);
}

#[test]
fn error_codes_map_to_their_http_statuses() {
    let table = [
        (ErrorCode::Unauthorized, 401),
        (ErrorCode::ForbiddenOrigin, 403),
        (ErrorCode::BadHost, 421),
        (ErrorCode::FeatureDisabled, 403),
        (ErrorCode::InvalidRequest, 400),
        (ErrorCode::UnknownCommand, 400),
        (ErrorCode::NotFound, 404),
        (ErrorCode::NotAtPrompt, 409),
        (ErrorCode::NoAgentSession, 409),
        (ErrorCode::TerminalReadOnly, 409),
        (ErrorCode::Conflict, 409),
        (ErrorCode::NotGitProject, 409),
        (ErrorCode::GitFailed, 500),
        (ErrorCode::PayloadTooLarge, 413),
        (ErrorCode::TooManyAttachments, 429),
        (ErrorCode::RateLimited, 429),
        (ErrorCode::BridgeUnavailable, 503),
        (ErrorCode::Unsupported, 501),
    ];
    for (code, status) in table {
        assert_eq!(code.http_status(), status);
    }
}

#[test]
fn api_errors_serialize_under_an_error_key() {
    assert_eq!(
        serde_json::to_value(ApiError::new(ErrorCode::Unauthorized, "no session")).unwrap(),
        json!({"error": {"code": "unauthorized", "message": "no session"}})
    );
}

#[test]
fn command_error_details_are_optional() {
    let with_details = CommandError::new(ErrorCode::GitFailed, "git failed")
        .with_details(json!({"stderr": "fatal"}));
    assert_eq!(
        serde_json::to_value(&with_details).unwrap(),
        json!({"code": "git_failed", "message": "git failed", "details": {"stderr": "fatal"}})
    );
}

#[test]
fn agent_counts_default_to_zero() {
    assert_eq!(
        AgentCounts::default(),
        AgentCounts {
            working: 0,
            needs_attention: 0
        }
    );
}
