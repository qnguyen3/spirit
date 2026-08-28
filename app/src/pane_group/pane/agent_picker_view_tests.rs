use std::cell::RefCell;
use std::rc::Rc;

use warp_core::ui::appearance::Appearance;
use warpui::platform::WindowStyle;
use warpui::{AddSingletonModel as _, App, TypedActionView as _, ViewHandle};

use super::{AgentPickerAction, AgentPickerView, AgentPickerViewEvent};
use crate::pane_group::PaneEvent;

fn picker_with_install_state(app: &mut App, installed: &[bool]) -> ViewHandle<AgentPickerView> {
    app.add_singleton_model(|_| Appearance::mock());
    let (_, view) = app.add_window(WindowStyle::NotStealFocus, AgentPickerView::new);
    let installed = installed.to_vec();
    view.update(app, |view, _ctx| {
        view.set_install_state_for_tests(&installed);
    });
    view
}

#[test]
fn initial_selection_is_first_installed_row() {
    App::test((), |mut app| async move {
        let view = picker_with_install_state(
            &mut app,
            &[false, true, true, false, false, false, false, false],
        );

        view.read(&app, |view, _ctx| {
            assert_eq!(view.selected_index_for_tests(), Some(1));
        });
    })
}

#[test]
fn down_skips_uninstalled_rows_and_wraps() {
    App::test((), |mut app| async move {
        let view = picker_with_install_state(
            &mut app,
            &[true, false, true, false, false, false, false, false],
        );

        view.update(&mut app, |view, ctx| {
            view.handle_action(&AgentPickerAction::Down, ctx);
            assert_eq!(view.selected_index_for_tests(), Some(2));
            view.handle_action(&AgentPickerAction::Down, ctx);
            assert_eq!(view.selected_index_for_tests(), Some(0));
        });
    })
}

#[test]
fn up_skips_uninstalled_rows_and_wraps() {
    App::test((), |mut app| async move {
        let view = picker_with_install_state(
            &mut app,
            &[true, false, true, false, false, false, false, false],
        );

        view.update(&mut app, |view, ctx| {
            view.handle_action(&AgentPickerAction::Up, ctx);
            assert_eq!(view.selected_index_for_tests(), Some(2));
            view.handle_action(&AgentPickerAction::Up, ctx);
            assert_eq!(view.selected_index_for_tests(), Some(0));
        });
    })
}

#[test]
fn navigation_is_noop_when_nothing_is_installed() {
    App::test((), |mut app| async move {
        let view = picker_with_install_state(&mut app, &[false; 8]);

        view.update(&mut app, |view, ctx| {
            view.handle_action(&AgentPickerAction::Down, ctx);
            assert_eq!(view.selected_index_for_tests(), None);
            view.handle_action(&AgentPickerAction::Up, ctx);
            assert_eq!(view.selected_index_for_tests(), None);
        });
    })
}

#[test]
fn selecting_an_uninstalled_row_does_not_change_selection() {
    App::test((), |mut app| async move {
        let view = picker_with_install_state(
            &mut app,
            &[true, false, true, false, false, false, false, false],
        );

        view.update(&mut app, |view, ctx| {
            view.handle_action(&AgentPickerAction::Select(1), ctx);
            assert_eq!(view.selected_index_for_tests(), Some(0));
        });
    })
}

#[test]
fn close_action_emits_close_pane_event() {
    App::test((), |mut app| async move {
        let view = picker_with_install_state(&mut app, &[true; 8]);
        let close_events: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
        let close_count = close_events.clone();
        app.update(|ctx| {
            ctx.subscribe_to_view(&view, move |_, event, _| {
                let AgentPickerViewEvent::Pane(PaneEvent::Close) = event else {
                    return;
                };
                *close_count.borrow_mut() += 1;
            });
        });

        view.update(&mut app, |view, ctx| {
            view.handle_action(&AgentPickerAction::Close, ctx);
        });

        assert_eq!(*close_events.borrow(), 1);
    })
}

#[cfg(unix)]
#[test]
fn install_state_is_detected_against_the_captured_shell_path() {
    use std::os::unix::fs::PermissionsExt as _;

    use crate::agent_launcher::catalog::agent_catalog;

    App::test((), |mut app| async move {
        let view = picker_with_install_state(&mut app, &[false; 8]);
        let temp_dir = tempfile::TempDir::new().unwrap();
        let binary_path = temp_dir.path().join(agent_catalog()[2].binary);
        std::fs::write(&binary_path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path_env = temp_dir.path().to_str().unwrap().to_owned();

        view.update(&mut app, |view, ctx| {
            view.apply_shell_path_for_tests(path_env, ctx);
            assert_eq!(
                view.install_state_for_tests(),
                vec![false, false, true, false, false, false, false, false]
            );
            assert_eq!(view.selected_index_for_tests(), Some(2));
        });
    })
}

#[test]
fn not_installed_section_starts_collapsed_and_toggles() {
    App::test((), |mut app| async move {
        let view = picker_with_install_state(
            &mut app,
            &[true, false, true, false, false, false, false, false],
        );

        view.update(&mut app, |view, ctx| {
            assert!(!view.not_installed_expanded_for_tests());
            view.handle_action(&AgentPickerAction::ToggleNotInstalled, ctx);
            assert!(view.not_installed_expanded_for_tests());
            view.handle_action(&AgentPickerAction::ToggleNotInstalled, ctx);
            assert!(!view.not_installed_expanded_for_tests());
        });
    })
}

#[cfg(unix)]
#[test]
fn not_installed_section_expands_when_no_agent_is_installed() {
    use std::os::unix::fs::PermissionsExt as _;

    use crate::agent_launcher::catalog::agent_catalog;

    App::test((), |mut app| async move {
        let view = picker_with_install_state(&mut app, &[true; 8]);
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path_env = temp_dir.path().to_str().unwrap().to_owned();

        view.update(&mut app, |view, ctx| {
            view.apply_shell_path_for_tests(path_env, ctx);
            assert_eq!(view.install_state_for_tests(), vec![false; 8]);
            assert!(view.not_installed_expanded_for_tests());
        });

        let binary_path = temp_dir.path().join(agent_catalog()[0].binary);
        std::fs::write(&binary_path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path_env = temp_dir.path().to_str().unwrap().to_owned();

        view.update(&mut app, |view, ctx| {
            view.apply_shell_path_for_tests(path_env, ctx);
            assert!(!view.not_installed_expanded_for_tests());
        });
    })
}
