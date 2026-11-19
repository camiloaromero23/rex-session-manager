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
- [x] T6 (S6) worker — shell from the bridge setting (`--shell`); first
      commit 5fe3cdc dropped the hardcoded fish, bc287c1 added the setting.
- [x] T5 (S4) human + parent — secret set by the user; `main` fast-forwarded to
      e353b23 and pushed, tag `v0.1.0` (2496e0b), release run 44434476992.

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
L16. T6 done (S6), commit bc287c1. `--shell SHELL` / `REX_SHELL` is a clap
    global option threaded through `session::jump`, `session::create`, and
    `marks::jump` into `TerminalOptions.shell`, which is left out when unset.
    The bridge has `M.SHELL` (nil) and `setup(PREFIX, options)`: a non-empty
    string sets it, a non-table options or a non-string/empty shell fails with
    a clear message and leaves `M.SHELL` unset, and `M.run` appends `--shell`
    before the subcommand. RED: 4 CLI tests (option exit 2, `REX_SHELL`
    ignored, `jump-mark`, no `--shell <SHELL>` in `--help`) and the `init-lua`
    test (no `M.SHELL`). GREEN: 34 unit + 71 CLI tests, clippy and fmt clean.
    Generated module: `luac -p` ok; a stubbed-`rex` `lua` script in a
    `mktemp -d` scratch dir showed `… --shell /opt/homebrew/bin/fish jump --
    dotfiles` with the setting and no `--shell` without it, `shell = 1` and
    `""` failing, and a non-table options failing. Live Rex not re-checked.
L17. T6 parent check on 8ed8e5f: 34 unit + 71 CLI tests pass. Live smoke with
    `--shell /opt/homebrew/bin/fish jump` on a temp preset: `rex capture`
    showed `RSM_FISH=4.9.3 login=yes` (fish, login, cwd /tmp, `run` executed);
    session killed, 0 left. Without `--shell` the earlier smoke (L13) opened
    -zsh. Worker scratch dirs removed by the parent.
L18. User: "Update the changes to the binary and rex config. Also i already set
    the models for review and have available the secret with github cli".
    Binary reinstalled with `cargo install --path .`; ~/.config/rex (dotfiles,
    uncommitted) switched to the generated bridge with
    `setup(PREFIX, { shell = "/opt/homebrew/bin/fish" })`, `jump_project` moved
    out of sessions.lua, session_management.lua deleted; `rex config check` 57
    bindings / 5 actions before and after; reloaded, `rex keymap` serves the
    keys from rex_session_manager.lua. Native review still blocked: this Pi
    session reports no `review-risk` model (session profile binding).
L19. T5 evidence: run 44434476992 all 7 jobs success (annotation: formula has no
    `test do`). Independent verify (S4, S5 PASS): release v0.1.0 Latest, not
    draft, only the two darwin archives; aarch64 sha256 OK; binary 0.1.0 with
    `--shell` and `init-lua`; tap Formula/rex-session-manager.rb version 0.1.0,
    both sha256 match, license any_of MIT/Apache-2.0. Parent install checks:
    `brew install camiloaromero23/tap/rex-session-manager` → 0.1.0, init-lua
    writes /opt/homebrew/bin/rex-session-manager (then uninstalled);
    `mise x github:camiloaromero23/rex-session-manager@0.1.0` → 0.1.0 (then
    uninstalled). Advisories: mise's default minimum_release_age hides a
    brand-new release for a while; Homebrew 7 refuses `brew info` on the
    untrusted tap until `brew trust camiloaromero23/tap` (documented in the
    README); binaries are ad-hoc signed, not notarized.
L20. Native review: lineage review-f0510c3d30fb1727 ran its 3 reviewers after
    the session adopted the saved "Claude" profile, but its frozen candidate
    (up to 1894ec4) no longer matched 3c67e2a..HEAD (binding drift, no
    closure). User chose "Start a fresh review": lineage review-1cf1f45f87165d2b
    over 3c67e2a..e5bc8bc (11 files, tier high), lenses risk, resilience,
    readability, reliability on claude-sonnet-5-5. The first reliability result
    was refused at admission (a finding without a proof reference); the
    reoffered slot re-ran once and closed APPROVED; acknowledgement burned the
    authority. Advisory, non-blocking follow-ups: R2-bridge-shell-doc (shell
    documented three times in src/bridge.lua:28-33), R2-shell-duplication
    (src/session.rs:274-300 test helper triplication), R3-bridge-shell-untested
    (WARNING: the Lua `--shell` forwarding and `setup` validation are only
    checked by substring tests, tests/cli.rs:1913-1921; no committed test runs
    the module).
