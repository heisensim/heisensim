use crate::injector::FaultEvent;
use serde::{Deserialize, Serialize};
/// Recording and replaying fault schedules.
use std::path::Path;

/// A fault schedule contains the initial simulation seed and an ordered list
/// of fault events. This allows for deterministic reproduction of any simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultSchedule {
    pub initial_seed: u64, // SimSeed
    pub events: Vec<FaultEvent>,
}

impl FaultSchedule {
    /// Create a new empty FaultSchedule with a given seed.
    pub fn new(seed: u64) -> Self {
        Self {
            initial_seed: seed,
            events: Vec::new(),
        }
    }

    /// Append a fault event to the schedule.
    pub fn record(&mut self, event: FaultEvent) {
        self.events.push(event);
    }

    /// Serialize the schedule to a JSON file.
    pub fn save(&self, _path: &Path) -> Result<(), std::io::Error> {
        todo!("Implement serialization to file")
    }

    /// Deserialize the schedule from a JSON file.
    pub fn load(_path: &Path) -> Result<Self, std::io::Error> {
        todo!("Implement deserialization from file")
    }

    /// Iterate over recorded events in order.
    pub fn iter(&self) -> impl Iterator<Item = &FaultEvent> {
        self.events.iter()
    }
}
