#[cfg(not(target_os = "linux"))]
compile_error!("agent-instructions supports Linux only");

mod locks;
mod output;
mod state;
mod tray;

fn main() {
    let mut args = std::env::args_os();
    let _program = args.next();
    let operation = args.next();

    if operation.as_deref() == Some(std::ffi::OsStr::new("__installer-claude-homes"))
        && args.next().is_none()
    {
        write_installer_claude_homes();
        return;
    }

    if operation.as_deref() == Some(std::ffi::OsStr::new("status")) && args.next().is_none() {
        match state::inspect() {
            Ok(inspection) => {
                println!("{}", inspection.state_token());
                print_warnings(&inspection);
            }
            Err(error) => {
                eprintln!(
                    "agent-instructions: {}",
                    output::escape_text(&error.to_string())
                );
                std::process::exit(1);
            }
        }
        return;
    }

    let requested = match operation.as_deref() {
        Some(value) if value == "enable" => Some(state::Operation::Enable),
        Some(value) if value == "disable" => Some(state::Operation::Disable),
        Some(value) if value == "toggle" => Some(state::Operation::Toggle),
        _ => None,
    };
    if let Some(requested) = requested.filter(|_| args.next().is_none()) {
        run_mutation(requested);
        return;
    }

    if operation.as_deref() == Some(std::ffi::OsStr::new("tray")) && args.next().is_none() {
        if let Err(error) = tray::run() {
            eprintln!("agent-instructions: {}", output::escape_text(&error));
            std::process::exit(1);
        }
        return;
    }

    eprintln!("usage: agent-instructions <status|enable|disable|toggle|tray>");
    std::process::exit(2);
}

fn run_mutation(operation: state::Operation) {
    match state::apply(operation) {
        Ok(result) => {
            let state = result.inspection.state_token();
            let message = mutation_message(&result);
            println!("{message}");
            print_warnings(&result.inspection);
            notify_best_effort(
                &format!("AGENTS: {state}"),
                &message_with_warnings(message, &result.inspection),
            );
        }
        Err(state::ApplyError::Conflict(inspection)) => {
            let message = "AGENTS: conflict. No instruction documents were changed.";
            eprintln!("{message}");
            print_warnings(&inspection);
            notify_best_effort(
                "AGENTS: conflict",
                &message_with_warnings(message, &inspection),
            );
            std::process::exit(1);
        }
        Err(state::ApplyError::Operational(error)) => {
            let message = format!(
                "The instruction-state change failed: {}",
                output::escape_text(&error)
            );
            eprintln!("agent-instructions: {message}");
            notify_best_effort("Agent instruction change failed", &message);
            std::process::exit(1);
        }
    }
}

fn write_installer_claude_homes() {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;

    let inspection = match state::inspect() {
        Ok(inspection) => inspection,
        Err(error) => {
            eprintln!(
                "agent-instructions: {}",
                output::escape_text(&error.to_string())
            );
            std::process::exit(1);
        }
    };
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    for directory in inspection.claude_profile_directories() {
        if stdout
            .write_all(directory.as_os_str().as_bytes())
            .and_then(|_| stdout.write_all(&[0]))
            .is_err()
        {
            std::process::exit(1);
        }
    }
}

fn mutation_message(result: &state::ApplyResult) -> &'static str {
    match result.outcome {
        state::ApplyOutcome::Changed
            if result.inspection.state() == state::InstructionState::On =>
        {
            "AGENTS: on. New contexts will load the managed global instruction documents."
        }
        state::ApplyOutcome::Changed => {
            "AGENTS: off. New contexts will not load the managed global instruction documents."
        }
        state::ApplyOutcome::Unchanged
            if result.inspection.state() == state::InstructionState::On =>
        {
            "AGENTS: on. The managed instruction documents were already enabled for new contexts."
        }
        state::ApplyOutcome::Unchanged => {
            "AGENTS: off. The managed instruction documents were already disabled for new contexts."
        }
        state::ApplyOutcome::RecoveredMixed => {
            "AGENTS: on. The mixed state was recovered to enabled. The requested change stopped; run it again to disable."
        }
    }
}

fn message_with_warnings(message: &str, inspection: &state::Inspection) -> String {
    let missing = inspection.missing_targets().collect::<Vec<_>>();
    let collisions = inspection.colliding_targets().collect::<Vec<_>>();
    let mut result = message.to_owned();
    if inspection.has_no_targets() {
        result.push_str(" No managed instruction targets were found under HOME.");
    }
    if !missing.is_empty() {
        result.push_str(" Missing: ");
        result.push_str(&missing.join(", "));
        result.push('.');
    }
    if !collisions.is_empty() {
        result.push_str(" Conflicting copies: ");
        result.push_str(&collisions.join(", "));
        result.push('.');
    }
    result
}

fn print_warnings(inspection: &state::Inspection) {
    if inspection.has_no_targets() {
        eprintln!("warning: no managed instruction targets were found under HOME");
    }
    for target in inspection.missing_targets() {
        eprintln!("warning: missing managed target: {target}");
    }
    for target in inspection.colliding_targets() {
        eprintln!("warning: both recognized and disabled instruction documents exist: {target}");
    }
}

fn notify_best_effort(summary: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .arg("--app-name=Agent Instructions")
        .arg(summary)
        .arg(body)
        .status();
}
