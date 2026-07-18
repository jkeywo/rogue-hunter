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
    let catalogue = catalogue();
    let generator = &catalogue.balance.generator;

    for seed in 0..24u64 {
        let result = rh_gen::generate(seed, &catalogue).expect("seed generates");
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
            .filter_map(|s| s.opportunity)
            .filter(|id| !structural(id))
            .collect();
        let fallback_ops: BTreeSet<_> = fallback
            .steps
            .iter()
            .filter_map(|s| s.opportunity)
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
    }
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
