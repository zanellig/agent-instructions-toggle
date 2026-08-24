use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};

use ksni::menu::StandardItem;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::desktop_notification;
use crate::instruction_state::{self, Action, Inspection, InstructionState, WatchLocations};
use crate::lock;

const ICON_SIZE: i32 = 22;

#[derive(Clone, Copy)]
enum TrayCommand {
    Apply(Action),
    Status,
    Quit,
}

enum TrayEvent {
    Command(TrayCommand),
    Filesystem(notify::Result<Event>),
}

struct TrayIndicator {
    inspection: Inspection,
    events: Sender<TrayEvent>,
}

impl TrayIndicator {
    fn new(inspection: Inspection, events: Sender<TrayEvent>) -> Self {
        Self { inspection, events }
    }
}

impl ksni::Tray for TrayIndicator {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "agent-instructions".to_owned()
    }

    fn title(&self) -> String {
        "Agent Instructions".to_owned()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![state_icon(self.inspection.state)]
    }

    fn overlay_icon_pixmap(&self) -> Vec<ksni::Icon> {
        if self.inspection.missing_targets.is_empty() {
            Vec::new()
        } else {
            vec![colored_circle(10, [255, 245, 166, 35])]
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let mut details: Vec<String> = self
            .inspection
            .missing_targets
            .iter()
            .map(|managed_target| format!("Missing: {managed_target}"))
            .collect();
        details.extend(
            self.inspection
                .collision_targets
                .iter()
                .map(|managed_target| format!("Conflict: {managed_target}")),
        );
        ksni::ToolTip {
            icon_pixmap: self.icon_pixmap(),
            title: format!("AGENTS: {}", self.inspection.state),
            description: details.join("\n"),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::MenuItem;

        vec![
            action_item(
                "Enable",
                "object-select",
                TrayCommand::Apply(Action::Enable),
            ),
            action_item(
                "Disable",
                "process-stop",
                TrayCommand::Apply(Action::Disable),
            ),
            action_item("Status", "dialog-information", TrayCommand::Status),
            MenuItem::Separator,
            action_item("Quit", "application-exit", TrayCommand::Quit),
        ]
    }
}

fn action_item(
    label: &str,
    icon_name: &str,
    command: TrayCommand,
) -> ksni::MenuItem<TrayIndicator> {
    StandardItem {
        label: label.to_owned(),
        icon_name: icon_name.to_owned(),
        activate: Box::new(move |tray: &mut TrayIndicator| {
            let _ = tray.events.send(TrayEvent::Command(command));
        }),
        ..Default::default()
    }
    .into()
}

pub fn run() -> Result<(), String> {
    use ksni::blocking::TrayMethods as _;

    let inspection = instruction_state::inspect().map_err(|error| error.to_string())?;
    let _instance_lock = lock::tray_instance(&inspection.watch_locations.home_directory)?;
    let (events, receiver) = mpsc::channel();
    let mut watcher = NativeWatcher::new(&inspection.watch_locations, events.clone())?;
    let handle = TrayIndicator::new(inspection, events)
        .assume_sni_available(true)
        .spawn()
        .map_err(|error| format!("cannot start tray: {error}"))?;

    for event in receiver {
        match event {
            TrayEvent::Command(TrayCommand::Quit) => break,
            TrayEvent::Command(TrayCommand::Apply(action)) => {
                match instruction_state::apply(action) {
                    Ok(result) => {
                        desktop_notification::transition(&result);
                        update_tray(&handle, &mut watcher, result.inspection)?;
                    }
                    Err(error) => {
                        desktop_notification::failure(&error);
                        refresh_tray(&handle, &mut watcher)?;
                    }
                }
            }
            TrayEvent::Command(TrayCommand::Status) => {
                let inspection = instruction_state::inspect().map_err(|error| error.to_string())?;
                desktop_notification::status(&inspection);
                update_tray(&handle, &mut watcher, inspection)?;
            }
            TrayEvent::Filesystem(Ok(event)) => {
                let locations = handle
                    .update(|tray| tray.inspection.watch_locations.clone())
                    .ok_or_else(|| "tray service stopped".to_owned())?;
                if !event.kind.is_access()
                    && event
                        .paths
                        .iter()
                        .any(|path| locations.is_relevant_path(path))
                {
                    refresh_tray(&handle, &mut watcher)?;
                }
            }
            TrayEvent::Filesystem(Err(error)) => {
                eprintln!("Warning: filesystem watch failed: {error}");
            }
        }
    }

    handle.shutdown().wait();
    Ok(())
}

fn refresh_tray(
    handle: &ksni::blocking::Handle<TrayIndicator>,
    watcher: &mut NativeWatcher,
) -> Result<(), String> {
    let inspection = instruction_state::inspect().map_err(|error| error.to_string())?;
    update_tray(handle, watcher, inspection)
}

fn update_tray(
    handle: &ksni::blocking::Handle<TrayIndicator>,
    watcher: &mut NativeWatcher,
    inspection: Inspection,
) -> Result<(), String> {
    watcher.reconfigure(&inspection.watch_locations)?;
    handle
        .update(move |tray| tray.inspection = inspection)
        .ok_or_else(|| "tray service stopped".to_owned())
}

struct NativeWatcher {
    watcher: RecommendedWatcher,
    watched: BTreeSet<PathBuf>,
}

impl NativeWatcher {
    fn new(locations: &WatchLocations, events: Sender<TrayEvent>) -> Result<Self, String> {
        let watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(TrayEvent::Filesystem(event));
        })
        .map_err(|error| format!("cannot start filesystem watcher: {error}"))?;
        let mut native = Self {
            watcher,
            watched: BTreeSet::new(),
        };
        native.reconfigure(locations)?;
        Ok(native)
    }

    fn reconfigure(&mut self, locations: &WatchLocations) -> Result<(), String> {
        let desired: BTreeSet<_> = std::iter::once(locations.home_directory.clone())
            .chain(locations.profile_directories.iter().cloned())
            .collect();
        let removed: Vec<_> = self.watched.difference(&desired).cloned().collect();
        let added: Vec<_> = desired.difference(&self.watched).cloned().collect();

        for path in removed {
            let _ = self.watcher.unwatch(&path);
            self.watched.remove(&path);
        }
        for path in added {
            self.watcher
                .watch(&path, RecursiveMode::NonRecursive)
                .map_err(|error| format!("cannot watch {}: {error}", path.display()))?;
            self.watched.insert(path);
        }
        Ok(())
    }
}

