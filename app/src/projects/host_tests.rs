use super::*;
use crate::app_state::{
    LeafContents, LeafSnapshot, PaneNodeSnapshot, ProjectScreenSnapshot, TabSnapshot,
    TerminalPaneSnapshot, WindowSnapshot,
};
use crate::features::FeatureFlag;
use crate::tab::SelectedTabColor;

fn tab(uuid_seed: u8) -> TabSnapshot {
    TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: true,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![uuid_seed],
                cwd: None,
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
    }
}

fn three_screen_snapshot() -> WindowSnapshot {
    let alpha = ProjectId::new();
    let beta = ProjectId::new();
    WindowSnapshot {
        screens: vec![
            ProjectScreenSnapshot {
                project_id: None,
                tabs: vec![tab(1)],
                active_tab_index: 0,
                tab_groups: vec![],
            },
            ProjectScreenSnapshot {
                project_id: Some(alpha),
                tabs: vec![tab(2), tab(3)],
                active_tab_index: 1,
                tab_groups: vec![],
            },
            ProjectScreenSnapshot {
                project_id: Some(beta),
                tabs: vec![tab(4)],
                active_tab_index: 0,
                tab_groups: vec![],
            },
        ],
        active_screen_index: 2,
        team_uid: None,
        bounds: None,
        fullscreen_state: Default::default(),
        quake_mode: false,
        universal_search_width: None,
        voltron_width: None,
        warp_drive_index_width: None,
        left_panel_open: false,
        vertical_tabs_panel_open: false,
        left_panel_width: None,
        right_panel_width: None,
    }
}

fn restored_source(window_snapshot: WindowSnapshot) -> NewWorkspaceSource {
    NewWorkspaceSource::Restored {
        window_snapshot,
        screen_index: 0,
        block_lists: Default::default(),
    }
}

fn screen_tabs(setting: &NewWorkspaceSource) -> usize {
    let NewWorkspaceSource::Restored {
        window_snapshot,
        screen_index,
        ..
    } = setting
    else {
        panic!("expected a restored setting");
    };
    window_snapshot.screens[*screen_index].tabs.len()
}

#[test]
fn a_multi_screen_restore_builds_one_setting_per_screen() {
    let _guard = FeatureFlag::AdeWorkspaces.override_enabled(true);
    let snapshot = three_screen_snapshot();
    let expected_active = snapshot.active_screen_index;
    let expected_projects: Vec<Option<ProjectId>> =
        snapshot.screens.iter().map(|s| s.project_id).collect();

    let (settings, active) = ProjectHost::screen_settings(restored_source(snapshot));

    assert_eq!(settings.len(), 3);
    assert_eq!(active, expected_active);
    assert_eq!(
        settings
            .iter()
            .map(|(project_id, _)| *project_id)
            .collect::<Vec<_>>(),
        expected_projects
    );
    assert_eq!(screen_tabs(&settings[0].1), 1);
    assert_eq!(screen_tabs(&settings[1].1), 2);
    assert_eq!(screen_tabs(&settings[2].1), 1);
}

#[test]
fn with_the_flag_off_every_tab_folds_into_home() {
    let _guard = FeatureFlag::AdeWorkspaces.override_enabled(false);
    let (settings, active) = ProjectHost::screen_settings(restored_source(three_screen_snapshot()));

    assert_eq!(settings.len(), 1, "flag off renders a single Home screen");
    assert_eq!(active, 0);
    assert_eq!(settings[0].0, None);
    assert_eq!(
        screen_tabs(&settings[0].1),
        4,
        "no tab may be lost or left unreachable when the flag is off"
    );
}

#[test]
fn a_plain_window_yields_a_single_home_screen() {
    let _guard = FeatureFlag::AdeWorkspaces.override_enabled(true);
    let (settings, active) = ProjectHost::screen_settings(NewWorkspaceSource::Empty {
        previous_active_window: None,
        shell: None,
    });

    assert_eq!(settings.len(), 1);
    assert_eq!(active, 0);
    assert_eq!(settings[0].0, None);
}
