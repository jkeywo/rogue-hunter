//! Rogue-hunter against the shared replay contract.
//!
//! `golden.rs` and `golden_values.rs` assert that runs reproduce. These assert
//! the properties underneath that — the ones that can be lost in a refactor
//! without any rule visibly breaking — checked by `vellum-replay`, so that
//! both games are held to the same statement of them.
//!
//! The one worth having is [`rejection_is_pure`]: it compares the state
//! *digest* either side of a refused command. `Sim::apply` documents that on
//! rejection nothing changed at all; a refusal that quietly consumed a draw
//! from the single RNG stream would shift every later draw while leaving the
//! visible outcome intact, and no share code records how many illegal things a
//! player tried. This is what checks it.

use rh_content::Catalogue;
use rh_core::command::Command;
use rh_core::sim::Sim;
use rh_replay::RunSession;
use vellum_replay::contract;

fn catalogue() -> Catalogue {
    rh_content::load_embedded().expect("embedded content")
}

fn sim_at(seed: u64) -> Sim {
    RunSession::new(seed, catalogue())
        .expect("run generates")
        .sim
}

/// Waiting always applies, so the script exercises the driver rather than
/// clever play; the golden suites cover real runs.
fn script() -> Vec<Command> {
    vec![Command::Wait, Command::Wait, Command::Wait]
}

#[test]
fn rogue_hunter_keeps_the_replay_contract() {
    // Consecration needs the altar underfoot, and the hunter does not start on
    // it — a refusal that does not depend on which villain the seed drew.
    contract::check_all(|| sim_at(7), &script(), &Command::Consecrate);
}

#[test]
fn the_contract_holds_for_more_than_one_seed() {
    // Travelling needs a marked exit underfoot; the hunter starts inland.
    contract::check_all(|| sim_at(23), &script(), &Command::Travel);
}

/// Uncovering the villain before the proofs corroborate is the refusal a
/// player meets most often, and it sits on the evidence rules rather than on
/// position — a different part of the simulation from the two above.
#[test]
fn an_early_accusation_costs_nothing() {
    contract::rejection_is_pure(|| sim_at(7), &script(), &Command::UncoverVillain);
}
