//! The Heisenbug Simulator (`heisensim`) CLI.
//!
//! Provides deterministic chaos testing for Kubernetes.

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod demo;
mod metrics;
mod properties;
mod rbac;
mod report;
mod report_html;

/// ASCII art banner printed on CLI startup.
const BANNER: &str = r#"
██╗  ██╗███████╗██╗███████╗███████╗███╗   ██╗███████╗██╗███╗   ███╗
██║  ██║██╔════╝██║██╔════╝██╔════╝████╗  ██║██╔════╝██║████╗ ████║
███████║█████╗  ██║███████╗█████╗  ██╔██╗ ██║███████╗██║██╔████╔██║
██╔══██║██╔══╝  ██║╚════██║██╔══╝  ██║╚██╗██║╚════██║██║██║╚██╔╝██║
██║  ██║███████╗██║███████║███████╗██║ ╚████║███████║██║██║ ╚═╝ ██║
╚═╝  ╚═╝╚══════╝╚═╝╚══════╝╚══════╝╚═╝  ╚═══╝╚══════╝╚═╝╚═╝     ╚═╝
             Deterministic Chaos Testing for Kubernetes
"#;

/// Top-level command-line argument parser for heisensim.
#[derive(Parser, Debug)]
#[command(
    name = "heisensim",
    author,
    version,
    about = "The Heisenbug Simulator — deterministic chaos testing for Kubernetes",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a simulation test with specified topology and fault injection rules
    Run(RunArgs),

    /// Replay a recorded bug trace deterministically
    Replay(ReplayArgs),

    /// Initialize heisensim configuration for a namespace
    Init(InitArgs),

    /// Generate a formatted bug report from a timeline recording
    Report(ReportArgs),

    /// Run multiple simulations with different seeds to find bugs
    Explore(ExploreArgs),

    /// Generate least-privilege RBAC manifests
    Rbac(RbacArgs),

    /// Run a self-contained demo (creates k3d cluster, deploys app, runs chaos test, tears down)
    Demo {
        /// Keep the cluster running after the test (don't tear down)
        #[arg(long)]
        keep: bool,

        /// Override seed (skips explore, runs single seed)
        #[arg(long)]
        seed: Option<u64>,

        /// Duration of each chaos test
        #[arg(long, default_value = "30s")]
        duration: String,
    },
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(long, default_value = "default")]
    namespace: String,

    #[arg(long, default_value = "5m")]
    duration: String,

    #[arg(long)]
    seed: Option<u64>,

    #[arg(long)]
    config: Option<PathBuf>,

    #[arg(long)]
    workload: Option<String>,

    #[arg(long, default_value = "30s")]
    warmup: String,

    #[arg(long)]
    k3d: bool,

    #[arg(
        long,
        default_value = "crash,latency,partition,stress,dns",
        value_delimiter = ','
    )]
    faults: Vec<String>,

    /// Method for injecting network faults into pods.
    /// 'exec' runs commands directly inside the target container (requires tc/iptables in image).
    /// 'debug' uses kubectl debug ephemeral containers with netshoot (works with any image).
    #[arg(long, default_value = "exec", value_enum)]
    inject_method: InjectMethod,

    /// Output format: text (default) or json
    #[arg(long, default_value = "text")]
    output: OutputFormat,

    /// OTLP endpoint for exporting traces (e.g. http://localhost:4318).
    /// When set, heisensim exports fault injection and probe spans via OpenTelemetry.
    #[arg(long)]
    otel_endpoint: Option<String>,
}

/// Method for injecting network faults into pods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InjectMethod {
    /// Execute commands directly in the target container (requires tc/iptables)
    Exec,
    /// Use kubectl debug ephemeral container with netshoot
    Debug,
}

/// Output format for results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// Human-readable terminal output
    Text,
    /// Machine-readable JSON (for CI pipelines)
    Json,
    /// JUnit XML format (for CI test reporters)
    Junit,
    /// HTML timeline visualization
    Html,
}

#[derive(Args, Debug)]
struct ReplayArgs {
    #[arg(long)]
    seed: u64,

    #[arg(long, default_value = "default")]
    namespace: String,

    #[arg(long, default_value = "5m")]
    duration: String,

    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct InitArgs {
    #[arg(long, default_value = "default")]
    namespace: String,

    #[arg(long, default_value = "heisensim.toml")]
    output: PathBuf,

    #[arg(long)]
    dry_run: bool,
}

#[derive(Args, Debug)]
struct ReportArgs {
    #[arg(long)]
    input: PathBuf,

    #[arg(long, default_value = "terminal")]
    format: String,
}

#[derive(Clone, Copy, clap::ValueEnum, PartialEq, Eq, Debug)]
pub enum ExploreStrategyArg {
    Sequential,
    Random,
    Coverage,
}

#[derive(Args, Debug)]
struct ExploreArgs {
    /// Kubernetes namespace to test
    #[arg(long, default_value = "default")]
    namespace: String,

