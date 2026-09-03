use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::FairMutex;
use pathfinder_color::ColorU;
use warp_core::ui::color::ContrastingColor;
use warp_core::ui::color::blend::Blend;
use warp_core::ui::color::contrast::MinimumAllowedContrast;
use warp_core::ui::theme::Fill;
use warp_terminal::model::escape_sequences::{BRACKETED_PASTE_END, BRACKETED_PASTE_START};
use warpui::r#async::Timer;
use warpui::elements::{
    ChildView, ConstrainedBox, Container, CrossAxisAlignment, Element, Empty, Flex,
    MainAxisAlignment, MainAxisSize, ParentElement, Wrap, WrapFill, WrapFillEntireRun,
};
use warpui::{
    AppContext, Entity, EntityId, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use super::init::OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING;
use super::{Event, TerminalView};
use crate::appearance::Appearance;
use crate::completer::SessionContext;
use crate::context_chips::display_chip::{
    DisplayChip, DisplayChipConfig, GitLineChanges, PromptChipShellCommand, PromptDisplayChipEvent,
};
use crate::context_chips::prompt_type::PromptType;
use crate::context_chips::{ChipResult, git_line_changes_from_chips, spacing};
use crate::features::FeatureFlag;
use crate::pane_group::CodeReviewPanelArg;
use crate::settings::CodeSettings;
use crate::settings_view::{SettingsSection, cli_agent_settings_widget_id};
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::{CLIAgentSessionsModel, CLIAgentSessionsModelEvent};
use crate::terminal::input::MenuPositioningProvider;
use crate::terminal::input::voice_input::VoiceInputButton;
use crate::terminal::model::TerminalModel;
use crate::terminal::model_events::ModelEventDispatcher;
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{
    ActionButton, ActionButtonTheme, ButtonSize, KeystrokeSource, TooltipAlignment,
};
use crate::workspace::view::TOGGLE_PROJECT_EXPLORER_BINDING_NAME;

const ITEM_SPACING: f32 = 4.;
const BRAND_ICON_PADDING_RIGHT: f32 = 8.;
const FOOTER_VERTICAL_PADDING: f32 = 4.;

/// Small delay inserted between separate PTY writes to CLI agents, so each write is
/// delivered as a distinct stdin read.
const CLI_AGENT_PTY_WRITE_DELAY: Duration = Duration::from_millis(50);

/// Longer delay for agents (like Copilot) that need extra time after a bracketed paste
/// before they will accept a submit keystroke.
const CLI_AGENT_BRACKETED_PASTE_ENTER_DELAY: Duration = Duration::from_millis(300);

/// ASCII prefixes that CLI agents use to switch input modes (e.g. `!` for bash mode in
/// Claude Code).
#[allow(clippy::byte_char_slices)]
const CLI_AGENT_MODE_SWITCH_PREFIXES: &[u8] = &[b'!', b'&'];

/// Toolbar rendered directly beneath a running CLI agent (Claude Code, Codex, Gemini, ...).
///
/// Renders nothing unless the pane has an active CLI agent session.
pub struct CliAgentFooter {
    terminal_view_id: EntityId,
    terminal_model: Arc<FairMutex<TerminalModel>>,
    prompt: ModelHandle<PromptType>,
    file_attach_button: ViewHandle<ActionButton>,
    voice_input_button: ViewHandle<VoiceInputButton>,
    file_explorer_button: ViewHandle<ActionButton>,
    rich_input_button: ViewHandle<ActionButton>,
    settings_button: ViewHandle<ActionButton>,
    left_display_chips: Vec<ViewHandle<DisplayChip>>,
    right_display_chips: Vec<ViewHandle<DisplayChip>>,
    display_chip_config: DisplayChipConfig,
}

#[derive(Clone, Debug)]
pub enum CliAgentFooterAction {
    SelectFile,
    InsertFilePath(String),
    ToggleFileExplorer,
    ToggleRichInput,
    OpenCodingAgentSettings,
}

pub enum CliAgentFooterEvent {
    /// Text that should be inserted into the agent's input, either the rich input
    /// composer when it is open or the PTY when it is not.
    InsertText(String),
    ToggleFileExplorer,
    ToggleRichInput,
    OpenCodeReview,
    TryExecuteChipCommand(PromptChipShellCommand),
}

impl CliAgentFooter {
    pub fn new(
        terminal_view_id: EntityId,
        terminal_model: Arc<FairMutex<TerminalModel>>,
        prompt: ModelHandle<PromptType>,
        menu_positioning_provider: Arc<dyn MenuPositioningProvider>,
        model_events: ModelHandle<ModelEventDispatcher>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let button_size = ButtonSize::AgentInputButton;
        let file_attach_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Plus)
                .with_tooltip("Attach file")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| ctx.dispatch_typed_action(CliAgentFooterAction::SelectFile))
        });

        let voice_input_button = ctx.add_typed_action_view(VoiceInputButton::new_for_agent_footer);

        let file_explorer_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("File explorer", AgentInputButtonTheme)
                .with_icon(Icon::FileCopy)
                .with_tooltip("Open file explorer")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_keybinding(
                    KeystrokeSource::Binding(TOGGLE_PROJECT_EXPLORER_BINDING_NAME),
                    ctx,
                )
                .with_compact_keybinding(true)
                .on_click(|ctx| ctx.dispatch_typed_action(CliAgentFooterAction::ToggleFileExplorer))
        });

        let rich_input_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("Rich Input", AgentInputButtonTheme)
                .with_icon(Icon::TextInput)
                .with_tooltip("Open Rich Input")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_keybinding(
                    KeystrokeSource::Binding(OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING),
                    ctx,
                )
                .with_compact_keybinding(true)
                .on_click(|ctx| ctx.dispatch_typed_action(CliAgentFooterAction::ToggleRichInput))
        });

        let settings_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Settings)
                .with_tooltip("Open coding agent settings")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CliAgentFooterAction::OpenCodingAgentSettings)
                })
        });

        ctx.subscribe_to_model(
            &CLIAgentSessionsModel::handle(ctx),
            move |me, _, event, ctx| {
                if event.terminal_view_id() != terminal_view_id {
                    return;
                }
                match event {
                    CLIAgentSessionsModelEvent::InputSessionChanged { .. } => {
                        me.sync_rich_input_button(ctx);
                    }
                    // The chips only start running once a session exists, so build them here
                    // rather than waiting for the next unrelated prompt change.
                    CLIAgentSessionsModelEvent::Started { .. } => {
                        let prompt = me.prompt.clone();
                        me.update_display_chips(&prompt, ctx);
                    }
                    _ => {}
                }
                me.notify_and_notify_children(ctx);
            },
        );

        // The File explorer item's availability follows this setting, so the footer has to
        // repaint when it is toggled rather than waiting for an unrelated re-render.
        ctx.subscribe_to_model(&CodeSettings::handle(ctx), |_, _, _, ctx| ctx.notify());

        ctx.observe(&prompt, |me, model, ctx| {
            me.update_display_chips(&model, ctx)
        });

        Self {
            terminal_view_id,
            terminal_model,
            prompt,
            file_attach_button,
            voice_input_button,
            file_explorer_button,
            rich_input_button,
            settings_button,
            left_display_chips: vec![],
            right_display_chips: vec![],
            display_chip_config: DisplayChipConfig {
                terminal_view_id,
                menu_positioning_provider,
                session_context: None,
                current_repo_path: None,
                model_events,
            },
        }
    }

    pub fn cli_agent(&self, app: &AppContext) -> Option<CLIAgent> {
        CLIAgentSessionsModel::as_ref(app)
            .session(self.terminal_view_id)
            .map(|session| session.agent)
    }

    pub fn handle_voice_hold_key(
        &mut self,
        key_state: warpui::event::KeyState,
        ctx: &mut ViewContext<Self>,
    ) {
        self.voice_input_button.update(ctx, |button, ctx| {
            button.handle_hold_key(key_state, ctx);
        });
    }

    pub fn update_session_context(
        &mut self,
        session_context: Option<SessionContext>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.display_chip_config.session_context = session_context.clone();
        for chip in self.all_display_chips() {
            chip.update(ctx, |chip, chip_ctx| {
                chip.update_session_context(session_context.clone(), chip_ctx);
            });
        }
    }

    pub fn update_repo_path(&mut self, repo_path: Option<PathBuf>, ctx: &mut ViewContext<Self>) {
        self.display_chip_config.current_repo_path = repo_path;
        ctx.notify();
    }

    pub fn has_open_chip_menu(&self, app: &AppContext) -> bool {
        self.all_display_chips()
            .any(|chip| chip.as_ref(app).display_chip_kind().has_open_menu())
    }

    fn all_display_chips(&self) -> impl Iterator<Item = &ViewHandle<DisplayChip>> {
        self.left_display_chips
            .iter()
            .chain(self.right_display_chips.iter())
    }

    fn sync_rich_input_button(&mut self, ctx: &mut ViewContext<Self>) {
        let is_open = CLIAgentSessionsModel::as_ref(ctx).is_input_open(self.terminal_view_id);
        self.rich_input_button.update(ctx, |button, ctx| {
            if is_open {
                button.set_label("Hide Rich Input", ctx);
                button.set_tooltip(Some("Hide Rich Input"), ctx);
            } else {
                button.set_label("Rich Input", ctx);
                button.set_tooltip(Some("Open Rich Input"), ctx);
            }
            button.set_keybinding(
                Some(KeystrokeSource::Binding(
                    OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING,
                )),
                ctx,
            );
        });
    }

    fn notify_and_notify_children(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.notify();
        self.file_attach_button.update(ctx, |_, ctx| ctx.notify());
        self.voice_input_button.update(ctx, |_, ctx| ctx.notify());
        self.file_explorer_button.update(ctx, |_, ctx| ctx.notify());
        self.rich_input_button.update(ctx, |_, ctx| ctx.notify());
        self.settings_button.update(ctx, |_, ctx| ctx.notify());
    }

    fn select_file(&mut self, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        let view_id = ctx.view_id();
        ctx.open_file_picker(
            move |result, ctx| {
                if let Ok(paths) = result
                    && let Some(path) = paths.first()
                {
                    ctx.dispatch_typed_action_for_view(
                        window_id,
                        view_id,
                        &CliAgentFooterAction::InsertFilePath(path.clone()),
                    );
                }
            },
            warpui::platform::FilePickerConfiguration::new(),
        );
    }

    fn chip_values_have_changed(
        existing_chips: &[ViewHandle<DisplayChip>],
        new_chips: &[ChipResult],
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        existing_chips.len() != new_chips.len()
            || new_chips.iter().enumerate().any(|(i, chip_result)| {
                existing_chips[i].read(ctx, |chip, _| {
                    chip.value() != chip_result.value()
                        || chip.chip_kind() != chip_result.kind()
                        || chip.on_click_values() != chip_result.on_click_values()
                })
            })
    }

    fn create_display_chips(
        &self,
        new_chips: &[ChipResult],
        git_line_changes_info: Option<GitLineChanges>,
        ctx: &mut ViewContext<Self>,
    ) -> Vec<ViewHandle<DisplayChip>> {
        let mut display_chips = Vec::with_capacity(new_chips.len());
        let mut new_chips = new_chips.iter().peekable();
        while let Some(chip_result) = new_chips.next() {
            let next_chip_kind = new_chips.peek().map(|chip| chip.kind().clone());

            let view_handle = ctx.add_typed_action_view(|ctx| {
                let mut chip = DisplayChip::new_for_agent_view(
                    chip_result.clone(),
                    next_chip_kind,
                    self.display_chip_config.clone(),
                    ctx,
                );
                chip.maybe_set_git_line_changes_info(git_line_changes_info.clone());
                chip.update_session_context(self.display_chip_config.session_context.clone(), ctx);
                chip
            });

            ctx.subscribe_to_view(&view_handle, |_, _, event, ctx| {
                match event {
                    PromptDisplayChipEvent::TryExecuteCommand(command) => {
                        ctx.emit(CliAgentFooterEvent::TryExecuteChipCommand(command.clone()));
                    }
                    PromptDisplayChipEvent::OpenCodeReview => {
                        ctx.emit(CliAgentFooterEvent::OpenCodeReview);
                    }
                    _ => {}
                }
                ctx.notify();
            });

            display_chips.push(view_handle);
        }

        display_chips
    }

    fn update_existing_display_chips(
        display_chips: &[ViewHandle<DisplayChip>],
        git_line_changes_info: Option<GitLineChanges>,
        ctx: &mut ViewContext<Self>,
    ) {
        for chip_view in display_chips {
            chip_view.update(ctx, |chip, ctx| {
                chip.maybe_set_git_line_changes_info(git_line_changes_info.clone());
                ctx.notify();
            });
        }
    }

    fn update_display_chips(
        &mut self,
        model: &ModelHandle<PromptType>,
        ctx: &mut ViewContext<Self>,
    ) {
        let new_left_chips: Vec<ChipResult> = model
            .as_ref(ctx)
            .cli_agent_left_chips(ctx)
            .into_iter()
            .filter(|chip_result| chip_result.value().is_some())
            .collect();
        let new_right_chips: Vec<ChipResult> = model
            .as_ref(ctx)
            .cli_agent_right_chips(ctx)
            .into_iter()
            .filter(|chip_result| chip_result.value().is_some())
            .collect();
        let git_line_changes_info = git_line_changes_from_chips(&new_left_chips);

        if Self::chip_values_have_changed(&self.left_display_chips, &new_left_chips, ctx) {
            self.left_display_chips =
                self.create_display_chips(&new_left_chips, git_line_changes_info.clone(), ctx);
        } else {
            Self::update_existing_display_chips(
                &self.left_display_chips,
                git_line_changes_info.clone(),
                ctx,
            );
        }

        if Self::chip_values_have_changed(&self.right_display_chips, &new_right_chips, ctx) {
            self.right_display_chips =
                self.create_display_chips(&new_right_chips, git_line_changes_info.clone(), ctx);
        } else {
            Self::update_existing_display_chips(
                &self.right_display_chips,
                git_line_changes_info,
                ctx,
            );
        }

        ctx.notify();
    }

    fn background_color(&self, appearance: &Appearance) -> ColorU {
        let terminal_model = self.terminal_model.lock();
        if terminal_model.is_alt_screen_active() {
            terminal_model
                .alt_screen()
                .inferred_bg_color()
                .unwrap_or_else(|| appearance.theme().surface_1().into_solid())
        } else {
            appearance.theme().surface_1().into_solid()
        }
    }

    fn render_brand_icon(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let appearance = Appearance::as_ref(app);
        let agent = self.cli_agent(app)?;
        let icon = agent.icon()?;
        let icon_color = agent
            .brand_color()
            .map(|color| {
                color.on_background(
                    self.background_color(appearance),
                    MinimumAllowedContrast::NonText,
                )
            })
            .unwrap_or_else(|| appearance.theme().foreground().into_solid());
        let icon_size = ButtonSize::AgentInputButton.icon_size(appearance, app);

        Some(
            Container::new(
                ConstrainedBox::new(icon.to_warpui_icon(Fill::Solid(icon_color)).finish())
                    .with_width(icon_size)
                    .with_height(icon_size)
                    .finish(),
            )
            .with_padding_right(BRAND_ICON_PADDING_RIGHT)
            .finish(),
        )
    }

    fn is_file_explorer_available(app: &AppContext) -> bool {
        cfg!(feature = "local_fs") && *CodeSettings::as_ref(app).show_project_explorer
    }
}

