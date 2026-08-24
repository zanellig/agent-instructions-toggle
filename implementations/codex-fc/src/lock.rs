use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

pub fn operation(home_directory: &Path) -> Result<File, String> {
    acquire(
        home_directory,
        "agent-instructions.lock",
        "operation.lock",
        LockMode::Blocking,
        "operation",
    )
}

pub fn tray_instance(home_directory: &Path) -> Result<File, String> {
    acquire(
        home_directory,
        "agent-instructions-tray.lock",
        "tray.lock",
        LockMode::NonBlocking,
        "tray",
    )
}

fn acquire(
    home_directory: &Path,
    runtime_filename: &str,
    fallback_filename: &str,
    mode: LockMode,
    purpose: &str,
) -> Result<File, String> {
    let runtime_attempt = env::var_os("XDG_RUNTIME_DIR")
        .map(|directory| open_and_lock(&PathBuf::from(directory).join(runtime_filename), mode));
    match runtime_attempt {
        Some(Ok(file)) => return Ok(file),
        Some(Err(LockError::AlreadyHeld)) => {
            return Err(format!("agent-instructions {purpose} is already running"));
        }
        _ => {}
    }

    let state_directory = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_directory.join(".local/state"))
        .join("agent-instructions");
    fs::create_dir_all(&state_directory).map_err(|error| {
        format!(
            "cannot create {purpose} state directory {}: {error}",
            state_directory.display()
        )
    })?;
    let fallback_path = state_directory.join(fallback_filename);
    match open_and_lock(&fallback_path, mode) {
        Ok(file) => Ok(file),
        Err(LockError::AlreadyHeld) => {
            Err(format!("agent-instructions {purpose} is already running"))
        }
        Err(LockError::Io(fallback_error)) => {
            let runtime_detail = match runtime_attempt {
                Some(Err(LockError::Io(runtime_error))) => {
                    format!("{runtime_error}; fallback failed: ")
                }
                _ => String::new(),
            };
            Err(format!(
                "cannot acquire {purpose} lock: {runtime_detail}{fallback_error}"
            ))
        }
    }
}

#[derive(Clone, Copy)]
enum LockMode {
    Blocking,
    NonBlocking,
}

enum LockError {
    AlreadyHeld,
    Io(io::Error),
}

fn open_and_lock(path: &Path, mode: LockMode) -> Result<File, LockError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(LockError::Io)?;
    match mode {
        LockMode::Blocking => file.lock().map_err(LockError::Io)?,
        LockMode::NonBlocking => match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(LockError::AlreadyHeld),
            Err(fs::TryLockError::Error(error)) => return Err(LockError::Io(error)),
        },
    }
    Ok(file)
}
