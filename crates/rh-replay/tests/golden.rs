//! Golden replay checks: the autoplayer must win complete runs for every
//! villain archetype, and every recorded command log must reproduce the
//! exact same run when replayed from its share code. These are the CI
//! "replay checks" from the PASM ci-and-pages-release contract.

use std::collections::BTreeMap;

use rh_content::Catalogue;
use rh_core::state::Outcome;
use rh_core::world::RouteAction;
use rh_replay::{autoplay, corpus, RunSession};

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

/// The estimate and the game it prices, held to each other. The viability
/// heuristic was calibrated against autoplayer runs rather than theory
/// (viability-model-calibration); this scan keeps that calibration honest.
/// One-sided on purpose: the autoplayer winning more often than certified is
/// fine, the estimate over-promising by a wide margin is the drift being
/// caught.
///
/// The band is wide because the two numbers measure different journeys: the
/// estimate prices the final fight under the route's loadout, while the scan
/// drives the whole run — ambushes, events, and the bot's own imperfect play
/// are deliberately unmodelled variance. First run measured the huntress at
/// a ~300 permille gap (won 645, certified 946) and the occultist at ~530
/// (won 354, certified 884): the occultist's extra distance is recorded
/// calibration debt — her bot tactics or her estimate need work — and the
/// band pins today's reality so any further drift trips it.
///
/// The band only ever moves down, and every reduction is backed by a scan.
/// Raising it would be hiding the debt rather than paying it.
const AGREEMENT_BAND_PERMILLE: u32 = 550;

#[test]
#[ignore = "slow corpus scan: holds the win rate to the certified estimate; run with --ignored"]
fn win_rate_tracks_certified_viability() {
    for hunter in catalogue().hunters.keys() {
        let records = corpus::scan(&catalogue(), hunter, 0..48);
        let summary = corpus::summarise(&records);
        let table = corpus::table(&records);
        // Printed either way: on failure it is the diagnosis, and under
        // --nocapture it is the instrument this milestone reads.
        println!("{table}");
        assert!(
            summary.won_permille + AGREEMENT_BAND_PERMILLE >= summary.promised_permille,
            "{hunter}: won {} permille of driven runs against a certified {} permille -              the estimate is over-promising
{table}",
            summary.won_permille,
            summary.promised_permille
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

/// Pin the instrument before trusting what it says. A diagnosis is only worth
/// as much as the report it reads, and the report has to agree with the world
/// it describes: the promise it quotes must be the route's own, and a run that
/// reached the fight must have been rescored against what she was carrying.
#[test]
fn the_autoplay_report_agrees_with_the_world_it_describes() {
    for hunter in catalogue().hunters.keys() {
        let (mut session, _used) = RunSession::new_from_viable_seed(17, catalogue(), hunter)
            .expect("seed 17 certifies for every hunter");
        let promised = session
            .sim
            .world
            .certified_routes
            .first()
            .map(|route| route.viability_permille)
            .expect("a certified world has a route");
        let report = autoplay::autoplay_reported(&mut session);

        assert_eq!(
            report.certified_permille, promised,
            "{hunter}: the report must quote the route's own promise"
        );
        assert_eq!(
            report.outcome,
            session.outcome(),
            "{hunter}: the report's outcome must be the world's"
        );
        assert_eq!(
            report.stage == corpus::stage_of_won(),
            report.end == rh_replay::autoplay::RunEnd::Victory,
            "{hunter}: only a victory is staged as won"
        );
        // Reaching the fight is what makes a rescore possible, and the two
        // must never disagree - a rescore without a fight would be pricing a
        // loadout that was never taken into one.
        assert_eq!(
            report.final_hunt_entered,
            report.rescored_permille.is_some(),
            "{hunter}: a rescore exists exactly when the fight was reached"
        );
        assert_eq!(
            report.loadout_at_final_hunt.is_some(),
            report.rescored_permille.is_some(),
            "{hunter}: the loadout and its price are recorded together"
        );
        assert!(
            report.route_steps_done <= report.route_steps_total,
            "{hunter}: cannot complete more steps than the route has"
        );
    }
}
