//! Harpoon-like marks: a short ordered list of presets to jump to by key.
//!
//! ```json
//! {
//!   "marks": [
//!     { "name": "dotfiles", "session_id": "session:…" },
//!     { "name": "notes" }
//!   ]
//! }
//! ```
//!
//! A mark is identified by its preset `name`; `session_id` is only the last
//! session seen for it, a fast path that is checked before use. A mark's
//! position is its 1-based index in the list, so removing one compacts the rest
//! and no stored number can drift. The first four positions have keys
//! ([`KEYS`]); the rest are reached from the marks menu, which also moves a mark
//! to another position.
//!
//! The file is strict like a preset: unknown keys, an empty name, or a repeated
//! name are errors naming it. A missing file is an empty list, and it is
//! written only when the list changes, in one rename.
//!
//! The list is written with a rename, so a lock on `marks.json` itself would
//! not serialize two writers: commands that change the list lock `.marks.lock`
//! beside it instead, from the load until the process is done with the list. A
//! read-only load (the menus) takes no lock and creates nothing.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    menu::{MARK_ICON, MOVE_ICON, Menu, Mode, PickItem, UNMARK_ICON, safe_label},
    preset,
    rex::{RexApi, SessionReader},
    save::{self, Saved},
    session::{self, Target},
};

/// The key that reaches each of the first positions, in order.
pub const KEYS: [&str; 4] = ["q", "w", "e", "r"];

const FILE_NAME: &str = "marks.json";

/// The file holding the write lock: never parsed as marks, and beside the
/// marks file so a rename of that file cannot move it.
const LOCK_FILE_NAME: &str = ".marks.lock";

/// The id of the marks menu's "Unmark…" row when no mark is named that.
const UNMARK_ROW: &str = "unmark";

/// The id of the marks menu's "Move…" row when no mark is named that.
const MOVE_ROW: &str = "move";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MarksFile {
    marks: Vec<Mark>,
}

/// The marks list and the file it came from.
#[derive(Debug)]
pub struct Marks {
    path: PathBuf,
    marks: Vec<Mark>,
    /// The exclusive lock held from the load until this list is dropped, so no
    /// other command can write the list from the copy this one read.
    _lock: Option<fs::File>,
}

/// `$XDG_STATE_HOME/rex-session-manager`, or
/// `$HOME/.local/state/rex-session-manager`.
pub fn default_directory() -> Result<PathBuf> {
    let state = match env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        Some(state) => PathBuf::from(state),
        None => env::home_dir()
            .context("cannot locate the home directory for the marks directory")?
            .join(".local")
            .join("state"),
    };
    Ok(state.join("rex-session-manager"))
}

impl Marks {
    /// Reads the marks file in `directory`; a missing file is an empty list.
    ///
    /// Nothing is created or locked, for the read-only menus.
    pub fn load(directory: &Path) -> Result<Self> {
        Self::read(directory, None)
    }

    /// Reads the marks file while holding the write lock, creating the
    /// directory and the lock file if needed. Every command that changes the
    /// list loads this way, so two runs never lose each other's change.
    pub fn load_for_update(directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
        let lock_path = directory.join(LOCK_FILE_NAME);
        let lock = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("cannot open {}", lock_path.display()))?;
        lock.lock()
            .with_context(|| format!("cannot lock {}", lock_path.display()))?;
        Self::read(directory, Some(lock))
    }

    fn read(directory: &Path, lock: Option<fs::File>) -> Result<Self> {
        let path = directory.join(FILE_NAME);
        let marks = match fs::read_to_string(&path) {
            Ok(text) => parse(&text).with_context(|| path.display().to_string())?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        Ok(Self {
            path,
            marks,
            _lock: lock,
        })
    }

    /// Writes the list back in one rename, creating the directory if needed.
    fn store(&self) -> Result<()> {
        let directory = self
            .path
            .parent()
            .with_context(|| format!("{} has no parent directory", self.path.display()))?;
        fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
        let file = MarksFile {
            marks: self.marks.clone(),
        };
        preset::replace(&self.path, &(serde_json::to_string_pretty(&file)? + "\n"))
    }

    /// The 0-based index of the mark `name`.
    fn index_of(&self, name: &str) -> Result<usize> {
        self.marks
            .iter()
            .position(|mark| mark.name == name)
            .with_context(|| format!("no mark named {name}"))
    }

    fn index_at(&self, position: usize) -> Result<usize> {
        let index = position.checked_sub(1).filter(|&i| i < self.marks.len());
        index.with_context(|| {
            format!(
                "no mark at position {position} ({} marked)",
                self.marks.len()
            )
        })
    }

    /// One menu row per mark, in position order; a keyed position shows its
    /// key as the subtitle.
    fn items(&self) -> Vec<PickItem> {
        self.marks
            .iter()
            .enumerate()
            .map(|(index, mark)| PickItem {
                id: mark.name.clone(),
                title: safe_label(&mark.name),
                subtitle: key(index).map(str::to_owned),
                icon: MARK_ICON,
            })
            .collect()
    }

    /// Removes every mark holding `session_id`, storing the list when it
    /// changed.
    pub fn forget_session(&mut self, session_id: &str) -> Result<()> {
        let before = self.marks.len();
        self.marks
            .retain(|mark| mark.session_id.as_deref() != Some(session_id));
        self.store_if(self.marks.len() != before)
    }

    /// Removes the mark `name`, if any, storing the list when it changed.
    pub fn forget_name(&mut self, name: &str) -> Result<()> {
        let before = self.marks.len();
        self.marks.retain(|mark| mark.name != name);
        self.store_if(self.marks.len() != before)
    }

    fn store_if(&self, changed: bool) -> Result<()> {
        if changed { self.store() } else { Ok(()) }
    }
}

