//! # heisensim-props
//!
//! Property checking framework for heisensim simulations.
//!
//! Properties are invariants that must hold throughout a simulation run.
//! When a property is violated, heisensim records the exact seed and fault
//! schedule that triggered the violation, enabling perfect reproduction.
//!
//! ## Built-in Properties
//!
//! - **NoCrash**: No process should crash unexpectedly
//! - **NoHang**: All processes should make progress (no deadlocks/livelocks)
//! - **NoDataLoss**: Acknowledged writes must be readable after recovery
//! - **Linearizable**: Operations must appear to execute atomically
//!
//! ## Custom Properties
//!
//! Implement the [`Property`] trait to define custom invariants.

pub mod builtin;
pub mod checker;

pub use checker::{
    ProcessInfo, Property, PropertyChecker, PropertyResult, Severity, SimulationSnapshot,
};
