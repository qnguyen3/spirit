#[cfg(not(target_family = "wasm"))]
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::FairMutex;
use pathfinder_color::ColorU;
#[cfg(not(target_family = "wasm"))]
use tokio::fs;
use warp_core::settings::Setting;
use warp_core::ui::color::blend::Blend;
use warp_core::ui::theme::Fill;
#[cfg(not(target_family = "wasm"))]
use warp_errors::report_error;
#[cfg(not(target_family = "wasm"))]
use warpui::r#async::Timer;
use warpui::elements::{CrossAxisAlignment, Element, Empty, Flex, ParentElement};
use warpui::presenter::ChildView;
use warpui::{
    AppContext, Entity, EntityId, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::appearance::Appearance;
#[cfg(not(target_family = "wasm"))]
use crate::features::FeatureFlag;
use crate::send_telemetry_from_ctx;
#[cfg(not(target_family = "wasm"))]
use crate::server::telemetry::PluginChipTelemetryAction;
use crate::server::telemetry::{PluginChipTelemetryKind, TelemetryEvent};
use crate::terminal::CLIAgent;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::ShellLaunchData;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::cli_agent_sessions::plugin_manager::{
    CliAgentPluginManager, PluginInstallError, PluginModalKind, compare_versions,
    plugin_manager_for, plugin_manager_for_with_shell,
};
use crate::terminal::cli_agent_sessions::{CLIAgentSessionsModel, CLIAgentSessionsModelEvent};
#[cfg(not(target_family = "wasm"))]
use crate::terminal::local_shell::LocalShellState;
use crate::terminal::model::TerminalModel;
use crate::terminal::session_settings::{NotificationsMode, SessionSettings};
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{
    ActionButton, ActionButtonTheme, AdjoinedSide, ButtonSize, TooltipAlignment,
};
#[cfg(not(target_family = "wasm"))]
use crate::view_components::{DismissibleToast, ToastLink};
#[cfg(not(target_family = "wasm"))]
use crate::workspace::{ToastStack, WorkspaceAction};

/// How long to wait after a session starts before offering the install chip for agents without
/// one-click install. Their plugin announces itself asynchronously, so showing the chip
/// immediately would flash it at users who already have the plugin.
const PLUGIN_CHIP_DEBOUNCE: Duration = Duration::from_secs(3);

/// Identifies a chip dismissal per agent, and per host for remote sessions.
fn plugin_chip_key(agent_prefix: &str, remote_host: &Option<String>) -> String {
    match remote_host {
        Some(host) => format!("{agent_prefix}@{host}"),
        None => agent_prefix.to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginChipKind {
    Install,
    Update,
}

impl From<PluginChipKind> for PluginChipTelemetryKind {
    fn from(kind: PluginChipKind) -> Self {
        match kind {
            PluginChipKind::Install => PluginChipTelemetryKind::Install,
            PluginChipKind::Update => PluginChipTelemetryKind::Update,
        }
    }
}

pub enum CliAgentPluginChipEvent {
    #[cfg(not(target_family = "wasm"))]
    OpenInstructionsPane(CLIAgent, PluginModalKind),
}

#[derive(Clone, Copy, Debug)]
pub enum CliAgentPluginChipAction {
    Install,
    Update,
    OpenInstallInstructions,
    OpenUpdateInstructions,
    Dismiss,
}

/// Offers to install or update the notification plugin for the CLI agent running in this pane.
pub struct CliAgentPluginChip {
    terminal_view_id: EntityId,
    terminal_model: Arc<FairMutex<TerminalModel>>,
    install_button: ViewHandle<ActionButton>,
    instructions_button: ViewHandle<ActionButton>,
    update_button: ViewHandle<ActionButton>,
    update_instructions_button: ViewHandle<ActionButton>,
    dismiss_button: ViewHandle<ActionButton>,
    operation_in_progress: bool,
    chip_ready: bool,
}

impl Entity for CliAgentPluginChip {
    type Event = CliAgentPluginChipEvent;
}

impl CliAgentPluginChip {
    pub fn new(
        terminal_view_id: EntityId,
        terminal_model: Arc<FairMutex<TerminalModel>>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let size = ButtonSize::AgentInputButton;

        let install_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Enable notifications", PluginChipTheme)
                .with_icon(Icon::Download)
                .with_tooltip("Install the Warp plugin to enable rich agent notifications")
                .with_size(size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| ctx.dispatch_typed_action(CliAgentPluginChipAction::Install))
        });

        let instructions_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Notifications setup instructions", PluginChipTheme)
                .with_icon(Icon::Info)
                .with_tooltip("View instructions to install the Warp plugin")
                .with_size(size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CliAgentPluginChipAction::OpenInstallInstructions)
                })
        });

        let update_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Update Warp plugin", PluginChipTheme)
                .with_icon(Icon::Download)
                .with_tooltip("A new version of the Warp plugin is available")
                .with_size(size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| ctx.dispatch_typed_action(CliAgentPluginChipAction::Update))
        });

        let update_instructions_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Plugin update instructions", PluginChipTheme)
                .with_icon(Icon::Info)
                .with_tooltip("View instructions to update the Warp plugin")
                .with_size(size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(CliAgentPluginChipAction::OpenUpdateInstructions)
                })
        });

        let dismiss_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", PluginChipTheme)
                .with_icon(Icon::X)
                .with_tooltip("Dismiss")
                .with_size(size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Left)
                .on_click(|ctx| ctx.dispatch_typed_action(CliAgentPluginChipAction::Dismiss))
        });

        ctx.subscribe_to_model(
            &CLIAgentSessionsModel::handle(ctx),
            move |me, _, event, ctx| {
                if event.terminal_view_id() != terminal_view_id {
                    return;
                }
                me.handle_sessions_model_event(event, ctx);
            },
        );

        Self {
            terminal_view_id,
            terminal_model,
            install_button,
            instructions_button,
            update_button,
            update_instructions_button,
            dismiss_button,
            operation_in_progress: false,
            chip_ready: false,
        }
    }

    fn handle_sessions_model_event(
        &mut self,
        event: &CLIAgentSessionsModelEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        if let CLIAgentSessionsModelEvent::Ended { .. } = event {
            self.chip_ready = false;
        }

        if self.plugin_announced_itself(ctx) {
            self.chip_ready = false;
        }

        if let CLIAgentSessionsModelEvent::Started { .. } = event
            && let Some(agent) = self.cli_agent(ctx)
        {
            let label = format!("Enable {} notifications", agent.display_name());
            self.install_button.update(ctx, |button, ctx| {
                button.set_label(label, ctx);
            });
            self.start_chip_debounce(agent, ctx);
        }

        ctx.notify();
    }

    fn cli_agent(&self, app: &AppContext) -> Option<CLIAgent> {
        CLIAgentSessionsModel::as_ref(app)
            .session(self.terminal_view_id)
            .map(|session| session.agent)
    }

    /// Whether a structured plugin has reported in on this session, which is proof it is
    /// installed and leaves the chip nothing to offer.
    fn plugin_announced_itself(&self, app: &AppContext) -> bool {
        CLIAgentSessionsModel::as_ref(app)
            .session(self.terminal_view_id)
            .is_some_and(|session| session.supports_rich_status())
    }

    fn dismiss(&mut self, ctx: &mut ViewContext<Self>) {
        let chip_kind = self.chip_kind(ctx);
        if let Some(agent) = self.cli_agent(ctx)
            && let Some(kind) = chip_kind
        {
            send_telemetry_from_ctx!(
                TelemetryEvent::CLIAgentPluginChipDismissed {
                    cli_agent: agent.into(),
                    chip_kind: kind.into(),
                },
                ctx
            );
        }

        let Some(session) = CLIAgentSessionsModel::as_ref(ctx)
            .session(self.terminal_view_id)
            .cloned()
        else {
            return;
        };
        let chip_key = plugin_chip_key(session.agent.command_prefix(), &session.remote_host);

        match chip_kind {
            Some(PluginChipKind::Update) => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(manager) = plugin_manager_for(session.agent) {
                    let version = manager.minimum_plugin_version().to_owned();
                    SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
                        settings.dismiss_plugin_update_chip(&chip_key, version, ctx);
                    });
                }
            }
            Some(PluginChipKind::Install) | None => {
                SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
                    settings.dismiss_plugin_install_chip(&chip_key, ctx);
                });
            }
        }
        ctx.notify();
    }
}

