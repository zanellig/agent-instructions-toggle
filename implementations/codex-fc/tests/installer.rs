use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);
const DESKTOP_FILE: &str = "io.github.zanellig.agent-instructions.desktop";

struct TestInstall {
    root: PathBuf,
    home: PathBuf,
    bin_home: PathBuf,
    data_home: PathBuf,
    build_directory: PathBuf,
    tools: PathBuf,
    cargo_log: PathBuf,
    metadata_log: PathBuf,
    status_log: PathBuf,
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
        let build_directory = root.join("target");
        let tools = root.join("tools");
        let cargo_log = root.join("cargo.log");
        let metadata_log = root.join("metadata.log");
        let status_log = root.join("status.log");
        for directory in [&home, &tools] {
            fs::create_dir_all(directory).unwrap();
        }

        let cargo = tools.join("cargo");
        fs::write(
            &cargo,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nmkdir -p \"$CARGO_TARGET_DIR/release\"\nprintf '%s\\n' '#!/bin/bash' 'if [[ \"$1 $2 $3\" == \"profiles --claude --null\" ]]; then' '  for profile in \"$HOME\"/.claude*; do' '    [[ -d \"$profile\" ]] && printf \"%s\\0\" \"$profile\"' '  done' 'elif [[ \"$1 $2\" == \"status --machine\" ]]; then' '  printf \"%s\\n\" \"${{AGENT_INSTRUCTIONS_TEST_STATE:-on}}\"' 'elif [[ \"$1 $2\" == \"status --segment\" ]]; then' '  case \"${{AGENT_INSTRUCTIONS_TEST_STATE:-on}}\" in' '    on) color=32 ;;' '    off) color=90 ;;' '    mixed) color=33 ;;' '    conflict) color=31 ;;' '  esac' '  printf \"\\033[%smAGENTS:%s\\033[0m\\n\" \"$color\" \"${{AGENT_INSTRUCTIONS_TEST_STATE:-on}}\"' 'fi' 'if [[ \"$1\" == status && -n \"${{AGENT_INSTRUCTIONS_TEST_WARNING:-}}\" ]]; then' '  printf \"Warning: %s\\n\" \"$AGENT_INSTRUCTIONS_TEST_WARNING\" >&2' 'fi' 'exit 0' > \"$CARGO_TARGET_DIR/release/agent-instructions\"\nchmod 755 \"$CARGO_TARGET_DIR/release/agent-instructions\"\n",
                cargo_log.display(),
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
            build_directory,
            tools,
            cargo_log,
            metadata_log,
            status_log,
        }
    }

    fn command(&self, args: &[&str]) -> Output {
        Command::new("bash")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_BIN_HOME", &self.bin_home)
            .env("XDG_DATA_HOME", &self.data_home)
            .env("CARGO_TARGET_DIR", &self.build_directory)
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

    fn autostart_entry(&self) -> PathBuf {
        self.data_home.join("autostart").join(DESKTOP_FILE)
    }

    fn claude_profile(&self, name: &str) -> PathBuf {
        self.home.join(name)
    }

    fn status_wrapper(&self, name: &str) -> PathBuf {
        self.claude_profile(name)
            .join("agent-instructions-statusline.sh")
    }

    fn status_metadata(&self, name: &str) -> PathBuf {
        self.claude_profile(name)
            .join(".agent-instructions-statusline.json")
    }

    fn command_with_preexisting_temp_symlinks(&self, profile_name: &str, victim: &Path) -> Output {
        let profile = self.claude_profile(profile_name);
        let settings = profile.join("settings.json");
        let metadata = self.status_metadata(profile_name);
        let wrapper = self.status_wrapper(profile_name);
        let mut command = Command::new("bash");
        command
            .arg("-c")
            .arg(
                r#"ln -s -- "$1" "$2.tmp.$$"
ln -s -- "$1" "$3.tmp.$$"
ln -s -- "$1" "$4.tmp.$$"
exec bash "$5""#,
            )
            .arg("temp-symlink-test")
            .arg(victim)
            .arg(settings)
            .arg(metadata)
            .arg(wrapper)
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
            .env("HOME", &self.home)
            .env("XDG_BIN_HOME", &self.bin_home)
            .env("XDG_DATA_HOME", &self.data_home)
            .env("CARGO_TARGET_DIR", &self.build_directory)
            .env("CARGO", self.tools.join("cargo"))
            .env("KBUILDSYCOCA", self.tools.join("kbuildsycoca6"));
        command.output().unwrap()
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
    assert!(
        fs::read_to_string(&installed_binary)
            .unwrap()
            .contains("status --machine")
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
    let autostart_entry = fs::read_to_string(install.autostart_entry()).unwrap();
    assert!(
        autostart_entry.contains(&format!("Exec=\"{}\" tray", installed_binary.display())),
        "{autostart_entry}"
    );
    assert!(autostart_entry.contains("Name=Agent Instructions Tray"));
    validate_desktop_entry(&install.autostart_entry());
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
    assert!(
        fs::read_to_string(&installed_binary)
            .unwrap()
            .contains("status --machine")
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
    assert!(!install.autostart_entry().exists());
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

#[test]
fn installer_extends_and_restores_each_claude_status_line_idempotently() {
    let install = TestInstall::new();
    let profile = install.claude_profile(".claude-work");
    fs::create_dir_all(&profile).unwrap();
    let existing_status = profile.join("existing-status.sh");
    fs::write(
        &existing_status,
        format!(
            "#!/bin/sh\ncat > \"{}\"\nprintf 'branch:main | ctx:42\\n'\n",
            install.status_log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&existing_status, fs::Permissions::from_mode(0o755)).unwrap();
    let settings = profile.join("settings.json");
    fs::write(
        &settings,
        format!(
            "{{\n  \"theme\": \"dark\",\n  \"statusLine\": {{\n    \"type\": \"command\",\n    \"command\": \"{}\",\n    \"padding\": 1\n  }}\n}}\n",
            existing_status.display()
        ),
    )
    .unwrap();

    let first = install.command(&[]);

    assert!(first.status.success(), "{}", stderr(&first));
    assert!(install.status_wrapper(".claude-work").exists());
    assert!(install.status_metadata(".claude-work").exists());
    let wrapper_contents = fs::read_to_string(install.status_wrapper(".claude-work")).unwrap();
    assert!(wrapper_contents.contains("status --segment"));
    assert!(!wrapper_contents.contains("case \"$state\""));
    let installed_settings = fs::read_to_string(&settings).unwrap();
    assert!(installed_settings.contains("\"theme\": \"dark\""));
    assert!(installed_settings.contains("agent-instructions-statusline.sh"));

    let second = install.command(&[]);
    assert!(second.status.success(), "{}", stderr(&second));
    let changed_settings = fs::read_to_string(&settings)
        .unwrap()
        .replace("\"padding\": 1", "\"padding\": 2");
    fs::write(&settings, changed_settings).unwrap();

    let mut child = Command::new(install.status_wrapper(".claude-work"))
        .env("HOME", &install.home)
        .env("AGENT_INSTRUCTIONS_TEST_STATE", "on")
        .env(
            "AGENT_INSTRUCTIONS_TEST_WARNING",
            "missing managed targets: .claude-empty/CLAUDE.md",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"cwd":"/tmp/project","model":{"display_name":"Opus"}}"#)
        .unwrap();
    let merged = child.wait_with_output().unwrap();
    assert!(merged.status.success(), "{}", stderr(&merged));
    assert_eq!(
        String::from_utf8(merged.stdout).unwrap(),
        "branch:main | ctx:42 | \u{1b}[32mAGENTS:on\u{1b}[0m \u{1b}[33m[missing managed targets: .claude-empty/CLAUDE.md]\u{1b}[0m\n"
    );
    assert_eq!(
        fs::read_to_string(&install.status_log).unwrap(),
        r#"{"cwd":"/tmp/project","model":{"display_name":"Opus"}}"#
    );

    let removed = install.command(&["--uninstall"]);
    assert!(removed.status.success(), "{}", stderr(&removed));
    assert!(!install.status_wrapper(".claude-work").exists());
    assert!(!install.status_metadata(".claude-work").exists());
    let restored_settings = fs::read_to_string(&settings).unwrap();
    assert!(restored_settings.contains(&existing_status.display().to_string()));
    assert!(restored_settings.contains("\"padding\": 2"));
    assert!(restored_settings.contains("\"theme\": \"dark\""));
}

#[test]
fn installer_uses_secure_sibling_temporary_files() {
    let install = TestInstall::new();
    let profile = install.claude_profile(".claude");
    fs::create_dir_all(&profile).unwrap();
    let victim = install.root.join("must-not-change");
    fs::write(&victim, "sentinel\n").unwrap();

    let installed = install.command_with_preexisting_temp_symlinks(".claude", &victim);

    assert!(installed.status.success(), "{}", stderr(&installed));
    assert_eq!(fs::read_to_string(&victim).unwrap(), "sentinel\n");
    for installed_file in [
        profile.join("settings.json"),
        install.status_metadata(".claude"),
        install.status_wrapper(".claude"),
    ] {
        assert!(installed_file.is_file(), "{}", installed_file.display());
        assert!(
            !fs::symlink_metadata(&installed_file)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{}",
            installed_file.display()
        );
    }
}

#[test]
fn installer_refuses_unsafe_or_invalid_claude_settings_before_installing() {
    for settings_content in [
        r#"{"statusLine":{"type":"command","command":"printf ok | sed s/o/x/"}}"#,
        r#"{"statusLine": "#,
    ] {
        let install = TestInstall::new();
        let profile = install.claude_profile(".claude");
        fs::create_dir_all(&profile).unwrap();
        let settings = profile.join("settings.json");
        fs::write(&settings, settings_content).unwrap();

        let output = install.command(&[]);

        assert!(!output.status.success());
        assert_eq!(fs::read_to_string(&settings).unwrap(), settings_content);
        assert!(!install.bin_home.join("agent-instructions").exists());
        assert!(!install.status_wrapper(".claude").exists());
        assert!(!install.status_metadata(".claude").exists());
        assert!(
            stderr(&output).contains("cannot safely extend")
                || stderr(&output).contains("not a valid JSON object"),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn installer_adds_all_state_colors_and_removes_a_new_status_line_cleanly() {
    let install = TestInstall::new();
    let profile = install.claude_profile(".claude-team");
    fs::create_dir_all(&profile).unwrap();

    let installed = install.command(&[]);

    assert!(installed.status.success(), "{}", stderr(&installed));
    for (state, color) in [("on", 32), ("off", 90), ("mixed", 33), ("conflict", 31)] {
        let output = Command::new(install.status_wrapper(".claude-team"))
            .env("HOME", &install.home)
            .env("AGENT_INSTRUCTIONS_TEST_STATE", state)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("\u{1b}[{color}mAGENTS:{state}\u{1b}[0m\n")
        );
    }

    let removed = install.command(&["--uninstall"]);

    assert!(removed.status.success(), "{}", stderr(&removed));
    assert!(!profile.join("settings.json").exists());
    assert!(!install.status_wrapper(".claude-team").exists());
    assert!(!install.status_metadata(".claude-team").exists());
}

#[test]
fn uninstaller_preserves_a_status_command_changed_after_installation() {
    let install = TestInstall::new();
    let profile = install.claude_profile(".claude");
    fs::create_dir_all(&profile).unwrap();
    let installed = install.command(&[]);
    assert!(installed.status.success(), "{}", stderr(&installed));
    let settings = profile.join("settings.json");
    fs::write(
        &settings,
        r#"{"statusLine":{"type":"command","command":"/user/replacement"}}"#,
    )
    .unwrap();

    let removed = install.command(&["--uninstall"]);

    assert!(removed.status.success(), "{}", stderr(&removed));
    assert_eq!(
        fs::read_to_string(&settings).unwrap(),
        r#"{"statusLine":{"type":"command","command":"/user/replacement"}}"#
    );
    assert!(!install.status_wrapper(".claude").exists());
    assert!(!install.status_metadata(".claude").exists());
}

#[test]
fn uninstaller_refuses_metadata_that_does_not_match_the_install_schema() {
    for invalid_metadata in [
        r#"{"version":1,"hadSettingsFile":false,"hadStatusLine":true,"previousStatusLine":{"type":"command","command":"/prior/status"}}"#,
        r#"{"version":1,"hadSettingsFile":false,"hadStatusLine":true,"previousStatusLine":null}"#,
        r#"{"version":1,"hadSettingsFile":false,"hadStatusLine":false,"previousStatusLine":{"type":"command","command":"/prior/status"}}"#,
        r#"{"version":1,"hadSettingsFile":true,"hadStatusLine":false,"previousStatusLine":{"type":"command","command":"/prior/status"}}"#,
    ] {
        let install = TestInstall::new();
        let profile = install.claude_profile(".claude");
        fs::create_dir_all(&profile).unwrap();
        let installed = install.command(&[]);
        assert!(installed.status.success(), "{}", stderr(&installed));
        let metadata = install.status_metadata(".claude");
        let wrapper = install.status_wrapper(".claude");
        let settings = profile.join("settings.json");
        let settings_before = fs::read_to_string(&settings).unwrap();
        fs::write(&metadata, invalid_metadata).unwrap();

        let removed = install.command(&["--uninstall"]);

        assert!(!removed.status.success(), "{invalid_metadata}");
        assert!(
            stderr(&removed).contains("invalid Claude status integration metadata"),
            "{invalid_metadata}"
        );
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            settings_before,
            "{invalid_metadata}"
        );
        assert!(wrapper.exists(), "{invalid_metadata}");
        assert!(metadata.exists(), "{invalid_metadata}");
        assert!(
            install.bin_home.join("agent-instructions").exists(),
            "{invalid_metadata}"
        );
    }
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
