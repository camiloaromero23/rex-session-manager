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
- [ ] T2 (S2) inline — package metadata and license files.
- [ ] T3 (S3, S5) inline — `init-lua` subcommand, embedded module, tests, docs.
- [ ] T4 (S4, S5) inline — dist release workflow, tap repository, install docs.
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
