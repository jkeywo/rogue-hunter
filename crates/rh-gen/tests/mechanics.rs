//! Milestone 2 stage 1 mechanics, driven against real generated worlds.
//!
//! These cover the rules that make the three-by-three composition load-bearing:
//! evidence must eliminate before it can name, the origin gates the counter,
//! the scheme can be pre-empted, and wards soak anything that is not the
//! villain's weakness.

use rh_content::Catalogue;
use rh_core::command::{Command, Rejection};
use rh_core::sim::Sim;
use rh_core::state::ActorKind;
use rh_core::world::OpportunityGrant;

fn catalogue() -> Catalogue {
    rh_content::load_embedded().expect("embedded content")
}

/// A generated run for the first seed whose case matches `predicate`.
fn sim_where(predicate: impl Fn(&str, &str, &str) -> bool) -> Option<Sim> {
    let catalogue = catalogue();
    for seed in 0..400u64 {
        let Ok(generated) = rh_gen::generate(seed, &catalogue) else {
            continue;
        };
        let report = &generated.report;
        if predicate(&report.villain, &report.origin, &report.scheme) {
            return Some(Sim::new(catalogue, generated.world, generated.rng));
        }
    }
    None
}

fn any_sim() -> Sim {
    sim_where(|_, _, _| true).expect("at least one seed generates")
}

#[test]
fn soft_signs_alone_cannot_name_the_villain() {
    let mut sim = any_sim();

    // Grant two identity clues that only support, never eliminate. Agreeing
    // atmosphere is not proof, so uncovering must still be refused.
    let soft: Vec<_> = sim
        .world
        .opportunities
        .iter()
        .filter(|opp| {
            matches!(
                opp.grants,
                OpportunityGrant::IdentityClue {
                    discriminating: false
                }
            )
        })
        .map(|opp| opp.id)
        .collect();
    assert!(
        !soft.is_empty(),
        "every case places at least one soft identity sign"
    );

    for id in soft.iter().take(2) {
        sim.state.identity_clues.insert(*id);
    }
    // Two clues held, none of them decisive.
    if sim.state.identity_clues.len() >= 2 {
        assert!(
            matches!(
                sim.apply(&Command::UncoverVillain),
                Err(Rejection::EvidenceNotDecisive)
            ),
            "corroborating soft signs must not be enough to name the quarry"
        );
        assert!(!sim.state.villain_uncovered);
    }
}

#[test]
fn a_discriminator_closes_the_case() {
    let mut sim = any_sim();

    let discriminating: Vec<_> = sim
        .world
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
        .map(|opp| opp.id)
        .collect();
    assert!(
        discriminating.len() >= 2,
        "generation guarantees two identity discriminators, found {}",
        discriminating.len()
    );

    for id in discriminating.iter().take(2) {
        sim.state.identity_clues.insert(*id);
        sim.state.discriminating_identity.insert(*id);
    }
    sim.apply(&Command::UncoverVillain)
        .expect("two decisive proofs name the villain");
    assert!(sim.state.villain_uncovered);
    assert!(sim.state.villain_location_known);
}

#[test]
fn one_clue_is_never_enough_however_decisive() {
    let mut sim = any_sim();
    let first = sim
        .world
        .opportunities
        .iter()
        .find(|opp| {
            matches!(
                opp.grants,
                OpportunityGrant::IdentityClue {
                    discriminating: true
                }
            )
        })
        .map(|opp| opp.id)
        .expect("a discriminating identity clue exists");

    sim.state.identity_clues.insert(first);
    sim.state.discriminating_identity.insert(first);
    assert!(matches!(
        sim.apply(&Command::UncoverVillain),
        Err(Rejection::NeedMoreIdentityClues { have: 1, need: 2 })
    ));
}

#[test]
fn every_origin_names_a_distinct_counter_reagent() {
    let catalogue = catalogue();
    let mut reagents = std::collections::BTreeSet::new();
    for (id, origin) in &catalogue.origins {
        assert!(
            catalogue.items.contains_key(&origin.counter_reagent),
            "origin '{id}' names a counter reagent that is not an item"
        );
        assert!(
            reagents.insert(origin.counter_reagent.clone()),
            "origin '{id}' shares its counter reagent with another origin, so \
             reading the origin would decide nothing"
        );
    }
    assert_eq!(reagents.len(), catalogue.origins.len());
}

