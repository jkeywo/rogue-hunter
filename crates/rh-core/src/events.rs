//! The persistent event log.
//!
//! Shared observable state: combat rolls, monster telegraphs, regeneration,
//! pounces, clue discoveries, and clock events, identical across terminal,
//! browser, and replay views. Text is rendered at emit time from authored
//! content so the log itself is what players (and tests) see.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEvent {
    /// Global travel turn when the event fired.
    pub global_turn: u8,
    /// Local encounter turn when the event fired.
    pub local_turn: u32,
    pub kind: EventKind,
    /// Rendered prose, excluded from serialisation.
    ///
    /// `RunState` is serialised for exactly one purpose -- the state digest
    /// that proves a replay matched -- and saves carry only the seed and the
    /// command log, so nothing needs this field back. Keeping it out means a
    /// copy edit or a translation cannot change a digest, which is what lets
    /// the string table sit outside the content fingerprint. What the run
    /// *did* is still fully covered: `kind`, the turn counters, and the rest
    /// of the state all still hash.
    #[serde(skip)]
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    /// Attacks, damage, deaths.
    Combat,
    /// Monster telegraphs: pounce warnings, vulnerability, regeneration.
    Telegraph,
    /// Clue discoveries and knowledge gains.
    Clue,
    /// Global-clock advances and scheme events.
    Clock,
    /// Conversation, relationships, favours, fallout.
    Social,
    /// Crafting, items, loot.
    Item,
    /// Travel, arrival, ambush.
    Travel,
    /// Run start/end and other system messages.
    System,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::digest;

    #[test]
    fn the_digest_ignores_rendered_log_text() {
        // Log prose is placeholder copy destined for the string table, and it
        // will be rewritten and translated. None of that is a change to what
        // the run did, so none of it may move the digest a replay is checked
        // against. Without `serde(skip)` on `text`, this fails.
        let event = |text: &str| LogEvent {
            global_turn: 3,
            local_turn: 11,
            kind: EventKind::Combat,
            text: text.to_owned(),
        };
        assert_eq!(
            digest(&event("You strike the wolf for 3.")),
            digest(&event("[You strike the {name} for {damage}.]")),
        );
        // The rest of the event still hashes, or the digest would prove nothing.
        let mut later = event("same");
        later.local_turn = 12;
        assert_ne!(digest(&event("same")), digest(&later));
    }
}
