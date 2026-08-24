use std::env;
use std::ffi::CString;
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::locks::{self, LockMode};

#[derive(Clone)]
pub struct Inspection {
    state: InstructionState,
    targets: Vec<ManagedTarget>,
    home: PathBuf,
}

impl Inspection {
    pub fn state_token(&self) -> &'static str {
        match self.state {
            InstructionState::On => "on",
            InstructionState::Off => "off",
            InstructionState::Mixed => "mixed",
            InstructionState::Conflict => "conflict",
        }
    }

    pub fn missing_targets(&self) -> impl Iterator<Item = &str> + '_ {
        self.targets.iter().filter_map(|target| {
            (target.status == TargetStatus::Missing).then_some(target.label.as_str())
        })
    }

    pub fn colliding_targets(&self) -> impl Iterator<Item = &str> + '_ {
        self.targets.iter().filter_map(|target| {
            (target.status == TargetStatus::Collision).then_some(target.label.as_str())
        })
    }

    pub fn state(&self) -> InstructionState {
        self.state
    }

    pub fn has_missing_targets(&self) -> bool {
        self.missing_targets().next().is_some()
    }

    pub fn has_no_targets(&self) -> bool {
        self.targets.is_empty()
    }

    pub fn watch_directories(&self) -> Vec<PathBuf> {
        let mut directories = vec![self.home.clone()];
        for target in &self.targets {
            let parent = target
                .active
                .parent()
                .expect("managed target has a parent directory");
            let directory = if parent.is_dir() {
                parent
            } else {
                parent
                    .parent()
                    .expect("managed target parent has a home directory")
            };
            if !directories.iter().any(|existing| existing == directory) {
                directories.push(directory.to_path_buf());
            }
        }
        directories
    }

    pub fn manages_path(&self, path: &Path) -> bool {
        self.targets.iter().any(|target| {
            path == target.active || path == target.disabled || target.active.parent() == Some(path)
        }) || (path.parent() == Some(self.home.as_path())
            && path
                .file_name()
                .map(|name| instruction_filename(&name.to_string_lossy()).is_some())
                .unwrap_or(false))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InstructionState {
    On,
    Off,
    Mixed,
    Conflict,
}

#[derive(Clone)]
struct ManagedTarget {
    label: String,
    active: PathBuf,
    disabled: PathBuf,
    status: TargetStatus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetStatus {
    On,
    Off,
    Missing,
    Collision,
}

#[derive(Clone, Copy)]
pub enum Operation {
    Enable,
    Disable,
    Toggle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Changed,
    Unchanged,
    RecoveredMixed,
}

pub struct ApplyResult {
    pub inspection: Inspection,
    pub outcome: ApplyOutcome,
}

pub enum ApplyError {
    Conflict(Inspection),
    Operational(String),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(_) => write!(formatter, "managed instruction documents conflict"),
            Self::Operational(message) => formatter.write_str(message),
        }
    }
}

pub fn inspect() -> io::Result<Inspection> {
    inspect_home(&home_directory()?)
}

pub fn apply(operation: Operation) -> Result<ApplyResult, ApplyError> {
    let home = home_directory().map_err(operational_error)?;
    let _lock = locks::acquire("operation.lock", LockMode::Wait).map_err(operational_error)?;
    hold_lock_for_concurrency_test();

    let before = inspect_home(&home).map_err(operational_error)?;
    if before.state == InstructionState::Conflict {
        return Err(ApplyError::Conflict(before));
    }

    if before.state == InstructionState::Mixed {
        rename_targets(&before, InstructionState::On)?;
        let inspection = inspect_home(&home).map_err(operational_error)?;
        return Ok(ApplyResult {
            inspection,
            outcome: ApplyOutcome::RecoveredMixed,
        });
    }

    let desired = match operation {
        Operation::Enable => InstructionState::On,
        Operation::Disable => InstructionState::Off,
        Operation::Toggle if before.state == InstructionState::On => InstructionState::Off,
        Operation::Toggle => InstructionState::On,
    };

    if before.state == desired {
        return Ok(ApplyResult {
            inspection: before,
            outcome: ApplyOutcome::Unchanged,
        });
    }

    rename_targets(&before, desired)?;
    let inspection = inspect_home(&home).map_err(operational_error)?;
    Ok(ApplyResult {
        inspection,
        outcome: ApplyOutcome::Changed,
    })
}

fn home_directory() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

fn inspect_home(home: &Path) -> io::Result<Inspection> {
    let mut active_count = 0;
    let mut disabled_count = 0;
    let mut has_collision = false;
    let discovered = discover_targets(home)?;
    let mut targets = Vec::with_capacity(discovered.len());
    for (directory, filename) in discovered {
        let active = directory.join(filename);
        let disabled = disabled_path(&active);
        let status = match (path_exists(&active)?, path_exists(&disabled)?) {
            (true, false) => {
                active_count += 1;
                TargetStatus::On
            }
            (false, true) => {
                disabled_count += 1;
                TargetStatus::Off
            }
            (false, false) => TargetStatus::Missing,
            (true, true) => {
                has_collision = true;
                TargetStatus::Collision
            }
        };
        let relative = active.strip_prefix(home).unwrap_or(&active);
        targets.push(ManagedTarget {
            label: format!("~/{}", crate::output::path(relative)),
            active,
            disabled,
            status,
        });
    }

    Ok(Inspection {
        state: match (has_collision, active_count > 0, disabled_count > 0) {
            (true, _, _) | (false, false, false) => InstructionState::Conflict,
            (false, true, false) => InstructionState::On,
            (false, false, true) => InstructionState::Off,
            (false, true, true) => InstructionState::Mixed,
        },
        targets,
        home: home.to_path_buf(),
    })
}

