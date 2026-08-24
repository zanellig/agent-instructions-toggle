//! Instruction state: the one place the rules live.
//!
//! Classification, locking, preflight, renames, rollback, recovery and warnings
//! all happen here. The CLI, the tray, the shortcut and the Claude status line
//! are adapters over `inspect` and `apply`; none of them may re-derive a rule.

use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// Appended to a recognized filename to make it unrecognizable to coding agents.
pub const DISABLED_SUFFIX: &str = ".no-auto-inject";

/// The fixed production target set, in rename order.
///
/// `.codex_backup` is deliberately absent: archived files stay untouched.
const TARGETS: &[(&str, &str)] = &[
    (".codex", "AGENTS.md"),
    (".codex_p", "AGENTS.md"),
    (".codex_p2", "AGENTS.md"),
    (".claude", "CLAUDE.md"),
];

/// The aggregate state derived from all managed targets.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum State {
    On,
    Off,
    Mixed,
    Conflict,
}

impl State {
    /// The single machine-readable token. Nothing else may be printed with it.
    pub fn token(self) -> &'static str {
        match self {
            State::On => "on",
            State::Off => "off",
            State::Mixed => "mixed",
            State::Conflict => "conflict",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

/// What the caller asked for.
#[derive(Copy, Clone, Debug)]
pub enum Action {
    Enable,
    Disable,
    Toggle,
}

/// A read-only view of every managed target.
#[derive(Clone, Debug)]
pub struct Report {
    pub state: State,
    /// Labels of targets where neither name exists.
    pub missing: Vec<String>,
    /// Labels of targets where both names exist.
    pub collisions: Vec<String>,
}

/// The result of a completed mutation.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub state: State,
    /// A mixed state was reconciled to `on` and the requested mutation stopped.
    pub recovered: bool,
    pub renamed: usize,
    pub missing: Vec<String>,
}

#[derive(Debug)]
pub enum Error {
    /// Nothing was changed and nothing will be.
    Conflict(Report),
    /// An operational failure. Any completed renames were rolled back.
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Conflict(report) => {
                if report.collisions.is_empty() {
                    write!(
                        f,
                        "conflict: no managed instruction documents found; no files changed"
                    )
                } else {
                    write!(
                        f,
                        "conflict: both the recognized and disabled names exist at {}; no files changed",
                        report.collisions.join(", ")
                    )
                }
            }
            Error::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

struct Target {
    label: String,
    enabled: PathBuf,
    disabled: PathBuf,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Presence {
    Enabled,
    Disabled,
    Missing,
    Collision,
}

/// Inspect the managed targets without changing anything.
pub fn inspect() -> Result<Report, Error> {
    Ok(report(&targets()?))
}

/// Apply `action`, or recover a mixed state and stop.
pub fn apply(action: Action) -> Result<Outcome, Error> {
    let targets = targets()?;
    // Held until the end of the function: every mutation is serialized.
    let _lock = acquire_lock()?;

    let report = report(&targets);
    if report.state == State::Conflict {
        return Err(Error::Conflict(report));
    }

    // Conservative recovery: reconcile to `on`, report it, and stop. A second
    // deliberate action may then disable.
    if report.state == State::Mixed {
        let moves = plan(&targets, State::On);
        rename_all(&moves)?;
        return Ok(Outcome {
            state: State::On,
            recovered: true,
            renamed: moves.len(),
            missing: report.missing,
        });
    }

    let desired = match action {
        Action::Enable => State::On,
        Action::Disable => State::Off,
        Action::Toggle if report.state == State::On => State::Off,
        Action::Toggle => State::On,
    };

    let moves = plan(&targets, desired);
    rename_all(&moves)?;
    Ok(Outcome {
        state: desired,
        recovered: false,
        renamed: moves.len(),
        missing: report.missing,
    })
}

/// Parent directories the tray watches for native filesystem events.
pub fn watched_dirs() -> Result<Vec<PathBuf>, Error> {
    Ok(targets()?
        .iter()
        .filter_map(|t| t.enabled.parent().map(Path::to_path_buf))
        .collect())
}

/// True when `name` is a filename this tool manages.
pub fn is_managed_name(name: &str) -> bool {
    let base = name.strip_suffix(DISABLED_SUFFIX).unwrap_or(name);
    TARGETS.iter().any(|(_, recognized)| *recognized == base)
}

fn targets() -> Result<Vec<Target>, Error> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Failed("HOME is not set".into()))?;
    Ok(TARGETS
        .iter()
        .map(|(dir, name)| Target {
            label: format!("~/{dir}/{name}"),
            enabled: home.join(dir).join(name),
            disabled: home.join(dir).join(format!("{name}{DISABLED_SUFFIX}")),
        })
        .collect())
}

fn presence(target: &Target) -> Presence {
    match (target.enabled.exists(), target.disabled.exists()) {
        (true, true) => Presence::Collision,
        (true, false) => Presence::Enabled,
        (false, true) => Presence::Disabled,
        (false, false) => Presence::Missing,
    }
}

fn report(targets: &[Target]) -> Report {
    let mut missing = Vec::new();
    let mut collisions = Vec::new();
    let (mut on, mut off) = (0usize, 0usize);

    for target in targets {
        match presence(target) {
            Presence::Enabled => on += 1,
            Presence::Disabled => off += 1,
            Presence::Missing => missing.push(target.label.clone()),
            Presence::Collision => collisions.push(target.label.clone()),
        }
    }

    // A single collision poisons the whole state, and so does having no target
    // left to derive a state from.
    let state = if !collisions.is_empty() || (on == 0 && off == 0) {
        State::Conflict
    } else if off == 0 {
        State::On
    } else if on == 0 {
        State::Off
    } else {
        State::Mixed
    };

    Report {
        state,
        missing,
        collisions,
    }
}

/// The renames needed to reach `desired`. Missing and already-correct targets
/// contribute nothing, so partial absence never blocks the healthy targets.
fn plan(targets: &[Target], desired: State) -> Vec<(PathBuf, PathBuf)> {
    targets
        .iter()
        .filter_map(|target| match (presence(target), desired) {
            (Presence::Disabled, State::On) => {
                Some((target.disabled.clone(), target.enabled.clone()))
            }
            (Presence::Enabled, State::Off) => {
                Some((target.enabled.clone(), target.disabled.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Preflight everything, then rename. A failure rolls back what already moved.
fn rename_all(moves: &[(PathBuf, PathBuf)]) -> Result<(), Error> {
    for (from, to) in moves {
        if !from.exists() {
            return Err(Error::Failed(format!(
                "preflight failed: {} disappeared; no files changed",
                from.display()
            )));
        }
        // `rename` would silently overwrite, so refuse before starting.
        if to.exists() {
            return Err(Error::Failed(format!(
                "preflight failed: {} already exists; no files changed",
                to.display()
            )));
        }
    }

    let mut done: Vec<&(PathBuf, PathBuf)> = Vec::new();
    for step in moves {
        let (from, to) = step;
        match fs::rename(from, to) {
            Ok(()) => done.push(step),
            Err(err) => return Err(Error::Failed(rollback(&done, from, to, err))),
        }
    }
    Ok(())
}

fn rollback(done: &[&(PathBuf, PathBuf)], from: &Path, to: &Path, cause: io::Error) -> String {
    let mut stuck = Vec::new();
    for (original, moved) in done.iter().rev() {
        if fs::rename(moved, original).is_err() {
            stuck.push(moved.display().to_string());
        }
    }
    let tail = if stuck.is_empty() {
        format!("rolled back {} earlier rename(s)", done.len())
    } else {
        format!(
            "ROLLBACK INCOMPLETE, still renamed: {}. Run `agent-instructions status`",
            stuck.join(", ")
        )
    };
    format!(
        "failed to rename {} to {}: {cause}; {tail}",
        from.display(),
        to.display()
    )
}

/// One stable lock for the whole tool, under the runtime directory when there is
/// one and the state directory otherwise. The instruction documents themselves
/// are never used as lockfiles.
fn acquire_lock() -> Result<File, Error> {
    let path = lock_path()?;
    open_lock(&path).map_err(|err| Error::Failed(format!("cannot lock {}: {err}", path.display())))
}

fn open_lock(path: &Path) -> io::Result<File> {
    let file = lock_file(path)?;
    file.lock()?;
    Ok(file)
}

/// Opens a lockfile without taking the lock, so callers that must not block
/// (the tray's single-instance guard) can `try_lock` it themselves.
pub(crate) fn lock_file(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

pub(crate) fn runtime_dir() -> Result<PathBuf, Error> {
    if let Some(dir) = env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("agent-instructions"));
    }
    if let Some(dir) = env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("agent-instructions"));
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Failed("HOME is not set".into()))?;
    Ok(home.join(".local/state/agent-instructions"))
}

fn lock_path() -> Result<PathBuf, Error> {
    Ok(runtime_dir()?.join("instructions.lock"))
}
