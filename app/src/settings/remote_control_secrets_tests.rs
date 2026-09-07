use std::collections::HashMap;
use std::sync::Mutex;

use remote_control::auth::{DeviceId, PairedDevice, SessionId, hash_session_id};
use settings::{PrivatePreferences, PublicPreferences, Setting as _, SettingsManager, SyncToCloud};
use warpui::{AppContext, SingletonEntity as _};
use warpui_extras::secure_storage::{self, AppContextExt as _};
use warpui_extras::user_preferences;

use super::{RemoteControlAccessTokenSetting, RemoteControlSecrets};

#[derive(Default)]
struct InMemorySecureStorage {
    values: Mutex<HashMap<String, String>>,
}

impl secure_storage::SecureStorage for InMemorySecureStorage {
    fn write_value(&self, key: &str, value: &str) -> Result<(), secure_storage::Error> {
        match self.values.lock() {
            Ok(mut values) => {
                values.insert(key.to_owned(), value.to_owned());
                Ok(())
            }
            Err(err) => Err(secure_storage::Error::Unknown(anyhow::anyhow!(
                err.to_string()
            ))),
        }
    }

    fn read_value(&self, key: &str) -> Result<String, secure_storage::Error> {
        match self.values.lock() {
            Ok(values) => values
                .get(key)
                .cloned()
                .ok_or(secure_storage::Error::NotFound),
            Err(err) => Err(secure_storage::Error::Unknown(anyhow::anyhow!(
                err.to_string()
            ))),
        }
    }

    fn remove_value(&self, key: &str) -> Result<(), secure_storage::Error> {
        match self.values.lock() {
            Ok(mut values) => {
                values.remove(key);
                Ok(())
            }
            Err(err) => Err(secure_storage::Error::Unknown(anyhow::anyhow!(
                err.to_string()
            ))),
        }
    }
}

fn install_settings_backends(ctx: &mut AppContext) {
    ctx.add_singleton_model(|_| {
        PublicPreferences::new(Box::<user_preferences::in_memory::InMemoryPreferences>::default())
    });
    ctx.add_singleton_model(|_| {
        PrivatePreferences::new(Box::<user_preferences::in_memory::InMemoryPreferences>::default())
    });
    ctx.add_singleton_model(|_| SettingsManager::default());
    ctx.add_singleton_model(|_| -> secure_storage::Model {
        Box::<InMemorySecureStorage>::default()
    });
    RemoteControlSecrets::register(ctx);
}

fn paired_device(label: &str) -> PairedDevice {
    PairedDevice {
        id: DeviceId::generate(),
        session_hash: hash_session_id(&SessionId::generate()),
        label: label.to_owned(),
        created_ts: 10,
        last_seen_ts: 20,
    }
}

#[test]
fn the_access_token_is_generated_once_and_kept_in_secure_storage() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);

        let first = app
            .update(RemoteControlSecrets::ensure_access_token)
            .expect("a token should be generated");
        let second = app
            .update(RemoteControlSecrets::ensure_access_token)
            .expect("the stored token should be reused");
        assert_eq!(first, second);

        app.read(|ctx| {
            let stored = ctx
                .secure_storage()
                .read_value(RemoteControlAccessTokenSetting::storage_key())
                .expect("the token should live in secure storage");
            assert!(stored.contains(first.reveal()));

            let public = RemoteControlAccessTokenSetting::preferences_for_setting(ctx)
                .read_value(RemoteControlAccessTokenSetting::storage_key())
                .expect("private preferences are readable");
            assert!(public.is_none());
        });
    });
}

#[test]
fn rotation_replaces_the_stored_token() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);
        let original = app
            .update(RemoteControlSecrets::ensure_access_token)
            .expect("a token should be generated");
        let rotated = app
            .update(RemoteControlSecrets::rotate_access_token)
            .expect("rotation should produce a token");
        assert_ne!(original, rotated);
        app.read(|ctx| {
            assert_eq!(
                RemoteControlSecrets::as_ref(ctx).access_token(),
                Some(rotated.clone())
            );
        });
    });
}

#[test]
fn paired_devices_round_trip_through_the_private_store() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);
        app.read(|ctx| {
            assert!(
                RemoteControlSecrets::as_ref(ctx)
                    .paired_devices()
                    .is_empty()
            );
        });

        let devices = vec![
            paired_device("iPhone · Safari"),
            paired_device("Mac · Chrome"),
        ];
        let expected = devices.clone();
        app.update(|ctx| RemoteControlSecrets::set_paired_devices(&devices, ctx));

        app.read(|ctx| {
            assert_eq!(RemoteControlSecrets::as_ref(ctx).paired_devices(), expected);
        });
    });
}

#[test]
fn corrupt_paired_device_json_reads_as_empty() {
    warpui::App::test((), |mut app| async move {
        app.update(install_settings_backends);
        app.update(|ctx| {
            RemoteControlSecrets::handle(ctx).update(ctx, |secrets, ctx| {
                secrets
                    .remote_control_paired_devices
                    .set_value("not json".to_owned(), ctx)
            })
        })
        .expect("writing the raw value should succeed");
        app.read(|ctx| {
            assert!(
                RemoteControlSecrets::as_ref(ctx)
                    .paired_devices()
                    .is_empty()
            );
        });
    });
}

#[test]
fn secrets_are_private_and_never_cloud_synced() {
    assert_eq!(
        RemoteControlAccessTokenSetting::sync_to_cloud(),
        SyncToCloud::Never
    );
    assert!(RemoteControlAccessTokenSetting::is_private());
}
