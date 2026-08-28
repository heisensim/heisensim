#![allow(clippy::new_without_default)]
/// Ptrace-based execution tracer with multi-thread support.
///
/// Supports both x86_64 and aarch64 Linux targets. Architecture-specific
/// syscall numbers and register access are abstracted via helper functions.
use crate::syscall::{InterceptedSyscall, SyscallResult};
use anyhow::Result;
use nix::unistd::Pid;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use std::collections::HashMap;

/// Convenience: true when ptrace fault injection is available.
/// Used to gate implementations that need Linux + (x86_64 or aarch64).
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod abi {
    // Architecture-specific syscall numbers and register accessors.

    use super::*;

    // ── Syscall numbers ───────────────────────────────────────────────

    #[cfg(target_arch = "x86_64")]
    pub mod nr {
        pub const CLOCK_GETTIME: i64 = 228;
        pub const GETTIMEOFDAY: i64 = 96;
        pub const NANOSLEEP: i64 = 35;
        pub const GETRANDOM: i64 = 318;
        pub const SOCKET: i64 = 41;
        pub const CONNECT: i64 = 42;
        pub const ACCEPT: i64 = 43;
        pub const SENDTO: i64 = 44;
        pub const RECVFROM: i64 = 45;
        pub const BIND: i64 = 49;
    }

    #[cfg(target_arch = "aarch64")]
    pub mod nr {
        pub const CLOCK_GETTIME: i64 = 113;
        pub const GETTIMEOFDAY: i64 = 169;
        pub const NANOSLEEP: i64 = 101;
        pub const GETRANDOM: i64 = 278;
        pub const SOCKET: i64 = 198;
        pub const CONNECT: i64 = 203;
        pub const ACCEPT: i64 = 202;
        pub const SENDTO: i64 = 206;
        pub const RECVFROM: i64 = 207;
        pub const BIND: i64 = 200;
    }

    // ── Register accessors ────────────────────────────────────────────

    /// Extract the syscall number from registers.
    #[cfg(target_arch = "x86_64")]
    pub fn syscall_nr(regs: &libc::user_regs_struct) -> i64 {
        regs.orig_rax as i64
    }

    #[cfg(target_arch = "aarch64")]
    pub fn syscall_nr(regs: &libc::user_regs_struct) -> i64 {
        regs.regs[8] as i64
    }

    /// Extract syscall argument 0 (first arg).
    #[cfg(target_arch = "x86_64")]
    pub fn arg0(regs: &libc::user_regs_struct) -> u64 {
        regs.rdi
    }

    #[cfg(target_arch = "aarch64")]
    pub fn arg0(regs: &libc::user_regs_struct) -> u64 {
        regs.regs[0]
    }

    /// Extract syscall argument 1 (second arg).
    #[cfg(target_arch = "x86_64")]
    pub fn arg1(regs: &libc::user_regs_struct) -> u64 {
        regs.rsi
    }

    #[cfg(target_arch = "aarch64")]
    pub fn arg1(regs: &libc::user_regs_struct) -> u64 {
        regs.regs[1]
    }

    /// Extract syscall argument 2 (third arg).
    #[cfg(target_arch = "x86_64")]
    pub fn arg2(regs: &libc::user_regs_struct) -> u64 {
        regs.rdx
    }

    #[cfg(target_arch = "aarch64")]
    pub fn arg2(regs: &libc::user_regs_struct) -> u64 {
        regs.regs[2]
    }

    /// Set the syscall return value in registers.
    #[cfg(target_arch = "x86_64")]
    pub fn set_return(regs: &mut libc::user_regs_struct, value: u64) {
        regs.rax = value;
    }

    #[cfg(target_arch = "aarch64")]
    pub fn set_return(regs: &mut libc::user_regs_struct, value: u64) {
        regs.regs[0] = value;
    }

    /// Block the current syscall by replacing the syscall number with an invalid one.
    ///
    /// On x86_64: sets `orig_rax = u64::MAX` to make the kernel skip the syscall.
    /// On aarch64: uses `PTRACE_SETREGSET` with `NT_ARM_SYSTEM_CALL` to set the
    /// syscall number to `-1`, which the kernel respects as "skip this syscall".
    pub fn block_syscall(pid: Pid, regs: &mut libc::user_regs_struct) -> Result<()> {
        use nix::sys::ptrace;

        #[cfg(target_arch = "x86_64")]
        {
            regs.orig_rax = u64::MAX;
            ptrace::setregs(pid, *regs)?;
        }

        #[cfg(target_arch = "aarch64")]
        {
            // Must use NT_ARM_SYSTEM_CALL to change the syscall number on aarch64.
            // Writing to regs[8] (x8) does NOT reliably cancel the syscall because
            // the kernel latches the syscall number before notifying ptrace.
            const NT_ARM_SYSTEM_CALL: libc::c_int = 0x404;
            let mut syscallno: libc::c_int = -1;
            let mut iov = libc::iovec {
                iov_base: &mut syscallno as *mut _ as *mut libc::c_void,
                iov_len: std::mem::size_of::<libc::c_int>(),
            };
            let ret = unsafe {
                libc::ptrace(
                    libc::PTRACE_SETREGSET,
                    libc::pid_t::from(pid.as_raw()),
                    NT_ARM_SYSTEM_CALL as libc::c_ulong,
                    &mut iov as *mut _ as *mut libc::c_void,
                )
            };
            if ret == -1 {
                return Err(anyhow::anyhow!(
                    "PTRACE_SETREGSET NT_ARM_SYSTEM_CALL failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            // Also update regs to reflect the blocked state
            ptrace::setregs(pid, *regs)?;
        }

        Ok(())
    }
}

/// Read `len` bytes from a traced process's memory at `addr`.
///
/// Uses `ptrace::read` (PTRACE_PEEKDATA) in word-sized chunks (8 bytes).
/// Returns the bytes read, or an empty vec on any error (graceful fallback).
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
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
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
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
///
/// Supports both x86_64 and aarch64 Linux.
pub struct PtraceTracer {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    threads: HashMap<Pid, ThreadState>,
    leader: Option<Pid>,
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    _pid: Option<Pid>,
}

impl PtraceTracer {
    /// Create a new tracer instance.
    pub fn new() -> Self {
        Self {
            #[cfg(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            ))]
            threads: HashMap::new(),
            leader: None,
            #[cfg(not(all(
                target_os = "linux",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )))]
            _pid: None,
        }
    }

    /// Return the number of currently traced threads.
    pub fn thread_count(&self) -> usize {
        #[cfg(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ))]
        {
            self.threads.len()
        }
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
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
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
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
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
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
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
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
    ///
    /// Uses architecture-specific syscall numbers and register accessors
    /// from the `abi` module.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn parse_syscall(&self, pid: Pid, regs: libc::user_regs_struct) -> InterceptedSyscall {
        match abi::syscall_nr(&regs) {
            abi::nr::CLOCK_GETTIME => InterceptedSyscall::ClockGettime {
                clock_id: abi::arg0(&regs) as i32,
            },
            abi::nr::GETTIMEOFDAY => InterceptedSyscall::GetTimeOfDay,
            abi::nr::NANOSLEEP => InterceptedSyscall::Nanosleep {
                requested_ns: abi::arg0(&regs),
            },
            abi::nr::GETRANDOM => InterceptedSyscall::GetRandom {
                len: abi::arg1(&regs) as usize,
            },
            abi::nr::SOCKET => InterceptedSyscall::Socket {
                domain: abi::arg0(&regs) as i32,
                sock_type: abi::arg1(&regs) as i32,
                protocol: abi::arg2(&regs) as i32,
            },
            abi::nr::CONNECT => {
                let addr_ptr = abi::arg1(&regs);
                let addr_len = (abi::arg2(&regs) as usize).min(128);
                let addr = read_process_memory(pid, addr_ptr, addr_len);
                InterceptedSyscall::Connect {
                    fd: abi::arg0(&regs) as i32,
                    addr,
                }
            }
            abi::nr::SENDTO => InterceptedSyscall::Send {
                fd: abi::arg0(&regs) as i32,
                len: abi::arg2(&regs) as usize,
            },
            abi::nr::RECVFROM => InterceptedSyscall::Recv {
                fd: abi::arg0(&regs) as i32,
                len: abi::arg2(&regs) as usize,
            },
            abi::nr::ACCEPT => InterceptedSyscall::Accept {
                fd: abi::arg0(&regs) as i32,
            },
            abi::nr::BIND => {
                let addr_ptr = abi::arg1(&regs);
                let addr_len = (abi::arg2(&regs) as usize).min(128);
                let addr = read_process_memory(pid, addr_ptr, addr_len);
                InterceptedSyscall::Bind {
                    fd: abi::arg0(&regs) as i32,
                    addr,
                }
            }
            nr => InterceptedSyscall::Unknown { syscall_nr: nr },
        }
    }

    /// Wait stub for unsupported platforms.
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    pub fn wait_for_syscall(
        &mut self,
        _deadline: Option<std::time::Instant>,
    ) -> Result<Option<(Pid, InterceptedSyscall)>> {
        anyhow::bail!("ptrace syscall tracing is only supported on x86_64/aarch64 Linux")
    }
}

