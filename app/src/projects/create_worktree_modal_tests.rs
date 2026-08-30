use warpui::App;

use super::CreateWorktreeModal;
use crate::test_util::layout::build_scene_for_root_view;
use crate::workspace::view::tests::initialize_app;

#[test]
fn modal_lays_out_inside_a_bounded_window() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        build_scene_for_root_view(&mut app, CreateWorktreeModal::new);
    });
}

#[test]
fn modal_lays_out_with_the_agent_menu_open() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        build_scene_for_root_view(&mut app, |ctx| {
            let mut modal = CreateWorktreeModal::new(ctx);
            modal.show_agent_menu(ctx);
            modal
        });
    });
}
