use crate::types::VirtualTime;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A timer registered in the virtual clock.
/// Generic over payload `T` so the clock doesn't know about domain types.
#[derive(Debug, Clone)]
pub struct Timer<T> {
    pub wake_time: VirtualTime,
    pub callback_id: u64,
    pub priority: u8,
    pub payload: T,
}

// Manual Ord impl: sort by (wake_time, priority, callback_id)
impl<T> PartialEq for Timer<T> {
    fn eq(&self, other: &Self) -> bool {
        self.wake_time == other.wake_time
            && self.priority == other.priority
            && self.callback_id == other.callback_id
    }
}
impl<T> Eq for Timer<T> {}

impl<T> PartialOrd for Timer<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Timer<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.wake_time
            .cmp(&other.wake_time)
            .then(self.priority.cmp(&other.priority))
            .then(self.callback_id.cmp(&other.callback_id))
    }
}

/// The virtual clock managing simulation time and timers.
pub struct VirtualClock<T> {
    current_time: VirtualTime,
    timers: BinaryHeap<Reverse<Timer<T>>>,
    next_callback_id: u64,
}

impl<T> Default for VirtualClock<T> {
    fn default() -> Self {
        Self {
            current_time: VirtualTime(0),
            timers: BinaryHeap::new(),
            next_callback_id: 0,
        }
    }
}

impl<T> VirtualClock<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn now(&self) -> VirtualTime {
        self.current_time
    }

    /// Schedule a timer with a payload and priority.
    /// Lower priority values fire first at the same wake_time.
    pub fn schedule(&mut self, wake_time: VirtualTime, priority: u8, payload: T) -> u64 {
        let callback_id = self.next_callback_id;
        self.next_callback_id += 1;
        self.timers.push(Reverse(Timer {
            wake_time,
            callback_id,
            priority,
            payload,
        }));
        callback_id
    }

    /// Schedule a timer relative to current time.
    pub fn schedule_after(&mut self, delay: VirtualTime, priority: u8, payload: T) -> u64 {
        let wake_time = self.current_time + delay;
        self.schedule(wake_time, priority, payload)
    }

    /// Returns true if there are no pending timers.
    pub fn is_empty(&self) -> bool {
        self.timers.is_empty()
    }

    /// Advances to the next timer and returns all expired timers up to that time.
    #[must_use]
    pub fn tick(&mut self) -> Vec<Timer<T>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_schedule_and_tick() {
        let mut clock: VirtualClock<&str> = VirtualClock::new();
        let _cb1 = clock.schedule_after(VirtualTime(100), 0, "timer-a");
        let _cb2 = clock.schedule_after(VirtualTime(200), 0, "timer-b");
        let _cb3 = clock.schedule_after(VirtualTime(50), 0, "timer-c");

        let expired = clock.tick();
        assert_eq!(clock.now(), VirtualTime(50));
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].payload, "timer-c");

        let expired = clock.tick();
        assert_eq!(clock.now(), VirtualTime(100));
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].payload, "timer-a");

        let expired = clock.tick();
        assert_eq!(clock.now(), VirtualTime(200));
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].payload, "timer-b");

        let expired = clock.tick();
        assert!(expired.is_empty());
        assert!(clock.is_empty());
    }

    #[test]
    fn test_clock_priority_ordering() {
        let mut clock: VirtualClock<&str> = VirtualClock::new();
        // Same wake_time, different priorities
        clock.schedule(VirtualTime(100), 10, "probe"); // lower priority (fires second)
        clock.schedule(VirtualTime(100), 0, "fault"); // higher priority (fires first)

        let expired = clock.tick();
        assert_eq!(expired.len(), 2);
        assert_eq!(expired[0].payload, "fault"); // priority 0 first
        assert_eq!(expired[1].payload, "probe"); // priority 10 second
    }
}
