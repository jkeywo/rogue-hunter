//! The frame viewmodel: everything a renderer needs, nothing it must compute.
//!
//! Both clients draw exactly this structure — a glyph grid with semantic
//! colors, side-panel lines, menus, and overlays — so terminal and browser
//! presentations cannot drift apart in behaviour.

use rh_content::Terrain;
use rh_core::events::EventKind;
use rh_core::geometry::{Point, MAP_HEIGHT, MAP_WIDTH};
use rh_core::state::ActorKind;
use rh_core::world::OpportunityAnchor;

use crate::{ActionEntry, ClientSession, Modal, Screen};

/// Semantic cell colors; each client maps them to its palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    Hunter,
    Npc,
    Enemy,
    Villain,
    VillainVulnerable,
    Terrain,
    TerrainDim,
    Feature,
    Opportunity,
    Exit,
    Snare,
    Unseen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub glyph: char,
    pub color: CellColor,
}

#[derive(Debug, Clone)]
pub struct ViewModel {
    pub screen: ScreenView,
    /// One-line status (rejection reasons, hints).
    pub status: String,
}

#[derive(Debug, Clone)]
pub enum ScreenView {
    Splash {
        title: String,
        intro: Vec<String>,
        bindings: Vec<(String, String)>,
        options: Vec<String>,
        selected: usize,
    },
    TextEntry {
        title: String,
        prompt: String,
        input: String,
        error: Option<String>,
    },
    Run(Box<RunView>),
    List {
        title: String,
        /// (heading, body) pairs; body may be empty for plain lists.
        entries: Vec<(String, String)>,
        selected: Option<usize>,
    },
    CaseReport(CaseReportView),
}

#[derive(Debug, Clone)]
pub struct RunView {
    /// Row-major 32x20 grid.
    pub cells: Vec<Cell>,
    pub header: String,
    pub clock_line: String,
    pub health_line: String,
    pub pools_line: String,
    pub stamina_line: String,
    pub inventory: Vec<String>,
    /// The context-sensitive, clickable action panel.
    pub actions: Vec<ActionEntry>,
    pub log_tail: Vec<(EventKind, String)>,
    /// What the look cursor / mouse is pointing at, if anything.
    pub cursor: Option<Point>,
    /// Whether keyboard look mode is engaged (marker vs. passive hover).
    pub looking: bool,
    /// Inspection text for the cursor tile; `None` when nothing is targeted.
    pub inspect: Option<String>,
    /// Active modal (menu / targeting), rendered as an overlay.
    pub overlay: Option<OverlayView>,
}

#[derive(Debug, Clone)]
pub struct OverlayView {
    pub title: String,
    /// (label, blocked-reason) pairs.
    pub items: Vec<(String, Option<String>)>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct CaseReportView {
    pub outcome: String,
    pub villain: String,
    pub origin: String,
    pub scheme: String,
    pub hidden_clues: Vec<String>,
    pub routes: Vec<String>,
    pub share_code: String,
}

pub fn terrain_glyph(terrain: Terrain) -> char {
    match terrain {
        Terrain::Floor => '.',
        Terrain::Wall => '#',
        Terrain::Tree => 'T',
        Terrain::Water => '~',
        Terrain::Grave => 'n',
        Terrain::Door => '+',
        Terrain::BarredDoor => '=',
        Terrain::Rubble => '%',
        Terrain::Road => ':',
        Terrain::Grass => ',',
        Terrain::Altar => 'A',
        Terrain::Workstation => 'W',
    }
}

pub fn terrain_name(terrain: Terrain) -> &'static str {
    match terrain {
        Terrain::Floor => "flagstones",
        Terrain::Wall => "a wall",
        Terrain::Tree => "dense growth",
        Terrain::Water => "water",
        Terrain::Grave => "a grave",
        Terrain::Door => "a door",
        Terrain::BarredDoor => "a barred door",
        Terrain::Rubble => "fallen rubble",
        Terrain::Road => "the road",
        Terrain::Grass => "open ground",
        Terrain::Altar => "the altar",
        Terrain::Workstation => "the workbench",
    }
}

