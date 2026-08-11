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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_config_serde() {
        let toml_str = r#"
            duration = "1h"
            
            [[nodes]]
            name = "node1"
            image = "ubuntu:latest"
            
            [faults]
            enabled = ["network_partition"]
            
            [[properties]]
            name = "no_crash"
            kind = "safety"
        "#;

        let config: SimulationConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.duration, "1h");
        assert_eq!(config.nodes.len(), 1);
        assert_eq!(config.nodes[0].name, "node1");
        assert_eq!(config.faults.enabled, vec!["network_partition"]);
        assert_eq!(config.properties.len(), 1);
        assert_eq!(config.properties[0].name, "no_crash");

        let ser = toml::to_string(&config).unwrap();
        let config2: SimulationConfig = toml::from_str(&ser).unwrap();
        assert_eq!(config.duration, config2.duration);
        assert_eq!(config.nodes[0].name, config2.nodes[0].name);
    }
}
