#![cfg(target_os = "linux")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TestHome {
    root: PathBuf,
}

impl TestHome {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agent-instructions-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create isolated test home");
        Self { root }
    }

    fn enable_all(&self) {
        for path in self.recognized_paths() {
            fs::create_dir_all(path.parent().expect("target parent")).expect("create target home");
            fs::write(path, "instructions\n").expect("write instruction document");
        }
    }

    fn disable_all(&self) {
        for path in self.recognized_paths() {
            fs::create_dir_all(path.parent().expect("target parent")).expect("create target home");
            fs::write(disabled(&path), "instructions\n").expect("write disabled document");
        }
    }

    fn recognized_paths(&self) -> Vec<PathBuf> {
        [
            ".codex/AGENTS.md",
            ".codex_p/AGENTS.md",
            ".codex_p2/AGENTS.md",
            ".claude/CLAUDE.md",
        ]
        .into_iter()
        .map(|path| self.root.join(path))
        .collect()
    }

    fn command(&self, operation: &str) -> Output {
        self.command_with_env(operation, &[])
    }

    fn command_with_env(&self, operation: &str, environment: &[(&str, &str)]) -> Output {
        let mut command = self.command_builder(operation);
        for (name, value) in environment {
            command.env(name, value);
        }
        command.output().expect("run agent-instructions")
    }

    fn command_builder(&self, operation: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-instructions"));
        command
            .arg(operation)
            .env("HOME", &self.root)
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("PATH", self.root.join("empty-path"));
        command
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        if self.root.starts_with(std::env::temp_dir()) {
            fs::remove_dir_all(&self.root).expect("remove isolated test home");
        }
    }
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("command output is UTF-8")
}

fn disabled(path: &Path) -> PathBuf {
    let name = format!(
        "{}.no-auto-inject",
        path.file_name()
            .expect("instruction filename")
            .to_string_lossy()
    );
    path.with_file_name(name)
}

#[test]
fn status_reports_on_for_recognized_instruction_documents() {
    let home = TestHome::new();
    home.enable_all();

    let output = home.command("status");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "on\n");
    assert_eq!(text(&output.stderr), "");
    for path in home.recognized_paths() {
        assert!(path.exists());
        assert!(!disabled(&path).exists());
    }
}

#[test]
fn status_reports_off_for_disabled_instruction_documents() {
    let home = TestHome::new();
    home.disable_all();

    let output = home.command("status");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "off\n");
    assert_eq!(text(&output.stderr), "");
}

#[test]
fn status_reports_mixed_when_present_targets_disagree() {
    let home = TestHome::new();
    home.enable_all();
    let active = home.recognized_paths().remove(0);
    fs::rename(&active, disabled(&active)).expect("split the instruction state");

    let output = home.command("status");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "mixed\n");
    assert_eq!(text(&output.stderr), "");
}

#[test]
fn status_reports_conflict_without_choosing_between_colliding_documents() {
    let home = TestHome::new();
    home.enable_all();
    let active = home.recognized_paths().remove(0);
    fs::write(disabled(&active), "different instructions\n").expect("create collision");

    let output = home.command("status");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "conflict\n");
    assert!(
        text(&output.stderr).contains("~/.codex/AGENTS.md"),
        "stderr: {}",
        text(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&active).unwrap(), "instructions\n");
    assert_eq!(
        fs::read_to_string(disabled(&active)).unwrap(),
        "different instructions\n"
    );
}

#[test]
fn status_warns_about_a_missing_target_without_blocking_healthy_targets() {
    let home = TestHome::new();
    home.enable_all();
    fs::remove_file(home.root.join(".codex_p2/AGENTS.md")).expect("remove unused profile");

    let output = home.command("status");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "on\n");
    assert_eq!(
        text(&output.stderr),
        "warning: missing managed target: ~/.codex_p2/AGENTS.md\n"
    );
}

#[test]
fn status_treats_an_all_missing_installation_as_conflict() {
    let home = TestHome::new();
    for path in home.recognized_paths() {
        fs::create_dir_all(path.parent().unwrap()).expect("create empty profile");
    }

    let output = home.command("status");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "conflict\n");
    assert_eq!(
        text(&output.stderr)
            .matches("missing managed target")
            .count(),
        4
    );
}

