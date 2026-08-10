//! Ordered event timeline and bus for heisensim chaos testing.

pub mod bus;
pub mod event;
pub mod query;

pub use bus::{Timeline, TimelineHandle};
pub use event::{EventKind, TimelineEvent};
pub use query::{
    TimelineSummary, events_in_window, failure_count, fault_count, fault_to_detection_latency,
    first_failure_after, summary,
};
