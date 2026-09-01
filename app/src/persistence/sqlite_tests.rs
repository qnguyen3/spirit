use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use diesel::connection::SimpleConnection;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::Vector2F;

use super::{
    app_database_file_path, database_file_path_for_current_scope, database_file_path_for_scope,
    decode_path, deduplicate_events, encode_path, get_all_codebase_index_metadata,
    read_sqlite_data, save_app_state, save_codebase_index_metadata, save_project, setup_database,
    start_writer,
};
use crate::app_state::{
    AppState, CodePaneSnapShot, CodePaneTabSnapshot, LeafContents, LeafSnapshot, PaneNodeSnapshot,
    ProjectScreenSnapshot, TabGroupSnapshot, TabSnapshot, TerminalPaneSnapshot, WindowSnapshot,
};
use crate::code::editor_management::CodeSource;
use crate::persistence::model::{Project as ProjectRow, ProjectWorktree as WorktreeRow};
use crate::persistence::{BlockCompleted, ModelEvent, PersistedDataScope, PersistenceScope};
use crate::projects::{Project, ProjectId, ProjectKind, Worktree, WorktreeId, WorktreeKind};
use crate::tab::SelectedTabColor;
use crate::terminal::ShellLaunchData;
use crate::terminal::model::block::SerializedBlock;
use crate::themes::theme::AnsiColorIdentifier;
use crate::workspace::tab_group::TabGroupId;
use crate::workspace_metadata::WorkspaceMetadata;

#[test]
fn app_scope_database_path_matches_app_database_path() {
    assert_eq!(
        database_file_path_for_scope(&PersistenceScope::App),
        app_database_file_path()
    );
}

#[test]
fn database_path_for_current_scope_defaults_to_app_scope() {
    // Unit tests never call `persistence::initialize`, so the process-wide
    // scope defaults to `App` and ad-hoc read-only connections resolve to
    // the GUI database. (nextest runs each test in its own process, so no
    // other test can have set the scope.)
    assert_eq!(
        database_file_path_for_current_scope(),
        app_database_file_path()
    );
}

#[test]
fn remote_server_daemon_scope_database_path_uses_identity_data_dir() {
    let path = database_file_path_for_scope(&PersistenceScope::RemoteServerDaemon {
        identity_key: "user@example.com/ssh host".to_string(),
    });
    let expected_data_dir =
        remote_server::setup::remote_server_daemon_data_dir("user@example.com/ssh host");

    assert!(path.is_absolute());
    assert_eq!(
        path,
        PathBuf::from(shellexpand::tilde(&expected_data_dir).into_owned()).join("warp.sqlite")
    );
}

#[test]
fn remote_server_daemon_scope_database_path_handles_empty_identity_key() {
    let path = database_file_path_for_scope(&PersistenceScope::RemoteServerDaemon {
        identity_key: String::new(),
    });
    let expected_data_dir = remote_server::setup::remote_server_daemon_data_dir("");

    assert_eq!(
        path,
        PathBuf::from(shellexpand::tilde(&expected_data_dir).into_owned()).join("warp.sqlite")
    );
}

#[cfg(unix)]
#[test]
fn remote_server_daemon_database_permissions_are_owner_only() {
    use std::fs::Permissions;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let daemon_dir = tempdir.path().join("daemon");
    let database_path = daemon_dir.join("warp.sqlite");

    std::fs::create_dir_all(&daemon_dir).expect("daemon dir should be created");
    std::fs::set_permissions(&daemon_dir, Permissions::from_mode(0o755))
        .expect("daemon dir permissions should be set");
    std::fs::write(&database_path, b"").expect("database file should be created");
    std::fs::set_permissions(&database_path, Permissions::from_mode(0o644))
        .expect("database file permissions should be set");

    super::ensure_owner_only_dir(&daemon_dir).expect("daemon dir should be owner-only");
    super::ensure_owner_only_file(&database_path).expect("database file should be owner-only");

    assert_eq!(daemon_dir.metadata().unwrap().mode() & 0o777, 0o700);
    assert_eq!(database_path.metadata().unwrap().mode() & 0o777, 0o600);
}

