use anyhow::Result;
use remote_control::auth::{AccessToken, PairedDevice};
use settings::macros::define_settings_group;
use settings::{SecureSetting, Setting, SettingSurfaces, SupportedPlatforms, SyncToCloud};
use warpui::{AppContext, ModelContext, SingletonEntity};
use warpui_extras::secure_storage;

const REMOTE_CONTROL_ACCESS_TOKEN_STORAGE_KEY: &str = "RemoteControlAccessToken";

define_settings_group!(RemoteControlSecrets, settings: [
    remote_control_access_token: RemoteControlAccessTokenSetting,
    remote_control_paired_devices: RemoteControlPairedDevicesSetting {
        type: String,
        default: String::new(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: SettingSurfaces::GUI,
        private: true,
        storage_key: "RemoteControlPairedDevices",
        description: "Hashed identifiers of the browsers paired with Remote Control.",
    },
]);

pub struct RemoteControlAccessTokenSetting {
    inner: Option<String>,
    is_explicitly_set: bool,
}

impl RemoteControlAccessTokenSetting {
    fn emit_changed(
        ctx: &mut ModelContext<RemoteControlSecrets>,
        change_event_reason: settings::ChangeEventReason,
    ) {
        ctx.emit(
            RemoteControlSecretsChangedEvent::RemoteControlAccessTokenSetting {
                change_event_reason,
            },
        );
    }
}

impl SecureSetting for RemoteControlAccessTokenSetting {
    fn write_secure_storage_value(
        storage: &dyn secure_storage::SecureStorage,
        key: &str,
        value: &str,
    ) -> Result<(), secure_storage::Error> {
        storage.write_value_with_owner_only_fallback(key, value)
    }
}

impl Setting for RemoteControlAccessTokenSetting {
    type Group = RemoteControlSecrets;
    type Value = Option<String>;

    fn new(value: Option<Self::Value>) -> Self {
        match value {
            Some(value) => Self {
                inner: value,
                is_explicitly_set: true,
            },
            None => Self {
                inner: Self::default_value(),
                is_explicitly_set: false,
            },
        }
    }

    fn setting_name() -> &'static str {
        "RemoteControlAccessTokenSetting"
    }

    fn storage_key() -> &'static str {
        REMOTE_CONTROL_ACCESS_TOKEN_STORAGE_KEY
    }

    fn supported_platforms() -> SupportedPlatforms {
        SupportedPlatforms::DESKTOP
    }

    fn sync_to_cloud() -> SyncToCloud {
        SyncToCloud::Never
    }

    fn is_private() -> bool {
        true
    }

    fn value(&self) -> &Self::Value {
        &self.inner
    }

    fn clear_value(&mut self, ctx: &mut ModelContext<Self::Group>) -> Result<()> {
        Self::clear_from_secure_storage(ctx)?;
        self.inner = Self::default_value();
        self.is_explicitly_set = false;
        Self::emit_changed(ctx, settings::ChangeEventReason::Clear);
        Ok(())
    }

    fn load_value(
        &mut self,
        new_value: Self::Value,
        explicitly_set: bool,
        ctx: &mut ModelContext<Self::Group>,
    ) -> Result<()> {
        if self.value() != &new_value || self.is_explicitly_set != explicitly_set {
            self.inner = new_value;
            self.is_explicitly_set = explicitly_set;
            Self::emit_changed(ctx, settings::ChangeEventReason::LocalChange);
        }
        Ok(())
    }

    fn set_value_from_cloud_sync(
        &mut self,
        _: Self::Value,
        _: &mut ModelContext<Self::Group>,
    ) -> Result<()> {
        Ok(())
    }

    fn set_value(
        &mut self,
        new_value: Self::Value,
        ctx: &mut ModelContext<Self::Group>,
    ) -> Result<()> {
        let changed_in_storage = Self::write_to_secure_storage(&new_value, ctx)?;
        if self.value() != &new_value || changed_in_storage {
            self.inner = new_value;
            self.is_explicitly_set = true;
            Self::emit_changed(ctx, settings::ChangeEventReason::LocalChange);
        }
        Ok(())
    }

    fn default_value() -> Self::Value {
        None
    }

    fn new_from_storage(ctx: &mut AppContext) -> Self {
        Self::new(Self::read_from_secure_storage(ctx))
    }

    fn is_supported_on_current_platform(&self) -> bool {
        SupportedPlatforms::DESKTOP.matches_current_platform()
    }

    fn is_value_explicitly_set(&self) -> bool {
        self.is_explicitly_set
    }
}

impl RemoteControlSecrets {
    pub fn access_token(&self) -> Option<AccessToken> {
        self.remote_control_access_token
            .value()
            .as_deref()
            .and_then(|secret| AccessToken::parse(secret).ok())
    }

    pub fn paired_devices(&self) -> Vec<PairedDevice> {
        let raw = self.remote_control_paired_devices.value();
        if raw.is_empty() {
            return Vec::new();
        }
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn ensure_access_token(ctx: &mut AppContext) -> Option<AccessToken> {
        if let Some(token) = Self::as_ref(ctx).access_token() {
            return Some(token);
        }
        Self::store_access_token(AccessToken::generate(), ctx)
    }

    pub fn rotate_access_token(ctx: &mut AppContext) -> Option<AccessToken> {
        Self::store_access_token(AccessToken::generate(), ctx)
    }

    pub fn set_paired_devices(devices: &[PairedDevice], ctx: &mut AppContext) {
        let encoded = serde_json::to_string(devices).unwrap_or_default();
        let result = Self::handle(ctx).update(ctx, |secrets, ctx| {
            secrets
                .remote_control_paired_devices
                .set_value(encoded, ctx)
        });
        warp_errors::report_if_error!(result);
    }

    fn store_access_token(token: AccessToken, ctx: &mut AppContext) -> Option<AccessToken> {
        let secret = token.reveal().to_owned();
        let result = Self::handle(ctx).update(ctx, |secrets, ctx| {
            secrets
                .remote_control_access_token
                .set_value(Some(secret), ctx)
        });
        match result {
            Ok(()) => Some(token),
            Err(error) => {
                log::warn!("Failed to persist the Remote Control access token: {error}");
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "remote_control_secrets_tests.rs"]
mod tests;
