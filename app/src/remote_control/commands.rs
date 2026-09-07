use std::path::PathBuf;

use base64::Engine as _;
use remote_control::auth::DeviceId;
use remote_control::limits::{MAX_INPUT_FRAME_BYTES, MAX_PASTE_BYTES};
use remote_control::protocol::{
    ApprovalModeWire, ClonePhase, CommandError, CommandName, ErrorCode, TerminalSignal,
};
use serde::Deserialize;
use serde_json::{Value, json};
use warp_core::features::FeatureFlag;
use warpui::{EntityId, ModelContext, SingletonEntity as _, TypedActionView as _};

use super::bridge::{ClientId, CommandOutcome, RemoteControlBridge};
use super::resolve;
use super::sessions::SessionStore;
use super::terminal_snapshot::build_attach_snapshot;
use crate::agent_launcher::catalog::{AgentLaunchRequest, agent_catalog};
use crate::projects::registry::ProjectRegistryModel;
use crate::projects::{ProjectKind, WorktreeId};
use crate::settings::AgentApprovalMode;
use crate::workspace::WorkspaceAction;
use crate::workspace::util::{NotificationOrigin, PaneViewLocator};

type Outcome = CommandOutcome;

fn ok(data: Value) -> Outcome {
    CommandOutcome::Immediate(Ok(data))
}

fn fail(error: CommandError) -> Outcome {
    CommandOutcome::Immediate(Err(error))
}

fn empty() -> Outcome {
    ok(json!({}))
}

fn params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, CommandError> {
    serde_json::from_value(params)
        .map_err(|error| CommandError::invalid_request(format!("bad parameters: {error}")))
}

