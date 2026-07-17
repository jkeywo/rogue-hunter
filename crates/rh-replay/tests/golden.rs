//! Golden replay checks: the autoplayer must win complete runs for both
//! villain archetypes, and every recorded command log must reproduce the
//! exact same run when replayed from its share code. These are the CI
//! "replay checks" from the PASM ci-and-pages-release contract.

use rh_content::Catalogue;
use rh_core::state::Outcome;
use rh_replay::{autoplay, RunSession};

fn catalogue() -> Catalogue {
    rh_content::load_embedded().expect("embedded content")
}

#[test]
fn autoplayer_wins_runs_for_both_archetypes() {
    let mut werewolf_wins = 0u32;
    let mut revenant_wins = 0u32;
    let mut outcomes = Vec::new();

    for seed in 0..32u64 {
        let mut session = RunSession::new(seed, catalogue()).expect("run generates");
        let archetype = session.sim.world.villain.archetype.clone();
        let outcome = autoplay::autoplay(&mut session);
        outcomes.push(format!(
            "seed {seed}: {archetype} -> {outcome:?} after {} commands (clock {})",
            session.commands.len(),
            session.sim.state.clock,
        ));
        if outcome == Some(Outcome::Victory) {
            match archetype.as_str() {
                "werewolf" => werewolf_wins += 1,
                "revenant" => revenant_wins += 1,
                _ => {}
            }

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

    assert!(
        werewolf_wins >= 1 && revenant_wins >= 1,
        "the autoplayer must win at least one run per archetype:\n{}",
        outcomes.join("\n")
    );
}