impl Entity for CliAgentFooter {
    type Event = CliAgentFooterEvent;
}

impl TypedActionView for CliAgentFooter {
    type Action = CliAgentFooterAction;

    fn handle_action(&mut self, action: &CliAgentFooterAction, ctx: &mut ViewContext<Self>) {
        match action {
            CliAgentFooterAction::SelectFile => self.select_file(ctx),
            CliAgentFooterAction::InsertFilePath(path) => {
                ctx.emit(CliAgentFooterEvent::InsertText(format!("{path} ")));
            }
            CliAgentFooterAction::ToggleFileExplorer => {
                ctx.emit(CliAgentFooterEvent::ToggleFileExplorer);
            }
            CliAgentFooterAction::ToggleRichInput => {
                ctx.emit(CliAgentFooterEvent::ToggleRichInput);
            }
            CliAgentFooterAction::OpenCodingAgentSettings => {
                #[cfg(not(target_family = "wasm"))]
                ctx.dispatch_typed_action_deferred(
                    crate::workspace::WorkspaceAction::ScrollToSettingsWidget {
                        page: SettingsSection::ThirdPartyCLIAgents,
                        widget_id: cli_agent_settings_widget_id(),
                    },
                );
            }
        }
    }
}

impl View for CliAgentFooter {
    fn ui_name() -> &'static str {
        "CliAgentFooter"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        if self.cli_agent(app).is_none() {
            return Empty::new().finish();
        }

