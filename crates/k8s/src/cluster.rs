use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::process::Command;

pub struct K3dCluster {
    pub name: String,
}

impl K3dCluster {
    pub async fn create(name: &str) -> Result<Self> {
        let status = Command::new("k3d")
            .args(&["cluster", "create", name, "--no-lb", "--wait"])
            .status()
            .await
            .context("Failed to run k3d create command")?;

        if !status.success() {
            anyhow::bail!("k3d cluster create failed with status {}", status);
        }

        Ok(Self {
            name: name.to_string(),
        })
    }

    pub async fn delete(&self) -> Result<()> {
        let status = Command::new("k3d")
            .args(&["cluster", "delete", &self.name])
            .status()
            .await
            .context("Failed to run k3d delete command")?;

        if !status.success() {
            anyhow::bail!("k3d cluster delete failed with status {}", status);
        }

        Ok(())
    }

    pub async fn exists(name: &str) -> Result<bool> {
        let output = Command::new("k3d")
            .args(&["cluster", "list", "-o", "json"])
            .stdout(Stdio::piped())
            .output()
            .await
            .context("Failed to run k3d list command")?;

        if !output.status.success() {
            anyhow::bail!("k3d cluster list failed");
        }

        let out_str = String::from_utf8(output.stdout)?;
        let clusters: serde_json::Value = serde_json::from_str(&out_str)?;

        if let Some(arr) = clusters.as_array() {
            for cluster in arr {
                if let Some(c_name) = cluster.get("name").and_then(|n| n.as_str()) {
                    if c_name == name {
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    pub async fn kubeconfig(&self) -> Result<String> {
        let output = Command::new("k3d")
            .args(&["kubeconfig", "get", &self.name])
            .stdout(Stdio::piped())
            .output()
            .await
            .context("Failed to get kubeconfig")?;

        if !output.status.success() {
            anyhow::bail!("k3d kubeconfig get failed");
        }

        let out_str = String::from_utf8(output.stdout)?;
        Ok(out_str)
    }
}
