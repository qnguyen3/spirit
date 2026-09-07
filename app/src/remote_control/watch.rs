use warpui::{EntityId, ModelContext, SingletonEntity as _};

use super::bridge::RemoteControlBridge;
use crate::projects::registry::ProjectRegistryModel;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::workspace::WorkspaceRegistry;

pub(crate) fn install_watchers(ctx: &mut ModelContext<RemoteControlBridge>) {
    let registry = WorkspaceRegistry::handle(ctx);
    ctx.observe(&registry, |bridge, _, ctx| bridge.mark_dirty(ctx));

    let projects = ProjectRegistryModel::handle(ctx);
    ctx.subscribe_to_model(&projects, |bridge, _, _, ctx| bridge.mark_dirty(ctx));
    ctx.observe(&projects, |bridge, _, ctx| bridge.mark_dirty(ctx));

    let sessions = CLIAgentSessionsModel::handle(ctx);
    ctx.subscribe_to_model(&sessions, |bridge, _, _, ctx| bridge.mark_dirty(ctx));
    ctx.observe(&sessions, |bridge, _, ctx| bridge.mark_dirty(ctx));
}

pub(crate) fn ensure_entity_watchers(
    bridge: &mut RemoteControlBridge,
    ctx: &mut ModelContext<RemoteControlBridge>,
) {
    let workspaces = WorkspaceRegistry::as_ref(ctx).all_workspaces(ctx);
    let mut live_workspaces = Vec::with_capacity(workspaces.len());
    let mut live_terminals = Vec::new();

    for (_, workspace) in &workspaces {
        let workspace_id = workspace.id();
        live_workspaces.push(workspace_id);
        let is_new = !bridge.watched_workspaces().contains(&workspace_id);
        if is_new {
            let handle = workspace.clone();
            ctx.subscribe_to_view(&handle, |bridge, _, _, ctx| bridge.mark_dirty(ctx));
            bridge.watched_workspaces().insert(workspace_id);
        }

        let pane_groups: Vec<_> = workspace
            .as_ref(ctx)
            .tabs
            .iter()
            .map(|tab| tab.pane_group.clone())
            .collect();
        for pane_group in pane_groups {
            let focus_state = pane_group.as_ref(ctx).focus_state_handle();
            if bridge.watched_focus_states().insert(focus_state.id()) {
                ctx.subscribe_to_model(&focus_state, |bridge, _, _, ctx| bridge.mark_dirty(ctx));
            }
            for pane_id in pane_group.as_ref(ctx).visible_pane_ids() {
                let terminal = pane_group
                    .as_ref(ctx)
                    .terminal_view_from_pane_id(pane_id, ctx);
                let Some(terminal) = terminal else {
                    continue;
                };
                live_terminals.push(terminal.id());
                if bridge.watched_terminals().insert(terminal.id()) {
                    let events = terminal.as_ref(ctx).model_event_dispatcher().clone();
                    ctx.subscribe_to_model(&events, |bridge, _, _, ctx| bridge.mark_dirty(ctx));
                }
            }
        }
    }

    prune(bridge.watched_workspaces(), &live_workspaces);
    prune(bridge.watched_terminals(), &live_terminals);
}

fn prune(watched: &mut std::collections::HashSet<EntityId>, live: &[EntityId]) {
    watched.retain(|id| live.contains(id));
}
