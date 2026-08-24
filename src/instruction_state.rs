use std::env;
use std::ffi::CString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::raw::{c_char, c_int, c_uint};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const BACKUP_TOKENS: [&[u8]; 8] = [
    b"bak",
    b"backup",
    b"old",
    b"orig",
    b"copy",
    b"archive",
    b"save",
    b"disabled",
];

const AT_FDCWD: c_int = -100;
const RENAME_NOREPLACE: c_uint = 1;

unsafe extern "C" {
    fn renameat2(
        old_directory: c_int,
        old_path: *const c_char,
        new_directory: c_int,
        new_path: *const c_char,
        flags: c_uint,
    ) -> c_int;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstructionState {
    On,
    Off,
    Mixed,
    Conflict,
}

impl fmt::Display for InstructionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Mixed => "mixed",
            Self::Conflict => "conflict",
        })
    }
}

#[derive(Debug)]
pub struct Inspection {
    pub state: InstructionState,
    pub missing_targets: Vec<String>,
    pub collision_targets: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Enable,
    Disable,
    Toggle,
}

#[derive(Debug)]
pub struct ApplyResult {
    pub inspection: Inspection,
    pub recovered_mixed_state: bool,
}

#[derive(Debug)]
pub struct Error {
    message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub fn inspect() -> Result<Inspection, Error> {
    let home = home_directory()?;
    Ok(inspect_at(&home)?.inspection)
}

pub fn apply(action: Action) -> Result<ApplyResult, Error> {
    let home = home_directory()?;
    let _lock = acquire_lock(&home)?;
    let snapshot = inspect_at(&home)?;

    if snapshot.inspection.state == InstructionState::Conflict {
        let message = if snapshot.inspection.collision_targets.is_empty() {
            "cannot change instruction state: every managed target is missing".to_owned()
        } else {
            format!(
                "cannot change instruction state: both names exist for {}",
                snapshot.inspection.collision_targets.join(", ")
            )
        };
        return Err(Error { message });
    }

    let recovered_mixed_state = snapshot.inspection.state == InstructionState::Mixed;
    let desired_state = if recovered_mixed_state {
        InstructionState::On
    } else {
        match action {
            Action::Enable => InstructionState::On,
            Action::Disable => InstructionState::Off,
            Action::Toggle if snapshot.inspection.state == InstructionState::On => {
                InstructionState::Off
            }
            Action::Toggle => InstructionState::On,
        }
    };

    rename_targets(&snapshot.targets, desired_state)?;

    Ok(ApplyResult {
        inspection: Inspection {
            state: desired_state,
            missing_targets: snapshot.inspection.missing_targets,
            collision_targets: Vec::new(),
        },
        recovered_mixed_state,
    })
}

fn inspect_at(home: &Path) -> Result<Snapshot, Error> {
    let mut has_enabled = false;
    let mut has_disabled = false;
    let mut has_collision = false;
    let mut missing_targets = Vec::new();
    let mut collision_targets = Vec::new();
    let discovered_targets = discover_targets(home)?;
    let mut targets = Vec::with_capacity(discovered_targets.len());

    for target in discovered_targets {
        let presence = target.presence()?;
        match presence {
            Presence::Enabled => has_enabled = true,
            Presence::Disabled => has_disabled = true,
            Presence::Collision => {
                has_collision = true;
                collision_targets.push(target.label.clone());
            }
            Presence::Missing => missing_targets.push(target.label.clone()),
        }
        targets.push((target, presence));
    }

    let state = if has_collision || (!has_enabled && !has_disabled) {
        InstructionState::Conflict
    } else if has_enabled && has_disabled {
        InstructionState::Mixed
    } else if has_enabled {
        InstructionState::On
    } else {
        InstructionState::Off
    };

    Ok(Snapshot {
        inspection: Inspection {
            state,
            missing_targets,
            collision_targets,
        },
        targets,
    })
}

fn discover_targets(home: &Path) -> Result<Vec<Target>, Error> {
    let entries = fs::read_dir(home).map_err(|error| Error {
        message: format!("cannot inspect home directory {}: {error}", home.display()),
    })?;
    let mut codex_targets = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| Error {
            message: format!("cannot inspect an entry under {}: {error}", home.display()),
        })?;
        let name = entry.file_name();
        if !is_codex_profile_name(&name) || is_backup_like_name(&name) {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| Error {
            message: format!("cannot inspect {}: {error}", entry.path().display()),
        })?;
        let is_directory = if file_type.is_dir() {
            true
        } else if file_type.is_symlink() {
            match fs::metadata(entry.path()) {
                Ok(metadata) => metadata.is_dir(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => {
                    return Err(Error {
                        message: format!("cannot inspect {}: {error}", entry.path().display()),
                    });
                }
            }
        } else {
            false
        };
        if is_directory {
            codex_targets.push(Target::new(
                entry.path(),
                "AGENTS.md",
                format!("{}/AGENTS.md", name.to_string_lossy()),
            ));
        }
    }

    codex_targets.sort_by(|left, right| left.directory.cmp(&right.directory));
    codex_targets.push(Target::new(
        home.join(".claude"),
        "CLAUDE.md",
        ".claude/CLAUDE.md".to_owned(),
    ));
    Ok(codex_targets)
}

fn is_codex_profile_name(name: &std::ffi::OsStr) -> bool {
    name.as_bytes().starts_with(b".codex")
}

