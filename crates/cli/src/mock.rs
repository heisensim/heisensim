use crate::{
    ExploreArgs, OutputFormat, RunArgs, metrics, parse_duration, properties, report, report_html,
};
use anyhow::{Context, Result};
use chrono::Utc;
use heisensim_timeline::{
    Timeline,
    event::{EventKind, TimelineEvent},
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

pub fn run_mock_simulation(
    seed: u64,
    duration: Duration,
    warmup: Duration,
    faults: &[String],
    num_pods: usize,
) -> Timeline {
    let timeline = Timeline::new();
    let mut rng = StdRng::seed_from_u64(seed);

    let pods: Vec<String> = (0..num_pods).map(|i| format!("mock-pod-{}", i)).collect();

    let emit_at = |kind: EventKind, elapsed: f64| {
        timeline.push_event(TimelineEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            elapsed: Duration::from_secs_f64(elapsed),
            kind,
        });
    };

    emit_at(
        EventKind::SimulationStarted {
            seed,
            duration_secs: duration.as_secs_f64(),
        },
        0.0,
    );

    let total_secs = duration.as_secs_f64();
    let warmup_secs = warmup.as_secs_f64();
    let mut elapsed = 0.0;

    while elapsed < warmup_secs {
        for pod in &pods {
            emit_at(
                EventKind::ProbeSuccess {
                    probe_name: pod.clone(),
                    latency_ms: rng.random_range(5..50),
                    status_code: Some(200),
                },
                elapsed,
            );
        }
        elapsed += 1.0;
    }

    let mut fault_times = Vec::new();
    if total_secs * 0.8 <= warmup_secs {
        warn!("warmup duration is >= 80% of total duration; no faults will be generated");
    } else {
        let fault_count = rng.random_range(2..=5);
        for _ in 0..fault_count {
            let fault_time = rng.random_range(warmup_secs..total_secs * 0.8);
            let fault_duration = rng.random_range(5.0..20.0);
            fault_times.push((fault_time, fault_duration));
        }
    }
    fault_times.sort_by(|a, b| a.0.total_cmp(&b.0));

    for (fault_time, fault_duration) in &fault_times {
        let target = &pods[rng.random_range(0..pods.len())];
        let fault_type = if faults.is_empty() {
            "crash".to_string()
        } else {
            faults[rng.random_range(0..faults.len())].clone()
        };
        let fault_id = Uuid::new_v4();

        emit_at(
            EventKind::FaultInjected {
                fault_id,
                fault_kind: fault_type.clone(),
                target: target.clone(),
                duration_secs: Some(*fault_duration),
            },
            *fault_time,
        );

        emit_at(
            EventKind::FaultReverted { fault_id },
            (fault_time + fault_duration).min(total_secs),
        );
    }

    elapsed = warmup_secs;
    while elapsed < total_secs {
        for pod in &pods {
            let under_fault = fault_times
                .iter()
                .any(|(ft, fd)| elapsed >= *ft && elapsed < ft + fd);

            if under_fault && rng.random_bool(0.3) {
                emit_at(
                    EventKind::ProbeFailed {
                        probe_name: pod.clone(),
                        error: "connection refused (mock)".to_string(),
                        latency_ms: Some(10),
                    },
                    elapsed,
                );
            } else {
                let latency = if under_fault {
                    rng.random_range(50..500)
                } else {
                    rng.random_range(5..50)
                };
                emit_at(
                    EventKind::ProbeSuccess {
                        probe_name: pod.clone(),
                        latency_ms: latency,
                        status_code: Some(200),
                    },
                    elapsed,
                );
            }
        }
        elapsed += 1.0;
    }

    let events = timeline.events();
    let summary = heisensim_timeline::query::summary(&events);

    emit_at(
        EventKind::SimulationEnded {
            total_faults: summary.total_faults,
            total_failures: summary.total_failures,
        },
        total_secs,
    );

    timeline
}

