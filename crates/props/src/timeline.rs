//! Timeline-aware property checking framework.
//!
//! Evaluates invariants against `Vec<TimelineEvent>` produced by chaos tests.
//! This is separate from the simulation-oriented `Property` trait which works
//! on `SimulationSnapshot` — these two domains have fundamentally different
//! data models (virtual time vs wall clock, processes vs probes).

use heisensim_timeline::event::TimelineEvent;
use serde::{Deserialize, Serialize};

/// A property that can be evaluated against a timeline of chaos test events.
pub trait TimelineProperty: Send + Sync {
    /// Human-readable name of this property.
    fn name(&self) -> &str;

    /// Short description of what this property checks.
    fn description(&self) -> &str;

    /// Evaluate the property against the given timeline events.
    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict;
}

/// The result of evaluating a timeline property.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyVerdict {
    /// Whether the property check passed.
    pub passed: bool,
    /// Name of the property that was checked.
    pub property_name: String,
    /// What was expected, e.g. "recovery < 30s".
    pub expected: String,
    /// What was actually observed, e.g. "12.3s".
    pub actual: String,
    /// Per-fault or per-probe breakdown details.
    pub details: Vec<String>,
}

impl PropertyVerdict {
    /// Create a passing verdict.
    pub fn pass(
        name: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            passed: true,
            property_name: name.into(),
            expected: expected.into(),
            actual: actual.into(),
            details: Vec::new(),
        }
    }

    /// Create a failing verdict.
    pub fn fail(
        name: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            passed: false,
            property_name: name.into(),
            expected: expected.into(),
            actual: actual.into(),
            details: Vec::new(),
        }
    }

    /// Add detail lines to the verdict.
    pub fn with_details(mut self, details: Vec<String>) -> Self {
        self.details = details;
        self
    }
}

/// Orchestrates evaluation of multiple timeline properties.
pub struct TimelineChecker {
    properties: Vec<Box<dyn TimelineProperty>>,
}

impl Default for TimelineChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TimelineChecker {
    /// Create a new empty checker.
    pub fn new() -> Self {
        Self {
            properties: Vec::new(),
        }
    }

    /// Add a property to evaluate.
    pub fn add(&mut self, prop: Box<dyn TimelineProperty>) {
        self.properties.push(prop);
    }

    /// Evaluate all properties against the timeline.
    pub fn evaluate_all(&self, events: &[TimelineEvent]) -> Vec<PropertyVerdict> {
        self.properties.iter().map(|p| p.evaluate(events)).collect()
    }

    /// Returns true if any property failed.
    pub fn any_failed(&self, events: &[TimelineEvent]) -> bool {
        self.evaluate_all(events).iter().any(|v| !v.passed)
    }

    /// Number of registered properties.
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// Whether the checker has no properties.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }
}
