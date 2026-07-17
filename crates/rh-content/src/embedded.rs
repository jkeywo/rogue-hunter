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
    content!("hunter.toml"),
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
    content!("ui.toml"),
    content!("maps/settlement.toml"),
    content!("maps/wilderness.toml"),
    content!("maps/outlying.toml"),
];
