//! Driving a corpus of seeds and reporting what happened to each.
//!
//! The agreement between the certified estimate and the driven game is
//! measured, not argued (viability-model-calibration), so the instrument that
//! measures it lives here rather than inside a test: a test can only fail,
//! and a debt this size needs to be *read* before it can be paid.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::Range;

use rh_content::Catalogue;

use crate::autoplay::{self, AutoplayReport, LossStage, RunEnd};
use crate::RunSession;

/// One driven seed, and everything the run could say about itself.
#[derive(Debug, Clone)]
pub struct SeedRecord {
    /// The seed asked for.
    pub seed: u64,
    /// The seed actually used. Generation walks forward from the request
    /// until it finds a world it can certify, so these differ, and the
    /// difference is itself a signal about how hard this hunter is to serve.
    pub used_seed: u64,
    pub hunter: String,
    pub archetype: String,
    pub origin: String,
    pub scheme: String,
    pub report: AutoplayReport,
}

/// What a corpus says in aggregate.
#[derive(Debug, Clone)]
pub struct Summary {
    pub hunter: String,
    pub runs: u32,
    pub wins: u32,
    pub won_permille: u32,
    pub promised_permille: u32,
    /// How many runs ended at each stage.
    pub stages: BTreeMap<&'static str, u32>,
    /// Commands refused across the whole corpus, by tag. A hunter reaching
    /// for an ability she does not own shows up here and nowhere else.
    pub rejections: BTreeMap<&'static str, u32>,
    /// Mean shortfall of the rescored loadout against the promise, over the
    /// runs that reached the fight. Large means the bot never assembled what
    /// was certified; near zero means the estimate itself is wrong.
    pub mean_shortfall: i32,
}

impl Summary {
    /// How far the driven game falls short of what was promised.
    pub fn gap(&self) -> i32 {
        self.promised_permille as i32 - self.won_permille as i32
    }
}

/// Drive every seed in `seeds` for one hunter.
pub fn scan(catalogue: &Catalogue, hunter: &str, seeds: Range<u64>) -> Vec<SeedRecord> {
    seeds
        .map(|seed| {
            let (mut session, used_seed) =
                RunSession::new_from_viable_seed(seed, catalogue.clone(), hunter)
                    .unwrap_or_else(|error| panic!("{hunter} near seed {seed}: {error}"));
            let villain = session.sim.world.villain.clone();
            let report = autoplay::autoplay_reported(&mut session);
            SeedRecord {
                seed,
                used_seed,
                hunter: hunter.to_owned(),
                archetype: villain.archetype,
                origin: villain.origin,
                scheme: villain.scheme,
                report,
            }
        })
        .collect()
}

pub fn summarise(records: &[SeedRecord]) -> Summary {
    let runs = records.len().max(1) as u32;
    let wins = records
        .iter()
        .filter(|record| record.report.end == RunEnd::Victory)
        .count() as u32;
    let promised: u64 = records
        .iter()
        .map(|record| u64::from(record.report.certified_permille))
        .sum();
    let mut stages: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut rejections: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut shortfalls: Vec<i32> = Vec::new();
    for record in records {
        *stages.entry(stage_label(record.report.stage)).or_insert(0) += 1;
        for (tag, count) in &record.report.command_rejections {
            *rejections.entry(tag).or_insert(0) += count;
        }
        if let Some(shortfall) = record.report.shortfall() {
            shortfalls.push(shortfall);
        }
    }
    let mean_shortfall = if shortfalls.is_empty() {
        0
    } else {
        shortfalls.iter().sum::<i32>() / shortfalls.len() as i32
    };
    Summary {
        hunter: records
            .first()
            .map(|record| record.hunter.clone())
            .unwrap_or_default(),
        runs,
        wins,
        won_permille: wins * 1000 / runs,
        promised_permille: (promised / u64::from(runs)) as u32,
        stages,
        rejections,
        mean_shortfall,
    }
}

pub fn stage_label(stage: LossStage) -> &'static str {
    match stage {
        LossStage::Won => "won",
        LossStage::NeverNamed => "never-named",
        LossStage::ArrivedUnderprepared => "underprepared",
        LossStage::FoughtBadly => "fought-badly",
        LossStage::DiedBeforeArriving => "died-before-arriving",
        LossStage::Stalled => "stalled",
    }
}

/// The corpus as a table, one line per seed, followed by the summary. Printed
/// on failure and on demand; this is what the diagnosis is actually read from.
pub fn table(records: &[SeedRecord]) -> String {
    let mut out = String::new();
    // Written out rather than formatted: every cell is a literal, and lining
    // them up by hand keeps the header beside the row format below it.
    out.push_str(
        "seed  used  stage                 archetype/origin     scheme               \
          cert rescor  short steps  clk dth  carried / refused\n",
    );
    for record in records {
        let report = &record.report;
        let rescored = report
            .rescored_permille
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned());
        let shortfall = report
            .shortfall()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_owned());
        let mut carried: Vec<String> = Vec::new();
        if report.route_steps_abandoned > 0 {
            carried.push(format!("gaveup={}", report.route_steps_abandoned));
        }
        if let Some(kit) = report.loadout_at_final_hunt {
            // What she actually had in hand when the fight began. The whole
            // question is which of these the promise assumed and she lacked.
            carried.push(format!("phys={}", kit.physical));
            carried.push(format!("drght={}", kit.draughts));
            carried.push(format!("silvr={}", kit.silver_bullets));
            carried.push(format!("charm={}", kit.binding_charms));
            carried.push(format!("iron={}", kit.counter_blades));
            if kit.on_consecrated_ground {
                carried.push("consecrated".to_owned());
            }
        }
        carried.extend(
            report
                .command_rejections
                .iter()
                .map(|(tag, count)| format!("{tag}={count}")),
        );
        let refused = carried;
        let _ = writeln!(
            out,
            "{:>4} {:>5}  {:<10} {:<20} {:<20} {:>5} {:>6} {:>6} {:>2}/{:<2} {:>4} {:>3}  {}",
            record.seed,
            record.used_seed,
            stage_label(report.stage),
            format!("{}/{}", record.archetype, record.origin),
            record.scheme,
            report.certified_permille,
            rescored,
            shortfall,
            report.route_steps_done - report.route_steps_abandoned,
            report.route_steps_total,
            report.clock_at_end,
            report.deaths_before_final,
            refused.join(" ")
        );
    }
    let summary = summarise(records);
    let stages: Vec<String> = summary
        .stages
        .iter()
        .map(|(stage, count)| format!("{stage}={count}"))
        .collect();
    let refused: Vec<String> = summary
        .rejections
        .iter()
        .map(|(tag, count)| format!("{tag}={count}"))
        .collect();
    let _ = writeln!(
        out,
        "{}: won {} of {} ({}permille) against certified {}permille, gap {}",
        summary.hunter,
        summary.wins,
        summary.runs,
        summary.won_permille,
        summary.promised_permille,
        summary.gap()
    );
    let _ = writeln!(out, "  stages: {}", stages.join(" "));
    let _ = writeln!(out, "  refused: {}", refused.join(" "));
    let _ = writeln!(
        out,
        "  mean shortfall of what she carried against what was promised: {}",
        summary.mean_shortfall
    );
    out
}

/// The stage a won run reports. Named rather than matched on at call sites so
/// a test asserting "won means victory" cannot drift from the classifier.
pub fn stage_of_won() -> LossStage {
    LossStage::Won
}
