//! Deterministic simulation PRNG.
//!
//! A single PCG32 stream drives both world generation and runtime random
//! events, per the command-replay contract. The generator is the fleet's
//! unified construction from `vellum-rng` — `Pcg32::seeded` (canonical PCG
//! warm-up over a SplitMix64-mixed seed) and the Lemire bounded draw — and
//! the shared `Pcg32` type is stored directly, so the serialised generator
//! is the same `{ state, inc }` shape across the fleet.
//!
//! This replaced the pre-unification layout (a single stored state half and
//! a remainder-based draw) under the fleet decision
//! `rng-unification-breaks-saves`: every fixture in this repository was
//! re-blessed, and `REPLAY_VERSION` bumped so codes recorded before the
//! migration are refused rather than misread.

use serde::{Deserialize, Serialize};

/// The single stream selector this game uses. One stream, fixed: generation
/// and runtime draw from the same sequence, per the command-replay contract.
const STREAM: u64 = 0;

/// The simulation's PCG32, stored as the fleet's shared generator type.
///
/// A thin vocabulary wrapper: the type, seeding, and draws are `vellum-rng`'s;
/// the helper names (`percent`, `index`) are this game's. `serde(transparent)`
/// keeps the serialised shape exactly the inner `{ state, inc }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SimRng {
    inner: vellum_rng::Pcg32,
}

impl SimRng {
    pub fn new(seed: u64) -> Self {
        Self {
            inner: vellum_rng::Pcg32::seeded(seed, STREAM),
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.inner.next_u32()
    }

    /// Uniform value in `0..bound`.
    pub fn below(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "SimRng::below requires a positive bound");
        self.inner.below(bound)
    }

    /// Uniform value in the inclusive range `lo..=hi`.
    pub fn in_range(&mut self, lo: u32, hi: u32) -> u32 {
        self.inner.range_inclusive(lo, hi)
    }

    /// Roll a whole-percent chance (0 never fires, 100 always fires).
    pub fn percent(&mut self, chance: u8) -> bool {
        self.inner.chance(u32::from(chance), 100)
    }

    /// Pick an index into a slice of the given length.
    pub fn index(&mut self, len: usize) -> usize {
        self.inner.pick_index(len)
    }
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

    /// Pin the exact sequence: replay compatibility depends on it never
    /// changing. These are the fleet construction's values (`seeded(0, 0)`),
    /// pinned in vellum-rng as well, so a drift fails in both places.
    #[test]
    fn sequence_is_pinned() {
        let mut rng = SimRng::new(0);
        let first: Vec<u32> = (0..4).map(|_| rng.next_u32()).collect();
        assert_eq!(first, [3234325189, 1963755818, 1465678534, 3792411884]);
    }
}
