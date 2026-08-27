//! heisensim-inject: Standalone vDSO time manipulation binary.
//!
//! This binary runs inside an ephemeral debug container (or any Linux
//! environment with CAP_SYS_PTRACE) to inject time offsets into a
//! target process.
//!
//! Usage:
//!   heisensim-inject --pid 1 --offset "+30d"
//!   heisensim-inject --pid 1 --offset "+2h" --speed 10x
//!   heisensim-inject --pid 1 --revert < handle.json

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
    #[arg(long, required_unless_present_any = ["revert", "fault"])]
    offset: Option<String>,

    /// Time speed multiplier (e.g., "10x", "0.5x").
    #[arg(long, default_value = "1x")]
    speed: String,

    /// Revert a previous injection. Reads the injection handle from stdin.
    #[arg(long)]
    revert: bool,

    /// Fault type to inject (connect-error, fd-exhaustion)
    #[arg(long, value_enum)]
    fault: Option<FaultMode>,

    /// Errno to return for connect-error (default: ECONNREFUSED = 111)
    #[arg(long, default_value = "111")]
    errno: i32,

    /// Duration to run fault injection (e.g., "15s", "1m")
    #[arg(long, default_value = "30s")]
    duration: String,

    /// Target port to filter (only inject faults on connections to this port)
    #[arg(long)]
    port: Option<u16>,

    /// Enable verbose logging.
    #[arg(long, short)]
    verbose: bool,
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum FaultMode {
    /// Inject ECONNREFUSED (or custom errno) on connect() calls
    ConnectError,
    /// Inject EMFILE (too many open files) on socket() calls
    FdExhaustion,
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

    if let Some(fault) = cli.fault {
        return run_fault_injection(cli.pid, fault, cli.errno, &cli.duration, cli.port);
    }

    let offset = cli.offset.as_deref().unwrap_or("+0s");

    inject(cli.pid, offset, &cli.speed)
}

fn parse_duration(s: &str) -> Result<std::time::Duration> {
    humantime::parse_duration(s).map_err(|e| anyhow::anyhow!("invalid duration: {}", e))
}

fn run_fault_injection(
    pid: u32,
    fault: FaultMode,
    errno: i32,
    duration_str: &str,
    port: Option<u16>,
) -> Result<()> {
    use heisensim_intercept::handler::NetworkFaultConfig;
    use heisensim_intercept::tracer::trace_with_faults;

    let duration = parse_duration(duration_str)?;

    let config = match fault {
        FaultMode::ConnectError => NetworkFaultConfig {
            connect_error: Some(errno),
            target_port: port,
            ..Default::default()
        },
        FaultMode::FdExhaustion => NetworkFaultConfig {
            socket_error: Some(24), // EMFILE — too many open files
            target_port: port,
            ..Default::default()
        },
    };

    trace_with_faults(pid, &config, duration)
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

    // Serialize the complete handle (including original_bytes) for revert
    let handle_json = serde_json::to_string(&handle)?;
    println!("{}", handle_json);

    Ok(())
}

fn revert_injection(pid: u32) -> Result<()> {
    tracing::info!(pid, "reading injection handle from stdin for revert...");

    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;

    let mut handle: heisensim_intercept::vdso::control::InjectionHandle =
        serde_json::from_str(&input)
            .map_err(|e| anyhow::anyhow!("failed to parse injection handle: {}", e))?;

    // Validate that the handle's PID matches the CLI PID to prevent
    // accidentally reverting in the wrong process's memory space
    if handle.pid != pid {
        anyhow::bail!(
            "PID mismatch: handle was created for PID {} but --pid {} was specified. \
             Refusing to write to wrong process memory.",
            handle.pid,
            pid
        );
    }

    tracing::info!(
        pid,
        trampoline_addr = format!("0x{:x}", handle.trampoline_addr),
        original_bytes_len = handle.original_bytes.len(),
        "reverting injection"
    );

    heisensim_intercept::vdso::injector::revert(&handle)?;
    tracing::info!(pid, "injection reverted successfully");

    Ok(())
}
