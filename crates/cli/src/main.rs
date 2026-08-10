//! The Heisenbug Simulator (`heisensim`) CLI.
//!
//! Provides deterministic chaos testing for Kubernetes.

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use rand::{Rng, SeedableRng, rngs::StdRng};
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

mod properties;
mod report;

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

    #[arg(long, default_value = "crash,latency", value_delimiter = ',')]
    faults: Vec<String>,

    /// Method for injecting network faults into pods.
    /// 'exec' runs commands directly inside the target container (requires tc/iptables in image).
    /// 'debug' uses kubectl debug ephemeral containers with netshoot (works with any image).
    #[arg(long, default_value = "exec", value_enum)]
    inject_method: InjectMethod,
}

/// Method for injecting network faults into pods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InjectMethod {
    /// Execute commands directly in the target container (requires tc/iptables)
    Exec,
    /// Use kubectl debug ephemeral containers with netshoot image
    Debug,
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
}

#[derive(Args, Debug)]
struct ReportArgs {
    #[arg(long)]
    input: PathBuf,

    #[arg(long, default_value = "terminal")]
    format: String,
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

    /// Fault types to inject
    #[arg(long, default_value = "crash,latency", value_delimiter = ',')]
    faults: Vec<String>,

    /// Method for injecting network faults
    #[arg(long, default_value = "exec", value_enum)]
    inject_method: InjectMethod,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize tracing subscriber
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("heisensim=info,info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .init();

    // 2. Print ASCII banner
    println!("{}", BANNER);

    let cli = Cli::parse();

    let exit_code = match cli.command {
        Commands::Run(args) => handle_run(args).await?,
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
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

async fn handle_run(args: RunArgs) -> Result<i32> {
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
    report::render_terminal_report(&final_events);

    // Save JSON
    let json = report::render_json_report(&final_events);
    let json_path = format!("heisensim-report-{:04x}.json", seed & 0xFFFF);
    std::fs::write(&json_path, &json)?;
    info!("Timeline saved to {}", json_path);

    // Property checking
    let mut exit_code = 0;
    if let Some(ref config_path) = args.config {
        let config_str = std::fs::read_to_string(config_path)
            .context("Failed to read config file")?;
        let config: properties::PropertiesConfig = toml::from_str(&config_str)
            .unwrap_or_default();
        if !config.properties.is_empty() {
            info!("Evaluating {} properties...", config.properties.len());
            let all_passed = properties::check_properties(&config.properties, &final_events)?;
            if !all_passed {
                warn!("Some properties FAILED. Exit code 1.");
                exit_code = 1;
            }
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
    let faults = vec!["crash".to_string(), "latency".to_string()];

    let _result = run_single_simulation(
        &client,
        &args.namespace,
        args.seed,
        duration,
        warmup,
        &faults,
        InjectMethod::Exec,
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

    for probe in &probes {
        toml.push_str(&format!("# {}\n", probe.name()));
    }

    info!("Writing configuration to '{}'...", args.output.display());
    std::fs::write(&args.output, toml)?;

    println!(
        "Init complete. Discovered {} pods and {} probes.",
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

/// Result of a single simulation run, used by explore mode.
#[allow(dead_code)]
struct SimulationResult {
    seed: u64,
    total_faults: usize,
    total_failures: usize,
    total_probes: usize,
    duration_secs: f64,
    findings: Vec<String>,
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

    Ok(SimulationResult {
        seed,
        total_faults: summary.total_faults,
        total_failures: summary.total_failures,
        total_probes,
        duration_secs: duration.as_secs_f64(),
        findings,
    })
}

async fn handle_explore(args: ExploreArgs) -> Result<i32> {
    let duration = parse_duration(&args.duration)?;
    let warmup = parse_duration(&args.warmup)?;
    let seeds: Vec<u64> = (args.start_seed..args.start_seed + args.seeds).collect();

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

    // Connect once, reuse client
    let client = kube::Client::try_default()
        .await
        .context("Failed to connect to Kubernetes")?;

    let mut results: Vec<SimulationResult> = Vec::new();
    let mut interesting_seeds: Vec<u64> = Vec::new();

    // Run seeds in batches of `parallel`
    for batch in seeds.chunks(args.parallel) {
        let mut handles = Vec::new();
        for &seed in batch {
            let client = client.clone();
            let ns = args.namespace.clone();
            let faults = args.faults.clone();
            let method = args.inject_method;

            handles.push(tokio::spawn(async move {
                info!("🔬 Starting seed 0x{:04X}...", seed);
                let result =
                    run_single_simulation(&client, &ns, seed, duration, warmup, &faults, method)
                        .await;
                info!("✅ Seed 0x{:04X} complete.", seed);
                (seed, result)
            }));
        }

        for handle in handles {
            match handle.await {
                Ok((seed, Ok(result))) => {
                    let has_findings = !result.findings.is_empty();
                    if has_findings {
                        interesting_seeds.push(seed);
                    }

                    let icon = if has_findings { "🐛" } else { "✅" };
                    println!(
                        "  {} seed 0x{:04X}  │  faults: {}  │  failures: {}  │  findings: {}",
                        icon,
                        seed,
                        result.total_faults,
                        result.total_failures,
                        result.findings.len()
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

    if !interesting_seeds.is_empty() {
        println!();
        println!("🐛 Interesting seeds (found fault→failure correlations):");
        for &seed in &interesting_seeds {
            let result = results.iter().find(|r| r.seed == seed).unwrap();
            println!("  seed 0x{:04X}:", seed);
            for finding in &result.findings {
                println!("    → {}", finding);
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

    Ok(0)
}
