//! `rh` — the headless developer toolchain.
//!
//! Generation inspector (seed, clue graph, certified routes, node costs,
//! candidate rejection reasons), replay checks, corpus stress validation,
//! content validation, and the autoplayer. CI drives these commands; they
//! are also the fastest way to diagnose generator or replay issues locally.

use std::time::Instant;

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use rh_replay::{autoplay, RunSession};

#[derive(Parser)]
#[command(name = "rh", about = "Rogue Hunter headless toolchain", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate the embedded content catalogue.
    Validate,
    /// Generate a world and print the inspector report.
    Generate {
        #[arg(long)]
        seed: u64,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Replay a share code and report the outcome.
    Replay {
        /// The RH1- share code (or a path to a file containing one).
        code: String,
        /// Print the full event log rather than the tail.
        #[arg(long)]
        full_log: bool,
    },
    /// Let the autoplayer drive a run from a seed.
    Autoplay {
        #[arg(long)]
        seed: u64,
        /// Print the resulting share code.
        #[arg(long)]
        emit_code: bool,
    },
    /// Bounded generator stress validation over a seed corpus.
    Corpus {
        /// Number of seeds to generate, starting from 0.
        #[arg(long, default_value_t = 64)]
        count: u64,
        /// Fail if the corpus takes longer than this many seconds.
        #[arg(long)]
        budget_seconds: Option<u64>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let catalogue = rh_content::load_embedded().context("content catalogue failed validation")?;

    match cli.command {
        Command::Validate => {
            println!(
                "content: OK ({} items, {} clues, {} maps)",
                catalogue.items.len(),
                catalogue.clues.len(),
                catalogue.maps.len()
            );
        }
        Command::Generate { seed, json } => {
            let generated = rh_gen::generate(seed, &catalogue)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&generated.report)?);
            } else {
                print_report(&generated);
            }
        }
        Command::Replay { code, full_log } => {
            let code = if code.starts_with("RH1-") {
                code
            } else {
                std::fs::read_to_string(&code)
                    .with_context(|| format!("reading share code file '{code}'"))?
            };
            let session = RunSession::from_share_code(code.trim(), catalogue)?;
            println!("seed: {}", session.seed);
            println!("commands: {}", session.commands.len());
            println!(
                "clock: {} of {}",
                session.sim.state.clock, session.sim.catalogue.balance.clock.travel_turns
            );
            println!("outcome: {:?}", session.outcome());
            println!("digest: {:016x}", session.state_digest());
            let log = &session.sim.state.log;
            let shown: Vec<_> = if full_log {
                log.iter().collect()
            } else {
                log.iter().rev().take(15).rev().collect()
            };
            println!(
                "--- event log ({} of {} events) ---",
                shown.len(),
                log.len()
            );
            for event in shown {
                println!(
                    "[g{} l{}] {}",
                    event.global_turn, event.local_turn, event.text
                );
            }
        }
        Command::Autoplay { seed, emit_code } => {
            let mut session = RunSession::new(seed, catalogue)?;
            let villain = format!(
                "{} / {} / {}",
                session.sim.world.villain.archetype,
                session.sim.world.villain.origin,
                session.sim.world.villain.scheme
            );
            let outcome = autoplay::autoplay(&mut session);
            println!("seed: {seed}");
            println!("villain: {villain}");
            println!("outcome: {outcome:?}");
            println!("commands: {}", session.commands.len());
            println!("clock: {}", session.sim.state.clock);
            println!("digest: {:016x}", session.state_digest());
            if emit_code {
                println!("share code:\n{}", session.share_code());
            }
        }
        Command::Corpus {
            count,
            budget_seconds,
        } => {
            let started = Instant::now();
            let mut failures = Vec::new();
            let mut combos = std::collections::BTreeSet::new();
            for seed in 0..count {
                match rh_gen::generate(seed, &catalogue) {
                    Ok(generated) => {
                        combos.insert(format!(
                            "{}/{}/{}",
                            generated.report.villain,
                            generated.report.origin,
                            generated.report.scheme
                        ));
                    }
                    Err(error) => failures.push(format!("seed {seed}: {error}")),
                }
            }
            let elapsed = started.elapsed();
            println!(
                "corpus: {count} seeds in {:.1}s, {} failures, {} villain combinations",
                elapsed.as_secs_f64(),
                failures.len(),
                combos.len()
            );
            for failure in &failures {
                println!("  {failure}");
            }
            let total =
                catalogue.villains.len() * catalogue.origins.len() * catalogue.schemes.len();
            if !failures.is_empty() {
                bail!("{} corpus seeds failed to generate", failures.len());
            }
            if combos.len() != total {
                bail!(
                    "corpus covered {} of {total} villain combinations",
                    combos.len()
                );
            }
            if let Some(budget) = budget_seconds {
                if elapsed.as_secs() > budget {
                    bail!(
                        "corpus took {:.1}s, over the {budget}s budget",
                        elapsed.as_secs_f64()
                    );
                }
            }
        }
    }
    Ok(())
}

fn print_report(generated: &rh_gen::Generated) {
    let report = &generated.report;
    let world = &generated.world;
    println!("seed: {}", report.seed);
    println!(
        "villain: {} / {} / {} ({})",
        report.villain, report.origin, report.scheme, world.villain.title
    );
    println!("ambush chance: {}%", report.ambush_percent);
    println!("npcs:");
    for npc in &world.npcs {
        let host = if world.villain.host == Some(npc.id) {
            "  [HOST]"
        } else {
            ""
        };
        println!(
            "  {} the {} ({:?}){host}",
            npc.name, npc.archetype, npc.disposition
        );
    }
    println!("attempts:");
    for attempt in &report.attempts {
        println!("  #{}: {}", attempt.attempt, attempt.outcome);
    }
    println!("clue graph ({} planner nodes):", report.nodes.len());
    for node in &report.nodes {
        let pool = node.pool.as_deref().unwrap_or("free");
        let gates = match (node.revealed_by, node.requires) {
            (Some(gate), Some(access)) => format!(" [after #{gate}, via #{access}]"),
            (Some(gate), None) => format!(" [after #{gate}]"),
            (None, Some(access)) => format!(" [via #{access}]"),
            (None, None) => String::new(),
        };
        println!(
            "  #{} {} @{} ({pool} x{}, obscurity {}) -> {}{gates}",
            node.id, node.name, node.map, node.cost, node.obscurity, node.grants
        );
    }
    println!("certified routes:");
    for route in &world.certified_routes {
        println!(
            "  {} (ready t{}, viability {}\u{2030}, effort {}, obscurity {}, {} legs{})",
            route.label,
            route.ready_by_turn,
            route.viability_permille,
            route.total_effort,
            route.total_obscurity,
            route.travel_legs,
            if route.uses_mystic_favour {
                ", uses favour"
            } else {
                ""
            }
        );
        for step in &route.steps {
            println!("    t{}: {}", step.turn, step.description);
        }
    }
}