pub(crate) fn execute(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    command_id: String,
    name: CommandName,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Outcome {
    match name {
        CommandName::AppPing => ok(json!({"instance_id": bridge.instance_id()})),
        CommandName::ProjectOpen => project_open(raw, ctx),
        CommandName::HomeActivate => home_activate(raw, ctx),
        CommandName::ScreenActivate => screen_activate(raw, ctx),
        CommandName::TabActivate => tab_activate(raw, ctx),
        CommandName::PaneFocus => pane_focus(raw, ctx),
        CommandName::DesktopReveal => desktop_reveal(raw, ctx),
        CommandName::TerminalAttach => terminal_attach(bridge, client_id, raw, ctx),
        CommandName::TerminalDetach => terminal_detach(bridge, client_id, raw),
        CommandName::TerminalInput => terminal_input(raw, ctx),
        CommandName::TerminalPaste => terminal_paste(raw, ctx),
        CommandName::TerminalRunCommand => terminal_run_command(raw, ctx),
        CommandName::TerminalAgentSubmit => terminal_agent_submit(raw, ctx),
        CommandName::TerminalAgentInsert => terminal_agent_insert(raw, ctx),
        CommandName::TerminalSignal => terminal_signal(raw, ctx),
        CommandName::TerminalCreate => terminal_create(raw, ctx),
        CommandName::AgentLaunch => agent_launch(raw, ctx),
        CommandName::WorktreeCreate => super::worktree_ops::create(client_id, command_id, raw, ctx),
        CommandName::WorktreeDirtyCheck => {
            super::worktree_ops::dirty_check(client_id, command_id, raw, ctx)
        }
        CommandName::WorktreeDelete => super::worktree_ops::delete(client_id, command_id, raw, ctx),
        CommandName::WorktreeRename => worktree_rename(raw, ctx),
        CommandName::TabClose => tab_close(raw, ctx),
        CommandName::HistoryList => super::history_ops::list(raw, ctx),
        CommandName::HistoryRefresh => super::history_ops::refresh(ctx),
        CommandName::HistoryResume => super::history_ops::resume(raw, ctx),
        CommandName::ProjectRegister => {
            super::project_ops::register(client_id, command_id, raw, ctx)
        }
        CommandName::ProjectClone => super::project_ops::clone(client_id, command_id, raw, ctx),
        CommandName::ProjectCloneCancel => super::project_ops::clone_cancel(raw, ctx),
        CommandName::ProjectCreate => super::project_ops::create(client_id, command_id, raw, ctx),
        CommandName::ProjectRename => super::project_ops::rename(raw, ctx),
        CommandName::ProjectRemove => super::project_ops::remove(raw, ctx),
        CommandName::ProjectReveal => super::project_ops::reveal(raw, ctx),
        CommandName::FsListDirs => super::project_ops::list_dirs(client_id, command_id, raw, ctx),
        CommandName::DevicesList => devices_list(bridge, client_id),
        CommandName::DevicesRevoke => devices_revoke(bridge, raw),
        CommandName::DevicesRename => devices_rename(bridge, raw),
    }
}

#[derive(Deserialize)]
struct ProjectRef {
    project_id: String,
}

#[derive(Deserialize)]
struct WindowRef {
    #[serde(default)]
    window_id: Option<String>,
}

#[derive(Deserialize)]
struct ScreenRef {
    screen_id: String,
}

#[derive(Deserialize)]
struct TabRef {
    tab_id: String,
}

#[derive(Deserialize)]
struct PaneRef {
    pane_id: String,
}

#[derive(Deserialize)]
struct TerminalRef {
    terminal_id: String,
}

#[derive(Deserialize)]
struct TerminalBytes {
    terminal_id: String,
    bytes: String,
}

#[derive(Deserialize)]
struct TerminalText {
    terminal_id: String,
    text: String,
}

#[derive(Deserialize)]
struct TerminalSignalParams {
    terminal_id: String,
    signal: TerminalSignal,
}

#[derive(Deserialize)]
struct AttachRef {
    attach_id: u32,
}

#[derive(Deserialize)]
struct CreateTerminal {
    screen_id: String,
    #[serde(default)]
    worktree_id: Option<String>,
}

#[derive(Deserialize)]
struct LaunchAgent {
    screen_id: String,
    #[serde(default)]
    worktree_id: Option<String>,
    catalog_index: usize,
    approval_mode: ApprovalModeWire,
}

#[derive(Deserialize)]
struct RenameWorktree {
    worktree_id: String,
    name: String,
}

#[derive(Deserialize)]
struct DeviceRef {
    device_id: String,
}

#[derive(Deserialize)]
struct RenameDevice {
    device_id: String,
    label: String,
}

#[derive(Deserialize)]
struct CloseTab {
    tab_id: String,
    #[serde(default)]
    confirm: bool,
}

pub(crate) fn approval_mode(mode: ApprovalModeWire) -> AgentApprovalMode {
    match mode {
        ApprovalModeWire::Yolo => AgentApprovalMode::Yolo,
        ApprovalModeWire::Normal => AgentApprovalMode::Normal,
    }
}

fn project_open(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: ProjectRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if !FeatureFlag::AdeWorkspaces.is_enabled() {
        return fail(CommandError::new(
            ErrorCode::FeatureDisabled,
            "Workspaces are not enabled in this build",
        ));
    }
    let project_id = match resolve::parse_project_id(&request.project_id) {
        Ok(project_id) => project_id,
        Err(error) => return fail(error),
    };
    if ProjectRegistryModel::as_ref(ctx)
        .project(project_id)
        .is_none()
    {
        return fail(CommandError::not_found("that Workspace"));
    }

    let window_id = crate::workspace::WorkspaceRegistry::as_ref(ctx)
        .window_owning_project(project_id)
        .map(Ok)
        .unwrap_or_else(|| resolve::default_window(ctx));
    let window_id = match window_id {
        Ok(window_id) => window_id,
        Err(error) => return fail(error),
    };
    let host = match resolve::project_host(window_id, ctx) {
        Ok(host) => host,
        Err(error) => return fail(error),
    };
    host.update(ctx, |host, ctx| host.open_project(project_id, ctx));

    match resolve::screen_for_project(project_id, ctx) {
        Some(target) => ok(json!({
            "screen_id": target.workspace.as_ref(ctx).screen_id().to_string(),
            "window_id": target.window_id.to_string(),
        })),
        None => fail(CommandError::not_found("that Workspace's screen")),
    }
}

fn home_activate(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: WindowRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let window_id = match request.window_id {
        Some(raw) => {
            let wanted = raw.clone();
            let found = crate::workspace::WorkspaceRegistry::as_ref(ctx)
                .all_workspaces(ctx)
                .into_iter()
                .map(|(window_id, _)| window_id)
                .find(|window_id| window_id.to_string() == wanted);
            match found {
                Some(window_id) => window_id,
                None => return fail(CommandError::not_found("that window")),
            }
        }
        None => match resolve::default_window(ctx) {
            Ok(window_id) => window_id,
            Err(error) => return fail(error),
        },
    };
    let host = match resolve::project_host(window_id, ctx) {
        Ok(host) => host,
        Err(error) => return fail(error),
    };
    host.update(ctx, |host, ctx| host.activate_home(ctx));

    let screen_id = crate::workspace::WorkspaceRegistry::as_ref(ctx)
        .active_workspace_view_id(window_id)
        .map(|id| id.to_string());
    ok(json!({ "screen_id": screen_id }))
}

fn screen_activate(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: ScreenRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    match activate_screen(&request.screen_id, ctx) {
        Ok(()) => empty(),
        Err(error) => fail(error),
    }
}

fn activate_screen(
    screen_id: &str,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Result<(), CommandError> {
    let target = resolve::screen(screen_id, ctx)?;
    let wanted = target.workspace.id();
    if crate::workspace::WorkspaceRegistry::as_ref(ctx).active_workspace_view_id(target.window_id)
        == Some(wanted)
    {
        return Ok(());
    }
    let index = resolve::screen_index(target.window_id, wanted, ctx)?;
    let host = resolve::project_host(target.window_id, ctx)?;
    host.update(ctx, |host, ctx| host.activate_screen(index, ctx));
    Ok(())
}

fn tab_activate(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TabRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let (target, index) = match resolve::tab(&request.tab_id, ctx) {
        Ok(found) => found,
        Err(error) => return fail(error),
    };
    let screen_id = target.workspace.as_ref(ctx).screen_id().to_string();
    if let Err(error) = activate_screen(&screen_id, ctx) {
        return fail(error);
    }
    target.workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(&WorkspaceAction::ActivateTab(index), ctx);
    });
    empty()
}

