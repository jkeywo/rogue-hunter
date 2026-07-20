//! Diagnostic trace for autoplayer tuning. Run with --ignored --nocapture.

use rh_replay::{autoplay, RunSession};

#[test]
#[ignore = "diagnostic trace; run with --ignored --nocapture"]
fn trace_seed() {
    let seed: u64 = std::env::var("RH_DEBUG_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let hunter = std::env::var("RH_DEBUG_HUNTER").unwrap_or_else(|_| "huntress".to_owned());
    let catalogue = rh_content::load_embedded().expect("content");
    // Reject forward to the same world the corpus scan drove, so a trace of a
    // losing seed is a trace of the loss the scan reported.
    let (mut session, used) =
        RunSession::new_from_viable_seed(seed, catalogue, &hunter).expect("run generates");
    println!(
        "=== seed {seed} (used {used}) as {hunter}: {} ===",
        session.sim.world.villain.archetype
    );
    for route in &session.sim.world.certified_routes {
        println!(
            "route '{}' ready t{} viability {}",
            route.label, route.ready_by_turn, route.viability_permille
        );
        for step in &route.steps {
            println!("  t{} {}", step.turn, step.description);
        }
    }
    let outcome = autoplay::autoplay(&mut session);
    println!(
        "outcome {outcome:?} clock {} commands {}",
        session.sim.state.clock,
        session.commands.len()
    );
    println!("--- event log ---");
    for event in &session.sim.state.log {
        println!(
            "g{} l{} [{:?}] {}",
            event.global_turn, event.local_turn, event.kind, event.text
        );
    }
}
