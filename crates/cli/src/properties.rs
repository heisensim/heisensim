//! Property checking integration for the CLI.
//!
//! Parses `[[properties]]` from TOML config and builds a `TimelineChecker`.
//! Supports pre-built property templates for common SLA tiers.

use anyhow::{Context, Result};
use clap::ValueEnum;
use heisensim_props::{
    Availability, DnsResolution, ErrorBudget, LatencyThreshold, NoCascade, PropertyVerdict,
    RecoveryTime, SteadyState, Throughput, TimelineChecker, TimelineProperty,
};
use serde::Deserialize;

/// Pre-built SLA property bundles for common workload types.
#[derive(Clone, Debug, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PropertyTemplate {
    /// 99% availability, 30s recovery, 500ms p99
    Basic,
    /// 99.9% availability, 15s recovery, 250ms p99, error budget, no cascade
    ThreeNines,
    /// 99.99% availability, 5s recovery, 100ms p99.9, strict error budget, no cascade
    FourNines,
    /// 95% availability, 10s recovery — fast CI smoke test
    Ci,
    /// 99.5% availability, 30s recovery, 500ms p99, no cascade, DNS, steady-state
    Microservice,
    /// 99% availability, 60s recovery, error budget, steady-state, throughput
    Stateful,
}

impl PropertyTemplate {
    /// Convert this template into a set of property definitions.
    pub fn to_property_defs(&self) -> Vec<PropertyDef> {
        match self {
            PropertyTemplate::Basic => vec![
                PropertyDef::availability("high-availability", 99.0),
                PropertyDef::recovery_time("fast-recovery", 30.0),
                PropertyDef::latency("p99-latency", 500, 99.0),
            ],
            PropertyTemplate::ThreeNines => vec![
                PropertyDef::availability("high-availability", 99.9),
                PropertyDef::recovery_time("fast-recovery", 15.0),
                PropertyDef::latency("p99-latency", 250, 99.0),
                PropertyDef::error_budget("error-budget", 3),
                PropertyDef::no_cascade("no-cascade", 30.0),
            ],
            PropertyTemplate::FourNines => vec![
                PropertyDef::availability("high-availability", 99.99),
                PropertyDef::recovery_time("fast-recovery", 5.0),
                PropertyDef::latency("p999-latency", 100, 99.9),
                PropertyDef::error_budget("error-budget", 1),
                PropertyDef::no_cascade("no-cascade", 15.0),
            ],
            PropertyTemplate::Ci => vec![
                PropertyDef::availability("ci-availability", 95.0),
                PropertyDef::recovery_time("ci-recovery", 10.0),
            ],
            PropertyTemplate::Microservice => vec![
                PropertyDef::availability("high-availability", 99.5),
                PropertyDef::recovery_time("fast-recovery", 30.0),
                PropertyDef::latency("p99-latency", 500, 99.0),
                PropertyDef::no_cascade("no-cascade", 30.0),
                PropertyDef::dns_resolution("dns-resolution", 10.0),
                PropertyDef::steady_state("steady-state", 30.0, 15.0),
            ],
            PropertyTemplate::Stateful => vec![
                PropertyDef::availability("high-availability", 99.0),
                PropertyDef::recovery_time("recovery", 60.0),
                PropertyDef::error_budget("error-budget", 5),
                PropertyDef::steady_state("steady-state", 60.0, 30.0),
                PropertyDef::throughput("throughput", 10.0, 60.0),
            ],
        }
    }
}

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
    // Steady state
    pub max_recovery_seconds: Option<f64>,
    pub baseline_seconds: Option<f64>,
    // Throughput
    pub min_per_minute: Option<f64>,
    // Cascade
    pub window_seconds: Option<f64>,
    pub allowed_failing_probes: Option<Vec<String>>,
    // Latency
    pub max_ms: Option<u64>,
    pub percentile: Option<f64>,
    // Common
    pub probe_filter: Option<String>,
}

