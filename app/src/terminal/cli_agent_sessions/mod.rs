pub mod event;
pub mod signal;

use std::collections::HashMap;
use std::time::Duration;

use event::{CLIAgentEvent, CLIAgentEventType};
use instant::Instant;
use warpui::r#async::SpawnedFutureHandle;
use warpui::{Entity, EntityId, ModelContext, SingletonEntity};

use self::signal::AgentSignal;
use super::CLIAgent;
use crate::ui_components::status_icons::ConversationStatus;

/// How long to wait, after observing a synthesized Ctrl-C write to a working
/// CLI agent session's PTY, for further agent activity before concluding the
/// interrupt silently cancelled the session. See `observe_ctrl_c_write`.
pub const CTRL_C_CANCEL_WINDOW: Duration = Duration::from_secs(2);

/// A bell and an OSC 777 body can both describe the same finished turn, so
/// notifications within this window of the last one are dropped.
pub const NOTIFY_COOLDOWN: Duration = Duration::from_secs(3);

/// Status of a tracked CLI agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CLIAgentSessionStatus {
    Idle,
    InProgress,
    Success,
    Failed {
        error_type: Option<String>,
        message: Option<String>,
    },
    Blocked {
        message: Option<String>,
    },
    /// The user interrupted the session with Ctrl-C and no further plugin
    /// activity was observed within the grace window (see
    /// `observe_ctrl_c_write`). Not terminal: a later `prompt_submit`
    /// returns the session to `InProgress` like any other resumed turn.
    Cancelled,
}

impl CLIAgentSessionStatus {
    pub fn to_conversation_status(&self) -> Option<ConversationStatus> {
        match self {
            CLIAgentSessionStatus::Idle => None,
            CLIAgentSessionStatus::InProgress => Some(ConversationStatus::InProgress),
            CLIAgentSessionStatus::Success => Some(ConversationStatus::Success),
            CLIAgentSessionStatus::Failed { .. } => Some(ConversationStatus::Error),
            CLIAgentSessionStatus::Blocked { message } => Some(ConversationStatus::Blocked {
                blocked_action: message.clone().unwrap_or_default(),
            }),
            CLIAgentSessionStatus::Cancelled => Some(ConversationStatus::Cancelled),
        }
    }
}

/// Rich context accumulated from CLI agent session events.
#[derive(Debug, Clone, Default)]
pub struct CLIAgentSessionContext {
    pub cwd: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input_preview: Option<String>,
    pub summary: Option<String>,
    pub query: Option<String>,
    pub response: Option<String>,
    pub transcript_path: Option<String>,
}

/// State of the rich input editor for composing a prompt to send to a CLI agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CLIAgentInputState {
    /// The rich input editor is not open.
    Closed,
    /// The rich input editor is open.
    Open,
}

impl CLIAgentSessionContext {
    pub(crate) fn display_title(&self) -> Option<String> {
        self.latest_user_prompt().or_else(|| self.title_like_text())
    }

    pub(crate) fn latest_user_prompt(&self) -> Option<String> {
        self.query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_owned)
    }

    /// Returns summary text suitable as a fallback title when no user prompt is available.
    pub(crate) fn title_like_text(&self) -> Option<String> {
        self.summary
            .as_deref()
            .map(str::trim)
            .filter(|summary| !summary.is_empty())
            .map(str::to_owned)
    }
}

/// A tracked CLI agent session.
#[derive(Debug, Clone)]
pub struct CLIAgentSession {
    pub agent: CLIAgent,
    pub status: CLIAgentSessionStatus,
    pub session_context: CLIAgentSessionContext,
    /// Rich input editor state.
    pub input_state: CLIAgentInputState,
    /// Whether status-driven auto-toggle is enabled for this session.
    pub should_auto_toggle_input: bool,
    /// Draft text saved from the rich input composer when it was closed.
    /// Restored into the editor when the composer is reopened.
    pub draft_text: Option<String>,
}

