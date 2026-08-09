use crate::types::{NodeId, VirtualTime};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A timer registered in the virtual clock.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timer {
    /// The time at which this timer should wake.
    pub wake_time: VirtualTime,
    /// The target node associated with this timer.
    pub target: NodeId,
    /// A unique identifier for the callback or event.
    pub callback_id: u64,
}

/// The virtual clock managing simulation time and timers.
#[derive(Debug, Default)]
pub struct VirtualClock {
    current_time: VirtualTime,
    timers: BinaryHeap<Reverse<Timer>>,
    next_callback_id: u64,
}

impl VirtualClock {
    /// Creates a new `VirtualClock` starting at time 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the current virtual time.
    pub fn now(&self) -> VirtualTime {
        self.current_time
    }

    /// Advances the clock to the specified time, returning an error if it's in the past.
    pub fn advance_to(&mut self, time: VirtualTime) {
        if time > self.current_time {
            self.current_time = time;
        }
    }

    /// Registers a timer to wake a node after a duration. Returns the callback id.
    pub fn sleep(&mut self, node: NodeId, duration: VirtualTime) -> u64 {
        let wake_time = self.current_time + duration;
        let callback_id = self.next_callback_id;
        self.next_callback_id += 1;

        self.timers.push(Reverse(Timer {
            wake_time,
            target: node,
            callback_id,
        }));

        callback_id
    }

    /// Advances to the next timer (if any) and returns all expired timers up to the new time.
    pub fn tick(&mut self) -> Vec<Timer> {
        let mut expired = Vec::new();

        if let Some(Reverse(next_timer)) = self.timers.peek() {
            let next_time = next_timer.wake_time;
            if next_time > self.current_time {
                self.current_time = next_time;
            }

            while let Some(Reverse(timer)) = self.timers.peek() {
                if timer.wake_time <= self.current_time {
                    expired.push(self.timers.pop().unwrap().0);
                } else {
                    break;
                }
            }
        }

        expired
    }
}