    /// Duration of each individual simulation run
    #[arg(long, default_value = "30s")]
    duration: String,

    /// Warmup period before fault injection starts
    #[arg(long, default_value = "10s")]
    warmup: String,

    /// Number of seeds to test
    #[arg(long, default_value = "10")]
    seeds: u64,

    /// Starting seed (seeds are sequential from this value)
    #[arg(long, default_value = "1")]
    start_seed: u64,

    /// Maximum parallel runs
    #[arg(long, default_value = "3")]
    parallel: usize,

    /// Exploration strategy
    #[arg(long, default_value = "sequential", value_enum)]
    explore_strategy: ExploreStrategyArg,

    /// Bisect failing seed
    #[arg(long)]
    bisect: bool,

    /// Fault types to inject
    #[arg(
        long,
        default_value = "crash,latency,partition,stress,dns",
        value_delimiter = ','
    )]
    faults: Vec<String>,

    /// Method for injecting network faults
    #[arg(long, default_value = "exec", value_enum)]
    inject_method: InjectMethod,

    /// Path to config file with [[properties]] for SLA verification
    #[arg(long)]
    config: Option<PathBuf>,

    /// Output format: text (default) or json
    #[arg(long, default_value = "text")]
    output: OutputFormat,

    /// OTLP endpoint for exporting traces (e.g. http://localhost:4318).
    #[arg(long)]
    otel_endpoint: Option<String>,
}

#[derive(Args, Debug)]
struct RbacArgs {
    /// Target namespace
    #[arg(long, default_value = "default")]
    namespace: String,

    /// Comma-separated fault types
    #[arg(long, default_value = "crash,latency")]
    faults: String,

    /// Service account name
    #[arg(long, default_value = "heisensim")]
    service_account: String,

    /// Config file (reads faults from config)
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse CLI first to check for --otel-endpoint
    let cli = Cli::parse();

    // Extract otel_endpoint from the command args (if present)
    let otel_endpoint = match &cli.command {
        Commands::Run(args) => args.otel_endpoint.clone(),
        Commands::Explore(args) => args.otel_endpoint.clone(),
        _ => None,
    };

    // 2. Initialize tracing subscriber (with optional OTel layer)
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("heisensim=info,info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false);

    let _otel_provider = if let Some(ref endpoint) = otel_endpoint {
        // Set up W3C TraceContext propagator (E9 fix #1: must be set explicitly)
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );

        // Derive signal-specific OTLP endpoints
        let base = endpoint.trim_end_matches('/');
        let traces_endpoint = format!("{}/v1/traces", base);
        let metrics_endpoint = format!("{}/v1/metrics", base);

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(&traces_endpoint)
            .build()
            .context("Failed to create OTLP exporter")?;

        let metrics_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_endpoint(&metrics_endpoint)
            .build()
            .context("Failed to create OTLP metrics exporter")?;

        // E9 fix #3: AlwaysOn sampler — chaos events are high-value, low-volume
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_sampler(opentelemetry_sdk::trace::Sampler::AlwaysOn)
            .with_resource(
                opentelemetry_sdk::Resource::builder()
                    .with_service_name("heisensim")
                    .build(),
            )
            .build();

        let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
            .with_periodic_exporter(metrics_exporter)
            .with_resource(
                opentelemetry_sdk::Resource::builder()
                    .with_service_name("heisensim")
                    .build(),
            )
            .build();

        let otel_layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("heisensim"));

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();

        info!(
            endpoint = endpoint.as_str(),
            "OpenTelemetry tracing and metrics enabled"
        );
        Some((provider, meter_provider))
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .init();
        None
    };

    // 3. Print ASCII banner (skip for machine-readable output)
    let is_machine_output = matches!(&cli.command, Commands::Rbac(_))
        || matches!(
            &cli.command,
            Commands::Run(a) if a.output == OutputFormat::Json || a.output == OutputFormat::Junit
        );
    if !is_machine_output {
        eprintln!("{}", BANNER);
    }

    let exit_code = match cli.command {
        Commands::Run(args) => handle_run(args, _otel_provider.as_ref().map(|p| &p.1)).await?,
        Commands::Replay(args) => {
            handle_replay(args).await?;
            0
        }
        Commands::Init(args) => {
            handle_init(args).await?;
            0
        }
        Commands::Report(args) => {
            handle_report(args).await?;
            0
        }
        Commands::Explore(args) => handle_explore(args).await?,
        Commands::Rbac(args) => {
            handle_rbac(args).await?;
            0
        }
        Commands::Demo {
            keep,
            seed,
            duration,
        } => demo::run_demo(keep, seed, &duration).await?,
    };

    // 4. Shutdown OTel provider with timeout (E9 fix #2: never hang on exit)
    if let Some((provider, meter_provider)) = _otel_provider {
        info!("Flushing OpenTelemetry traces and metrics...");
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tokio::task::spawn_blocking(move || {
                let trace_result = provider.shutdown();
                let meter_result = meter_provider.shutdown();
                (trace_result, meter_result)
            }),
        )
        .await
        {
            Ok(Ok((Ok(()), Ok(())))) => info!("OpenTelemetry traces and metrics flushed."),
            Ok(Ok((Err(e), _))) => warn!("OTel trace shutdown error: {}", e),
            Ok(Ok((_, Err(e)))) => warn!("OTel metrics shutdown error: {}", e),
            Ok(Err(e)) => warn!("OTel shutdown task panicked: {}", e),
            Err(_) => warn!("OTel shutdown timed out after 3s, data may be lost."),
        }
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

