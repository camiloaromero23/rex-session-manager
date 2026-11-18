//! Saving a running session as a project preset.
//!
//! Each tab becomes one preset tab built from its focused terminal pane: the
//! pane's working directory, and the program running in its foreground, if
//! that is not the shell itself. Other panes of a split tab are not saved, and
//! a tab with no terminal pane is skipped.
//!
//! The preset is named after the session label. If a preset of that name
//! already exists, in any file, `save` writes nothing and returns
//! [`Saved::Exists`] with a menu asking whether to overwrite it; the caller
//! confirms by running `save --overwrite NAME`, which replaces that same file.
//! NAME must still be the session's label, so a session renamed after the
//! question never replaces a preset nobody confirmed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::{
    menu::Menu,
    preset::{self, Preset, Tab},
    rex::{ProcessView, SessionReader, SessionView, WindowView},
};

/// Block flavors under this prefix are terminals, which answer `pwd` and
/// `process`.
const TERMINAL_FLAVOR_PREFIX: &str = "com.superlogical.terminal.";

/// Foreground programs that mean "nothing is running": the pane's own shell.
const SHELLS: [&str; 10] = [
    "fish", "zsh", "bash", "sh", "dash", "ksh", "tcsh", "csh", "nu", "login",
];

/// What `save` did, printed as `{"status": …, "name": …, "path": …}`.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Saved {
    /// A new preset file was written.
    Created { name: String, path: PathBuf },
    /// A preset of that name exists; nothing was written. `menu` asks whether
    /// to overwrite it.
    Exists {
        name: String,
        path: PathBuf,
        menu: Menu,
    },
    /// The existing preset file was replaced.
    Overwritten { name: String, path: PathBuf },
}

/// Saves session `session_id` as a preset in `directory`.
///
/// `overwrite` is the preset name the caller confirmed replacing, if any.
///
/// `command_line` returns the full command line of a local process ID, since
/// Rex reports only a program's name; `None` keeps the name alone.
pub fn save(
    rex: &impl SessionReader,
    directory: &Path,
    session_id: &str,
    overwrite: Option<&str>,
    home: Option<&Path>,
    command_line: impl Fn(u32) -> Option<String>,
) -> Result<Saved> {
    let view = rex.session_view(session_id)?;
    let name = view.label.clone();
    if name.is_empty() {
        bail!("session {session_id} has no label; name it before saving");
    }
    if let Some(confirmed) = overwrite.filter(|confirmed| *confirmed != name) {
        bail!("session {session_id} is now labelled {name}, not {confirmed}; save it again");
    }

    let existing = preset::find(directory, &name)?;
    if let (Some(path), None) = (&existing, overwrite) {
        return Ok(Saved::Exists {
            menu: Menu::overwrite(&name, path),
            name,
            path: path.clone(),
        });
    }

    let preset = Preset {
        tabs: tabs(rex, session_id, &view, home, &command_line)?,
        name,
    };
    let text = preset.to_file_text()?;
    let name = preset.name;
    match existing {
        Some(path) => {
            preset::replace(&path, &text)?;
            Ok(Saved::Overwritten { name, path })
        }
        None => {
            let path = directory.join(file_name(&name));
            preset::create(&path, &text)?;
            Ok(Saved::Created { name, path })
        }
    }
}

/// One preset tab per window that has a terminal pane, in tab order.
fn tabs(
    rex: &impl SessionReader,
    session_id: &str,
    view: &SessionView,
    home: Option<&Path>,
    command_line: &impl Fn(u32) -> Option<String>,
) -> Result<Vec<Tab>> {
    let mut tabs = Vec::new();
    for block_id in view.windows.iter().filter_map(terminal_block) {
        let number = tabs.len() + 1;
        let pwd = rex.terminal_pwd(session_id, block_id)?.pwd;
        let foreground = rex.terminal_process(session_id, block_id)?.foreground;

        let cwd = pwd
            .as_deref()
            .and_then(path_of_file_url)
            .or_else(|| foreground.as_ref().and_then(|process| process.cwd.clone()))
            .filter(|cwd| !cwd.is_empty())
            .with_context(|| {
                format!(
                    "tab {number} of session {} reports no working directory",
                    view.label
                )
            })?;
        tabs.push(Tab {
            cwd: contract_home(&cwd, home),
            run: foreground
                .as_ref()
                .and_then(|process| run_of(process, command_line)),
        });
    }
    if tabs.is_empty() {
        bail!("session {} has no terminal tabs to save", view.label);
    }
    Ok(tabs)
}

/// The window's focused pane if it is a terminal, else its first terminal pane.
fn terminal_block(window: &WindowView) -> Option<&str> {
    let terminals: Vec<&str> = window
        .layers
        .iter()
        .flat_map(|layer| &layer.blocks)
        .filter(|block| block.flavor.starts_with(TERMINAL_FLAVOR_PREFIX))
        .map(|block| block.block_id.as_str())
        .collect();
    window
        .focused_block_id
        .as_deref()
        .filter(|focused| terminals.contains(focused))
        .or_else(|| terminals.first().copied())
}

