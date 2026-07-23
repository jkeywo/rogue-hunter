//! Shared UI-agnostic client layer.
//!
//! One session state machine and viewmodel serve both the terminal client
//! and the WASM web client: each is a thin renderer over [`view::ViewModel`]
//! plus a translator from raw events to [`input::Intent`]. All simulation
//! mutations funnel through the semantic command boundary in `rh-core`.

pub mod input;
pub mod view;

use rh_content::Catalogue;
use rh_core::command::{Command, Target};
use rh_core::geometry::{Direction, Point};
use rh_core::state::ActorKind;
use rh_core::world::{FeatureKind, GraveContents, OpportunityAnchor};
use rh_replay::RunSession;

pub use input::{ControlScheme, InputMode, Intent, Key};
pub use view::{Cell, CellColor, ViewModel};

/// What a client should do with its persisted save right now. The policy
/// lives here so the two clients cannot drift; they supply only the
/// platform I/O (a save file, browser local storage).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveAction {
    /// Write this share code as the active-run save.
    Write(String),
    /// Remove the save: the run ended or was abandoned.
    Clear,
    /// Leave whatever is stored alone.
    Keep,
}

/// Which screen the client is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Splash {
        selected: usize,
    },
    SeedEntry {
        input: String,
        error: Option<String>,
    },
    /// Choosing who takes the case. The world is certified for the hunter, so
    /// this is asked before generation, not after.
    HunterSelect {
        selected: usize,
        /// The seed the player asked for, or `None` to take a fresh one.
        seed: Option<u64>,
    },
    CodeEntry {
        input: String,
        error: Option<String>,
    },
    Run,
    Grimoire {
        selected: usize,
    },
    /// How a hunt is solved, for a player who has never solved one.
    Guide {
        selected: usize,
    },
    Relationships {
        selected: usize,
    },
    RegionMap {
        selected: usize,
    },
    EventLog {
        selected: usize,
    },
    /// The case dossier: the synthesis of what is known, as against the
    /// record's chronology of what happened.
    Dossier {
        selected: usize,
    },
    CaseReport,
}

/// Modal input in progress (multi-key actions, menus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    /// Choosing a ranged target among visible hostiles.
    FireTarget { silver: bool, selected: usize },
    /// Sprint: waiting for a single direction (moves several tiles that way).
    SprintDirection,
    /// Snare: waiting for a direction.
    SnareDirection,
    /// A context menu of actions built by the Interact intent.
    Menu {
        title: String,
        items: Vec<MenuItem>,
        selected: usize,
    },
    /// A yes/no gate in front of a command whose cost is not visible at the
    /// moment of choosing it. Row 0 goes ahead; row 1 backs out.
    Confirm {
        prompt: String,
        detail: Option<String>,
        command: Command,
        selected: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    /// Present when the action is currently blocked; shown, never hidden.
    pub blocked: Option<String>,
    pub action: MenuAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Do(Command),
    Nothing,
}

/// One row of the on-screen action panel: a labelled, keyed, clickable
/// affordance. Disabled entries stay visible with a reason, never hidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEntry {
    /// Key hint shown to the player (e.g. "e", "f", ";").
    pub key: String,
    pub label: String,
    /// Whether the action can be taken right now.
    pub enabled: bool,
    /// Short note: a cost, or why it is unavailable.
    pub note: Option<String>,
    /// The intent a click on this row fires.
    pub intent: Intent,
}

pub struct ClientSession {
    pub catalogue: Catalogue,
    pub screen: Screen,
    pub modal: Option<Modal>,
    pub run: Option<RunSession>,
    /// Hovered map tile for inspection (mouse).
    pub hover: Option<Point>,
    /// Detached look cursor (keyboard look mode); `None` when not looking.
    pub look_cursor: Option<Point>,
    /// One-line status: last rejection reason or notable event.
    pub status: String,
    /// Where an interrupted walk was heading, so it can be picked up again
    /// rather than re-aimed. Cleared on arrival and when the path is lost.
    pub travel_target: Option<Point>,
    /// Whether that target is still being walked toward. Held apart from the
    /// target itself because the two answer different questions: this one
    /// says whether the client should tick another step, while the target
    /// outlives an interruption so the walk can be offered again.
    walk_active: bool,
    /// Steps taken on the walk in progress, against `MAX_WALK_STEPS`.
    steps_walked: usize,
    /// A first-time teaching line, shown once and then never again.
    pub hint: Option<String>,
    /// Hint ids already spent this session.
    hints_seen: std::collections::BTreeSet<&'static str>,
    /// Which control scheme the player drives with.
    pub controls: input::ControlScheme,
    /// Random-ish seed source for "New Run" (clients pass a clock value).
    seed_nonce: u64,
}

/// One thing the hunter can currently see, for the in-sight panel and for
/// cursor cycling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SightEntry {
    pub name: String,
    /// Health for a foe, role for a villager.
    pub detail: String,
    pub at: Point,
    pub distance: i16,
    pub hostile: bool,
}

/// What a walk compares between steps to decide whether to keep going.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WalkWatch {
    hp: u16,
    pos: Point,
    over: bool,
    discovered: usize,
    threats: Vec<rh_core::state::ActorId>,
}

impl Default for WalkWatch {
    fn default() -> Self {
        Self {
            hp: 0,
            pos: Point::new(0, 0),
            over: true,
            discovered: 0,
            threats: Vec::new(),
        }
    }
}

/// How far one walk command may carry the hunter before it gives up. A map
/// is 32x20, so anything reachable is well inside this; the cap exists so a
/// pathological path cannot spin.
const MAX_WALK_STEPS: usize = 64;

/// Rows on the splash: new run, seed, share code, control scheme.
const SPLASH_ROWS: usize = 4;

/// Move a list highlight with the one semantics every screen and modal
/// shares: Up and Down clamp to the list, and a hover moves the highlight
/// to the row it points at without choosing it. Returns the new highlight,
/// or `None` when the intent is not highlight movement — choosing and
/// closing stay with each list's own handler.
fn list_move(intent: &Intent, selected: usize, count: usize) -> Option<usize> {
    match intent {
        Intent::Up => Some(selected.saturating_sub(1)),
        Intent::Down => Some((selected + 1).min(count.saturating_sub(1))),
        Intent::HoverRow(index) if *index < count => Some(*index),
        _ => None,
    }
}

impl ClientSession {
    pub fn new(catalogue: Catalogue, seed_nonce: u64) -> Self {
        Self {
            catalogue,
            screen: Screen::Splash { selected: 0 },
            modal: None,
            run: None,
            hover: None,
            look_cursor: None,
            status: String::new(),
            travel_target: None,
            walk_active: false,
            steps_walked: 0,
            hint: None,
            controls: input::ControlScheme::default(),
            hints_seen: std::collections::BTreeSet::new(),
            seed_nonce,
        }
    }

    /// The current share code, if a run is active (for saves and copy/paste).
    pub fn share_code(&self) -> Option<String> {
        self.run.as_ref().map(|run| run.share_code())
    }

    /// How the session is currently listening: text entry, list
    /// navigation, or tactical movement.
    pub fn input_mode(&self) -> InputMode {
        if matches!(
            self.screen,
            Screen::SeedEntry { .. } | Screen::CodeEntry { .. }
        ) {
            InputMode::TextEntry
        } else if self.modal.is_some() || !matches!(self.screen, Screen::Run) {
            InputMode::ListNav
        } else {
            InputMode::Tactical
        }
    }

    /// Translate a key press into the intent it means right now. Both
    /// clients call this and neither owns a binding of its own, so they
    /// cannot disagree about what a key does.
    pub fn intent_for_key(&self, key: Key) -> Option<Intent> {
        input::intent_for_key(self.input_mode(), self.controls, key)
    }

    /// Whether the client sits idle on the splash menu — where the terminal
    /// lets Esc quit the program rather than the screen.
    pub fn on_splash(&self) -> bool {
        matches!(self.screen, Screen::Splash { .. }) && self.modal.is_none()
    }