/// Wait for syscall-exit stop on a specific thread, forwarding any intervening signals.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
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
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
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
                abi::set_return(&mut regs, value as u64);
                ptrace::setregs(tid, regs)?;
            }
            SyscallResult::Block(errno) => {
                let mut regs = ptrace::getregs(tid)?;
                abi::block_syscall(tid, &mut regs)?;
                ptrace::syscall(tid, None)?;
                wait_for_syscall_exit(tid)?;
                let mut regs = ptrace::getregs(tid)?;
                abi::set_return(&mut regs, (-errno as i64) as u64);
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

    /// Set result stub for unsupported platforms.
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    pub fn set_result(&mut self, _tid: Pid, _result: SyscallResult) -> Result<()> {
        anyhow::bail!("ptrace syscall tracing is only supported on x86_64/aarch64 Linux")
    }

    /// Detach from all traced threads.
    #[cfg(target_os = "linux")]
    pub fn detach(&mut self) -> Result<()> {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
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
        #[cfg(not(all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )))]
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
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    fn test_wait_for_syscall_non_linux_errors() {
        let mut tracer = PtraceTracer::new();
        let result = tracer.wait_for_syscall(None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supported on x86_64/aarch64 Linux")
        );
    }

    #[test]
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    fn test_set_result_non_linux_errors() {
        let mut tracer = PtraceTracer::new();
        let result = tracer.set_result(Pid::from_raw(1), SyscallResult::Allow);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("only supported on x86_64/aarch64 Linux")
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