impl CLIAgentSession {
    /// Clears state populated by `PermissionRequest`. Called whenever the
    /// session leaves the permission flow (the user replied, a blocking tool
    /// completed, a new prompt is submitted, or the session ends successfully)
    /// so the permission summary doesn't leak into later UI surfaces — most
    /// visibly the tab title, which can fall back to `summary` when `query`
    /// is unset.
    fn clear_permission_scoped_state(&mut self) {
        self.session_context.summary = None;
        self.session_context.tool_name = None;
        self.session_context.tool_input_preview = None;
    }

    /// Applies an event to this session, updating context and status.
    /// Returns the new status if it changed, or `None` if the event was irrelevant.
    fn apply_event(&mut self, event: &CLIAgentEvent) -> Option<CLIAgentSessionStatus> {
        self.session_context.cwd = event.cwd.clone().or(self.session_context.cwd.take());
        self.session_context.project = event
            .project
            .clone()
            .or(self.session_context.project.take());
        self.session_context.session_id = event
            .session_id
            .clone()
            .or(self.session_context.session_id.take());
        self.session_context.transcript_path = event
            .payload
            .transcript_path
            .clone()
            .or(self.session_context.transcript_path.take());

        let new_status = match &event.event {
            CLIAgentEventType::PromptSubmit => {
                self.session_context.query = event.payload.query.clone();
                self.session_context.response = None;
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::InProgress
            }
            CLIAgentEventType::ToolComplete => {
                if !matches!(self.status, CLIAgentSessionStatus::Blocked { .. }) {
                    return None;
                }
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::InProgress
            }
            CLIAgentEventType::Stop => {
                self.session_context.query = event.payload.query.clone();
                self.session_context.response = event.payload.response.clone();
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::Success
            }
            CLIAgentEventType::StopFailure => {
                self.session_context.query = event.payload.query.clone();
                self.session_context.response = event.payload.response.clone();
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::Failed {
                    error_type: event.payload.error_type.clone(),
                    message: event.payload.response.clone(),
                }
            }
            CLIAgentEventType::PermissionRequest => {
                self.session_context.summary = event.payload.summary.clone();
                self.session_context.tool_name = event.payload.tool_name.clone();
                self.session_context.tool_input_preview = event.payload.tool_input_preview.clone();
                CLIAgentSessionStatus::Blocked {
                    message: event.payload.summary.clone(),
                }
            }
            CLIAgentEventType::QuestionAsked => CLIAgentSessionStatus::Blocked {
                message: event
                    .payload
                    .summary
                    .clone()
                    .or_else(|| Some("Waiting for your answer".to_owned())),
            },
            CLIAgentEventType::PermissionReplied => {
                if !matches!(self.status, CLIAgentSessionStatus::Blocked { .. }) {
                    return None;
                }
                self.clear_permission_scoped_state();
                CLIAgentSessionStatus::InProgress
            }
            // IdlePrompt means the agent is sitting at its prompt waiting for input.
            // This should not affect status — otherwise it would override Success after a Stop event.
            CLIAgentEventType::IdlePrompt => return None,
            CLIAgentEventType::SessionStart => return None,
            CLIAgentEventType::Unknown(_) => return None,
        };

        self.status = new_status.clone();
        Some(new_status)
    }
}

/// Events emitted by `CLIAgentSessionsModel` for subscribers (e.g., `AgentNotificationsModel`).
#[allow(dead_code)] // `agent` fields on Started/InputSessionChanged/Ended are used for logging and future subscribers.
#[derive(Debug, Clone)]
pub enum CLIAgentSessionsModelEvent {
    Started {
        terminal_view_id: EntityId,
        agent: CLIAgent,
    },
    StatusChanged {
        terminal_view_id: EntityId,
        agent: CLIAgent,
        status: CLIAgentSessionStatus,
        session_context: Box<CLIAgentSessionContext>,
    },
    InputSessionChanged {
        terminal_view_id: EntityId,
        agent: CLIAgent,
        /// The input state BEFORE this change. When transitioning from
        /// `Open` → `Closed`, contains the saved input config to restore.
        previous_input_state: CLIAgentInputState,
        /// The input state AFTER this change.
        new_input_state: CLIAgentInputState,
    },
    Ended {
        terminal_view_id: EntityId,
        agent: CLIAgent,
    },
    /// The agent session has been updated. Subscribers may use this as a trigger for best-effort
    /// saving of state derived from the agent's session.
    SessionUpdated {
        terminal_view_id: EntityId,
        agent: CLIAgent,
    },
}

