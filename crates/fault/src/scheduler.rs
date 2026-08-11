//! Fault scheduler for Kubernetes targets

use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Types of faults that can be injected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaultType {
    /// Crash the target pod.
    Crash,
    /// Introduce latency to the target pod.
    Latency { delay_ms: u32, jitter_ms: u32 },
    /// Partition the target pod from the network.
    Partition,
}

/// A scheduled fault to be injected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledFault {
    /// The type of fault to inject.
    pub fault_type: FaultType,
    /// The target pod name.
    pub target_pod: String,
    /// The namespace of the target pod.
    pub namespace: String,
}

/// Scheduler for faults.
pub struct FaultScheduler {
    pub seed: u64,
    pub rng: StdRng,
    pub enabled_faults: Vec<FaultType>,
    pub target_pods: Vec<String>,
    pub namespace: String,
}

impl FaultScheduler {
    /// Creates a new fault scheduler.
    pub fn new(
        seed: u64,
        enabled_faults: Vec<FaultType>,
        target_pods: Vec<String>,
        namespace: String,
    ) -> Self {
        Self {
            seed,
            rng: StdRng::seed_from_u64(seed),
            enabled_faults,
            target_pods,
            namespace,
        }
    }

    /// Returns the next fault to inject.
    pub fn next_fault(&mut self) -> Option<ScheduledFault> {
        if self.enabled_faults.is_empty() || self.target_pods.is_empty() {
            return None;
        }

        let fault_idx = self.rng.random_range(0..self.enabled_faults.len());
        let pod_idx = self.rng.random_range(0..self.target_pods.len());

        Some(ScheduledFault {
            fault_type: self.enabled_faults[fault_idx].clone(),
            target_pod: self.target_pods[pod_idx].clone(),
            namespace: self.namespace.clone(),
        })
    }