pub async fn handle_mock_run(
    args: &RunArgs,
    meter_provider: Option<&opentelemetry_sdk::metrics::SdkMeterProvider>,
) -> Result<i32> {
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    println!("Config Summary (MOCK MODE):");
    println!("  Namespace: {}", args.namespace);
    println!("  Duration: {}", args.duration);
    println!("  Seed: 0x{:04X}", seed);
    println!("  Warmup: {}", args.warmup);
    println!("  Faults: {:?}", args.faults);

    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;

    info!("Starting mock simulation...");
    let timeline = run_mock_simulation(seed, duration, warmup, &args.faults, 3);

    let final_events = timeline.events();
    info!("Simulation complete. Rendering report...");

    if args.output == OutputFormat::Text {
        report::render_terminal_report(&final_events);
    }

    let json = report::render_json_report(&final_events);
    let json_path = format!("heisensim-report-{:04x}.json", seed & 0xFFFF);
    std::fs::write(&json_path, &json)?;
    info!("Timeline saved to {}", json_path);

    let mut exit_code = 0;
    let mut verdicts = Vec::new();
    if let Some(ref config_path) = args.config {
        let config_str =
            std::fs::read_to_string(config_path).context("Failed to read config file")?;
        let config: properties::PropertiesConfig = toml::from_str(&config_str).unwrap_or_default();
        if !config.properties.is_empty() {
            info!("Evaluating {} properties...", config.properties.len());
            let checker = properties::build_checker(&config.properties)?;
            verdicts = checker.evaluate_all(&final_events);
            let all_passed = verdicts.iter().all(|v| v.passed);
            if args.output == OutputFormat::Text {
                properties::print_verdicts(&verdicts);
            }
            if !all_passed {
                warn!("Some properties FAILED. Exit code 1.");
                exit_code = 1;
            }
        }
    }

    if let Some(mp) = meter_provider {
        let summary = heisensim_timeline::query::summary(&final_events);
        metrics::emit_verdict_metrics(
            mp,
            &verdicts,
            seed,
            summary.duration.as_secs_f64(),
            summary.total_faults,
        );
    }

    if args.output == OutputFormat::Json {
        let summary = heisensim_timeline::query::summary(&final_events);
        let output = serde_json::json!({
            "seed": seed,
            "duration_secs": duration.as_secs_f64(),
            "total_faults": summary.total_faults,
            "total_failures": summary.total_failures,
            "properties": verdicts,
            "passed": exit_code == 0,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if args.output == OutputFormat::Junit {
        let junit = report::render_junit_report(&final_events, &verdicts);
        println!("{}", junit);
    } else if args.output == OutputFormat::Html {
        let html =
            report_html::render_html_report(&final_events, &verdicts, seed, duration.as_secs_f64());
        let html_path = "heisensim-report.html";
        if let Err(e) = std::fs::write(html_path, &html) {
            warn!("Failed to write HTML report: {}", e);
        } else {
            info!("HTML report saved to {}", html_path);
            println!(
                "{}",
                std::env::current_dir()
                    .unwrap_or_default()
                    .join(html_path)
                    .display()
            );
        }
    }

    Ok(exit_code)
}

pub async fn handle_mock_explore(args: &ExploreArgs) -> Result<i32> {
    anyhow::ensure!(args.parallel > 0, "--parallel must be at least 1");
    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;

    if args.bisect {
        warn!("--bisect is not performed in mock mode");
    }

    if args.output == OutputFormat::Text {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!(
            "║  HEISENSIM EXPLORE (MOCK)             {} seeds, {}s each   ║",
            args.seeds,
            duration.as_secs()
        );
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║  Namespace: {:16} │  Faults: {:20} ║",
            args.namespace,
            args.faults.join(",")
        );
        println!("╚══════════════════════════════════════════════════════════════╝");
        println!();
    }

    let mut results = Vec::new();
    let mut interesting_seeds: Vec<u64> = Vec::new();

    let property_defs: Vec<properties::PropertyDef> = if let Some(ref config_path) = args.config {
        let config_str =
            std::fs::read_to_string(config_path).context("Failed to read config file")?;
        let config: properties::PropertiesConfig = toml::from_str(&config_str).unwrap_or_default();
        config.properties
    } else {
        Vec::new()
    };

    if !property_defs.is_empty() {
        info!("Loaded {} properties from config.", property_defs.len());
    }

    use heisensim_fault::explorer::{ExploreStrategy, StrategicExplorer};

    let strategy = match args.explore_strategy {
        crate::ExploreStrategyArg::Random => ExploreStrategy::Random,
        crate::ExploreStrategyArg::Coverage => ExploreStrategy::Coverage,
        crate::ExploreStrategyArg::Sequential => ExploreStrategy::Sequential,
    };

    let mut explorer = StrategicExplorer::new(strategy, args.start_seed);
    let mut remaining = args.seeds;

    while remaining > 0 {
        let batch_size = std::cmp::min(remaining, args.parallel as u64) as usize;
        let mut batch = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            batch.push(explorer.next_seed());
        }
        remaining -= batch_size as u64;

        for &seed in &batch {
            info!(seed = seed, "🔬 Starting seed 0x{:04X}...", seed);

            let timeline = run_mock_simulation(seed, duration, warmup, &args.faults, 3);
            let events = timeline.events();
            let summary = heisensim_timeline::query::summary(&events);

            let mut findings = Vec::new();
            for event in &events {
                if let EventKind::FaultInjected {
                    fault_id,
                    fault_kind,
                    target,
                    ..
                } = &event.kind
                {
                    if let Some(latency) =
                        heisensim_timeline::query::fault_to_detection_latency(&events, *fault_id)
                    {
                        findings.push(format!(
                            "{} {} → detection in {:.1}s",
                            target,
                            fault_kind,
                            latency.as_secs_f64()
                        ));
                    }
                }
            }

            let verdicts = if !property_defs.is_empty() {
                let checker = properties::build_checker(&property_defs)?;
                checker.evaluate_all(&events)
            } else {
                Vec::new()
            };

            let result = crate::SimulationResult {
                seed,
                total_faults: summary.total_faults,
                total_failures: summary.total_failures,
                total_probes: 3 * summary.duration.as_secs() as usize,
                duration_secs: summary.duration.as_secs_f64(),
                findings: findings.clone(),
                verdicts: verdicts.clone(),
                events,
            };

            let has_findings = !findings.is_empty();
            let props_failed = verdicts.iter().any(|v| !v.passed);

            if has_findings || props_failed {
                interesting_seeds.push(seed);
            }

            if strategy == ExploreStrategy::Coverage {
                let mut combos = std::collections::HashSet::new();
                for ev in &result.events {
                    if let EventKind::FaultInjected {
                        fault_kind, target, ..
                    } = &ev.kind
                    {
                        combos.insert((fault_kind.clone(), target.clone()));
                    }
                }
                explorer.record_coverage(combos);
            }

            let icon = if props_failed {
                "❌"
            } else if has_findings {
                "🐛"
            } else {
                "✅"
            };
            let props_str = if result.verdicts.is_empty() {
                String::new()
            } else {
                let passed = result.verdicts.iter().filter(|v| v.passed).count();
                format!("  │  props: {}/{}", passed, result.verdicts.len())
            };
            if args.output == OutputFormat::Text {
                println!(
                    "  {} seed 0x{:04X}  │  faults: {}  │  failures: {}{}",
                    icon, seed, result.total_faults, result.total_failures, props_str
                );
            }

            results.push(result);
            info!(seed = seed, "✅ Seed 0x{:04X} complete.", seed);
        }
    }

    if args.output == OutputFormat::Text {
        println!();
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  EXPLORE SUMMARY (MOCK)                                    ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║  Seeds tested: {:4}  │  Interesting: {:4}                   ║",
            results.len(),
            interesting_seeds.len()
        );
        println!("╚══════════════════════════════════════════════════════════════╝");

        if args.explore_strategy == crate::ExploreStrategyArg::Coverage {
            println!("\n{}", explorer.coverage_summary());
        }

        if !interesting_seeds.is_empty() {
            println!();
            println!(
                "🐛 Interesting seeds (found fault→failure correlations or property violations):"
            );
            for &seed in &interesting_seeds {
                let result = results.iter().find(|r| r.seed == seed).unwrap();
                println!("  seed 0x{:04X}:", seed);
                for finding in &result.findings {
                    println!("    → {}", finding);
                }
                for verdict in result.verdicts.iter().filter(|v| !v.passed) {
                    println!(
                        "    ❌ {}: {} (actual: {})",
                        verdict.property_name, verdict.expected, verdict.actual
                    );
                }
            }
        } else {
            println!();
            println!("No interesting findings. Try more seeds or longer duration.");
        }
    }

    let any_prop_failures = results.iter().any(|r| r.verdicts.iter().any(|v| !v.passed));

    if args.output == OutputFormat::Json {
        let json_results: Vec<_> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "seed": r.seed,
                    "total_faults": r.total_faults,
                    "total_failures": r.total_failures,
                    "duration_secs": r.duration_secs,
                    "findings": r.findings,
                    "properties": r.verdicts,
                    "passed": r.verdicts.iter().all(|v| v.passed),
                })
            })
            .collect();
        let output = serde_json::json!({
            "seeds_tested": results.len(),
            "interesting_seeds": interesting_seeds,
            "all_passed": !any_prop_failures,
            "results": json_results,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    if any_prop_failures {
        warn!("Some seeds had property failures. Exit code 1.");
        return Ok(1);
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_generates_events() {
        let timeline = run_mock_simulation(
            42,
            Duration::from_secs(30),
            Duration::from_secs(5),
            &["crash".to_string(), "latency".to_string()],
            3,
        );
        let events = timeline.events();
        assert!(!events.is_empty());
        assert!(events.len() > 10);
    }

    #[test]
    fn test_mock_deterministic() {
        let timeline1 = run_mock_simulation(
            42,
            Duration::from_secs(30),
            Duration::from_secs(5),
            &["crash".to_string()],
            3,
        );
        let timeline2 = run_mock_simulation(
            42,
            Duration::from_secs(30),
            Duration::from_secs(5),
            &["crash".to_string()],
            3,
        );
        let events1 = timeline1.events();
        let events2 = timeline2.events();
        assert_eq!(events1.len(), events2.len());
        for (e1, e2) in events1.iter().zip(events2.iter()) {
            assert_eq!(e1.elapsed, e2.elapsed);
        }
    }

    #[test]
    fn test_mock_respects_duration() {
        let duration = Duration::from_secs(15);
        let timeline = run_mock_simulation(
            42,
            duration,
            Duration::from_secs(5),
            &["crash".to_string()],
            3,
        );
        let events = timeline.events();
        for event in events {
            assert!(event.elapsed <= duration);
        }
    }
}
