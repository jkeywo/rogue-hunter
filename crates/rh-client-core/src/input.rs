//! Client-agnostic input intents.
//!
//! Terminal keys and browser events are first translated into these intents;
//! the session turns them into validated semantic commands. Neither client
//! ever constructs a `Command` directly, so both stay identical in behaviour.

use rh_core::geometry::{Direction, Point};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Move / bump-attack in a direction (arrows, HJKL+YUBN).
    Move(Direction),
    Wait,
    /// Fire the flintlock: enters or confirms target selection.
    Fire,
    /// Fire a silver bullet: enters or confirms target selection.
    FireSilver,
    /// Aim manoeuvre.
    Aim,
    /// Power Attack manoeuvre.
    PowerAttack,
    /// Sprint: captures two direction presses.
    Sprint,
    /// Set Snare: captures one direction press.
    SetSnare,
    /// Killing Blow on an adjacent eligible target.
    KillingBlow,
    /// Drink a wound draught.
    Draught,
    /// Use a binding charm on an adjacent villain.
    Charm,
    /// Context interact: opportunities, travel, crafting, graves, NPCs.
    Interact,
    /// Open the grimoire.
    Grimoire,
    /// Open the relationship map.
    Relationships,
    /// Open the region (travel) map.
    RegionMap,
    /// Open the full event log.
    EventLog,
    /// Toggle look mode: detach an inspection cursor from the hunter.
    ToggleLook,
    /// Fire the action at this index in the current action list (clicks on
    /// the on-screen action panel).
    DoAction(usize),
    /// Menu navigation.
    Up,
    Down,
    Confirm,
    Cancel,
    /// Text entry for seed / share-code screens.
    Char(char),
    Backspace,
    Paste(String),
    /// Mouse hover over a map tile (map coordinates).
    Hover(Point),
    /// Mouse hover left the map.
    HoverClear,
    /// Mouse click on a map tile (map coordinates).
    Click(Point),
    /// Copy the current share code (client performs the clipboard part).
    CopyCode,
}
