//! The frame viewmodel: everything a renderer needs, nothing it must compute.
//!
//! Both clients draw exactly this structure — a glyph grid with semantic
//! colors, side-panel lines, menus, and overlays — so terminal and browser
//! presentations cannot drift apart in behaviour.

use rh_content::{StringTable, Terrain};
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
    /// Panel chrome, resolved here so neither renderer holds prose of its own.
    pub labels: PanelLabels,
}

/// The fixed furniture around the frame: panel titles and idle hints.
///
/// These live on the viewmodel rather than in each renderer so the terminal
/// and the browser cannot drift, and so every word the player reads resolves
/// through one string table.
#[derive(Debug, Clone)]
pub struct PanelLabels {
    pub hunter: String,
    pub actions: String,
    pub record: String,
    pub detail: String,
    pub case_report: String,
    pub pack: String,
    /// Heading for the look panel, by how the player is pointing.
    pub look_cursor: String,
    pub look_hover: String,
    pub look_plain: String,
    /// Shown in the look panel when nothing is targeted.
    pub look_hint: String,
    /// Shown while a direction is awaited.
    pub direction_hint: String,
    /// Footer on list screens.
    pub list_hint: String,
    /// Footer on the case report.
    pub case_report_footer: String,
}

impl PanelLabels {
    fn build(strings: &StringTable) -> Self {
        Self {
            hunter: strings.ui("ui.panel.hunter").to_owned(),
            actions: strings.ui("ui.panel.actions").to_owned(),
            record: strings.ui("ui.panel.record").to_owned(),
            detail: strings.ui("ui.panel.detail").to_owned(),
            case_report: strings.ui("ui.panel.case-report").to_owned(),
            pack: strings.ui("ui.panel.pack").to_owned(),
            look_cursor: strings.ui("ui.panel.look-cursor").to_owned(),
            look_hover: strings.ui("ui.panel.look-hover").to_owned(),
            look_plain: strings.ui("ui.panel.look").to_owned(),
            look_hint: strings.ui("ui.hint.look").to_owned(),
            direction_hint: strings.ui("ui.hint.direction").to_owned(),
            list_hint: strings.ui("ui.hint.list-nav").to_owned(),
            case_report_footer: strings.ui("ui.hint.case-report-footer").to_owned(),
        }
    }
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

pub fn terrain_name(strings: &StringTable, terrain: Terrain) -> &str {
    match terrain {
        Terrain::Floor => strings.ui("ui.terrain.floor"),
        Terrain::Wall => strings.ui("ui.terrain.wall"),
        Terrain::Tree => strings.ui("ui.terrain.tree"),
        Terrain::Water => strings.ui("ui.terrain.water"),
        Terrain::Grave => strings.ui("ui.terrain.grave"),
        Terrain::Door => strings.ui("ui.terrain.door"),
        Terrain::BarredDoor => strings.ui("ui.terrain.barred-door"),
        Terrain::Rubble => strings.ui("ui.terrain.rubble"),
        Terrain::Road => strings.ui("ui.terrain.road"),
        Terrain::Grass => strings.ui("ui.terrain.grass"),
        Terrain::Altar => strings.ui("ui.terrain.altar"),
        Terrain::Workstation => strings.ui("ui.terrain.workstation"),
    }
}

pub fn build(session: &ClientSession) -> ViewModel {
    let screen = match &session.screen {
        Screen::Splash { selected } => {
            let ui = &session.catalogue.ui;
            let strings = &session.catalogue.strings;
            ScreenView::Splash {
                title: strings.get(&ui.splash_title).to_owned(),
                intro: ui
                    .splash_intro
                    .iter()
                    .map(|id| strings.get(id).to_owned())
                    .collect(),
                bindings: ui
                    .key_bindings
                    .iter()
                    .map(|binding| {
                        (
                            strings.get(&binding.keys).to_owned(),
                            strings.get(&binding.action).to_owned(),
                        )
                    })
                    .collect(),
                options: vec![
                    session
                        .catalogue
                        .strings
                        .ui("ui.splash.option.new-run")
                        .to_owned(),
                    session
                        .catalogue
                        .strings
                        .ui("ui.splash.option.enter-seed")
                        .to_owned(),
                    session
                        .catalogue
                        .strings
                        .ui("ui.splash.option.paste-code")
                        .to_owned(),
                ],
                selected: *selected,
            }
        }
        Screen::HunterSelect { selected, .. } => ScreenView::List {
            title: session
                .catalogue
                .strings
                .ui("ui.hunter-select.title")
                .to_owned(),
            entries: session
                .catalogue
                .hunter_roster()
                .map(|(_, hunter)| {
                    // Lead with what actually differs between them, so the
                    // choice reads as two approaches rather than two stat
                    // blocks: pools first, then what only she can do.
                    let pools = session.catalogue.strings.ui_fill(
                        "ui.hunter-select.pools",
                        &[
                            ("health", &hunter.health.to_string()),
                            ("lore", &hunter.lore_cap.to_string()),
                            ("social", &hunter.social_cap.to_string()),
                            ("mystic", &hunter.mystic_cap.to_string()),
                            ("physical", &hunter.physical_cap.to_string()),
                        ],
                    );
                    let strings = &session.catalogue.strings;
                    let signatures = hunter
                        .signatures
                        .iter()
                        .map(|signature| strings.get(&signature.name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    (
                        strings.get(&hunter.name).to_owned(),
                        format!("{}\n{pools}\n{signatures}", strings.get(&hunter.title)),
                    )
                })
                .collect(),
            selected: Some(*selected),
        },
        Screen::SeedEntry { input, error } => ScreenView::TextEntry {
            title: session
                .catalogue
                .strings
                .ui("ui.seed-entry.title")
                .to_owned(),
            prompt: session
                .catalogue
                .strings
                .ui("ui.seed-entry.prompt")
                .to_owned(),
            input: input.clone(),
            error: error.clone(),
        },
        Screen::CodeEntry { input, error } => ScreenView::TextEntry {
            title: session
                .catalogue
                .strings
                .ui("ui.replay-entry.title")
                .to_owned(),
            prompt: session
                .catalogue
                .strings
                .ui("ui.replay-entry.prompt")
                .to_owned(),
            input: input.clone(),
            error: error.clone(),
        },
        Screen::Run => ScreenView::Run(Box::new(build_run_view(session))),
        Screen::Grimoire { selected } => ScreenView::List {
            title: session.catalogue.strings.ui("ui.grimoire.title").to_owned(),
            entries: session
                .catalogue
                .grimoire
                .iter()
                .map(|entry| {
                    (
                        session.catalogue.strings.get(&entry.title).to_owned(),
                        session.catalogue.strings.get(&entry.body).to_owned(),
                    )
                })
                .collect(),
            selected: Some(*selected),
        },
        Screen::Relationships { selected } => {
            let entries = relationship_entries(session);
            ScreenView::List {
                title: session
                    .catalogue
                    .strings
                    .ui("ui.relationships.title")
                    .to_owned(),
                selected: Some((*selected).min(entries.len().saturating_sub(1))),
                entries,
            }
        }
        Screen::RegionMap { selected } => {
            let entries = region_entries(session);
            ScreenView::List {
                title: session
                    .catalogue
                    .strings
                    .ui("ui.region-map.title")
                    .to_owned(),
                selected: Some((*selected).min(entries.len().saturating_sub(1))),
                entries,
            }
        }
        Screen::EventLog { selected } => {
            let entries = record_entries(session);
            ScreenView::List {
                title: session
                    .catalogue
                    .strings
                    .ui("ui.event-log.title")
                    .to_owned(),
                selected: Some((*selected).min(entries.len().saturating_sub(1))),
                entries,
            }
        }
        Screen::CaseReport => ScreenView::CaseReport(build_case_report(session)),
    };
    ViewModel {
        screen,
        status: session.status.clone(),
        labels: PanelLabels::build(&session.catalogue.strings),
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
                    // A kill site draws as its terrain, like any other
                    // landmark; the distinction is where minions gather.
                    rh_core::world::FeatureKind::KillSite
                    | rh_core::world::FeatureKind::Landmark => cell.glyph,
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
    let strings = &sim.catalogue.strings;
    let inventory: Vec<String> = hunter
        .inventory
        .iter()
        .map(|(item, count)| {
            let name = sim
                .catalogue
                .items
                .get(item)
                .map(|def| sim.catalogue.strings.get(&def.name).to_owned())
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
                    session
                        .catalogue
                        .strings
                        .ui("ui.target.silver-shot")
                        .to_owned()
                } else {
                    session.catalogue.strings.ui("ui.target.fire").to_owned()
                },
                items,
                selected: *selected,
            }
        }
        Modal::SprintDirection => OverlayView {
            title: session
                .catalogue
                .strings
                .ui("ui.direction.sprint")
                .to_owned(),
            items: Vec::new(),
            selected: 0,
        },
        Modal::SnareDirection => OverlayView {
            title: session
                .catalogue
                .strings
                .ui("ui.direction.snare")
                .to_owned(),
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
        header: strings.ui_fill(
            "ui.status.header",
            &[("place", &world_map.name), ("seed", &run.seed.to_string())],
        ),
        clock_line: format!(
            "{}{final_hunt_note}",
            strings.ui_fill(
                "ui.clock.day",
                &[
                    ("day", &state.clock.min(clock.travel_turns).to_string()),
                    ("total", &clock.travel_turns.to_string()),
                ],
            )
        ),
        health_line: strings.ui_fill(
            "ui.status.health",
            &[
                ("current", &hunter.hp.to_string()),
                ("max", &hunter.max_hp.to_string()),
            ],
        ),
        pools_line: strings.ui_fill(
            "ui.status.pools",
            &[
                ("lore", &hunter.lore.to_string()),
                ("lore_cap", &hunter_def.lore_cap.to_string()),
                ("social", &hunter.social.to_string()),
                ("social_cap", &hunter_def.social_cap.to_string()),
                (
                    "mystic",
                    &if hunter.mystic_bonus > 0 {
                        format!("{}+{}", hunter.mystic, hunter.mystic_bonus)
                    } else {
                        hunter.mystic.to_string()
                    },
                ),
                ("physical", &hunter.physical.to_string()),
                ("physical_cap", &hunter_def.physical_cap.to_string()),
            ],
        ),
        stamina_line: strings.ui_fill(
            "ui.status.stamina",
            &[
                ("current", &hunter.stamina.to_string()),
                ("max", &hunter_def.stamina_cap.to_string()),
            ],
        ),
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
            run.sim
                .catalogue
                .strings
                .ui("ui.clock.final-night")
                .to_owned()
        } else {
            run.sim.catalogue.strings.ui_fill(
                "ui.clock.day",
                &[
                    ("day", &day.to_string()),
                    ("total", &travel_turns.to_string()),
                ],
            )
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
        lines.push(session.catalogue.strings.ui_fill(
            "ui.npc.disposition",
            &[("disposition", &format!("{:?}", spec.disposition))],
        ));
        if !npc_state.alive {
            lines.push(
                session
                    .catalogue
                    .strings
                    .ui("ui.npc.dead-by-your-hand")
                    .to_owned(),
            );
        } else if npc_state.fled {
            lines.push(session.catalogue.strings.ui("ui.npc.fled").to_owned());
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
        // `spec.archetype` is a structural id; the label a player reads is the
        // archetype's authored name, which is what the log line already uses.
        let role = session
            .catalogue
            .npcs
            .archetypes
            .get(&spec.archetype)
            .map(|def| session.catalogue.strings.get(&def.name))
            .unwrap_or_default();
        entries.push((
            session.catalogue.strings.ui_fill(
                "ui.npc.name-and-role",
                &[("name", &spec.name), ("role", role)],
            ),
            lines.join("\n"),
        ));
    }
    if entries.is_empty() {
        entries.push((
            session
                .catalogue
                .strings
                .ui("ui.relationships.empty.title")
                .to_owned(),
            session
                .catalogue
                .strings
                .ui("ui.relationships.empty.body")
                .to_owned(),
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
            format!(
                "{}\nRoads: {}",
                map.role_line(&session.catalogue.strings),
                connections.join(", ")
            ),
        ));
    }
    entries
}

trait RoleLine {
    fn role_line(&self, strings: &StringTable) -> String;
}

impl RoleLine for rh_core::world::WorldMap {
    fn role_line(&self, strings: &StringTable) -> String {
        match self.role {
            rh_content::MapRole::Settlement => strings.ui("ui.region.settlement").to_owned(),
            rh_content::MapRole::Wilderness => strings.ui("ui.region.wilderness").to_owned(),
            rh_content::MapRole::OutlyingSite => strings.ui("ui.region.outlying").to_owned(),
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
        Some(rh_core::state::Outcome::Victory) => session
            .catalogue
            .strings
            .ui("ui.outcome.victory")
            .to_owned(),
        Some(rh_core::state::Outcome::Defeat) => {
            session.catalogue.strings.ui("ui.outcome.defeat").to_owned()
        }
        None => session
            .catalogue
            .strings
            .ui("ui.outcome.ongoing")
            .to_owned(),
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