fn discover_targets(home: &Path) -> io::Result<Vec<(PathBuf, &'static str)>> {
    let mut targets = Vec::new();
    for entry in fs::read_dir(home)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(filename) = instruction_filename(&name) else {
            continue;
        };
        let path = entry.path();
        if path.is_dir() {
            targets.push((path, filename));
        }
    }
    targets.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(targets)
}

fn instruction_filename(directory_name: &str) -> Option<&'static str> {
    if is_active_profile_name(directory_name, ".codex") {
        Some("AGENTS.md")
    } else if is_active_profile_name(directory_name, ".claude") {
        Some("CLAUDE.md")
    } else {
        None
    }
}

fn is_active_profile_name(directory_name: &str, base: &str) -> bool {
    let Some(suffix) = directory_name.strip_prefix(base) else {
        return false;
    };
    !looks_archived(suffix)
}

fn looks_archived(suffix: &str) -> bool {
    if suffix.ends_with('~') {
        return true;
    }
    let suffix = suffix.to_ascii_lowercase();
    include_str!("../assets/archive-markers.txt")
        .lines()
        .filter(|marker| !marker.is_empty())
        .any(|marker| suffix.contains(marker))
}

fn rename_targets(inspection: &Inspection, desired: InstructionState) -> Result<(), ApplyError> {
    let plans = inspection
        .targets
        .iter()
        .filter_map(|target| match (target.status, desired) {
            (TargetStatus::On, InstructionState::Off) => Some(RenamePlan {
                from: target.active.clone(),
                to: target.disabled.clone(),
            }),
            (TargetStatus::Off, InstructionState::On) => Some(RenamePlan {
                from: target.disabled.clone(),
                to: target.active.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    for plan in &plans {
        let source = fs::symlink_metadata(&plan.from).map_err(|error| {
            ApplyError::Operational(format!(
                "preflight could not inspect {}: {error}",
                crate::output::path(&plan.from)
            ))
        })?;
        if !source.file_type().is_file() && !source.file_type().is_symlink() {
            return Err(ApplyError::Operational(format!(
                "preflight rejected {} because it is not a document",
                crate::output::path(&plan.from)
            )));
        }
        if path_exists(&plan.to).map_err(operational_error)? {
            return Err(ApplyError::Operational(format!(
                "preflight refused to overwrite {}",
                crate::output::path(&plan.to)
            )));
        }
    }

    let mut completed = Vec::with_capacity(plans.len());
    for (index, plan) in plans.iter().enumerate() {
        let result = if should_fail_rename(index + 1) {
            Err(io::Error::other("test rename failure"))
        } else {
            rename_without_overwrite(&plan.from, &plan.to)
        };

        if let Err(error) = result {
            let rollback_failures = rollback(&completed);
            let mut message = format!(
                "could not rename {} to {}: {error}",
                crate::output::path(&plan.from),
                crate::output::path(&plan.to)
            );
            if !rollback_failures.is_empty() {
                message.push_str("; rollback also failed: ");
                message.push_str(&rollback_failures.join(", "));
            }
            return Err(ApplyError::Operational(message));
        }
        completed.push(plan.clone());
    }

    Ok(())
}

#[derive(Clone)]
struct RenamePlan {
    from: PathBuf,
    to: PathBuf,
}

fn rollback(completed: &[RenamePlan]) -> Vec<String> {
    let mut failures = Vec::new();
    for plan in completed.iter().rev() {
        if let Err(error) = rename_without_overwrite(&plan.to, &plan.from) {
            failures.push(format!(
                "{} to {}: {error}",
                crate::output::path(&plan.to),
                crate::output::path(&plan.from)
            ));
        }
    }
    failures
}

fn rename_without_overwrite(from: &Path, to: &Path) -> io::Result<()> {
    const AT_FDCWD: i32 = -100;
    const RENAME_NOREPLACE: u32 = 1;

    let from = CString::new(from.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let to = CString::new(to.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination path contains NUL")
    })?;
    unsafe extern "C" {
        fn renameat2(
            old_directory: i32,
            old_path: *const std::ffi::c_char,
            new_directory: i32,
            new_path: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    // SAFETY: Both pointers come from live CString values and stay valid for this call.
    let result = unsafe {
        renameat2(
            AT_FDCWD,
            from.as_ptr(),
            AT_FDCWD,
            to.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn operational_error(error: io::Error) -> ApplyError {
    ApplyError::Operational(crate::output::text(&error.to_string()))
}

#[cfg(debug_assertions)]
fn should_fail_rename(index: usize) -> bool {
    env::var("AGENT_INSTRUCTIONS_TEST_FAIL_RENAME_AT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        == Some(index)
}

#[cfg(not(debug_assertions))]
fn should_fail_rename(_index: usize) -> bool {
    false
}

#[cfg(debug_assertions)]
fn hold_lock_for_concurrency_test() {
    let Some(milliseconds) = env::var("AGENT_INSTRUCTIONS_TEST_HOLD_LOCK_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return;
    };
    std::thread::sleep(std::time::Duration::from_millis(milliseconds.min(5_000)));
}

#[cfg(not(debug_assertions))]
fn hold_lock_for_concurrency_test() {}

fn path_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn disabled_path(active: &Path) -> PathBuf {
    let filename = format!(
        "{}.no-auto-inject",
        active
            .file_name()
            .expect("managed target has a filename")
            .to_string_lossy()
    );
    active.with_file_name(filename)
}
