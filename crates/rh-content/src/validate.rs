//! Cross-reference and contract validation for the content catalogue.
//!
//! These checks run in every build that loads content and as a dedicated CI
//! gate, so hand-edited content fails loudly with author-readable messages.

use crate::catalogue::Catalogue;
use crate::schema::*;

pub const MAP_WIDTH: usize = 32;
pub const MAP_HEIGHT: usize = 20;

/// Validate the whole catalogue, returning every issue found.
pub fn validate(cat: &Catalogue) -> Vec<String> {
    let mut issues = Vec::new();
    check_balance(cat, &mut issues);
    check_hunter(cat, &mut issues);
    check_items_and_recipes(cat, &mut issues);
    check_enemies(cat, &mut issues);
    check_villains(cat, &mut issues);
    check_schemes(cat, &mut issues);
    check_clues(cat, &mut issues);
    check_npcs(cat, &mut issues);
    check_maps(cat, &mut issues);
    check_gathers(cat, &mut issues);
    check_grimoire(cat, &mut issues);
    check_ui(cat, &mut issues);
    issues
}

fn check_gathers(cat: &Catalogue, issues: &mut Vec<String>) {
    for (id, gather) in &cat.gathers {
        let Some(map) = cat.maps.get(&gather.map) else {
            issues.push(format!(
                "gathers: '{id}' references unknown map '{}'",
                gather.map
            ));
            continue;
        };
        if !map.slots.iter().any(|slot| slot.id == gather.slot) {
            issues.push(format!(
                "gathers: '{id}' references unknown slot '{}' on map '{}'",
                gather.slot, gather.map
            ));
        }
        for item in &gather.items {
            if !cat.items.contains_key(item) {
                issues.push(format!("gathers: '{id}' grants unknown item '{item}'"));
            }
        }
        if gather.items.is_empty() {
            issues.push(format!("gathers: '{id}' grants nothing"));
        }
        if gather.pool.is_some() && gather.cost == 0 {
            issues.push(format!("gathers: '{id}' names a pool but costs 0"));
        }
        match &gather.discovery {
            GatherDiscovery::RevealedByClue { clue } | GatherDiscovery::SightOrClue { clue } => {
                if !cat.clues.contains_key(clue) {
                    issues.push(format!("gathers: '{id}' revealed by unknown clue '{clue}'"));
                }
            }
            GatherDiscovery::Sight => {}
        }
    }
    // Enough raw ingredient supply must exist for every recipe path the
    // planner relies on: two draughts, one charm, and silver by two routes.
    for required in ["moon-herb", "silver"] {
        let sources = cat
            .gathers
            .values()
            .filter(|gather| gather.items.iter().any(|item| item == required))
            .count();
        if sources < 2 {
            issues.push(format!(
                "gathers: ingredient '{required}' needs at least two gather sources, found {sources}"
            ));
        }
    }
}

fn check_balance(cat: &Catalogue, issues: &mut Vec<String>) {
    let b = &cat.balance;
    for (label, value) in [
        ("combat.melee_hit_percent", b.combat.melee_hit_percent),
        ("combat.ranged_hit_percent", b.combat.ranged_hit_percent),
        (
            "combat.trapped_attack_penalty_percent",
            b.combat.trapped_attack_penalty_percent,
        ),
        (
            "combat.pounce_attack_bonus_percent",
            b.combat.pounce_attack_bonus_percent,
        ),
        (
            "combat.killing_blow_health_percent",
            b.combat.killing_blow_health_percent,
        ),
        ("loot.drop_percent", b.loot.drop_percent),
        (
            "generator.ambush_percent_min",
            b.generator.ambush_percent_min,
        ),
        (
            "generator.ambush_percent_max",
            b.generator.ambush_percent_max,
        ),
    ] {
        if value > 100 {
            issues.push(format!("balance: {label} is {value}, above 100 percent"));
        }
    }
    if b.generator.ambush_percent_min > b.generator.ambush_percent_max {
        issues.push("balance: generator.ambush_percent_min exceeds ambush_percent_max".into());
    }
    // Spec bounds for the combat-viability tuning entity: 0.50 to 0.95.
    let threshold = b.generator.viability_threshold_permille;
    if !(500..=950).contains(&threshold) {
        issues.push(format!(
            "balance: generator.viability_threshold_permille is {threshold}, outside the \
             PASM tuning bounds 500..=950"
        ));
    }
    if b.generator.early_route_deadline > b.generator.fallback_route_deadline {
        issues.push("balance: early_route_deadline is later than fallback_route_deadline".into());
    }
    if b.generator.fallback_route_deadline >= b.clock.travel_turns {
        issues.push("balance: fallback_route_deadline must fall before the final hunt".into());
    }
    if b.clock.minor_event_turn >= b.clock.major_event_turn
        || b.clock.major_event_turn >= b.clock.travel_turns
    {
        issues.push("balance: scheme event turns must satisfy minor < major < travel_turns".into());
    }
}

