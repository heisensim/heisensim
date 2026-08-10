//! Error budget property — asserts no more than N consecutive probe failures per probe.

use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::collections::HashMap;

/// Asserts no more than `max_consecutive` consecutive failures for any single probe.
pub struct ErrorBudget {
    name: String,
    max_consecutive: u32,
}

impl ErrorBudget {
    /// Create a new error budget property.
    pub fn new(name: impl Into<String>, max_consecutive: u32) -> Self {
        Self {
            name: name.into(),
            max_consecutive,
        }
    }
}

impl TimelineProperty for ErrorBudget {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts no probe exceeds a consecutive failure threshold"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        // Track per-probe: current streak, worst streak
        let mut current_streak: HashMap<String, u32> = HashMap::new();
        let mut worst_streak: HashMap<String, u32> = HashMap::new();

        for event in events {
            match &event.kind {
                EventKind::ProbeFailed { probe_name, .. }
                | EventKind::ProbeTimeout { probe_name, .. } => {
                    let streak = current_streak.entry(probe_name.clone()).or_insert(0);
                    *streak += 1;
                    let worst = worst_streak.entry(probe_name.clone()).or_insert(0);
                    *worst = (*worst).max(*streak);
                }
                EventKind::ProbeSuccess { probe_name, .. } => {
                    current_streak.insert(probe_name.clone(), 0);
                    // Ensure worst is tracked even for probes that only succeed
                    worst_streak.entry(probe_name.clone()).or_insert(0);
                }
                _ => {}
            }
        }

        // Also account for trailing streaks
        for (name, streak) in &current_streak {
            let worst = worst_streak.entry(name.clone()).or_insert(0);
            *worst = (*worst).max(*streak);
        }

        // Find the worst offender
        let mut worst_probe = None;
        let mut worst_count: u32 = 0;
        for (name, count) in &worst_streak {
            if *count > worst_count {
                worst_count = *count;
                worst_probe = Some(name.clone());
            }
        }

        let expected = format!("max {} consecutive", self.max_consecutive);
        let actual = format!("{} ({})", worst_count, worst_probe.as_deref().unwrap_or("none"));

        if worst_count <= self.max_consecutive {
            PropertyVerdict::pass(&self.name, expected, actual)
        } else {
            let details = worst_streak
                .iter()
                .filter(|(_, count)| **count > self.max_consecutive)
                .map(|(name, count)| {
                    format!(
                        "  ❌ {}: {} consecutive failures (max: {})",
                        name, count, self.max_consecutive
                    )
                })
                .collect();
            PropertyVerdict::fail(&self.name, expected, actual).with_details(details)
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
    fn test_error_budget_pass() {
        let events = vec![
            event(
                1,
                EventKind::ProbeFailed {
                    probe_name: "api".into(),
                    error: "err".into(),
                    latency_ms: None,
                },
            ),
            event(
                2,
                EventKind::ProbeFailed {
                    probe_name: "api".into(),
                    error: "err".into(),
                    latency_ms: None,
                },
            ),
            event(
                3,
                EventKind::ProbeSuccess {
                    probe_name: "api".into(),
                    latency_ms: 5,
                    status_code: None,
                },
            ),
        ];

        let prop = ErrorBudget::new("bounded", 3);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed);
    }

    #[test]
    fn test_error_budget_fail() {
        let events: Vec<_> = (0..10)
            .map(|i| {
                event(
                    i,
                    EventKind::ProbeFailed {
                        probe_name: "db".into(),
                        error: "down".into(),
                        latency_ms: None,
                    },
                )
            })
            .collect();

        let prop = ErrorBudget::new("bounded", 5);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed);
    }

    #[test]
    fn test_error_budget_multiple_probes() {
        let events = vec![
            event(
                1,
                EventKind::ProbeFailed {
                    probe_name: "api".into(),
                    error: "e".into(),
                    latency_ms: None,
                },
            ),
            event(
                2,
                EventKind::ProbeSuccess {
                    probe_name: "api".into(),
                    latency_ms: 5,
                    status_code: None,
                },
            ),
            // db has a long streak
            event(
                3,
                EventKind::ProbeFailed {
                    probe_name: "db".into(),
                    error: "e".into(),
                    latency_ms: None,
                },
            ),
            event(
                4,
                EventKind::ProbeFailed {
                    probe_name: "db".into(),
                    error: "e".into(),
                    latency_ms: None,
                },
            ),
            event(
                5,
                EventKind::ProbeFailed {
                    probe_name: "db".into(),
                    error: "e".into(),
                    latency_ms: None,
                },
            ),
        ];

        // api streak = 1, db streak = 3
        let prop = ErrorBudget::new("bounded", 2);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed);
        assert!(verdict.actual.contains("3"));
    }

    #[test]
    fn test_error_budget_no_events() {
        let prop = ErrorBudget::new("bounded", 5);
        let verdict = prop.evaluate(&[]);
        assert!(verdict.passed);
    }
}
