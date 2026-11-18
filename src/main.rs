//! `rex-session-manager`: project sessions for the Rex terminal multiplexer.
//!
//! The binary owns the logic: it reads the project presets, talks to the Rex
//! server, and builds the menus. The Rex config keeps only what must run inside
//! Rex itself: the `rex.ui.pick` prompt (it can only ask from a Lua action) and
//! `rex.client.queue("session.select", …)`, which runs after that action
//! returns. Every subcommand that returns data prints exactly one JSON value on
//! stdout; errors go to stderr with a non-zero exit status.

mod menu;
mod preset;
mod rex;
mod session;

use std::{path::PathBuf, process::ExitCode};

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::{
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

    match cli.command {
        Command::Menu { mode } => {
            let items = match mode {
                Mode::Projects => menu::project_items(&preset::load(&sessions_dir()?)?),
                Mode::Active | Mode::Close => menu::session_items(&rex.list_sessions()?),
            };
            print_json(&Menu::new(mode, items))
        }
        Command::Jump { project } => {
            let presets = preset::load(&sessions_dir()?)?;
            print_json(&session::jump(&rex, &presets, &project)?)
        }
        Command::Kill { session_id } => rex.destroy_session(&session_id),
    }
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}
