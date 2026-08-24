use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);
const DESKTOP_FILE: &str = "io.github.zanellig.agent-instructions.desktop";

struct TestInstall {
    root: PathBuf,
    home: PathBuf,
    bin_home: PathBuf,
    data_home: PathBuf,
    target: PathBuf,
    tools: PathBuf,
    cargo_log: PathBuf,
    metadata_log: PathBuf,
}

impl TestInstall {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agent-instructions-installer-test-{}-{sequence}",
            std::process::id()
        ));
        let home = root.join("home");
        let bin_home = root.join("user bin");
        let data_home = root.join("user-data");
        let target = root.join("target");
        let tools = root.join("tools");
        let cargo_log = root.join("cargo.log");
        let metadata_log = root.join("metadata.log");
        for directory in [&home, &tools] {
            fs::create_dir_all(directory).unwrap();
        }

        let cargo = tools.join("cargo");
        fs::write(
            &cargo,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nmkdir -p \"$CARGO_TARGET_DIR/release\"\nprintf 'release-binary\\n' > \"$CARGO_TARGET_DIR/release/agent-instructions\"\nchmod 755 \"$CARGO_TARGET_DIR/release/agent-instructions\"\n",
                cargo_log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();

        let refresher = tools.join("kbuildsycoca6");
        fs::write(
            &refresher,
            format!(
                "#!/bin/sh\nprintf 'refreshed\\n' >> \"{}\"\n",
                metadata_log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&refresher, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            root,
            home,
            bin_home,
            data_home,
            target,
            tools,
            cargo_log,
            metadata_log,
        }
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_BIN_HOME", &self.bin_home)
            .env("XDG_DATA_HOME", &self.data_home)
            .env("CARGO_TARGET_DIR", &self.target)
            .env("CARGO", self.tools.join("cargo"))
            .env("KBUILDSYCOCA", self.tools.join("kbuildsycoca6"))
            .output()
            .unwrap()
    }

    fn application_entry(&self) -> PathBuf {
        self.data_home.join("applications").join(DESKTOP_FILE)
    }

    fn kglobalaccel_entry(&self) -> PathBuf {
        self.data_home.join("kglobalaccel").join(DESKTOP_FILE)
    }
}

impl Drop for TestInstall {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn installer_copies_release_binary_and_registers_the_plasma_shortcut_idempotently() {
    let install = TestInstall::new();
    let unrelated_application = install.data_home.join("applications/keep.desktop");
    let unrelated_shortcut = install.data_home.join("kglobalaccel/keep.desktop");
    fs::create_dir_all(unrelated_application.parent().unwrap()).unwrap();
    fs::create_dir_all(unrelated_shortcut.parent().unwrap()).unwrap();
    fs::write(&unrelated_application, "keep application\n").unwrap();
    fs::write(&unrelated_shortcut, "keep shortcut\n").unwrap();

    let first = install.command(&[]);

    assert!(first.status.success(), "{}", stderr(&first));
    let installed_binary = install.bin_home.join("agent-instructions");
    assert_eq!(
        fs::read_to_string(&installed_binary).unwrap(),
        "release-binary\n"
    );
    assert!(
        !fs::symlink_metadata(&installed_binary)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_ne!(
        fs::metadata(&installed_binary)
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert_eq!(
        fs::read_to_string(&install.cargo_log).unwrap(),
        "build\n--release\n--locked\n"
    );

    let application_entry = fs::read_to_string(install.application_entry()).unwrap();
    assert_eq!(
        application_entry,
        fs::read_to_string(install.kglobalaccel_entry()).unwrap()
    );
    assert!(
        application_entry.contains(&format!(
            "Exec=\"{}\" toggle --notify",
            installed_binary.display()
        )),
        "{application_entry}"
    );
    assert!(
        application_entry.contains("X-KDE-Shortcuts=Meta+Ctrl+Shift+A"),
        "{application_entry}"
    );
    validate_desktop_entry(&install.application_entry());
    validate_desktop_entry(&install.kglobalaccel_entry());
    assert_eq!(
        fs::read_to_string(&unrelated_application).unwrap(),
        "keep application\n"
    );
    assert_eq!(
        fs::read_to_string(&unrelated_shortcut).unwrap(),
        "keep shortcut\n"
    );

    fs::write(&installed_binary, "stale\n").unwrap();
    let second = install.command(&[]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(
        fs::read_to_string(&installed_binary).unwrap(),
        "release-binary\n"
    );
    assert_eq!(
        fs::read_to_string(&install.metadata_log).unwrap(),
        "refreshed\nrefreshed\n"
    );
}

#[test]
fn uninstaller_removes_only_owned_artifacts_and_refreshes_plasma_metadata() {
    let install = TestInstall::new();
    let installed = install.command(&[]);
    assert!(installed.status.success(), "{}", stderr(&installed));
    let unrelated_application = install.data_home.join("applications/keep.desktop");
    let unrelated_shortcut = install.data_home.join("kglobalaccel/keep.desktop");
    fs::write(&unrelated_application, "keep application\n").unwrap();
    fs::write(&unrelated_shortcut, "keep shortcut\n").unwrap();

    let removed = install.command(&["--uninstall"]);

    assert!(removed.status.success(), "{}", stderr(&removed));
    assert!(!install.bin_home.join("agent-instructions").exists());
    assert!(!install.application_entry().exists());
    assert!(!install.kglobalaccel_entry().exists());
    assert_eq!(
        fs::read_to_string(&unrelated_application).unwrap(),
        "keep application\n"
    );
    assert_eq!(
        fs::read_to_string(&unrelated_shortcut).unwrap(),
        "keep shortcut\n"
    );
    assert_eq!(
        fs::read_to_string(&install.cargo_log).unwrap(),
        "build\n--release\n--locked\n"
    );
    assert_eq!(
        fs::read_to_string(&install.metadata_log).unwrap(),
        "refreshed\nrefreshed\n"
    );
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn validate_desktop_entry(path: &Path) {
    let validation = match Command::new("desktop-file-validate").arg(path).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("cannot run desktop-file-validate: {error}"),
    };
    assert!(
        validation.status.success(),
        "{}",
        String::from_utf8(validation.stderr).unwrap()
    );
}
