//! Graph-first mystery generator and solvability planner.
//!
//! Generation builds a directed, costed clue graph (opportunities with pool
//! costs, knowledge gates, and physical-access gates), certifies an early
//! hunt-ready route by turn 3 and a more obvious independent fallback by
//! turn 5 against the combat-viability heuristic, and only then returns the
//! materialised world. Failed attempts record rejection reasons for the
//! developer inspector.

mod cast;
mod materialise;
mod planner;

use rh_content::{Catalogue, ConditionAxis, ConditionDef};
use rh_core::rng::SimRng;
use rh_core::world::World;

/// Successful generation: the world, the RNG mid-stream (runtime continues
/// it), and the inspector report.
pub struct Generated {
    pub world: World,
    pub rng: SimRng,
    pub report: GenReport,
}

/// Developer-only generation inspector data: seed, clue graph, certified
/// routes, node costs, and candidate rejection reasons.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GenReport {
    pub seed: u64,
    /// The hunter these routes were certified for.
    pub hunter: String,
    /// Template chosen for each role this run, in role order.
    pub templates: Vec<String>,
    /// The opening entry id, and the node banked before play if any.
    pub opening: String,
    pub conditions: Vec<String>,
    /// Variation packs drawn for each map, in map order.
    pub packs: Vec<Vec<String>>,
    /// Machines embedded in this run's templates.
    pub machines: Vec<String>,
    /// The optional-event deck dealt to each map, in map order.
    pub events: Vec<Vec<String>>,
    pub banked_node: Option<String>,
    pub villain: String,
    pub origin: String,
    pub scheme: String,
    pub ambush_percent: u8,
    pub attempts: Vec<AttemptReport>,
    /// Planner node costs for the accepted world.
    pub nodes: Vec<NodeReport>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AttemptReport {
    pub attempt: u8,
    pub outcome: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeReport {
    pub id: u16,
    pub name: String,
    pub map: String,
    pub pool: Option<String>,
    pub cost: u8,
    pub obscurity: u8,
    pub grants: String,
    pub revealed_by: Option<u16>,
    pub requires: Option<u16>,
}

#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("generation exhausted {attempts} attempts for seed {seed}: {last_reason}")]
    Exhausted {
        seed: u64,
        attempts: u8,
        last_reason: String,
    },
}

const MAX_ATTEMPTS: u8 = 8;