fn check_hunter(cat: &Catalogue, issues: &mut Vec<String>) {
    for item in &cat.hunter.starting_items {
        if !cat.items.contains_key(item) {
            issues.push(format!(
                "hunter: starting item '{item}' is not in items.toml"
            ));
        }
    }
    if cat.hunter.mystic_cap != 0 {
        // The MVP hunter is fixed; the over-cap Mystic point comes from the favour.
        issues.push("hunter: the MVP Huntress must have mystic_cap 0 per the spec".into());
    }
    let mut ability_ids: Vec<&str> = cat
        .hunter
        .manoeuvres
        .iter()
        .map(|m| m.id.as_str())
        .chain(cat.hunter.signatures.iter().map(|s| s.id.as_str()))
        .collect();
    ability_ids.sort_unstable();
    ability_ids.dedup();
    if ability_ids.len() != cat.hunter.manoeuvres.len() + cat.hunter.signatures.len() {
        issues.push("hunter: manoeuvre/signature ids must be unique".into());
    }
    for manoeuvre in &cat.hunter.manoeuvres {
        if manoeuvre.stamina_cost > cat.hunter.stamina_cap {
            issues.push(format!(
                "hunter: manoeuvre '{}' costs more stamina than the cap",
                manoeuvre.id
            ));
        }
    }
    for signature in &cat.hunter.signatures {
        if signature.physical_cost > cat.hunter.physical_cap {
            issues.push(format!(
                "hunter: signature '{}' costs more Physical than the cap",
                signature.id
            ));
        }
    }
}

fn check_items_and_recipes(cat: &Catalogue, issues: &mut Vec<String>) {
    for (id, item) in &cat.items {
        if let ItemKind::RangedWeapon { ammo, .. } = &item.kind {
            match cat.items.get(ammo).map(|a| &a.kind) {
                Some(ItemKind::Ammunition) | Some(ItemKind::WeaknessAmmunition { .. }) => {}
                Some(_) => issues.push(format!(
                    "items: '{id}' ammo '{ammo}' is not an ammunition kind"
                )),
                None => issues.push(format!("items: '{id}' references unknown ammo '{ammo}'")),
            }
        }
    }
    for (id, recipe) in &cat.recipes {
        for input in &recipe.inputs {
            if !cat.items.contains_key(input) {
                issues.push(format!(
                    "recipes: '{id}' input '{input}' is not in items.toml"
                ));
            }
        }
        if !cat.items.contains_key(&recipe.output) {
            issues.push(format!(
                "recipes: '{id}' output '{}' is not in items.toml",
                recipe.output
            ));
        }
        if recipe.inputs.is_empty() {
            issues.push(format!("recipes: '{id}' has no inputs"));
        }
    }
}

fn check_enemies(cat: &Catalogue, issues: &mut Vec<String>) {
    for (id, enemy) in &cat.enemies {
        if enemy.health == 0 {
            issues.push(format!("enemies: '{id}' has zero health"));
        }
        if enemy.hit_percent > 100 {
            issues.push(format!("enemies: '{id}' hit_percent above 100"));
        }
        if let Some(ranged) = &enemy.ranged {
            if ranged.hit_percent > 100 || ranged.range == 0 {
                issues.push(format!("enemies: '{id}' has an invalid ranged profile"));
            }
        }
    }
}

