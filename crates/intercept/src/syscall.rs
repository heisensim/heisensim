/// Syscall definitions and variants for intercepted syscalls.

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
    /// Redirect the syscall (e.g. to another file descriptor or path)
    Redirect,
}
