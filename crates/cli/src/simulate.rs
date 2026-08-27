use crate::dst::{DstConfig, DstResult};
use crate::{FaultProfile, OutputFormat};
use anyhow::Result;
use clap::Args;
use heisensim_core::types::VirtualTime;

#[derive(Args, Debug)]
pub struct SimulateArgs {
    /// Simulation seed (hex: 0xBEEF or decimal: 42)
    #[arg(long)]
    seed: Option<String>,

    /// Simulation duration (e.g. 5m, 30s, 1h)
    #[arg(long, default_value = "5m")]
    duration: String,

    /// Warmup period before faults begin
    #[arg(long, default_value = "30s")]
    warmup: String,

    /// Fault types to simulate
    #[arg(
        long,
        default_value = "crash,latency,partition,stress,dns",
        value_delimiter = ','
    )]
    faults: Vec<String>,

    /// Number of simulated pods
    #[arg(long, default_value = "3")]
    pods: usize,

    /// Time scale for terminal output (e.g. 100x, 10x, instant)
    #[arg(long, default_value = "instant")]
    time_scale: String,

    /// Output format
    #[arg(long, default_value = "text", value_enum)]
    output: OutputFormat,

    /// Fault profile preset
    #[arg(long, value_enum, conflicts_with = "faults")]
    profile: Option<FaultProfile>,

    /// Path to heisensim.toml config file
    #[arg(long)]
    config: Option<std::path::PathBuf>,

    /// Pre-built SLA property template (e.g. three-nines, microservice, ci)
    #[arg(long, value_enum)]
    property_template: Option<crate::properties::PropertyTemplate>,
}

pub async fn handle_simulate(args: SimulateArgs) -> Result<i32> {
    // Parse seed
    let seed = match &args.seed {
        Some(s) => crate::parse_seed(s)?,
        None => rand::random::<u64>(),
    };

    // Parse durations
    let duration = crate::parse_duration(&args.duration)?;
    let warmup = crate::parse_duration(&args.warmup)?;

    // Resolve faults from profile
    let resolved_faults = crate::resolve_faults(&args.faults, args.profile.as_ref());

    let property_defs = if let Some(ref config_path) = args.config {
        crate::properties::load_and_validate(config_path)?
    } else {
        Vec::new()
    };
    // Merge CLI template flag with config-file properties
    let property_defs = if args.property_template.is_some() {
        crate::properties::resolve_with_template(
            args.property_template.as_ref(),
            &property_defs,
            None,
        )
    } else {
        property_defs
    };

    let config = DstConfig {
        seed,
        duration: VirtualTime::from_millis(duration.as_millis() as u64),
        warmup: VirtualTime::from_millis(warmup.as_millis() as u64),
        faults: resolved_faults,
        pod_count: args.pods,
        probe_interval: VirtualTime::from_secs(5),
        property_defs,
    };

    let start = std::time::Instant::now();
    let result = crate::dst::run(config)?;
    let wall_time = start.elapsed();

    // Parse time scale
    let time_scale: f64 = if args.time_scale == "instant" {
        0.0
    } else if let Some(stripped) = args.time_scale.strip_suffix('x') {
        stripped
            .parse::<f64>()
            .map_err(|e| anyhow::anyhow!("Invalid --time-scale '{}': {}", args.time_scale, e))?
    } else {
        anyhow::bail!(
            "Invalid --time-scale '{}'. Use 'instant', '10x', '100x', etc.",
            args.time_scale
        );
    };

    if time_scale < 0.0 {
        anyhow::bail!("--time-scale must be positive, got '{}'", args.time_scale);
    }

    let exit_code = if result.verdicts.iter().any(|v| !v.passed) {
        1
    } else {
        0
    };

    match args.output {
        OutputFormat::Text => {
            // Render in a blocking task since --time-scale uses thread::sleep
            tokio::task::spawn_blocking(move || {
                render_simulate_output(&result, time_scale, wall_time);
            })
            .await?;
        }
        OutputFormat::Json => {
            let json = serde_json::json!({
                "seed": result.seed,
                "seed_hex": format!("0x{:04X}", result.seed),
                "hash": format!("{:016x}", result.hash),
                "total_faults": result.total_faults,
                "total_failures": result.total_failures,
                "events": result.events,
                "verdicts": result.verdicts,
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        OutputFormat::Junit => {
            let junit = crate::report::render_junit_report(&result.events, &result.verdicts);
            println!("{}", junit);
        }
        OutputFormat::Html => {
            let duration_secs = result
                .events
                .last()
                .map(|e| e.elapsed.as_secs_f64())
                .unwrap_or(0.0);
            let html = crate::report_html::render_html_report(
                &result.events,
                &result.verdicts,
                result.seed,
                duration_secs,
            );
            println!("{}", html);
        }
    }

    Ok(exit_code)
}

fn render_simulate_output(result: &DstResult, time_scale: f64, wall_time: std::time::Duration) {
    use heisensim_timeline::event::EventKind;

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!(
        "║  HEISENSIM SIMULATE                    seed: 0x{:04X}          ║",
        result.seed & 0xFFFF
    );
    println!("╠══════════════════════════════════════════════════════════════╣");

    let mut prev_elapsed = std::time::Duration::ZERO;

    for event in &result.events {
        // Time scaling: sleep between events for watchable output
        if time_scale > 0.0 {
            let gap = event.elapsed.saturating_sub(prev_elapsed);
            let scaled_gap = gap.div_f64(time_scale);
            if scaled_gap > std::time::Duration::from_millis(1) {
                std::thread::sleep(scaled_gap);
            }
            prev_elapsed = event.elapsed;
        }

        let elapsed_secs = event.elapsed.as_secs_f64();
        let mins = (elapsed_secs / 60.0) as u64;
        let secs = elapsed_secs % 60.0;

        let icon_and_msg = match &event.kind {
            EventKind::SimulationStarted { .. } => "🚀 Simulation started".to_string(),
            EventKind::SimulationEnded {
                total_faults,
                total_failures,
            } => {
                format!(
                    "🏁 Simulation ended ({} faults, {} failures)",
                    total_faults, total_failures
                )
            }
            EventKind::FaultInjected {
                fault_kind, target, ..
            } => {
                format!("💥 {} → {}", fault_kind, target)
            }
            EventKind::FaultReverted { .. } => "♻️  Fault reverted".to_string(),
            EventKind::ProbeSuccess {
                probe_name,
                latency_ms,
                ..
            } => {
                format!("✅ {} ({}ms)", probe_name, latency_ms)
            }
            EventKind::ProbeFailed {
                probe_name, error, ..
            } => {
                format!("❌ {} — {}", probe_name, error)
            }
            EventKind::ProbeTimeout { probe_name, .. } => {
                format!("⏱️  {} timed out", probe_name)
            }
            _ => continue,
        };

        println!("║  [T+{:02}:{:06.3}] {}", mins, secs, icon_and_msg);
    }

    println!("╠══════════════════════════════════════════════════════════════╣");

    // Property verdicts
    if !result.verdicts.is_empty() {
        let passed = result.verdicts.iter().filter(|v| v.passed).count();
        let total = result.verdicts.len();
        println!("║  Properties: {}/{} passed", passed, total);
        for v in &result.verdicts {
            let icon = if v.passed { "✅" } else { "❌" };
            println!("║    {} {}", icon, v.property_name);
        }
    }

    println!("║  Timeline Hash: {:016x}", result.hash);
    println!("║  Computed in: {:.1}ms", wall_time.as_secs_f64() * 1000.0);
    println!("╚══════════════════════════════════════════════════════════════╝");
}
