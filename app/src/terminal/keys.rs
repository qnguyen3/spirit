//! Utilities for terminal keybindings.

use warpui::{Entity, ModelContext, ModelHandle, SingletonEntity};

use crate::settings_view::keybindings::{KeybindingChangedEvent, KeybindingChangedNotifier};
use crate::util::bindings::{
    CustomAction, custom_tag_to_keystroke, keybinding_name_to_display_string,
};

/// The keybinding label to display for a [`CustomAction`].
pub fn custom_action_to_display(action: CustomAction) -> Option<String> {
    custom_tag_to_keystroke(action.into()).map(|keystroke| keystroke.displayed())
}
