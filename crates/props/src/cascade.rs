//! No Cascading Failures property — asserts faults don't cause unexpected probe failures.
//!
//! Uses explicit `allowed_failing_probes` rather than guessing topology from strings.
//! A fault on pod A should only cause probes related to A (or explicitly allowed) to fail.

use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::time::Duration;

/// Asserts that faults don't cause unexpected cascading failures.
pub struct NoCascade {
    name: String,
    window_seconds: f64,
    allowed_failing_probes: Vec<String>,
}

impl NoCascade {
    /// Create a new cascade detection property.
    ///
    /// * `window_seconds` — how long after a fault to watch for cascading failures
    /// * `allowed_failing_probes` — probe names that are expected to fail during faults
    pub fn new(
        name: impl Into<String>,
        window_seconds: f64,
        allowed_failing_probes: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            window_seconds,
            allowed_failing_probes,
        }
    }

    /// Check if a probe failure is expected given the fault target.
    fn is_expected_failure(&self, probe_name: &str, fault_target: &str) -> bool {
        // Probe name contains the fault target (e.g. "redis-health" matches fault on "redis")
        if probe_name.contains(fault_target) {
            return true;
        }
        // Probe is in the explicit allow list
        self.allowed_failing_probes
            .iter()
            .any(|allowed| probe_name.contains(allowed.as_str()))
    }
}

impl TimelineProperty for NoCascade {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts that faults don't cause unexpected cascading failures"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let window_dur = Duration::from_secs_f64(self.window_seconds);
        let mut cascades: Vec<String> = Vec::new();

        for (i, event) in events.iter().enumerate() {
            let target = match &event.kind {
                EventKind::FaultInjected { target, .. } => target.clone(),
                _ => continue,
            };

            let fault_time = event.elapsed;
            let window_end = fault_time + window_dur;

            // Check probe failures in the window after this fault
            for window_event in events[i + 1..].iter() {
                if window_event.elapsed > window_end {
                    break;
                }

                let probe_name = match &window_event.kind {
                    EventKind::ProbeFailed { probe_name, .. }
                    | EventKind::ProbeTimeout { probe_name, .. } => probe_name,
                    _ => continue,
                };

                if !self.is_expected_failure(probe_name, &target) {
                    cascades.push(format!(
                        "  ❌ fault on \"{}\" caused \"{}\" to fail at +{:.1}s",
                        target,
                        probe_name,
                        window_event
                            .elapsed
                            .saturating_sub(fault_time)
                            .as_secs_f64()
                    ));
                }
            }
        }

        let expected = "no cascading failures".to_string();

        if cascades.is_empty() {
            PropertyVerdict::pass(&self.name, &expected, "none detected")
        } else {
            let actual = format!("{} cascade(s)", cascades.len());
            PropertyVerdict::fail(&self.name, expected, actual).with_details(cascades)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn event(elapsed_secs: u64, kind: EventKind) -> TimelineEvent {
        TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs(elapsed_secs),
            kind,
        }
    }

    #[test]
    fn test_no_cascade_pass() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                10,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "crash".into(),
                    target: "redis".into(),
                    duration_secs: None,
                },
            ),
            event(
                12,
                EventKind::ProbeFailed {
                    probe_name: "redis-health".into(),
                    error: "down".into(),
                    latency_ms: None,
                },
            ),
            // API probe succeeds — no cascade
            event(
                13,
                EventKind::ProbeSuccess {
                    probe_name: "api-health".into(),
                    latency_ms: 10,
                    status_code: Some(200),
                },
            ),
        ];

        let prop = NoCascade::new("no-cascade", 30.0, vec![]);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Expected pass: {:?}", verdict);
    }

    #[test]
    fn test_cascade_detected() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                10,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "crash".into(),
                    target: "redis".into(),
                    duration_secs: None,
                },
            ),
            event(
                12,
                EventKind::ProbeFailed {
                    probe_name: "redis-health".into(),
                    error: "down".into(),
                    latency_ms: None,
                },
            ),
            // API also failed — cascade!
            event(
                14,
                EventKind::ProbeFailed {
                    probe_name: "api-health".into(),
                    error: "503".into(),
                    latency_ms: None,
                },
            ),
        ];

        let prop = NoCascade::new("no-cascade", 30.0, vec![]);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "Expected fail: {:?}", verdict);
        assert!(verdict.details[0].contains("api-health"));
    }

    #[test]
    fn test_allowed_probes() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                10,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "crash".into(),
                    target: "redis".into(),
                    duration_secs: None,
                },
            ),
            event(
                14,
                EventKind::ProbeFailed {
                    probe_name: "api-health".into(),
                    error: "503".into(),
                    latency_ms: None,
                },
            ),
        ];

        // api-health is in the allow list — should pass
        let prop = NoCascade::new("no-cascade", 30.0, vec!["api-health".into()]);
        let verdict = prop.evaluate(&events);
        assert!(
            verdict.passed,
            "Expected pass with allow list: {:?}",
            verdict
        );
    }
}
