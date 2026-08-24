#![cfg(target_os = "linux")]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct LiveTray {
    child: Child,
    home: PathBuf,
}

impl LiveTray {
    fn start() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "agent-instructions-live-tray-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(home.join(".codex")).expect("create Codex profile");
        fs::create_dir_all(home.join(".claude")).expect("create Claude profile");
        fs::write(home.join(".codex/AGENTS.md"), "codex instructions\n")
            .expect("write Codex instructions");
        fs::write(home.join(".claude/CLAUDE.md"), "claude instructions\n")
            .expect("write Claude instructions");
        let child = tray_command(&home)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start tray process");
        Self { child, home }
    }

    fn service_name(&self) -> String {
        let prefix = format!("org.kde.StatusNotifierItem-{}-", self.child.id());
        wait_for(Duration::from_secs(5), || {
            let output = Command::new("busctl")
                .args([
                    "--user",
                    "get-property",
                    "org.kde.StatusNotifierWatcher",
                    "/StatusNotifierWatcher",
                    "org.kde.StatusNotifierWatcher",
                    "RegisteredStatusNotifierItems",
                ])
                .output()
                .ok()?;
            String::from_utf8(output.stdout)
                .ok()?
                .split_whitespace()
                .map(|value| value.trim_matches('"'))
                .find(|value| value.starts_with(&prefix))
                .and_then(|value| value.split('/').next())
                .map(str::to_owned)
        })
    }
}

impl Drop for LiveTray {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if self.home.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }
}

#[test]
#[ignore = "requires a live D-Bus session and StatusNotifier host"]
fn live_status_notifier_contract() {
    let tray = LiveTray::start();
    let service = tray.service_name();

    let tooltip = property(&service, "ToolTip");
    assert!(tooltip.contains("AGENTS: on"), "tooltip: {tooltip}");
    assert!(property(&service, "ItemIsMenu").contains("true"));
    let menu = Command::new("busctl")
        .args([
            "--user",
            "call",
            &service,
            "/MenuBar",
            "com.canonical.dbusmenu",
            "GetLayout",
            "iias",
            "--",
            "0",
            "-1",
            "0",
        ])
        .output()
        .expect("query tray menu");
    assert!(
        menu.status.success(),
        "menu query failed: {}",
        String::from_utf8_lossy(&menu.stderr)
    );
    let menu = String::from_utf8(menu.stdout).expect("menu response is UTF-8");
    for label in ["Enable", "Disable", "Status", "Quit"] {
        assert!(menu.contains(label), "menu omitted {label}: {menu}");
    }

    let second = tray_command(&tray.home)
        .output()
        .expect("try to start a second tray");
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));

    fs::create_dir(tray.home.join(".codex_work")).expect("add a discovered profile");
    wait_for(Duration::from_secs(5), || {
        property(&service, "ToolTip")
            .contains(".codex_work/AGENTS.md")
            .then_some(())
    });
    fs::write(
        tray.home.join(".codex_work/AGENTS.md.no-auto-inject"),
        "disabled work instructions\n",
    )
    .expect("make discovered profile disabled");
    wait_for(Duration::from_secs(5), || {
        property(&service, "ToolTip")
            .contains("AGENTS: mixed")
            .then_some(())
    });
}

fn tray_command(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-instructions"));
    command
        .arg("tray")
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("runtime"))
        .env("XDG_STATE_HOME", home.join("state"));
    command
}

fn property(service: &str, property: &str) -> String {
    let output = Command::new("busctl")
        .args([
            "--user",
            "get-property",
            service,
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
            property,
        ])
        .output()
        .expect("query tray property");
    assert!(
        output.status.success(),
        "property query failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("property response is UTF-8")
}

fn wait_for<T>(timeout: Duration, mut operation: impl FnMut() -> Option<T>) -> T {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(value) = operation() {
            return value;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("condition was not met within {timeout:?}");
}
