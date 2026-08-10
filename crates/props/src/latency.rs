//! Latency threshold property — asserts probe latency percentile stays under a threshold.
//!
//! Supports optional `probe_filter` to scope to specific probes.

use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};

/// Asserts that probe latency at a given percentile stays under `max_ms`.
pub struct LatencyThreshold {
    name: String,
    max_ms: u64,
    percentile: f64,
    probe_filter: Option<String>,
}

impl LatencyThreshold {
    /// Create a new latency threshold property (defaults to p99).
    pub fn new(name: impl Into<String>, max_ms: u64) -> Self {
        Self {
            name: name.into(),
            max_ms,
            percentile: 99.0,
            probe_filter: None,
        }
    }

    /// Set the percentile to check (e.g. 95.0 for p95, 99.0 for p99).
    pub fn with_percentile(mut self, percentile: f64) -> Self {
        self.percentile = percentile;
        self
    }

    /// Only evaluate probes whose name contains this substring.
    pub fn with_probe_filter(mut self, filter: impl Into<String>) -> Self {
        self.probe_filter = Some(filter.into());
        self
    }

    fn matches_filter(&self, probe_name: &str) -> bool {
        match &self.probe_filter {
            Some(filter) => probe_name.contains(filter.as_str()),
            None => true,
        }
    }
}

impl TimelineProperty for LatencyThreshold {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts probe latency percentile stays under a threshold"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let mut latencies: Vec<u64> = Vec::new();

        for event in events {
            if let EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                ..
            } = &event.kind
            {
                if self.matches_filter(probe_name) {
                    latencies.push(*latency_ms);
                }
            }
        }

        if latencies.is_empty() {
            return PropertyVerdict::pass(
                &self.name,
                format!("p{} < {}ms", self.percentile, self.max_ms),
                "no data",
            );
        }

        latencies.sort_unstable();

        // Nearest-rank percentile: index = ceil(P/100 * N) - 1
        // For p99 with 100 samples, this gives index 98 (the 99th value)
        let rank = (self.percentile / 100.0 * latencies.len() as f64).ceil() as usize;
        let index = rank.saturating_sub(1).min(latencies.len() - 1);
        let actual_ms = latencies[index];

        let expected = format!("p{} < {}ms", self.percentile, self.max_ms);
        let actual = format!("{}ms (n={})", actual_ms, latencies.len());

        if actual_ms <= self.max_ms {
            PropertyVerdict::pass(&self.name, expected, actual)
        } else {
            let mut details = vec![
                format!("  p50: {}ms", latencies[latencies.len() / 2]),
                format!("  p{}: {}ms", self.percentile, actual_ms),
                format!("  max: {}ms", latencies.last().unwrap()),
                format!("  samples: {}", latencies.len()),
            ];
            if let Some(ref filter) = self.probe_filter {
                details.push(format!("  probe filter: \"{}\"", filter));
            }
            PropertyVerdict::fail(&self.name, expected, actual).with_details(details)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::time::Duration;
    use uuid::Uuid;

    fn probe_event(elapsed_secs: u64, name: &str, latency_ms: u64) -> TimelineEvent {
        TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs(elapsed_secs),
            kind: EventKind::ProbeSuccess {
                probe_name: name.into(),
                latency_ms,
                status_code: Some(200),
            },
        }
    }

    #[test]
    fn test_latency_pass() {
        let events: Vec<_> = (0..100)
            .map(|i| probe_event(i, "api", 10 + (i % 20)))
            .collect();

        let prop = LatencyThreshold::new("low-lat", 500);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Expected pass: {:?}", verdict);
    }

    #[test]
    fn test_latency_fail() {
        let mut events: Vec<_> = (0..98).map(|i| probe_event(i, "api", 10)).collect();
        // Two outliers so p99 is definitely above threshold
        events.push(probe_event(98, "api", 1000));
        events.push(probe_event(99, "api", 1000));

        let prop = LatencyThreshold::new("low-lat", 500);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "Expected fail: {:?}", verdict);
    }

    #[test]
    fn test_latency_with_filter() {
        let events = vec![
            probe_event(1, "api-health", 10),
            probe_event(2, "api-health", 20),
            probe_event(3, "redis-health", 999), // high latency but filtered out
        ];

        let prop = LatencyThreshold::new("api-lat", 500).with_probe_filter("api");
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Filtered should pass: {:?}", verdict);
    }

    #[test]
    fn test_latency_empty() {
        let prop = LatencyThreshold::new("low-lat", 500);
        let verdict = prop.evaluate(&[]);
        assert!(verdict.passed);
    }

    #[test]
    fn test_latency_p95() {
        let mut events: Vec<_> = (0..90).map(|i| probe_event(i, "api", 10)).collect();
        // 10 high-latency events (last 10%)
        for i in 90..100 {
            events.push(probe_event(i, "api", 600));
        }

        // p95 is in the high-latency region → should fail
        let prop = LatencyThreshold::new("lat-p95", 500).with_percentile(95.0);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "p95 should fail: {:?}", verdict);

        // p85 is in the low-latency region → should pass
        let prop85 = LatencyThreshold::new("lat-p85", 500).with_percentile(85.0);
        let verdict85 = prop85.evaluate(&events);
        assert!(verdict85.passed, "p85 should pass: {:?}", verdict85);
    }
}
