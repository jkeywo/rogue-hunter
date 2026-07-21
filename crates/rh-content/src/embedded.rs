//! Compile-time embedded content sources.
//!
//! Embedding guarantees native, WASM, and CI builds run byte-identical
//! content, which the deterministic replay contract depends on.

macro_rules! content {
    ($name:literal) => {
        ($name, include_str!(concat!("../../../content/", $name)))
    };
}

pub const SOURCES: &[(&str, &str)] = &[
    content!("balance.toml"),
    content!("hunters/huntress.toml"),
    content!("hunters/occultist.toml"),
    content!("hunters/advocate.toml"),
    content!("enemies.toml"),
    content!("villains.toml"),
    content!("origins.toml"),
    content!("schemes.toml"),
    content!("items.toml"),
    content!("recipes.toml"),
    content!("clues.toml"),
    content!("npcs.toml"),
    content!("gathers.toml"),
    content!("grimoire.toml"),
    content!("guide.toml"),
    content!("ui.toml"),
    content!("openings.toml"),
    content!("machines.toml"),
    content!("events.toml"),
    content!("maps/settlement.toml"),
    content!("maps/settlement-mill.toml"),
    content!("maps/wilderness.toml"),
    content!("maps/wilderness-gorge.toml"),
    content!("maps/outlying.toml"),
    content!("maps/outlying-abbey.toml"),
];

/// The localisation string table, embedded like the rest but held apart from
/// `SOURCES` on purpose: it is excluded from `content_fingerprint`, so a copy
/// edit or a translation leaves every share code still valid. See `strings`
/// for the rule that exclusion depends on.
pub const STRINGS_CSV: &str = include_str!("../../../content/strings.csv");