/// Generate a validated world from a base seed.
///
/// Fully deterministic: the same seed always yields the same world, the same
/// certified routes, and the same post-generation RNG state.
pub fn generate(seed: u64, catalogue: &Catalogue) -> Result<Generated, GenError> {
    let mut rng = SimRng::new(seed);
    let combo = cast::pick_combo(&mut rng, catalogue);
    let generator = &catalogue.balance.generator;
    let ambush_percent = rng.in_range(
        u32::from(generator.ambush_percent_min),
        u32::from(generator.ambush_percent_max),
    ) as u8;

    let mut attempts = Vec::new();
    let mut last_reason = String::from("no attempt ran");
    for attempt in 0..MAX_ATTEMPTS {
        let candidate = build_candidate(seed, catalogue, &combo, ambush_percent, &mut rng);
        let mut world = match candidate {
            Ok(world) => world,
            Err(reason) => {
                attempts.push(AttemptReport {
                    attempt,
                    outcome: format!("rejected before planning: {reason}"),
                });
                last_reason = reason;
                continue;
            }
        };
        // Conditions are drawn before certification, because the bane may be
        // one that taxes Social work and the planner must certify against that
        // rather than have it applied behind its back.
        let mut conditions = draw_conditions(catalogue, &mut rng, None);
        let mut certified = planner::certify(catalogue, &world, has_surcharge(&conditions));
        if certified.is_err() && has_surcharge(&conditions) {
            // The valley's mood would have made the case uncertifiable, so the
            // bane moves to an axis that only costs the journey.
            conditions = draw_conditions(catalogue, &mut rng, Some(ConditionAxis::Reception));
            certified = planner::certify(catalogue, &world, has_surcharge(&conditions));
        }
        match certified {
            Ok(certification) => {
                let prior = certification
                    .opening
                    .and_then(|index| planner::op_id_at(catalogue, &world, index));
                world.certified_routes = certification.routes;
                world.opening = pick_opening(catalogue, &world, prior, &mut rng);
                world.opening.conditions = conditions.iter().map(|c| c.id.clone()).collect();
                for condition in &conditions {
                    if let Some(effect) = &condition.effect {
                        apply_condition_to_world(&mut world, effect);
                    }
                }
                if let Err(reason) = final_validation(catalogue, &world) {
                    attempts.push(AttemptReport {
                        attempt,
                        outcome: format!("failed final validation: {reason}"),
                    });
                    last_reason = reason;
                    continue;
                }
                attempts.push(AttemptReport {
                    attempt,
                    outcome: "accepted".to_owned(),
                });
                let nodes = planner::node_report(catalogue, &world);
                let templates: Vec<String> =
                    world.maps.iter().map(|m| m.template.clone()).collect();
                let opening_id = world.opening.opening.clone();
                let condition_ids = world.opening.conditions.clone();
                let pack_ids = world.packs.clone();
                let machine_ids: Vec<String> = world
                    .opportunities
                    .iter()
                    .filter(|opp| {
                        matches!(opp.grants, rh_core::world::OpportunityGrant::Machine { .. })
                    })
                    .map(|opp| opp.source.clone())
                    .collect();
                let event_decks = world.event_decks.clone();
                let banked_node = world
                    .opening
                    .prior
                    .map(|id| world.opportunity(id).name.clone());
                return Ok(Generated {
                    world,
                    rng,
                    report: GenReport {
                        seed,
                        hunter: catalogue.hunter_id.clone(),
                        templates,
                        opening: opening_id,
                        conditions: condition_ids,
                        packs: pack_ids,
                        machines: machine_ids,
                        events: event_decks,
                        banked_node,
                        villain: combo.villain.clone(),
                        origin: combo.origin.clone(),
                        scheme: combo.scheme.clone(),
                        ambush_percent,
                        attempts,
                        nodes,
                    },
                });
            }
            Err(reason) => {
                attempts.push(AttemptReport {
                    attempt,
                    outcome: format!("planner rejected: {reason}"),
                });
                last_reason = reason;
            }
        }
    }
    Err(GenError::Exhausted {
        seed,
        attempts: MAX_ATTEMPTS,
        last_reason,
    })
}

/// Flood fill over walkable-or-forceable terrain, eight ways.
fn forceable_flood(tiles: &[rh_content::Terrain], from: rh_core::geometry::Point) -> Vec<bool> {
    use rh_core::geometry::{MAP_HEIGHT, MAP_WIDTH};
    let passable = |terrain: rh_content::Terrain| {
        rh_content::is_walkable(terrain) || rh_content::is_forceable(terrain)
    };
    let mut seen = vec![false; MAP_WIDTH as usize * MAP_HEIGHT as usize];
    let start = from.y as usize * MAP_WIDTH as usize + from.x as usize;
    if !passable(tiles[start]) {
        return seen;
    }
    seen[start] = true;
    let mut queue = vec![from];
    while let Some(point) = queue.pop() {
        for dx in -1i16..=1 {
            for dy in -1i16..=1 {
                let next = rh_core::geometry::Point::new(point.x + dx, point.y + dy);
                if !next.in_bounds() {
                    continue;
                }
                let index = next.y as usize * MAP_WIDTH as usize + next.x as usize;
                if !seen[index] && passable(tiles[index]) {
                    seen[index] = true;
                    queue.push(next);
                }
            }
        }
    }
    seen
}

fn build_candidate(
    seed: u64,
    catalogue: &Catalogue,
    combo: &cast::Combo,
    ambush_percent: u8,
    rng: &mut SimRng,
) -> Result<World, String> {
    let cast = cast::pick_cast(rng, catalogue, combo)?;
    materialise::build_world(seed, catalogue, combo, &cast, ambush_percent, rng)
}