fn test_codebase_metadata(path: &str) -> WorkspaceMetadata {
    WorkspaceMetadata {
        path: PathBuf::from(path),
        navigated_ts: Some(Utc::now()),
        modified_ts: None,
        queried_ts: None,
    }
}

#[test]
fn sqlite_read_restores_app_state_and_codebase_metadata() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![test_terminal_window_snapshot(false)],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");

    let metadata = test_codebase_metadata("/tmp/remote-repo");
    save_codebase_index_metadata(&mut conn, metadata.clone())
        .expect("codebase index metadata should save");
    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("persisted data should load");
    let restored_app_state = restored
        .app_state
        .expect("app state should be present for the full scope");
    assert_eq!(restored_app_state.windows.len(), 1);
    assert_eq!(restored.codebase_indices.len(), 1);
    assert_eq!(restored.codebase_indices[0].path, metadata.path);
}

#[test]
fn sqlite_writer_reuses_codebase_index_metadata_events() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let conn = setup_database(&database_path).expect("database should initialize");

    let writer = start_writer(conn, database_path.clone()).expect("writer should start");
    let metadata = test_codebase_metadata("/tmp/writer-repo");
    writer
        .sender
        .send(ModelEvent::UpsertCodebaseIndexMetadata {
            index_metadata: Box::new(metadata.clone()),
        })
        .expect("upsert event should send");
    writer
        .sender
        .send(ModelEvent::Terminate)
        .expect("terminate event should send");
    writer.handle.join().expect("writer should terminate");

    let mut conn = setup_database(&database_path).expect("database should reopen");
    let restored = get_all_codebase_index_metadata(&mut conn).expect("metadata should load");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].path, metadata.path);

    let writer = start_writer(conn, database_path.clone()).expect("writer should restart");
    writer
        .sender
        .send(ModelEvent::DeleteCodebaseIndexMetadata {
            repo_path: metadata.path,
        })
        .expect("delete event should send");
    writer
        .sender
        .send(ModelEvent::Terminate)
        .expect("terminate event should send");
    writer.handle.join().expect("writer should terminate");

    let mut conn = setup_database(&database_path).expect("database should reopen");
    let restored = get_all_codebase_index_metadata(&mut conn).expect("metadata should load");
    assert!(restored.is_empty());
}
#[test]
fn test_deduplicate_snapshots() {
    let completed_block_1 = BlockCompleted {
        pane_id: vec![1, 2, 3],
        block: Arc::new(SerializedBlock::default()),
        is_local: true,
    };
    let completed_block_2 = BlockCompleted {
        pane_id: vec![4, 5, 6],
        block: Arc::new(SerializedBlock::default()),
        is_local: true,
    };
    let snapshot_1 = AppState {
        active_window_index: Some(1),
        block_lists: Default::default(),
        windows: Default::default(),
    };
    let snapshot_2 = AppState {
        active_window_index: Some(2),
        block_lists: Default::default(),
        windows: Default::default(),
    };
    let snapshot_3 = AppState {
        active_window_index: Some(3),
        block_lists: Default::default(),
        windows: Default::default(),
    };

    let original_events = vec![
        ModelEvent::Snapshot(snapshot_1.clone()),
        ModelEvent::SaveBlock(completed_block_1.clone()),
        ModelEvent::Snapshot(snapshot_2.clone()),
        ModelEvent::SaveBlock(completed_block_2.clone()),
        ModelEvent::Snapshot(snapshot_3.clone()),
    ];

    let filtered_events = deduplicate_events(original_events);
    assert_eq!(filtered_events.len(), 3);

    // The first snapshot should have been filtered out.
    assert!(matches!(&filtered_events[0], &ModelEvent::SaveBlock(_)));
    // The second snapshot should have been filtered out.
    assert!(matches!(&filtered_events[1], &ModelEvent::SaveBlock(_)));
    // The third snapshot should be preserved.
    match &filtered_events[2] {
        ModelEvent::Snapshot(snapshot) => assert_eq!(snapshot, &snapshot_3),
        other => panic!("Expected ModelEvent::Snapshot, got {other:?}"),
    }
}

