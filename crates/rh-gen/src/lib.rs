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

use rh_content::Catalogue;
use rh_core::rng::SimRng;
use rh_core::world::World;

pub use planner::{PlanOp, PlannerConfig};

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
        match planner::certify(catalogue, &world) {
            Ok(routes) => {
                world.certified_routes = routes;
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
                return Ok(Generated {
                    world,
                    rng,
                    report: GenReport {
                        seed,
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
    // Both certified routes must be recorded.
    if world.certified_routes.len() < 2 {
        return Err("certified routes were not recorded".to_owned());
    }
    Ok(())
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
        let catalogue = rh_content::load_embedded().expect("content");
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
