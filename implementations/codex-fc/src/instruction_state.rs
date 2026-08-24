use std::ffi::CString;
use std::fmt;
use std::fs;
use std::io;
use std::os::raw::{c_char, c_int, c_uint};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::lock;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateAppearance {
    Green,
    Gray,
    Amber,
    Red,
}

impl InstructionState {
    pub fn appearance(self) -> StateAppearance {
        match self {
            Self::On => StateAppearance::Green,
            Self::Off => StateAppearance::Gray,
            Self::Mixed => StateAppearance::Amber,
            Self::Conflict => StateAppearance::Red,
        }
    }
}

impl StateAppearance {
    pub fn argb(self) -> [u8; 4] {
        match self {
            Self::Green => [255, 46, 160, 67],
            Self::Gray => [255, 117, 117, 117],
            Self::Amber => [255, 245, 166, 35],
            Self::Red => [255, 211, 47, 47],
        }
    }

    pub fn ansi_sgr(self) -> u8 {
        match self {
            Self::Green => 32,
            Self::Gray => 90,
            Self::Amber => 33,
            Self::Red => 31,
        }
    }
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

#[derive(Clone, Debug)]
pub struct Inspection {
    pub state: InstructionState,
    pub missing_targets: Vec<String>,
    pub collision_targets: Vec<String>,
    pub claude_profile_directories: Vec<PathBuf>,
    pub watch_locations: WatchLocations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchLocations {
    pub home_directory: PathBuf,
    pub profile_directories: Vec<PathBuf>,
    pub instruction_paths: Vec<PathBuf>,
}

impl WatchLocations {
    pub fn is_relevant_path(&self, path: &Path) -> bool {
        self.instruction_paths
            .iter()
            .any(|candidate| candidate == path)
            || (path.parent() == Some(self.home_directory.as_path())
                && path
                    .file_name()
                    .and_then(ProfileKind::from_profile_name)
                    .is_some())
    }
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
    outcome: FailureOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureOutcome {
    Unchanged,
    MayHaveChanged,
}

impl Error {
    fn unchanged(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: FailureOutcome::Unchanged,
        }
    }

    fn may_have_changed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: FailureOutcome::MayHaveChanged,
        }
    }

    pub fn guarantees_unchanged(&self) -> bool {
        self.outcome == FailureOutcome::Unchanged
    }
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
    let _lock = lock::operation(&home).map_err(Error::unchanged)?;
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
        return Err(Error::unchanged(message));
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

    rename_targets(&snapshot.managed_targets, desired_state)?;

    Ok(ApplyResult {
        inspection: Inspection {
            state: desired_state,
            missing_targets: snapshot.inspection.missing_targets,
            collision_targets: Vec::new(),
            claude_profile_directories: snapshot.inspection.claude_profile_directories,
            watch_locations: snapshot.inspection.watch_locations,
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
    let discovered_managed_targets = discover_managed_targets(home)?;
    let mut managed_targets = Vec::with_capacity(discovered_managed_targets.len());

    for managed_target in discovered_managed_targets {
        let presence = managed_target.presence()?;
        match presence {
            Presence::Enabled => has_enabled = true,
            Presence::Disabled => has_disabled = true,
            Presence::Collision => {
                has_collision = true;
                collision_targets.push(managed_target.display_label.clone());
            }
            Presence::Missing => missing_targets.push(managed_target.display_label.clone()),
        }
        managed_targets.push((managed_target, presence));
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
            claude_profile_directories: managed_targets
                .iter()
                .filter(|(managed_target, _)| managed_target.kind == ProfileKind::Claude)
                .map(|(managed_target, _)| managed_target.directory.clone())
                .collect(),
            watch_locations: WatchLocations {
                home_directory: home.to_owned(),
                profile_directories: managed_targets
                    .iter()
                    .filter(|(managed_target, _)| managed_target.directory.is_dir())
                    .map(|(managed_target, _)| managed_target.directory.clone())
                    .collect(),
                instruction_paths: managed_targets
                    .iter()
                    .flat_map(|(managed_target, _)| {
                        [
                            managed_target.enabled_path(),
                            managed_target.disabled_path(),
                        ]
                    })
                    .collect(),
            },
        },
        managed_targets,
    })
}

fn discover_managed_targets(home: &Path) -> Result<Vec<ManagedTarget>, Error> {
    let entries = fs::read_dir(home).map_err(|error| {
        Error::unchanged(format!(
            "cannot inspect home directory {}: {error}",
            display_path(home)
        ))
    })?;
    let mut managed_targets = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| {
            Error::unchanged(format!(
                "cannot inspect an entry under {}: {error}",
                display_path(home)
            ))
        })?;
        let name = entry.file_name();
        let Some(kind) = ProfileKind::from_profile_name(&name) else {
            continue;
        };
        let file_type = entry.file_type().map_err(|error| {
            Error::unchanged(format!(
                "cannot inspect {}: {error}",
                display_path(&entry.path())
            ))
        })?;
        let is_directory = if file_type.is_dir() {
            true
        } else if file_type.is_symlink() {
            match fs::metadata(entry.path()) {
                Ok(metadata) => metadata.is_dir(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => {
                    return Err(Error::unchanged(format!(
                        "cannot inspect {}: {error}",
                        display_path(&entry.path())
                    )));
                }
            }
        } else {
            false
        };
        if is_directory {
            managed_targets.push(ManagedTarget::new(
                entry.path(),
                kind,
                sanitize_display_label(&format!(
                    "{}/{}",
                    name.to_string_lossy(),
                    kind.instruction_filename()
                )),
            ));
        }
    }

