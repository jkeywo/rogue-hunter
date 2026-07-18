//! Deterministic run harness: share codes, command logs, replay execution.
//!
//! A run is fully described by its base seed plus the semantic command log.
//! The pair round-trips through a compact share code (`RH1-...`) usable for
//! copy/paste between the terminal and browser clients, bug reports, and
//! shared runs. Active runs persist as exactly these share codes.

pub mod autoplay;
mod codec;

use rh_content::Catalogue;
use rh_core::command::{Command, Rejection};
use rh_core::sim::Sim;
use rh_core::state::Outcome;
use rh_gen::GenReport;
use serde::{Deserialize, Serialize};

/// Replay format version. Share codes embed it; mismatches are rejected
/// rather than misinterpreted, since any rules change invalidates old logs.
/// Version 2 adds the selected hunter: routes are certified per hunter, so a
/// seed alone no longer identifies a run.
pub const REPLAY_VERSION: u8 = 2;

/// How far `new_from_viable_seed` will walk forward before giving up. Well
/// above the observed rejection rate, so exhausting it means something is
/// wrong with the content, not with this seed.
const MAX_SEED_REJECTIONS: u64 = 32;

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("world generation failed: {0}")]
    Generation(#[from] rh_gen::GenError),
    #[error("share code is malformed: {0}")]
    Malformed(String),
    #[error("share code version {found} is not supported (expected {expected})")]
    VersionMismatch { found: u8, expected: u8 },
    #[error("share code was recorded against different game content")]
    ContentMismatch,
    #[error("replayed command {index} was rejected: {rejection}")]
    RejectedCommand { index: usize, rejection: Rejection },
}

/// The serialized form of a run: seed plus semantic command log, stamped
/// with the content fingerprint the run was recorded against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRecord {
    pub version: u8,
    pub content: u16,
    pub seed: u64,
    /// Which hunter the run was generated and certified for.
    pub hunter: String,
    pub commands: Vec<Command>,
}

/// A live run: the authoritative sim plus the command log that recreates it.
pub struct RunSession {
    pub sim: Sim,
    pub seed: u64,
    /// Id of the hunter this run was generated for.
    pub hunter: String,
    pub commands: Vec<Command>,
    /// Generation inspector data for the developer toolchain and case report.
    pub report: GenReport,
}

impl RunSession {
    /// Start a fresh run from a base seed, for whichever hunter the catalogue
    /// has selected.
    pub fn new(seed: u64, catalogue: Catalogue) -> Result<Self, ReplayError> {
        let hunter = catalogue.hunter_id.clone();
        let generated = rh_gen::generate(seed, &catalogue)?;
        let sim = Sim::new(catalogue, generated.world, generated.rng);
        Ok(Self {
            sim,
            seed,
            hunter,
            commands: Vec::new(),
            report: generated.report,
        })
    }

    /// Start a fresh run for a named hunter.
    pub fn new_with_hunter(
        seed: u64,
        catalogue: Catalogue,
        hunter: &str,
    ) -> Result<Self, ReplayError> {
        let catalogue = catalogue
            .with_hunter(hunter)
            .map_err(|error| ReplayError::Malformed(error.to_string()))?;
        Self::new(seed, catalogue)
    }

    /// Start a run at or after `seed`, skipping seeds this hunter cannot be
    /// given a fair case for.
    ///
    /// Certification can legitimately refuse a world: a case that cannot be
    /// solved two independent ways by *this* hunter is one she should never be
    /// handed. Rejecting forward is the same principle generation already
    /// applies when it retries candidate worlds, one level up. The seed that
    /// actually produced the run is the one recorded, so share codes stay
    /// exact.
    pub fn new_from_viable_seed(
        seed: u64,
        catalogue: Catalogue,
        hunter: &str,
    ) -> Result<(Self, u64), ReplayError> {
        let mut last = None;
        for offset in 0..MAX_SEED_REJECTIONS {
            let candidate = seed.wrapping_add(offset);
            match Self::new_with_hunter(candidate, catalogue.clone(), hunter) {
                Ok(session) => return Ok((session, candidate)),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| ReplayError::Malformed("no seed was tried".to_owned())))
    }

