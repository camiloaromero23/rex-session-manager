//! `init-lua`: the Lua module the Rex config loads to reach this binary.
//!
//! Rex runs Lua actions with a bare `PATH`, so the module calls the binary by
//! absolute path. The path is written in when the module is printed, which
//! keeps it right wherever Homebrew, mise, or cargo installed the binary.

use std::path::Path;

use anyhow::{Context, Result, bail};

const TEMPLATE: &str = include_str!("bridge.lua");
const PLACEHOLDER: &str = "@BINARY@";

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
}
