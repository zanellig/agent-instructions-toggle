//! A windowless StatusNotifier observer.
//!
//! The tray is optional. It never reconciles anything on its own, it never
//! polls, and quitting it leaves the command and the shortcut working.

use std::process::ExitCode;
use std::sync::mpsc;

use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, ToolTip};
use notify::{Event, RecursiveMode, Watcher};

use crate::state::{self, Action, Report};
use crate::ui;

const SIZE: i32 = 22;
const AMBER: u32 = 0xf39c12;

struct AgentTray {
    report: Report,
}

impl AgentTray {
    fn refresh(&mut self) {
        if let Ok(report) = state::inspect() {
            self.report = report;
        }
    }

    fn act(&mut self, action: Action) {
        match state::apply(action) {
            Ok(outcome) => ui::notify_outcome(&outcome),
            Err(err) => ui::notify_error(&err),
        }
        self.refresh();
    }
}

impl ksni::Tray for AgentTray {
    /// A stray click must not change global behavior, so activation opens the
    /// menu and every action stays deliberate.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        env!("CARGO_PKG_NAME").into()
    }

    fn title(&self) -> String {
        "Agent instructions".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![icon(&self.report)]
    }

    /// Never color alone: the state is spelled out on the first line and any
    /// missing target is named on the lines after it.
    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: format!("AGENTS: {}", self.report.state),
            description: ui::warnings(&self.report).join("\n"),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Enable".into(),
                activate: Box::new(|this: &mut Self| this.act(Action::Enable)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Disable".into(),
                activate: Box::new(|this: &mut Self| this.act(Action::Disable)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Status".into(),
                activate: Box::new(|this: &mut Self| {
                    this.refresh();
                    ui::notify_report(&this.report);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                // Stops this process only. The command and the shortcut keep working.
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn run() -> ExitCode {
    let lock = match state::runtime_dir().map(|dir| dir.join("tray.lock")) {
        Ok(path) => match state::lock_file(&path) {
            Ok(file) => file,
            Err(err) => {
                eprintln!("cannot open the tray lock: {err}");
                return ExitCode::FAILURE;
            }
        },
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    if lock.try_lock().is_err() {
        eprintln!("agent-instructions tray is already running");
        return ExitCode::SUCCESS;
    }

    let report = match state::inspect() {
        Ok(report) => report,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    let handle = match (AgentTray { report }).spawn() {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("cannot register with the StatusNotifier host: {err:?}");
            return ExitCode::FAILURE;
        }
    };

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(watcher) => watcher,
        Err(err) => {
            eprintln!("cannot watch the managed directories: {err}");
            return ExitCode::FAILURE;
        }
    };
    for dir in state::watched_dirs().unwrap_or_default() {
        // A profile directory that does not exist is a warning, not a reason to
        // refuse to run.
        let _ = watcher.watch(&dir, RecursiveMode::NonRecursive);
    }

    for received in &rx {
        let Ok(event) = received else { continue };
        if !relevant(&event) {
            continue;
        }
        // Collapse the burst a single rename produces into one refresh.
        while rx.try_recv().is_ok() {}
        handle.update(|tray: &mut AgentTray| tray.refresh());
    }

    ExitCode::SUCCESS
}

fn relevant(event: &Event) -> bool {
    event.paths.iter().any(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(state::is_managed_name)
    })
}

fn icon(report: &Report) -> Icon {
    use state::State::*;
    let color = match report.state {
        On => 0x2ecc71,
        Off => 0x95a5a6,
        Mixed => AMBER,
        Conflict => 0xe74c3c,
    };
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];
    disc(&mut data, 11.0, 11.0, 9.0, color);
    if !report.missing.is_empty() {
        // The base state stays readable; the amber corner adds the warning.
        disc(&mut data, 17.0, 17.0, 4.5, AMBER);
    }
    Icon {
        width: SIZE,
        height: SIZE,
        data,
    }
}

/// ARGB32 in network byte order, which is what StatusNotifier hosts expect.
fn disc(data: &mut [u8], cx: f32, cy: f32, radius: f32, rgb: u32) {
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            if dx * dx + dy * dy > radius * radius {
                continue;
            }
            let i = ((y * SIZE + x) * 4) as usize;
            data[i] = 0xff;
            data[i + 1] = (rgb >> 16) as u8;
            data[i + 2] = (rgb >> 8) as u8;
            data[i + 3] = rgb as u8;
        }
    }
}
