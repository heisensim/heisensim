//! Architecture-specific trampoline code generation.
//!
//! Generates the JMP instruction that overwrites the start of
//! `__vdso_clock_gettime` to redirect into our payload, and the
//! payload itself that reads from shared memory and returns
//! manipulated time values.

use anyhow::Result;

/// Size of the x86_64 absolute JMP trampoline (MOV RAX, addr; JMP RAX).
pub const X86_64_TRAMPOLINE_SIZE: usize = 12;

/// Size of the AArch64 absolute branch trampoline (LDR X16, [PC+8]; BR X16; addr).
pub const AARCH64_TRAMPOLINE_SIZE: usize = 16;

/// Generate an x86_64 absolute JMP trampoline.
///
/// Produces a 12-byte sequence:
/// ```asm
/// 48 B8 <8-byte addr>   ; MOV RAX, <target_addr>
/// FF E0                 ; JMP RAX
/// ```
///
/// This overwrites the first 12 bytes of `__vdso_clock_gettime`.
pub fn generate_x86_64_trampoline(target_addr: u64) -> [u8; X86_64_TRAMPOLINE_SIZE] {
    let mut trampoline = [0u8; X86_64_TRAMPOLINE_SIZE];

    // MOV RAX, imm64 — REX.W + opcode B8
    trampoline[0] = 0x48; // REX.W prefix
    trampoline[1] = 0xB8; // MOV RAX, imm64
    trampoline[2..10].copy_from_slice(&target_addr.to_le_bytes());

    // JMP RAX
    trampoline[10] = 0xFF;
    trampoline[11] = 0xE0;

    trampoline
}

/// Generate an AArch64 absolute branch trampoline.
///
/// Produces a 16-byte sequence:
/// ```asm
/// 58 00 00 50      ; LDR X16, [PC+8]  (load address from 8 bytes ahead)
/// 00 02 1F D6      ; BR X16           (branch to loaded address)
/// <8-byte addr>    ; target address literal
/// ```
pub fn generate_aarch64_trampoline(target_addr: u64) -> [u8; AARCH64_TRAMPOLINE_SIZE] {
    let mut trampoline = [0u8; AARCH64_TRAMPOLINE_SIZE];

    // LDR X16, [PC+8] — loads from the literal pool 8 bytes ahead
    trampoline[0..4].copy_from_slice(&0x5800_0058_u32.to_le_bytes());

    // BR X16
    trampoline[4..8].copy_from_slice(&0xD61F_0200_u32.to_le_bytes());

    // 8-byte target address literal
    trampoline[8..16].copy_from_slice(&target_addr.to_le_bytes());

    trampoline
}

/// Generate the trampoline for the current target architecture.
pub fn generate_trampoline(target_addr: u64) -> Result<Vec<u8>> {
    #[cfg(target_arch = "x86_64")]
    {
        Ok(generate_x86_64_trampoline(target_addr).to_vec())
    }

    #[cfg(target_arch = "aarch64")]
    {
        Ok(generate_aarch64_trampoline(target_addr).to_vec())
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = target_addr;
        anyhow::bail!("unsupported architecture for vDSO trampoline")
    }
}

/// Returns the trampoline size for the current architecture.
pub fn trampoline_size() -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        X86_64_TRAMPOLINE_SIZE
    }

    #[cfg(target_arch = "aarch64")]
    {
        AARCH64_TRAMPOLINE_SIZE
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        0
    }
}

