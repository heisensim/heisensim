use heisensim_core::process::ProcessState;
use heisensim_core::types::VirtualTime;
use serde::{Deserialize, Serialize};

/// Indicates the severity level of a property outcome or violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Informational message, not a violation.
    Info,
    /// Potential issue or non-critical anomaly.
    Warning,
    /// Definite property violation.
    Error,
    /// Critical system failure or invariant breach.
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Error => write!(f, "ERROR"),
            Severity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Result of evaluating a property against a simulation snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyResult {
    /// Whether the property check passed (`true`) or failed (`false`).
    pub passed: bool,
    /// The severity level associated with this result.
    pub severity: Severity,
    /// Descriptive message explaining the pass or failure rationale.
    pub message: String,
    /// Simulation virtual time at which the check was performed.
    pub timestamp: VirtualTime,
}

impl PropertyResult {
    /// Creates a successful property result.
    pub fn pass(timestamp: VirtualTime, message: impl Into<String>) -> Self {
        Self {
            passed: true,
            severity: Severity::Info,
            message: message.into(),
            timestamp,
        }
    }

    /// Creates a failed property result with a given severity.
    pub fn fail(severity: Severity, timestamp: VirtualTime, message: impl Into<String>) -> Self {
        Self {
            passed: false,
            severity,
            message: message.into(),
            timestamp,
        }
    }
}

/// Information tracking process state within a simulation snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// Process identifier.
    pub pid: u32,
    /// Node identifier hosting the process.
    pub node_id: u64,
    /// Human-readable name of the process.
    pub name: String,
    /// Current state of the process.
    pub state: ProcessState,
    /// Last virtual timestamp when progress was recorded for this process.
    pub last_progress: VirtualTime,
}

/// A point-in-time snapshot of the simulation state used for invariant verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSnapshot {
    /// Current virtual time of the simulation.
    pub current_time: VirtualTime,
    /// List of tracked process states in the simulation.
    pub processes: Vec<ProcessInfo>,
    /// Number of network messages currently in flight across all nodes.
    pub network_messages_in_flight: usize,
    /// Arbitrary domain-specific state serialized as JSON for custom properties.
    pub custom_state: serde_json::Value,
}

impl SimulationSnapshot {
    /// Creates a new `SimulationSnapshot` with default custom state.
    pub fn new(
        current_time: VirtualTime,
        processes: Vec<ProcessInfo>,
        network_messages_in_flight: usize,
    ) -> Self {
        Self {
            current_time,
            processes,
            network_messages_in_flight,
            custom_state: serde_json::Value::Null,
        }
    }

    /// Attaches custom JSON state to the snapshot.
    pub fn with_custom_state(mut self, custom_state: serde_json::Value) -> Self {
        self.custom_state = custom_state;
        self
    }
}

/// Trait implemented by properties (invariants) to be verified against simulation snapshots.
pub trait Property: Send + Sync {
    /// Returns the unique name of the property.
    fn name(&self) -> &str;

    /// Returns a human-readable description of what this property checks.
    fn description(&self) -> &str;

    /// Evaluates the property against the given simulation state snapshot.
    fn check(&self, state: &SimulationSnapshot) -> PropertyResult;
}

/// Orchestrates registered properties and executes checks against simulation snapshots.
#[derive(Default)]
pub struct PropertyChecker {
    properties: Vec<Box<dyn Property>>,
}

impl PropertyChecker {
    /// Creates a new empty `PropertyChecker`.
    pub fn new() -> Self {
        Self {
            properties: Vec::new(),
        }
    }

    /// Adds a boxed property to the checker.
    pub fn add_property(&mut self, prop: Box<dyn Property>) {
        self.properties.push(prop);
    }

    /// Evaluates all registered properties against the given simulation snapshot.
    pub fn check_all(&self, snapshot: &SimulationSnapshot) -> Vec<PropertyResult> {
        let mut results = Vec::with_capacity(self.properties.len());
        for prop in &self.properties {
            let res = prop.check(snapshot);
            tracing::debug!(
                property = prop.name(),
                passed = res.passed,
                severity = %res.severity,
                message = %res.message,
                "Checked property"
            );
            results.push(res);
        }
        results
    }

    /// Returns `true` if any registered property failed (violated) for the snapshot.
    pub fn has_violations(&self, snapshot: &SimulationSnapshot) -> bool {
        self.check_all(snapshot).iter().any(|res| !res.passed)
    }

    /// Returns the number of registered properties.
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// Returns `true` if no properties are registered.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }
}
