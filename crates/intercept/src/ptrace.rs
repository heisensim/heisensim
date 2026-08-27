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
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn wait_for_syscall(&mut self) -> Result<InterceptedSyscall> {
        use nix::sys::{
            ptrace,
            wait::{WaitStatus, waitpid},
        };

        let pid = self
            .pid
            .ok_or_else(|| anyhow::anyhow!("no process attached"))?;

        loop {
            // Resume until next syscall entry
            ptrace::syscall(pid, None)?;

            match waitpid(pid, None)? {
                WaitStatus::PtraceSyscall(_) => {
                    // This is a syscall stop — read registers
                    break;
                }
                WaitStatus::Stopped(_, sig) => {
                    // Signal-delivery stop — forward the signal and let the
                    // loop's next iteration pick up the resulting stop.
                    // We do NOT call waitpid here; the next ptrace::syscall +
                    // waitpid at the top of the loop handles it correctly.
                    ptrace::syscall(pid, Some(sig))?;
                    // Skip the ptrace::syscall(None) at the top of the next
                    // iteration — we already resumed. Go directly to waitpid.
                    match waitpid(pid, None)? {
                        WaitStatus::PtraceSyscall(_) => break,
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
                    // Other statuses (continued, etc.) — retry
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

        Ok(syscall)
    }

    /// Wait stub for non-x86_64-Linux.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    pub fn wait_for_syscall(&mut self) -> Result<InterceptedSyscall> {
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
        let result = tracer.wait_for_syscall();
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