fn check_villains(cat: &Catalogue, issues: &mut Vec<String>) {
    for (id, villain) in &cat.villains {
        if !cat.items.contains_key(&villain.weakness_item) {
            issues.push(format!(
                "villains: '{id}' weakness item '{}' is not in items.toml",
                villain.weakness_item
            ));
        }
        if villain.tier_behaviours.len() < 2 {
            issues.push(format!(
                "villains: '{id}' needs enhanced behaviours for both threat tiers"
            ));
        }
        match villain.concealment {
            Concealment::NpcHost => {
                if !cat.npcs.archetypes.values().any(|npc| npc.can_host_villain) {
                    issues.push(format!(
                        "villains: '{id}' hides in an NPC but no archetype can_host_villain"
                    ));
                }
                if villain.pounce.is_none() {
                    issues.push(format!(
                        "villains: NPC-host villain '{id}' must have a pounce"
                    ));
                }
            }
            Concealment::DormantGrave => {
                if villain.cadence.is_none() {
                    issues.push(format!(
                        "villains: grave villain '{id}' must have a vulnerability cadence"
                    ));
                }
            }
        }
        if villain.affected_by_consecration && villain.cadence.is_none() {
            issues.push(format!(
                "villains: '{id}' is affected by consecration but has no cadence to override"
            ));
        }
    }
}

fn check_schemes(cat: &Catalogue, issues: &mut Vec<String>) {
    for (id, scheme) in &cat.schemes {
        if !cat.enemies.contains_key(&scheme.minion_enemy) {
            issues.push(format!(
                "schemes: '{id}' minion enemy '{}' is not in enemies.toml",
                scheme.minion_enemy
            ));
        }
        for (label, event) in [
            ("minor", &scheme.minor_event),
            ("major", &scheme.major_event),
        ] {
            if !cat.maps.contains_key(&event.site_map) {
                issues.push(format!(
                    "schemes: '{id}' {label} event site map '{}' does not exist",
                    event.site_map
                ));
            }
        }
    }
}

fn check_clues(cat: &Catalogue, issues: &mut Vec<String>) {
    for (id, clue) in &cat.clues {
        if clue.archetype != "any" && !cat.villains.contains_key(&clue.archetype) {
            issues.push(format!(
                "clues: '{id}' archetype '{}' is unknown",
                clue.archetype
            ));
        }
        for origin in &clue.origins {
            if !cat.origins.contains_key(origin) {
                issues.push(format!("clues: '{id}' origin '{origin}' is unknown"));
            }
        }
        if clue.obscurity > 3 {
            issues.push(format!("clues: '{id}' obscurity must be 0..=3"));
        }
        let pool_matches = matches!(
            (clue.action, clue.pool),
            (
                OpportunityAction::Examine | OpportunityAction::Track,
                PoolKind::Lore
            ) | (
                OpportunityAction::Gossip | OpportunityAction::Persuade | OpportunityAction::Spy,
                PoolKind::Social
            ) | (OpportunityAction::Commune, PoolKind::Mystic)
                | (OpportunityAction::Force, PoolKind::Physical)
                | (
                    OpportunityAction::Scavenge,
                    PoolKind::Lore | PoolKind::Physical
                )
        );
        if !pool_matches {
            issues.push(format!(
                "clues: '{id}' action {:?} does not match pool {:?}",
                clue.action, clue.pool
            ));
        }
    }
    // Every villain x origin combination needs enough raw material: at least
    // four identity clues (early route pair + fallback pair) and one location clue.
    for villain_id in cat.villains.keys() {
        for origin_id in cat.origins.keys() {
            let fits = |clue: &ClueTemplate| {
                (clue.archetype == "any" || clue.archetype == *villain_id)
                    && (clue.origins.is_empty() || clue.origins.iter().any(|o| o == origin_id))
            };
            let identity = cat
                .clues
                .values()
                .filter(|c| fits(c) && c.category == ClueCategory::Identity)
                .count();
            let location = cat
                .clues
                .values()
                .filter(|c| fits(c) && c.category == ClueCategory::Location)
                .count();
            if identity < 4 {
                issues.push(format!(
                    "clues: villain '{villain_id}' origin '{origin_id}' has only {identity} \
                     identity clue templates; the generator needs at least 4"
                ));
            }
            if location < 1 {
                issues.push(format!(
                    "clues: villain '{villain_id}' origin '{origin_id}' has no location clue"
                ));
            }
        }
    }
}

