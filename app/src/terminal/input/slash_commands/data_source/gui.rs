use std::collections::HashMap;
use std::path::PathBuf;

use warpui::{AppContext, Entity, EntityId, ModelContext, ModelHandle};

use super::core::subscribe_to_shared_dependencies;
use super::{
    InlineItem, SlashCommandDataSource, SlashCommandDataSourceState, UpdatedActiveCommands,
};
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::search::slash_command_menu::StaticCommand;
use crate::search::slash_command_menu::static_commands::Availability;
use crate::search::slash_command_menu::static_commands::commands::COMMAND_REGISTRY;
use crate::settings::{InputSettings, InputSettingsChangedEvent};
use crate::terminal::input::slash_commands::AcceptSlashCommandOrSavedPrompt;
use crate::terminal::model::session::active_session::ActiveSession;
use crate::workspaces::user_workspaces::TeamContextResolver;

pub struct GuiDataSourceArgs {
    pub active_session: ModelHandle<ActiveSession>,
    pub terminal_view_id: EntityId,
    /// Resolves this data source's terminal surface's window's team context. Minted by the
    /// owning view at construction via `UserWorkspaces::team_context_resolver`.
    pub team_context_resolver: TeamContextResolver,
}

pub struct GuiSlashCommandDataSource {
    state: SlashCommandDataSourceState,
}

impl GuiSlashCommandDataSource {
    pub fn new(args: GuiDataSourceArgs, ctx: &mut ModelContext<Self>) -> Self {
        let GuiDataSourceArgs {
            active_session,
            terminal_view_id,
            team_context_resolver,
        } = args;

        subscribe_to_shared_dependencies(
            &active_session,
            terminal_view_id,
            Self::recompute_active_commands,
            ctx,
        );
        ctx.subscribe_to_model(&InputSettings::handle(ctx), |me, _, event, ctx| {
            if matches!(
                event,
                InputSettingsChangedEvent::EnableSlashCommandsInTerminal { .. }
            ) {
                me.recompute_active_commands(ctx);
            }
        });

        let mut me = Self {
            state: SlashCommandDataSourceState::new(
                active_session,
                terminal_view_id,
                team_context_resolver,
            ),
        };
        me.recompute_active_commands(ctx);
        me
    }

    pub fn set_active_repo_root(
        &mut self,
        repo_root: Option<PathBuf>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.update_active_repo_root(repo_root) {
            self.recompute_active_commands(ctx);
        }
    }

    pub(crate) fn command_is_active(&self, command: &StaticCommand, ctx: &AppContext) -> bool {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        command.supports_gui() && self.command_passes_common_gates(command, availability, &gates)
    }

    fn recompute_active_commands(&mut self, ctx: &mut ModelContext<Self>) {
        let availability = self.availability(ctx);
        let gates = self.common_command_gates(ctx);
        let commands = HashMap::from_iter(
            COMMAND_REGISTRY
                .all_commands_by_id()
                .filter(|(_, command)| {
                    command.supports_gui()
                        && self.command_passes_common_gates(command, availability, &gates)
                })
                .map(|(id, command)| (id, command.clone())),
        );
        if self.replace_active_commands(commands) {
            ctx.emit(UpdatedActiveCommands);
        }
    }

    fn availability(&self, ctx: &AppContext) -> Availability {
        self.base_availability(ctx) | Availability::TERMINAL_VIEW
    }
}

impl SyncDataSource for GuiSlashCommandDataSource {
    type Action = AcceptSlashCommandOrSavedPrompt;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        if query.text.is_empty() {
            return Ok(vec![]);
        }

        let query_text = query.text.trim().to_lowercase();
        let results = self.match_active_commands(&query_text, app);

        Ok(results
            .into_iter()
            .map(|item: InlineItem| item.into())
            .collect())
    }
}

impl SlashCommandDataSource for GuiSlashCommandDataSource {
    fn state(&self) -> &SlashCommandDataSourceState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut SlashCommandDataSourceState {
        &mut self.state
    }
}
impl Entity for GuiSlashCommandDataSource {
    type Event = UpdatedActiveCommands;
}
