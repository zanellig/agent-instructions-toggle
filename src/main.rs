mod desktop_notification;
mod instruction_state;
mod tray;

use std::process::ExitCode;
use std::{io, io::Write, os::unix::ffi::OsStrExt};

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
                        desktop_notification::failure(&error);
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
                desktop_notification::transition(&result);
            }
            Ok(())
        }
        "tray" if args.next().is_none() => tray::run(),
        "profiles"
            if args.next().as_deref() == Some("--claude")
                && args.next().as_deref() == Some("--null")
                && args.next().is_none() =>
        {
            let inspection = instruction_state::inspect().map_err(|error| error.to_string())?;
            let mut stdout = io::stdout().lock();
            for profile in inspection.claude_profile_directories {
                stdout
                    .write_all(profile.as_os_str().as_bytes())
                    .and_then(|()| stdout.write_all(&[0]))
                    .map_err(|error| format!("cannot write Claude profile list: {error}"))?;
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

fn usage() -> String {
    "usage: agent-instructions status [--machine] | (enable | disable | toggle) [--notify] | tray | profiles --claude --null"
        .to_owned()
}
