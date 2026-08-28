#![allow(clippy::new_without_default)]
/// Ptrace-based execution tracer.
use crate::syscall::{InterceptedSyscall, SyscallResult};
use anyhow::Result;
use nix::unistd::Pid;

/// PtraceTracer manages traced processes and intercepts their syscalls using `ptrace`.
///
/// It acts as a supervisor that pauses the process on syscall entry, examines the
/// syscall, optionally modifies it, and then resumes the process.
pub struct PtraceTracer {
    pid: Option<Pid>,
}

impl PtraceTracer {
    /// Create a new tracer instance.
    pub fn new() -> Self {
        Self { pid: None }
    }

    /// Attach the tracer to a target process by PID.
    #[cfg(target_os = "linux")]
    pub fn trace_process(&mut self, pid: Pid) -> Result<()> {
        use nix::sys::{ptrace, wait::waitpid};

        ptrace::attach(pid)?;
        waitpid(pid, None)?;
        // Set TRACESYSGOOD so we can distinguish syscall stops from signal stops
        ptrace::setoptions(
            pid,
            ptrace::Options::PTRACE_O_TRACESYSGOOD | ptrace::Options::PTRACE_O_EXITKILL,
        )?;
        self.pid = Some(pid);
        Ok(())
    }

    /// Attach stub for non-Linux.
    #[cfg(not(target_os = "linux"))]
    pub fn trace_process(&mut self, _pid: Pid) -> Result<()> {
        anyhow::bail!("ptrace is only supported on Linux")
    }

    /// Wait for the target process to enter a syscall, and return the parsed syscall.
    ///
    /// If `deadline` is `Some`, returns `Ok(None)` when the deadline expires without
    /// a syscall stop. Uses non-blocking `WNOHANG` polling with idle backoff.
    /// If `deadline` is `None`, blocks until the next syscall (legacy behavior for vDSO).
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn wait_for_syscall(
        &mut self,
        deadline: Option<std::time::Instant>,
    ) -> Result<Option<InterceptedSyscall>> {
        use nix::sys::{
            ptrace,
            wait::{WaitPidFlag, WaitStatus, waitpid},
        };

        let pid = self
            .pid
            .ok_or_else(|| anyhow::anyhow!("no process attached"))?;

        // Resume until next syscall entry
        ptrace::syscall(pid, None)?;

        loop {
            // Check deadline before waiting
            if let Some(d) = deadline {
                if std::time::Instant::now() >= d {
                    return Ok(None);
                }
            }

            // Use WNOHANG when we have a deadline, blocking wait otherwise
            let wait_flags = if deadline.is_some() {
                Some(WaitPidFlag::WNOHANG)
            } else {
                None
            };

            match waitpid(pid, wait_flags)? {
                WaitStatus::StillAlive => {
                    // Target is idle — back off briefly before polling again
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                WaitStatus::PtraceSyscall(_) => {
                    // Syscall stop — read registers below
                    break;
                }
                WaitStatus::Stopped(_, sig) => {
                    // Signal-delivery stop — forward the signal and wait again
                    ptrace::syscall(pid, Some(sig))?;
                    match waitpid(pid, wait_flags)? {
                        WaitStatus::PtraceSyscall(_) => break,
                        WaitStatus::StillAlive => continue,
                        WaitStatus::Signaled(_, sig, _) => {
                            anyhow::bail!("traced process killed by signal {:?}", sig);
                        }
                        WaitStatus::Exited(_, code) => {
                            anyhow::bail!("traced process exited with code {}", code);
                        }
                        _ => continue,
                    }
                }
                WaitStatus::Signaled(_, sig, _) => {
                    anyhow::bail!("traced process killed by signal {:?}", sig);
                }
                WaitStatus::Exited(_, code) => {
                    anyhow::bail!("traced process exited with code {}", code);
                }
                status => {
                    tracing::debug!(?status, "unexpected wait status, retrying");
                    continue;
                }
            }
        }

        // Read registers to determine which syscall
        let regs = ptrace::getregs(pid)?;

        // Map syscall number to our enum (x86_64 syscall numbers)
        let syscall = match regs.orig_rax as i64 {
            228 => InterceptedSyscall::ClockGettime {
                clock_id: regs.rdi as i32,
            },
            96 => InterceptedSyscall::GetTimeOfDay,
            35 => InterceptedSyscall::Nanosleep {
                requested_ns: regs.rdi,
            },
            318 => InterceptedSyscall::GetRandom {
                len: regs.rsi as usize,
            },
            41 => InterceptedSyscall::Socket {
                domain: regs.rdi as i32,
                sock_type: regs.rsi as i32,
                protocol: regs.rdx as i32,
            },
            42 => InterceptedSyscall::Connect {
                fd: regs.rdi as i32,
                addr: vec![],
            },
            44 => InterceptedSyscall::Send {
                fd: regs.rdi as i32,
                len: regs.rdx as usize,
            },
            45 => InterceptedSyscall::Recv {
                fd: regs.rdi as i32,
                len: regs.rdx as usize,
            },
            43 => InterceptedSyscall::Accept {
                fd: regs.rdi as i32,
            },
            49 => InterceptedSyscall::Bind {
                fd: regs.rdi as i32,
                addr: vec![],
            },
            nr => InterceptedSyscall::Unknown { syscall_nr: nr },
        };

        Ok(Some(syscall))
    }

    /// Wait stub for non-x86_64-Linux.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    pub fn wait_for_syscall(
        &mut self,
        _deadline: Option<std::time::Instant>,
    ) -> Result<Option<InterceptedSyscall>> {
        anyhow::bail!("ptrace syscall tracing is only supported on x86_64 Linux")
    }
}