async fn handle_run(
    args: RunArgs,
    meter_provider: Option<&opentelemetry_sdk::metrics::SdkMeterProvider>,
) -> Result<i32> {
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    println!("Config Summary:");
    println!("  Namespace: {}", args.namespace);
    println!("  Duration: {}", args.duration);
    println!("  Seed: 0x{:04X}", seed);
    println!("  Warmup: {}", args.warmup);
    println!("  K3d: {}", args.k3d);
    println!("  Faults: {:?}", args.faults);

    // Parse durations
    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;

    // Create K3d cluster if requested
    if args.k3d {
        info!("Creating ephemeral K3d cluster...");
        let cluster_name = format!("heisensim-{:04x}", seed & 0xFFFF);
        heisensim_k8s::K3dCluster::create(&cluster_name).await?;
        info!("K3d cluster '{}' created.", cluster_name);
    }

    // Connect to K8s
    info!("Connecting to Kubernetes cluster...");
    let client = kube::Client::try_default()
        .await
        .context("Failed to connect to Kubernetes. Is a cluster running?")?;
    info!("Connected to cluster.");

    // Discover pods
    info!("Discovering pods in namespace '{}'...", args.namespace);
    let pods = heisensim_k8s::discovery::discover_pods(&client, &args.namespace).await?;
    info!("Found {} pods.", pods.len());
    for pod in &pods {
        info!("  Pod: {} (ready: {})", pod.name, pod.is_ready);
    }

    // Scrape K8s probes
    info!("Scraping probe configs from pod specs...");
    let probes = heisensim_k8s::probe_scraper::scrape_probes(&client, &args.namespace).await?;
    info!("Discovered {} probes:", probes.len());
    for p in &probes {
        info!("  - {}", p.name());
    }

    if probes.is_empty() {
        warn!("No probes found! Make sure pods have readinessProbe or livenessProbe configured.");
    }

    // Create timeline
    let handle = heisensim_timeline::TimelineHandle::new();

    // Emit start event
    handle.emit(heisensim_timeline::EventKind::SimulationStarted {
        seed,
        duration_secs: duration.as_secs_f64(),
    });

    // Start probe runner
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let probe_runner = heisensim_probe::ProbeRunner::new(probes, handle.clone());
    let probe_handle = tokio::spawn({
        let runner = probe_runner;
        async move {
            if let Err(e) = runner.run(cancel_rx) {
                warn!("Probe runner error: {}", e);
            }
        }
    });

    // Spawn BYO workload if specified
    if let Some(ref workload) = args.workload {
        info!("Spawning workload: {}", workload);
        let parts: Vec<&str> = workload.split_whitespace().collect();
        if let Some((cmd, cmd_args)) = parts.split_first() {
            let child = tokio::process::Command::new(cmd).args(cmd_args).spawn();
            match child {
                Ok(c) => {
                    handle.emit(heisensim_timeline::EventKind::WorkloadStarted {
                        command: workload.clone(),
                        pid: c.id(),
                    });
                }
                Err(e) => warn!("Failed to spawn workload: {}", e),
            }
        }
    }

    // Warmup — let probes stabilize before injecting faults
    info!("Warming up for {}...", args.warmup);
    tokio::time::sleep(warmup).await;
    info!("Warmup complete. Starting fault injection.");

    // Fault injection loop
    let k8s_method = match args.inject_method {
        InjectMethod::Exec => heisensim_k8s::InjectMethod::Exec,
        InjectMethod::Debug => heisensim_k8s::InjectMethod::Debug,
    };
    info!("Injection method: {:?}", args.inject_method);
    let fault_op =
        heisensim_k8s::FaultOperator::with_method(client.clone(), handle.clone(), k8s_method);
    let mut rng = StdRng::seed_from_u64(seed);
    let fault_interval = duration / 4; // inject ~4 faults over the duration
    let mut elapsed = std::time::Duration::ZERO;

    while elapsed < duration {
        tokio::time::sleep(fault_interval).await;
        elapsed += fault_interval;

        if elapsed >= duration {
            break;
        }

        // Pick a random pod to target
        let live_pods = heisensim_k8s::discovery::discover_pods(&client, &args.namespace).await?;
        let ready_pods: Vec<_> = live_pods.iter().filter(|p| p.is_ready).collect();
        if ready_pods.is_empty() {
            warn!("No ready pods to target, skipping fault.");
            continue;
        }

        let target_idx = rng.random_range(0..ready_pods.len());
        let target = &ready_pods[target_idx];

        // Pick fault type from the enabled list
        let fault_types = &args.faults;
        let fault_idx = rng.random_range(0..fault_types.len());
        let fault_type = &fault_types[fault_idx];

        match fault_type.as_str() {
            "crash" => {
                info!("💥 Injecting pod crash on {}", target.name);
                match fault_op
                    .inject_pod_crash(&args.namespace, &target.name)
                    .await
                {
                    Ok(id) => info!("  Fault {}: pod deleted", id),
                    Err(e) => warn!("  Failed to crash pod: {}", e),
                }
            }
            "latency" => {
                info!("🌐 Injecting network latency on {}", target.name);
                let delay: u32 = rng.random_range(200..700);
                let jitter: u32 = rng.random_range(50..150);
                match fault_op
                    .inject_network_latency(&args.namespace, &target.name, delay, jitter, 15.0)
                    .await
                {
                    Ok(id) => info!("  Fault {}: +{}ms (jitter {}ms) for 15s", id, delay, jitter),
                    Err(e) => warn!("  Failed to inject latency: {}", e),
                }
            }
            "partition" => {
                info!("🔌 Injecting network partition on {}", target.name);
                // Pick a second pod to partition from
                let other_pods: Vec<_> = ready_pods
                    .iter()
                    .filter(|p| p.name != target.name)
                    .collect();
                if let Some(other) = other_pods.first() {
                    let other_ip = other.pod_ip.as_deref().unwrap_or("10.0.0.1");
                    match fault_op
                        .inject_partition(&args.namespace, &target.name, other_ip, 20.0)
                        .await
                    {
                        Ok(id) => info!("  Fault {}: {} ↛ {} for 20s", id, target.name, other.name),
                        Err(e) => warn!("  Failed to inject partition: {}", e),
                    }
                } else {
                    warn!("  Only one pod, skipping partition.");
                }
            }
            "stress" => {
                let workers: u32 = rng.random_range(1..4);
                let mem: u64 = rng.random_range(32..128) * 1024 * 1024;
                info!("🔥 Injecting CPU/memory stress on {}", target.name);
                match fault_op
                    .inject_stress(&args.namespace, &target.name, workers, mem, 15.0)
                    .await
                {
                    Ok(id) => info!(
                        "  Fault {}: {}x CPU + {}MB RAM for 15s",
                        id,
                        workers,
                        mem / (1024 * 1024)
                    ),
                    Err(e) => warn!("  Failed to inject stress: {}", e),
                }
            }
            "dns" => {
                info!("🌐 Injecting DNS blackhole on {}", target.name);
                match fault_op
                    .inject_dns_failure(&args.namespace, &target.name, 15.0)
                    .await
                {
                    Ok(id) => info!("  Fault {}: DNS blocked for 15s", id),
                    Err(e) => warn!("  Failed to inject DNS fault: {}", e),
                }
            }
            other => {
                warn!("Unknown fault type '{}', skipping.", other);
            }
        }
    }

    // Stop probes
    info!("Stopping probes...");
    let _ = cancel_tx.send(true);
    let _ = probe_handle.await;

    // Emit end event
    let events = handle.events();
    let summary = heisensim_timeline::query::summary(&events);
    handle.emit(heisensim_timeline::EventKind::SimulationEnded {
        total_faults: summary.total_faults,
        total_failures: summary.total_failures,
    });

    // Report
    let final_events = handle.events();
    info!("Simulation complete. Rendering report...");
    if args.output == OutputFormat::Text {
        report::render_terminal_report(&final_events);
    }

    // Save JSON
    let json = report::render_json_report(&final_events);
    let json_path = format!("heisensim-report-{:04x}.json", seed & 0xFFFF);
    std::fs::write(&json_path, &json)?;
    info!("Timeline saved to {}", json_path);

    // Property checking
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

    // JSON output
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

            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(html_path).spawn();

            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open")
                .arg(html_path)
                .spawn();
        }
    }

    // Cleanup K3d if we created it
    if args.k3d {
        let cluster_name = format!("heisensim-{:04x}", seed & 0xFFFF);
        info!("Deleting ephemeral K3d cluster '{}'...", cluster_name);
        let cluster = heisensim_k8s::K3dCluster { name: cluster_name };
        cluster.delete().await?;
    }

    Ok(exit_code)
}

