mod data_source;
mod mixer;
mod search_item;
pub(super) mod view;

#[cfg(feature = "local_fs")]
use std::path::PathBuf;

pub use data_source::*;
pub use mixer::{SlashCommandMixer, build_slash_command_mixer, slash_command_query};
pub use view::{CloseReason, InlineSlashCommandView, SlashCommandsEvent};
use warp_core::features::FeatureFlag;
use warp_core::send_telemetry_from_ctx;
use warp_core::ui::theme::AnsiColorIdentifier;
#[cfg(feature = "local_fs")]
use warp_util::path::{CleanPathResult, LineAndColumnArg};
use warpui::{AppContext, SingletonEntity, ViewContext};

use crate::TelemetryEvent;
use crate::code_review::telemetry_event::CodeReviewPaneEntrypoint;
use crate::search::slash_command_menu::static_commands::SlashCommandKind;
use crate::search::slash_command_menu::static_commands::commands::COMMAND_REGISTRY;
use crate::search::slash_command_menu::{SlashCommandId, StaticCommand};
use crate::server::telemetry::SlashCommandAcceptedDetails;
use crate::tab::SelectedTabColor;
use crate::terminal::input::decorations::InputBackgroundJobOptions;
use crate::terminal::input::inline_menu::{InlineMenuAction, InlineMenuType};
use crate::terminal::input::slash_command_model::{
    SlashCommandEntryState, UpdatedSlashCommandModel,
};
use crate::terminal::input::{CompletionsTrigger, Event, Input, InputSuggestionsMode};
#[cfg(feature = "local_fs")]
use crate::terminal::model::session::Session;
use crate::terminal::view::TerminalAction;
use crate::ui_components::color_dot;
use crate::view_components::DismissibleToast;
use crate::workspace::{ToastStack, WorkspaceAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptSlashCommandOrSavedPrompt {
    SlashCommand { id: SlashCommandId },
}
impl InlineMenuAction for AcceptSlashCommandOrSavedPrompt {
    const MENU_TYPE: InlineMenuType = InlineMenuType::SlashCommands;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandSelectionBehavior {
    InsertCommandText(String),
    Execute,
}

/// Shared menu-selection policy for static slash commands.
///
/// Accepting a menu row either inserts the slash command text for further argument entry, or
/// executes the command immediately.
pub fn slash_command_selection_behavior(command: &StaticCommand) -> SlashCommandSelectionBehavior {
    if command
        .argument
        .as_ref()
        .is_some_and(|argument| !argument.should_execute_on_selection)
    {
        SlashCommandSelectionBehavior::InsertCommandText(format!("{} ", command.name))
    } else {
        SlashCommandSelectionBehavior::Execute
    }
}

/// Whether an already-open slash command menu should close after the input becomes an exact
/// static-command match.
///
/// Exact input stays visible while multiple prior results remain, but a unique match or the start
/// of argument entry closes the menu.
pub fn should_close_slash_command_menu_for_exact_match(
    result_count: usize,
    argument_started: bool,
) -> bool {
    result_count < 2 || argument_started
}

/// Records a static slash command accepted from the input.
pub fn record_static_slash_command_accepted(command_name: &str, ctx: &mut AppContext) {
    send_telemetry_from_ctx!(
        TelemetryEvent::SlashCommandAccepted {
            command_details: SlashCommandAcceptedDetails::StaticCommand {
                command_name: command_name.to_owned(),
            },
            is_in_agent_view: false,
        },
        ctx
    );
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SlashCommandTrigger {
    Input { cmd_or_ctrl_enter: bool },
    Keybinding,
}

impl SlashCommandTrigger {
    fn cmd_or_ctrl_enter() -> Self {
        Self::Input {
            cmd_or_ctrl_enter: true,
        }
    }

    pub fn input() -> Self {
        Self::Input {
            cmd_or_ctrl_enter: false,
        }
    }

    pub(super) fn keybinding() -> Self {
        Self::Keybinding
    }

    pub fn is_keybinding(&self) -> bool {
        matches!(self, Self::Keybinding)
    }
}

#[cfg(feature = "local_fs")]
fn open_file_command_path(
    session: &Session,
    current_dir: &str,
    raw_arg: &str,
) -> (PathBuf, Option<LineAndColumnArg>) {
    let parsed_path = CleanPathResult::with_line_and_column_number(raw_arg.trim());
    // The argument may contain shell-escaped characters (e.g. `\ ` for spaces) from auto-suggest.
    // Unescape them so the path matches the actual filesystem entry.
    let unescaped_path = session.shell_family().unescape(&parsed_path.path);
    // Expand `~` to the user's home directory.
    let expanded_path = shellexpand::tilde(&unescaped_path);

    let shell_path = session
        .convert_directory_to_typed_path_buf(current_dir.to_owned())
        .join(session.convert_directory_to_typed_path_buf(expanded_path.into_owned()))
        .normalize();
    let file_path = session
        .maybe_convert_to_native_path(&shell_path.to_path())
        .unwrap_or_else(|err| {
            log::warn!("unable to convert /open-file path to native path: {err:?}");
            PathBuf::from(shell_path.to_string_lossy().into_owned())
        });

    (file_path, parsed_path.line_and_column_num)
}

impl Input {
    fn is_slash_command_available(&self, command: &StaticCommand, ctx: &AppContext) -> bool {
        self.slash_command_data_source
            .as_ref(ctx)
            .command_is_active(command, ctx)
    }

    pub(super) fn select_slash_command(
        &mut self,
        command: &StaticCommand,
        trigger: SlashCommandTrigger,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.is_slash_command_available(command, ctx) {
            return;
        }
        match slash_command_selection_behavior(command) {
            SlashCommandSelectionBehavior::Execute => {
                let argument = if command
                    .argument
                    .as_ref()
                    .is_some_and(|arg| arg.should_execute_on_selection)
                    && !self.suggestions_mode_model.as_ref(ctx).is_slash_commands()
                {
                    let trimmed = self.buffer_text(ctx).trim().to_owned();
                    (!trimmed.is_empty()).then_some(trimmed)
                } else {
                    None
                };
                self.execute_slash_command(command, argument.as_ref(), trigger, ctx);
            }
            SlashCommandSelectionBehavior::InsertCommandText(text) => {
                self.editor.update(ctx, |editor, ctx| {
                    editor.set_buffer_text(&text, ctx);
                });
            }
        }
    }

    pub(super) fn close_slash_commands_menu(&mut self, ctx: &mut ViewContext<Self>) {
        self.suggestions_mode_model.update(ctx, |model, ctx| {
            model.set_mode(InputSuggestionsMode::Closed, ctx);
        });
        ctx.notify();
    }

    pub(super) fn handle_slash_command_model_event(
        &mut self,
        event: &UpdatedSlashCommandModel,
        ctx: &mut ViewContext<Self>,
    ) {
        // Refresh decorations if the slash command detection state changed, since
        // detected commands affect syntax highlighting.
        let new_state = self.slash_command_model.as_ref(ctx).state();
        if event.old_state.is_detected_command() != new_state.is_detected_command() {
            let _ = self
                .debounce_input_background_tx
                .try_send(InputBackgroundJobOptions::default().with_command_decoration());
        }

        match self.slash_command_model.as_ref(ctx).state().clone() {
            SlashCommandEntryState::None => {
                if self.suggestions_mode_model.as_ref(ctx).is_slash_commands() {
                    self.close_slash_commands_menu(ctx);
                }
            }
            SlashCommandEntryState::Composing { .. } => {
                if self.suggestions_mode_model.as_ref(ctx).is_closed() {
                    self.open_slash_commands_menu(ctx);
                } else if !self.suggestions_mode_model.as_ref(ctx).is_slash_commands() {
                    self.slash_command_model.update(ctx, |model, ctx| {
                        model.disable(ctx);
                    });
                }
            }
            SlashCommandEntryState::SlashCommand(detected_command) => {
                // If there is only one result (or zero, but that should be impossible if there is
                // a valid command in the input) OR if the user has started typing arguments, hide
                // the menu.
                if self.suggestions_mode_model.as_ref(ctx).is_slash_commands()
                    && should_close_slash_command_menu_for_exact_match(
                        self.inline_slash_commands_view
                            .as_ref(ctx)
                            .result_count(ctx),
                        detected_command.argument.is_some(),
                    )
                {
                    self.close_slash_commands_menu(ctx);
                }

                if detected_command.command.kind == SlashCommandKind::Edit
                    && detected_command
                        .argument
                        .as_ref()
                        .is_some_and(|argument| argument.is_empty())
                    && self.suggestions_mode_model.as_ref(ctx).is_closed()
                {
                    self.open_completion_suggestions(CompletionsTrigger::SlashCommandAutoOpen, ctx);
                }
            }
        }
    }

    pub(crate) fn handle_slash_commands_menu_event(
        &mut self,
        event: &SlashCommandsEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            SlashCommandsEvent::Close(reason) => {
                if reason.is_manual_dismissal() {
                    self.slash_command_model.update(ctx, |model, ctx| {
                        model.disable(ctx);
                    });
                }

                self.suggestions_mode_model.update(ctx, |model, ctx| {
                    model.set_mode(InputSuggestionsMode::Closed, ctx);
                });
                ctx.notify();
            }
            SlashCommandsEvent::SelectedStaticCommand {
                id,
                cmd_or_ctrl_enter,
            } => {
                let Some(command) = COMMAND_REGISTRY.get_command(id) else {
                    return;
                };
                self.select_slash_command(
                    command,
                    SlashCommandTrigger::Input {
                        cmd_or_ctrl_enter: *cmd_or_ctrl_enter,
                    },
                    ctx,
                );
            }
        }
    }

    /// Executes the given `command` with `argument`, if any.
    ///
    /// Returns `true` if execution was 'handled' (whether or not it resulted in success or failure).
    pub(super) fn execute_slash_command(
        &mut self,
        command: &StaticCommand,
        argument: Option<&String>,
        trigger: SlashCommandTrigger,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let _ = trigger;
        fn show_error_toast(message: String, ctx: &mut ViewContext<Input>) {
            let window_id = ctx.window_id();
            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                toast_stack.add_ephemeral_toast(DismissibleToast::error(message), window_id, ctx);
            });
        }

        match command.kind {
            SlashCommandKind::RenameTab => {
                let Some(name) = argument
                    .map(|name| name.trim())
                    .filter(|name| !name.is_empty())
                else {
                    show_error_toast(
                        "Please provide a tab name after /rename-tab".to_owned(),
                        ctx,
                    );
                    return true;
                };

                ctx.dispatch_typed_action(&WorkspaceAction::SetActiveTabName(name.to_owned()));
            }
            SlashCommandKind::SetTabColor => {
                let supported_options = || {
                    color_dot::TAB_COLOR_OPTIONS
                        .iter()
                        .map(|c| c.to_string().to_ascii_lowercase())
                        .chain(std::iter::once("none".to_owned()))
                        .collect::<Vec<_>>()
                        .join(", ")
                };

                let Some(arg) = argument
                    .map(|name| name.trim())
                    .filter(|name| !name.is_empty())
                else {
                    show_error_toast(
                        format!(
                            "Please provide a color after /set-tab-color ({})",
                            supported_options()
                        ),
                        ctx,
                    );
                    return true;
                };

                let color = if arg.eq_ignore_ascii_case("none") {
                    SelectedTabColor::Cleared
                } else {
                    let parsed = arg
                        .parse::<AnsiColorIdentifier>()
                        .ok()
                        .filter(|c| color_dot::TAB_COLOR_OPTIONS.contains(c));
                    match parsed {
                        Some(c) => SelectedTabColor::Color(c),
                        None => {
                            show_error_toast(
                                format!(
                                    "Unknown tab color '{arg}'. Use one of: {}.",
                                    supported_options()
                                ),
                                ctx,
                            );
                            return true;
                        }
                    }
                };

                ctx.dispatch_typed_action(&WorkspaceAction::SetActiveTabColor(color));
            }
            SlashCommandKind::Edit => {
                #[cfg(feature = "local_fs")]
                match argument {
                    Some(args) if !args.is_empty() => {
                        let Some(session_id) = self.active_block_session_id() else {
                            return false;
                        };

                        let Some(session) = self.sessions.as_ref(ctx).get(session_id) else {
                            return false;
                        };

                        if !session.is_local() {
                            let window_id = ctx.window_id();
                            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                                toast_stack.add_ephemeral_toast(
                                    DismissibleToast::error(
                                        "The /open-file command is only available for local sessions"
                                            .to_owned(),
                                    ),
                                    window_id,
                                    ctx,
                                );
                            });
                            return false;
                        }

                        let current_dir = self
                            .active_block_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.current_working_directory())
                            .map(str::to_owned);

                        let Some(current_dir) = current_dir else {
                            return false;
                        };

                        let (file_path, line_col) =
                            open_file_command_path(&session, &current_dir, args);

                        match std::fs::metadata(&file_path) {
                            Ok(metadata) if metadata.is_file() => {
                                use crate::util::file::external_editor;

                                ctx.dispatch_typed_action(&TerminalAction::OpenCodeInWarp {
                                    path: file_path,
                                    layout: external_editor::settings::EditorLayout::SplitPane,
                                    line_col,
                                });
                            }
                            Ok(_) => {
                                show_error_toast(
                                    "The /open-file command only works for files, not directories"
                                        .to_owned(),
                                    ctx,
                                );
                                return true;
                            }
                            Err(_) => {
                                show_error_toast(
                                    format!("File not found: {}", file_path.display()),
                                    ctx,
                                );
                                return true;
                            }
                        }
                    }
                    _ => {
                        use crate::server::telemetry::PaletteSource;

                        ctx.emit(Event::OpenFilesPalette {
                            source: PaletteSource::Keybinding,
                        });
                    }
                }
                #[cfg(not(feature = "local_fs"))]
                {
                    show_error_toast(
                        "The /open-file command is not supported in this build".to_owned(),
                        ctx,
                    );
                    return true;
                }
            }
            SlashCommandKind::Changelog => {
                if !FeatureFlag::Changelog.is_enabled() {
                    return false;
                }
                ctx.dispatch_typed_action(&WorkspaceAction::ViewLatestChangelog);
            }
            SlashCommandKind::Feedback => {
                ctx.dispatch_typed_action(&WorkspaceAction::SendFeedback);
            }
            SlashCommandKind::OpenCodeReview => {
                ctx.dispatch_typed_action(&TerminalAction::ToggleCodeReviewPane {
                    entrypoint: CodeReviewPaneEntrypoint::SlashCommand,
                });
            }
            SlashCommandKind::OpenSettingsFile => {
                if !FeatureFlag::SettingsFile.is_enabled() || !cfg!(feature = "local_fs") {
                    return false;
                }
                ctx.dispatch_typed_action(&WorkspaceAction::OpenSettingsFile);
            }
        }

        self.editor.update(ctx, |editor, ctx| {
            editor.clear_buffer(ctx);
        });

        record_static_slash_command_accepted(command.name, ctx);
        true
    }

    /// Handles cmd+enter (Mac) / ctrl+enter (Linux/Windows) for slash commands.
    ///
    /// Returns `true` if the keypress was handled.
    pub(super) fn maybe_handle_cmd_or_ctrl_shift_enter_for_slash_command(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        // If slash command menu is open, accept the selected item with cmd_or_ctrl_enter=true.
        if matches!(
            self.suggestions_mode_model.as_ref(ctx).mode(),
            InputSuggestionsMode::SlashCommands
        ) {
            self.inline_slash_commands_view.update(ctx, |view, ctx| {
                view.accept_selected_item(true, ctx);
            });
            return true;
        }

        // If no menu but slash command detected in buffer, execute with cmd_or_ctrl_enter=true
        match self.slash_command_model.as_ref(ctx).state() {
            SlashCommandEntryState::SlashCommand(detected_command) => {
                let command = detected_command.command.clone();
                let argument = detected_command.argument.clone();
                if !self.is_slash_command_available(&command, ctx) {
                    return false;
                }
                self.execute_slash_command(
                    &command,
                    argument.as_ref(),
                    SlashCommandTrigger::cmd_or_ctrl_enter(),
                    ctx,
                )
            }
            SlashCommandEntryState::None | SlashCommandEntryState::Composing { .. } => false,
        }
    }

    /// Executes a slash command on `enter` keypress.
    ///
    /// If the slash command menu is open, then "accepts" the slash command:
    ///   * If the slash command does not take arguments, executes it
    ///   * If the slash command does take arguments, inserts it into the input.
    ///
    /// If the slash command menu is not open, then "executes" the slash command in the input, if
    /// there is one.
    ///
    /// Returns `true` if the enter keypress was 'handled', else upstream enter keypress handling
    /// logic should continue.
    pub(super) fn maybe_handle_enter_for_slash_command(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if matches!(
            self.suggestions_mode_model.as_ref(ctx).mode(),
            InputSuggestionsMode::SlashCommands
        ) {
            self.inline_slash_commands_view.update(ctx, |view, ctx| {
                view.accept_selected_item(false, ctx);
            });
            return true;
        }

        match self.slash_command_model.as_ref(ctx).state() {
            SlashCommandEntryState::SlashCommand(detected_command) => {
                let command = detected_command.command.clone();
                let argument = detected_command.argument.clone();
                if !self.is_slash_command_available(&command, ctx) {
                    return false;
                }
                self.execute_slash_command(
                    &command,
                    argument.as_ref(),
                    SlashCommandTrigger::input(),
                    ctx,
                )
            }
            SlashCommandEntryState::None | SlashCommandEntryState::Composing { .. } => false,
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
