//! Project presets: one JSON file per project in the sessions directory.
//!
//! ```json
//! {
//!   "name": "dotfiles",
//!   "tabs": [
//!     { "cwd": "~/dotfiles", "run": "nvim" },
//!     { "cwd": "~/dotfiles" }
//!   ]
//! }
//! ```
//!
//! `name` is required and unique across files; the file name is only for
//! humans. `tabs` is non-empty; each tab needs a `cwd` and may name a program to
//! `run`. Unknown keys are rejected, so a typo such as `command` for `run` fails
//! loudly instead of being ignored. The directory is read on every call, so a
//! preset change takes effect on the next menu or jump.
//!
//! `save` writes presets back: a new file never replaces an existing one, and a
//! replacement is written beside its target and renamed over it, so a reader
//! never sees a half-written preset.

use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// Presets keyed by project name, sorted by name.
pub type Presets = BTreeMap<String, Preset>;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

impl Preset {
    /// Parses and validates one preset file's contents.
    pub fn parse(text: &str) -> Result<Self> {
        // Serde would also accept a positional array for a struct; a preset is
        // only ever an object.
        let value: serde_json::Value = serde_json::from_str(text)?;
        ensure!(value.is_object(), "the file must contain a JSON object");
        let preset: Self = serde_json::from_value(value)?;
        preset.validate()?;
        Ok(preset)
    }

    /// The preset as a file: two-space indented JSON and a final newline. The
    /// text is parsed back first, so `save` never writes a preset `load` would
    /// reject.
    pub fn to_file_text(&self) -> Result<String> {
        let text = serde_json::to_string_pretty(self)? + "\n";
        Self::parse(&text).context("the generated preset is invalid")?;
        Ok(text)
    }

    fn validate(&self) -> Result<()> {
        ensure!(!self.name.is_empty(), r#"needs a non-empty string "name""#);
        ensure!(!self.tabs.is_empty(), "needs at least one tab");
        for (index, tab) in self.tabs.iter().enumerate() {
            let number = index + 1;
            ensure!(
                !tab.cwd.is_empty(),
                r#"tab {number} needs a non-empty string "cwd""#
            );
            ensure!(
                tab.run.as_deref() != Some(""),
                r#"tab {number} has a "run" that is not a non-empty string"#
            );
        }
        Ok(())
    }
}

/// `$XDG_CONFIG_HOME/rex/sessions`, or `$HOME/.config/rex/sessions`.
pub fn default_directory() -> Result<PathBuf> {
    let config = match env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(config) => PathBuf::from(config),
        None => env::home_dir()
            .context("cannot locate the home directory for the sessions directory")?
            .join(".config"),
    };
    Ok(config.join("rex").join("sessions"))
}

/// Loads every `*.json` preset in `directory`, in file-name order.
///
/// The extension match ignores case and hidden files are skipped. Any broken
/// preset, or two presets sharing a name, is an error naming the file: a preset
/// never disappears silently.
pub fn load(directory: &Path) -> Result<Presets> {
    let files = preset_files(directory)?;
    ensure!(
        !files.is_empty(),
        "no project session files in {}",
        directory.display()
    );
    Ok(read_all(files)?
        .into_values()
        .map(|(_, preset)| (preset.name.clone(), preset))
        .collect())
}

/// The file holding the preset called `name`, if any.
///
/// Every preset is read with the same rules as [`load`], so a broken preset or
/// a duplicate name is still an error. A missing directory holds no presets.
pub fn find(directory: &Path, name: &str) -> Result<Option<PathBuf>> {
    let files = match preset_files(directory) {
        Ok(files) => files,
        Err(_) if !directory.exists() => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(read_all(files)?.remove(name).map(|(path, _)| path))
}

/// Writes `text` to `path`, which must not exist yet, creating the directory
/// if needed. An existing file, preset or not, is never replaced; a preset
/// already at `path` is named in the error, since a label whose file name
/// clashes with it needs renaming.
pub fn create(path: &Path, text: &str) -> Result<()> {
    let directory = parent(path)?;
    fs::create_dir_all(directory)
        .with_context(|| format!("cannot create {}", directory.display()))?;
    let temporary = write_temporary(path, text)?;
    // A hard link fails instead of replacing an existing target, atomically.
    let linked = fs::hard_link(&temporary, path);
    let _ = fs::remove_file(&temporary);
    match linked {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => match held_preset(path) {
            Some(name) => bail!(
                "cannot create {}: it already holds preset {name:?}; rename the session or that preset",
                path.display()
            ),
            None => bail!("cannot create {}: the file already exists", path.display()),
        },
        Err(error) => Err(error).with_context(|| format!("cannot create {}", path.display())),
    }
}

/// The name of the preset the file at `path` holds, if it holds one.
///
/// The file is read at the moment of the clash, so a file that raced into place
/// or was not a preset falls back to the generic error.
fn held_preset(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    Preset::parse(&text).ok().map(|preset| preset.name)
}

/// Replaces the file at `path` with `text` in one rename.
pub fn replace(path: &Path, text: &str) -> Result<()> {
    let temporary = write_temporary(path, text)?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        anyhow::Error::new(error).context(format!("cannot replace {}", path.display()))
    })
}

/// Writes `text` to a hidden file beside `path`, which `load` skips.
fn write_temporary(path: &Path, text: &str) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("{} has no file name", path.display()))?;
    let temporary = parent(path)?.join(format!(".{file_name}.{}.tmp", process::id()));
    let written = fs::File::create(&temporary).and_then(|mut file| {
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
    });
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| format!("cannot write {}", temporary.display()));
    }
    Ok(temporary)
}