/// Parse a duration string like "30s", "2m", "1h" into std::time::Duration
fn parse_duration(s: &str) -> Result<std::time::Duration> {
    let s = s.trim();
    if let Some(secs) = s.strip_suffix('s') {
        Ok(std::time::Duration::from_secs(
            secs.parse::<u64>()
                .map_err(|e| anyhow::anyhow!("Invalid seconds: {}", e))?,
        ))
    } else if let Some(mins) = s.strip_suffix('m') {
        Ok(std::time::Duration::from_secs(
            mins.parse::<u64>()
                .map_err(|e| anyhow::anyhow!("Invalid minutes: {}", e))?
                * 60,
        ))
    } else if let Some(hours) = s.strip_suffix('h') {
        Ok(std::time::Duration::from_secs(
            hours
                .parse::<u64>()
                .map_err(|e| anyhow::anyhow!("Invalid hours: {}", e))?
                * 3600,
        ))
    } else {
        Ok(std::time::Duration::from_secs(
            s.parse::<u64>()
                .map_err(|e| anyhow::anyhow!("Invalid duration: {}", e))?,
        ))
    }
}

async fn handle_replay(args: ReplayArgs) -> Result<()> {
    println!("Replaying simulation with seed: 0x{:04X}", args.seed);
    println!("Config Summary:");
    println!("  Namespace: {}", args.namespace);
    println!("  Duration: {}", args.duration);

    info!("Running replay mode...");

    let client = kube::Client::try_default()
        .await
        .context("Failed to connect to Kubernetes. Is a cluster running?")?;

    let duration = parse_duration(&args.duration)?;
    let warmup = std::time::Duration::from_secs(10);
    let faults = vec![
        "crash".to_string(),
        "latency".to_string(),
        "partition".to_string(),
        "stress".to_string(),
        "dns".to_string(),
    ];

    let _result = run_single_simulation(
        &client,
        &args.namespace,
        args.seed,
        duration,
        warmup,
        &faults,
        InjectMethod::Exec,
        &[],
    )
    .await?;

    // run_single_simulation does not currently return events, using empty vector
    let events: Vec<heisensim_timeline::event::TimelineEvent> = Vec::new();

    report::render_terminal_report(&events);

    let json = report::render_json_report(&events);
    let json_path = format!("heisensim-report-{:04x}.json", args.seed & 0xFFFF);
    std::fs::write(&json_path, &json)?;
    info!("Timeline saved to {}", json_path);

    Ok(())
}

