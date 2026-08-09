use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::{Pod, Service};
use kube::{api::ListParams, Api, Client};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodInfo {
    pub name: String,
    pub namespace: String,
    pub labels: BTreeMap<String, String>,
    pub container_names: Vec<String>,
    pub is_ready: bool,
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
    let pods = api.list(&ListParams::default()).await.context("Failed to list pods")?;

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
        if let Some(status) = &pod.status {
            if let Some(conditions) = &status.conditions {
                is_ready = conditions.iter().any(|c| c.type_ == "Ready" && c.status == "True");
            }
        }

        pod_infos.push(PodInfo {
            name,
            namespace,
            labels,
            container_names,
            is_ready,
        });
    }

    Ok(pod_infos)
}

pub async fn discover_services(client: &Client, namespace: &str) -> Result<Vec<ServiceInfo>> {
    let api: Api<Service> = Api::namespaced(client.clone(), namespace);
    let services = api.list(&ListParams::default()).await.context("Failed to list services")?;

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
