use onboarding::{SelectedSettings, UICustomizationSettings};
use settings::Setting as _;
use warp_errors::report_if_error;
use warpui::{AppContext, SingletonEntity as _};

use crate::settings::CodeSettings;
use crate::workspace::tab_settings::TabSettings;

pub(crate) fn apply_account_first_onboarding_settings(
    selected_settings: &SelectedSettings,
    app: &mut AppContext,
) {
    apply_onboarding_settings(selected_settings, app);
}

/// Applies onboarding settings based on the user's selected mode.
pub(crate) fn apply_onboarding_settings(
    selected_settings: &SelectedSettings,
    app: &mut AppContext,
) {
    if let Some(ui) = &selected_settings.ui_customization {
        apply_ui_customization_settings(ui, app);
    }
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
    });
}

#[cfg(test)]
#[path = "onboarding_tests.rs"]
mod tests;
