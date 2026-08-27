//! Shared memory control plane for dynamic time manipulation.
//!
//! The injected payload reads from a `TimeControl` struct in shared memory
//! to determine how to manipulate time values. The heisensim process writes
//! to this shared memory to change the offset/speed dynamically.

use anyhow::{Context, Result};
use std::fmt;

/// Control struct written to shared memory, read by the injected payload.
///
/// Uses `#[repr(C)]` for stable layout across process boundaries.
/// The payload reads this atomically (fields are naturally aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TimeControl {
    /// Seconds to add to the real time.
    pub offset_seconds: i64,
    /// Nanoseconds to add to the real time (will be normalized by payload).
    pub offset_nanos: i64,
    /// Speed multiplier numerator (e.g., 10 for 10x speed).
    pub speed_numerator: u32,
    /// Speed multiplier denominator (e.g., 1 for 10x speed).
    pub speed_denominator: u32,
    /// Whether time manipulation is active: 0 = passthrough, 1 = active.
    pub enabled: u32,
    /// Padding for alignment.
    pub _pad: u32,
}

impl TimeControl {
    /// Size of the control struct in bytes.
    pub const SIZE: usize = std::mem::size_of::<Self>();

    /// Create a new `TimeControl` with the given offset. Speed defaults to 1x.
    pub fn with_offset(seconds: i64, nanos: i64) -> Self {
        Self {
            offset_seconds: seconds,
            offset_nanos: nanos,
            speed_numerator: 1,
            speed_denominator: 1,
            enabled: 1,
            _pad: 0,
        }
    }

    /// Create a new `TimeControl` with offset and speed.
    pub fn with_offset_and_speed(seconds: i64, nanos: i64, speed_num: u32, speed_den: u32) -> Self {
        Self {
            offset_seconds: seconds,
            offset_nanos: nanos,
            speed_numerator: speed_num,
            speed_denominator: speed_den,
            enabled: 1,
            _pad: 0,
        }
    }

    /// Create a disabled (passthrough) control — time is not manipulated.
    pub fn disabled() -> Self {
        Self {
            offset_seconds: 0,
            offset_nanos: 0,
            speed_numerator: 1,
            speed_denominator: 1,
            enabled: 0,
            _pad: 0,
        }
    }

    /// Serialize to bytes for writing to shared memory.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        // SAFETY: TimeControl is repr(C) with no padding issues
        unsafe { std::mem::transmute_copy(self) }
    }

    /// Deserialize from bytes read from shared memory.
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        // SAFETY: TimeControl is repr(C) with no padding issues
        unsafe { std::ptr::read(bytes.as_ptr() as *const Self) }
    }
}

impl fmt::Display for TimeControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.enabled == 0 {
            write!(f, "TimeControl(disabled)")
        } else {
            write!(
                f,
                "TimeControl(offset: {}s {}ns, speed: {}/{}x)",
                self.offset_seconds,
                self.offset_nanos,
                self.speed_numerator,
                self.speed_denominator
            )
        }
    }
}

/// Handle to an injection's shared memory segment.
///
/// Holds the original vDSO bytes so we can restore them on revert.
#[derive(Debug)]
pub struct InjectionHandle {
    /// PID of the target process.
    pub pid: u32,
    /// Address of the shared memory in the target process.
    pub shm_addr: u64,
    /// Address of the payload in the target process.
    pub payload_addr: u64,
    /// Original bytes that were overwritten by the trampoline.
    pub original_bytes: Vec<u8>,
    /// Address where the trampoline was written (start of __vdso_clock_gettime).
    pub trampoline_addr: u64,
    /// Size of the allocated region in the target.
    pub allocated_size: usize,
    /// Address of the allocated region.
    pub allocated_addr: u64,
}

