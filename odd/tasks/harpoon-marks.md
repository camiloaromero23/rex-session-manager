# harpoon-marks

Add harpoon-like marks to `rex-session-manager`: a small ordered list of presets
you jump to with `q`, `w`, `e`, `r`, so a session is one chord away instead of a
pick menu. Branch `feature/harpoon-marks`.

## Specs

S1. A mark stores the preset name (its identity, since names are unique) and the
    last session id seen for it. Jumping selects that session when it is still
    running and still carries the mark's name; otherwise the one session
    labelled with the name, else it recreates the session from the preset,
    exactly like `jump`, and records the id it found (L6, L15).
S2. Marks live in one file, `marks.json`, in `$XDG_STATE_HOME/rex-session-manager`,
    else `~/.local/state/rex-session-manager`. `--marks-dir DIR` (`REX_MARKS_DIR`)
    overrides the directory. The file is strict JSON: an unknown key, an empty
    name, or a duplicate name is an error naming the file. A missing file is an
    empty list, never an error.
S3. New marks append; there is no maximum. A position is the mark's 1-based index
    in the file, so removing a mark compacts the rest. `q`, `w`, `e`, `r` address
    positions 1-4; nothing addresses the rest, which are reachable from the menu.
S4. Marking a session that has no preset writes the preset first, under the same
    rules as `save` (the label is the name), then appends the mark. A session
    with no label is refused, as `save` refuses it.
S5. Marking a preset that is already marked updates its session id in place and
    keeps its position; it never duplicates.
S6. `kill SESSION_ID` removes every mark holding that session id, whether or not
    a preset could recreate it; its stdout and exit status are unchanged.
    `delete --confirmed NAME` removes the mark named `NAME`.
S7. A mark with no running session and no preset cannot jump: the error names the
    mark and says to unmark it.
S8. `menu marks` lists marks in position order, each row showing the key that
    reaches it, then one "Unmark…" row whose id (`unmark_id`, never a mark's
    name) opens `menu unmark`; `menu unmark` lists the same rows and removes the
    chosen one. No marks means no rows. `unmark NAME` removes by name;
    `jump-mark --position N` (keys) or `jump-mark NAME` (menu) jumps. No
    separate unmark key (L9).

S9. Moving a mark from the marks menu (L19-L21). With 2+ marks, `menu marks`
    gets a "Move…" row before "Unmark…", whose id `move_id` is never a
    mark's name nor `unmark_id`. `menu move` lists the marks to move.
    `move NAME` writes nothing and prints `{"status":"choose","name","from",
    "menu"}`, a pick of target positions. `move --to N NAME` removes the mark
    and reinserts it at position N, so the marks between shift by one; it
    prints `{"status":"moved","name","from","to","key"?}` and leaves the file
    untouched when N is its current position. N beyond the list is an error;
    N = 0 is a usage error.

## Tasks

| ID | Spec | Route | Commit |
| --- | --- | --- | --- |
| T1 | S1-S3 | inline: `src/marks.rs` store, load/save, append, remove, positions; cargo test 29+48, clippy | a2168b3 |
| T2 | S4, S5, S8 | inline code + README via worker | a2168b3 |
| T3 | S1, S7 | inline: `jump-mark` with preset fallback and id write-back | a2168b3 |
| T4 | S6 | inline: cascade in `kill` / `delete --confirmed` (Risk item 3, own commit); cargo test 29+51, clippy; independent verify running | 4e46148 |
| T6 | S2, S3, S6 | worker: `.marks.lock` held load-to-store; RED 7/8 marks lost → GREEN; cargo test 29+55, clippy, fmt clean | 14e8b9d |
| T7 | S9 | worker: `move`, `menu move`, Move… row, tests, README; cargo test 29+63, clippy, fmt; smoke of the bridge argv | ac682bf |
| T8 | S9 | inline bridge: Move… flow in `marks_menu`, KEYMAPS.md; luac, config check | dotfiles 49acb3a |
| T5 | keys | Lua bridge via worker: `Prefix+Ctrl+H` then `a`/`m` (L17, dotfiles ecd09f3), `Prefix+q/w/e/r`; `rex config check` 56 bindings/0 diagnostics, `luac -p` clean; live key test pending (user) | dotfiles a87ab94 |

## Log

L1 (user): Now I want to work in the harpoon-like feature

L2 (user): 1. Both: id, else preset — 2. One global list file — 3. (custom) Can you
explain this further and ask again? — 4. Auto-save, then mark

