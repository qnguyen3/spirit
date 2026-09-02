use std::path::Path;

use warpui::platform::{StatusItem, StatusItemEntry, TerminationMode};
use warpui::{AppContext, AssetProvider, Entity, EntityId, SingletonEntity, WindowId};

use crate::ASSETS;
use crate::projects::ProjectId;
use crate::root_view::{open_new_or_restore_session, quake_mode_window_id};
use crate::terminal::cli_agent_sessions::{
    CLIAgentSession, CLIAgentSessionStatus, CLIAgentSessionsModel,
};
use crate::workspace::{NotificationOrigin, PaneViewLocator, WorkspaceRegistry};

const SHOW_ACTION: &str = "status_item:show";
const QUIT_ACTION: &str = "status_item:quit";
const FOCUS_SESSION_ACTION: &str = "status_item:focus_session";
const ICON_ASSET: &str = "bundled/png/blue.png";
const APP_NAME: &str = "Spirit";
const SESSION_TITLE_MAX_CHARS: usize = 40;

pub struct InstalledStatusItem {
    entries: Vec<StatusItemEntry>,
}

impl Entity for InstalledStatusItem {
    type Event = ();
}

impl SingletonEntity for InstalledStatusItem {}

struct SessionTarget {
    window_id: WindowId,
    project_id: Option<ProjectId>,
    workspace_name: String,
    locator: PaneViewLocator,
}

struct SessionEntry {
    rank: u8,
    label: String,
    terminal_view_id: EntityId,
}

pub fn init(app: &mut AppContext) {
    app.add_singleton_model(|_| InstalledStatusItem {
        entries: Vec::new(),
    });
    app.add_global_action(SHOW_ACTION, show);
    app.add_global_action(QUIT_ACTION, quit);
    app.add_global_action(FOCUS_SESSION_ACTION, focus_session);
}

pub fn install(ctx: &mut AppContext) {
    ctx.subscribe_to_model(&CLIAgentSessionsModel::handle(ctx), |_, _, ctx| {
        refresh(ctx);
    });
    refresh(ctx);
}

fn refresh(ctx: &mut AppContext) {
    let entries = menu_entries(ctx);
    let changed = InstalledStatusItem::handle(ctx).update(ctx, |installed, _| {
        if installed.entries == entries {
            return false;
        }
        installed.entries = entries.clone();
        true
    });
    if !changed {
        return;
    }
    let Ok(icon_png) = ASSETS.get(ICON_ASSET) else {
        return;
    };
    ctx.set_status_item(Some(StatusItem {
        tooltip: APP_NAME.to_owned(),
        icon_png,
        entries,
    }));
}

fn menu_entries(app: &AppContext) -> Vec<StatusItemEntry> {
    let mut entries = vec![action("Open Spirit", SHOW_ACTION, String::new())];
    let sessions = agent_session_entries(app);
    if !sessions.is_empty() {
        entries.push(StatusItemEntry::Separator);
        entries.extend(sessions);
    }
    entries.push(StatusItemEntry::Separator);
    entries.push(action("Quit Spirit", QUIT_ACTION, String::new()));
    entries
}

fn action(label: &str, action: &'static str, argument: String) -> StatusItemEntry {
    StatusItemEntry::Action {
        label: label.to_owned(),
        action,
        argument,
    }
}

fn agent_session_entries(app: &AppContext) -> Vec<StatusItemEntry> {
    let mut entries: Vec<SessionEntry> = CLIAgentSessionsModel::as_ref(app)
        .sessions()
        .map(|(terminal_view_id, session)| SessionEntry {
            rank: status_rank(&session.status),
            label: session_label(session, &session_location(terminal_view_id, session, app)),
            terminal_view_id,
        })
        .collect();
    entries.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| left.label.cmp(&right.label))
    });
    entries
        .into_iter()
        .map(|entry| {
            action(
                &entry.label,
                FOCUS_SESSION_ACTION,
                entry.terminal_view_id.to_string(),
            )
        })
        .collect()
}

