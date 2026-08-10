#![allow(dead_code, clippy::new_without_default)]
/// Seccomp-BPF filter for efficient syscall interception.
use anyhow::Result;
use std::os::unix::io::RawFd;

/// Manages a Seccomp-BPF filter that traps targeted syscalls.
///
/// Unlike pure ptrace which stops the process on every syscall, seccomp-BPF
/// allows the kernel to evaluate a small program on every syscall and only
/// trap to userspace (via a notification fd or ptrace event) for the syscalls
/// we explicitly care about (time, network, etc.), drastically reducing overhead.
pub struct SeccompFilter {
    // filter state
}

impl SeccompFilter {
    /// Create a new SeccompFilter.
    pub fn new() -> Self {
        Self {}
    }

    /// Installs a seccomp-BPF filter that traps targeted syscalls.
    pub fn install_filter(&mut self) -> Result<()> {
        todo!("Implement seccomp filter installation")
    }

    /// Creates and returns a file descriptor for seccomp user notifications (SECCOMP_RET_USER_NOTIF).
    pub fn create_notification_fd(&mut self) -> Result<RawFd> {
        todo!("Implement SECCOMP_RET_USER_NOTIF fd creation")
    }
}
