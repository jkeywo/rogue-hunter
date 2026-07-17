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