impl CLIAgentSessionsModelEvent {
    pub fn terminal_view_id(&self) -> EntityId {
        match self {
            CLIAgentSessionsModelEvent::Started {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::StatusChanged {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::InputSessionChanged {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::Ended {
                terminal_view_id, ..
            }
            | CLIAgentSessionsModelEvent::SessionUpdated {
                terminal_view_id, ..
            } => *terminal_view_id,
        }
    }
}

/// Per-session state for a Ctrl-C-initiated pending cancellation. Kept
/// separate from `CLIAgentSession` because it is synthesized entirely
/// client-side (see `observe_ctrl_c_write`) rather than reported by the
/// plugin protocol.
#[derive(Default)]
struct CtrlCCancelState {
    /// Whether a `prompt_submit` has been seen for this session. Guards
    /// against arming on the optimistic `InProgress` status set when a
    /// session is first registered, before any turn has actually started.
    has_seen_prompt_submit: bool,
    /// Abort handle for the in-flight grace-window timer, if armed.
    pending_cancel: Option<SpawnedFutureHandle>,
    /// Identifies the window `pending_cancel` belongs to. `SpawnedFutureHandle::abort`
    /// only takes effect the next time the future is polled, so a timer that has
    /// already completed (and queued its resolve callback) can still run after
    /// `abort()` is called. The callback captures this token and only acts if it
    /// still matches when it fires, so a stale callback racing a disarming event
    /// is a no-op instead of overwriting that event's status with `Cancelled`.
    armed_token: Option<u64>,
}

/// Singleton model that tracks pane-scoped CLI agent state and session context.
pub struct CLIAgentSessionsModel {
    sessions: HashMap<EntityId, CLIAgentSession>,
    last_notified_at: HashMap<EntityId, Instant>,
    /// Ctrl-C pending-cancel state, keyed by terminal view. See `observe_ctrl_c_write`.
    ctrl_c_cancel_state: HashMap<EntityId, CtrlCCancelState>,
    /// Source of `CtrlCCancelState::armed_token` values. Monotonically increasing;
    /// never reused, so a stale callback can never alias a newer window.
    next_ctrl_c_token: u64,
}

impl Entity for CLIAgentSessionsModel {
    type Event = CLIAgentSessionsModelEvent;
}

impl SingletonEntity for CLIAgentSessionsModel {}

impl CLIAgentSessionsModel {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            last_notified_at: HashMap::new(),
            ctrl_c_cancel_state: HashMap::new(),
            next_ctrl_c_token: 0,
        }
    }

    pub fn session(&self, terminal_view_id: EntityId) -> Option<&CLIAgentSession> {
        self.sessions.get(&terminal_view_id)
    }

    pub fn sessions(&self) -> impl Iterator<Item = (EntityId, &CLIAgentSession)> {
        self.sessions
            .iter()
            .map(|(terminal_view_id, session)| (*terminal_view_id, session))
    }

    /// Returns `true` if the rich input editor is currently open for this terminal.
    pub fn is_input_open(&self, terminal_view_id: EntityId) -> bool {
        self.sessions
            .get(&terminal_view_id)
            .is_some_and(|s| matches!(s.input_state, CLIAgentInputState::Open))
    }

    pub fn remove_session(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        // Closed before the session disappears so subscribers can restore the input.
        self.close_input(terminal_view_id, false, ctx);
        self.abort_pending_cancel(terminal_view_id);
        self.ctrl_c_cancel_state.remove(&terminal_view_id);
        self.last_notified_at.remove(&terminal_view_id);
        if let Some(session) = self.sessions.remove(&terminal_view_id) {
            ctx.emit(CLIAgentSessionsModelEvent::Ended {
                terminal_view_id,
                agent: session.agent,
            });
        }
    }

    /// Updates the session's status and context from a parsed CLI agent event.
    pub fn update_from_event(
        &mut self,
        terminal_view_id: EntityId,
        event: &CLIAgentEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        if !self.sessions.contains_key(&terminal_view_id) {
            return;
        }

        // Any agent event other than `IdlePrompt` is evidence the CLI agent
        // process is still alive — an interrupt produces silence instead.
        // Disarm a pending Ctrl-C cancellation window so this event's own
        // status transition drives the session. `IdlePrompt` is excluded:
        // it means the CLI is sitting idle at its interactive prompt, which
        // is evidence of idleness rather than aliveness, so treating it as
        // disarming would let an idle notification that arrives instead of
        // a genuine `stop`/`stop_failure` after an interrupt silently defeat
        // the grace window, leaving the session stuck exactly like the bug
        // this feature exists to fix. `apply_event` still treats `IdlePrompt`
        // as a no-op for status, independent of this.
        if !matches!(event.event, CLIAgentEventType::IdlePrompt) {
            self.abort_pending_cancel(terminal_view_id);
        }
        if matches!(event.event, CLIAgentEventType::PromptSubmit) {
            self.ctrl_c_cancel_state
                .entry(terminal_view_id)
                .or_default()
                .has_seen_prompt_submit = true;
        }

        let session = self
            .sessions
            .get_mut(&terminal_view_id)
            .expect("session presence checked above");

        let event_type = &event.event;
        if let Some(new_status) = session.apply_event(event) {
            let agent = session.agent;
            ctx.emit(CLIAgentSessionsModelEvent::StatusChanged {
                terminal_view_id,
                agent,
                status: new_status,
                session_context: Box::new(session.session_context.clone()),
            });
        }

        if matches!(
            event_type,
            CLIAgentEventType::SessionStart
                | CLIAgentEventType::PromptSubmit
                | CLIAgentEventType::ToolComplete
        ) {
            ctx.emit(CLIAgentSessionsModelEvent::SessionUpdated {
                terminal_view_id,
                agent: session.agent,
            });
        }
    }

    pub fn apply_signal(
        &mut self,
        terminal_view_id: EntityId,
        signal: AgentSignal,
        message: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&terminal_view_id) else {
            return;
        };

        // A repeat of a finished status must still emit: the next turn can start
        // without a carriage return to observe, and skipping it here would
        // silently drop that turn's notification.
        let new_status = signal.into_status(message);
        if signal == AgentSignal::Working && session.status == new_status {
            return;
        }
        session.status = new_status.clone();
        let agent = session.agent;
        let session_context = Box::new(session.session_context.clone());

        self.abort_pending_cancel(terminal_view_id);
        ctx.emit(CLIAgentSessionsModelEvent::StatusChanged {
            terminal_view_id,
            agent,
            status: new_status,
            session_context,
        });
    }

