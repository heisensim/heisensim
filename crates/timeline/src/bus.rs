use crate::event::{EventKind, TimelineEvent};
use chrono::Utc;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use uuid::Uuid;

/// A thread-safe append-only timeline of events.
#[derive(Debug)]
pub struct Timeline {
    events: RwLock<Vec<TimelineEvent>>,
    start_time: Instant,
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    /// Create a new empty timeline, starting the clock.
    pub fn new() -> Self {
        Self {
            events: RwLock::new(Vec::new()),
            start_time: Instant::now(),
        }
    }

    /// Emit a new event into the timeline.
    pub fn emit(&self, kind: EventKind) {
        let event = TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: self.start_time.elapsed(),
            kind: kind.clone(),
        };

        // Structured tracing that bridges to OTel spans/events
        match &event.kind {
            EventKind::FaultInjected {
                fault_id,
                fault_kind,
                target,
                ..
            } => {
                tracing::info!(
                    fault.id = %fault_id,
                    fault.kind = %fault_kind,
                    fault.target = %target,
                    "fault.injected"
                );
            }
            EventKind::FaultReverted { fault_id } => {
                tracing::info!(fault.id = %fault_id, "fault.reverted");
            }
            EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                status_code,
            } => {
                tracing::debug!(
                    probe.name = %probe_name,
                    probe.latency_ms = latency_ms,
                    http.status_code = ?status_code,
                    "probe.success"
                );
            }
            EventKind::ProbeFailed {
                probe_name, error, ..
            } => {
                tracing::warn!(
                    probe.name = %probe_name,
                    error = %error,
                    "probe.failed"
                );
            }
            EventKind::ProbeTimeout {
                probe_name,
                timeout_ms,
            } => {
                tracing::warn!(
                    probe.name = %probe_name,
                    probe.timeout_ms = timeout_ms,
                    "probe.timeout"
                );
            }
            _ => {
                tracing::debug!(?event, "timeline.event");
            }
        }

        let mut events = self.events.write().expect("Timeline lock poisoned");
        events.push(event);
    }

    /// Retrieve a clone of all events currently in the timeline.
    pub fn events(&self) -> Vec<TimelineEvent> {
        let events = self.events.read().expect("Timeline lock poisoned");
        events.clone()
    }

    /// The number of events in the timeline.
    pub fn len(&self) -> usize {
        let events = self.events.read().expect("Timeline lock poisoned");
        events.len()
    }

    /// Returns true if there are no events in the timeline.
    pub fn is_empty(&self) -> bool {
        let events = self.events.read().expect("Timeline lock poisoned");
        events.is_empty()
    }
}

/// A clonable handle to a shared timeline.
#[derive(Debug, Clone)]
pub struct TimelineHandle {
    timeline: Arc<Timeline>,
}

impl Default for TimelineHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl TimelineHandle {
    /// Create a new timeline and return a handle to it.
    pub fn new() -> Self {
        Self {
            timeline: Arc::new(Timeline::new()),
        }
    }

    /// Emit a new event into the shared timeline.
    pub fn emit(&self, kind: EventKind) {
        self.timeline.emit(kind)
    }

    /// Retrieve a clone of all events currently in the timeline.
    pub fn events(&self) -> Vec<TimelineEvent> {
        self.timeline.events()
    }

    /// The number of events in the timeline.
    pub fn len(&self) -> usize {
        self.timeline.len()
    }

    /// Returns true if there are no events in the timeline.
    pub fn is_empty(&self) -> bool {
        self.timeline.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_timeline_emit_and_len() {
        let timeline = Timeline::new();
        assert!(timeline.is_empty());

        timeline.emit(EventKind::Note {
            message: "Test".to_string(),
        });

        assert_eq!(timeline.len(), 1);
        assert!(!timeline.is_empty());
    }

    #[test]
    fn test_timeline_handle_thread_safety() {
        let handle = TimelineHandle::new();
        let mut threads = Vec::new();

        for i in 0..10 {
            let handle_clone = handle.clone();
            threads.push(thread::spawn(move || {
                handle_clone.emit(EventKind::Note {
                    message: format!("Message {}", i),
                });
            }));
        }

        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(handle.len(), 10);

        // Verify all 10 events arrived (thread safety = no lost writes).
        // Note: ordering is NOT guaranteed for concurrent emits because
        // Instant::now() granularity + mutex acquisition order are both
        // non-deterministic across threads.
        let events = handle.events();
        assert_eq!(events.len(), 10);
    }

    #[test]
    fn test_fault_injected_event_fields() {
        let timeline = Timeline::new();
        let fault_id = Uuid::new_v4();
        timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "network_delay".to_string(),
            target: "pod_a".to_string(),
            duration_secs: Some(5.0),
        });

