use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use remote_control::limits::MAX_DIRECTORY_ENTRIES;
use remote_control::protocol::{ClonePhase, CommandError, ErrorCode};
use serde::Deserialize;
use serde_json::{Value, json};
use warp_core::features::FeatureFlag;
use warpui::{AppContext, ModelContext, SingletonEntity as _, WindowId};

use super::bridge::{ClientId, CommandOutcome, RemoteControlBridge};
use super::commands::clone_phase_name;
use super::resolve;
use super::worktree_ops::resolve_deferred_command;
use crate::projects::host::CloneRequest;
use crate::projects::registry::ProjectRegistryModel;
use crate::projects::{ProjectKind, git_ops};
use crate::workspace::WorkspaceRegistry;

type CloneUpdate = (ClonePhase, Option<u8>, Option<String>);

thread_local! {
    static CLONE_JOBS: RefCell<HashMap<String, Arc<AtomicBool>>> = RefCell::default();
}

#[derive(Deserialize)]
struct RegisterProject {
    path: String,
}

#[derive(Deserialize)]
struct CloneProject {
    url: String,
    parent: String,
    #[serde(default)]
    directory_name: Option<String>,
}

#[derive(Deserialize)]
struct CancelClone {
    job_id: String,
}

#[derive(Deserialize)]
struct CreateProject {
    name: String,
    parent: String,
}

#[derive(Deserialize)]
struct RenameProject {
    project_id: String,
    name: String,
}

#[derive(Deserialize)]
struct RemoveProject {
    project_id: String,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
struct ProjectRef {
    project_id: String,
}

#[derive(Deserialize)]
struct ListDirs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include_hidden: bool,
}

fn params<T: for<'de> Deserialize<'de>>(raw: Value) -> Result<T, CommandError> {
    serde_json::from_value(raw)
        .map_err(|error| CommandError::invalid_request(format!("bad parameters: {error}")))
}

fn fail(error: CommandError) -> CommandOutcome {
    CommandOutcome::Immediate(Err(error))
}

fn empty() -> CommandOutcome {
    CommandOutcome::Immediate(Ok(json!({})))
}

fn git_failed(message: &str, summary: String) -> CommandError {
    CommandError::new(ErrorCode::GitFailed, message)
        .with_details(json!({ "error_summary": summary }))
}

fn ensure_enabled() -> Result<(), CommandError> {
    if FeatureFlag::AdeWorkspaces.is_enabled() {
        Ok(())
    } else {
        Err(CommandError::new(
            ErrorCode::FeatureDisabled,
            "Workspaces are not enabled in this build",
        ))
    }
}

fn send_phase(
    bridge: &RemoteControlBridge,
    client_id: ClientId,
    job_id: String,
    phase: ClonePhase,
    percent: Option<u8>,
    message: Option<String>,
) {
    let phase_name = clone_phase_name(phase);
    log::debug!("remote clone {job_id} is {phase_name}");
    bridge.send_clone_progress(client_id, job_id, phase, percent, message);
}

fn with_bridge(
    ctx: &mut AppContext,
    run: impl FnOnce(&mut RemoteControlBridge, &mut ModelContext<RemoteControlBridge>),
) {
    RemoteControlBridge::handle(ctx).update(ctx, run);
}

fn project_payload(project_id: crate::projects::ProjectId) -> Value {
    json!({ "project_id": project_id.to_string() })
}

pub(crate) fn register(
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: RegisterProject = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = ensure_enabled() {
        return fail(error);
    }
    let path = PathBuf::from(request.path);
    ctx.spawn(
        async move {
            let resolved = dunce::canonicalize(&path).unwrap_or(path);
            resolved.is_dir().then_some(resolved)
        },
        move |bridge: &mut RemoteControlBridge, resolved, ctx| {
            let Some(path) = resolved else {
                bridge.resolve_deferred(
                    client_id,
                    command_id,
                    Err(CommandError::not_found("that folder")),
                );
                return;
            };
            let host = resolve::default_window(ctx).and_then(|window_id| {
                let _ = window_id;
                resolve::project_host(window_id, ctx)
            });
            let host = match host {
                Ok(host) => host,
                Err(error) => {
                    bridge.resolve_deferred(client_id, command_id, Err(error));
                    return;
                }
            };
            host.update(ctx, move |host, ctx| {
                host.register_folder(
                    path,
                    Some(Box::new(move |_, project_id, ctx| {
                        resolve_deferred_command(
                            client_id,
                            command_id,
                            Ok(project_payload(project_id)),
                            ctx,
                        );
                    })),
                    ctx,
                );
            });
            bridge.mark_dirty(ctx);
        },
    );
    CommandOutcome::Deferred
}