    /// What to do with the persisted save after this state change: write
    /// the code while a run is live, clear it once the run ends or is
    /// abandoned, and otherwise leave it be.
    pub fn save_action(&self) -> SaveAction {
        match (&self.screen, self.share_code()) {
            (Screen::CaseReport, _) | (Screen::Splash { .. }, _) => SaveAction::Clear,
            (_, Some(code)) => SaveAction::Write(code),
            _ => SaveAction::Keep,
        }
    }

    /// Restore a persisted run (native file / browser localStorage).
    pub fn restore(&mut self, code: &str) -> bool {
        match RunSession::from_share_code(code, self.catalogue.clone()) {
            Ok(run) => {
                self.screen = if run.outcome().is_some() {
                    Screen::CaseReport
                } else {
                    Screen::Run
                };
                self.run = Some(run);
                true
            }
            Err(_) => false,
        }
    }

    /// Feed one intent through the state machine.
    pub fn handle(&mut self, intent: Intent) {
        self.status.clear();
        self.hint = None;
        match &self.screen {
            Screen::Splash { .. } => self.handle_splash(intent),
            Screen::SeedEntry { .. } => self.handle_seed_entry(intent),
            Screen::HunterSelect { .. } => self.handle_hunter_select(intent),
            Screen::CodeEntry { .. } => self.handle_code_entry(intent),
            Screen::Run => self.handle_run(intent),
            Screen::Grimoire { .. } => self.handle_list_screen(intent),
            Screen::Guide { .. } => self.handle_list_screen(intent),
            Screen::Relationships { .. } => self.handle_list_screen(intent),
            Screen::RegionMap { .. } => self.handle_list_screen(intent),
            Screen::EventLog { .. } => self.handle_list_screen(intent),
            Screen::Dossier { .. } => self.handle_list_screen(intent),
            Screen::CaseReport => self.handle_case_report(intent),
        }
    }

    // -- Screens ---------------------------------------------------------------

    fn handle_splash(&mut self, intent: Intent) {
        let Screen::Splash { selected } = &mut self.screen else {
            return;
        };
        if let Some(row) = list_move(&intent, *selected, SPLASH_ROWS) {
            *selected = row;
            return;
        }
        match intent {
            Intent::Select(index) if index < SPLASH_ROWS => {
                *selected = index;
                self.handle_splash(Intent::Confirm);
            }
            Intent::Confirm => match *selected {
                0 => {
                    self.screen = Screen::HunterSelect {
                        selected: 0,
                        seed: None,
                    }
                }
                1 => {
                    self.screen = Screen::SeedEntry {
                        input: String::new(),
                        error: None,
                    }
                }
                2 => {
                    self.screen = Screen::CodeEntry {
                        input: String::new(),
                        error: None,
                    }
                }
                // The scheme row cycles in place rather than opening a
                // screen: there are two schemes, so a submenu would be a
                // whole screen spent saying one word.
                _ => self.controls = self.controls.next(),
            },
            Intent::Click(_) => {}
            _ => {}
        }
    }

    fn handle_seed_entry(&mut self, intent: Intent) {
        let Screen::SeedEntry { input, error } = &mut self.screen else {
            return;
        };
        match intent {
            Intent::Char(c) if c.is_ascii_digit() && input.len() < 20 => input.push(c),
            Intent::Backspace => {
                input.pop();
            }
            Intent::Paste(text) => {
                input.extend(text.chars().filter(|c| c.is_ascii_digit()).take(20));
            }
            Intent::Cancel => self.screen = Screen::Splash { selected: 1 },
            Intent::Confirm => match input.parse::<u64>() {
                Ok(seed) => {
                    self.screen = Screen::HunterSelect {
                        selected: 0,
                        seed: Some(seed),
                    }
                }
                Err(_) => {
                    *error = Some(
                        self.catalogue
                            .strings
                            .ui("ui.error.enter-number")
                            .to_owned(),
                    )
                }
            },
            _ => {}
        }
    }

    fn handle_hunter_select(&mut self, intent: Intent) {
        let count = self.catalogue.hunters.len();
        let Screen::HunterSelect { selected, seed } = &mut self.screen else {
            return;
        };
        if let Some(row) = list_move(&intent, *selected, count) {
            *selected = row;
            return;
        }
        match intent {
            Intent::Select(index) if index < count => {
                *selected = index;
                self.handle_hunter_select(Intent::Confirm);
            }
            Intent::Cancel => self.screen = Screen::Splash { selected: 0 },
            Intent::Confirm => {
                let chosen = *selected;
                let seed = *seed;
                let Some(hunter) = self.catalogue.hunters.keys().nth(chosen).cloned() else {
                    return;
                };
                let seed = seed.unwrap_or_else(|| self.next_seed());
                self.start_run(seed, &hunter);
            }
            _ => {}
        }
    }

    fn handle_code_entry(&mut self, intent: Intent) {
        let Screen::CodeEntry { input, error } = &mut self.screen else {
            return;
        };
        match intent {
            Intent::Char(c) if !c.is_control() => input.push(c),
            Intent::Backspace => {
                input.pop();
            }
            Intent::Paste(text) => input.push_str(text.trim()),
            Intent::Cancel => self.screen = Screen::Splash { selected: 2 },
            Intent::Confirm => {
                match RunSession::from_share_code(input.trim(), self.catalogue.clone()) {
                    Ok(run) => {
                        self.screen = if run.outcome().is_some() {
                            Screen::CaseReport
                        } else {
                            Screen::Run
                        };
                        self.run = Some(run);
                    }
                    Err(err) => *error = Some(err.to_string()),
                }
            }
            _ => {}
        }
    }

    /// Shared arrow-navigable list screens: grimoire, faces, the valley, and
    /// the record. Up/Down move the highlighted entry; the screen's own key,
    /// Esc, or Enter close it.
    fn handle_list_screen(&mut self, intent: Intent) {
        let count = self.list_len();
        let toggle = match self.screen {
            Screen::Grimoire { .. } => Some(Intent::Grimoire),
            Screen::Guide { .. } => Some(Intent::Guide),
            Screen::Relationships { .. } => Some(Intent::Relationships),
            Screen::RegionMap { .. } => Some(Intent::RegionMap),
            Screen::EventLog { .. } => Some(Intent::EventLog),
            Screen::Dossier { .. } => Some(Intent::Dossier),
            _ => None,
        };
        match &mut self.screen {
            Screen::Grimoire { selected }
            | Screen::Guide { selected }
            | Screen::Relationships { selected }
            | Screen::RegionMap { selected }
            | Screen::EventLog { selected }
            | Screen::Dossier { selected } => {
                if let Some(row) = list_move(&intent, *selected, count) {
                    *selected = row;
                    return;
                }
                // Reference lists have nothing to activate, so a click just
                // moves the reading position like a hover does.
                if let Intent::Select(index) = intent {
                    if index < count {
                        *selected = index;
                    }
                    return;
                }
            }
            _ => return,
        }
        if matches!(intent, Intent::Cancel | Intent::Confirm) || Some(&intent) == toggle.as_ref() {
            self.back_to_run();
        }
    }

    /// Number of entries in the current list screen, for clamping selection.
    fn list_len(&self) -> usize {
        match self.screen {
            Screen::Grimoire { .. } => self.catalogue.grimoire.len(),
            Screen::Guide { .. } => self.catalogue.guide.len(),
            Screen::Relationships { .. } => view::relationship_entries(self).len(),
            Screen::RegionMap { .. } => view::region_entries(self).len(),
            Screen::EventLog { .. } => view::record_entries(self).len(),
            Screen::Dossier { .. } => view::dossier_entries(self).len(),
            _ => 0,
        }
    }

    fn handle_case_report(&mut self, intent: Intent) {
        match intent {
            Intent::Confirm | Intent::Cancel => {
                self.run = None;
                self.screen = Screen::Splash { selected: 0 };
            }
            _ => {}
        }
    }

