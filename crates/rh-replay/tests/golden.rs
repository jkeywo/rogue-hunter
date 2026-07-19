//! Golden replay checks: the autoplayer must win complete runs for every
//! villain archetype, and every recorded command log must reproduce the
//! exact same run when replayed from its share code. These are the CI
//! "replay checks" from the PASM ci-and-pages-release contract.

use std::collections::BTreeMap;

use rh_content::Catalogue;
use rh_core::state::Outcome;
use rh_core::world::RouteAction;
use rh_replay::{autoplay, RunSession};

fn catalogue() -> Catalogue {
    rh_content::load_embedded().expect("embedded content")
}

#[test]
fn autoplayer_wins_runs_for_every_hunter() {
    // Every selectable hunter must be able to finish a case. A hunter who
    // cannot is a certification bug, not a difficulty setting.
    for hunter in catalogue().hunters.keys() {
        let mut wins = 0u32;
        let mut outcomes = Vec::new();
        for seed in 0..48u64 {
            // Certification may refuse a world for this hunter; rejecting
            // forward is the contract, so the test asserts a playable run is
            // always reachable rather than that every seed is usable.
            let (mut session, _used) = RunSession::new_from_viable_seed(seed, catalogue(), hunter)
                .unwrap_or_else(|error| panic!("{hunter} near seed {seed}: {error}"));
            let outcome = autoplay::autoplay(&mut session);
            outcomes.push(format!(
                "seed {seed}: {} -> {outcome:?} after {} commands",
                session.sim.world.villain.archetype,
                session.commands.len(),
            ));
            if outcome == Some(Outcome::Victory) {
                wins += 1;
                let code = session.share_code();
                let replayed =
                    RunSession::from_share_code(&code, catalogue()).expect("share code replays");
                assert_eq!(
                    replayed.hunter, *hunter,
                    "seed {seed}: the share code must restore the hunter it was recorded for"
                );
                assert_eq!(
                    replayed.state_digest(),
                    session.state_digest(),
                    "seed {seed}: replay diverged from the live run"
                );
            }
        }
        assert!(
            wins > 0,
            "the autoplayer never won as {hunter}:\n{}",
            outcomes.join("\n")
        );
    }
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

#[test]
fn no_run_ever_logs_an_unresolved_string() {
    // The string table resolves missing ids to a visible sentinel rather than
    // panicking, so this is what stops one reaching a player: play real runs
    // and read every line the log produced.
    for seed in 0..12u64 {
        let mut session = RunSession::new(seed, catalogue()).expect("run generates");
        autoplay::autoplay(&mut session);
        for event in &session.sim.state.log {
            assert!(
                !event.text.contains("!missing"),
                "seed {seed}: unresolved string id in the log: {:?}",
                event.text
            );
            assert!(
                !event.text.contains('{'),
                "seed {seed}: unsubstituted placeholder in the log: {:?}",
                event.text
            );
            // Every log line resolves through the table, and every row in the
            // table is bracketed placeholder copy -- so a line that is not
            // bracketed is prose still hardcoded in Rust. This is what catches
            // the ones a grep for `format!` misses, like a bare `.to_owned()`.
            assert!(
                event.text.starts_with('[') && event.text.ends_with(']'),
                "seed {seed}: log line is not from the string table: {:?}",
                event.text
            );
        }
    }
}

#[test]
fn translating_the_route_text_does_not_change_what_the_autoplayer_does() {
    // The autoplayer used to recover each step by parsing its description --
    // starts_with("Craft: "), strip_prefix("Travel to "). Translating the game
    // would have silently stopped it crafting, and it fails soft, so the
    // golden replays would have kept passing while the bot did less. Steps
    // carry a typed action now, so rewriting every route string must leave the
    // command log identical, not merely still winning.
    let sources = rh_content::embedded_sources();
    let translated_csv: String = rh_content::embedded_strings()
        .lines()
        .map(|line| {
            if line.starts_with("ui.route.") {
                // Keep the id and context, replace the English wholesale --
                // no "Craft: " prefix, no recognisable word, placeholders gone.
                let mut parts = line.splitn(3, ',');
                let id = parts.next().unwrap_or_default();
                format!("{id},Translated for the test,\"[???]\"")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");

    let plain = rh_content::Catalogue::from_sources(sources).expect("embedded content");
    let translated = rh_content::Catalogue::from_sources_with_strings(sources, &translated_csv)
        .expect("a catalogue with translated route text loads");
    assert_eq!(
        translated.strings.try_get("ui.route.craft"),
        Some("[???]"),
        "the perturbation must actually replace the route copy"
    );

    let hunter = plain.hunter_id.clone();
    let drive = |catalogue: Catalogue| {
        let (mut session, _used) =
            RunSession::new_from_viable_seed(7, catalogue, &hunter).expect("a viable run");
        let outcome = autoplay::autoplay(&mut session);
        (outcome, session.commands.clone(), session.state_digest())
    };

    let (plain_outcome, plain_commands, plain_digest) = drive(plain);
    let (other_outcome, other_commands, other_digest) = drive(translated);

    assert_eq!(plain_outcome, other_outcome, "the outcome changed");
    assert_eq!(
        plain_commands, other_commands,
        "the autoplayer made different moves once the route text changed"
    );
    assert_eq!(plain_digest, other_digest, "the run diverged");
    assert!(
        !plain_commands.is_empty(),
        "the run must actually do something for this to prove anything"
    );
    // And it must exercise the dispatch that used to be parsed out of prose,
    // or the invariance above would be about a route with nothing in it.
    let (mut probe, _used) =
        RunSession::new_from_viable_seed(7, catalogue(), &hunter).expect("a viable run");
    autoplay::autoplay(&mut probe);
    let route = probe
        .sim
        .world
        .certified_routes
        .first()
        .expect("the run is certified");
    let kinds: Vec<&str> = route
        .steps
        .iter()
        .map(|step| match step.action {
            RouteAction::Resolve(_) => "resolve",
            RouteAction::Craft { .. } => "craft",
            RouteAction::Consecrate => "consecrate",
            RouteAction::Travel(_) => "travel",
            RouteAction::InitiateHunt => "initiate",
        })
        .collect();
    for required in ["craft", "travel", "resolve"] {
        assert!(
            kinds.contains(&required),
            "seed 7's route has no {required} step, so this proves nothing about it: {kinds:?}"
        );
    }
}
