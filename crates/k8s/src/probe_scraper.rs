use anyhow::{Context, Result};
use heisensim_probe::{
    ExecProbeConfig, GrpcProbeConfig, HttpMethod, HttpProbeConfig, ProbeConfig, TcpProbeConfig,
};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, Api, Client};

/// Scrape Kubernetes readiness and liveness probes from all pods in a namespace,
/// converting them into heisensim `ProbeConfig` entries.
pub async fn scrape_probes(client: &Client, namespace: &str) -> Result<Vec<ProbeConfig>> {
    let api: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pods = api
        .list(&ListParams::default())
        .await
        .context("Failed to list pods")?;

    let mut configs = Vec::new();

    for pod in pods {
        let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");
        let pod_ip = match pod.status.as_ref().and_then(|s| s.pod_ip.as_deref()) {
            Some(ip) => ip,
            None => continue,
        };

        let Some(spec) = pod.spec else { continue };

        for container in &spec.containers {
            // Process readiness probe
            if let Some(probe) = &container.readiness_probe {
                let name_prefix = format!("{}/{}/readiness", pod_name, container.name);
                let interval_ms = (probe.period_seconds.unwrap_or(10) as u64) * 1000;
                let timeout_ms = (probe.timeout_seconds.unwrap_or(5) as u64) * 1000;

                if let Some(config) =
                    convert_probe(probe, &name_prefix, pod_ip, interval_ms, timeout_ms)
                {
                    configs.push(config);
                }
            }

            // Process liveness probe
            if let Some(probe) = &container.liveness_probe {
                let name_prefix = format!("{}/{}/liveness", pod_name, container.name);
                let interval_ms = (probe.period_seconds.unwrap_or(10) as u64) * 1000;
                let timeout_ms = (probe.timeout_seconds.unwrap_or(5) as u64) * 1000;

                if let Some(config) =
                    convert_probe(probe, &name_prefix, pod_ip, interval_ms, timeout_ms)
                {
                    configs.push(config);
                }
            }
        }
    }

    Ok(configs)
}

/// Convert a single K8s probe spec into a heisensim ProbeConfig.
fn convert_probe(
    probe: &k8s_openapi::api::core::v1::Probe,
    name: &str,
    pod_ip: &str,
    interval_ms: u64,
    timeout_ms: u64,
) -> Option<ProbeConfig> {
    if let Some(http) = &probe.http_get {
        let port = match &http.port {
            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(i) => *i as u16,
            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String(s) => {
                s.parse().unwrap_or(80)
            }
        };
        let path = http.path.as_deref().unwrap_or("/");
        let scheme = http.scheme.as_deref().unwrap_or("HTTP").to_lowercase();
        let url = format!("{}://{}:{}{}", scheme, pod_ip, port, path);

        Some(ProbeConfig::Http(HttpProbeConfig {
            name: name.to_string(),
            url,
            method: HttpMethod::Get,
            expected_status: 200,
            timeout_ms,
            interval_ms,
            headers: None,
        }))
    } else if let Some(tcp) = &probe.tcp_socket {
        let port = match &tcp.port {
            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(i) => *i as u16,
            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String(s) => {
                s.parse().unwrap_or(80)
            }
        };

        Some(ProbeConfig::Tcp(TcpProbeConfig {
            name: name.to_string(),
            host: pod_ip.to_string(),
            port,
            timeout_ms,
            interval_ms,
        }))
    } else if let Some(grpc) = &probe.grpc {
        Some(ProbeConfig::Grpc(GrpcProbeConfig {
            name: name.to_string(),
            address: format!("{}:{}", pod_ip, grpc.port),
            service: grpc.service.clone(),
            timeout_ms,
            interval_ms,
        }))
    } else if let Some(exec_action) = &probe.exec {
        Some(ProbeConfig::Exec(ExecProbeConfig {
            name: name.to_string(),
            command: exec_action.command.clone().unwrap_or_default(),
            timeout_ms,
            interval_ms,
        }))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_scrape_probes() {
        let client = Client::try_default().await.unwrap();
        let probes = scrape_probes(&client, "default").await.unwrap();
        println!("Discovered {} probes", probes.len());
        for p in &probes {
            println!("  - {}", p.name());
        }
    }
}
