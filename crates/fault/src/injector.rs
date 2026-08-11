/// Fault injection engine.
use serde::{Deserialize, Serialize};

/// Types of faults that can be injected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FaultKind {
    NetworkPartition,
    ProcessCrash,
    ClockSkew(u64),
    PacketLoss(f64),
    DiskFailure,
}

/// Represents a specific fault event injected at a virtual time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultEvent {
    pub time: u64, // VirtualTime
    pub kind: FaultKind,
    pub target: Vec<u64>,      // NodeIds
    pub duration: Option<u64>, // Option<VirtualTime>
}

/// Injects faults into a running simulation.
pub struct FaultInjector {
    // virtual_network: Arc<Mutex<VirtualNetwork>>,
    // process_table: Arc<Mutex<ProcessTable>>,
    // virtual_clock: Arc<Mutex<VirtualClock>>,
    // rng: StdRng,
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl FaultInjector {
    /// Create a new FaultInjector.
    pub fn new() -> Self {
        Self {}
    }

    /// Create a network partition between two sets of nodes.
    pub fn inject_partition(
        &mut self,
        _nodes_a: Vec<u64>,
        _nodes_b: Vec<u64>,
        _duration: Option<u64>,
    ) {
        todo!()
    }

    /// Crash a specific process.
    pub fn inject_crash(&mut self, _node_id: u64) {
        todo!()
    }

    /// Introduce clock drift for a specific node.
    pub fn inject_clock_skew(&mut self, _node_id: u64, _skew_ms: u64) {
        todo!()
    }

    /// Set a packet loss rate for the network.
    pub fn inject_packet_loss(&mut self, _probability: f64, _duration: Option<u64>) {
        todo!()
    }

    /// Simulate disk I/O errors for a specific node.
    pub fn inject_disk_failure(&mut self, _node_id: u64, _duration: Option<u64>) {
        todo!()
    }

    /// Pick a random fault type and target to inject.
    pub fn inject_random_fault(&mut self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_injector_new() {
        let _injector = FaultInjector::new();
        let _injector2 = FaultInjector::default();
    }
}
