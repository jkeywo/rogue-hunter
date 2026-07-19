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
    /// Walk a path to a tile, stopping when anything worth a decision
    /// happens. A click on distant ground, or Enter on the look cursor.
    TravelTo(Point),
    /// Put the look cursor on the next thing in sight.
    NextThreat,
    /// Open the case dossier: what the hunter knows, owes, and carries.
    Dossier,
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
    /// Pick the row at this index directly, as a mouse click on a menu does.
    /// On a menu whose rows do something, this also activates the row; on a
    /// reference list it just moves the selection.
    Select(usize),
    /// Move the highlight to this row without activating it: the mouse is
    /// merely over it. Keeps keyboard and mouse agreeing about what is
    /// selected, so confirming after hovering does what the highlight shows.
    HoverRow(usize),
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

/// A platform-neutral key press. Each client maps its raw events (crossterm
/// key codes, browser `event.key` strings) onto these; what a key *means* is
/// decided once, in [`intent_for_key`], so the clients cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    Tab,
    Home,
    End,
    PageUp,
    PageDown,
    /// Numpad 5 with NumLock off.
    Clear,
}

/// How the session is currently listening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Seed or share-code entry: characters go to the field.
    TextEntry,
    /// A modal or a non-run screen: arrows and jk are list navigation.
    ListNav,
    /// The run screen proper: arrows and the roguelike keys move the hunter.
    Tactical,
}

/// The run screen's character bindings: one table that the key translator
/// and the action panel both read, so what is pressed and what is printed
/// beside an action can never disagree. Movement characters (hjkl, yubn,
/// the numpad) live in [`intent_for_key`] because their meaning depends on
/// the input mode.
const CHAR_BINDINGS: &[(char, Intent)] = &[
    ('e', Intent::Interact),
    (';', Intent::ToggleLook),
    ('.', Intent::Wait),
    (' ', Intent::Wait),
    ('f', Intent::Fire),
    ('F', Intent::FireSilver),
    ('a', Intent::Aim),
    ('p', Intent::PowerAttack),
    ('s', Intent::Sprint),
    ('x', Intent::SetSnare),
    ('K', Intent::KillingBlow),
    ('q', Intent::Draught),
    ('c', Intent::Charm),
    ('d', Intent::Dossier),
    ('g', Intent::Grimoire),
    ('r', Intent::Relationships),
    ('v', Intent::RegionMap),
    ('L', Intent::EventLog),
];

/// Translate a key press into the intent it means under the given mode.
/// Pure and total over the binding tables; the session wraps it with the
/// mode it is actually in.
pub fn intent_for_key(mode: InputMode, key: Key) -> Option<Intent> {
    if mode == InputMode::TextEntry {
        return match key {
            Key::Char(c) => Some(Intent::Char(c)),
            Key::Backspace => Some(Intent::Backspace),
            Key::Enter => Some(Intent::Confirm),
            Key::Escape => Some(Intent::Cancel),
            _ => None,
        };
    }
    let in_menu = mode == InputMode::ListNav;
    match key {
        Key::Escape => Some(Intent::Cancel),
        Key::Enter => Some(Intent::Confirm),
        // Tab sweeps the look cursor over everything in sight.
        Key::Tab if !in_menu => Some(Intent::NextThreat),
        Key::Up if in_menu => Some(Intent::Up),
        Key::Down if in_menu => Some(Intent::Down),
        Key::Up => Some(Intent::Move(Direction::North)),
        Key::Down => Some(Intent::Move(Direction::South)),
        Key::Left => Some(Intent::Move(Direction::West)),
        Key::Right => Some(Intent::Move(Direction::East)),
        // Numpad diagonals (NumLock off sends these navigation keys).
        Key::Home if !in_menu => Some(Intent::Move(Direction::NorthWest)),
        Key::PageUp if !in_menu => Some(Intent::Move(Direction::NorthEast)),
        Key::End if !in_menu => Some(Intent::Move(Direction::SouthWest)),
        Key::PageDown if !in_menu => Some(Intent::Move(Direction::SouthEast)),
        Key::Clear if !in_menu => Some(Intent::Wait),
        Key::Char(c) => char_intent(c, in_menu),
        _ => None,
    }
}

fn char_intent(c: char, in_menu: bool) -> Option<Intent> {
    // Numpad digits (NumLock on) are roguelike movement in the run screen.
    if !in_menu {
        if let Some(intent) = numpad_move(c) {
            return Some(intent);
        }
    }
    match c {
        'j' if in_menu => return Some(Intent::Down),
        'k' if in_menu => return Some(Intent::Up),
        'j' => return Some(Intent::Move(Direction::South)),
        'k' => return Some(Intent::Move(Direction::North)),
        'h' => return Some(Intent::Move(Direction::West)),
        'l' => return Some(Intent::Move(Direction::East)),
        'y' => return Some(Intent::Move(Direction::NorthWest)),
        'u' => return Some(Intent::Move(Direction::NorthEast)),
        'b' => return Some(Intent::Move(Direction::SouthWest)),
        'n' => return Some(Intent::Move(Direction::SouthEast)),
        _ => {}
    }
    CHAR_BINDINGS
        .iter()
        .find(|(key, _)| *key == c)
        .map(|(_, intent)| intent.clone())
}

/// Roguelike numpad movement: 1-9 laid out like the keypad, 5 waits.
fn numpad_move(c: char) -> Option<Intent> {
    Some(match c {
        '1' => Intent::Move(Direction::SouthWest),
        '2' => Intent::Move(Direction::South),
        '3' => Intent::Move(Direction::SouthEast),
        '4' => Intent::Move(Direction::West),
        '5' => Intent::Wait,
        '6' => Intent::Move(Direction::East),
        '7' => Intent::Move(Direction::NorthWest),
        '8' => Intent::Move(Direction::North),
        '9' => Intent::Move(Direction::NorthEast),
        _ => return None,
    })
}

/// The key hint printed beside an action, read back off the same table
/// that translates the press.
pub(crate) fn key_label(intent: &Intent) -> Option<String> {
    if matches!(intent, Intent::NextThreat) {
        return Some("Tab".to_owned());
    }
    CHAR_BINDINGS
        .iter()
        .find(|(_, bound)| bound == intent)
        .map(|(key, _)| key.to_string())
}