L3 (user): I want append list with no fixed amount of sessions but set up only
{q,w,e,r} for 1,2,3,4. Also note that if I delete a session, if it is saved, it
must be deleted from the harpoon list as well

L4 (user): 1. Append, compact on removal — 2. Always remove on kill

L5 (user): read only `~/.config/rex/*.lua` for the keymap audit

L6 (decision, mine): a mark's identity is the preset name, because preset names
are unique and `save` refuses an unlabelled session. The session id is a fast
path, never the identity, so `delete --confirmed` can cascade by name and a
recreated session updates in place instead of duplicating.

L7 (decision, mine): positions are the file's array order, so no stored position
field can drift from the list; compaction is a consequence of removal, not a
separate step.

L8 (decision, mine): the key letter for a position is computed by the binary
(`q/w/e/r`), not by the Lua config, so the menu shows the same mapping the keys
use.

L9 (user): (custom) h+a mark, h+m menu. I do not want unmark since this will be
done in the menu. Correct me if I'm wrong — then, after I flagged that
`Prefix+h` is focus-left: Prefix+h+a/ Prefix+h+m

L10 (user): Lua bridge: Yes, edit and commit in dotfiles

L11 (evidence): free chords per `~/.config/rex/*.lua` audit: `Prefix+q/w/e/r`,
`a`, `m` unbound (`q`/`w` only commented out, `sessions.lua:55-56`);
`Prefix+h` = `pane.focus` left (`init.lua:68-72`). A Lua action gets
`ctx.session_id` only when the call names a session (`client.action` docs);
whether an app key press names the shown session is unproven.

L12 (decision, mine): `kill` reads the marks file before destroying, so a
broken marks file destroys nothing; `delete --confirmed` likewise. Sessions
closed by Rex itself (`session.close`, `Prefix+Shift+D`) bypass the binary and
keep their marks; `jump-mark` then recreates from the preset.

L13 (evidence): slice boundary. a2168b3 is 988 insertions (444 of them tests),
over the ~400-line slice; split by behavior rather than by line count, with the
Risk item 3 cascade alone in 4e46148 (92 lines). Native assess: reviewDue false
(under_budget), independentVerifier true.

L14 (evidence): the bridge passes `--marks-dir` on every call, so the installed
binary had to gain it before any reload; `cargo install --path .` from 4e46148
replaced `~/.cargo/bin/rex-session-manager`, and `menu marks` / `menu projects`
answered against the real presets.

L15 (evidence): independent verify passed S1-S8 and a 13/13 identical
differential against main. Findings: (1) medium, two concurrent `mark` runs kept
one of two marks in 20/20 rounds, both exit 0 — load-modify-store has no lock;
(2) S1's sentence was stricter than L6, now reworded; (3) no test pinned
`delete --confirmed` on a broken marks file. Decision (mine): fix (1) with an
exclusive std `File::lock` on a lock file beside `marks.json`, held from load to
store, and pin (3).

L16 (evidence): T6 committed after `fix: lock the marks list…` on
feature/harpoon-marks; binary reinstalled from it. `cargo fmt --check` had
failed at 4e46148 (my formatting); fixed in the same commit.

L17 (user): Use Prefix+Ctrl+h+a/m instead

L18 (evidence): bindings moved to `PREFIX .. ">ctrl+h>a"` / `">ctrl+h>m"`
(dotfiles ecd09f3); `rex config check` 56 bindings, diagnostics []; `luac -p`
clean; no `Prefix+h` sequence remains, so focus-left no longer shares a prefix.

L19 (user): How can I delete/move an entry from the marks menu?

L20 (mine, proposed): a Move… row next to Unmark…: pick the mark, then pick
its new position from a list showing each position and its key; behind it a
`move NAME --to N` command that shifts the others and holds the marks lock.

L21 (user): Lets try with move...

L22 (evidence): T7 RED was captured against base sources after the edit
landed (worker's own note), 11 failures for the expected reasons; GREEN 29+63.
Binary reinstalled from ac682bf. Branch is now 1701 authored lines in 4
commits, far past the ~400-line slice; each commit is one behavior.

L23 (user): Add the possibility to use Ctrl+m as a keymap as well as m for the
menu

L24 (evidence): `PREFIX .. ">ctrl+h>ctrl+m"` -> marks_menu added beside
`>ctrl+h>m` (dotfiles b60c0c4); config check 57 bindings, diagnostics [];
whether Rex delivers Ctrl+M distinctly from Enter needs a live key press.