/// Final validation over the assembled world before it is returned.
fn final_validation(catalogue: &Catalogue, world: &World) -> Result<(), String> {
    // The travel triangle must be intact with paired exits.
    if world.maps.len() != 3 {
        return Err(format!("expected 3 maps, found {}", world.maps.len()));
    }
    for (index, map) in world.maps.iter().enumerate() {
        if map.exits.len() < 2 {
            return Err(format!("map '{}' has fewer than two exits", map.template));
        }
        for exit in &map.exits {
            let dest = world.map(exit.to_map);
            let paired = dest
                .exits
                .iter()
                .any(|back| back.to_map.0 as usize == index && back.at == exit.to_point);
            if !paired {
                return Err(format!(
                    "exit from '{}' to '{}' is not paired",
                    map.template, dest.template
                ));
            }
        }
    }
    // The evidence contract. A case must be corroborable, closable, and — on
    // every axis that decides something — resolvable by investigation rather
    // than by guessing.
    use rh_core::world::{DiscoveryRule, OpportunityGrant};
    let identity = world
        .opportunities
        .iter()
        .filter(|opp| matches!(opp.grants, OpportunityGrant::IdentityClue { .. }))
        .count();
    if identity < 2 {
        return Err(format!("only {identity} identity clues were placed"));
    }
    let identity_discriminators = world
        .opportunities
        .iter()
        .filter(|opp| {
            matches!(
                opp.grants,
                OpportunityGrant::IdentityClue {
                    discriminating: true
                }
            )
        })
        .count();
    if identity_discriminators < 2 {
        return Err(format!(
            "only {identity_discriminators} discriminating identity clues were placed; \
             a case needs two so losing one informant cannot strand it"
        ));
    }
    // A discriminator the player can only reach through another clue is not a
    // guarantee, so the origin and scheme each need one findable by looking.
    for (axis, is_axis) in [
        (
            "origin",
            &(|grant: &OpportunityGrant| {
                matches!(
                    grant,
                    OpportunityGrant::OriginSign {
                        discriminating: true
                    }
                )
            }) as &dyn Fn(&OpportunityGrant) -> bool,
        ),
        (
            "scheme",
            &(|grant: &OpportunityGrant| {
                matches!(
                    grant,
                    OpportunityGrant::SchemeSign {
                        discriminating: true
                    }
                )
            }) as &dyn Fn(&OpportunityGrant) -> bool,
        ),
    ] {
        let reachable = world.opportunities.iter().any(|opp| {
            is_axis(&opp.grants)
                && matches!(
                    opp.discovery,
                    DiscoveryRule::Sight | DiscoveryRule::SightOr(_)
                )
        });
        if !reachable {
            return Err(format!(
                "no discriminating {axis} sign is findable by sight; the {axis} decides a \
                 real preparation, so it must be resolvable by investigation"
            ));
        }
    }
    // Villain concealment must be consistent.
    let villain = &catalogue.villains[&world.villain.archetype];
    match villain.concealment {
        rh_content::Concealment::NpcHost => {
            if world.villain.host.is_none() {
                return Err("NPC-host villain has no host".to_owned());
            }
        }
        rh_content::Concealment::DormantGrave => {
            if world.villain.grave.is_none() {
                return Err("grave villain has no grave".to_owned());
            }
        }
    }
    // Nothing the varied maps carry may be walled off. The planner never
    // reasons about tiles, so a pack combination that sealed a feature or an
    // exit would break a certified route in silence; this is the last line
    // after per-pack content validation.
    for map in &world.maps {
        // Forceable terrain counts as passable: a barred door is a price, not
        // a wall, and routes are allowed to schedule that price.
        let reached = forceable_flood(&map.tiles, map.entry);
        let index_of = |at: rh_core::geometry::Point| {
            at.y as usize * rh_core::geometry::MAP_WIDTH as usize + at.x as usize
        };
        for feature in &map.features {
            // Features are used from beside them (altars, workstations), so a
            // reached neighbour is as good as a reached tile.
            let mut approachable = reached[index_of(feature.at)];
            for dx in -1i16..=1 {
                for dy in -1i16..=1 {
                    let near = rh_core::geometry::Point::new(feature.at.x + dx, feature.at.y + dy);
                    if near.in_bounds() && reached[index_of(near)] {
                        approachable = true;
                    }
                }
            }
            if !approachable {
                return Err(format!(
                    "feature '{}' on '{}' cannot be walked to",
                    feature.name, map.template
                ));
            }
        }
        for exit in &map.exits {
            if !reached[index_of(exit.at)] {
                return Err(format!(
                    "the exit from '{}' to map {} cannot be walked to",
                    map.template, exit.to_map.0
                ));
            }
        }
    }
    // Both certified routes must be recorded.
    if world.certified_routes.len() < 2 {
        return Err("certified routes were not recorded".to_owned());
    }
    // The opening must be nameable, and anything banked before play must be a
    // node it is honest to have already resolved.
    if !catalogue
        .openings
        .iter()
        .any(|opening| opening.id == world.opening.opening)
    {
        return Err(format!(
            "opening '{}' is not in openings.toml",
            world.opening.opening
        ));
    }
    if let Some(id) = world.opening.prior {
        let spec = world.opportunity(id);
        if spec.clears_terrain {
            return Err("a banked node must not be one that forces terrain".to_owned());
        }
        if spec.requires.is_some() {
            return Err("a banked node must not sit behind a gate".to_owned());
        }
        match spec.grants {
            OpportunityGrant::IdentityClue {
                discriminating: true,
            } => {
                return Err(
                    "a discriminating identity clue must never be banked: it would leave one                      ambiguous sign between the player and the villain's name"
                        .to_owned(),
                )
            }
            OpportunityGrant::MysticFavour => {
                return Err("the mystical favour must not be banked".to_owned())
            }
            _ => {}
        }
    }
    Ok(())
}

