use warpui::integration::TestStep;
use warpui::{SingletonEntity, async_assert};

use crate::integration_testing::step::new_step_with_default_assertions;
use crate::integration_testing::view_getters::workspace_view;
use crate::tab::{tab_position_id, vertical_tabs_forced};
use crate::undo_close::UndoCloseStack;
use crate::workspace::{
    NEW_SESSION_MENU_TERMINAL_LABEL, VERTICAL_TABS_ADD_TAB_POSITION_ID, Workspace,
    vtab_close_button_position_id,
};

/// Mock pressing a button on the Warp-native quit modal. Note that this modal is currently only
/// used on Linux, not macOS.
pub fn press_native_modal_button(button_index: usize) -> TestStep {
    TestStep::new("Press a native modal button")
        .with_action(move |app, _, _data| {
            let active_window = app
                .read(|ctx| ctx.windows().active_window())
                .expect("no active window");
            let workspace = workspace_view(app, active_window);
            app.update(|ctx| {
                assert!(
                    workspace.as_ref(ctx).is_native_quit_modal_open(ctx),
                    "Native modal should be open"
                );
                Workspace::press_native_modal_button(&workspace, button_index, ctx);
            });
        })
        .add_assertion(|app, window_id| {
            let workspace = workspace_view(app, window_id);
            workspace.read(app, |workspace, ctx| {
                async_assert!(
                    !workspace.is_native_quit_modal_open(ctx),
                    "Native modal is still open"
                )
            })
        })
}

/// Trigger undo close (restore closed pane/tab/window) action.
pub fn trigger_undo_close() -> TestStep {
    TestStep::new("Trigger undo close").with_action(move |app, _, _data| {
        app.update(|ctx| {
            UndoCloseStack::handle(ctx).update(ctx, |stack, model_ctx| {
                stack.undo_close(model_ctx);
            });
        });
    })
}

pub fn add_terminal_tab_with_new_tab_button() -> Vec<TestStep> {
    vec![
        new_step_with_default_assertions("Open the new session menu from the add tab button")
            .with_click_on_saved_position(VERTICAL_TABS_ADD_TAB_POSITION_ID),
        new_step_with_default_assertions("Pick the terminal entry in the new session menu")
            .with_click_on_saved_position(NEW_SESSION_MENU_TERMINAL_LABEL),
    ]
}

pub fn close_tab_with_close_button(tab_index: usize) -> Vec<TestStep> {
    vec![
        // The close button only exists while the row is hovered, so the hover needs its own frame.
        new_step_with_default_assertions("Hover the tab to reveal its close button")
            .with_hover_over_saved_position(tab_position_id(tab_index)),
        new_step_with_default_assertions("Close the tab with its close button")
            .with_hover_over_saved_position(vtab_close_button_position_id(tab_index))
            .with_click_on_saved_position(vtab_close_button_position_id(tab_index)),
    ]
}

pub fn horizontal_tab_bar_available() -> bool {
    !vertical_tabs_forced()
}