/// What to type into the new tab's shell: the foreground program's full command
/// line when `command_line` finds the same program, else its name. Nothing when
/// the foreground is a shell.
fn run_of(process: &ProcessView, command_line: &impl Fn(u32) -> Option<String>) -> Option<String> {
    let name = process.name.trim_start_matches('-');
    if name.is_empty() || SHELLS.contains(&name) {
        return None;
    }
    let argv0 = process.argv0.as_deref().map(basename);
    let full = process.pid.and_then(command_line).filter(|line| {
        let program = line.split_whitespace().next().map(basename);
        program.is_some() && (program == Some(name) || program == argv0)
    });
    Some(full.unwrap_or_else(|| name.to_owned()))
}

/// The last path component, without a login shell's leading `-`.
fn basename(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_start_matches('-')
}

/// The decoded path of a `file://HOST/PATH` URL. A bare absolute path is kept.
fn path_of_file_url(url: &str) -> Option<String> {
    let path = match url.strip_prefix("file://") {
        Some(rest) => &rest[rest.find('/')?..],
        None if url.starts_with('/') => url,
        None => return None,
    };
    percent_decode(path)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// `~/…` for a path inside `home`, which a preset expands back. `home` itself
/// stays absolute: a preset expands only a leading `~/`.
fn contract_home(path: &str, home: Option<&Path>) -> String {
    let relative = home.and_then(|home| Path::new(path).strip_prefix(home).ok());
    match relative {
        Some(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.to_owned(),
    }
}

/// A file name for a new preset: the name with anything but letters, digits,
/// `-`, `_`, and `.` replaced by `-`, never hidden.
fn file_name(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let stem = stem.trim_start_matches('.');
    format!("{}.json", if stem.is_empty() { "session" } else { stem })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, argv0: Option<&str>) -> ProcessView {
        ProcessView {
            pid: Some(7),
            name: name.into(),
            argv0: argv0.map(str::to_owned),
            cwd: None,
        }
    }

    #[test]
    fn decodes_file_urls() {
        assert_eq!(
            path_of_file_url("file://host/Users/me/web%20app").as_deref(),
            Some("/Users/me/web app")
        );
        assert_eq!(path_of_file_url("file:///tmp").as_deref(), Some("/tmp"));
        assert_eq!(
            path_of_file_url("/plain/path").as_deref(),
            Some("/plain/path")
        );
        assert_eq!(path_of_file_url("file://host"), None);
        assert_eq!(path_of_file_url("file://host/bad%2"), None);
        assert_eq!(path_of_file_url("file://host/bad%zz"), None);
        assert_eq!(path_of_file_url("relative"), None);
        assert_eq!(path_of_file_url(""), None);
    }

    #[test]
    fn contracts_only_paths_inside_home() {
        let home = Some(Path::new("/Users/me"));
        assert_eq!(contract_home("/Users/me/code", home), "~/code");
        assert_eq!(contract_home("/Users/me", home), "/Users/me");
        assert_eq!(contract_home("/Users/me2/code", home), "/Users/me2/code");
        assert_eq!(contract_home("/tmp", home), "/tmp");
        assert_eq!(contract_home("/Users/me/code", None), "/Users/me/code");
    }

    #[test]
    fn runs_nothing_for_a_shell() {
        let never = |_: u32| -> Option<String> { panic!("a shell needs no command line") };
        for (name, argv0) in [
            ("fish", "-fish"),
            ("-zsh", "-zsh"),
            ("bash", "bash"),
            ("", ""),
        ] {
            assert_eq!(run_of(&process(name, Some(argv0)), &never), None, "{name}");
        }
    }

    #[test]
    fn runs_the_full_command_line_of_the_same_program() {
        let line = |text: &'static str| move |_: u32| Some(text.to_owned());
        let nvim = process("nvim", Some("nvim"));
        assert_eq!(
            run_of(&nvim, &line("nvim src/main.rs")).as_deref(),
            Some("nvim src/main.rs")
        );
        assert_eq!(
            run_of(&nvim, &line("/opt/homebrew/bin/nvim -R x")).as_deref(),
            Some("/opt/homebrew/bin/nvim -R x")
        );
        // A different program at that PID (or a remote one) keeps the name.
        assert_eq!(run_of(&nvim, &line("vim x")).as_deref(), Some("nvim"));
        assert_eq!(run_of(&nvim, &line("")).as_deref(), Some("nvim"));
        assert_eq!(run_of(&nvim, &|_| None).as_deref(), Some("nvim"));
        // argv0 may differ from the executable name.
        let node = process("node", Some("tsx"));
        assert_eq!(
            run_of(&node, &line("tsx watch")).as_deref(),
            Some("tsx watch")
        );
    }

    #[test]
    fn names_files_after_the_preset() {
        assert_eq!(file_name("web app"), "web-app.json");
        assert_eq!(file_name("a/b\\c"), "a-b-c.json");
        assert_eq!(file_name("..hidden"), "hidden.json");
        assert_eq!(file_name("..."), "session.json");
        assert_eq!(file_name("ñandú_1.2"), "ñandú_1.2.json");
    }
}
