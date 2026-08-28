//! Syscall definitions and variants for intercepted syscalls.
#![allow(dead_code)]

/// Represents a system call that has been intercepted by the tracing engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterceptedSyscall {
    /// `clock_gettime` syscall
    ClockGettime { clock_id: i32 },
    /// `gettimeofday` syscall
    GetTimeOfDay,
    /// `nanosleep` syscall
    Nanosleep { requested_ns: u64 },
    /// `getrandom` syscall
    GetRandom { len: usize },
    /// `socket` syscall
    Socket {
        domain: i32,
        sock_type: i32,
        protocol: i32,
    },
    /// `connect` syscall
    Connect { fd: i32, addr: Vec<u8> },
    /// `send` syscall
    Send { fd: i32, len: usize },
    /// `recv` syscall
    Recv { fd: i32, len: usize },
    /// `accept` syscall
    Accept { fd: i32 },
    /// `bind` syscall
    Bind { fd: i32, addr: Vec<u8> },
    /// Any unknown or unhandled syscall
    Unknown { syscall_nr: i64 },
}

/// The result returned to the process after intercepting a syscall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyscallResult {
    /// Allow the syscall to execute normally
    Allow,
    /// Replace the syscall return value with a custom value
    Replace(i64),
    /// Block the syscall and return an error number (errno)
    Block(i32),
    /// Delay the syscall by sleeping for the specified duration, then allow it
    Delay(std::time::Duration),
    /// Redirect the syscall (e.g. to another file descriptor or path)
    Redirect,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::SyscallHandler;

    #[test]
    fn test_intercepted_syscall_variants() {
        let s = InterceptedSyscall::GetTimeOfDay;
        assert_eq!(s, InterceptedSyscall::GetTimeOfDay);
        assert_eq!(format!("{:?}", s), "GetTimeOfDay");

        let s2 = InterceptedSyscall::Unknown { syscall_nr: 42 };
        assert_eq!(s2.clone(), InterceptedSyscall::Unknown { syscall_nr: 42 });
    }

    #[test]
    fn test_syscall_result_variants() {
        let r = SyscallResult::Allow;
        assert_eq!(r, SyscallResult::Allow);
        assert_eq!(format!("{:?}", r), "Allow");

        let r2 = SyscallResult::Replace(100);
        assert_eq!(r2.clone(), SyscallResult::Replace(100));
    }

    #[test]
    fn test_handler_new_and_unknown() {
        let mut handler = SyscallHandler::new();
        let result = handler.handle(InterceptedSyscall::Unknown { syscall_nr: 999 });
        assert_eq!(result, SyscallResult::Allow);
    }
}