pub fn build(session: &ClientSession) -> ViewModel {
    let screen = match &session.screen {
        Screen::Splash { selected } => {
            let ui = &session.catalogue.ui;
            ScreenView::Splash {
                title: ui.splash_title.clone(),
                intro: ui.splash_intro.clone(),
                bindings: ui
                    .key_bindings
                    .iter()
                    .map(|binding| (binding.keys.clone(), binding.action.clone()))
                    .collect(),
                options: vec![
                    "New Run".to_owned(),
                    "Enter Seed".to_owned(),
                    "Paste Replay Code".to_owned(),
                ],
                selected: *selected,
            }
        }
        Screen::HunterSelect { selected, .. } => ScreenView::List {
            title: "Who Takes the Case".to_owned(),
            entries: session
                .catalogue
                .hunter_roster()
                .map(|(_, hunter)| {
                    // Lead with what actually differs between them, so the
                    // choice reads as two approaches rather than two stat
                    // blocks: pools first, then what only she can do.
                    let pools = format!(
                        "Health {}  Lore {}  Social {}  Mystic {}  Physical {}",
                        hunter.health,
                        hunter.lore_cap,
                        hunter.social_cap,
                        hunter.mystic_cap,
                        hunter.physical_cap
                    );
                    let signatures = hunter
                        .signatures
                        .iter()
                        .map(|signature| signature.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ");
                    (
                        hunter.name.clone(),
                        format!("{}\n{pools}\n{signatures}", hunter.title),
                    )
                })
                .collect(),
            selected: Some(*selected),
        },
        Screen::SeedEntry { input, error } => ScreenView::TextEntry {
            title: "Enter Seed".to_owned(),
            prompt: "Type a number, then confirm.".to_owned(),
            input: input.clone(),
            error: error.clone(),
        },
        Screen::CodeEntry { input, error } => ScreenView::TextEntry {
            title: "Paste Replay Code".to_owned(),
            prompt: "Paste an RH1- share code, then confirm.".to_owned(),
            input: input.clone(),
            error: error.clone(),
        },
        Screen::Run => ScreenView::Run(Box::new(build_run_view(session))),
        Screen::Grimoire { selected } => ScreenView::List {
            title: "The Grimoire".to_owned(),
            entries: session
                .catalogue
                .grimoire
                .iter()
                .map(|entry| (entry.title.clone(), entry.body.clone()))
                .collect(),
            selected: Some(*selected),
        },
        Screen::Relationships { selected } => {
            let entries = relationship_entries(session);
            ScreenView::List {
                title: "Faces and Entanglements".to_owned(),
                selected: Some((*selected).min(entries.len().saturating_sub(1))),
                entries,
            }
        }
        Screen::RegionMap { selected } => {
            let entries = region_entries(session);
            ScreenView::List {
                title: "The Valley".to_owned(),
                selected: Some((*selected).min(entries.len().saturating_sub(1))),
                entries,
            }
        }
        Screen::EventLog { selected } => {
            let entries = record_entries(session);
            ScreenView::List {
                title: "The Record".to_owned(),
                selected: Some((*selected).min(entries.len().saturating_sub(1))),
                entries,
            }
        }
        Screen::CaseReport => ScreenView::CaseReport(build_case_report(session)),
    };
    ViewModel {
        screen,
        status: session.status.clone(),
    }
}

fn build_run_view(session: &ClientSession) -> RunView {
    let Some(run) = session.run.as_ref() else {
        return empty_run_view();
    };
    let sim = &run.sim;
    let state = &sim.state;
    let map_id = state.current_map;
    let world_map = sim.world.map(map_id);

    let mut cells = vec![
        Cell {
            glyph: ' ',
            color: CellColor::Unseen
        };
        (MAP_WIDTH * MAP_HEIGHT) as usize
    ];
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let point = Point::new(x, y);
            let index = (y * MAP_WIDTH + x) as usize;
            if !state.is_seen(map_id, point) {
                continue;
            }
            let visible = state.is_visible(point);
            let terrain = state.terrain(&sim.world, map_id, point);
            let mut cell = Cell {
                glyph: terrain_glyph(terrain),
                color: if visible {
                    CellColor::Terrain
                } else {
                    CellColor::TerrainDim
                },
            };
            if sim
                .world
                .map(map_id)
                .exits
                .iter()
                .any(|exit| exit.at == point)
            {
                cell = Cell {
                    glyph: '>',
                    color: CellColor::Exit,
                };
            }
            if let Some(feature) = world_map.feature_at(point) {
                // An opened grave shows an emptied pit ('u') rather than a mound.
                let opened_grave =
                    matches!(feature.kind, rh_core::world::FeatureKind::Grave { .. })
                        && state.opened_graves.contains(&feature.id);
                let glyph = match feature.kind {
                    rh_core::world::FeatureKind::Altar => 'A',
                    rh_core::world::FeatureKind::Workstation => 'W',
                    rh_core::world::FeatureKind::Grave { .. } if opened_grave => 'u',
                    rh_core::world::FeatureKind::Grave { .. } => 'n',
                    rh_core::world::FeatureKind::Landmark => cell.glyph,
                };
                cell = Cell {
                    glyph,
                    // Opened graves read as spent: dimmed like unseen ground.
                    color: if !visible || opened_grave {
                        CellColor::TerrainDim
                    } else {
                        CellColor::Feature
                    },
                };
            }
            // Discovered, unresolved tile opportunities glow.
            let has_lead = sim.world.opportunities.iter().any(|opp| {
                opp.map == map_id
                    && state.discovered.contains(&opp.id)
                    && !state.resolved.contains(&opp.id)
                    && !state.lost.contains(&opp.id)
                    && opp.anchor == OpportunityAnchor::Tile(point)
            });
            if has_lead {
                cell = Cell {
                    glyph: '?',
                    color: CellColor::Opportunity,
                };
            }
            if state
                .snares
                .iter()
                .any(|snare| snare.map == map_id && snare.at == point)
            {
                cell = Cell {
                    glyph: '^',
                    color: CellColor::Snare,
                };
            }
            if visible {
                if let Some(npc_id) = state.npc_at(&sim.world, map_id, point) {
                    let spec = sim.world.npc(npc_id);
                    cell = Cell {
                        glyph: spec.glyph,
                        color: CellColor::Npc,
                    };
                }
                if let Some(actor) = state.actor_at(map_id, point) {
                    let (glyph, color) = match &actor.kind {
                        ActorKind::Enemy(enemy) => (
                            sim.catalogue
                                .enemies
                                .get(enemy)
                                .map(|def| def.glyph)
                                .unwrap_or('e'),
                            CellColor::Enemy,
                        ),
                        ActorKind::Villain => {
                            let def = sim.villain_def();
                            let vulnerable = sim.villain_is_vulnerable(actor.id);
                            (
                                def.glyph,
                                if vulnerable {
                                    CellColor::VillainVulnerable
                                } else {
                                    CellColor::Villain
                                },
                            )
                        }
                    };
                    cell = Cell { glyph, color };
                }
            }
            if point == state.hunter.pos {
                cell = Cell {
                    glyph: sim.catalogue.hunter.glyph,
                    color: CellColor::Hunter,
                };
            }
            cells[index] = cell;
        }
    }

    // Villain marker once the location is known but the villain is unseen.
    if state.villain_location_known && !state.villain.active {
        let (marker_map, at) = sim.world.villain.lair;
        if marker_map == map_id {
            let index = (at.y * MAP_WIDTH + at.x) as usize;
            if cells[index].color != CellColor::Hunter {
                cells[index] = Cell {
                    glyph: '!',
                    color: CellColor::Villain,
                };
            }
        }
    }

    let hunter = &state.hunter;
    let clock = &sim.catalogue.balance.clock;
    let hunter_def = &sim.catalogue.hunter;
    let inventory: Vec<String> = hunter
        .inventory
        .iter()
        .map(|(item, count)| {
            let name = sim
                .catalogue
                .items
                .get(item)
                .map(|def| def.name.clone())
                .unwrap_or_else(|| item.clone());
            if *count > 1 {
                format!("{name} x{count}")
            } else {
                name
            }
        })
        .collect();

    let log_tail: Vec<(EventKind, String)> = state
        .log
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|event| (event.kind, event.text.clone()))
        .collect();

    let overlay = session.modal.as_ref().map(|modal| match modal {
        Modal::FireTarget { silver, selected } => {
            let items = session
                .fire_targets()
                .iter()
                .map(|(target, at)| {
                    let name = match target {
                        rh_core::command::Target::Actor(id) => state
                            .actor(*id)
                            .map(|actor| {
                                format!(
                                    "{} ({}/{})",
                                    sim.actor_name(&actor.kind),
                                    actor.hp,
                                    actor.max_hp
                                )
                            })
                            .unwrap_or_else(|| "?".to_owned()),
                        rh_core::command::Target::Npc(npc) => sim.world.npc(*npc).name.clone(),
                    };
                    (format!("{name} at {},{}", at.x, at.y), None)
                })
                .collect();
            OverlayView {
                title: if *silver {
                    "Silver shot at...".to_owned()
                } else {
                    "Fire at...".to_owned()
                },
                items,
                selected: *selected,
            }
        }
        Modal::SprintDirection => OverlayView {
            title: "Sprint: which direction?".to_owned(),
            items: Vec::new(),
            selected: 0,
        },
        Modal::SnareDirection => OverlayView {
            title: "Set snare: which direction?".to_owned(),
            items: Vec::new(),
            selected: 0,
        },
        Modal::Menu {
            title,
            items,
            selected,
        } => OverlayView {
            title: title.clone(),
            items: items
                .iter()
                .map(|item| (item.label.clone(), item.blocked.clone()))
                .collect(),
            selected: *selected,
        },
    });

    let final_hunt_note = if state.final_hunt {
        "  THE HUNT IS HERE"
    } else {
        ""
    };
    RunView {
        cells,
        header: format!("{}  (seed {})", world_map.name, run.seed),
        clock_line: format!(
            "Day {} of {}{final_hunt_note}",
            state.clock.min(clock.travel_turns),
            clock.travel_turns
        ),
        health_line: format!("Health {}/{}", hunter.hp, hunter.max_hp),
        pools_line: format!(
            "Lore {}/{}  Social {}/{}  Mystic {}{}  Physical {}/{}",
            hunter.lore,
            hunter_def.lore_cap,
            hunter.social,
            hunter_def.social_cap,
            hunter.mystic,
            if hunter.mystic_bonus > 0 {
                format!("+{}", hunter.mystic_bonus)
            } else {
                String::new()
            },
            hunter.physical,
            hunter_def.physical_cap,
        ),
        stamina_line: format!("Stamina {}/{}", hunter.stamina, hunter_def.stamina_cap),
        inventory,
        actions: session.available_actions(),
        log_tail,
        cursor: session.look_point(),
        looking: session.is_looking(),
        inspect: session
            .look_point()
            .and_then(|point| session.inspect(point)),
        overlay,
    }
}

