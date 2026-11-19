//! CLI-level tests: argv, exit codes, stdout, and the exact `rex api call`
//! invocations, against a fake `rex` selected through `REX_BIN`.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
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
            .arg("--marks-dir")
            .arg(self.path("state"))
            .args(args)
            .env("REX_BIN", &rex)
            .env("HOME", self.path("home"))
            .output()
            .unwrap()
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.path("rex.log")).unwrap()
    }

    fn marks_file(&self) -> PathBuf {
        self.path("state").join("marks.json")
    }

    /// Writes `marks` as the marks file: `(name, session_id)` pairs in order.
    fn marks(&self, marks: &[(&str, Option<&str>)]) {
        fs::create_dir_all(self.path("state")).unwrap();
        fs::write(self.marks_file(), marks_json(marks)).unwrap();
    }

    /// The marks file as `(name, session_id)` pairs; `None` when it is absent.
    fn read_marks(&self) -> Option<Vec<(String, Option<String>)>> {
        let text = fs::read_to_string(self.marks_file()).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        Some(
            value["marks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|mark| {
                    (
                        mark["name"].as_str().unwrap().to_owned(),
                        mark["session_id"].as_str().map(str::to_owned),
                    )
                })
                .collect(),
        )
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

fn marks_json(marks: &[(&str, Option<&str>)]) -> String {
    let marks: Vec<_> = marks
        .iter()
        .map(|(name, id)| match id {
            Some(id) => serde_json::json!({"name": name, "session_id": id}),
            None => serde_json::json!({"name": name}),
        })
        .collect();
    serde_json::json!({ "marks": marks }).to_string()
}

fn owned(marks: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    marks
        .iter()
        .map(|(name, id)| ((*name).to_owned(), id.map(str::to_owned)))
        .collect()
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
    let home = s.path("home");
    let create = format!(
        concat!(
            r#"api call session.create {{"label":"dotfiles","initial_windows":["#,
            r#"{{"layout":{{"block":{{"flavor":"com.superlogical.terminal.shell","options":{{"#,
            r#""cwd":"{home}/dotfiles","initial_input":"nvim\n"}}}}}}}},"#,
            r#"{{"layout":{{"block":{{"flavor":"com.superlogical.terminal.shell","options":{{"#,
            r#""cwd":"/tmp/x y"}}}}}}}}]}}"#
        ),
        home = Path::new(&home).display(),
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
    assert_eq!(s.read_marks(), None, "kill must not create a marks file");
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
    assert_eq!(s.read_marks(), None, "delete must not create a marks file");
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
        .env("REX_MARKS_DIR", s.path("state"))
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: session.destroy failed (exit status: 7): boom: nope\n"
    );
}

// ---- marks ----

const WEB_APP_PRESET: &str = r#"{"name":"web app","tabs":[{"cwd":"/w"}]}"#;

const VIEW_CALL: &str = "api call session.view {\"session_id\":\"session:1\"}\n";

#[test]
fn mark_saves_a_session_without_a_preset_and_appends_it() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/d"}]}"#);
    s.marks(&[("dotfiles", Some("session:0"))]);
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["mark", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"status\":\"added\",\"name\":\"web app\",\"position\":2,\"key\":\"w\",\"saved\":true}\n"
    );
    assert_eq!(
        fs::read_to_string(s.sessions_file("web-app.json")).unwrap(),
        SAVED_WEB_APP
    );
    assert_eq!(
        s.read_marks(),
        Some(owned(&[
            ("dotfiles", Some("session:0")),
            ("web app", Some("session:1"))
        ]))
    );
    assert_eq!(s.calls(), ALL_SAVE_CALLS);
}

#[test]
fn mark_creates_the_marks_file_for_a_session_with_a_preset() {
    let s = Sandbox::new();
    s.preset("w.json", WEB_APP_PRESET);
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["mark", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"status\":\"added\",\"name\":\"web app\",\"position\":1,\"key\":\"q\",\"saved\":false}\n"
    );
    assert_eq!(
        fs::read_to_string(s.marks_file()).unwrap(),
        "{\n  \"marks\": [\n    {\n      \"name\": \"web app\",\n      \"session_id\": \"session:1\"\n    }\n  ]\n}\n"
    );
    assert_eq!(
        fs::read_to_string(s.sessions_file("w.json")).unwrap(),
        WEB_APP_PRESET
    );
    assert_eq!(s.sessions_listing(), ["w.json"]);
    assert_eq!(s.calls(), VIEW_CALL);
}