fn check_npcs(cat: &Catalogue, issues: &mut Vec<String>) {
    let slot_ids: Vec<&str> = cat
        .maps
        .values()
        .flat_map(|map| map.slots.iter().map(|slot| slot.id.as_str()))
        .collect();
    for (id, npc) in &cat.npcs.archetypes {
        if npc.name_pool.is_empty() {
            issues.push(format!("npcs: archetype '{id}' has an empty name pool"));
        }
        if !slot_ids.contains(&npc.work_slot.as_str()) {
            issues.push(format!(
                "npcs: archetype '{id}' work slot '{}' not found",
                npc.work_slot
            ));
        }
        for secret in &npc.secrets {
            if !cat.npcs.secrets.contains_key(secret) {
                issues.push(format!(
                    "npcs: archetype '{id}' secret '{secret}' is unknown"
                ));
            }
        }
        if npc.secrets.is_empty() {
            issues.push(format!(
                "npcs: archetype '{id}' needs at least one secret template"
            ));
        }
    }
    if cat.npcs.archetypes.len() < 4 {
        issues.push("npcs: need at least 4 archetypes so three-NPC casts can vary".into());
    }
    if !cat.npcs.archetypes.values().any(|npc| npc.mystical) {
        issues.push("npcs: at least one archetype must be mystical for the favour route".into());
    }
    for (id, secret) in &cat.npcs.secrets {
        match (secret.false_secret, &secret.disproof) {
            (true, None) => issues.push(format!("npcs: false secret '{id}' must carry a disproof")),
            (false, Some(_)) => issues.push(format!(
                "npcs: true secret '{id}' must not carry a disproof"
            )),
            _ => {}
        }
    }
    if cat.npcs.relationship_kinds.len() < 3 {
        issues.push("npcs: need at least 3 relationship kinds for varied links".into());
    }
}

fn check_maps(cat: &Catalogue, issues: &mut Vec<String>) {
    let expected_roles = [
        ("settlement", MapRole::Settlement),
        ("wilderness", MapRole::Wilderness),
        ("outlying", MapRole::OutlyingSite),
    ];
    for (map_id, role) in expected_roles {
        match cat.maps.get(map_id) {
            None => issues.push(format!("maps: required map '{map_id}' is missing")),
            Some(map) if map.role != role => {
                issues.push(format!("maps: '{map_id}' must have role {role:?}"))
            }
            Some(_) => {}
        }
    }

    for (id, map) in &cat.maps {
        if map.rows.len() != MAP_HEIGHT {
            issues.push(format!(
                "maps: '{id}' has {} rows, expected {MAP_HEIGHT}",
                map.rows.len()
            ));
        }
        for (y, row) in map.rows.iter().enumerate() {
            let width = row.chars().count();
            if width != MAP_WIDTH {
                issues.push(format!(
                    "maps: '{id}' row {y} has {width} glyphs, expected {MAP_WIDTH}"
                ));
            }
            for (x, glyph) in row.chars().enumerate() {
                if !map.legend.contains_key(&glyph) {
                    issues.push(format!(
                        "maps: '{id}' glyph '{glyph}' at {x},{y} not in legend"
                    ));
                }
            }
        }
        let terrain_at = |at: Coord| -> Option<Terrain> {
            map.rows
                .get(at[1] as usize)
                .and_then(|row| row.chars().nth(at[0] as usize))
                .and_then(|glyph| map.legend.get(&glyph))
                .copied()
        };
        let mut slot_ids: Vec<&str> = map.slots.iter().map(|s| s.id.as_str()).collect();
        slot_ids.sort_unstable();
        slot_ids.dedup();
        if slot_ids.len() != map.slots.len() {
            issues.push(format!("maps: '{id}' has duplicate slot ids"));
        }
        for slot in &map.slots {
            match terrain_at(slot.at) {
                None => issues.push(format!(
                    "maps: '{id}' slot '{}' is out of bounds at {},{}",
                    slot.id, slot.at[0], slot.at[1]
                )),
                Some(terrain) => {
                    let ok = match slot.kind {
                        SiteKind::Grave => terrain == Terrain::Grave,
                        SiteKind::Church => {
                            matches!(terrain, Terrain::Altar | Terrain::Floor)
                        }
                        SiteKind::Workstation => terrain == Terrain::Workstation,
                        _ => is_walkable(terrain) || is_forceable(terrain),
                    };
                    if !ok {
                        issues.push(format!(
                            "maps: '{id}' slot '{}' ({:?}) sits on incompatible terrain {terrain:?}",
                            slot.id, slot.kind
                        ));
                    }
                }
            }
        }
        for spawn in &map.initial_enemies {
            if !cat.enemies.contains_key(&spawn.enemy) {
                issues.push(format!(
                    "maps: '{id}' spawns unknown enemy '{}'",
                    spawn.enemy
                ));
            }
            if !map.slots.iter().any(|slot| slot.id == spawn.near_slot) {
                issues.push(format!(
                    "maps: '{id}' spawns near unknown slot '{}'",
                    spawn.near_slot
                ));
            }
        }
        for exit in &map.exits {
            if !cat.maps.contains_key(&exit.to) {
                issues.push(format!(
                    "maps: '{id}' exit leads to unknown map '{}'",
                    exit.to
                ));
            }
            match terrain_at(exit.at) {
                Some(terrain) if is_walkable(terrain) => {}
                _ => issues.push(format!(
                    "maps: '{id}' exit to '{}' must sit on walkable terrain",
                    exit.to
                )),
            }
        }
        if (map.cover_pockets.len() as u8) < cat.balance.generator.min_cover_pockets_per_map {
            issues.push(format!(
                "maps: '{id}' reserves {} cover pockets, needs at least {}",
                map.cover_pockets.len(),
                cat.balance.generator.min_cover_pockets_per_map
            ));
        }
        for (index, pocket) in map.cover_pockets.iter().enumerate() {
            if pocket.tiles.is_empty() {
                issues.push(format!("maps: '{id}' cover pocket {index} is empty"));
            }
            for tile in &pocket.tiles {
                match terrain_at(*tile) {
                    Some(terrain) if is_opaque(terrain) => {}
                    _ => issues.push(format!(
                        "maps: '{id}' cover pocket {index} tile {},{} is not opaque cover",
                        tile[0], tile[1]
                    )),
                }
            }
        }
    }

    // The three maps must form a triangle of paired exits.
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for (id, map) in &cat.maps {
        for exit in &map.exits {
            pairs.push((id.as_str(), exit.to.as_str()));
        }
    }
    for (from, to) in &pairs {
        let reciprocal = pairs.iter().filter(|(f, t)| f == to && t == from).count();
        let forward = pairs.iter().filter(|(f, t)| f == from && t == to).count();
        if reciprocal != forward {
            issues.push(format!(
                "maps: exits between '{from}' and '{to}' are not paired"
            ));
        }
    }
    for (a, b) in [
        ("settlement", "wilderness"),
        ("wilderness", "outlying"),
        ("settlement", "outlying"),
    ] {
        if !pairs.iter().any(|(f, t)| *f == a && *t == b) {
            issues.push(format!(
                "maps: the triangle is missing a route from '{a}' to '{b}'"
            ));
        }
    }
}

