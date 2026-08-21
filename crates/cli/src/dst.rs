//! Deterministic Simulation Testing (DST) engine.
//!
//! Runs a complete chaos simulation in-memory using a discrete-event loop.
//! Same seed → same timeline → same hash.

use crate::properties::PropertyDef;
use heisensim_core::clock::VirtualClock;
use heisensim_core::net::VirtualNetwork;
use heisensim_core::types::{NodeId, VirtualTime};
use heisensim_fault::scheduler::{FaultScheduler, FaultType};
use heisensim_props::PropertyVerdict;
use heisensim_timeline::event::{EventKind, TimelineEvent};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::BTreeMap;
use uuid::Uuid;

use chrono::{DateTime, Utc};

use std::sync::LazyLock;

/// Fixed epoch for deterministic timestamps.
static DST_EPOCH_DT: LazyLock<DateTime<Utc>> = LazyLock::new(|| {
    "2025-01-01T00:00:00Z"
        .parse::<DateTime<Utc>>()
        .expect("invalid DST epoch")
});

/// Priority levels for timer ordering at the same virtual time.
const CONTROL_PRIORITY: u8 = 0;
const FAULT_PRIORITY: u8 = 5;
const PROBE_PRIORITY: u8 = 10;

/// Configuration for a DST simulation run.
#[derive(Debug, Clone)]
pub struct DstConfig {
    pub seed: u64,
    pub duration: VirtualTime,
    pub warmup: VirtualTime,
    pub faults: Vec<String>,
    pub pod_count: usize,
    pub probe_interval: VirtualTime,
    pub property_defs: Vec<PropertyDef>,
}

/// Result of a DST simulation run.
#[derive(Debug)]
pub struct DstResult {
    pub events: Vec<TimelineEvent>,
    pub hash: u64,
    pub verdicts: Vec<PropertyVerdict>,
    pub seed: u64,
    pub total_faults: usize,
    pub total_failures: usize,
}

/// Internal event types for the discrete-event loop.
#[derive(Debug, Clone)]
enum SimEvent {
    Probe { pod_idx: usize },
    ScheduleFault,
    RevertFault { fault_id: Uuid },
    End,
}

/// Tracks an active fault in the simulation.
struct ActiveFault {
    fault_kind: String,
    target_node: NodeId,
    partition_id: Option<u64>,
    latency_params: Option<(u32, u32)>,
}

