//! Parse vDSO ELF to locate `__vdso_clock_gettime`.
//!
//! The vDSO is a full ELF shared object mapped into every process. We parse
//! its dynamic symbol table to find the offset of `__vdso_clock_gettime`,
//! which we'll overwrite with our trampoline.

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;

/// Information about a located vDSO symbol.
#[derive(Debug, Clone)]
pub struct VdsoSymbol {
    /// Offset from the start of the vDSO mapping.
    pub offset: u64,
    /// Size of the function (if available from ELF).
    pub size: u64,
    /// The symbol name.
    pub name: String,
}

/// Names of clock_gettime symbols to search for, in priority order.
#[cfg(target_os = "linux")]
const CLOCK_GETTIME_SYMBOLS: &[&str] = &[
    "__vdso_clock_gettime",
    "__vdso_clock_gettime64", // 32-bit compat on some kernels
];

/// Find `__vdso_clock_gettime` in the vDSO ELF bytes.
///
/// The `vdso_bytes` must be the raw memory contents read from the vDSO mapping.
/// The `mapping_base` is the virtual address where the vDSO is mapped (from /proc/pid/maps).
#[cfg(target_os = "linux")]
pub fn find_clock_gettime(vdso_bytes: &[u8], mapping_base: u64) -> Result<VdsoSymbol> {
    use goblin::elf::Elf;

    let elf = Elf::parse(vdso_bytes).context("failed to parse vDSO as ELF")?;

    // Search dynamic symbols (dynsyms) — the vDSO uses dynamic linking
    for sym in &elf.dynsyms {
        if sym.st_value == 0 || !sym.is_function() {
            continue;
        }

        let name = elf.dynstrtab.get_at(sym.st_name).unwrap_or("");

        if CLOCK_GETTIME_SYMBOLS.contains(&name) {
            // Symbol value in a shared object is a virtual address relative to load base.
            // We need the offset from the mapping start.
            let offset = sym.st_value.wrapping_sub(find_load_base(&elf));

            return Ok(VdsoSymbol {
                offset,
                size: sym.st_size,
                name: name.to_string(),
            });
        }
    }

    anyhow::bail!(
        "could not find clock_gettime symbol in vDSO (searched: {:?})",
        CLOCK_GETTIME_SYMBOLS
    )
}

/// Find the load base address from the ELF program headers.
/// This is the virtual address of the first LOAD segment.
#[cfg(target_os = "linux")]
fn find_load_base(elf: &goblin::elf::Elf) -> u64 {
    for ph in &elf.program_headers {
        if ph.p_type == goblin::elf::program_header::PT_LOAD {
            return ph.p_vaddr;
        }
    }
    0
}

/// Stub for non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub fn find_clock_gettime(vdso_bytes: &[u8], mapping_base: u64) -> Result<VdsoSymbol> {
    let _ = (vdso_bytes, mapping_base);
    anyhow::bail!("vDSO interception is only supported on Linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn test_clock_gettime_symbols_order() {
        // Ensure the primary symbol is checked first
        assert_eq!(CLOCK_GETTIME_SYMBOLS[0], "__vdso_clock_gettime");
    }

    #[test]
    fn test_vdso_symbol_debug() {
        let sym = VdsoSymbol {
            offset: 0xb20,
            size: 64,
            name: "__vdso_clock_gettime".to_string(),
        };
        let dbg = format!("{:?}", sym);
        assert!(dbg.contains("__vdso_clock_gettime"));
        assert!(dbg.contains("2848")); // 0xb20 = 2848
    }
}