fn session_location(
    terminal_view_id: EntityId,
    session: &CLIAgentSession,
    app: &AppContext,
) -> String {
    if let Some(target) = resolve_session(terminal_view_id, app) {
        return target.workspace_name;
    }
    let context = &session.session_context;
    context
        .project
        .clone()
        .or_else(|| {
            context
                .cwd
                .as_deref()
                .and_then(|cwd| Path::new(cwd).file_name())
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| APP_NAME.to_owned())
}

fn session_label(session: &CLIAgentSession, location: &str) -> String {
    let mut label = format!(
        "{} · {location} — {}",
        session.agent.display_name(),
        status_text(&session.status)
    );
    if let Some(title) = session.session_context.display_title() {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        label.push_str(": ");
        label.push_str(&truncate(&title, SESSION_TITLE_MAX_CHARS));
    }
    label
}

fn status_text(status: &CLIAgentSessionStatus) -> &'static str {
    match status {
        CLIAgentSessionStatus::InProgress => "working",
        CLIAgentSessionStatus::Blocked { .. } => "needs input",
        CLIAgentSessionStatus::Failed { .. } => "failed",
        CLIAgentSessionStatus::Idle => "idle",
        CLIAgentSessionStatus::Success => "done",
        CLIAgentSessionStatus::Cancelled => "cancelled",
    }
}

fn status_rank(status: &CLIAgentSessionStatus) -> u8 {
    match status {
        CLIAgentSessionStatus::Blocked { .. } | CLIAgentSessionStatus::Failed { .. } => 0,
        CLIAgentSessionStatus::InProgress => 1,
        CLIAgentSessionStatus::Idle
        | CLIAgentSessionStatus::Success
        | CLIAgentSessionStatus::Cancelled => 2,
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    truncated.push('\u{2026}');
    truncated
}

fn resolve_session(terminal_view_id: EntityId, app: &AppContext) -> Option<SessionTarget> {
    WorkspaceRegistry::as_ref(app)
        .all_workspaces(app)
        .into_iter()
        .find_map(|(window_id, workspace)| {
            let workspace = workspace.as_ref(app);
            workspace.tabs.iter().find_map(|tab| {
                let pane_id = tab
                    .pane_group
                    .as_ref(app)
                    .pane_id_for_terminal_view(terminal_view_id, app)?;
                Some(SessionTarget {
                    window_id,
                    project_id: workspace.project_id(),
                    workspace_name: workspace.workspace_switcher_label(app),
                    locator: PaneViewLocator {
                        pane_group_id: tab.pane_group.id(),
                        pane_id,
                    },
                })
            })
        })
}

fn normal_window(ctx: &AppContext) -> Option<WindowId> {
    let quake_window_id = quake_mode_window_id();
    ctx.window_ids()
        .find(|window_id| Some(*window_id) != quake_window_id)
}

fn show(_: &String, ctx: &mut AppContext) {
    if let Some(window_id) = normal_window(ctx) {
        ctx.windows().show_window_and_focus_app(window_id);
        return;
    }
    open_new_or_restore_session(ctx);
    if let Some(window_id) = normal_window(ctx) {
        ctx.windows().show_window_and_focus_app(window_id);
    }
    ctx.dispatch_global_action("workspace:save_app", &());
}

fn focus_session(argument: &String, ctx: &mut AppContext) {
    let Some(terminal_view_id) = argument.parse().ok().map(EntityId::from_usize) else {
        return;
    };
    let target = resolve_session(terminal_view_id, ctx).or_else(|| {
        if normal_window(ctx).is_none() {
            open_new_or_restore_session(ctx);
        }
        resolve_session(terminal_view_id, ctx)
    });
    let Some(target) = target else {
        show(argument, ctx);
        return;
    };
    ctx.windows().show_window_and_focus_app(target.window_id);
    let Some(root_view_id) = ctx.root_view_id(target.window_id) else {
        return;
    };
    ctx.dispatch_action(
        target.window_id,
        &[root_view_id],
        "root_view:handle_notification_click",
        &NotificationOrigin {
            project_id: target.project_id,
            locator: target.locator,
        },
        log::Level::Info,
    );
}

fn quit(_: &String, ctx: &mut AppContext) {
    ctx.terminate_app(TerminationMode::Cancellable, None);
}
