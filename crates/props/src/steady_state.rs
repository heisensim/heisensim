use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::time::Duration;

/// Asserts the system returns to pre-fault behavior within `max_recovery_seconds`.
pub struct SteadyState {
    name: String,
    max_recovery_seconds: f64,
    baseline_seconds: f64,
}

impl SteadyState {
    pub fn new(name: impl Into<String>, max_recovery_seconds: f64, baseline_seconds: f64) -> Self {
        Self {
            name: name.into(),
            max_recovery_seconds,
            baseline_seconds,
        }
    }
}

impl TimelineProperty for SteadyState {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts the system returns to pre-fault behavior within a given time after the last fault is reverted"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let baseline_dur = Duration::from_secs_f64(self.baseline_seconds);
        let max_recovery_dur = Duration::from_secs_f64(self.max_recovery_seconds);

        let mut details = Vec::new();

        // 1. Compute baseline success rate
        let baseline_count = events
            .iter()
            .filter(|e| e.elapsed <= baseline_dur)
            .filter(|e| matches!(e.kind, EventKind::ProbeSuccess { .. }))
            .count();
        let baseline_rate = baseline_count as f64 / self.baseline_seconds;

        // 2. Find the last FaultReverted event
        let last_revert = events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::FaultReverted { .. }))
            .map(|e| e.elapsed)
            .max();

        if let Some(revert_time) = last_revert {
            // We need to find at least one window of size `baseline_seconds`
            // starting between `revert_time` and `revert_time + max_recovery_seconds`
            // where the rate is >= 0.95 * baseline_rate.

            let search_start = revert_time;
            let search_end = revert_time + max_recovery_dur;
            let last_elapsed = events.last().map(|e| e.elapsed).unwrap_or_default();

            let threshold = baseline_rate * 0.95;
            let mut found_steady_state = false;
            let mut best_rate = 0.0;

            // We can check windows starting at `revert_time` and at each event in the search range
            let mut check_points = vec![revert_time, search_end.min(last_elapsed)];
            for e in events {
                if e.elapsed >= search_start && e.elapsed <= search_end {
                    check_points.push(e.elapsed);
                }
            }

            for start in check_points {
                let end = start + baseline_dur;
                if end > last_elapsed {
                    continue; // Skip if window goes beyond timeline
                }

                let count = events
                    .iter()
                    .filter(|e| e.elapsed >= start && e.elapsed <= end)
                    .filter(|e| matches!(e.kind, EventKind::ProbeSuccess { .. }))
                    .count();

                let rate = count as f64 / self.baseline_seconds;
                if rate > best_rate {
                    best_rate = rate;
                }

                if rate >= threshold {
                    found_steady_state = true;
                    break;
                }
            }

            let expected = format!(
                "rate >= {:.2}/s within {}s of revert",
                threshold, self.max_recovery_seconds
            );
            let actual = format!("best post-revert rate was {:.2}/s", best_rate);

            if found_steady_state || threshold == 0.0 {
                details.push(format!(
                    "  ✅ baseline rate was {:.2}/s, post-revert rate recovered",
                    baseline_rate
                ));
                PropertyVerdict::pass(&self.name, expected, actual).with_details(details)
            } else {
                details.push(format!("  ❌ baseline rate was {:.2}/s (threshold {:.2}/s), but system stayed at best {:.2}/s", baseline_rate, threshold, best_rate));
                PropertyVerdict::fail(&self.name, expected, actual).with_details(details)
            }
        } else {
            // No revert events found. If there are injected events, fail.
            let injected = events
                .iter()
                .any(|e| matches!(e.kind, EventKind::FaultInjected { .. }));
            if injected {
                details.push("  ❌ faults were injected but never reverted".to_string());
                PropertyVerdict::fail(
                    &self.name,
                    "faults reverted".to_string(),
                    "never reverted".to_string(),
                )
                .with_details(details)
            } else {
                details.push("  ✅ no faults injected".to_string());
                PropertyVerdict::pass(
                    &self.name,
                    "steady state".to_string(),
                    "no faults".to_string(),
                )
                .with_details(details)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn event(elapsed_secs: f64, kind: EventKind) -> TimelineEvent {
        TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs_f64(elapsed_secs),
            kind,
        }
    }

    #[test]
    fn test_steadystate_pass() {
        let fid = Uuid::new_v4();
        let events = vec![
            // Baseline 0..10
            event(
                1.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                5.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                9.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            // Fault 10..20
            event(
                10.0,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "crash".into(),
                    target: "redis".into(),
                    duration_secs: None,
                },
            ),
            event(20.0, EventKind::FaultReverted { fault_id: fid }),
            // Recovery 20..30
            event(
                21.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                25.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                29.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                30.0,
                EventKind::SimulationStarted {
                    seed: 1,
                    duration_secs: 30.0,
                },
            ), // padding
        ];

        let prop = SteadyState::new("steady", 10.0, 10.0);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Expected pass, got {:?}", verdict);
    }

    #[test]
    fn test_steadystate_fail() {
        let fid = Uuid::new_v4();
        let events = vec![
            // Baseline 0..10 -> 3 successes -> 0.3/s
            event(
                1.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                5.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                9.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            // Fault 10..20
            event(
                10.0,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "crash".into(),
                    target: "redis".into(),
                    duration_secs: None,
                },
            ),
            event(20.0, EventKind::FaultReverted { fault_id: fid }),
            // Recovery 20..30 -> only 1 success -> 0.1/s (less than 0.3 * 0.95)
            event(
                21.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                35.0,
                EventKind::SimulationStarted {
                    seed: 1,
                    duration_secs: 35.0,
                },
            ), // padding
        ];

        let prop = SteadyState::new("steady", 10.0, 10.0);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "Expected fail, got {:?}", verdict);
    }
}