#[test]
fn test_deduplicate_no_snapshots() {
    let original_events = vec![ModelEvent::SaveBlock(BlockCompleted {
        pane_id: vec![1, 2, 3],
        block: Default::default(),
        is_local: true,
    })];
    let filtered_events = deduplicate_events(original_events);
    assert_eq!(filtered_events.len(), 1);
    assert!(matches!(&filtered_events[0], &ModelEvent::SaveBlock(_)));
}

/// Decision D4: window snapshots written by older builds can contain panes for AI features that
/// no longer exist. Restoring one must not abort, and must not drop the surrounding tab -- each
/// removed pane kind decodes into a fresh empty terminal pane so the tab layout survives the
/// downgrade. Simulates the legacy rows by rewriting a saved terminal pane's kind, since the
/// current writer can no longer produce them.
#[test]
fn legacy_ai_panes_restore_as_terminal_panes() {
    for legacy_kind in [
        "ai_memory",
        "mcp_server",
        "ai_document",
        "ambient_agent",
        "execution_profile_editor",
        "workflow",
        "env_var_collection",
    ] {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let database_path = tempdir.path().join("warp.sqlite");
        let mut conn = setup_database(&database_path).expect("database should initialize");

        let app_state = AppState {
            windows: vec![test_terminal_window_snapshot(false)],
            active_window_index: Some(0),
            block_lists: Default::default(),
        };
        save_app_state(&mut conn, &app_state).expect("app state should save");

        // `terminal_panes` is keyed on (id, kind) and CHECKs kind = 'terminal', so the child
        // row goes before the leaf can take a legacy kind.
        conn.batch_execute(&format!(
            "DELETE FROM terminal_panes; \
             UPDATE pane_leaves SET kind = '{legacy_kind}' WHERE kind = 'terminal';"
        ))
        .expect("legacy pane kind should be written");

        let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
            .unwrap_or_else(|err| panic!("restore must not fail for {legacy_kind}: {err}"));
        let restored_app_state = restored
            .app_state
            .unwrap_or_else(|| panic!("app state should be present for {legacy_kind}"));

        assert_eq!(
            restored_app_state.windows.len(),
            1,
            "the window must survive a {legacy_kind} pane"
        );
        let tabs = &restored_app_state.windows[0].tabs();
        assert_eq!(tabs.len(), 1, "the tab must survive a {legacy_kind} pane");

        let PaneNodeSnapshot::Leaf(leaf) = &tabs[0].root else {
            panic!("restored root should be a leaf for {legacy_kind}");
        };
        let LeafContents::Terminal(terminal) = &leaf.contents else {
            panic!("a {legacy_kind} pane should restore as a terminal pane");
        };
        assert!(
            terminal.cwd.is_none() && terminal.shell_launch_data.is_none(),
            "a {legacy_kind} pane should restore as an empty terminal, not inherit stale state"
        );
        assert!(
            !terminal.uuid.is_empty(),
            "the replacement terminal pane needs its own uuid for {legacy_kind}"
        );
    }
}

fn test_terminal_window_snapshot(vertical_tabs_panel_open: bool) -> WindowSnapshot {
    WindowSnapshot {
        screens: vec![ProjectScreenSnapshot {
            project_id: None,
            active_tab_index: 0,
            tab_groups: vec![],
            tabs: vec![TabSnapshot {
                custom_title: None,
                root: PaneNodeSnapshot::Leaf(LeafSnapshot {
                    is_focused: true,
                    custom_vertical_tabs_title: None,
                    contents: LeafContents::Terminal(TerminalPaneSnapshot {
                        uuid: vec![u8::from(vertical_tabs_panel_open) + 1],
                        cwd: Some("/tmp".to_string()),
                        shell_launch_data: Some(ShellLaunchData::Executable {
                            executable_path: PathBuf::from("/bin/zsh"),
                            shell_type: crate::terminal::shell::ShellType::Zsh,
                        }),
                        is_active: true,
                        is_read_only: false,
                    }),
                }),
                default_directory_color: None,
                selected_color: SelectedTabColor::default(),
                left_panel: None,
                right_panel: None,
                group_id: None,
                pinned: false,
                worktree_id: None,
            }],
        }],
        active_screen_index: 0,
        bounds: None,
        fullscreen_state: Default::default(),
        quake_mode: false,
        universal_search_width: None,
        voltron_width: None,
        left_panel_open: false,
        vertical_tabs_panel_open,
        left_panel_width: None,
        right_panel_width: None,
    }
}

