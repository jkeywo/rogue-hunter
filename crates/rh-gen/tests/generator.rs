//! Generator contract tests: determinism, corpus coverage, and route
//! certification. The corpus test doubles as the bounded local stress
//! validation (it must stay well under the 30-second local budget).

use std::collections::BTreeSet;

use rh_content::Catalogue;

fn catalogue() -> Catalogue {
    rh_content::load_embedded().expect("embedded content")
}

#[test]
fn generation_is_deterministic() {
    let catalogue = catalogue();
    let first = rh_gen::generate(42, &catalogue).expect("seed 42 generates");
    let second = rh_gen::generate(42, &catalogue).expect("seed 42 generates again");
    assert_eq!(
        rh_core::hash::digest(&first.world),
        rh_core::hash::digest(&second.world),
        "same seed must produce byte-identical worlds"
    );
    assert_eq!(
        first.rng, second.rng,
        "post-generation RNG state must match"
    );
}

#[test]
fn corpus_covers_every_villain_combination() {
    let catalogue = catalogue();
    let mut combos_seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    let total_combos = catalogue.villains.len() * catalogue.origins.len() * catalogue.schemes.len();
    let mut failures = Vec::new();
    let mut generated = 0u32;

    // Twenty-seven compositions need a longer sweep than eight did: coupon
    // collection over 27 uniform outcomes averages around 105 draws.
    for seed in 0..400u64 {
        match rh_gen::generate(seed, &catalogue) {
            Ok(result) => {
                generated += 1;
                combos_seen.insert((
                    result.report.villain.clone(),
                    result.report.origin.clone(),
                    result.report.scheme.clone(),
                ));
            }
            Err(error) => failures.push(format!("seed {seed}: {error}")),
        }
        if combos_seen.len() == total_combos && seed >= 127 {
            break;
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} seeds failed to generate:\n{}",
        failures.len(),
        generated + failures.len() as u32,
        failures.join("\n")
    );
    assert_eq!(
        combos_seen.len(),
        total_combos,
        "corpus must cover all {total_combos} villain combinations, saw {:?}",
        combos_seen
    );
}

#[test]
fn certified_routes_meet_the_generator_contract() {
    // Every hunter is held to the contract, not just the default one. A new
    // hunter is obligated the moment she is in the roster, which is the whole
    // point of certifying per hunter: the Confessor cannot quietly ship with
    // routes that break the budgets the Huntress's never would.
    let base = catalogue();
    for hunter in base.hunters.keys() {
        let catalogue = base.clone().with_hunter(hunter).expect("hunter");
        certified_routes_hold_for(&catalogue, hunter);
    }
}

fn certified_routes_hold_for(catalogue: &Catalogue, hunter: &str) {
    let generator = &catalogue.balance.generator;

    // Check the budgets on the first several worlds that certify, rather than
    // demanding every seed certify: not every seed can be given fairly to
    // every hunter, and proving a seed *cannot* be is the planner's slowest
    // path. Eight certified worlds exercise the contract without paying for a
    // string of exhaustive refusals — a real cost for the Confessor, whose
    // social routes the planner searches hardest before giving up on.
    let mut checked = 0u32;
    for seed in 0..64u64 {
        let Ok(result) = rh_gen::generate(seed, catalogue) else {
            continue;
        };
        checked += 1;
        let routes = &result.world.certified_routes;
        assert_eq!(routes.len(), 2, "seed {seed}: two certified routes");

        let early = &routes[0];
        let fallback = &routes[1];
        assert!(
            early.ready_by_turn <= generator.early_route_deadline,
            "seed {seed}: early route ready at turn {}",
            early.ready_by_turn
        );
        assert!(
            fallback.ready_by_turn <= generator.fallback_route_deadline,
            "seed {seed}: fallback ready at turn {}",
            fallback.ready_by_turn
        );
        for route in routes {
            assert!(
                route.viability_permille >= generator.viability_threshold_permille,
                "seed {seed}: route '{}' viability {}",
                route.label,
                route.viability_permille
            );
            assert!(
                route.total_effort <= generator.route_effort_budget,
                "seed {seed}: route '{}' effort {}",
                route.label,
                route.total_effort
            );
            assert!(
                route.travel_legs <= generator.route_travel_budget,
                "seed {seed}: route '{}' legs {}",
                route.label,
                route.travel_legs
            );
        }
        assert!(
            fallback.total_obscurity <= generator.fallback_obscurity_budget,
            "seed {seed}: fallback obscurity {}",
            fallback.total_obscurity
        );

        // Route independence: no shared opportunity nodes, except structural
        // access ops (forced doors/rubble stay cleared for both routes).
        let structural = |id: &rh_core::OpportunityId| result.world.opportunity(*id).clears_terrain;
        let early_ops: BTreeSet<_> = early
            .steps
            .iter()
            .filter_map(|s| s.opportunity())
            .filter(|id| !structural(id))
            .collect();
        let fallback_ops: BTreeSet<_> = fallback
            .steps
            .iter()
            .filter_map(|s| s.opportunity())
            .filter(|id| !structural(id))
            .collect();
        assert!(
            early_ops.is_disjoint(&fallback_ops),
            "seed {seed}: routes share opportunities"
        );

        // The mystical boon appears on at most one required route.
        assert!(
            !(early.uses_mystic_favour && fallback.uses_mystic_favour),
            "seed {seed}: both routes lean on the mystical favour"
        );

        if checked >= 8 {
            break;
        }
    }
    assert!(checked >= 8, "{hunter}: too few worlds certified to check");
}

