use onboarding::{
    AgentOnboardingView, OnboardingAuthState, SelectedSettings, UICustomizationSettings,
};
use warp_core::features::FeatureFlag;
use warp_core::user_preferences::GetUserPreferences as _;
use warpui::elements::Empty;
use warpui::platform::WindowStyle;
use warpui::{
    App, AppContext, Element, Entity, EntityId, SingletonEntity, TypedActionView, View, ViewHandle,
};

use super::{
    AuthOnboardingState, AuthOnboardingTarget, HAS_COMPLETED_ONBOARDING_KEY, NewWorkspaceSource,
    RootView, WorkspaceArgs, has_completed_local_onboarding, refresh_pending_onboarding_choices,
    requires_post_onboarding_login,
};
use crate::GlobalResourceHandles;
use crate::appearance::Appearance;
use crate::auth::AuthStateProvider;
use crate::auth::auth_manager::AuthManager;
use crate::auth::login_slide::{LoginSlideSource, LoginSlideView};
use crate::server::server_api::ServerApiProvider;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::themes::onboarding_theme_picker_themes;

fn initialize_app(app: &mut App) {
    app.update(crate::settings::init_and_register_user_preferences);
    app.add_singleton_model(|_ctx| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AuthManager::new_for_test);
}

fn set_local_onboarding_completed(app: &mut App, completed: bool) {
    app.update(|ctx| {
        ctx.private_user_preferences()
            .write_value(
                HAS_COMPLETED_ONBOARDING_KEY,
                serde_json::to_string(&completed).unwrap(),
            )
            .unwrap();
    });
}

#[test]
fn account_first_requires_login_for_logged_out_users() {
    let _account_first = FeatureFlag::AccountFirstOnboarding.override_enabled(true);

    assert!(requires_post_onboarding_login(false));
    assert!(!requires_post_onboarding_login(true));
}

#[test]
fn fallback_flow_never_requires_login_after_onboarding() {
    let _account_first = FeatureFlag::AccountFirstOnboarding.override_enabled(false);

    assert!(!requires_post_onboarding_login(false));
    assert!(!requires_post_onboarding_login(true));
}

#[test]
fn refreshing_pending_onboarding_choices_replaces_stale_settings() {
    let settings = |use_vertical_tabs| SelectedSettings {
        ui_customization: Some(UICustomizationSettings {
            use_vertical_tabs,
            show_conversation_history: false,
            show_project_explorer: true,
            show_global_search: false,
            show_code_review_button: true,
        }),
        cli_agent_toolbar_enabled: true,
        show_agent_notifications: false,
    };

    let mut pending_settings = Some(settings(false));
    let mut pending_tutorial = None;
    let latest_settings = settings(true);

    refresh_pending_onboarding_choices(
        &latest_settings,
        &mut pending_settings,
        &mut pending_tutorial,
    );

    let Some(ui) = pending_settings.and_then(|settings| settings.ui_customization) else {
        panic!("latest terminal settings should replace the pending snapshot");
    };
    assert!(ui.use_vertical_tabs);
    assert!(pending_tutorial.is_some());
}

/// Regression test for the bug fixed by introducing
/// `RootView::sync_local_onboarding_to_server`: when a user completed onboarding
/// pre-login and later authenticated via a non-login-slide entrypoint (i.e. while
/// already in `Terminal` state), the server-side `is_onboarded` flag was never
/// flipped. The helper runs unconditionally on `AuthComplete` and must flip the
/// flag when all preconditions hold.
#[test]
fn test_sync_flips_server_is_onboarded_when_local_onboarding_completed() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // Seed the "has_completed_local_onboarding" preference and make the user
        // appear not yet onboarded on the server. The default test user is
        // non-anonymous, so the guards in the helper won't short-circuit.
        set_local_onboarding_completed(&mut app, true);
        app.update(|ctx| {
            AuthStateProvider::as_ref(ctx).get().set_is_onboarded(false);
            assert!(has_completed_local_onboarding(ctx));
            assert_eq!(
                AuthStateProvider::as_ref(ctx).get().is_onboarded(),
                Some(false)
            );
        });

        app.update(|ctx| {
            let auth_state = AuthStateProvider::as_ref(ctx).get().clone();
            RootView::sync_local_onboarding_to_server(&auth_state, ctx);
        });

        app.read(|ctx| {
            assert_eq!(
                AuthStateProvider::as_ref(ctx).get().is_onboarded(),
                Some(true),
                "sync should have invoked AuthManager::set_user_onboarded"
            );
        });
    });
}

/// If the user hasn't completed local onboarding, the helper must leave the
/// server-side flag untouched — onboarding hasn't actually happened yet.
#[test]
fn test_sync_noop_when_local_onboarding_not_completed() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // Do not set HAS_COMPLETED_ONBOARDING_KEY; it defaults to false.
        app.update(|ctx| {
            AuthStateProvider::as_ref(ctx).get().set_is_onboarded(false);
        });

        app.update(|ctx| {
            let auth_state = AuthStateProvider::as_ref(ctx).get().clone();
            RootView::sync_local_onboarding_to_server(&auth_state, ctx);
        });

        app.read(|ctx| {
            assert_eq!(
                AuthStateProvider::as_ref(ctx).get().is_onboarded(),
                Some(false),
                "sync should not have changed is_onboarded when local onboarding is incomplete"
            );
        });
    });
}

