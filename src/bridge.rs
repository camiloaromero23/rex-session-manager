//! `init`: the Lua bridge the Rex config loads to reach this binary.
//!
//! Rex runs Lua actions with a bare `PATH`, so the bridge calls the binary by
//! absolute path. `init` writes the bridge into the Rex config and makes
//! `init.lua` load it, so the path is right wherever Homebrew, mise, or cargo
//! installed the binary.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::preset;

const TEMPLATE: &str = include_str!("bridge.lua");
const PLACEHOLDER: &str = "@BINARY@";

/// The module the Rex config requires to reach the bridge.
const MODULE: &str = "rex_session_manager";
/// The bridge file, in the Rex config directory.
const FILE_NAME: &str = "rex_session_manager.lua";
/// The Rex config entry point that loads the bridge.
const INIT_FILE: &str = "init.lua";
/// The first line of a bridge `init` writes.
pub const HEADER: &str = "-- rex_session_manager.lua: the Rex side of `rex-session-manager init`.";
/// The first line the removed `init-lua` wrote: it counts as generated too, so
/// the file it left is replaced instead of refused.
const OLD_HEADER: &str = "-- rex_session_manager.lua: the Rex side of `rex-session-manager`.";

/// The module with `binary` written in as its `BINARY` path.
pub fn module(binary: &Path) -> Result<String> {
    if !binary.is_absolute() {
        bail!(
            "--bin must be an absolute path, because Rex runs actions from an unknown directory: {}",
            binary.display()
        );
    }
    let path = binary
        .to_str()
        .with_context(|| format!("the binary path is not UTF-8: {}", binary.display()))?;
    Ok(TEMPLATE.replacen(PLACEHOLDER, &lua_string(path), 1))
}

/// What `init` did, printed as one JSON object.
#[derive(Debug, Serialize)]
pub struct Report {
    pub status: &'static str,
    pub path: PathBuf,
    pub init_lua: Loaded,
}

/// What `init` did to `init.lua`.
#[derive(Debug, Serialize)]
pub struct Loaded {
    pub status: &'static str,
    pub path: PathBuf,
}

/// Writes the bridge into `config` and makes `config/init.lua` load it.
///
/// Everything is decided before anything is written, so a refusal leaves both
/// files as they were.
pub fn install(
    config: &Path,
    binary: &Path,
    prefix: &str,
    shell: Option<&str>,
    force: bool,
) -> Result<Report> {
    let text = module(binary)?;
    let bridge_path = config.join(FILE_NAME);
    let init_path = config.join(INIT_FILE);

    let bridge = plan_bridge(&bridge_path, &text, force)?;
    let init_lua = plan_init(&init_path, &require_line(prefix, shell))?;

    // Writes come last: the bridge was accepted and init.lua read by now.
    fs::create_dir_all(config).with_context(|| format!("cannot create {}", config.display()))?;
    for plan in [&bridge, &init_lua] {
        if let Some(text) = &plan.write {
            preset::replace(&plan.target, text)?;
        }
    }

    Ok(Report {
        status: bridge.status,
        path: bridge_path,
        init_lua: Loaded {
            status: init_lua.status,
            path: init_path,
        },
    })
}

/// What `install` will do to one file, decided before any write.
struct Plan {
    status: &'static str,
    /// The file to write, with a symlink followed to its target.
    target: PathBuf,
    /// The new text; `None` leaves the file alone.
    write: Option<String>,
}

/// Keeps an existing bridge only when this command, or the `init-lua` it
/// replaced, wrote it; anything else needs --force.
fn plan_bridge(path: &Path, text: &str, force: bool) -> Result<Plan> {
    let target = follow(path);
    let Some(existing) = read(&target, path)? else {
        return Ok(Plan {
            status: "created",
            target,
            write: Some(text.to_owned()),
        });
    };
    if !is_generated(&existing) && !force {
        bail!(
            "{} was not written by `rex-session-manager init`; pass --force to overwrite it",
            path.display()
        );
    }
    if existing == text {
        return Ok(Plan {
            status: "unchanged",
            target,
            write: None,
        });
    }
    Ok(Plan {
        status: "updated",
        target,
        write: Some(text.to_owned()),
    })
}

