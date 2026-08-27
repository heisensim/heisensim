//! heisensim-inject: Standalone vDSO time manipulation binary.
//!
//! This binary runs inside an ephemeral debug container (or any Linux
//! environment with CAP_SYS_PTRACE) to inject time offsets into a
//! target process.
//!
//! Usage:
//!   heisensim-inject --pid 1 --offset "+30d"
//!   heisensim-inject --pid 1 --offset "+2h" --speed 10x
//!   heisensim-inject --pid 1 --revert

use anyhow::Result;
use clap::Parser;

/// Inject time manipulation into a running process via vDSO trampoline.
#[derive(Parser, Debug)]
#[command(name = "heisensim-inject", version, about)]
struct Cli {
    /// Target process ID to inject into.
    #[arg(long)]
    pid: u32,

    /// Time offset to apply (e.g., "+30d", "-2h", "+90m", "+3600s").
    #[arg(long)]
    offset: Option<String>,

    /// Time speed multiplier (e.g., "10x", "0.5x").
    #[arg(long, default_value = "1x")]
    speed: String,

    /// Revert a previous injection (restore original vDSO).
    #[arg(long)]
    revert: bool,

    /// Enable verbose logging.
    #[arg(long, short)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    if cli.revert {
        return revert_injection(cli.pid);
    }

    let offset = cli.offset.as_deref().unwrap_or("+0s");

    inject(cli.pid, offset, &cli.speed)
}

fn inject(pid: u32, offset: &str, speed: &str) -> Result<()> {
    use heisensim_intercept::vdso::control::{TimeControl, parse_speed, parse_time_offset};
    use heisensim_intercept::vdso::injector::{self, InjectionConfig};

    let (offset_secs, offset_nanos) = parse_time_offset(offset)?;
    let (speed_num, speed_den) = parse_speed(speed)?;

    let time_control =
        TimeControl::with_offset_and_speed(offset_secs, offset_nanos, speed_num, speed_den);

    tracing::info!(
        pid,
        offset,
        speed,
        "injecting time manipulation: {}",
        time_control
    );

    let config = InjectionConfig { pid, time_control };

    let handle = injector::inject(&config)?;

    tracing::info!(
        pid,
        shm_addr = format!("0x{:x}", handle.shm_addr),
        payload_addr = format!("0x{:x}", handle.payload_addr),
        "injection successful"
    );

    // Print handle info for the caller to use for revert
    println!(
        "{{\"pid\":{},\"shm_addr\":{},\"payload_addr\":{},\"trampoline_addr\":{},\"allocated_addr\":{},\"allocated_size\":{}}}",
        handle.pid,
        handle.shm_addr,
        handle.payload_addr,
        handle.trampoline_addr,
        handle.allocated_addr,
        handle.allocated_size,
    );

    Ok(())
}

fn revert_injection(pid: u32) -> Result<()> {
    // For revert, we need the injection handle. In practice, the caller
    // (heisensim CLI or k8s operator) would pass this information.
    // For now, we read from stdin.
    tracing::info!(pid, "reading injection handle from stdin for revert...");

    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;

    let handle: serde_json::Value = serde_json::from_str(&input)
        .map_err(|e| anyhow::anyhow!("failed to parse injection handle from stdin: {}", e))?;

    let injection_handle = heisensim_intercept::vdso::control::InjectionHandle {
        pid,
        shm_addr: handle["shm_addr"].as_u64().unwrap_or(0),
        payload_addr: handle["payload_addr"].as_u64().unwrap_or(0),
        original_bytes: vec![], // Will need to be stored externally
        trampoline_addr: handle["trampoline_addr"].as_u64().unwrap_or(0),
        allocated_size: handle["allocated_size"].as_u64().unwrap_or(0) as usize,
        allocated_addr: handle["allocated_addr"].as_u64().unwrap_or(0),
    };

    heisensim_intercept::vdso::injector::revert(&injection_handle)?;
    tracing::info!(pid, "injection reverted successfully");

    Ok(())
}
