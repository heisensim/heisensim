//! # heisensim-intercept
//!
//! Syscall interception layer for heisensim. Provides two mechanisms:
//!
//! ## vDSO Trampoline (primary)
//!
//! Patches the Linux vDSO `clock_gettime` in a running process to intercept
//! time queries and return controlled values. Enables time offset and speed
//! manipulation without restarting the target. See [`vdso`] module.
//!
//! ## ptrace + seccomp (planned)
//!
//! Traditional syscall interception for non-time syscalls (network, randomness).
//! See [`ptrace`] and [`seccomp`] modules.

pub mod handler;
pub mod ptrace;
pub mod seccomp;
pub mod syscall;
pub mod tracer;

/// vDSO trampoline injection for deterministic time control.
/// Full injection only available on Linux; types and parsers are cross-platform.
pub mod vdso;

pub use handler::SyscallHandler;
