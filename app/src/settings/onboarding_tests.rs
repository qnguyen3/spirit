use onboarding::{SelectedSettings, UICustomizationSettings};
use warp_core::features::FeatureFlag;
use warpui::{App, SingletonEntity};

use crate::auth::AuthStateProvider;
use crate::network::NetworkStatus;
use crate::settings::{CodeSettings, PrivacySettings, apply_account_first_onboarding_settings};
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspace::tab_settings::TabSettings;
use crate::workspaces::team_tester::TeamTesterStatus;
use crate::workspaces::user_workspaces::UserWorkspaces;

#[test]
fn account_first_settings_apply_ui_choices() {
    let _account_first = FeatureFlag::AccountFirstOnboarding.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
        app.add_singleton_model(|_| NetworkStatus::new());
        app.add_singleton_model(TeamTesterStatus::mock);
        app.add_singleton_model(PrivacySettings::mock);
        app.add_singleton_model(UserWorkspaces::default_mock);

        let selected_settings = SelectedSettings {
            ui_customization: Some(UICustomizationSettings {
                use_vertical_tabs: false,
                show_conversation_history: false,
                show_project_explorer: true,
                show_global_search: false,
                show_code_review_button: true,
            }),
            cli_agent_toolbar_enabled: true,
            show_agent_notifications: true,
        };

        app.update(|ctx| {
            apply_account_first_onboarding_settings(&selected_settings, ctx);
        });
        app.read(|ctx| {
            assert!(!*TabSettings::as_ref(ctx).use_vertical_tabs);
            assert!(*TabSettings::as_ref(ctx).show_code_review_button);
            assert!(*CodeSettings::as_ref(ctx).show_project_explorer);
            assert!(!*CodeSettings::as_ref(ctx).show_global_search);
        });
    });
}
