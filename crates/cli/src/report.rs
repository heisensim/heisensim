use heisensim_timeline::{
    event::{EventKind, TimelineEvent},
    query,
};
use std::io::{self, IsTerminal};

const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const BOLD_WHITE: &str = "\x1b[1;37m";

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let mins = secs / 60;
    let rem_secs = secs % 60;
    if mins > 0 {
        format!("{}m {:02}s", mins, rem_secs)
    } else {
        let ms = d.subsec_millis();
        if secs > 0 {
            format!("{}.{:03}s", secs, ms)
        } else {
            format!("{}ms", ms)
        }
    }
}

fn format_timestamp(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let mins = secs / 60;
    let rem_secs = secs % 60;
    let ms = d.subsec_millis();
    format!("{:02}:{:02}.{:03}", mins, rem_secs, ms)
}

pub fn render_terminal_report(events: &[TimelineEvent]) {
    let use_color = io::stdout().is_terminal();
    let summary = query::summary(events);

    // Find seed from SimulationStarted
    let seed = events
        .iter()
        .find_map(|e| {
            if let EventKind::SimulationStarted { seed, .. } = &e.kind {
                Some(*seed)
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut report = String::new();

    report.push_str(&format!(
        "╔══════════════════════════════════════════════════════════════╗\n"
    ));

    let header = if use_color {
        format!("{}HEISENSIM REPORT{}", BOLD_WHITE, RESET)
    } else {
        "HEISENSIM REPORT".to_string()
    };
    report.push_str(&format!("║  {:<42} seed: 0x{:04X}   ║\n", header, seed));

    report.push_str(&format!(
        "╠══════════════════════════════════════════════════════════════╣\n"
    ));

    let duration_str = format!(
        "{:02}m {:02}s",
        summary.duration.as_secs() / 60,
        summary.duration.as_secs() % 60
    );
    report.push_str(&format!(
        "║  Duration: {:<7} │  Faults: {:<3} │  Failures: {:<11} ║\n",
        duration_str, summary.total_faults, summary.total_failures
    ));
    report.push_str(&format!(
        "╚══════════════════════════════════════════════════════════════╝\n\n"
    ));

    report.push_str("Timeline:\n");

    for event in events {
        let ts = format_timestamp(event.elapsed);

        let (icon, category, msg, color) = match &event.kind {
            EventKind::FaultInjected {
                target, fault_kind, ..
            } => {
                let icon = if fault_kind.contains("network") || fault_kind.contains("latency") {
                    "🌐"
                } else {
                    "💥"
                };
                (icon, "FAULT", format!("{} on {}", fault_kind, target), CYAN)
            }
            EventKind::FaultReverted { .. } => {
                ("✅", "REVERT", "Fault reverted".to_string(), GREEN)
            }
            EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                ..
            } => (
                "✅",
                "PROBE",
                format!("{} OK ({}ms)", probe_name, latency_ms),
                GREEN,
            ),
            EventKind::ProbeFailed {
                probe_name, error, ..
            } => (
                "❌",
                "PROBE",
                format!("{} FAILED ({})", probe_name, error),
                RED,
            ),
            EventKind::ProbeTimeout {
                probe_name,
                timeout_ms,
            } => (
                "⚠️",
                "PROBE",
                format!("{} SLOW ({}ms)", probe_name, timeout_ms),
                YELLOW,
            ),
            EventKind::WorkloadStarted { command, .. } => {
                ("🚀", "START", format!("Workload: {}", command), BOLD_WHITE)
            }
            EventKind::WorkloadExited { exit_code } => (
                "🏁",
                "EXIT",
                format!("Workload exited with code {}", exit_code),
                BOLD_WHITE,
            ),
            EventKind::SimulationStarted { .. } => {
                ("🚀", "START", "Simulation started".to_string(), BOLD_WHITE)
            }
            EventKind::SimulationEnded { .. } => {
                ("🏁", "END", "Simulation ended".to_string(), BOLD_WHITE)
            }
            EventKind::Note { message } => ("📝", "NOTE", message.clone(), BOLD_WHITE),
        };

        let cat_str = if use_color {
            format!("{}{}{}", color, category, RESET)
        } else {
            category.to_string()
        };

        report.push_str(&format!("  {}  {}  {:<16} {}\n", ts, icon, cat_str, msg));
    }

    report.push_str("\nFindings:\n");

    let faults: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FaultInjected { .. }))
        .collect();
    if faults.is_empty() {
        report.push_str("  No faults injected.\n");
    } else {
        for (i, fault_event) in faults.iter().enumerate() {
            if let EventKind::FaultInjected {
                fault_id,
                target,
                fault_kind,
                ..
            } = &fault_event.kind
            {
                if let Some(latency) = query::fault_to_detection_latency(events, *fault_id) {
                    report.push_str(&format!(
                        "  {}. {} {} → probe failure in {}\n",
                        i + 1,
                        target,
                        fault_kind,
                        format_duration(latency)
                    ));
                } else {
                    report.push_str(&format!(
                        "  {}. {} {} → NO PROBE FAILURE DETECTED\n",
                        i + 1,
                        target,
                        fault_kind
                    ));
                }
            }
        }
    }

    println!("{}", report);
}