    fn back_to_run(&mut self) {
        self.screen = if self.run.is_some() {
            Screen::Run
        } else {
            Screen::Splash { selected: 0 }
        };
    }

    fn next_seed(&mut self) -> u64 {
        // Splash "New Run": derive successive seeds from the client nonce.
        self.seed_nonce = self
            .seed_nonce
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        self.seed_nonce % 1_000_000
    }

    fn start_run(&mut self, seed: u64, hunter: &str) {
        match RunSession::new_from_viable_seed(seed, self.catalogue.clone(), hunter) {
            Ok((run, used)) => {
                let name = run
                    .sim
                    .catalogue
                    .strings
                    .get(&run.sim.catalogue.hunter.name)
                    .to_owned();
                self.run = Some(run);
                self.modal = None;
                self.look_cursor = None;
                self.hover = None;
                self.screen = Screen::Run;
                // Say so when the requested seed had no fair case for this
                // hunter: the player asked for a specific world and did not
                // get it, which they should hear from us rather than notice.
                self.status = if used == seed {
                    self.catalogue.strings.ui_fill(
                        "ui.status.run-started",
                        &[("hunter", &name), ("seed", &seed.to_string())],
                    )
                } else {
                    self.catalogue.strings.ui_fill(
                        "ui.status.run-started-substitute",
                        &[
                            ("hunter", &name),
                            ("seed", &seed.to_string()),
                            ("used", &used.to_string()),
                        ],
                    )
                };
            }
            Err(error) => {
                self.screen = Screen::SeedEntry {
                    input: seed.to_string(),
                    error: Some(self.catalogue.strings.ui_fill(
                        "ui.error.generation-failed",
                        &[("error", &error.to_string())],
                    )),
                };
            }
        }
    }

    // -- The run screen ---------------------------------------------------------

    fn handle_run(&mut self, intent: Intent) {
        if self.modal.is_some() {
            self.handle_modal(intent);
            return;
        }
        // Look mode intercepts movement to drive the detached cursor.
        if self.look_cursor.is_some() {
            match &intent {
                Intent::Move(dir) => {
                    self.move_cursor(*dir);
                    return;
                }
                Intent::ToggleLook | Intent::Cancel => {
                    self.look_cursor = None;
                    self.status = self.catalogue.strings.ui("ui.status.look-off").to_owned();
                    return;
                }
                // Enter on the cursor walks there: the keyboard gets the
                // same click-to-destination the mouse already has.
                Intent::Confirm => {
                    let target = self.look_cursor;
                    self.look_cursor = None;
                    if let Some(target) = target {
                        self.walk_to(target);
                    }
                    return;
                }
                Intent::NextThreat => {
                    self.next_threat();
                    return;
                }
                Intent::Hover(point) => {
                    self.hover = Some(*point);
                    return;
                }
                Intent::HoverClear => {
                    self.hover = None;
                    return;
                }
                // Any other action leaves look mode, then takes effect.
                _ => self.look_cursor = None,
            }
        }
        // A click on the on-screen action panel dispatches that row's intent.
        if let Intent::DoAction(index) = intent {
            if let Some(entry) = self.available_actions().into_iter().nth(index) {
                self.handle(entry.intent);
            }
            return;
        }
        match intent {
            Intent::ToggleLook => self.enter_look_mode(),
            Intent::Move(dir) => self.apply(Command::Move(dir)),
            Intent::Wait => self.apply(Command::Wait),
            Intent::Aim => self.apply(Command::Manoeuvre {
                id: "aim".into(),
                steps: vec![],
            }),
            Intent::PowerAttack => self.apply(Command::Manoeuvre {
                id: "power-attack".into(),
                steps: vec![],
            }),
            Intent::Sprint => self.modal = Some(Modal::SprintDirection),
            Intent::SetSnare => self.modal = Some(Modal::SnareDirection),
            Intent::KillingBlow => match self.adjacent_hostile() {
                Some(target) => self.apply(Command::Signature {
                    id: "killing-blow".into(),
                    dir: None,
                    target: Some(target),
                }),
                None => {
                    self.status = self
                        .catalogue
                        .strings
                        .ui("ui.status.no-killing-blow-target")
                        .to_owned()
                }
            },
            Intent::Draught => self.apply(Command::UseDraught),
            Intent::Charm => match self.adjacent_hostile() {
                Some(target) => self.apply(Command::UseBindingCharm { target }),
                None => {
                    self.status = self
                        .catalogue
                        .strings
                        .ui("ui.status.no-charm-target")
                        .to_owned()
                }
            },
            Intent::Fire => self.open_fire_menu(false),
            Intent::FireSilver => self.open_fire_menu(true),
            Intent::Interact => self.open_interact_menu(),
            Intent::TravelTo(target) => self.walk_to(target),
            Intent::NextThreat => self.next_threat(),
            Intent::Dossier => self.screen = Screen::Dossier { selected: 0 },
            Intent::Grimoire => self.screen = Screen::Grimoire { selected: 0 },
            Intent::Guide => self.screen = Screen::Guide { selected: 0 },
            Intent::Relationships => self.screen = Screen::Relationships { selected: 0 },
            Intent::RegionMap => self.screen = Screen::RegionMap { selected: 0 },
            Intent::EventLog => {
                // Open the record at the most recent day.
                let last = view::record_entries(self).len().saturating_sub(1);
                self.screen = Screen::EventLog { selected: last };
            }
            Intent::Hover(point) => self.hover = Some(point),
            Intent::HoverClear => self.hover = None,
            Intent::Click(point) => self.handle_click(point),
            Intent::Cancel => {}
            _ => {}
        }
        self.check_run_over();
    }

    fn check_run_over(&mut self) {
        if let Some(run) = &self.run {
            if run.outcome().is_some() && self.screen == Screen::Run {
                self.screen = Screen::CaseReport;
            }
        }
    }

    fn handle_modal(&mut self, intent: Intent) {
        let Some(modal) = self.modal.clone() else {
            return;
        };
        // Highlight movement is one semantics for every list-shaped modal;
        // the arms below only choose and close.
        let moved = match &modal {
            Modal::FireTarget { silver, selected } => {
                list_move(&intent, *selected, self.fire_targets().len()).map(|row| {
                    Modal::FireTarget {
                        silver: *silver,
                        selected: row,
                    }
                })
            }
            Modal::Menu {
                title,
                items,
                selected,
            } => list_move(&intent, *selected, items.len()).map(|row| Modal::Menu {
                title: title.clone(),
                items: items.clone(),
                selected: row,
            }),
            Modal::Confirm {
                prompt,
                detail,
                command,
                selected,
            } => list_move(&intent, *selected, 2).map(|row| Modal::Confirm {
                prompt: prompt.clone(),
                detail: detail.clone(),
                command: command.clone(),
                selected: row,
            }),
            _ => None,
        };
        if let Some(next) = moved {
            self.modal = Some(next);
            self.check_run_over();
            return;
        }
        match (modal, intent) {
            (Modal::SprintDirection, Intent::Move(dir)) => {
                self.modal = None;
                // Sprint moves several tiles, all in the one chosen direction.
                let tiles = self.sprint_tiles();
                self.apply(Command::Manoeuvre {
                    id: "sprint".into(),
                    steps: vec![dir; usize::from(tiles)],
                });
            }
            (Modal::SnareDirection, Intent::Move(dir)) => {
                self.modal = None;
                self.apply(Command::Signature {
                    id: "set-snare".into(),
                    dir: Some(dir),
                    target: None,
                });
            }
            (Modal::FireTarget { silver, selected }, intent) => {
                let targets = self.fire_targets();
                match intent {
                    Intent::Confirm | Intent::Fire | Intent::FireSilver => {
                        self.modal = None;
                        if let Some((target, _)) = targets.get(selected) {
                            self.apply(Command::Ranged {
                                target: *target,
                                silver,
                            });
                        }
                    }
                    Intent::Click(point) => {
                        self.modal = None;
                        if let Some((target, _)) = targets.iter().find(|(_, at)| *at == point) {
                            self.apply(Command::Ranged {
                                target: *target,
                                silver,
                            });
                        } else {
                            self.status = self
                                .catalogue
                                .strings
                                .ui("ui.status.no-target-there")
                                .to_owned();
                        }
                    }
                    Intent::Cancel => self.modal = None,
                    _ => {}
                }
            }
            (
                Modal::Menu {
                    title,
                    items,
                    selected,
                },
                intent,
            ) => match intent {
                Intent::Select(index) if index < items.len() => {
                    self.modal = Some(Modal::Menu {
                        title,
                        items,
                        selected: index,
                    });
                    self.handle(Intent::Confirm);
                }
                Intent::Confirm | Intent::Interact => {
                    let choice = items.get(selected).cloned();
                    self.modal = None;
                    if let Some(item) = choice {
                        match (item.blocked, item.action) {
                            (Some(reason), _) => self.status = reason,
                            (None, MenuAction::Do(command)) => self.apply(command),
                            (None, MenuAction::Nothing) => {}
                        }
                    }
                }
                Intent::Cancel => self.modal = None,
                _ => {}
            },
            (
                Modal::Confirm {
                    command, selected, ..
                },
                intent,
            ) => {
                match intent {
                    Intent::Select(index) if index <= 1 => {
                        self.modal = None;
                        if index == 0 {
                            self.apply_now(command);
                        }
                    }
                    Intent::Confirm => {
                        self.modal = None;
                        if selected == 0 {
                            self.apply_now(command);
                        }
                    }
                    Intent::Cancel => self.modal = None,
                    // Anything else leaves the confirmation standing.
                    _ => {}
                }
            }
            (kept, Intent::Cancel) => {
                let _ = kept;
                self.modal = None;
            }
            (kept, _) => self.modal = Some(kept),
        }
        self.check_run_over();
    }

