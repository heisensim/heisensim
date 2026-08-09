//! # heisensim-intercept
//!
//! Syscall interception layer for heisensim. Provides the ability to intercept
//! and replace non-deterministic syscalls (time, network, randomness, I/O)
//! in target processes using Linux ptrace and seccomp-BPF.
//!
//! ## Architecture
//!
//! The interceptor acts as a supervisor process that traces target processes.
//! When a target makes a syscall that would introduce non-determinism, the
//! interceptor catches it and returns a controlled, deterministic result.
//!
//! ### Intercepted Syscalls
//!
//! | Category    | Syscalls                                          |
//! |-------------|---------------------------------------------------|
//! | Time        | clock_gettime, gettimeofday, nanosleep             |
//! | Network     | socket, connect, send, recv, accept, bind, epoll_* |
//! | Randomness  | getrandom, /dev/urandom reads                     |
//! | Process     | fork, clone, execve                                |
//! | Filesystem  | open, read, write, fsync                           |

pub mod handler;
pub mod ptrace;
pub mod seccomp;
pub mod syscall;

pub use handler::SyscallHandler;
