mod instruction_state;

use std::process::ExitCode;

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
            if args.next().is_some() {
                return Err(usage());
            }
            let action = match command.as_str() {
                "enable" => Action::Enable,
                "disable" => Action::Disable,
                "toggle" => Action::Toggle,
                _ => unreachable!(),
            };
            let result = instruction_state::apply(action).map_err(|error| error.to_string())?;
            if result.recovered_mixed_state {
                println!(
                    "Recovered mixed instruction state to on; requested action was not applied."
                );
            }
            print_inspection(&result.inspection, false);
            print_inspection_warnings(&result.inspection);
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
    "usage: agent-instructions status [--machine] | enable | disable | toggle".to_owned()
}
