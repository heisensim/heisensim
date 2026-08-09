//! Built-in invariant properties for simulation verification.

use crate::checker::{Property, PropertyResult, Severity, SimulationSnapshot};
use heisensim_core::process::ProcessState;
use heisensim_core::types::VirtualTime;

/// Property verifying that no process in the simulation has entered a crashed state.
///
/// Under deterministic simulation, unexpected crashes are caught immediately
/// along with the exact fault injection seed that triggered them.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoCrash;

impl NoCrash {
    /// Creates a new `NoCrash` property instance.
    pub fn new() -> Self {
        Self
    }
}

impl Property for NoCrash {
    fn name(&self) -> &str {
        "NoCrash"
    }

    fn description(&self) -> &str {
        "Verifies that no simulated process has crashed unexpectedly"
    }

    fn check(&self, state: &SimulationSnapshot) -> PropertyResult {
        let crashed_procs: Vec<_> = state
            .processes
            .iter()
            .filter(|proc| proc.state == ProcessState::Crashed)
            .collect();

        if let Some(crashed) = crashed_procs.first() {
            PropertyResult::fail(
                Severity::Error,
                state.current_time,
                format!(
                    "Process '{}' (PID {}) has crashed at virtual time {}",
                    crashed.name, crashed.pid, state.current_time
                ),
            )
        } else {
            PropertyResult::pass(
                state.current_time,
                format!(
                    "All {} processes are healthy (no crashes detected)",
                    state.processes.len()
                ),
            )
        }
    }
}

/// Property verifying that at least one process has made progress within a specified virtual duration.
///
/// Helps detect deadlocks, livelocks, or stalled execution loops in distributed protocols.
#[derive(Debug, Clone, Copy)]
pub struct NoHang {
    /// Maximum allowed duration without process progress (in virtual time).
    pub timeout: VirtualTime,
}

impl NoHang {
    /// Creates a `NoHang` property with the given virtual time timeout.
    pub fn new(timeout: VirtualTime) -> Self {
        Self { timeout }
    }

    /// Creates a `NoHang` property with a timeout specified in seconds.
    pub fn from_secs(secs: u64) -> Self {
        Self {
            timeout: VirtualTime::from_secs(secs),
        }
    }
}

impl Default for NoHang {
    fn default() -> Self {
        Self::from_secs(30)
    }
}

impl Property for NoHang {
    fn name(&self) -> &str {
        "NoHang"
    }

    fn description(&self) -> &str {
        "Verifies that processes make forward progress without deadlocks or livelocks"
    }

    fn check(&self, state: &SimulationSnapshot) -> PropertyResult {
        if state.processes.is_empty() {
            return PropertyResult::pass(
                state.current_time,
                "No processes present in simulation snapshot",
            );
        }

        let any_progress = state.processes.iter().any(|proc| {
            if state.current_time >= proc.last_progress {
                (state.current_time - proc.last_progress) <= self.timeout
            } else {
                true
            }
        });

        if any_progress {
            PropertyResult::pass(
                state.current_time,
                "Simulation processes are actively making progress",
            )
        } else {
            PropertyResult::fail(
                Severity::Error,
                state.current_time,
                format!(
                    "No process has made progress in the last {} virtual seconds (timeout: {})",
                    self.timeout.as_secs(),
                    self.timeout
                ),
            )
        }
    }
}

/// Property verifying data durability across crashes and network partitions.
///
/// Acknowledged writes must remain readable and consistent following system recovery.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoDataLoss;

impl NoDataLoss {
    /// Creates a new `NoDataLoss` property instance.
    pub fn new() -> Self {
        Self
    }
}

impl Property for NoDataLoss {
    fn name(&self) -> &str {
        "NoDataLoss"
    }

    fn description(&self) -> &str {
        "Verifies that acknowledged writes remain durable and readable after recovery"
    }

    fn check(&self, _state: &SimulationSnapshot) -> PropertyResult {
        todo!("NoDataLoss property checking is not yet implemented")
    }
}

/// Property verifying strict linearizability of operations.
///
/// Operations must appear to take effect atomically at a point in time between
/// their invocation and response.
#[derive(Debug, Default, Clone, Copy)]
pub struct Linearizable;

impl Linearizable {
    /// Creates a new `Linearizable` property instance.
    pub fn new() -> Self {
        Self
    }
}

impl Property for Linearizable {
    fn name(&self) -> &str {
        "Linearizable"
    }

    fn description(&self) -> &str {
        "Verifies that operations across processes execute in a linearizable order"
    }

    fn check(&self, _state: &SimulationSnapshot) -> PropertyResult {
        todo!("Linearizable property checking is not yet implemented")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::{ProcessInfo, PropertyChecker, SimulationSnapshot};
    use heisensim_core::process::ProcessState;
    use heisensim_core::types::VirtualTime;

    #[test]
    fn test_no_crash_pass() {
        let snapshot = SimulationSnapshot::new(
            VirtualTime::from_secs(5),
            vec![ProcessInfo {
                pid: 1,
                node_id: 100,
                name: "etcd-1".into(),
                state: ProcessState::Running,
                last_progress: VirtualTime::from_secs(5),
            }],
            0,
        );

        let prop = NoCrash::new();
        let res = prop.check(&snapshot);
        assert!(res.passed);
    }

    #[test]
    fn test_no_crash_fail() {
        let snapshot = SimulationSnapshot::new(
            VirtualTime::from_secs(5),
            vec![ProcessInfo {
                pid: 1,
                node_id: 100,
                name: "etcd-1".into(),
                state: ProcessState::Crashed,
                last_progress: VirtualTime::from_secs(2),
            }],
            0,
        );

        let prop = NoCrash::new();
        let res = prop.check(&snapshot);
        assert!(!res.passed);
        assert_eq!(res.severity, Severity::Error);
    }

    #[test]
    fn test_no_hang_pass() {
        let snapshot = SimulationSnapshot::new(
            VirtualTime::from_secs(10),
            vec![ProcessInfo {
                pid: 1,
                node_id: 100,
                name: "etcd-1".into(),
                state: ProcessState::Running,
                last_progress: VirtualTime::from_secs(8),
            }],
            0,
        );

        let prop = NoHang::from_secs(5);
        let res = prop.check(&snapshot);
        assert!(res.passed);
    }

    #[test]
    fn test_no_hang_fail() {
        let snapshot = SimulationSnapshot::new(
            VirtualTime::from_secs(20),
            vec![ProcessInfo {
                pid: 1,
                node_id: 100,
                name: "etcd-1".into(),
                state: ProcessState::Blocked,
                last_progress: VirtualTime::from_secs(5),
            }],
            0,
        );

        let prop = NoHang::from_secs(10);
        let res = prop.check(&snapshot);
        assert!(!res.passed);
    }

    #[test]
    fn test_property_checker() {
        let mut checker = PropertyChecker::new();
        checker.add_property(Box::new(NoCrash::new()));
        checker.add_property(Box::new(NoHang::from_secs(30)));

        let snapshot = SimulationSnapshot::new(
            VirtualTime::from_secs(5),
            vec![ProcessInfo {
                pid: 1,
                node_id: 100,
                name: "etcd-1".into(),
                state: ProcessState::Running,
                last_progress: VirtualTime::from_secs(5),
            }],
            0,
        );

        assert!(!checker.has_violations(&snapshot));
        assert_eq!(checker.check_all(&snapshot).len(), 2);
    }
}