fn empty_run_view() -> RunView {
    RunView {
        cells: vec![
            Cell {
                glyph: ' ',
                color: CellColor::Unseen
            };
            (MAP_WIDTH * MAP_HEIGHT) as usize
        ],
        header: String::new(),
        clock_line: String::new(),
        health_line: String::new(),
        pools_line: String::new(),
        stamina_line: String::new(),
        inventory: Vec::new(),
        actions: Vec::new(),
        log_tail: Vec::new(),
        cursor: None,
        looking: false,
        inspect: None,
        overlay: None,
    }
}

/// The record grouped as one entry per day, each with its events as lines.
pub(crate) fn record_entries(session: &ClientSession) -> Vec<(String, String)> {
    let Some(run) = session.run.as_ref() else {
        return Vec::new();
    };
    let travel_turns = run.sim.catalogue.balance.clock.travel_turns;
    let mut entries: Vec<(String, String)> = Vec::new();
    for event in &run.sim.state.log {
        let day = event.global_turn;
        let heading = if day >= travel_turns {
            "The final night".to_owned()
        } else {
            format!("Day {} of {}", day, travel_turns)
        };
        match entries.last_mut() {
            Some((last_heading, body)) if *last_heading == heading => {
                body.push('\n');
                body.push_str(&event.text);
            }
            _ => entries.push((heading, event.text.clone())),
        }
    }
    entries
}