#[test]
fn test_sqlite_round_trips_vertical_tabs_panel_open() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![
            test_terminal_window_snapshot(false),
            test_terminal_window_snapshot(true),
        ],
        active_window_index: Some(1),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the full scope");

    assert_eq!(restored.active_window_index, Some(1));
    assert_eq!(
        restored
            .windows
            .iter()
            .map(|window| window.vertical_tabs_panel_open)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn test_sqlite_round_trips_custom_vertical_tabs_title() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            screens: vec![ProjectScreenSnapshot {
                project_id: None,
                tabs: vec![TabSnapshot {
                    custom_title: None,
                    root: PaneNodeSnapshot::Leaf(LeafSnapshot {
                        is_focused: true,
                        custom_vertical_tabs_title: Some("Production API".to_string()),
                        contents: LeafContents::Terminal(TerminalPaneSnapshot {
                            uuid: vec![42],
                            cwd: Some("/tmp".to_string()),
                            shell_launch_data: Some(ShellLaunchData::Executable {
                                executable_path: PathBuf::from("/bin/zsh"),
                                shell_type: crate::terminal::shell::ShellType::Zsh,
                            }),
                            is_active: true,
                            is_read_only: false,
                        }),
                    }),
                    default_directory_color: None,
                    selected_color: SelectedTabColor::default(),
                    left_panel: None,
                    right_panel: None,
                    group_id: None,
                    pinned: false,
                    worktree_id: None,
                }],
                active_tab_index: 0,
                tab_groups: vec![],
            }],
            active_screen_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the full scope");

    let PaneNodeSnapshot::Leaf(LeafSnapshot {
        custom_vertical_tabs_title,
        ..
    }) = &restored.windows[0].tabs()[0].root
    else {
        panic!("Expected terminal pane leaf");
    };
    assert_eq!(
        custom_vertical_tabs_title.as_deref(),
        Some("Production API")
    );
}

#[test]
fn test_sqlite_round_trips_code_pane_with_multiple_tabs() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            screens: vec![ProjectScreenSnapshot {
                project_id: None,
                tabs: vec![TabSnapshot {
                    custom_title: None,
                    root: PaneNodeSnapshot::Leaf(LeafSnapshot {
                        is_focused: true,
                        custom_vertical_tabs_title: None,
                        contents: LeafContents::Code(CodePaneSnapShot::Local {
                            tabs: vec![
                                CodePaneTabSnapshot {
                                    path: Some(PathBuf::from("/tmp/main.rs")),
                                },
                                CodePaneTabSnapshot {
                                    path: Some(PathBuf::from("/tmp/lib.rs")),
                                },
                                CodePaneTabSnapshot { path: None },
                            ],
                            active_tab_index: 1,
                            source: Some(CodeSource::FileTree {
                                location: crate::code::buffer_location::LocalOrRemotePath::Local(
                                    PathBuf::from("/tmp/main.rs"),
                                ),
                            }),
                        }),
                    }),
                    default_directory_color: None,
                    selected_color: SelectedTabColor::default(),
                    left_panel: None,
                    right_panel: None,
                    group_id: None,
                    pinned: false,
                    worktree_id: None,
                }],
                active_tab_index: 0,
                tab_groups: vec![],
            }],
            active_screen_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the full scope");

    assert_eq!(restored.windows.len(), 1);
    let restored_tab = &restored.windows[0].tabs()[0];
    let PaneNodeSnapshot::Leaf(LeafSnapshot {
        contents:
            LeafContents::Code(CodePaneSnapShot::Local {
                tabs,
                active_tab_index,
                source,
            }),
        ..
    }) = &restored_tab.root
    else {
        panic!("Expected code pane leaf");
    };

    assert_eq!(tabs.len(), 3);
    assert_eq!(*active_tab_index, 1);
    assert_eq!(tabs[0].path, Some(PathBuf::from("/tmp/main.rs")));
    assert_eq!(tabs[1].path, Some(PathBuf::from("/tmp/lib.rs")));
    assert_eq!(tabs[2].path, None);
    assert!(matches!(source, Some(CodeSource::FileTree { .. })));
}