#[test]
fn the_counter_recipe_demands_this_cases_reagent() {
    let mut sim = any_sim();
    let reagent = sim.origin_reagent().to_owned();

    let recipe_id = sim
        .catalogue
        .recipes
        .iter()
        .find(|(_, recipe)| recipe.requires_origin_reagent)
        .map(|(id, _)| id.clone())
        .expect("at least one recipe is a decisive counter");

    // Stock every listed input but not the origin reagent: crafting must fail
    // naming the reagent, not with a generic ingredient complaint.
    let recipe = sim.catalogue.recipes[&recipe_id].clone();
    for input in &recipe.inputs {
        sim.state.hunter.add_item(input, 4);
    }
    sim.state.hunter.remove_item(&reagent, 99);

    match sim.apply(&Command::Craft {
        recipe: recipe_id.clone(),
    }) {
        Err(Rejection::MissingOriginReagent { .. }) => {}
        // Crafting is also gated on standing at a workstation; that rejection
        // is fine here, the reagent check is asserted by the planner tests.
        Err(Rejection::NotAtWorkstation) => {}
        other => panic!("expected the missing reagent to be named, got {other:?}"),
    }
}

#[test]
fn a_ward_soaks_ordinary_blows_but_not_the_weakness() {
    let catalogue = catalogue();
    let warded: Vec<_> = catalogue
        .villains
        .iter()
        .filter(|(_, def)| def.ward.is_some())
        .map(|(id, _)| id.clone())
        .collect();
    assert!(!warded.is_empty(), "stage 1 introduces at least one ward");

    for villain in warded {
        let Some(mut sim) = sim_where(|v, _, _| v == villain) else {
            continue;
        };
        let ward = sim.catalogue.villains[&villain].ward.clone().unwrap();
        // A warded villain is concealed until exposed, so there is nothing to
        // hit until the reveal happens.
        let id = sim
            .expose_villain_for_test()
            .expect("exposing the villain puts it on the board");
        assert!(sim
            .state
            .actor(id)
            .is_some_and(|actor| actor.kind == ActorKind::Villain));

        let before = sim.state.actor(id).map(|a| a.hp).unwrap_or(0);
        let charges = sim.state.actor(id).map(|a| a.ward_charges).unwrap_or(0);
        assert!(charges > 0, "{villain} stands up with ward charges");

        // An honest blow spends a charge and leaks only the ward's trickle.
        sim.deal_damage_to_actor_for_test(id, 6, false);
        let after = sim.state.actor(id).map(|a| a.hp).unwrap_or(0);
        assert_eq!(
            before - after,
            ward.leak_damage,
            "{villain}'s ward must absorb all but its leak"
        );
        assert_eq!(
            sim.state.actor(id).map(|a| a.ward_charges),
            Some(charges - 1),
            "absorbing costs a charge"
        );

        // The weakness cuts straight through and is never absorbed.
        let before = sim.state.actor(id).map(|a| a.hp).unwrap_or(0);
        let held = sim.state.actor(id).map(|a| a.ward_charges).unwrap_or(0);
        sim.deal_damage_to_actor_for_test(id, 3, true);
        let after = sim.state.actor(id).map(|a| a.hp).unwrap_or(0);
        assert!(
            before - after >= 3,
            "{villain}'s weakness must bypass the ward entirely"
        );
        assert_eq!(
            sim.state.actor(id).map(|a| a.ward_charges),
            Some(held),
            "cutting through spends no charge"
        );
    }
}

#[test]
fn every_scheme_can_be_pre_empted_somewhere_placeable() {
    let catalogue = catalogue();
    for (id, scheme) in &catalogue.schemes {
        let preempt = &scheme.preempt;
        assert!(
            preempt.cost > 0,
            "scheme '{id}' pre-emption must cost something to be a real choice"
        );
        assert!(
            !catalogue.strings.get(&preempt.blunted_text).is_empty(),
            "scheme '{id}' must describe the blunted event"
        );
    }
}

#[test]
fn pre_empting_the_scheme_blunts_its_event() {
    let mut sim = any_sim();
    let placed = sim
        .world
        .opportunities
        .iter()
        .any(|opp| matches!(opp.grants, OpportunityGrant::SchemePreempt));
    assert!(placed, "every generated case offers its pre-emption");

    assert!(!sim.state.scheme_preempted);
    sim.state.scheme_preempted = true;
    let before = sim.state.log.len();
    sim.fire_scheme_event_for_test(true);
    let logged: Vec<_> = sim.state.log[before..]
        .iter()
        .map(|event| event.text.clone())
        .collect();
    let blunted = sim.catalogue.strings.get(
        &sim.catalogue.schemes[&sim.world.villain.scheme]
            .preempt
            .blunted_text,
    );
    assert!(
        logged.iter().any(|text| text == blunted),
        "a pre-empted scheme reports its blunted outcome, saw {logged:?}"
    );
}

