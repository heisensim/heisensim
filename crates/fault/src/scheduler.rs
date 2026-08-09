//! Fault scheduler for Kubernetes targets

use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Types of faults that can be injected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultType {
    /// Crash the target pod.
    Crash,
    /// Introduce latency to the target pod.
    Latency { delay_ms: u32, jitter_ms: u32 },
    /// Partition the target pod from the network.
    Partition,
}

/// A scheduled fault to be injected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledFault {
    /// The type of fault to inject.
    pub fault_type: FaultType,
    /// The target pod name.
    pub target_pod: String,
    /// The namespace of the target pod.
    pub namespace: String,
}

/// Scheduler for faults.
pub struct FaultScheduler {
    pub seed: u64,
    pub rng: StdRng,
    pub enabled_faults: Vec<FaultType>,
    pub target_pods: Vec<String>,
    pub namespace: String,
}

impl FaultScheduler {
    /// Creates a new fault scheduler.
    pub fn new(seed: u64, enabled_faults: Vec<FaultType>, target_pods: Vec<String>, namespace: String) -> Self {
        Self {
            seed,
            rng: StdRng::seed_from_u64(seed),
            enabled_faults,
            target_pods,
            namespace,
        }
    }

    /// Returns the next fault to inject.
    pub fn next_fault(&mut self) -> Option<ScheduledFault> {
        if self.enabled_faults.is_empty() || self.target_pods.is_empty() {
            return None;
        }

        let fault_idx = self.rng.gen_range(0..self.enabled_faults.len());
        let pod_idx = self.rng.gen_range(0..self.target_pods.len());

        Some(ScheduledFault {
            fault_type: self.enabled_faults[fault_idx].clone(),
            target_pod: self.target_pods[pod_idx].clone(),
            namespace: self.namespace.clone(),
        })
    }

    /// Returns a random delay between min and max seconds.
    pub fn next_delay(&mut self, min_secs: f64, max_secs: f64) -> Duration {
        if min_secs >= max_secs {
            return Duration::from_secs_f64(min_secs);
        }
        let delay = self.rng.gen_range(min_secs..max_secs);
        Duration::from_secs_f64(delay)
    }
}
