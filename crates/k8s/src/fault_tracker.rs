use anyhow::Result;
use std::sync::Arc;
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
    /// CPU/memory stress via stress-ng (self-terminating, tracked for bookkeeping)
    Stress,
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
pub struct FaultTracker {
    active: Arc<Mutex<Vec<ActiveFault>>>,
}

impl FaultTracker {
    /// Create a new empty fault tracker.
    pub fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(Vec::new())),
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
    /// Returns a vec of (fault_id, result) for each revert attempt.
    /// Uses a 30s total timeout — if K8s API is unreachable, logs loudly.
    pub async fn revert_all(&self, fault_op: &FaultOperator) -> Vec<(Uuid, Result<()>)> {
        let faults = {
            let mut active = self.active.lock().await;
            std::mem::take(&mut *active)
        };

        if faults.is_empty() {
            return Vec::new();
        }

        info!(
            count = faults.len(),
            "🧹 Reverting all active faults (graceful shutdown)..."
        );

        let mut results = Vec::new();

        // 30s total timeout for all reverts
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);

        for fault in faults {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                warn!(
                    fault_id = %fault.fault_id,
                    pod = fault.pod_name.as_str(),
                    "⚠️  Revert timeout reached — manual intervention may be required!"
                );
                results.push((
                    fault.fault_id,
                    Err(anyhow::anyhow!("Revert timeout exceeded")),
                ));
                continue;
            }

            let result = tokio::time::timeout(remaining, async {
                match &fault.kind {
                    ActiveFaultKind::NetworkLatency => {
                        fault_op
                            .revert_network_latency(
                                &fault.namespace,
                                &fault.pod_name,
                                fault.fault_id,
                            )
                            .await
                    }
                    ActiveFaultKind::Partition { target_ip } => {
                        fault_op
                            .revert_partition(
                                &fault.namespace,
                                &fault.pod_name,
                                target_ip,
                                fault.fault_id,
                            )
                            .await
                    }
                    ActiveFaultKind::DnsFailure => {
                        fault_op
                            .revert_dns_failure(&fault.namespace, &fault.pod_name, fault.fault_id)
                            .await
                    }
                    ActiveFaultKind::Stress => {
                        // Stress-ng is self-terminating; just emit revert event
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
                    results.push((fault.fault_id, Ok(())));
                }
                Ok(Err(e)) => {
                    warn!(
                        fault_id = %fault.fault_id,
                        pod = fault.pod_name.as_str(),
                        error = %e,
                        "❌ Failed to revert fault"
                    );
                    results.push((fault.fault_id, Err(e)));
                }
                Err(_) => {
                    warn!(
                        fault_id = %fault.fault_id,
                        pod = fault.pod_name.as_str(),
                        "⚠️  Revert timed out — manual intervention may be required!"
                    );
                    results.push((fault.fault_id, Err(anyhow::anyhow!("Revert timed out"))));
                }
            }
        }

        let (ok, failed): (Vec<_>, Vec<_>) = results.iter().partition(|(_, r)| r.is_ok());
        info!(
            reverted = ok.len(),
            failed = failed.len(),
            "🧹 Fault cleanup complete"
        );

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