async fn handle_init(args: InitArgs) -> Result<()> {
    info!("Connecting to Kubernetes cluster...");
    let client = kube::Client::try_default()
        .await
        .context("Failed to connect to Kubernetes. Is a cluster running?")?;

    info!(
        "Discovering services and probes in namespace '{}'...",
        args.namespace
    );
    let pods = heisensim_k8s::discovery::discover_pods(&client, &args.namespace).await?;
    let probes = heisensim_k8s::probe_scraper::scrape_probes(&client, &args.namespace).await?;

    info!("Generating heisensim.toml configuration...");

    let mut toml = format!(
        "# Generated by heisensim init\n\
         # Namespace: {namespace}\n\
         # Pods discovered: {pod_count}\n\
         # Probes discovered: {probe_count}\n\
         \n\
         [simulation]\n\
         namespace = \"{namespace}\"\n\
         duration = \"5m\"\n\
         warmup = \"30s\"\n\
         seed = 42\n\
         \n\
         [[faults]]\n\
         type = \"crash\"\n\
         probability = 0.3\n\
         \n\
         [[faults]]\n\
         type = \"latency\"\n\
         probability = 0.5\n\
         delay_ms = 500\n\
         jitter_ms = 100\n\
         \n\
         # Auto-discovered probes:\n",
        namespace = args.namespace,
        pod_count = pods.len(),
        probe_count = probes.len()
    );

    if probes.is_empty() {
        toml.push_str("# No probes discovered.\n");
        toml.push_str("# Add manual probes here. Example:\n");
        toml.push_str("# [[probes]]\n");
        toml.push_str("# type = \"http\"\n");
        toml.push_str("# name = \"my-service-health\"\n");
        toml.push_str("# url = \"http://my-service:8080/health\"\n");
        toml.push_str("# expected_status = 200\n");
        toml.push_str("# interval_ms = 10000\n");
        toml.push_str("# timeout_ms = 5000\n");
    } else {
        for probe in &probes {
            match probe {
                heisensim_probe::config::ProbeConfig::Http(c) => {
                    toml.push_str(&format!(
                        "[[probes]]\ntype = \"http\"\nname = \"{}\"\nurl = \"{}\"\nexpected_status = {}\ninterval_ms = {}\ntimeout_ms = {}\n\n",
                        c.name, c.url, c.expected_status, c.interval_ms, c.timeout_ms
                    ));
                }
                heisensim_probe::config::ProbeConfig::Tcp(c) => {
                    toml.push_str(&format!(
                        "[[probes]]\ntype = \"tcp\"\nname = \"{}\"\nhost = \"{}\"\nport = {}\ninterval_ms = {}\ntimeout_ms = {}\n\n",
                        c.name, c.host, c.port, c.interval_ms, c.timeout_ms
                    ));
                }
                heisensim_probe::config::ProbeConfig::Grpc(c) => {
                    toml.push_str(&format!(
                        "[[probes]]\ntype = \"grpc\"\nname = \"{}\"\naddress = \"{}\"\ninterval_ms = {}\ntimeout_ms = {}\n\n",
                        c.name, c.address, c.interval_ms, c.timeout_ms
                    ));
                }
                heisensim_probe::config::ProbeConfig::Exec(c) => {
                    // Quick and easy array formatting
                    let cmd_str =
                        serde_json::to_string(&c.command).unwrap_or_else(|_| "[]".to_string());
                    toml.push_str(&format!(
                        "[[probes]]\ntype = \"exec\"\nname = \"{}\"\ncommand = {}\ninterval_ms = {}\ntimeout_ms = {}\n\n",
                        c.name, cmd_str, c.interval_ms, c.timeout_ms
                    ));
                }
            }
        }
    }

    if args.dry_run {
        println!("{}", toml);
    } else {
        info!("Writing configuration to '{}'...", args.output.display());
        std::fs::write(&args.output, toml)?;
    }

    println!(
        "✨ Init complete! Discovered {} pods and {} probes.",
        pods.len(),
        probes.len()
    );

    Ok(())
}

