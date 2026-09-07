use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use remote_control::protocol::{ApprovalModeWire, CommandError, ErrorCode};
use serde::Deserialize;
use serde_json::{Value, json};
use warp_core::features::FeatureFlag;
use warpui::{AppContext, ModelContext, SingletonEntity as _, ViewHandle};

use super::bridge::{ClientId, CommandOutcome, RemoteControlBridge};
use super::commands::{approval_mode, ensure_git_project, sanitize_worktree_name};
use super::resolve;
use crate::agent_launcher::catalog::{AgentLaunchRequest, agent_catalog};
use crate::projects::registry::ProjectRegistryModel;
use crate::projects::{ProjectId, ProjectKind, git_ops};
use crate::workspace::WorkspaceRegistry;
use crate::workspace::view::Workspace;
use crate::workspace::view::worktrees::WorktreeCreatedInfo;

type Slot = Rc<RefCell<Option<Result<Value, CommandError>>>>;

#[derive(Deserialize)]
struct AgentSpec {
    catalog_index: usize,
    approval_mode: ApprovalModeWire,
}

#[derive(Deserialize)]
struct CreateWorktree {
    project_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    agent: Option<AgentSpec>,
}

#[derive(Deserialize)]
struct WorktreeRef {
    worktree_id: String,
}

#[derive(Deserialize)]
struct DeleteWorktree {
    worktree_id: String,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    force: bool,
}

fn params<T: for<'de> Deserialize<'de>>(raw: Value) -> Result<T, CommandError> {
    serde_json::from_value(raw)
        .map_err(|error| CommandError::invalid_request(format!("bad parameters: {error}")))
}

fn git_failed(message: &str, summary: String) -> CommandError {
    CommandError::new(ErrorCode::GitFailed, message)
        .with_details(json!({ "error_summary": summary }))
}

pub(super) fn resolve_deferred_command(
    client_id: ClientId,
    command_id: String,
    result: Result<Value, CommandError>,
    ctx: &mut AppContext,
) {
    RemoteControlBridge::handle(ctx).update(ctx, |bridge, ctx| {
        bridge.resolve_deferred(client_id, command_id, result);
        bridge.mark_dirty(ctx);
    });
}

fn with_dirty_state<F>(
    directory: PathBuf,
    ctx: &mut ModelContext<RemoteControlBridge>,
    on_result: F,
) where
    F: 'static
        + FnOnce(
            &mut RemoteControlBridge,
            Result<bool, CommandError>,
            &mut ModelContext<RemoteControlBridge>,
        ),
{
    ctx.spawn(
        async move { directory },
        move |bridge: &mut RemoteControlBridge, directory, ctx| {
            let path_env = bridge.path_env().map(str::to_owned);
            ctx.spawn(
                async move { git_ops::status_is_dirty(&directory, path_env.as_deref()).await },
                move |bridge: &mut RemoteControlBridge, result, ctx| {
                    let state = result.map_err(|err| {
                        git_failed(
                            "git could not inspect that worktree",
                            crate::projects::error_summary(&err),
                        )
                    });
                    on_result(bridge, state, ctx);
                },
            );
        },
    );
}

fn workspace_for_project(
    project_id: ProjectId,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> Result<ViewHandle<Workspace>, CommandError> {
    if let Some(target) = resolve::screen_for_project(project_id, ctx) {
        return Ok(target.workspace);
    }
    let window_id = WorkspaceRegistry::as_ref(ctx)
        .window_owning_project(project_id)
        .map(Ok)
        .unwrap_or_else(|| resolve::default_window(ctx))?;
    let host = resolve::project_host(window_id, ctx)?;
    host.update(ctx, |host, ctx| host.open_project(project_id, ctx));
    resolve::screen_for_project(project_id, ctx)
        .map(|target| target.workspace)
        .ok_or_else(|| CommandError::not_found("that Workspace's screen"))
}

fn creation_payload(result: Result<WorktreeCreatedInfo, String>) -> Result<Value, CommandError> {
    match result {
        Ok(info) => Ok(json!({
            "worktree_id": info.worktree_id.to_string(),
            "tab_id": info.tab_id.to_string(),
            "branch": info.branch,
            "path": info.path.to_string_lossy(),
        })),
        Err(summary) => Err(git_failed("the worktree could not be created", summary)),
    }
}

pub(crate) fn create(
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: CreateWorktree = match params(raw) {
        Ok(request) => request,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    if !FeatureFlag::AdeWorkspaces.is_enabled() {
        return CommandOutcome::Immediate(Err(CommandError::new(
            ErrorCode::FeatureDisabled,
            "Workspaces are not enabled in this build",
        )));
    }
    let project_id = match resolve::parse_project_id(&request.project_id) {
        Ok(project_id) => project_id,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };

    let taken_names = {
        let registry = ProjectRegistryModel::as_ref(ctx);
        let Some(project) = registry.project(project_id) else {
            return CommandOutcome::Immediate(Err(CommandError::not_found("that Workspace")));
        };
        if !matches!(project.kind, ProjectKind::Git) || project.primary_branch.is_none() {
            return CommandOutcome::Immediate(Err(CommandError::new(
                ErrorCode::NotGitProject,
                "that Workspace is not a git repository",
            )));
        }
        registry.worktree_names_for_project(project_id)
    };

    let launch = match request.agent {
        Some(agent) => {
            if agent_catalog().get(agent.catalog_index).is_none() {
                return CommandOutcome::Immediate(Err(CommandError::invalid_request(
                    "no agent at that index",
                )));
            }
            Some(AgentLaunchRequest {
                catalog_index: agent.catalog_index,
                approval_mode: approval_mode(agent.approval_mode),
            })
        }
        None => None,
    };

    let name = request
        .name
        .map(|raw| sanitize_worktree_name(&raw))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            let existing = taken_names.iter().map(String::as_str).collect();
            warp_util::worktree_names::generate_worktree_branch_name(&existing)
        });

    let workspace = match workspace_for_project(project_id, ctx) {
        Ok(workspace) => workspace,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };

    let synchronous = Rc::new(Cell::new(true));
    let slot: Slot = Rc::new(RefCell::new(None));
    let captured_synchronous = synchronous.clone();
    let captured_slot = slot.clone();
    workspace.update(ctx, move |workspace, ctx| {
        workspace.create_worktree_headless(
            name,
            launch,
            Box::new(move |_, result, ctx| {
                let payload = creation_payload(result);
                if captured_synchronous.get() {
                    *captured_slot.borrow_mut() = Some(payload);
                } else {
                    resolve_deferred_command(client_id, command_id, payload, ctx);
                }
            }),
            ctx,
        );
    });
    synchronous.set(false);

    let answered = slot.borrow_mut().take();
    match answered {
        Some(payload) => CommandOutcome::Immediate(payload),
        None => CommandOutcome::Deferred,
    }
}

