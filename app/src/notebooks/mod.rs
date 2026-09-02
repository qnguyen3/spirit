mod context_menu;
pub mod editor;
pub mod file;
pub mod link;
mod styles;

use serde::{Deserialize, Serialize};
use warpui::AppContext;

/// A notebook location. Mainly, this lets us distinguish between cloud and file-based notebooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum NotebookLocation {
    /// A notebook backed by a local file.
    LocalFile,
    /// A notebook backed by a remote file.
    RemoteFile,
}
/// Initialize notebooks-related keybindings.
pub fn init(app: &mut AppContext) {
    self::file::init(app);
    self::editor::view::init(app);
}
