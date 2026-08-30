//! Diverge preview environment integration for heisensim.
//!
//! Provides a ConnectRPC JSON client to discover preview environments,
//! resolve routing headers, and identify changed services for
//! blast-radius-aware chaos testing.

pub mod client;
pub mod types;

pub use client::{DivergeClient, DivergeContext};
pub use types::Environment;
