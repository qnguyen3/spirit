use warp_core::user_preferences::GetUserPreferences as _;
use warpui::App;

use super::{HAS_COMPLETED_ONBOARDING_KEY, has_completed_local_onboarding};

fn initialize_app(app: &mut App) {
    app.update(crate::settings::init_and_register_user_preferences);
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

/// The startup state machine is `HasCompletedOnboarding ? Terminal : Onboarding`,
/// so a fresh install must report the onboarding slides as unseen.
#[test]
fn a_fresh_install_has_not_completed_onboarding() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        app.read(|ctx| assert!(!has_completed_local_onboarding(ctx)));
    });
}

#[test]
fn the_completed_onboarding_marker_round_trips_through_user_preferences() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        set_local_onboarding_completed(&mut app, true);
        app.read(|ctx| assert!(has_completed_local_onboarding(ctx)));

        set_local_onboarding_completed(&mut app, false);
        app.read(|ctx| assert!(!has_completed_local_onboarding(ctx)));
    });
}