    /// Bells and OSC bodies only ever describe a turn *ending*, so without this
    /// a session that reached `Success` would never leave it.
    pub fn observe_prompt_submit_write(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        if !self.sessions.contains_key(&terminal_view_id) {
            return;
        }
        self.ctrl_c_cancel_state
            .entry(terminal_view_id)
            .or_default()
            .has_seen_prompt_submit = true;
        self.apply_signal(terminal_view_id, AgentSignal::Working, None, ctx);
    }

    pub fn claim_notification_slot(&mut self, terminal_view_id: EntityId) -> bool {
        let now = Instant::now();
        let is_due = self
            .last_notified_at
            .get(&terminal_view_id)
            .is_none_or(|last| now.duration_since(*last) >= NOTIFY_COOLDOWN);
        if is_due {
            self.last_notified_at.insert(terminal_view_id, now);
        }
        is_due
    }

    /// Observes a Ctrl-C byte (`0x03`) written to this session's PTY.
    ///
    /// This is observation only: the caller is responsible for forwarding the
    /// byte to the PTY unchanged and immediately, regardless of what this
    /// does. If the session is currently interruptible — `InProgress` or
    /// `Blocked`, rich-status-capable (excludes the Codex OSC 9 fallback),
    /// and has seen at least one `prompt_submit` (guarding against the
    /// optimistic `InProgress` set at registration, before any turn has
    /// started) — arms a grace window after which the session resolves to
    /// `Cancelled` if no disarming plugin activity arrives first (any event
    /// except `IdlePrompt`, which is evidence of idleness rather than
    /// aliveness; see `update_from_event`). A second Ctrl-C while a window
    /// is already armed reuses it rather than resetting the clock.
    pub fn observe_ctrl_c_write(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.observe_ctrl_c_write_with_window(terminal_view_id, CTRL_C_CANCEL_WINDOW, ctx);
    }

