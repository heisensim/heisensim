//! Property checking integration for the CLI.
//!
//! Parses `[[properties]]` from TOML config and builds a `TimelineChecker`.

use anyhow::{Context, Result};
use heisensim_props::{
    Availability, ErrorBudget, LatencyThreshold, NoCascade, PropertyVerdict, RecoveryTime,
    TimelineChecker, TimelineProperty,
};
use heisensim_timeline::event::TimelineEvent;
use serde::Deserialize;

/// A property definition from TOML config.
#[derive(Debug, Clone, Deserialize)]
pub struct PropertyDef {
    pub name: String,
    #[serde(rename = "type")]
    pub prop_type: String,
    // Recovery time
    pub max_seconds: Option<f64>,
    pub max_window_seconds: Option<f64>,
    // Availability
    pub min_percent: Option<f64>,
    // Error budget
    pub max_consecutive: Option<u32>,
    // Cascade
    pub window_seconds: Option<f64>,
    pub allowed_failing_probes: Option<Vec<String>>,
    // Latency
    pub max_ms: Option<u64>,
    pub percentile: Option<f64>,
    // Common
    pub probe_filter: Option<String>,
}

/// The `[[properties]]` section wrapper.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PropertiesConfig {
    #[serde(default)]
    pub properties: Vec<PropertyDef>,
}

/// Build a TimelineChecker from parsed property definitions.
pub fn build_checker(defs: &[PropertyDef]) -> Result<TimelineChecker> {
    let mut checker = TimelineChecker::new();

    for def in defs {
        let prop: Box<dyn TimelineProperty> = match def.prop_type.as_str() {
            "recovery_time" => {
                let max = def
                    .max_seconds
                    .context("recovery_time requires 'max_seconds'")?;
                let mut prop = RecoveryTime::new(&def.name, max);
                if let Some(window) = def.max_window_seconds {
                    prop = prop.with_max_window(window);
                }
                Box::new(prop)
            }
            "availability" => {
                let min = def
                    .min_percent
                    .context("availability requires 'min_percent'")?;
                let mut prop = Availability::new(&def.name, min);
                if let Some(ref filter) = def.probe_filter {
                    prop = prop.with_probe_filter(filter);
                }
                Box::new(prop)
            }
            "error_budget" => {
                let max = def
                    .max_consecutive
                    .context("error_budget requires 'max_consecutive'")?;
                Box::new(ErrorBudget::new(&def.name, max))
            }
            "no_cascade" => {
                let window = def.window_seconds.unwrap_or(30.0);
                let allowed = def.allowed_failing_probes.clone().unwrap_or_default();
                Box::new(NoCascade::new(&def.name, window, allowed))
            }
            "latency_p99" | "latency" => {
                let max = def.max_ms.context("latency requires 'max_ms'")?;
                let mut prop = LatencyThreshold::new(&def.name, max);
                if let Some(pct) = def.percentile {
                    prop = prop.with_percentile(pct);
                }
                if let Some(ref filter) = def.probe_filter {
                    prop = prop.with_probe_filter(filter);
                }
                Box::new(prop)
            }
            other => anyhow::bail!(
                "Unknown property type: '{}'. Supported: recovery_time, availability, error_budget, no_cascade, latency_p99",
                other
            ),
        };
        checker.add(prop);
    }

    Ok(checker)
}

/// Print a pretty property verdict table to stdout.
pub fn print_verdicts(verdicts: &[PropertyVerdict]) {
    println!();
    println!("╔═══════════════════════════════════════════════════════════════╗");

    let passed = verdicts.iter().filter(|v| v.passed).count();
    let total = verdicts.len();
    println!(
        "║  PROPERTY RESULTS {:>39} ║",
        format!("{}/{} PASS", passed, total)
    );

    println!("╠═══════════════════════════════════════════════════════════════╣");

    for v in verdicts {
        let icon = if v.passed { "✅" } else { "❌" };
        let status = if v.passed { "PASS" } else { "FAIL" };
        // Truncate for display
        let name = if v.property_name.len() > 18 {
            &v.property_name[..18]
        } else {
            &v.property_name
        };
        let detail = format!("{} (actual: {})", v.expected, v.actual);
        let detail = if detail.len() > 30 {
            format!("{}…", &detail[..29])
        } else {
            detail
        };
        println!("║  {} {}  {:<18} {:<30} ║", icon, status, name, detail);
    }

    println!("╚═══════════════════════════════════════════════════════════════╝");

    // Print details for failures
    for v in verdicts
        .iter()
        .filter(|v| !v.passed && !v.details.is_empty())
    {
        println!();
        println!("  {} details:", v.property_name);
        for detail in &v.details {
            println!("{}", detail);
        }
    }
    println!();
}

/// Run property checking against timeline events and print results.
/// Returns true if all properties passed.
pub fn check_properties(defs: &[PropertyDef], events: &[TimelineEvent]) -> Result<bool> {
    if defs.is_empty() {
        return Ok(true);
    }

    let checker = build_checker(defs)?;
    let verdicts = checker.evaluate_all(events);
    print_verdicts(&verdicts);

    Ok(verdicts.iter().all(|v| v.passed))
}
