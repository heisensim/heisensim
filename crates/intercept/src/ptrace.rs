#![allow(clippy::new_without_default)]
/// Ptrace-based execution tracer with multi-thread support.
use crate::syscall::{InterceptedSyscall, SyscallResult};
use anyhow::Result;
use nix::unistd::Pid;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::collections::HashMap;

/// Read `len` bytes from a traced process's memory at `addr`.
///
/// Uses `ptrace::read` (PTRACE_PEEKDATA) in word-sized chunks (8 bytes on x86_64).
/// Returns the bytes read, or an empty vec on any error (graceful fallback).
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_process_memory(pid: Pid, addr: u64, len: usize) -> Vec<u8> {
    use nix::sys::ptrace;

    if len == 0 {
        return vec![];
    }

    let word_size = std::mem::size_of::<libc::c_long>();
    let mut buf = Vec::with_capacity(len);
    let mut offset = 0usize;

    while buf.len() < len {
        let ptr = (addr as usize + offset) as *mut libc::c_void;
        match ptrace::read(pid, ptr) {
            Ok(word) => {
                let bytes = word.to_ne_bytes();
                let remaining = len - buf.len();
                buf.extend_from_slice(&bytes[..remaining.min(word_size)]);
                offset += word_size;
            }
            Err(_) => {
                // Can't read target memory — return what we have (may be empty)
                break;
            }
        }
    }

    buf
}

/// Per-thread state in the ptrace event loop.
///
/// Tracks whether each thread is running (resumed via PTRACE_SYSCALL)
/// or stopped at a syscall entry waiting for the tracer to decide.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Debug)]
enum ThreadState {
    /// Thread has been resumed via PTRACE_SYSCALL and is running.
    Running,
    /// Thread is stopped at syscall-entry; we've parsed the syscall
    /// and are waiting for set_result to be called.
    SyscallEntry,
}

/// PtraceTracer manages traced processes and intercepts their syscalls using `ptrace`.
///
/// Supports multi-threaded targets: attaches to all threads via `/proc/PID/task/`
/// and tracks new threads created via `clone()` using `PTRACE_O_TRACECLONE`.
pub struct PtraceTracer {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    threads: HashMap<Pid, ThreadState>,
    leader: Option<Pid>,
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    _pid: Option<Pid>,
}

impl PtraceTracer {
    /// Create a new tracer instance.
    pub fn new() -> Self {
        Self {
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            threads: HashMap::new(),
            leader: None,
            #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
            _pid: None,
        }
    }

