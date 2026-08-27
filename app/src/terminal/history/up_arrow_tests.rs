use std::sync::Arc;

use chrono::{Duration, Local};
use warp_core::SessionId;
use warpui::{App, AppContext, SingletonEntity};

use super::UpArrowHistoryConfig;
use crate::input_suggestions::HistoryInputSuggestion;
use crate::suggestions::ignored_suggestions_model::{IgnoredSuggestionsModel, SuggestionType};
use crate::terminal::model::session::command_executor::NoOpCommandExecutor;
use crate::terminal::model::session::{Session, SessionInfo};
use crate::terminal::{History, HistoryEntry, LinkedWorkflowData};

fn command_entry(
    session_id: SessionId,
    command: &str,
    age: i64,
    workflow_command: Option<&str>,
) -> HistoryEntry {
    HistoryEntry {
        session_id: Some(session_id),
        command: command.to_owned(),
        pwd: None,
        start_ts: Some(Local::now() + Duration::milliseconds(age)),
        completed_ts: None,
        exit_code: None,
        git_head: None,
        shell_host: None,
        workflow_id: None,
        workflow_command: workflow_command.map(str::to_owned),
        is_for_restored_block: false,
        is_agent_executed: false,
    }
}

fn command_history(
    session_id: SessionId,
    app: &AppContext,
) -> Vec<(String, Option<LinkedWorkflowData>)> {
    History::handle(app)
        .as_ref(app)
        .up_arrow_suggestions_for_terminal_surface(
            Some(session_id),
            UpArrowHistoryConfig {
                include_commands: true,
            },
            app,
        )
        .into_iter()
        .map(|suggestion| {
            let text = suggestion.normalized_text().to_owned();
            match suggestion {
                HistoryInputSuggestion::Command { entry } => (text, entry.linked_workflow_data()),
            }
        })
        .collect()
}

async fn add_command_history(app: &mut App, session_id: SessionId, entries: Vec<HistoryEntry>) {
    let mut session_info = SessionInfo::new_for_test();
    session_info.session_id = session_id;
    let session = Arc::new(Session::new(
        session_info,
        Arc::new(NoOpCommandExecutor::default()),
    ));
    let (initialized_tx, initialized_rx) = async_channel::bounded(1);
    let history = app.add_singleton_model(|_| History::default());
    app.update(|ctx| {
        ctx.subscribe_to_model(&history, move |_, event, _| match event {
            crate::terminal::HistoryEvent::Initialized(id) if *id == session_id => {
                let _ = initialized_tx.try_send(());
            }
            crate::terminal::HistoryEvent::Initialized(_) => {}
        });
        history.update(ctx, |history, ctx| {
            history.init_session_with(session, async { Vec::new() }, ctx);
        });
    });
    initialized_rx
        .recv()
        .await
        .expect("history initialization should complete");
    history.update(app, |history, _| {
        for entry in entries {
            history.append_commands(session_id, vec![entry]);
        }
    });
}

#[test]
fn command_history_dedupes_orders_and_excludes_whitespace() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(1);
        add_command_history(
            &mut app,
            session_id,
            vec![
                command_entry(session_id, " same ", 0, None),
                command_entry(session_id, "older command", 1, None),
                command_entry(session_id, "same", 2, None),
                command_entry(session_id, "   ", 3, None),
            ],
        )
        .await;

        app.read(|ctx| {
            assert_eq!(
                command_history(session_id, ctx),
                vec![
                    ("older command".to_owned(), None),
                    ("same".to_owned(), None),
                ]
            );
        });
    });
}

#[test]
fn command_history_preserves_command_workflow_data() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(1);
        add_command_history(
            &mut app,
            session_id,
            vec![command_entry(
                session_id,
                "deploy",
                0,
                Some("deploy {{environment}}"),
            )],
        )
        .await;

        app.read(|ctx| {
            assert_eq!(
                command_history(session_id, ctx),
                vec![(
                    "deploy".to_owned(),
                    Some(LinkedWorkflowData::Command(
                        "deploy {{environment}}".to_owned(),
                    )),
                )]
            );
        });
    });
}

#[test]
fn command_history_excludes_ignored_commands() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(1);
        add_command_history(
            &mut app,
            session_id,
            vec![
                command_entry(session_id, "keep command", 0, None),
                command_entry(session_id, "ignore command", 1, None),
            ],
        )
        .await;
        app.add_singleton_model(|_| {
            IgnoredSuggestionsModel::new(vec![(
                "ignore command".to_owned(),
                SuggestionType::ShellCommand,
            )])
        });

        app.read(|ctx| {
            assert_eq!(
                command_history(session_id, ctx)
                    .into_iter()
                    .map(|(text, _)| text)
                    .collect::<Vec<_>>(),
                vec!["keep command"]
            );
        });
    });
}
