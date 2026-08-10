//! Recovery time property — asserts probes recover within a threshold after each fault.
//!
//! Algorithm (E9-hardened):
//! 1. For each FaultInjected, open a window of `max_window` seconds
//! 2. If zero probe failures in window → recovery = 0s (system absorbed it)
//! 3. If failures found, find first ProbeSuccess after last failure → recovery time
//! 4. If no recovery in window → "never recovered", FAIL

use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::time::Duration;

/// Asserts that after each fault injection, probes recover within `max_seconds`.
pub struct RecoveryTime {
    name: String,
    max_seconds: f64,
    max_window_seconds: f64,
}

impl RecoveryTime {
    /// Create a new recovery time property.
    pub fn new(name: impl Into<String>, max_seconds: f64) -> Self {
        Self {
            name: name.into(),
            max_seconds,
            max_window_seconds: max_seconds * 2.0,
        }
    }

    /// Set the maximum observation window (defaults to 2× max_seconds).
    pub fn with_max_window(mut self, seconds: f64) -> Self {
        self.max_window_seconds = seconds;
        self
    }
}

impl TimelineProperty for RecoveryTime {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts probes recover within a time threshold after fault injection"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let max_dur = Duration::from_secs_f64(self.max_seconds);
        let window_dur = Duration::from_secs_f64(self.max_window_seconds);
        let mut worst_recovery: Option<Duration> = None;
        let mut details = Vec::new();
        let mut any_failed = false;

        // Find all fault injection events
        for (i, event) in events.iter().enumerate() {
            let (fault_id, target) = match &event.kind {
                EventKind::FaultInjected {
                    fault_id, target, ..
                } => (*fault_id, target.clone()),
                _ => continue,
            };

            let fault_time = event.elapsed;
            let window_end = fault_time + window_dur;

            // Collect probe events in the window after this fault
            let window_events: Vec<&TimelineEvent> = events[i + 1..]
                .iter()
                .take_while(|e| e.elapsed <= window_end)
                .collect();

            // Find failures in the window
            let failures: Vec<&TimelineEvent> = window_events
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        EventKind::ProbeFailed { .. } | EventKind::ProbeTimeout { .. }
                    )
                })
                .copied()
                .collect();

            if failures.is_empty() {
                // System absorbed the fault — no probe failures at all
                details.push(format!("  fault {} on {}: absorbed (0s)", fault_id, target));
                // Recovery time is 0, which is always within threshold
                continue;
            }

            // Find the last failure time
            let last_failure_time = failures.last().unwrap().elapsed;

            // Find first success after the last failure
            let recovery_event = window_events.iter().find(|e| {
                e.elapsed > last_failure_time && matches!(e.kind, EventKind::ProbeSuccess { .. })
            });

            match recovery_event {
                Some(recovery) => {
                    let recovery_time = recovery.elapsed.saturating_sub(fault_time);
                    if recovery_time > max_dur {
                        any_failed = true;
                        details.push(format!(
                            "  ❌ fault {} on {}: recovered in {:.1}s (exceeds {:.0}s)",
                            fault_id,
                            target,
                            recovery_time.as_secs_f64(),
                            self.max_seconds
                        ));
                    } else {
                        details.push(format!(
                            "  ✅ fault {} on {}: recovered in {:.1}s",
                            fault_id,
                            target,
                            recovery_time.as_secs_f64()
                        ));
                    }

                    worst_recovery = Some(match worst_recovery {
                        Some(w) => w.max(recovery_time),
                        None => recovery_time,
                    });
                }
                None => {
                    // Never recovered within window
                    any_failed = true;
                    details.push(format!(
                        "  ❌ fault {} on {}: never recovered within {:.0}s window",
                        fault_id, target, self.max_window_seconds
                    ));
                    worst_recovery = Some(window_dur);
                }
            }
        }

        let actual = match worst_recovery {
            Some(d) => format!("{:.1}s", d.as_secs_f64()),
            None => "no faults".to_string(),
        };

        let expected = format!("recovery < {:.0}s", self.max_seconds);

        if any_failed {
            PropertyVerdict::fail(&self.name, expected, actual).with_details(details)
        } else {
            PropertyVerdict::pass(&self.name, expected, actual).with_details(details)
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
    fn test_recovery_pass() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                0,
                EventKind::SimulationStarted {
                    seed: 1,
                    duration_secs: 60.0,
                },
            ),
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
                    error: "connection refused".into(),
                    latency_ms: None,
                },
            ),
            event(
                18,
                EventKind::ProbeSuccess {
                    probe_name: "redis-health".into(),
                    latency_ms: 5,
                    status_code: None,
                },
            ),
        ];

        let prop = RecoveryTime::new("fast-recovery", 30.0);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Expected pass, got: {:?}", verdict);
    }

    #[test]
    fn test_recovery_fail_too_slow() {
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
                    error: "timeout".into(),
                    latency_ms: None,
                },
            ),
            event(
                55,
                EventKind::ProbeSuccess {
                    probe_name: "redis-health".into(),
                    latency_ms: 5,
                    status_code: None,
                },
            ),
        ];

        let prop = RecoveryTime::new("fast-recovery", 30.0);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "Expected fail, got: {:?}", verdict);
    }

    #[test]
    fn test_recovery_absorbed() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                10,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "latency".into(),
                    target: "api".into(),
                    duration_secs: Some(5.0),
                },
            ),
            event(
                12,
                EventKind::ProbeSuccess {
                    probe_name: "api-health".into(),
                    latency_ms: 300,
                    status_code: Some(200),
                },
            ),
        ];

        let prop = RecoveryTime::new("fast-recovery", 10.0);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Absorbed fault should pass");
    }

    #[test]
    fn test_recovery_never_recovers() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                10,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "crash".into(),
                    target: "db".into(),
                    duration_secs: None,
                },
            ),
            event(
                12,
                EventKind::ProbeFailed {
                    probe_name: "db-health".into(),
                    error: "down".into(),
                    latency_ms: None,
                },
            ),
            event(
                15,
                EventKind::ProbeFailed {
                    probe_name: "db-health".into(),
                    error: "down".into(),
                    latency_ms: None,
                },
            ),
        ];

        let prop = RecoveryTime::new("fast-recovery", 10.0);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "Never-recovers should fail");
    }
}
