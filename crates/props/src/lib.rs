//! # heisensim-props
//!
//! Property checking framework for heisensim simulations.
//!
//! ## Timeline Properties (K8s chaos tests)
//!
//! The [`TimelineProperty`](timeline::TimelineProperty) trait evaluates invariants
//! against `Vec<TimelineEvent>` from real chaos tests:
//!
//! - [`RecoveryTime`](recovery::RecoveryTime) — probes recover within N seconds after fault
//! - [`Availability`](availability::Availability) — probe success rate ≥ N%
//! - [`ErrorBudget`](error_budget::ErrorBudget) — max consecutive failures per probe
//! - [`NoCascade`](cascade::NoCascade) — faults don't cause unexpected probe failures
//! - [`LatencyThreshold`](latency::LatencyThreshold) — probe latency pNN stays under threshold
//!
//! ## Simulation Properties (future)
//!
//! The [`Property`](checker::Property) trait evaluates invariants against
//! `SimulationSnapshot` for deterministic simulation testing.

pub mod availability;
pub mod baseline;
pub mod builtin;
pub mod cascade;
pub mod checker;
pub mod dns_resolution;
pub mod error_budget;
pub mod latency;
pub mod recovery;
pub mod steady_state;
pub mod throughput;
pub mod timeline;

// Timeline properties (primary API for K8s chaos)
pub use availability::Availability;
pub use baseline::{
    BaselineAvailabilityDiff, BaselineLatencyDiff, BaselineSnapshot, capture_baseline,
    smart_baseline_duration,
};
pub use cascade::NoCascade;
pub use dns_resolution::DnsResolution;
pub use error_budget::ErrorBudget;
pub use latency::LatencyThreshold;
pub use recovery::RecoveryTime;
pub use steady_state::SteadyState;
pub use throughput::Throughput;
pub use timeline::{PropertyVerdict, TimelineChecker, TimelineProperty};

// Simulation properties (existing)
pub use checker::{
    ProcessInfo, Property, PropertyChecker, PropertyResult, Severity, SimulationSnapshot,
};
