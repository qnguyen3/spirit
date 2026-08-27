use std::cell::Cell;
use std::rc::Rc;

use session_sharing_protocol::common::CLIAgentSessionState;
use warpui::{AppContext, SingletonEntity, WeakViewHandle};

use crate::terminal::cli_agent_sessions::{
    CLIAgentInputEntrypoint, CLIAgentInputState, CLIAgentRichInputCloseReason, CLIAgentSession,
    CLIAgentSessionContext, CLIAgentSessionStatus, CLIAgentSessionsModel,
};
use crate::terminal::{CLIAgent, TerminalView};

/// Applies CLI agent session + rich-input state from the remote side.
/// Creates/removes the session and opens/closes rich input based on
/// the given `CLIAgentSessionState`.
pub(crate) fn apply_cli_agent_state_update(
    weak_view_handle: &WeakViewHandle<TerminalView>,
    cli_agent_session: &CLIAgentSessionState,
    _guard: &ActiveRemoteUpdate,
    ctx: &mut AppContext,
) {
    let Some(view) = weak_view_handle.upgrade(ctx) else {
        return;
    };
    let view_id = view.id();

    match cli_agent_session {
        CLIAgentSessionState::Active {
            cli_agent,
            is_rich_input_open,
        } => {
            let agent = CLIAgent::from_serialized_name(cli_agent);

            // Create the agent session if it does not exist.
            let already_exists = CLIAgentSessionsModel::as_ref(ctx)
                .session(view_id)
                .is_some_and(|s| s.agent == agent);
            if !already_exists {
                CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions_model, ctx| {
                    sessions_model.set_session(
                        view_id,
                        CLIAgentSession {
                            agent,
                            status: CLIAgentSessionStatus::InProgress,
                            session_context: CLIAgentSessionContext::default(),
                            input_state: CLIAgentInputState::Closed,
                            listener: None,
                            plugin_version: None,
                            remote_host: None,
                            draft_text: None,
                            custom_command_prefix: None,
                            received_rich_notification: false,
                            // Viewer input is managed by the sync protocol,
                            // not local status-change auto-toggle.
                            should_auto_toggle_input: false,
                        },
                        ctx,
                    );
                });

                view.update(ctx, |view, ctx| {
                    view.apply_cli_agent_footer_visibility(true, ctx);
                });
            }

            // For cloud agent sessions with non-Oz harnesses, auto-open rich
            // input when creating a new CLI agent session so the viewer gets the
            // composer immediately (byte-sharing has roundtrip lag without it).
            let effective_rich_input_open =
                if !already_exists && view.as_ref(ctx).is_shared_ambient_agent_session() {
                    true
                } else {
                    *is_rich_input_open
                };

            // Update the rich input state.
            let currently_open = CLIAgentSessionsModel::as_ref(ctx).is_input_open(view_id);
            if currently_open != effective_rich_input_open {
                view.update(ctx, |view, ctx| {
                    if effective_rich_input_open {
                        view.open_cli_agent_rich_input(
                            CLIAgentInputEntrypoint::SharedSessionSync,
                            ctx,
                        );
                    } else {
                        view.close_cli_agent_rich_input(CLIAgentRichInputCloseReason::Other, ctx);
                    }
                });
            }

            view.update(ctx, |view, ctx| {
                view.sync_agent_view_for_shared_third_party_viewer(ctx);
            });
        }
        CLIAgentSessionState::Inactive => {
            // Session cleanup is handled by BlockCompleted events on the
            // viewer side, so no explicit teardown is needed here.
        }
    }
}

// ---------------------------------------------------------------------------
// Echo-suppression for remote session-sharing context updates.
//
// When a participant (viewer or sharer) receives a
// `UniversalDeveloperInputContextUpdate` from the remote side, the `apply_*`
// helpers above update local state which fires model events. Those events are
// observed by broadcast subscribers that would normally send the value *back*
// over the network, creating an echo loop.
//
// To prevent this, each side creates a `RemoteUpdateGuard` and:
//   1. Clones it into every broadcast subscriber, which calls
//      `guard.should_broadcast()` and skips when `false`.
//   2. Wraps incoming `apply_*` calls with `guard.start_remote_update()`,
//      which returns an `ActiveRemoteUpdate` RAII token that suppresses
//      broadcasts for the duration of the synchronous update.
//
// When adding a **new** field to `UniversalDeveloperInputContextUpdate`:
//   - Check `guard.should_broadcast()` in the new broadcast subscriber.
//   - Ensure the new `apply_*` call sits inside the existing
//     `ActiveRemoteUpdate` scope in the incoming handler.
// ---------------------------------------------------------------------------

/// Shared guard that tracks whether we are currently applying a remote
/// session-sharing context update.
#[derive(Clone)]
pub(crate) struct RemoteUpdateGuard {
    inner: Rc<Cell<bool>>,
}

impl RemoteUpdateGuard {
    /// Creates a new guard, initially not suppressing broadcasts.
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(Cell::new(false)),
        }
    }

    /// Returns `true` when a context update originated locally and should be
    /// broadcast to the remote side. Returns `false` when we are in the middle
    /// of applying a remote update (i.e. the echo should be suppressed).
    pub(crate) fn should_broadcast(&self) -> bool {
        !self.inner.get()
    }

    /// Returns an RAII token that suppresses outgoing broadcasts until dropped.
    /// Wrap all `apply_*` calls for incoming remote updates in this so that
    /// the synchronous event dispatch sees the guard as active.
    pub(crate) fn start_remote_update(&self) -> ActiveRemoteUpdate {
        debug_assert!(
            !self.inner.get(),
            "RemoteUpdateGuard::start_remote_update called while already active"
        );
        self.inner.set(true);
        ActiveRemoteUpdate {
            inner: self.inner.clone(),
        }
    }
}

/// RAII token that suppresses outgoing broadcasts while held.
pub(crate) struct ActiveRemoteUpdate {
    inner: Rc<Cell<bool>>,
}

impl Drop for ActiveRemoteUpdate {
    fn drop(&mut self) {
        self.inner.set(false);
    }
}