/// The server-side flag should also be left untouched when it is already set,
/// even if local onboarding is complete — avoids redundant server calls.
#[test]
fn test_sync_noop_when_already_onboarded_on_server() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        set_local_onboarding_completed(&mut app, true);
        app.update(|ctx| {
            // User::test() defaults to is_onboarded = true; assert that and
            // leave it in place.
            assert_eq!(
                AuthStateProvider::as_ref(ctx).get().is_onboarded(),
                Some(true)
            );
        });

        app.update(|ctx| {
            let auth_state = AuthStateProvider::as_ref(ctx).get().clone();
            RootView::sync_local_onboarding_to_server(&auth_state, ctx);
        });

        app.read(|ctx| {
            assert_eq!(
                AuthStateProvider::as_ref(ctx).get().is_onboarded(),
                Some(true)
            );
        });
    });
}

struct SsoLinkTestHarnessView {
    login_slide_view: ViewHandle<LoginSlideView>,
    onboarding_view: ViewHandle<AgentOnboardingView>,
}

impl Entity for SsoLinkTestHarnessView {
    type Event = ();
}

impl View for SsoLinkTestHarnessView {
    fn ui_name() -> &'static str {
        "SsoLinkTestHarnessView"
    }

    fn render(&self, _app: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

impl TypedActionView for SsoLinkTestHarnessView {
    type Action = ();
}

/// Regression test: completing browser auth with `needs_sso_link = true` while
/// a pre-terminal onboarding state was showing (`Onboarding`, `LoginSlide`, or
/// `PostAuthOnboarding`) used to silently no-op in `show_needs_sso_link_view`,
/// leaving the UI stuck on the login slide ("Sign in on your browser to
/// continue") instead of showing the SSO blocker. Each of those states must
/// convert to `NeedsSsoLink` and preserve its target.
#[test]
fn test_show_needs_sso_link_view_blocks_pre_terminal_onboarding_states() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.add_singleton_model(|_ctx| ServerApiProvider::new_for_test());
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
        app.add_singleton_model(AuthManager::new_for_test);
        app.add_singleton_model(|_| Appearance::mock());
        app.add_singleton_model(|_| KeybindingChangedNotifier::new());

        let (_, harness) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
            let login_slide_view = ctx.add_typed_action_view(|ctx| {
                LoginSlideView::new("Dark", false, LoginSlideSource::OnboardingFlow, ctx)
            });
            let onboarding_view = ctx.add_typed_action_view(|ctx| {
                AgentOnboardingView::new(
                    onboarding_theme_picker_themes(),
                    false,
                    OnboardingAuthState::LoggedOut,
                    ctx,
                )
            });
            SsoLinkTestHarnessView {
                login_slide_view,
                onboarding_view,
            }
        });

        let (login_slide_view, onboarding_view) = app.read(|ctx| {
            let harness = harness.as_ref(ctx);
            (
                harness.login_slide_view.clone(),
                harness.onboarding_view.clone(),
            )
        });

        fn workspace_target(app: &mut App) -> (AuthOnboardingTarget, EntityId) {
            let global_resource_handles = GlobalResourceHandles::mock(app);
            let marker = global_resource_handles.tips_completed.id();
            let target = AuthOnboardingTarget::Workspace(Box::new(WorkspaceArgs {
                global_resource_handles,
                server_time: None,
                workspace_setting: NewWorkspaceSource::Empty {
                    previous_active_window: None,
                    shell: None,
                },
            }));
            (target, marker)
        }

        fn assert_becomes_needs_sso_link(
            mut state: AuthOnboardingState,
            marker: EntityId,
            case: &str,
        ) {
            state.show_needs_sso_link_view();
            match state {
                AuthOnboardingState::NeedsSsoLink(AuthOnboardingTarget::Workspace(args)) => {
                    assert_eq!(
                        args.global_resource_handles.tips_completed.id(),
                        marker,
                        "{case}: the pre-login target must be preserved"
                    );
                }
                _ => panic!("{case}: expected transition to NeedsSsoLink"),
            }
        }

        let (target, marker) = workspace_target(&mut app);
        assert_becomes_needs_sso_link(
            AuthOnboardingState::LoginSlide {
                login_slide_view: login_slide_view.clone(),
                onboarding_view: onboarding_view.clone(),
                target,
            },
            marker,
            "LoginSlide",
        );

        let (target, marker) = workspace_target(&mut app);
        assert_becomes_needs_sso_link(
            AuthOnboardingState::Onboarding {
                onboarding_view: onboarding_view.clone(),
                target,
            },
            marker,
            "Onboarding",
        );
    });
}