pub fn render_markdown_report(events: &[TimelineEvent]) -> String {
    let summary = query::summary(events);
    let seed = events
        .iter()
        .find_map(|e| {
            if let EventKind::SimulationStarted { seed, .. } = &e.kind {
                Some(*seed)
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut report = String::new();
    report.push_str("# Heisensim Report\n\n");
    report.push_str(&format!("* **Seed**: `0x{:04X}`\n", seed));
    let duration_str = format!(
        "{:02}m {:02}s",
        summary.duration.as_secs() / 60,
        summary.duration.as_secs() % 60
    );
    report.push_str(&format!("* **Duration**: {}\n", duration_str));
    report.push_str(&format!("* **Faults**: {}\n", summary.total_faults));
    report.push_str(&format!("* **Failures**: {}\n\n", summary.total_failures));

    report.push_str("## Timeline\n\n");
    report.push_str("| Time | Event | Details |\n");
    report.push_str("|------|-------|---------|\n");

    for event in events {
        let ts = format_timestamp(event.elapsed);

        let (icon, category, msg) = match &event.kind {
            EventKind::FaultInjected {
                target, fault_kind, ..
            } => {
                let icon = if fault_kind.contains("network") || fault_kind.contains("latency") {
                    "🌐"
                } else {
                    "💥"
                };
                (icon, "FAULT", format!("{} on {}", fault_kind, target))
            }
            EventKind::FaultReverted { .. } => ("✅", "REVERT", "Fault reverted".to_string()),
            EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                ..
            } => (
                "✅",
                "PROBE",
                format!("{} OK ({}ms)", probe_name, latency_ms),
            ),
            EventKind::ProbeFailed {
                probe_name, error, ..
            } => ("❌", "PROBE", format!("{} FAILED ({})", probe_name, error)),
            EventKind::ProbeTimeout {
                probe_name,
                timeout_ms,
            } => (
                "⚠️",
                "PROBE",
                format!("{} SLOW ({}ms)", probe_name, timeout_ms),
            ),
            EventKind::WorkloadStarted { command, .. } => {
                ("🚀", "START", format!("Workload: {}", command))
            }
            EventKind::WorkloadExited { exit_code } => (
                "🏁",
                "EXIT",
                format!("Workload exited with code {}", exit_code),
            ),
            EventKind::SimulationStarted { .. } => {
                ("🚀", "START", "Simulation started".to_string())
            }
            EventKind::SimulationEnded { .. } => ("🏁", "END", "Simulation ended".to_string()),
            EventKind::Note { message } => ("📝", "NOTE", message.clone()),
        };

        report.push_str(&format!("| `{}` | {} {} | {} |\n", ts, icon, category, msg));
    }

    report.push_str("\n## Findings\n\n");
    let faults: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FaultInjected { .. }))
        .collect();
    if faults.is_empty() {
        report.push_str("No faults injected.\n");
    } else {
        for (i, fault_event) in faults.iter().enumerate() {
            if let EventKind::FaultInjected {
                fault_id,
                target,
                fault_kind,
                ..
            } = &fault_event.kind
            {
                if let Some(latency) = query::fault_to_detection_latency(events, *fault_id) {
                    report.push_str(&format!(
                        "{}. **{} {}** → probe failure in {}\n",
                        i + 1,
                        target,
                        fault_kind,
                        format_duration(latency)
                    ));
                } else {
                    report.push_str(&format!(
                        "{}. **{} {}** → NO PROBE FAILURE DETECTED\n",
                        i + 1,
                        target,
                        fault_kind
                    ));
                }
            }
        }
    }

    report
}

pub fn render_json_report(events: &[TimelineEvent]) -> String {
    let summary = query::summary(events);
    let report = serde_json::json!({
        "summary": summary,
        "timeline": events,
    });
    serde_json::to_string_pretty(&report).unwrap_or_default()
}
