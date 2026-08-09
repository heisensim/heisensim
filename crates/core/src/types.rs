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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
