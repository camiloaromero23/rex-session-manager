# rex-session-manager

Project sessions for the [Rex](https://superlogical.com/rex) terminal
multiplexer: open a project from a JSON preset, switch between running
sessions, or close one.

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
| `menu projects` | Lists every preset, sorted by name. | `{title, confirm, items}` for `rex.ui.pick` |
| `menu active` / `menu close` | Lists running sessions; a repeated label shows the session ID as subtitle. | same |
| `jump PROJECT` | Reuses the session labelled exactly `PROJECT`, or creates it from the preset in one all-or-nothing `session.create`. Two sessions with that label is an error. | `{session_id, window_id?}` (`window_id` only for a new session) |
| `kill SESSION_ID` | Destroys that session and every block in it. | nothing |

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

## Layout

| Module | Holds |
| --- | --- |
| `main.rs` | CLI (clap) and dispatch. |
| `preset.rs` | Preset parsing, validation, and directory loading. |
| `menu.rs` | The three menus and label sanitization. |
| `session.rs` | `jump` and the `session.create` request. |
| `rex.rs` | `RexApi` trait and its `rex api call` implementation. |

`session.rs` depends on the `RexApi` trait, so its tests use an in-memory fake.
`tests/cli.rs` runs the real binary against a fake `rex` (selected with
`REX_BIN`) and checks exit codes, stdout, and the exact `rex api call` lines.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```
