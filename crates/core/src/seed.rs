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
