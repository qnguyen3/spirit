use onboarding::{SelectedSettings, UICustomizationSettings};
use settings::Setting as _;
use warpui::{App, SingletonEntity};

use crate::network::NetworkStatus;
use crate::settings::{
    AgentApprovalMode, CLIAgentSettings, CodeSettings, PrivacySettings, apply_onboarding_settings,
};
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspace::tab_settings::TabSettings;

#[test]
fn onboarding_settings_apply_ui_choices() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.add_singleton_model(|_| NetworkStatus::new());
        app.add_singleton_model(PrivacySettings::mock);

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
            agent_approval_yolo: true,
        };

        app.update(|ctx| {
            apply_onboarding_settings(&selected_settings, ctx);
        });
        app.read(|ctx| {
            assert!(!*TabSettings::as_ref(ctx).use_vertical_tabs);
            assert!(*TabSettings::as_ref(ctx).show_code_review_button);
            assert!(*CodeSettings::as_ref(ctx).show_project_explorer);
            assert!(!*CodeSettings::as_ref(ctx).show_global_search);
            assert!(!*CodeSettings::as_ref(ctx).show_agent_session_history);
            assert_eq!(
                *CLIAgentSettings::as_ref(ctx).agent_approval_mode.value(),
                AgentApprovalMode::Yolo
            );
        });
    });
}

#[test]
fn onboarding_settings_apply_normal_agent_approval_mode() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.add_singleton_model(|_| NetworkStatus::new());
        app.add_singleton_model(PrivacySettings::mock);

        let selected_settings = SelectedSettings {
            ui_customization: None,
            cli_agent_toolbar_enabled: true,
            show_agent_notifications: true,
            agent_approval_yolo: false,
        };

        app.update(|ctx| {
            apply_onboarding_settings(&selected_settings, ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                *CLIAgentSettings::as_ref(ctx).agent_approval_mode.value(),
                AgentApprovalMode::Normal
            );
        });
    });
}