/// Verifies that a tab group and its membership round-trip through save/restore.
#[test]
fn test_sqlite_round_trips_tab_groups() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let group_id = TabGroupId::new();
    let tab_in_group = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: true,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![1],
                cwd: Some("/tmp/grouped".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: true,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: Some(group_id),
        pinned: false,
        worktree_id: None,
    };
    let tab_outside_group = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: false,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![2],
                cwd: Some("/tmp/ungrouped".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: false,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: None,
        pinned: false,
        worktree_id: None,
    };

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            screens: vec![ProjectScreenSnapshot {
                project_id: None,
                tabs: vec![tab_in_group, tab_outside_group],
                active_tab_index: 0,
                tab_groups: vec![TabGroupSnapshot {
                    id: group_id,
                    name: Some("Backend".to_string()),
                    color: SelectedTabColor::Color(AnsiColorIdentifier::Blue),
                    collapsed: true,
                    pinned: false,
                }],
            }],
            active_screen_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the full scope");

    assert_eq!(restored.windows.len(), 1);
    let restored_window = &restored.windows[0];
    assert_eq!(restored_window.tab_groups().len(), 1);
    let restored_group = &restored_window.tab_groups()[0];
    assert_eq!(restored_group.name.as_deref(), Some("Backend"));
    assert_eq!(
        restored_group.color,
        SelectedTabColor::Color(AnsiColorIdentifier::Blue)
    );
    assert!(restored_group.collapsed);

    // The in-memory `TabGroupId` is minted fresh on restore, so we check that
    // the grouped tab points at the restored group, and the ungrouped tab
    // remains ungrouped.
    assert_eq!(restored_window.tabs().len(), 2);
    assert_eq!(restored_window.tabs()[0].group_id, Some(restored_group.id));
    assert_eq!(restored_window.tabs()[1].group_id, None);
}

/// Verifies that the `pinned` flag on tabs and tab groups round-trips through
/// save/restore so the user's pinned layout survives an app restart.
#[test]
fn test_sqlite_round_trips_pinned_state() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let pinned_group_id = TabGroupId::new();
    let unpinned_group_id = TabGroupId::new();

    let pinned_tab = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: true,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![10],
                cwd: Some("/tmp/pinned".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: true,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: None,
        pinned: true,
        worktree_id: None,
    };
    let unpinned_tab = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: false,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![11],
                cwd: Some("/tmp/unpinned".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: false,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: Some(unpinned_group_id),
        pinned: false,
        worktree_id: None,
    };
    let tab_in_pinned_group = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: false,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![12],
                cwd: Some("/tmp/pinned-group".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: false,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: Some(pinned_group_id),
        pinned: false,
        worktree_id: None,
    };

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            screens: vec![ProjectScreenSnapshot {
                project_id: None,
                tabs: vec![pinned_tab, tab_in_pinned_group, unpinned_tab],
                active_tab_index: 0,
                tab_groups: vec![
                    TabGroupSnapshot {
                        id: pinned_group_id,
                        name: Some("Pinned".to_string()),
                        color: SelectedTabColor::default(),
                        collapsed: false,
                        pinned: true,
                    },
                    TabGroupSnapshot {
                        id: unpinned_group_id,
                        name: Some("Loose".to_string()),
                        color: SelectedTabColor::default(),
                        collapsed: false,
                        pinned: false,
                    },
                ],
            }],
            active_screen_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the full scope");

    assert_eq!(restored.windows.len(), 1);
    let restored_window = &restored.windows[0];

    // Tabs come back in insertion order; pinned flag should match what we saved.
    assert_eq!(restored_window.tabs().len(), 3);
    assert!(restored_window.tabs()[0].pinned);
    assert!(!restored_window.tabs()[1].pinned);
    assert!(!restored_window.tabs()[2].pinned);

    // Both groups round-trip with their pinned state preserved. Group ids are
    // minted fresh on restore, so we look them up by name.
    assert_eq!(restored_window.tab_groups().len(), 2);
    let restored_pinned_group = restored_window
        .tab_groups()
        .iter()
        .find(|group| group.name.as_deref() == Some("Pinned"))
        .expect("pinned group should restore");
    let restored_loose_group = restored_window
        .tab_groups()
        .iter()
        .find(|group| group.name.as_deref() == Some("Loose"))
        .expect("unpinned group should restore");
    assert!(restored_pinned_group.pinned);
    assert!(!restored_loose_group.pinned);
}

