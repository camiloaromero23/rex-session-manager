//! CLI-level tests: argv, exit codes, stdout, and the exact `rex api call`
//! invocations, against a fake `rex` selected through `REX_BIN`.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use tempfile::TempDir;

struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sessions")).unwrap();
        fs::create_dir(dir.path().join("home")).unwrap();
        fs::write(dir.path().join("rex.log"), "").unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn preset(&self, file: &str, json: &str) {
        fs::write(self.path("sessions").join(file), json).unwrap();
    }

    /// Runs the binary against a fake `rex` that logs its argv and prints `reply`.
    fn run(&self, reply: &str, args: &[&str]) -> Output {
        fs::write(self.path("reply.json"), reply).unwrap();
        let rex = self.path("rex");
        fs::write(
            &rex,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncat '{}'\n",
                self.path("rex.log").display(),
                self.path("reply.json").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&rex, fs::Permissions::from_mode(0o755)).unwrap();

        Command::new(env!("CARGO_BIN_EXE_rex-session-manager"))
            .arg("--sessions-dir")
            .arg(self.path("sessions"))
            .args(args)
            .env("REX_BIN", &rex)
            .env("HOME", self.path("home"))
            .output()
            .unwrap()
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.path("rex.log")).unwrap()
    }
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}

fn stderr(output: &Output) -> &str {
    std::str::from_utf8(&output.stderr).unwrap()
}

#[test]
fn menu_projects_sorts_and_ignores_other_files() {
    let s = Sandbox::new();
    // File order (a, b) is the reverse of name order (alpha, zeta).
    s.preset("a.json", r#"{"name":"zeta","tabs":[{"cwd":"/"}]}"#);
    s.preset("b.JSON", r#"{"name":"alpha","tabs":[{"cwd":"/"}]}"#);
    s.preset(".hidden.json", "not json");
    s.preset("README.md", "not json");

    let out = s.run("{}", &["menu", "projects"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Select Session","confirm":"Open Session","items":["#,
            r#"{"id":"alpha","title":"alpha","icon":"code"},"#,
            r#"{"id":"zeta","title":"zeta","icon":"code"}]}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "", "menu projects must not call rex");
}

#[test]
fn broken_preset_exits_one_naming_the_file() {
    let s = Sandbox::new();
    s.preset(
        "bad.json",
        r#"{"name":"x","tabs":[{"cwd":"/","command":"nvim"}]}"#,
    );

    let out = s.run("{}", &["menu", "projects"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    let error = stderr(&out);
    assert!(error.starts_with("rex-session-manager: "), "{error}");
    assert!(error.contains("bad.json"), "{error}");
    assert!(error.contains("unknown field `command`"), "{error}");
}

#[test]
fn menu_active_marks_duplicates_and_unlabelled_sessions() {
    let s = Sandbox::new();
    let reply = r#"{"sessions":[
        {"session_id":"session:1","label":"work"},
        {"session_id":"session:2","label":"work"},
        {"session_id":"session:3","label":""}]}"#;

    let out = s.run(reply, &["menu", "active"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Active Sessions 🟢","confirm":"Switch Session","items":["#,
            r#"{"id":"session:1","title":"work","subtitle":"session:1","icon":"code"},"#,
            r#"{"id":"session:2","title":"work","subtitle":"session:2","icon":"code"},"#,
            r#"{"id":"session:3","title":"(unlabelled)","icon":"code"}]}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "api call session.list {}\n");
}

#[test]
fn jump_creates_a_missing_session_from_the_preset() {
    let s = Sandbox::new();
    s.preset(
        "d.json",
        r#"{"name":"dotfiles","tabs":[{"cwd":"~/dotfiles","run":"nvim"},{"cwd":"/tmp/x y"}]}"#,
    );
    // Parses as an empty session list too, since `sessions` defaults to empty.
    let reply = r#"{"session_id":"session:new","initial_windows":[{"window_id":"window:first"}]}"#;

    let out = s.run(reply, &["jump", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"session_id\":\"session:new\",\"window_id\":\"window:first\"}\n"
    );
    let shell = r#""command":["/opt/homebrew/bin/fish","--login","--interactive"],"shell":"none""#;
    let home = s.path("home");
    let create = format!(
        concat!(
            r#"api call session.create {{"label":"dotfiles","initial_windows":["#,
            r#"{{"layout":{{"block":{{"flavor":"com.superlogical.terminal.shell","options":{{"#,
            r#""cwd":"{home}/dotfiles",{shell},"initial_input":"nvim\n"}}}}}}}},"#,
            r#"{{"layout":{{"block":{{"flavor":"com.superlogical.terminal.shell","options":{{"#,
            r#""cwd":"/tmp/x y",{shell}}}}}}}}}]}}"#
        ),
        home = Path::new(&home).display(),
        shell = shell,
    );
    assert_eq!(s.calls(), format!("api call session.list {{}}\n{create}\n"));
}

#[test]
fn jump_reuses_the_exact_label_without_a_window() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/"}]}"#);
    let reply = r#"{"sessions":[
        {"session_id":"session:1","label":"Dotfiles"},
        {"session_id":"session:2","label":"dotfiles"}]}"#;

    let out = s.run(reply, &["jump", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "{\"session_id\":\"session:2\"}\n");
    assert_eq!(s.calls(), "api call session.list {}\n");
}

#[test]
fn jump_refuses_an_ambiguous_label() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/"}]}"#);
    let reply = r#"{"sessions":[
        {"session_id":"session:1","label":"dotfiles"},
        {"session_id":"session:2","label":"dotfiles"}]}"#;

    let out = s.run(reply, &["jump", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: more than one session is labelled dotfiles\n"
    );
    assert_eq!(s.calls(), "api call session.list {}\n");
}

#[test]
fn jump_refuses_an_unknown_project_without_calling_rex() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/"}]}"#);

    let out = s.run("{}", &["jump", "--", "nope"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stderr(&out), "rex-session-manager: unknown project: nope\n");
    assert_eq!(s.calls(), "");
}

#[test]
fn kill_destroys_only_the_chosen_id() {
    let s = Sandbox::new();

    let out = s.run("{}", &["kill", "--", "--weird id"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        s.calls(),
        "api call session.destroy {\"session_id\":\"--weird id\"}\n"
    );
}

#[test]
fn menu_close_names_the_committing_row() {
    let s = Sandbox::new();
    let reply = r#"{"sessions":[{"session_id":"session:1","label":"work"}]}"#;

    let out = s.run(reply, &["menu", "close"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Kill Session 💀","confirm":"Close Session","items":["#,
            r#"{"id":"session:1","title":"work","icon":"code"}]}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "api call session.list {}\n");
}

#[test]
fn invalid_command_line_exits_two() {
    let s = Sandbox::new();

    let out = s.run("{}", &["bogus"]);

    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    assert!(
        stderr(&out).contains("unrecognized subcommand"),
        "{}",
        stderr(&out)
    );
    assert_eq!(s.calls(), "");
}

#[test]
fn rex_failure_is_reported_with_one_prefix() {
    let s = Sandbox::new();
    let rex = s.path("failing-rex");
    fs::write(&rex, "#!/bin/sh\necho 'boom: nope' >&2\nexit 7\n").unwrap();
    fs::set_permissions(&rex, fs::Permissions::from_mode(0o755)).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rex-session-manager"))
        .args(["kill", "--", "session:1"])
        .env("REX_BIN", &rex)
        .env("HOME", s.path("home"))
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: session.destroy failed (exit status: 7): boom: nope\n"
    );
}
