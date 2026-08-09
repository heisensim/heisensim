use anyhow::{Context, Result};
use heisensim_timeline::{EventKind, TimelineHandle};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::{Api, DeleteParams}, Client};
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

/// Handles fault injection operations against Kubernetes pods.
pub struct FaultOperator {
    client: Client,
    timeline: TimelineHandle,
}

impl FaultOperator {
    /// Create a new fault operator.
    pub fn new(client: Client, timeline: TimelineHandle) -> Self {
        Self { client, timeline }
    }

    /// Inject a pod crash by deleting the pod.
    pub async fn inject_pod_crash(&self, namespace: &str, pod_name: &str) -> Result<Uuid> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let fault_id = Uuid::new_v4();

        info!(pod = pod_name, "Injecting pod crash: deleting pod");
        api.delete(pod_name, &DeleteParams::default())
            .await
            .context("Failed to delete pod")?;

        self.timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "pod_crash".to_string(),
            target: format!("{}/{}", namespace, pod_name),
            duration_secs: None,
        });

        Ok(fault_id)
    }

    /// Execute a command inside a pod using `kubectl exec` via tokio::process.
    ///
    /// We use the kubectl CLI rather than the kube-rs exec API because the
    /// latter requires websocket support and complex stream handling.
    /// For chaos testing, shelling out to kubectl is simpler and more robust.
    pub async fn exec_in_pod(
        &self,
        namespace: &str,
        pod_name: &str,
        command: &[&str],
    ) -> Result<String> {
        info!(pod = pod_name, cmd = ?command, "Executing command in pod");

        let output = tokio::process::Command::new("kubectl")
            .args(["exec", "-n", namespace, pod_name, "--"])
            .args(command)
            .output()
            .await
            .context("Failed to run kubectl exec")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            warn!(
                pod = pod_name,
                stderr = stderr.as_str(),
                "kubectl exec failed"
            );
        }

        info!(pod = pod_name, stdout = stdout.as_str(), "Exec output");
        Ok(stdout)
    }

    /// Inject network latency on a pod's eth0 interface using tc netem.
    pub async fn inject_network_latency(
        &self,
        namespace: &str,
        pod_name: &str,
        delay_ms: u32,
        jitter_ms: u32,
        duration_secs: f64,
    ) -> Result<Uuid> {
        let fault_id = Uuid::new_v4();

        let delay_arg = format!("{}ms", delay_ms);
        let jitter_arg = format!("{}ms", jitter_ms);
        self.exec_in_pod(
            namespace,
            pod_name,
            &[
                "tc", "qdisc", "add", "dev", "eth0", "root", "netem", "delay", &delay_arg,
                &jitter_arg,
            ],
        )
        .await?;

        self.timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "network_latency".to_string(),
            target: format!("{}/{}", namespace, pod_name),
            duration_secs: Some(duration_secs),
        });

        // Spawn background task to revert after duration
        let client_clone = self.client.clone();
        let timeline_clone = self.timeline.clone();
        let ns = namespace.to_string();
        let pn = pod_name.to_string();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64(duration_secs)).await;

            let op = FaultOperator::new(client_clone, timeline_clone);
            if let Err(e) = op.revert_network_latency(&ns, &pn, fault_id).await {
                warn!(pod = pn.as_str(), error = %e, "Failed to revert network latency");
            }
        });

        Ok(fault_id)
    }

    /// Revert network latency by removing the tc qdisc.
    pub async fn revert_network_latency(
        &self,
        namespace: &str,
        pod_name: &str,
        fault_id: Uuid,
    ) -> Result<()> {
        self.exec_in_pod(namespace, pod_name, &["tc", "qdisc", "del", "dev", "eth0", "root"])
            .await?;

        self.timeline.emit(EventKind::FaultReverted { fault_id });
        info!(pod = pod_name, "Reverted network latency");
        Ok(())
    }

    /// Inject a network partition by dropping all traffic to a target IP.
    pub async fn inject_partition(
        &self,
        namespace: &str,
        pod_a: &str,
        pod_b_ip: &str,
        duration_secs: f64,
    ) -> Result<Uuid> {
        let fault_id = Uuid::new_v4();

        self.exec_in_pod(
            namespace,
            pod_a,
            &["iptables", "-A", "OUTPUT", "-d", pod_b_ip, "-j", "DROP"],
        )
        .await?;

        self.timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "network_partition".to_string(),
            target: format!("{}/{} -> {}", namespace, pod_a, pod_b_ip),
            duration_secs: Some(duration_secs),
        });

        // Spawn background task to revert
        let client_clone = self.client.clone();
        let timeline_clone = self.timeline.clone();
        let ns = namespace.to_string();
        let pa = pod_a.to_string();
        let pip = pod_b_ip.to_string();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64(duration_secs)).await;

            let op = FaultOperator::new(client_clone, timeline_clone);
            if let Err(e) = op.revert_partition(&ns, &pa, &pip, fault_id).await {
                warn!(pod = pa.as_str(), error = %e, "Failed to revert partition");
            }
        });

        Ok(fault_id)
    }

    /// Revert a network partition by removing the iptables DROP rule.
    pub async fn revert_partition(
        &self,
        namespace: &str,
        pod_name: &str,
        target_ip: &str,
        fault_id: Uuid,
    ) -> Result<()> {
        self.exec_in_pod(
            namespace,
            pod_name,
            &["iptables", "-D", "OUTPUT", "-d", target_ip, "-j", "DROP"],
        )
        .await?;

        self.timeline.emit(EventKind::FaultReverted { fault_id });
        info!(pod = pod_name, target = target_ip, "Reverted partition");
        Ok(())
    }
}
