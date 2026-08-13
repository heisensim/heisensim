use crate::timeline::{PropertyVerdict, TimelineProperty};
use heisensim_timeline::event::{EventKind, TimelineEvent};
use std::collections::HashSet;
use std::time::Duration;

/// Verifies DNS queries resume within `max_recovery_seconds` after a DNS fault is reverted.
pub struct DnsResolution {
    name: String,
    max_recovery_seconds: f64,
}

impl DnsResolution {
    pub fn new(name: impl Into<String>, max_recovery_seconds: f64) -> Self {
        Self {
            name: name.into(),
            max_recovery_seconds,
        }
    }
}

impl TimelineProperty for DnsResolution {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Asserts DNS queries resume quickly after a DNS fault is reverted"
    }

    fn evaluate(&self, events: &[TimelineEvent]) -> PropertyVerdict {
        let max_recovery_dur = Duration::from_secs_f64(self.max_recovery_seconds);
        let mut details = Vec::new();
        let mut any_failed = false;

        // Find all DNS faults
        let mut dns_fault_ids = HashSet::new();
        for e in events {
            if let EventKind::FaultInjected {
                fault_id,
                fault_kind,
                ..
            } = &e.kind
            {
                if fault_kind.to_lowercase().contains("dns") {
                    dns_fault_ids.insert(*fault_id);
                }
            }
        }

        if dns_fault_ids.is_empty() {
            return PropertyVerdict::pass(&self.name, "DNS fault recovery", "no DNS faults")
                .with_details(vec!["  ✅ no DNS faults injected".to_string()]);
        }

        let mut worst_recovery: Option<Duration> = None;

        for (i, event) in events.iter().enumerate() {
            if let EventKind::FaultReverted { fault_id } = &event.kind {
                if !dns_fault_ids.contains(fault_id) {
                    continue; // Skip non-DNS faults
                }

                let revert_time = event.elapsed;

                // Find first success after revert
                let recovery_event = events[i + 1..]
                    .iter()
                    .find(|e| matches!(e.kind, EventKind::ProbeSuccess { .. }));

                match recovery_event {
                    Some(success) => {
                        let recovery_time = success.elapsed.saturating_sub(revert_time);
                        if recovery_time > max_recovery_dur {
                            any_failed = true;
                            details.push(format!(
                                "  ❌ DNS fault {}: recovered in {:.1}s (exceeds {:.0}s)",
                                fault_id,
                                recovery_time.as_secs_f64(),
                                self.max_recovery_seconds
                            ));
                        } else {
                            details.push(format!(
                                "  ✅ DNS fault {}: recovered in {:.1}s",
                                fault_id,
                                recovery_time.as_secs_f64()
                            ));
                        }

                        worst_recovery = Some(match worst_recovery {
                            Some(w) => w.max(recovery_time),
                            None => recovery_time,
                        });
                    }
                    None => {
                        any_failed = true;
                        details.push(format!("  ❌ DNS fault {}: never recovered", fault_id));
                    }
                }
            }
        }

        let expected = format!("recovery < {:.0}s", self.max_recovery_seconds);
        let actual = match worst_recovery {
            Some(d) => format!("{:.1}s", d.as_secs_f64()),
            None => "never recovered".to_string(),
        };

        if any_failed {
            PropertyVerdict::fail(&self.name, expected, actual).with_details(details)
        } else {
            PropertyVerdict::pass(&self.name, expected, actual).with_details(details)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn event(elapsed_secs: f64, kind: EventKind) -> TimelineEvent {
        TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs_f64(elapsed_secs),
            kind,
        }
    }

    #[test]
    fn test_dns_pass() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                10.0,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "dns-block".into(),
                    target: "redis".into(),
                    duration_secs: None,
                },
            ),
            event(20.0, EventKind::FaultReverted { fault_id: fid }),
            event(
                25.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
        ];

        let prop = DnsResolution::new("dns", 10.0);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed, "Expected pass, got {:?}", verdict);
    }

    #[test]
    fn test_dns_fail() {
        let fid = Uuid::new_v4();
        let events = vec![
            event(
                10.0,
                EventKind::FaultInjected {
                    fault_id: fid,
                    fault_kind: "dns_failure".into(),
                    target: "redis".into(),
                    duration_secs: None,
                },
            ),
            event(20.0, EventKind::FaultReverted { fault_id: fid }),
            event(
                35.0,
                EventKind::ProbeSuccess {
                    probe_name: "p".into(),
                    latency_ms: 1,
                    status_code: None,
                },
            ),
        ];

        let prop = DnsResolution::new("dns", 10.0);
        let verdict = prop.evaluate(&events);
        assert!(!verdict.passed, "Expected fail, got {:?}", verdict);
    }

    #[test]
    fn test_dns_no_events() {
        let events = vec![];
        let prop = DnsResolution::new("dns", 10.0);
        let verdict = prop.evaluate(&events);
        assert!(verdict.passed);
    }
}
