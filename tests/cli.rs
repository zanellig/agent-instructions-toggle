//! The public CLI is the test seam. Every test drives the real binary against
//! real files in an isolated home, and asserts only what a user can observe:
//! exit status, output, and the filenames left on disk.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_agent-instructions");
const DISABLED: &str = ".no-auto-inject";
const CODEX: [&str; 3] = [".codex", ".codex_p", ".codex_p2"];
const ENABLED_NAMES: [&str; 4] = [
    ".codex/AGENTS.md",
    ".codex_p/AGENTS.md",
    ".codex_p2/AGENTS.md",
    ".claude/CLAUDE.md",
];

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Home {
    root: PathBuf,
}

impl Home {
    /// An empty home with the managed directories present but no documents.
    fn empty() -> Home {
        let root = std::env::temp_dir().join(format!(
            "agent-instructions-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for dir in CODEX.iter().chain([".claude", ".codex_backup"].iter()) {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        Home { root }
    }

    /// The normal installation: every managed target present and recognized,
    /// plus an archived document in the excluded backup home.
    fn populated() -> Home {
        let home = Home::empty();
        for name in ENABLED_NAMES {
            home.write(name, &format!("contents of {name}"));
        }
        home.write(".codex_backup/AGENTS.md", "archived");
        home
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn write(&self, rel: &str, contents: &str) {
        fs::write(self.path(rel), contents).unwrap();
    }

    fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BIN);
        command
            .env("HOME", &self.root)
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env_remove("XDG_STATE_HOME");
        command
    }

    fn run(&self, args: &[&str]) -> Run {
        Run::from(self.command().args(args).output().unwrap())
    }

    fn state(&self) -> String {
        let run = self.run(&["status", "--machine"]);
        assert!(run.ok, "status must succeed for every state: {}", run.err);
        run.out.trim().to_string()
    }

    /// Rename a target to its disabled name behind the tool's back.
    fn disable_behind_the_scenes(&self, rel: &str) {
        fs::rename(self.path(rel), self.path(&format!("{rel}{DISABLED}"))).unwrap();
    }

    fn assert_all_enabled(&self) {
        for name in ENABLED_NAMES {
            assert!(self.exists(name), "{name} should use its recognized name");
            assert!(
                !self.exists(&format!("{name}{DISABLED}")),
                "{name} disabled copy"
            );
        }
    }

    fn assert_all_disabled(&self) {
        for name in ENABLED_NAMES {
            assert!(
                !self.exists(name),
                "{name} should not use its recognized name"
            );
            assert!(
                self.exists(&format!("{name}{DISABLED}")),
                "{name} disabled copy"
            );
        }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        // A test may have made a directory read-only to force a rename failure.
        for dir in CODEX.iter().chain([".claude", ".codex_backup"].iter()) {
            let path = self.root.join(dir);
            if let Ok(meta) = fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Run {
    ok: bool,
    out: String,
    err: String,
}

impl From<Output> for Run {
    fn from(output: Output) -> Run {
        Run {
            ok: output.status.success(),
            out: String::from_utf8_lossy(&output.stdout).into_owned(),
            err: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

#[test]
fn toggle_disables_then_restores_without_touching_contents() {
    let home = Home::populated();
    assert_eq!(home.state(), "on");

    let run = home.run(&["toggle"]);
    assert!(run.ok, "{}", run.err);
    home.assert_all_disabled();
    assert_eq!(home.state(), "off");
    assert_eq!(
        home.read(".codex/AGENTS.md.no-auto-inject"),
        "contents of .codex/AGENTS.md",
        "disabling must rename, never empty or rewrite"
    );

    let run = home.run(&["toggle"]);
    assert!(run.ok, "{}", run.err);
    home.assert_all_enabled();
    assert_eq!(home.state(), "on");
    assert_eq!(
        home.read(".claude/CLAUDE.md"),
        "contents of .claude/CLAUDE.md"
    );
}

#[test]
fn enable_and_disable_are_idempotent() {
    let home = Home::populated();

    for _ in 0..2 {
        assert!(home.run(&["disable"]).ok);
        home.assert_all_disabled();
    }
    for _ in 0..2 {
        assert!(home.run(&["enable"]).ok);
        home.assert_all_enabled();
    }
}

#[test]
fn status_succeeds_for_every_observable_state() {
    let home = Home::populated();
    assert_eq!(home.state(), "on");

    home.run(&["disable"]);
    assert_eq!(home.state(), "off");

    fs::rename(
        home.path(".codex/AGENTS.md.no-auto-inject"),
        home.path(".codex/AGENTS.md"),
    )
    .unwrap();
    assert_eq!(home.state(), "mixed");

    home.write(".codex/AGENTS.md.no-auto-inject", "and the other name too");
    assert_eq!(home.state(), "conflict");
}

#[test]
fn machine_status_prints_one_token() {
    let home = Home::populated();
    let run = home.run(&["status", "--machine"]);
    assert!(run.ok);
    assert_eq!(run.out, "on\n");
}

#[test]
fn human_status_names_the_state_and_the_missing_targets() {
    let home = Home::populated();
    fs::remove_file(home.path(".codex_p2/AGENTS.md")).unwrap();

    let run = home.run(&["status"]);
    assert!(run.ok);
    assert!(run.out.contains("AGENTS: on"), "{}", run.out);
    assert!(
        run.out.contains("missing: ~/.codex_p2/AGENTS.md"),
        "{}",
        run.out
    );
}

#[test]
fn a_missing_target_warns_while_the_rest_transition() {
    let home = Home::populated();
    fs::remove_file(home.path(".codex_p/AGENTS.md")).unwrap();

    let run = home.run(&["disable"]);
    assert!(run.ok, "{}", run.err);
    assert!(
        run.out.contains("missing: ~/.codex_p/AGENTS.md"),
        "{}",
        run.out
    );
    assert_eq!(home.state(), "off");
    assert!(home.exists(".codex/AGENTS.md.no-auto-inject"));
    assert!(home.exists(".claude/CLAUDE.md.no-auto-inject"));
    assert!(!home.exists(".codex_p/AGENTS.md.no-auto-inject"));
}

#[test]
fn an_empty_installation_is_a_conflict_and_refuses_to_mutate() {
    let home = Home::empty();
    assert_eq!(home.state(), "conflict");

    let run = home.run(&["disable"]);
    assert!(
        !run.ok,
        "a state that cannot be derived must not be claimed"
    );
    assert!(
        run.err.contains("no managed instruction documents"),
        "{}",
        run.err
    );
}

#[test]
fn a_mixed_state_recovers_to_on_and_stops() {
    let home = Home::populated();
    home.disable_behind_the_scenes(".codex_p/AGENTS.md");
    home.disable_behind_the_scenes(".claude/CLAUDE.md");
    assert_eq!(home.state(), "mixed");

    let run = home.run(&["disable"]);
    assert!(run.ok, "{}", run.err);
    assert!(run.out.contains("recovered from mixed"), "{}", run.out);
    home.assert_all_enabled();
    assert_eq!(
        home.state(),
        "on",
        "recovery reconciles to on and stops there"
    );

    // Only a second, deliberate action disables.
    assert!(home.run(&["disable"]).ok);
    assert_eq!(home.state(), "off");
}

#[test]
fn a_same_target_collision_refuses_every_mutation() {
    let home = Home::populated();
    home.write(".claude/CLAUDE.md.no-auto-inject", "a second copy");

    for action in ["enable", "disable", "toggle"] {
        let run = home.run(&[action]);
        assert!(!run.ok, "{action} must fail on a collision");
        assert!(run.err.contains("~/.claude/CLAUDE.md"), "{}", run.err);
    }

    // Both copies survive, and no other target moved.
    assert!(home.exists(".claude/CLAUDE.md"));
    assert!(home.exists(".claude/CLAUDE.md.no-auto-inject"));
    assert!(home.exists(".codex/AGENTS.md"));
}

#[test]
fn the_backup_codex_home_is_never_touched() {
    let home = Home::populated();
    assert!(home.run(&["disable"]).ok);

    assert!(
        home.exists(".codex_backup/AGENTS.md"),
        "archived files stay put"
    );
    assert!(!home.exists(&format!(".codex_backup/AGENTS.md{DISABLED}")));
    assert_eq!(home.read(".codex_backup/AGENTS.md"), "archived");
}

#[test]
fn concurrent_toggles_serialize() {
    let home = Home::populated();

    // Eight flips of a two-state value land back on `on` only if every
    // operation observed the previous one's result.
    let children: Vec<_> = (0..8)
        .map(|_| home.command().arg("toggle").spawn().unwrap())
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }

    assert_eq!(home.state(), "on");
    home.assert_all_enabled();
}

#[test]
fn a_failed_rename_rolls_back_the_completed_ones() {
    let home = Home::populated();

    // `.claude` sorts last in the rename order, so three renames succeed before
    // this one fails.
    let dir = home.path(".claude");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(dir.join("probe"), "").is_ok() {
        let _ = fs::remove_file(dir.join("probe"));
        eprintln!("skipped: this user can write to a read-only directory");
        return;
    }

    let run = home.run(&["disable"]);
    assert!(
        !run.ok,
        "an unfinished transition must be reported as a failure"
    );
    assert!(run.err.contains("rolled back 3"), "{}", run.err);

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    home.assert_all_enabled();
    assert_eq!(home.state(), "on");
}

#[test]
fn an_unknown_command_is_a_usage_error() {
    let home = Home::populated();
    let output = home.command().arg("frobnicate").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(home.state(), "on", "a usage error changes nothing");
}