        let events = timeline.events();
        assert_eq!(events.len(), 1);
        if let EventKind::FaultInjected {
            fault_id: fid,
            fault_kind: fkind,
            target: tgt,
            duration_secs: dsecs,
        } = &events[0].kind
        {
            assert_eq!(*fid, fault_id);
            assert_eq!(fkind, "network_delay");
            assert_eq!(tgt, "pod_a");
            assert_eq!(*dsecs, Some(5.0));
        } else {
            panic!("Expected FaultInjected event");
        }
    }

    #[test]
    fn test_fault_reverted_event() {
        let timeline = Timeline::new();
        let fault_id = Uuid::new_v4();
        timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "crash".to_string(),
            target: "db".to_string(),
            duration_secs: None,
        });
        timeline.emit(EventKind::FaultReverted { fault_id });

        let events = timeline.events();
        assert_eq!(events.len(), 2);
        match &events[1].kind {
            EventKind::FaultReverted { fault_id: fid } => {
                assert_eq!(*fid, fault_id);
            }
            _ => panic!("Expected FaultReverted event"),
        }
    }

    #[test]
    fn test_probe_success_event() {
        let timeline = Timeline::new();
        timeline.emit(EventKind::ProbeSuccess {
            probe_name: "health_check".to_string(),
            latency_ms: 42,
            status_code: Some(200),
        });

        let events = timeline.events();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                status_code,
            } => {
                assert_eq!(probe_name, "health_check");
                assert_eq!(*latency_ms, 42);
                assert_eq!(*status_code, Some(200));
            }
            _ => panic!("Expected ProbeSuccess event"),
        }
    }

    #[test]
    fn test_probe_failed_event() {
        let timeline = Timeline::new();
        timeline.emit(EventKind::ProbeFailed {
            probe_name: "liveness".to_string(),
            error: "Connection refused".to_string(),
            latency_ms: Some(10),
        });

        let events = timeline.events();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            EventKind::ProbeFailed {
                probe_name,
                error,
                latency_ms,
            } => {
                assert_eq!(probe_name, "liveness");
                assert_eq!(error, "Connection refused");
                assert_eq!(*latency_ms, Some(10));
            }
            _ => panic!("Expected ProbeFailed event"),
        }
    }

    #[test]
    fn test_probe_timeout_event() {
        let timeline = Timeline::new();
        timeline.emit(EventKind::ProbeTimeout {
            probe_name: "read_query".to_string(),
            timeout_ms: 5000,
        });

        let events = timeline.events();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            EventKind::ProbeTimeout {
                probe_name,
                timeout_ms,
            } => {
                assert_eq!(probe_name, "read_query");
                assert_eq!(*timeout_ms, 5000);
            }
            _ => panic!("Expected ProbeTimeout event"),
        }
    }

    #[test]
    fn test_simulation_lifecycle() {
        let timeline = Timeline::new();
        timeline.emit(EventKind::SimulationStarted {
            seed: 12345,
            duration_secs: 60.0,
        });
        timeline.emit(EventKind::SimulationEnded {
            total_faults: 3,
            total_failures: 1,
        });

        let events = timeline.events();
        assert_eq!(events.len(), 2);
        match &events[0].kind {
            EventKind::SimulationStarted { seed, .. } => assert_eq!(*seed, 12345),
            _ => panic!("Expected SimulationStarted"),
        }
        match &events[1].kind {
            EventKind::SimulationEnded {
                total_faults,
                total_failures,
            } => {
                assert_eq!(*total_faults, 3);
                assert_eq!(*total_failures, 1);
            }
            _ => panic!("Expected SimulationEnded"),
        }
    }

    #[test]
    fn test_events_have_unique_ids() {
        let timeline = Timeline::new();
        for _ in 0..100 {
            timeline.emit(EventKind::Note {
                message: "msg".to_string(),
            });
        }

        let events = timeline.events();
        let mut ids = std::collections::HashSet::new();
        for e in events {
            assert!(ids.insert(e.id));
        }
        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn test_elapsed_monotonic() {
        use std::time::Duration;
        let timeline = Timeline::new();
        for _ in 0..5 {
            timeline.emit(EventKind::Note {
                message: "step".to_string(),
            });
            std::thread::sleep(Duration::from_millis(1));
        }

        let events = timeline.events();
        for i in 0..events.len() - 1 {
            assert!(events[i].elapsed <= events[i + 1].elapsed);
        }
    }

    use proptest::prelude::*;

    fn arb_event_kind() -> impl Strategy<Value = EventKind> {
        prop_oneof![
            (any::<u64>(), ".*", ".*").prop_map(|(_, kind, target)| EventKind::FaultInjected {
                fault_id: Uuid::new_v4(),
                fault_kind: kind,
                target,
                duration_secs: None,
            }),
            Just(EventKind::FaultReverted {
                fault_id: Uuid::new_v4()
            }),
            (".*", 0..10000u64).prop_map(|(name, lat)| EventKind::ProbeSuccess {
                probe_name: name,
                latency_ms: lat,
                status_code: Some(200),
            }),
            (".*", ".*").prop_map(|(name, err)| EventKind::ProbeFailed {
                probe_name: name,
                error: err,
                latency_ms: Some(100),
            }),
            (".*", 0..30000u64).prop_map(|(name, t)| EventKind::ProbeTimeout {
                probe_name: name,
                timeout_ms: t,
            }),
            ".*".prop_map(|msg| EventKind::Note { message: msg }),
        ]
    }

    proptest! {
        #[test]
        fn proptest_any_event_kind_emittable(kind in arb_event_kind()) {
            let timeline = Timeline::new();
            timeline.emit(kind.clone());
            let events = timeline.events();
            assert_eq!(events.len(), 1);
        }

        #[test]
        fn proptest_timeline_preserves_all_events(count in 1..200usize) {
            let timeline = Timeline::new();
            for _ in 0..count {
                timeline.emit(EventKind::Note { message: "msg".to_string() });
            }
            assert_eq!(timeline.len(), count);
            assert_eq!(timeline.events().len(), count);
        }

        #[test]
        fn proptest_concurrent_emits_never_lose_events(
            n_threads in 1..50usize,
            m_events in 1..10usize,
        ) {
            let handle = TimelineHandle::new();
            let mut threads = Vec::new();

            for _ in 0..n_threads {
                let h = handle.clone();
                threads.push(std::thread::spawn(move || {
                    for _ in 0..m_events {
                        h.emit(EventKind::Note { message: "test".to_string() });
                    }
                }));
            }

            for t in threads {
                t.join().unwrap();
            }

            assert_eq!(handle.len(), n_threads * m_events);
        }
    }
}
