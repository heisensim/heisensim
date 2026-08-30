//! vDSO injection orchestrator.
//!
//! Coordinates ptrace attach, vDSO discovery, trampoline injection,
//! and cleanup for a target process.

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
#[cfg(target_os = "linux")]
use tracing::{debug, info};

use super::control::{InjectionHandle, TimeControl};

/// Configuration for a vDSO injection.
#[derive(Debug, Clone)]
pub struct InjectionConfig {
    /// Target process ID.
    pub pid: u32,
    /// Time manipulation settings.
    pub time_control: TimeControl,
}

/// Orchestrates vDSO trampoline injection into a target process.
///
/// The injection flow:
/// 1. `ptrace(PTRACE_ATTACH)` to pause the target
/// 2. Parse `/proc/<pid>/maps` to find the vDSO mapping
/// 3. Read vDSO bytes via `/proc/<pid>/mem`
/// 4. Parse ELF to find `__vdso_clock_gettime` offset
/// 5. Allocate executable memory in target via `mmap` syscall injection
/// 6. Write payload + TimeControl to allocated memory
/// 7. Save original function bytes
/// 8. Overwrite function start with JMP trampoline
/// 9. `ptrace(PTRACE_DETACH)` — target resumes with patched vDSO
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn inject(config: &InjectionConfig) -> Result<InjectionHandle> {
    use super::{elf, maps, trampoline};

    let pid = config.pid;
    info!(pid, "starting vDSO injection");

    // Step 1: Attach via ptrace
    ptrace_attach(pid)?;

    // Drop guard guarantees detach even if we panic
    struct DetachGuard(u32);
    impl Drop for DetachGuard {
        fn drop(&mut self) {
            if let Err(e) = ptrace_detach(self.0) {
                // Can't use tracing in Drop during panic, but we try our best
                eprintln!("heisensim: failed to ptrace detach pid {}: {}", self.0, e);
            }
        }
    }
    let guard = DetachGuard(pid);

    // Step 2: Find vDSO mapping
    let vdso = maps::find_vdso(pid)?;
    debug!(
        pid,
        start = format!("0x{:x}", vdso.start),
        end = format!("0x{:x}", vdso.end),
        "found vDSO mapping"
    );

    // Step 3: Read vDSO bytes
    let vdso_bytes = read_proc_mem(pid, vdso.start, vdso.size())?;
    debug!(pid, size = vdso_bytes.len(), "read vDSO bytes");

    // Step 4: Find __vdso_clock_gettime
    let sym = elf::find_clock_gettime(&vdso_bytes, vdso.start)?;
    let fn_addr = vdso.start + sym.offset;
    info!(pid, symbol = %sym.name, addr = format!("0x{:x}", fn_addr), size = sym.size, "found clock_gettime");

    // Step 5: Allocate memory in target for payload + shm + saved bytes
    let tramp_size = trampoline::trampoline_size();
    let payload_code = trampoline::generate_x86_64_payload(
        0, // placeholder shm_addr — we'll patch after allocation
        0, // placeholder original_bytes_addr
        fn_addr + tramp_size as u64,
        0, // placeholder base_time_addr
    );

    // Layout of allocated region:
    //   [0..payload_size)           = payload code
    //   [payload_size..+ctrl_size)  = TimeControl shared memory
    //   [+ctrl_size..+tramp_size)   = saved original bytes
    //   [+tramp_size..+8)           = base real time snapshot
    let payload_size = payload_code.len();
    let ctrl_size = TimeControl::SIZE;
    let total_size = payload_size + ctrl_size + tramp_size + 8;

    // Allocate via ptrace mmap syscall injection
    let alloc_addr = ptrace_mmap(pid, total_size)?;
    debug!(
        pid,
        addr = format!("0x{:x}", alloc_addr),
        size = total_size,
        "allocated payload region"
    );

    let shm_addr = alloc_addr + payload_size as u64;
    let saved_bytes_addr = shm_addr + ctrl_size as u64;
    let base_time_addr = saved_bytes_addr + tramp_size as u64;

    // Re-generate payload with real addresses
    let payload_code = trampoline::generate_x86_64_payload(
        shm_addr,
        saved_bytes_addr,
        fn_addr + tramp_size as u64,
        base_time_addr,
    );

    // Step 6: Write payload to allocated region
    write_proc_mem(pid, alloc_addr, &payload_code)?;
    debug!(pid, "wrote payload code");

    // Write TimeControl
    let ctrl_bytes = config.time_control.to_bytes();
    write_proc_mem(pid, shm_addr, &ctrl_bytes)?;
    debug!(pid, "wrote TimeControl: {}", config.time_control);

    // Step 7: Save original function bytes
    let original_bytes = read_proc_mem(pid, fn_addr, tramp_size)?;
    write_proc_mem(pid, saved_bytes_addr, &original_bytes)?;
    debug!(pid, "saved {} original bytes", tramp_size);

    // Step 8: Write trampoline JMP
    let trampoline_bytes = trampoline::generate_trampoline(alloc_addr)?;
    write_proc_mem(pid, fn_addr, &trampoline_bytes)?;
    info!(
        pid,
        "trampoline installed at 0x{:x} → 0x{:x}", fn_addr, alloc_addr
    );

    let handle = InjectionHandle {
        pid,
        shm_addr,
        payload_addr: alloc_addr,
        original_bytes,
        trampoline_addr: fn_addr,
        allocated_size: total_size,
        allocated_addr: alloc_addr,
    };

    // Explicitly detach (guard consumed)
    drop(guard);

    Ok(handle)
}

