use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

/// Represents a simulated node in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

/// Represents time in the virtual simulation (in nanoseconds).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct VirtualTime(pub u64);

impl VirtualTime {
    /// Creates a VirtualTime from milliseconds.
    pub fn from_millis(millis: u64) -> Self {
        Self(millis * 1_000_000)
    }

    /// Creates a VirtualTime from seconds.
    pub fn from_secs(secs: u64) -> Self {
        Self(secs * 1_000_000_000)
    }

    /// Returns the virtual time as milliseconds.
    pub fn as_millis(&self) -> u64 {
        self.0 / 1_000_000
    }

    /// Returns the virtual time as seconds.
    pub fn as_secs(&self) -> u64 {
        self.0 / 1_000_000_000
    }

    /// Converts to std::time::Duration for timeline events.
    pub fn as_std_duration(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.0)
    }
}

impl Add for VirtualTime {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for VirtualTime {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for VirtualTime {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for VirtualTime {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl fmt::Display for VirtualTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

/// Represents a simulated process ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProcessId(pub u32);

/// Represents the kinds of faults that can be injected into the simulation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaultKind {
    NetworkPartition,
    ProcessCrash,
    DiskFailure,
    ClockSkew,
    PacketLoss,
    PacketDelay,
    Custom(String),
}

/// Represents the state of the overall simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SimulationState {
    Initializing,
    Running,
    Paused,
    Completed,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_time() {
        let t1 = VirtualTime::from_millis(500);
        let t2 = VirtualTime::from_millis(500);
        assert_eq!(t1 + t2, VirtualTime::from_secs(1));

        let mut t3 = VirtualTime::from_secs(2);
        t3 -= VirtualTime::from_millis(500);
        assert_eq!(t3.as_millis(), 1500);

        let mut t4 = VirtualTime::from_secs(1);
        t4 += VirtualTime::from_secs(1);
        assert_eq!(t4.as_secs(), 2);

        assert_eq!(format!("{}", t1), "500000000ns");
    }

    #[test]
    fn test_types_serde() {
        let nid = NodeId(42);
        let json = serde_json::to_string(&nid).unwrap();
        let nid2: NodeId = serde_json::from_str(&json).unwrap();
        assert_eq!(nid, nid2);

        let state = SimulationState::Running;
        let json = serde_json::to_string(&state).unwrap();
        let state2: SimulationState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, state2);

        let fault = FaultKind::Custom("foo".to_string());
        let json = serde_json::to_string(&fault).unwrap();
        let fault2: FaultKind = serde_json::from_str(&json).unwrap();
        assert_eq!(fault, fault2);
    }
}