#[test]
fn the_confessor_is_certified_through_people() {
    // The Confessor's whole reason to exist is a route that runs on social
    // work. The planner is not told to prefer it — social_cap 3 only makes it
    // affordable — so this measures whether it happens emergently. If it stops
    // happening, either her caps drifted or the social evidence thinned, and
    // either way she is no longer the hunter she was added to be.
    let cat = catalogue().with_hunter("confessor").expect("confessor");
    let mut runs = 0u32;
    let mut with_social = 0u32;
    // The first sixteen worlds that certify for her. Bounded so a slow refusal
    // does not dominate: for her, a seed that cannot be given fairly is the
    // planner's most expensive answer.
    for seed in 0..64u64 {
        let Ok(result) = rh_gen::generate(seed, &cat) else {
            continue;
        };
        runs += 1;
        let social_steps = result
            .world
            .certified_routes
            .iter()
            .flat_map(|route| route.steps.iter())
            .filter_map(|step| step.opportunity())
            .filter(|id| result.world.opportunity(*id).pool == Some(rh_content::PoolKind::Social))
            .count();
        if social_steps > 0 {
            with_social += 1;
        }
        if runs >= 16 {
            break;
        }
    }
    assert!(runs > 0, "the confessor must certify on some seed");
    // Most of her certified worlds should route through at least one witness.
    // Not all: some cases genuinely lie in the wood or the grave, and forcing
    // a social step onto those would be the planner preferring flavour over
    // the shortest honest route.
    assert!(
        with_social * 2 >= runs,
        "only {with_social} of {runs} confessor worlds route through anyone - \
         her social identity is not reaching her certified routes"
    );
}

#[test]
#[ignore = "diagnostic sweep: per-hunter generation failure rate; run with --ignored"]
fn sweep_generation_failures_per_hunter() {
    let base = catalogue();
    for hunter in base.hunters.keys() {
        let cat = base.clone().with_hunter(hunter).expect("hunter");
        let mut failures = Vec::new();
        for seed in 0..120u64 {
            if let Err(error) = rh_gen::generate(seed, &cat) {
                failures.push(format!("{seed}: {error}"));
            }
        }
        println!(
            "{hunter}: {} failures in 120 seeds{}",
            failures.len(),
            if failures.is_empty() {
                String::new()
            } else {
                format!("\n  {}", failures.join("\n  "))
            }
        );
    }
}

#[test]
fn every_role_template_gets_used_and_selection_is_deterministic() {
    let catalogue = catalogue();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for seed in 0..64u64 {
        let Ok(result) = rh_gen::generate(seed, &catalogue) else {
            continue;
        };
        // Exactly one template per role, in role order, and the world agrees
        // with what the report claims.
        assert_eq!(result.report.templates.len(), 3);
        let roles: Vec<_> = result.world.maps.iter().map(|map| map.role).collect();
        assert_eq!(
            roles,
            rh_content::MapRole::ORDER.to_vec(),
            "seed {seed}: maps must stay in role order"
        );
        for template in &result.report.templates {
            seen.insert(template.clone());
        }

        // Same seed, same dressing.
        let again = rh_gen::generate(seed, &catalogue).expect("regenerates");
        assert_eq!(
            again.report.templates, result.report.templates,
            "seed {seed}: template choice must be deterministic"
        );
    }

    // Every authored template must be reachable by some seed, or it is dead
    // content that no player will ever see.
    for role in rh_content::MapRole::ORDER {
        for template in catalogue.templates_for(role) {
            assert!(
                seen.contains(template),
                "no seed in 0..64 ever chose template '{template}'"
            );
        }
    }
}

