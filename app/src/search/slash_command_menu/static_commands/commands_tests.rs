use std::collections::HashSet;

use super::*;

#[test]
fn command_names_and_kinds_are_unique_per_surface() {
    for settings_mode in [settings::SettingsMode::Gui, settings::SettingsMode::Tui] {
        let mut names = HashSet::new();
        let mut kinds = HashSet::new();
        for command in all_commands(settings_mode) {
            assert!(
                names.insert(command.name),
                "duplicate slash command name on {settings_mode:?}: {}",
                command.name
            );
            assert!(
                kinds.insert(command.kind),
                "duplicate slash command kind on {settings_mode:?}: {:?}",
                command.kind
            );
        }
    }
}

#[test]
fn gui_icon_metadata_matches_surface_support() {
    let mut checked_kinds = HashSet::new();
    for settings_mode in [settings::SettingsMode::Gui, settings::SettingsMode::Tui] {
        for command in all_commands(settings_mode) {
            if checked_kinds.insert(command.kind) {
                assert_eq!(
                    command.supported_surfaces.gui_icon_path().is_some(),
                    command.supports_gui(),
                    "{} has inconsistent GUI icon metadata",
                    command.name
                );
            }
        }
    }
}
#[test]
fn command_registry_filters_explicit_surface_metadata() {
    for settings_mode in [settings::SettingsMode::Gui, settings::SettingsMode::Tui] {
        for command in all_commands(settings_mode) {
            assert!(
                command.supports_surface(settings_mode),
                "{} should support {settings_mode:?}",
                command.name
            );
        }
    }
}

#[test]
fn command_registry_exposes_surface_metadata_by_name() {
    let registry = Registry::new();

    assert!(matches!(
        registry
            .get_command_with_name(CHANGELOG.name)
            .map(|command| command.supported_surfaces),
        Some(SlashCommandSurfaces::GuiOnly {
            icon_path: "bundled/svg/book-open.svg"
        })
    ));
}

#[test]
fn version_command_is_not_registered() {
    for settings_mode in [settings::SettingsMode::Gui, settings::SettingsMode::Tui] {
        assert!(
            all_commands(settings_mode)
                .iter()
                .all(|command| command.name != "/version")
        );
    }
}

#[test]
fn rename_tab_command_requires_argument() {
    let command = COMMAND_REGISTRY
        .get_command_with_name(RENAME_TAB.name)
        .expect("expected /rename-tab to be registered");
    let argument = command
        .argument
        .as_ref()
        .expect("expected /rename-tab to require an argument");

    assert!(!argument.is_optional);
    assert!(!argument.should_execute_on_selection);
    assert_eq!(argument.hint_text, Some("<tab name>"));
}

#[test]
fn set_tab_color_command_requires_argument() {
    let command = COMMAND_REGISTRY
        .get_command_with_name(SET_TAB_COLOR.name)
        .expect("expected /set-tab-color to be registered");
    let argument = command
        .argument
        .as_ref()
        .expect("expected /set-tab-color to require an argument");

    assert!(!argument.is_optional);
    assert!(!argument.should_execute_on_selection);

    let hint = argument
        .hint_text
        .expect("/set-tab-color hint text is set dynamically");
    for color in color_dot::TAB_COLOR_OPTIONS {
        let lower = color.to_string().to_ascii_lowercase();
        assert!(hint.contains(&lower), "hint should mention `{lower}`");
    }
    assert!(hint.contains("none"), "hint should mention `none`");
}
