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

    /// Replies with `reply` to `METHOD`, or to `METHOD` on `BLOCK` when `target`
    /// is `METHOD@BLOCK`, instead of the default reply.
    fn reply(&self, target: &str, reply: &str) {
        let file = match target.split_once('@') {
            Some((method, block)) => format!("reply.{method}.{block}.json"),
            None => format!("reply.{target}.json"),
        };
        fs::write(self.path(&file), reply).unwrap();
    }

    /// Runs the binary against a fake `rex` that logs its argv and prints the
    /// most specific reply: per method and block, per method, then `reply`.
    fn run(&self, reply: &str, args: &[&str]) -> Output {
        fs::write(self.path("reply.json"), reply).unwrap();
        let rex = self.path("rex");
        let dir = self.dir.path().display();
        fs::write(
            &rex,
            format!(
                concat!(
                    "#!/bin/sh\n",
                    "printf '%s\\n' \"$*\" >> '{dir}/rex.log'\n",
                    "block=$(printf '%s' \"$4\" | sed -n 's/.*\"block_id\":\"\\([^\"]*\\)\".*/\\1/p')\n",
                    "for f in '{dir}'/\"reply.$3.$block.json\" '{dir}'/\"reply.$3.json\" '{dir}/reply.json'; do\n",
                    "  if [ -f \"$f\" ]; then cat \"$f\"; exit 0; fi\n",
                    "done\n",
                ),
                dir = dir
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

    fn sessions_file(&self, file: &str) -> PathBuf {
        self.path("sessions").join(file)
    }

    /// Every entry of the sessions directory, hidden ones included, sorted.
    fn sessions_listing(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(self.path("sessions"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
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

const SHELL_FLAVOR: &str = "com.superlogical.terminal.shell";

/// A session labelled `label`: tab 1 is split (block:a, focused block:b), tab 2
/// holds block:c, and tab 3 holds only a non-terminal block.
fn three_tab_view(label: &str) -> String {
    format!(
        concat!(
            r#"{{"revision":3,"session_id":"session:1","label":"{label}","windows":["#,
            r#"{{"window_id":"window:1","focused_block_id":"block:b","layers":[{{"layer_id":"layer:1","kind":"tiled","blocks":["#,
            r#"{{"block_id":"block:a","flavor":"{flavor}"}},{{"block_id":"block:b","flavor":"{flavor}"}}]}}]}},"#,
            r#"{{"window_id":"window:2","focused_block_id":"block:c","layers":[{{"layer_id":"layer:2","kind":"tiled","blocks":["#,
            r#"{{"block_id":"block:c","flavor":"{flavor}"}}]}}]}},"#,
            r#"{{"window_id":"window:3","focused_block_id":"block:d","layers":[{{"layer_id":"layer:3","kind":"tiled","blocks":["#,
            r#"{{"block_id":"block:d","flavor":"com.example.browser"}}]}}]}}]}}"#
        ),
        label = label,
        flavor = SHELL_FLAVOR,
    )
}

/// Replies for [`three_tab_view`]: block:b runs nvim in `~/code/web app`;
/// block:c is an idle fish whose shell never reported a pwd.
fn reply_three_tabs(s: &Sandbox, label: &str) {
    let home = s.path("home");
    s.reply("session.view", &three_tab_view(label));
    s.reply(
        "com.superlogical.terminal.pwd@block:b",
        &format!(
            r#"{{"pwd":"file://host{}/code/web%20app","owner":{{"pid":1,"name":"fish"}}}}"#,
            home.display()
        ),
    );
    // A PID no process can hold, so the full command line falls back to the name.
    s.reply(
        "com.superlogical.terminal.process@block:b",
        r#"{"foreground":{"pid":999999999,"name":"nvim","argv0":"nvim","cwd":"/elsewhere"}}"#,
    );
    s.reply("com.superlogical.terminal.pwd@block:c", r#"{"pwd":""}"#);
    s.reply(
        "com.superlogical.terminal.process@block:c",
        r#"{"foreground":{"pid":999999998,"name":"fish","argv0":"-fish","cwd":"/tmp"}}"#,
    );
}

const SAVED_WEB_APP: &str = r#"{
  "name": "web app",
  "tabs": [
    {
      "cwd": "~/code/web app",
      "run": "nvim"
    },
    {
      "cwd": "/tmp"
    }
  ]
}
"#;

const ALL_SAVE_CALLS: &str = concat!(
    "api call session.view {\"session_id\":\"session:1\"}\n",
    "api call com.superlogical.terminal.pwd {\"session_id\":\"session:1\",\"block_id\":\"block:b\"}\n",
    "api call com.superlogical.terminal.process {\"session_id\":\"session:1\",\"block_id\":\"block:b\"}\n",
    "api call com.superlogical.terminal.pwd {\"session_id\":\"session:1\",\"block_id\":\"block:c\"}\n",
    "api call com.superlogical.terminal.process {\"session_id\":\"session:1\",\"block_id\":\"block:c\"}\n",
);

#[test]
fn menu_save_lists_running_sessions() {
    let s = Sandbox::new();
    let reply = r#"{"sessions":[{"session_id":"session:1","label":"work"}]}"#;

    let out = s.run(reply, &["menu", "save"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Save Session 💾","confirm":"Save Preset","items":["#,
            r#"{"id":"session:1","title":"work","icon":"code"}]}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "api call session.list {}\n");
}

#[test]
fn save_creates_a_preset_from_the_focused_pane_of_each_terminal_tab() {
    let s = Sandbox::new();
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["save", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let path = s.sessions_file("web-app.json");
    assert_eq!(
        stdout(&out),
        format!(
            "{{\"status\":\"created\",\"name\":\"web app\",\"path\":\"{}\"}}\n",
            path.display()
        )
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), SAVED_WEB_APP);
    assert_eq!(s.calls(), ALL_SAVE_CALLS);
    assert_eq!(s.sessions_listing(), ["web-app.json"]);

    // The saved preset is a valid project for the existing menu.
    let menu = s.run("{}", &["menu", "projects"]);
    assert_eq!(menu.status.code(), Some(0), "{}", stderr(&menu));
    assert!(stdout(&menu).contains(r#"{"id":"web app","title":"web app","icon":"code"}"#));
}

#[test]
fn save_creates_the_sessions_directory_when_missing() {
    let s = Sandbox::new();
    fs::remove_dir(s.path("sessions")).unwrap();
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["save", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        fs::read_to_string(s.sessions_file("web-app.json")).unwrap(),
        SAVED_WEB_APP
    );
}

#[test]
fn save_reports_an_existing_preset_without_writing() {
    let s = Sandbox::new();
    let existing = r#"{"name":"web app","tabs":[{"cwd":"/old"}]}"#;
    s.preset("custom.json", existing);
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["save", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let path = s.sessions_file("custom.json").display().to_string();
    assert_eq!(
        stdout(&out),
        format!(
            concat!(
                r#"{{"status":"exists","name":"web app","path":"{path}","menu":{{"#,
                r#""title":"Overwrite web app?","confirm":"Confirm","items":["#,
                r#"{{"id":"overwrite","title":"Overwrite","subtitle":"{path}","icon":"code"}},"#,
                r#"{{"id":"cancel","title":"Cancel","icon":"code"}}]}}}}"#,
                "\n"
            ),
            path = path
        )
    );
    assert_eq!(
        fs::read_to_string(s.sessions_file("custom.json")).unwrap(),
        existing
    );
    assert_eq!(s.sessions_listing(), ["custom.json"]);
    assert_eq!(
        s.calls(),
        "api call session.view {\"session_id\":\"session:1\"}\n"
    );
}

#[test]
fn save_overwrite_replaces_the_existing_preset_in_place() {
    let s = Sandbox::new();
    s.preset(
        "custom.json",
        r#"{"name":"web app","tabs":[{"cwd":"/old"}]}"#,
    );
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["save", "--overwrite", "web app", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let path = s.sessions_file("custom.json");
    assert_eq!(
        stdout(&out),
        format!(
            "{{\"status\":\"overwritten\",\"name\":\"web app\",\"path\":\"{}\"}}\n",
            path.display()
        )
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), SAVED_WEB_APP);
    assert_eq!(s.sessions_listing(), ["custom.json"]);
    assert_eq!(s.calls(), ALL_SAVE_CALLS);
}

#[test]
fn save_overwrite_without_a_preset_creates_one() {
    let s = Sandbox::new();
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["save", "--overwrite", "web app", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).starts_with(r#"{"status":"created","#),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        fs::read_to_string(s.sessions_file("web-app.json")).unwrap(),
        SAVED_WEB_APP
    );
}

#[test]
fn save_never_clobbers_another_preset_holding_the_file_name() {
    let s = Sandbox::new();
    let other = r#"{"name":"other","tabs":[{"cwd":"/"}]}"#;
    s.preset("web-app.json", other);
    reply_three_tabs(&s, "web app");

    for args in [
        &["save", "--", "session:1"][..],
        &["save", "--overwrite", "web app", "--", "session:1"],
    ] {
        let out = s.run("{}", args);

        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stdout(&out), "");
        assert_eq!(
            stderr(&out),
            format!(
                "rex-session-manager: cannot create {}: it already holds preset \"other\"; rename the session or that preset\n",
                s.sessions_file("web-app.json").display()
            )
        );
        assert_eq!(
            fs::read_to_string(s.sessions_file("web-app.json")).unwrap(),
            other
        );
        assert_eq!(s.sessions_listing(), ["web-app.json"]);
    }
}

#[test]
fn save_overwrite_refuses_a_session_renamed_since_the_confirmation() {
    let s = Sandbox::new();
    let other = r#"{"name":"other","tabs":[{"cwd":"/old"}]}"#;
    s.preset("other.json", other);
    reply_three_tabs(&s, "other");

    let out = s.run("{}", &["save", "--overwrite", "web app", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: session session:1 is now labelled other, not web app; save it again\n"
    );
    assert_eq!(
        fs::read_to_string(s.sessions_file("other.json")).unwrap(),
        other
    );
    assert_eq!(s.sessions_listing(), ["other.json"]);
    assert_eq!(
        s.calls(),
        "api call session.view {\"session_id\":\"session:1\"}\n"
    );
}

#[test]
fn save_refuses_an_unlabelled_session() {
    let s = Sandbox::new();
    reply_three_tabs(&s, "");

    let out = s.run("{}", &["save", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: session session:1 has no label; name it before saving\n"
    );
    assert!(s.sessions_listing().is_empty());
    assert_eq!(
        s.calls(),
        "api call session.view {\"session_id\":\"session:1\"}\n"
    );
}

#[test]
fn save_refuses_a_session_without_terminal_tabs() {
    let s = Sandbox::new();
    s.reply(
        "session.view",
        &format!(
            concat!(
                r#"{{"session_id":"session:1","label":"web","windows":[{{"window_id":"window:1","#,
                r#""layers":[{{"kind":"tiled","blocks":[{{"block_id":"block:d","flavor":"{}"}}]}}]}}]}}"#
            ),
            "com.example.browser"
        ),
    );

    let out = s.run("{}", &["save", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: session web has no terminal tabs to save\n"
    );
    assert!(s.sessions_listing().is_empty());
}

#[test]
fn save_refuses_a_tab_without_a_known_working_directory() {
    let s = Sandbox::new();
    reply_three_tabs(&s, "web app");
    s.reply(
        "com.superlogical.terminal.process@block:c",
        r#"{"foreground":{"pid":999999998,"name":"fish","argv0":"-fish"}}"#,
    );

    let out = s.run("{}", &["save", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "rex-session-manager: tab 2 of session web app reports no working directory\n"
    );
    assert!(s.sessions_listing().is_empty());
}

#[test]
fn save_refuses_when_an_existing_preset_is_broken() {
    let s = Sandbox::new();
    s.preset("bad.json", r#"{"name":""}"#);
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["save", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("bad.json"), "{}", stderr(&out));
    assert_eq!(s.sessions_listing(), ["bad.json"]);
}

#[test]
fn menu_delete_lists_presets_without_calling_rex() {
    let s = Sandbox::new();
    // File order (a, b) is the reverse of name order (alpha, zeta).
    s.preset("a.json", r#"{"name":"zeta","tabs":[{"cwd":"/"}]}"#);
    s.preset("b.JSON", r#"{"name":"alpha","tabs":[{"cwd":"/"}]}"#);
    s.preset(".hidden.json", "not json");
    s.preset("README.md", "not json");

    let out = s.run("{}", &["menu", "delete"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Delete Preset 🗑","confirm":"Delete Preset","items":["#,
            r#"{"id":"alpha","title":"alpha","icon":"code"},"#,
            r#"{"id":"zeta","title":"zeta","icon":"code"}]}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "", "menu delete must not call rex");
}

#[test]
fn delete_asks_before_removing_a_preset() {
    let s = Sandbox::new();
    let dotfiles = r#"{"name":"dotfiles","tabs":[{"cwd":"/d"}]}"#;
    s.preset("dotfiles.json", dotfiles);
    s.preset("notes.json", r#"{"name":"notes","tabs":[{"cwd":"/n"}]}"#);

    let out = s.run("{}", &["delete", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let path = s.sessions_file("dotfiles.json").display().to_string();
    assert_eq!(
        stdout(&out),
        format!(
            concat!(
                r#"{{"status":"confirm","name":"dotfiles","path":"{path}","menu":{{"#,
                r#""title":"Delete dotfiles?","confirm":"Confirm","items":["#,
                r#"{{"id":"delete","title":"Delete","subtitle":"{path}","icon":"code"}},"#,
                r#"{{"id":"cancel","title":"Cancel","icon":"code"}}]}}}}"#,
                "\n"
            ),
            path = path
        )
    );
    assert_eq!(
        fs::read_to_string(s.sessions_file("dotfiles.json")).unwrap(),
        dotfiles
    );
    assert_eq!(s.sessions_listing(), ["dotfiles.json", "notes.json"]);
    assert_eq!(s.calls(), "", "delete must not call rex");
}

#[test]
fn delete_confirmed_removes_only_that_preset() {
    let s = Sandbox::new();
    s.preset(
        "dotfiles.json",
        r#"{"name":"dotfiles","tabs":[{"cwd":"/d"}]}"#,
    );
    let notes = r#"{"name":"notes","tabs":[{"cwd":"/n"}]}"#;
    s.preset("notes.json", notes);

    let out = s.run("{}", &["delete", "--confirmed", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let path = s.sessions_file("dotfiles.json").display().to_string();
    assert_eq!(
        stdout(&out),
        format!("{{\"status\":\"deleted\",\"name\":\"dotfiles\",\"path\":\"{path}\"}}\n")
    );
    assert!(!s.sessions_file("dotfiles.json").exists());
    assert_eq!(
        fs::read_to_string(s.sessions_file("notes.json")).unwrap(),
        notes
    );
    assert_eq!(s.sessions_listing(), ["notes.json"]);
    assert_eq!(s.calls(), "", "delete must not call rex");
}

#[test]
fn delete_finds_a_preset_stored_under_another_file_name() {
    let s = Sandbox::new();
    s.preset(
        "custom.json",
        r#"{"name":"dotfiles","tabs":[{"cwd":"/d"}]}"#,
    );
    let notes = r#"{"name":"notes","tabs":[{"cwd":"/n"}]}"#;
    s.preset("notes.json", notes);

    let ask = s.run("{}", &["delete", "--", "dotfiles"]);

    assert_eq!(ask.status.code(), Some(0), "{}", stderr(&ask));
    let path = s.sessions_file("custom.json").display().to_string();
    let asked = stdout(&ask);
    assert!(asked.contains(&format!(r#""path":"{path}""#)), "{asked}");
    assert!(
        asked.contains(&format!(r#""subtitle":"{path}""#)),
        "{asked}"
    );

    let out = s.run("{}", &["delete", "--confirmed", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!("{{\"status\":\"deleted\",\"name\":\"dotfiles\",\"path\":\"{path}\"}}\n")
    );
    assert_eq!(s.sessions_listing(), ["notes.json"]);
    assert_eq!(
        fs::read_to_string(s.sessions_file("notes.json")).unwrap(),
        notes
    );
    assert_eq!(s.calls(), "", "delete must not call rex");
}

#[test]
fn delete_refuses_an_unknown_project_without_calling_rex() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/"}]}"#);

    for args in [
        &["delete", "--", "nope"][..],
        &["delete", "--confirmed", "--", "nope"],
    ] {
        let out = s.run("{}", args);

        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stdout(&out), "");
        assert_eq!(stderr(&out), "rex-session-manager: unknown project: nope\n");
        assert_eq!(s.sessions_listing(), ["d.json"]);
    }
    assert_eq!(s.calls(), "", "delete must not call rex");
}

#[test]
fn delete_refuses_when_any_preset_is_broken() {
    let s = Sandbox::new();
    s.preset("good.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/"}]}"#);
    s.preset(
        "bad.json",
        r#"{"name":"x","tabs":[{"cwd":"/","command":"nvim"}]}"#,
    );

    for args in [
        &["delete", "--", "dotfiles"][..],
        &["delete", "--confirmed", "--", "dotfiles"],
    ] {
        let out = s.run("{}", args);

        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stdout(&out), "");
        let error = stderr(&out);
        assert!(error.starts_with("rex-session-manager: "), "{error}");
        assert!(error.contains("bad.json"), "{error}");
        assert!(error.contains("unknown field `command`"), "{error}");
        assert_eq!(s.sessions_listing(), ["bad.json", "good.json"]);
    }
    assert_eq!(s.calls(), "", "delete must not call rex");
}

#[test]
fn delete_refuses_duplicate_preset_names() {
    let s = Sandbox::new();
    for file in ["one.json", "two.json"] {
        s.preset(file, r#"{"name":"same","tabs":[{"cwd":"/"}]}"#);
    }

    for args in [
        &["delete", "--", "same"][..],
        &["delete", "--confirmed", "--", "same"],
    ] {
        let out = s.run("{}", args);

        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(stdout(&out), "");
        let error = stderr(&out);
        assert!(
            error.starts_with("rex-session-manager: duplicate project"),
            "{error}"
        );
        assert!(error.contains("one.json"), "{error}");
        assert!(error.contains("two.json"), "{error}");
        assert_eq!(s.sessions_listing(), ["one.json", "two.json"]);
    }
    assert_eq!(s.calls(), "", "delete must not call rex");
}

#[test]
fn delete_treats_a_missing_sessions_directory_as_an_unknown_project() {
    let s = Sandbox::new();
    fs::remove_dir(s.path("sessions")).unwrap();

    let out = s.run("{}", &["delete", "--confirmed", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: unknown project: dotfiles\n"
    );
    assert_eq!(s.calls(), "", "delete must not call rex");
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