#[test]
fn every_run_opens_somewhere_and_banked_nodes_are_honest() {
    let base = catalogue();
    let mut banked = 0u32;
    let mut generic = 0u32;

    for hunter in base.hunters.keys() {
        let cat = base.clone().with_hunter(hunter).expect("hunter");
        // Enough certified worlds to see both opening kinds, capped so a run of
        // slow refusals cannot dominate — the Confessor's uncertifiable seeds
        // are the planner's most expensive answer, and this test does not need
        // to pay for them to check that openings are honest.
        let mut checked = 0u32;
        for seed in 0..64u64 {
            let Ok(result) = rh_gen::generate(seed, &cat) else {
                continue;
            };
            checked += 1;
            if checked > 16 {
                break;
            }
            let opening = &result.world.opening;
            assert!(
                cat.openings.iter().any(|o| o.id == opening.opening),
                "{hunter} seed {seed}: opening '{}' is not authored",
                opening.opening
            );

            let Some(prior) = opening.prior else {
                generic += 1;
                assert!(
                    cat.openings
                        .iter()
                        .any(|o| o.id == opening.opening && o.is_generic()),
                    "{hunter} seed {seed}: banked nothing but used a keyed opening"
                );
                continue;
            };
            banked += 1;
            let spec = result.world.opportunity(prior);

            // A banked node must be one it is honest to have already resolved.
            assert!(
                !spec.clears_terrain,
                "{hunter} seed {seed}: banked a forced door"
            );
            assert!(
                spec.requires.is_none(),
                "{hunter} seed {seed}: banked a gated node"
            );
            assert!(
                !matches!(
                    spec.grants,
                    rh_core::world::OpportunityGrant::IdentityClue {
                        discriminating: true
                    }
                ),
                "{hunter} seed {seed}: banked a discriminating identity clue, which would leave \
                 one ambiguous sign between the player and the villain's name"
            );
            assert!(
                !matches!(spec.grants, rh_core::world::OpportunityGrant::MysticFavour),
                "{hunter} seed {seed}: banked the mystical favour"
            );

            // The whole point: it belongs to neither route, because it was
            // resolved before either began.
            for route in &result.world.certified_routes {
                assert!(
                    !route
                        .steps
                        .iter()
                        .any(|step| step.opportunity() == Some(prior)),
                    "{hunter} seed {seed}: the banked node still appears in route '{}'",
                    route.label
                );
            }
        }
    }

    assert!(generic > 0, "no run opened on a generic hook");
    assert!(
        banked > 0,
        "no run banked a node, so the in-media-res path was never exercised"
    );
}

#[test]
fn banking_lets_the_occultist_be_given_every_seed() {
    // These seeds could not be certified for her before nodes could be banked.
    let cat = catalogue().with_hunter("occultist").expect("occultist");
    let mut failures = Vec::new();
    for seed in 0..48u64 {
        if let Err(error) = rh_gen::generate(seed, &cat) {
            failures.push(format!("{seed}: {error}"));
        }
    }
    assert!(
        failures.is_empty(),
        "the Occultist was refused {} seeds:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn milestone_two_variety_is_observable() {
    // The corpus must actually show a player everything that was authored:
    // every pack, every machine, every event family. Content no seed can
    // reach is a bug, not a spare — the same assertion that caught the
    // materialiser reporting role labels as template ids in stage 3.
    let catalogue = catalogue();
    let mut packs_seen: BTreeSet<String> = BTreeSet::new();
    let mut machines_seen: BTreeSet<String> = BTreeSet::new();
    let mut events_seen: BTreeSet<String> = BTreeSet::new();

    for seed in 0..96u64 {
        let Ok(result) = rh_gen::generate(seed, &catalogue) else {
            continue;
        };
        let report = &result.report;
        for packs in &report.packs {
            for pack in packs {
                packs_seen.insert(pack.clone());
            }
        }
        for machine in &report.machines {
            machines_seen.insert(machine.clone());
        }
        for deck in &report.events {
            for event in deck {
                events_seen.insert(event.clone());
            }
        }

        // Conflicting packs never co-occur on the same map.
        for (index, map) in result.world.maps.iter().enumerate() {
            let template = &catalogue.maps[&map.template];
            let drawn = &result.world.packs[index];
            for id in drawn {
                let Some(pack) = template.packs.iter().find(|pack| &pack.id == id) else {
                    panic!(
                        "seed {seed}: pack '{id}' is not authored on '{}'",
                        map.template
                    );
                };
                for conflict in &pack.conflicts_with {
                    assert!(
                        !drawn.contains(conflict),
                        "seed {seed}: '{id}' and '{conflict}' drawn together on '{}'",
                        map.template
                    );
                }
            }
        }

        // A machine is never load-bearing: no certified route step touches one.
        for route in &result.world.certified_routes {
            for step in &route.steps {
                if let Some(op) = step.opportunity() {
                    assert!(
                        !matches!(
                            result.world.opportunity(op).grants,
                            rh_core::world::OpportunityGrant::Machine { .. }
                        ),
                        "seed {seed}: route '{}' leans on a machine",
                        route.label
                    );
                }
            }
        }

        // Same seed, same dressing.
        let again = rh_gen::generate(seed, &catalogue).expect("regenerates");
        assert_eq!(
            again.report.packs, report.packs,
            "seed {seed}: packs drifted"
        );
        assert_eq!(
            again.report.events, report.events,
            "seed {seed}: decks drifted"
        );
    }

    for (template, def) in &catalogue.maps {
        for pack in &def.packs {
            assert!(
                packs_seen.contains(&pack.id),
                "no seed in 0..96 ever drew pack '{}' on '{template}'",
                pack.id
            );
        }
    }
    for machine in catalogue.machines.keys() {
        assert!(
            machines_seen.contains(machine),
            "no seed in 0..96 ever embedded machine '{machine}'"
        );
    }
    for event in catalogue.events.keys() {
        assert!(
            events_seen.contains(event),
            "no seed in 0..96 ever dealt event '{event}'"
        );
    }
}
