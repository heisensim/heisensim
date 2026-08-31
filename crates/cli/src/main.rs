//! The Heisenbug Simulator (`heisensim`) CLI.
//!
//! Provides deterministic chaos testing for Kubernetes.

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use heisensim_props::TimelineProperty;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod demo;
pub mod diff;
pub mod dst;
mod metrics;
mod mock;
mod properties;
mod rbac;
mod report;
mod report_html;
pub mod simulate;

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
    /// Run a discrete-event simulation of the cluster locally
    Simulate(simulate::SimulateArgs),

    /// Run a simulation test with specified topology and fault injection rules
    Run(RunArgs),

    /// Replay a recorded bug trace deterministically
    Replay(ReplayArgs),

    /// Generate a starter heisensim.toml configuration file
    Init(InitArgs),

    /// Validate a heisensim configuration file without running
    Validate(ValidateArgs),

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

    /// Compare two simulation runs side-by-side
    Diff(DiffArgs),

    /// Manipulate time in a running pod via vDSO trampoline (Linux only)
    TimeWarp(TimeWarpArgs),

    /// Inject process-level faults (connect-error, fd-exhaustion) via ptrace (Linux only)
    ProcessFault(ProcessFaultArgs),

    /// Diverge preview environment integration
    #[command(subcommand)]
    Diverge(DivergeCmds),
}

/// Diverge preview environment subcommands.
#[derive(Subcommand, Debug)]
enum DivergeCmds {
    /// Run chaos testing against a Diverge preview environment
    Run(DivergeRunArgs),
}

#[derive(Args, Debug)]
struct DivergeRunArgs {
    /// Diverge environment name (e.g. "pr-123")
    pub env_name: String,

    /// Kubernetes namespace (default: auto-detect from Diverge API)
    #[arg(long)]
    pub namespace: Option<String>,

    /// Diverge API URL (or set DIVERGE_URL env var)
    #[arg(long, default_value = "http://localhost:8080")]
    pub url: String,

    /// Auth token for Diverge API (auto-detects from DIVERGE_TOKEN env/SA if not set)
    #[arg(long)]
    pub token: Option<String>,

    /// Max time to wait for environment to become Running
    #[arg(long, default_value = "30s")]
    pub wait: String,

    /// Target ALL services (default: only changed services + upstream callers)
    #[arg(long)]
    pub all_services: bool,

    /// Chaos config file (if omitted, auto-generates probes from Diverge metadata)
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Duration of chaos testing
    #[arg(long, default_value = "30s")]
    pub duration: String,

    /// Seed for deterministic replay
    #[arg(long)]
    pub seed: Option<u64>,

    /// Fault types to inject
    #[arg(long, default_value = "latency,partition,dns", value_delimiter = ',')]
    pub faults: Vec<String>,

    /// Run in soft-fail mode (exit 0, report only — for CI trust-building)
    #[arg(long)]
    pub soft_fail: bool,

    /// Output format
    #[arg(long, default_value = "text")]
    pub output: OutputFormat,

    /// OTLP endpoint for exporting traces
    #[arg(long)]
    pub otel_endpoint: Option<String>,

    /// Enable A/B baseline diffing
    #[arg(long)]
    pub baseline: bool,

    /// Maximum allowed latency multiplier vs baseline (default: 3.0)
    #[arg(long, default_value = "3.0")]
    pub max_latency_multiplier: f64,

    /// Maximum allowed availability drop in percentage points (default: 10.0)
    #[arg(long, default_value = "10.0")]
    pub max_availability_drop: f64,

    /// Export baseline snapshot to a JSON file
    #[arg(long)]
    pub export_baseline: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct DiffArgs {
    /// First seed to compare
    #[arg(long)]
    pub seed_a: String,

    /// Second seed to compare  
    #[arg(long)]
    pub seed_b: String,

    /// Duration of each simulation
    #[arg(long, default_value = "30s")]
    pub duration: String,

    /// Warmup period
    #[arg(long, default_value = "10s")]
    pub warmup: String,

    /// Fault types
    #[arg(
        long,
        default_value = "crash,latency,partition,stress,dns",
        value_delimiter = ','
    )]
    pub faults: Vec<String>,

    /// Fault profile
    #[arg(long, value_enum, conflicts_with = "faults")]
    pub profile: Option<FaultProfile>,

    /// Config file for properties
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Output format
    #[arg(long, default_value = "text")]
    pub output: OutputFormat,

    /// Pre-built SLA property template (e.g. three-nines, microservice, ci)
    #[arg(long, value_enum)]
    pub property_template: Option<properties::PropertyTemplate>,
}

#[derive(Args, Debug)]
pub struct TimeWarpArgs {
    /// Target pod name
    #[arg(long)]
    pub pod: String,

    /// Kubernetes namespace
    #[arg(long, default_value = "default")]
    pub namespace: String,

    /// Time offset to apply (e.g., "+30d", "-2h", "+90m", "+3600s")
    #[arg(long)]
    pub offset: String,

    /// Time speed multiplier (e.g., "10x", "0.5x")
    #[arg(long, default_value = "1x")]
    pub speed: String,

    /// Revert a previous time warp on the pod
    #[arg(long)]
    pub revert: bool,

    /// Container name within the pod (defaults to first container)
    #[arg(long)]
    pub container: Option<String>,
}

#[derive(Args, Debug)]
pub struct ProcessFaultArgs {
    /// Target pod name
    #[arg(long)]
    pub pod: String,

    /// Kubernetes namespace
    #[arg(long, default_value = "default")]
    pub namespace: String,

    /// Fault type to inject
    #[arg(long, value_enum)]
    pub fault: ProcessFaultType,

    /// Duration to run fault injection (e.g., "30s", "1m", "5m")
    #[arg(long, default_value = "30s")]
    pub duration: String,

    /// Target port to filter (only inject faults on connections to this port)
    #[arg(long)]
    pub port: Option<u16>,

    /// Custom errno for connect-error (default: 111/ECONNREFUSED)
    #[arg(long, default_value = "111")]
    pub errno: i32,

    /// Container name within the pod (defaults to first container)
    #[arg(long)]
    pub container: Option<String>,