pub(crate) fn relationship_entries(session: &ClientSession) -> Vec<(String, String)> {
    let Some(run) = session.run.as_ref() else {
        return Vec::new();
    };
    let sim = &run.sim;
    let state = &sim.state;
    let mut entries = Vec::new();
    for spec in &sim.world.npcs {
        if !state.met_npcs.contains(&spec.id) {
            continue;
        }
        let npc_state = &state.npcs[spec.id.0 as usize];
        let mut lines = Vec::new();
        lines.push(format!("Disposition: {:?}", spec.disposition));
        if !npc_state.alive {
            lines.push("Dead by your hand.".to_owned());
        } else if npc_state.fled {
            lines.push("Fled.".to_owned());
        }
        if state.known_secrets.contains(&spec.id) {
            let disproved = state.disproved_secrets.contains(&spec.id);
            lines.push(format!(
                "Secret: {}{}",
                spec.secret.text,
                if disproved {
                    " (disproved - the whisper is false)"
                } else {
                    ""
                }
            ));
        }
        for link in &spec.links {
            let key = rh_core::world::link_key(spec.id, link.to);
            if state.known_links.contains(&key) {
                lines.push(link.discovered_text.clone());
            }
        }
        entries.push((
            format!("{}, the {}", spec.name, spec.archetype),
            lines.join("\n"),
        ));
    }
    if entries.is_empty() {
        entries.push((
            "No faces yet".to_owned(),
            "Meet the villagers; their entanglements are a deduction tool.".to_owned(),
        ));
    }
    entries
}

