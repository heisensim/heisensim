use anyhow::{Context, Result};
use heisensim_timeline::{EventKind, TimelineHandle};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Client,
    api::{Api, DeleteParams},
};
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

/// Method for injecting network faults into pods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectMethod {
    /// Execute commands directly in the target container.
    /// Requires tc/iptables to be installed in the container image.
    Exec,
    /// Use `kubectl debug` to inject an ephemeral container with `nicolaka/netshoot`.
    /// Works with any image — the debug container shares the pod's network namespace.
    Debug,
}

/// Handles fault injection operations against Kubernetes pods.
pub struct FaultOperator {
    client: Client,
    timeline: TimelineHandle,
    inject_method: InjectMethod,
}

impl FaultOperator {
    /// Create a new fault operator.
    pub fn new(client: Client, timeline: TimelineHandle) -> Self {
        Self {
            client,
            timeline,
            inject_method: InjectMethod::Exec,
        }
    }

    /// Create a new fault operator with the specified injection method.
    pub fn with_method(client: Client, timeline: TimelineHandle, method: InjectMethod) -> Self {
        Self {
            client,
            timeline,
            inject_method: method,
        }
    }

    /// Inject a pod crash by deleting the pod.
    pub async fn inject_pod_crash(&self, namespace: &str, pod_name: &str) -> Result<Uuid> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let fault_id = Uuid::new_v4();
        let target = format!("{}/{}", namespace, pod_name);

        let _span = tracing::info_span!(
            "fault.inject",
            fault.id = %fault_id,
            fault.kind = "pod_crash",
            fault.target = %target,
            otel.name = format!("inject crash → {}", pod_name),
        );
        let _guard = _span.enter();

        info!(pod = pod_name, "Injecting pod crash: deleting pod");
        drop(_guard); // Must drop before .await (EnteredSpan is !Send)
        api.delete(pod_name, &DeleteParams::default())
            .await
            .context("Failed to delete pod")?;

        self.timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "pod_crash".to_string(),
            target,
            duration_secs: None,
        });

        Ok(fault_id)
    }

    /// Execute a network command on a pod, dispatching based on the inject method.
    ///
    /// - `Exec`: Runs the command directly inside the target container.
    ///   Requires tc/iptables to be installed in the container image.
    /// - `Debug`: Uses `kubectl debug` to create an ephemeral container with
    ///   `nicolaka/netshoot`, which shares the target pod's network namespace.
    ///   Works with any image (including minimal Alpine images).
    pub async fn exec_network_command(
        &self,
        namespace: &str,
        pod_name: &str,
        command: &[&str],
    ) -> Result<String> {
        match self.inject_method {
            InjectMethod::Exec => self.exec_in_pod(namespace, pod_name, command).await,
            InjectMethod::Debug => {
                self.exec_via_debug_container(namespace, pod_name, command)
                    .await
            }
        }
    }

    /// Execute a command inside a pod using `kubectl exec` via tokio::process.
    ///
    /// Requires tc/iptables to be installed in the target container image.
    async fn exec_in_pod(
        &self,
        namespace: &str,
        pod_name: &str,
        command: &[&str],
    ) -> Result<String> {
        info!(pod = pod_name, cmd = ?command, method = "exec", "Executing command in pod");

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

    /// Execute a command via an ephemeral debug container using `kubectl debug`.
    ///
    /// This creates a temporary container using `nicolaka/netshoot` that shares
    /// the target pod's network namespace. The container runs the specified
    /// command and then exits. This is ideal for pods whose images don't include
    /// networking tools like tc or iptables.
    ///
    /// Requires Kubernetes 1.25+ (ephemeral containers GA).
    async fn exec_via_debug_container(
        &self,
        namespace: &str,
        pod_name: &str,
        command: &[&str],
    ) -> Result<String> {
        let debug_name = format!("heisensim-{}", &Uuid::new_v4().to_string()[..8]);
        let cmd_str = command.join(" ");

        info!(
            pod = pod_name,
            debug_container = debug_name.as_str(),
            cmd = cmd_str.as_str(),
            method = "debug",
            "Executing via ephemeral debug container"
        );

        let output = tokio::process::Command::new("kubectl")
            .args([
                "debug",
                "-n",
                namespace,
                pod_name,
                "--image=nicolaka/netshoot:latest",
                &format!("--container={}", debug_name),
                "--target=", // share network namespace with first container
                "--",
            ])
            .args(command)
            .output()
            .await
            .context("Failed to run kubectl debug")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            warn!(
                pod = pod_name,
                stderr = stderr.as_str(),
                "kubectl debug failed"
            );
        }

        info!(
            pod = pod_name,
            stdout = stdout.as_str(),
            "Debug container output"
        );
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
        let target = format!("{}/{}", namespace, pod_name);

        let _span = tracing::info_span!(
            "fault.inject",
            fault.id = %fault_id,
            fault.kind = "network_latency",
            fault.target = %target,
            fault.delay_ms = delay_ms,
            fault.duration_secs = duration_secs,
            otel.name = format!("inject latency {}ms → {}", delay_ms, pod_name),
        );
        let _guard = _span.enter();

        let delay_arg = format!("{}ms", delay_ms);
        let jitter_arg = format!("{}ms", jitter_ms);
        drop(_guard); // Must drop before .await
        self.exec_network_command(
            namespace,
            pod_name,
            &[
                "tc",
                "qdisc",
                "add",
                "dev",
                "eth0",
                "root",
                "netem",
                "delay",
                &delay_arg,
                &jitter_arg,
            ],
        )
        .await?;

        self.timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "network_latency".to_string(),
            target,
            duration_secs: Some(duration_secs),
        });

        // Spawn background task to revert after duration
        let client_clone = self.client.clone();
        let timeline_clone = self.timeline.clone();
        let ns = namespace.to_string();
        let pn = pod_name.to_string();

        let inject_method = self.inject_method;

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64(duration_secs)).await;

            let op = FaultOperator::with_method(client_clone, timeline_clone, inject_method);
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
        self.exec_network_command(
            namespace,
            pod_name,
            &["tc", "qdisc", "del", "dev", "eth0", "root"],
        )
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
        let target = format!("{}/{} -> {}", namespace, pod_a, pod_b_ip);

        let _span = tracing::info_span!(
            "fault.inject",
            fault.id = %fault_id,
            fault.kind = "network_partition",
            fault.target = %target,
            fault.duration_secs = duration_secs,
            otel.name = format!("inject partition {} ↛ {}", pod_a, pod_b_ip),
        );
        let _guard = _span.enter();

        drop(_guard); // Must drop before .await
        self.exec_network_command(
            namespace,
            pod_a,
            &["iptables", "-A", "OUTPUT", "-d", pod_b_ip, "-j", "DROP"],
        )
        .await?;

        self.timeline.emit(EventKind::FaultInjected {
            fault_id,
            fault_kind: "network_partition".to_string(),
            target,
            duration_secs: Some(duration_secs),
        });

        // Spawn background task to revert
        let client_clone = self.client.clone();
        let timeline_clone = self.timeline.clone();
        let ns = namespace.to_string();
        let pa = pod_a.to_string();
        let pip = pod_b_ip.to_string();

        let inject_method = self.inject_method;

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs_f64(duration_secs)).await;

            let op = FaultOperator::with_method(client_clone, timeline_clone, inject_method);
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
        self.exec_network_command(
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