        let mut left_buttons = Wrap::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_run_spacing(ITEM_SPACING)
            .with_spacing(ITEM_SPACING);

        if let Some(brand_icon) = self.render_brand_icon(app) {
            left_buttons.add_child(brand_icon);
        }
        left_buttons.add_child(ChildView::new(&self.file_attach_button).finish());
        left_buttons.add_child(ChildView::new(&self.voice_input_button).finish());
        left_buttons.add_children(
            self.left_display_chips
                .iter()
                .filter(|chip| chip.as_ref(app).should_render(app))
                .map(|chip| ChildView::new(chip).finish()),
        );
        if Self::is_file_explorer_available(app) {
            left_buttons.add_child(ChildView::new(&self.file_explorer_button).finish());
        }
        if FeatureFlag::CLIAgentRichInput.is_enabled() {
            left_buttons.add_child(ChildView::new(&self.rich_input_button).finish());
        }

        let mut right_buttons = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(ITEM_SPACING);
        right_buttons.add_children(
            self.right_display_chips
                .iter()
                .filter(|chip| chip.as_ref(app).should_render(app))
                .map(|chip| ChildView::new(chip).finish()),
        );
        right_buttons.add_child(ChildView::new(&self.settings_button).finish());

        let content = Wrap::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(WrapFillEntireRun::new(left_buttons.finish()).finish())
            .with_child(WrapFill::new(0., right_buttons.finish()).finish())
            .with_run_spacing(spacing::UDI_ROW_RUN_SPACING)
            .finish();

