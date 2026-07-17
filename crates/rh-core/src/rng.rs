//! Deterministic simulation PRNG.
//!
//! A single hand-rolled PCG32 stream drives both world generation and
//! runtime random events, per the command-replay contract. The constants are
//! the reference PCG-XSH-RR 64/32 parameters; implementing them in-crate
//! (rather than depending on an external RNG crate) pins the byte-exact
//! sequence across toolchain and dependency upgrades.

use serde::{Deserialize, Serialize};

const MULTIPLIER: u64 = 6364136223846793005;
const INCREMENT: u64 = 1442695040888963407;

/// PCG-XSH-RR 64/32 with a fixed stream, seeded via SplitMix64 so that
/// low-entropy user seeds (e.g. `42`) still start from well-mixed state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimRng {
    state: u64,
}

impl SimRng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: splitmix64(seed),
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform value in `0..bound` (Lemire-style rejection to avoid modulo bias).
    pub fn below(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "SimRng::below requires a positive bound");
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u32();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    /// Uniform value in the inclusive range `lo..=hi`.
    pub fn in_range(&mut self, lo: u32, hi: u32) -> u32 {
        debug_assert!(lo <= hi);
        lo + self.below(hi - lo + 1)
    }

    /// Roll a whole-percent chance (0 never fires, 100 always fires).
    pub fn percent(&mut self, chance: u8) -> bool {
        self.below(100) < u32::from(chance)
    }

    /// Pick an index into a slice of the given length.
    pub fn index(&mut self, len: usize) -> usize {
        debug_assert!(len > 0);
        self.below(len as u32) as usize
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E3779B97F4A7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D049BB133111EB);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_seeds_produce_identical_streams() {
        let mut a = SimRng::new(12345);
        let mut b = SimRng::new(12345);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = SimRng::new(1);
        let mut b = SimRng::new(2);
        let same = (0..100).filter(|_| a.next_u32() == b.next_u32()).count();
        assert!(same < 3, "streams should be effectively independent");
    }

    #[test]
    fn below_stays_in_bounds() {
        let mut rng = SimRng::new(7);
        for _ in 0..1000 {
            assert!(rng.below(6) < 6);
            let value = rng.in_range(2, 4);
            assert!((2..=4).contains(&value));
        }
    }

    /// Pin the exact sequence: replay compatibility depends on it never changing.
    #[test]
    fn sequence_is_pinned() {
        let mut rng = SimRng::new(0);
        let first: Vec<u32> = (0..4).map(|_| rng.next_u32()).collect();
        assert_eq!(first, [1092706980, 278790474, 1039822109, 1377468856]);
    }
}