    /// Apply a semantic command, first putting a gate in front of it if its
    /// cost is one the player cannot see at the moment of choosing.
    fn apply(&mut self, command: Command) {
        if matches!(command, Command::Travel) {
            self.open_travel_confirm(command);
            return;
        }
        self.apply_now(command);
    }

    /// Apply a semantic command; rejections become the status line.
    fn apply_now(&mut self, command: Command) {
        let Some(run) = &mut self.run else { return };
        if let Err(rejection) = run.apply(command) {
            self.status = rejection.to_string();
        }
        self.check_hints();
    }

    /// Travel spends one of six days and can wake the quarry's next move, and
    /// neither of those is legible from the exit tile the hunter is standing
    /// on. So it is asked for rather than taken.
    fn open_travel_confirm(&mut self, command: Command) {
        let Some(run) = self.run.as_ref() else { return };
        let sim = &run.sim;
        let state = &sim.state;
        let strings = &sim.catalogue.strings;
        let clock = &sim.catalogue.balance.clock;
        let next_day = state.clock.saturating_add(1);
        let destination = sim
            .world
            .map(state.current_map)
            .exits
            .iter()
            .find(|exit| exit.at == state.hunter.pos)
            .map(|exit| sim.world.map(exit.to_map).name.clone())
            .unwrap_or_default();
        let prompt = strings.ui_fill(
            "ui.confirm.travel",
            &[
                ("place", &destination),
                ("day", &next_day.min(clock.travel_turns).to_string()),
                ("total", &clock.travel_turns.to_string()),
            ],
        );
        // Say so when the day being spent is one the villain's scheme has
        // already claimed: that is the whole reason the choice matters.
        let detail = if next_day >= clock.travel_turns {
            Some(strings.ui("ui.confirm.travel-final").to_owned())
        } else if next_day == clock.minor_event_turn || next_day == clock.major_event_turn {
            Some(strings.ui("ui.confirm.travel-event").to_owned())
        } else {
            None
        };
        self.modal = Some(Modal::Confirm {
            prompt,
            detail,
            command,
            selected: 0,
        });
    }

