//! Menus for the Rex `rex.ui.pick` prompt.
//!
//! A [`Menu`] serializes to exactly the table `rex.ui.pick` takes, so the Lua
//! side passes it through untouched. Each item's `id` is the exact thing acted
//! on: a preset name, or a full session ID — never a label.

use std::{borrow::Cow, collections::HashMap, path::Path};

use clap::ValueEnum;
use serde::Serialize;

use crate::{preset::Presets, rex::Session};

/// Every row of every menu uses this icon; change it here to restyle all of
/// them.
const ROW_ICON: &str = "code";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// Every project preset; choosing one jumps to it.
    Projects,
    /// Running sessions; choosing one switches to it.
    Active,
    /// Running sessions; choosing one destroys it at once.
    Close,
    /// Running sessions; choosing one saves it as a preset.
    Save,
    /// Every project preset; choosing one asks to delete it.
    Delete,
}

impl Mode {
    fn title(self) -> &'static str {
        match self {
            Self::Projects => "Select Session",
            Self::Active => "Active Sessions 🟢",
            Self::Close => "Kill Session 💀",
            Self::Save => "Save Session 💾",
            Self::Delete => "Delete Preset 🗑",
        }
    }

    /// The committing row's label. For `Close` the choice is the whole
    /// decision, so the row says what it does.
    fn confirm(self) -> &'static str {
        match self {
            Self::Projects => "Open Session",
            Self::Active => "Switch Session",
            Self::Close => "Close Session",
            Self::Save => "Save Preset",
            Self::Delete => "Delete Preset",
        }
    }
}

/// The `id` of the overwrite menu's row that replaces the preset; any other
/// answer, Escape included, keeps it.
pub const OVERWRITE: &str = "overwrite";

/// The `id` of the delete menu's row that removes the preset; any other
/// answer, Escape included, keeps it.
pub const DELETE: &str = "delete";

/// The `id` of the row that keeps the preset in a confirmation menu.
const CANCEL: &str = "cancel";

#[derive(Debug, PartialEq, Serialize)]
pub struct Menu {
    pub title: Cow<'static, str>,
    pub confirm: &'static str,
    pub items: Vec<PickItem>,
}

impl Menu {
    pub fn new(mode: Mode, items: Vec<PickItem>) -> Self {
        Self {
            title: Cow::Borrowed(mode.title()),
            confirm: mode.confirm(),
            items,
        }
    }

    /// Asks whether to replace the preset `name` stored at `path`.
    pub fn overwrite(name: &str, path: &Path) -> Self {
        Self::confirming("Overwrite", OVERWRITE, name, path)
    }

    /// Asks whether to delete the preset `name` stored at `path`.
    pub fn delete(name: &str, path: &Path) -> Self {
        Self::confirming("Delete", DELETE, name, path)
    }

