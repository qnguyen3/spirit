use onboarding::{SelectedSettings, UICustomizationSettings};
use settings::Setting as _;
use warp_errors::report_if_error;
use warpui::{AppContext, SingletonEntity as _};

use crate::settings::{AgentApprovalMode, CLIAgentSettings, CodeSettings};
use crate::workspace::tab_settings::TabSettings;

/// Applies onboarding settings based on the user's selected mode.
pub(crate) fn apply_onboarding_settings(
    selected_settings: &SelectedSettings,
    app: &mut AppContext,
) {
    if let Some(ui) = &selected_settings.ui_customization {
        apply_ui_customization_settings(ui, app);
    }

    apply_agent_approval_mode(selected_settings.agent_approval_yolo, app);
}

fn apply_agent_approval_mode(yolo: bool, app: &mut AppContext) {
    let mode = if yolo {
        AgentApprovalMode::Yolo
    } else {
        AgentApprovalMode::Normal
    };

    CLIAgentSettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(settings.agent_approval_mode.set_value(mode, ctx));
    });
}

/// Applies the explicit UI customization settings chosen during the
/// "Customize your UI" onboarding slide.
fn apply_ui_customization_settings(ui: &UICustomizationSettings, app: &mut AppContext) {
    TabSettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(
            settings
                .use_vertical_tabs
                .set_value(ui.use_vertical_tabs, ctx)
        );
        report_if_error!(
            settings
                .show_code_review_button
                .set_value(ui.show_code_review_button, ctx)
        );
    });

    CodeSettings::handle(app).update(app, |settings, ctx| {
        report_if_error!(
            settings
                .show_project_explorer
                .set_value(ui.show_project_explorer, ctx)
        );
        report_if_error!(
            settings
                .show_global_search
                .set_value(ui.show_global_search, ctx)
        );
        report_if_error!(
            settings
                .show_agent_session_history
                .set_value(ui.show_conversation_history, ctx)
        );
    });
}

#[cfg(test)]
#[path = "onboarding_tests.rs"]
mod tests;
