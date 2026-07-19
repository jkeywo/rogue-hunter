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

pub use input::Intent;
pub use view::{Cell, CellColor, ViewModel};

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
    Relationships {
        selected: usize,
    },
    RegionMap {
        selected: usize,
    },
    EventLog {
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
    /// Random-ish seed source for "New Run" (clients pass a clock value).
    seed_nonce: u64,
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
            seed_nonce,
        }
    }

    /// The current share code, if a run is active (for saves and copy/paste).
    pub fn share_code(&self) -> Option<String> {
        self.run.as_ref().map(|run| run.share_code())
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
        match &self.screen {
            Screen::Splash { .. } => self.handle_splash(intent),
            Screen::SeedEntry { .. } => self.handle_seed_entry(intent),
            Screen::HunterSelect { .. } => self.handle_hunter_select(intent),
            Screen::CodeEntry { .. } => self.handle_code_entry(intent),
            Screen::Run => self.handle_run(intent),
            Screen::Grimoire { .. } => self.handle_list_screen(intent),
            Screen::Relationships { .. } => self.handle_list_screen(intent),
            Screen::RegionMap { .. } => self.handle_list_screen(intent),
            Screen::EventLog { .. } => self.handle_list_screen(intent),
            Screen::CaseReport => self.handle_case_report(intent),
        }
    }

    // -- Screens ---------------------------------------------------------------

    fn handle_splash(&mut self, intent: Intent) {
        let Screen::Splash { selected } = &mut self.screen else {
            return;
        };
        match intent {
            Intent::Up => *selected = selected.saturating_sub(1),
            Intent::Down => *selected = (*selected + 1).min(2),
            Intent::HoverRow(index) if index <= 2 => *selected = index,
            Intent::Select(index) if index <= 2 => {
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
                _ => {
                    self.screen = Screen::CodeEntry {
                        input: String::new(),
                        error: None,
                    }
                }
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
        match intent {
            Intent::Up => *selected = selected.saturating_sub(1),
            Intent::Down => *selected = (*selected + 1).min(count.saturating_sub(1)),
            Intent::HoverRow(index) if index < count => *selected = index,
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
            Screen::Relationships { .. } => Some(Intent::Relationships),
            Screen::RegionMap { .. } => Some(Intent::RegionMap),
            Screen::EventLog { .. } => Some(Intent::EventLog),
            _ => None,
        };
        match &mut self.screen {
            Screen::Grimoire { selected }
            | Screen::Relationships { selected }
            | Screen::RegionMap { selected }
            | Screen::EventLog { selected } => match &intent {
                Intent::Up => {
                    *selected = selected.saturating_sub(1);
                    return;
                }
                Intent::Select(index) | Intent::HoverRow(index) if *index < count => {
                    // Reference lists have nothing to activate, so a click or
                    // a hover both just move the reading position.
                    *selected = *index;
                    return;
                }
                Intent::Down => {
                    *selected = (*selected + 1).min(count.saturating_sub(1));
                    return;
                }
                _ => {}
            },
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
            Screen::Relationships { .. } => view::relationship_entries(self).len(),
            Screen::RegionMap { .. } => view::region_entries(self).len(),
            Screen::EventLog { .. } => view::record_entries(self).len(),
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
                let name = run.sim.catalogue.hunter.name.clone();
                self.run = Some(run);
                self.modal = None;
                self.look_cursor = None;
                self.hover = None;
                self.screen = Screen::Run;
                // Say so when the requested seed had no fair case for this
                // hunter: the player asked for a specific world and did not
                // get it, which they should hear from us rather than notice.
                self.status = if used == seed {
                    format!("{name}. Seed {seed}.")
                } else {
                    format!("{name}. Seed {seed} had no case for her; seed {used}.")
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
            Intent::Grimoire => self.screen = Screen::Grimoire { selected: 0 },
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
                    Intent::Up => {
                        self.modal = Some(Modal::FireTarget {
                            silver,
                            selected: selected.saturating_sub(1),
                        })
                    }
                    Intent::Down => {
                        self.modal = Some(Modal::FireTarget {
                            silver,
                            selected: (selected + 1).min(targets.len().saturating_sub(1)),
                        })
                    }
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
                Intent::Up => {
                    self.modal = Some(Modal::Menu {
                        title,
                        items,
                        selected: selected.saturating_sub(1),
                    })
                }
                Intent::Down => {
                    let last = items.len().saturating_sub(1);
                    self.modal = Some(Modal::Menu {
                        title,
                        items,
                        selected: (selected + 1).min(last),
                    })
                }
                Intent::HoverRow(index) if index < items.len() => {
                    self.modal = Some(Modal::Menu {
                        title,
                        items,
                        selected: index,
                    });
                }
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
            (kept, Intent::Cancel) => {
                let _ = kept;
                self.modal = None;
            }
            (kept, _) => self.modal = Some(kept),
        }
        self.check_run_over();
    }

    /// Apply a semantic command; rejections become the status line.
    fn apply(&mut self, command: Command) {
        let Some(run) = &mut self.run else { return };
        if let Err(rejection) = run.apply(command) {
            self.status = rejection.to_string();
        }
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

        let mut push =
            |key: &str, label: &str, enabled: bool, note: Option<String>, intent: Intent| {
                actions.push(ActionEntry {
                    key: key.to_owned(),
                    label: label.to_owned(),
                    enabled,
                    note,
                    intent,
                });
            };

        push(
            "e",
            strings.ui("ui.action.interact"),
            true,
            None,
            Intent::Interact,
        );
        push(
            ";",
            strings.ui("ui.action.look"),
            true,
            None,
            Intent::ToggleLook,
        );
        push(".", strings.ui("ui.action.wait"), true, None, Intent::Wait);

        let targets = !self.fire_targets().is_empty();
        let shots = hunter.item_count("flintlock-shot");
        push(
            "f",
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
                "F",
                strings.ui("ui.action.fire-silver"),
                targets,
                (!targets).then(|| strings.ui("ui.blocked.nothing-in-sight").to_owned()),
                Intent::FireSilver,
            );
        }

        let stamina = hunter.stamina;
        let aim_cost = self.manoeuvre_cost("aim");
        push(
            "a",
            strings.ui("ui.action.aim"),
            stamina >= aim_cost,
            (stamina < aim_cost).then(|| format!("{aim_cost} stamina")),
            Intent::Aim,
        );
        let power_cost = self.manoeuvre_cost("power-attack");
        push(
            "p",
            strings.ui("ui.action.power-attack"),
            stamina >= power_cost,
            (stamina < power_cost).then(|| {
                strings.ui_fill("ui.blocked.stamina", &[("cost", &power_cost.to_string())])
            }),
            Intent::PowerAttack,
        );
        let sprint_cost = self.manoeuvre_cost("sprint");
        push(
            "s",
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
            "x",
            strings.ui("ui.action.set-snare"),
            physical >= 1,
            (physical < 1).then(|| strings.ui("ui.blocked.one-physical").to_owned()),
            Intent::SetSnare,
        );
        let adjacent_foe = self.adjacent_hostile().is_some();
        push(
            "K",
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
                "q",
                strings.ui("ui.action.draught"),
                true,
                None,
                Intent::Draught,
            );
        }
        if hunter.item_count("binding-charm") > 0 {
            let adjacent_villain = self.adjacent_villain();
            push(
                "c",
                strings.ui("ui.action.binding-charm"),
                adjacent_villain,
                (!adjacent_villain)
                    .then(|| strings.ui("ui.blocked.no-revenant-adjacent").to_owned()),
                Intent::Charm,
            );
        }

        push(
            "g",
            strings.ui("ui.action.grimoire"),
            true,
            None,
            Intent::Grimoire,
        );
        push(
            "r",
            strings.ui("ui.action.faces"),
            true,
            None,
            Intent::Relationships,
        );
        push(
            "v",
            strings.ui("ui.action.valley"),
            true,
            None,
            Intent::RegionMap,
        );
        push(
            "L",
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
            items.push(MenuItem {
                label: format!("Travel to {name} (spends a day)"),
                blocked: state
                    .final_hunt
                    .then(|| "The hunt is here; there is no time.".into()),
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
            let blocked = opp.pool.and_then(|pool| {
                let mut cost = opp.cost;
                if pool == rh_content::PoolKind::Social && state.settlement_hostile {
                    cost += 1;
                }
                (state.hunter.pool(pool) < cost)
                    .then(|| format!("Needs {cost} {pool:?} point(s); travel restores your pools."))
            });
            let cost_note = match opp.pool {
                Some(pool) => format!(" [{} {pool:?}]", opp.cost),
                None => String::new(),
            };
            items.push(MenuItem {
                label: format!("{}{}", opp.name, cost_note),
                blocked,
                action: MenuAction::Do(Command::Interact(opp.id)),
            });
        }

        // Graves can be opened with muscle.
        for feature in &sim.world.map(map).features {
            if let FeatureKind::Grave { contents } = feature.kind {
                let _ = contents;
                if hunter == feature.at || hunter.is_adjacent(feature.at) {
                    let blocked = if state.opened_graves.contains(&feature.id) {
                        Some(
                            sim.catalogue
                                .strings
                                .ui("ui.blocked.already-opened")
                                .to_owned(),
                        )
                    } else if state.hunter.physical < 1 {
                        Some("Needs 1 Physical point.".to_owned())
                    } else {
                        None
                    };
                    items.push(MenuItem {
                        label: format!("Open {} [1 Physical]", feature.name),
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
                    blocked: (state.hunter.physical < 1)
                        .then(|| "Needs 1 Physical point.".to_owned()),
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
                Some("The hunt is here; there is no time.".to_owned())
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
                let blocked =
                    (!missing.is_empty()).then(|| format!("Missing: {}.", missing.join(", ")));
                items.push(MenuItem {
                    label: format!("Craft: {}", recipe.name),
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
                label: format!("Talk with {}", spec.name),
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
                    label: format!("Buy powder and ball from {} [2 coin]", spec.name),
                    blocked,
                    action: MenuAction::Do(Command::BuyShot(spec.id)),
                });
            }
        }

        // Naming the villain once the proofs agree.
        if state.identity_clues.len() >= 2 && !state.villain_uncovered {
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
        // Step along a path toward the clicked tile.
        if let Some(dir) = self.path_step(point) {
            self.apply(Command::Move(dir));
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
            parts.push(format!("{} (you)", sim.catalogue.hunter.name));
        }
        if visible {
            if let Some(actor) = state.actor_at(map, point) {
                let name = sim.actor_name(&actor.kind);
                let mut line = format!("{name} ({}/{} health)", actor.hp, actor.max_hp);
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
                        parts.push(format!("Lead: {}", opp.name));
                    }
                }
            }
        }
        if let Some(feature) = sim.world.map(map).feature_at(point) {
            let opened = matches!(feature.kind, FeatureKind::Grave { .. })
                && state.opened_graves.contains(&feature.id);
            if opened {
                parts.push(format!("{} (opened)", feature.name));
            } else {
                parts.push(feature.name.clone());
            }
        }
        for opp in &sim.world.opportunities {
            if opp.map == map
                && state.discovered.contains(&opp.id)
                && !state.resolved.contains(&opp.id)
                && !state.lost.contains(&opp.id)
                && opp.anchor == OpportunityAnchor::Tile(point)
            {
                parts.push(format!("Lead: {}", opp.name));
            }
        }
        if let Some(exit) = sim
            .world
            .map(map)
            .exits
            .iter()
            .find(|exit| exit.at == point)
        {
            parts.push(format!("Road to {}", sim.world.map(exit.to_map).name));
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

/// A grave's contents are secret until opened; used by the case report.
pub fn grave_contents_name(strings: &rh_content::StringTable, contents: GraveContents) -> &str {
    match contents {
        GraveContents::Empty => strings.ui("ui.grave.empty"),
        GraveContents::Mundane => strings.ui("ui.grave.mundane"),
        GraveContents::Villain => strings.ui("ui.grave.villain"),
    }
}
