use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

/// A random seed used for deterministic simulations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SimSeed(pub u64);

impl SimSeed {
    /// Creates a new `SimSeed` from a given `u64`.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Creates a new random `SimSeed`.
    pub fn random() -> Self {
        let mut rng = rand::rng();
        Self(rng.random())
    }

    /// Creates a seeded RNG from this seed.
    pub fn rng(&self) -> impl Rng {
        StdRng::seed_from_u64(self.0)
    }
}

impl fmt::Display for SimSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "seed:0x{:016X}", self.0)
    }
}

impl FromStr for SimSeed {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_start_matches("seed:");
        if s.starts_with("0x") || s.starts_with("0X") {
            u64::from_str_radix(&s[2..], 16).map(Self)
        } else {
            s.parse::<u64>().map(Self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_sim_seed_new() {
        let seed = SimSeed::new(42);
        assert_eq!(seed.0, 42);
    }

    #[test]
    fn test_sim_seed_random() {
        let seed1 = SimSeed::random();
        let seed2 = SimSeed::random();
        assert_ne!(seed1, seed2);
    }

    #[test]
    fn test_sim_seed_rng_same_sequence() {
        let seed = SimSeed::new(12345);
        let mut rng1 = seed.rng();
        let mut rng2 = seed.rng();
        assert_eq!(rng1.random::<u64>(), rng2.random::<u64>());
        assert_eq!(rng1.random::<u64>(), rng2.random::<u64>());
    }

    #[test]
    fn test_sim_seed_rng_different_sequence() {
        let seed1 = SimSeed::new(1);
        let seed2 = SimSeed::new(2);
        let mut rng1 = seed1.rng();
        let mut rng2 = seed2.rng();
        assert_ne!(rng1.random::<u64>(), rng2.random::<u64>());
    }

    #[test]
    fn test_sim_seed_display() {
        let seed = SimSeed::new(0x123456789ABCDEF0);
        assert_eq!(seed.to_string(), "seed:0x123456789ABCDEF0");
    }

    #[test]
    fn test_sim_seed_from_str_decimal() {
        let seed = SimSeed::from_str("12345").unwrap();
        assert_eq!(seed.0, 12345);
    }

    #[test]
    fn test_sim_seed_from_str_hex_prefix() {
        let seed = SimSeed::from_str("0x1A").unwrap();
        assert_eq!(seed.0, 0x1A);
        let seed2 = SimSeed::from_str("0X1B").unwrap();
        assert_eq!(seed2.0, 0x1B);
    }

    #[test]
    fn test_sim_seed_from_str_seed_prefix() {
        let seed = SimSeed::from_str("seed:0x1A").unwrap();
        assert_eq!(seed.0, 0x1A);
        let seed2 = SimSeed::from_str("seed:12345").unwrap();
        assert_eq!(seed2.0, 12345);
    }

    #[test]
    fn test_sim_seed_from_str_invalid() {
        assert!(SimSeed::from_str("invalid").is_err());
        assert!(SimSeed::from_str("seed:invalid").is_err());
    }

    proptest! {
        #[test]
        fn test_sim_seed_roundtrip(val in any::<u64>()) {
            let seed = SimSeed::new(val);
            let s = seed.to_string();
            let parsed = SimSeed::from_str(&s).unwrap();
            assert_eq!(seed, parsed);
        }
    }
}