    /// Latency in milliseconds for connect-latency (default: 200ms)
    #[arg(long, default_value = "200")]
    pub latency: u64,
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum ProcessFaultType {
    /// Reject all connect() calls with ECONNREFUSED
    ConnectError,
    /// Exhaust file descriptors (EMFILE on socket())
    FdExhaustion,
    /// Add latency to connect() calls
    ConnectLatency,
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Output path (default: heisensim.toml)
    #[arg(short, long, default_value = "heisensim.toml")]
    pub output: String,

    /// Config preset template
    #[arg(long, value_enum)]
    pub preset: Option<InitPreset>,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum InitPreset {
    /// Basic chaos testing config (default)
    Basic,
    /// Microservice mesh resilience testing
    Microservice,
    /// Stateful workload (database, queue) testing
    Stateful,
    /// CI-optimized quick smoke test
    Ci,
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

    /// Fault types to inject (available: crash, latency, partition, stress, dns, eviction, time-warp, connect-error, fd-exhaustion, connect-latency)
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

    /// Run in mock mode (no Kubernetes cluster required)
    #[arg(long)]
    mock: bool,
    /// Grace period in seconds for crash faults (default: K8s default 30s).
    /// Use 0 for immediate kill (no SIGTERM).
    #[arg(long)]
    crash_grace_period: Option<u32>,

    /// Fault profile preset. Overrides --faults when set.
    #[arg(long, value_enum, conflicts_with = "faults")]
    profile: Option<FaultProfile>,

    /// Pre-built SLA property template (e.g. three-nines, microservice, ci)
    #[arg(long, value_enum)]
    property_template: Option<properties::PropertyTemplate>,

    /// Enable A/B baseline diffing. Captures probe metrics during warmup as baseline,
    /// then asserts chaos-phase metrics stay within acceptable degradation.
    #[arg(long)]
    baseline: bool,

    /// Maximum allowed latency multiplier vs baseline (default: 3.0).
    /// Only used with --baseline. Example: 3.0 means chaos p95 ≤ 3x baseline p95.
    #[arg(long, default_value = "3.0")]
    max_latency_multiplier: f64,

    /// Maximum allowed availability drop in percentage points (default: 10.0).
    /// Only used with --baseline. Example: 10.0 means avail can drop at most 10pp.
    #[arg(long, default_value = "10.0")]
    max_availability_drop: f64,

    /// Export baseline snapshot to a JSON file for CI drift tracking.
    #[arg(long)]
    export_baseline: Option<PathBuf>,
}

/// Fault profile presets.
#[derive(Clone, Debug, ValueEnum)]
pub enum FaultProfile {
    /// Network and pod faults (crash, latency, partition, stress, dns)
    Standard,
    /// Standard + eviction (tests PodDisruptionBudgets)
    Aggressive,
}

pub fn resolve_faults(faults: &[String], profile: Option<&FaultProfile>) -> Vec<String> {
    if let Some(p) = profile {
        match p {
            FaultProfile::Standard => vec!["crash", "latency", "partition", "stress", "dns"]
                .into_iter()
                .map(String::from)
                .collect(),
            FaultProfile::Aggressive => {
                vec!["crash", "latency", "partition", "stress", "dns", "eviction"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            }
        }
    } else {
        faults.to_vec()
    }
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
pub enum OutputFormat {
    /// Human-readable terminal output
    Text,
    /// Machine-readable JSON (for CI pipelines)
    Json,
    /// JUnit XML format (for CI test reporters)
    Junit,
    /// HTML timeline visualization
    Html,
}

pub fn parse_seed(s: &str) -> Result<u64> {
    if let Some(stripped) = s.strip_prefix("0x") {
        u64::from_str_radix(stripped, 16).map_err(|e| anyhow::anyhow!("Invalid hex seed: {}", e))
    } else {
        s.parse::<u64>()
            .map_err(|e| anyhow::anyhow!("Invalid decimal seed: {}", e))
    }
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

    /// Fault types to inject (available: crash, latency, partition, stress, dns, eviction, time-warp, connect-error, fd-exhaustion, connect-latency)
    #[arg(
        long,
        default_value = "crash,latency,partition,stress,dns",
        value_delimiter = ','
    )]
    faults: Vec<String>,

    /// Method for injecting network faults
    #[arg(long, default_value = "exec", value_enum)]
    inject_method: InjectMethod,

    /// Grace period in seconds for crash faults (default: K8s default 30s).
    #[arg(long)]
    crash_grace_period: Option<u32>,

    /// Fault profile preset. Overrides --faults when set.
    #[arg(long, value_enum, conflicts_with = "faults")]
    profile: Option<FaultProfile>,
}

#[derive(Args, Debug)]
struct ValidateArgs {
    /// Path to config file
    #[arg(short, long)]
    config: String,
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

    /// Use deterministic simulation engine instead of live Kubernetes
    #[arg(long)]
    simulate: bool,

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

    /// Fault types to inject (available: crash, latency, partition, stress, dns, eviction, time-warp, connect-error, fd-exhaustion, connect-latency)
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

    /// Run in mock mode (no Kubernetes cluster required)
    #[arg(long)]
    mock: bool,
    /// Grace period in seconds for crash faults (default: K8s default 30s).
    /// Use 0 for immediate kill (no SIGTERM).
    #[arg(long)]
    crash_grace_period: Option<u32>,

    /// Fault profile preset. Overrides --faults when set.
    #[arg(long, value_enum, conflicts_with = "faults")]
    profile: Option<FaultProfile>,

    /// Pre-built SLA property template (e.g. three-nines, microservice, ci)
    #[arg(long, value_enum)]
    property_template: Option<properties::PropertyTemplate>,
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
        Commands::Diverge(DivergeCmds::Run(args)) => args.otel_endpoint.clone(),
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

        // Set global meter provider so any crate can emit streaming metrics
        // via opentelemetry::global::meter("heisensim") — no-op if not set.
        opentelemetry::global::set_meter_provider(meter_provider.clone());

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
        )
        || matches!(
            &cli.command,
            Commands::Diverge(DivergeCmds::Run(a)) if a.output == OutputFormat::Json
        );
    if !is_machine_output {
        eprintln!("{}", BANNER);
    }

    let exit_code = match cli.command {
        Commands::Simulate(args) => simulate::handle_simulate(args).await?,
        Commands::Run(args) => handle_run(args, _otel_provider.as_ref().map(|p| &p.1)).await?,
        Commands::Replay(args) => {
            handle_replay(args).await?;
            0
        }
        Commands::Init(args) => {
            handle_init(args).await?;
            0
        }
        Commands::Validate(args) => {
            handle_validate(args).await?;
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
        Commands::Diff(args) => crate::diff::handle_diff(args).await?,
        Commands::TimeWarp(args) => {
            handle_time_warp(args).await?;
            0
        }
        Commands::ProcessFault(args) => {
            handle_process_fault(args).await?;
            0
        }
        Commands::Diverge(DivergeCmds::Run(args)) => handle_diverge_run(args).await?,
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

    if args.mock {
        tracing::warn!(
            "--mock is deprecated. Use `heisensim simulate` for a fully deterministic in-memory simulation engine instead."
        );
        return mock::handle_mock_run(&args, meter_provider).await;
    }

    println!("Config Summary:");
    println!("  Namespace: {}", args.namespace);
    println!("  Duration: {}", args.duration);
    println!("  Seed: 0x{:04X}", seed);
    println!("  Warmup: {}", args.warmup);
    println!("  K3d: {}", args.k3d);
    let resolved_faults = resolve_faults(&args.faults, args.profile.as_ref());
    println!("  Faults: {:?}", resolved_faults);

    // Parse durations
    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;

    // Parse property config early — fail before cluster creation on bad TOML
    let property_defs: Vec<properties::PropertyDef> = if let Some(ref config_path) = args.config {
        let config_str =
            std::fs::read_to_string(config_path).context("Failed to read config file")?;
        let config: properties::PropertiesConfig =
            toml::from_str(&config_str).context("Failed to parse properties config")?;
        properties::resolve_with_template(
            args.property_template.as_ref(),
            &config.properties,
            config.template.as_ref(),
        )
    } else if let Some(ref tmpl) = args.property_template {
        tmpl.to_property_defs()
    } else {
        Vec::new()
    };

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
    info!("Warmup complete.");

    // Capture baseline snapshot (if --baseline is enabled)
    let baseline_snapshot = if args.baseline {
        let events = handle.events();
        match heisensim_props::capture_baseline(&events, warmup) {
            Some(snapshot) => {
                info!(
                    "📊 Baseline captured: {} probes, {} total samples",
                    snapshot.probes.len(),
                    snapshot.total_samples
                );
                for (name, probe) in &snapshot.probes {
                    info!(
                        "  {} — p50: {}ms, p95: {}ms, avail: {:.1}%",
                        name, probe.p50_ms, probe.p95_ms, probe.success_rate
                    );
                }

                // Sanity check: warn if baseline is already degraded
                for (name, probe) in &snapshot.probes {
                    if probe.success_rate < 90.0 {
                        warn!(
                            "⚠️  Baseline for {} has low availability ({:.1}%) — results may be unreliable",
                            name, probe.success_rate
                        );
                    }
                }

                // Export baseline JSON if requested
                if let Some(ref path) = args.export_baseline {
                    let json = serde_json::to_string_pretty(&snapshot)?;
                    std::fs::write(path, &json)?;
                    info!("📁 Baseline exported to {}", path.display());
                }

                Some(snapshot)
            }
            None => {
                anyhow::bail!(
                    "No probe data captured during warmup — cannot establish baseline.\n\
                     Check that probes are configured correctly and warmup duration is sufficient."
                );
            }
        }
    } else {
        None
    };

    info!("Starting fault injection.");

    // Fault injection loop
    let k8s_method = match args.inject_method {
        InjectMethod::Exec => heisensim_k8s::InjectMethod::Exec,
        InjectMethod::Debug => heisensim_k8s::InjectMethod::Debug,
    };
    info!("Injection method: {:?}", args.inject_method);
    let fault_op =
        heisensim_k8s::FaultOperator::with_method(client.clone(), handle.clone(), k8s_method);

    // Initialize fault tracker for graceful shutdown
    let tracker = std::sync::Arc::new(heisensim_k8s::FaultTracker::new());

    // SIGINT handler — revert all active faults before exiting
    let shutdown_tracker = tracker.clone();
    let shutdown_fault_op = fault_op.clone();
    let shutdown_cancel = cancel_tx.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            warn!("⚠️  SIGINT received — reverting all active faults...");
            let _ = shutdown_cancel.send(true); // Stop probes
            let results = shutdown_tracker.revert_all(&shutdown_fault_op).await;
            let failed = results.iter().filter(|(_, r)| r.is_err()).count();
            if failed > 0 {
                warn!(
                    "⚠️  {} faults could not be reverted — manual intervention may be required!",
                    failed
                );
            }
            std::process::exit(130); // 128 + SIGINT
        }
    });

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
        let fault_types = &resolved_faults;
        let fault_idx = rng.random_range(0..fault_types.len());
        let fault_type = &fault_types[fault_idx];

