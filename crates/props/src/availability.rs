//! Availability property — asserts probe success rate meets a minimum threshold.
//!
//! Supports optional `probe_filter` to scope to specific probes,
//! preventing high-traffic probes from masking outages of low-traffic services.

use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};

/// Asserts that probe success rate ≥ `min_percent`.
pub struct Availability {
    name: String,
    min_percent: f64,
    probe_filter: Option<String>,
}

impl Availability {
    /// Create a new availability property.
    pub fn new(name: impl Into<String>, min_percent: f64) -> Self {
        Self {
            name: name.into(),
            min_percent,
            probe_filter: None,
        }
    }

    /// Only evaluate probes whose name contains this substring.
    pub fn with_probe_filter(mut self, filter: impl Into<String>) -> Self {
        self.probe_filter = Some(filter.into());
        self
    }
}

impl TimelineProperty for Availability {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts probe success rate meets a minimum percentage"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let mut successes: u64 = 0;
        let mut failures: u64 = 0;

        for event in events {
            match &event.kind {
                EventKind::ProbeSuccess { probe_name, .. } if self.matches_filter(probe_name) => {
                    successes += 1;
                }
                EventKind::ProbeFailed { probe_name, .. }
                | EventKind::ProbeTimeout { probe_name, .. }
                    if self.matches_filter(probe_name) =>
                {
                    failures += 1;
                }
                _ => {}
            }
        }

        let total = successes + failures;
        if total == 0 {
            return PropertyVerdict::pass(
                &self.name,
                format!("avail ≥ {:.1}%", self.min_percent),
                "no probes",
            );
        }

        let actual_percent = (successes as f64 / total as f64) * 100.0;
        let expected = format!("avail ≥ {:.1}%", self.min_percent);
        let actual = format!("{:.1}% ({}/{})", actual_percent, successes, total);

        if actual_percent >= self.min_percent {
            PropertyVerdict::pass(&self.name, expected, actual)
        } else {
            let mut details = vec![format!(
                "  {} successes, {} failures out of {} total",
                successes, failures, total
            )];
            if let Some(ref filter) = self.probe_filter {
                details.push(format!("  probe filter: \"{}\"", filter));
            }
            PropertyVerdict::fail(&self.name, expected, actual).with_details(details)
        }
    }
}

impl Availability {
    fn matches_filter(&self, probe_name: &str) -> bool {
        match &self.probe_filter {
            Some(filter) => probe_name.contains(filter.as_str()),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::time::Duration;
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
    fn test_availability_pass() {
        let events: Vec<_> = (0..100)
            .map(|i| {
                event(
                    i,
                    EventKind::ProbeSuccess {
                        probe_name: "api".into(),
                        latency_ms: 10,
                        status_code: Some(200),
                    },
                )
            })
            .collect();

        let prop = Availability::new("ha", 99.0);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed);
    }

    #[test]
    fn test_availability_fail() {
        let mut events: Vec<_> = (0..95)
            .map(|i| {
                event(
                    i,
                    EventKind::ProbeSuccess {
                        probe_name: "api".into(),
                        latency_ms: 10,
                        status_code: Some(200),
                    },
                )
            })
            .collect();

        for i in 95..100 {
            events.push(event(
                i,
                EventKind::ProbeFailed {
                    probe_name: "api".into(),
                    error: "down".into(),
                    latency_ms: None,
                },
            ));
        }

        let prop = Availability::new("ha", 99.0);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed);
        assert!(verdict.actual.contains("95.0%"));
    }

    #[test]
    fn test_availability_with_filter() {
        let events = vec![
            event(
                1,
                EventKind::ProbeSuccess {
                    probe_name: "redis-health".into(),
                    latency_ms: 5,
                    status_code: None,
                },
            ),
            event(
                2,
                EventKind::ProbeFailed {
                    probe_name: "redis-health".into(),
                    error: "down".into(),
                    latency_ms: None,
                },
            ),
            // This one is api — should be excluded by filter
            event(
                3,
                EventKind::ProbeSuccess {
                    probe_name: "api-health".into(),
                    latency_ms: 10,
                    status_code: Some(200),
                },
            ),
        ];

        let prop = Availability::new("redis-avail", 90.0).with_probe_filter("redis");
        let verdict = prop.evaluate(&events);
        // 1 success, 1 failure = 50% < 90%
        assert!(!verdict.passed);
        assert!(verdict.actual.contains("50.0%"));
    }

    #[test]
    fn test_availability_no_events() {
        let prop = Availability::new("ha", 99.0);
        let verdict = prop.evaluate(&[]);
        assert!(verdict.passed);
    }
}
