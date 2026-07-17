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
