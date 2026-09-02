use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::FaultOperator;

/// Describes the kind of active fault for revert dispatch.
#[derive(Debug, Clone)]
pub enum ActiveFaultKind {
    /// Network latency via tc qdisc netem
    NetworkLatency,
    /// Network partition via iptables DROP
    Partition { target_ip: String },
    /// DNS blackhole via iptables DROP on port 53
    DnsFailure,
    /// CPU/memory stress via stress-ng (killed on cleanup).
    /// Stores the debug container name when using ephemeral containers,
    /// so pkill targets the correct container.
    Stress { debug_container: Option<String> },
}

/// An active fault that has been injected but not yet reverted.
#[derive(Debug, Clone)]
pub struct ActiveFault {
    /// Unique fault ID (matches timeline FaultInjected event)
    pub fault_id: Uuid,
    /// What kind of fault was injected
    pub kind: ActiveFaultKind,
    /// Kubernetes namespace
    pub namespace: String,
    /// Target pod name
    pub pod_name: String,
    /// When the fault was injected
    pub injected_at: Instant,
}

/// Tracks all active faults and provides graceful shutdown revert capability.
///
/// On SIGINT or test completion, `revert_all()` iterates through all tracked
/// faults and calls the appropriate revert method. This prevents orphaned
/// tc/iptables rules from being left on target pods.
///
/// Safety: uses an `AtomicBool` revert guard to prevent concurrent `revert_all`
/// calls from draining the fault list and racing to exit.
pub struct FaultTracker {
    active: Arc<Mutex<Vec<ActiveFault>>>,
    reverting: AtomicBool,
}