        match fault_type.as_str() {
            "crash" => {
                info!("💥 Injecting pod crash on {}", target.name);
                match fault_op
                    .inject_pod_crash(&args.namespace, &target.name, args.crash_grace_period)
                    .await
                {
                    Ok(id) => info!("  Fault {}: pod deleted", id),
                    Err(e) => warn!("  Failed to crash pod: {}", e),
                }
            }
            "eviction" => {
                info!("🏚️  Evicting pod {}", target.name);
                match fault_op
                    .inject_eviction(&args.namespace, &target.name)
                    .await
                {
                    Ok((id, true)) => info!("  Fault {}: pod evicted (tests PDB)", id),
                    Ok((id, false)) => info!("  Fault {}: eviction blocked by PDB ✓", id),
                    Err(e) => warn!("  Failed to evict pod: {}", e),
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
                    Ok(id) => {
                        info!("  Fault {}: +{}ms (jitter {}ms) for 15s", id, delay, jitter);
                        let fault = heisensim_k8s::ActiveFault {
                            fault_id: id,
                            kind: heisensim_k8s::ActiveFaultKind::NetworkLatency,
                            namespace: args.namespace.clone(),
                            pod_name: target.name.clone(),
                            injected_at: std::time::Instant::now(),
                        };
                        tracker.track(fault).await;
                        // Schedule timed revert
                        let t = tracker.clone();
                        let fo = fault_op.clone();
                        let ns = args.namespace.clone();
                        let pn = target.name.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                            match fo.revert_network_latency(&ns, &pn, id).await {
                                Ok(()) => t.untrack(id).await,
                                Err(e) => {
                                    warn!(fault_id = %id, error = %e, "Timed revert failed; will retry at shutdown")
                                }
                            }
                        });
                    }
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
                        Ok(id) => {
                            info!("  Fault {}: {} ↛ {} for 20s", id, target.name, other.name);
                            let fault = heisensim_k8s::ActiveFault {
                                fault_id: id,
                                kind: heisensim_k8s::ActiveFaultKind::Partition {
                                    target_ip: other_ip.to_string(),
                                },
                                namespace: args.namespace.clone(),
                                pod_name: target.name.clone(),
                                injected_at: std::time::Instant::now(),
                            };
                            tracker.track(fault).await;
                            let t = tracker.clone();
                            let fo = fault_op.clone();
                            let ns = args.namespace.clone();
                            let pn = target.name.clone();
                            let ip = other_ip.to_string();
                            tokio::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                                match fo.revert_partition(&ns, &pn, &ip, id).await {
                                    Ok(()) => t.untrack(id).await,
                                    Err(e) => {
                                        warn!(fault_id = %id, error = %e, "Timed revert failed; will retry at shutdown")
                                    }
                                }
                            });
                        }
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
                    Ok(id) => {
                        info!("  Fault {}: DNS blocked for 15s", id);
                        let fault = heisensim_k8s::ActiveFault {
                            fault_id: id,
                            kind: heisensim_k8s::ActiveFaultKind::DnsFailure,
                            namespace: args.namespace.clone(),
                            pod_name: target.name.clone(),
                            injected_at: std::time::Instant::now(),
                        };
                        tracker.track(fault).await;
                        let t = tracker.clone();
                        let fo = fault_op.clone();
                        let ns = args.namespace.clone();
                        let pn = target.name.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                            match fo.revert_dns_failure(&ns, &pn, id).await {
                                Ok(()) => t.untrack(id).await,
                                Err(e) => {
                                    warn!(fault_id = %id, error = %e, "Timed revert failed; will retry at shutdown")
                                }
                            }
                        });
                    }
                    Err(e) => warn!("  Failed to inject DNS fault: {}", e),
                }
            }
            "time-warp" => {
                info!("⏰ Injecting time warp on {}", target.name);
                // Time warp uses vDSO trampoline via ephemeral container.
                // --target shares the PID namespace with the workload container.
                let target_container = target
                    .container_names
                    .first()
                    .map(|c| c.as_str())
                    .unwrap_or(&target.name);
                let target_flag = format!("--target={}", target_container);
                let mut cmd = tokio::process::Command::new("kubectl");
                cmd.args([
                    "debug",
                    "-n",
                    &args.namespace,
                    &target.name,
                    "--image=ghcr.io/heisensim/heisensim:latest",
                    "--container=heisensim-timewarp",
                    &target_flag,
                    "--",
                    "heisensim-inject",
                    "--pid",
                    "1",
                    "--offset",
                    "+1h",
                ]);
                match cmd.output().await {
                    Ok(output) if output.status.success() => {
                        info!("  Time warp applied (+1h offset)");
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        warn!("  Failed to inject time warp: {}", stderr.trim());
                    }
                    Err(e) => warn!("  Failed to inject time warp: {}", e),
                }
            }
            "connect-error" => {
                info!("🔌 Injecting connect-error on {}", target.name);
                let container_name =
                    format!("heisensim-fault-{}", &uuid::Uuid::new_v4().to_string()[..8]);
                let target_flag = format!(
                    "--target={}",
                    target
                        .container_names
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or(&target.name)
                );
                let container_flag = format!("--container={}", container_name);
                let duration_str = "15s".to_string();

                let output = tokio::process::Command::new("kubectl")
                    .args([
                        "debug",
                        "-n",
                        &args.namespace,
                        &target.name,
                        "--image=ghcr.io/heisensim/heisensim:latest",
                        &container_flag,
                        &target_flag,
                        "--",
                        "heisensim-inject",
                        "--pid",
                        "1",
                        "--fault",
                        "connect-error",
                        "--duration",
                        &duration_str,
                    ])
                    .output()
                    .await;

                match output {
                    Ok(o) if o.status.success() => {
                        info!("  ✅ connect-error active for {}", duration_str);
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        warn!("  Failed to inject connect-error: {}", stderr.trim());
                    }
                    Err(e) => warn!("  Failed to inject connect-error: {}", e),
                }
            }
            "fd-exhaustion" => {
                info!("🔌 Injecting fd-exhaustion on {}", target.name);
                let container_name =
                    format!("heisensim-fault-{}", &uuid::Uuid::new_v4().to_string()[..8]);
                let target_flag = format!(
                    "--target={}",
                    target
                        .container_names
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or(&target.name)
                );
                let container_flag = format!("--container={}", container_name);
                let duration_str = "15s".to_string();

                let output = tokio::process::Command::new("kubectl")
                    .args([
                        "debug",
                        "-n",
                        &args.namespace,
                        &target.name,
                        "--image=ghcr.io/heisensim/heisensim:latest",
                        &container_flag,
                        &target_flag,
                        "--",
                        "heisensim-inject",
                        "--pid",
                        "1",
                        "--fault",
                        "fd-exhaustion",
                        "--duration",
                        &duration_str,
                    ])
                    .output()
                    .await;

                match output {
                    Ok(o) if o.status.success() => {
                        info!("  ✅ fd-exhaustion active for {}", duration_str);
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        warn!("  Failed to inject fd-exhaustion: {}", stderr.trim());
                    }
                    Err(e) => warn!("  Failed to inject fd-exhaustion: {}", e),
                }
            }
            "connect-latency" => {
                info!("🔌 Injecting connect-latency on {}", target.name);
                let container_name =
                    format!("heisensim-fault-{}", &uuid::Uuid::new_v4().to_string()[..8]);
                let target_flag = format!(
                    "--target={}",
                    target
                        .container_names
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or(&target.name)
                );
                let container_flag = format!("--container={}", container_name);
                let duration_str = "15s".to_string();

                let output = tokio::process::Command::new("kubectl")
                    .args([
                        "debug",
                        "-n",
                        &args.namespace,
                        &target.name,
                        "--image=ghcr.io/heisensim/heisensim:latest",
                        &container_flag,
                        &target_flag,
                        "--",
                        "heisensim-inject",
                        "--pid",
                        "1",
                        "--fault",
                        "connect-latency",
                        "--latency",
                        "200",
                        "--duration",
                        &duration_str,
                    ])
                    .output()
                    .await;

                match output {
                    Ok(o) if o.status.success() => {
                        info!("  ✅ connect-latency active for {}", duration_str);
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        warn!("  Failed to inject connect-latency: {}", stderr.trim());
                    }
                    Err(e) => warn!("  Failed to inject connect-latency: {}", e),
                }
            }
            other => {
                warn!("Unknown fault type '{}', skipping.", other);
            }
        }
    }

    // Revert any remaining active faults (graceful cleanup)
    let active = tracker.active_count().await;
    if active > 0 {
        info!(
            "🧹 Cleaning up {} active fault(s) before shutdown...",
            active
        );
        tracker.revert_all(&fault_op).await;
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

    // Property checking (defs were parsed early, before cluster creation)
    let mut exit_code = 0;
    let mut verdicts = Vec::new();
    if !property_defs.is_empty() {
        info!("Evaluating {} properties...", property_defs.len());
        let checker = properties::build_checker(&property_defs)?;
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

    // Baseline diff properties (if --baseline was enabled and we have a snapshot)
    if let Some(ref snapshot) = baseline_snapshot {
        info!("📊 Evaluating baseline diff properties...");
        let latency_prop = heisensim_props::BaselineLatencyDiff::new(
            snapshot.clone(),
            warmup,
            args.max_latency_multiplier,
        );
        let avail_prop = heisensim_props::BaselineAvailabilityDiff::new(
            snapshot.clone(),
            warmup,
            args.max_availability_drop,
        );

        let lat_verdict = latency_prop.evaluate(&final_events);
        let avail_verdict = avail_prop.evaluate(&final_events);

        if args.output == OutputFormat::Text {
            properties::print_verdicts(&[lat_verdict.clone(), avail_verdict.clone()]);
        }

        if !lat_verdict.passed || !avail_verdict.passed {
            warn!("Baseline diff properties FAILED. Exit code 1.");
            exit_code = 1;
        }

        verdicts.push(lat_verdict);
        verdicts.push(avail_verdict);
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
pub fn parse_duration(s: &str) -> Result<std::time::Duration> {
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
    let resolved_faults = resolve_faults(&args.faults, args.profile.as_ref());

    let result = run_single_simulation(
        &client,
        &args.namespace,
        args.seed,
        duration,
        warmup,
        &resolved_faults,
        args.inject_method,
        args.crash_grace_period,
        &[],
    )
    .await?;

    report::render_terminal_report(&result.events);

    let json = report::render_json_report(&result.events);
    let json_path = format!("heisensim-report-{:04x}.json", args.seed & 0xFFFF);
    std::fs::write(&json_path, &json)?;
    info!("Timeline saved to {}", json_path);

    Ok(())
}

async fn handle_init(args: InitArgs) -> Result<()> {
    let preset = args.preset.unwrap_or(InitPreset::Basic);
    let output = &args.output;

    let (config, preset_name) = match preset {
        InitPreset::Basic => (
            r#"# heisensim configuration (basic preset)
# Docs: https://heisensim.dev

# Simulation duration
duration = "30s"

# Warmup period before fault injection begins
warmup = "5s"

# Kubernetes namespace to target
namespace = "default"

# Fault types to inject
# Available: network-delay, network-loss, network-partition, stress, dns
faults = ["network-delay", "network-loss"]
# Or use --profile standard (default) or --profile aggressive (adds eviction)

# HTTP probes to monitor during chaos testing
[[probes]]
name = "my-service"
url = "http://my-service.default.svc:8080/health"
interval = "1s"

# SLA properties to verify
# Exit code 1 if any property fails — CI-native

[[properties]]
name = "fast-recovery"
type = "recovery_time"
max_seconds = 30

[[properties]]
name = "high-availability"
type = "availability"
min_percent = 99.0

[[properties]]
name = "low-latency"
type = "latency_p99"
max_ms = 500
"#,
            "basic",
        ),
        InitPreset::Microservice => (
            r#"# heisensim configuration (microservice preset)
# Tests resilience of a microservice mesh with API gateway and backend

duration = "5m"
warmup = "15s"
namespace = "default"
faults = ["crash", "latency", "partition", "dns"]

[[probes]]
name = "api-gateway"
url = "http://api-gateway.default.svc:8080/health"
interval = "1s"

[[probes]]
name = "backend-service"
url = "http://backend-service.default.svc:8080/ready"
interval = "2s"

[[properties]]
name = "high-availability"
type = "availability"
min_percent = 99.5

[[properties]]
name = "fast-recovery"
type = "recovery_time"
max_seconds = 30

[[properties]]
name = "low-latency"
type = "latency_p99"
max_ms = 500

[[properties]]
name = "prevent-cascading-failure"
type = "no_cascade"

[[properties]]
name = "dns-resilience"
type = "dns-resolution"
max_recovery_seconds = 10
"#,
            "microservice",
        ),
        InitPreset::Stateful => (
            r#"# heisensim configuration (stateful preset)
# Tests resilience of stateful workloads like databases or message queues

duration = "3m"
warmup = "30s"
namespace = "default"
faults = ["crash", "eviction", "stress"]

[[probes]]
name = "database-primary"
url = "http://database-primary.default.svc:9090/healthz"
interval = "5s"

[[properties]]
name = "extreme-availability"
type = "availability"
min_percent = 99.9

[[properties]]
name = "recovery-time"
type = "recovery_time"
max_seconds = 60

[[properties]]
name = "error-budget"
type = "error_budget"
max_consecutive = 3

[[properties]]
name = "returns-to-normal"
type = "steady-state"
max_recovery_seconds = 60
baseline_seconds = 30
"#,
            "stateful",
        ),
        InitPreset::Ci => (
            r#"# heisensim configuration (CI preset)
# Optimized for fast CI feedback

duration = "1m"
warmup = "5s"
namespace = "default"
faults = ["crash", "latency", "partition"]

[[probes]]
name = "my-service"
url = "http://my-service.default.svc:8080/health"
interval = "1s"

[[properties]]
name = "high-availability"
type = "availability"
min_percent = 99.0

[[properties]]
name = "fast-recovery"
type = "recovery_time"
max_seconds = 15
"#,
            "ci",
        ),
    };

    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                anyhow::anyhow!(
                    "Output file '{}' already exists. Use a different path or remove it first.",
                    output
                )
            } else {
                anyhow::anyhow!("Failed to create '{}': {}", output, e)
            }
        })?;
    file.write_all(config.as_bytes())?;

    let preset_duration = match preset {
        InitPreset::Basic => "30s",
        InitPreset::Microservice => "5m",
        InitPreset::Stateful => "3m",
        InitPreset::Ci => "1m",
    };

    println!(
        "Created {} ({} preset) — edit it for your setup, then run:",
        output, preset_name
    );
    println!("  heisensim validate --config {}", output);
    println!(
        "  heisensim simulate --seed 0x42 --duration {} --config {}",
        preset_duration, output
    );

    Ok(())
}