/// Appends the require line to an `init.lua` that does not load the bridge
/// yet, and leaves one that does byte-for-byte alone.
fn plan_init(path: &Path, require_line: &str) -> Result<Plan> {
    let target = follow(path);
    let Some(mut existing) = read(&target, path)? else {
        return Ok(Plan {
            status: "created",
            target,
            write: Some(format!("{require_line}\n")),
        });
    };
    if existing.lines().any(requires_bridge) {
        return Ok(Plan {
            status: "present",
            target,
            write: None,
        });
    }
    if !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(require_line);
    existing.push('\n');
    Ok(Plan {
        status: "appended",
        target,
        write: Some(existing),
    })
}

/// The text of `target`, or `None` when it does not exist. `path` is what the
/// person wrote, for the error.
fn read(target: &Path, path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(target) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// `path` with a symlink resolved to the file it names, so a write lands on
/// that file instead of replacing the link with a regular one.
fn follow(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether the bridge `text` was generated, by `init` or by `init-lua` before
/// it.
fn is_generated(text: &str) -> bool {
    matches!(text.lines().next(), Some(HEADER | OLD_HEADER))
}

/// Whether `line` requires the bridge, with either quote style and with or
/// without parentheses.
fn requires_bridge(line: &str) -> bool {
    let keyword = "require";
    let mut rest = line;
    while let Some((_, after)) = rest.split_once(keyword) {
        let after = after.trim_start();
        let after = after.strip_prefix('(').unwrap_or(after).trim_start();
        for quote in ['"', '\''] {
            if let Some(name) = after.strip_prefix(quote)
                && name
                    .strip_prefix(MODULE)
                    .is_some_and(|rest| rest.starts_with(quote))
            {
                return true;
            }
        }
        rest = after;
    }
    false
}

/// The line `init.lua` gets: `setup` with the prefix, and the shell when one
/// is set.
fn require_line(prefix: &str, shell: Option<&str>) -> String {
    let options = shell.map_or_else(String::new, |shell| {
        format!(", {{ shell = {} }}", lua_string(shell))
    });
    format!(
        "require(\"{MODULE}\").setup({}{options})",
        lua_string(prefix)
    )
}

/// `text` as a double-quoted Lua string literal.
fn lua_string(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push('"');
    for c in text.chars() {
        match c {
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            c if c.is_ascii_control() => literal.push_str(&format!("\\{:03}", c as u32)),
            c => literal.push(c),
        }
    }
    literal.push('"');
    literal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_string_escapes_quotes_backslashes_and_controls() {
        assert_eq!(lua_string("/a b/c"), "\"/a b/c\"");
        assert_eq!(
            lua_string("a\"b\\c\nd\re\tf\0"),
            "\"a\\\"b\\\\c\\nd\\re\\009f\\000\""
        );
    }

    #[test]
    fn the_template_holds_the_placeholder_exactly_once() {
        assert_eq!(TEMPLATE.matches(PLACEHOLDER).count(), 1);
    }

    #[test]
    fn the_template_starts_with_the_header_this_command_writes() {
        assert_eq!(TEMPLATE.lines().next(), Some(HEADER));
    }

    #[test]
    fn the_require_line_names_the_prefix_and_the_shell() {
        assert_eq!(
            require_line("ctrl+a", None),
            "require(\"rex_session_manager\").setup(\"ctrl+a\")"
        );
        assert_eq!(
            require_line("ctrl+b", Some("/opt/homebrew/bin/fish")),
            "require(\"rex_session_manager\").setup(\"ctrl+b\", \
             { shell = \"/opt/homebrew/bin/fish\" })"
        );
    }

    #[test]
    fn recognises_the_require_of_the_bridge_in_either_quote_style() {
        for line in [
            "require(\"rex_session_manager\").setup(\"ctrl+a\")",
            "require('rex_session_manager')",
            "require \"rex_session_manager\"",
            "require   (  'rex_session_manager'  )",
            "local bridge = require(\"rex_session_manager\")",
        ] {
            assert!(requires_bridge(line), "{line}");
        }
        for line in [
            "-- nothing here",
            "require(\"rex_session_manager_extra\")",
            "require(\"sessions\")",
            "requires(\"rex_session_manager\")",
        ] {
            assert!(!requires_bridge(line), "{line}");
        }
    }

    #[test]
    fn recognises_both_generated_headers() {
        assert!(is_generated(HEADER));
        assert!(is_generated(&format!(
            "{OLD_HEADER}\n\nlocal BINARY = \"/x\"\n"
        )));
        assert!(!is_generated("-- my own module\n"));
        assert!(!is_generated(""));
    }
}