impl PropertyDef {
    /// Helper: create an availability property definition.
    pub fn availability(name: &str, min_percent: f64) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "availability".to_string(),
            min_percent: Some(min_percent),
            ..Self::empty()
        }
    }

    /// Helper: create a recovery time property definition.
    pub fn recovery_time(name: &str, max_seconds: f64) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "recovery_time".to_string(),
            max_seconds: Some(max_seconds),
            ..Self::empty()
        }
    }

    /// Helper: create a latency property definition.
    pub fn latency(name: &str, max_ms: u64, percentile: f64) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "latency".to_string(),
            max_ms: Some(max_ms),
            percentile: Some(percentile),
            ..Self::empty()
        }
    }

    /// Helper: create an error budget property definition.
    pub fn error_budget(name: &str, max_consecutive: u32) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "error_budget".to_string(),
            max_consecutive: Some(max_consecutive),
            ..Self::empty()
        }
    }

    /// Helper: create a no-cascade property definition.
    pub fn no_cascade(name: &str, window_seconds: f64) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "no_cascade".to_string(),
            window_seconds: Some(window_seconds),
            ..Self::empty()
        }
    }

    /// Helper: create a DNS resolution property definition.
    pub fn dns_resolution(name: &str, max_recovery_seconds: f64) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "dns-resolution".to_string(),
            max_recovery_seconds: Some(max_recovery_seconds),
            ..Self::empty()
        }
    }

    /// Helper: create a steady-state property definition.
    pub fn steady_state(name: &str, max_recovery_seconds: f64, baseline_seconds: f64) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "steady-state".to_string(),
            max_recovery_seconds: Some(max_recovery_seconds),
            baseline_seconds: Some(baseline_seconds),
            ..Self::empty()
        }
    }

    /// Helper: create a throughput property definition.
    pub fn throughput(name: &str, min_per_minute: f64, window_seconds: f64) -> Self {
        Self {
            name: name.to_string(),
            prop_type: "throughput".to_string(),
            min_per_minute: Some(min_per_minute),
            window_seconds: Some(window_seconds),
            ..Self::empty()
        }
    }

    fn empty() -> Self {
        Self {
            name: String::new(),
            prop_type: String::new(),
            max_seconds: None,
            max_window_seconds: None,
            min_percent: None,
            max_consecutive: None,
            max_recovery_seconds: None,
            baseline_seconds: None,
            min_per_minute: None,
            window_seconds: None,
            allowed_failing_probes: None,
            max_ms: None,
            percentile: None,
            probe_filter: None,
        }
    }
}