fn state_icon(state: InstructionState) -> ksni::Icon {
    colored_circle(ICON_SIZE, state.appearance().argb())
}

fn colored_circle(size: i32, color: [u8; 4]) -> ksni::Icon {
    let mut data = vec![0; (size * size * 4) as usize];
    let center = size / 2;
    let radius = size / 2 - 1;
    for y in 0..size {
        for x in 0..size {
            let dx = x - center;
            let dy = y - center;
            if dx * dx + dy * dy <= radius * radius {
                let offset = ((y * size + x) * 4) as usize;
                data[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    ksni::Icon {
        width: size,
        height: size,
        data,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use ksni::Tray as _;

    use super::*;
    use crate::instruction_state::{Inspection, InstructionState, WatchLocations};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn status_notifier_contract_exposes_menu_and_tooltip_details() {
        let (commands, _receiver) = mpsc::channel();
        let tray = TrayIndicator::new(
            inspection(InstructionState::On, &[".codex-empty/AGENTS.md"]),
            commands,
        );

        const { assert!(<TrayIndicator as ksni::Tray>::MENU_ON_ACTIVATE) };
        let tooltip = tray.tool_tip();
        assert_eq!(tooltip.title, "AGENTS: on");
        assert_eq!(tooltip.description, "Missing: .codex-empty/AGENTS.md");
        assert_eq!(tray.overlay_icon_pixmap().len(), 1);

        let labels: Vec<_> = tray
            .menu()
            .into_iter()
            .filter_map(|item| match item {
                ksni::MenuItem::Standard(item) => Some(item.label),
                _ => None,
            })
            .collect();
        assert_eq!(labels, ["Enable", "Disable", "Status", "Quit"]);
    }

    #[test]
    fn status_notifier_contract_uses_the_state_colors() {
        let expected = [
            (InstructionState::On, [255, 46, 160, 67]),
            (InstructionState::Off, [255, 117, 117, 117]),
            (InstructionState::Mixed, [255, 245, 166, 35]),
            (InstructionState::Conflict, [255, 211, 47, 47]),
        ];

        for (state, center_pixel) in expected {
            let (commands, _receiver) = mpsc::channel();
            let tray = TrayIndicator::new(inspection(state, &[]), commands);
            let icon = tray.icon_pixmap().remove(0);
            assert!(tray.overlay_icon_pixmap().is_empty());
            let center = ((icon.height / 2 * icon.width + icon.width / 2) * 4) as usize;
            assert_eq!(&icon.data[center..center + 4], &center_pixel, "{state}");
        }
    }

    #[test]
    fn native_watcher_delivers_instruction_changes_after_reconfiguration() {
        let root = temp_directory();
        let home = root.join("home");
        let profile = home.join(".codex-work");
        fs::create_dir_all(&home).unwrap();
        let (events, receiver) = mpsc::channel();
        let mut watcher = NativeWatcher::new(
            &WatchLocations {
                home_directory: home.clone(),
                profile_directories: Vec::new(),
                instruction_paths: Vec::new(),
            },
            events,
        )
        .unwrap();

        fs::create_dir(&profile).unwrap();
        watcher
            .reconfigure(&WatchLocations {
                home_directory: home,
                profile_directories: vec![profile.clone()],
                instruction_paths: vec![profile.join("AGENTS.md")],
            })
            .unwrap();
        while receiver.try_recv().is_ok() {}
        fs::write(profile.join("AGENTS.md"), "instructions\n").unwrap();

        let event = receiver.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(matches!(event, TrayEvent::Filesystem(Ok(_))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn watch_locations_recognize_only_potential_profiles_and_instruction_documents() {
        let home = PathBuf::from("/tmp/home");
        let profile = home.join(".codex-work");
        let locations = WatchLocations {
            home_directory: home.clone(),
            profile_directories: vec![profile.clone()],
            instruction_paths: vec![
                profile.join("AGENTS.md"),
                profile.join("AGENTS.md.no-auto-inject"),
            ],
        };

        assert!(locations.is_relevant_path(&home.join(".codex-new")));
        assert!(locations.is_relevant_path(&home.join(".claude-team")));
        assert!(locations.is_relevant_path(&profile.join("AGENTS.md")));
        assert!(!locations.is_relevant_path(&home.join("Downloads")));
        assert!(!locations.is_relevant_path(&home.join(".codex-backup")));
        assert!(!locations.is_relevant_path(&profile.join("settings.json")));
    }

    fn inspection(state: InstructionState, missing_targets: &[&str]) -> Inspection {
        Inspection {
            state,
            missing_targets: missing_targets
                .iter()
                .map(|managed_target| (*managed_target).to_owned())
                .collect(),
            collision_targets: Vec::new(),
            claude_profile_directories: Vec::new(),
            watch_locations: WatchLocations {
                home_directory: PathBuf::from("/tmp/home"),
                profile_directories: Vec::new(),
                instruction_paths: Vec::new(),
            },
        }
    }

    fn temp_directory() -> PathBuf {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "agent-instructions-tray-test-{}-{sequence}",
            std::process::id()
        ))
    }
}
