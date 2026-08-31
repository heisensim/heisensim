//! A/B Baseline Diffing — measure baseline without chaos, assert on degradation delta.
//!
//! Instead of requiring users to know absolute SLA numbers upfront, this module
//! captures a baseline of probe metrics during a no-fault warmup period, then
//! compares chaos-phase metrics against that baseline.
//!
//! Example verdicts:
//! - ✅ baseline-latency: p95 increased 1.8x (baseline 120ms → chaos 222ms) — within 3.0x threshold
//! - ❌ baseline-availability: success rate dropped 15pp (baseline 99.5% → 84.2%) — exceeds 10pp threshold

use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Per-probe baseline metrics captured during the no-fault warmup period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeBaseline {
    /// Number of successful probes during baseline
    pub success_count: usize,
    /// Number of failed/timed-out probes during baseline
    pub failure_count: usize,
    /// All observed latencies in milliseconds
    pub latencies_ms: Vec<u64>,
    /// Success rate as a percentage (0-100)
    pub success_rate: f64,
    /// Median latency in milliseconds
    pub p50_ms: u64,
    /// 95th percentile latency in milliseconds
    pub p95_ms: u64,
}

/// Snapshot of all probe baselines captured during the no-fault period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineSnapshot {
    /// Per-probe baseline metrics, keyed by probe name
    pub probes: HashMap<String, ProbeBaseline>,
    /// Duration of the baseline measurement window
    pub duration: Duration,
    /// Total probe samples across all probes
    pub total_samples: usize,
}

/// Calculate the smart baseline duration based on probe intervals.
///
/// Returns `max(10s, max_probe_interval * 10)` to ensure sufficient sample
/// count for valid statistics. With a 10x multiplier, even the slowest probe
/// gets ~10 data points.
pub fn smart_baseline_duration(probe_intervals: &[Duration]) -> Duration {
    let min_duration = Duration::from_secs(10);
    let max_interval = probe_intervals
        .iter()
        .copied()
        .max()
        .unwrap_or(Duration::from_secs(1));
    std::cmp::max(min_duration, max_interval * 10)
}

/// Capture baseline metrics from timeline events within a time window.
///
/// Events are filtered to those with `elapsed < baseline_duration`.
/// Returns `None` if no probe events were recorded during the baseline.
pub fn capture_baseline(
    events: &[TimelineEvent],
    baseline_duration: Duration,
) -> Option<BaselineSnapshot> {
    let mut probes: HashMap<String, (Vec<u64>, usize, usize)> = HashMap::new();

    for event in events {
        if event.elapsed >= baseline_duration {
            continue; // Skip events outside baseline window (may not be strictly ordered)
        }

        match &event.kind {
            EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                ..
            } => {
                let entry = probes.entry(probe_name.clone()).or_default();
                entry.0.push(*latency_ms);
                entry.1 += 1; // success
            }
            EventKind::ProbeFailed {
                probe_name,
                latency_ms,
                ..
            } => {
                let entry = probes.entry(probe_name.clone()).or_default();
                if let Some(ms) = latency_ms {
                    entry.0.push(*ms);
                }
                entry.2 += 1; // failure
            }
            EventKind::ProbeTimeout { probe_name, .. } => {
                let entry = probes.entry(probe_name.clone()).or_default();
                entry.2 += 1; // failure
            }
            _ => {}
        }
    }

    if probes.is_empty() {
        return None;
    }

    let mut total_samples = 0;
    let mut snapshot_probes = HashMap::new();

    for (name, (mut latencies, successes, failures)) in probes {
        latencies.sort_unstable();
        let total = successes + failures;
        total_samples += total;

        let success_rate = if total > 0 {
            (successes as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        let p50_ms = percentile(&latencies, 50);
        let p95_ms = percentile(&latencies, 95);

        snapshot_probes.insert(
            name,
            ProbeBaseline {
                success_count: successes,
                failure_count: failures,
                latencies_ms: latencies,
                success_rate,
                p50_ms,
                p95_ms,
            },
        );
    }

    Some(BaselineSnapshot {
        probes: snapshot_probes,
        duration: baseline_duration,
        total_samples,
    })
}

/// Extract chaos-phase probe metrics from timeline events after the baseline window.
fn chaos_phase_metrics(
    events: &[TimelineEvent],
    baseline_duration: Duration,
) -> HashMap<String, (Vec<u64>, usize, usize)> {
    let mut probes: HashMap<String, (Vec<u64>, usize, usize)> = HashMap::new();

    for event in events {
        if event.elapsed < baseline_duration {
            continue; // Skip baseline window
        }

        match &event.kind {
            EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                ..
            } => {
                let entry = probes.entry(probe_name.clone()).or_default();
                entry.0.push(*latency_ms);
                entry.1 += 1;
            }
            EventKind::ProbeFailed {
                probe_name,
                latency_ms,
                ..
            } => {
                let entry = probes.entry(probe_name.clone()).or_default();
                if let Some(ms) = latency_ms {
                    entry.0.push(*ms);
                }
                entry.2 += 1;
            }
            EventKind::ProbeTimeout { probe_name, .. } => {
                let entry = probes.entry(probe_name.clone()).or_default();
                entry.2 += 1;
            }
            _ => {}
        }
    }

    probes
}