    fn observe_ctrl_c_write_with_window(
        &mut self,
        terminal_view_id: EntityId,
        window: Duration,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get(&terminal_view_id) else {
            return;
        };
        let can_arm = matches!(
            session.status,
            CLIAgentSessionStatus::InProgress | CLIAgentSessionStatus::Blocked { .. }
        ) && self
            .ctrl_c_cancel_state
            .get(&terminal_view_id)
            .is_some_and(|state| state.has_seen_prompt_submit);
        if !can_arm {
            return;
        }
        if self
            .ctrl_c_cancel_state
            .get(&terminal_view_id)
            .is_some_and(|state| state.pending_cancel.is_some())
        {
            return;
        }

        let token = self.next_ctrl_c_token;
        self.next_ctrl_c_token += 1;
        let state = self
            .ctrl_c_cancel_state
            .entry(terminal_view_id)
            .or_default();
        let handle = ctx.spawn_abortable(
            async move { warpui::r#async::Timer::after(window).await },
            move |model, _, ctx| model.resolve_pending_cancel(terminal_view_id, token, ctx),
            |_, _| {},
        );
        state.pending_cancel = Some(handle);
        state.armed_token = Some(token);
    }

    /// Called when a session's pending-cancel window lapses with no
    /// disarming plugin event. Transitions the session to `Cancelled` unless
    /// `token` no longer matches the currently armed window — meaning this
    /// callback was already queued (post `Timer::after` completion, pre-poll)
    /// when the window was disarmed, replaced, or removed, and
    /// `SpawnedFutureHandle::abort` did not take effect in time to stop it.
    fn resolve_pending_cancel(
        &mut self,
        terminal_view_id: EntityId,
        token: u64,
        ctx: &mut ModelContext<Self>,
    ) {
        let owns_current_window = self
            .ctrl_c_cancel_state
            .get_mut(&terminal_view_id)
            .is_some_and(|state| {
                if state.armed_token != Some(token) {
                    return false;
                }
                state.pending_cancel = None;
                state.armed_token = None;
                true
            });
        if !owns_current_window {
            return;
        }
        self.force_cancel(terminal_view_id, ctx);
    }

    /// Aborts and clears any armed pending-cancel window for this terminal.
    /// Invalidates the token so a callback already queued when the abort
    /// fires too late to matter (see `resolve_pending_cancel`) is a no-op.
    fn abort_pending_cancel(&mut self, terminal_view_id: EntityId) {
        if let Some(state) = self.ctrl_c_cancel_state.get_mut(&terminal_view_id) {
            state.armed_token = None;
            if let Some(handle) = state.pending_cancel.take() {
                handle.abort();
            }
        }
    }

    /// Sets `status` to `Cancelled` and emits `StatusChanged`, unless the
    /// session has already moved past `InProgress`/`Blocked` or no longer
    /// exists.
    fn force_cancel(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        let Some(session) = self.sessions.get_mut(&terminal_view_id) else {
            return;
        };
        if !matches!(
            session.status,
            CLIAgentSessionStatus::InProgress | CLIAgentSessionStatus::Blocked { .. }
        ) {
            return;
        }

        session.status = CLIAgentSessionStatus::Cancelled;
        let agent = session.agent;
        let session_context = Box::new(session.session_context.clone());
        ctx.emit(CLIAgentSessionsModelEvent::StatusChanged {
            terminal_view_id,
            agent,
            status: CLIAgentSessionStatus::Cancelled,
            session_context,
        });
    }

    /// Whether Ctrl-C cancellation has already resolved (`Cancelled`) or is
    /// still pending (the grace window is armed) for this session. Only
    /// used by tests.
    #[cfg(test)]
    pub(crate) fn has_pending_or_resolved_ctrl_c_cancel(&self, terminal_view_id: EntityId) -> bool {
        if matches!(
            self.sessions.get(&terminal_view_id).map(|s| &s.status),
            Some(CLIAgentSessionStatus::Cancelled)
        ) {
            return true;
        }
        self.ctrl_c_cancel_state
            .get(&terminal_view_id)
            .is_some_and(|state| state.pending_cancel.is_some())
    }

    pub fn open_input(
        &mut self,
        terminal_view_id: EntityId,
        should_auto_toggle_input: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&terminal_view_id) else {
            return;
        };
        if session.input_state == CLIAgentInputState::Open {
            return;
        }

        let previous_input_state = session.input_state;
        session.input_state = CLIAgentInputState::Open;
        session.should_auto_toggle_input = should_auto_toggle_input;
        ctx.emit(CLIAgentSessionsModelEvent::InputSessionChanged {
            terminal_view_id,
            agent: session.agent,
            previous_input_state,
            new_input_state: CLIAgentInputState::Open,
        });
    }

