use remote_control::protocol::{AppSnapshot, Features, ServerInfo};

use super::{ClientId, snapshots_match};

fn snapshot(version: u64, clients: usize) -> AppSnapshot {
    AppSnapshot {
        version,
        instance_id: "inst_1".to_owned(),
        active_window_id: Some("1".to_owned()),
        windows: Vec::new(),
        projects: Vec::new(),
        sessions: Vec::new(),
        agents: Vec::new(),
        server: ServerInfo {
            connected_clients: clients,
            lan_access: false,
        },
        features: Features {
            ade_workspaces: true,
            session_history: false,
        },
    }
}

#[test]
fn client_ids_render_with_a_stable_prefix() {
    assert_eq!(ClientId(1).as_string(), "c1");
    assert_eq!(ClientId(42).as_string(), "c42");
    assert_ne!(ClientId(1).as_string(), ClientId(2).as_string());
}

#[test]
fn the_version_counter_is_ignored_when_diffing() {
    assert!(snapshots_match(&snapshot(1, 1), &snapshot(9, 1)));
}

#[test]
fn a_changed_field_makes_the_snapshots_differ() {
    assert!(!snapshots_match(&snapshot(1, 1), &snapshot(1, 2)));

    let mut other = snapshot(1, 1);
    other.instance_id = "inst_2".to_owned();
    assert!(!snapshots_match(&snapshot(1, 1), &other));

    let mut other = snapshot(1, 1);
    other.active_window_id = None;
    assert!(!snapshots_match(&snapshot(1, 1), &other));

    let mut other = snapshot(1, 1);
    other.features.session_history = true;
    assert!(!snapshots_match(&snapshot(1, 1), &other));
}

#[test]
fn identical_snapshots_match() {
    assert!(snapshots_match(&snapshot(3, 4), &snapshot(3, 4)));
}
