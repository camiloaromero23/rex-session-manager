# rex-session-manager

Project sessions for the [Rex](https://superlogical.com/rex) terminal
multiplexer: open a project from a JSON preset, switch between running
sessions, close one, save a running session as a preset, or delete a preset you
no longer need.

The binary owns the logic. A thin Lua bridge in the Rex config calls it, because
two things only work from Lua: the native `rex.ui.pick` prompt and the
client-side `session.select`.

```text
key → Lua action → rex-session-manager menu <mode>  → JSON menu
                 → rex.ui.pick                       → chosen id
                 → rex-session-manager jump|kill     → rex api call …
                 → rex.client.queue("session.select") (jump / switch)
```

## Install

```sh
cargo install --path .
```

Rex runs Lua actions with a bare environment
(`PATH=/usr/bin:/bin:/usr/sbin:/sbin`), so the Lua bridge calls the binary by
absolute path (`$HOME/.cargo/bin/rex-session-manager`). The binary finds the
`rex` client itself, in this order: `$REX_BIN`, `$REX_BIN_DIR/rex` (set in Rex
terminals), `rex` on `PATH`, then the Rex server's macOS install location.

## Commands

| Command | Does | Prints |
| --- | --- | --- |
| `menu projects` / `menu delete` | Lists every preset, sorted by name; `menu delete` asks which preset to delete. | `{title, confirm, items}` for `rex.ui.pick` |
| `menu active` / `menu close` / `menu save` | Lists running sessions; a repeated label shows the session ID as subtitle. | same |
| `jump PROJECT` | Reuses the session labelled exactly `PROJECT`, or creates it from the preset in one all-or-nothing `session.create`. Two sessions with that label is an error. | `{session_id, window_id?}` (`window_id` only for a new session) |
| `kill SESSION_ID` | Destroys that session and every block in it. | nothing |
| `save SESSION_ID` | Saves the session as a preset named after its label (see [Saving a session](#saving-a-session)). Writes nothing when that preset exists. | `{status, name, path}`; `status` is `created`, or `exists` plus an overwrite `menu` |
| `save --overwrite NAME SESSION_ID` | Replaces the existing preset `NAME` in its own file. Refused if the session is no longer labelled `NAME`. | `{status, name, path}`; `status` is `overwritten`, or `created` when no preset exists |
| `delete NAME` | Removes nothing: reports the file holding preset `NAME`, whatever its file is called, with a menu asking whether to delete it. Running sessions are never touched. | `{status, name, path, menu}`; `status` is `confirm` |
| `delete --confirmed NAME` | Removes the file holding preset `NAME`. | `{status, name, path}`; `status` is `deleted` |

`--sessions-dir DIR` (or `REX_SESSIONS_DIR`) picks the preset directory;
the default is `$XDG_CONFIG_HOME/rex/sessions`, else `~/.config/rex/sessions`.
Errors go to stderr as `rex-session-manager: <message>` with exit status 1;
an invalid command line is reported by clap with exit status 2.

## Presets

One JSON file per project, read on every call:

```json
{
  "name": "dotfiles",
  "tabs": [
    { "cwd": "~/dotfiles", "run": "nvim" },
    { "cwd": "~/dotfiles" }
  ]
}
```

- `name` is required and unique across files; the file name is only for humans.
- `tabs` is non-empty. `cwd` is required (a leading `~/` expands to `HOME`);
  `run` is optional and is typed into the tab's interactive fish.
- Unknown keys, a non-object file, and duplicate names are errors that name the
  file. Only `*.json` (any case) is read; hidden files are skipped.

## Saving a session

`save` turns a running session into a preset that `jump` can recreate:

- `name` is the session label; an unlabelled session is refused.
- One tab per window that has a terminal pane, in tab order. A split tab keeps
  only its focused terminal pane; the other panes are not saved.
- `cwd` is the directory the shell last reported (OSC 7), else the foreground
  program's directory, written as `~/…` inside `HOME`.
- `run` is the foreground program, omitted when that is the shell itself.
  Rex reports only the program name, so its arguments come from `/bin/ps`;
  a pane on another host keeps the bare name.
- A new preset goes to `<name>.json` (characters other than letters, digits,
  `-`, `_`, `.` become `-`) and never replaces an existing file. An overwrite
  replaces the preset's current file in one rename, whatever its file name.

The Lua bridge asks before overwriting: `save` prints `status: "exists"` with
a ready `rex.ui.pick` menu (`overwrite` / `cancel`), and choosing `overwrite`
runs `save --overwrite NAME`.

## Layout

| Module | Holds |
| --- | --- |
| `main.rs` | CLI (clap) and dispatch. |
| `preset.rs` | Preset parsing, validation, directory loading, and writing. |
| `menu.rs` | The menus and label sanitization. |
| `session.rs` | `jump` and the `session.create` request. |
| `save.rs` | `save`: reading a session's tabs into a preset. |
| `delete.rs` | `delete`: finding a preset's file and removing it on confirmation. |
| `rex.rs` | `RexApi` and `SessionReader` traits and their `rex api call` implementation. |

`session.rs` depends on the `RexApi` trait, so its tests use an in-memory fake.
`tests/cli.rs` runs the real binary against a fake `rex` (selected with
`REX_BIN`) and checks exit codes, stdout, and the exact `rex api call` lines.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```