fn is_backup_like_name(name: &std::ffi::OsStr) -> bool {
    let name = name.as_bytes();
    name.ends_with(b"~")
        || name
            .split(|byte| !byte.is_ascii_alphanumeric())
            .any(is_backup_token)
}

fn is_backup_token(segment: &[u8]) -> bool {
    BACKUP_TOKENS.iter().any(|token| {
        segment.len() >= token.len()
            && segment[..token.len()].eq_ignore_ascii_case(token)
            && segment[token.len()..].iter().all(u8::is_ascii_digit)
    })
}

fn acquire_lock(home: &Path) -> Result<File, Error> {
    let runtime_error = env::var_os("XDG_RUNTIME_DIR").map(|directory| {
        let path = PathBuf::from(directory).join("agent-instructions.lock");
        open_and_lock(&path)
    });

    if let Some(Ok(lock)) = runtime_error {
        return Ok(lock);
    }

    let state_directory = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("agent-instructions");
    fs::create_dir_all(&state_directory).map_err(|error| Error {
        message: format!(
            "cannot create state directory {}: {error}",
            state_directory.display()
        ),
    })?;
    let fallback_path = state_directory.join("operation.lock");
    open_and_lock(&fallback_path).map_err(|fallback_error| {
        let message = match runtime_error {
            Some(Err(runtime_error)) => format!(
                "cannot acquire operation lock ({runtime_error}; fallback failed: {fallback_error})"
            ),
            _ => format!("cannot acquire operation lock: {fallback_error}"),
        };
        Error { message }
    })
}

fn open_and_lock(path: &Path) -> Result<File, io::Error> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}

fn rename_targets(
    targets: &[(Target, Presence)],
    desired_state: InstructionState,
) -> Result<(), Error> {
    let mut renames = Vec::new();
    for (target, presence) in targets {
        let paths = match (presence, desired_state) {
            (Presence::Enabled, InstructionState::Off) => {
                Some((target.enabled_path(), target.disabled_path()))
            }
            (Presence::Disabled, InstructionState::On) => {
                Some((target.disabled_path(), target.enabled_path()))
            }
            _ => None,
        };
        if let Some((source, destination)) = paths {
            renames.push(Rename {
                source,
                destination,
            });
        }
    }

    for rename in &renames {
        if !path_exists(&rename.source)? {
            return Err(Error {
                message: format!(
                    "cannot change instruction state: source {} disappeared during preflight",
                    rename.source.display()
                ),
            });
        }
        if path_exists(&rename.destination)? {
            return Err(Error {
                message: format!(
                    "cannot change instruction state: destination {} already exists",
                    rename.destination.display()
                ),
            });
        }
    }

    let mut completed: Vec<&Rename> = Vec::new();
    for rename in &renames {
        if let Err(error) = rename_no_replace(&rename.source, &rename.destination) {
            let mut rollback_failures = Vec::new();
            for completed_rename in completed.into_iter().rev() {
                if let Err(rollback_error) =
                    rename_no_replace(&completed_rename.destination, &completed_rename.source)
                {
                    rollback_failures.push(format!(
                        "{} to {}: {rollback_error}",
                        completed_rename.destination.display(),
                        completed_rename.source.display()
                    ));
                }
            }
            let rollback = if rollback_failures.is_empty() {
                "completed renames were rolled back".to_owned()
            } else {
                format!("rollback also failed: {}", rollback_failures.join("; "))
            };
            return Err(Error {
                message: format!(
                    "cannot rename {} to {}: {error}; {rollback}",
                    rename.source.display(),
                    rename.destination.display()
                ),
            });
        }
        completed.push(rename);
    }

    Ok(())
}

fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), io::Error> {
    let old_path = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains a NUL"))?;
    let new_path = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination path contains a NUL",
        )
    })?;

    // SAFETY: both C strings remain alive for the call and contain terminating NUL bytes.
    let result = unsafe {
        renameat2(
            AT_FDCWD,
            old_path.as_ptr(),
            AT_FDCWD,
            new_path.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

struct Snapshot {
    inspection: Inspection,
    targets: Vec<(Target, Presence)>,
}

struct Rename {
    source: PathBuf,
    destination: PathBuf,
}

fn home_directory() -> Result<PathBuf, Error> {
    env::var_os("HOME").map(PathBuf::from).ok_or_else(|| Error {
        message: "HOME is not set".to_owned(),
    })
}

#[derive(Clone)]
struct Target {
    directory: PathBuf,
    filename: &'static str,
    label: String,
}

impl Target {
    fn new(directory: PathBuf, filename: &'static str, label: String) -> Self {
        Self {
            directory,
            filename,
            label,
        }
    }

    fn enabled_path(&self) -> PathBuf {
        self.directory.join(self.filename)
    }

    fn disabled_path(&self) -> PathBuf {
        self.directory
            .join(format!("{}.no-auto-inject", self.filename))
    }

    fn presence(&self) -> Result<Presence, Error> {
        let enabled = path_exists(&self.enabled_path())?;
        let disabled = path_exists(&self.disabled_path())?;
        Ok(match (enabled, disabled) {
            (true, false) => Presence::Enabled,
            (false, true) => Presence::Disabled,
            (true, true) => Presence::Collision,
            (false, false) => Presence::Missing,
        })
    }
}

fn path_exists(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error {
            message: format!("cannot inspect {}: {error}", path.display()),
        }),
    }
}

#[derive(Clone, Copy)]
enum Presence {
    Enabled,
    Disabled,
    Collision,
    Missing,
}
