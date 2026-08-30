use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::{Pod, Service};
use kube::{Api, Client, api::ListParams};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodInfo {
    pub name: String,
    pub namespace: String,
    pub labels: BTreeMap<String, String>,
    pub container_names: Vec<String>,
    pub is_ready: bool,
    pub pod_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub name: String,
    pub cluster_ip: Option<String>,
    pub ports: Vec<u16>,
    pub selector: BTreeMap<String, String>,
}

pub async fn discover_pods(client: &Client, namespace: &str) -> Result<Vec<PodInfo>> {
    let api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pods = api
        .list(&ListParams::default())
        .await
        .context("Failed to list pods")?;

    let mut pod_infos = Vec::new();
    for pod in pods {
        let meta = pod.metadata;
        let name = meta.name.unwrap_or_else(|| "unknown".to_string());
        let namespace = meta.namespace.unwrap_or_else(|| namespace.to_string());
        let labels = meta.labels.unwrap_or_default();

        let mut container_names = Vec::new();
        if let Some(spec) = &pod.spec {
            for c in &spec.containers {
                container_names.push(c.name.clone());
            }
        }

        let mut is_ready = false;
        let mut pod_ip = None;
        if let Some(status) = &pod.status {
            pod_ip = status.pod_ip.clone();
            if let Some(conditions) = &status.conditions {
                is_ready = conditions
                    .iter()
                    .any(|c| c.type_ == "Ready" && c.status == "True");
            }
        }

        pod_infos.push(PodInfo {
            name,
            namespace,
            labels,
            container_names,
            is_ready,
            pod_ip,
        });
    }

    Ok(pod_infos)
}

pub async fn discover_services(client: &Client, namespace: &str) -> Result<Vec<ServiceInfo>> {
    let api: Api<Service> = Api::namespaced(client.clone(), namespace);
    let services = api
        .list(&ListParams::default())
        .await
        .context("Failed to list services")?;

    let mut service_infos = Vec::new();
    for svc in services {
        let meta = svc.metadata;
        let name = meta.name.unwrap_or_else(|| "unknown".to_string());

        let mut cluster_ip = None;
        let mut ports = Vec::new();
        let mut selector = BTreeMap::new();

        if let Some(spec) = svc.spec {
            cluster_ip = spec.cluster_ip;
            selector = spec.selector.unwrap_or_default();
            if let Some(p_list) = spec.ports {
                for p in p_list {
                    ports.push(p.port as u16);
                }
            }
        }

        service_infos.push(ServiceInfo {
            name,
            cluster_ip,
            ports,
            selector,
        });
    }

    Ok(service_infos)
}

/// Discover pods that belong to specific services by matching their label selectors.
///
/// This enables Diverge blast-radius targeting: only discover pods belonging to
/// the changed services, not all pods in the namespace.
pub async fn discover_pods_for_services(
    client: &Client,
    namespace: &str,
    service_names: &[String],
) -> Result<Vec<PodInfo>> {
    if service_names.is_empty() {
        return discover_pods(client, namespace).await;
    }

    // First, discover all services and their label selectors
    let all_services = discover_services(client, namespace).await?;

    // Filter to only the target services
    let target_selectors: Vec<&BTreeMap<String, String>> = all_services
        .iter()
        .filter(|s| service_names.contains(&s.name))
        .filter(|s| !s.selector.is_empty())
        .map(|s| &s.selector)
        .collect();

    if target_selectors.is_empty() {
        tracing::warn!(
            services = ?service_names,
            "No matching service selectors found, falling back to all pods"
        );
        return discover_pods(client, namespace).await;
    }

    // Discover all pods and filter by matching ANY service selector
    let all_pods = discover_pods(client, namespace).await?;
    let filtered: Vec<PodInfo> = all_pods
        .into_iter()
        .filter(|pod| {
            target_selectors
                .iter()
                .any(|selector| selector.iter().all(|(k, v)| pod.labels.get(k) == Some(v)))
        })
        .collect();

    tracing::info!(
        services = ?service_names,
        pods = filtered.len(),
        "Discovered pods for target services"
    );

    Ok(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_discover_pods() {
        let client = Client::try_default().await.unwrap();
        let pods = discover_pods(&client, "default").await.unwrap();
        println!("Pods: {:?}", pods);
    }
}
