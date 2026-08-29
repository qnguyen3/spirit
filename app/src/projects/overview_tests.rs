use std::path::PathBuf;

use warpui::App;
use warpui::platform::WindowStyle;

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

#[test]
fn a_git_project_card_carries_kind_branch_and_missing_chips() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let project_id = app.update(|ctx| {
            ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
                registry.register_project(
                    PathBuf::from("/nonexistent/spirit-overview-test"),
                    "spirit".to_owned(),
                    ProjectKind::Git,
                    Some("main".to_owned()),
                    ctx,
                )
            })
        });

        let (_, view) = app.add_window(WindowStyle::NotStealFocus, WorkspaceOverviewView::new);
        view.read(&app, |view, app| {
            let content = view.card_content(CardKind::Project(project_id), app);
            assert_eq!(content.name, "spirit");
            assert_eq!(content.icon, Icon::GitBranch);
            let labels: Vec<&str> = content
                .chips
                .iter()
                .map(|chip| chip.label.as_str())
                .collect();
            assert!(labels.contains(&"main"), "{labels:?}");
            assert!(labels.contains(&"missing"), "{labels:?}");
            let missing = content
                .chips
                .iter()
                .find(|chip| chip.label == "missing")
                .unwrap();
            assert!(missing.emphasized);
        });
    });
}

#[test]
fn home_and_new_cards_have_fixed_content() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let (_, view) = app.add_window(WindowStyle::NotStealFocus, WorkspaceOverviewView::new);
        view.read(&app, |view, app| {
            let home = view.card_content(CardKind::Home, app);
            assert_eq!(home.name, "Home");
            assert!(home.chips.is_empty());
            let new = view.card_content(CardKind::New, app);
            assert_eq!(new.name, "New Workspace");
            assert_eq!(new.icon, Icon::Plus);
        });
    });
}
