//! Authored declarative content catalogue for Rogue Hunter.
//!
//! Content lives in human-editable TOML files under `content/`, embedded at
//! compile time so every build (native, WASM, CI) ships byte-identical data.
//! The catalogue is pure data: `rh-core` owns the runtime rules that
//! interpret it, and `rh-gen` materialises worlds from it.

mod catalogue;
mod embedded;
mod schema;
mod validate;

pub use catalogue::{Catalogue, ContentError};
pub use schema::*;

/// Load and validate the embedded content catalogue.
///
/// Every consumer (generator, simulation, clients, CI checks) goes through
/// this single entry point so all builds agree on content byte-for-byte.
pub fn load_embedded() -> Result<Catalogue, ContentError> {
    Catalogue::from_sources(embedded::SOURCES)
}

/// A fingerprint of the embedded content bytes. Share codes carry it so a
/// replay recorded against different authored numbers fails loudly instead
/// of silently diverging.
pub fn content_fingerprint() -> u16 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for (name, source) in embedded::SOURCES {
        for byte in name.bytes().chain(source.bytes()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    (hash ^ (hash >> 16) ^ (hash >> 32) ^ (hash >> 48)) as u16
}
