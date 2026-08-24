mod state;
mod tray;
mod ui;

use std::env;
use std::process::ExitCode;

use state::Action;

const USAGE: &str = "\
agent-instructions - toggle global agent instruction documents

USAGE:
    agent-instructions status [--machine]
    agent-instructions enable  [--notify]
    agent-instructions disable [--notify]
    agent-instructions toggle  [--notify]
    agent-instructions tray

Changes apply to new agent contexts only. A context that is already live keeps
the instructions it has already loaded.
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let flags: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();

    match args.first().map(String::as_str) {
        Some("status") => status(flags.contains(&"--machine")),
        Some("enable") => mutate(Action::Enable, flags.contains(&"--notify")),
        Some("disable") => mutate(Action::Disable, flags.contains(&"--notify")),
        Some("toggle") => mutate(Action::Toggle, flags.contains(&"--notify")),
        Some("tray") => tray::run(),
        Some("--version") | Some("-V") => {
            println!("agent-instructions {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("-h") | None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Status never changes files, so every observable state exits successfully.
fn status(machine: bool) -> ExitCode {
    let report = match state::inspect() {
        Ok(report) => report,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    if machine {
        println!("{}", report.state.token());
        return ExitCode::SUCCESS;
    }

    println!("AGENTS: {}", report.state);
    println!("{}", ui::state_meaning(report.state));
    for line in ui::warnings(&report) {
        println!("{line}");
    }
    ExitCode::SUCCESS
}

fn mutate(action: Action, notify: bool) -> ExitCode {
    match state::apply(action) {
        Ok(outcome) => {
            println!("{}", ui::outcome_line(&outcome));
            for label in &outcome.missing {
                println!("missing: {label}");
            }
            if notify {
                ui::notify_outcome(&outcome);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            if notify {
                ui::notify_error(&err);
            }
            ExitCode::FAILURE
        }
    }
}
