use warpui::{Entity, ModelContext, ModelHandle};

use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryFilter};
use crate::search::mixer::SearchMixer;
use crate::terminal::input::slash_commands::AcceptSlashCommandOrSavedPrompt;

pub type SlashCommandMixer = SearchMixer<AcceptSlashCommandOrSavedPrompt>;

pub fn build_slash_command_mixer<Primary, ZeroState>(
    slash_commands_source: ModelHandle<Primary>,
    zero_state_source: ZeroState,
    ctx: &mut ModelContext<SlashCommandMixer>,
) -> SlashCommandMixer
where
    Primary: Entity + SyncDataSource<Action = AcceptSlashCommandOrSavedPrompt>,
    ZeroState: SyncDataSource<Action = AcceptSlashCommandOrSavedPrompt>,
{
    let mut mixer = SlashCommandMixer::new();
    mixer.add_sync_source(
        slash_commands_source.clone(),
        [QueryFilter::StaticSlashCommands],
    );
    mixer.add_sync_source(zero_state_source, [QueryFilter::StaticSlashCommands]);
    mixer.run_query(slash_command_query(""), ctx);
    mixer
}

pub fn slash_command_query(text: &str) -> Query {
    Query {
        text: text.to_owned(),
        filters: [QueryFilter::StaticSlashCommands].into(),
    }
}
