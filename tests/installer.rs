#![cfg(target_os = "linux")]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct Installation {
    home: PathBuf,
}

impl Installation {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "agent-instructions-installer-%U-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(home.join(".codex")).expect("create Codex home");
        fs::create_dir_all(home.join(".claude")).expect("create Claude home");
        fs::write(home.join(".codex/AGENTS.md"), "codex instructions\n")
            .expect("write Codex instructions");
        fs::write(home.join(".claude/CLAUDE.md"), "claude instructions\n")
            .expect("write Claude instructions");
        Self { home }
    }

    fn data_home(&self) -> PathBuf {
        self.home.join("data")
    }

    fn config_home(&self) -> PathBuf {
        self.home.join("config")
    }

    fn install(&self) -> std::process::Output {
        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .arg("--binary")
            .arg(env!("CARGO_BIN_EXE_agent-instructions"))
            .arg("--no-refresh")
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.data_home())
            .env("XDG_CONFIG_HOME", self.config_home())
            .output()
            .expect("run installer")
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        if self.home.starts_with(std::env::temp_dir()) {
            fs::remove_dir_all(&self.home).expect("remove isolated installation");
        }
    }
}

#[test]
fn installer_copies_the_binary_and_extends_the_existing_status_line() {
    let installation = Installation::new();
    let base_status = installation.home.join(".claude/existing-status.sh");
    fs::write(
        &base_status,
        "payload=$(cat)\ncase \"$payload\" in *demo*) printf 'MODEL:Opus repo:demo\\n' ;; *) exit 3 ;; esac\n",
    )
    .expect("write existing status line");
    fs::write(
        installation.home.join(".claude/settings.json"),
        format!(
            "{{\n  \"theme\": \"dark\",\n  \"statusLine\": {{\"type\": \"command\", \"command\": \"bash {} | sed 's/repo/repo/'\"}}\n}}\n",
            base_status.display()
        ),
    )
    .expect("write Claude settings");

    let output = installation.install();
    assert!(
        output.status.success(),
        "installer stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let binary = installation.home.join(".local/bin/agent-instructions");
    let application = installation
        .data_home()
        .join("applications/agent-instructions-toggle.desktop");
    let autostart = installation
        .config_home()
        .join("autostart/agent-instructions-tray.desktop");
    assert!(binary.exists());
    assert!(
        fs::read_to_string(&application)
            .unwrap()
            .contains("X-KDE-Shortcuts=Meta+Ctrl+Shift+A")
    );
    assert!(fs::read_to_string(&application).unwrap().contains("%%U"));
    assert!(
        fs::read_to_string(&autostart)
            .unwrap()
            .contains("agent-instructions\" tray")
    );

    let settings = fs::read_to_string(installation.home.join(".claude/settings.json")).unwrap();
    assert!(settings.contains("\"theme\": \"dark\""));
    assert!(settings.contains("claude-status-line.py"));

    let first_status = run_status_line(&installation);
    assert!(first_status.status.success());
    assert_eq!(
        String::from_utf8(first_status.stdout).unwrap(),
        "MODEL:Opus repo:demo AGENTS:on\n"
    );

    let reinstall = installation.install();
    assert!(reinstall.status.success());
    let second_status = run_status_line(&installation);
    assert_eq!(
        String::from_utf8(second_status.stdout).unwrap(),
        "MODEL:Opus repo:demo AGENTS:on\n"
    );

    let codex = installation.home.join(".codex/AGENTS.md");
    let codex_disabled = installation.home.join(".codex/AGENTS.md.no-auto-inject");
    fs::rename(&codex, &codex_disabled).expect("make state mixed");
    assert_status_line(&installation, "mixed");

    fs::write(&codex, "colliding instructions\n").expect("make state conflict");
    assert_status_line(&installation, "conflict");

    fs::remove_file(&codex).expect("remove colliding copy");
    fs::rename(
        installation.home.join(".claude/CLAUDE.md"),
        installation.home.join(".claude/CLAUDE.md.no-auto-inject"),
    )
    .expect("make state off");
    assert_status_line(&installation, "off");

    validate_desktop_file(&application);
    validate_desktop_file(&autostart);
}

#[test]
fn installer_refuses_invalid_claude_settings_without_overwriting_them() {
    let installation = Installation::new();
    let settings = installation.home.join(".claude/settings.json");
    fs::write(&settings, "{not valid JSON\n").expect("write invalid settings");

    let output = installation.install();

    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(settings).unwrap(), "{not valid JSON\n");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("could not install desktop integration"));
    assert!(!stderr.contains("Traceback"));
}

fn assert_status_line(installation: &Installation, expected_state: &str) {
    let output = run_status_line(installation);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("MODEL:Opus repo:demo AGENTS:{expected_state}\n")
    );
}

fn run_status_line(installation: &Installation) -> std::process::Output {
    let wrapper = installation
        .data_home()
        .join("agent-instructions/claude-status-line.py");
    let mut child = Command::new("python3")
        .arg(wrapper)
        .env("HOME", &installation.home)
        .env("XDG_RUNTIME_DIR", installation.home.join("runtime"))
        .env("XDG_STATE_HOME", installation.home.join("state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start installed status line");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"workspace":{"current_dir":"demo"}}"#)
        .expect("write representative Claude input");
    child.wait_with_output().expect("wait for status line")
}

fn validate_desktop_file(path: &Path) {
    let available = Command::new("desktop-file-validate")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if available.is_ok() {
        let status = Command::new("desktop-file-validate")
            .arg(path)
            .status()
            .expect("run desktop-file-validate");
        assert!(status.success(), "invalid desktop file: {}", path.display());
    }
}
