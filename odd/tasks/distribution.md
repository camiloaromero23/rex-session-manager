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

## Tasks

- [x] T1 (S1) inline — repository created, `main` pushed; no commit.
- [x] T2 (S2) inline — package metadata and license files. Commit 7442fe1.
- [x] T3 (S3, S5) inline — `init-lua` subcommand, embedded module, tests, docs.
      Commit 3c67e2a.
- [x] T4 (S4, S5) inline — dist release workflow, tap repository, install docs.
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