/// Generate a deterministic UUID from a seeded RNG.
fn deterministic_uuid(rng: &mut StdRng) -> Uuid {
    let bytes: [u8; 16] = rng.random();
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

/// Generate a deterministic timestamp from virtual elapsed time.
fn virtual_timestamp(elapsed: VirtualTime) -> DateTime<Utc> {
    *DST_EPOCH_DT + chrono::Duration::nanoseconds(elapsed.0 as i64)
}

/// Create a TimelineEvent with deterministic id and timestamp.
fn make_event(rng: &mut StdRng, elapsed: VirtualTime, kind: EventKind) -> TimelineEvent {
    TimelineEvent {
        id: deterministic_uuid(rng),
        timestamp: virtual_timestamp(elapsed),
        elapsed: elapsed.as_std_duration(),
        kind,
    }
}

/// Run a deterministic simulation. Same seed = same result.
pub fn run(config: DstConfig) -> anyhow::Result<DstResult> {
    anyhow::ensure!(config.pod_count > 0, "pod_count must be at least 1");
    anyhow::ensure!(
        config.probe_interval > VirtualTime(0),
        "probe_interval must be greater than 0"
    );
    let mut rng = StdRng::seed_from_u64(config.seed);
    let mut clock: VirtualClock<SimEvent> = VirtualClock::new();
    let network_seed: u64 = rng.random();
    let mut network = VirtualNetwork::new_seeded(network_seed);
    let mut events: Vec<TimelineEvent> = Vec::new();
    let mut active_faults: BTreeMap<Uuid, ActiveFault> = BTreeMap::new();
    let mut total_faults: usize = 0;
    let mut total_failures: usize = 0;

    // Generate pod names
    let pod_names: Vec<String> = (0..config.pod_count)
        .map(|i| format!("pod-{}", i))
        .collect();

    let probe_names: Vec<String> = pod_names
        .iter()
        .map(|name| format!("http/{}", name))
        .collect();

    // Build fault types for scheduler — reject unknown fault names
    let mut fault_types: Vec<FaultType> = Vec::new();
    for f in &config.faults {
        let ft = match f.as_str() {
            "crash" => FaultType::Crash,
            "latency" => FaultType::Latency {
                delay_ms: 0,
                jitter_ms: 0,
            },
            "partition" => FaultType::Partition,
            "stress" => FaultType::Stress {
                cpu_workers: 0,
                mem_bytes: 0,
            },
            "dns" => FaultType::Dns,
            "eviction" => FaultType::Eviction,
            other => anyhow::bail!(
                "Unknown fault type '{}'. Supported: crash, latency, partition, stress, dns, eviction",
                other
            ),
        };
        fault_types.push(ft);
    }

    let mut scheduler = FaultScheduler::new(
        config.seed,
        fault_types,
        pod_names.clone(),
        "sim".to_string(),
    );

    // A "controller" node (NodeId 0) represents the probe sender
    let controller = NodeId(0);

    // Emit SimulationStarted
    events.push(make_event(
        &mut rng,
        VirtualTime(0),
        EventKind::SimulationStarted {
            seed: config.seed,
            duration_secs: config.duration.as_secs() as f64,
        },
    ));

    // Schedule initial probe timers for each pod
    for pod_idx in 0..config.pod_count {
        // Stagger probes slightly so they don't all fire at the same time
        let offset = VirtualTime::from_millis(100 * pod_idx as u64);
        clock.schedule(
            config.probe_interval + offset,
            PROBE_PRIORITY,
            SimEvent::Probe { pod_idx },
        );
    }

    // Schedule first fault after warmup
    if !config.faults.is_empty() {
        clock.schedule(config.warmup, FAULT_PRIORITY, SimEvent::ScheduleFault);
    }

    // Schedule simulation end
    clock.schedule(config.duration, CONTROL_PRIORITY, SimEvent::End);

    // === Discrete-event loop ===
    'outer: loop {
        let expired = clock.tick();
        if expired.is_empty() {
            break;
        }

        for timer in expired {
            match timer.payload {
                SimEvent::End => {
                    break 'outer;
                }

                SimEvent::Probe { pod_idx } => {
                    let now = clock.now();
                    let target_node = NodeId((pod_idx + 1) as u64); // pod nodes are 1-indexed
                    let probe_name = probe_names[pod_idx].clone();

                    // Check if the pod is reachable via the virtual network
                    let is_partitioned = network.has_partition_between(controller, target_node);

                    let mut is_crashed = false;
                    let mut is_dns_blocked = false;
                    let mut has_latency_fault = false;
                    let mut latency_params: Option<(u32, u32)> = None;

                    for fault in active_faults.values() {
                        if fault.target_node == target_node {
                            match fault.fault_kind.as_str() {
                                "crash" | "eviction" => is_crashed = true,
                                "dns" => is_dns_blocked = true,
                                "latency" => {
                                    has_latency_fault = true;
                                    latency_params = fault.latency_params;
                                }
                                "stress" => {
                                    has_latency_fault = true;
                                    // Stress causes degraded latency but not as severe as explicit latency injection
                                    if latency_params.is_none() {
                                        latency_params = Some((100, 200)); // moderate degradation from CPU pressure
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    if is_crashed || is_partitioned || is_dns_blocked {
                        // Probe fails
                        total_failures += 1;
                        let error = if is_crashed {
                            "connection refused (pod crashed)".to_string()
                        } else if is_partitioned {
                            "network unreachable (partitioned)".to_string()
                        } else {
                            "DNS resolution failed".to_string()
                        };
                        events.push(make_event(
                            &mut rng,
                            now,
                            EventKind::ProbeFailed {
                                probe_name: probe_name.clone(),
                                error,
                                latency_ms: None,
                            },
                        ));
                    } else {
                        // Probe succeeds
                        let base_latency: u64 = rng.random_range(5..50);
                        let latency = if has_latency_fault {
                            let (delay, jitter) = latency_params.unwrap_or((200, 100));
                            base_latency + rng.random_range(delay as u64..(delay + jitter) as u64)
                        } else {
                            base_latency
                        };
                        events.push(make_event(
                            &mut rng,
                            now,
                            EventKind::ProbeSuccess {
                                probe_name: probe_name.clone(),
                                latency_ms: latency,
                                status_code: Some(200),
                            },
                        ));
                    }

                    // Reschedule next probe
                    clock.schedule_after(
                        config.probe_interval,
                        PROBE_PRIORITY,
                        SimEvent::Probe { pod_idx },
                    );
                }

                SimEvent::ScheduleFault => {
                    let now = clock.now();
                    if now >= config.duration {
                        continue;
                    }

                    if let Some(scheduled) = scheduler.next_fault() {
                        let fault_id = deterministic_uuid(&mut rng);
                        let target_pod_idx = pod_names
                            .iter()
                            .position(|n| *n == scheduled.target_pod)
                            .unwrap_or(0);
                        let target_node = NodeId((target_pod_idx + 1) as u64);

                        let mut latency_params = None;
                        let (fault_kind_str, duration_secs) = match &scheduled.fault_type {
                            FaultType::Crash => ("crash", 15.0),
                            FaultType::Latency { .. } => {
                                let delay_ms = 200 + (rng.random::<u32>() % 500);
                                let jitter_ms = 50 + (rng.random::<u32>() % 100);
                                latency_params = Some((delay_ms, jitter_ms));
                                ("latency", 15.0)
                            }
                            FaultType::Partition => ("partition", 20.0),
                            FaultType::Stress { .. } => ("stress", 10.0),
                            FaultType::Dns => ("dns", 10.0),
                            FaultType::Eviction => ("eviction", 15.0),
                        };

                        // Apply fault effects to virtual network
                        let mut partition_id = None;
                        if fault_kind_str == "partition" {
                            use std::collections::BTreeSet;
                            let mut a = BTreeSet::new();
                            a.insert(controller);
                            let mut b = BTreeSet::new();
                            b.insert(target_node);
                            partition_id = Some(network.add_partition(a, b));
                        }

                        active_faults.insert(
                            fault_id,
                            ActiveFault {
                                fault_kind: fault_kind_str.to_string(),
                                target_node,
                                partition_id,
                                latency_params,
                            },
                        );

                        total_faults += 1;
                        events.push(make_event(
                            &mut rng,
                            now,
                            EventKind::FaultInjected {
                                fault_id,
                                fault_kind: fault_kind_str.to_string(),
                                target: scheduled.target_pod.clone(),
                                duration_secs: Some(duration_secs),
                            },
                        ));

                        // Schedule revert
                        let revert_delay = VirtualTime::from_secs(duration_secs as u64);
                        clock.schedule_after(
                            revert_delay,
                            FAULT_PRIORITY,
                            SimEvent::RevertFault { fault_id },
                        );
                    }

                    // Schedule next fault injection
                    let delay = scheduler.next_delay(10.0, 30.0);
                    let delay_vt = VirtualTime::from_millis(delay.as_millis() as u64);
                    clock.schedule_after(delay_vt, FAULT_PRIORITY, SimEvent::ScheduleFault);
                }

                SimEvent::RevertFault { fault_id } => {
                    let now = clock.now();
                    if let Some(fault) = active_faults.remove(&fault_id) {
                        // Remove network partition if applicable
                        if let Some(pid) = fault.partition_id {
                            network.remove_partition(pid);
                        }
                    }

                    events.push(make_event(
                        &mut rng,
                        now,
                        EventKind::FaultReverted { fault_id },
                    ));
                }
            }
        }
    }

    // Emit SimulationEnded
    events.push(make_event(
        &mut rng,
        config.duration,
        EventKind::SimulationEnded {
            total_faults,
            total_failures,
        },
    ));

    // Compute deterministic hash of the timeline
    let hash = compute_timeline_hash(&events);

    // Evaluate properties
    let verdicts = if !config.property_defs.is_empty() {
        crate::properties::evaluate_properties(&events, &config.property_defs)
    } else {
        Vec::new()
    };

    Ok(DstResult {
        events,
        hash,
        verdicts,
        seed: config.seed,
        total_faults,
        total_failures,
    })
}

fn compute_timeline_hash(events: &[TimelineEvent]) -> u64 {
    use xxhash_rust::xxh64::xxh64;
    // Hash the serialized event kinds and timestamps for determinism
    let mut data = Vec::new();
    for event in events {
        // Include id, elapsed, and kind in the hash
        data.extend_from_slice(event.id.as_bytes());
        data.extend_from_slice(&event.elapsed.as_nanos().to_le_bytes());
        // Serialize kind to JSON for consistent hashing
        let kind_json = serde_json::to_string(&event.kind).expect("EventKind must be serializable");
        let len = (kind_json.len() as u32).to_le_bytes();
        data.extend_from_slice(&len);
        data.extend_from_slice(kind_json.as_bytes());
    }
    xxh64(&data, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(seed: u64) -> DstConfig {
        DstConfig {
            seed,
            duration: VirtualTime::from_secs(300), // 5 minutes
            warmup: VirtualTime::from_secs(30),
            faults: vec![
                "crash".to_string(),
                "latency".to_string(),
                "partition".to_string(),
            ],
            pod_count: 3,
            probe_interval: VirtualTime::from_secs(5),
            property_defs: Vec::new(),
        }
    }

    #[test]
    fn test_dst_determinism() {
        let r1 = run(test_config(42)).unwrap();
        let r2 = run(test_config(42)).unwrap();

        assert_eq!(r1.hash, r2.hash, "Same seed must produce same hash");
        assert_eq!(r1.events.len(), r2.events.len());

        for (a, b) in r1.events.iter().zip(r2.events.iter()) {
            assert_eq!(a.id, b.id, "Event IDs must match");
            assert_eq!(a.elapsed, b.elapsed, "Elapsed times must match");
            assert_eq!(a.timestamp, b.timestamp, "Timestamps must match");
        }
    }

    #[test]
    fn test_dst_different_seeds_diverge() {
        let r1 = run(test_config(42)).unwrap();
        let r2 = run(test_config(43)).unwrap();

        assert_ne!(
            r1.hash, r2.hash,
            "Different seeds must produce different hashes"
        );
    }

    #[test]
    fn test_dst_time_compression() {
        let result = run(test_config(42)).unwrap();
        // Verify simulation produced events spanning the full virtual duration
        // without asserting wall-clock time (which is flaky on CI)
        assert!(!result.events.is_empty());
        assert!(result.total_faults > 0);
        // Verify events span the expected virtual timeline
        let last_event = result.events.last().unwrap();
        assert!(
            last_event.elapsed.as_secs() >= 299,
            "Simulation should span ~300s of virtual time, got {}s",
            last_event.elapsed.as_secs()
        );
    }

    #[test]
    fn test_dst_events_have_faults_and_probes() {
        let result = run(test_config(42)).unwrap();

        let has_fault = result
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::FaultInjected { .. }));
        let has_probe = result.events.iter().any(|e| {
            matches!(
                e.kind,
                EventKind::ProbeSuccess { .. } | EventKind::ProbeFailed { .. }
            )
        });
        let has_revert = result
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::FaultReverted { .. }));

        assert!(has_fault, "Should have fault injections");
        assert!(has_probe, "Should have probe events");
        assert!(has_revert, "Should have fault reverts");
    }

    #[test]
    fn test_dst_partition_causes_probe_failure() {
        // Use only partition faults
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(120),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["partition".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(2),
            property_defs: Vec::new(),
        };

        let result = run(config).unwrap();
        let has_failure = result.events.iter().any(|e| {
            matches!(e.kind, EventKind::ProbeFailed { ref error, .. } if error.contains("partitioned"))
        });

        assert!(has_failure, "Partition fault should cause probe failures");
    }

    #[test]
    fn test_dst_no_faults() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(60),
            warmup: VirtualTime::from_secs(10),
            faults: Vec::new(),
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(5),
            property_defs: Vec::new(),
        };

        let result = run(config).unwrap();
        assert_eq!(result.total_faults, 0);
        assert_eq!(result.total_failures, 0);
        // All probes should succeed
        let all_success = result
            .events
            .iter()
            .all(|e| !matches!(e.kind, EventKind::ProbeFailed { .. }));
        assert!(all_success, "With no faults, all probes should succeed");
    }

    #[test]
    fn test_dst_hash_stable() {
        // Run 10 times, all hashes must be identical
        let hashes: Vec<u64> = (0..10)
            .map(|_| run(test_config(0xBEEF)).unwrap().hash)
            .collect();
        assert!(
            hashes.windows(2).all(|w| w[0] == w[1]),
            "Hash must be stable across runs"
        );
    }

    #[test]
    fn test_dst_zero_pods_errors() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(60),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["crash".to_string()],
            pod_count: 0,
            probe_interval: VirtualTime::from_secs(5),
            property_defs: Vec::new(),
        };
        let err = run(config).unwrap_err();
        assert!(
            err.to_string().contains("pod_count must be at least 1"),
            "Expected pod_count error, got: {}",
            err
        );
    }

    #[test]
    fn test_dst_warmup_exceeds_duration() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(30),
            warmup: VirtualTime::from_secs(60), // warmup > duration
            faults: vec!["crash".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(5),
            property_defs: Vec::new(),
        };
        let result = run(config).unwrap();
        // Should complete without panicking, faults just never fire
        assert_eq!(result.total_faults, 0);
    }

    #[test]
    fn test_dst_crash_fault_causes_probe_failure() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(120),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["crash".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(2),
            property_defs: Vec::new(),
        };
        let result = run(config).unwrap();
        let has_crash_failure = result.events.iter().any(|e| {
            matches!(e.kind, EventKind::ProbeFailed { ref error, .. } if error.contains("crashed"))
        });
        assert!(has_crash_failure, "Crash fault should cause probe failures");
    }

    #[test]
    fn test_dst_dns_fault_causes_probe_failure() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(120),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["dns".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(2),
            property_defs: Vec::new(),
        };
        let result = run(config).unwrap();
        let has_dns_failure = result.events.iter().any(
            |e| matches!(e.kind, EventKind::ProbeFailed { ref error, .. } if error.contains("DNS")),
        );
        assert!(has_dns_failure, "DNS fault should cause probe failures");
    }

    #[test]
    fn test_dst_latency_fault_increases_latency() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(120),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["latency".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(2),
            property_defs: Vec::new(),
        };
        let result = run(config).unwrap();

        // Find probe successes during fault period (high latency)
        let high_latency_probes: Vec<_> = result.events.iter().filter(|e| {
            matches!(&e.kind, EventKind::ProbeSuccess { latency_ms, .. } if *latency_ms > 100)
        }).collect();

        assert!(
            !high_latency_probes.is_empty(),
            "Latency fault should cause elevated probe latency"
        );
    }

    #[test]
    fn test_dst_stress_fault_affects_probes() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(120),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["stress".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(2),
            property_defs: Vec::new(),
        };
        let result = run(config).unwrap();

        // Stress faults should cause elevated latency
        let high_latency_probes: Vec<_> = result.events.iter().filter(|e| {
            matches!(&e.kind, EventKind::ProbeSuccess { latency_ms, .. } if *latency_ms > 50)
        }).collect();

        assert!(
            !high_latency_probes.is_empty(),
            "Stress fault should cause elevated probe latency"
        );
    }

    #[test]
    fn test_dst_eviction_causes_probe_failure() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(120),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["eviction".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(2),
            property_defs: Vec::new(),
        };
        let result = run(config).unwrap();
        let has_failure = result.events.iter().any(|e| {
            matches!(e.kind, EventKind::ProbeFailed { ref error, .. } if error.contains("crashed"))
        });
        assert!(
            has_failure,
            "Eviction fault should cause probe failures (treated as crash)"
        );
    }

    #[test]
    fn test_dst_overlapping_faults() {
        // Multiple fault types simultaneously
        let config = DstConfig {
            seed: 100,
            duration: VirtualTime::from_secs(60),
            warmup: VirtualTime::from_secs(5),
            faults: vec![
                "crash".to_string(),
                "latency".to_string(),
                "partition".to_string(),
                "dns".to_string(),
                "stress".to_string(),
            ],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(2),
            property_defs: Vec::new(),
        };
        let result = run(config.clone()).unwrap();
        // Should complete without panicking and produce diverse events
        assert!(result.total_faults > 0);
        assert!(!result.events.is_empty());
        // Determinism still holds
        let r2 = run(config).unwrap();
        assert_eq!(result.hash, r2.hash);
    }

    #[test]
    fn test_dst_unknown_fault_errors() {
        let config = DstConfig {
            seed: 42,
            duration: VirtualTime::from_secs(60),
            warmup: VirtualTime::from_secs(10),
            faults: vec!["typo_fault".to_string()],
            pod_count: 2,
            probe_interval: VirtualTime::from_secs(5),
            property_defs: Vec::new(),
        };
        let err = run(config).unwrap_err();
        assert!(
            err.to_string().contains("Unknown fault type"),
            "Expected unknown fault error, got: {}",
            err
        );
    }
}