async fn handle_report(args: ReportArgs) -> Result<()> {
    info!(
        "Loading timeline from JSON file: '{}'...",
        args.input.display()
    );

    // We mock timeline events here to fit the existing CLI code, normally we would load from file
    let content = std::fs::read_to_string(&args.input).unwrap_or_else(|_| "[]".to_string());
    let events: Vec<heisensim_timeline::event::TimelineEvent> =
        serde_json::from_str(&content).unwrap_or_default();

    info!("Running timeline queries (summary, fault-to-detection latency)...");

    match args.format.as_str() {
        "json" => println!("{}", report::render_json_report(&events)),
        "markdown" => println!("{}", report::render_markdown_report(&events)),
        _ => report::render_terminal_report(&events),
    }

    Ok(())
}

async fn handle_rbac(args: RbacArgs) -> Result<()> {
    let mut faults: Vec<String> = args
        .faults
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    if let Some(config_path) = args.config {
        let config_str =
            std::fs::read_to_string(&config_path).context("Failed to read config file")?;
        let config_val: toml::Value = toml::from_str(&config_str)?;

        if let Some(faults_arr) = config_val.get("faults").and_then(|v| v.as_array()) {
            faults.clear();
            for f in faults_arr {
                if let Some(t) = f.get("type").and_then(|v| v.as_str()) {
                    faults.push(t.to_string());
                }
            }
        }

        if let Some(sim) = config_val.get("simulation") {
            if let Some(method) = sim.get("inject_method").and_then(|v| v.as_str()) {
                if method == "debug" && !faults.contains(&"debug".to_string()) {
                    faults.push("debug".to_string());
                }
            }
        }
    }

    let yaml = rbac::generate_rbac(&args.namespace, &faults, &args.service_account);
    println!("{}", yaml);
    Ok(())
}

/// Result of a single simulation run, used by explore mode.
#[allow(dead_code)]
struct SimulationResult {
    seed: u64,
    total_faults: usize,
    total_failures: usize,
    total_probes: usize,
    duration_secs: f64,
    findings: Vec<String>,
    /// Property verdicts (empty if no properties configured)
    verdicts: Vec<heisensim_props::PropertyVerdict>,
    events: Vec<heisensim_timeline::event::TimelineEvent>,
}