/// Parse a time offset string like "+30d", "-2h", "+90m", "+3600s".
pub fn parse_time_offset(s: &str) -> Result<(i64, i64)> {
    let s = s.trim();
    let (sign, rest) = if let Some(stripped) = s.strip_prefix('+') {
        (1i64, stripped)
    } else if let Some(stripped) = s.strip_prefix('-') {
        (-1i64, stripped)
    } else {
        (1i64, s)
    };

    let (num_str, unit) = if let Some(n) = rest.strip_suffix('d') {
        (n, "d")
    } else if let Some(n) = rest.strip_suffix('h') {
        (n, "h")
    } else if let Some(n) = rest.strip_suffix('m') {
        (n, "m")
    } else if let Some(n) = rest.strip_suffix('s') {
        (n, "s")
    } else {
        (rest, "s")
    };

    let value: f64 = num_str
        .parse()
        .with_context(|| format!("invalid time offset number: '{}'", num_str))?;

    let total_seconds = match unit {
        "d" => value * 86400.0,
        "h" => value * 3600.0,
        "m" => value * 60.0,
        "s" => value,
        _ => unreachable!(),
    };

    let seconds = (total_seconds * sign as f64).trunc() as i64;
    let nanos = ((total_seconds * sign as f64).fract() * 1_000_000_000.0) as i64;

    Ok((seconds, nanos))
}

/// Parse a speed string like "10x", "0.5x", "100x".
pub fn parse_speed(s: &str) -> Result<(u32, u32)> {
    let s = s.trim().trim_end_matches('x').trim_end_matches('X');
    let value: f64 = s
        .parse()
        .with_context(|| format!("invalid speed: '{}'", s))?;

    // Convert to rational: e.g., 10x = 10/1, 0.5x = 1/2
    if value >= 1.0 {
        Ok((value as u32, 1))
    } else if value > 0.0 {
        Ok((1, (1.0 / value).round() as u32))
    } else {
        anyhow::bail!("speed must be positive, got: {}", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_control_size() {
        assert_eq!(TimeControl::SIZE, 32);
    }

    #[test]
    fn test_time_control_roundtrip() {
        let tc = TimeControl::with_offset(3600, 500_000_000);
        let bytes = tc.to_bytes();
        let tc2 = TimeControl::from_bytes(&bytes);
        assert_eq!(tc2.offset_seconds, 3600);
        assert_eq!(tc2.offset_nanos, 500_000_000);
        assert_eq!(tc2.enabled, 1);
    }

    #[test]
    fn test_time_control_disabled() {
        let tc = TimeControl::disabled();
        assert_eq!(tc.enabled, 0);
        assert_eq!(tc.offset_seconds, 0);
        let display = format!("{}", tc);
        assert!(display.contains("disabled"));
    }

    #[test]
    fn test_time_control_display() {
        let tc = TimeControl::with_offset_and_speed(86400, 0, 10, 1);
        let display = format!("{}", tc);
        assert!(display.contains("86400s"));
        assert!(display.contains("10/1x"));
    }

    #[test]
    fn test_parse_time_offset_days() {
        let (s, n) = parse_time_offset("+30d").unwrap();
        assert_eq!(s, 30 * 86400);
        assert_eq!(n, 0);
    }

    #[test]
    fn test_parse_time_offset_hours() {
        let (s, _) = parse_time_offset("-2h").unwrap();
        assert_eq!(s, -7200);
    }

    #[test]
    fn test_parse_time_offset_minutes() {
        let (s, _) = parse_time_offset("+90m").unwrap();
        assert_eq!(s, 5400);
    }

    #[test]
    fn test_parse_time_offset_seconds() {
        let (s, _) = parse_time_offset("3600s").unwrap();
        assert_eq!(s, 3600);
    }

    #[test]
    fn test_parse_speed() {
        assert_eq!(parse_speed("10x").unwrap(), (10, 1));
        assert_eq!(parse_speed("100x").unwrap(), (100, 1));
        assert_eq!(parse_speed("1x").unwrap(), (1, 1));
    }

    #[test]
    fn test_parse_speed_slow() {
        assert_eq!(parse_speed("0.5x").unwrap(), (1, 2));
    }

    #[test]
    fn test_parse_speed_invalid() {
        assert!(parse_speed("0x").is_err());
        assert!(parse_speed("-1x").is_err());
    }
}
