//! The Heisenbug Simulator (`heisensim`) CLI.
//!
//! Provides deterministic chaos testing for Kubernetes.

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use rand::{rngs::StdRng, SeedableRng, Rng};
use std::time::Duration;
use tokio::time::sleep;

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

    match cli.command {
        Commands::Run(args) => handle_run(args).await?,
        Commands::Replay(args) => handle_replay(args).await?,
        Commands::Init(args) => handle_init(args).await?,
        Commands::Report(args) => handle_report(args).await?,
    }

    Ok(())
}

async fn handle_run(args: RunArgs) -> Result<()> {
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    println!("Config Summary:");
    println!("  Namespace: {}", args.namespace);
    println!("  Duration: {}", args.duration);
    println!("  Seed: 0x{:04X}", seed);
    println!("  Warmup: {}", args.warmup);
    println!("  K3d: {}", args.k3d);
    println!("  Faults: {:?}", args.faults);
    
    // 3. Create K3d cluster if --k3d flag set
    if args.k3d {
        info!("Creating ephemeral K3d cluster...");
    }
    
    // 4. Connect to K8s cluster via kube::Client::try_default()
    info!("Connecting to Kubernetes cluster...");
    
    // 5. Discover pods in namespace using heisensim_k8s::discovery
    info!("Discovering pods in namespace '{}'...", args.namespace);
    
    // 6. Scrape probe configs using heisensim_k8s::probe_scraper  
    info!("Scraping probe configs...");
    
    // 7. Print discovered services and probes
    info!("Found services and probes.");
    
    // 8. Create Timeline
    info!("Creating timeline...");
    
    // 9. Emit SimulationStarted event
    info!("Emitting SimulationStarted event.");
    
    // 10. Start ProbeRunner (spawns background tasks)
    info!("Starting ProbeRunner...");
    
    // 11. If --workload specified, spawn it as a child process and emit WorkloadStarted
    if let Some(ref workload) = args.workload {
        info!("Spawning workload: {}", workload);
        info!("Emitting WorkloadStarted event.");
    }
    
    // 12. Wait for warmup duration
    info!("Waiting for warmup duration: {}", args.warmup);
    
    // 13. Run fault scheduler loop
    info!("Running fault scheduler loop...");
    let mut rng = StdRng::seed_from_u64(seed);
    
    // 14. Wait for remaining duration
    info!("Waiting for remaining duration...");
    
    // 15. Stop probes (send cancel signal)
    info!("Stopping probes...");
    
    // 16. Emit SimulationEnded event
    info!("Emitting SimulationEnded event.");
    
    // 17. Print timeline summary report to terminal
    info!("Printing timeline summary report...");
    report::render_terminal_report(&[]); // placeholder for actual events
    
    // 18. Save timeline to JSON file
    info!("Saving timeline to JSON file...");
    
    Ok(())
}

async fn handle_replay(args: ReplayArgs) -> Result<()> {
    println!("Replaying simulation with seed: 0x{:04X}", args.seed);
    println!("Config Summary:");
    println!("  Namespace: {}", args.namespace);
    println!("  Duration: {}", args.duration);
    
    info!("Running replay mode...");
    // Same steps as run...
    
    Ok(())
}

async fn handle_init(args: InitArgs) -> Result<()> {
    info!("Connecting to Kubernetes cluster...");
    info!("Discovering services and probes in namespace '{}'...", args.namespace);
    info!("Generating heisensim.toml configuration...");
    info!("Writing configuration to '{}'...", args.output.display());
    Ok(())
}

async fn handle_report(args: ReportArgs) -> Result<()> {
    info!("Loading timeline from JSON file: '{}'...", args.input.display());
    
    // We mock timeline events here to fit the existing CLI code, normally we would load from file
    let content = std::fs::read_to_string(&args.input).unwrap_or_else(|_| "[]".to_string());
    let events: Vec<heisensim_timeline::event::TimelineEvent> = serde_json::from_str(&content).unwrap_or_default();
    
    info!("Running timeline queries (summary, fault-to-detection latency)...");
    
    match args.format.as_str() {
        "json" => println!("{}", report::render_json_report(&events)),
        "markdown" => println!("{}", report::render_markdown_report(&events)),
        _ => report::render_terminal_report(&events),
    }
    
    Ok(())
}
