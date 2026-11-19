# distribution

Ship `rex-session-manager` to other people through Homebrew and mise from one
GitHub Release pipeline built with `dist`. Branch `feature/distribution`.

## Specs

S1. "Create the repository using the gh cli": a public GitHub repository
    `camiloaromero23/rex-session-manager` holding `main` (L2).
S2. The package ships under "MIT OR Apache-2.0" with both license texts and the
    repository/homepage metadata Homebrew and dist need (L3).
S3. `rex-session-manager init-lua` prints the Lua bridge with the binary's own
    absolute path written into it, so the bridge works wherever brew, mise, or
    cargo put the binary ("Binary prints it: `init-lua`", L3). `--bin PATH`
    writes PATH instead. The printed module keeps the bridge API (`run`, `jump`,
    `jump_mark`, `SESSIONS_PATH`, `MARKS_PATH`) and adds `setup(PREFIX)`, which
    registers the session manager, mark, and project-jump actions with the
    default keys.
S4. A tag `vX.Y.Z` builds macOS binaries only ("macOS only", L3:
    aarch64-apple-darwin, x86_64-apple-darwin), attaches them to a GitHub
    Release, and publishes a Homebrew formula to
    `camiloaromero23/homebrew-tap`, so "My preferred ways are homebrew or mise"
    (L1) both install the release: `brew install
    camiloaromero23/tap/rex-session-manager` and `mise use -g
    github:camiloaromero23/rex-session-manager`.
S5. The README documents the Homebrew, mise, and cargo installs and the
    `init-lua` setup in place of `cargo install --path .`.
S6. "I do not want to open my login shell. I want to open my configured shell
    (fish)" (L14; supersedes "let rex use its own default shell", L11). The
    shell is a bridge setting (L15): `setup(PREFIX, { shell =
    "/opt/homebrew/bin/fish" })` makes every binary call pass `--shell PATH`,
    and `jump` / `jump-mark` then send it as the `shell` option of each tab's
    `com.superlogical.terminal.shell` block in `session.create`. Without the
    setting, no `shell` is sent and Rex picks its default (as T6's first
    commit). `cwd` and `initial_input` (`"<run>\n"`) are unchanged.

## Tasks

- [x] T1 (S1) inline — repository created, `main` pushed; no commit.
- [x] T2 (S2) inline — package metadata and license files. Commit 7442fe1.
- [x] T3 (S3, S5) inline — `init-lua` subcommand, embedded module, tests, docs.
      Commit 3c67e2a.
- [x] T4 (S4, S5) inline — dist release workflow, tap repository, install docs.
- [ ] T6 (S6) worker — shell from the bridge setting (`--shell`); first
      commit 5fe3cdc dropped the hardcoded fish.
- [ ] T5 (S4) human — `HOMEBREW_TAP_TOKEN` secret, merge, tag `v0.1.0`, verify
      `brew install` and `mise use` from the release.

## Log

L1. "Now I want to look for the best way to distribute my library for it to be
    used by other people. My preferred ways are homebrew or mise"
L2. "Create the repository using the gh cli. Then continue with what is needed"
L3. Answers: license "MIT OR Apache-2.0 (Recommended)"; targets "macOS only
    (Recommended)"; bridge "Binary prints it: `init-lua` (Recommended)".
L4. T1 evidence: https://github.com/camiloaromero23/rex-session-manager,
    PUBLIC, default branch `main`. Tracked files scanned for secrets first: only
    `/Users/me` test fixtures.
L5. Decision: `init-lua` writes `std::env::current_exe()` unresolved. Through
    Homebrew that is the stable `$(brew --prefix)/bin` link; mise runs the binary
    from a versioned install directory, so mise users re-run `init-lua` after an
    upgrade or pass `--bin`.
L6. T3 evidence: RED (4 `init_lua_*` CLI tests failed, unknown subcommand exit
    2), then GREEN: 98 tests, clippy and fmt clean. Generated module passed
    `luac -p` and a stubbed-`rex` load: 5 actions, 12 binds, `jump_project`
    ran `<bin> --sessions-dir <cfg>/sessions --marks-dir … jump -- nvim` and
    queued `session.select`; a failing exec raised "rex-session-manager
    exited 1: no preset named x". Not loaded in a live Rex yet.