pub(crate) fn dirty_check(
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: WorktreeRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    let worktree_id = match resolve::parse_worktree_id(&request.worktree_id) {
        Ok(worktree_id) => worktree_id,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    let directory = match ensure_git_project(worktree_id, ctx) {
        Ok(directory) => directory,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };

    with_dirty_state(directory, ctx, move |bridge, state, ctx| {
        let payload = state.map(|dirty| json!({ "dirty": dirty }));
        bridge.resolve_deferred(client_id, command_id, payload);
        bridge.mark_dirty(ctx);
    });
    CommandOutcome::Deferred
}

pub(crate) fn delete(
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: DeleteWorktree = match params(raw) {
        Ok(request) => request,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    if !request.confirm {
        return CommandOutcome::Immediate(Err(CommandError::new(
            ErrorCode::Conflict,
            "deleting a worktree must be confirmed",
        )));
    }
    let worktree_id = match resolve::parse_worktree_id(&request.worktree_id) {
        Ok(worktree_id) => worktree_id,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    let is_primary = ProjectRegistryModel::as_ref(ctx)
        .worktree(worktree_id)
        .map(|worktree| worktree.is_primary());
    match is_primary {
        None => return CommandOutcome::Immediate(Err(CommandError::not_found("that worktree"))),
        Some(true) => {
            return CommandOutcome::Immediate(Err(CommandError::invalid_request(
                "the primary worktree cannot be deleted",
            )));
        }
        Some(false) => {}
    }
    let project_id = match resolve::worktree_project(worktree_id, ctx) {
        Ok(project_id) => project_id,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };
    let directory = match ensure_git_project(worktree_id, ctx) {
        Ok(directory) => directory,
        Err(error) => return CommandOutcome::Immediate(Err(error)),
    };

    let force = request.force;
    with_dirty_state(directory, ctx, move |bridge, state, ctx| {
        let dirty = match state {
            Ok(dirty) => dirty,
            Err(error) => {
                bridge.resolve_deferred(client_id, command_id, Err(error));
                return;
            }
        };
        if dirty && !force {
            bridge.resolve_deferred(
                client_id,
                command_id,
                Err(CommandError::new(
                    ErrorCode::Conflict,
                    "that worktree has uncommitted changes",
                )),
            );
            return;
        }
        let workspace = match workspace_for_project(project_id, ctx) {
            Ok(workspace) => workspace,
            Err(error) => {
                bridge.resolve_deferred(client_id, command_id, Err(error));
                return;
            }
        };
        let synchronous = Rc::new(Cell::new(true));
        let slot: Slot = Rc::new(RefCell::new(None));
        let captured_synchronous = synchronous.clone();
        let captured_slot = slot.clone();
        let deferred_command_id = command_id.clone();
        workspace.update(ctx, move |workspace, ctx| {
            workspace.delete_worktree_headless(
                worktree_id,
                force,
                Box::new(move |_, result, ctx| {
                    let payload = match result {
                        Ok(branch_kept) => Ok(json!({ "branch_kept": branch_kept })),
                        Err(summary) => {
                            Err(git_failed("the worktree could not be deleted", summary))
                        }
                    };
                    if captured_synchronous.get() {
                        *captured_slot.borrow_mut() = Some(payload);
                    } else {
                        resolve_deferred_command(client_id, deferred_command_id, payload, ctx);
                    }
                }),
                ctx,
            );
        });
        synchronous.set(false);
        let answered = slot.borrow_mut().take();
        if let Some(payload) = answered {
            bridge.resolve_deferred(client_id, command_id, payload);
        }
        bridge.mark_dirty(ctx);
    });
    CommandOutcome::Deferred
}