#[cfg(not(target_family = "wasm"))]
impl CliAgentPluginChip {
    /// Which chip, if any, this pane should be offering right now.
    ///
    /// Returns `None` whenever the plugin is known-good, the user dismissed the chip, or an
    /// operation is already running.
    fn chip_kind(&self, app: &AppContext) -> Option<PluginChipKind> {
        if self.operation_in_progress || !FeatureFlag::HOANotifications.is_enabled() {
            return None;
        }

        if matches!(
            SessionSettings::as_ref(app).notifications.value().mode,
            NotificationsMode::Disabled
        ) {
            return None;
        }

        let session = CLIAgentSessionsModel::as_ref(app).session(self.terminal_view_id)?;
        let manager = plugin_manager_for(session.agent)?;
        let min_version = manager.minimum_plugin_version();
        let chip_key = plugin_chip_key(session.agent.command_prefix(), &session.remote_host);
        let settings = SessionSettings::as_ref(app);

        if self.plugin_announced_itself(app) && manager.supports_update() {
            let needs_update = match &session.plugin_version {
                // A plugin that reports no version predates versioning, so it is always outdated.
                None => true,
                Some(version) => compare_versions(version, min_version).is_lt(),
            };
            if !needs_update {
                return None;
            }
            let dismissed = settings.plugin_update_chip_dismissed_version(&chip_key);
            if !dismissed.is_empty() && compare_versions(dismissed, min_version).is_ge() {
                return None;
            }
            return Some(PluginChipKind::Update);
        }

        if !manager.can_auto_install() && !self.chip_ready {
            return None;
        }

        let install_dismissed = settings.is_plugin_install_chip_dismissed(&chip_key);

        // A remote session's plugin lives on the other host, so the local filesystem checks below
        // would report on the wrong machine.
        if session.is_remote() {
            return (!install_dismissed).then_some(PluginChipKind::Install);
        }

        if manager.is_installed() {
            if manager.needs_update() {
                let dismissed = settings.plugin_update_chip_dismissed_version(&chip_key);
                if !dismissed.is_empty() && compare_versions(dismissed, min_version).is_ge() {
                    return None;
                }
                return Some(PluginChipKind::Update);
            }
            return None;
        }

        (!install_dismissed).then_some(PluginChipKind::Install)
    }