#[test]
fn mark_of_a_marked_preset_updates_its_session_in_place() {
    let s = Sandbox::new();
    s.preset("w.json", WEB_APP_PRESET);
    s.marks(&[("web app", Some("session:old")), ("notes", None)]);
    reply_three_tabs(&s, "web app");

    let out = s.run("{}", &["mark", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"status\":\"updated\",\"name\":\"web app\",\"position\":1,\"key\":\"q\",\"saved\":false}\n"
    );
    assert_eq!(
        s.read_marks(),
        Some(owned(&[("web app", Some("session:1")), ("notes", None)]))
    );
}

#[test]
fn mark_refuses_an_unlabelled_session_and_writes_nothing() {
    let s = Sandbox::new();
    reply_three_tabs(&s, "");

    let out = s.run("{}", &["mark", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: session session:1 has no label; name it before saving\n"
    );
    assert_eq!(s.read_marks(), None);
    assert!(s.sessions_listing().is_empty());
}

#[test]
fn a_broken_marks_file_is_an_error_naming_it_and_nothing_runs() {
    let cases = [
        (
            r#"{"marks":[{"name":"a","slot":1}]}"#,
            "unknown field `slot`",
        ),
        (
            r#"{"marks":[{"name":"a"},{"name":"a"}]}"#,
            r#"duplicate mark "a""#,
        ),
        (
            r#"{"marks":[{"name":""}]}"#,
            r#"needs a non-empty string "name""#,
        ),
        ("[]", "must contain a JSON object"),
    ];
    for (text, expected) in cases {
        let s = Sandbox::new();
        fs::create_dir_all(s.path("state")).unwrap();
        fs::write(s.marks_file(), text).unwrap();
        reply_three_tabs(&s, "web app");

        let out = s.run("{}", &["mark", "--", "session:1"]);

        assert_eq!(out.status.code(), Some(1), "{text}");
        assert_eq!(stdout(&out), "", "{text}");
        let error = stderr(&out);
        assert!(error.starts_with("rex-session-manager: "), "{error}");
        assert!(
            error.contains(&s.marks_file().display().to_string()),
            "{error}"
        );
        assert!(error.contains(expected), "{error}");
        assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), text);
        assert!(s.sessions_listing().is_empty(), "{text}");
        assert_eq!(s.calls(), "", "{text}");
    }
}

#[test]
fn marks_default_to_the_xdg_state_directory_then_home() {
    let s = Sandbox::new();
    // Creates the fake `rex`.
    s.run("{}", &["menu", "marks"]);
    let run = |xdg: Option<PathBuf>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rex-session-manager"));
        command
            .args(["menu", "marks"])
            .env("REX_BIN", s.path("rex"))
            .env("HOME", s.path("home"))
            .env_remove("REX_MARKS_DIR")
            .env_remove("XDG_STATE_HOME");
        if let Some(xdg) = xdg {
            command.env("XDG_STATE_HOME", xdg);
        }
        command.output().unwrap()
    };

    for (xdg, file) in [
        (
            Some(s.path("xdg")),
            s.path("xdg/rex-session-manager/marks.json"),
        ),
        (
            None,
            s.path("home/.local/state/rex-session-manager/marks.json"),
        ),
    ] {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, marks_json(&[("web app", None)])).unwrap();

        let out = run(xdg);

        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            stdout(&out).contains(r#"{"id":"web app","#),
            "{}",
            stdout(&out)
        );
        fs::remove_file(&file).unwrap();
    }
}