pub(crate) fn region_entries(session: &ClientSession) -> Vec<(String, String)> {
    let Some(run) = session.run.as_ref() else {
        return Vec::new();
    };
    let sim = &run.sim;
    let state = &sim.state;
    let mut entries = Vec::new();
    for (index, map) in sim.world.maps.iter().enumerate() {
        let here = state.current_map.0 as usize == index;
        let connections: Vec<String> = map
            .exits
            .iter()
            .map(|exit| {
                let name = &sim.world.map(exit.to_map).name;
                if exit.ambush_route {
                    format!("{name} (ambush country, {}%)", sim.world.ambush_percent)
                } else {
                    name.to_string()
                }
            })
            .collect();
        entries.push((
            format!(
                "{}{}",
                map.name,
                if here { "  <- you are here" } else { "" }
            ),
            format!("{}\nRoads: {}", map.role_line(), connections.join(", ")),
        ));
    }
    entries
}

trait RoleLine {
    fn role_line(&self) -> String;
}

impl RoleLine for rh_core::world::WorldMap {
    fn role_line(&self) -> String {
        match self.role {
            rh_content::MapRole::Settlement => "Hearth and rumour. Safe by daylight.".to_owned(),
            rh_content::MapRole::Wilderness => {
                "The deep wood. The shortcut runs through it.".to_owned()
            }
            rh_content::MapRole::OutlyingSite => "The forsaken manor and its crypt.".to_owned(),
        }
    }
}

fn build_case_report(session: &ClientSession) -> CaseReportView {
    let Some(run) = session.run.as_ref() else {
        return CaseReportView {
            outcome: String::new(),
            villain: String::new(),
            origin: String::new(),
            scheme: String::new(),
            hidden_clues: Vec::new(),
            routes: Vec::new(),
            share_code: String::new(),
        };
    };
    let sim = &run.sim;
    let state = &sim.state;
    let outcome = match run.outcome() {
        Some(rh_core::state::Outcome::Victory) => {
            "THE VALLEY IS DELIVERED. The villain is destroyed.".to_owned()
        }
        Some(rh_core::state::Outcome::Defeat) => {
            "THE DARK KEEPS THE VALLEY. The hunter fell on the final night.".to_owned()
        }
        None => "The run continues...".to_owned(),
    };
    let villain_def = sim.villain_def();
    let origin = &sim.catalogue.origins[&sim.world.villain.origin];
    let scheme = &sim.catalogue.schemes[&sim.world.villain.scheme];
    let hidden_clues: Vec<String> = sim
        .world
        .opportunities
        .iter()
        .filter(|opp| {
            !state.resolved.contains(&opp.id)
                && matches!(
                    opp.grants,
                    rh_core::world::OpportunityGrant::IdentityClue { .. }
                        | rh_core::world::OpportunityGrant::OriginSign { .. }
                        | rh_core::world::OpportunityGrant::SchemeSign { .. }
                        | rh_core::world::OpportunityGrant::LocationClue
                )
        })
        .map(|opp| format!("{} — {}", opp.name, opp.reveal))
        .collect();
    let routes: Vec<String> = sim
        .world
        .certified_routes
        .iter()
        .map(|route| {
            let steps: Vec<String> = route
                .steps
                .iter()
                .map(|step| format!("t{}: {}", step.turn, step.description))
                .collect();
            format!(
                "{} (ready by day {}, {}\u{2030} viable)\n{}",
                route.label,
                route.ready_by_turn,
                route.viability_permille,
                steps.join("\n")
            )
        })
        .collect();
    CaseReportView {
        outcome,
        villain: format!("{} — {}", villain_def.name, sim.world.villain.title),
        origin: format!("{}: {}", origin.name, origin.description),
        scheme: format!("{}: {}", scheme.name, scheme.description),
        hidden_clues,
        routes,
        share_code: run.share_code(),
    }
}