pub(crate) fn clone(
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: CloneProject = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = ensure_enabled() {
        return fail(error);
    }
    let directory_name = request
        .directory_name
        .filter(|name| !name.trim().is_empty())
        .or_else(|| git_ops::derive_clone_directory_name(&request.url));
    let Some(directory_name) = directory_name else {
        return fail(CommandError::invalid_request(
            "no directory name could be derived from that URL",
        ));
    };
    let window_id = match resolve::default_window(ctx) {
        Ok(window_id) => window_id,
        Err(error) => return fail(error),
    };
    let host = match resolve::project_host(window_id, ctx) {
        Ok(host) => host,
        Err(error) => return fail(error),
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    CLONE_JOBS.with_borrow_mut(|jobs| jobs.insert(command_id.clone(), cancelled.clone()));

    let (progress_sender, progress_receiver) = futures::channel::mpsc::unbounded::<CloneUpdate>();
    let _ = progress_sender.unbounded_send((ClonePhase::Starting, None, None));
    let stream_job_id = command_id.clone();
    ctx.spawn_stream_local(
        progress_receiver,
        move |bridge: &mut RemoteControlBridge, (phase, percent, message), _| {
            send_phase(
                bridge,
                client_id,
                stream_job_id.clone(),
                phase,
                percent,
                message,
            );
        },
        |_, _| {},
    );

    let job_id = command_id.clone();
    let cancel_flag = cancelled.clone();
    let parent = PathBuf::from(request.parent);
    host.update(ctx, move |host, ctx| {
        host.clone_project(
            CloneRequest {
                url: request.url,
                parent,
                directory_name,
                cancelled,
            },
            Box::new(move |update| {
                let _ = progress_sender.unbounded_send((
                    ClonePhase::Cloning,
                    update.percent,
                    Some(update.phase.label().to_owned()),
                ));
            }),
            Box::new(move |host, result, ctx| {
                CLONE_JOBS.with_borrow_mut(|jobs| jobs.remove(&job_id));
                match result {
                    Ok(root) => {
                        with_bridge(ctx, |bridge, _| {
                            send_phase(
                                bridge,
                                client_id,
                                job_id.clone(),
                                ClonePhase::Registering,
                                None,
                                None,
                            );
                        });
                        host.register_folder(
                            root,
                            Some(Box::new(move |_, project_id, ctx| {
                                with_bridge(ctx, |bridge, _| {
                                    send_phase(
                                        bridge,
                                        client_id,
                                        job_id,
                                        ClonePhase::Done,
                                        Some(100),
                                        None,
                                    );
                                });
                                resolve_deferred_command(
                                    client_id,
                                    command_id,
                                    Ok(project_payload(project_id)),
                                    ctx,
                                );
                            })),
                            ctx,
                        );
                    }
                    Err(summary) => {
                        let was_cancelled = cancel_flag.load(Ordering::Relaxed);
                        let (phase, error) = if was_cancelled {
                            (
                                ClonePhase::Cancelled,
                                CommandError::new(ErrorCode::Conflict, "Clone cancelled"),
                            )
                        } else {
                            (
                                ClonePhase::Failed,
                                git_failed("the repository could not be cloned", summary),
                            )
                        };
                        with_bridge(ctx, |bridge, _| {
                            send_phase(bridge, client_id, job_id, phase, None, None);
                        });
                        resolve_deferred_command(client_id, command_id, Err(error), ctx);
                    }
                }
            }),
            ctx,
        );
    });
    CommandOutcome::Deferred
}

pub(crate) fn clone_cancel(
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let _ = ctx;
    let request: CancelClone = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    let found = CLONE_JOBS.with_borrow(|jobs| {
        jobs.get(&request.job_id)
            .map(|cancelled| cancelled.store(true, Ordering::Relaxed))
            .is_some()
    });
    if found {
        empty()
    } else {
        fail(CommandError::not_found("that clone"))
    }
}

pub(crate) fn create(
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: CreateProject = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = ensure_enabled() {
        return fail(error);
    }
    let name = request.name.trim().to_owned();
    if name.is_empty() {
        return fail(CommandError::invalid_request("that name is empty"));
    }
    let window_id = match resolve::default_window(ctx) {
        Ok(window_id) => window_id,
        Err(error) => return fail(error),
    };
    let host = match resolve::project_host(window_id, ctx) {
        Ok(host) => host,
        Err(error) => return fail(error),
    };
    let parent = PathBuf::from(request.parent);
    host.update(ctx, move |host, ctx| {
        host.create_project(
            name,
            parent,
            Box::new(move |host, result, ctx| match result {
                Ok((root, branch)) => {
                    let project_id =
                        host.finish_registering_folder((root, ProjectKind::Git, branch), ctx);
                    resolve_deferred_command(
                        client_id,
                        command_id,
                        Ok(project_payload(project_id)),
                        ctx,
                    );
                }
                Err(summary) => {
                    resolve_deferred_command(
                        client_id,
                        command_id,
                        Err(git_failed("the Workspace could not be created", summary)),
                        ctx,
                    );
                }
            }),
            ctx,
        );
    });
    CommandOutcome::Deferred
}

pub(crate) fn rename(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let request: RenameProject = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = ensure_enabled() {
        return fail(error);
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
    let name = request.name.trim().to_owned();
    if name.is_empty() {
        return fail(CommandError::invalid_request("that name is empty"));
    }
    ProjectRegistryModel::handle(ctx).update(ctx, |registry, ctx| {
        registry.rename_project(project_id, name, ctx)
    });
    empty()
}

pub(crate) fn remove(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let request: RemoveProject = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = ensure_enabled() {
        return fail(error);
    }
    if !request.confirm {
        return fail(CommandError::invalid_request(
            "removing a Workspace must be confirmed",
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

    let mut windows: Vec<WindowId> = WorkspaceRegistry::as_ref(ctx)
        .all_workspaces(ctx)
        .into_iter()
        .filter(|(_, workspace)| workspace.as_ref(ctx).project_id() == Some(project_id))
        .map(|(window_id, _)| window_id)
        .collect();
    windows.sort();
    windows.dedup();

    let primary = match windows.first().copied() {
        Some(window_id) => window_id,
        None => match resolve::default_window(ctx) {
            Ok(window_id) => window_id,
            Err(error) => return fail(error),
        },
    };
    for window_id in windows
        .into_iter()
        .filter(|window_id| *window_id != primary)
    {
        if let Ok(host) = resolve::project_host(window_id, ctx) {
            host.update(ctx, |host, ctx| host.close_project_screen(project_id, ctx));
        }
    }
    let host = match resolve::project_host(primary, ctx) {
        Ok(host) => host,
        Err(error) => return fail(error),
    };
    host.update(ctx, |host, ctx| host.remove_project(project_id, ctx));
    empty()
}

pub(crate) fn reveal(raw: Value, ctx: &mut ModelContext<RemoteControlBridge>) -> CommandOutcome {
    let request: ProjectRef = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = ensure_enabled() {
        return fail(error);
    }
    let project_id = match resolve::parse_project_id(&request.project_id) {
        Ok(project_id) => project_id,
        Err(error) => return fail(error),
    };
    let root_path = ProjectRegistryModel::as_ref(ctx)
        .project(project_id)
        .map(|project| project.root_path.clone());
    let Some(root_path) = root_path else {
        return fail(CommandError::not_found("that Workspace"));
    };
    ctx.open_file_path_in_explorer(&root_path);
    empty()
}

pub(crate) fn list_dirs(
    client_id: ClientId,
    command_id: String,
    raw: Value,
    ctx: &mut ModelContext<RemoteControlBridge>,
) -> CommandOutcome {
    let request: ListDirs = match params(raw) {
        Ok(request) => request,
        Err(error) => return fail(error),
    };
    if let Err(error) = ensure_enabled() {
        return fail(error);
    }
    let requested = request.path.map(PathBuf::from).or_else(dirs::home_dir);
    let Some(requested) = requested else {
        return fail(CommandError::not_found("a home folder"));
    };
    let include_hidden = request.include_hidden;
    ctx.spawn(
        async move { list_directory(requested, include_hidden) },
        move |bridge: &mut RemoteControlBridge, payload, ctx| {
            bridge.resolve_deferred(client_id, command_id, payload);
            bridge.mark_dirty(ctx);
        },
    );
    CommandOutcome::Deferred
}

fn list_directory(path: PathBuf, include_hidden: bool) -> Result<Value, CommandError> {
    let path = dunce::canonicalize(&path).unwrap_or(path);
    if !path.is_dir() {
        return Err(CommandError::not_found("that folder"));
    }
    let reader = std::fs::read_dir(&path).map_err(|error| {
        CommandError::new(
            ErrorCode::NotFound,
            format!("that folder could not be read: {error}"),
        )
    })?;

    let mut entries: Vec<(String, PathBuf, bool)> = Vec::new();
    for entry in reader.flatten() {
        if entries.len() >= MAX_DIRECTORY_ENTRIES as usize {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }
        let is_git_repo = child.join(".git").exists();
        entries.push((name, child, is_git_repo));
    }
    entries.sort_by(|left, right| {
        left.0
            .to_lowercase()
            .cmp(&right.0.to_lowercase())
            .then_with(|| left.0.cmp(&right.0))
    });

    let entries: Vec<Value> = entries
        .into_iter()
        .map(|(name, child, is_git_repo)| {
            json!({
                "name": name,
                "path": child.to_string_lossy(),
                "is_git_repo": is_git_repo,
            })
        })
        .collect();
    Ok(json!({
        "path": path.to_string_lossy(),
        "parent": path.parent().map(|parent| parent.to_string_lossy().into_owned()),
        "entries": entries,
    }))
}