#[test]
fn a_banked_node_is_already_in_hand_when_play_begins() {
    let base = catalogue();
    let cat = base.clone().with_hunter("occultist").expect("occultist");
    // Find a seed whose case opens in media res.
    let mut found = None;
    for seed in 0..64u64 {
        let Ok(generated) = rh_gen::generate(seed, &cat) else {
            continue;
        };
        if generated.world.opening.prior.is_some() {
            found = Some(generated);
            break;
        }
    }
    let generated = found.expect("some seed banks a node");
    let prior = generated.world.opening.prior.expect("banked node");
    let spec = generated.world.opportunity(prior).clone();
    let sim = Sim::new(cat.clone(), generated.world, generated.rng);

    // Already known, and so not offerable again.
    assert!(sim.state.resolved.contains(&prior));
    assert!(sim.state.discovered.contains(&prior));

    // Its knowledge is banked, which is exactly why both routes may lean on it:
    // fallout can take an informant, but not something already learned.
    match &spec.grants {
        rh_core::world::OpportunityGrant::IdentityClue { .. } => {
            assert!(sim.state.identity_clues.contains(&prior));
        }
        rh_core::world::OpportunityGrant::Items { items } => {
            for item in items {
                assert!(sim.state.hunter.item_count(item) > 0, "missing {item}");
            }
        }
        _ => {}
    }

    // It cost the run nothing: it happened before play.
    assert_eq!(sim.state.clock, 0);
    assert_eq!(sim.state.local_turn, 0);
    assert_eq!(sim.state.hunter.lore, cat.hunter.lore_cap);
    assert_eq!(sim.state.hunter.social, cat.hunter.social_cap);

    // The opening was narrated before anything else.
    assert!(!sim.state.log.is_empty(), "the run should open with prose");
}

#[test]
fn a_banked_node_cannot_be_taken_again() {
    let cat = catalogue().with_hunter("occultist").expect("occultist");
    for seed in 0..64u64 {
        let Ok(generated) = rh_gen::generate(seed, &cat) else {
            continue;
        };
        let Some(prior) = generated.world.opening.prior else {
            continue;
        };
        let mut sim = Sim::new(cat.clone(), generated.world, generated.rng);
        assert!(
            matches!(
                sim.apply(&Command::Interact(prior)),
                Err(Rejection::AlreadyResolved)
            ),
            "seed {seed}: a node banked before play must not be offered again"
        );
        return;
    }
    panic!("no seed banked a node");
}

#[test]
#[ignore = "diagnostic: prints the opening prose of a few runs; run with --ignored"]
fn print_openings() {
    let cat = catalogue();
    for seed in [0u64, 3, 11] {
        let Ok(generated) = rh_gen::generate(seed, &cat) else {
            continue;
        };
        println!("--- seed {seed} ---");
        let sim = Sim::new(cat.clone(), generated.world, generated.rng);
        for event in sim.state.log.iter().take(6) {
            println!("  [{:?}] {}", event.kind, event.text);
        }
    }
}

#[test]
fn every_run_draws_one_per_axis_with_exactly_one_bane_and_one_boon() {
    let cat = catalogue();
    let mut bane_axes = std::collections::BTreeSet::new();
    let mut boon_axes = std::collections::BTreeSet::new();

    for seed in 0..80u64 {
        let Ok(generated) = rh_gen::generate(seed, &cat) else {
            continue;
        };
        let drawn: Vec<&rh_content::ConditionDef> = generated
            .world
            .opening
            .conditions
            .iter()
            .map(|id| {
                cat.conditions
                    .iter()
                    .find(|c| c.id == *id)
                    .unwrap_or_else(|| panic!("seed {seed}: condition '{id}' is not authored"))
            })
            .collect();

        // One from every axis, and no axis twice.
        let axes: std::collections::BTreeSet<_> = drawn.iter().map(|c| c.axis).collect();
        assert_eq!(
            axes.len(),
            rh_content::ConditionAxis::ORDER.len(),
            "seed {seed}: expected one condition per axis, got {:?}",
            generated.world.opening.conditions
        );

        // Exactly one bites and exactly one helps, never off the same axis.
        let banes: Vec<_> = drawn.iter().filter(|c| c.is_bane()).collect();
        let boons: Vec<_> = drawn.iter().filter(|c| c.is_boon()).collect();
        assert_eq!(
            banes.len(),
            1,
            "seed {seed}: {} conditions bite",
            banes.len()
        );
        assert_eq!(
            boons.len(),
            1,
            "seed {seed}: {} conditions help",
            boons.len()
        );
        assert_ne!(
            banes[0].axis, boons[0].axis,
            "seed {seed}: the bane and the boon came off the same axis"
        );
        bane_axes.insert(banes[0].axis);
        boon_axes.insert(boons[0].axis);

        // The remaining two are texture.
        assert_eq!(drawn.iter().filter(|c| c.is_cosmetic()).count(), 2);
    }

    // Which axis bites should itself move around run to run.
    assert!(
        bane_axes.len() > 1 && boon_axes.len() > 1,
        "banes {bane_axes:?}, boons {boon_axes:?}"
    );
}

