//! Deterministic simulation PRNG.
//!
//! A single PCG32 stream drives both world generation and runtime random
//! events, per the command-replay contract. The constants are the reference
//! PCG-XSH-RR 64/32 parameters; keeping them out of the `rand` ecosystem pins
//! the byte-exact sequence across toolchain and dependency upgrades.
//!
//! The arithmetic lives in `vellum-rng`, shared with the other game that wrote
//! the same generator for the same reason. The *layout* stays here: only the
//! state half is stored, which is the shape `RunState` has always had, and
//! `RunState` is serialised into every share code. Storing the shared type
//! would add its increment to the postcard bytes and invalidate every code a
//! player has saved, so the crate is borrowed a draw at a time.
//!
//! Note that the bounded draw here is rejection-then-remainder, and the other
//! game's is Lemire's multiply-and-shift. They compute the same rejection
//! threshold, which makes them look interchangeable in a diff; they are not.

use serde::{Deserialize, Serialize};

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
        self.borrow(vellum_rng::Pcg32::next_u32)
    }

    /// Uniform value in `0..bound` (rejection, then remainder).
    pub fn below(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "SimRng::below requires a positive bound");
        self.borrow(|rng| rng.below_modulo(bound))
    }

    /// Run one draw on the shared generator and take the advanced state back.
    ///
    /// Only the state half is stored, because that is the shape `RunState` has
    /// always had and `RunState` is serialised into every share code. Adopting
    /// `vellum_rng::Pcg32` as a field would add its increment to the postcard
    /// bytes and invalidate every code a player has saved, so the arithmetic
    /// is borrowed and the layout stays here.
    fn borrow<T>(&mut self, draw: impl FnOnce(&mut vellum_rng::Pcg32) -> T) -> T {
        let mut rng = vellum_rng::Pcg32::from_parts(self.state, INCREMENT);
        let result = draw(&mut rng);
        self.state = rng.into_parts().0;
        result
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

fn splitmix64(value: u64) -> u64 {
    vellum_rng::split_mix_64(value)
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
