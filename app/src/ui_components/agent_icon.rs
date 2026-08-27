//! Source-facing helpers that centralize the derivation of the agent-icon shape
//! ([`IconWithStatusVariant`]) from the underlying state models. The invariant the
//! helpers enforce: any single logical agent run renders as the same brand color, glyph,
//! and ambient-vs-local treatment regardless of which surface is rendering it (vertical
//! tabs, pane header, conversation list, notifications mailbox).
//!
//! Each helper is a thin adapter over one data source. Surfaces call the helper for
//! whichever source they hold and feed the resulting variant into
//! [`render_icon_with_status`]. The pure inner functions in this module are exercised
//! directly by the cross-surface consistency tests in `agent_icon_tests.rs`.
use warpui::{AppContext, SingletonEntity};

use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::view::TerminalView;
use crate::ui_components::icon_with_status::IconWithStatusVariant;
use crate::ui_components::status_icons::ConversationStatus;

/// Returns the agent-icon variant for a live [`TerminalView`], or `None` when the terminal is
/// not an agent surface (plain terminal / shell / empty session). A
/// [`CLIAgentSessionsModel`] session with a known agent wins; plugin-backed sessions
/// surface rich status while command-detected sessions don't.
pub(crate) fn terminal_view_agent_icon_variant(
    terminal_view: &TerminalView,
    app: &AppContext,
) -> Option<IconWithStatusVariant> {
    let session = CLIAgentSessionsModel::as_ref(app).session(terminal_view.id())?;
    if matches!(session.agent, CLIAgent::Unknown) {
        return None;
    }
    let status: Option<ConversationStatus> = (session.listener.is_some()
        && session.supports_rich_status())
    .then(|| session.status.to_conversation_status());
    Some(IconWithStatusVariant::CLIAgent {
        agent: session.agent,
        status,
        is_ambient: false,
    })
}