    /// Return the number of currently traced threads.
    pub fn thread_count(&self) -> usize {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            self.threads.len()
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            0
        }
    }

    /// Attach the tracer to a target process and all its threads.
    ///
    /// Enumerates `/proc/PID/task/` to find all existing threads and attaches
    /// to each one. Sets `PTRACE_O_TRACECLONE` to auto-trace new threads.
    #[cfg(target_os = "linux")]
    pub fn trace_process(&mut self, pid: Pid) -> Result<()> {
        use nix::sys::{ptrace, wait::waitpid};

        self.leader = Some(pid);

        let task_dir = format!("/proc/{}/task", pid);
        let entries: Vec<_> = std::fs::read_dir(&task_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();

        if entries.is_empty() {
            // Fallback: just attach to the main PID
            tracing::warn!(
                "could not enumerate /proc/{}/task, attaching to main thread only",
                pid
            );
            ptrace::attach(pid)?;
            waitpid(pid, None)?;
            ptrace::setoptions(
                pid,
                ptrace::Options::PTRACE_O_TRACESYSGOOD
                    | ptrace::Options::PTRACE_O_EXITKILL
                    | ptrace::Options::PTRACE_O_TRACECLONE,
            )?;
            #[cfg(target_arch = "x86_64")]
            self.threads.insert(pid, ThreadState::Running);
            return Ok(());
        }

        for entry in entries {
            let tid_str = entry.file_name().to_string_lossy().to_string();
            let tid_raw: i32 = match tid_str.parse() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let tid = Pid::from_raw(tid_raw);
            match ptrace::attach(tid) {
                Ok(()) => {}
                Err(nix::errno::Errno::ESRCH) => {
                    // Thread exited between enumeration and attach — skip
                    tracing::debug!(tid = tid_raw, "thread vanished during attach, skipping");
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
            waitpid(tid, None)?;
            ptrace::setoptions(
                tid,
                ptrace::Options::PTRACE_O_TRACESYSGOOD
                    | ptrace::Options::PTRACE_O_EXITKILL
                    | ptrace::Options::PTRACE_O_TRACECLONE,
            )?;
            #[cfg(target_arch = "x86_64")]
            self.threads.insert(tid, ThreadState::Running);
        }

        tracing::info!(
            pid = pid.as_raw(),
            threads = self.thread_count(),
            "attached to all threads"
        );
        Ok(())
    }

    /// Attach stub for non-Linux.
    #[cfg(not(target_os = "linux"))]
    pub fn trace_process(&mut self, _pid: Pid) -> Result<()> {
        anyhow::bail!("ptrace is only supported on Linux")
    }

    /// Wait for any traced thread to enter a syscall.
    ///
    /// Returns the thread PID and the parsed syscall, or `Ok(None)` if the
    /// deadline expires. Handles clone events (new threads) and thread exits
    /// transparently.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn wait_for_syscall(
        &mut self,
        deadline: Option<std::time::Instant>,
    ) -> Result<Option<(Pid, InterceptedSyscall)>> {
        use nix::sys::{
            ptrace,
            wait::{WaitPidFlag, WaitStatus, waitpid},
        };

        // Resume all threads that are in Running state
        for (&tid, state) in self.threads.iter() {
            if matches!(state, ThreadState::Running) {
                let _ = ptrace::syscall(tid, None);
            }
        }

        loop {
            // Check deadline
            if let Some(d) = deadline {
                if std::time::Instant::now() >= d {
                    return Ok(None);
                }
            }

            // Use WNOHANG when we have a deadline, blocking wait otherwise
            // Always use __WALL to catch clone threads
            let wait_flags = if deadline.is_some() {
                Some(WaitPidFlag::WNOHANG | WaitPidFlag::__WALL)
            } else {
                Some(WaitPidFlag::__WALL)
            };

            match waitpid(None, wait_flags)? {
                WaitStatus::StillAlive => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                WaitStatus::PtraceSyscall(tid) => {
                    let regs = ptrace::getregs(tid)?;
                    let syscall = self.parse_syscall(tid, regs);
                    self.threads.insert(tid, ThreadState::SyscallEntry);
                    return Ok(Some((tid, syscall)));
                }
                WaitStatus::PtraceEvent(tid, _sig, event) if event == libc::PTRACE_EVENT_CLONE => {
                    let new_tid_raw = ptrace::getevent(tid)? as i32;
                    let new_tid = Pid::from_raw(new_tid_raw);
                    tracing::debug!(
                        parent = tid.as_raw(),
                        child = new_tid_raw,
                        "new thread detected via PTRACE_EVENT_CLONE"
                    );
                    match waitpid(new_tid, Some(WaitPidFlag::__WALL)) {
                        Ok(_) => {
                            ptrace::setoptions(
                                new_tid,
                                ptrace::Options::PTRACE_O_TRACESYSGOOD
                                    | ptrace::Options::PTRACE_O_EXITKILL
                                    | ptrace::Options::PTRACE_O_TRACECLONE,
                            )?;
                            let _ = ptrace::syscall(new_tid, None);
                            self.threads.insert(new_tid, ThreadState::Running);
                        }
                        Err(nix::errno::Errno::ECHILD) => {
                            tracing::debug!(
                                tid = new_tid_raw,
                                "cloned thread exited before attach"
                            );
                        }
                        Err(e) => return Err(e.into()),
                    }
                    let _ = ptrace::syscall(tid, None);
                    self.threads.insert(tid, ThreadState::Running);
                    continue;
                }
                WaitStatus::Stopped(tid, sig) => {
                    let _ = ptrace::syscall(tid, Some(sig));
                    if let Some(state) = self.threads.get_mut(&tid) {
                        *state = ThreadState::Running;
                    }
                    continue;
                }
                WaitStatus::Exited(tid, code) => {
                    self.threads.remove(&tid);
                    if Some(tid) == self.leader {
                        tracing::info!(code, "leader thread exited, stopping");
                        return Ok(None);
                    }
                    if self.threads.is_empty() {
                        tracing::info!("all threads exited, stopping");
                        return Ok(None);
                    }
                    tracing::debug!(
                        tid = tid.as_raw(),
                        code,
                        remaining = self.threads.len(),
                        "worker thread exited"
                    );
                    continue;
                }
                WaitStatus::Signaled(tid, sig, _) => {
                    self.threads.remove(&tid);
                    if Some(tid) == self.leader || self.threads.is_empty() {
                        anyhow::bail!("traced process killed by signal {:?}", sig);
                    }
                    tracing::debug!(tid = tid.as_raw(), ?sig, "worker thread killed by signal");
                    continue;
                }
                _status => {
                    continue;
                }
            }
        }
    }

    /// Parse registers into an InterceptedSyscall enum.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn parse_syscall(&self, pid: Pid, regs: libc::user_regs_struct) -> InterceptedSyscall {
        match regs.orig_rax as i64 {
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
                let addr_ptr = regs.rsi;
                let addr_len = (regs.rdx as usize).min(128);
                let addr = read_process_memory(pid, addr_ptr, addr_len);
                InterceptedSyscall::Connect {
                    fd: regs.rdi as i32,
                    addr,
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
            49 => {
                let addr_ptr = regs.rsi;
                let addr_len = (regs.rdx as usize).min(128);
                let addr = read_process_memory(pid, addr_ptr, addr_len);
                InterceptedSyscall::Bind {
                    fd: regs.rdi as i32,
                    addr,
                }
            }
            nr => InterceptedSyscall::Unknown { syscall_nr: nr },
        }
    }

    /// Wait stub for non-x86_64-Linux.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    pub fn wait_for_syscall(
        &mut self,
        _deadline: Option<std::time::Instant>,
    ) -> Result<Option<(Pid, InterceptedSyscall)>> {
        anyhow::bail!("ptrace syscall tracing is only supported on x86_64 Linux")
    }
}

