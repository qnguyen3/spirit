use warp::integration_testing::assertions::assert_is_left_panel_open;
use warp::integration_testing::step::new_step_with_default_assertions;
use warp::integration_testing::terminal::wait_until_bootstrapped_single_pane_for_tab;
use warp::integration_testing::view_getters::workspace_view;
use warp::workspace::WorkspaceAction;
use warp::workspace::view::sessions::SESSIONS_PANEL_HEADER_POSITION_ID;
use warpui_core::async_assert;

use super::{Builder, new_builder};

pub fn test_sessions_panel_opens() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Open Sessions panel")
                .with_action(|app, window_id, _| {
                    let workspace = workspace_view(app, window_id);
                    app.update(|ctx| {
                        ctx.dispatch_typed_action_for_view(
                            window_id,
                            workspace.id(),
                            &WorkspaceAction::ToggleSessions,
                        );
                    });
                })
                .add_assertion(assert_is_left_panel_open())
                .add_assertion(|app, window_id| {
                    let presenter = app.presenter(window_id).expect("presenter should exist");
                    let presenter = presenter.borrow();
                    async_assert!(
                        presenter
                            .position_cache()
                            .get_position(SESSIONS_PANEL_HEADER_POSITION_ID)
                            .is_some(),
                        "Expected the Sessions panel header to be rendered"
                    )
                }),
        )
}