fn parse(text: &str) -> Result<Vec<Mark>> {
    // Serde would also accept a positional array for a struct.
    let value: serde_json::Value = serde_json::from_str(text)?;
    ensure!(value.is_object(), "the file must contain a JSON object");
    let file: MarksFile = serde_json::from_value(value)?;
    for (index, mark) in file.marks.iter().enumerate() {
        let number = index + 1;
        ensure!(
            !mark.name.is_empty(),
            r#"mark {number} needs a non-empty string "name""#
        );
        if file.marks[..index]
            .iter()
            .any(|seen| seen.name == mark.name)
        {
            bail!("duplicate mark {:?}", mark.name);
        }
    }
    Ok(file.marks)
}

/// The key reaching the 0-based `index`, if it has one.
fn key(index: usize) -> Option<&'static str> {
    KEYS.get(index).copied()
}

/// The marks menu: a row per mark, then a "Move…" row when there are at least
/// two marks, then an "Unmark…" row whose id `unmark_id` is never a mark's
/// name. The `move_id` of the "Move…" row is never a mark's name either, nor
/// `unmark_id`, so the Lua bridge can tell the two apart from a chosen mark and
/// open `menu move` or `menu unmark`. No marks means no rows at all.
#[derive(Debug, Serialize)]
pub struct MarksMenu {
    #[serde(flatten)]
    menu: Menu,
    unmark_id: String,
    move_id: String,
}

pub fn menu(marks: &Marks) -> MarksMenu {
    let mut unmark_id = UNMARK_ROW.to_owned();
    while marks.marks.iter().any(|mark| mark.name == unmark_id) {
        unmark_id.push('+');
    }
    let mut move_id = MOVE_ROW.to_owned();
    while marks.marks.iter().any(|mark| mark.name == move_id) || move_id == unmark_id {
        move_id.push('+');
    }
    let mut items = marks.items();
    if !items.is_empty() {
        if marks.marks.len() > 1 {
            items.push(PickItem {
                id: move_id.clone(),
                title: "Move…".to_owned(),
                subtitle: None,
                icon: MOVE_ICON,
            });
        }
        items.push(PickItem {
            id: unmark_id.clone(),
            title: "Unmark…".to_owned(),
            subtitle: None,
            icon: UNMARK_ICON,
        });
    }
    MarksMenu {
        menu: Menu::new(Mode::Marks, items),
        unmark_id,
        move_id,
    }
}

pub fn unmark_menu(marks: &Marks) -> Menu {
    Menu::new(Mode::Unmark, marks.items())
}

pub fn move_menu(marks: &Marks) -> Menu {
    Menu::new(Mode::Move, marks.items())
}

/// What `mark` did, printed as `{"status": …, "name": …, "position": …,
/// "key": …, "saved": …}`; `key` is left out past the keyed positions.
#[derive(Debug, PartialEq, Serialize)]
pub struct Marked {
    status: MarkStatus,
    name: String,
    position: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'static str>,
    /// Whether a preset was written for the session first.
    saved: bool,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum MarkStatus {
    /// Appended at the end of the list.
    Added,
    /// Already marked: its session was updated and it kept its position.
    Updated,
}

/// Marks session `session_id`, saving it as a preset first when it has none.
///
/// The marks file is read before anything is saved, so a broken one changes
/// nothing. The preset is saved exactly as `save` would, so an unlabelled
/// session is refused with the same error.
pub fn mark(
    rex: &impl SessionReader,
    marks: &mut Marks,
    sessions_dir: &Path,
    session_id: &str,
    home: Option<&Path>,
    command_line: impl Fn(u32) -> Option<String>,
) -> Result<Marked> {
    let (name, saved) = match save::save(rex, sessions_dir, session_id, None, home, command_line)? {
        Saved::Created { name, .. } => (name, true),
        Saved::Exists { name, .. } => (name, false),
        Saved::Overwritten { .. } => unreachable!("save without --overwrite never overwrites"),
    };
    let session_id = Some(session_id.to_owned());
    let (status, index) = match marks.index_of(&name) {
        Ok(index) => {
            marks.marks[index].session_id = session_id;
            (MarkStatus::Updated, index)
        }
        Err(_) => {
            marks.marks.push(Mark {
                name: name.clone(),
                session_id,
            });
            (MarkStatus::Added, marks.marks.len() - 1)
        }
    };
    marks.store()?;
    Ok(Marked {
        status,
        name,
        position: index + 1,
        key: key(index),
        saved,
    })
}

/// What `unmark` did: `{"status": "removed", "name": …, "position": …}`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Unmarked {
    Removed { name: String, position: usize },
}