    /// Apply a command; successful commands are recorded in the log.
    pub fn apply(&mut self, command: Command) -> Result<(), Rejection> {
        self.sim.apply(&command)?;
        self.commands.push(command);
        Ok(())
    }

    /// Encode this run as a compact share code.
    pub fn share_code(&self) -> String {
        codec::encode(&ReplayRecord {
            version: REPLAY_VERSION,
            content: rh_content::content_fingerprint(),
            seed: self.seed,
            hunter: self.hunter.clone(),
            commands: self.commands.clone(),
        })
    }

    /// Recreate a run from a share code, replaying its full command log.
    pub fn from_share_code(code: &str, catalogue: Catalogue) -> Result<Self, ReplayError> {
        let record = codec::decode(code)?;
        Self::from_record(record, catalogue)
    }

    /// Recreate a run from a decoded record.
    pub fn from_record(record: ReplayRecord, catalogue: Catalogue) -> Result<Self, ReplayError> {
        if record.version != REPLAY_VERSION {
            return Err(ReplayError::VersionMismatch {
                found: record.version,
                expected: REPLAY_VERSION,
            });
        }
        if record.content != rh_content::content_fingerprint() {
            return Err(ReplayError::ContentMismatch);
        }
        // The hunter is an input to generation, so it must be restored before
        // the world is built, not after.
        let mut session = Self::new_with_hunter(record.seed, catalogue, &record.hunter)?;
        for (index, command) in record.commands.into_iter().enumerate() {
            session
                .apply(command)
                .map_err(|rejection| ReplayError::RejectedCommand { index, rejection })?;
        }
        Ok(session)
    }

    /// Deterministic digest of the full run state; identical across native
    /// and WASM builds for the same seed and command log.
    pub fn state_digest(&self) -> u64 {
        rh_core::hash::digest(&self.sim.state)
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.sim.state.outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rh_core::command::Target;
    use rh_core::geometry::Direction;

    fn catalogue() -> Catalogue {
        rh_content::load_embedded().expect("embedded content")
    }

    #[test]
    fn share_code_round_trips() {
        let mut session = RunSession::new(7, catalogue()).expect("run starts");
        // A few real commands so the log is non-trivial.
        for dir in [Direction::North, Direction::East, Direction::South] {
            let _ = session.apply(Command::Move(dir));
        }
        let _ = session.apply(Command::Wait);
        let code = session.share_code();
        assert!(code.starts_with("RH1-"), "share code prefix: {code}");

        let restored = RunSession::from_share_code(&code, catalogue()).expect("code decodes");
        assert_eq!(restored.seed, session.seed);
        assert_eq!(restored.commands, session.commands);
        assert_eq!(restored.state_digest(), session.state_digest());
    }

    #[test]
    fn corrupted_share_codes_are_rejected() {
        let session = RunSession::new(7, catalogue()).expect("run starts");
        let code = session.share_code();
        // Flip a character in the payload.
        let mut corrupted = code.clone();
        let index = code.len() - 3;
        let replacement = if corrupted.as_bytes()[index] == b'A' {
            'B'
        } else {
            'A'
        };
        corrupted.replace_range(index..index + 1, &replacement.to_string());
        assert!(RunSession::from_share_code(&corrupted, catalogue()).is_err());
        assert!(RunSession::from_share_code("not-a-code", catalogue()).is_err());
        assert!(RunSession::from_share_code("RH1-!!!!", catalogue()).is_err());
    }

    #[test]
    fn rejected_commands_never_enter_the_log() {
        let mut session = RunSession::new(7, catalogue()).expect("run starts");
        // Firing at a non-existent actor is rejected and must not be logged.
        let bogus = Command::Ranged {
            target: Target::Actor(rh_core::state::ActorId(999)),
            silver: false,
        };
        assert!(session.apply(bogus).is_err());
        assert!(session.commands.is_empty());
    }
}
