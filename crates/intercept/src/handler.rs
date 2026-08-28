#![allow(clippy::new_without_default)]
//! Syscall handler logic for deterministic execution.
//!
//! The handler dispatches intercepted syscalls to specific logic:
//! - Time syscalls return values from the virtual clock / time control.
//! - Network and randomness syscalls are planned for future phases.

/// Extract the destination port from a raw `sockaddr` byte buffer.
///
/// Supports `AF_INET` (2) and `AF_INET6` (10). Returns `None` for
/// `AF_UNIX`, unsupported families, or buffers too short to contain a port.
pub fn extract_port(addr: &[u8]) -> Option<u16> {
    if addr.len() < 4 {
        return None;
    }
    let family = u16::from_ne_bytes([addr[0], addr[1]]);
    match family {
        2 | 10 => {
            // AF_INET / AF_INET6: port is at bytes 2..4 in network byte order
            Some(u16::from_be_bytes([addr[2], addr[3]]))
        }
        _ => None, // AF_UNIX, AF_NETLINK, etc. — no IP port
    }
}

use crate::syscall::{InterceptedSyscall, SyscallResult};
use crate::vdso::control::TimeControl;

/// Configuration for process-level network fault injection.
#[derive(Debug, Clone, Default)]
pub struct NetworkFaultConfig {
    /// If set, `connect()` calls return this errno (e.g. libc::ECONNREFUSED = 111)
    pub connect_error: Option<i32>,
    /// If set, `socket()` calls return this errno (e.g. libc::EMFILE = 24)
    pub socket_error: Option<i32>,
    /// If set, delay `connect()` calls by this many milliseconds, then allow
    pub connect_latency_ms: Option<u64>,
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
            InterceptedSyscall::Connect { ref addr, .. } => {
                if let Some(ref config) = self.network_fault {
                    // Check port filter — skip fault if port doesn't match
                    if let Some(target_port) = config.target_port {
                        match extract_port(addr) {
                            Some(port) if port == target_port => {} // match — continue to fault
                            _ => return SyscallResult::Allow,       // no match or can't parse
                        }
                    }
                    if let Some(errno) = config.connect_error {
                        return SyscallResult::Block(errno);
                    }
                    if let Some(ms) = config.connect_latency_ms {
                        return SyscallResult::Delay(std::time::Duration::from_millis(ms));
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

    #[test]
    fn test_handler_connect_delay_with_latency() {
        let config = NetworkFaultConfig {
            connect_latency_ms: Some(200),
            ..Default::default()
        };
        let mut handler = SyscallHandler::with_network_fault(config);
        let result = handler.handle(InterceptedSyscall::Connect {
            fd: 5,
            addr: vec![],
        });
        assert_eq!(
            result,
            SyscallResult::Delay(std::time::Duration::from_millis(200))
        );
    }

    #[test]
    fn test_handler_connect_error_takes_precedence_over_latency() {
        let config = NetworkFaultConfig {
            connect_error: Some(111),
            connect_latency_ms: Some(200),
            ..Default::default()
        };
        let mut handler = SyscallHandler::with_network_fault(config);
        let result = handler.handle(InterceptedSyscall::Connect {
            fd: 5,
            addr: vec![],
        });
        // Block should take precedence over Delay
        assert_eq!(result, SyscallResult::Block(111));
    }

    #[test]
    fn test_extract_port_ipv4() {
        // AF_INET (2) sockaddr_in: family=2, port=443 (0x01BB)
        let mut addr = vec![0u8; 16];
        let family: u16 = 2; // AF_INET
        addr[0..2].copy_from_slice(&family.to_ne_bytes());
        addr[2..4].copy_from_slice(&443u16.to_be_bytes());
        assert_eq!(extract_port(&addr), Some(443));
    }

    #[test]
    fn test_extract_port_ipv6() {
        // AF_INET6 (10) sockaddr_in6: family=10, port=8080
        let mut addr = vec![0u8; 28];
        let family: u16 = 10; // AF_INET6
        addr[0..2].copy_from_slice(&family.to_ne_bytes());
        addr[2..4].copy_from_slice(&8080u16.to_be_bytes());
        assert_eq!(extract_port(&addr), Some(8080));
    }

    #[test]
    fn test_extract_port_unix_socket() {
        // AF_UNIX (1) — no IP port
        let mut addr = vec![0u8; 16];
        let family: u16 = 1;
        addr[0..2].copy_from_slice(&family.to_ne_bytes());
        assert_eq!(extract_port(&addr), None);
    }

    #[test]
    fn test_extract_port_too_short() {
        assert_eq!(extract_port(&[]), None);
        assert_eq!(extract_port(&[2, 0]), None); // family only, no port
        assert_eq!(extract_port(&[2, 0, 0]), None); // 3 bytes, need 4
    }

    #[test]
    fn test_handler_connect_port_filter_match() {
        // Port matches target — should fault
        let config = NetworkFaultConfig {
            connect_error: Some(111),
            target_port: Some(5432),
            ..Default::default()
        };
        let mut handler = SyscallHandler::with_network_fault(config);
        let mut addr = vec![0u8; 16];
        addr[0..2].copy_from_slice(&2u16.to_ne_bytes()); // AF_INET
        addr[2..4].copy_from_slice(&5432u16.to_be_bytes());
        let result = handler.handle(InterceptedSyscall::Connect { fd: 5, addr });
        assert_eq!(result, SyscallResult::Block(111));
    }

    #[test]
    fn test_handler_connect_port_filter_no_match() {
        // Port doesn't match target — should allow
        let config = NetworkFaultConfig {
            connect_error: Some(111),
            target_port: Some(5432),
            ..Default::default()
        };
        let mut handler = SyscallHandler::with_network_fault(config);
        let mut addr = vec![0u8; 16];
        addr[0..2].copy_from_slice(&2u16.to_ne_bytes()); // AF_INET
        addr[2..4].copy_from_slice(&6379u16.to_be_bytes()); // Redis, not Postgres
        let result = handler.handle(InterceptedSyscall::Connect { fd: 5, addr });
        assert_eq!(result, SyscallResult::Allow);
    }

    #[test]
    fn test_handler_connect_no_port_filter() {
        // No port filter — should fault all connections
        let config = NetworkFaultConfig {
            connect_error: Some(111),
            target_port: None,
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
    fn test_handler_connect_port_filter_unix_socket_allows() {
        // Unix socket with port filter set — can't parse port, should allow
        let config = NetworkFaultConfig {
            connect_error: Some(111),
            target_port: Some(5432),
            ..Default::default()
        };
        let mut handler = SyscallHandler::with_network_fault(config);
        let mut addr = vec![0u8; 16];
        addr[0..2].copy_from_slice(&1u16.to_ne_bytes()); // AF_UNIX
        let result = handler.handle(InterceptedSyscall::Connect { fd: 5, addr });
        assert_eq!(result, SyscallResult::Allow);
    }
}
