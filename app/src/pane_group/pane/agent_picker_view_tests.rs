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