#[test]
fn toggle_renames_documents_without_changing_contents_permissions_or_backup() {
    let home = TestHome::new();
    home.enable_all();
    let first = home.recognized_paths().remove(0);
    fs::write(&first, "keep these instructions\n").expect("write distinctive content");
    fs::set_permissions(&first, fs::Permissions::from_mode(0o640)).expect("set permissions");
    let backup = home.root.join(".codex_backup/AGENTS.md");
    fs::create_dir_all(backup.parent().unwrap()).expect("create backup home");
    fs::write(&backup, "archived instructions\n").expect("write backup instructions");

    let disabled_output = home.command("toggle");

    assert!(
        disabled_output.status.success(),
        "stderr: {}",
        text(&disabled_output.stderr)
    );
    assert!(text(&disabled_output.stdout).contains("AGENTS: off"));
    assert!(text(&disabled_output.stdout).contains("New contexts"));
    assert!(!first.exists());
    assert_eq!(
        fs::read_to_string(disabled(&first)).unwrap(),
        "keep these instructions\n"
    );
    assert_eq!(
        fs::metadata(disabled(&first)).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::read_to_string(&backup).unwrap(),
        "archived instructions\n"
    );

    let enabled_output = home.command("toggle");

    assert!(
        enabled_output.status.success(),
        "stderr: {}",
        text(&enabled_output.stderr)
    );
    assert!(text(&enabled_output.stdout).contains("AGENTS: on"));
    assert_eq!(
        fs::read_to_string(&first).unwrap(),
        "keep these instructions\n"
    );
    assert!(!disabled(&first).exists());
}

#[test]
fn enable_and_disable_are_idempotent() {
    let home = TestHome::new();
    home.enable_all();

    let already_enabled = home.command("enable");
    assert!(already_enabled.status.success());
    assert!(text(&already_enabled.stdout).contains("already enabled"));

    let disabled_once = home.command("disable");
    assert!(disabled_once.status.success());
    let already_disabled = home.command("disable");
    assert!(already_disabled.status.success());
    assert!(text(&already_disabled.stdout).contains("already disabled"));

    let enabled_once = home.command("enable");
    assert!(enabled_once.status.success());
    for active in home.recognized_paths() {
        assert!(active.exists());
        assert!(!disabled(&active).exists());
    }
}

#[test]
fn a_mixed_state_recovers_to_on_and_stops_the_requested_disable() {
    let home = TestHome::new();
    home.enable_all();
    let split = home.recognized_paths().remove(0);
    fs::rename(&split, disabled(&split)).expect("split the instruction state");

    let recovery = home.command("disable");

    assert!(
        recovery.status.success(),
        "stderr: {}",
        text(&recovery.stderr)
    );
    assert!(text(&recovery.stdout).contains("recovered to enabled"));
    assert!(text(&recovery.stdout).contains("run it again"));
    for active in home.recognized_paths() {
        assert!(active.exists());
        assert!(!disabled(&active).exists());
    }

    let deliberate_disable = home.command("disable");
    assert!(deliberate_disable.status.success());
    for active in home.recognized_paths() {
        assert!(!active.exists());
        assert!(disabled(&active).exists());
    }
}

#[test]
fn a_collision_refuses_mutation_and_preserves_both_documents() {
    let home = TestHome::new();
    home.enable_all();
    let active = home.recognized_paths().remove(1);
    fs::write(disabled(&active), "disabled copy\n").expect("create collision");

    let output = home.command("toggle");

    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("AGENTS: conflict"));
    assert!(active.exists());
    assert_eq!(
        fs::read_to_string(disabled(&active)).unwrap(),
        "disabled copy\n"
    );
    for other in home.recognized_paths() {
        assert!(other.exists());
    }
}

#[test]
fn a_missing_target_does_not_block_healthy_target_transitions() {
    let home = TestHome::new();
    home.enable_all();
    let missing = home.recognized_paths().remove(2);
    fs::remove_file(&missing).expect("remove unused profile");

    let output = home.command("disable");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert!(text(&output.stderr).contains("~/.codex_p2/AGENTS.md"));
    for active in home.recognized_paths() {
        if active == missing {
            assert!(!active.exists());
            assert!(!disabled(&active).exists());
        } else {
            assert!(!active.exists());
            assert!(disabled(&active).exists());
        }
    }
}

#[test]
fn a_late_rename_failure_rolls_back_completed_renames() {
    let home = TestHome::new();
    home.enable_all();

    let output = home.command_with_env(
        "disable",
        &[("AGENT_INSTRUCTIONS_TEST_FAIL_RENAME_AT", "2")],
    );

    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("change failed"));
    for active in home.recognized_paths() {
        assert!(active.exists(), "{} was not rolled back", active.display());
        assert!(!disabled(&active).exists());
    }
}

#[test]
fn concurrent_mutations_wait_for_the_same_lock() {
    let home = TestHome::new();
    home.enable_all();
    let mut first_command = home.command_builder("toggle");
    first_command.env("AGENT_INSTRUCTIONS_TEST_HOLD_LOCK_MS", "500");
    let first = first_command.spawn().expect("start lock holder");
    std::thread::sleep(std::time::Duration::from_millis(100));

    let started_waiting = std::time::Instant::now();
    let second = home.command("toggle");
    let waited = started_waiting.elapsed();
    let first = first.wait_with_output().expect("wait for lock holder");

    assert!(first.status.success(), "stderr: {}", text(&first.stderr));
    assert!(second.status.success(), "stderr: {}", text(&second.stderr));
    assert!(
        waited >= std::time::Duration::from_millis(250),
        "second mutation waited only {waited:?}"
    );
    assert_eq!(text(&home.command("status").stdout), "on\n");
}

