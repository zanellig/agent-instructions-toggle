use std::io;
use std::sync::mpsc::{self, Receiver, Sender};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::locks::{self, LockMode};
use crate::state::{self, Inspection, InstructionState, Operation};

enum TrayEvent {
    Apply(Operation),
    Status,
    Filesystem(notify::Result<Event>),
    Quit,
}

struct Indicator {
    inspection: Inspection,
    sender: Sender<TrayEvent>,
}

impl ksni::Tray for Indicator {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "agent-instructions".into()
    }

    fn title(&self) -> String {
        "Agent Instructions".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn status(&self) -> ksni::Status {
        if matches!(
            self.inspection.state(),
            InstructionState::Mixed | InstructionState::Conflict
        ) || self.inspection.has_missing_targets()
        {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![state_icon(&self.inspection)]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let missing = self.inspection.missing_targets().collect::<Vec<_>>();
        let collisions = self.inspection.colliding_targets().collect::<Vec<_>>();
        let mut details = Vec::new();
        if self.inspection.has_no_targets() {
            details.push("No managed instruction targets were found under HOME".to_owned());
        }
        if !missing.is_empty() {
            details.push(format!("Missing: {}", missing.join(", ")));
        }
        if !collisions.is_empty() {
            details.push(format!("Conflicting copies: {}", collisions.join(", ")));
        }
        ksni::ToolTip {
            icon_pixmap: self.icon_pixmap(),
            title: format!("AGENTS: {}", self.inspection.state_token()),
            description: details.join("\n"),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            menu_action("Enable", Operation::Enable),
            menu_action("Disable", Operation::Disable),
            StandardItem {
                label: "Status".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::Status);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn menu_action(label: &str, operation: Operation) -> ksni::MenuItem<Indicator> {
    StandardItem {
        label: label.into(),
        activate: Box::new(move |tray: &mut Indicator| {
            let _ = tray.sender.send(TrayEvent::Apply(operation));
        }),
        ..Default::default()
    }
    .into()
}

pub fn run() -> Result<(), String> {
    let _instance = locks::acquire("tray.lock", LockMode::Try).map_err(|error| {
        if error.kind() == io::ErrorKind::WouldBlock {
            "the tray indicator is already running".to_owned()
        } else {
            format!("could not start the tray indicator: {error}")
        }
    })?;
    let inspection =
        state::inspect().map_err(|error| crate::output::escape_text(&error.to_string()))?;
    let (sender, receiver) = mpsc::channel();
    let indicator = Indicator {
        inspection: inspection.clone(),
        sender: sender.clone(),
    };
    let handle = indicator
        .assume_sni_available(true)
        .spawn()
        .map_err(|_| "could not register the tray indicator".to_owned())?;
    let mut watcher = watcher_for(&inspection, &sender)?;

    event_loop(&receiver, &sender, &handle, &mut watcher);
    handle.shutdown().wait();
    Ok(())
}

fn event_loop(
    receiver: &Receiver<TrayEvent>,
    sender: &Sender<TrayEvent>,
    handle: &Handle<Indicator>,
    watcher: &mut RecommendedWatcher,
) {
    while let Ok(event) = receiver.recv() {
        match event {
            TrayEvent::Apply(operation) => apply(operation, handle),
            TrayEvent::Status => show_status(handle),
            TrayEvent::Filesystem(Ok(event)) => {
                let Some(current) = handle.update(|tray| tray.inspection.clone()) else {
                    break;
                };
                if event.kind.is_access()
                    || !event.paths.iter().any(|path| current.manages_path(path))
                {
                    continue;
                }
                if let Ok(inspection) = state::inspect() {
                    if let Ok(replacement) = watcher_for(&inspection, sender) {
                        *watcher = replacement;
                    }
                    let _ = handle.update(|tray| tray.inspection = inspection);
                }
            }
            TrayEvent::Filesystem(Err(error)) => {
                eprintln!(
                    "agent-instructions: could not observe an instruction document: {}",
                    crate::output::escape_text(&error.to_string())
                );
            }
            TrayEvent::Quit => break,
        }
    }
}

fn apply(operation: Operation, handle: &Handle<Indicator>) {
    match state::apply(operation) {
        Ok(result) => {
            let message = crate::mutation_message(&result);
            crate::notify_best_effort(
                &format!("AGENTS: {}", result.inspection.state_token()),
                &crate::message_with_warnings(message, &result.inspection),
            );
            let _ = handle.update(|tray| tray.inspection = result.inspection);
        }
        Err(state::ApplyError::Conflict(inspection)) => {
            let message = "AGENTS: conflict. No instruction documents were changed.";
            crate::notify_best_effort(
                "AGENTS: conflict",
                &crate::message_with_warnings(message, &inspection),
            );
            let _ = handle.update(|tray| tray.inspection = inspection);
        }
        Err(state::ApplyError::Operational(error)) => {
            crate::notify_best_effort(
                "Agent instruction change failed",
                &format!("The instruction-state change failed: {error}"),
            );
        }
    }
}

fn show_status(handle: &Handle<Indicator>) {
    let Ok(inspection) = state::inspect() else {
        crate::notify_best_effort(
            "Agent instruction status unavailable",
            "The instruction state could not be read.",
        );
        return;
    };
    let base = format!("AGENTS: {}.", inspection.state_token());
    crate::notify_best_effort(
        &format!("AGENTS: {}", inspection.state_token()),
        &crate::message_with_warnings(&base, &inspection),
    );
    let _ = handle.update(|tray| tray.inspection = inspection);
}

fn watcher_for(
    inspection: &Inspection,
    sender: &Sender<TrayEvent>,
) -> Result<RecommendedWatcher, String> {
    let event_sender = sender.clone();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = event_sender.send(TrayEvent::Filesystem(event));
    })
    .map_err(|error| {
        format!(
            "could not observe instruction documents: {}",
            crate::output::escape_text(&error.to_string())
        )
    })?;
    for directory in inspection.watch_directories() {
        watcher
            .watch(&directory, RecursiveMode::NonRecursive)
            .map_err(|error| {
                format!(
                    "could not observe {}: {error}",
                    crate::output::escape_path(&directory),
                    error = crate::output::escape_text(&error.to_string())
                )
            })?;
    }
    Ok(watcher)
}

fn state_icon(inspection: &Inspection) -> ksni::Icon {
    const SIZE: i32 = 22;
    let base = match inspection.state() {
        InstructionState::On => [0xFF, 0x2E, 0xA0, 0x43],
        InstructionState::Off => [0xFF, 0x7A, 0x7A, 0x7A],
        InstructionState::Mixed => [0xFF, 0xE3, 0x9B, 0x16],
        InstructionState::Conflict => [0xFF, 0xD3, 0x2F, 0x2F],
    };
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let distance = (x - 10) * (x - 10) + (y - 10) * (y - 10);
            let mut pixel = if distance <= 90 {
                base
            } else {
                [0x00, 0x00, 0x00, 0x00]
            };
            if inspection.has_missing_targets() && x >= 14 && y <= 7 {
                pixel = [0xFF, 0xE3, 0x9B, 0x16];
            }
            data.extend_from_slice(&pixel);
        }
    }
    ksni::Icon {
        width: SIZE,
        height: SIZE,
        data,
    }
}
