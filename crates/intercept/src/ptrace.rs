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
        self.pid = Some(pid);
        Ok(())
    }

    /// Attach stub for non-Linux.
    #[cfg(not(target_os = "linux"))]
    pub fn trace_process(&mut self, _pid: Pid) -> Result<()> {
        anyhow::bail!("ptrace is only supported on Linux")
    }

    /// Wait for the target process to enter a syscall, and return the parsed syscall.
    #[cfg(target_os = "linux")]
    pub fn wait_for_syscall(&mut self) -> Result<InterceptedSyscall> {
        use nix::sys::{ptrace, wait::waitpid};

        let pid = self
            .pid
            .ok_or_else(|| anyhow::anyhow!("no process attached"))?;

        // Resume until next syscall entry
        ptrace::syscall(pid, None)?;
        waitpid(pid, None)?;

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
            42 => {
                // For connect, we'd need to read the sockaddr from memory
                InterceptedSyscall::Connect {
                    fd: regs.rdi as i32,
                    addr: vec![],
                }
            }
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

    /// Wait stub for non-Linux.
    #[cfg(not(target_os = "linux"))]
    pub fn wait_for_syscall(&mut self) -> Result<InterceptedSyscall> {
        anyhow::bail!("ptrace is only supported on Linux")
    }

    /// Inject the result of a syscall back into the process, modifying registers if needed.
    #[cfg(target_os = "linux")]
    pub fn set_result(&mut self, result: SyscallResult) -> Result<()> {
        use nix::sys::ptrace;

        let pid = self
            .pid
            .ok_or_else(|| anyhow::anyhow!("no process attached"))?;

        match result {
            SyscallResult::Allow => {
                // Let the syscall proceed normally — resume to exit
                ptrace::syscall(pid, None)?;
                nix::sys::wait::waitpid(pid, None)?;
            }
            SyscallResult::Replace(value) => {
                // Skip to syscall exit, then override RAX with our value
                ptrace::syscall(pid, None)?;
                nix::sys::wait::waitpid(pid, None)?;
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
                nix::sys::wait::waitpid(pid, None)?;
                let mut regs = ptrace::getregs(pid)?;
                regs.rax = (-errno as i64) as u64;
                ptrace::setregs(pid, regs)?;
            }
            SyscallResult::Redirect => {
                // Redirect is not yet implemented — treat as allow
                ptrace::syscall(pid, None)?;
                nix::sys::wait::waitpid(pid, None)?;
            }
        }

        Ok(())
    }

    /// Set result stub for non-Linux.
    #[cfg(not(target_os = "linux"))]
    pub fn set_result(&mut self, _result: SyscallResult) -> Result<()> {
        anyhow::bail!("ptrace is only supported on Linux")
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
}
