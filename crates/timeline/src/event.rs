use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Unique timeline event representing a single occurrence in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Unique event ID
    pub id: Uuid,
    /// Wall clock timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Time elapsed since simulation start
    pub elapsed: Duration,
    /// The specific kind of event
    pub kind: EventKind,
}

/// Categorized event types for chaos testing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventKind {
    /// A fault was injected into the system
    FaultInjected {
        fault_id: Uuid,
        fault_kind: String,
        target: String,
        duration_secs: Option<f64>,
    },
    /// A fault was reverted or expired
    FaultReverted { fault_id: Uuid },
    /// A probe successfully executed
    ProbeSuccess {
        probe_name: String,
        latency_ms: u64,
        status_code: Option<u16>,
    },
    /// A probe failed during execution
    ProbeFailed {
        probe_name: String,
        error: String,
        latency_ms: Option<u64>,
    },
    /// A probe timed out
    ProbeTimeout {
        probe_name: String,
        timeout_ms: u64,
    },
    /// The workload being tested started
    WorkloadStarted { command: String, pid: Option<u32> },
    /// The workload being tested exited
    WorkloadExited { exit_code: i32 },
    /// The simulation itself started
    SimulationStarted { seed: u64, duration_secs: f64 },
    /// The simulation ended
    SimulationEnded {
        total_faults: usize,
        total_failures: usize,
    },
    /// Arbitrary note for debugging or logging
    Note { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event = TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs(5),
            kind: EventKind::SimulationStarted {
                seed: 42,
                duration_secs: 60.0,
            },
        };

        let serialized = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: TimelineEvent =
            serde_json::from_str(&serialized).expect("Failed to deserialize");
        
        assert_eq!(event.id, deserialized.id);
        assert_eq!(event.elapsed, deserialized.elapsed);
    }
}