        Container::new(content)
            .with_vertical_padding(FOOTER_VERTICAL_PADDING)
            .finish()
    }
}

pub(crate) struct AgentInputButtonTheme;

impl ActionButtonTheme for AgentInputButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        // Solid surface fills keep the button readable even when its parent isn't
        // `theme.background()` (for example, over an alt-screen CLI agent).
        let theme = appearance.theme();
        Some(if hovered {
            theme.surface_2()
        } else {
            theme.surface_1()
        })
    }

    fn text_color(
        &self,
        _hovered: bool,
        background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        // If a caller overrides `background()` with a translucent fill, blend it over
        // `surface_1` so text contrast is computed against the actual rendered color.
        let base_bg = appearance.theme().surface_1();
        let effective_bg = background
            .map(|overlay| base_bg.blend(&overlay))
            .unwrap_or(base_bg);

        appearance.theme().sub_text_color(effective_bg).into_solid()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        Some(warp_core::ui::theme::color::internal_colors::neutral_3(
            appearance.theme(),
        ))
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// Theme for the mic button. Uses a blue icon when hovered.
pub(crate) struct ActiveMicButtonTheme;

impl ActionButtonTheme for ActiveMicButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        AgentInputButtonTheme.background(hovered, appearance)
    }

    fn text_color(
        &self,
        hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        if hovered {
            appearance.theme().ansi_fg_blue()
        } else {
            appearance
                .theme()
                .sub_text_color(appearance.theme().surface_1())
                .into_solid()
        }
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        AgentInputButtonTheme.border(appearance)
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// How rich input delivers text + Enter to the CLI agent's PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RichInputSubmitStrategy {
    /// Send text bytes followed by `\r` in a single write.
    Inline,
    /// Wrap text in bracketed paste escape sequences, then send `\r` separately.
    /// Required for agents whose paste-burst heuristics would otherwise suppress
    /// a rapid Enter after a character stream.
    BracketedPaste,
    /// Send text first, then `\r` after a short delay. For agents that don't respond
    /// to `\r` when it arrives in the same buffer as the text.
    DelayedEnter,
    /// Wrap text in bracketed paste, then send `\r` after a delay.
    BracketedPasteDelayedEnter,
}