    /// Whether the chip should open written instructions instead of running the install itself.
    fn should_use_manual_mode(&self, app: &AppContext) -> bool {
        let Some(session) = CLIAgentSessionsModel::as_ref(app).session(self.terminal_view_id)
        else {
            return false;
        };

        let command_may_not_be_the_standard_cli = session.custom_command_prefix.is_some();
        if command_may_not_be_the_standard_cli || session.is_remote() {
            return true;
        }

        if CLIAgentSessionsModel::as_ref(app)
            .has_plugin_auto_failed(session.agent, &session.remote_host)
        {
            return true;
        }

        if let Some(manager) = plugin_manager_for(session.agent)
            && (!manager.can_auto_install() || manager.has_local_marketplace_override())
        {
            return true;
        }

        // Auto-install runs against the host's shell config, so for a containerised session it
        // would install the plugin outside the container the agent is actually running in.
        let shell_data = self
            .terminal_model
            .lock()
            .active_shell_launch_data()
            .cloned();
        matches!(shell_data, Some(ShellLaunchData::DockerSandbox { .. }))
    }

    fn start_chip_debounce(&mut self, agent: CLIAgent, ctx: &mut ViewContext<Self>) {
        let Some(manager) = plugin_manager_for(agent) else {
            return;
        };
        if manager.can_auto_install() {
            return;
        }
        ctx.spawn(
            Timer::after(PLUGIN_CHIP_DEBOUNCE),
            |me, _, ctx: &mut ViewContext<Self>| {
                let plugin_connected = CLIAgentSessionsModel::as_ref(ctx)
                    .session(me.terminal_view_id)
                    .is_some_and(|session| session.supports_rich_status());
                if !plugin_connected {
                    me.chip_ready = true;
                    ctx.notify();
                }
            },
        );
    }
}

