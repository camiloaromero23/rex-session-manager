//! Typed access to the Rex server API.
//!
//! [`RexApi`] is the small surface the session manager needs; [`RexCli`]
//! implements it with `rex api call METHOD JSON`, which is the documented,
//! stable way for an external process to reach the server. Arguments are passed
//! as separate process arguments, never through a shell.

use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// The Rex server operations the session manager uses.
pub trait RexApi {
    fn list_sessions(&self) -> Result<Vec<Session>>;
    fn create_session(&self, request: &CreateSession) -> Result<CreatedSession>;
    fn destroy_session(&self, session_id: &str) -> Result<()>;
}

/// One running session, as listed by `session.list`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Session {
    pub session_id: String,
    #[serde(default)]
    pub label: String,
}

/// Input for `session.create`: the session and its windows in one
/// all-or-nothing call.
#[derive(Debug, PartialEq, Serialize)]
pub struct CreateSession {
    pub label: String,
    pub initial_windows: Vec<InitialWindow>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct InitialWindow {
    pub layout: Layout,
}

/// A layout node. Only single-block windows are needed today; a split would
/// become a second shape here.
#[derive(Debug, PartialEq, Serialize)]
pub struct Layout {
    pub block: Block,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Block {
    pub flavor: &'static str,
    pub options: TerminalOptions,
}

/// Options for the `com.superlogical.terminal.shell` block flavor.
#[derive(Debug, PartialEq, Serialize)]
pub struct TerminalOptions {
    pub cwd: String,
    pub command: Vec<String>,
    /// `"none"` runs `command` directly instead of through a login shell.
    pub shell: &'static str,
    /// Text typed into the terminal once it starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_input: Option<String>,
}

/// Output of `session.create`.
#[derive(Debug, Deserialize)]
pub struct CreatedSession {
    pub session_id: String,
    #[serde(default)]
    pub initial_windows: Vec<CreatedWindow>,
}

#[derive(Debug, Deserialize)]
pub struct CreatedWindow {
    pub window_id: String,
}

#[derive(Deserialize)]
struct SessionList {
    #[serde(default)]
    sessions: Vec<Session>,
}

#[derive(Serialize)]
struct SessionTarget<'a> {
    session_id: &'a str,
}

/// Where the Rex server installs its `rex` client on macOS, relative to `HOME`.
const MACOS_REX: &str = "Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex";

/// [`RexApi`] backed by the `rex` command-line client.
#[derive(Debug)]
pub struct RexCli {
    program: PathBuf,
}

impl RexCli {
    /// Finds the `rex` client.
    ///
    /// A Lua action runs this binary with the Rex server's bare environment
    /// (`PATH=/usr/bin:/bin:/usr/sbin:/sbin`, no `REX_BIN_DIR`), so a plain
    /// `PATH` lookup is not enough. In order: `$REX_BIN`, `$REX_BIN_DIR/rex`
    /// (set in Rex terminals), `rex` on `PATH`, then the macOS install location.
    pub fn locate() -> Self {
        let home = env::home_dir();
        let program = locate_rex(
            env::var_os("REX_BIN").map(PathBuf::from),
            env::var_os("REX_BIN_DIR").map(PathBuf::from),
            env::var_os("PATH")
                .map(|path| env::split_paths(&path).collect())
                .unwrap_or_default(),
            home.map(|home| home.join(MACOS_REX)),
            Path::is_file,
        );
        Self { program }
    }

    fn call<A, R>(&self, method: &str, args: &A) -> Result<R>
    where
        A: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let args = serde_json::to_string(args)?;
        let output = Command::new(&self.program)
            .args(["api", "call", method])
            .arg(&args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot run {}", self.program.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("{method} failed ({}): {}", output.status, stderr.trim());
        }
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("{method} returned an unexpected response"))
    }
}

/// The first existing candidate, or bare `rex` so the error names it.
fn locate_rex(
    explicit: Option<PathBuf>,
    bin_dir: Option<PathBuf>,
    path: Vec<PathBuf>,
    installed: Option<PathBuf>,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    if let Some(explicit) = explicit.filter(|explicit| !explicit.as_os_str().is_empty()) {
        return explicit;
    }
    bin_dir
        .into_iter()
        .chain(path)
        .map(|dir| dir.join("rex"))
        .chain(installed)
        .find(|candidate| exists(candidate))
        .unwrap_or_else(|| PathBuf::from("rex"))
}

impl RexApi for RexCli {
    fn list_sessions(&self) -> Result<Vec<Session>> {
        let list: SessionList = self.call("session.list", &serde_json::Map::new())?;
        Ok(list.sessions)
    }

    fn create_session(&self, request: &CreateSession) -> Result<CreatedSession> {
        self.call("session.create", request)
    }

    fn destroy_session(&self, session_id: &str) -> Result<()> {
        // `rex api call` attaches to the top-level `session_id` before the
        // request, which authorizes this connection for exactly that ID.
        let _: serde::de::IgnoredAny =
            self.call("session.destroy", &SessionTarget { session_id })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn create_session_serializes_to_the_api_shape() {
        let request = CreateSession {
            label: "dotfiles".into(),
            initial_windows: vec![InitialWindow {
                layout: Layout {
                    block: Block {
                        flavor: "com.superlogical.terminal.shell",
                        options: TerminalOptions {
                            cwd: "/tmp".into(),
                            command: vec!["/bin/fish".into()],
                            shell: "none",
                            initial_input: None,
                        },
                    },
                },
            }],
        };

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "label": "dotfiles",
                "initial_windows": [{
                    "layout": {"block": {
                        "flavor": "com.superlogical.terminal.shell",
                        "options": {"cwd": "/tmp", "command": ["/bin/fish"], "shell": "none"}
                    }}
                }]
            })
        );
    }

    #[test]
    fn locates_rex_in_priority_order() {
        let all = |_: &Path| true;
        let none = |_: &Path| false;
        let only = |wanted: &'static str| move |path: &Path| path == Path::new(wanted);
        let locate = |explicit: Option<&str>, exists: &dyn Fn(&Path) -> bool| {
            locate_rex(
                explicit.map(PathBuf::from),
                Some("/bin-dir".into()),
                vec!["/a".into(), "/b".into()],
                Some("/installed/rex".into()),
                exists,
            )
        };

        assert_eq!(
            locate(Some("/explicit/rex"), &none),
            Path::new("/explicit/rex")
        );
        assert_eq!(locate(Some(""), &all), Path::new("/bin-dir/rex"));
        assert_eq!(locate(None, &all), Path::new("/bin-dir/rex"));
        assert_eq!(locate(None, &only("/b/rex")), Path::new("/b/rex"));
        assert_eq!(
            locate(None, &only("/installed/rex")),
            Path::new("/installed/rex")
        );
        assert_eq!(locate(None, &none), Path::new("rex"));
    }

    #[test]
    fn session_list_tolerates_missing_labels() {
        let list: SessionList =
            serde_json::from_value(json!({"sessions": [{"session_id": "session:1"}]})).unwrap();
        assert_eq!(list.sessions[0].label, "");
    }
}
