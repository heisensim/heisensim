use serde::Deserialize;
use std::collections::HashMap;

// ConnectRPC response wrappers
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetEnvironmentResponse {
    pub environment: Environment,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEnvironmentsResponse {
    #[serde(default)]
    pub environments: Vec<Environment>,
    #[serde(default)]
    pub next_page_token: String,
    #[serde(default)]
    pub total_size: i32,
}

// Core Environment model (matches Diverge proto)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub annotations: HashMap<String, String>,
    pub spec: EnvironmentSpec,
    #[serde(default)]
    pub status: EnvironmentStatus,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSpec {
    #[serde(default)]
    pub source: EnvironmentSource,
    #[serde(default)]
    pub deploy: EnvironmentDeploy,
    #[serde(default)]
    pub routing: EnvironmentRouting,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSource {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub commit_sha: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDeploy {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub changed_services: Vec<String>,
    #[serde(default)]
    pub namespace: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRouting {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub header_key: Option<String>,
    #[serde(default)]
    pub header_value: Option<String>,
    #[serde(default)]
    pub external_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentStatus {
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub services: Vec<String>,
}

impl Environment {
    /// Get the routing header key (defaults to "x-preview-env")
    pub fn header_key(&self) -> &str {
        self.spec
            .routing
            .header_key
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("x-preview-env")
    }

    /// Get the routing header value (defaults to environment name)
    pub fn header_value(&self) -> &str {
        self.spec
            .routing
            .header_value
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.name)
    }

    /// Check if the environment is in Running phase
    pub fn is_running(&self) -> bool {
        self.status.phase.eq_ignore_ascii_case("running")
    }

    /// Get the external URL
    pub fn url(&self) -> Option<&str> {
        self.status
            .url
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or(self
                .spec
                .routing
                .external_url
                .as_deref()
                .filter(|s| !s.trim().is_empty()))
    }

    /// Get the list of changed services
    pub fn changed_services(&self) -> &[String] {
        &self.spec.deploy.changed_services
    }
}
