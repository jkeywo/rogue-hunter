//! Semantic commands and their rejection reasons.
//!
//! Keyboard, mouse, replay, terminal, and browser inputs all become exactly
//! these commands before the simulation changes state. Commands are the unit
//! of the replay log, so their encoding must stay stable within a replay
//! format version.

use rh_content::PoolKind;
use serde::{Deserialize, Serialize};

use crate::geometry::Direction;
use crate::state::ActorId;
use crate::world::{FeatureId, NpcId, OpportunityId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Step one tile; bumping a hostile actor melees it instead.
    Move(Direction),
    Wait,
    Melee(Target),
    /// Fire the flintlock. `silver` loads a silver bullet instead of shot.
    Ranged {
        target: Target,
        silver: bool,
    },
    /// Generic stamina manoeuvre by content id ("aim", "power-attack", ...).
    /// Dash-style manoeuvres carry the tiles to move, in order.
    Manoeuvre {
        id: String,
        steps: Vec<Direction>,
    },
    /// Hunter signature by content id. Set Snare targets a direction;
    /// Killing Blow targets an adjacent actor.
    Signature {
        id: String,
        dir: Option<Direction>,
        target: Option<Target>,
    },
    /// Drink a wound draught (consumes the action).
    UseDraught,
    /// Press a binding charm on an adjacent villain.
    UseBindingCharm {
        target: Target,
    },
    /// Resolve a discovered opportunity (investigate, gather, social action).
    Interact(OpportunityId),
    /// Free conversation with an adjacent NPC.
    Talk(NpcId),
    /// Buy one flintlock shot from an adjacent trading NPC.
    BuyShot(NpcId),
    /// Use the paired exit under the hunter's feet. Advances the global clock.
    Travel,
    /// Craft a recipe at an adjacent workstation.
    Craft {
        recipe: String,
    },
    /// Perform the consecration rite at the altar. Advances the global clock.
    Consecrate,
    /// Force open a grave (1 Physical): the informed play or the gamble.
    OpenGrave(FeatureId),
    /// Force adjacent barred terrain (1 Physical): doors and rubble.
    Force(Direction),
    /// Combine two corroborating identity clues. Free action.
    UncoverVillain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    /// Hostile actor by stable id.
    Actor(ActorId),
    /// A villager (attacking one has consequences).
    Npc(NpcId),
}

/// Why a command was rejected. Rendered by the UI so blocked actions are
/// explained rather than hidden, per the visible-affordances contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rejection {
    RunOver,
    Blocked { what: String },
    NoSuchTarget,
    OutOfRange,
    NoLineOfSight,
    NotAdjacent,
    NoAmmo { item: String },
    PoolEmpty { pool: PoolKind, needed: u8 },
    StaminaShort { needed: u8 },
    NotDiscovered,
    AlreadyResolved,
    NpcUnavailable,
    NpcWillNotTalk,
    NotEnoughCoin { needed: u16 },
    NotAtExit,
    TravelBlockedByFinalHunt,
    NotAtWorkstation,
    MissingIngredients { recipe: String },
    NotAtAltar,
    AlreadyConsecrated,
    NotAtGrave,
    GraveAlreadyOpened,
    NothingToForce,
    NeedMoreIdentityClues { have: u8, need: u8 },
    AlreadyUncovered,
    UnknownAbility { id: String },
    BadAbilityArguments,
    NothingThere,
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejection::RunOver => write!(f, "The run is over."),
            Rejection::Blocked { what } => write!(f, "The way is blocked by {what}."),
            Rejection::NoSuchTarget => write!(f, "There is no such target."),
            Rejection::OutOfRange => write!(f, "The target is out of range."),
            Rejection::NoLineOfSight => write!(f, "You cannot see a clear line to the target."),
            Rejection::NotAdjacent => write!(f, "You need to be adjacent to do that."),
            Rejection::NoAmmo { item } => write!(f, "You have no {item} left."),
            Rejection::PoolEmpty { pool, needed } => {
                write!(f, "That needs {needed} {pool:?} point(s) you do not have right now. Travel restores your pools.")
            }
            Rejection::StaminaShort { needed } => {
                write!(f, "You need {needed} stamina; catch your breath a moment.")
            }
            Rejection::NotDiscovered => write!(f, "You do not know about that yet."),
            Rejection::AlreadyResolved => write!(f, "That is already done."),
            Rejection::NpcUnavailable => write!(f, "They are not here to be dealt with."),
            Rejection::NpcWillNotTalk => write!(f, "They want nothing to do with you."),
            Rejection::NotEnoughCoin { needed } => write!(f, "That costs {needed} coin."),
            Rejection::NotAtExit => write!(f, "Stand on a marked exit to travel."),
            Rejection::TravelBlockedByFinalHunt => {
                write!(f, "There is no time left to run. The hunt is here.")
            }
            Rejection::NotAtWorkstation => write!(f, "You need a workstation to craft."),
            Rejection::MissingIngredients { recipe } => {
                write!(f, "You lack the ingredients for {recipe}.")
            }
            Rejection::NotAtAltar => write!(f, "The rite must be performed at the altar."),
            Rejection::AlreadyConsecrated => write!(f, "This ground is already warded."),
            Rejection::NotAtGrave => write!(f, "Stand at a grave to open it."),
            Rejection::NothingToForce => {
                write!(f, "There is nothing there that force will move.")
            }
            Rejection::GraveAlreadyOpened => write!(f, "This grave already lies open."),
            Rejection::NeedMoreIdentityClues { have, need } => {
                write!(
                    f,
                    "You have {have} of the {need} corroborating proofs a hunt demands."
                )
            }
            Rejection::AlreadyUncovered => write!(f, "You already know your quarry."),
            Rejection::UnknownAbility { id } => write!(f, "You know no ability called '{id}'."),
            Rejection::BadAbilityArguments => write!(f, "That ability needs a different target."),
            Rejection::NothingThere => write!(f, "There is nothing there."),
        }
    }
}
