//! Parse `/proc/<pid>/maps` to locate the vDSO mapping.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Describes the memory mapping of the vDSO in a process.
#[derive(Debug, Clone)]
pub struct VdsoMapping {
    /// Start address of the vDSO mapping.
    pub start: u64,
    /// End address of the vDSO mapping.
    pub end: u64,
}

impl VdsoMapping {
    /// Size of the vDSO mapping in bytes.
    pub fn size(&self) -> usize {
        (self.end - self.start) as usize
    }
}

/// Find the vDSO mapping for a given process by parsing `/proc/<pid>/maps`.
///
/// The vDSO line looks like:
/// ```text
/// 7fff12345000-7fff12346000 r-xp 00000000 00:00 0   [vdso]
/// ```
pub fn find_vdso(pid: u32) -> Result<VdsoMapping> {
    let maps_path = format!("/proc/{}/maps", pid);
    find_vdso_in(Path::new(&maps_path))
}

/// Parse a maps file to find the vDSO mapping. Extracted for testability.
pub fn find_vdso_in(maps_path: &Path) -> Result<VdsoMapping> {
    let contents = fs::read_to_string(maps_path)
        .with_context(|| format!("reading {}", maps_path.display()))?;
    parse_vdso_from_maps(&contents)
}

/// Parse the vDSO mapping from maps file content.
pub fn parse_vdso_from_maps(maps_content: &str) -> Result<VdsoMapping> {
    for line in maps_content.lines() {
        if !line.contains("[vdso]") {
            continue;
        }

        // Format: "start-end perms offset dev inode pathname"
        let addr_range = line.split_whitespace().next().context("empty maps line")?;

        let (start_hex, end_hex) = addr_range
            .split_once('-')
            .context("invalid address range format")?;

        let start = u64::from_str_radix(start_hex, 16).context("invalid start address hex")?;
        let end = u64::from_str_radix(end_hex, 16).context("invalid end address hex")?;

        return Ok(VdsoMapping { start, end });
    }

    anyhow::bail!("no [vdso] mapping found in process maps")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MAPS: &str = "\
55a1b2c3d000-55a1b2c5e000 r--p 00000000 08:01 1234567  /usr/bin/myapp
55a1b2c5e000-55a1b2d8a000 r-xp 00021000 08:01 1234567  /usr/bin/myapp
7f8a12000000-7f8a12022000 r--p 00000000 08:01 2345678  /usr/lib/libc.so.6
7fff9a5fe000-7fff9a600000 r-xp 00000000 00:00 0        [vdso]
7fff9a600000-7fff9a602000 r--p 00000000 00:00 0        [vvar]
";

    #[test]
    fn test_parse_vdso_found() {
        let mapping = parse_vdso_from_maps(SAMPLE_MAPS).unwrap();
        assert_eq!(mapping.start, 0x7fff9a5fe000);
        assert_eq!(mapping.end, 0x7fff9a600000);
        assert_eq!(mapping.size(), 0x2000);
    }

    #[test]
    fn test_parse_vdso_not_found() {
        let maps = "55a1b2c3d000-55a1b2c5e000 r--p 00000000 08:01 1234567  /usr/bin/myapp\n";
        assert!(parse_vdso_from_maps(maps).is_err());
    }

    #[test]
    fn test_parse_vdso_empty() {
        assert!(parse_vdso_from_maps("").is_err());
    }
}
