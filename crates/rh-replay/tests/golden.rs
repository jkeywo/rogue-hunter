//! Golden replay checks: the autoplayer must win complete runs for every
//! villain archetype, and every recorded command log must reproduce the
//! exact same run when replayed from its share code. These are the CI
//! "replay checks" from the PASM ci-and-pages-release contract.

use std::collections::BTreeMap;

use rh_content::Catalogue;
use rh_core::state::Outcome;
use rh_replay::{autoplay, RunSession};

fn catalogue() -> Catalogue {
    rh_content::load_embedded().expect("embedded content")
}

#[test]
fn autoplayer_wins_runs_for_every_archetype() {
    // Driven off the catalogue rather than a hardcoded pair: adding a villain
    // must extend the proof obligation, not silently escape it.
    let mut wins: BTreeMap<String, u32> = catalogue()
        .villains
        .keys()
        .map(|id| (id.clone(), 0))
        .collect();
    let mut outcomes = Vec::new();

    // Three villains times three origins times three schemes needs a wider
    // sweep than two archetypes did to see each villain a few times.
    for seed in 0..96u64 {
        let mut session = RunSession::new(seed, catalogue()).expect("run generates");
        let archetype = session.sim.world.villain.archetype.clone();
        let outcome = autoplay::autoplay(&mut session);
        outcomes.push(format!(
            "seed {seed}: {archetype} ({}/{}) -> {outcome:?} after {} commands (clock {})",
            session.sim.world.villain.origin,
            session.sim.world.villain.scheme,
            session.commands.len(),
            session.sim.state.clock,
        ));
        if outcome == Some(Outcome::Victory) {
            *wins.entry(archetype).or_insert(0) += 1;

            // Replay determinism through the full stack: the share code must
            // rebuild the identical end state.
            let code = session.share_code();
            let replayed =
                RunSession::from_share_code(&code, catalogue()).expect("share code replays");
            assert_eq!(
                replayed.state_digest(),
                session.state_digest(),
                "seed {seed}: replay diverged from the live run"
            );
            assert_eq!(replayed.outcome(), Some(Outcome::Victory));
        }
    }

    let unbeaten: Vec<_> = wins
        .iter()
        .filter(|(_, won)| **won == 0)
        .map(|(id, _)| id.clone())
        .collect();
    assert!(
        unbeaten.is_empty(),
        "the autoplayer never won as {}:\n{}",
        unbeaten.join(", "),
        outcomes.join("\n")
    );
}
