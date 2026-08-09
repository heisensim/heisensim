use crate::error::HeisensimError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Main configuration for a simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub seed: Option<u64>,
    pub duration: String,
    pub nodes: Vec<NodeConfig>,
    pub faults: FaultConfig,
    pub properties: Vec<PropertyConfig>,
}

/// Configuration for a simulated node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub name: String,
    pub image: String,
    pub command: Option<String>,
    pub ports: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
}

/// Configuration for faults to inject.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FaultConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub probabilities: HashMap<String, f64>,
    #[serde(default)]
    pub timing: HashMap<String, String>,
}

/// Configuration for properties to check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyConfig {
    pub name: String,
    pub kind: String,
    pub params: Option<HashMap<String, String>>,
}

impl SimulationConfig {
    /// Reads configuration from a TOML file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, HeisensimError> {
        let content = std::fs::read_to_string(path)?;
        let config: SimulationConfig =
            toml::from_str(&content).map_err(|e| HeisensimError::ConfigError(e.to_string()))?;
        Ok(config)
    }

    /// Reads a docker-compose.yml file and converts it into a SimulationConfig.
    pub fn from_compose<P: AsRef<Path>>(_path: P) -> Result<Self, HeisensimError> {
        // Placeholder for docker-compose conversion logic
        Err(HeisensimError::ConfigError(
            "from_compose not yet implemented".into(),
        ))
    }
}