/// Choose how the run opens: prose that explains the banked node, or a generic
/// hook when nothing was banked.
fn pick_opening(
    catalogue: &Catalogue,
    world: &World,
    prior: Option<rh_core::world::OpportunityId>,
    rng: &mut SimRng,
) -> rh_core::world::OpeningSituation {
    use rh_content::{OpeningAnchor, OpeningGrant};
    use rh_core::world::{OpeningSituation, OpportunityAnchor, OpportunityGrant};

    let keyed = prior.and_then(|id| {
        let spec = world.opportunity(id);
        let anchor = match spec.anchor {
            OpportunityAnchor::Npc(_) => OpeningAnchor::Npc,
            OpportunityAnchor::Tile(_) => OpeningAnchor::Tile,
        };
        let grant = match spec.grants {
            OpportunityGrant::Items { .. } => Some(OpeningGrant::Items),
            OpportunityGrant::Lead => Some(OpeningGrant::Lead),
            OpportunityGrant::IdentityClue { .. } => Some(OpeningGrant::Identity),
            _ => None,
        }?;
        Some((anchor, grant))
    });

    let pool: Vec<&rh_content::OpeningDef> = match keyed {
        Some((anchor, grant)) => catalogue
            .openings
            .iter()
            .filter(|opening| opening.matches(anchor, grant))
            .collect(),
        None => catalogue
            .openings
            .iter()
            .filter(|opening| opening.is_generic())
            .collect(),
    };
    // Content validation guarantees both pools are non-empty; falling back to
    // a generic hook keeps a content slip from panicking a run.
    let chosen = if pool.is_empty() {
        catalogue.openings.first()
    } else {
        pool.get(rng.index(pool.len())).copied()
    };
    OpeningSituation {
        opening: chosen.map(|o| o.id.clone()).unwrap_or_default(),
        // Filled in by the caller, which drew them before certification.
        conditions: Vec::new(),
        // Only bank the node if its kind is one the prose can narrate.
        prior: keyed.and(prior),
    }
}

