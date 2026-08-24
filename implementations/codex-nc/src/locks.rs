use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub enum LockMode {
    Wait,
    Try,
}

pub struct Lock {
    _file: File,
}

pub fn acquire(filename: &str, mode: LockMode) -> io::Result<Lock> {
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    let mut errors = Vec::new();

    if let Some(runtime) = absolute_environment_path("XDG_RUNTIME_DIR") {
        match acquire_in(&runtime.join("agent-instructions"), filename, mode) {
            Ok(lock) => return Ok(lock),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Err(error),
            Err(error) => errors.push(format!("runtime lock: {error}")),
        }
    }

    let state_home =
        absolute_environment_path("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state"));
    acquire_in(&state_home.join("agent-instructions"), filename, mode).map_err(|error| {
        errors.push(format!("state lock: {error}"));
        io::Error::new(error.kind(), errors.join("; "))
    })
}

fn acquire_in(directory: &Path, filename: &str, mode: LockMode) -> io::Result<Lock> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(directory)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(directory.join(filename))?;
    match mode {
        LockMode::Wait => file.lock()?,
        LockMode::Try => file.try_lock()?,
    }
    Ok(Lock { _file: file })
}

fn absolute_environment_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}