    /// Saves draft text from the rich input composer for the given terminal.
    /// Stores `None` for empty or whitespace-only text.
    pub fn set_draft(&mut self, terminal_view_id: EntityId, text: String) {
        if let Some(session) = self.sessions.get_mut(&terminal_view_id) {
            session.draft_text = if text.trim().is_empty() {
                None
            } else {
                Some(text)
            };
        }
    }

    /// Clears any saved draft text for the given terminal.
    pub fn clear_draft(&mut self, terminal_view_id: EntityId) {
        if let Some(session) = self.sessions.get_mut(&terminal_view_id) {
            session.draft_text = None;
        }
    }

    /// Returns and clears the draft text for the given terminal, if any.
    pub fn take_draft(&mut self, terminal_view_id: EntityId) -> Option<String> {
        self.sessions
            .get_mut(&terminal_view_id)
            .and_then(|s| s.draft_text.take())
    }

    pub fn close_input(
        &mut self,
        terminal_view_id: EntityId,
        should_auto_toggle_input: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&terminal_view_id) else {
            return;
        };
        if session.input_state == CLIAgentInputState::Closed {
            return;
        }

        let previous_input_state = session.input_state;
        session.input_state = CLIAgentInputState::Closed;
        session.should_auto_toggle_input = should_auto_toggle_input;
        ctx.emit(CLIAgentSessionsModelEvent::InputSessionChanged {
            terminal_view_id,
            agent: session.agent,
            previous_input_state,
            new_input_state: CLIAgentInputState::Closed,
        });
    }

    pub fn set_session(
        &mut self,
        terminal_view_id: EntityId,
        session: CLIAgentSession,
        ctx: &mut ModelContext<Self>,
    ) {
        let agent = session.agent;
        // Close any open rich input before replacing, so subscribers can
        // restore input config before the session ends.
        self.close_input(terminal_view_id, false, ctx);
        // A fresh session must re-observe `prompt_submit` before Ctrl-C can
        // arm, and any pending window belonged to the session being replaced.
        self.abort_pending_cancel(terminal_view_id);
        self.ctrl_c_cancel_state.remove(&terminal_view_id);
        self.last_notified_at.remove(&terminal_view_id);
        if let Some(old) = self.sessions.insert(terminal_view_id, session) {
            ctx.emit(CLIAgentSessionsModelEvent::Ended {
                terminal_view_id,
                agent: old.agent,
            });
        }

        ctx.emit(CLIAgentSessionsModelEvent::Started {
            terminal_view_id,
            agent,
        });
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
