use std::process::{Command, Stdio};

use crate::instruction_state::{ApplyResult, Inspection, InstructionState};

pub fn transition(result: &ApplyResult) {
    let (title, message) = if result.recovered_mixed_state {
        (
            "Agent instructions recovered",
            "Mixed instruction state was restored to on. Press the shortcut again to disable global instructions for new contexts.",
        )
    } else {
        match result.inspection.state {
            InstructionState::On => (
                "Agent instructions enabled",
                "New coding-agent contexts will include global instructions.",
            ),
            InstructionState::Off => (
                "Agent instructions disabled",
                "New coding-agent contexts will start without global instructions.",
            ),
            InstructionState::Mixed | InstructionState::Conflict => return,
        }
    };
    let mut body = message.to_owned();
    append_missing_targets(&mut body, &result.inspection);
    send(title, &body);
}

pub fn failure(error: &str) {
    send(
        "Agent instructions unchanged",
        &format!("Could not change global instructions: {error}"),
    );
}

pub fn status(inspection: &Inspection) {
    let mut body = format!("AGENTS: {}", inspection.state);
    append_missing_targets(&mut body, inspection);
    if !inspection.collision_targets.is_empty() {
        body.push_str(" Conflicting names: ");
        body.push_str(&inspection.collision_targets.join(", "));
        body.push('.');
    }
    send("Agent instruction status", &body);
}

fn append_missing_targets(body: &mut String, inspection: &Inspection) {
    if !inspection.missing_targets.is_empty() {
        body.push_str(" Missing managed targets: ");
        body.push_str(&inspection.missing_targets.join(", "));
        body.push('.');
    }
}

fn send(title: &str, body: &str) {
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