async fn handle_validate(args: ValidateArgs) -> Result<()> {
    let config_str = std::fs::read_to_string(&args.config)
        .context(format!("Failed to read config file: {}", args.config))?;

    let val: toml::Value = toml::from_str(&config_str)
        .context(format!("Failed to parse config file: {}", args.config))?;

    let duration = match val.get("duration") {
        Some(v) => v.as_str().context("'duration' must be a string")?,
        None => "30s",
    };
    let warmup = match val.get("warmup") {
        Some(v) => v.as_str().context("'warmup' must be a string")?,
        None => "5s",
    };
    let namespace = match val.get("namespace") {
        Some(v) => v.as_str().context("'namespace' must be a string")?,
        None => "default",
    };

    parse_duration(duration).context("Invalid duration")?;
    parse_duration(warmup).context("Invalid warmup")?;

    if namespace.is_empty() {
        anyhow::bail!("Namespace cannot be empty");
    }

    let mut faults = Vec::new();
    if let Some(f_val) = val.get("faults") {
        let f = f_val.as_array().context("'faults' must be an array")?;
        for v in f {
            if let Some(s) = v.as_str() {
                faults.push(s.to_string());
            } else {
                anyhow::bail!("Faults array must contain only strings");
            }
        }
    }

    let mut probes_count = 0;
    let mut probe_names = Vec::new();
    if let Some(probes_val) = val.get("probes") {
        let p_arr = probes_val.as_array().context("'probes' must be an array")?;
        for p in p_arr {
            let name = match p.get("name") {
                Some(v) => v
                    .as_str()
                    .context("probe 'name' must be a string")?
                    .to_string(),
                None => "unnamed".to_string(),
            };
            if let Some(url_val) = p.get("url") {
                let url = url_val.as_str().context("probe 'url' must be a string")?;
                let valid_scheme = url.starts_with("http://") || url.starts_with("https://");
                if !valid_scheme {
                    anyhow::bail!("Invalid URL in probe '{}': {}", name, url);
                }
            }
            probe_names.push(name);
            probes_count += 1;
        }
    }

    let config: properties::PropertiesConfig =
        toml::from_str(&config_str).context("Failed to parse properties config")?;

    let _checker =
        properties::build_checker(&config.properties).context("Invalid properties definition")?;

    let properties_count = config.properties.len();
    let property_names: Vec<_> = config.properties.iter().map(|p| p.name.clone()).collect();

    println!("✅ Config valid: {}", args.config);
    println!("   Duration: {}", duration);
    println!("   Warmup: {}", warmup);
    println!("   Namespace: {}", namespace);

    if faults.is_empty() {
        println!("   Faults: 0");
    } else {
        println!("   Faults: {} ({})", faults.len(), faults.join(", "));
    }

    if probes_count == 0 {
        println!("   Probes: 0");
    } else {
        println!("   Probes: {} ({})", probes_count, probe_names.join(", "));
    }

    if properties_count == 0 {
        println!("   Properties: 0");
    } else {
        println!(
            "   Properties: {} ({})",
            properties_count,
            property_names.join(", ")
        );
    }

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
pub(crate) struct SimulationResult {
    pub seed: u64,
    pub total_faults: usize,
    pub total_failures: usize,
    pub duration_secs: f64,
    pub findings: Vec<String>,
    /// Property verdicts (empty if no properties configured)
    pub verdicts: Vec<heisensim_props::PropertyVerdict>,
    pub events: Vec<heisensim_timeline::event::TimelineEvent>,
}

#[allow(clippy::too_many_arguments)]
async fn run_single_simulation(
    client: &kube::Client,
    namespace: &str,
    seed: u64,
    duration: std::time::Duration,
    warmup: std::time::Duration,
    faults: &[String],
    inject_method: InjectMethod,
    crash_grace_period: Option<u32>,
    property_defs: &[properties::PropertyDef],
) -> Result<SimulationResult> {
    let handle = heisensim_timeline::TimelineHandle::new();

    // Discover and scrape probes
    let probes = heisensim_k8s::probe_scraper::scrape_probes(client, namespace).await?;

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
    let tracker = std::sync::Arc::new(heisensim_k8s::FaultTracker::new());
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
                let _ = fault_op
                    .inject_pod_crash(namespace, &target.name, crash_grace_period)
                    .await;
            }
            "eviction" => {
                let _ = fault_op.inject_eviction(namespace, &target.name).await;
            }
            "latency" => {
                let delay: u32 = rng.random_range(200..700);
                let jitter: u32 = rng.random_range(50..150);
                if let Ok(id) = fault_op
                    .inject_network_latency(namespace, &target.name, delay, jitter, 15.0)
                    .await
                {
                    let fault = heisensim_k8s::ActiveFault {
                        fault_id: id,
                        kind: heisensim_k8s::ActiveFaultKind::NetworkLatency,
                        namespace: namespace.to_string(),
                        pod_name: target.name.clone(),
                        injected_at: std::time::Instant::now(),
                    };
                    tracker.track(fault).await;
                    let t = tracker.clone();
                    let fo = fault_op.clone();
                    let ns = namespace.to_string();
                    let pn = target.name.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                        match fo.revert_network_latency(&ns, &pn, id).await {
                            Ok(()) => t.untrack(id).await,
                            Err(e) => warn!(fault_id = %id, error = %e, "Timed revert failed"),
                        }
                    });
                }
            }
            "partition" => {
                let other_pods: Vec<_> = ready_pods
                    .iter()
                    .filter(|p| p.name != target.name)
                    .collect();
                if let Some(other) = other_pods.first() {
                    let other_ip = other.pod_ip.as_deref().unwrap_or("10.0.0.1");
                    if let Ok(id) = fault_op
                        .inject_partition(namespace, &target.name, other_ip, 20.0)
                        .await
                    {
                        let fault = heisensim_k8s::ActiveFault {
                            fault_id: id,
                            kind: heisensim_k8s::ActiveFaultKind::Partition {
                                target_ip: other_ip.to_string(),
                            },
                            namespace: namespace.to_string(),
                            pod_name: target.name.clone(),
                            injected_at: std::time::Instant::now(),
                        };
                        tracker.track(fault).await;
                        let t = tracker.clone();
                        let fo = fault_op.clone();
                        let ns = namespace.to_string();
                        let pn = target.name.clone();
                        let ip = other_ip.to_string();
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                            match fo.revert_partition(&ns, &pn, &ip, id).await {
                                Ok(()) => t.untrack(id).await,
                                Err(e) => warn!(fault_id = %id, error = %e, "Timed revert failed"),
                            }
                        });
                    }
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
        duration_secs: duration.as_secs_f64(),
        findings,
        verdicts,
        events,
    })
}

