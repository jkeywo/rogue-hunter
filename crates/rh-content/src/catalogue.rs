//! Parsing and assembly of the content catalogue from TOML sources.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::schema::*;
use crate::validate;

/// The fully parsed, cross-validated content catalogue.
#[derive(Debug, Clone)]
pub struct Catalogue {
    pub balance: Balance,
    /// The hunter this run is being generated and played for. Everything
    /// downstream — planner, viability model, views — reads this rather than
    /// asking which hunter was chosen, so a catalogue instance is per-run.
    pub hunter: HunterDef,
    /// Id of the selected hunter, carried into the share code.
    pub hunter_id: String,
    /// Every selectable hunter, keyed by id.
    pub hunters: BTreeMap<String, HunterDef>,
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
    #[error("no hunter with id '{0}'")]
    UnknownHunter(String),
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

        let mut hunters = BTreeMap::new();
        for (name, source) in sources {
            if let Some(stem) = name
                .strip_prefix("hunters/")
                .and_then(|n| n.strip_suffix(".toml"))
            {
                let hunter: HunterDef = parse(name, source)?;
                hunters.insert(stem.to_owned(), hunter);
            }
        }
        // A run always has a selected hunter; the default is the first by id,
        // and callers override it before generating.
        let (default_id, default_hunter) = hunters
            .iter()
            .next()
            .map(|(id, hunter)| (id.clone(), hunter.clone()))
            .ok_or_else(|| ContentError::MissingFile("hunters/*.toml".to_owned()))?;

        let grimoire_file: GrimoireFile = parse("grimoire.toml", text("grimoire.toml")?)?;
        let catalogue = Self {
            balance: parse("balance.toml", text("balance.toml")?)?,
            hunter: default_hunter,
            hunter_id: default_id,
            hunters,
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

    /// Choose the hunter this run is for. Must be called before generating:
    /// route certification is per-hunter, so the choice is an input to
    /// generation rather than a costume applied afterwards.
    pub fn select_hunter(&mut self, id: &str) -> Result<(), ContentError> {
        let hunter = self
            .hunters
            .get(id)
            .ok_or_else(|| ContentError::UnknownHunter(id.to_owned()))?;
        self.hunter = hunter.clone();
        self.hunter_id = id.to_owned();
        Ok(())
    }

    /// The same catalogue with a different hunter selected.
    pub fn with_hunter(mut self, id: &str) -> Result<Self, ContentError> {
        self.select_hunter(id)?;
        Ok(self)
    }

    /// Selectable hunters in a stable order, as `(id, definition)`.
    pub fn hunter_roster(&self) -> impl Iterator<Item = (&String, &HunterDef)> {
        self.hunters.iter()
    }

    /// Template ids that can fill `role`, in a stable order. Generation picks
    /// one of these per role, so the order must not depend on iteration
    /// accidents: `BTreeMap` keeps it alphabetical and therefore reproducible.
    pub fn templates_for(&self, role: MapRole) -> Vec<&String> {
        self.maps
            .iter()
            .filter(|(_, template)| template.role == role)
            .map(|(id, _)| id)
            .collect()
    }
}

fn parse<T: serde::de::DeserializeOwned>(file: &str, source: &str) -> Result<T, ContentError> {
    toml::from_str(source).map_err(|error| ContentError::Parse {
        file: file.to_owned(),
        message: error.to_string(),
    })
}
