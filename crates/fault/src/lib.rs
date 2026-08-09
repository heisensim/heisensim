//! # heisensim-fault
//!
//! Fault injection and autonomous state exploration for heisensim.
//!
//! This crate provides:
//! - A fault injection framework that can introduce network partitions,
//!   process crashes, disk failures, and clock skew into a running simulation
//! - An exploration engine that uses coverage-guided fuzzing to autonomously
//!   discover interesting execution paths
//! - A fault schedule recorder for perfect reproduction

pub mod explorer;
pub mod injector;
pub mod schedule;

pub use explorer::Explorer;
pub use injector::FaultInjector;
pub use schedule::FaultSchedule;
