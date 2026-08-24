//! Shared human-facing phrasing and best-effort desktop notifications.
//!
//! Every message is about *new* agent contexts. An already-live context keeps
//! the instructions it loaded, and nothing here may suggest otherwise.

use std::process::Command;

use crate::state::{Error, Outcome, Report, State};

pub fn state_meaning(state: State) -> &'static str {
    match state {
        State::On => "New agent contexts load your global instructions.",
        State::Off => "New agent contexts start without your global instructions.",
        State::Mixed => "Managed documents disagree. New contexts load some instructions.",
        State::Conflict => "The state cannot be derived. New contexts are unpredictable.",
    }
}

/// Warning lines for a report, one per missing target.
pub fn warnings(report: &Report) -> Vec<String> {
    let mut lines: Vec<String> = report
        .missing
        .iter()
        .map(|label| format!("missing: {label}"))
        .collect();
    lines.extend(
        report
            .collisions
            .iter()
            .map(|label| format!("collision: {label} exists under both names")),
    );
    lines
}

/// The one-line result of a completed mutation.
pub fn outcome_line(outcome: &Outcome) -> String {
    if outcome.recovered {
        return format!(
            "AGENTS: {} (recovered from mixed, {} restored). Disable again to turn off.",
            outcome.state, outcome.renamed
        );
    }
    if outcome.renamed == 0 {
        return format!("AGENTS: {} (already {})", outcome.state, outcome.state);
    }
    format!(
        "AGENTS: {} ({} renamed). {}",
        outcome.state,
        outcome.renamed,
        state_meaning(outcome.state)
    )
}

/// Notify about a completed mutation. Delivery is outside the transaction.
pub fn notify_outcome(outcome: &Outcome) {
    let body = if outcome.missing.is_empty() {
        state_meaning(outcome.state).to_string()
    } else {
        format!(
            "{}\nMissing: {}",
            state_meaning(outcome.state),
            outcome.missing.join(", ")
        )
    };
    let summary = if outcome.recovered {
        "Agent instructions recovered to on".to_string()
    } else {
        format!("Agent instructions: {}", outcome.state)
    };
    notify(&summary, &body, Urgency::Normal);
}

pub fn notify_error(error: &Error) {
    notify(
        "Agent instructions unchanged",
        &error.to_string(),
        Urgency::Critical,
    );
}

pub fn notify_report(report: &Report) {
    let mut body = state_meaning(report.state).to_string();
    for line in warnings(report) {
        body.push('\n');
        body.push_str(&line);
    }
    notify(
        &format!("Agent instructions: {}", report.state),
        &body,
        Urgency::Normal,
    );
}

pub enum Urgency {
    Normal,
    Critical,
}

/// Best effort: a missing notification daemon must never turn a successful
/// rename into a failure, so every error here is dropped on purpose.
fn notify(summary: &str, body: &str, urgency: Urgency) {
    let urgency = match urgency {
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    };
    let _ = Command::new("notify-send")
        .arg("--app-name=agent-instructions")
        .arg("--icon=preferences-desktop-notification")
        .arg(format!("--urgency={urgency}"))
        .arg("--")
        .arg(summary)
        .arg(body)
        .status();
}