fn assert_encode_then_decode_preserves_original_path(original_path: PathBuf) {
    let bytes = encode_path(original_path.clone());
    let decoded_path = decode_path(bytes);
    assert_eq!(original_path, decoded_path);
}

/// Test that a local path can be encoded and decoded. We use this when persisting a local
/// file path for notebooks in sqlite. We need this test because Windows `OsString`s are
/// often arbitrary sequences of 16-bit values, unlike Unix which uses sequences of 8-bit
/// values (bytes). Since `diesel::sql_types::Binary` deals with sequences of bytes (`u8`)
/// we need to perform special casting on `OsString`s on Windows.
#[test]
fn test_path_encode_decode() {
    // Empty path
    assert_encode_then_decode_preserves_original_path(PathBuf::new());

    // Windows-style paths
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"C:\windows\system32.dll"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("c:temp"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\emoji\🙈.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\ñoñàscii\temp.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\hindi\हिन्दी"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\cjk\狗没有耐心"));

    // Unix-style paths
    assert_encode_then_decode_preserves_original_path(PathBuf::from(
        "/home/persistence/example.sql",
    ));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("./database/log.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/emoji/🙈.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/ñoñàscii/temp.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/hindi/हिन्दी"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/cjk/狗没有耐心"));
}

// Regression: GH#10083. The macOS green-tile button could leave a 1px-wide
// window bound in `AppContext::window_bounds`, which previously round-tripped
// through SQLite and restored as an unusable 1px sliver. Bounds below the
// platform minimum window size must be dropped on save.
#[test]
fn test_sqlite_drops_too_small_bounds_on_save() {
    use diesel::prelude::*;

    use crate::persistence::schema::windows;

    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let mut snapshot = test_terminal_window_snapshot(false);
    snapshot.bounds = Some(RectF::new(
        Vector2F::new(0.0, -1410.0),
        Vector2F::new(1.0, 1410.0),
    ));

    let app_state = AppState {
        windows: vec![snapshot],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    // Query the row directly so the assertion isolates the save guard and is
    // not masked by the read-side guard in `read_sqlite_data`.
    let row: (Option<f32>, Option<f32>, Option<f32>, Option<f32>) = windows::dsl::windows
        .select((
            windows::columns::window_width,
            windows::columns::window_height,
            windows::columns::origin_x,
            windows::columns::origin_y,
        ))
        .first(&mut conn)
        .expect("a windows row should have been inserted");

    assert_eq!(
        row,
        (None, None, None, None),
        "save-path guard must persist NULL bound columns for sub-minimum geometry"
    );
}

// Regression: GH#10083. Users whose warp.sqlite already contains a 1px row
// (because they hit the bug on an earlier build) must still recover to default
// geometry on next launch rather than restoring the sliver.
#[test]
fn test_sqlite_drops_too_small_bounds_on_read() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    // Save with no bounds so a row exists, then corrupt it directly to bypass
    // the save-path guard and simulate a pre-existing bad row.
    let app_state = AppState {
        windows: vec![test_terminal_window_snapshot(false)],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");

    conn.batch_execute(
        "UPDATE windows \
         SET window_width = 1.0, window_height = 1410.0, \
             origin_x = 0.0, origin_y = -1410.0",
    )
    .expect("corrupting update should succeed");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the full scope");

    assert_eq!(restored.windows.len(), 1);
    assert!(
        restored.windows[0].bounds.is_none(),
        "tiny persisted bounds must be discarded on read so users recover from a corrupt DB"
    );
}

#[test]
fn projects_and_worktrees_round_trip_through_sqlite() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let conn = setup_database(&database_path).expect("database should initialize");

    let project = Project {
        id: ProjectId::new(),
        root_path: PathBuf::from("/tmp/spirit-round-trip"),
        display_name: "spirit".to_owned(),
        kind: ProjectKind::Git,
        primary_branch: Some("main".to_owned()),
        created_ts: 100,
        last_opened_ts: 200,
    };
    let primary = Worktree {
        id: WorktreeId::new(),
        project_id: project.id,
        name: "spirit".to_owned(),
        kind: WorktreeKind::Primary,
        created_ts: 100,
    };
    let linked = Worktree {
        id: WorktreeId::new(),
        project_id: project.id,
        name: "auth".to_owned(),
        kind: WorktreeKind::Linked {
            path: PathBuf::from("/tmp/spirit-round-trip-wt/auth"),
            branch: "auth".to_owned(),
            base_branch: "main".to_owned(),
        },
        created_ts: 300,
    };

    let writer = start_writer(conn, database_path.clone()).expect("writer should start");
    for event in [
        ModelEvent::UpsertProject {
            project: ProjectRow::from(&project),
        },
        ModelEvent::UpsertWorktree {
            worktree: WorktreeRow::from(&primary),
        },
        ModelEvent::UpsertWorktree {
            worktree: WorktreeRow::from(&linked),
        },
        ModelEvent::Terminate,
    ] {
        writer.sender.send(event).expect("event should send");
    }
    writer.handle.join().expect("writer should terminate");

    let mut conn = setup_database(&database_path).expect("database should reopen");
    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("persisted data should load");

    assert_eq!(restored.projects.len(), 1);
    assert_eq!(restored.projects[0].id, project.id);
    assert_eq!(restored.projects[0].root_path, project.root_path);
    assert_eq!(restored.projects[0].kind, ProjectKind::Git);
    assert_eq!(restored.projects[0].primary_branch.as_deref(), Some("main"));
    assert_eq!(restored.worktrees.len(), 2);
    assert!(
        restored
            .worktrees
            .iter()
            .any(|worktree| worktree.is_primary() && worktree.id == primary.id)
    );
    let restored_linked = restored
        .worktrees
        .iter()
        .find(|worktree| worktree.id == linked.id)
        .expect("linked worktree should restore");
    assert_eq!(restored_linked.branch(), Some("auth"));
    assert_eq!(
        restored_linked.directory(&restored.projects[0]),
        std::path::Path::new("/tmp/spirit-round-trip-wt/auth")
    );
}

