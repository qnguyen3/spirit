use std::path::PathBuf;

use warpui::App;

use super::*;
use crate::test_util::layout::build_scene_for_root_view;
use crate::workspace::view::tests::initialize_app;

#[test]
fn middle_truncate_keeps_both_ends() {
    let short = PathBuf::from("/tmp/repo");
    assert_eq!(middle_truncate(&short, 34), "/tmp/repo");

    let long = PathBuf::from("/Users/someone/very/deeply/nested/checkout/of/a/repository");
    let truncated = middle_truncate(&long, 20);
    assert!(truncated.chars().count() <= 20, "{truncated}");
    assert!(truncated.starts_with("/Users/so"), "{truncated}");
    assert!(truncated.ends_with("epository"), "{truncated}");
    assert!(truncated.contains('\u{2026}'), "{truncated}");
}

#[test]
fn overview_lays_out_inside_a_bounded_window() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        build_scene_for_root_view(&mut app, WorkspaceOverviewView::new);
    });
}
