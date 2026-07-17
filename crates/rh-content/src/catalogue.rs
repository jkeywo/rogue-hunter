//! Parsing and assembly of the content catalogue from TOML sources.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::schema::*;
use crate::validate;

/// The fully parsed, cross-validated content catalogue.
#[derive(Debug, Clone)]
pub struct Catalogue {
    pub balance: Balance,
    pub hunter: HunterDef,
    pub enemies: BTreeMap<String, EnemyDef>,
    pub villains: BTreeMap<String, VillainDef>,
    pub origins: BTreeMap<String, OriginDef>,
    pub schemes: BTreeMap<String, SchemeDef>,
    pub items: BTreeMap<String, ItemDef>,
    pub recipes: BTreeMap<String, RecipeDef>,
    pub clues: BTreeMap<String, ClueTemplate>,
    pub npcs: NpcCatalogue,
    pub maps: BTreeMap<String, MapTemplate>,
    pub gathers: BTreeMap<String, GatherDef>,
    pub grimoire: Vec<GrimoireEntry>,
    pub ui: UiText,
}

#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error("content file '{file}' failed to parse: {message}")]
    Parse { file: String, message: String },
    #[error("missing content file '{0}'")]
    MissingFile(String),
    #[error("content validation failed:\n{}", issues.join("\n"))]
    Invalid { issues: Vec<String> },
}

/// TOML wrapper for `grimoire.toml`, which is a list of entries.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrimoireFile {
    entries: Vec<GrimoireEntry>,
}

impl Catalogue {
    /// Parse a catalogue from `(file name, TOML text)` pairs and validate it.
    pub fn from_sources(sources: &[(&str, &str)]) -> Result<Self, ContentError> {
        let lookup: BTreeMap<&str, &str> = sources.iter().copied().collect();
        let text = |file: &str| -> Result<&str, ContentError> {
            lookup
                .get(file)
                .copied()
                .ok_or_else(|| ContentError::MissingFile(file.to_owned()))
        };

        let mut maps = BTreeMap::new();
        for (name, source) in sources {
            if let Some(stem) = name
                .strip_prefix("maps/")
                .and_then(|n| n.strip_suffix(".toml"))
            {
                let template: MapTemplate = parse(name, source)?;
                maps.insert(stem.to_owned(), template);
            }
        }

        let grimoire_file: GrimoireFile = parse("grimoire.toml", text("grimoire.toml")?)?;
        let catalogue = Self {
            balance: parse("balance.toml", text("balance.toml")?)?,
            hunter: parse("hunter.toml", text("hunter.toml")?)?,
            enemies: parse("enemies.toml", text("enemies.toml")?)?,
            villains: parse("villains.toml", text("villains.toml")?)?,
            origins: parse("origins.toml", text("origins.toml")?)?,
            schemes: parse("schemes.toml", text("schemes.toml")?)?,
            items: parse("items.toml", text("items.toml")?)?,
            recipes: parse("recipes.toml", text("recipes.toml")?)?,
            clues: parse("clues.toml", text("clues.toml")?)?,
            npcs: parse("npcs.toml", text("npcs.toml")?)?,
            maps,
            gathers: parse("gathers.toml", text("gathers.toml")?)?,
            grimoire: grimoire_file.entries,
            ui: parse("ui.toml", text("ui.toml")?)?,
        };

        let issues = validate::validate(&catalogue);
        if issues.is_empty() {
            Ok(catalogue)
        } else {
            Err(ContentError::Invalid { issues })
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(file: &str, source: &str) -> Result<T, ContentError> {
    toml::from_str(source).map_err(|error| ContentError::Parse {
        file: file.to_owned(),
        message: error.to_string(),
    })
}