fn rich_input_submit_strategy(agent: CLIAgent) -> RichInputSubmitStrategy {
    match agent {
        CLIAgent::Codex | CLIAgent::OhMyPi | CLIAgent::Hermes => {
            RichInputSubmitStrategy::BracketedPaste
        }
        CLIAgent::Copilot => RichInputSubmitStrategy::BracketedPasteDelayedEnter,
        CLIAgent::Claude
        | CLIAgent::OpenCode
        | CLIAgent::Gemini
        | CLIAgent::Auggie
        | CLIAgent::CursorCli => RichInputSubmitStrategy::DelayedEnter,
        CLIAgent::Amp
        | CLIAgent::Droid
        | CLIAgent::Pi
        | CLIAgent::Goose
        | CLIAgent::Vibe
        | CLIAgent::Antigravity
        | CLIAgent::Grok
        | CLIAgent::Trae
        | CLIAgent::Cline
        | CLIAgent::QwenCode
        | CLIAgent::Devin
        | CLIAgent::Unknown => RichInputSubmitStrategy::Inline,
    }
}

impl TerminalView {
    pub(super) fn handle_cli_agent_footer_event(
        &mut self,
        event: &CliAgentFooterEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            CliAgentFooterEvent::InsertText(text) => {
                self.insert_text_into_cli_agent_input(text, ctx);
            }
            CliAgentFooterEvent::ToggleFileExplorer => {
                self.toggle_cli_agent_file_explorer(ctx);
            }
            CliAgentFooterEvent::ToggleRichInput => {
                self.toggle_cli_agent_rich_input(ctx);
            }
            CliAgentFooterEvent::OpenCodeReview => {
                ctx.emit(Event::OpenCodeReviewPane(CodeReviewPanelArg {
                    repo_path: self.current_repo_path.clone(),
                    terminal_view: self.view_handle.clone(),
                    focus_new_pane: true,
                    cli_agent: self.cli_agent_footer.as_ref(ctx).cli_agent(ctx),
                }));
            }
            CliAgentFooterEvent::TryExecuteChipCommand(command) => {
                let command = command.clone();
                self.input.update(ctx, |input, ctx| {
                    input.execute_prompt_chip_command(&command, ctx);
                });
            }
        }
    }

    pub(super) fn has_active_cli_agent_input_session(&self, app: &AppContext) -> bool {
        CLIAgentSessionsModel::as_ref(app).is_input_open(self.view_id)
    }

    /// Whether the CLI agent footer should be rendered in place of the (hidden) input box.
    pub(super) fn should_render_cli_agent_footer(&self, app: &AppContext) -> bool {
        self.cli_agent_footer.as_ref(app).cli_agent(app).is_some()
            && !self.has_active_cli_agent_input_session(app)
    }

    fn toggle_cli_agent_file_explorer(&mut self, ctx: &mut ViewContext<Self>) {
        let _cli_agent = self.cli_agent_footer.as_ref(ctx).cli_agent(ctx);
        self.toggle_left_panel_file_tree(false, ctx);
    }

    pub(super) fn toggle_cli_agent_rich_input(&mut self, ctx: &mut ViewContext<Self>) {
        if self.has_active_cli_agent_input_session(ctx) {
            self.close_cli_agent_rich_input(ctx);
        } else {
            self.open_cli_agent_rich_input(ctx);
        }
    }

    pub(super) fn open_cli_agent_rich_input(&mut self, ctx: &mut ViewContext<Self>) {
        if !FeatureFlag::CLIAgentRichInput.is_enabled()
            || self.has_active_cli_agent_input_session(ctx)
            || self.cli_agent_footer.as_ref(ctx).cli_agent(ctx).is_none()
        {
            return;
        }

        let view_id = self.view_id;
        CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions_model, ctx| {
            sessions_model.open_input(view_id, true, ctx);
        });

        self.focus_input_box(ctx);
        ctx.notify();
    }

    pub(super) fn close_cli_agent_rich_input(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.has_active_cli_agent_input_session(ctx) {
            return;
        }

        let draft = self.input.as_ref(ctx).buffer_text(ctx);
        let view_id = self.view_id;
        CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions_model, ctx| {
            sessions_model.set_draft(view_id, draft);
            sessions_model.close_input(view_id, false, ctx);
        });

        self.redetermine_terminal_focus(ctx);
        ctx.notify();
    }

    /// Inserts `text` into the active CLI agent's input without submitting it, routing to the
    /// rich input composer when it is open and to the PTY when it is not.
    pub(super) fn insert_text_into_cli_agent_input(
        &mut self,
        text: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        if text.is_empty() {
            return;
        }

        if self.has_active_cli_agent_input_session(ctx) {
            self.input.update(ctx, |input, ctx| {
                input.append_to_buffer(text, ctx);
            });
            self.focus_input_box(ctx);
            return;
        }

        let Some(agent) = self.cli_agent_footer.as_ref(ctx).cli_agent(ctx) else {
            return;
        };
        self.write_cli_agent_text(text.as_bytes(), rich_input_submit_strategy(agent), ctx);
    }

    pub(super) fn submit_cli_agent_rich_input(
        &mut self,
        text: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if !self.has_active_cli_agent_input_session(ctx) || text.trim().is_empty() {
            return;
        }

        let view_id = self.view_id;
        CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions_model, _| {
            sessions_model.clear_draft(view_id);
        });

        let strategy = CLIAgentSessionsModel::as_ref(ctx)
            .session(view_id)
            .map(|session| rich_input_submit_strategy(session.agent))
            .unwrap_or(RichInputSubmitStrategy::Inline);

        let text_bytes = text.into_bytes();

        // Cleared eagerly so a close from any path sees an empty buffer and doesn't
        // re-save the submitted text as a draft.
        self.input.update(ctx, |input, ctx| {
            input.clear_buffer_and_reset_undo_stack(ctx);
        });

        // The prefix is written on its own so the agent switches modes before the rest arrives.
        if text_bytes.len() > 1 && CLI_AGENT_MODE_SWITCH_PREFIXES.contains(&text_bytes[0]) {
            self.write_user_bytes_to_pty(vec![text_bytes[0]], ctx);
            let rest = text_bytes[1..].to_vec();
            ctx.spawn(
                Timer::after(CLI_AGENT_PTY_WRITE_DELAY),
                move |me, _, ctx| {
                    me.write_cli_agent_text_then_submit(rest, strategy, ctx);
                },
            );
        } else {
            self.write_cli_agent_text_then_submit(text_bytes, strategy, ctx);
        }
    }

    fn write_cli_agent_text(
        &mut self,
        text_bytes: &[u8],
        strategy: RichInputSubmitStrategy,
        ctx: &mut ViewContext<Self>,
    ) {
        let bytes = match strategy {
            RichInputSubmitStrategy::BracketedPaste
            | RichInputSubmitStrategy::BracketedPasteDelayedEnter => {
                let mut bytes = Vec::with_capacity(
                    BRACKETED_PASTE_START.len() + text_bytes.len() + BRACKETED_PASTE_END.len(),
                );
                bytes.extend_from_slice(BRACKETED_PASTE_START);
                bytes.extend_from_slice(text_bytes);
                bytes.extend_from_slice(BRACKETED_PASTE_END);
                bytes
            }
            RichInputSubmitStrategy::Inline | RichInputSubmitStrategy::DelayedEnter => {
                text_bytes.to_vec()
            }
        };
        self.write_user_bytes_to_pty(bytes, ctx);
    }

    fn write_cli_agent_text_then_submit(
        &mut self,
        text_bytes: Vec<u8>,
        strategy: RichInputSubmitStrategy,
        ctx: &mut ViewContext<Self>,
    ) {
        match strategy {
            RichInputSubmitStrategy::Inline => {
                let mut bytes = text_bytes;
                bytes.extend_from_slice(b"\r");
                self.write_user_bytes_to_pty(bytes, ctx);
            }
            RichInputSubmitStrategy::BracketedPaste => {
                self.write_cli_agent_text(&text_bytes, strategy, ctx);
                self.write_user_bytes_to_pty(b"\r".to_vec(), ctx);
            }
            RichInputSubmitStrategy::DelayedEnter => {
                self.write_user_bytes_to_pty(text_bytes, ctx);
                ctx.spawn(
                    Timer::after(CLI_AGENT_PTY_WRITE_DELAY),
                    move |me, _, ctx| {
                        me.write_user_bytes_to_pty(b"\r".to_vec(), ctx);
                    },
                );
            }
            RichInputSubmitStrategy::BracketedPasteDelayedEnter => {
                self.write_cli_agent_text(&text_bytes, strategy, ctx);
                ctx.spawn(
                    Timer::after(CLI_AGENT_BRACKETED_PASTE_ENTER_DELAY),
                    move |me, _, ctx| {
                        me.write_user_bytes_to_pty(b"\r".to_vec(), ctx);
                    },
                );
            }
        }
    }
}
