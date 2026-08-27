use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp_core::features::FeatureFlag;

use super::{Availability, SlashCommandKind, SlashCommandSurfaces};
use crate::search::slash_command_menu::StaticCommand;
use crate::search::slash_command_menu::static_commands::Argument;
use crate::ui_components::color_dot;
pub static EDIT: LazyLock<StaticCommand> = LazyLock::new(|| StaticCommand {
    name: "/open-file",
    description: "Open a file in Warp's code editor",
    kind: SlashCommandKind::Edit,
    supported_surfaces: SlashCommandSurfaces::GuiOnly {
        icon_path: "bundled/svg/file-code-02.svg",
    },
    availability: Availability::LOCAL,
    auto_enter_ai_mode: false,
    argument: Some(
        Argument::optional().with_hint_text("<path/to/file[:line[:col]]> or \"@\" to search"),
    ),
});

pub static RENAME_TAB: LazyLock<StaticCommand> = LazyLock::new(|| StaticCommand {
    name: "/rename-tab",
    description: "Rename the current tab",
    kind: SlashCommandKind::RenameTab,
    supported_surfaces: SlashCommandSurfaces::GuiOnly {
        icon_path: "bundled/svg/pencil-line.svg",
    },
    availability: Availability::ALWAYS,
    auto_enter_ai_mode: false,
    argument: Some(Argument::required().with_hint_text("<tab name>")),
});

static SET_TAB_COLOR_HINT: LazyLock<String> = LazyLock::new(|| {
    let mut hint = String::from("<");
    for color in color_dot::TAB_COLOR_OPTIONS {
        hint.push_str(&color.to_string().to_ascii_lowercase());
        hint.push('|');
    }
    hint.push_str("none>");
    hint
});

pub static SET_TAB_COLOR: LazyLock<StaticCommand> = LazyLock::new(|| StaticCommand {
    name: "/set-tab-color",
    description: "Set the color of the current tab",
    kind: SlashCommandKind::SetTabColor,
    supported_surfaces: SlashCommandSurfaces::GuiOnly {
        icon_path: "bundled/svg/ellipse.svg",
    },
    availability: Availability::ALWAYS,
    auto_enter_ai_mode: false,
    argument: Some(Argument::required().with_hint_text(SET_TAB_COLOR_HINT.as_str())),
});

pub const OPEN_CODE_REVIEW: StaticCommand = StaticCommand {
    name: "/open-code-review",
    description: "Open code review",
    kind: SlashCommandKind::OpenCodeReview,
    supported_surfaces: SlashCommandSurfaces::GuiOnly {
        icon_path: "bundled/svg/diff.svg",
    },
    availability: Availability::REPOSITORY,
    auto_enter_ai_mode: false,
    argument: None,
};

pub const OPEN_SETTINGS_FILE: StaticCommand = StaticCommand {
    name: "/open-settings-file",
    description: "Open settings file (TOML)",
    kind: SlashCommandKind::OpenSettingsFile,
    supported_surfaces: SlashCommandSurfaces::GuiOnly {
        icon_path: "bundled/svg/file-code-02.svg",
    },
    availability: Availability::LOCAL,
    auto_enter_ai_mode: false,
    argument: None,
};

pub const CHANGELOG: StaticCommand = StaticCommand {
    name: "/changelog",
    description: "Open the latest changelog",
    kind: SlashCommandKind::Changelog,
    supported_surfaces: SlashCommandSurfaces::GuiOnly {
        icon_path: "bundled/svg/book-open.svg",
    },
    availability: Availability::ALWAYS,
    auto_enter_ai_mode: false,
    argument: None,
};

// Accepts an optional argument so that buffers like `/feedback some text` still parse to
// this command (the trailing text is ignored on execution). Without this, typing any
// argument after `/feedback` would fall through and be treated as plain input.
pub static FEEDBACK: LazyLock<StaticCommand> = LazyLock::new(|| StaticCommand {
    name: "/feedback",
    description: "Send feedback",
    kind: SlashCommandKind::Feedback,
    supported_surfaces: SlashCommandSurfaces::GuiOnly {
        icon_path: "bundled/svg/feedback.svg",
    },
    availability: Availability::ALWAYS,
    auto_enter_ai_mode: false,
    argument: Some(Argument::optional().with_execute_on_selection()),
});
pub static COMMAND_REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

/// A unique identifier for a static slash command.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct SlashCommandId(Uuid);

impl SlashCommandId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SlashCommandId {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Registry {
    commands: HashMap<SlashCommandId, StaticCommand>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        let mut commands = HashMap::new();
        for command in all_commands_for_all_surfaces() {
            debug_assert!(
                !command
                    .availability
                    .contains(Availability::TERMINAL_VIEW | Availability::AGENT_VIEW),
                "command `{}` sets both TERMINAL_VIEW and AGENT_VIEW, which is unsatisfiable",
                command.name,
            );
            commands.insert(SlashCommandId::new(), command);
        }
        Self { commands }
    }

    pub fn all_commands_by_id(&self) -> impl Iterator<Item = (SlashCommandId, &StaticCommand)> {
        self.commands.iter().map(|(id, cmd)| (*id, cmd))
    }

    pub fn all_commands(&self) -> impl Iterator<Item = &StaticCommand> {
        self.commands.values()
    }

    pub fn get_command(&self, id: &SlashCommandId) -> Option<&StaticCommand> {
        self.commands.get(id)
    }

    pub fn get_command_with_name(&self, name: &str) -> Option<&StaticCommand> {
        self.commands.values().find(|command| command.name == name)
    }

    #[cfg(test)]
    pub fn get_command_id_with_name(&self, name: &str) -> Option<&SlashCommandId> {
        self.commands
            .iter()
            .find(|(_, command)| command.name == name)
            .map(|(id, _)| id)
    }
}

#[cfg(test)]
fn all_commands(settings_mode: settings::SettingsMode) -> Vec<StaticCommand> {
    all_commands_for_all_surfaces()
        .into_iter()
        .filter(|command| command.supports_surface(settings_mode))
        .collect()
}

fn all_commands_for_all_surfaces() -> Vec<StaticCommand> {
    let mut commands = vec![
        FEEDBACK.clone(),
        RENAME_TAB.clone(),
        SET_TAB_COLOR.clone(),
        CHANGELOG,
        OPEN_CODE_REVIEW,
    ];

    if !cfg!(target_family = "wasm") {
        commands.push(EDIT.clone());
    }

    if FeatureFlag::SettingsFile.is_enabled() && cfg!(feature = "local_fs") {
        commands.push(OPEN_SETTINGS_FILE);
    }

    commands
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
