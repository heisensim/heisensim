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
}
