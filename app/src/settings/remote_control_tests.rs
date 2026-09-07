use settings::{PrivatePreferences, PublicPreferences, Setting as _, SettingsManager};
use warpui::{AppContext, SingletonEntity as _};
use warpui_extras::user_preferences;

use super::{
    DEFAULT_REMOTE_CONTROL_PORT, MIN_REMOTE_CONTROL_PORT, RemoteControlSettings, clamp_port,
};

fn install_settings_backends(ctx: &mut AppContext) {
    ctx.add_singleton_model(|_| {
        PublicPreferences::new(Box::<user_preferences::in_memory::InMemoryPreferences>::default())
    });
    ctx.add_singleton_model(|_| {
        PrivatePreferences::new(Box::<user_preferences::in_memory::InMemoryPreferences>::default())
    });
    ctx.add_singleton_model(|_| SettingsManager::default());
    RemoteControlSettings::register(ctx);
}

#[test]
fn defaults_are_off_on_the_documented_port() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);
        app.read(|ctx| {
            let settings = RemoteControlSettings::as_ref(ctx);
            assert!(!settings.is_enabled());
            assert_eq!(settings.port(), DEFAULT_REMOTE_CONTROL_PORT);
            assert!(!settings.allows_lan_access());
        });
    });
}

#[test]
fn port_is_clamped_into_the_unprivileged_range() {
    assert_eq!(clamp_port(0), MIN_REMOTE_CONTROL_PORT);
    assert_eq!(clamp_port(80), MIN_REMOTE_CONTROL_PORT);
    assert_eq!(clamp_port(1023), MIN_REMOTE_CONTROL_PORT);
    assert_eq!(clamp_port(1024), 1024);
    assert_eq!(clamp_port(7777), 7777);
    assert_eq!(clamp_port(u16::MAX), u16::MAX);
}

#[test]
fn a_hand_edited_privileged_port_is_clamped_on_read() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);
        app.update(|ctx| {
            RemoteControlSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings.remote_control_port.set_value(80, ctx)
            })
        })
        .expect("port update should succeed");
        app.read(|ctx| {
            assert_eq!(
                RemoteControlSettings::as_ref(ctx).port(),
                MIN_REMOTE_CONTROL_PORT
            );
        });
    });
}

#[test]
fn enabling_and_disabling_round_trips() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);
        app.update(|ctx| {
            RemoteControlSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings.remote_control_enabled.set_value(true, ctx)
            })
        })
        .expect("enabling should succeed");
        app.read(|ctx| assert!(RemoteControlSettings::as_ref(ctx).is_enabled()));

        app.update(|ctx| {
            RemoteControlSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings.remote_control_enabled.set_value(false, ctx)
            })
        })
        .expect("disabling should succeed");
        app.read(|ctx| assert!(!RemoteControlSettings::as_ref(ctx).is_enabled()));
    });
}

#[test]
fn lan_access_defaults_off_and_can_be_turned_on() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);
        app.read(|ctx| assert!(!RemoteControlSettings::as_ref(ctx).allows_lan_access()));
        app.update(|ctx| {
            RemoteControlSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .remote_control_allow_lan_access
                    .set_value(true, ctx)
            })
        })
        .expect("enabling LAN access should succeed");
        app.read(|ctx| assert!(RemoteControlSettings::as_ref(ctx).allows_lan_access()));
    });
}
