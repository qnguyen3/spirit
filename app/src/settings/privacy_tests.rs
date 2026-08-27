use settings::schema::SettingSchemaEntry;
use settings::{Setting, SettingSurfaces};

use super::{IsCloudConversationStorageEnabled, IsCrashReportingEnabled, IsTelemetryEnabled};

#[test]
fn privacy_settings_apply_to_gui_and_tui() {
    for storage_key in [
        IsTelemetryEnabled::toml_key(),
        IsCrashReportingEnabled::toml_key(),
        IsCloudConversationStorageEnabled::toml_key(),
    ] {
        let entry = inventory::iter::<SettingSchemaEntry>
            .into_iter()
            .find(|entry| entry.storage_key == storage_key)
            .unwrap_or_else(|| panic!("missing schema entry for {storage_key}"));
        let surfaces = (entry.surfaces_fn)();

        assert_eq!(surfaces, SettingSurfaces::ALL, "{storage_key}");
        assert!(surfaces.includes(SettingSurfaces::GUI), "{storage_key}");
        assert!(surfaces.includes(SettingSurfaces::TUI), "{storage_key}");
    }
}
