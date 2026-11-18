//! Deleting a project preset.
//!
//! `delete NAME` removes nothing: it reports the file holding that preset and
//! asks, with a ready `rex.ui.pick` menu, whether to delete it. The caller
//! confirms by running `delete --confirmed NAME`, which removes exactly that
//! file. The preset is found by its `name`, whatever its file is called, so a
//! confirmation is always about the file the menu showed.
//!
//! Running sessions are never touched, and `delete` runs no `rex` command.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{menu::Menu, preset};

/// What `delete` did, printed as one JSON object tagged by `status`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Deleted {
    /// Nothing was removed. `menu` asks whether to delete the preset.
    Confirm {
        name: String,
        path: PathBuf,
        menu: Menu,
    },
    /// The preset's file was removed.
    Deleted { name: String, path: PathBuf },
}

/// Deletes the preset `name` from `directory` when `confirmed`, else asks.
///
/// A preset that does not exist is an error naming it, confirmed or not. A
/// broken preset file, or a duplicate name, keeps its usual load error, so
/// nothing is ever removed from an unreadable directory.
pub fn delete(directory: &Path, name: &str, confirmed: bool) -> Result<Deleted> {
    let path =
        preset::find(directory, name)?.with_context(|| format!("unknown project: {name}"))?;
    if !confirmed {
        return Ok(Deleted::Confirm {
            menu: Menu::delete(name, &path),
            name: name.to_owned(),
            path,
        });
    }
    fs::remove_file(&path).with_context(|| format!("cannot delete {}", path.display()))?;
    Ok(Deleted::Deleted {
        name: name.to_owned(),
        path,
    })
}
