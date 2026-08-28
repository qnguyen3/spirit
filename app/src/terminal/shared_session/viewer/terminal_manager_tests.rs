//! Regression tests for the viewer `TerminalManager`'s `on_view_detached`
//! discriminator.
//!
//! `TerminalManager::on_view_detached` tears the viewer session down on
//! `DetachType::Closed`, while deliberately preserving it on `HiddenForClose`
//! (undo-close grace window) and `Moved`.

use async_broadcast::broadcast;
use warpui::App;

use super::*;
// Bring the `TerminalManager` trait into scope (named under a different alias
// since the local `TerminalManager` struct shadows it) so the trait method
// `on_view_detached` is callable on the struct.
use crate::terminal::TerminalManager as _;
use crate::terminal::model::session::Sessions;
use crate::test_util::add_window_with_terminal;
use crate::test_util::terminal::initialize_app_for_terminal_view;

/// Constructs a viewer `TerminalManager` whose model is an active viewer of a
/// shared session, and returns that model so callers can observe the teardown.
///
/// Deliberately bypasses `TerminalManager::new_internal` / `new_deferred`
/// (which would build a whole view stack with a real `TerminalView::new`
/// instead of `TerminalView::new_for_test`); the `on_view_detached` path only
/// depends on a small subset of the manager's fields, so a struct-literal
/// construction keeps the test focused.
fn build_active_viewer_manager(app: &mut App) -> (TerminalManager, Arc<FairMutex<TerminalModel>>) {
    let terminal_view = add_window_with_terminal(app, None);

    // The network-side fields are left in their `Idle` / `None` defaults so
    // `on_view_detached` short-circuits the live-network teardown branch.
    let (wakeups_tx, _wakeups_rx) = async_channel::unbounded();
    let (events_tx, events_rx) = async_channel::unbounded();
    let (pty_reads_tx, pty_reads_rx) = broadcast(8);
    let inactive_pty_reads_rx = pty_reads_rx.deactivate();
    let channel_event_proxy = ChannelEventListener::new(wakeups_tx, events_tx, pty_reads_tx);

    let model = Arc::new(FairMutex::new(TerminalModel::mock(None, None)));
    model
        .lock()
        .set_shared_session_status(SharedSessionStatus::ActiveViewer {
            role: Default::default(),
        });
    let sessions = app.add_model(|_| Sessions::new_for_test());
    let model_events =
        app.add_model(|ctx| ModelEventDispatcher::new(events_rx, sessions.clone(), ctx));
    let prompt_type =
        app.add_model(|_| PromptType::new_static(vec![], false, WarpPromptSeparator::None));

    let manager = TerminalManager {
        model: model.clone(),
        view: terminal_view,
        _model_events: model_events,
        _inactive_pty_reads_rx: inactive_pty_reads_rx,
        network_state: NetworkState::Idle,
        network_resources: NetworkResources {
            prompt_type,
            channel_event_proxy,
        },
        current_network: Arc::new(FairMutex::new(None)),
        viewer_remote_update_guard: RemoteUpdateGuard::new(),
        outbound_handlers_registered: false,
    };
    (manager, model)
}

#[test]
fn on_view_detached_closed_finishes_the_viewer_session() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let (manager, model) = build_active_viewer_manager(&mut app);

        app.update(|ctx| manager.on_view_detached(DetachType::Closed, ctx));

        assert!(
            model.lock().shared_session_status().is_finished_viewer(),
            "post-detach (Closed): the viewer session should be finished"
        );
    });
}

#[test]
fn on_view_detached_hidden_for_close_keeps_the_viewer_session_alive() {
    // Negative case: HiddenForClose is part of the undo-close grace
    // window. The viewer session must stay alive so the pane restores
    // seamlessly if the user undoes the close.
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let (manager, model) = build_active_viewer_manager(&mut app);

        app.update(|ctx| manager.on_view_detached(DetachType::HiddenForClose, ctx));

        assert!(
            !model.lock().shared_session_status().is_finished_viewer(),
            "HiddenForClose must NOT finish the viewer (undo-close grace window)"
        );
    });
}

#[test]
fn on_view_detached_moved_keeps_the_viewer_session_alive() {
    // Negative case: Moved transfers the `TerminalManager` to a new pane
    // group. Tearing the session down would break the live session on the
    // moved pane.
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let (manager, model) = build_active_viewer_manager(&mut app);

        app.update(|ctx| manager.on_view_detached(DetachType::Moved, ctx));

        assert!(
            !model.lock().shared_session_status().is_finished_viewer(),
            "Moved must NOT finish the viewer (the manager is reused in the new pane group)"
        );
    });
}