    managed_targets.sort_by(|left, right| left.directory.cmp(&right.directory));
    Ok(managed_targets)
}

fn sanitize_display_label(label: &str) -> String {
    label
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '<' | '>' | '&' | '\'' | '"') {
                '?'
            } else {
                character
            }
        })
        .collect()
}

fn display_path(path: &Path) -> String {
    sanitize_display_label(&path.to_string_lossy())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileKind {
    Codex,
    Claude,
}

impl ProfileKind {
    fn from_profile_name(name: &std::ffi::OsStr) -> Option<Self> {
        [Self::Codex, Self::Claude].into_iter().find(|kind| {
            name.as_bytes().starts_with(kind.profile_prefix())
                && !is_backup_like_name(name, kind.profile_prefix().len())
        })
    }

    fn profile_prefix(self) -> &'static [u8] {
        match self {
            Self::Codex => b".codex",
            Self::Claude => b".claude",
        }
    }

    fn instruction_filename(self) -> &'static str {
        match self {
            Self::Codex => "AGENTS.md",
            Self::Claude => "CLAUDE.md",
        }
    }
}

fn is_backup_like_name(name: &std::ffi::OsStr, prefix_length: usize) -> bool {
    let name = name.as_bytes();
    let suffix = &name[prefix_length..];
    suffix.ends_with(b"~")
        || BACKUP_TOKENS
            .iter()
            .any(|token| contains_ignore_ascii_case(suffix, token))
}

fn contains_ignore_ascii_case(value: &[u8], needle: &[u8]) -> bool {
    value
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn rename_targets(
    managed_targets: &[(ManagedTarget, Presence)],
    desired_state: InstructionState,
) -> Result<(), Error> {
    let mut renames = Vec::new();
    for (managed_target, presence) in managed_targets {
        let paths = match (presence, desired_state) {
            (Presence::Enabled, InstructionState::Off) => Some((
                managed_target.enabled_path(),
                managed_target.disabled_path(),
            )),
            (Presence::Disabled, InstructionState::On) => Some((
                managed_target.disabled_path(),
                managed_target.enabled_path(),
            )),
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
            return Err(Error::unchanged(format!(
                "cannot change instruction state: source {} disappeared during preflight",
                display_path(&rename.source)
            )));
        }
        if path_exists(&rename.destination)? {
            return Err(Error::unchanged(format!(
                "cannot change instruction state: destination {} already exists",
                display_path(&rename.destination)
            )));
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
                        display_path(&completed_rename.destination),
                        display_path(&completed_rename.source)
                    ));
                }
            }
            let rollback_succeeded = rollback_failures.is_empty();
            let rollback = if rollback_succeeded {
                "completed renames were rolled back".to_owned()
            } else {
                format!("rollback also failed: {}", rollback_failures.join("; "))
            };
            let message = format!(
                "cannot rename {} to {}: {error}; {rollback}",
                display_path(&rename.source),
                display_path(&rename.destination)
            );
            return Err(if rollback_succeeded {
                Error::unchanged(message)
            } else {
                Error::may_have_changed(message)
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
    managed_targets: Vec<(ManagedTarget, Presence)>,
}

struct Rename {
    source: PathBuf,
    destination: PathBuf,
}

fn home_directory() -> Result<PathBuf, Error> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::unchanged("HOME is not set"))
}

#[derive(Clone)]
struct ManagedTarget {
    directory: PathBuf,
    kind: ProfileKind,
    display_label: String,
}

impl ManagedTarget {
    fn new(directory: PathBuf, kind: ProfileKind, display_label: String) -> Self {
        Self {
            directory,
            kind,
            display_label,
        }
    }

    fn enabled_path(&self) -> PathBuf {
        self.directory.join(self.kind.instruction_filename())
    }

    fn disabled_path(&self) -> PathBuf {
        self.directory.join(format!(
            "{}.no-auto-inject",
            self.kind.instruction_filename()
        ))
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
        Err(error) => Err(Error::unchanged(format!(
            "cannot inspect {}: {error}",
            display_path(path)
        ))),
    }
}

#[derive(Clone, Copy)]
enum Presence {
    Enabled,
    Disabled,
    Collision,
    Missing,
}