fn parent(path: &Path) -> Result<&Path> {
    path.parent()
        .with_context(|| format!("{} has no parent directory", path.display()))
}

/// The preset files in `directory`, sorted by file name.
fn preset_files(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(directory).with_context(|| format!("cannot list {}", directory.display()))?
    {
        let path = entry
            .with_context(|| format!("cannot list {}", directory.display()))?
            .path();
        if is_preset_file(&path) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Parses every file, keyed by preset name with its source path.
fn read_all(files: Vec<PathBuf>) -> Result<BTreeMap<String, (PathBuf, Preset)>> {
    let mut presets: BTreeMap<String, (PathBuf, Preset)> = BTreeMap::new();
    for path in files {
        let text =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        let preset = Preset::parse(&text).with_context(|| path.display().to_string())?;
        if let Some((first, _)) = presets.get(&preset.name) {
            bail!(
                "duplicate project {:?} in {} and {}",
                preset.name,
                first.display(),
                path.display()
            );
        }
        presets.insert(preset.name.clone(), (path, preset));
    }
    Ok(presets)
}

fn is_preset_file(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| name.starts_with('.'));
    let json = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    !hidden && json
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_of(result: Result<impl std::fmt::Debug>) -> String {
        format!("{:#}", result.unwrap_err())
    }

    #[test]
    fn parses_a_preset_with_optional_run() {
        let preset = Preset::parse(
            r#"{"name": "dotfiles", "tabs": [{"cwd": "~/dotfiles", "run": "nvim"}, {"cwd": "~/dotfiles"}]}"#,
        )
        .unwrap();

        assert_eq!(preset.name, "dotfiles");
        assert_eq!(preset.tabs[0].run.as_deref(), Some("nvim"));
        assert_eq!(preset.tabs[1].run, None);
    }

    #[test]
    fn rejects_invalid_presets() {
        let cases = [
            (r#"{"tabs": [{"cwd": "/"}]}"#, "missing field `name`"),
            (
                r#"{"name": "", "tabs": [{"cwd": "/"}]}"#,
                r#"needs a non-empty string "name""#,
            ),
            (r#"{"name": "x", "tabs": []}"#, "needs at least one tab"),
            (
                r#"{"name": "x", "tabs": {}}"#,
                "invalid type: map, expected a sequence",
            ),
            (
                r#"{"name": "x", "tabs": [{"cwd": ""}]}"#,
                r#"tab 1 needs a non-empty string "cwd""#,
            ),
            (
                r#"{"name": "x", "tabs": [{"cwd": "/"}, {"cwd": "/", "run": ""}]}"#,
                r#"tab 2 has a "run" that is not a non-empty string"#,
            ),
            (
                r#"{"name": "x", "tabs": [{"cwd": "/", "command": "nvim"}]}"#,
                "unknown field `command`",
            ),
            (
                r#"{"name": "x", "dir": "/", "tabs": [{"cwd": "/"}]}"#,
                "unknown field `dir`",
            ),
            (
                r#"["x", [{"cwd": "/"}]]"#,
                "the file must contain a JSON object",
            ),
            ("{", "EOF while parsing"),
        ];

        for (text, expected) in cases {
            let error = error_of(Preset::parse(text));
            assert!(error.contains(expected), "{text}: {error}");
        }
    }

    #[test]
    fn loads_json_files_sorted_and_skips_others() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("b.json"),
            r#"{"name": "zeta", "tabs": [{"cwd": "/"}]}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("A.JSON"),
            r#"{"name": "alpha", "tabs": [{"cwd": "/"}]}"#,
        )
        .unwrap();
        fs::write(dir.path().join(".hidden.json"), "not json").unwrap();
        fs::write(dir.path().join("README.md"), "not json").unwrap();

        let presets = load(dir.path()).unwrap();

        assert_eq!(presets.keys().collect::<Vec<_>>(), ["alpha", "zeta"]);
    }

    #[test]
    fn names_the_file_of_a_broken_preset() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bad.json"), r#"{"name": ""}"#).unwrap();

        let error = error_of(load(dir.path()));

        assert!(
            error.starts_with(&dir.path().join("bad.json").display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn rejects_duplicate_names() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["one.json", "two.json"] {
            fs::write(
                dir.path().join(file),
                r#"{"name": "same", "tabs": [{"cwd": "/"}]}"#,
            )
            .unwrap();
        }

        let error = error_of(load(dir.path()));

        assert!(
            error.starts_with(r#"duplicate project "same" in "#),
            "{error}"
        );
        assert!(error.ends_with("two.json"), "{error}");
    }

    #[test]
    fn rejects_an_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(error_of(load(dir.path())).starts_with("no project session files in "));
    }

    #[test]
    fn creating_names_the_preset_already_holding_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("web-app.json");
        let held = r#"{"name": "web app", "tabs": [{"cwd": "/"}]}"#;
        fs::write(&path, held).unwrap();

        let error = error_of(create(
            &path,
            r#"{"name": "web-app", "tabs": [{"cwd": "/"}]}"#,
        ));

        assert_eq!(
            error,
            format!(
                r#"cannot create {}: it already holds preset "web app"; rename the session or that preset"#,
                path.display()
            )
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), held);
        let mut names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["web-app.json"], "no temporary file is left");
    }

    #[test]
    fn creating_keeps_the_generic_error_for_a_file_that_is_not_a_preset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("web-app.json");
        fs::write(&path, "not a preset").unwrap();

        let error = error_of(create(&path, r#"{"name": "x", "tabs": [{"cwd": "/"}]}"#));

        assert_eq!(
            error,
            format!("cannot create {}: the file already exists", path.display())
        );
    }
}