/// Draw this run's four conditions: one from every axis, shaped so that
/// exactly one bites, exactly one helps, and the other two are texture.
///
/// `avoid_bane_on` forces the bane somewhere else — used when the valley's
/// mood turned out to make the case uncertifiable.
fn draw_conditions(
    catalogue: &Catalogue,
    rng: &mut SimRng,
    avoid_bane_on: Option<ConditionAxis>,
) -> Vec<ConditionDef> {
    let axes: Vec<ConditionAxis> = ConditionAxis::ORDER
        .iter()
        .copied()
        .filter(|axis| catalogue.conditions.iter().any(|c| c.axis == *axis))
        .collect();
    if axes.len() < 2 {
        return Vec::new();
    }

    // Which axis bites, and which helps. Never the same one.
    let bane_choices: Vec<ConditionAxis> = axes
        .iter()
        .copied()
        .filter(|axis| Some(*axis) != avoid_bane_on)
        .filter(|axis| {
            catalogue
                .conditions
                .iter()
                .any(|c| c.axis == *axis && c.is_bane())
        })
        .collect();
    let bane_axis = bane_choices
        .get(rng.index(bane_choices.len().max(1)))
        .copied();
    let boon_choices: Vec<ConditionAxis> = axes
        .iter()
        .copied()
        .filter(|axis| Some(*axis) != bane_axis)
        .filter(|axis| {
            catalogue
                .conditions
                .iter()
                .any(|c| c.axis == *axis && c.is_boon())
        })
        .collect();
    let boon_axis = boon_choices
        .get(rng.index(boon_choices.len().max(1)))
        .copied();

    let mut drawn = Vec::new();
    for axis in axes {
        let pool: Vec<&ConditionDef> = catalogue
            .conditions
            .iter()
            .filter(|condition| {
                condition.axis == axis
                    && if Some(axis) == bane_axis {
                        condition.is_bane()
                    } else if Some(axis) == boon_axis {
                        condition.is_boon()
                    } else {
                        condition.is_cosmetic()
                    }
            })
            .collect();
        if pool.is_empty() {
            continue;
        }
        drawn.push(pool[rng.index(pool.len())].clone());
    }
    drawn
}

/// Whether the drawn set includes the one bane the planner has to know about.
fn has_surcharge(conditions: &[ConditionDef]) -> bool {
    conditions.iter().any(|condition| {
        condition
            .effect
            .as_ref()
            .is_some_and(|effect| effect.is_certification_visible())
    })
}

/// Apply the parts of a condition that live in the world rather than the run
/// state. Nothing here touches the final fight: certification promised the
/// hunt is winnable and a condition may not take that back.
fn apply_condition_to_world(world: &mut World, effect: &rh_content::ConditionEffect) {
    use rh_content::ConditionEffect;
    match effect {
        ConditionEffect::Ambush { percent } => {
            world.ambush_percent = world.ambush_percent.saturating_add(*percent).min(100);
        }
        ConditionEffect::QuietRoads { percent } => {
            world.ambush_percent = world.ambush_percent.saturating_sub(*percent);
        }
        ConditionEffect::Pressure { extra } => {
            // Away from the settlement only: the valley's own streets stay as
            // safe as they ever were.
            for map in world.maps.iter_mut().skip(1) {
                let existing: Vec<_> = map.initial_enemies.clone();
                for spawn in existing.iter().take(usize::from(*extra)) {
                    map.initial_enemies.push(spawn.clone());
                }
            }
        }
        // Applied to the run state at construction, not to the world.
        ConditionEffect::SocialSurcharge
        | ConditionEffect::ShortSight { .. }
        | ConditionEffect::LongSight { .. }
        | ConditionEffect::WellSupplied { .. } => {}
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;

    #[test]
    #[ignore = "diagnostic dump for generator tuning; run with --ignored"]
    fn dump_seed() {
        let seed: u64 = std::env::var("RH_DEBUG_SEED")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(42);
        let mut catalogue = rh_content::load_embedded().expect("content");
        if let Ok(hunter) = std::env::var("RH_DEBUG_HUNTER") {
            catalogue.select_hunter(&hunter).expect("known hunter");
        }
        println!("hunter={}", catalogue.hunter_id);
        let mut rng = SimRng::new(seed);
        let combo = cast::pick_combo(&mut rng, &catalogue);
        let generator = &catalogue.balance.generator;
        let ambush = rng.in_range(
            u32::from(generator.ambush_percent_min),
            u32::from(generator.ambush_percent_max),
        ) as u8;
        for attempt in 0..2 {
            let candidate = build_candidate(seed, &catalogue, &combo, ambush, &mut rng);
            match candidate {
                Ok(world) => {
                    println!("--- attempt {attempt} ---");
                    println!("{}", planner::debug_certify(&catalogue, &world));
                }
                Err(reason) => println!("--- attempt {attempt}: candidate failed: {reason}"),
            }
        }
    }
}
