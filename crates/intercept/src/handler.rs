#![allow(clippy::new_without_default)]
//! Syscall handler logic for deterministic execution.
//!
//! The handler dispatches intercepted syscalls to specific logic:
//! - Time syscalls return values from the virtual clock / time control.
//! - Network and randomness syscalls are planned for future phases.

use crate::syscall::{InterceptedSyscall, SyscallResult};
use crate::vdso::control::TimeControl;

/// SyscallHandler holds simulation state and determines how to handle
/// intercepted system calls to preserve determinism.
pub struct SyscallHandler {
    /// Time manipulation control. When set, time syscalls are intercepted.
    time_control: Option<TimeControl>,
}

impl SyscallHandler {
    /// Create a new SyscallHandler with no active interceptions.
    pub fn new() -> Self {
        Self { time_control: None }
    }

    /// Create a handler with time control enabled.
    pub fn with_time_control(control: TimeControl) -> Self {
        Self {
            time_control: Some(control),
        }
    }

    /// Set or update the time control.
    pub fn set_time_control(&mut self, control: TimeControl) {
        self.time_control = Some(control);
    }

    /// Clear the time control (passthrough mode).
    pub fn clear_time_control(&mut self) {
        self.time_control = None;
    }

    /// Handle an intercepted syscall.
    ///
    /// Returns a `SyscallResult` indicating how the syscall should be processed:
    /// - `Allow`: let it proceed normally
    /// - `Replace(value)`: override the return value
    /// - `Block(errno)`: fail the syscall with an error code
    pub fn handle(&mut self, syscall: InterceptedSyscall) -> SyscallResult {
        match syscall {
            InterceptedSyscall::ClockGettime { .. } => {
                // Time interception is handled by the vDSO trampoline payload,
                // not by ptrace-level interception. If we're here, it means the
                // process bypassed vDSO (e.g., direct syscall instruction).
                // Apply time control if configured.
                if let Some(ref tc) = self.time_control {
                    if tc.enabled != 0 {
                        // Signal to the caller that this syscall needs time manipulation.
                        // The actual manipulation happens post-syscall by modifying
                        // the timespec in the target's memory.
                        return SyscallResult::Allow;
                    }
                }
                SyscallResult::Allow
            }
            InterceptedSyscall::GetTimeOfDay => {
                // Same as ClockGettime — vDSO handles most cases
                SyscallResult::Allow
            }
            InterceptedSyscall::Nanosleep { .. } => {
                // Let nanosleep proceed normally for now.
                // Future: scale the sleep duration by time speed factor.
                SyscallResult::Allow
            }
            InterceptedSyscall::GetRandom { .. } => {
                // Future: replace with deterministic PRNG output
                SyscallResult::Allow
            }
            InterceptedSyscall::Socket { .. }
            | InterceptedSyscall::Connect { .. }
            | InterceptedSyscall::Send { .. }
            | InterceptedSyscall::Recv { .. }
            | InterceptedSyscall::Accept { .. }
            | InterceptedSyscall::Bind { .. } => {
                // Future: route through virtual network
                SyscallResult::Allow
            }
            InterceptedSyscall::Unknown { .. } => SyscallResult::Allow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscall::InterceptedSyscall;

    #[test]
    fn test_handler_new() {
        let handler = SyscallHandler::new();
        assert!(handler.time_control.is_none());
    }

    #[test]
    fn test_handler_with_time_control() {
        let tc = TimeControl::with_offset(3600, 0);
        let handler = SyscallHandler::with_time_control(tc);
        assert!(handler.time_control.is_some());
        assert_eq!(handler.time_control.unwrap().offset_seconds, 3600);
    }

    #[test]
    fn test_handler_clock_gettime_allows() {
        let mut handler = SyscallHandler::new();
        let result = handler.handle(InterceptedSyscall::ClockGettime { clock_id: 0 });
        assert!(matches!(result, SyscallResult::Allow));
    }

    #[test]
    fn test_handler_clock_gettime_with_control() {
        let tc = TimeControl::with_offset(3600, 0);
        let mut handler = SyscallHandler::with_time_control(tc);
        let result = handler.handle(InterceptedSyscall::ClockGettime { clock_id: 0 });
        assert!(matches!(result, SyscallResult::Allow));
    }

    #[test]
    fn test_handler_unknown_allows() {
        let mut handler = SyscallHandler::new();
        let result = handler.handle(InterceptedSyscall::Unknown { syscall_nr: 999 });
        assert!(matches!(result, SyscallResult::Allow));
    }

    #[test]
    fn test_handler_network_allows() {
        let mut handler = SyscallHandler::new();
        let result = handler.handle(InterceptedSyscall::Socket {
            domain: 2,
            sock_type: 1,
            protocol: 0,
        });
        assert!(matches!(result, SyscallResult::Allow));
    }

    #[test]
    fn test_set_clear_time_control() {
        let mut handler = SyscallHandler::new();
        assert!(handler.time_control.is_none());

        handler.set_time_control(TimeControl::with_offset(100, 0));
        assert!(handler.time_control.is_some());

        handler.clear_time_control();
        assert!(handler.time_control.is_none());
    }
}
