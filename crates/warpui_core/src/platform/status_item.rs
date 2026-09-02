use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq)]
pub struct StatusItem {
    pub tooltip: String,
    pub icon_png: Cow<'static, [u8]>,
    pub entries: Vec<StatusItemEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatusItemEntry {
    Action {
        label: String,
        action: &'static str,
        argument: String,
    },
    Separator,
}

impl StatusItem {
    pub fn primary_action(&self) -> Option<(&'static str, String)> {
        self.entries.iter().find_map(|entry| match entry {
            StatusItemEntry::Action {
                action, argument, ..
            } => Some((*action, argument.clone())),
            StatusItemEntry::Separator => None,
        })
    }
}
