#![cfg(target_os = "linux")]

use std::process::Command;

#[test]
#[ignore = "requires the desktop entry to be installed in a live KDE Plasma session"]
fn kglobalaccel_has_the_installed_shortcut() {
    let component = qdbus(&[
        "org.kde.kglobalaccel",
        "/kglobalaccel",
        "org.kde.KGlobalAccel.getComponent",
        "agent-instructions-toggle.desktop",
    ]);
    let component = component.trim();
    assert!(
        component.starts_with("/component/"),
        "shortcut component was not registered: {component}"
    );

    let shortcuts = qdbus(&[
        "--literal",
        "org.kde.kglobalaccel",
        component,
        "org.kde.kglobalaccel.Component.allShortcutInfos",
    ]);
    assert!(shortcuts.contains("agent-instructions-toggle.desktop"));
    assert!(
        shortcuts.contains("369098817"),
        "Meta+Ctrl+Shift+A was not registered: {shortcuts}"
    );
}

fn qdbus(arguments: &[&str]) -> String {
    let output = Command::new("qdbus6")
        .args(arguments)
        .output()
        .expect("run qdbus6");
    assert!(
        output.status.success(),
        "qdbus6 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("D-Bus response is UTF-8")
}
