use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Configuration for a health probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProbeConfig {
    Http(HttpProbeConfig),
    Tcp(TcpProbeConfig),
    Grpc(GrpcProbeConfig),
    Exec(ExecProbeConfig),
}

/// HTTP method for HTTP probes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
}

/// Configuration for an HTTP health probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpProbeConfig {
    pub name: String,
    pub url: String,
    pub method: HttpMethod,
    pub expected_status: u16,
    pub timeout_ms: u64,
    pub interval_ms: u64,
    pub headers: Option<HashMap<String, String>>,
}

/// Configuration for a TCP health probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpProbeConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub timeout_ms: u64,
    pub interval_ms: u64,
}

/// Configuration for a gRPC health probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcProbeConfig {
    pub name: String,
    pub address: String,
    pub service: Option<String>,
    pub timeout_ms: u64,
    pub interval_ms: u64,
}

/// Configuration for an Exec health probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecProbeConfig {
    pub name: String,
    pub command: Vec<String>,
    pub timeout_ms: u64,
    pub interval_ms: u64,
}

impl ProbeConfig {
    /// Gets the name of the probe.
    pub fn name(&self) -> &str {
        match self {
            ProbeConfig::Http(c) => &c.name,
            ProbeConfig::Tcp(c) => &c.name,
            ProbeConfig::Grpc(c) => &c.name,
            ProbeConfig::Exec(c) => &c.name,
        }
    }

    /// Gets the execution interval of the probe.
    pub fn interval(&self) -> Duration {
        let ms = match self {
            ProbeConfig::Http(c) => c.interval_ms,
            ProbeConfig::Tcp(c) => c.interval_ms,
            ProbeConfig::Grpc(c) => c.interval_ms,
            ProbeConfig::Exec(c) => c.interval_ms,
        };
        Duration::from_millis(ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_config_name_and_interval() {
        let config = ProbeConfig::Tcp(TcpProbeConfig {
            name: "test-tcp".to_string(),
            host: "localhost".to_string(),
            port: 8080,
            timeout_ms: 1000,
            interval_ms: 5000,
        });
        
        assert_eq!(config.name(), "test-tcp");
        assert_eq!(config.interval(), Duration::from_millis(5000));
    }

    #[test]
    fn test_http_probe_config_deserialization() {
        let json = r#"{
            "type": "Http",
            "name": "health-check",
            "url": "http://localhost:8080/health",
            "method": "Get",
            "expected_status": 200,
            "timeout_ms": 2000,
            "interval_ms": 10000
        }"#;

        let config: ProbeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name(), "health-check");
        assert_eq!(config.interval().as_millis(), 10000);
        
        if let ProbeConfig::Http(http_cfg) = config {
            assert_eq!(http_cfg.expected_status, 200);
            assert!(matches!(http_cfg.method, HttpMethod::Get));
        } else {
            panic!("Expected Http probe config");
        }
    }
}