#[test]
fn menu_marks_lists_marks_with_their_keys_and_an_unmark_row() {
    let s = Sandbox::new();
    s.marks(&[
        ("a", Some("session:1")),
        ("b", None),
        ("c", None),
        ("d", None),
        ("e\tf", None),
    ]);

    let out = s.run("{}", &["menu", "marks"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Marks ⚓","confirm":"Jump","items":["#,
            r#"{"id":"a","title":"a","subtitle":"q","icon":"tag"},"#,
            r#"{"id":"b","title":"b","subtitle":"w","icon":"tag"},"#,
            r#"{"id":"c","title":"c","subtitle":"e","icon":"tag"},"#,
            r#"{"id":"d","title":"d","subtitle":"r","icon":"tag"},"#,
            r#"{"id":"e\tf","title":"e?f","icon":"tag"},"#,
            r#"{"id":"move","title":"Move…","icon":"branch"},"#,
            r#"{"id":"unmark","title":"Unmark…","icon":"package"}],"#,
            r#""unmark_id":"unmark","move_id":"move"}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "", "menu marks must not call rex");
}

#[test]
fn menu_marks_never_gives_the_unmark_row_a_marks_id() {
    let s = Sandbox::new();
    s.marks(&[("unmark", None), ("unmark+", None)]);

    let out = s.run("{}", &["menu", "marks"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).ends_with(concat!(
            r#"{"id":"move","title":"Move…","icon":"branch"},"#,
            r#"{"id":"unmark++","title":"Unmark…","icon":"package"}],"#,
            r#""unmark_id":"unmark++","move_id":"move"}"#,
            "\n"
        )),
        "{}",
        stdout(&out)
    );
}

#[test]
fn menu_marks_is_empty_without_a_marks_file() {
    let s = Sandbox::new();

    let out = s.run("{}", &["menu", "marks"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"title\":\"Marks ⚓\",\"confirm\":\"Jump\",\"items\":[],\"unmark_id\":\"unmark\",\"move_id\":\"move\"}\n"
    );
    assert_eq!(s.read_marks(), None);
}

#[test]
fn menu_unmark_lists_the_marks_to_remove() {
    let s = Sandbox::new();
    s.marks(&[("a", Some("session:1")), ("b", None)]);

    let out = s.run("{}", &["menu", "unmark"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Unmark ⚓","confirm":"Unmark","items":["#,
            r#"{"id":"a","title":"a","subtitle":"q","icon":"tag"},"#,
            r#"{"id":"b","title":"b","subtitle":"w","icon":"tag"}]}"#,
            "\n"
        )
    );
}

#[test]
fn unmark_removes_a_mark_and_compacts_the_positions() {
    let s = Sandbox::new();
    s.marks(&[
        ("a", Some("session:1")),
        ("b", Some("session:2")),
        ("c", None),
    ]);

    let out = s.run("{}", &["unmark", "--", "b"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"status\":\"removed\",\"name\":\"b\",\"position\":2}\n"
    );
    assert_eq!(
        s.read_marks(),
        Some(owned(&[("a", Some("session:1")), ("c", None)]))
    );
    assert_eq!(s.calls(), "", "unmark must not call rex");

    // `c` moved from the third position to the second.
    let menu = s.run("{}", &["menu", "unmark"]);
    assert!(stdout(&menu).contains(r#"{"id":"c","title":"c","subtitle":"w","icon":"tag"}"#));
}

#[test]
fn unmark_refuses_an_unknown_mark() {
    let s = Sandbox::new();
    s.marks(&[("a", None)]);
    let before = fs::read_to_string(s.marks_file()).unwrap();

    let out = s.run("{}", &["unmark", "--", "z"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "rex-session-manager: no mark named z\n");
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);
}

#[test]
fn jump_mark_selects_the_marked_session_while_it_runs() {
    let s = Sandbox::new();
    s.marks(&[("a", Some("session:1"))]);
    let before = fs::read_to_string(s.marks_file()).unwrap();
    let reply = r#"{"sessions":[{"session_id":"session:1","label":"a"}]}"#;

    let out = s.run(reply, &["jump-mark", "--position", "1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "{\"session_id\":\"session:1\"}\n");
    assert_eq!(s.calls(), "api call session.list {}\n");
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);
}

#[test]
fn jump_mark_recreates_a_closed_session_from_its_preset_and_records_it() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/d"}]}"#);
    s.marks(&[("notes", None), ("dotfiles", Some("session:gone"))]);
    let reply = r#"{"session_id":"session:new","initial_windows":[{"window_id":"window:first"}]}"#;

    let out = s.run(reply, &["jump-mark", "--position", "2"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"session_id\":\"session:new\",\"window_id\":\"window:first\"}\n"
    );
    assert_eq!(
        s.read_marks(),
        Some(owned(&[("notes", None), ("dotfiles", Some("session:new"))]))
    );
    assert_eq!(
        s.calls(),
        concat!(
            "api call session.list {}\n",
            r#"api call session.create {"label":"dotfiles","initial_windows":["#,
            r#"{"layout":{"block":{"flavor":"com.superlogical.terminal.shell","options":{"#,
            r#""cwd":"/d"}}}}]}"#,
            "\n"
        )
    );
}

#[test]
fn jump_mark_by_name_follows_the_label_when_the_marked_session_was_relabelled() {
    let s = Sandbox::new();
    s.marks(&[("a", Some("session:1"))]);
    let reply = r#"{"sessions":[
        {"session_id":"session:1","label":"other"},
        {"session_id":"session:2","label":"a"}]}"#;

    let out = s.run(reply, &["jump-mark", "--", "a"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "{\"session_id\":\"session:2\"}\n");
    assert_eq!(s.read_marks(), Some(owned(&[("a", Some("session:2"))])));
    assert_eq!(s.calls(), "api call session.list {}\n");
}

#[test]
fn jump_mark_without_a_session_or_preset_says_to_unmark_it() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/d"}]}"#);
    s.marks(&[("ghost", Some("session:9"))]);
    let before = fs::read_to_string(s.marks_file()).unwrap();

    let out = s.run("{}", &["jump-mark", "--position", "1"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: mark ghost has no running session and no preset; unmark it\n"
    );
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);
    assert_eq!(s.calls(), "api call session.list {}\n");
}

#[test]
fn jump_mark_refuses_a_position_with_no_mark() {
    let s = Sandbox::new();
    s.marks(&[("a", None)]);

    let out = s.run("{}", &["jump-mark", "--position", "3"]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: no mark at position 3 (1 marked)\n"
    );
    assert_eq!(s.calls(), "");

    let zero = s.run("{}", &["jump-mark", "--position", "0"]);
    assert_eq!(zero.status.code(), Some(2));
    assert_eq!(stdout(&zero), "");

    let unknown = s.run("{}", &["jump-mark", "--", "z"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(stderr(&unknown), "rex-session-manager: no mark named z\n");
}

#[test]
fn kill_removes_every_mark_of_the_killed_session() {
    let s = Sandbox::new();
    s.marks(&[("a", Some("session:1")), ("b", Some("session:2"))]);

    let out = s.run("{}", &["kill", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert_eq!(s.read_marks(), Some(owned(&[("b", Some("session:2"))])));
    assert_eq!(
        s.calls(),
        "api call session.destroy {\"session_id\":\"session:1\"}\n"
    );
}

#[test]
fn kill_destroys_nothing_while_the_marks_file_is_broken() {
    let s = Sandbox::new();
    fs::create_dir_all(s.path("state")).unwrap();
    fs::write(s.marks_file(), "not json").unwrap();

    let out = s.run("{}", &["kill", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(&s.marks_file().display().to_string()),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        s.calls(),
        "",
        "kill must not destroy a session it cannot unmark"
    );
}

#[test]
fn delete_confirmed_removes_the_presets_mark() {
    let s = Sandbox::new();
    s.preset("d.json", r#"{"name":"dotfiles","tabs":[{"cwd":"/d"}]}"#);
    s.preset("n.json", r#"{"name":"notes","tabs":[{"cwd":"/n"}]}"#);
    s.marks(&[
        ("notes", Some("session:1")),
        ("dotfiles", Some("session:2")),
    ]);

    let out = s.run("{}", &["delete", "--confirmed", "--", "dotfiles"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let path = s.sessions_file("d.json").display().to_string();
    assert_eq!(
        stdout(&out),
        format!("{{\"status\":\"deleted\",\"name\":\"dotfiles\",\"path\":\"{path}\"}}\n")
    );
    assert_eq!(s.read_marks(), Some(owned(&[("notes", Some("session:1"))])));
    assert_eq!(s.calls(), "", "delete must not call rex");
}

#[test]
fn delete_confirmed_deletes_nothing_while_the_marks_file_is_broken() {
    let s = Sandbox::new();
    s.preset("n.json", r#"{"name":"notes","tabs":[{"cwd":"/n"}]}"#);
    fs::create_dir_all(s.path("state")).unwrap();
    fs::write(s.marks_file(), "not json").unwrap();

    let out = s.run("{}", &["delete", "--confirmed", "--", "notes"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(&s.marks_file().display().to_string()),
        "{}",
        stderr(&out)
    );
    assert_eq!(s.sessions_listing(), ["n.json"]);
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), "not json");
    assert_eq!(s.calls(), "");
}

#[test]
fn kill_removes_all_marks_of_the_killed_session() {
    let s = Sandbox::new();
    s.marks(&[
        ("a", Some("session:1")),
        ("b", Some("session:1")),
        ("c", Some("session:2")),
    ]);

    let out = s.run("{}", &["kill", "--", "session:1"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert_eq!(s.read_marks(), Some(owned(&[("c", Some("session:2"))])));
    assert_eq!(
        s.calls(),
        "api call session.destroy {\"session_id\":\"session:1\"}\n"
    );
}

#[test]
fn kill_and_delete_leave_an_unmatched_marks_file_byte_identical() {
    let s = Sandbox::new();
    s.preset("z.json", r#"{"name":"zz","tabs":[{"cwd":"/z"}]}"#);
    s.marks(&[("a", Some("session:1"))]);
    let before = fs::read_to_string(s.marks_file()).unwrap();

    let kill = s.run("{}", &["kill", "--", "session:2"]);
    assert_eq!(kill.status.code(), Some(0), "{}", stderr(&kill));
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);

    let delete = s.run("{}", &["delete", "--confirmed", "--", "zz"]);
    assert_eq!(delete.status.code(), Some(0), "{}", stderr(&delete));
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);
}

/// A fake `rex` that answers `session.view` with a label derived from the
/// session id, so concurrent `mark` runs never share a reply file.
fn write_labelling_rex(s: &Sandbox) {
    let dir = s.path("").display().to_string();
    let rex = s.path("rex");
    fs::write(
        &rex,
        format!(
            concat!(
                "#!/bin/sh\n",
                "printf '%s\\n' \"$*\" >> '{dir}/rex.log'\n",
                "id=$(printf '%s' \"$4\" | sed -n 's/.*\"session_id\":\"\\([^\"]*\\)\".*/\\1/p')\n",
                "case \"$3\" in\n",
                "  session.view) printf '{{\"session_id\":\"%s\",\"label\":\"%s\",\"windows\":[]}}' \"$id\" \"${{id#session:}}\" ;;\n",
                "  *) printf '{{}}' ;;\n",
                "esac\n",
            ),
            dir = dir
        ),
    )
    .unwrap();
    fs::set_permissions(&rex, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn concurrent_marks_for_different_sessions_are_all_kept() {
    let s = Sandbox::new();
    for n in 0..8 {
        s.preset(
            &format!("p{n}.json"),
            &format!(r#"{{"name":"p{n}","tabs":[{{"cwd":"/p{n}"}}]}}"#),
        );
    }
    write_labelling_rex(&s);

    for round in 0..5 {
        let _ = fs::remove_file(s.marks_file());
        let children: Vec<_> = (0..8)
            .map(|n| {
                Command::new(env!("CARGO_BIN_EXE_rex-session-manager"))
                    .arg("--sessions-dir")
                    .arg(s.path("sessions"))
                    .arg("--marks-dir")
                    .arg(s.path("state"))
                    .args(["mark", "--", &format!("session:p{n}")])
                    .env("REX_BIN", s.path("rex"))
                    .env("HOME", s.path("home"))
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap()
            })
            .collect();
        for child in children {
            let out = child.wait_with_output().unwrap();
            assert_eq!(
                out.status.code(),
                Some(0),
                "round {round}: {}",
                stderr(&out)
            );
        }

        let mut names: Vec<_> = s
            .read_marks()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        names.sort();
        let expected = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"];
        assert_eq!(names, expected, "round {round}: concurrent marks were lost");
    }
}

#[test]
fn menu_marks_shows_no_move_row_for_a_single_mark() {
    let s = Sandbox::new();
    s.marks(&[("a", Some("session:1"))]);

    let out = s.run("{}", &["menu", "marks"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Marks ⚓","confirm":"Jump","items":["#,
            r#"{"id":"a","title":"a","subtitle":"q","icon":"tag"},"#,
            r#"{"id":"unmark","title":"Unmark…","icon":"package"}],"#,
            r#""unmark_id":"unmark","move_id":"move"}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "");
}

#[test]
fn menu_marks_never_gives_the_move_row_a_marks_id() {
    let s = Sandbox::new();
    s.marks(&[("move", None), ("z", None)]);

    let out = s.run("{}", &["menu", "marks"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Marks ⚓","confirm":"Jump","items":["#,
            r#"{"id":"move","title":"move","subtitle":"q","icon":"tag"},"#,
            r#"{"id":"z","title":"z","subtitle":"w","icon":"tag"},"#,
            r#"{"id":"move+","title":"Move…","icon":"branch"},"#,
            r#"{"id":"unmark","title":"Unmark…","icon":"package"}],"#,
            r#""unmark_id":"unmark","move_id":"move+"}"#,
            "\n"
        )
    );
    assert_eq!(s.calls(), "");
}

#[test]
fn menu_move_lists_the_marks_to_move() {
    let s = Sandbox::new();
    s.marks(&[("a", Some("session:1")), ("b", None)]);
    let before = fs::read_to_string(s.marks_file()).unwrap();

    let out = s.run("{}", &["menu", "move"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"title":"Move ⚓","confirm":"Choose","items":["#,
            r#"{"id":"a","title":"a","subtitle":"q","icon":"tag"},"#,
            r#"{"id":"b","title":"b","subtitle":"w","icon":"tag"}]}"#,
            "\n"
        )
    );
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);
    assert_eq!(s.calls(), "", "menu move must not call rex");
}

#[test]
fn move_asks_where_the_named_mark_goes_and_writes_nothing() {
    let s = Sandbox::new();
    s.marks(&[
        ("a", Some("session:1")),
        ("b", None),
        ("c", None),
        ("d", None),
        ("e\tf", None),
    ]);
    let before = fs::read_to_string(s.marks_file()).unwrap();

    let out = s.run("{}", &["move", "--", "b"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        concat!(
            r#"{"status":"choose","name":"b","from":2,"menu":{"#,
            r#""title":"Move b to…","confirm":"Move","items":["#,
            r#"{"id":"1","title":"1 · q","subtitle":"a","icon":"tag"},"#,
            r#"{"id":"2","title":"2 · w","subtitle":"b","icon":"tag"},"#,
            r#"{"id":"3","title":"3 · e","subtitle":"c","icon":"tag"},"#,
            r#"{"id":"4","title":"4 · r","subtitle":"d","icon":"tag"},"#,
            r#"{"id":"5","title":"5","subtitle":"e?f","icon":"tag"}]}}"#,
            "\n"
        )
    );
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);
    let mut entries: Vec<_> = fs::read_dir(s.path("state"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    entries.sort();
    assert_eq!(entries, ["marks.json"], "move NAME must create nothing");
    assert_eq!(s.calls(), "", "move must not call rex");
}

#[test]
fn move_to_a_later_position_shifts_the_marks_between_down() {
    let s = Sandbox::new();
    s.marks(&[
        ("a", Some("session:1")),
        ("b", None),
        ("c", None),
        ("d", None),
    ]);

    let out = s.run("{}", &["move", "--to", "3", "--", "a"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"status\":\"moved\",\"name\":\"a\",\"from\":1,\"to\":3,\"key\":\"e\"}\n"
    );
    assert_eq!(
        s.read_marks(),
        Some(owned(&[
            ("b", None),
            ("c", None),
            ("a", Some("session:1")),
            ("d", None)
        ]))
    );
    assert_eq!(s.calls(), "", "move must not call rex");

    // Past the keyed positions there is no key.
    s.marks(&[
        ("a", None),
        ("b", None),
        ("c", None),
        ("d", None),
        ("e", None),
    ]);
    let far = s.run("{}", &["move", "--to", "5", "--", "a"]);

    assert_eq!(far.status.code(), Some(0), "{}", stderr(&far));
    assert_eq!(
        stdout(&far),
        "{\"status\":\"moved\",\"name\":\"a\",\"from\":1,\"to\":5}\n"
    );
    assert_eq!(
        s.read_marks(),
        Some(owned(&[
            ("b", None),
            ("c", None),
            ("d", None),
            ("e", None),
            ("a", None)
        ]))
    );
}

#[test]
fn move_to_an_earlier_position_shifts_the_marks_between_up() {
    let s = Sandbox::new();
    s.marks(&[
        ("a", None),
        ("b", None),
        ("c", None),
        ("d", Some("session:4")),
    ]);

    let out = s.run("{}", &["move", "--to", "2", "--", "d"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"status\":\"moved\",\"name\":\"d\",\"from\":4,\"to\":2,\"key\":\"w\"}\n"
    );
    assert_eq!(
        s.read_marks(),
        Some(owned(&[
            ("a", None),
            ("d", Some("session:4")),
            ("b", None),
            ("c", None)
        ]))
    );
    assert_eq!(s.calls(), "", "move must not call rex");
}

#[test]
fn move_to_the_position_it_holds_writes_nothing() {
    let s = Sandbox::new();
    s.marks(&[
        ("a", Some("session:1")),
        ("b", Some("session:2")),
        ("c", None),
    ]);
    let before = fs::read_to_string(s.marks_file()).unwrap();

    let out = s.run("{}", &["move", "--to", "2", "--", "b"]);

    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "{\"status\":\"moved\",\"name\":\"b\",\"from\":2,\"to\":2,\"key\":\"w\"}\n"
    );
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);
    assert_eq!(s.calls(), "", "move must not call rex");
}

#[test]
fn move_refuses_a_position_past_the_list_an_unknown_mark_and_zero() {
    let s = Sandbox::new();
    s.marks(&[("a", None), ("b", None), ("c", None)]);
    let before = fs::read_to_string(s.marks_file()).unwrap();

    let out = s.run("{}", &["move", "--to", "9", "--", "a"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "rex-session-manager: no position 9 (3 marked)\n"
    );
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);

    let unknown = s.run("{}", &["move", "--to", "2", "--", "z"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(stdout(&unknown), "");
    assert_eq!(stderr(&unknown), "rex-session-manager: no mark named z\n");
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);

    let zero = s.run("{}", &["move", "--to", "0", "--", "a"]);
    assert_eq!(zero.status.code(), Some(2));
    assert_eq!(stdout(&zero), "");
    assert_eq!(fs::read_to_string(s.marks_file()).unwrap(), before);

    assert_eq!(s.calls(), "", "move must not call rex");
}

/// Runs `init-lua` with `args`, against a sandbox whose fake `rex` logs any call.
fn init_lua(args: &[&str]) -> (Sandbox, Output) {
    let s = Sandbox::new();
    let mut argv = vec!["init-lua"];
    argv.extend_from_slice(args);
    let output = s.run("{}", &argv);
    (s, output)
}

#[test]
fn init_lua_writes_the_given_binary_path_into_the_bridge() {
    let (s, output) = init_lua(&["--bin", "/opt/homebrew/bin/rex-session-manager"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(stderr(&output), "");
    let lua = stdout(&output);
    assert!(
        lua.contains("\nlocal BINARY = \"/opt/homebrew/bin/rex-session-manager\"\n"),
        "{lua}"
    );
    assert!(lua.ends_with("\nreturn M\n"), "{lua}");
    // The bridge API the actions use, and the setup that registers them.
    for name in [
        "function M.run(",
        "function M.jump(",
        "function M.jump_mark(",
        "M.SESSIONS_PATH =",
        "M.MARKS_PATH =",
        "function M.setup(PREFIX)",
    ] {
        assert!(lua.contains(name), "missing {name}");
    }
    assert!(!lua.contains("@BINARY@"), "placeholder left in the module");
    // Printing the module talks to no Rex server.
    assert_eq!(s.calls(), "");
}

#[test]
fn init_lua_defaults_to_the_running_binary() {
    let (_, output) = init_lua(&[]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let binary = env!("CARGO_BIN_EXE_rex-session-manager");
    assert!(
        stdout(&output).contains(&format!("\nlocal BINARY = \"{binary}\"\n")),
        "{}",
        stdout(&output)
    );
}

#[test]
fn init_lua_escapes_the_path_as_a_lua_string() {
    let (_, output) = init_lua(&["--bin", "/opt/a\"b\\c\nd/rex-session-manager"]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("\nlocal BINARY = \"/opt/a\\\"b\\\\c\\nd/rex-session-manager\"\n"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn init_lua_refuses_a_relative_binary_path() {
    let (s, output) = init_lua(&["--bin", "bin/rex-session-manager"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        "rex-session-manager: --bin must be an absolute path, because Rex runs actions \
         from an unknown directory: bin/rex-session-manager\n"
    );
    assert_eq!(s.calls(), "");
}
