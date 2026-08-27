#![allow(clippy::new_without_default)]
//! Syscall handler logic for deterministic execution.
//!
//! The handler dispatches intercepted syscalls to specific logic:
//! - Time syscalls return values from the virtual clock / time control.
//! - Network and randomness syscalls are planned for future phases.

use crate::syscall::{InterceptedSyscall, SyscallResult};
use crate::vdso::control::TimeControl;

/// Configuration for process-level network fault injection.
#[derive(Debug, Clone, Default)]
pub struct NetworkFaultConfig {
    /// If set, `connect()` calls return this errno (e.g. libc::ECONNREFUSED = 111)
    pub connect_error: Option<i32>,
    /// If set, `socket()` calls return this errno (e.g. libc::EMFILE = 24)
    pub socket_error: Option<i32>,
    /// Optional port filter — only inject faults for connections to this port
    pub target_port: Option<u16>,
}

/// SyscallHandler holds simulation state and determines how to handle
/// intercepted system calls to preserve determinism.
pub struct SyscallHandler {
    /// Time manipulation control. When set, time syscalls are intercepted.
    time_control: Option<TimeControl>,
    /// Network fault injection config.
    network_fault: Option<NetworkFaultConfig>,
}

impl SyscallHandler {
    /// Create a new SyscallHandler with no active interceptions.
    pub fn new() -> Self {
        Self {
            time_control: None,
            network_fault: None,
        }
    }

    /// Create a handler with time control enabled.
    pub fn with_time_control(control: TimeControl) -> Self {
        Self {
            time_control: Some(control),
            network_fault: None,
        }
    }

    /// Create a handler with network fault injection enabled.
    pub fn with_network_fault(config: NetworkFaultConfig) -> Self {
        Self {
            time_control: None,
            network_fault: Some(config),
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

    /// Set or update the network fault config.
    pub fn set_network_fault(&mut self, config: NetworkFaultConfig) {
        self.network_fault = Some(config);
    }

    /// Clear the network fault config.
    pub fn clear_network_fault(&mut self) {
        self.network_fault = None;
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
            InterceptedSyscall::Socket { .. } => {
                if let Some(ref config) = self.network_fault {
                    if let Some(errno) = config.socket_error {
                        return SyscallResult::Block(errno);
                    }
                }
                SyscallResult::Allow
            }
            InterceptedSyscall::Connect { .. } => {
                if let Some(ref config) = self.network_fault {
                    if let Some(errno) = config.connect_error {
                        return SyscallResult::Block(errno);
                    }
                }
                SyscallResult::Allow
            }
            InterceptedSyscall::Send { .. }
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

    #[test]
    fn test_handler_socket_block_emfile() {
        let config = NetworkFaultConfig {
            socket_error: Some(24), // EMFILE
            ..Default::default()
        };
        let mut handler = SyscallHandler::with_network_fault(config);
        let result = handler.handle(InterceptedSyscall::Socket {
            domain: 2,
            sock_type: 1,
            protocol: 0,
        });
        assert_eq!(result, SyscallResult::Block(24));
    }

    #[test]
    fn test_handler_connect_block_econnrefused() {
        let config = NetworkFaultConfig {
            connect_error: Some(111), // ECONNREFUSED
            ..Default::default()
        };
        let mut handler = SyscallHandler::with_network_fault(config);
        let result = handler.handle(InterceptedSyscall::Connect {
            fd: 5,
            addr: vec![],
        });
        assert_eq!(result, SyscallResult::Block(111));
    }

    #[test]
    fn test_handler_socket_allows_without_fault() {
        let mut handler = SyscallHandler::new();
        let result = handler.handle(InterceptedSyscall::Socket {
            domain: 2,
            sock_type: 1,
            protocol: 0,
        });
        assert_eq!(result, SyscallResult::Allow);
    }

    #[test]
    fn test_handler_connect_allows_without_fault() {
        let mut handler = SyscallHandler::new();
        let result = handler.handle(InterceptedSyscall::Connect {
            fd: 5,
            addr: vec![],
        });
        assert_eq!(result, SyscallResult::Allow);
    }
}