/// The `[[properties]]` section wrapper, with optional template.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PropertiesConfig {
    /// Pre-built SLA template (e.g. "three-nines", "microservice")
    pub template: Option<PropertyTemplate>,
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
            "throughput" => {
                let min = def
                    .min_per_minute
                    .context("throughput requires 'min_per_minute'")?;
                let window = def
                    .window_seconds
                    .context("throughput requires 'window_seconds'")?;
                Box::new(Throughput::new(&def.name, min, window))
            }
            "steady-state" => {
                let max_rec = def
                    .max_recovery_seconds
                    .context("steady-state requires 'max_recovery_seconds'")?;
                let base = def
                    .baseline_seconds
                    .context("steady-state requires 'baseline_seconds'")?;
                Box::new(SteadyState::new(&def.name, max_rec, base))
            }
            "dns-resolution" => {
                let max_rec = def
                    .max_recovery_seconds
                    .context("dns-resolution requires 'max_recovery_seconds'")?;
                Box::new(DnsResolution::new(&def.name, max_rec))
            }
            other => anyhow::bail!(
                "Unknown property type: '{}'. Supported: recovery_time, availability, error_budget, no_cascade, latency_p99, latency, throughput, steady-state, dns-resolution",
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

/// Evaluates a list of properties against a timeline of events.
pub fn evaluate_properties(
    events: &[heisensim_timeline::event::TimelineEvent],
    defs: &[PropertyDef],
) -> Vec<PropertyVerdict> {
    if let Ok(checker) = build_checker(defs) {
        checker.evaluate_all(events)
    } else {
        Vec::new()
    }
}

/// Load and validate property definitions from a config file.
///
/// Reads the TOML file, parses `[[properties]]` and optional `template`,
/// and validates all definitions by building a checker. Explicit `[[properties]]`
/// override template properties with the same name.
pub fn load_and_validate(config_path: &std::path::Path) -> Result<Vec<PropertyDef>> {
    let config_str = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;
    let config: PropertiesConfig = toml::from_str(&config_str)
        .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;

    let defs = merge_template_and_explicit(config.template.as_ref(), &config.properties);

    // Validate all property definitions eagerly
    if !defs.is_empty() {
        build_checker(&defs).with_context(|| {
            format!("Invalid property in config file: {}", config_path.display())
        })?;
    }

    Ok(defs)
}

/// Resolve property definitions from a CLI template flag and/or config file.
///
/// CLI `--property-template` overrides config file `template`.
/// Explicit `[[properties]]` override template properties by name.
pub fn resolve_with_template(
    cli_template: Option<&PropertyTemplate>,
    config_defs: &[PropertyDef],
    config_template: Option<&PropertyTemplate>,
) -> Vec<PropertyDef> {
    // CLI flag takes precedence over config template
    let template = cli_template.or(config_template);
    merge_template_and_explicit(template, config_defs)
}

/// Merge template defaults with explicit property definitions.
/// Explicit defs override template defs with the same name.
fn merge_template_and_explicit(
    template: Option<&PropertyTemplate>,
    explicit: &[PropertyDef],
) -> Vec<PropertyDef> {
    let mut defs = if let Some(tmpl) = template {
        tmpl.to_property_defs()
    } else {
        Vec::new()
    };

    // Explicit properties override template properties with the same name
    let explicit_names: std::collections::HashSet<&str> =
        explicit.iter().map(|d| d.name.as_str()).collect();
    defs.retain(|d| !explicit_names.contains(d.name.as_str()));
    defs.extend(explicit.iter().cloned());

    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_template_builds_valid_checker() {
        let defs = PropertyTemplate::Basic.to_property_defs();
        assert_eq!(defs.len(), 3);
        let checker = build_checker(&defs).unwrap();
        assert_eq!(checker.len(), 3);
    }

    #[test]
    fn test_three_nines_template_builds_valid_checker() {
        let defs = PropertyTemplate::ThreeNines.to_property_defs();
        assert_eq!(defs.len(), 5);
        let checker = build_checker(&defs).unwrap();
        assert_eq!(checker.len(), 5);
    }

    #[test]
    fn test_four_nines_template_builds_valid_checker() {
        let defs = PropertyTemplate::FourNines.to_property_defs();
        assert_eq!(defs.len(), 5);
        build_checker(&defs).unwrap();
    }

    #[test]
    fn test_ci_template_builds_valid_checker() {
        let defs = PropertyTemplate::Ci.to_property_defs();
        assert_eq!(defs.len(), 2);
        build_checker(&defs).unwrap();
    }

    #[test]
    fn test_microservice_template_builds_valid_checker() {
        let defs = PropertyTemplate::Microservice.to_property_defs();
        assert_eq!(defs.len(), 6);
        build_checker(&defs).unwrap();
    }

    #[test]
    fn test_stateful_template_builds_valid_checker() {
        let defs = PropertyTemplate::Stateful.to_property_defs();
        assert_eq!(defs.len(), 5);
        build_checker(&defs).unwrap();
    }

    #[test]
    fn test_explicit_overrides_template_by_name() {
        let explicit = vec![PropertyDef::availability("high-availability", 99.99)];
        let defs = merge_template_and_explicit(Some(&PropertyTemplate::Basic), &explicit);
        // Basic has 3 (availability, recovery, latency), but "high-availability" is overridden
        assert_eq!(defs.len(), 3);
        let avail = defs.iter().find(|d| d.name == "high-availability").unwrap();
        assert_eq!(avail.min_percent, Some(99.99));
    }

    #[test]
    fn test_explicit_adds_to_template() {
        let explicit = vec![PropertyDef::throughput("custom-throughput", 50.0, 30.0)];
        let defs = merge_template_and_explicit(Some(&PropertyTemplate::Basic), &explicit);
        // Basic has 3 + 1 custom = 4
        assert_eq!(defs.len(), 4);
    }

    #[test]
    fn test_no_template_returns_explicit_only() {
        let explicit = vec![PropertyDef::availability("my-avail", 99.0)];
        let defs = merge_template_and_explicit(None, &explicit);
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn test_resolve_cli_overrides_config_template() {
        let config_defs = Vec::new();
        let defs = resolve_with_template(
            Some(&PropertyTemplate::FourNines),
            &config_defs,
            Some(&PropertyTemplate::Ci),
        );
        // CLI FourNines (5 properties) should override config Ci (2 properties)
        assert_eq!(defs.len(), 5);
    }

    #[test]
    fn test_all_templates_produce_valid_checkers() {
        let templates = [
            PropertyTemplate::Basic,
            PropertyTemplate::ThreeNines,
            PropertyTemplate::FourNines,
            PropertyTemplate::Ci,
            PropertyTemplate::Microservice,
            PropertyTemplate::Stateful,
        ];
        for tmpl in &templates {
            let defs = tmpl.to_property_defs();
            build_checker(&defs).unwrap_or_else(|e| {
                panic!("Template {:?} failed to build checker: {}", tmpl, e);
            });
        }
    }
}