#[cfg(not(target_family = "wasm"))]
impl CliAgentPluginChip {
    fn handle_install_plugin(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        let success = self
            .cli_agent(ctx)
            .and_then(plugin_manager_for)
            .map(|manager| manager.install_success_message())
            .unwrap_or("Warp plugin installed. Please restart the session to activate.");
        self.run_plugin_operation(
            "Installing Warp plugin...",
            "Failed to install Warp plugin",
            success,
            PluginChipTelemetryKind::Install,
            |manager| async move { manager.install().await },
            ctx,
        )
    }

    fn handle_update_plugin(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        let success = self
            .cli_agent(ctx)
            .and_then(plugin_manager_for)
            .map(|manager| manager.update_success_message())
            .unwrap_or("Warp plugin updated. Please restart the session to activate.");
        self.run_plugin_operation(
            "Updating Warp plugin...",
            "Failed to update Warp plugin",
            success,
            PluginChipTelemetryKind::Update,
            |manager| async move { manager.update().await },
            ctx,
        )
    }

    /// Runs `operation` against a plugin manager bound to this session's shell, reporting
    /// progress and the outcome through the toast stack.
    ///
    /// Returns `false` when the operation could not be started at all, which the caller turns
    /// into a nudge toward the manual instructions.
    fn run_plugin_operation<F, Fut>(
        &mut self,
        progress_toast: &str,
        error_label: &str,
        success_toast: &str,
        operation_kind: PluginChipTelemetryKind,
        operation: F,
        ctx: &mut ViewContext<Self>,
    ) -> bool
    where
        F: FnOnce(Box<dyn CliAgentPluginManager>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), PluginInstallError>> + Send + 'static,
    {
        let Some(agent) = self.cli_agent(ctx) else {
            return false;
        };

        let shell_data = self
            .terminal_model
            .lock()
            .active_shell_launch_data()
            .cloned();
        let (shell_path, shell_type) = match shell_data {
            Some(ShellLaunchData::Executable {
                executable_path,
                shell_type,
            })
            | Some(ShellLaunchData::MSYS2 {
                executable_path,
                shell_type,
            }) => (Some(executable_path), Some(shell_type)),
            None => (None, None),
            Some(ShellLaunchData::WSL { .. }) | Some(ShellLaunchData::DockerSandbox { .. }) => {
                return false;
            }
        };

        // Await the interactive PATH so version-manager-installed CLIs are reachable.
        let path_future = LocalShellState::handle(ctx).update(ctx, |shell_state, ctx| {
            shell_state.get_interactive_path_env_var(ctx)
        });

        self.operation_in_progress = true;
        ctx.notify();

        let window_id = ctx.window_id();
        let toast_id = "cli-agent-plugin-operation".to_owned();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_persistent_toast(
                DismissibleToast::default(progress_toast.to_owned())
                    .with_object_id(toast_id.clone()),
                window_id,
                ctx,
            );
        });

        let error_label = error_label.to_owned();
        let success_toast = success_toast.to_owned();
        ctx.spawn(
            async move {
                let path_env_var = path_future.await;
                let Some(manager) =
                    plugin_manager_for_with_shell(agent, shell_path, shell_type, path_env_var)
                else {
                    return Err((
                        PluginInstallError {
                            message: "No plugin manager available".to_owned(),
                            log: String::new(),
                        },
                        None,
                    ));
                };
                match operation(manager).await {
                    Ok(()) => Ok(()),
                    Err(err) => {
                        let log_path = write_install_log(agent, &err).await;
                        Err((err, log_path))
                    }
                }
            },
            move |me, result, ctx| {
                me.operation_in_progress = false;

                if result.is_ok() {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginOperationSucceeded {
                            cli_agent: agent.into(),
                            operation: operation_kind,
                        },
                        ctx
                    );
                } else {
                    send_telemetry_from_ctx!(
                        TelemetryEvent::CLIAgentPluginOperationFailed {
                            cli_agent: agent.into(),
                            operation: operation_kind,
                        },
                        ctx
                    );
                }

                ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    let toast = match result {
                        Ok(()) => DismissibleToast::success(success_toast.clone()),
                        Err((err, log_path)) => {
                            let remote_host = CLIAgentSessionsModel::as_ref(ctx)
                                .session(me.terminal_view_id)
                                .and_then(|session| session.remote_host.clone());
                            CLIAgentSessionsModel::handle(ctx).update(ctx, |model, _| {
                                model.record_plugin_auto_failure(agent, remote_host);
                            });
                            log::error!("Failed plugin operation log: {}", err.log);
                            let mut toast =
                                DismissibleToast::error(format!("{error_label}: {err}"));
                            report_error!(
                                anyhow::Error::new(err).context("Failed plugin operation"),
                                extra: { "agent" => ?agent }
                            );
                            if let Some(log_path) = log_path {
                                toast = toast.with_link(
                                    ToastLink::new("See logs for details".to_owned())
                                        .with_onclick_action(WorkspaceAction::OpenFilePath {
                                            path: log_path,
                                        }),
                                );
                            }
                            toast
                        }
                    };
                    toast_stack.add_ephemeral_toast(
                        toast.with_object_id(toast_id.clone()),
                        window_id,
                        ctx,
                    );
                });
                ctx.notify();
            },
        );
        true
    }

    /// Records that one-click install is not viable here, so the chip switches to instructions.
    fn record_auto_failure_and_notify(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(agent) = self.cli_agent(ctx) {
            let remote_host = CLIAgentSessionsModel::as_ref(ctx)
                .session(self.terminal_view_id)
                .and_then(|session| session.remote_host.clone());
            CLIAgentSessionsModel::handle(ctx).update(ctx, |model, _| {
                model.record_plugin_auto_failure(agent, remote_host);
            });
        }
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_ephemeral_toast(
                DismissibleToast::error(
                    "Could not automatically install plugin. \
                     Please click the chip again for manual installation steps."
                        .to_owned(),
                ),
                window_id,
                ctx,
            );
        });
        ctx.notify();
    }

    fn open_instructions(&mut self, kind: PluginModalKind, ctx: &mut ViewContext<Self>) {
        let Some(agent) = self.cli_agent(ctx) else {
            return;
        };
        send_telemetry_from_ctx!(
            TelemetryEvent::CLIAgentPluginChipClicked {
                cli_agent: agent.into(),
                action: match kind {
                    PluginModalKind::Install => PluginChipTelemetryAction::InstallInstructions,
                    PluginModalKind::Update => PluginChipTelemetryAction::UpdateInstructions,
                },
            },
            ctx
        );
        ctx.emit(CliAgentPluginChipEvent::OpenInstructionsPane(agent, kind));
    }
}

