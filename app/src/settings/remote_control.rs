use settings::macros::define_settings_group;
use settings::{SettingSurfaces, SupportedPlatforms, SyncToCloud};

pub const DEFAULT_REMOTE_CONTROL_PORT: u16 = 7777;
pub const MIN_REMOTE_CONTROL_PORT: u16 = 1024;
pub const MAX_REMOTE_CONTROL_PORT: u16 = u16::MAX;

define_settings_group!(RemoteControlSettings, settings: [
    remote_control_enabled: RemoteControlEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: SettingSurfaces::GUI,
        private: false,
        toml_path: "remote_control.enabled",
        description: "Whether Spirit serves the Remote Control web app on this machine.",
    },
    remote_control_port: RemoteControlPort {
        type: u16,
        default: DEFAULT_REMOTE_CONTROL_PORT,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: SettingSurfaces::GUI,
        private: false,
        toml_path: "remote_control.port",
        description: "TCP port the Remote Control server listens on (1024-65535).",
    },
    remote_control_allow_lan_access: RemoteControlAllowLanAccess {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: SettingSurfaces::GUI,
        private: false,
        toml_path: "remote_control.allow_lan_access",
        description: "Whether Remote Control also accepts connections from other devices on this network.",
    },
]);

impl RemoteControlSettings {
    pub fn is_enabled(&self) -> bool {
        *self.remote_control_enabled
    }

    pub fn port(&self) -> u16 {
        clamp_port(*self.remote_control_port)
    }

    pub fn allows_lan_access(&self) -> bool {
        *self.remote_control_allow_lan_access
    }
}

pub fn clamp_port(port: u16) -> u16 {
    port.clamp(MIN_REMOTE_CONTROL_PORT, MAX_REMOTE_CONTROL_PORT)
}

#[cfg(test)]
#[path = "remote_control_tests.rs"]
mod tests;
