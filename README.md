# rex-session-manager

Project sessions for the [Rex](https://superlogical.com/rex) terminal
multiplexer: open a project from a JSON preset, switch between running
sessions, close one, save a running session as a preset, delete a preset you no
longer need, or keep a short ordered list of marks reached with `q`, `w`, `e`,
`r`.

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
| `menu marks` / `menu unmark` / `menu move` | Lists the marks in position order, each row showing the key reaching it when it has one (see [Marks](#marks)); `menu marks` ends with a `Move…` row once there are 2+ marks and an `Unmark…` row, `menu unmark` removes the chosen mark, and `menu move` chooses the mark to move. | `{title, confirm, items, unmark_id, move_id}` for `menu marks` (`unmark_id` and `move_id` are those rows' ids, never a mark's name); `{title, confirm, items}` for the others |
| `jump PROJECT` | Reuses the session labelled exactly `PROJECT`, or creates it from the preset in one all-or-nothing `session.create`. Two sessions with that label is an error. | `{session_id, window_id?}` (`window_id` only for a new session) |
| `kill SESSION_ID` | Destroys that session and every block in it, and removes every mark holding it. The marks file is read first, so a broken one destroys nothing. | nothing |
| `save SESSION_ID` | Saves the session as a preset named after its label (see [Saving a session](#saving-a-session)). Writes nothing when that preset exists. | `{status, name, path}`; `status` is `created`, or `exists` plus an overwrite `menu` |
| `save --overwrite NAME SESSION_ID` | Replaces the existing preset `NAME` in its own file. Refused if the session is no longer labelled `NAME`. | `{status, name, path}`; `status` is `overwritten`, or `created` when no preset exists |
| `delete NAME` | Removes nothing: reports the file holding preset `NAME`, whatever its file is called, with a menu asking whether to delete it. Running sessions are never touched. | `{status, name, path, menu}`; `status` is `confirm` |
| `delete --confirmed NAME` | Removes the file holding preset `NAME`, and the mark `NAME` if there is one. | `{status, name, path}`; `status` is `deleted` |
| `mark SESSION_ID` | Marks the session, saving it as a preset first when it has none, under the same rules as `save` (see [Marks](#marks)). A marked preset keeps its position. | `{status, name, position, key, saved}`; `status` is `added` or `updated`, `key` (`q`, `w`, `e`, `r`) only for the first four positions, `saved` says whether a preset was written |
| `unmark NAME` | Removes the mark `NAME`; the marks after it move up one position. | `{status, name, position}`; `status` is `removed` |
| `move NAME` | Moves nothing: reports the mark's position and a menu of the positions it can move to (see [Marks](#marks)). | `{status, name, from, menu}`; `status` is `choose` |
| `move --to N NAME` | Moves the mark `NAME` to position `N`; the marks between shift by one. A mark already at `N` leaves the file untouched. | `{status, name, from, to, key}`; `status` is `moved`, `key` (`q`, `w`, `e`, `r`) only when `N` is one of the first four positions |
| `jump-mark --position N` / `jump-mark NAME` | Goes to the mark by its 1-based position (the `q`, `w`, `e`, `r` keys) or by name (the menu). | `{session_id, window_id?}` like `jump` |

`--sessions-dir DIR` (or `REX_SESSIONS_DIR`) picks the preset directory;
the default is `$XDG_CONFIG_HOME/rex/sessions`, else `~/.config/rex/sessions`.
`--marks-dir DIR` (or `REX_MARKS_DIR`) picks the directory holding
`marks.json`; the default is `$XDG_STATE_HOME/rex-session-manager`, else
`~/.local/state/rex-session-manager`.
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
  When another preset already holds the new file name (`web app` and
  `web-app`), the error names it so either label can be renamed.

The Lua bridge asks before overwriting: `save` prints `status: "exists"` with
a ready `rex.ui.pick` menu (`overwrite` / `cancel`), and choosing `overwrite`
runs `save --overwrite NAME`.

## Marks

A short ordered list of presets, each one chord away. One file holds them all,
`marks.json`, in the directory `--marks-dir` picks:

```json
{
  "marks": [
    { "name": "dotfiles", "session_id": "session:…" },
    { "name": "notes" }
  ]
}
```

- A mark's identity is its preset `name`, which is unique; `session_id` is only
  the last session seen for it, left out when there is none.
- A mark's position is its 1-based index in the list, so removing one moves the
  rest up; no stored number can drift. New marks append, with no maximum.
- `q`, `w`, `e`, `r` reach positions 1-4. The later marks are reached from the
  marks menu, which shows each row's key.
- `mark` saves the session as a preset first when it has none, under the same
  rules and with the same errors as `save` (the label is the name), then appends
  the mark. Marking a preset that is already marked updates its session id in
  place and keeps its position; it never duplicates.
- `jump-mark` looks for the mark's recorded session while it still runs and
  still carries the mark's name, else the one session labelled with the name,
  else a new session from the preset — recording the id it found. A mark with
  neither is an error:
  `mark NAME has no running session and no preset; unmark it`.
- `menu marks` ends with an `Unmark…` row whose id is `unmark_id`, never a mark's
  name, so the bridge can tell it from a chosen mark and open `menu unmark`; a
  mark called `unmark` only pushes that id to `unmark+`. With 2+ marks a `Move…`
  row comes just before it, under the same rule for its id `move_id`, and opens
  `menu move`. No marks means no rows.
- `move --to N NAME` removes the mark and reinserts it at position `N`, so the
  marks between shift by one; a mark already at `N` leaves the file untouched.
  `move NAME` writes nothing: it prints the target positions as rows, each
  titled with the position and the key that reaches it. A position past the
  last mark is an error (`no position N (L marked)`), and `N = 0` is a command
  line error.
- The file is strict like a preset: an unknown key, an empty name, a repeated
  name, and a non-object are errors naming the file. A missing file is an empty
  list, never an error, and the list is written in one rename.
- Commands that change the marks hold an exclusive lock on `.marks.lock` beside
  `marks.json` from the read to the write, so concurrent marks are never lost;
  read-only menus take no lock and create no file.
- Known limit: a session closed by Rex itself (`session.close`, `Prefix+Shift+D`)
  never reaches the binary, so its mark stays; `jump-mark` then recreates the
  session from the preset.

## Layout

| Module | Holds |
| --- | --- |
| `main.rs` | CLI (clap) and dispatch. |
| `preset.rs` | Preset parsing, validation, directory loading, and writing. |
| `menu.rs` | The menus and label sanitization. |
| `session.rs` | `jump` and the `session.create` request. |
| `save.rs` | `save`: reading a session's tabs into a preset. |
| `delete.rs` | `delete`: finding a preset's file and removing it on confirmation. |
| `marks.rs` | Marks: the `marks.json` store, `mark`/`unmark`, `move`, and `jump-mark`. |
| `rex.rs` | `RexApi` and `SessionReader` traits and their `rex api call` implementation. |

`session.rs` depends on the `RexApi` trait, so its tests use an in-memory fake.
`tests/cli.rs` runs the real binary against a fake `rex` (selected with
`REX_BIN`) and checks exit codes, stdout, and the exact `rex api call` lines.

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
