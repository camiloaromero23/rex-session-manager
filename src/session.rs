//! Jumping to a project's session: reuse it by label, or create it from the
//! preset.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::{
    preset::{Preset, Presets, Tab},
    rex::{Block, CreateSession, InitialWindow, Layout, RexApi, Session, TerminalOptions},
};

const SHELL_FLAVOR: &str = "com.superlogical.terminal.shell";

/// Every tab starts this interactive fish. The absolute path matters: Rex.app's
/// `PATH` does not include Homebrew's bin directory.
const SHELL: [&str; 3] = ["/opt/homebrew/bin/fish", "--login", "--interactive"];

/// Where to go after a jump. `window_id` is set only for a newly created
/// session, so an existing one keeps its current tab.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Target {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
}

/// Returns the session labelled `project`, creating it from its preset when no
/// session has that label.
///
/// The label match is exact and case-sensitive. Two sessions sharing the label
/// is an error rather than a guess.
pub fn jump(rex: &impl RexApi, presets: &Presets, project: &str) -> Result<Target> {
    let preset = presets
        .get(project)
        .with_context(|| format!("unknown project: {project}"))?;

    let sessions = rex.list_sessions()?;
    match labelled(&sessions, project)? {
        Some(session) => Ok(Target {
            session_id: session.session_id.clone(),
            window_id: None,
        }),
        None => create(rex, preset, std::env::home_dir().as_deref()),
    }
}

/// The one session labelled exactly `label`, if any; two is an error.
pub fn labelled<'a>(sessions: &'a [Session], label: &str) -> Result<Option<&'a Session>> {
    let mut matches = sessions.iter().filter(|session| session.label == label);
    match (matches.next(), matches.next()) {
        (Some(_), Some(_)) => bail!("more than one session is labelled {label}"),
        (session, _) => Ok(session),
    }
}

/// Creates a session from `preset` and targets its first window.
pub fn create(rex: &impl RexApi, preset: &Preset, home: Option<&Path>) -> Result<Target> {
    let created = rex.create_session(&create_request(preset, home))?;
    Ok(Target {
        session_id: created.session_id,
        window_id: created
            .initial_windows
            .into_iter()
            .next()
            .map(|w| w.window_id),
    })
}

/// The `session.create` request for a preset: one window per tab.
fn create_request(preset: &Preset, home: Option<&Path>) -> CreateSession {
    CreateSession {
        label: preset.name.clone(),
        initial_windows: preset
            .tabs
            .iter()
            .map(|tab| InitialWindow {
                layout: Layout {
                    block: Block {
                        flavor: SHELL_FLAVOR,
                        options: terminal_options(tab, home),
                    },
                },
            })
            .collect(),
    }
}

/// A tab with `run` types the program into the interactive shell, so it runs
/// in the foreground and the shell returns when it exits.
fn terminal_options(tab: &Tab, home: Option<&Path>) -> TerminalOptions {
    TerminalOptions {
        cwd: expand_home(&tab.cwd, home),
        command: SHELL.map(str::to_owned).to_vec(),
        shell: "none",
        initial_input: tab.run.as_ref().map(|run| format!("{run}\n")),
    }
}

/// Expands only a leading `~/`; everything else, spaces included, stays literal.
fn expand_home(path: &str, home: Option<&Path>) -> String {
    match (path.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest).display().to_string(),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::rex::{CreatedSession, CreatedWindow, Session};

    /// An in-memory Rex server that records the sessions it is asked to create.
    #[derive(Default)]
    struct FakeRex {
        sessions: Vec<Session>,
        created: RefCell<Vec<String>>,
    }

    impl RexApi for FakeRex {
        fn list_sessions(&self) -> Result<Vec<Session>> {
            Ok(self.sessions.clone())
        }

        fn create_session(&self, request: &CreateSession) -> Result<CreatedSession> {
            self.created.borrow_mut().push(request.label.clone());
            Ok(CreatedSession {
                session_id: "session:new".into(),
                initial_windows: vec![
                    CreatedWindow {
                        window_id: "window:first".into(),
                    },
                    CreatedWindow {
                        window_id: "window:second".into(),
                    },
                ],
            })
        }

        fn destroy_session(&self, _: &str) -> Result<()> {
            unreachable!("jump never destroys")
        }
    }

    fn presets() -> Presets {
        let preset = Preset {
            name: "dotfiles".into(),
            tabs: vec![
                Tab {
                    cwd: "~/dotfiles".into(),
                    run: Some("nvim".into()),
                },
                Tab {
                    cwd: "/tmp/with space".into(),
                    run: None,
                },
            ],
        };
        Presets::from([(preset.name.clone(), preset)])
    }

    fn session(id: &str, label: &str) -> Session {
        Session {
            session_id: id.into(),
            label: label.into(),
        }
    }

    #[test]
    fn reuses_the_session_with_the_exact_label() {
        let rex = FakeRex {
            sessions: vec![
                session("session:a", "Dotfiles"),
                session("session:b", "dotfiles"),
            ],
            ..FakeRex::default()
        };

        let target = jump(&rex, &presets(), "dotfiles").unwrap();

        assert_eq!(
            target,
            Target {
                session_id: "session:b".into(),
                window_id: None
            }
        );
        assert!(rex.created.borrow().is_empty());
    }

    #[test]
    fn creates_a_missing_session_and_targets_its_first_window() {
        let rex = FakeRex::default();

        let target = jump(&rex, &presets(), "dotfiles").unwrap();

        assert_eq!(
            target,
            Target {
                session_id: "session:new".into(),
                window_id: Some("window:first".into())
            }
        );
        assert_eq!(*rex.created.borrow(), ["dotfiles"]);
    }

    #[test]
    fn refuses_an_ambiguous_label() {
        let rex = FakeRex {
            sessions: vec![
                session("session:a", "dotfiles"),
                session("session:b", "dotfiles"),
            ],
            ..FakeRex::default()
        };

        let error = jump(&rex, &presets(), "dotfiles").unwrap_err();

        assert_eq!(
            error.to_string(),
            "more than one session is labelled dotfiles"
        );
        assert!(rex.created.borrow().is_empty());
    }

    #[test]
    fn refuses_an_unknown_project() {
        let error = jump(&FakeRex::default(), &presets(), "nope").unwrap_err();
        assert_eq!(error.to_string(), "unknown project: nope");
    }

    #[test]
    fn builds_one_fish_window_per_tab() {
        let request = create_request(&presets()["dotfiles"], Some(Path::new("/Users/me")));

        let options: Vec<_> = request
            .initial_windows
            .iter()
            .map(|window| &window.layout.block.options)
            .collect();
        assert_eq!(options[0].cwd, "/Users/me/dotfiles");
        assert_eq!(options[0].initial_input.as_deref(), Some("nvim\n"));
        assert_eq!(options[0].command, SHELL);
        assert_eq!(options[1].cwd, "/tmp/with space");
        assert_eq!(options[1].initial_input, None);
    }

    #[test]
    fn expands_only_a_leading_tilde_slash() {
        let home = Some(Path::new("/Users/me"));
        assert_eq!(expand_home("~/notes", home), "/Users/me/notes");
        assert_eq!(expand_home("~notes", home), "~notes");
        assert_eq!(expand_home("/a/~/b", home), "/a/~/b");
        assert_eq!(expand_home("~/notes", None), "~/notes");
    }
}