/// Revert a previous injection — restore original vDSO bytes.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn revert(handle: &InjectionHandle) -> Result<()> {
    let pid = handle.pid;
    info!(pid, "reverting vDSO injection");

    ptrace_attach(pid)?;

    struct DetachGuard(u32);
    impl Drop for DetachGuard {
        fn drop(&mut self) {
            if let Err(e) = ptrace_detach(self.0) {
                eprintln!("heisensim: failed to ptrace detach pid {}: {}", self.0, e);
            }
        }
    }
    let guard = DetachGuard(pid);

    // Restore original bytes
    write_proc_mem(pid, handle.trampoline_addr, &handle.original_bytes)?;
    debug!(
        pid,
        "restored original bytes at 0x{:x}", handle.trampoline_addr
    );

    // Free allocated memory
    ptrace_munmap(pid, handle.allocated_addr, handle.allocated_size)?;
    debug!(pid, "freed allocated region");

    info!(pid, "vDSO injection reverted");

    drop(guard);
    Ok(())
}

/// Update the TimeControl in a previously injected process.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn update_time(handle: &InjectionHandle, control: &TimeControl) -> Result<()> {
    // We can write to /proc/pid/mem without ptrace if we have permission,
    // but to be safe we attach briefly.
    let pid = handle.pid;
    ptrace_attach(pid)?;

    struct DetachGuard(u32);
    impl Drop for DetachGuard {
        fn drop(&mut self) {
            let _ = ptrace_detach(self.0);
        }
    }
    let guard = DetachGuard(pid);

    let result = write_proc_mem(pid, handle.shm_addr, &control.to_bytes());
    drop(guard);
    result.context("updating TimeControl in shared memory")
}

// --- Low-level helpers ---

#[cfg(target_os = "linux")]
fn ptrace_attach(pid: u32) -> Result<()> {
    use nix::sys::{ptrace, wait::waitpid};
    use nix::unistd::Pid;

    let nix_pid = Pid::from_raw(pid as i32);
    ptrace::attach(nix_pid).with_context(|| format!("ptrace attach to pid {}", pid))?;
    waitpid(nix_pid, None).with_context(|| format!("waitpid for pid {}", pid))?;
    debug!(pid, "ptrace attached");
    Ok(())
}

#[cfg(target_os = "linux")]
fn ptrace_detach(pid: u32) -> Result<()> {
    use nix::sys::ptrace;
    use nix::unistd::Pid;

    ptrace::detach(Pid::from_raw(pid as i32), None)
        .with_context(|| format!("ptrace detach from pid {}", pid))?;
    debug!(pid, "ptrace detached");
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_proc_mem(pid: u32, addr: u64, size: usize) -> Result<Vec<u8>> {
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom};

    let mem_path = format!("/proc/{}/mem", pid);
    let mut file = OpenOptions::new()
        .read(true)
        .open(&mem_path)
        .with_context(|| format!("opening {}", mem_path))?;

    file.seek(SeekFrom::Start(addr))
        .with_context(|| format!("seeking to 0x{:x} in {}", addr, mem_path))?;

    let mut buf = vec![0u8; size];
    file.read_exact(&mut buf)
        .with_context(|| format!("reading {} bytes at 0x{:x} from {}", size, addr, mem_path))?;

    Ok(buf)
}

#[cfg(target_os = "linux")]
fn write_proc_mem(pid: u32, addr: u64, data: &[u8]) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom, Write};

    let mem_path = format!("/proc/{}/mem", pid);
    let mut file = OpenOptions::new()
        .write(true)
        .open(&mem_path)
        .with_context(|| format!("opening {} for writing", mem_path))?;

    file.seek(SeekFrom::Start(addr))
        .with_context(|| format!("seeking to 0x{:x} in {}", addr, mem_path))?;

    file.write_all(data).with_context(|| {
        format!(
            "writing {} bytes at 0x{:x} to {}",
            data.len(),
            addr,
            mem_path
        )
    })?;

    Ok(())
}