    /// Returns a random delay between min and max seconds.
    pub fn next_delay(&mut self, min_secs: f64, max_secs: f64) -> Duration {
        if min_secs >= max_secs {
            return Duration::from_secs_f64(min_secs);
        }
        let delay = self.rng.random_range(min_secs..max_secs);
        Duration::from_secs_f64(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::time::Duration;

    #[test]
    fn test_new() {
        let scheduler = FaultScheduler::new(42, vec![], vec![], "default".to_string());
        assert_eq!(scheduler.seed, 42);
        assert_eq!(scheduler.namespace, "default");
    }

    #[test]
    fn test_next_fault_empty() {
        let mut scheduler =
            FaultScheduler::new(42, vec![], vec!["pod-1".to_string()], "ns".to_string());
        assert!(scheduler.next_fault().is_none());

        let mut scheduler =
            FaultScheduler::new(42, vec![FaultType::Crash], vec![], "ns".to_string());
        assert!(scheduler.next_fault().is_none());
    }

    #[test]
    fn test_next_fault_valid() {
        let mut scheduler = FaultScheduler::new(
            42,
            vec![FaultType::Crash],
            vec!["pod-1".to_string()],
            "ns".to_string(),
        );
        let fault = scheduler.next_fault().unwrap();
        assert_eq!(fault.fault_type, FaultType::Crash);
        assert_eq!(fault.target_pod, "pod-1");
        assert_eq!(fault.namespace, "ns");
    }

    #[test]
    fn test_determinism_same_seed() {
        let mut s1 = FaultScheduler::new(
            100,
            vec![FaultType::Crash, FaultType::Partition],
            vec!["p1".to_string(), "p2".to_string()],
            "ns".to_string(),
        );
        let mut s2 = FaultScheduler::new(
            100,
            vec![FaultType::Crash, FaultType::Partition],
            vec!["p1".to_string(), "p2".to_string()],
            "ns".to_string(),
        );

        for _ in 0..10 {
            let f1 = s1.next_fault().unwrap();
            let f2 = s2.next_fault().unwrap();
            assert_eq!(f1.fault_type, f2.fault_type);
            assert_eq!(f1.target_pod, f2.target_pod);
        }
    }

    #[test]
    fn test_determinism_different_seed() {
        let mut s1 = FaultScheduler::new(
            100,
            vec![FaultType::Crash, FaultType::Partition],
            vec!["p1".to_string(), "p2".to_string()],
            "ns".to_string(),
        );
        let mut s2 = FaultScheduler::new(
            200,
            vec![FaultType::Crash, FaultType::Partition],
            vec!["p1".to_string(), "p2".to_string()],
            "ns".to_string(),
        );

        let mut all_same = true;
        for _ in 0..10 {
            let f1 = s1.next_fault().unwrap();
            let f2 = s2.next_fault().unwrap();
            if f1.fault_type != f2.fault_type || f1.target_pod != f2.target_pod {
                all_same = false;
                break;
            }
        }
        assert!(!all_same);
    }

    #[test]
    fn test_all_faults_and_pods_selected() {
        let faults = vec![
            FaultType::Crash,
            FaultType::Partition,
            FaultType::Latency {
                delay_ms: 10,
                jitter_ms: 5,
            },
        ];
        let pods = vec!["p1".to_string(), "p2".to_string(), "p3".to_string()];
        let mut s = FaultScheduler::new(42, faults.clone(), pods.clone(), "ns".to_string());

        let mut seen_faults = std::collections::HashSet::new();
        let mut seen_pods = std::collections::HashSet::new();

        for _ in 0..100 {
            let f = s.next_fault().unwrap();
            if !seen_faults.iter().any(|x| x == &f.fault_type) {
                seen_faults.insert(f.fault_type.clone());
            }
            seen_pods.insert(f.target_pod);
        }

        assert_eq!(seen_faults.len(), 3);
        assert_eq!(seen_pods.len(), 3);
    }

    #[test]
    fn test_next_delay() {
        let mut s = FaultScheduler::new(42, vec![], vec![], "ns".to_string());
        let delay = s.next_delay(5.0, 5.0);
        assert_eq!(delay.as_secs_f64(), 5.0);

        let delay = s.next_delay(6.0, 5.0);
        assert_eq!(delay.as_secs_f64(), 6.0);

        for _ in 0..10 {
            let delay = s.next_delay(1.0, 5.0);
            assert!(delay.as_secs_f64() >= 1.0 && delay.as_secs_f64() < 5.0);
        }
    }

    proptest! {
        #[test]
        fn pbt_next_fault_valid(seed in any::<u64>()) {
            let faults = vec![FaultType::Crash, FaultType::Partition];
            let pods = vec!["p1".to_string(), "p2".to_string()];
            let mut s = FaultScheduler::new(seed, faults.clone(), pods.clone(), "ns".to_string());
            for _ in 0..10 {
                let f = s.next_fault().unwrap();
                prop_assert!(faults.contains(&f.fault_type));
                prop_assert!(pods.contains(&f.target_pod));
                prop_assert_eq!(f.namespace, "ns".to_string());
            }
        }

        #[test]
        fn pbt_next_delay_valid(seed in any::<u64>(), min in 0.0..1000.0f64, max in 0.0..1000.0f64) {
            let mut s = FaultScheduler::new(seed, vec![], vec![], "ns".to_string());
            let delay = s.next_delay(min, max);
            if min >= max {
                prop_assert_eq!(delay, std::time::Duration::from_secs_f64(min));
            } else {
                prop_assert!(delay >= std::time::Duration::from_secs_f64(min));
                prop_assert!(delay <= std::time::Duration::from_secs_f64(max));
            }
        }

        #[test]
        fn pbt_determinism(seed in any::<u64>()) {
            let faults = vec![FaultType::Crash, FaultType::Partition];
            let pods = vec!["p1".to_string(), "p2".to_string()];
            let mut s1 = FaultScheduler::new(seed, faults.clone(), pods.clone(), "ns".to_string());
            let mut s2 = FaultScheduler::new(seed, faults.clone(), pods.clone(), "ns".to_string());
            for _ in 0..10 {
                let f1 = s1.next_fault().unwrap();
                let f2 = s2.next_fault().unwrap();
                prop_assert_eq!(f1.fault_type, f2.fault_type);
                prop_assert_eq!(f1.target_pod, f2.target_pod);
            }
        }
    }
}
