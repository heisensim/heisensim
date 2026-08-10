use crate::config::ExecProbeConfig;
use crate::http::ProbeResult;
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Executes a Kubernetes exec probe by running a command inside a pod.
///
/// Uses `kubectl exec` to run the probe command in the target pod.
/// The probe succeeds if the command exits with code 0, matching
/// Kubernetes exec probe semantics.
pub async fn check_exec(config: &ExecProbeConfig) -> ProbeResult {
    let start = Instant::now();
    let timeout_dur = Duration::from_millis(config.timeout_ms);

    if config.command.is_empty() {
        return ProbeResult {
            success: false,
            latency: start.elapsed(),
            status_code: None,
            error: Some("Exec probe has empty command".to_string()),
        };
    }

    let fut = async {
        let output = tokio::process::Command::new("kubectl")
            .args(["exec", "-n", &config.namespace, &config.pod_name, "--"])
            .args(&config.command)
            .output()
            .await;

        match output {
            Ok(o) => {
                let exit_code = o.status.code().unwrap_or(-1);
                if o.status.success() {
                    ProbeResult {
                        success: true,
                        latency: start.elapsed(),
                        status_code: Some(exit_code as u16),
                        error: None,
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    ProbeResult {
                        success: false,
                        latency: start.elapsed(),
                        status_code: Some(exit_code as u16),
                        error: Some(format!(
                            "exec probe exited with code {}: {}",
                            exit_code,
                            stderr.trim()
                        )),
                    }
                }
            }
            Err(e) => ProbeResult {
                success: false,
                latency: start.elapsed(),
                status_code: None,
                error: Some(format!("Failed to run kubectl exec: {}", e)),
            },
        }
    };

    match timeout(timeout_dur, fut).await {
        Ok(result) => result,
        Err(_) => ProbeResult {
            success: false,
            latency: start.elapsed(),
            status_code: None,
            error: Some(format!(
                "exec probe timed out after {}ms",
                config.timeout_ms
            )),
        },
    }
}
