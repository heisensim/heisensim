use heisensim_props::PropertyVerdict;
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::collections::{HashMap, HashSet};

pub fn render_html_report(
    events: &[TimelineEvent],
    verdicts: &[PropertyVerdict],
    seed: u64,
    duration_secs: f64,
) -> String {
    let mut html = String::new();

    // 1. Discover pods/targets
    let mut pods = HashSet::new();
    for event in events {
        match &event.kind {
            EventKind::FaultInjected { target, .. } => {
                pods.insert(target.clone());
            }
            EventKind::ProbeSuccess { probe_name, .. } => {
                pods.insert(probe_name.clone());
            }
            EventKind::ProbeFailed { probe_name, .. } => {
                pods.insert(probe_name.clone());
            }
            EventKind::ProbeTimeout { probe_name, .. } => {
                pods.insert(probe_name.clone());
            }
            _ => {}
        }
    }
    let mut pods: Vec<_> = pods.into_iter().collect();
    pods.sort();

    // Stats
    let total_events = events.len();
    let total_faults = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FaultInjected { .. }))
        .count();
    let probe_successes = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::ProbeSuccess { .. }))
        .count();
    let probe_attempts = events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::ProbeSuccess { .. }
                    | EventKind::ProbeFailed { .. }
                    | EventKind::ProbeTimeout { .. }
            )
        })
        .count();
    let success_rate = if probe_attempts > 0 {
        (probe_successes as f64 / probe_attempts as f64) * 100.0
    } else {
        100.0
    };

    html.push_str("<!DOCTYPE html>\n");
    html.push_str("<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"UTF-8\">\n");
    html.push_str("<title>Heisensim Report</title>\n");
    html.push_str("<style>\n");
    html.push_str("body { background-color: #0d1117; color: #c9d1d9; font-family: system-ui, sans-serif; margin: 0; padding: 20px; }\n");
    html.push_str(
        "h1, h2 { color: #c9d1d9; border-bottom: 1px solid #30363d; padding-bottom: 8px; }\n",
    );
    html.push_str(".stats { display: flex; gap: 20px; margin-bottom: 20px; }\n");
    html.push_str(".stat-card { background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 15px; flex: 1; }\n");
    html.push_str(
        ".stat-card h3 { margin-top: 0; margin-bottom: 5px; font-size: 14px; color: #8b949e; }\n",
    );
    html.push_str(".stat-card .value { font-size: 24px; font-weight: bold; }\n");

    html.push_str(".timeline { margin-top: 20px; border: 1px solid #30363d; border-radius: 6px; background: #010409; overflow-x: auto; padding-bottom: 10px; }\n");
    html.push_str(".timeline-grid { position: relative; min-width: 800px; margin: 20px; }\n");
    html.push_str(".lane { position: relative; height: 40px; border-bottom: 1px dashed #30363d; margin-bottom: 10px; }\n");
    html.push_str(".lane-label { position: absolute; left: -10px; top: 10px; transform: translateX(-100%); width: 120px; text-align: right; font-family: monospace; font-size: 12px; color: #8b949e; overflow: hidden; text-overflow: ellipsis; }\n");

    html.push_str(".event { position: absolute; top: 50%; transform: translate(-50%, -50%); border-radius: 50%; }\n");
    html.push_str(
        ".probe-success { width: 8px; height: 8px; background-color: #3fb950; z-index: 2; }\n",
    );
    html.push_str(
        ".probe-failed { width: 10px; height: 10px; background-color: #f85149; z-index: 3; }\n",
    );
    html.push_str(
        ".probe-timeout { width: 10px; height: 10px; background-color: #d29922; z-index: 3; }\n",
    );
    html.push_str(".fault-injected { position: absolute; top: 10%; height: 80%; background-color: rgba(248, 81, 73, 0.3); border-left: 2px solid #f85149; border-right: 2px solid #f85149; transform: none; border-radius: 2px; z-index: 1; }\n");
    html.push_str(".fault-label { position: absolute; top: -15px; left: 2px; font-size: 10px; color: #f85149; white-space: nowrap; font-family: monospace; }\n");

    html.push_str("table { width: 100%; border-collapse: collapse; margin-top: 20px; }\n");
    html.push_str("th, td { padding: 10px; text-align: left; border-bottom: 1px solid #30363d; font-family: monospace; }\n");
    html.push_str("th { background: #161b22; color: #8b949e; }\n");
    html.push_str(".pass { color: #3fb950; }\n");
    html.push_str(".fail { color: #f85149; }\n");
    html.push_str("</style>\n");
    html.push_str("</head>\n<body>\n");

    // Header
    html.push_str("<h1>heisensim report</h1>\n");
    html.push_str(&format!(
        "<p>Seed: <code>0x{:04X}</code> | Duration: {:.1}s</p>\n",
        seed, duration_secs
    ));

    // Summary Stats
    html.push_str("<div class=\"stats\">\n");
    html.push_str(&format!(
        "<div class=\"stat-card\"><h3>Total Events</h3><div class=\"value\">{}</div></div>\n",
        total_events
    ));
    html.push_str(&format!(
        "<div class=\"stat-card\"><h3>Faults Injected</h3><div class=\"value\">{}</div></div>\n",
        total_faults
    ));
    html.push_str(&format!("<div class=\"stat-card\"><h3>Probe Success Rate</h3><div class=\"value\">{:.1}%</div></div>\n", success_rate));
    html.push_str("</div>\n");

    // Timeline Visualization
    html.push_str("<h2>Timeline</h2>\n");
    html.push_str("<div class=\"timeline\">\n");
    html.push_str("<div class=\"timeline-grid\" style=\"margin-left: 140px;\">\n"); // space for lane labels

    // Grid lines (every 10%)
    for i in 0..=10 {
        let pct = i * 10;
        html.push_str(&format!("<div style=\"position: absolute; left: {}%; top: 0; bottom: 0; border-left: 1px solid #30363d; z-index: 0;\"></div>\n", pct));
    }

    // Render each pod's lane
    let mut fault_starts = HashMap::new(); // fault_id -> (start_elapsed, target, fault_kind)
    for event in events {
        if let EventKind::FaultInjected {
            fault_id,
            target,
            fault_kind,
            ..
        } = &event.kind
        {
            fault_starts.insert(
                *fault_id,
                (
                    event.elapsed.as_secs_f64(),
                    target.clone(),
                    fault_kind.clone(),
                ),
            );
        }
    }

    let actual_duration = if duration_secs <= 0.0 {
        1.0
    } else {
        duration_secs
    };

    for pod in &pods {
        html.push_str(&format!("<div class=\"lane\" data-pod=\"{}\">\n", pod));
        html.push_str(&format!("<div class=\"lane-label\">{}</div>\n", pod));

        // Draw faults for this pod
        for event in events {
            if let EventKind::FaultReverted { fault_id } = &event.kind {
                if let Some((start, target, kind)) = fault_starts.get(fault_id) {
                    if target == pod {
                        let end = event.elapsed.as_secs_f64();
                        let start_pct = (start / actual_duration) * 100.0;
                        let end_pct = (end / actual_duration) * 100.0;
                        let width = (end_pct - start_pct).max(0.5); // minimum width

                        html.push_str(&format!(
                            "<div class=\"fault-injected\" style=\"left: {}%; width: {}%;\">\n",
                            start_pct, width
                        ));
                        html.push_str(&format!("<span class=\"fault-label\">{}</span>\n", kind));
                        html.push_str("</div>\n");
                    }
                }
            }

            // Draw probes
            let (is_pod, pct, class) = match &event.kind {
                EventKind::ProbeSuccess { probe_name, .. } if probe_name == pod => (
                    true,
                    (event.elapsed.as_secs_f64() / actual_duration) * 100.0,
                    "probe-success",
                ),
                EventKind::ProbeFailed { probe_name, .. } if probe_name == pod => (
                    true,
                    (event.elapsed.as_secs_f64() / actual_duration) * 100.0,
                    "probe-failed",
                ),
                EventKind::ProbeTimeout { probe_name, .. } if probe_name == pod => (
                    true,
                    (event.elapsed.as_secs_f64() / actual_duration) * 100.0,
                    "probe-timeout",
                ),
                _ => (false, 0.0, ""),
            };

            if is_pod {
                html.push_str(&format!(
                    "<span class=\"event {}\" style=\"left: {}%\"></span>\n",
                    class, pct
                ));
            }
        }

        html.push_str("</div>\n");
    }

    html.push_str("</div>\n</div>\n");

    // Verdicts Table
    html.push_str("<h2>Properties</h2>\n");
    html.push_str("<table>\n");
    html.push_str("<thead><tr><th>Status</th><th>Property</th><th>Expected</th><th>Actual</th></tr></thead>\n");
    html.push_str("<tbody>\n");

    for v in verdicts {
        let (status, class) = if v.passed {
            ("✅ PASS", "pass")
        } else {
            ("❌ FAIL", "fail")
        };
        html.push_str("<tr>\n");
        html.push_str(&format!("<td class=\"{}\">{}</td>\n", class, status));
        html.push_str(&format!("<td>{}</td>\n", v.property_name));
        html.push_str(&format!("<td>{}</td>\n", v.expected));
        html.push_str(&format!("<td>{}</td>\n", v.actual));
        html.push_str("</tr>\n");

        if !v.details.is_empty() {
            html.push_str("<tr><td colspan=\"4\">\n");
            html.push_str("<details><summary>Details</summary><ul>\n");
            for detail in &v.details {
                html.push_str(&format!("<li>{}</li>\n", detail));
            }
            html.push_str("</ul></details>\n");
            html.push_str("</td></tr>\n");
        }
    }

    html.push_str("</tbody>\n</table>\n");

    html.push_str("</body>\n</html>\n");

    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_render_html_basic() {
        let event = TimelineEvent {
            id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            elapsed: Duration::from_secs(10),
            kind: EventKind::SimulationStarted {
                seed: 1234,
                duration_secs: 60.0,
            },
        };
        let html = render_html_report(&[event], &[], 1234, 60.0);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("0x04D2"));
    }

    #[test]
    fn test_render_html_empty() {
        let html = render_html_report(&[], &[], 0, 10.0);
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn test_render_html_verdicts() {
        let verdict = PropertyVerdict {
            passed: true,
            property_name: "test-prop".to_string(),
            expected: "foo".to_string(),
            actual: "bar".to_string(),
            details: vec!["detail 1".to_string()],
        };
        let html = render_html_report(&[], &[verdict], 0, 10.0);
        assert!(html.contains("test-prop"));
        assert!(html.contains("detail 1"));
    }
}
