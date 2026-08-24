mod instruction_state;

use std::process::{Command, ExitCode, Stdio};

use instruction_state::Action;

fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let Some(command) = args.next() else {
        return Err(usage());
    };

    match command.as_str() {
        "status" => {
            let machine = match args.next().as_deref() {
                None => false,
                Some("--machine") if args.next().is_none() => true,
                _ => return Err(usage()),
            };
            let inspection = instruction_state::inspect().map_err(|error| error.to_string())?;
            print_inspection(&inspection, machine);
            print_inspection_warnings(&inspection);
            Ok(())
        }
        "enable" | "disable" | "toggle" => {
            let notify = match args.next().as_deref() {
                None => false,
                Some("--notify") if args.next().is_none() => true,
                _ => return Err(usage()),
            };
            let action = match command.as_str() {
                "enable" => Action::Enable,
                "disable" => Action::Disable,
                "toggle" => Action::Toggle,
                _ => unreachable!(),
            };
            let result = match instruction_state::apply(action) {
                Ok(result) => result,
                Err(error) => {
                    let error = error.to_string();
                    if notify {
                        send_notification(
                            "Agent instructions unchanged",
                            &format!("Could not change global instructions: {error}"),
                        );
                    }
                    return Err(error);
                }
            };
            if result.recovered_mixed_state {
                println!(
                    "Recovered mixed instruction state to on; requested action was not applied."
                );
            }
            print_inspection(&result.inspection, false);
            print_inspection_warnings(&result.inspection);
            if notify {
                notify_transition(&result);
            }
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn print_inspection(inspection: &instruction_state::Inspection, machine: bool) {
    if machine {
        println!("{}", inspection.state);
    } else {
        println!("Instruction state: {}", inspection.state);
    }
}

fn print_inspection_warnings(inspection: &instruction_state::Inspection) {
    if !inspection.missing_targets.is_empty() {
        eprintln!(
            "Warning: missing managed targets: {}",
            inspection.missing_targets.join(", ")
        );
    }
    if !inspection.collision_targets.is_empty() {
        eprintln!(
            "Warning: conflicting names for managed targets: {}",
            inspection.collision_targets.join(", ")
        );
    }
}

fn notify_transition(result: &instruction_state::ApplyResult) {
    let (title, message) = if result.recovered_mixed_state {
        (
            "Agent instructions recovered",
            "Mixed instruction state was restored to on. Press the shortcut again to disable global instructions for new contexts.",
        )
    } else {
        match result.inspection.state {
            instruction_state::InstructionState::On => (
                "Agent instructions enabled",
                "New coding-agent contexts will include global instructions.",
            ),
            instruction_state::InstructionState::Off => (
                "Agent instructions disabled",
                "New coding-agent contexts will start without global instructions.",
            ),
            instruction_state::InstructionState::Mixed
            | instruction_state::InstructionState::Conflict => return,
        }
    };
    let mut body = message.to_owned();
    if !result.inspection.missing_targets.is_empty() {
        body.push_str(" Missing managed targets: ");
        body.push_str(&result.inspection.missing_targets.join(", "));
        body.push('.');
    }

    send_notification(title, &body);
}

fn send_notification(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args([
            "--app-name=Agent Instructions",
            "--icon=preferences-system",
            title,
            body,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn usage() -> String {
    "usage: agent-instructions status [--machine] | (enable | disable | toggle) [--notify]"
        .to_owned()
}