#[test]
fn removing_a_project_cascades_its_worktree_rows() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let conn = setup_database(&database_path).expect("database should initialize");

    let project = Project {
        id: ProjectId::new(),
        root_path: PathBuf::from("/tmp/spirit-cascade"),
        display_name: "spirit".to_owned(),
        kind: ProjectKind::Folder,
        primary_branch: None,
        created_ts: 1,
        last_opened_ts: 1,
    };
    let primary = Worktree {
        id: WorktreeId::new(),
        project_id: project.id,
        name: "spirit".to_owned(),
        kind: WorktreeKind::Primary,
        created_ts: 1,
    };

    let writer = start_writer(conn, database_path.clone()).expect("writer should start");
    for event in [
        ModelEvent::UpsertProject {
            project: ProjectRow::from(&project),
        },
        ModelEvent::UpsertWorktree {
            worktree: WorktreeRow::from(&primary),
        },
        ModelEvent::RemoveProject {
            project_id: project.id.to_string(),
        },
        ModelEvent::Terminate,
    ] {
        writer.sender.send(event).expect("event should send");
    }
    writer.handle.join().expect("writer should terminate");

    let mut conn = setup_database(&database_path).expect("database should reopen");
    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("persisted data should load");

    assert!(restored.projects.is_empty());
    assert!(restored.worktrees.is_empty());
}

