//! Authored declarative content catalogue for Rogue Hunter.
//!
//! Content lives in human-editable TOML files under `content/`, embedded at
//! compile time so every build (native, WASM, CI) ships byte-identical data.
//! The catalogue is pure data: `rh-core` owns the runtime rules that
//! interpret it, and `rh-gen` materialises worlds from it.

mod catalogue;
mod embedded;
mod schema;
mod strings;
mod validate;

pub use catalogue::{Catalogue, ContentError};
pub use schema::*;
pub use strings::{is_term, StringId, StringRow, StringTable};
pub use validate::referenced_ids as referenced_string_ids;

/// Load and validate the embedded content catalogue.
///
/// Every consumer (generator, simulation, clients, CI checks) goes through
/// this single entry point so all builds agree on content byte-for-byte.
/// The embedded content sources, for tests that need to perturb one file and
/// check that validation refuses the result.
pub fn embedded_sources() -> &'static [(&'static str, &'static str)] {
    embedded::SOURCES
}

/// The embedded string-table CSV, for tests that need to perturb it.
pub fn embedded_strings() -> &'static str {
    embedded::STRINGS_CSV
}

pub fn load_embedded() -> Result<Catalogue, ContentError> {
    Catalogue::from_sources(embedded::SOURCES)
}

/// A fingerprint of the embedded content bytes. Share codes carry it so a
/// replay recorded against different authored numbers fails loudly instead
/// of silently diverging.
pub fn content_fingerprint() -> u16 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for (name, source) in embedded::SOURCES {
        // Carriage returns are skipped, so the fingerprint is a statement
        // about the content rather than about the machine that compiled it.
        //
        // The content files are stored with LF and checked out with CRLF on
        // Windows, and `include_str!` embeds whatever is on disk. Hashing raw
        // bytes therefore produced one fingerprint from a Windows build and a
        // different one from Linux or wasm — and since this number rides
        // inside every ReplayRecord and is checked on load, a share code
        // recorded on one platform was refused as ContentMismatch on the
        // other. Nothing in the game logged it; the two builds simply
        // disagreed about which content they were running.
        //
        // TOML and the string table both treat the two endings alike, so this
        // changes no behaviour beyond making the number portable.
        for byte in name.bytes().chain(source.bytes()).filter(|b| *b != b'\r') {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    (hash ^ (hash >> 16) ^ (hash >> 32) ^ (hash >> 48)) as u16
}