    /// A menu asking whether to `action` the preset `name` stored at `path`.
    /// The committing row's id is `id`; any other answer keeps the preset.
    fn confirming(action: &str, id: &str, name: &str, path: &Path) -> Self {
        let row = |id: &str, title: &str, subtitle: Option<String>| PickItem {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle,
            icon: ROW_ICON,
        };
        Self {
            title: Cow::Owned(format!("{action} {}?", safe_label(name))),
            confirm: "Confirm",
            items: vec![
                row(id, action, Some(path.display().to_string())),
                row(CANCEL, "Cancel", None),
            ],
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct PickItem {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub icon: &'static str,
}

/// One row per preset, sorted by name.
pub fn project_items(presets: &Presets) -> Vec<PickItem> {
    presets
        .keys()
        .map(|name| PickItem {
            id: name.clone(),
            title: name.clone(),
            subtitle: None,
            icon: ROW_ICON,
        })
        .collect()
}

/// One row per running session, in server order.
///
/// Labels are neither unique nor stable, while the prompt needs unique ids, so
/// the id is the session ID. A repeated label keeps its title and shows the ID
/// as the subtitle, so duplicates stay distinguishable.
pub fn session_items(sessions: &[Session]) -> Vec<PickItem> {
    let sessions: Vec<_> = sessions
        .iter()
        .filter(|session| !session.session_id.is_empty())
        .map(|session| (session, safe_label(&session.label)))
        .collect();

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (_, label) in &sessions {
        *counts.entry(label).or_default() += 1;
    }

    sessions
        .iter()
        .map(|(session, label)| PickItem {
            id: session.session_id.clone(),
            title: if label.is_empty() {
                "(unlabelled)".to_owned()
            } else {
                label.clone()
            },
            subtitle: (counts[label.as_str()] > 1).then(|| session.session_id.clone()),
            icon: ROW_ICON,
        })
        .collect()
}

/// Replaces control characters, which would corrupt a palette row, with `?`.
fn safe_label(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{Preset, Tab};
    use serde_json::json;

    fn session(id: &str, label: &str) -> Session {
        Session {
            session_id: id.into(),
            label: label.into(),
        }
    }

    #[test]
    fn menu_serializes_to_the_pick_table() {
        let menu = Menu::new(Mode::Close, session_items(&[session("session:1", "work")]));

        assert_eq!(
            serde_json::to_value(menu).unwrap(),
            json!({
                "title": "Kill Session 💀",
                "confirm": "Close Session",
                "items": [{"id": "session:1", "title": "work", "icon": "code"}]
            })
        );
    }

    #[test]
    fn project_items_use_the_preset_name() {
        let tab = Tab {
            cwd: "/".into(),
            run: None,
        };
        let presets = Presets::from([
            (
                "notes".into(),
                Preset {
                    name: "notes".into(),
                    tabs: vec![tab.clone()],
                },
            ),
            (
                "dotfiles".into(),
                Preset {
                    name: "dotfiles".into(),
                    tabs: vec![tab],
                },
            ),
        ]);

        let ids: Vec<_> = project_items(&presets)
            .into_iter()
            .map(|item| item.id)
            .collect();

        assert_eq!(ids, ["dotfiles", "notes"]);
    }

    #[test]
    fn confirmation_menus_name_the_preset_and_its_file() {
        let path = Path::new("/sessions/dot files.json");

        assert_eq!(
            serde_json::to_value(Menu::overwrite("dot files", path)).unwrap(),
            json!({
                "title": "Overwrite dot files?",
                "confirm": "Confirm",
                "items": [
                    {"id": "overwrite", "title": "Overwrite", "subtitle": "/sessions/dot files.json", "icon": "code"},
                    {"id": "cancel", "title": "Cancel", "icon": "code"}
                ]
            })
        );
        assert_eq!(
            serde_json::to_value(Menu::delete("dot files", path)).unwrap(),
            json!({
                "title": "Delete dot files?",
                "confirm": "Confirm",
                "items": [
                    {"id": "delete", "title": "Delete", "subtitle": "/sessions/dot files.json", "icon": "code"},
                    {"id": "cancel", "title": "Cancel", "icon": "code"}
                ]
            })
        );
    }

    #[test]
    fn repeated_labels_show_the_session_id() {
        let items = session_items(&[
            session("session:1", "work"),
            session("session:2", "work"),
            session("session:3", "notes"),
        ]);

        let subtitles: Vec<_> = items.iter().map(|item| item.subtitle.as_deref()).collect();
        assert_eq!(subtitles, [Some("session:1"), Some("session:2"), None]);
    }

    #[test]
    fn labels_are_sanitized_and_empty_ones_named() {
        let items = session_items(&[
            session("session:1", "a\tb\u{1b}c\u{85}é"),
            session("session:2", ""),
            session("", "skipped"),
        ]);

        let titles: Vec<_> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["a?b?c?é", "(unlabelled)"]);
    }
}