fn test_screen_snapshot(
    project_id: Option<ProjectId>,
    uuid_seed: u8,
    active_tab_index: usize,
) -> ProjectScreenSnapshot {
    ProjectScreenSnapshot {
        project_id,
        tabs: vec![TabSnapshot {
            custom_title: None,
            root: PaneNodeSnapshot::Leaf(LeafSnapshot {
                is_focused: true,
                custom_vertical_tabs_title: None,
                contents: LeafContents::Terminal(TerminalPaneSnapshot {
                    uuid: vec![uuid_seed],
                    cwd: Some("/tmp".to_string()),
                    shell_launch_data: None,
                    is_active: true,
                    is_read_only: false,
                }),
            }),
            default_directory_color: None,
            selected_color: SelectedTabColor::default(),
            left_panel: None,
            right_panel: None,
            group_id: None,
            pinned: false,
            worktree_id: None,
        }],
        active_tab_index,
        tab_groups: vec![],
    }
}

fn test_multi_screen_window(screens: Vec<ProjectScreenSnapshot>, active: usize) -> WindowSnapshot {
    WindowSnapshot {
        screens,
        active_screen_index: active,
        bounds: None,
        fullscreen_state: Default::default(),
        quake_mode: false,
        universal_search_width: None,
        voltron_width: None,
        left_panel_open: false,
        vertical_tabs_panel_open: false,
        left_panel_width: None,
        right_panel_width: None,
    }
}

fn persist_test_project(
    conn: &mut diesel::SqliteConnection,
    root: &str,
    last_opened_ts: i64,
) -> ProjectId {
    let project = Project {
        id: ProjectId::new(),
        root_path: PathBuf::from(root),
        display_name: root.to_owned(),
        kind: ProjectKind::Git,
        primary_branch: Some("main".to_owned()),
        created_ts: 1,
        last_opened_ts,
    };
    save_project(conn, ProjectRow::from(&project)).expect("project should save");
    project.id
}

#[test]
fn screens_round_trip_grouped_by_project() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let older = persist_test_project(&mut conn, "/tmp/spirit-older", 10);
    let newer = persist_test_project(&mut conn, "/tmp/spirit-newer", 20);

    let app_state = AppState {
        windows: vec![test_multi_screen_window(
            vec![
                test_screen_snapshot(None, 1, 0),
                test_screen_snapshot(Some(older), 2, 0),
                test_screen_snapshot(Some(newer), 3, 0),
            ],
            2,
        )],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("persisted data should load")
        .app_state
        .expect("app state should be present");

    let window = &restored.windows[0];
    assert_eq!(window.screens.len(), 3);
    assert_eq!(window.screens[0].project_id, None, "Home restores first");
    assert_eq!(
        window.screens[1].project_id,
        Some(newer),
        "project screens restore MRU-first"
    );
    assert_eq!(window.screens[2].project_id, Some(older));
    assert_eq!(
        window.active_screen_index, 1,
        "the active screen follows windows.active_project_id"
    );
    for screen in &window.screens {
        assert_eq!(screen.tabs.len(), 1);
    }
}

#[test]
fn tabs_of_an_unknown_project_fold_into_home() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let forgotten = ProjectId::new();
    let app_state = AppState {
        windows: vec![test_multi_screen_window(
            vec![
                test_screen_snapshot(None, 1, 0),
                test_screen_snapshot(Some(forgotten), 2, 0),
            ],
            1,
        )],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("persisted data should load")
        .app_state
        .expect("app state should be present");

    let window = &restored.windows[0];
    assert_eq!(window.screens.len(), 1, "no screen for a forgotten project");
    assert_eq!(window.screens[0].project_id, None);
    assert_eq!(
        window.screens[0].tabs.len(),
        2,
        "both tabs survive, folded into Home"
    );
    assert_eq!(window.active_screen_index, 0);
}

#[test]
fn a_legacy_window_restores_into_a_single_home_screen() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![test_terminal_window_snapshot(false)],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn, None, PersistedDataScope::Full)
        .expect("persisted data should load")
        .app_state
        .expect("app state should be present");

    let window = &restored.windows[0];
    assert_eq!(window.screens.len(), 1);
    assert_eq!(window.screens[0].project_id, None);
    assert_eq!(window.active_screen_index, 0);
    assert_eq!(window.tabs().len(), 1);
}
