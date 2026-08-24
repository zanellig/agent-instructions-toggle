use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDesktop {
    root: PathBuf,
    home: PathBuf,
    runtime: PathBuf,
    state: PathBuf,
    bin: PathBuf,
    notifications: PathBuf,
}

impl TestDesktop {
    fn new(notification_exit_code: i32) -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agent-instructions-desktop-test-{}-{sequence}",
            std::process::id()
        ));
        let home = root.join("home");
        let runtime = root.join("runtime");
        let state = root.join("state");
        let bin = root.join("bin");
        let notifications = root.join("notifications");
        for directory in [&home, &runtime, &state, &bin] {
            fs::create_dir_all(directory).unwrap();
        }

        let notify_send = bin.join("notify-send");
        fs::write(
            &notify_send,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nexit {notification_exit_code}\n",
                notifications.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&notify_send, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            root,
            home,
            runtime,
            state,
            bin,
            notifications,
        }
    }

    fn write_enabled(&self, target: &str, filename: &str) {
        let directory = self.home.join(target);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(filename), "instructions\n").unwrap();
    }

    fn write_disabled(&self, target: &str, filename: &str) {
        let directory = self.home.join(target);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{filename}.no-auto-inject")),
            "instructions\n",
        )
        .unwrap();
    }

    fn create_target(&self, target: &str) {
        fs::create_dir_all(self.home.join(target)).unwrap();
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-instructions"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_STATE_HOME", &self.state)
            .env("PATH", &self.bin)
            .output()
            .unwrap()
    }
}

impl Drop for TestDesktop {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn notified_toggle_reports_the_completed_transition() {
    let desktop = TestDesktop::new(0);
    desktop.write_enabled(".codex-work", "AGENTS.md");

    let output = desktop.command(&["toggle", "--notify"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_path_exists(&desktop.home.join(".codex-work/AGENTS.md.no-auto-inject"));
    let notification = fs::read_to_string(&desktop.notifications).unwrap();
    assert!(
        notification.contains("Agent instructions disabled"),
        "{notification}"
    );
    assert!(
        notification.contains("New coding-agent contexts will start without global instructions."),
        "{notification}"
    );
}

#[test]
fn notification_failure_does_not_change_a_completed_transition() {
    let desktop = TestDesktop::new(1);
    desktop.write_enabled(".codex-work", "AGENTS.md");

    let output = desktop.command(&["toggle", "--notify"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_path_exists(&desktop.home.join(".codex-work/AGENTS.md.no-auto-inject"));
}

#[test]
fn notified_transition_names_missing_managed_targets() {
    let desktop = TestDesktop::new(0);
    desktop.write_enabled(".codex-work", "AGENTS.md");
    desktop.create_target(".codex-empty");

    let output = desktop.command(&["disable", "--notify"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let notification = fs::read_to_string(&desktop.notifications).unwrap();
    assert!(
        notification.contains("Missing managed targets: .codex-empty/AGENTS.md."),
        "{notification}"
    );
}

#[test]
fn notified_mixed_state_reports_recovery_without_claiming_the_requested_action() {
    let desktop = TestDesktop::new(0);
    desktop.write_enabled(".codex-alpha", "AGENTS.md");
    desktop.write_disabled(".codex-zeta", "AGENTS.md");

    let output = desktop.command(&["disable", "--notify"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_path_exists(&desktop.home.join(".codex-zeta/AGENTS.md"));
    let notification = fs::read_to_string(&desktop.notifications).unwrap();
    assert!(
        notification.contains("Agent instructions recovered"),
        "{notification}"
    );
    assert!(
        notification.contains(
            "Mixed instruction state was restored to on. Press the shortcut again to disable global instructions for new contexts."
        ),
        "{notification}"
    );
}

#[test]
fn notified_conflict_reports_that_instruction_files_were_unchanged() {
    let desktop = TestDesktop::new(0);
    desktop.write_enabled(".codex-work", "AGENTS.md");
    desktop.write_disabled(".codex-work", "AGENTS.md");

    let output = desktop.command(&["toggle", "--notify"]);

    assert!(!output.status.success());
    assert_path_exists(&desktop.home.join(".codex-work/AGENTS.md"));
    assert_path_exists(&desktop.home.join(".codex-work/AGENTS.md.no-auto-inject"));
    let notification = fs::read_to_string(&desktop.notifications).unwrap();
    assert!(
        notification.contains("Agent instructions unchanged"),
        "{notification}"
    );
    assert!(
        notification.contains("both names exist for .codex-work/AGENTS.md"),
        "{notification}"
    );
}

#[test]
fn notified_operational_failure_reports_that_no_transition_completed() {
    let desktop = TestDesktop::new(0);
    fs::remove_dir_all(&desktop.home).unwrap();

    let output = desktop.command(&["toggle", "--notify"]);

    assert!(!output.status.success());
    let notification = fs::read_to_string(&desktop.notifications).unwrap();
    assert!(
        notification.contains("Agent instructions unchanged"),
        "{notification}"
    );
    assert!(
        notification.contains("cannot inspect home directory"),
        "{notification}"
    );
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn assert_path_exists(path: &Path) {
    assert!(path.exists(), "expected {} to exist", path.display());
}