    /// Stamina cost of a manoeuvre by id, from the authored hunter profile.
    fn manoeuvre_cost(&self, id: &str) -> u8 {
        self.catalogue
            .hunter
            .manoeuvres
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.stamina_cost)
            .unwrap_or(0)
    }

    /// Tiles the Sprint manoeuvre moves, from the authored hunter profile.
    fn sprint_tiles(&self) -> u8 {
        self.catalogue
            .hunter
            .manoeuvres
            .iter()
            .find(|m| m.id == "sprint")
            .and_then(|m| match m.effect {
                rh_content::ManoeuvreEffect::Dash { tiles } => Some(tiles),
                _ => None,
            })
            .unwrap_or(2)
    }

    // -- Walking, sighting, and teaching ---------------------------------------

    /// Walk toward a tile, one recorded `Move` at a time, stopping the moment
    /// something happens that deserves a decision.
    ///
    /// Every step is an ordinary command, so a walk is indistinguishable from
    /// the same keys pressed by hand and the replay stays exact. What the
    /// feature actually removes is the pressing, not the turns.
    fn walk_to(&mut self, target: Point) {
        let Some(run) = self.run.as_ref() else { return };
        if !target.in_bounds() || run.sim.state.hunter.pos == target {
            self.travel_target = None;
            self.walk_active = false;
            return;
        }
        self.travel_target = Some(target);
        self.walk_active = true;
        self.steps_walked = 0;
        // Take the first step now so the click feels answered, and leave the
        // rest to `step_walk`. The client paces those, because a hunter
        // crossing a square should be watchable — a walk that resolves inside
        // one frame reads as a teleport and hides whatever interrupted it.
        self.step_walk();
    }

    /// Whether a click-to-walk is still under way.
    pub fn walking(&self) -> bool {
        self.walk_active
    }

    /// Take one step of a walk in progress. Returns whether more remain, so
    /// the client knows to schedule another tick.
    pub fn step_walk(&mut self) -> bool {
        let Some(target) = self.travel_target.filter(|_| self.walk_active) else {
            return false;
        };
        self.steps_walked += 1;
        if self.steps_walked > MAX_WALK_STEPS {
            self.walk_active = false;
            self.status = self.catalogue.strings.ui("ui.status.walk-far").to_owned();
            return false;
        }
        let strings = self.catalogue.strings.clone();
        let Some(direction) = self.path_step(target) else {
            // No path left: there is nothing worth picking up again either.
            self.travel_target = None;
            self.walk_active = false;
            self.status = strings.ui("ui.status.walk-blocked").to_owned();
            return false;
        };
        let before = self.walk_watch();
        self.apply_now(Command::Move(direction));
        let after = self.walk_watch();

        // Every reason a walk stops short. The hunter keeps what she learned
        // and stands where she got to; only the walking is cancelled.
        let halt = |session: &mut Self, message: &str| {
            session.walk_active = false;
            session.status = strings.ui(message).to_owned();
        };
        if after.over || after.pos == before.pos {
            // Run ended, or the move was refused and `apply_now` said why.
            self.walk_active = false;
            return false;
        }
        if after.pos == target {
            self.travel_target = None;
            halt(self, "ui.status.walk-arrived");
            return false;
        }
        if after.hp < before.hp {
            halt(self, "ui.status.walk-hurt");
            return false;
        }
        if after.threats.iter().any(|id| !before.threats.contains(id)) {
            halt(self, "ui.status.walk-sighted");
            return false;
        }
        if after.discovered > before.discovered {
            halt(self, "ui.status.walk-lead");
            return false;
        }
        true
    }

    /// The handful of facts a walk watches between steps.
    fn walk_watch(&self) -> WalkWatch {
        let Some(run) = self.run.as_ref() else {
            return WalkWatch::default();
        };
        let state = &run.sim.state;
        WalkWatch {
            hp: state.hunter.hp,
            pos: state.hunter.pos,
            over: state.outcome.is_some(),
            discovered: state.discovered.len(),
            threats: state
                .actors
                .iter()
                .filter(|actor| {
                    actor.map == state.current_map && actor.hp > 0 && state.is_visible(actor.pos)
                })
                .map(|actor| actor.id)
                .collect(),
        }
    }

    /// Everything in sight worth pointing at, nearest first: hostiles before
    /// villagers, so the list reads as a threat list that also holds people.
    pub fn in_sight(&self) -> Vec<SightEntry> {
        let Some(run) = self.run.as_ref() else {
            return Vec::new();
        };
        let sim = &run.sim;
        let state = &sim.state;
        let map = state.current_map;
        let hunter = state.hunter.pos;
        let strings = &sim.catalogue.strings;
        let mut entries: Vec<SightEntry> = Vec::new();
        for actor in &state.actors {
            if actor.map != map || actor.hp == 0 || !state.is_visible(actor.pos) {
                continue;
            }
            let mut detail = strings.ui_fill(
                "ui.sight.health",
                &[
                    ("current", &actor.hp.to_string()),
                    ("max", &actor.max_hp.to_string()),
                ],
            );
            if actor.trapped > 0 {
                detail.push_str(strings.ui("ui.sight.held"));
            } else if actor.kind == ActorKind::Villain && sim.villain_is_vulnerable(actor.id) {
                detail.push_str(strings.ui("ui.sight.vulnerable"));
            }
            entries.push(SightEntry {
                name: sim.actor_name(&actor.kind),
                detail,
                at: actor.pos,
                distance: hunter.distance(actor.pos),
                hostile: true,
            });
        }
        for (spec, npc_state) in sim.world.npcs.iter().zip(state.npcs.iter()) {
            if spec.map != map || !npc_state.alive || npc_state.fled {
                continue;
            }
            if !state.is_visible(npc_state.pos) {
                continue;
            }
            let role = sim
                .catalogue
                .npcs
                .archetypes
                .get(&spec.archetype)
                .map(|def| strings.get(&def.name))
                .unwrap_or_default();
            entries.push(SightEntry {
                name: spec.name.clone(),
                detail: role.to_owned(),
                at: npc_state.pos,
                distance: hunter.distance(npc_state.pos),
                hostile: false,
            });
        }
        entries.sort_by_key(|entry| (!entry.hostile, entry.distance, entry.at.y, entry.at.x));
        entries
    }

    /// Put the look cursor on the next thing in sight, wrapping round. The
    /// keyboard equivalent of sweeping the mouse over everything that moved.
    fn next_threat(&mut self) {
        let entries = self.in_sight();
        if entries.is_empty() {
            self.status = self
                .catalogue
                .strings
                .ui("ui.blocked.nothing-in-sight")
                .to_owned();
            return;
        }
        let current = self.look_point();
        let next = current
            .and_then(|at| entries.iter().position(|entry| entry.at == at))
            .map(|index| (index + 1) % entries.len())
            .unwrap_or(0);
        // The cursor is the answer, so a stale mouse hover must not outrank it.
        self.hover = None;
        self.look_cursor = Some(entries[next].at);
    }

    /// Say a thing once, the first time it is true and could bite.
    ///
    /// These are the moments a new player misreads: a cost they did not know
    /// they were paying, a clock they did not know was running. Each fires at
    /// most once, most urgent first, and only ever one per action so the line
    /// is never a wall.
    fn check_hints(&mut self) {
        let Some(run) = self.run.as_ref() else { return };
        let state = &run.sim.state;
        let hunter = &state.hunter;
        let has_lead = run.sim.world.opportunities.iter().any(|opp| {
            state.discovered.contains(&opp.id)
                && !state.resolved.contains(&opp.id)
                && !state.lost.contains(&opp.id)
        });
        // The moment the proofs first agree is the pivot of the whole case and
        // nothing on screen shouts it, so a first-timer can gather what they
        // need and not realise they may now name the thing.
        let can_name = state.corroboration(&run.sim.catalogue).met() && !state.villain_uncovered;
        // A lead she has found but cannot yet pay for teaches that pools gate
        // the work and that travel restores them — the first wall a new player
        // hits without knowing it is one.
        let unaffordable_lead = run.sim.world.opportunities.iter().any(|opp| {
            state.discovered.contains(&opp.id)
                && !state.resolved.contains(&opp.id)
                && !state.lost.contains(&opp.id)
                && matches!(
                    rh_core::economy::opportunity_cost(opp.pool, opp.cost, state.settlement_hostile),
                    Some((pool, cost)) if hunter.pool(pool) < cost
                )
        });
        let candidates: [(&'static str, bool); 8] = [
            ("ui.hint.first.final-hunt", state.final_hunt),
            (
                "ui.hint.first.wounded",
                hunter.hp * 2 <= hunter.max_hp && hunter.hp > 0,
            ),
            ("ui.hint.first.can-name", can_name),
            ("ui.hint.first.villain-tier", state.villain.tier > 0),
            ("ui.hint.first.day-passed", state.clock > 0),
            ("ui.hint.first.stamina-empty", hunter.stamina == 0),
            ("ui.hint.first.unaffordable", unaffordable_lead),
            ("ui.hint.first.lead", has_lead),
        ];
        let Some(id) = candidates
            .iter()
            .find(|(id, live)| *live && !self.hints_seen.contains(id))
            .map(|(id, _)| *id)
        else {
            return;
        };
        self.hints_seen.insert(id);
        self.hint = Some(self.catalogue.strings.ui(id).to_owned());
    }

    // -- Look mode -------------------------------------------------------------

    fn enter_look_mode(&mut self) {
        let start = self.run.as_ref().map(|run| run.sim.state.hunter.pos);
        if let Some(start) = start {
            self.look_cursor = Some(self.hover.unwrap_or(start));
            self.status = self.catalogue.strings.ui("ui.status.look-on").to_owned();
        }
    }

    fn move_cursor(&mut self, dir: Direction) {
        if let Some(cursor) = self.look_cursor {
            let next = cursor.step(dir);
            if next.in_bounds() {
                self.look_cursor = Some(next);
            }
        }
    }

    /// The tile currently being inspected: the mouse hover if any, else the
    /// keyboard look cursor. Both clients render a marker here.
    pub fn look_point(&self) -> Option<Point> {
        self.hover.or(self.look_cursor)
    }

    /// Whether keyboard look mode is engaged.
    pub fn is_looking(&self) -> bool {
        self.look_cursor.is_some()
    }

    // -- Action panel ----------------------------------------------------------

    /// The context-sensitive list of actions offered on the right of the
    /// screen. Every entry is keyed and clickable; disabled ones stay visible
    /// with a reason. Clicking row `i` dispatches `entry.intent`.
    pub fn available_actions(&self) -> Vec<ActionEntry> {
        let Some(run) = self.run.as_ref() else {
            return Vec::new();
        };
        let state = &run.sim.state;
        let hunter = &state.hunter;
        let strings = &run.sim.catalogue.strings;
        let mut actions: Vec<ActionEntry> = Vec::new();

        // The key hint is read off the binding table, so the panel can
        // never print a key the translator does not honour. `Some` is for
        // the one row that is not a key at all: the walk row's return
        // glyph, which stands for Enter on the look cursor.
        let mut push = |key: Option<&str>,
                        label: &str,
                        enabled: bool,
                        note: Option<String>,
                        intent: Intent| {
            // A greyed-out row must always say why it is greyed out. The colour
            // that dims it is the last channel a player without colour has, so
            // the reason cannot live only in the hue. Any disabled row reaching
            // here without a note is an authoring gap; the assert catches it in
            // tests, and the fallback keeps a real player from ever facing a
            // dead row with no explanation.
            debug_assert!(
                enabled || note.is_some(),
                "disabled action '{label}' carries no reason"
            );
            let note =
                note.or_else(|| (!enabled).then(|| strings.ui("ui.action.unavailable").to_owned()));
            actions.push(ActionEntry {
                key: key
                    .map(str::to_owned)
                    .or_else(|| input::key_label(self.controls, &intent))
                    .unwrap_or_default(),
                label: label.to_owned(),
                enabled,
                note,
                intent,
            });
        };

        push(
            None,
            strings.ui("ui.action.interact"),
            true,
            None,
            Intent::Interact,
        );
        push(
            None,
            strings.ui("ui.action.look"),
            true,
            None,
            Intent::ToggleLook,
        );
        push(None, strings.ui("ui.action.wait"), true, None, Intent::Wait);

        // Walking: to whatever is pointed at, or back onto an interrupted
        // walk. One row, because they are the same act to the player.
        let pointed = self.look_point().filter(|at| *at != hunter.pos);
        let resume = self.travel_target.filter(|at| *at != hunter.pos);
        match (pointed, resume) {
            (Some(at), _) => push(
                Some("\u{21b5}"),
                strings.ui("ui.action.walk-to"),
                true,
                None,
                Intent::TravelTo(at),
            ),
            (None, Some(at)) => push(
                Some("\u{21b5}"),
                strings.ui("ui.action.resume-walk"),
                true,
                None,
                Intent::TravelTo(at),
            ),
            (None, None) => push(
                Some("\u{21b5}"),
                strings.ui("ui.action.walk-to"),
                false,
                Some(strings.ui("ui.blocked.no-walk-target").to_owned()),
                Intent::Wait,
            ),
        }
        let sighted = !self.in_sight().is_empty();
        push(
            None,
            strings.ui("ui.action.next-in-sight"),
            sighted,
            (!sighted).then(|| strings.ui("ui.blocked.nothing-in-sight").to_owned()),
            Intent::NextThreat,
        );

        let targets = !self.fire_targets().is_empty();
        let shots = hunter.item_count("flintlock-shot");
        push(
            None,
            strings.ui("ui.action.fire-flintlock"),
            targets && shots > 0,
            if shots == 0 {
                Some(strings.ui("ui.blocked.out-of-shot").to_owned())
            } else if !targets {
                Some(strings.ui("ui.blocked.nothing-in-sight").to_owned())
            } else {
                None
            },
            Intent::Fire,
        );
        if hunter.item_count("silver-bullet") > 0 {
            push(
                None,
                strings.ui("ui.action.fire-silver"),
                targets,
                (!targets).then(|| strings.ui("ui.blocked.nothing-in-sight").to_owned()),
                Intent::FireSilver,
            );
        }

        let stamina = hunter.stamina;
        let aim_cost = self.manoeuvre_cost("aim");
        push(
            None,
            strings.ui("ui.action.aim"),
            stamina >= aim_cost,
            (stamina < aim_cost).then(|| format!("{aim_cost} stamina")),
            Intent::Aim,
        );
        let power_cost = self.manoeuvre_cost("power-attack");
        push(
            None,
            strings.ui("ui.action.power-attack"),
            stamina >= power_cost,
            (stamina < power_cost).then(|| {
                strings.ui_fill("ui.blocked.stamina", &[("cost", &power_cost.to_string())])
            }),
            Intent::PowerAttack,
        );
        let sprint_cost = self.manoeuvre_cost("sprint");
        push(
            None,
            &strings.ui_fill(
                "ui.action.sprint",
                &[("tiles", &self.sprint_tiles().to_string())],
            ),
            stamina >= sprint_cost,
            (stamina < sprint_cost).then(|| format!("{sprint_cost} stamina")),
            Intent::Sprint,
        );

        let physical = hunter.physical;
        push(
            None,
            strings.ui("ui.action.set-snare"),
            physical >= 1,
            (physical < 1).then(|| strings.ui("ui.blocked.one-physical").to_owned()),
            Intent::SetSnare,
        );
        let adjacent_foe = self.adjacent_hostile().is_some();
        push(
            None,
            strings.ui("ui.action.killing-blow"),
            physical >= 1 && adjacent_foe,
            if physical < 1 {
                Some("1 Physical".into())
            } else if !adjacent_foe {
                Some(strings.ui("ui.blocked.no-foe-adjacent").to_owned())
            } else {
                None
            },
            Intent::KillingBlow,
        );

        if hunter.item_count("wound-draught") > 0 {
            push(
                None,
                strings.ui("ui.action.draught"),
                true,
                None,
                Intent::Draught,
            );
        }
        if hunter.item_count("binding-charm") > 0 {
            let adjacent_villain = self.adjacent_villain();
            push(
                None,
                strings.ui("ui.action.binding-charm"),
                adjacent_villain,
                (!adjacent_villain)
                    .then(|| strings.ui("ui.blocked.no-revenant-adjacent").to_owned()),
                Intent::Charm,
            );
        }

        push(
            None,
            strings.ui("ui.action.dossier"),
            true,
            None,
            Intent::Dossier,
        );
        push(
            None,
            strings.ui("ui.action.grimoire"),
            true,
            None,
            Intent::Grimoire,
        );
        push(
            None,
            strings.ui("ui.action.faces"),
            true,
            None,
            Intent::Relationships,
        );
        push(
            None,
            strings.ui("ui.action.valley"),
            true,
            None,
            Intent::RegionMap,
        );
        push(
            None,
            strings.ui("ui.action.record"),
            true,
            None,
            Intent::EventLog,
        );

        actions
    }

    fn adjacent_villain(&self) -> bool {
        let Some(run) = self.run.as_ref() else {
            return false;
        };
        let state = &run.sim.state;
        let hunter = state.hunter.pos;
        let map = state.current_map;
        state.actors.iter().any(|actor| {
            actor.map == map
                && actor.hp > 0
                && actor.kind == ActorKind::Villain
                && hunter.is_adjacent(actor.pos)
        })
    }

    fn adjacent_hostile(&self) -> Option<Target> {
        let run = self.run.as_ref()?;
        let hunter = run.sim.state.hunter.pos;
        let map = run.sim.state.current_map;
        run.sim
            .state
            .actors
            .iter()
            .filter(|actor| actor.map == map && actor.hp > 0 && hunter.is_adjacent(actor.pos))
            .min_by_key(|actor| actor.id.0)
            .map(|actor| Target::Actor(actor.id))
    }

    /// Visible hostile actors in flintlock range, nearest first.
    pub fn fire_targets(&self) -> Vec<(Target, Point)> {
        let Some(run) = self.run.as_ref() else {
            return Vec::new();
        };
        let state = &run.sim.state;
        let hunter = state.hunter.pos;
        let map = state.current_map;
        let mut targets: Vec<(Target, Point, i16)> = state
            .actors
            .iter()
            .filter(|actor| actor.map == map && actor.hp > 0 && state.is_visible(actor.pos))
            .map(|actor| {
                (
                    Target::Actor(actor.id),
                    actor.pos,
                    hunter.distance(actor.pos),
                )
            })
            .collect();
        targets.sort_by_key(|(_, at, distance)| (*distance, at.y, at.x));
        targets
            .into_iter()
            .map(|(target, at, _)| (target, at))
            .collect()
    }

    fn open_fire_menu(&mut self, silver: bool) {
        let targets = self.fire_targets();
        if targets.is_empty() {
            self.status = self
                .catalogue
                .strings
                .ui("ui.status.nothing-to-shoot")
                .to_owned();
            return;
        }
        self.modal = Some(Modal::FireTarget {
            silver,
            selected: 0,
        });
    }

    /// Build the context menu for the hunter's position: opportunities,
    /// travel, crafting, graves, forceable terrain, and NPC dealings.
    fn open_interact_menu(&mut self) {
        let Some(run) = self.run.as_ref() else { return };
        let sim = &run.sim;
        let state = &sim.state;
        let hunter = state.hunter.pos;
        let map = state.current_map;
        let mut items: Vec<MenuItem> = Vec::new();

        // Travel from an exit tile.
        if let Some(exit) = sim
            .world
            .map(map)
            .exits
            .iter()
            .find(|exit| exit.at == hunter)
        {
            let name = sim.world.map(exit.to_map).name.clone();
            let strings = &sim.catalogue.strings;
            items.push(MenuItem {
                label: strings.ui_fill("ui.action.travel", &[("place", &name)]),
                blocked: state
                    .final_hunt
                    .then(|| strings.ui("ui.blocked.no-time").to_owned()),
                action: MenuAction::Do(Command::Travel),
            });
        }

        // Discovered opportunities anchored nearby.
        for opp in &sim.world.opportunities {
            if opp.map != map
                || !state.discovered.contains(&opp.id)
                || state.resolved.contains(&opp.id)
                || state.lost.contains(&opp.id)
            {
                continue;
            }
            let near = match opp.anchor {
                OpportunityAnchor::Tile(at) => hunter == at || hunter.is_adjacent(at),
                OpportunityAnchor::Npc(npc) => {
                    let npc_state = &state.npcs[npc.0 as usize];
                    npc_state.alive && !npc_state.fled && hunter.is_adjacent(npc_state.pos)
                }
            };
            if !near {
                continue;
            }
            let blocked =
                rh_core::economy::opportunity_cost(opp.pool, opp.cost, state.settlement_hostile)
                    .and_then(|(pool, cost)| {
                        (state.hunter.pool(pool) < cost).then(|| {
                            sim.catalogue.strings.ui_fill(
                                "ui.blocked.needs-pool",
                                &[
                                    ("cost", &cost.to_string()),
                                    ("pool", crate::view::pool_name(&sim.catalogue.strings, pool)),
                                ],
                            )
                        })
                    });
            let cost_note = match opp.pool {
                Some(pool) => sim.catalogue.strings.ui_fill(
                    "ui.action.cost-suffix",
                    &[
                        ("cost", &opp.cost.to_string()),
                        ("pool", crate::view::pool_name(&sim.catalogue.strings, pool)),
                    ],
                ),
                None => String::new(),
            };
            items.push(MenuItem {
                label: format!(
                    "{}{}",
                    opp.lead(&sim.catalogue.strings, rh_core::world::LeadFraming::Act),
                    cost_note
                ),
                blocked,
                action: MenuAction::Do(Command::Interact(opp.id)),
            });
        }

        // Graves can be opened with muscle.
        for feature in &sim.world.map(map).features {
            if let FeatureKind::Grave { .. } = feature.kind {
                if hunter == feature.at || hunter.is_adjacent(feature.at) {
                    let blocked = if state.opened_graves.contains(&feature.id) {
                        Some(
                            sim.catalogue
                                .strings
                                .ui("ui.blocked.already-opened")
                                .to_owned(),
                        )
                    } else if state.hunter.physical < 1 {
                        Some(
                            sim.catalogue
                                .strings
                                .ui("ui.blocked.needs-physical")
                                .to_owned(),
                        )
                    } else {
                        None
                    };
                    items.push(MenuItem {
                        label: sim
                            .catalogue
                            .strings
                            .ui_fill("ui.action.open-grave", &[("grave", &feature.name)]),
                        blocked,
                        action: MenuAction::Do(Command::OpenGrave(feature.id)),
                    });
                }
            }
        }

        // Forceable terrain.
        for dir in Direction::ALL {
            let at = hunter.step(dir);
            let terrain = state.terrain(&sim.world, map, at);
            let label = match terrain {
                rh_content::Terrain::BarredDoor => Some("Force the barred door [1 Physical]"),
                rh_content::Terrain::Rubble => Some("Shift the rubble [1 Physical]"),
                _ => None,
            };
            if let Some(label) = label {
                items.push(MenuItem {
                    label: label.to_owned(),
                    blocked: (state.hunter.physical < 1).then(|| {
                        sim.catalogue
                            .strings
                            .ui("ui.blocked.needs-physical")
                            .to_owned()
                    }),
                    action: MenuAction::Do(Command::Force(dir)),
                });
            }
        }

        // The altar: consecration.
        let at_altar = sim.world.map(map).features.iter().any(|feature| {
            feature.kind == FeatureKind::Altar
                && (hunter == feature.at || hunter.is_adjacent(feature.at))
        });
        if at_altar {
            let blocked = if state.church_consecrated {
                Some(
                    sim.catalogue
                        .strings
                        .ui("ui.blocked.already-warded")
                        .to_owned(),
                )
            } else if state.final_hunt {
                Some(sim.catalogue.strings.ui("ui.blocked.no-time").to_owned())
            } else {
                None
            };
            items.push(MenuItem {
                label: sim.catalogue.strings.ui("ui.action.consecrate").to_owned(),
                blocked,
                action: MenuAction::Do(Command::Consecrate),
            });
        }

        // The workstation: crafting.
        let at_forge = sim.world.map(map).features.iter().any(|feature| {
            feature.kind == FeatureKind::Workstation
                && (hunter == feature.at || hunter.is_adjacent(feature.at))
        });
        if at_forge {
            for (recipe_id, recipe) in &sim.catalogue.recipes {
                let mut needed = std::collections::BTreeMap::new();
                for input in &recipe.inputs {
                    *needed.entry(input.clone()).or_insert(0u16) += 1;
                }
                let missing: Vec<String> = needed
                    .iter()
                    .filter(|(item, count)| state.hunter.item_count(item) < **count)
                    .map(|(item, _)| {
                        sim.catalogue
                            .items
                            .get(item)
                            .map(|def| sim.catalogue.strings.get(&def.name).to_owned())
                            .unwrap_or_else(|| item.clone())
                    })
                    .collect();
                let blocked = (!missing.is_empty()).then(|| {
                    sim.catalogue.strings.ui_fill(
                        "ui.blocked.missing-items",
                        &[("items", &missing.join(", "))],
                    )
                });
                items.push(MenuItem {
                    label: sim.catalogue.strings.ui_fill(
                        "ui.action.craft",
                        &[("recipe", sim.catalogue.strings.get(&recipe.name))],
                    ),
                    blocked,
                    action: MenuAction::Do(Command::Craft {
                        recipe: recipe_id.clone(),
                    }),
                });
            }
        }

        // Adjacent villagers: talk, trade.
        for (spec, npc_state) in sim.world.npcs.iter().zip(state.npcs.iter()) {
            if spec.map != map || !npc_state.alive || npc_state.fled {
                continue;
            }
            if !hunter.is_adjacent(npc_state.pos) {
                continue;
            }
            items.push(MenuItem {
                label: sim
                    .catalogue
                    .strings
                    .ui_fill("ui.action.talk", &[("npc", &spec.name)]),
                blocked: npc_state.attacked.then(|| {
                    sim.catalogue
                        .strings
                        .ui("ui.blocked.npc-refuses")
                        .to_owned()
                }),
                action: MenuAction::Do(Command::Talk(spec.id)),
            });
            if spec.trades {
                let blocked = if state.settlement_hostile || npc_state.attacked {
                    Some(sim.catalogue.strings.ui("ui.blocked.no-trade").to_owned())
                } else if state.hunter.item_count("coin") < 2 {
                    Some(sim.catalogue.strings.ui("ui.blocked.costs-coin").to_owned())
                } else {
                    None
                };
                items.push(MenuItem {
                    label: sim
                        .catalogue
                        .strings
                        .ui_fill("ui.action.buy-ammunition", &[("npc", &spec.name)]),
                    blocked,
                    action: MenuAction::Do(Command::BuyShot(spec.id)),
                });
            }
        }

        // Naming the villain once the proofs agree.
        if state.corroboration(&sim.catalogue).corroborated() && !state.villain_uncovered {
            items.push(MenuItem {
                label: sim.catalogue.strings.ui("ui.action.name-quarry").to_owned(),
                blocked: None,
                action: MenuAction::Do(Command::UncoverVillain),
            });
        }

        if items.is_empty() {
            self.status = self
                .catalogue
                .strings
                .ui("ui.status.nothing-here")
                .to_owned();
            return;
        }
        // A single unblocked action fires immediately.
        if items.len() == 1 && items[0].blocked.is_none() {
            let action = items.remove(0).action;
            if let MenuAction::Do(command) = action {
                self.apply(command);
            }
            return;
        }
        self.modal = Some(Modal::Menu {
            title: self.catalogue.strings.ui("ui.panel.actions").to_owned(),
            items,
            selected: 0,
        });
    }

    /// Click resolution: adjacent enemies are attacked, adjacent villagers
    /// talked to, otherwise step toward the tile.
    fn handle_click(&mut self, point: Point) {
        let Some(run) = self.run.as_ref() else { return };
        let state = &run.sim.state;
        let map = state.current_map;
        let hunter = state.hunter.pos;
        if point == hunter {
            self.apply(Command::Wait);
            return;
        }
        if let Some(actor) = state.actor_at(map, point) {
            let id = actor.id;
            if hunter.is_adjacent(point) {
                self.apply(Command::Melee(Target::Actor(id)));
            } else if state.is_visible(point) {
                self.open_fire_menu(false);
            }
            return;
        }
        if hunter.is_adjacent(point) {
            if run.sim.state.npc_at(&run.sim.world, map, point).is_some() {
                self.open_interact_menu();
                return;
            }
            if let Some(dir) = Direction::toward(hunter, point) {
                self.apply(Command::Move(dir));
            }
            return;
        }
        // Walk the whole path toward the clicked tile, not one step of it.
        if self.path_step(point).is_some() {
            self.walk_to(point);
        } else {
            self.status = self.catalogue.strings.ui("ui.status.no-path").to_owned();
        }
    }

    /// First step of a BFS path from the hunter to a target tile.
    fn path_step(&self, target: Point) -> Option<Direction> {
        let run = self.run.as_ref()?;
        let sim = &run.sim;
        let state = &sim.state;
        let map = state.current_map;
        let start = state.hunter.pos;
        use rh_core::geometry::{MAP_HEIGHT, MAP_WIDTH};
        let index = |p: Point| p.y as usize * MAP_WIDTH as usize + p.x as usize;
        let mut came: Vec<Option<(Point, Direction)>> =
            vec![None; (MAP_WIDTH * MAP_HEIGHT) as usize];
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        let mut goal = None;
        // Orthogonal-first so equal-length paths favour straight steps over
        // diagonal ones (matches how players expect click-to-move to walk).
        'search: while let Some(point) = queue.pop_front() {
            for dir in Direction::ORTHOGONAL_FIRST {
                let next = point.step(dir);
                if !next.in_bounds() || came[index(next)].is_some() || next == start {
                    continue;
                }
                if !rh_core::fov::is_walkable(state.terrain(&sim.world, map, next))
                    || state.tile_occupied(&sim.world, map, next)
                {
                    continue;
                }
                came[index(next)] = Some((point, dir));
                if next == target {
                    goal = Some(next);
                    break 'search;
                }
                queue.push_back(next);
            }
        }
        let mut current = goal?;
        let mut first = None;
        while current != start {
            let (parent, dir) = came[index(current)]?;
            first = Some(dir);
            current = parent;
        }
        first
    }

    /// Hover inspection text for a map tile, if anything visible is there.
    pub fn inspect(&self, point: Point) -> Option<String> {
        let run = self.run.as_ref()?;
        let sim = &run.sim;
        let state = &sim.state;
        let map = state.current_map;
        if !point.in_bounds() {
            return None;
        }
        let seen = state.is_seen(map, point);
        let visible = state.is_visible(point);
        if !seen {
            return Some(sim.catalogue.strings.ui("ui.inspect.unexplored").to_owned());
        }
        let mut parts: Vec<String> = Vec::new();
        if point == state.hunter.pos {
            parts.push(sim.catalogue.strings.ui_fill(
                "ui.inspect.hunter",
                &[(
                    "hunter",
                    sim.catalogue.strings.get(&sim.catalogue.hunter.name),
                )],
            ));
        }
        if visible {
            if let Some(actor) = state.actor_at(map, point) {
                let name = sim.actor_name(&actor.kind);
                let mut line = sim.catalogue.strings.ui_fill(
                    "ui.inspect.actor-health",
                    &[
                        ("name", &name),
                        ("hp", &actor.hp.to_string()),
                        ("max", &actor.max_hp.to_string()),
                    ],
                );
                if actor.kind == ActorKind::Villain {
                    if actor.trapped > 0 {
                        line.push_str(", held fast");
                    }
                    if sim.villain_is_vulnerable(actor.id) {
                        line.push_str(", VULNERABLE");
                    }
                }
                parts.push(line);
            }
            if let Some(npc_id) = state.npc_at(&sim.world, map, point) {
                let spec = sim.world.npc(npc_id);
                let role = sim
                    .catalogue
                    .npcs
                    .archetypes
                    .get(&spec.archetype)
                    .map(|def| sim.catalogue.strings.get(&def.name))
                    .unwrap_or_default();
                parts.push(sim.catalogue.strings.ui_fill(
                    "ui.npc.name-and-role",
                    &[("name", &spec.name), ("role", role)],
                ));
                // Leads that involve talking to this villager.
                for opp in &sim.world.opportunities {
                    if opp.map == map
                        && state.discovered.contains(&opp.id)
                        && !state.resolved.contains(&opp.id)
                        && !state.lost.contains(&opp.id)
                        && opp.anchor == OpportunityAnchor::Npc(npc_id)
                    {
                        parts.push(sim.catalogue.strings.ui_fill(
                            "ui.inspect.lead",
                            &[(
                                "what",
                                &opp.lead(
                                    &sim.catalogue.strings,
                                    rh_core::world::LeadFraming::Perceive,
                                ),
                            )],
                        ));
                    }
                }
            }
        }
        if let Some(feature) = sim.world.map(map).feature_at(point) {
            let strings = &sim.catalogue.strings;
            // A grave's contents are secret until it is opened. Once it has
            // been, say so here as well as in the log: the log line scrolls
            // away, and a player walking a graveyard needs to be able to
            // re-check which one held nothing.
            let opened_grave = match feature.kind {
                FeatureKind::Grave { contents } if state.opened_graves.contains(&feature.id) => {
                    Some(contents)
                }
                _ => None,
            };
            parts.push(match opened_grave {
                Some(contents) => strings.ui_fill(
                    "ui.feature.opened-grave",
                    &[
                        ("name", &feature.name),
                        ("contents", grave_contents_name(strings, contents)),
                    ],
                ),
                None => feature.name.clone(),
            });
        }
        for opp in &sim.world.opportunities {
            if opp.map == map
                && state.discovered.contains(&opp.id)
                && !state.resolved.contains(&opp.id)
                && !state.lost.contains(&opp.id)
                && opp.anchor == OpportunityAnchor::Tile(point)
            {
                parts.push(sim.catalogue.strings.ui_fill(
                    "ui.inspect.lead",
                    &[(
                        "what",
                        &opp.lead(
                            &sim.catalogue.strings,
                            rh_core::world::LeadFraming::Perceive,
                        ),
                    )],
                ));
            }
        }
        if let Some(exit) = sim
            .world
            .map(map)
            .exits
            .iter()
            .find(|exit| exit.at == point)
        {
            parts.push(sim.catalogue.strings.ui_fill(
                "ui.inspect.road",
                &[("place", &sim.world.map(exit.to_map).name)],
            ));
        }
        if state
            .snares
            .iter()
            .any(|snare| snare.map == map && snare.at == point)
        {
            parts.push(sim.catalogue.strings.ui("ui.inspect.snare").to_owned());
        }
        let terrain = state.terrain(&sim.world, map, point);
        parts.push(view::terrain_name(&sim.catalogue.strings, terrain).to_owned());
        Some(parts.join(" — "))
    }

    /// Build the current frame's viewmodel.
    pub fn view(&self) -> ViewModel {
        view::build(self)
    }
}

/// What an opened grave held, for the inspect line.
///
/// A grave's contents are secret until it is opened, so callers must check
/// `opened_graves` first.
pub fn grave_contents_name(strings: &rh_content::StringTable, contents: GraveContents) -> &str {
    match contents {
        GraveContents::Empty => strings.ui("ui.grave.empty"),
        GraveContents::Mundane => strings.ui("ui.grave.mundane"),
        GraveContents::Villain => strings.ui("ui.grave.villain"),
    }
}
