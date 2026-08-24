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

/// Codex profiles are discovered: any immediate child of `$HOME` whose name
/// starts with this prefix is a candidate. Nothing about them is hard-coded.
const CODEX_PREFIX: &str = ".codex";
const CODEX_DOCUMENT: &str = "AGENTS.md";

/// Claude Code reads exactly one global document, so its home is fixed. It stays
/// a managed target whether or not it exists, which is what makes an absent
/// document a reported warning rather than a silent omission.
const CLAUDE_HOME: &str = ".claude";
const CLAUDE_DOCUMENT: &str = "CLAUDE.md";

/// A profile whose name carries one of these words is an archive, not a working
/// profile, so it is left alone. Matching is on whole words: `.codex_old` is a
/// backup, `.codex_bold` is a real profile.
const BACKUP_WORDS: &[&str] = &[
    "archive", "archived", "backup", "backups", "bak", "copy", "disabled", "old", "orig",
    "original", "save", "saved",
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
    /// Agent homes skipped as backups. Reported so that a profile this tool
    /// declines to manage is never a silent omission.
    pub ignored: Vec<String>,
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
                        "conflict: no managed instruction documents found under any agent home; no files changed"
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

/// What one scan of `$HOME` found.
struct Discovery {
    /// Sorted, so the rename order is the same on every run.
    targets: Vec<Target>,
    ignored: Vec<String>,
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
    Ok(report(&discover()?))
}

/// Apply `action`, or recover a mixed state and stop.
pub fn apply(action: Action) -> Result<Outcome, Error> {
    // Held until the end of the function: every mutation is serialized.
    let _lock = acquire_lock()?;
    let discovery = discover()?;
    let targets = &discovery.targets;

    let report = report(&discovery);
    if report.state == State::Conflict {
        return Err(Error::Conflict(report));
    }

    // Conservative recovery: reconcile to `on`, report it, and stop. A second
    // deliberate action may then disable.
    if report.state == State::Mixed {
        let moves = plan(targets, State::On);
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

    let moves = plan(targets, desired);
    rename_all(&moves)?;
    Ok(Outcome {
        state: desired,
        recovered: false,
        renamed: moves.len(),
        missing: report.missing,
    })
}

/// Directories the tray watches for native filesystem events: `$HOME`, so that
/// a profile appearing or disappearing is noticed, and every managed directory,
/// for changes to the documents themselves.
pub fn watched_dirs() -> Result<Vec<PathBuf>, Error> {
    let home = home()?;
    let mut dirs = vec![home];
    dirs.extend(
        discover()?
            .targets
            .iter()
            .filter_map(|t| t.enabled.parent().map(Path::to_path_buf)),
    );
    Ok(dirs)
}

/// True when a filesystem event at `path` could change the instruction state:
/// either a managed document, or a profile directory coming or going.
pub fn is_relevant_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let document = name.strip_suffix(DISABLED_SUFFIX).unwrap_or(name);
    document == CODEX_DOCUMENT
        || document == CLAUDE_DOCUMENT
        || name == CLAUDE_HOME
        || codex_profile_suffix(name).is_some()
}

/// Find the working Codex profiles under `$HOME`, and add the fixed Claude home.
fn discover() -> Result<Discovery, Error> {
    let home = home()?;
    let entries = fs::read_dir(&home)
        .map_err(|err| Error::Failed(format!("cannot read {}: {err}", home.display())))?;

    let mut targets = vec![target(&home, CLAUDE_HOME, CLAUDE_DOCUMENT)];
    let mut ignored = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(suffix) = codex_profile_suffix(name) else {
            continue;
        };
        // `is_dir` follows symlinks, so a profile reached through one counts.
        if !entry.path().is_dir() {
            continue;
        }
        if is_backup(suffix) {
            ignored.push(format!("~/{name}"));
            continue;
        }
        targets.push(target(&home, name, CODEX_DOCUMENT));
    }

    // `read_dir` order is whatever the filesystem hands back. Sorting keeps the
    // rename order, and therefore the rollback order, reproducible.
    targets.sort_by(|a, b| a.enabled.cmp(&b.enabled));
    ignored.sort();
    Ok(Discovery { targets, ignored })
}

fn target(home: &Path, dir: &str, document: &str) -> Target {
    Target {
        label: format!("~/{dir}/{document}"),
        enabled: home.join(dir).join(document),
        disabled: home.join(dir).join(format!("{document}{DISABLED_SUFFIX}")),
    }
}

/// The part of a Codex profile directory name that identifies the profile, or
/// `None` when the name is not a profile at all.
///
/// A suffix has to start at a separator or a digit, so `.codex_p2` and `.codex2`
/// are profiles while `.codexrc` is an unrelated dotfile.
fn codex_profile_suffix(name: &str) -> Option<&str> {
    let suffix = name.strip_prefix(CODEX_PREFIX)?;
    match suffix.chars().next() {
        None => Some(suffix),
        Some(c) if c.is_ascii_digit() || c == '_' || c == '-' || c == '.' => Some(suffix),
        Some(_) => None,
    }
}

fn is_backup(suffix: &str) -> bool {
    suffix.ends_with('~')
        || suffix
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|word| BACKUP_WORDS.contains(&word.to_ascii_lowercase().as_str()))
}

fn home() -> Result<PathBuf, Error> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Failed("HOME is not set".into()))
}

fn presence(target: &Target) -> Presence {
    match (target.enabled.exists(), target.disabled.exists()) {
        (true, true) => Presence::Collision,
        (true, false) => Presence::Enabled,
        (false, true) => Presence::Disabled,
        (false, false) => Presence::Missing,
    }
}

fn report(discovery: &Discovery) -> Report {
    let mut missing = Vec::new();
    let mut collisions = Vec::new();
    let (mut on, mut off) = (0usize, 0usize);

    for target in &discovery.targets {
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
        ignored: discovery.ignored.clone(),
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
    Ok(home()?.join(".local/state/agent-instructions"))
}

fn lock_path() -> Result<PathBuf, Error> {
    Ok(runtime_dir()?.join("instructions.lock"))
}
