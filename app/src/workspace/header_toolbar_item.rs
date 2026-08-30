use serde::{Deserialize, Serialize};
use settings::Setting as _;
use warpui::{AppContext, SingletonEntity};

use crate::ui_components::icons::Icon;
use crate::workspace::tab_settings::TabSettings;

/// A configurable item in the vertical tabs header toolbar.
///
/// Each variant represents a panel toggle button that can be placed on either
/// the left or right side of the toolbar. The side determines which side of the
/// main content area the panel opens on.
#[derive(
    Clone,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(rename_all = "snake_case")]
pub enum HeaderToolbarItemKind {
    TabsPanel,
    #[serde(alias = "ToolsPanel", alias = "tools_panel")]
    RightSidebar,
    #[serde(alias = "CodeReview", alias = "code_review")]
    SourceControl,
    #[serde(alias = "NotificationsMailbox", alias = "notifications_mailbox")]
    Notifications,
}

impl HeaderToolbarItemKind {
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::TabsPanel => "Tabs Panel",
            Self::RightSidebar => "Right Sidebar",
            Self::SourceControl => "Source Control",
            Self::Notifications => "Notifications",
        }
    }

    pub fn icon(&self) -> Icon {
        match self {
            Self::TabsPanel => Icon::Menu,
            Self::RightSidebar => Icon::Tool2,
            Self::SourceControl => Icon::Diff,
            Self::Notifications => Icon::Inbox,
        }
    }

    /// Whether this item is supported on the current platform/configuration
    /// (feature flags, compile-time features, AI enabled, auth state).
    /// Does not check user show/hide preferences — use `is_available` for that.
    pub fn is_supported(&self, app: &AppContext) -> bool {
        match self {
            Self::TabsPanel => crate::tab::uses_vertical_tabs(app),
            Self::RightSidebar => true,
            Self::SourceControl => cfg!(feature = "local_fs"),
            Self::Notifications => true,
        }
    }

    /// Whether this item should be shown in the toolbar.
    /// Checks both `is_supported` and user show/hide preferences.
    pub fn is_available(&self, app: &AppContext) -> bool {
        if !self.is_supported(app) {
            return false;
        }
        match self {
            Self::SourceControl => *TabSettings::as_ref(app).show_code_review_button.value(),
            Self::TabsPanel | Self::RightSidebar | Self::Notifications => true,
        }
    }

    /// Whether this item opens a side panel (as opposed to replacing the content
    /// area or opening a popover).
    pub fn is_panel(&self) -> bool {
        matches!(
            self,
            Self::TabsPanel | Self::RightSidebar | Self::SourceControl
        )
    }

    pub fn default_left() -> Vec<Self> {
        vec![Self::TabsPanel]
    }

    pub fn default_right() -> Vec<Self> {
        vec![Self::Notifications, Self::RightSidebar, Self::SourceControl]
    }

    /// All toolbar item variants (availability filtering is done at the call site).
    pub fn all_items() -> Vec<Self> {
        vec![
            Self::TabsPanel,
            Self::Notifications,
            Self::RightSidebar,
            Self::SourceControl,
        ]
    }
}