/// Inject an mmap syscall into the target process via ptrace.
///
/// This allocates a read-write-execute memory region in the target process
/// by hijacking its execution to make an `mmap` syscall, then restoring
/// the original register state.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn ptrace_mmap(pid: u32, size: usize) -> Result<u64> {
    use nix::sys::ptrace;
    use nix::unistd::Pid;

    let nix_pid = Pid::from_raw(pid as i32);

    // Save original registers
    let orig_regs = ptrace::getregs(nix_pid).context("getregs for mmap injection")?;

    // Set up mmap syscall:
    //   mmap(NULL, size, PROT_READ|PROT_WRITE|PROT_EXEC, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)
    let mut regs = orig_regs;
    regs.rax = 9; // __NR_mmap
    regs.rdi = 0; // addr = NULL
    regs.rsi = size as u64; // length
    regs.rdx = 7; // prot = PROT_READ|PROT_WRITE|PROT_EXEC
    regs.r10 = 0x22; // flags = MAP_PRIVATE|MAP_ANONYMOUS
    regs.r8 = u64::MAX; // fd = -1
    regs.r9 = 0; // offset = 0

    // We need to execute a syscall instruction. Read 2 bytes at RIP,
    // replace with 0x0F 0x05 (syscall), execute, then restore.
    let rip = orig_regs.rip;
    let saved_code = read_proc_mem(pid, rip, 2)?;

    // Write syscall instruction
    write_proc_mem(pid, rip, &[0x0F, 0x05])?;

    // Set registers and single-step
    ptrace::setregs(nix_pid, regs).context("setregs for mmap")?;
    ptrace::step(nix_pid, None).context("single-step for mmap")?;

    // Wait for the step to complete
    nix::sys::wait::waitpid(nix_pid, None).context("waitpid after mmap step")?;

    // Read the result from RAX
    let result_regs = ptrace::getregs(nix_pid).context("getregs after mmap")?;
    let mmap_result = result_regs.rax;

    // Restore original code and registers
    write_proc_mem(pid, rip, &saved_code)?;
    ptrace::setregs(nix_pid, orig_regs).context("restore regs after mmap")?;

    // Check mmap result
    if mmap_result > 0xFFFF_FFFF_FFFF_F000 {
        anyhow::bail!(
            "mmap failed in target process (error code: {})",
            -(mmap_result as i64)
        );
    }

    debug!(
        pid,
        addr = format!("0x{:x}", mmap_result),
        size,
        "mmap succeeded"
    );
    Ok(mmap_result)
}

/// Inject a munmap syscall into the target process to free allocated memory.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn ptrace_munmap(pid: u32, addr: u64, size: usize) -> Result<()> {
    use nix::sys::ptrace;
    use nix::unistd::Pid;

    let nix_pid = Pid::from_raw(pid as i32);

    let orig_regs = ptrace::getregs(nix_pid).context("getregs for munmap")?;

    let mut regs = orig_regs;
    regs.rax = 11; // __NR_munmap
    regs.rdi = addr;
    regs.rsi = size as u64;

    let rip = orig_regs.rip;
    let saved_code = read_proc_mem(pid, rip, 2)?;
    write_proc_mem(pid, rip, &[0x0F, 0x05])?;

    ptrace::setregs(nix_pid, regs).context("setregs for munmap")?;
    ptrace::step(nix_pid, None).context("single-step for munmap")?;
    nix::sys::wait::waitpid(nix_pid, None).context("waitpid after munmap step")?;

    write_proc_mem(pid, rip, &saved_code)?;
    ptrace::setregs(nix_pid, orig_regs).context("restore regs after munmap")?;

    debug!(
        pid,
        addr = format!("0x{:x}", addr),
        size,
        "munmap succeeded"
    );
    Ok(())
}

// --- Non-Linux stubs ---

#[cfg(not(target_os = "linux"))]
pub fn inject(config: &InjectionConfig) -> Result<InjectionHandle> {
    let _ = config;
    anyhow::bail!("vDSO injection is only supported on Linux")
}

#[cfg(not(target_os = "linux"))]
pub fn revert(handle: &InjectionHandle) -> Result<()> {
    let _ = handle;
    anyhow::bail!("vDSO injection is only supported on Linux")
}

#[cfg(not(target_os = "linux"))]
pub fn update_time(handle: &InjectionHandle, control: &TimeControl) -> Result<()> {
    let _ = (handle, control);
    anyhow::bail!("vDSO injection is only supported on Linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_injection_config() {
        let config = InjectionConfig {
            pid: 12345,
            time_control: TimeControl::with_offset(3600, 0),
        };
        assert_eq!(config.pid, 12345);
        assert_eq!(config.time_control.offset_seconds, 3600);
    }
}
