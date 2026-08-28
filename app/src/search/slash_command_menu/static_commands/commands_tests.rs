use std::collections::HashSet;

use super::*;

#[test]
fn command_names_and_kinds_are_unique() {
    let mut names = HashSet::new();
    let mut kinds = HashSet::new();
    for command in all_commands() {
        assert!(
            names.insert(command.name),
            "duplicate slash command name: {}",
            command.name
        );
        assert!(
            kinds.insert(command.kind),
            "duplicate slash command kind: {:?}",
            command.kind
        );
    }
}

#[test]
fn command_registry_exposes_icon_metadata_by_name() {
    let registry = Registry::new();

    assert_eq!(
        registry
            .get_command_with_name(CHANGELOG.name)
            .map(|command| command.icon_path),
        Some("bundled/svg/book-open.svg")
    );
}

#[test]
fn version_command_is_not_registered() {
    assert!(
        all_commands()
            .iter()
            .all(|command| command.name != "/version")
    );
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

#[test]
fn is_active_requires_every_availability_bit_the_command_declares() {
    assert!(OPEN_CODE_REVIEW.is_active(Availability::REPOSITORY));
    assert!(OPEN_CODE_REVIEW.is_active(Availability::REPOSITORY.union(Availability::LOCAL)));
    assert!(!OPEN_CODE_REVIEW.is_active(Availability::ALWAYS));
    assert!(!OPEN_CODE_REVIEW.is_active(Availability::LOCAL));

    assert!(EDIT.is_active(Availability::LOCAL));
    assert!(!EDIT.is_active(Availability::REPOSITORY));

    assert!(FEEDBACK.is_active(Availability::ALWAYS));
    assert!(FEEDBACK.is_active(Availability::LOCAL));
}

#[test]
fn docker_sandbox_command_is_local_only_and_takes_no_argument() {
    assert_eq!(
        CREATE_DOCKER_SANDBOX.kind,
        SlashCommandKind::CreateDockerSandbox
    );
    assert!(CREATE_DOCKER_SANDBOX.argument.is_none());
    assert!(CREATE_DOCKER_SANDBOX.is_active(Availability::LOCAL));
    assert!(!CREATE_DOCKER_SANDBOX.is_active(Availability::ALWAYS));
}