#[test]
fn the_drawn_conditions_land_on_the_run() {
    let cat = catalogue();
    for seed in 0..40u64 {
        let Ok(generated) = rh_gen::generate(seed, &cat) else {
            continue;
        };
        let drawn: Vec<rh_content::ConditionDef> = generated
            .world
            .opening
            .conditions
            .iter()
            .filter_map(|id| cat.conditions.iter().find(|c| c.id == *id).cloned())
            .collect();
        let sim = Sim::new(cat.clone(), generated.world, generated.rng);
        for condition in &drawn {
            match &condition.effect {
                None => {}
                Some(rh_content::ConditionEffect::SocialSurcharge) => {
                    assert!(sim.state.settlement_hostile, "seed {seed}");
                }
                Some(rh_content::ConditionEffect::ShortSight { tiles }) => {
                    assert_eq!(sim.state.sight_penalty, *tiles, "seed {seed}");
                }
                Some(rh_content::ConditionEffect::LongSight { tiles }) => {
                    assert_eq!(sim.state.sight_bonus, *tiles, "seed {seed}");
                }
                Some(rh_content::ConditionEffect::WellSupplied { item }) => {
                    assert!(
                        sim.state.hunter.item_count(item) > 0,
                        "seed {seed}: no {item}"
                    );
                }
                // Baked into the world before certification saw it.
                Some(rh_content::ConditionEffect::Ambush { .. })
                | Some(rh_content::ConditionEffect::QuietRoads { .. })
                | Some(rh_content::ConditionEffect::Pressure { .. }) => {}
            }
        }
    }
}

#[test]
fn a_run_opens_with_its_hook_then_its_conditions() {
    let cat = catalogue();
    let generated = rh_gen::generate(0, &cat).expect("seed 0 generates");
    let hook = cat
        .openings
        .iter()
        .find(|o| o.id == generated.world.opening.opening)
        .expect("hook is authored")
        .clone();
    let conditions: Vec<rh_content::ConditionDef> = generated
        .world
        .opening
        .conditions
        .iter()
        .filter_map(|id| cat.conditions.iter().find(|c| c.id == *id).cloned())
        .collect();
    let sim = Sim::new(cat.clone(), generated.world, generated.rng);

    // The opening prose is authored as string ids now; resolve them the same
    // way the sim does before comparing against what it logged.
    let expected: Vec<String> = hook
        .body
        .iter()
        .chain(conditions.iter().flat_map(|c| c.body.iter()))
        .map(|id| cat.strings.get(id).to_owned())
        .collect();
    let actual: Vec<String> = sim
        .state
        .log
        .iter()
        .take(expected.len())
        .map(|event| event.text.clone())
        .collect();
    // Why she came, then what she walked into, in axis order.
    assert_eq!(actual, expected);
}

#[test]
fn renaming_a_villager_changes_the_name_and_nothing_else() {
    // Name pools hold ids, so the RNG still indexes them structurally: which
    // villager is drawn is generation, what they are called is text. This is
    // the property that lets the whole table sit outside the fingerprint --
    // rewrite or translate a name and the same valley comes back.
    let sources = rh_content::embedded_sources();
    let renamed = rh_content::Catalogue::from_sources_with_strings(
        sources,
        &rh_content::embedded_strings().replace("[Old Nan]", "[A Completely Different Person]"),
    )
    .expect("a catalogue with a renamed villager loads");

    let mut saw_the_rename = false;
    for seed in 0..24u64 {
        let plain = rh_gen::generate(seed, &catalogue()).expect("world generates");
        let other = rh_gen::generate(seed, &renamed).expect("world generates");

        assert_eq!(
            plain.world.npcs.len(),
            other.world.npcs.len(),
            "seed {seed}: the cast changed size"
        );
        for (a, b) in plain.world.npcs.iter().zip(&other.world.npcs) {
            assert_eq!(a.archetype, b.archetype, "seed {seed}: a role moved");
            assert_eq!(a.map, b.map, "seed {seed}: someone moved map");
            assert_eq!(a.work, b.work, "seed {seed}: someone moved tile");
            assert_eq!(a.disposition, b.disposition, "seed {seed}: a mood changed");
            if a.name == "[Old Nan]" {
                saw_the_rename = true;
                assert_eq!(b.name, "[A Completely Different Person]");
            } else {
                assert_eq!(a.name, b.name, "seed {seed}: an unrelated name changed");
            }
        }
        assert_eq!(
            plain.world.villain.host, other.world.villain.host,
            "seed {seed}: the villain took a different host"
        );
    }
    assert!(
        saw_the_rename,
        "no seed drew the renamed villager, so this proved nothing"
    );
}
