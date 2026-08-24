mod desktop_notification;
mod instruction_state;
mod lock;
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
            let format = match args.next().as_deref() {
                None => StatusFormat::Human,
                Some("--machine") if args.next().is_none() => StatusFormat::Machine,
                Some("--segment") if args.next().is_none() => StatusFormat::Segment,
                _ => return Err(usage()),
            };
            let inspection = instruction_state::inspect().map_err(|error| error.to_string())?;
            print_inspection(&inspection, format);
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
                    if notify {
                        desktop_notification::failure(&error);
                    }
                    return Err(error.to_string());
                }
            };
            if result.recovered_mixed_state {
                println!(
                    "Recovered mixed instruction state to on; requested action was not applied."
                );
            }
            print_inspection(&result.inspection, StatusFormat::Human);
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

#[derive(Clone, Copy)]
enum StatusFormat {
    Human,
    Machine,
    Segment,
}

fn print_inspection(inspection: &instruction_state::Inspection, format: StatusFormat) {
    match format {
        StatusFormat::Human => println!("Instruction state: {}", inspection.state),
        StatusFormat::Machine => println!("{}", inspection.state),
        StatusFormat::Segment => println!(
            "\u{1b}[{}mAGENTS:{}\u{1b}[0m",
            inspection.state.appearance().ansi_sgr(),
            inspection.state
        ),
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
    "usage: agent-instructions status [--machine | --segment] | (enable | disable | toggle) [--notify] | tray | profiles --claude --null"
        .to_owned()
}
