//! Cross-reference and contract validation for the content catalogue.
//!
//! These checks run in every build that loads content and as a dedicated CI
//! gate, so hand-edited content fails loudly with author-readable messages.

use crate::catalogue::Catalogue;
use crate::schema::*;
use crate::strings::StringId;

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
    check_origins(cat, &mut issues);
    check_counter_ingredients(cat, &mut issues);
    check_schemes(cat, &mut issues);
    check_clues(cat, &mut issues);
    check_npcs(cat, &mut issues);
    check_maps(cat, &mut issues);
    check_gathers(cat, &mut issues);
    check_grimoire(cat, &mut issues);
    check_ui(cat, &mut issues);
    check_openings(cat, &mut issues);
    check_conditions(cat, &mut issues);
    check_strings(cat, &mut issues);
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
    if cat.hunters.is_empty() {
        issues.push("hunters: no hunter profiles were loaded".into());
        return;
    }
    if !cat.hunters.contains_key(&cat.hunter_id) {
        issues.push(format!(
            "hunters: selected hunter '{}' is not in the roster",
            cat.hunter_id
        ));
    }
    for (id, hunter) in &cat.hunters {
        for item in &hunter.starting_items {
            if !cat.items.contains_key(item) {
                issues.push(format!(
                    "hunters: '{id}' starting item '{item}' is not in items.toml"
                ));
            }
        }
        // The Mystic pool is the favour's over-cap point for a hunter with no
        // Mystic of her own; a hunter who reads the occult may have her own.
        let mut ability_ids: Vec<&str> = hunter
            .manoeuvres
            .iter()
            .map(|m| m.id.as_str())
            .chain(hunter.signatures.iter().map(|s| s.id.as_str()))
            .collect();
        ability_ids.sort_unstable();
        ability_ids.dedup();
        if ability_ids.len() != hunter.manoeuvres.len() + hunter.signatures.len() {
            issues.push(format!(
                "hunters: '{id}' manoeuvre/signature ids must be unique"
            ));
        }
        for manoeuvre in &hunter.manoeuvres {
            if manoeuvre.stamina_cost > hunter.stamina_cap {
                issues.push(format!(
                    "hunters: '{id}' manoeuvre '{}' costs more stamina than the cap",
                    manoeuvre.id
                ));
            }
        }
        for signature in &hunter.signatures {
            if signature.physical_cost > hunter.physical_cap {
                issues.push(format!(
                    "hunters: '{id}' signature '{}' costs more Physical than the cap",
                    signature.id
                ));
            }
        }
        // A hunter with no way to spend a pool point on evidence cannot be
        // certified through the clue graph at all.
        if hunter.lore_cap == 0 && hunter.social_cap == 0 && hunter.mystic_cap == 0 {
            issues.push(format!(
                "hunters: '{id}' has no investigation pool, so no route can be certified for her"
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
        // The decisive counter must actually be a counter.
        match cat.items.get(&villain.weakness_item).map(|item| &item.kind) {
            Some(ItemKind::WeaknessAmmunition { .. })
            | Some(ItemKind::WeaknessBlade { .. })
            | Some(ItemKind::BindingCharm) => {}
            Some(_) => issues.push(format!(
                "villains: '{id}' weakness item '{}' is not a counter kind",
                villain.weakness_item
            )),
            None => {}
        }
        // Every villain needs a defining tactical behaviour to fight around.
        if villain.pounce.is_none() && villain.cadence.is_none() && villain.ward.is_none() {
            issues.push(format!(
                "villains: '{id}' has no pounce, cadence, or ward to make its fight distinct"
            ));
        }
        match villain.concealment {
            Concealment::NpcHost => {
                if !cat.npcs.archetypes.values().any(|npc| npc.can_host_villain) {
                    issues.push(format!(
                        "villains: '{id}' hides in an NPC but no archetype can_host_villain"
                    ));
                }
                if villain.pounce.is_none() && villain.ward.is_none() {
                    issues.push(format!(
                        "villains: NPC-host villain '{id}' needs a pounce or a ward"
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
        if let Some(ward) = &villain.ward {
            if ward.charges == 0 {
                issues.push(format!("villains: '{id}' ward has no charges"));
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
        // The pre-emption must be placeable: some map of the named role has to
        // offer the site kind it is performed at.
        let preempt = &scheme.preempt;
        let placeable = cat.maps.values().any(|map| {
            map.role == preempt.map_role && map.slots.iter().any(|slot| slot.kind == preempt.site)
        });
        if !placeable {
            issues.push(format!(
                "schemes: '{id}' pre-emption needs a {:?} site on a {:?} map, and none exists",
                preempt.site, preempt.map_role
            ));
        }
        if preempt.cost == 0 {
            issues.push(format!("schemes: '{id}' pre-emption must cost a point"));
        }
    }
}

/// How many independent ways a run has to obtain `item`: gathering sites,
/// clues that hand it over, and the hunter's own starting kit.
fn item_sources(cat: &Catalogue, item: &str) -> usize {
    let gathered = cat
        .gathers
        .values()
        .filter(|gather| gather.items.iter().any(|granted| granted == item))
        .count();
    let from_clues = cat
        .clues
        .values()
        .filter(|clue| clue.grants_items.iter().any(|granted| granted == item))
        .count();
    // Anything in the starting kit is always available to both routes, so it
    // can never be the thing that strands one of them.
    let carried = usize::from(cat.hunter.starting_items.iter().any(|held| held == item));
    gathered + from_clues + carried * 2
}

/// Every ingredient of a villain's counter must be obtainable twice over.
///
/// This is the same trap as the origin reagent one map further out: route
/// certification forbids the two routes from sharing a node, so a single-source
/// ingredient anywhere in a counter recipe strands the second route. The origin
/// reagent rule alone missed it, because the reagent was not the ingredient
/// that ran out.
fn check_counter_ingredients(cat: &Catalogue, issues: &mut Vec<String>) {
    for (villain_id, villain) in &cat.villains {
        let Some(recipe) = cat
            .recipes
            .values()
            .find(|recipe| recipe.output == villain.weakness_item)
        else {
            continue;
        };
        for input in &recipe.inputs {
            let sources = item_sources(cat, input);
            if sources < 2 {
                issues.push(format!(
                    "recipes: '{}' is the only counter to '{villain_id}' but its ingredient \
                     '{input}' has {sources} source(s); it needs at least 2 so two independent \
                     routes can each craft the counter",
                    recipe.name
                ));
            }
        }
    }
}

fn check_origins(cat: &Catalogue, issues: &mut Vec<String>) {
    for (id, origin) in &cat.origins {
        // The reagent is what makes reading the origin load-bearing, so it
        // must exist and be gatherable somewhere.
        if !cat.items.contains_key(&origin.counter_reagent) {
            issues.push(format!(
                "origins: '{id}' counter reagent '{}' is not in items.toml",
                origin.counter_reagent
            ));
            continue;
        }
        // Two independent certified routes may not share a node, so a reagent
        // with a single source strands the second route with no way to finish
        // its counter. Every reagent needs at least two ways to get it.
        let sources = item_sources(cat, &origin.counter_reagent);
        if sources < 2 {
            issues.push(format!(
                "origins: '{id}' counter reagent '{}' has {sources} source(s); it needs at \
                 least 2 so two independent routes can each obtain it",
                origin.counter_reagent
            ));
        }
    }
    // Each origin must demand a different reagent, or the axis decides nothing.
    let mut reagents: Vec<&String> = cat
        .origins
        .values()
        .map(|origin| &origin.counter_reagent)
        .collect();
    reagents.sort();
    let distinct = {
        let mut unique = reagents.clone();
        unique.dedup();
        unique.len()
    };
    if distinct != cat.origins.len() {
        issues.push(
            "origins: every origin must demand a distinct counter reagent, otherwise reading \
             the origin changes nothing"
                .into(),
        );
    }
}

fn check_clues(cat: &Catalogue, issues: &mut Vec<String>) {
    let axis_values = |axis: EvidenceAxis| -> Vec<&String> {
        match axis {
            EvidenceAxis::Villain => cat.villains.keys().collect(),
            EvidenceAxis::Origin => cat.origins.keys().collect(),
            EvidenceAxis::Scheme => cat.schemes.keys().collect(),
        }
    };
    for (id, clue) in &cat.clues {
        // Church clues anchor to an authored slot. The generator has no
        // fallback, so a missing or stray declaration is a load-time error
        // rather than a surprise at world-build time.
        match (clue.site, clue.church_slot) {
            (SiteKind::Church, None) => {
                issues.push(format!(
                    "clues: '{id}' is a church clue without a church_slot"
                ));
            }
            (site, Some(_)) if site != SiteKind::Church => {
                issues.push(format!(
                    "clues: '{id}' declares a church_slot but its site is {site:?}"
                ));
            }
            _ => {}
        }
        // Cross-axis scoping filters must name real values.
        for (label, list, known) in [
            (
                "villains",
                &clue.villains,
                cat.villains.keys().collect::<Vec<_>>(),
            ),
            (
                "origins",
                &clue.origins,
                cat.origins.keys().collect::<Vec<_>>(),
            ),
            (
                "schemes",
                &clue.schemes,
                cat.schemes.keys().collect::<Vec<_>>(),
            ),
        ] {
            for value in list {
                if !known.contains(&value) {
                    issues.push(format!("clues: '{id}' {label} lists unknown '{value}'"));
                }
            }
        }
        // Evidence claims must name real values on the clue's own axis, and a
        // clue may not both support and rule out the same value.
        match clue.category.axis() {
            None => {
                if !clue.supports.is_empty() || !clue.rules_out.is_empty() {
                    issues.push(format!(
                        "clues: '{id}' is category {:?}, which makes no claim, so it must \
                         not set supports/rules_out",
                        clue.category
                    ));
                }
            }
            Some(axis) => {
                let known = axis_values(axis);
                for (label, list) in [("supports", &clue.supports), ("rules_out", &clue.rules_out)]
                {
                    for value in list {
                        if !known.contains(&value) {
                            issues.push(format!(
                                "clues: '{id}' {label} names '{value}', which is not a value \
                                 on the {axis:?} axis"
                            ));
                        }
                    }
                }
                for value in &clue.rules_out {
                    if clue.supports.contains(value) {
                        issues.push(format!(
                            "clues: '{id}' both supports and rules out '{value}'"
                        ));
                    }
                }
                // A discriminator that eliminates everything leaves no case.
                if !clue.rules_out.is_empty() && clue.rules_out.len() >= known.len() {
                    issues.push(format!("clues: '{id}' rules out every value on its axis"));
                }
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
    // Every composition on the three axes must have enough raw material to
    // build a readable, solvable case: identity clues to corroborate with,
    // two discriminators per axis so uncertainty is always resolvable by
    // investigation, a location clue, and a decisive weakness clue.
    for villain_id in cat.villains.keys() {
        for origin_id in cat.origins.keys() {
            for scheme_id in cat.schemes.keys() {
                let case = format!("{villain_id}/{origin_id}/{scheme_id}");
                let fitting: Vec<&ClueTemplate> = cat
                    .clues
                    .values()
                    .filter(|clue| clue.fits(villain_id, origin_id, scheme_id))
                    .collect();
                let count = |category: ClueCategory| {
                    fitting.iter().filter(|c| c.category == category).count()
                };
                let discriminators = |category: ClueCategory| {
                    fitting
                        .iter()
                        .filter(|c| c.category == category && c.is_discriminating())
                        .count()
                };

                let identity = count(ClueCategory::Identity);
                if identity < 4 {
                    issues.push(format!(
                        "clues: case {case} has only {identity} identity clues; the generator \
                         needs at least 4"
                    ));
                }
                // The spec's contract: at least two reachable discriminators
                // on every axis the case is composed on.
                for (label, category) in [
                    ("identity", ClueCategory::Identity),
                    ("origin", ClueCategory::OriginSign),
                    ("scheme", ClueCategory::SchemeSign),
                ] {
                    let found = discriminators(category);
                    if found < 2 {
                        issues.push(format!(
                            "clues: case {case} has only {found} discriminating {label} \
                             clue(s); every axis needs at least 2"
                        ));
                    }
                }
                // Soft signs are what make the case ambiguous at first; a case
                // with none reads as a labelled answer from the first clue.
                let soft_identity = fitting
                    .iter()
                    .filter(|c| c.category == ClueCategory::Identity && !c.is_discriminating())
                    .count();
                if soft_identity < 1 {
                    issues.push(format!("clues: case {case} has no ambiguous identity sign"));
                }
                if count(ClueCategory::Location) < 1 {
                    issues.push(format!("clues: case {case} has no location clue"));
                }
                if count(ClueCategory::Weakness) < 1 {
                    issues.push(format!("clues: case {case} has no weakness clue"));
                }
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

    // Exits name a role, not a template, because which template fills a role
    // is chosen per run. Every template must therefore reach both other roles,
    // and every role must have something to reach.
    for (id, map) in &cat.maps {
        let mut reached: Vec<MapRole> = Vec::new();
        for exit in &map.exits {
            match MapRole::from_content(&exit.to) {
                None => issues.push(format!(
                    "maps: '{id}' has an exit to '{}', which is not a map role",
                    exit.to
                )),
                Some(role) if role == map.role => issues.push(format!(
                    "maps: '{id}' has an exit to its own role '{}'",
                    exit.to
                )),
                Some(role) => reached.push(role),
            }
        }
        for role in MapRole::ORDER {
            if role != map.role && !reached.contains(&role) {
                issues.push(format!(
                    "maps: '{id}' has no exit to the '{}' role, so the travel triangle \
                     breaks whenever this template is chosen",
                    role.label()
                ));
            }
        }
    }
    for role in MapRole::ORDER {
        if cat.templates_for(role).is_empty() {
            issues.push(format!(
                "maps: no template fills the '{}' role",
                role.label()
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

/// Every `StringId` the catalogue references, paired with a label naming
/// where it came from.
///
/// Written out by hand rather than derived: it is the list that both the
/// resolve check and the orphan check read, and being able to grep for a
/// field here is worth more than the boilerplate it saves.
fn referenced_ids(cat: &Catalogue) -> Vec<(String, &StringId)> {
    let mut ids = Vec::new();
    ids.push(("ui.splash_title".to_owned(), &cat.ui.splash_title));
    for (index, id) in cat.ui.splash_intro.iter().enumerate() {
        ids.push((format!("ui.splash_intro[{index}]"), id));
    }
    for (index, binding) in cat.ui.key_bindings.iter().enumerate() {
        ids.push((format!("ui.key_bindings[{index}].keys"), &binding.keys));
        ids.push((format!("ui.key_bindings[{index}].action"), &binding.action));
    }
    ids
}

fn check_strings(cat: &Catalogue, issues: &mut Vec<String>) {
    for (label, id) in referenced_ids(cat) {
        if cat.strings.try_get(id.as_str()).is_none() {
            issues.push(format!(
                "strings: {label} references unknown string id '{id}'"
            ));
        }
    }
    // A row nobody can read is a row nobody should be asked to translate.
    for (id, row) in cat.strings.rows() {
        if row.context.trim().is_empty() {
            issues.push(format!("strings: '{id}' has no context note"));
        }
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

/// Openings must cover every kind of node that can be banked before play, or
/// generation could produce a run whose opening it cannot narrate.
fn check_openings(cat: &Catalogue, issues: &mut Vec<String>) {
    let mut ids: Vec<&str> = Vec::new();
    for opening in &cat.openings {
        if opening.id.trim().is_empty() {
            issues.push("openings: an entry has an empty id".into());
        }
        ids.push(opening.id.as_str());
        if opening.body.is_empty() {
            issues.push(format!("openings: '{}' has no prose", opening.id));
        }
        for line in &opening.body {
            if line.trim().is_empty() {
                issues.push(format!("openings: '{}' has an empty paragraph", opening.id));
            }
        }
        // Half-keyed entries would match nodes their prose does not fit.
        if opening.anchor.is_some() != opening.grant.is_some() {
            issues.push(format!(
                "openings: '{}' sets only one of anchor/grant; an opening is keyed on both or \
                 on neither",
                opening.id
            ));
        }
        // A person's name has nowhere to come from in a place-anchored opening.
        let mentions_npc = opening.body.iter().any(|line| line.contains("{npc}"));
        if mentions_npc && opening.anchor != Some(OpeningAnchor::Npc) {
            issues.push(format!(
                "openings: '{}' uses {{npc}} but is not anchored on a person",
                opening.id
            ));
        }
        for line in &opening.body {
            let mut rest = line.as_str();
            while let Some(start) = rest.find('{') {
                let Some(end) = rest[start..].find('}') else {
                    break;
                };
                let placeholder = &rest[start..start + end + 1];
                if !matches!(placeholder, "{npc}" | "{clue}" | "{place}") {
                    issues.push(format!(
                        "openings: '{}' uses unknown placeholder '{placeholder}'",
                        opening.id
                    ));
                }
                rest = &rest[start + end + 1..];
            }
        }
    }
    ids.sort_unstable();
    let unique = ids.len();
    ids.dedup();
    if ids.len() != unique {
        issues.push("openings: ids must be unique".into());
    }

    let generic = cat.openings.iter().filter(|o| o.is_generic()).count();
    if generic < 2 {
        issues.push(format!(
            "openings: only {generic} generic hook(s); most runs bank nothing, so they need \
             more than one way to begin"
        ));
    }
    // Every bankable node kind needs prose. The planner will not bank anything
    // outside this set, so this is the whole space.
    for anchor in [OpeningAnchor::Tile, OpeningAnchor::Npc] {
        for grant in [
            OpeningGrant::Items,
            OpeningGrant::Lead,
            OpeningGrant::Identity,
        ] {
            if !cat.openings.iter().any(|o| o.matches(anchor, grant)) {
                issues.push(format!(
                    "openings: nothing narrates a {anchor:?}-anchored {grant:?} node banked \
                     before play"
                ));
            }
        }
    }
}

/// A run draws exactly one condition, so the rules here are about keeping any
/// single draw sane: every axis worth drawing from, and no axis that is mostly
/// teeth.
fn check_conditions(cat: &Catalogue, issues: &mut Vec<String>) {
    let mut ids: Vec<&str> = Vec::new();
    for condition in &cat.conditions {
        if condition.id.trim().is_empty() {
            issues.push("conditions: an entry has an empty id".into());
        }
        ids.push(condition.id.as_str());
        if condition.body.is_empty() {
            issues.push(format!("conditions: '{}' has no prose", condition.id));
        }
        for line in &condition.body {
            if line.trim().is_empty() {
                issues.push(format!(
                    "conditions: '{}' has an empty paragraph",
                    condition.id
                ));
            }
            // A condition is drawn whether or not a node was banked, so it has
            // no clue and no informant to name.
            for placeholder in ["{npc}", "{clue}"] {
                if line.contains(placeholder) {
                    issues.push(format!(
                        "conditions: '{}' uses {placeholder}, but a condition is drawn                          independently of whether anything was banked",
                        condition.id
                    ));
                }
            }
        }
        if cat.openings.iter().any(|o| o.id == condition.id) {
            issues.push(format!(
                "conditions: '{}' shares an id with an opening",
                condition.id
            ));
        }
    }
    ids.sort_unstable();
    let unique = ids.len();
    ids.dedup();
    if ids.len() != unique {
        issues.push("conditions: ids must be unique".into());
    }

    for axis in [
        ConditionAxis::Season,
        ConditionAxis::Reception,
        ConditionAxis::Hour,
        ConditionAxis::Provenance,
    ] {
        let on_axis: Vec<&ConditionDef> =
            cat.conditions.iter().filter(|c| c.axis == axis).collect();
        if on_axis.len() < 3 {
            issues.push(format!(
                "conditions: axis {axis:?} has only {} entries; an axis worth drawing from                  needs at least three",
                on_axis.len()
            ));
        }
        // Every run draws one condition per axis and shapes the set: one axis
        // supplies the bane, another the boon, the rest texture. So an axis
        // needs exactly one of each to draw from, whichever role it is given.
        let banes = on_axis.iter().filter(|c| c.is_bane()).count();
        let boons = on_axis.iter().filter(|c| c.is_boon()).count();
        if banes != 1 {
            issues.push(format!(
                "conditions: axis {axis:?} has {banes} conditions that bite; it needs exactly                  one, since any axis may be the one that bites this run"
            ));
        }
        if boons != 1 {
            issues.push(format!(
                "conditions: axis {axis:?} has {boons} conditions that help; it needs exactly                  one, since any axis may be the one that helps this run"
            ));
        }
        if on_axis.iter().filter(|c| c.is_cosmetic()).count() < 3 {
            issues.push(format!(
                "conditions: axis {axis:?} needs at least three neutral entries; two axes are                  texture every run"
            ));
        }
    }
}