async fn handle_explore(args: ExploreArgs) -> Result<i32> {
    anyhow::ensure!(args.parallel > 0, "--parallel must be at least 1");

    if args.mock {
        tracing::warn!(
            "--mock is deprecated. Use `heisensim simulate` for a fully deterministic in-memory simulation engine instead."
        );
        return mock::handle_mock_explore(&args).await;
    }

    if args.simulate {
        if args.parallel != 3 {
            warn!(
                "--parallel is not used with --simulate (simulation is single-threaded and instant)"
            );
        }
        if args.namespace != "default" {
            warn!("--namespace is not used with --simulate (no cluster connection)");
        }
        return handle_simulate_explore(&args).await;
    }

    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;

    let resolved_faults = resolve_faults(&args.faults, args.profile.as_ref());

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
            resolved_faults.join(",")
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
        let config: properties::PropertiesConfig =
            toml::from_str(&config_str).context("Failed to parse properties config")?;
        properties::resolve_with_template(
            args.property_template.as_ref(),
            &config.properties,
            config.template.as_ref(),
        )
    } else if let Some(ref tmpl) = args.property_template {
        tmpl.to_property_defs()
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
            let faults = resolved_faults.clone();
            let method = args.inject_method;
            let cgp = args.crash_grace_period;
            let prop_defs = property_defs.clone();

            handles.push(tokio::spawn(async move {
                info!(seed = seed, "🔬 Starting seed 0x{:04X}...", seed);
                let result = run_single_simulation(
                    &client, &ns, seed, duration, warmup, &faults, method, cgp, &prop_defs,
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
                            let mut nearest_failing = seed;

                            while low <= high {
                                let mid = low + (high - low) / 2;
                                println!("    Checking candidate seed {}...", mid);
                                let b_result = run_single_simulation(
                                    &client,
                                    &args.namespace,
                                    mid,
                                    duration,
                                    warmup,
                                    &resolved_faults,
                                    args.inject_method,
                                    args.crash_grace_period,
                                    &property_defs,
                                )
                                .await;
                                match b_result {
                                    Ok(res) => {
                                        let b_fail = !res.findings.is_empty()
                                            || res.verdicts.iter().any(|v| !v.passed);
                                        if b_fail {
                                            nearest_failing = mid;
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
                                "  Nearest failing seed: {} (bisected from {})",
                                nearest_failing, seed
                            );
                            if nearest_failing != seed {
                                interesting_seeds.push(nearest_failing);
                            }
                        }
                    } else {
                        last_known_good = Some(seed);
                    }

                    if strategy == heisensim_fault::explorer::ExploreStrategy::Coverage {
                        explorer.record_coverage(fault_target_combos(&result.events));
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

/// Extract unique (fault_kind, target) combinations from timeline events for coverage tracking.
fn fault_target_combos(
    events: &[heisensim_timeline::event::TimelineEvent],
) -> std::collections::HashSet<(String, String)> {
    let mut combos = std::collections::HashSet::new();
    for ev in events {
        if let heisensim_timeline::EventKind::FaultInjected {
            fault_kind, target, ..
        } = &ev.kind
        {
            combos.insert((fault_kind.clone(), target.clone()));
        }
    }
    combos
}

async fn handle_simulate_explore(args: &ExploreArgs) -> Result<i32> {
    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;
    let resolved_faults = resolve_faults(&args.faults, args.profile.as_ref());

    // Load and validate property definitions
    let property_defs: Vec<properties::PropertyDef> = if let Some(ref config_path) = args.config {
        let config_str =
            std::fs::read_to_string(config_path).context("Failed to read config file")?;
        let config: properties::PropertiesConfig =
            toml::from_str(&config_str).context("Failed to parse properties config")?;
        properties::resolve_with_template(
            args.property_template.as_ref(),
            &config.properties,
            config.template.as_ref(),
        )
    } else if let Some(ref tmpl) = args.property_template {
        tmpl.to_property_defs()
    } else {
        Vec::new()
    };

    if args.output == OutputFormat::Text {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!(
            "║  HEISENSIM EXPLORE (simulate)          {} seeds, {}s each   ║",
            args.seeds,
            duration.as_secs()
        );
        println!("╠══════════════════════════════════════════════════════════════╣");
    }

    let strategy = match args.explore_strategy {
        ExploreStrategyArg::Random => heisensim_fault::explorer::ExploreStrategy::Random,
        ExploreStrategyArg::Coverage => heisensim_fault::explorer::ExploreStrategy::Coverage,
        ExploreStrategyArg::Sequential => heisensim_fault::explorer::ExploreStrategy::Sequential,
    };
    let mut explorer = heisensim_fault::explorer::StrategicExplorer::new(strategy, args.start_seed);

    let start = std::time::Instant::now();
    let mut interesting_seeds: Vec<u64> = Vec::new();
    let mut all_results: Vec<(u64, crate::dst::DstResult)> = Vec::new();
    let mut any_prop_failures = false;
    let mut last_known_good: Option<u64> = None;

    for _ in 0..args.seeds {
        let seed = explorer.next_seed();
        let config = crate::dst::DstConfig {
            seed,
            duration: heisensim_core::types::VirtualTime::from_millis(duration.as_millis() as u64),
            warmup: heisensim_core::types::VirtualTime::from_millis(warmup.as_millis() as u64),
            faults: resolved_faults.clone(),
            pod_count: 3,
            probe_interval: heisensim_core::types::VirtualTime::from_secs(5),
            property_defs: property_defs.clone(),
        };

        let result = match crate::dst::run(config) {
            Ok(r) => r,
            Err(e) => {
                if args.output == OutputFormat::Text {
                    println!("  ❌ seed 0x{:04X}  │  ERROR: {}", seed, e);
                }
                continue;
            }
        };

        let has_failures = result.total_failures > 0;
        let props_failed = result.verdicts.iter().any(|v| !v.passed);
        if props_failed {
            any_prop_failures = true;
        }
        if has_failures || props_failed {
            interesting_seeds.push(seed);
        } else {
            last_known_good = Some(seed);
        }

        if args.output == OutputFormat::Text {
            let icon = if props_failed {
                "❌"
            } else if has_failures {
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
                "  {} seed 0x{:04X}  │  faults: {}  │  failures: {}  │  hash: {:016x}{}",
                icon, seed, result.total_faults, result.total_failures, result.hash, props_str
            );
        }

        // Record coverage for coverage strategy
        if strategy == heisensim_fault::explorer::ExploreStrategy::Coverage {
            explorer.record_coverage(fault_target_combos(&result.events));
        }

        all_results.push((seed, result));
    }

    let mut bisected_seeds: Vec<u64> = Vec::new();
    if args.bisect && !interesting_seeds.is_empty() {
        if args.output == OutputFormat::Text {
            println!("\n🔍 Bisecting interesting seeds...");
        }
        for &failing_seed in &interesting_seeds {
            let good_seed = last_known_good.unwrap_or(0);
            if good_seed >= failing_seed {
                bisected_seeds.push(failing_seed);
                continue;
            }

            let mut low = good_seed;
            let mut high = failing_seed;
            let mut nearest_failing = failing_seed;

            while low < high {
                let mid = low + (high - low) / 2;
                let config = crate::dst::DstConfig {
                    seed: mid,
                    duration: heisensim_core::types::VirtualTime::from_millis(
                        duration.as_millis() as u64
                    ),
                    warmup: heisensim_core::types::VirtualTime::from_millis(
                        warmup.as_millis() as u64
                    ),
                    faults: resolved_faults.clone(),
                    pod_count: 3,
                    probe_interval: heisensim_core::types::VirtualTime::from_secs(5),
                    property_defs: property_defs.clone(),
                };
                match crate::dst::run(config) {
                    Ok(result) => {
                        let fails =
                            result.total_failures > 0 || result.verdicts.iter().any(|v| !v.passed);
                        if fails {
                            nearest_failing = mid;
                            high = mid;
                        } else {
                            low = mid + 1;
                        }
                    }
                    Err(_) => {
                        high = mid;
                    }
                }
            }

            if args.output == OutputFormat::Text {
                if nearest_failing != failing_seed {
                    println!(
                        "  Bisected seed 0x{:04X} → minimal failing seed: 0x{:04X}",
                        failing_seed, nearest_failing
                    );
                } else {
                    println!("  Seed 0x{:04X} is already minimal", failing_seed);
                }
            }
            bisected_seeds.push(nearest_failing);
        }
        bisected_seeds.sort_unstable();
        bisected_seeds.dedup();
    }

    let wall_time = start.elapsed();

    if args.output == OutputFormat::Text {
        println!();
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  EXPLORE SUMMARY (deterministic)                             ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!(
            "║  Seeds: {:4}  │  Interesting: {:4}  │  Wall time: {:.1}ms       ║",
            all_results.len(),
            interesting_seeds.len(),
            wall_time.as_secs_f64() * 1000.0
        );
        println!("╚══════════════════════════════════════════════════════════════╝");

        if !interesting_seeds.is_empty() {
            println!();
            println!("🐛 Interesting seeds:");
            for &seed in &interesting_seeds {
                println!(
                    "  heisensim simulate --seed 0x{:04X} --duration {}",
                    seed, args.duration
                );
            }
        }

        if args.explore_strategy == ExploreStrategyArg::Coverage {
            println!("\n{}", explorer.coverage_summary());
        }
    }

    if args.output == OutputFormat::Json {
        let json_results: Vec<_> = all_results
            .iter()
            .map(|(seed, r)| {
                serde_json::json!({
                    "seed": seed,
                    "seed_hex": format!("0x{:04X}", seed),
                    "hash": format!("{:016x}", r.hash),
                    "total_faults": r.total_faults,
                    "total_failures": r.total_failures,
                    "passed": r.verdicts.iter().all(|v| v.passed),
                    "verdicts": r.verdicts,
                })
            })
            .collect();
        let output = serde_json::json!({
            "seeds_tested": all_results.len(),
            "interesting_seeds": interesting_seeds,
            "bisected_seeds": bisected_seeds,
            "all_passed": !any_prop_failures,
            "wall_time_ms": wall_time.as_secs_f64() * 1000.0,
            "results": json_results,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    if any_prop_failures { Ok(1) } else { Ok(0) }
}

/// Handle the `time-warp` subcommand — inject time manipulation into a running pod.
///
/// This uses `kubectl debug` to inject an ephemeral container with `CAP_SYS_PTRACE`,
/// then runs `heisensim-inject` inside it to patch the target process's vDSO.
async fn handle_time_warp(args: TimeWarpArgs) -> Result<()> {
    use heisensim_intercept::vdso::control::{parse_speed, parse_time_offset};

    // Validate inputs before touching K8s
    let (offset_secs, offset_nanos) = parse_time_offset(&args.offset)?;
    let (speed_num, speed_den) = parse_speed(&args.speed)?;

    info!(
        "⏰ Time warp: pod={}, ns={}, offset={}, speed={}",
        args.pod, args.namespace, args.offset, args.speed
    );
    info!(
        "   Resolved: {}s {}ns, speed {}/{}x",
        offset_secs, offset_nanos, speed_num, speed_den
    );

    if args.revert {
        info!("🔄 Reverting time warp on pod {}", args.pod);
        // Revert would kill the debug container or send a revert signal
        // For now, we print instructions
        println!("To revert, delete the debug container:");
        println!(
            "  kubectl delete pod {} -n {} --grace-period=0",
            args.pod, args.namespace
        );
        return Ok(());
    }

    // Build the heisensim-inject command for the ephemeral container
    let inject_cmd = format!(
        "heisensim-inject --pid 1 --offset '{}' --speed '{}'",
        args.offset, args.speed
    );

    info!("🚀 Launching ephemeral container with CAP_SYS_PTRACE...");
    info!("   Command: {}", inject_cmd);

    // Use kubectl debug to inject an ephemeral container
    let container_name = "heisensim-timewarp";
    // --target shares PID namespace with the workload container
    let target_container = args.container.as_deref().unwrap_or(&args.pod);
    let target_flag = format!("--target={}", target_container);
    let container_flag = format!("--container={}", container_name);
    let mut cmd = tokio::process::Command::new("kubectl");
    cmd.args([
        "debug",
        "-n",
        &args.namespace,
        &args.pod,
        "--image=ghcr.io/heisensim/heisensim:latest",
        &container_flag,
        &target_flag,
        "--",
        "heisensim-inject",
        "--pid",
        "1",
        "--offset",
        &args.offset,
        "--speed",
        &args.speed,
    ]);

    let output = cmd.output().await?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("✅ Time warp applied successfully");
        if !stdout.is_empty() {
            println!("{}", stdout);
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to inject time warp: {}", stderr.trim());
    }

    Ok(())
}

/// Handle the `process-fault` subcommand — inject process-level faults via ptrace.
///
/// Uses `kubectl debug` to inject an ephemeral container with `CAP_SYS_PTRACE`,
/// then runs `heisensim-inject --fault <type>` inside it to intercept syscalls.
async fn handle_process_fault(args: ProcessFaultArgs) -> Result<()> {
    let fault_str = match args.fault {
        ProcessFaultType::ConnectError => "connect-error",
        ProcessFaultType::FdExhaustion => "fd-exhaustion",
        ProcessFaultType::ConnectLatency => "connect-latency",
    };

    info!(
        "🔌 Process fault: pod={}, ns={}, fault={}, duration={}",
        args.pod, args.namespace, fault_str, args.duration
    );

    // Build inject args
    let container_name = format!("heisensim-fault-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let target_container = args.container.as_deref().unwrap_or(&args.pod);
    let target_flag = format!("--target={}", target_container);
    let container_flag = format!("--container={}", container_name);
    let errno_str = args.errno.to_string();

    let mut cmd_args = vec![
        "debug",
        "-n",
        &args.namespace,
        &args.pod,
        "--image=ghcr.io/heisensim/heisensim:latest",
        &container_flag,
        &target_flag,
        "--",
        "heisensim-inject",
        "--pid",
        "1",
        "--fault",
        fault_str,
        "--duration",
        &args.duration,
        "--errno",
        &errno_str,
    ];

    let port_str;
    if let Some(port) = args.port {
        port_str = port.to_string();
        cmd_args.push("--port");
        cmd_args.push(&port_str);
    }

    let latency_str = args.latency.to_string();
    if matches!(args.fault, ProcessFaultType::ConnectLatency) {
        cmd_args.push("--latency");
        cmd_args.push(&latency_str);
    }

    info!("🚀 Launching ephemeral container with CAP_SYS_PTRACE...");

    let output = tokio::process::Command::new("kubectl")
        .args(&cmd_args)
        .output()
        .await?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("✅ Process fault injection complete");
        if !stdout.is_empty() {
            println!("{}", stdout);
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to inject process fault: {}", stderr.trim());
    }

    Ok(())
}

/// Handle the `diverge run` subcommand — chaos testing against a Diverge preview environment.
///
/// Discovers the preview environment via ConnectRPC, resolves routing headers,
/// validates namespace fencing, and runs chaos with blast-radius targeting.
async fn handle_diverge_run(args: DivergeRunArgs) -> Result<i32> {
    use heisensim_diverge::DivergeClient;
    use heisensim_k8s::fencing;

    info!(
        env = args.env_name.as_str(),
        url = args.url.as_str(),
        "🔗 Connecting to Diverge API..."
    );

    // 1. Resolve URL from env var if using default
    let url = if args.url == "http://localhost:8080" {
        std::env::var("DIVERGE_URL").unwrap_or(args.url)
    } else {
        args.url
    };

    // 2. Create client with auth chain
    let client = DivergeClient::new(&url, args.token.clone());

    // 3. Parse wait timeout
    let wait_secs = args
        .wait
        .trim_end_matches('s')
        .parse::<u64>()
        .map_err(|_| {
            anyhow::anyhow!(
                "Invalid --wait value '{}'. Expected a duration like '30s' or '120s'.",
                args.wait
            )
        })?;
    let wait_timeout = std::time::Duration::from_secs(wait_secs);

    // 4. Resolve namespace (required — no misleading "default" fallback)
    let namespace = args.namespace.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Namespace is required. Use --namespace to specify the Diverge environment namespace."
        )
    })?;

    // 4. Wait for environment to become Running
    let env = client
        .wait_for_running(namespace, &args.env_name, wait_timeout)
        .await?;

    // 5. Resolve context
    let ctx = client.resolve_context(&env);

    // 6. Validate namespace fencing
    fencing::validate_namespace(&ctx.namespace)?;

    // 7. Determine target services (blast-radius default vs --all-services)
    let target_services = if args.all_services {
        &ctx.services
    } else if !ctx.changed_services.is_empty() {
        info!(
            changed = ?ctx.changed_services,
            "Targeting changed services only (use --all-services to override)"
        );
        &ctx.changed_services
    } else {
        info!("No changed services detected, targeting all services");
        &ctx.services
    };

    info!(
        env = args.env_name.as_str(),
        namespace = ctx.namespace.as_str(),
        header = format!("{}: {}", ctx.header_key, ctx.header_value).as_str(),
        services = ?target_services,
        url = ctx.url.as_deref().unwrap_or("(none)"),
        soft_fail = args.soft_fail,
        "✅ Diverge environment discovered"
    );

    // 8. Warmup ping — wake scale-to-zero pods
    if let Some(url) = &ctx.url {
        info!(url = url.as_str(), "Sending warmup ping...");
        let warmup_client = reqwest::Client::new();
        match warmup_client
            .get(url.as_str())
            .header(&ctx.header_key, &ctx.header_value)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) => {
                info!(status = %resp.status(), "Warmup ping complete");
            }
            Err(e) => {
                warn!("Warmup ping failed (pods may need more time): {}", e);
            }
        }
    }

    // 9. Connect to K8s
    info!("Connecting to Kubernetes cluster...");
    let k8s_client = kube::Client::try_default()
        .await
        .context("Failed to connect to Kubernetes. Is a cluster running?")?;
    info!("Connected to cluster.");

    // 10. Discover pods — filtered by target services (blast-radius targeting)
    info!(
        namespace = ctx.namespace.as_str(),
        services = ?target_services,
        "Discovering pods for target services..."
    );
    let pods = heisensim_k8s::discovery::discover_pods_for_services(
        &k8s_client,
        &ctx.namespace,
        target_services,
    )
    .await?;
    info!("Found {} pods for target services.", pods.len());
    for pod in &pods {
        info!("  Pod: {} (ready: {})", pod.name, pod.is_ready);
    }

    if pods.is_empty() {
        warn!("No pods found for target services! Check that the services are deployed.");
        return if args.soft_fail { Ok(0) } else { Ok(1) };
    }

    // 11. Scrape probes and inject Diverge routing headers
    info!("Scraping probe configs from pod specs...");
    let raw_probes =
        heisensim_k8s::probe_scraper::scrape_probes(&k8s_client, &ctx.namespace).await?;

    let routing_headers = {
        let mut h = std::collections::HashMap::new();
        h.insert(ctx.header_key.clone(), ctx.header_value.clone());
        h
    };

    let probes: Vec<_> = raw_probes
        .into_iter()
        .map(|p| p.with_extra_headers(&routing_headers))
        .collect();

    info!(
        "Discovered {} probes (with {} routing header injected):",
        probes.len(),
        ctx.header_key
    );
    for p in &probes {
        info!("  - {}", p.name());
    }

    if probes.is_empty() {
        warn!("No probes found! Make sure pods have readinessProbe or livenessProbe configured.");
    }

    // 12. Create timeline and start probe runner
    let duration = parse_duration(&args.duration)?;
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    let handle = heisensim_timeline::TimelineHandle::new();

    handle.emit(heisensim_timeline::EventKind::SimulationStarted {
        seed,
        duration_secs: duration.as_secs_f64(),
    });

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let probe_runner = heisensim_probe::ProbeRunner::new(probes, handle.clone());
    let probe_handle = tokio::spawn(async move {
        if let Err(e) = probe_runner.run(cancel_rx) {
            warn!("Probe runner error: {}", e);
        }
    });

    // 13. Warmup — let probes stabilize before injecting faults
    let diverge_warmup = std::time::Duration::from_secs(10);
    info!("Warming up for 10s (letting probes stabilize)...");
    tokio::time::sleep(diverge_warmup).await;
    info!("Warmup complete.");

    // Capture baseline snapshot (if --baseline is enabled)
    let baseline_snapshot = if args.baseline {
        let events = handle.events();
        match heisensim_props::capture_baseline(&events, diverge_warmup) {
            Some(snapshot) => {
                info!(
                    "📊 Baseline captured: {} probes, {} total samples",
                    snapshot.probes.len(),
                    snapshot.total_samples
                );
                if let Some(ref path) = args.export_baseline {
                    let json = serde_json::to_string_pretty(&snapshot)?;
                    std::fs::write(path, &json)?;
                    info!("📁 Baseline exported to {}", path.display());
                }
                Some(snapshot)
            }
            None => {
                anyhow::bail!(
                    "No probe data captured during warmup — cannot establish baseline.\n\
                     Check that probes are configured correctly and warmup duration is sufficient."
                );
            }
        }
    } else {
        None
    };

    info!("Starting fault injection.");

    // 14. Fault injection loop
    let fault_op = heisensim_k8s::FaultOperator::with_method(
        k8s_client.clone(),
        handle.clone(),
        heisensim_k8s::InjectMethod::Debug,
    );

    // Initialize fault tracker for graceful shutdown
    let tracker = std::sync::Arc::new(heisensim_k8s::FaultTracker::new());

    // SIGINT handler — revert all active faults before exiting
    let shutdown_tracker = tracker.clone();
    let shutdown_fault_op = fault_op.clone();
    let shutdown_cancel = cancel_tx.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            warn!("⚠️  SIGINT received — reverting all active faults...");
            let _ = shutdown_cancel.send(true);
            let results = shutdown_tracker.revert_all(&shutdown_fault_op).await;
            let failed = results.iter().filter(|(_, r)| r.is_err()).count();
            if failed > 0 {
                warn!(
                    "⚠️  {} faults could not be reverted — manual intervention may be required!",
                    failed
                );
            }
            std::process::exit(130);
        }
    });

    let resolved_faults = resolve_faults(&args.faults, None);
    let mut rng = StdRng::seed_from_u64(seed);
    let fault_interval = duration / 4;
    let mut elapsed = std::time::Duration::ZERO;

    while elapsed < duration {
        tokio::time::sleep(fault_interval).await;
        elapsed += fault_interval;

        if elapsed >= duration {
            break;
        }

        // Re-discover live pods for target services
        let live_pods = heisensim_k8s::discovery::discover_pods_for_services(
            &k8s_client,
            &ctx.namespace,
            target_services,
        )
        .await?;
        let ready_pods: Vec<_> = live_pods.iter().filter(|p| p.is_ready).collect();
        if ready_pods.is_empty() {
            warn!("No ready pods to target, skipping fault.");
            continue;
        }

        let target_idx = rng.random_range(0..ready_pods.len());
        let target = &ready_pods[target_idx];

        let fault_idx = rng.random_range(0..resolved_faults.len());
        let fault_type = &resolved_faults[fault_idx];

        match fault_type.as_str() {
            "latency" => {
                info!("🌐 Injecting network latency on {}", target.name);
                let delay: u32 = rng.random_range(200..700);
                let jitter: u32 = rng.random_range(50..150);
                match fault_op
                    .inject_network_latency(&ctx.namespace, &target.name, delay, jitter, 15.0)
                    .await
                {
                    Ok(id) => {
                        info!("  Fault {}: +{}ms (jitter {}ms) for 15s", id, delay, jitter);
                        let fault = heisensim_k8s::ActiveFault {
                            fault_id: id,
                            kind: heisensim_k8s::ActiveFaultKind::NetworkLatency,
                            namespace: ctx.namespace.clone(),
                            pod_name: target.name.clone(),
                            injected_at: std::time::Instant::now(),
                        };
                        tracker.track(fault).await;
                        let t = tracker.clone();
                        let fo = fault_op.clone();
                        let ns = ctx.namespace.clone();
                        let pn = target.name.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                            match fo.revert_network_latency(&ns, &pn, id).await {
                                Ok(()) => t.untrack(id).await,
                                Err(e) => {
                                    warn!(fault_id = %id, error = %e, "Timed revert failed; will retry at shutdown")
                                }
                            }
                        });
                    }
                    Err(e) => warn!("  Failed to inject latency: {}", e),
                }
            }
            "partition" => {
                info!("🔌 Injecting network partition on {}", target.name);
                let other_pods: Vec<_> = ready_pods
                    .iter()
                    .filter(|p| p.name != target.name)
                    .collect();
                if let Some(other) = other_pods.first() {
                    let other_ip = other.pod_ip.as_deref().unwrap_or("10.0.0.1");
                    match fault_op
                        .inject_partition(&ctx.namespace, &target.name, other_ip, 20.0)
                        .await
                    {
                        Ok(id) => {
                            info!("  Fault {}: {} ↛ {} for 20s", id, target.name, other.name);
                            let fault = heisensim_k8s::ActiveFault {
                                fault_id: id,
                                kind: heisensim_k8s::ActiveFaultKind::Partition {
                                    target_ip: other_ip.to_string(),
                                },
                                namespace: ctx.namespace.clone(),
                                pod_name: target.name.clone(),
                                injected_at: std::time::Instant::now(),
                            };
                            tracker.track(fault).await;
                            let t = tracker.clone();
                            let fo = fault_op.clone();
                            let ns = ctx.namespace.clone();
                            let pn = target.name.clone();
                            let ip = other_ip.to_string();
                            tokio::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                                match fo.revert_partition(&ns, &pn, &ip, id).await {
                                    Ok(()) => t.untrack(id).await,
                                    Err(e) => {
                                        warn!(fault_id = %id, error = %e, "Timed revert failed; will retry at shutdown")
                                    }
                                }
                            });
                        }
                        Err(e) => warn!("  Failed to inject partition: {}", e),
                    }
                } else {
                    warn!("  Only one pod, skipping partition.");
                }
            }
            "dns" => {
                info!("🌐 Injecting DNS blackhole on {}", target.name);
                match fault_op
                    .inject_dns_failure(&ctx.namespace, &target.name, 15.0)
                    .await
                {
                    Ok(id) => {
                        info!("  Fault {}: DNS blocked for 15s", id);
                        let fault = heisensim_k8s::ActiveFault {
                            fault_id: id,
                            kind: heisensim_k8s::ActiveFaultKind::DnsFailure,
                            namespace: ctx.namespace.clone(),
                            pod_name: target.name.clone(),
                            injected_at: std::time::Instant::now(),
                        };
                        tracker.track(fault).await;
                        let t = tracker.clone();
                        let fo = fault_op.clone();
                        let ns = ctx.namespace.clone();
                        let pn = target.name.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                            match fo.revert_dns_failure(&ns, &pn, id).await {
                                Ok(()) => t.untrack(id).await,
                                Err(e) => {
                                    warn!(fault_id = %id, error = %e, "Timed revert failed; will retry at shutdown")
                                }
                            }
                        });
                    }
                    Err(e) => warn!("  Failed to inject DNS fault: {}", e),
                }
            }
            other => {
                warn!("Unknown or unsupported fault type '{}', skipping.", other);
            }
        }
    }

    // 15. Revert any remaining active faults
    let active = tracker.active_count().await;
    if active > 0 {
        info!(
            "🧹 Cleaning up {} active fault(s) before shutdown...",
            active
        );
        tracker.revert_all(&fault_op).await;
    }

    // 16. Stop probes
    info!("Stopping probes...");
    let _ = cancel_tx.send(true);
    let _ = probe_handle.await;

    // 16. Evaluate results
    let events = handle.events();
    let summary = heisensim_timeline::query::summary(&events);
    handle.emit(heisensim_timeline::EventKind::SimulationEnded {
        total_faults: summary.total_faults,
        total_failures: summary.total_failures,
    });

    let final_events = handle.events();
    let mut exit_code = if summary.total_failures > 0 { 1 } else { 0 };

    // Baseline diff properties (if --baseline was enabled) — evaluate BEFORE report
    let mut baseline_verdicts = Vec::new();
    if let Some(ref snapshot) = baseline_snapshot {
        info!("📊 Evaluating baseline diff properties...");
        let latency_prop = heisensim_props::BaselineLatencyDiff::new(
            snapshot.clone(),
            diverge_warmup,
            args.max_latency_multiplier,
        );
        let avail_prop = heisensim_props::BaselineAvailabilityDiff::new(
            snapshot.clone(),
            diverge_warmup,
            args.max_availability_drop,
        );

        let lat_verdict = latency_prop.evaluate(&final_events);
        let avail_verdict = avail_prop.evaluate(&final_events);

        if !lat_verdict.passed || !avail_verdict.passed {
            warn!("Baseline diff properties FAILED.");
            exit_code = 1;
        }

        baseline_verdicts.push(lat_verdict);
        baseline_verdicts.push(avail_verdict);
    }

    // 17. Output results
    match args.output {
        OutputFormat::Text => {
            report::render_terminal_report(&final_events);
            if !baseline_verdicts.is_empty() {
                properties::print_verdicts(&baseline_verdicts);
            }
            info!(
                seed = seed,
                faults = summary.total_faults,
                failures = summary.total_failures,
                env = args.env_name.as_str(),
                "🏁 Diverge chaos test complete"
            );
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "seed": seed,
                "environment": args.env_name,
                "namespace": ctx.namespace,
                "header": format!("{}: {}", ctx.header_key, ctx.header_value),
                "target_services": target_services,
                "duration_secs": duration.as_secs_f64(),
                "total_faults": summary.total_faults,
                "total_failures": summary.total_failures,
                "baseline_verdicts": baseline_verdicts,
                "passed": exit_code == 0,
                "soft_fail": args.soft_fail,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            report::render_terminal_report(&final_events);
            if !baseline_verdicts.is_empty() {
                properties::print_verdicts(&baseline_verdicts);
            }
        }
    }

    // 18. Soft-fail: override exit code to 0
    if args.soft_fail && exit_code != 0 {
        info!(
            "Soft-fail mode: {} failures detected but exiting with code 0.",
            summary.total_failures
        );
        Ok(0)
    } else {
        Ok(exit_code)
    }
}
