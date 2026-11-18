//! `rex-session-manager`: project sessions for the Rex terminal multiplexer.
//!
//! The binary owns the logic: it reads the project presets, talks to the Rex
//! server, and builds the menus. The Rex config keeps only what must run inside
//! Rex itself: the `rex.ui.pick` prompt (it can only ask from a Lua action) and
//! `rex.client.queue("session.select", …)`, which runs after that action
//! returns. Every subcommand that returns data prints exactly one JSON value on
//! stdout; errors go to stderr with a non-zero exit status.

mod delete;
mod marks;
mod menu;
mod preset;
mod rex;
mod save;
mod session;

use std::{
    path::PathBuf,
    process::{ExitCode, Stdio},
};

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::{
    marks::{Marks, Selector},
    menu::{Menu, Mode},
    rex::{RexApi, RexCli},
};

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Directory holding the project preset files (`*.json`). Defaults to
    /// `$XDG_CONFIG_HOME/rex/sessions`, or `~/.config/rex/sessions`.
    #[arg(long, env = "REX_SESSIONS_DIR", global = true, value_name = "DIR")]
    sessions_dir: Option<PathBuf>,

    /// Directory holding `marks.json`. Defaults to
    /// `$XDG_STATE_HOME/rex-session-manager`, or
    /// `~/.local/state/rex-session-manager`.
    #[arg(long, env = "REX_MARKS_DIR", global = true, value_name = "DIR")]
    marks_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the `rex.ui.pick` menu for MODE as JSON: `{title, confirm, items}`.
    Menu {
        #[arg(value_enum)]
        mode: Mode,
    },
    /// Reuse the session labelled PROJECT, or create it from its preset.
    ///
    /// Prints `{"session_id": …, "window_id": …}`; `window_id` is present only
    /// for a newly created session. Selecting it in the app is left to the
    /// caller.
    Jump { project: String },
    /// Destroy the running session SESSION_ID and every block in it.
    Kill { session_id: String },
    /// Save the running session SESSION_ID as a preset named after its label.
    ///
    /// Each tab keeps its focused terminal pane: its working directory and the
    /// program in its foreground, unless that is the shell. Prints
    /// `{"status": "created"|"overwritten", "name": …, "path": …}`. When a preset
    /// of that name exists and --overwrite is absent, nothing is written and
    /// the status is `exists`, with a `menu` asking whether to overwrite it.
    Save {
        /// Replace the existing preset NAME, in its current file. NAME is the
        /// `name` the `exists` status reported; a session labelled otherwise by
        /// now is refused, so only the confirmed preset is ever replaced.
        #[arg(long, value_name = "NAME")]
        overwrite: Option<String>,
        session_id: String,
    },
    /// Delete the project preset NAME.
    ///
    /// Without --confirmed nothing is removed: prints
    /// `{"status": "confirm", "name": …, "path": …, "menu": …}`, where `menu`
    /// asks whether to delete that file. With --confirmed, removes the file
    /// holding NAME and prints `{"status": "deleted", "name": …, "path": …}`.
    /// Running sessions are never touched.
    Delete {
        /// Remove NAME instead of asking whether to.
        #[arg(long)]
        confirmed: bool,
        /// The preset name, not its file name.
        name: String,
    },
    /// Mark the running session SESSION_ID, saving it as a preset first when it
    /// has none.
    ///
    /// A new mark is appended; a marked preset keeps its position and records
    /// this session. Prints `{"status": "added"|"updated", "name": …,
    /// "position": …, "key": …, "saved": …}`; `key` (q, w, e, r) is left out
    /// past position 4, and `saved` says whether a preset was written.
    Mark { session_id: String },
    /// Remove the mark NAME; the marks after it move up one position.
    ///
    /// Prints `{"status": "removed", "name": …, "position": …}`.
    Unmark { name: String },
    /// Go to a mark: its recorded session while it runs, else the session
    /// labelled with its name, else a new session from its preset.
    ///
    /// Prints `{"session_id": …, "window_id": …}` like `jump`.
    JumpMark {
        /// The mark's 1-based position, as the q, w, e, r keys use.
        #[arg(
            long,
            value_name = "N",
            conflicts_with = "name",
            required_unless_present = "name",
            value_parser = clap::value_parser!(u64).range(1..)
        )]
        position: Option<u64>,
        /// The mark's name, as the marks menu uses.
        name: Option<String>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rex-session-manager: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let rex = RexCli::locate();
    // Resolved lazily: `kill` never reads the presets.
    let sessions_dir = || {
        cli.sessions_dir
            .clone()
            .map_or_else(preset::default_directory, Ok)
    };
    let load_marks = || {
        let directory = cli
            .marks_dir
            .clone()
            .map_or_else(marks::default_directory, Ok)?;
        Marks::load(&directory)
    };
    let home = std::env::home_dir();

    match cli.command {
        Command::Menu { mode: Mode::Marks } => print_json(&marks::menu(&load_marks()?)),
        Command::Menu { mode: Mode::Unmark } => print_json(&marks::unmark_menu(&load_marks()?)),
        Command::Menu { mode } => {
            let items = match mode {
                Mode::Projects | Mode::Delete => {
                    menu::project_items(&preset::load(&sessions_dir()?)?)
                }
                Mode::Active | Mode::Close | Mode::Save => {
                    menu::session_items(&rex.list_sessions()?)
                }
                Mode::Marks | Mode::Unmark => unreachable!("handled above"),
            };
            print_json(&Menu::new(mode, items))
        }
        Command::Jump { project } => {
            let presets = preset::load(&sessions_dir()?)?;
            print_json(&session::jump(&rex, &presets, &project)?)
        }
        Command::Kill { session_id } => rex.destroy_session(&session_id),
        Command::Save {
            overwrite,
            session_id,
        } => print_json(&save::save(
            &rex,
            &sessions_dir()?,
            &session_id,
            overwrite.as_deref(),
            home.as_deref(),
            command_line,
        )?),
        Command::Delete { confirmed, name } => {
            print_json(&delete::delete(&sessions_dir()?, &name, confirmed)?)
        }
        Command::Mark { session_id } => {
            let mut marks = load_marks()?;
            print_json(&marks::mark(
                &rex,
                &mut marks,
                &sessions_dir()?,
                &session_id,
                home.as_deref(),
                command_line,
            )?)
        }
        Command::Unmark { name } => print_json(&marks::unmark(&mut load_marks()?, &name)?),
        Command::JumpMark { position, name } => {
            let selector = match (position, name) {
                (Some(position), _) => Selector::Position(usize::try_from(position)?),
                (None, Some(name)) => Selector::Name(name),
                (None, None) => unreachable!("clap requires one"),
            };
            let mut marks = load_marks()?;
            print_json(&marks::jump(
                &rex,
                &mut marks,
                &sessions_dir()?,
                &selector,
                home.as_deref(),
            )?)
        }
    }
}

/// The full command line of local process `pid`, which Rex does not report.
/// `/bin/ps` by absolute path: Lua actions run with a bare `PATH`.
fn command_line(pid: u32) -> Option<String> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "args=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let line = String::from_utf8(output.stdout).ok()?;
    let line = line.trim();
    (output.status.success() && !line.is_empty()).then(|| line.to_owned())
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}
