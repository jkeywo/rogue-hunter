//! Deterministic state digests.
//!
//! Serializes with postcard (target-independent varint encoding) and hashes
//! with FNV-1a 64. The same run replayed on native and WASM must produce the
//! same digest; CI and the cross-client checks compare these.
//!
//! The arithmetic lives in `vellum-digest`, shared with the other game that
//! reinvented it. These remain the names rogue-hunter calls it by: the digest
//! is part of this game's save format, so it keeps a home here where the
//! pinned tests can sit next to it.

use serde::Serialize;

pub use vellum_digest::fnv1a;

/// Digest any serializable state (typically [`crate::state::RunState`]).
pub fn digest<T: Serialize>(value: &T) -> u64 {
    vellum_digest::digest_postcard(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_stable_for_equal_values() {
        #[derive(Serialize)]
        struct Sample {
            a: u32,
            b: String,
        }
        let one = Sample {
            a: 7,
            b: "wolf".into(),
        };
        let two = Sample {
            a: 7,
            b: "wolf".into(),
        };
        assert_eq!(digest(&one), digest(&two));
    }

    #[test]
    fn digest_differs_for_different_values() {
        assert_ne!(digest(&1u32), digest(&2u32));
    }

    /// The shared implementation must still be the one this game's saves were
    /// written against. Kept here as well as in vellum so that an engine
    /// change which moved the hash would fail in the consumer, not only in the
    /// crate that made it.
    #[test]
    fn fnv1a_is_unchanged_by_the_shared_crate() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