/// Wait for syscall-exit stop on a specific thread, forwarding any intervening signals.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn wait_for_syscall_exit(pid: Pid) -> Result<()> {
    use nix::sys::{
        ptrace,
        wait::{WaitPidFlag, WaitStatus, waitpid},
    };
    loop {
        match waitpid(pid, Some(WaitPidFlag::__WALL))? {
            WaitStatus::PtraceSyscall(_) => return Ok(()),
            WaitStatus::Stopped(_, sig) => {
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
    /// Inject the result of a syscall back into a specific thread.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn set_result(&mut self, tid: Pid, result: SyscallResult) -> Result<()> {
        use nix::sys::ptrace;

        match result {
            SyscallResult::Allow => {
                ptrace::syscall(tid, None)?;
                wait_for_syscall_exit(tid)?;
            }
            SyscallResult::Replace(value) => {
                ptrace::syscall(tid, None)?;
                wait_for_syscall_exit(tid)?;
                let mut regs = ptrace::getregs(tid)?;
                regs.rax = value as u64;
                ptrace::setregs(tid, regs)?;
            }
            SyscallResult::Block(errno) => {
                let mut regs = ptrace::getregs(tid)?;
                regs.orig_rax = u64::MAX;
                ptrace::setregs(tid, regs)?;
                ptrace::syscall(tid, None)?;
                wait_for_syscall_exit(tid)?;
                let mut regs = ptrace::getregs(tid)?;
                regs.rax = (-errno as i64) as u64;
                ptrace::setregs(tid, regs)?;
            }
            SyscallResult::Delay(duration) => {
                std::thread::sleep(duration);
                ptrace::syscall(tid, None)?;
                wait_for_syscall_exit(tid)?;
            }
            SyscallResult::Redirect => {
                ptrace::syscall(tid, None)?;
                wait_for_syscall_exit(tid)?;
            }
        }

        // Mark thread as running again after handling
        self.threads.insert(tid, ThreadState::Running);
        Ok(())
    }

    /// Set result stub for non-x86_64-Linux.
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    pub fn set_result(&mut self, _tid: Pid, _result: SyscallResult) -> Result<()> {
        anyhow::bail!("ptrace syscall tracing is only supported on x86_64 Linux")
    }

    /// Detach from all traced threads.
    #[cfg(target_os = "linux")]
    pub fn detach(&mut self) -> Result<()> {
        #[cfg(target_arch = "x86_64")]
        {
            for tid in self.threads.drain().map(|(tid, _)| tid) {
                if let Err(e) = nix::sys::ptrace::detach(tid, None) {
                    tracing::debug!(tid = tid.as_raw(), error = %e, "failed to detach thread");
                }
            }
        }
        self.leader = None;
        Ok(())
    }

    /// Detach stub for non-Linux.
    #[cfg(not(target_os = "linux"))]
    pub fn detach(&mut self) -> Result<()> {
        self.leader = None;
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
        {
            self._pid = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracer() {
        let tracer = PtraceTracer::new();
        assert!(tracer.leader.is_none());
        assert_eq!(tracer.thread_count(), 0);
    }

    #[test]
    fn test_detach_without_attach_is_ok() {
        let mut tracer = PtraceTracer::new();
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
        let result = tracer.set_result(Pid::from_raw(1), SyscallResult::Allow);
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
    fn test_detach_non_linux_clears_leader() {
        let mut tracer = PtraceTracer::new();
        tracer.leader = Some(Pid::from_raw(42));
        assert!(tracer.detach().is_ok());
        assert!(tracer.leader.is_none());
    }
}