pub fn unmark(marks: &mut Marks, name: &str) -> Result<Unmarked> {
    let index = marks.index_of(name)?;
    let mark = marks.marks.remove(index);
    marks.store()?;
    Ok(Unmarked::Removed {
        name: mark.name,
        position: index + 1,
    })
}

/// What `move` did, printed as one JSON object tagged by `status`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Moved {
    /// Nothing was written. `menu` lists the positions the mark can go to.
    Choose {
        name: String,
        from: usize,
        menu: Menu,
    },
    /// The mark moved to `to`, or was already there and the file untouched.
    Moved {
        name: String,
        from: usize,
        to: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<&'static str>,
    },
}

/// Asks where the mark `name` goes: one row per position, its id the position
/// as a decimal string, its title the position and the key reaching it, and its
/// subtitle the mark that position holds now.
///
/// Nothing is written and the list is read without the lock, since this only
/// reports what `move --to` would do.
pub fn choose(marks: &Marks, name: &str) -> Result<Moved> {
    let from = marks.index_of(name)?;
    let items = marks
        .marks
        .iter()
        .enumerate()
        .map(|(index, mark)| PickItem {
            id: (index + 1).to_string(),
            title: match key(index) {
                Some(key) => format!("{} · {key}", index + 1),
                None => (index + 1).to_string(),
            },
            subtitle: Some(safe_label(&mark.name)),
            icon: MARK_ICON,
        })
        .collect();
    Ok(Moved::Choose {
        name: name.to_owned(),
        from: from + 1,
        menu: Menu::moving(name, items),
    })
}

/// Moves the mark `name` to the 1-based `position`, so the marks between shift
/// by one. A mark already there is reported without writing the file.
pub fn move_to(marks: &mut Marks, name: &str, position: usize) -> Result<Moved> {
    let index = marks.index_of(name)?;
    ensure!(
        position != 0 && position <= marks.marks.len(),
        "no position {position} ({} marked)",
        marks.marks.len()
    );
    let from = index + 1;
    if position != from {
        let mark = marks.marks.remove(index);
        marks.marks.insert(position - 1, mark);
        marks.store()?;
    }
    Ok(Moved::Moved {
        name: name.to_owned(),
        from,
        to: position,
        key: key(position - 1),
    })
}

/// Which mark `jump-mark` goes to.
#[derive(Debug, Clone)]
pub enum Selector {
    Position(usize),
    Name(String),
}

/// Goes to the mark `selector` names.
///
/// Its recorded session is used while it runs and still carries the mark's
/// name. Otherwise this is `jump` for the name: the one session labelled with
/// it, else a new session from its preset, in `shell` when one is configured.
/// A mark with neither is an error. The session found is recorded when it
/// changed.
pub fn jump(
    rex: &impl RexApi,
    marks: &mut Marks,
    sessions_dir: &Path,
    selector: &Selector,
    home: Option<&Path>,
    shell: Option<&str>,
) -> Result<Target> {
    let index = match selector {
        Selector::Position(position) => marks.index_at(*position)?,
        Selector::Name(name) => marks.index_of(name)?,
    };
    let mark = marks.marks[index].clone();
    let sessions = rex.list_sessions()?;

    let recorded = mark.session_id.as_deref().filter(|id| {
        sessions
            .iter()
            .any(|session| session.session_id == *id && session.label == mark.name)
    });
    if let Some(id) = recorded {
        return Ok(Target {
            session_id: id.to_owned(),
            window_id: None,
        });
    }

    let target = match session::labelled(&sessions, &mark.name)? {
        Some(session) => Target {
            session_id: session.session_id.clone(),
            window_id: None,
        },
        None => {
            let Some(preset) = preset::get(sessions_dir, &mark.name)? else {
                bail!(
                    "mark {} has no running session and no preset; unmark it",
                    mark.name
                );
            };
            session::create(rex, &preset, home, shell)?
        }
    };
    marks.marks[index].session_id = Some(target.session_id.clone());
    marks.store()?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_session_id_is_left_out_of_the_file() {
        let marks = Marks {
            path: PathBuf::from("/unused"),
            marks: vec![Mark {
                name: "notes".into(),
                session_id: None,
            }],
            _lock: None,
        };
        let file = MarksFile {
            marks: marks.marks.clone(),
        };

        assert_eq!(
            serde_json::to_string(&file).unwrap(),
            r#"{"marks":[{"name":"notes"}]}"#
        );
    }

    #[test]
    fn only_the_first_four_positions_have_keys() {
        let keys: Vec<_> = (0..6).map(key).collect();

        assert_eq!(
            keys,
            [Some("q"), Some("w"), Some("e"), Some("r"), None, None]
        );
    }
}