/// Writes the detailed plugin operation log to a temp file, returning its path on success.
#[cfg(not(target_family = "wasm"))]
async fn write_install_log(agent: CLIAgent, err: &PluginInstallError) -> Option<PathBuf> {
    let log_path = std::env::temp_dir().join("warp-plugin-install.log");
    let contents = format!(
        "Warp plugin installation — {agent:?}\n\
         \n\
         {log}",
        log = err.log,
    );
    fs::write(&log_path, contents).await.ok()?;
    Some(log_path)
}

#[cfg(target_family = "wasm")]
impl CliAgentPluginChip {
    fn chip_kind(&self, _app: &AppContext) -> Option<PluginChipKind> {
        None
    }

    fn should_use_manual_mode(&self, _app: &AppContext) -> bool {
        false
    }

    fn start_chip_debounce(&mut self, _agent: CLIAgent, _ctx: &mut ViewContext<Self>) {}
}

impl TypedActionView for CliAgentPluginChip {
    type Action = CliAgentPluginChipAction;

    fn handle_action(&mut self, action: &CliAgentPluginChipAction, ctx: &mut ViewContext<Self>) {
        match action {
            CliAgentPluginChipAction::Install => {
                #[cfg(not(target_family = "wasm"))]
                {
                    if let Some(agent) = self.cli_agent(ctx) {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::CLIAgentPluginChipClicked {
                                cli_agent: agent.into(),
                                action: PluginChipTelemetryAction::Install,
                            },
                            ctx
                        );
                    }
                    if !self.handle_install_plugin(ctx) {
                        self.record_auto_failure_and_notify(ctx);
                    }
                }
            }
            CliAgentPluginChipAction::Update => {
                #[cfg(not(target_family = "wasm"))]
                {
                    if let Some(agent) = self.cli_agent(ctx) {
                        send_telemetry_from_ctx!(
                            TelemetryEvent::CLIAgentPluginChipClicked {
                                cli_agent: agent.into(),
                                action: PluginChipTelemetryAction::Update,
                            },
                            ctx
                        );
                    }
                    if !self.handle_update_plugin(ctx) {
                        self.record_auto_failure_and_notify(ctx);
                    }
                }
            }
            CliAgentPluginChipAction::OpenInstallInstructions => {
                #[cfg(not(target_family = "wasm"))]
                self.open_instructions(PluginModalKind::Install, ctx);
            }
            CliAgentPluginChipAction::OpenUpdateInstructions => {
                #[cfg(not(target_family = "wasm"))]
                self.open_instructions(PluginModalKind::Update, ctx);
            }
            CliAgentPluginChipAction::Dismiss => self.dismiss(ctx),
        }
    }
}

