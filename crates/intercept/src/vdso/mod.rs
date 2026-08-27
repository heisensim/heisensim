//! vDSO trampoline injection for deterministic time control.
//!
//! This module patches the Linux vDSO `clock_gettime` function in a running
//! process to intercept time queries and return controlled values. This enables
//! time manipulation (offset and speed) without restarting the target process.
//!
//! ## How it works
//!
//! 1. Attach to the target process via `ptrace`
//! 2. Locate the vDSO mapping in `/proc/<pid>/maps`
//! 3. Parse the vDSO ELF to find `__vdso_clock_gettime`
//! 4. Allocate executable memory in the target via `mmap` syscall injection
//! 5. Write a payload that reads time offset from shared memory
//! 6. Overwrite `__vdso_clock_gettime` with a JMP to our payload
//! 7. Detach — the target now gets manipulated time on every call

pub mod control;
pub mod elf;
pub mod injector;
pub mod maps;
pub mod trampoline;
