//! Source-facing helpers that centralize the derivation of the CLI-agent icon shape
//! ([`IconWithStatusVariant`]) from the underlying state models. The invariant the
//! helpers enforce: any single logical agent run renders as the same brand color and
//! glyph regardless of which surface is rendering it (vertical tabs, pane header).
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