#[test]
fn mutation_falls_back_to_the_state_directory_when_runtime_locking_is_unavailable() {
    let home = TestHome::new();
    home.enable_all();
    let blocked_runtime = home.root.join("blocked-runtime");
    fs::write(&blocked_runtime, "not a directory").expect("block runtime lock directory");
    let blocked_runtime = blocked_runtime.to_string_lossy();

    let output = home.command_with_env("disable", &[("XDG_RUNTIME_DIR", &blocked_runtime)]);

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert!(
        home.root
            .join("state/agent-instructions/operation.lock")
            .exists()
    );
}

#[test]
fn an_all_missing_installation_refuses_mutation() {
    let home = TestHome::new();

    let output = home.command("toggle");

    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("AGENTS: conflict"));
    for active in home.recognized_paths() {
        assert!(!active.exists());
        assert!(!disabled(&active).exists());
    }
}

#[test]
fn preflight_rejects_a_non_document_before_any_rename_starts() {
    let home = TestHome::new();
    home.enable_all();
    let invalid = home.recognized_paths().remove(1);
    fs::remove_file(&invalid).expect("remove document");
    fs::create_dir(&invalid).expect("replace document with directory");

    let output = home.command("disable");

    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("preflight"));
    for active in home.recognized_paths() {
        assert!(
            active.exists(),
            "{} changed before preflight finished",
            active.display()
        );
        assert!(!disabled(&active).exists());
    }
}

#[test]
fn profiles_are_discovered_while_backup_directories_are_ignored() {
    let home = TestHome::new();
    home.enable_all();
    let discovered = home.root.join(".codex_work/AGENTS.md");
    fs::create_dir_all(discovered.parent().unwrap()).expect("create discovered profile");
    fs::write(&discovered, "work profile\n").expect("write discovered profile");
    let compact_profile = home.root.join(".codex2/AGENTS.md");
    fs::create_dir_all(compact_profile.parent().unwrap()).expect("create compact profile name");
    fs::write(&compact_profile, "compact profile\n").expect("write compact profile");
    let discovered_claude = home.root.join(".claude_work/CLAUDE.md");
    fs::create_dir_all(discovered_claude.parent().unwrap())
        .expect("create discovered Claude profile");
    fs::write(&discovered_claude, "work profile\n").expect("write discovered Claude profile");
    let backups = [
        ".codex_backup/AGENTS.md",
        ".codex_backup2/AGENTS.md",
        ".codexPersonalBackup/AGENTS.md",
        ".codex-personal.bak/AGENTS.md",
        ".codex.old/AGENTS.md",
        ".codex_orig/AGENTS.md",
        ".codex_copy/AGENTS.md",
        ".codex_save/AGENTS.md",
        ".codex_disabled/AGENTS.md",
        ".codex_p2~/AGENTS.md",
        ".claude_archive/CLAUDE.md",
        ".claude_mybackup/CLAUDE.md",
    ];
    for backup in backups {
        let path = home.root.join(backup);
        fs::create_dir_all(path.parent().unwrap()).expect("create backup profile");
        fs::write(path, "archived profile\n").expect("write backup profile");
    }

    let output = home.command("disable");

    assert!(output.status.success(), "stderr: {}", text(&output.stderr));
    assert!(!discovered.exists());
    assert!(disabled(&discovered).exists());
    assert!(!compact_profile.exists());
    assert!(disabled(&compact_profile).exists());
    assert!(!discovered_claude.exists());
    assert!(disabled(&discovered_claude).exists());
    for backup in backups {
        let path = home.root.join(backup);
        assert!(path.exists(), "backup was renamed: {}", path.display());
        assert!(!disabled(&path).exists());
    }
}

#[test]
fn profile_labels_escape_terminal_controls_and_tooltip_markup() {
    let home = TestHome::new();
    home.enable_all();
    let hostile_name = ".codex_\u{1b}[31m\n<b&>";
    fs::create_dir(home.root.join(hostile_name)).expect("create hostile profile name");

    let output = home.command("status");

    assert!(output.status.success());
    let stderr = text(&output.stderr);
    assert!(!stderr.contains('\u{1b}'));
    assert!(!stderr.contains("\n<b&>"));
    assert!(stderr.contains("\\u{1b}[31m\\n\\u{3c}b\\u{26}\\u{3e}"));

    fs::create_dir(home.root.join(hostile_name).join("AGENTS.md"))
        .expect("replace missing document with a directory");
    let mutation = home.command("disable");
    let mutation_error = text(&mutation.stderr);
    assert!(!mutation.status.success());
    assert!(!mutation_error.contains('\u{1b}'));
    assert!(!mutation_error.contains("\n<b&>"));
    assert!(mutation_error.contains("\\u{1b}[31m\\n\\u{3c}b\\u{26}\\u{3e}"));
}
