//! Tracer event loop for process-level fault injection.
//!
//! Attaches to a target process, intercepts syscalls, applies fault rules
//! from the SyscallHandler, and detaches after a specified duration.

use crate::handler::NetworkFaultConfig;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use crate::handler::SyscallHandler;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use crate::ptrace::PtraceTracer;
use anyhow::Result;
use std::time::Duration;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::time::Instant;

/// Run the ptrace fault injection loop on a target process.
///
/// Attaches to `pid`, intercepts syscalls for `duration`, applying faults
/// from `config`. Detaches cleanly on timeout or error.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn trace_with_faults(pid: u32, config: &NetworkFaultConfig, duration: Duration) -> Result<()> {
    use nix::unistd::Pid;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let target = Pid::from_raw(pid as i32);
    let mut tracer = PtraceTracer::new();
    let mut handler = SyscallHandler::with_network_fault(config.clone());

    tracer.trace_process(target)?;
    tracing::info!(
        pid,
        threads = tracer.thread_count(),
        ?duration,
        "attached, starting fault injection loop"
    );

    // Register SIGINT handler for clean detach on Ctrl-C
    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))?;

    let start = Instant::now();
    let result = run_loop(&mut tracer, &mut handler, duration, start, &shutdown);

    // Always detach, even on error — iterates all tracked TIDs
    tracing::info!(
        threads = tracer.thread_count(),
        "detaching from all threads"
    );
    if let Err(e) = tracer.detach() {
        tracing::warn!("failed to detach: {}", e);
    }

    result
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run_loop(
    tracer: &mut PtraceTracer,
    handler: &mut SyscallHandler,
    duration: Duration,
    start: Instant,
    shutdown: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    use std::sync::atomic::Ordering;

    let deadline = start + duration;
    loop {
        // Check for SIGINT before waiting
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!("SIGINT received, stopping gracefully");
            return Ok(());
        }

        match tracer.wait_for_syscall(Some(deadline))? {
            Some((tid, syscall)) => {
                let result = handler.handle(syscall);
                tracer.set_result(tid, result)?;
            }
            None => {
                tracing::info!("duration expired, stopping");
                return Ok(());
            }
        }
    }
}

/// Stub for non-x86_64-Linux.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn trace_with_faults(
    _pid: u32,
    _config: &NetworkFaultConfig,
    _duration: Duration,
) -> Result<()> {
    anyhow::bail!("process-level fault injection is only supported on x86_64 Linux")
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    fn test_trace_with_faults_non_linux_errors() {
        let config = NetworkFaultConfig::default();
        let result = trace_with_faults(1, &config, Duration::from_secs(5));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supported on x86_64 Linux")
        );
    }
}
