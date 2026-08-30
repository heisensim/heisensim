use crate::types::{Environment, GetEnvironmentResponse, ListEnvironmentsResponse};
use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{info, warn};

/// Diverge ConnectRPC client.
pub struct DivergeClient {
    base_url: String,
    http: reqwest::Client,
    token: Option<String>,
}

/// Resolved context from a Diverge preview environment.
#[derive(Debug, Clone)]
pub struct DivergeContext {
    /// Header name for preview routing (e.g. "x-preview-env")
    pub header_key: String,
    /// Header value for preview routing (e.g. "pr-123")
    pub header_value: String,
    /// Services deployed in this environment
    pub services: Vec<String>,
    /// Changed services (for blast-radius targeting)
    pub changed_services: Vec<String>,
    /// Kubernetes namespace
    pub namespace: String,
    /// External URL of the preview environment
    pub url: Option<String>,
}

impl DivergeClient {
    /// Create a new client with auth token resolution.
    ///
    /// Token resolution order:
    /// 1. Explicit token parameter
    /// 2. `DIVERGE_TOKEN` environment variable
    /// 3. Kubernetes ServiceAccount token at `/var/run/secrets/kubernetes.io/serviceaccount/token`
    /// 4. No auth (unauthenticated)
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        let resolved_token = token
            .or_else(|| std::env::var("DIVERGE_TOKEN").ok())
            .or_else(|| {
                std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
                    .ok()
                    .map(|t| t.trim().to_string())
            });

        if resolved_token.is_some() {
            info!("Diverge auth: using bearer token");
        } else {
            warn!("Diverge auth: no token found (unauthenticated)");
        }

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
            token: resolved_token,
        }
    }

    /// Call a ConnectRPC method (JSON encoding over HTTP POST).
    async fn call<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        request: &Req,
    ) -> Result<Resp> {
        let url = format!("{}/{}", self.base_url, method);
        let mut req = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(request);

        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req
            .send()
            .await
            .with_context(|| format!("Failed to connect to Diverge API at {}", url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Diverge API error ({}): {}\n\nIs the Diverge server running? Check --diverge-url or set DIVERGE_URL.",
                status,
                body
            );
        }

        resp.json::<Resp>()
            .await
            .context("Failed to deserialize Diverge API response")
    }

    /// Get a specific environment by namespace and name.
    pub async fn get_environment(&self, namespace: &str, name: &str) -> Result<Environment> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            namespace: &'a str,
            name: &'a str,
        }

        let resp: GetEnvironmentResponse = self
            .call(
                "diverge.v1alpha1.EnvironmentService/GetEnvironment",
                &Req { namespace, name },
            )
            .await?;

        Ok(resp.environment)
    }

    /// List environments in a namespace.
    pub async fn list_environments(&self, namespace: &str) -> Result<Vec<Environment>> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            namespace: &'a str,
        }

        let resp: ListEnvironmentsResponse = self
            .call(
                "diverge.v1alpha1.EnvironmentService/ListEnvironments",
                &Req { namespace },
            )
            .await?;

        Ok(resp.environments)
    }

    /// Wait for an environment to reach Running phase.
    ///
    /// Polls every 2 seconds up to the specified timeout.
    pub async fn wait_for_running(
        &self,
        namespace: &str,
        name: &str,
        timeout: Duration,
    ) -> Result<Environment> {
        let start = std::time::Instant::now();

        loop {
            let env = self.get_environment(namespace, name).await?;

            if env.is_running() {
                return Ok(env);
            }

            let phase = &env.status.phase;
            if phase.eq_ignore_ascii_case("failed") || phase.eq_ignore_ascii_case("error") {
                anyhow::bail!(
                    "Diverge environment '{}' is in phase '{}' and will not become Running.",
                    name,
                    phase
                );
            }

            if start.elapsed() > timeout {
                anyhow::bail!(
                    "Timed out waiting for Diverge environment '{}' to become Running (current phase: '{}', waited {:?}).",
                    name,
                    phase,
                    timeout
                );
            }

            info!(
                env = name,
                phase = phase.as_str(),
                elapsed = ?start.elapsed(),
                timeout = ?timeout,
                "Waiting for environment to become Running..."
            );

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Resolve a DivergeContext from an environment.
    pub fn resolve_context(&self, env: &Environment) -> DivergeContext {
        DivergeContext {
            header_key: env.header_key().to_string(),
            header_value: env.header_value().to_string(),
            services: env.status.services.clone(),
            changed_services: env.changed_services().to_vec(),
            namespace: env.namespace.clone(),
            url: env.url().map(|s| s.to_string()),
        }
    }
}