L7. Slice boundary: T2+T3 reached 851 authored lines (the Lua module is ~330);
    T4 is its own slice.
L8. T4 evidence: `dist init` (0.34.0) ignored the `-t` flags, so targets were
    set by hand in `dist-workspace.toml`, with `tap`, `publish-jobs =
    ["homebrew"]`, `install-updater = false`. `dist plan` lists only the two
    macOS archives + formula; a local `dist build` for aarch64 produced the
    archive (binary, both licenses, README; `--version` 0.1.0) and a formula
    with `license any_of: ["MIT", "Apache-2.0"]`. Tap repository
    https://github.com/camiloaromero23/homebrew-tap created (public).
L9. T4 independent verify (gentle-ai-verify): S4 PASS, S5 PASS. `dist generate
    --check` exit 0 and a full regenerate is byte-identical to the committed
    workflow; tag glob matches `v0.1.0`; matrix is macos-15 / macos-15-intel
    only. Advisories, not applied: workflow-wide `contents: write` with
    `pull_request` (stock dist template, PRs run only `plan`); README does not
    say brew/mise work only from the first release; `dist` is not a declared
    project tool.
L10. Native review lineage `review-f0510c3d30fb1727` (commit 1894ec4, lenses
    risk/resilience/readability) is waiting on collect: no model is configured
    for `review-risk`, `review-resilience`, `review-readability` in the Pi
    routing config (user action). Delivery chosen: "Fast-forward main and
    push", held until the review resolves.
L11. Found before release: `src/session.rs:18` hardcodes
    `/opt/homebrew/bin/fish --login --interactive` for every tab, which breaks
    for anyone without fish there. User: "let rex use its own default shell".
L12. T6 evidence (S6). API, read-only: `rex api describe session.create` shows
    a block with only `flavor` (no `options`) in its request example, and
    `rex api call block_creator.list {}` gives the
    `com.superlogical.terminal.shell` options schema: `shell` "Empty uses the
    user's default shell", `command` "A program and its arguments, to run
    instead of an interactive shell", `login` "Left out, it is true". So
    omitting `command` and `shell` starts Rex's own default shell as a login
    shell. No mutating API was called. RED: after the tests asked for the new
    shape, `builds_one_window_per_tab_that_leaves_the_shell_to_rex` failed
    (left carried `command`/`shell`) and the two CLI tests (`jump`, `jump-mark`)
    failed on the full `session.create` argument. GREEN: 31 + 67 tests pass,
    `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean.
    `TerminalOptions` no longer has `command`/`shell`; `cwd` and
    `initial_input` are unchanged. `src/save.rs` SHELLS detection untouched.
L13. T6 parent check: 98 tests pass on 4e6528d. Live smoke against the running
    Rex: `jump` on a temp preset `{"cwd":"/tmp","run":"echo RSM_SMOKE_OK $0
    $PWD"}` created the session; `rex capture` showed `RSM_SMOKE_OK -zsh /tmp`
    (the user's login shell, UserShell /bin/zsh, started as a login shell, in
    the preset cwd, with `run` typed and executed); `kill` removed it, 0
    sessions left. Consequence: the author's own tabs now open zsh, not fish,
    unless the login shell changes.
L14. User correction after T6: "I do not want to open my login shell. I want to
    open my configured shell (fish)". Findings: macOS login shell is /bin/zsh;
    `$SHELL` in the user's terminals is /opt/homebrew/bin/fish; the Rex app
    preference `Workspace.terminalStartupCommand` is
    `/opt/homebrew/bin/fish --login --interactive` (~/.config/rex/preferences.plist)
    and `/opt/homebrew/bin/fish -li` (`defaults read com.superlogical.rex`).
    Server-side `session.create` ignores that client preference (smoke showed
    -zsh). The `session.create` terminal options accept `shell` ("The shell to
    start"). T6 reopened pending the user's choice of where the shell comes from.
L15. Decision: "Bridge setting (Recommended)" — the shell comes from
    `setup(PREFIX, { shell = ... })`, passed as `--shell`, sent as Rex's
    `shell` option; unset leaves Rex's default.
