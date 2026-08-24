use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TestHome {
    root: PathBuf,
    home: PathBuf,
    runtime: PathBuf,
    state: PathBuf,
}

impl TestHome {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agent-instructions-test-{}-{sequence}",
            std::process::id()
        ));
        let home = root.join("home");
        let runtime = root.join("runtime");
        let state = root.join("state");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&state).unwrap();

        Self {
            root,
            home,
            runtime,
            state,
        }
    }

    fn write_enabled(&self, target: &str, filename: &str) {
        let directory = self.home.join(target);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(filename), format!("{target} instructions\n")).unwrap();
    }

    fn write_disabled(&self, target: &str, filename: &str) {
        let directory = self.home.join(target);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{filename}.no-auto-inject")),
            format!("{target} instructions\n"),
        )
        .unwrap();
    }

    fn create_directory(&self, target: &str) {
        fs::create_dir_all(self.home.join(target)).unwrap();
    }

    fn write_symlinked_profile(&self, link_name: &str) {
        let source = self.root.join("linked-profile");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("AGENTS.md"), "linked instructions\n").unwrap();
        symlink(source, self.home.join(link_name)).unwrap();
    }

    fn command(&self, args: &[&str]) -> Output {
        self.command_builder(args).output().unwrap()
    }

    fn command_without_runtime(&self, args: &[&str]) -> Output {
        self.command_builder(args)
            .env_remove("XDG_RUNTIME_DIR")
            .output()
            .unwrap()
    }

    fn spawn(&self, args: &[&str]) -> Child {
        self.command_builder(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn command_builder(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-instructions"));
        command
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_STATE_HOME", &self.state);
        command
    }

    fn path(&self, target: &str, filename: &str) -> PathBuf {
        self.home.join(target).join(filename)
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn create_all_enabled(home: &TestHome) {
    home.write_enabled(".codex", "AGENTS.md");
    home.write_enabled(".codex-lab", "AGENTS.md");
    home.write_enabled(".codex.team", "AGENTS.md");
    home.write_enabled(".claude", "CLAUDE.md");
}

#[test]
fn status_reports_on_in_human_and_machine_formats() {
    let home = TestHome::new();
    create_all_enabled(&home);

    let human = home.command(&["status"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert_eq!(stdout(&human), "Instruction state: on\n");

    let machine = home.command(&["status", "--machine"]);
    assert!(machine.status.success(), "{}", stderr(&machine));
    assert_eq!(stdout(&machine), "on\n");
}

#[test]
fn status_reports_partial_absence_without_hiding_the_healthy_state() {
    let home = TestHome::new();
    home.write_enabled(".codex-work", "AGENTS.md");
    home.create_directory(".codex-zeta");
    home.create_directory(".codex-alpha");

    let status = home.command(&["status", "--machine"]);

    assert!(status.status.success(), "{}", stderr(&status));
    assert_eq!(stdout(&status), "on\n");
    assert_eq!(
        stderr(&status),
        "Warning: missing managed targets: .codex-alpha/AGENTS.md, .codex-zeta/AGENTS.md, .claude/CLAUDE.md\n"
    );
}

#[test]
fn status_reports_every_observable_instruction_state_successfully() {
    let off = TestHome::new();
    off.write_disabled(".codex-lab", "AGENTS.md");
    let off_status = off.command(&["status", "--machine"]);
    assert!(off_status.status.success());
    assert_eq!(stdout(&off_status), "off\n");

    let mixed = TestHome::new();
    mixed.write_enabled(".codex-alpha", "AGENTS.md");
    mixed.write_disabled(".codex-zeta", "AGENTS.md");
    let mixed_status = mixed.command(&["status", "--machine"]);
    assert!(mixed_status.status.success());
    assert_eq!(stdout(&mixed_status), "mixed\n");

    let collision = TestHome::new();
    collision.write_enabled(".codex-work", "AGENTS.md");
    collision.write_disabled(".codex-work", "AGENTS.md");
    let collision_status = collision.command(&["status", "--machine"]);
    assert!(collision_status.status.success());
    assert_eq!(stdout(&collision_status), "conflict\n");
    assert!(
        stderr(&collision_status)
            .contains("conflicting names for managed targets: .codex-work/AGENTS.md")
    );

    let all_missing = TestHome::new();
    let missing_status = all_missing.command(&["status", "--machine"]);
    assert!(missing_status.status.success());
    assert_eq!(stdout(&missing_status), "conflict\n");
}

#[test]
fn mutation_commands_transition_all_targets_and_are_idempotent() {
    let home = TestHome::new();
    create_all_enabled(&home);

    let disable = home.command(&["disable"]);
    assert!(disable.status.success(), "{}", stderr(&disable));
    assert_eq!(stdout(&disable), "Instruction state: off\n");
    for (target, filename) in [
        (".codex", "AGENTS.md"),
        (".codex-lab", "AGENTS.md"),
        (".codex.team", "AGENTS.md"),
        (".claude", "CLAUDE.md"),
    ] {
        assert!(!home.path(target, filename).exists());
        assert_path_exists(&home.path(target, &format!("{filename}.no-auto-inject")));
    }

    let repeated_disable = home.command(&["disable"]);
    assert!(
        repeated_disable.status.success(),
        "{}",
        stderr(&repeated_disable)
    );
    assert_eq!(stdout(&repeated_disable), "Instruction state: off\n");

    let toggle = home.command(&["toggle"]);
    assert!(toggle.status.success(), "{}", stderr(&toggle));
    assert_eq!(stdout(&toggle), "Instruction state: on\n");

    let enable = home.command(&["enable"]);
    assert!(enable.status.success(), "{}", stderr(&enable));
    assert_eq!(stdout(&enable), "Instruction state: on\n");
    assert_eq!(
        fs::read_to_string(home.path(".codex", "AGENTS.md")).unwrap(),
        ".codex instructions\n"
    );
}

#[test]
fn mutation_warns_about_missing_targets_and_changes_healthy_targets() {
    let home = TestHome::new();
    home.write_enabled(".codex-work", "AGENTS.md");
    home.create_directory(".codex-empty");

    let disable = home.command(&["disable"]);

    assert!(disable.status.success(), "{}", stderr(&disable));
    assert_eq!(stdout(&disable), "Instruction state: off\n");
    assert_eq!(
        stderr(&disable),
        "Warning: missing managed targets: .codex-empty/AGENTS.md, .claude/CLAUDE.md\n"
    );
    assert_path_exists(&home.path(".codex-work", "AGENTS.md.no-auto-inject"));
}

#[test]
fn mutation_recovers_mixed_state_to_on_and_stops() {
    let home = TestHome::new();
    home.write_enabled(".codex-alpha", "AGENTS.md");
    home.write_disabled(".codex-zeta", "AGENTS.md");

    let disable = home.command(&["disable"]);

    assert!(disable.status.success(), "{}", stderr(&disable));
    assert_eq!(
        stdout(&disable),
        "Recovered mixed instruction state to on; requested action was not applied.\nInstruction state: on\n"
    );
    assert_path_exists(&home.path(".codex-alpha", "AGENTS.md"));
    assert_path_exists(&home.path(".codex-zeta", "AGENTS.md"));
    assert!(
        !home
            .path(".codex-zeta", "AGENTS.md.no-auto-inject")
            .exists()
    );
}

#[test]
fn mutation_refuses_collisions_without_changing_either_file() {
    let home = TestHome::new();
    home.write_enabled(".codex-work", "AGENTS.md");
    home.write_disabled(".codex-work", "AGENTS.md");

    let disable = home.command(&["disable"]);

    assert!(!disable.status.success());
    assert!(
        stderr(&disable).contains("both names exist for .codex-work/AGENTS.md"),
        "{}",
        stderr(&disable)
    );
    assert_path_exists(&home.path(".codex-work", "AGENTS.md"));
    assert_path_exists(&home.path(".codex-work", "AGENTS.md.no-auto-inject"));
}

#[test]
fn mutation_refuses_an_all_missing_installation() {
    let home = TestHome::new();

    let toggle = home.command(&["toggle"]);

    assert!(!toggle.status.success());
    assert!(
        stderr(&toggle).contains("every managed target is missing"),
        "{}",
        stderr(&toggle)
    );
    assert_eq!(fs::read_dir(&home.home).unwrap().count(), 0);
}

#[test]
fn later_rename_failure_rolls_back_completed_renames() {
    let home = TestHome::new();
    home.write_enabled(".codex-alpha", "AGENTS.md");
    home.write_enabled(".codex-zeta", "AGENTS.md");
    let blocked_directory = home.path(".codex-zeta", "");
    fs::set_permissions(&blocked_directory, fs::Permissions::from_mode(0o555)).unwrap();

    let disable = home.command(&["disable"]);

    fs::set_permissions(&blocked_directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!disable.status.success());
    assert!(
        stderr(&disable).contains("completed renames were rolled back"),
        "{}",
        stderr(&disable)
    );
    assert_path_exists(&home.path(".codex-alpha", "AGENTS.md"));
    assert!(
        !home
            .path(".codex-alpha", "AGENTS.md.no-auto-inject")
            .exists()
    );
    assert_path_exists(&home.path(".codex-zeta", "AGENTS.md"));
    assert!(
        !home
            .path(".codex-zeta", "AGENTS.md.no-auto-inject")
            .exists()
    );
}

#[test]
fn concurrent_mutations_are_serialized() {
    let home = TestHome::new();
    create_all_enabled(&home);

    let children: Vec<_> = (0..20).map(|_| home.spawn(&["toggle"])).collect();
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();

    for output in outputs {
        assert!(output.status.success(), "{}", stderr(&output));
    }
    let status = home.command(&["status", "--machine"]);
    assert!(status.status.success(), "{}", stderr(&status));
    assert_eq!(stdout(&status), "on\n");
}

#[test]
fn mutation_uses_the_state_directory_when_no_runtime_directory_is_available() {
    let home = TestHome::new();
    home.write_enabled(".codex-work", "AGENTS.md");

    let disable = home.command_without_runtime(&["disable"]);

    assert!(disable.status.success(), "{}", stderr(&disable));
    assert_path_exists(&home.state.join("agent-instructions/operation.lock"));
    assert_path_exists(&home.path(".codex-work", "AGENTS.md.no-auto-inject"));
}

#[test]
fn mutation_discovers_profiles_added_between_invocations() {
    let home = TestHome::new();
    home.write_enabled(".codex-alpha", "AGENTS.md");
    home.write_enabled(".claude", "CLAUDE.md");

    let initial = home.command(&["status", "--machine"]);
    assert!(initial.status.success(), "{}", stderr(&initial));
    assert_eq!(stdout(&initial), "on\n");

    home.write_enabled(".codex-new-team", "AGENTS.md");
    let after_addition = home.command(&["disable"]);
    assert!(
        after_addition.status.success(),
        "{}",
        stderr(&after_addition)
    );
    assert_path_exists(&home.path(".codex-alpha", "AGENTS.md.no-auto-inject"));
    assert_path_exists(&home.path(".codex-new-team", "AGENTS.md.no-auto-inject"));
}

#[test]
fn mutation_discovers_symlinked_profile_directories() {
    let home = TestHome::new();
    home.write_symlinked_profile(".codex-linked");

    let disable = home.command(&["disable"]);

    assert!(disable.status.success(), "{}", stderr(&disable));
    assert_path_exists(&home.path(".codex-linked", "AGENTS.md.no-auto-inject"));
    assert!(!home.path(".codex-linked", "AGENTS.md").exists());
}

#[test]
fn discovery_excludes_backup_like_profiles_and_non_directories() {
    let home = TestHome::new();
    let excluded = [
        ".codex-bak",
        ".codex-team-BACKUP2-active",
        ".codex.old3",
        ".codex-orig",
        ".codex_copy7",
        ".codex.archive",
        ".codex-save99",
        ".codex-disabled",
        ".codex-session~",
    ];
    for profile in excluded {
        home.write_enabled(profile, "AGENTS.md");
    }
    home.write_enabled(".codex-backupish", "AGENTS.md");
    home.write_enabled(".codexold", "AGENTS.md");
    fs::write(home.home.join(".codex-regular-file"), "not a profile\n").unwrap();

    let disable = home.command(&["disable"]);

    assert!(disable.status.success(), "{}", stderr(&disable));
    for profile in excluded {
        assert_path_exists(&home.path(profile, "AGENTS.md"));
        assert!(!home.path(profile, "AGENTS.md.no-auto-inject").exists());
    }
    for profile in [".codex-backupish", ".codexold"] {
        assert_path_exists(&home.path(profile, "AGENTS.md.no-auto-inject"));
        assert!(!home.path(profile, "AGENTS.md").exists());
    }
    assert_eq!(
        fs::read_to_string(home.home.join(".codex-regular-file")).unwrap(),
        "not a profile\n"
    );
}

fn assert_path_exists(path: &Path) {
    assert!(path.exists(), "expected {} to exist", path.display());
}
