use warpui::App;

use super::DeleteWorktreeDialog;
use crate::test_util::layout::build_scene_for_root_view;
use crate::workspace::view::tests::initialize_app;

#[test]
fn dialog_lays_out_inside_a_bounded_window() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        build_scene_for_root_view(&mut app, DeleteWorktreeDialog::new);
    });
}