fn check_grimoire(cat: &Catalogue, issues: &mut Vec<String>) {
    let mut ids: Vec<&str> = cat.grimoire.iter().map(|entry| entry.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != cat.grimoire.len() {
        issues.push("grimoire: entry ids must be unique".into());
    }
    // The grimoire must document every monster, origin, scheme, and weakness.
    let required: Vec<&String> = cat
        .villains
        .keys()
        .chain(cat.enemies.keys())
        .chain(cat.origins.keys())
        .chain(cat.schemes.keys())
        .collect();
    for id in required {
        if !ids.contains(&id.as_str()) {
            issues.push(format!("grimoire: missing required entry '{id}'"));
        }
    }
    for villain in cat.villains.values() {
        if !ids.contains(&villain.weakness_item.as_str()) {
            issues.push(format!(
                "grimoire: missing weakness entry '{}'",
                villain.weakness_item
            ));
        }
    }
}

fn check_ui(cat: &Catalogue, issues: &mut Vec<String>) {
    if cat.ui.splash_intro.is_empty() {
        issues.push("ui: splash_intro must have at least one paragraph".into());
    }
    if cat.ui.key_bindings.is_empty() {
        issues.push("ui: key_bindings must not be empty".into());
    }
}

/// Whether actors can stand on this terrain.
pub fn is_walkable(terrain: Terrain) -> bool {
    matches!(
        terrain,
        Terrain::Floor | Terrain::Door | Terrain::Road | Terrain::Grass | Terrain::Grave
    )
}

/// Whether this terrain can be cleared with a Physical-point forceful action.
pub fn is_forceable(terrain: Terrain) -> bool {
    matches!(terrain, Terrain::BarredDoor | Terrain::Rubble)
}

/// Whether this terrain blocks line of sight (and pounce lanes).
pub fn is_opaque(terrain: Terrain) -> bool {
    matches!(
        terrain,
        Terrain::Wall | Terrain::Tree | Terrain::Rubble | Terrain::BarredDoor
    )
}