/// Keeps the chip readable over an alt-screen CLI agent by tinting the surface fill rather than
/// relying on the pane's background.
struct PluginChipTheme;

impl ActionButtonTheme for PluginChipTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        let green = appearance.theme().ansi_fg_green();
        let base = appearance.theme().surface_1();
        Some(if hovered {
            base.blend(&Fill::Solid(green).with_opacity(30))
        } else {
            base.blend(&Fill::Solid(green).with_opacity(15))
        })
    }

    fn text_color(
        &self,
        _hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        appearance.theme().ansi_fg_green()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        let green = appearance.theme().ansi_fg_green();
        Some(ColorU::new(green.r, green.g, green.b, 80))
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

impl View for CliAgentPluginChip {
    fn ui_name() -> &'static str {
        "CliAgentPluginChip"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let Some(chip_kind) = self.chip_kind(app) else {
            return Empty::new().finish();
        };

        let manual = self.should_use_manual_mode(app);
        let chip = match (chip_kind, manual) {
            (PluginChipKind::Install, false) => ChildView::new(&self.install_button).finish(),
            (PluginChipKind::Install, true) => ChildView::new(&self.instructions_button).finish(),
            (PluginChipKind::Update, false) => ChildView::new(&self.update_button).finish(),
            (PluginChipKind::Update, true) => {
                ChildView::new(&self.update_instructions_button).finish()
            }
        };

        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(chip)
            .with_child(ChildView::new(&self.dismiss_button).finish())
            .finish()
    }
}

#[cfg(test)]
#[path = "cli_agent_plugin_chip_tests.rs"]
mod tests;
