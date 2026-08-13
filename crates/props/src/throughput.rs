use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::time::Duration;

/// Asserts that probe success rate doesn't drop below `min_per_minute` during any sliding window.
pub struct Throughput {
    name: String,
    min_per_minute: f64,
    window_seconds: f64,
}

impl Throughput {
    pub fn new(name: impl Into<String>, min_per_minute: f64, window_seconds: f64) -> Self {
        Self {
            name: name.into(),
            min_per_minute,
            window_seconds,
        }
    }
}

impl TimelineProperty for Throughput {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts that probe success rate doesn't drop below a minimum during any sliding window"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        if events.is_empty() {
            return PropertyVerdict::pass(&self.name, "throughput", "no events");
        }

        let window_dur = Duration::from_secs_f64(self.window_seconds);
        let mut min_rate = f64::MAX;
        let mut details = Vec::new();

        let last_elapsed = events.last().map(|e| e.elapsed).unwrap_or_default();

        if last_elapsed < window_dur {
            // Simulation was shorter than the window, just check the whole thing if we want,
            // or pass automatically. Let's check the whole duration.
            let count = events
                .iter()
                .filter(|e| matches!(e.kind, EventKind::ProbeSuccess { .. }))
                .count();

            let rate = if last_elapsed.as_secs_f64() > 0.0 {
                count as f64 * (60.0 / last_elapsed.as_secs_f64())
            } else {
                0.0
            };

            min_rate = rate;
            if rate < self.min_per_minute {
                details.push(format!(
                    "  ❌ simulation too short and rate {:.1} < {:.1}",
                    rate, self.min_per_minute
                ));
            }
        } else {
            // Check window starting at each event's time
            for event in events {
                let start = event.elapsed;
                let end = start + window_dur;

                if end > last_elapsed {
                    continue; // Skip windows that go beyond the simulation end
                }

                let count = events
                    .iter()
                    .filter(|e| e.elapsed >= start && e.elapsed <= end)
                    .filter(|e| matches!(e.kind, EventKind::ProbeSuccess { .. }))
                    .count();

                let rate = count as f64 * (60.0 / self.window_seconds);

                if rate < min_rate {
                    min_rate = rate;
                }
            }
        }

        let expected = format!(
            ">= {} per min over {}s",
            self.min_per_minute, self.window_seconds
        );
        let actual = if min_rate == f64::MAX {
            "no valid windows".to_string()
        } else {
            format!("{:.1} per min", min_rate)
        };

        if min_rate < self.min_per_minute && min_rate != f64::MAX {
            details.push(format!(
                "  ❌ worst window had {:.1} successes/min",
                min_rate
            ));
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

    fn event(elapsed_secs: f64, kind: EventKind) -> TimelineEvent {
        TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs_f64(elapsed_secs),
            kind,
        }
    }

    #[test]
    fn test_throughput_pass() {
        let events = vec![
            event(
                0.0,
                EventKind::SimulationStarted {
                    seed: 1,
                    duration_secs: 120.0,
                },
            ),
            event(
                10.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                30.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                50.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                70.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                90.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                110.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                120.0,
                EventKind::SimulationStarted {
                    seed: 1,
                    duration_secs: 120.0,
                },
            ), // Just to pad the end
        ];

        // 3 successes per 60s window => 3/min
        let prop = Throughput::new("t", 2.0, 60.0);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Expected pass");
    }

    #[test]
    fn test_throughput_fail() {
        let events = vec![
            event(
                0.0,
                EventKind::SimulationStarted {
                    seed: 1,
                    duration_secs: 120.0,
                },
            ),
            event(
                10.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            // Big gap
            event(
                110.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
            event(
                120.0,
                EventKind::SimulationStarted {
                    seed: 1,
                    duration_secs: 120.0,
                },
            ),
        ];

        // Gap from 10 to 110 means window [10.1, 70.1] has 0 successes => rate 0.
        let prop = Throughput::new("t", 2.0, 60.0);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "Expected fail");
    }

    #[test]
    fn test_throughput_no_events() {
        let events = vec![];
        let prop = Throughput::new("t", 2.0, 60.0);
        let verdict = prop.evaluate(&events);
        assert!(
            verdict.passed,
            "Empty events should pass or at least not crash"
        );
    }
}