impl FaultTracker {
    /// Create a new empty fault tracker.
    pub fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(Vec::new())),
            reverting: AtomicBool::new(false),
        }
    }

    /// Register an active fault for tracking.
    pub async fn track(&self, fault: ActiveFault) {
        let mut active = self.active.lock().await;
        info!(
            fault_id = %fault.fault_id,
            kind = ?fault.kind,
            pod = fault.pod_name.as_str(),
            "Tracking active fault"
        );
        active.push(fault);
    }

    /// Remove a fault from tracking (called after natural revert).
    pub async fn untrack(&self, fault_id: Uuid) {
        let mut active = self.active.lock().await;
        active.retain(|f| f.fault_id != fault_id);
    }

    /// Get the number of currently active (tracked) faults.
    pub async fn active_count(&self) -> usize {
        self.active.lock().await.len()
    }

    /// Forcibly revert ALL active faults. Used during graceful shutdown.
    ///
    /// Uses an atomic guard to prevent concurrent calls from draining the list.
    /// Reverts are parallelized across pods with a 10s per-pod timeout and
    /// 30s total deadline. Faults that fail to revert or time out are requeued
    /// so subsequent calls can retry them.
    pub async fn revert_all(&self, fault_op: &FaultOperator) -> Vec<(Uuid, Result<()>)> {
        // Guard: only one revert_all can run at a time
        if self
            .reverting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            info!("Revert already in progress — waiting for completion");
            // Spin-wait for the first caller to finish
            while self.reverting.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            return Vec::new();
        }

        let faults = {
            let mut active = self.active.lock().await;
            std::mem::take(&mut *active)
        };

        if faults.is_empty() {
            self.reverting.store(false, Ordering::SeqCst);
            return Vec::new();
        }

        info!(
            count = faults.len(),
            "🧹 Reverting all active faults (graceful shutdown)..."
        );

        // Parallel reverts with per-pod timeout (10s) under a 30s total deadline
        let per_pod_timeout = std::time::Duration::from_secs(10);

        let mut join_set = tokio::task::JoinSet::new();

        for fault in faults {
            let fo = fault_op.clone();
            let timeout = per_pod_timeout;

            join_set.spawn(async move {
                let result = tokio::time::timeout(timeout, async {
                    match &fault.kind {
                        ActiveFaultKind::NetworkLatency => {
                            fo.revert_network_latency(
                                &fault.namespace,
                                &fault.pod_name,
                                fault.fault_id,
                            )
                            .await
                        }
                        ActiveFaultKind::Partition { target_ip } => {
                            fo.revert_partition(
                                &fault.namespace,
                                &fault.pod_name,
                                target_ip,
                                fault.fault_id,
                            )
                            .await
                        }
                        ActiveFaultKind::DnsFailure => {
                            fo.revert_dns_failure(&fault.namespace, &fault.pod_name, fault.fault_id)
                                .await
                        }
                        ActiveFaultKind::Stress {
                            debug_container: Some(container),
                        } => {
                            // Kill stress-ng in the specific debug container
                            let _ = tokio::process::Command::new("kubectl")
                                .args([
                                    "exec",
                                    "-n",
                                    &fault.namespace,
                                    &fault.pod_name,
                                    "-c",
                                    container,
                                    "--",
                                    "pkill",
                                    "-9",
                                    "stress-ng",
                                ])
                                .kill_on_drop(true)
                                .output()
                                .await;
                            Ok(())
                        }
                        ActiveFaultKind::Stress {
                            debug_container: None,
                        } => {
                            // Exec mode: pkill in the pod's default container
                            fo.exec_in_pod(
                                &fault.namespace,
                                &fault.pod_name,
                                &["pkill", "-9", "stress-ng"],
                            )
                            .await
                            .ok(); // Best-effort kill
                            Ok(())
                        }
                    }
                })
                .await;

                match result {
                    Ok(Ok(())) => {
                        info!(
                            fault_id = %fault.fault_id,
                            pod = fault.pod_name.as_str(),
                            kind = ?fault.kind,
                            "✅ Reverted fault"
                        );
                        (fault.fault_id, Ok(()), None) // success — don't requeue
                    }
                    Ok(Err(e)) => {
                        warn!(
                            fault_id = %fault.fault_id,
                            pod = fault.pod_name.as_str(),
                            error = %e,
                            "❌ Failed to revert fault — requeued for retry"
                        );
                        let err_msg = format!("{}", e);
                        (
                            fault.fault_id,
                            Err(anyhow::anyhow!("{}", err_msg)),
                            Some(fault), // requeue
                        )
                    }
                    Err(_) => {
                        warn!(
                            fault_id = %fault.fault_id,
                            pod = fault.pod_name.as_str(),
                            "⚠️  Revert timed out — requeued for retry"
                        );
                        (
                            fault.fault_id,
                            Err(anyhow::anyhow!("Revert timed out")),
                            Some(fault), // requeue
                        )
                    }
                }
            });
        }

        // Collect results with 30s total deadline
        let total_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut results = Vec::new();
        let mut requeue = Vec::new();

        while let Ok(Some(result)) =
            tokio::time::timeout_at(total_deadline, join_set.join_next()).await
        {
            if let Ok((fault_id, outcome, maybe_fault)) = result {
                results.push((fault_id, outcome));
                if let Some(fault) = maybe_fault {
                    requeue.push(fault);
                }
            }
        }

        // Requeue any faults that failed or timed out
        if !requeue.is_empty() {
            warn!(
                count = requeue.len(),
                "⚠️  {} fault(s) could not be reverted — requeued for manual cleanup",
                requeue.len()
            );
            let mut active = self.active.lock().await;
            active.extend(requeue);
        }

        let (ok, failed): (Vec<_>, Vec<_>) = results.iter().partition(|(_, r)| r.is_ok());
        info!(
            reverted = ok.len(),
            failed = failed.len(),
            "🧹 Fault cleanup complete"
        );

        self.reverting.store(false, Ordering::SeqCst);
        results
    }
}

impl Default for FaultTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_track_and_untrack() {
        let tracker = FaultTracker::new();
        let id = Uuid::new_v4();

        tracker
            .track(ActiveFault {
                fault_id: id,
                kind: ActiveFaultKind::NetworkLatency,
                namespace: "default".to_string(),
                pod_name: "test-pod".to_string(),
                injected_at: Instant::now(),
            })
            .await;

        assert_eq!(tracker.active_count().await, 1);

        tracker.untrack(id).await;
        assert_eq!(tracker.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_track_multiple() {
        let tracker = FaultTracker::new();

        for _ in 0..5 {
            tracker
                .track(ActiveFault {
                    fault_id: Uuid::new_v4(),
                    kind: ActiveFaultKind::DnsFailure,
                    namespace: "test".to_string(),
                    pod_name: "pod".to_string(),
                    injected_at: Instant::now(),
                })
                .await;
        }

        assert_eq!(tracker.active_count().await, 5);
    }

    #[tokio::test]
    async fn test_untrack_nonexistent_is_noop() {
        let tracker = FaultTracker::new();
        tracker.untrack(Uuid::new_v4()).await;
        assert_eq!(tracker.active_count().await, 0);
    }

    #[tokio::test]
    async fn test_default() {
        let tracker = FaultTracker::default();
        assert_eq!(tracker.active_count().await, 0);
    }
}
