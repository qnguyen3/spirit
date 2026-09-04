use std::collections::HashSet;

use warpui::{App, AssetProvider as _};

use super::{AgentIcon, agent_catalog};
use crate::settings::AgentApprovalMode;
use crate::terminal::CLIAgent;

#[test]
fn catalog_is_non_empty() {
    assert!(!agent_catalog().is_empty());
}

#[test]
fn binaries_are_single_tokens() {
    for def in agent_catalog() {
        assert!(
            !def.binary.contains(char::is_whitespace),
            "binary {:?} for {} must be a single token",
            def.binary,
            def.display_name
        );
        assert!(!def.binary.is_empty());
    }
}

#[test]
fn commands_start_with_binary_name() {
    for def in agent_catalog() {
        let first_token = def
            .command
            .split_whitespace()
            .next()
            .expect("command must not be empty");
        assert_eq!(
            first_token, def.binary,
            "command for {} must start with its binary name",
            def.display_name
        );
    }
}

#[test]
fn yolo_launch_commands_start_with_binary_name() {
    for def in agent_catalog() {
        let launch_command = def.launch_command(AgentApprovalMode::Yolo);
        let first_token = launch_command
            .split_whitespace()
            .next()
            .expect("launch command must not be empty");
        assert_eq!(
            first_token, def.binary,
            "YOLO launch command for {} must start with its binary name",
            def.display_name
        );
    }
}

#[test]
fn normal_launch_command_is_the_base_command() {
    for def in agent_catalog() {
        assert_eq!(
            def.launch_command(AgentApprovalMode::Normal),
            def.command,
            "Normal launch command for {} must be its base command",
            def.display_name
        );
    }
}

#[test]
fn yolo_launch_command_appends_yolo_args() {
    for def in agent_catalog() {
        let launch_command = def.launch_command(AgentApprovalMode::Yolo);
        match def.yolo_args {
            Some(args) => assert_eq!(
                launch_command,
                format!("{} {args}", def.command),
                "YOLO launch command for {} must append its YOLO args",
                def.display_name
            ),
            None => assert_eq!(
                launch_command, def.command,
                "{} has no YOLO args, so YOLO must launch its base command",
                def.display_name
            ),
        }
    }
}

#[test]
fn claude_yolo_launch_command_appends_its_flag() {
    let claude = agent_catalog()
        .iter()
        .find(|def| def.display_name == "Claude Code")
        .expect("catalog must contain Claude Code");
    assert_eq!(
        claude.launch_command(AgentApprovalMode::Yolo),
        "claude --dangerously-skip-permissions"
    );
    assert_eq!(claude.launch_command(AgentApprovalMode::Normal), "claude");
}

#[test]
fn image_icons_resolve_through_the_bundled_asset_provider() {
    for def in agent_catalog() {
        let AgentIcon::Image(path) = def.icon else {
            continue;
        };
        assert!(
            warp_assets::Assets.get(path).is_ok(),
            "bundled asset {path:?} for {} does not exist",
            def.display_name
        );
    }
}

#[test]
fn image_icons_match_the_agents_artwork() {
    for def in agent_catalog() {
        match def.icon {
            AgentIcon::Image(path) => assert_eq!(
                Some(path),
                def.cli_agent.image_icon(),
                "catalog artwork for {} must match its CLI agent's artwork",
                def.display_name
            ),
            AgentIcon::Glyph(_) => assert_eq!(
                None,
                def.cli_agent.image_icon(),
                "{} renders a glyph, so its CLI agent must not carry artwork",
                def.display_name
            ),
        }
    }
}

#[test]
fn display_names_are_unique() {
    let mut seen = HashSet::new();
    for def in agent_catalog() {
        assert!(
            seen.insert(def.display_name),
            "duplicate display name {:?}",
            def.display_name
        );
    }
}

#[test]
fn install_docs_urls_are_https() {
    for def in agent_catalog() {
        assert!(
            def.install_docs_url.starts_with("https://"),
            "install docs URL for {} must be https",
            def.display_name
        );
    }
}

#[cfg(unix)]
#[test]
fn is_installed_resolves_against_the_supplied_path() {
    use std::os::unix::fs::PermissionsExt as _;

    use super::is_installed;

    let def = &agent_catalog()[0];
    let temp_dir = tempfile::TempDir::new().unwrap();
    let binary_path = temp_dir.path().join(def.binary);
    std::fs::write(&binary_path, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let path_env = temp_dir.path().to_str().unwrap();
    assert!(is_installed(def, Some(path_env)));

    let empty_dir = tempfile::TempDir::new().unwrap();
    assert!(!is_installed(def, Some(empty_dir.path().to_str().unwrap())));
}

#[test]
fn launch_commands_detect_as_their_own_agent() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            for def in agent_catalog() {
                for mode in [AgentApprovalMode::Normal, AgentApprovalMode::Yolo] {
                    let command = def.launch_command(mode);
                    assert_eq!(
                        CLIAgent::detect(&command, None, None, ctx),
                        Some(def.cli_agent),
                        "{} is launched as {command:?} but does not detect as {:?}, so it gets \
                         no agent footer and no rich input",
                        def.display_name,
                        def.cli_agent,
                    );
                }
            }
        });
    });
}
