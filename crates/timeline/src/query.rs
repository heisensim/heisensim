use crate::event::{EventKind, TimelineEvent};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Summary statistics of a timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelineSummary {
    /// Total number of events
    pub total_events: usize,
    /// Total number of injected faults
    pub total_faults: usize,
    /// Total number of probe failures
    pub total_failures: usize,
    /// Total number of probe successes
    pub total_probe_successes: usize,
    /// Total duration of the timeline (last event elapsed time)
    pub duration: Duration,
    /// Elapsed time of the first failure, if any
    pub first_failure_elapsed: Option<Duration>,
}

/// Find the first probe failure event after a specific fault was injected.
pub fn first_failure_after(events: &[TimelineEvent], fault_id: Uuid) -> Option<&TimelineEvent> {
    let mut fault_time = None;

    for event in events {
        if let EventKind::FaultInjected { fault_id: id, .. } = &event.kind {
            if *id == fault_id {
                fault_time = Some(event.elapsed);
                break;
            }
        }
    }

    let fault_time = fault_time?;

    events.iter().find(|e| {
        e.elapsed >= fault_time
            && matches!(
                e.kind,
                EventKind::ProbeFailed { .. } | EventKind::ProbeTimeout { .. }
            )
    })
}

/// Calculate the latency between a fault injection and the first subsequent probe failure.
pub fn fault_to_detection_latency(events: &[TimelineEvent], fault_id: Uuid) -> Option<Duration> {
    let fault_event = events.iter().find(|e| {
        if let EventKind::FaultInjected { fault_id: id, .. } = &e.kind {
            *id == fault_id
        } else {
            false
        }
    })?;

    let failure_event = first_failure_after(events, fault_id)?;

    Some(failure_event.elapsed.saturating_sub(fault_event.elapsed))
}

/// Get all events within a specific elapsed time window.
pub fn events_in_window(
    events: &[TimelineEvent],
    start: Duration,
    end: Duration,
) -> Vec<&TimelineEvent> {
    events
        .iter()
        .filter(|e| e.elapsed >= start && e.elapsed <= end)
        .collect()
}

/// Count the total number of probe failures (failed or timeout).
pub fn failure_count(events: &[TimelineEvent]) -> usize {
    events
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::ProbeFailed { .. } | EventKind::ProbeTimeout { .. }
            )
        })
        .count()
}

/// Count the total number of faults injected.
pub fn fault_count(events: &[TimelineEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FaultInjected { .. }))
        .count()
}

/// Generate a summary of the timeline.
pub fn summary(events: &[TimelineEvent]) -> TimelineSummary {
    let mut total_faults = 0;
    let mut total_failures = 0;
    let mut total_probe_successes = 0;
    let mut first_failure_elapsed = None;

    for event in events {
        match &event.kind {
            EventKind::FaultInjected { .. } => total_faults += 1,
            EventKind::ProbeFailed { .. } | EventKind::ProbeTimeout { .. } => {
                total_failures += 1;
                if first_failure_elapsed.is_none() {
                    first_failure_elapsed = Some(event.elapsed);
                }
            }
            EventKind::ProbeSuccess { .. } => total_probe_successes += 1,
            _ => {}
        }
    }

    let duration = events.last().map(|e| e.elapsed).unwrap_or(Duration::ZERO);

    TimelineSummary {
        total_events: events.len(),
        total_faults,
        total_failures,
        total_probe_successes,
        duration,
        first_failure_elapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_event(elapsed_secs: u64, kind: EventKind) -> TimelineEvent {
        TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs(elapsed_secs),
            kind,
        }
    }

    #[test]
    fn test_queries() {
        let fault_id = Uuid::new_v4();
        let events = vec![
            create_event(
                1,
                EventKind::ProbeSuccess {
                    probe_name: "A".into(),
                    latency_ms: 10,
                    status_code: None,
                },
            ),
            create_event(
                2,
                EventKind::FaultInjected {
                    fault_id,
                    fault_kind: "drop".into(),
                    target: "net".into(),
                    duration_secs: None,
                },
            ),
            create_event(
                3,
                EventKind::ProbeSuccess {
                    probe_name: "A".into(),
                    latency_ms: 10,
                    status_code: None,
                },
            ),
            create_event(
                5,
                EventKind::ProbeFailed {
                    probe_name: "A".into(),
                    error: "timeout".into(),
                    latency_ms: None,
                },
            ),
        ];

        assert_eq!(fault_count(&events), 1);
        assert_eq!(failure_count(&events), 1);

        let latency = fault_to_detection_latency(&events, fault_id).unwrap();
        assert_eq!(latency, Duration::from_secs(3));

        let window = events_in_window(&events, Duration::from_secs(2), Duration::from_secs(4));
        assert_eq!(window.len(), 2);

        let sum = summary(&events);
        assert_eq!(sum.total_events, 4);
        assert_eq!(sum.total_faults, 1);
        assert_eq!(sum.total_failures, 1);
        assert_eq!(sum.total_probe_successes, 2);
        assert_eq!(sum.duration, Duration::from_secs(5));
        assert_eq!(sum.first_failure_elapsed, Some(Duration::from_secs(5)));
    }
}