fn pane_focus(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: PaneRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let found = match resolve::pane(&request.pane_id, ctx) {
        Ok(found) => found,
        Err(error) => return fail(error),
    };
    let locator = PaneViewLocator {
        pane_group_id: found.pane_group.id(),
        pane_id: found.pane_id,
    };
    found
        .workspace
        .update(ctx, |workspace, ctx| workspace.focus_pane(locator, ctx));
    empty()
}

fn desktop_reveal(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TerminalRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let target = match resolve::terminal(&request.terminal_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    let project_id = target.workspace.as_ref(ctx).project_id();
    let locator = PaneViewLocator {
        pane_group_id: target.pane_group.id(),
        pane_id: target.pane_id,
    };
    ctx.windows().show_window_and_focus_app(target.window_id);
    let Some(root_view_id) = ctx.root_view_id(target.window_id) else {
        return fail(CommandError::not_found("that window"));
    };
    ctx.dispatch_action(
        target.window_id,
        &[root_view_id],
        "root_view:handle_notification_click",
        &NotificationOrigin {
            project_id,
            locator,
        },
        log::Level::Info,
    );
    empty()
}

fn terminal_attach(
    bridge: &mut RemoteControlBridge,
    client_id: ClientId,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Outcome {
    let request: TerminalRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let target = match resolve::terminal(&request.terminal_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    if bridge.streams().count_for_client(client_id)
        >= remote_control::limits::MAX_ATTACHMENTS_PER_CLIENT as usize
    {
        return fail(CommandError::new(
            ErrorCode::TooManyAttachments,
            "this device already mirrors the maximum number of terminals",
        ));
    }
    let snapshot = build_attach_snapshot(&target.terminal, ctx);
    let Some(sender) = bridge.stream_sender(client_id) else {
        return fail(CommandError::new(
            ErrorCode::BridgeUnavailable,
            "this connection is closing",
        ));
    };
    let attach_id = bridge
        .streams()
        .attach(client_id, &target.terminal, sender, ctx);
    ok(json!({
        "attach_id": attach_id.0,
        "cols": snapshot.cols,
        "rows": snapshot.rows,
        "mode": snapshot.mode,
        "snapshot": snapshot.encoded(),
    }))
}

fn terminal_detach(bridge: &mut RemoteControlBridge, client_id: ClientId, raw: Value) -> Outcome {
    let request: AttachRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if bridge.streams().detach(request.attach_id, client_id) {
        empty()
    } else {
        fail(CommandError::not_found("that attachment"))
    }
}

fn decode_input(encoded: &str, limit: u64) -> Result<Vec<u8>, CommandError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| CommandError::invalid_request("bytes must be base64"))?;
    if bytes.len() as u64 > limit {
        return Err(CommandError::new(
            ErrorCode::PayloadTooLarge,
            "that input is too large",
        ));
    }
    Ok(bytes)
}

pub(crate) fn write_bytes_to_terminal(
    terminal_id: &str,
    bytes: Vec<u8>,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Result<(), CommandError> {
    let target = resolve::terminal(terminal_id, ctx)?;
    let read_only = target.terminal.as_ref(ctx).model.lock().is_read_only();
    if read_only {
        return Err(CommandError::new(
            ErrorCode::TerminalReadOnly,
            "that terminal has exited",
        ));
    }
    target.terminal.update(ctx, |terminal, ctx| {
        terminal.write_viewer_bytes_to_pty(bytes, ctx)
    });
    Ok(())
}

fn terminal_input(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TerminalBytes = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let bytes = match decode_input(&request.bytes, MAX_INPUT_FRAME_BYTES) {
        Ok(bytes) => bytes,
        Err(error) => return fail(error),
    };
    match write_bytes_to_terminal(&request.terminal_id, bytes, ctx) {
        Ok(()) => empty(),
        Err(error) => fail(error),
    }
}

fn terminal_paste(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TerminalText = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if request.text.len() as u64 > MAX_PASTE_BYTES {
        return fail(CommandError::new(
            ErrorCode::PayloadTooLarge,
            "that paste is too large",
        ));
    }
    let target = match resolve::terminal(&request.terminal_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    let normalized = request.text.replace("\r\n", "\r").replace('\n', "\r");
    let (at_prompt, bracketed) = {
        let view = target.terminal.as_ref(ctx);
        let mut model = view.model.lock();
        let bracketed = model.needs_bracketed_paste();
        let at_prompt = view.is_input_box_visible(&model, ctx);
        (at_prompt, bracketed)
    };
    if at_prompt {
        target.terminal.update(ctx, |terminal, ctx| {
            terminal.insert_text_into_input(&normalized, ctx)
        });
        return empty();
    }
    let payload = if bracketed {
        let mut wrapped = b"\x1b[200~".to_vec();
        wrapped.extend_from_slice(normalized.as_bytes());
        wrapped.extend_from_slice(b"\x1b[201~");
        wrapped
    } else {
        normalized.into_bytes()
    };
    match write_bytes_to_terminal(&request.terminal_id, payload, ctx) {
        Ok(()) => empty(),
        Err(error) => fail(error),
    }
}

fn terminal_run_command(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TerminalText = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let target = match resolve::terminal(&request.terminal_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    let at_prompt = {
        let view = target.terminal.as_ref(ctx);
        let model = view.model.lock();
        !model.is_alt_screen_active() && view.is_input_box_visible(&model, ctx)
    };
    if !at_prompt {
        return fail(CommandError::new(
            ErrorCode::NotAtPrompt,
            "a program is running in that terminal",
        ));
    }
    target.terminal.update(ctx, |terminal, ctx| {
        terminal.execute_command_or_set_pending(&request.text, ctx)
    });
    empty()
}

fn agent_session_open(
    terminal_id: EntityId,
    ctx: &ModelContext<RemoteControlBridge>,
) -> Result<(), CommandError> {
    let sessions = crate::terminal::cli_agent_sessions::CLIAgentSessionsModel::as_ref(ctx);
    if sessions.session(terminal_id).is_some() {
        Ok(())
    } else {
        Err(CommandError::new(
            ErrorCode::NoAgentSession,
            "no CLI agent is running in that terminal",
        ))
    }
}

fn terminal_agent_submit(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TerminalText = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if request.text.len() as u64 > MAX_PASTE_BYTES {
        return fail(CommandError::new(
            ErrorCode::PayloadTooLarge,
            "that message is too large",
        ));
    }
    let target = match resolve::terminal(&request.terminal_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    if let Err(error) = agent_session_open(target.terminal.id(), ctx) {
        return fail(error);
    }
    target.terminal.update(ctx, |terminal, ctx| {
        terminal.submit_cli_agent_rich_input(request.text.clone(), ctx)
    });
    empty()
}

fn terminal_agent_insert(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TerminalText = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let target = match resolve::terminal(&request.terminal_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    if let Err(error) = agent_session_open(target.terminal.id(), ctx) {
        return fail(error);
    }
    target.terminal.update(ctx, |terminal, ctx| {
        terminal.insert_text_into_cli_agent_input(&request.text, ctx)
    });
    empty()
}

fn terminal_signal(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: TerminalSignalParams = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    match write_bytes_to_terminal(&request.terminal_id, vec![request.signal.byte()], ctx) {
        Ok(()) => empty(),
        Err(error) => fail(error),
    }
}

fn created_tab(
    before: &[String],
    ctx: &mut ModelContext<RemoteControlBridge>,
    screen_id: &str,
) -> Option<(String, Option<String>)> {
    let target = resolve::screen(screen_id, ctx).ok()?;
    let workspace = target.workspace.as_ref(ctx);
    let tab = workspace
        .tabs
        .iter()
        .find(|tab| !before.contains(&tab.pane_group.id().to_string()))
        .or_else(|| workspace.tabs.get(workspace.active_tab_index()))?;
    let tab_id = tab.pane_group.id().to_string();
    let pane_group = tab.pane_group.clone();
    let group = pane_group.as_ref(ctx);
    let terminal_id = group
        .visible_pane_ids()
        .into_iter()
        .find_map(|pane_id| group.terminal_view_from_pane_id(pane_id, ctx))
        .map(|view| view.id().to_string());
    Some((tab_id, terminal_id))
}

fn tab_ids(screen_id: &str, ctx: &mut ModelContext<RemoteControlBridge>) -> Vec<String> {
    resolve::screen(screen_id, ctx)
        .map(|target| {
            target
                .workspace
                .as_ref(ctx)
                .tabs
                .iter()
                .map(|tab| tab.pane_group.id().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn terminal_create(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: CreateTerminal = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = activate_screen(&request.screen_id, ctx) {
        return fail(error);
    }
    let target = match resolve::screen(&request.screen_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    let before = tab_ids(&request.screen_id, ctx);

    match request.worktree_id {
        Some(raw_worktree) => {
            let worktree_id = match resolve::parse_worktree_id(&raw_worktree) {
                Ok(worktree_id) => worktree_id,
                Err(error) => return fail(error),
            };
            if ProjectRegistryModel::as_ref(ctx)
                .worktree(worktree_id)
                .is_none()
            {
                return fail(CommandError::not_found("that worktree"));
            }
            target.workspace.update(ctx, |workspace, ctx| {
                workspace
                    .handle_action(&WorkspaceAction::NewTerminalInWorktree { worktree_id }, ctx);
            });
        }
        None => {
            target.workspace.update(ctx, |workspace, ctx| {
                workspace.handle_action(
                    &WorkspaceAction::AddTerminalTab {
                        hide_homepage: true,
                    },
                    ctx,
                );
            });
        }
    }

    match created_tab(&before, ctx, &request.screen_id) {
        Some((tab_id, terminal_id)) => ok(json!({
            "tab_id": tab_id,
            "terminal_id": terminal_id,
        })),
        None => fail(CommandError::new(
            ErrorCode::Conflict,
            "the terminal could not be created",
        )),
    }
}

fn agent_launch(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: LaunchAgent = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if agent_catalog().get(request.catalog_index).is_none() {
        return fail(CommandError::invalid_request("no agent at that index"));
    }
    if let Err(error) = activate_screen(&request.screen_id, ctx) {
        return fail(error);
    }
    let target = match resolve::screen(&request.screen_id, ctx) {
        Ok(target) => target,
        Err(error) => return fail(error),
    };
    let before = tab_ids(&request.screen_id, ctx);
    let launch = AgentLaunchRequest {
        catalog_index: request.catalog_index,
        approval_mode: approval_mode(request.approval_mode),
    };

    match request.worktree_id {
        Some(raw_worktree) => {
            let worktree_id = match resolve::parse_worktree_id(&raw_worktree) {
                Ok(worktree_id) => worktree_id,
                Err(error) => return fail(error),
            };
            if ProjectRegistryModel::as_ref(ctx)
                .worktree(worktree_id)
                .is_none()
            {
                return fail(CommandError::not_found("that worktree"));
            }
            target.workspace.update(ctx, |workspace, ctx| {
                workspace.add_tab_in_worktree(worktree_id, Some(launch), ctx)
            });
        }
        None => {
            target.workspace.update(ctx, |workspace, ctx| {
                workspace.handle_action(
                    &WorkspaceAction::AddTerminalTab {
                        hide_homepage: true,
                    },
                    ctx,
                );
                workspace.launch_agent_in_active_tab(launch, ctx);
            });
        }
    }

    match created_tab(&before, ctx, &request.screen_id) {
        Some((tab_id, terminal_id)) => ok(json!({
            "tab_id": tab_id,
            "terminal_id": terminal_id,
        })),
        None => fail(CommandError::new(
            ErrorCode::Conflict,
            "the agent could not be launched",
        )),
    }
}

fn worktree_rename(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: RenameWorktree = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let worktree_id = match resolve::parse_worktree_id(&request.worktree_id) {
        Ok(worktree_id) => worktree_id,
        Err(error) => return fail(error),
    };
    let project_id = match resolve::worktree_project(worktree_id, ctx) {
        Ok(project_id) => project_id,
        Err(error) => return fail(error),
    };
    let taken = ProjectRegistryModel::as_ref(ctx).worktree_names_for_project(project_id);
    let sanitized = sanitize_worktree_name(&request.name);
    if sanitized.is_empty() {
        return fail(CommandError::invalid_request("that name is empty"));
    }
    let current = ProjectRegistryModel::as_ref(ctx)
        .worktree(worktree_id)
        .map(|worktree| worktree.name.clone());
    if current.as_deref() != Some(sanitized.as_str()) && taken.contains(&sanitized) {
        return fail(CommandError::new(
            ErrorCode::Conflict,
            "another worktree already uses that name",
        ));
    }
    ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
        registry.rename_worktree(worktree_id, sanitized, ctx)
    });
    empty()
}

pub(crate) fn sanitize_worktree_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn tab_close(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> Outcome {
    let request: CloseTab = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if !request.confirm {
        return fail(CommandError::new(
            ErrorCode::Conflict,
            "closing a tab must be confirmed",
        ));
    }
    let (target, index) = match resolve::tab(&request.tab_id, ctx) {
        Ok(found) => found,
        Err(error) => return fail(error),
    };
    let closed = target.workspace.update(ctx, |workspace, ctx| {
        workspace.close_tabs([index].into_iter(), true, true, ctx)
    });
    if closed {
        empty()
    } else {
        fail(CommandError::new(
            ErrorCode::Conflict,
            "that tab refused to close",
        ))
    }
}

fn devices_store(bridge: &RemoteControlBridge) -> Result<&SessionStore, CommandError> {
    bridge.sessions().ok_or_else(|| {
        CommandError::new(
            ErrorCode::BridgeUnavailable,
            "Remote Control is not accepting connections",
        )
    })
}

fn devices_list(bridge: &RemoteControlBridge, client_id: ClientId) -> Outcome {
    let current = bridge.client_device(client_id);
    let store = match devices_store(bridge) {
        Ok(store) => store,
        Err(error) => return fail(error),
    };
    ok(json!({ "devices": store.devices(current.as_ref()) }))
}

fn devices_revoke(bridge: &mut RemoteControlBridge, raw: Value) -> Outcome {
    let request: DeviceRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let revoked = match devices_store(bridge) {
        Ok(store) => store.revoke_device(&request.device_id),
        Err(error) => return fail(error),
    };
    if !revoked {
        return fail(CommandError::not_found("that device"));
    }
    bridge.disconnect_device(&DeviceId::from_string(request.device_id));
    empty()
}

fn devices_rename(bridge: &mut RemoteControlBridge, raw: Value) -> Outcome {
    let request: RenameDevice = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let label = request.label.trim();
    if label.is_empty() {
        return fail(CommandError::invalid_request("that name is empty"));
    }
    let renamed = match devices_store(bridge) {
        Ok(store) => store.rename_device(&request.device_id, label),
        Err(error) => return fail(error),
    };
    if renamed {
        empty()
    } else {
        fail(CommandError::not_found("that device"))
    }
}

pub(crate) fn clone_phase_name(phase: ClonePhase) -> &'static str {
    match phase {
        ClonePhase::Starting => "starting",
        ClonePhase::Cloning => "cloning",
        ClonePhase::Registering => "registering",
        ClonePhase::Done => "done",
        ClonePhase::Failed => "failed",
        ClonePhase::Cancelled => "cancelled",
    }
}

pub(crate) fn ensure_git_project(
    worktree_id: WorktreeId,
    ctx: &ModelContext<RemoteControlBridge>,
) -> Result<PathBuf, CommandError> {
    let registry = ProjectRegistryModel::as_ref(ctx);
    let worktree = registry
        .worktree(worktree_id)
        .ok_or_else(|| CommandError::not_found("that worktree"))?;
    let project = registry
        .project(worktree.project_id)
        .ok_or_else(|| CommandError::not_found("that Workspace"))?;
    if !matches!(project.kind, ProjectKind::Git) {
        return Err(CommandError::new(
            ErrorCode::NotGitProject,
            "that Workspace is not a git repository",
        ));
    }
    registry
        .worktree_directory(worktree_id)
        .ok_or_else(|| CommandError::not_found("that worktree's directory"))
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