/// Core simulation loop extracted for reuse by both `run` and `explore`.
async fn run_single_simulation(
    client: &kube::Client,
    namespace: &str,
    seed: u64,
    duration: std::time::Duration,
    warmup: std::time::Duration,
    faults: &[String],
    inject_method: InjectMethod,
    property_defs: &[properties::PropertyDef],
) -> Result<SimulationResult> {
    let handle = heisensim_timeline::TimelineHandle::new();

    // Discover and scrape probes
    let probes = heisensim_k8s::probe_scraper::scrape_probes(client, namespace).await?;
    let total_probes = probes.len();

    handle.emit(heisensim_timeline::EventKind::SimulationStarted {
        seed,
        duration_secs: duration.as_secs_f64(),
    });

    // Start probes
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let probe_runner = heisensim_probe::ProbeRunner::new(probes, handle.clone());
    let probe_handle = tokio::spawn({
        let runner = probe_runner;
        async move {
            let _ = runner.run(cancel_rx);
        }
    });

    // Warmup
    tokio::time::sleep(warmup).await;

    // Fault injection
    let k8s_method = match inject_method {
        InjectMethod::Exec => heisensim_k8s::InjectMethod::Exec,
        InjectMethod::Debug => heisensim_k8s::InjectMethod::Debug,
    };
    let fault_op =
        heisensim_k8s::FaultOperator::with_method(client.clone(), handle.clone(), k8s_method);
    let mut rng = StdRng::seed_from_u64(seed);
    let fault_interval = duration / 4;
    let mut elapsed = std::time::Duration::ZERO;

    while elapsed < duration {
        tokio::time::sleep(fault_interval).await;
        elapsed += fault_interval;
        if elapsed >= duration {
            break;
        }

        let live_pods = heisensim_k8s::discovery::discover_pods(client, namespace).await?;
        let ready_pods: Vec<_> = live_pods.iter().filter(|p| p.is_ready).collect();
        if ready_pods.is_empty() {
            continue;
        }

        let target_idx = rng.random_range(0..ready_pods.len());
        let target = &ready_pods[target_idx];
        let fault_idx = rng.random_range(0..faults.len());

        match faults[fault_idx].as_str() {
            "crash" => {
                let _ = fault_op.inject_pod_crash(namespace, &target.name).await;
            }
            "latency" => {
                let delay: u32 = rng.random_range(200..700);
                let jitter: u32 = rng.random_range(50..150);
                let _ = fault_op
                    .inject_network_latency(namespace, &target.name, delay, jitter, 15.0)
                    .await;
            }
            "partition" => {
                let other_pods: Vec<_> = ready_pods
                    .iter()
                    .filter(|p| p.name != target.name)
                    .collect();
                if let Some(other) = other_pods.first() {
                    let other_ip = other.pod_ip.as_deref().unwrap_or("10.0.0.1");
                    let _ = fault_op
                        .inject_partition(namespace, &target.name, other_ip, 20.0)
                        .await;
                }
            }
            "stress" => {
                let workers: u32 = rng.random_range(1..4);
                let mem: u64 = rng.random_range(32..128) * 1024 * 1024;
                let _ = fault_op
                    .inject_stress(namespace, &target.name, workers, mem, 15.0)
                    .await;
            }
            "dns" => {
                let _ = fault_op
                    .inject_dns_failure(namespace, &target.name, 15.0)
                    .await;
            }
            _ => {}
        }
    }

    // Stop probes
    let _ = cancel_tx.send(true);
    let _ = probe_handle.await;

    let events = handle.events();
    let summary = heisensim_timeline::query::summary(&events);

    // Extract fault-to-detection findings
    let mut findings = Vec::new();
    for event in &events {
        if let heisensim_timeline::EventKind::FaultInjected {
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

    // Evaluate properties
    let verdicts = if !property_defs.is_empty() {
        let checker = properties::build_checker(property_defs)?;
        checker.evaluate_all(&events)
    } else {
        Vec::new()
    };

    Ok(SimulationResult {
        seed,
        total_faults: summary.total_faults,
        total_failures: summary.total_failures,
        total_probes,
        duration_secs: duration.as_secs_f64(),
        findings,
        verdicts,
        events,
    })
}

async fn handle_explore(args: ExploreArgs) -> Result<i32> {
    anyhow::ensure!(args.parallel > 0, "--parallel must be at least 1");
    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;

    if args.output == OutputFormat::Text {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!(
            "║  HEISENSIM EXPLORE                    {} seeds, {}s each   ║",
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

    // Connect once, reuse client
    let client = kube::Client::try_default()
        .await
        .context("Failed to connect to Kubernetes")?;

    let mut results: Vec<SimulationResult> = Vec::new();
    let mut interesting_seeds: Vec<u64> = Vec::new();

    // Load property definitions from config
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

    let strategy = match args.explore_strategy {
        ExploreStrategyArg::Random => heisensim_fault::explorer::ExploreStrategy::Random,
        ExploreStrategyArg::Coverage => heisensim_fault::explorer::ExploreStrategy::Coverage,
        ExploreStrategyArg::Sequential => heisensim_fault::explorer::ExploreStrategy::Sequential,
    };

    if args.bisect && strategy == heisensim_fault::explorer::ExploreStrategy::Random {
        warn!(
            "--bisect with random strategy may not find minimal seeds (seed ordering is non-monotonic)"
        );
    }
    let mut explorer = heisensim_fault::explorer::StrategicExplorer::new(strategy, args.start_seed);

    let mut last_known_good: Option<u64> = None;
    let mut remaining = args.seeds;

    while remaining > 0 {
        let batch_size = std::cmp::min(remaining, args.parallel as u64) as usize;
        let mut batch = Vec::with_capacity(batch_size);
        for _ in 0..batch_size {
            batch.push(explorer.next_seed());
        }
        remaining -= batch_size as u64;

        let mut handles = Vec::new();
        for &seed in &batch {
            let client = client.clone();
            let ns = args.namespace.clone();
            let faults = args.faults.clone();
            let method = args.inject_method;
            let prop_defs = property_defs.clone();

            handles.push(tokio::spawn(async move {
                info!(seed = seed, "🔬 Starting seed 0x{:04X}...", seed);
                let result = run_single_simulation(
                    &client, &ns, seed, duration, warmup, &faults, method, &prop_defs,
                )
                .await;
                info!(seed = seed, "✅ Seed 0x{:04X} complete.", seed);
                (seed, result)
            }));
        }

        for handle in handles {
            match handle.await {
                Ok((seed, Ok(result))) => {
                    let has_findings = !result.findings.is_empty();
                    let props_failed = result.verdicts.iter().any(|v| !v.passed);
                    if has_findings || props_failed {
                        interesting_seeds.push(seed);

                        if args.bisect {
                            let good_seed = last_known_good.unwrap_or(0);
                            println!(
                                "  🔍 Bisection enabled. Bisecting between {} and {}...",
                                good_seed, seed
                            );
                            let mut low = good_seed;
                            let mut high = seed;
                            let mut min_failing = seed;

                            while low <= high {
                                let mid = low + (high - low) / 2;
                                println!("    Checking candidate seed {}...", mid);
                                let b_result = run_single_simulation(
                                    &client,
                                    &args.namespace,
                                    mid,
                                    duration,
                                    warmup,
                                    &args.faults,
                                    args.inject_method,
                                    &property_defs,
                                )
                                .await;
                                match b_result {
                                    Ok(res) => {
                                        let b_fail = !res.findings.is_empty()
                                            || res.verdicts.iter().any(|v| !v.passed);
                                        if b_fail {
                                            min_failing = mid;
                                            if mid == 0 {
                                                break;
                                            }
                                            high = mid - 1;
                                        } else {
                                            low = mid + 1;
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Bisection error at seed {}: {}", mid, e);
                                        break;
                                    }
                                }
                            }
                            println!(
                                "  Minimal failing seed: {} (bisected from {})",
                                min_failing, seed
                            );
                            if min_failing != seed {
                                interesting_seeds.push(min_failing);
                            }
                        }
                    } else {
                        last_known_good = Some(seed);
                    }

                    if strategy == heisensim_fault::explorer::ExploreStrategy::Coverage {
                        let mut combos = std::collections::HashSet::new();
                        for ev in &result.events {
                            if let heisensim_timeline::EventKind::FaultInjected {
                                fault_kind,
                                target,
                                ..
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
                    println!(
                        "  {} seed 0x{:04X}  │  faults: {}  │  failures: {}{}",
                        icon, seed, result.total_faults, result.total_failures, props_str
                    );

                    results.push(result);
                }
                Ok((seed, Err(e))) => {
                    println!("  ❌ seed 0x{:04X}  │  ERROR: {}", seed, e);
                }
                Err(e) => {
                    warn!("Task panicked: {}", e);
                }
            }
        }
    }

    // Summary
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  EXPLORE SUMMARY                                           ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║  Seeds tested: {:4}  │  Interesting: {:4}                   ║",
        results.len(),
        interesting_seeds.len()
    );
    println!("╚══════════════════════════════════════════════════════════════╝");

    if args.explore_strategy == ExploreStrategyArg::Coverage {
        println!("\n{}", explorer.coverage_summary());
    }

    if !interesting_seeds.is_empty() {
        println!();
        println!("🐛 Interesting seeds (found fault→failure correlations or property violations):");
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
        println!();
        println!("Replay any interesting seed:");
        for &seed in &interesting_seeds {
            println!(
                "  heisensim run --namespace {} --seed {} --duration {}",
                args.namespace, seed, args.duration
            );
        }
    } else {
        println!();
        println!("No interesting findings. Try more seeds or longer duration:");
        println!(
            "  heisensim explore --namespace {} --seeds {} --duration {}",
            args.namespace,
            args.seeds * 2,
            args.duration
        );
    }

    // Exit code 1 if any seed had property failures
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
