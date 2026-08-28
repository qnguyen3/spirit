use super::*;

fn screen() -> EntityId {
    EntityId::new()
}

#[test]
fn syncing_every_tab_of_one_screen_does_not_reach_another_screen() {
    let mut state = SyncedInputState::new();
    let workspace_screen = screen();
    let home_screen = screen();
    let workspace_tab = EntityId::new();
    let home_tab = EntityId::new();

    state.toggle_sync_terminal_inputs_in_tab(
        workspace_tab,
        [workspace_tab].into_iter(),
        1,
        workspace_screen,
    );

    assert!(
        state.should_sync_this_pane_group(workspace_tab, workspace_screen),
        "the screen that enabled sync must sync its own tab"
    );
    assert!(
        !state.should_sync_this_pane_group(home_tab, home_screen),
        "keystrokes must not reach a tab on another Workspace screen"
    );
    assert!(!state.is_syncing_any_inputs(home_screen));
}

#[test]
fn sync_all_is_scoped_to_the_screen_that_enabled_it() {
    let mut state = SyncedInputState::new();
    let workspace_screen = screen();
    let home_screen = screen();

    state.toggle_sync_all_terminal_inputs_in_all_tabs(workspace_screen);

    assert!(state.is_syncing_all_inputs(workspace_screen));
    assert!(!state.is_syncing_all_inputs(home_screen));
    assert!(
        !state.should_sync_this_pane_group(EntityId::new(), home_screen),
        "SyncedPanes::All must not leak into another screen"
    );
}

#[test]
fn disabling_sync_on_one_screen_leaves_the_other_alone() {
    let mut state = SyncedInputState::new();
    let first = screen();
    let second = screen();

    state.toggle_sync_all_terminal_inputs_in_all_tabs(first);
    state.toggle_sync_all_terminal_inputs_in_all_tabs(second);
    state.disable_sync_terminal_inputs(first);

    assert!(!state.is_syncing_any_inputs(first));
    assert!(state.is_syncing_all_inputs(second));
}
