//! Deterministic state digests.
//!
//! Serializes with postcard (target-independent varint encoding) and hashes
//! with FNV-1a 64. The same run replayed on native and WASM must produce the
//! same digest; CI and the cross-client checks compare these.

use serde::Serialize;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Digest any serializable state (typically [`crate::state::RunState`]).
pub fn digest<T: Serialize>(value: &T) -> u64 {
    match postcard::to_allocvec(value) {
        Ok(bytes) => fnv1a(&bytes),
        // Serialization of plain-old-data state cannot fail in practice;
        // a distinguishable sentinel beats a panic in release builds.
        Err(_) => u64::MAX,
    }
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
}