/// Wait for syscall-exit stop, forwarding any intervening signals.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn wait_for_syscall_exit(pid: Pid) -> Result<()> {
    use nix::sys::{
        ptrace,
        wait::{WaitStatus, waitpid},
    };
    loop {
        match waitpid(pid, None)? {
            WaitStatus::PtraceSyscall(_) => return Ok(()),
            WaitStatus::Stopped(_, sig) => {
                // Forward the signal and continue waiting
                ptrace::syscall(pid, Some(sig))?;
            }
            WaitStatus::Signaled(_, sig, _) => {
                anyhow::bail!("traced process killed by signal {:?}", sig);
            }
            WaitStatus::Exited(_, code) => {
                anyhow::bail!("traced process exited with code {}", code);
            }
            _ => continue,
        }
    }
}

impl PtraceTracer {
    /// Inject the result of a syscall back into the process, modifying registers if needed.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn set_result(&mut self, result: SyscallResult) -> Result<()> {
        use nix::sys::ptrace;

        let pid = self
            .pid
            .ok_or_else(|| anyhow::anyhow!("no process attached"))?;

        match result {
            SyscallResult::Allow => {
                // Let the syscall proceed normally — resume to exit
                ptrace::syscall(pid, None)?;
                wait_for_syscall_exit(pid)?;
            }
            SyscallResult::Replace(value) => {
                // Skip to syscall exit, then override RAX with our value
                ptrace::syscall(pid, None)?;
                wait_for_syscall_exit(pid)?;
                let mut regs = ptrace::getregs(pid)?;
                regs.rax = value as u64;
                ptrace::setregs(pid, regs)?;
            }
            SyscallResult::Block(errno) => {
                // Replace syscall number with -1 (invalid) so kernel returns ENOSYS,
                // then override with our errno
                let mut regs = ptrace::getregs(pid)?;
                regs.orig_rax = u64::MAX; // -1: invalid syscall
                ptrace::setregs(pid, regs)?;
                ptrace::syscall(pid, None)?;
                wait_for_syscall_exit(pid)?;
                let mut regs = ptrace::getregs(pid)?;
                regs.rax = (-errno as i64) as u64;
                ptrace::setregs(pid, regs)?;
            }
            SyscallResult::Delay(duration) => {
                // Sleep to introduce latency, then let the syscall proceed
                std::thread::sleep(duration);
                ptrace::syscall(pid, None)?;
                wait_for_syscall_exit(pid)?;
            }
            SyscallResult::Redirect => {
                // Redirect is not yet implemented — treat as allow
                ptrace::syscall(pid, None)?;
                wait_for_syscall_exit(pid)?;
            }
        }

        Ok(())
    }

    /// Set result stub for non-x86_64-Linux.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    pub fn set_result(&mut self, _result: SyscallResult) -> Result<()> {
        anyhow::bail!("ptrace syscall tracing is only supported on x86_64 Linux")
    }

    /// Detach from the traced process.
    #[cfg(target_os = "linux")]
    pub fn detach(&mut self) -> Result<()> {
        if let Some(pid) = self.pid.take() {
            nix::sys::ptrace::detach(pid, None)?;
        }
        Ok(())
    }

    /// Detach stub for non-Linux.
    #[cfg(not(target_os = "linux"))]
    pub fn detach(&mut self) -> Result<()> {
        self.pid = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracer() {
        let tracer = PtraceTracer::new();
        assert!(tracer.pid.is_none());
    }

    #[test]
    fn test_detach_without_attach_is_ok() {
        let mut tracer = PtraceTracer::new();
        // Detaching when nothing is attached should succeed silently
        assert!(tracer.detach().is_ok());
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_trace_process_non_linux_errors() {
        let mut tracer = PtraceTracer::new();
        let result = tracer.trace_process(Pid::from_raw(1));
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supported on Linux")
        );
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_wait_for_syscall_non_linux_errors() {
        let mut tracer = PtraceTracer::new();
        let result = tracer.wait_for_syscall(None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supported on x86_64 Linux")
        );
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_set_result_non_linux_errors() {
        let mut tracer = PtraceTracer::new();
        let result = tracer.set_result(SyscallResult::Allow);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supported on x86_64 Linux")
        );
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_detach_non_linux_clears_pid() {
        let mut tracer = PtraceTracer::new();
        // Manually set pid to simulate an attached state
        tracer.pid = Some(Pid::from_raw(42));
        assert!(tracer.detach().is_ok());
        assert!(tracer.pid.is_none());
    }
}