/// Calculate a percentile value from a sorted slice.
fn percentile(sorted: &[u64], p: u32) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((p as f64 / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Property that asserts chaos-phase latency does not exceed a multiplier of baseline.
///
/// Example: with `max_multiplier = 3.0`, if baseline p95 is 100ms, chaos p95
/// must be ≤ 300ms.
pub struct BaselineLatencyDiff {
    baseline: BaselineSnapshot,
    baseline_duration: Duration,
    max_multiplier: f64,
}

impl BaselineLatencyDiff {
    /// Create a new baseline latency diff property.
    ///
    /// `max_multiplier` is the maximum allowed latency increase as a multiplier.
    /// For example, 3.0 means chaos latency can be at most 3x baseline.
    pub fn new(
        baseline: BaselineSnapshot,
        baseline_duration: Duration,
        max_multiplier: f64,
    ) -> Self {
        Self {
            baseline,
            baseline_duration,
            max_multiplier,
        }
    }
}

impl TimelineProperty for BaselineLatencyDiff {
    fn name(&self) -> &str {
        "baseline-latency"
    }

    fn description(&self) -> &str {
        "Asserts chaos-phase latency does not exceed a multiplier of baseline"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let chaos = chaos_phase_metrics(events, self.baseline_duration);
        let mut worst_multiplier: f64 = 0.0;
        let mut worst_probe = String::new();
        let mut details = Vec::new();
        let mut any_exceeded = false;
        let mut probes_evaluated = 0;

        for (probe_name, baseline) in &self.baseline.probes {
            // Clamp zero baseline p95 to 1ms to avoid skipping sub-millisecond probes
            let baseline_p95 = baseline.p95_ms.max(1);

            // Warn on small sample sizes
            let sample_count = baseline.latencies_ms.len();
            if sample_count > 0 && sample_count < 20 {
                details.push(format!(
                    "⚠️  {}: only {} baseline samples (p95 ≈ max), consider longer warmup",
                    probe_name, sample_count
                ));
            }

            // Check if probe disappeared during chaos
            let chaos_data = chaos.get(probe_name);
            if chaos_data.is_none() {
                // Probe existed in baseline but vanished during chaos — treat as failure
                any_exceeded = true;
                probes_evaluated += 1;
                details.push(format!(
                    "❌ {}: probe disappeared during chaos (0 events — possible crash/hang)",
                    probe_name
                ));
                continue;
            }

            let (latencies, successes, failures) = chaos_data.unwrap();
            let chaos_total = successes + failures;

            // If probe had events but zero latency samples (all timeouts/failures), treat as degraded
            if latencies.is_empty() {
                if chaos_total > 0 {
                    // 100% failure during chaos — treat as worst case
                    any_exceeded = true;
                    probes_evaluated += 1;
                    details.push(format!(
                        "❌ {}: 100% probe failure during chaos ({} failures, 0 latency samples)",
                        probe_name, failures
                    ));
                }
                continue;
            }

            let mut sorted = latencies.clone();
            sorted.sort_unstable();
            let chaos_p95 = percentile(&sorted, 95);

            let multiplier = chaos_p95 as f64 / baseline_p95 as f64;
            probes_evaluated += 1;

            let status = if multiplier > self.max_multiplier {
                any_exceeded = true;
                "❌"
            } else {
                "✅"
            };

            details.push(format!(
                "{} {}: p95 {:.1}x (baseline {}ms → chaos {}ms)",
                status, probe_name, multiplier, baseline_p95, chaos_p95
            ));

            if multiplier > worst_multiplier {
                worst_multiplier = multiplier;
                worst_probe = probe_name.clone();
            }
        }

        if probes_evaluated == 0 {
            let expected = format!("p95 latency ≤ {:.1}x baseline", self.max_multiplier);
            return PropertyVerdict::fail(
                "baseline-latency",
                expected,
                "no probes evaluated".to_string(),
            )
            .with_details(details);
        }

        let expected = format!("p95 latency ≤ {:.1}x baseline", self.max_multiplier);
        let actual = if worst_probe.is_empty() {
            "all probes failed or disappeared".to_string()
        } else {
            format!("worst: {:.1}x on {}", worst_multiplier, worst_probe)
        };

        if any_exceeded {
            PropertyVerdict::fail("baseline-latency", expected, actual).with_details(details)
        } else {
            PropertyVerdict::pass("baseline-latency", expected, actual).with_details(details)
        }
    }
}

/// Property that asserts chaos-phase availability does not drop more than
/// a specified number of percentage points from baseline.
///
/// Example: with `max_drop_pp = 10.0`, if baseline availability is 99.5%,
/// chaos availability must be ≥ 89.5%.
pub struct BaselineAvailabilityDiff {
    baseline: BaselineSnapshot,
    baseline_duration: Duration,
    max_drop_pp: f64,
}

impl BaselineAvailabilityDiff {
    /// Create a new baseline availability diff property.
    ///
    /// `max_drop_pp` is the maximum allowed availability drop in percentage points.
    /// For example, 10.0 means availability can drop at most 10pp from baseline.
    pub fn new(baseline: BaselineSnapshot, baseline_duration: Duration, max_drop_pp: f64) -> Self {
        Self {
            baseline,
            baseline_duration,
            max_drop_pp,
        }
    }
}

impl TimelineProperty for BaselineAvailabilityDiff {
    fn name(&self) -> &str {
        "baseline-availability"
    }

    fn description(&self) -> &str {
        "Asserts chaos-phase availability does not drop more than N percentage points from baseline"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let chaos = chaos_phase_metrics(events, self.baseline_duration);
        let mut worst_drop: f64 = 0.0;
        let mut worst_probe = String::new();
        let mut details = Vec::new();
        let mut any_exceeded = false;
        let mut probes_evaluated = 0;

        for (probe_name, baseline) in &self.baseline.probes {
            // Check if probe disappeared during chaos
            let chaos_data = chaos.get(probe_name);
            let (chaos_successes, chaos_failures) = if let Some((_, s, f)) = chaos_data {
                (*s, *f)
            } else {
                // Probe existed in baseline but vanished during chaos — treat as 0% availability
                let drop_pp = baseline.success_rate; // e.g. 100% → 0% = 100pp drop
                any_exceeded = drop_pp > self.max_drop_pp;
                probes_evaluated += 1;
                let status = if any_exceeded { "❌" } else { "✅" };
                details.push(format!(
                    "{} {}: probe disappeared during chaos (baseline {:.1}% → chaos 0.0%)",
                    status, probe_name, baseline.success_rate
                ));
                if drop_pp > worst_drop {
                    worst_drop = drop_pp;
                    worst_probe = probe_name.clone();
                }
                continue;
            };

            let chaos_total = chaos_successes + chaos_failures;
            if chaos_total == 0 {
                // Had events but zero total — treat as disappeared
                let drop_pp = baseline.success_rate;
                if drop_pp > self.max_drop_pp {
                    any_exceeded = true;
                }
                probes_evaluated += 1;
                details.push(format!(
                    "❌ {}: 0 probe events during chaos (baseline {:.1}% → chaos 0.0%)",
                    probe_name, baseline.success_rate
                ));
                if drop_pp > worst_drop {
                    worst_drop = drop_pp;
                    worst_probe = probe_name.clone();
                }
                continue;
            }

            let chaos_rate = (chaos_successes as f64 / chaos_total as f64) * 100.0;
            let drop_pp = baseline.success_rate - chaos_rate;
            probes_evaluated += 1;

            let status = if drop_pp > self.max_drop_pp {
                any_exceeded = true;
                "❌"
            } else {
                "✅"
            };

            details.push(format!(
                "{} {}: avail dropped {:.1}pp (baseline {:.1}% → chaos {:.1}%)",
                status,
                probe_name,
                drop_pp.max(0.0),
                baseline.success_rate,
                chaos_rate
            ));

            if drop_pp > worst_drop {
                worst_drop = drop_pp;
                worst_probe = probe_name.clone();
            }
        }

        if probes_evaluated == 0 {
            let expected = format!("availability drop ≤ {:.1}pp", self.max_drop_pp);
            return PropertyVerdict::fail(
                "baseline-availability",
                expected,
                "no probes evaluated".to_string(),
            )
            .with_details(details);
        }

        let expected = format!("availability drop ≤ {:.1}pp", self.max_drop_pp);
        let actual = if worst_probe.is_empty() {
            "no availability degradation detected".to_string()
        } else {
            format!(
                "worst: {:.1}pp drop on {}",
                worst_drop.max(0.0),
                worst_probe
            )
        };

        if any_exceeded {
            PropertyVerdict::fail("baseline-availability", expected, actual).with_details(details)
        } else {
            PropertyVerdict::pass("baseline-availability", expected, actual).with_details(details)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heisensim_timeline::event::{EventKind, TimelineEvent};
    use std::time::Duration;
    use uuid::Uuid;

    fn make_event(elapsed_ms: u64, kind: EventKind) -> TimelineEvent {
        TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            elapsed: Duration::from_millis(elapsed_ms),
            kind,
        }
    }

    fn probe_success(elapsed_ms: u64, name: &str, latency_ms: u64) -> TimelineEvent {
        make_event(
            elapsed_ms,
            EventKind::ProbeSuccess {
                probe_name: name.to_string(),
                latency_ms,
                status_code: Some(200),
            },
        )
    }

    fn probe_failed(elapsed_ms: u64, name: &str, latency_ms: u64) -> TimelineEvent {
        make_event(
            elapsed_ms,
            EventKind::ProbeFailed {
                probe_name: name.to_string(),
                error: "connection refused".to_string(),
                latency_ms: Some(latency_ms),
            },
        )
    }

    fn probe_timeout(elapsed_ms: u64, name: &str, timeout_ms: u64) -> TimelineEvent {
        make_event(
            elapsed_ms,
            EventKind::ProbeTimeout {
                probe_name: name.to_string(),
                timeout_ms,
            },
        )
    }

    #[test]
    fn test_capture_baseline_empty() {
        let events: Vec<TimelineEvent> = vec![];
        assert!(capture_baseline(&events, Duration::from_secs(10)).is_none());
    }

    #[test]
    fn test_capture_baseline_basic() {
        let events = vec![
            probe_success(1000, "http-check", 100),
            probe_success(2000, "http-check", 120),
            probe_success(3000, "http-check", 110),
            probe_success(4000, "http-check", 130),
            probe_success(5000, "http-check", 105),
            // These are after baseline window
            probe_success(11000, "http-check", 500),
        ];

        let baseline = capture_baseline(&events, Duration::from_secs(10)).unwrap();
        assert_eq!(baseline.probes.len(), 1);
        assert_eq!(baseline.total_samples, 5);

        let probe = &baseline.probes["http-check"];
        assert_eq!(probe.success_count, 5);
        assert_eq!(probe.failure_count, 0);
        assert!((probe.success_rate - 100.0).abs() < 0.1);
        assert_eq!(probe.p50_ms, 110);
        assert_eq!(probe.p95_ms, 130);
    }

    #[test]
    fn test_capture_baseline_with_failures() {
        let events = vec![
            probe_success(1000, "api", 50),
            probe_success(2000, "api", 60),
            probe_failed(3000, "api", 100),
            probe_success(4000, "api", 55),
            probe_timeout(5000, "api", 5000),
        ];

        let baseline = capture_baseline(&events, Duration::from_secs(10)).unwrap();
        let probe = &baseline.probes["api"];
        assert_eq!(probe.success_count, 3);
        assert_eq!(probe.failure_count, 2);
        assert!((probe.success_rate - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_baseline_latency_diff_pass() {
        let events = vec![
            // Baseline (0-10s): latency ~100ms
            probe_success(1000, "svc", 100),
            probe_success(2000, "svc", 110),
            probe_success(3000, "svc", 105),
            probe_success(4000, "svc", 115),
            probe_success(5000, "svc", 95),
            // Chaos (10s+): latency ~200ms (2x, within 3x threshold)
            probe_success(11000, "svc", 200),
            probe_success(12000, "svc", 210),
            probe_success(13000, "svc", 195),
            probe_success(14000, "svc", 205),
            probe_success(15000, "svc", 220),
        ];

        let baseline = capture_baseline(&events, Duration::from_secs(10)).unwrap();
        let prop = BaselineLatencyDiff::new(baseline, Duration::from_secs(10), 3.0);
        let verdict = prop.evaluate(&events);

        assert!(verdict.passed);
        assert!(!verdict.details.is_empty());
    }

    #[test]
    fn test_baseline_latency_diff_fail() {
        let events = vec![
            // Baseline (0-10s): latency ~100ms
            probe_success(1000, "svc", 100),
            probe_success(2000, "svc", 110),
            probe_success(3000, "svc", 105),
            probe_success(4000, "svc", 115),
            probe_success(5000, "svc", 95),
            // Chaos (10s+): latency ~500ms (5x, exceeds 3x threshold)
            probe_success(11000, "svc", 500),
            probe_success(12000, "svc", 510),
            probe_success(13000, "svc", 495),
            probe_success(14000, "svc", 520),
            probe_success(15000, "svc", 480),
        ];

        let baseline = capture_baseline(&events, Duration::from_secs(10)).unwrap();
        let prop = BaselineLatencyDiff::new(baseline, Duration::from_secs(10), 3.0);
        let verdict = prop.evaluate(&events);

        assert!(!verdict.passed);
    }

    #[test]
    fn test_baseline_availability_diff_pass() {
        let events = vec![
            // Baseline: 100% availability
            probe_success(1000, "api", 50),
            probe_success(2000, "api", 55),
            probe_success(3000, "api", 60),
            probe_success(4000, "api", 52),
            probe_success(5000, "api", 58),
            probe_success(6000, "api", 53),
            probe_success(7000, "api", 57),
            probe_success(8000, "api", 51),
            probe_success(9000, "api", 56),
            probe_success(9500, "api", 54),
            // Chaos: 90% availability (1 failure out of 10 = 10pp drop, within threshold)
            probe_success(11000, "api", 100),
            probe_success(12000, "api", 110),
            probe_success(13000, "api", 105),
            probe_success(14000, "api", 115),
            probe_success(15000, "api", 95),
            probe_success(16000, "api", 100),
            probe_success(17000, "api", 110),
            probe_success(18000, "api", 105),
            probe_success(19000, "api", 115),
            probe_failed(20000, "api", 500),
        ];

        let baseline = capture_baseline(&events, Duration::from_secs(10)).unwrap();
        let prop = BaselineAvailabilityDiff::new(baseline, Duration::from_secs(10), 10.0);
        let verdict = prop.evaluate(&events);

        assert!(verdict.passed);
    }

    #[test]
    fn test_baseline_availability_diff_fail() {
        let events = vec![
            // Baseline: 100% availability
            probe_success(1000, "api", 50),
            probe_success(2000, "api", 55),
            probe_success(3000, "api", 60),
            probe_success(4000, "api", 52),
            probe_success(5000, "api", 58),
            // Chaos: 60% availability (2 failures out of 5 = 40pp drop, exceeds 10pp)
            probe_success(11000, "api", 100),
            probe_success(12000, "api", 110),
            probe_success(13000, "api", 105),
            probe_failed(14000, "api", 500),
            probe_failed(15000, "api", 600),
        ];

        let baseline = capture_baseline(&events, Duration::from_secs(10)).unwrap();
        let prop = BaselineAvailabilityDiff::new(baseline, Duration::from_secs(10), 10.0);
        let verdict = prop.evaluate(&events);

        assert!(!verdict.passed);
    }

    #[test]
    fn test_smart_baseline_duration_default() {
        let intervals = vec![Duration::from_secs(1)];
        assert_eq!(smart_baseline_duration(&intervals), Duration::from_secs(10));
    }

    #[test]
    fn test_smart_baseline_duration_slow_probe() {
        let intervals = vec![Duration::from_secs(5), Duration::from_secs(2)];
        assert_eq!(smart_baseline_duration(&intervals), Duration::from_secs(50));
    }

    #[test]
    fn test_smart_baseline_duration_empty() {
        let intervals: Vec<Duration> = vec![];
        assert_eq!(smart_baseline_duration(&intervals), Duration::from_secs(10));
    }

    #[test]
    fn test_percentile_edge_cases() {
        assert_eq!(percentile(&[], 95), 0);
        assert_eq!(percentile(&[42], 50), 42);
        assert_eq!(percentile(&[42], 95), 42);
        assert_eq!(percentile(&[10, 20, 30], 50), 20);
    }

    #[test]
    fn test_multiple_probes_baseline() {
        let events = vec![
            probe_success(1000, "http", 100),
            probe_success(1500, "grpc", 50),
            probe_success(2000, "http", 110),
            probe_success(2500, "grpc", 55),
            probe_success(3000, "http", 105),
            probe_success(3500, "grpc", 52),
        ];

        let baseline = capture_baseline(&events, Duration::from_secs(10)).unwrap();
        assert_eq!(baseline.probes.len(), 2);
        assert!(baseline.probes.contains_key("http"));
        assert!(baseline.probes.contains_key("grpc"));
        assert_eq!(baseline.total_samples, 6);
    }
}