/// The payload shellcode template for x86_64.
///
/// This function generates position-independent code that:
/// 1. Reads the `TimeControl` struct from a known shared memory address
/// 2. If `enabled == 0`, calls the original `clock_gettime` (saved bytes)
/// 3. If `enabled == 1`, calls the original, then applies offset and speed
///
/// The payload is parameterized by:
/// - `shm_addr`: address of the mapped `TimeControl` shared memory
/// - `original_bytes_addr`: address where we saved the original function bytes
/// - `original_fn_addr`: address of the original function + trampoline_size
///   (i.e., the continuation point after our JMP)
pub fn generate_x86_64_payload(
    shm_addr: u64,
    original_bytes_addr: u64,
    _original_fn_continue_addr: u64,
    _base_time_addr: u64,
) -> Vec<u8> {
    // The payload is structured as a function with C calling convention:
    //   int clock_gettime(clockid_t clk_id, struct timespec *tp)
    //
    // Arguments (System V AMD64 ABI):
    //   RDI = clk_id
    //   RSI = tp (pointer to timespec)
    //
    // Strategy:
    //   1. Save registers
    //   2. Check shm->enabled
    //   3. If disabled: execute original bytes + jump to continuation
    //   4. If enabled: call original, then apply offset
    //   5. Restore registers and return

    let mut payload = Vec::with_capacity(256);

    // --- Prologue: save callee-saved registers we'll use ---
    payload.extend_from_slice(&[
        0x55, // push rbp
        0x48, 0x89, 0xE5, // mov rbp, rsp
        0x41, 0x54, // push r12
        0x41, 0x55, // push r13
        0x53, // push rbx
        0x48, 0x83, 0xEC, 0x10, // sub rsp, 16 (align + local space)
    ]);

    // Save arguments for later
    payload.extend_from_slice(&[
        0x49, 0x89, 0xFC, // mov r12, rdi  (clk_id)
        0x49, 0x89, 0xF5, // mov r13, rsi  (tp)
    ]);

    // --- Load shm_addr into RBX, check enabled field ---
    // mov rbx, shm_addr
    payload.extend_from_slice(&[0x48, 0xBB]);
    payload.extend_from_slice(&shm_addr.to_le_bytes());

    // mov eax, [rbx + 24]  (enabled field at offset 24)
    payload.extend_from_slice(&[0x8B, 0x43, 0x18]);

    // test eax, eax
    payload.extend_from_slice(&[0x85, 0xC0]);

    // jnz .apply_offset (skip past the disabled path)
    // We'll patch this jump target after we know the offset
    let jnz_patch_offset = payload.len();
    payload.extend_from_slice(&[0x0F, 0x85, 0x00, 0x00, 0x00, 0x00]); // jnz rel32

    // --- Disabled path: call original function ---
    // Restore args
    payload.extend_from_slice(&[
        0x4C, 0x89, 0xE7, // mov rdi, r12
        0x4C, 0x89, 0xEE, // mov rsi, r13
    ]);
    // mov rax, original_fn_continue_addr - trampoline_size (i.e., original function start)
    // We need to call the original. Since we overwrote the start, we execute
    // the saved original bytes then jump to the continuation.
    // mov rax, original_bytes_addr
    payload.extend_from_slice(&[0x48, 0xB8]);
    payload.extend_from_slice(&original_bytes_addr.to_le_bytes());
    // call rax
    payload.extend_from_slice(&[0xFF, 0xD0]);

    // jmp .epilogue
    let jmp_epilogue_offset = payload.len();
    payload.extend_from_slice(&[0xE9, 0x00, 0x00, 0x00, 0x00]); // jmp rel32

    // --- Enabled path: call original then apply offset ---
    let enabled_path_offset = payload.len();

    // Patch the jnz to jump here
    let rel = (enabled_path_offset - (jnz_patch_offset + 6)) as i32;
    payload[jnz_patch_offset + 2..jnz_patch_offset + 6].copy_from_slice(&rel.to_le_bytes());

    // Call original function (via saved bytes)
    payload.extend_from_slice(&[
        0x4C, 0x89, 0xE7, // mov rdi, r12
        0x4C, 0x89, 0xEE, // mov rsi, r13
    ]);
    payload.extend_from_slice(&[0x48, 0xB8]);
    payload.extend_from_slice(&original_bytes_addr.to_le_bytes());
    payload.extend_from_slice(&[0xFF, 0xD0]);

    // Save return value
    payload.extend_from_slice(&[
        0x48, 0x89, 0xC3, // mov rbx, rax (save retval)
    ]);

    // Load shm pointer again
    // mov rcx, shm_addr
    payload.extend_from_slice(&[0x48, 0xB9]);
    payload.extend_from_slice(&shm_addr.to_le_bytes());

    // Apply offset to timespec:
    //   tp->tv_sec += shm->offset_seconds
    //   tp->tv_nsec += shm->offset_nanos
    // Then normalize (carry nanoseconds into seconds)

    // mov rax, [rcx + 0]  (offset_seconds)
    payload.extend_from_slice(&[0x48, 0x8B, 0x01]);
    // add [r13 + 0], rax  (tp->tv_sec += offset_seconds)
    payload.extend_from_slice(&[0x49, 0x01, 0x45, 0x00]);

    // mov rax, [rcx + 8]  (offset_nanos)
    payload.extend_from_slice(&[0x48, 0x8B, 0x41, 0x08]);
    // add [r13 + 8], rax  (tp->tv_nsec += offset_nanos)
    payload.extend_from_slice(&[0x49, 0x01, 0x45, 0x08]);

    // Normalize: if tv_nsec >= 1_000_000_000, carry
    // mov rax, [r13 + 8]
    payload.extend_from_slice(&[0x49, 0x8B, 0x45, 0x08]);
    // mov rdx, 1000000000
    payload.extend_from_slice(&[0x48, 0xBA]);
    payload.extend_from_slice(&1_000_000_000_i64.to_le_bytes());
    // cmp rax, rdx
    payload.extend_from_slice(&[0x48, 0x39, 0xD0]);
    // jl .no_carry
    let jl_offset = payload.len();
    payload.extend_from_slice(&[0x7C, 0x00]); // jl rel8, patch later

    // carry: tv_sec += 1, tv_nsec -= 1_000_000_000
    // sub rax, rdx
    payload.extend_from_slice(&[0x48, 0x29, 0xD0]);
    // mov [r13 + 8], rax
    payload.extend_from_slice(&[0x49, 0x89, 0x45, 0x08]);
    // add qword [r13 + 0], 1
    payload.extend_from_slice(&[0x49, 0x83, 0x45, 0x00, 0x01]);

    let no_carry_offset = payload.len();
    payload[jl_offset + 1] = (no_carry_offset - (jl_offset + 2)) as u8;

    // Restore original return value
    payload.extend_from_slice(&[
        0x48, 0x89, 0xD8, // mov rax, rbx
    ]);

    // --- Epilogue ---
    let epilogue_offset = payload.len();

    // Patch the jmp to epilogue
    let rel = (epilogue_offset - (jmp_epilogue_offset + 5)) as i32;
    payload[jmp_epilogue_offset + 1..jmp_epilogue_offset + 5].copy_from_slice(&rel.to_le_bytes());

    payload.extend_from_slice(&[
        0x48, 0x83, 0xC4, 0x10, // add rsp, 16
        0x5B, // pop rbx
        0x41, 0x5D, // pop r13
        0x41, 0x5C, // pop r12
        0x5D, // pop rbp
        0xC3, // ret
    ]);

    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x86_64_trampoline() {
        let target = 0xDEAD_BEEF_CAFE_BABEu64;
        let t = generate_x86_64_trampoline(target);

        assert_eq!(t.len(), X86_64_TRAMPOLINE_SIZE);
        // REX.W MOV RAX, imm64
        assert_eq!(t[0], 0x48);
        assert_eq!(t[1], 0xB8);
        // Little-endian address
        assert_eq!(&t[2..10], &target.to_le_bytes());
        // JMP RAX
        assert_eq!(t[10], 0xFF);
        assert_eq!(t[11], 0xE0);
    }

    #[test]
    fn test_aarch64_trampoline() {
        let target = 0xDEAD_BEEF_CAFE_BABEu64;
        let t = generate_aarch64_trampoline(target);

        assert_eq!(t.len(), AARCH64_TRAMPOLINE_SIZE);
        // LDR X16, [PC+8]
        assert_eq!(&t[0..4], &0x5800_0058_u32.to_le_bytes());
        // BR X16
        assert_eq!(&t[4..8], &0xD61F_0200_u32.to_le_bytes());
        // Address literal
        assert_eq!(&t[8..16], &target.to_le_bytes());
    }

    #[test]
    fn test_trampoline_current_arch() {
        let result = generate_trampoline(0x1234_5678_9ABC_DEF0);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_x86_64_payload_generation() {
        let payload = generate_x86_64_payload(
            0x7f00_0000_0000, // shm_addr
            0x7f00_0001_0000, // original_bytes_addr
            0x7f00_0002_000c, // original_fn_continue_addr
            0x7f00_0003_0000, // base_time_addr
        );

        // Payload should be non-empty and start with push rbp
        assert!(!payload.is_empty());
        assert_eq!(payload[0], 0x55); // push rbp

        // Payload should end with ret
        assert_eq!(*payload.last().unwrap(), 0xC3);
    }
}
