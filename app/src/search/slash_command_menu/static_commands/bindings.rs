use warpui::keymap::BindingDescription;

use super::StaticCommand;

pub fn binding_description(command: &StaticCommand) -> BindingDescription {
    BindingDescription::new_preserve_case(format!("Slash command: {}", command.name))
}
