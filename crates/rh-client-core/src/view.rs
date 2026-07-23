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

use crate::{ActionEntry, ClientSession, Modal, Screen, SightEntry};

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
    /// Heading and empty-state for the in-sight panel.
    pub in_sight: String,
    pub in_sight_empty: String,
    /// Textual markers so hostility in the sight list is not colour alone.
    pub sight_hostile: String,
    pub sight_villager: String,
    /// Heading for the map-key panel.
    pub legend: String,
    /// Heading for the case report's what-you-carried section.
    pub preparations: String,
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
    /// Case report headings, each with a `{what}` slot.
    pub report_villain: String,
    pub report_origin: String,
    pub report_scheme: String,
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
            in_sight: strings.ui("ui.panel.in-sight").to_owned(),
            in_sight_empty: strings.ui("ui.panel.in-sight-empty").to_owned(),
            sight_hostile: strings.ui("ui.sight.hostile").to_owned(),
            sight_villager: strings.ui("ui.sight.villager").to_owned(),
            legend: strings.ui("ui.legend.title").to_owned(),
            preparations: strings.ui("ui.report.preparations-title").to_owned(),
            look_cursor: strings.ui("ui.panel.look-cursor").to_owned(),
            look_hover: strings.ui("ui.panel.look-hover").to_owned(),
            look_plain: strings.ui("ui.panel.look").to_owned(),
            look_hint: strings.ui("ui.hint.look").to_owned(),
            direction_hint: strings.ui("ui.hint.direction").to_owned(),
            list_hint: strings.ui("ui.hint.list-nav").to_owned(),
            report_villain: strings.ui("ui.report.heading.villain").to_owned(),
            report_origin: strings.ui("ui.report.heading.origin").to_owned(),
            report_scheme: strings.ui("ui.report.heading.scheme").to_owned(),
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

/// One line of the pack: what it is called, and what it is for.
#[derive(Debug, Clone)]
pub struct PackItem {
    pub label: String,
    pub description: String,
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
    /// What is in the pack, each with what it actually does — the clients
    /// show the description on hover, so a player never has to guess what
    /// a name means.
    pub inventory: Vec<PackItem>,
    /// Everything currently in sight, nearest first, hostiles before people.
    pub in_sight: Vec<SightEntry>,
    /// A first-time teaching line for this frame, if one fired.
    pub hint: Option<String>,
    /// The context-sensitive, clickable action panel.
    pub actions: Vec<ActionEntry>,
    pub log_tail: Vec<(EventKind, String)>,
    /// What the look cursor / mouse is pointing at, if anything.
    pub cursor: Option<Point>,
    /// Whether keyboard look mode is engaged (marker vs. passive hover).
    pub looking: bool,
    /// Inspection text for the cursor tile; `None` when nothing is targeted.
    pub inspect: Option<String>,
    /// A key to the glyphs on the map: what each character means, so the map
    /// is never a picture a player has to have colour to read. Fixed
    /// vocabulary, not the live board — a legend, not a second map.
    pub legend: Vec<LegendEntry>,
    /// Active modal (menu / targeting), rendered as an overlay.
    pub overlay: Option<OverlayView>,
}

/// One row of the map key: a glyph and what it stands for.
#[derive(Debug, Clone)]
pub struct LegendEntry {
    pub glyph: char,
    pub meaning: String,
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
    /// How far the quarry's scheme got, in tiers.
    pub tier: String,
    /// What the hunter actually held and knew when it ended.
    pub preparations: Vec<String>,
    /// The certified routes, each step marked against what was really done.
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
        Terrain::Workstation => '=',
    }
}

/// A pool's name as the player reads it.
///
/// These used to render through `{pool:?}`, which puts a Rust identifier in
/// front of the player and cannot be translated.
pub fn pool_name(strings: &StringTable, pool: rh_content::PoolKind) -> &str {
    match pool {
        rh_content::PoolKind::Lore => strings.ui("ui.pool.lore"),
        rh_content::PoolKind::Social => strings.ui("ui.pool.social"),
        rh_content::PoolKind::Mystic => strings.ui("ui.pool.mystic"),
        rh_content::PoolKind::Physical => strings.ui("ui.pool.physical"),
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

/// The key that currently fires an intent, for the splash bindings table.
fn key_hint(session: &ClientSession, intent: &crate::Intent) -> String {
    crate::input::key_label(session.controls, intent).unwrap_or_default()
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
                        // The keys column is filled from the same binding
                        // table the translator reads, so switching scheme
                        // cannot leave the splash advertising the old keys.
                        (
                            strings.ui_fill(
                                binding.keys.as_str(),
                                &[
                                    ("steer", strings.ui(session.controls.steer_id())),
                                    ("silver", &key_hint(session, &crate::Intent::FireSilver)),
                                    ("killing", &key_hint(session, &crate::Intent::KillingBlow)),
                                    ("log", &key_hint(session, &crate::Intent::EventLog)),
                                ],
                            ),
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
                    // Reads as a setting rather than a destination: the row
                    // says which scheme is in force, and picking it swaps.
                    strings.ui_fill(
                        "ui.splash.option.controls",
                        &[("scheme", strings.ui(session.controls.label_id()))],
                    ),
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
        Screen::Guide { selected } => ScreenView::List {
            title: session.catalogue.strings.ui("ui.guide.title").to_owned(),
            entries: session
                .catalogue
                .guide
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
        Screen::Dossier { selected } => {
            let entries = dossier_entries(session);
            ScreenView::List {
                title: session.catalogue.strings.ui("ui.dossier.title").to_owned(),
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
                    rh_core::world::FeatureKind::Workstation => '=',
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
    let inventory: Vec<PackItem> = hunter
        .inventory
        .iter()
        .map(|(item, count)| {
            let def = sim.catalogue.items.get(item);
            let name = def
                .map(|def| sim.catalogue.strings.get(&def.name).to_owned())
                .unwrap_or_else(|| item.clone());
            PackItem {
                label: if *count > 1 {
                    format!("{name} x{count}")
                } else {
                    name
                },
                description: def
                    .map(|def| sim.catalogue.strings.get(&def.description).to_owned())
                    .unwrap_or_default(),
            }
        })
        .collect();

    // Each line is tagged with its kind, so the eight event colours are not
    // the only thing saying what a line is: a reader without colour gets the
    // tag, a reader with it gets both. The kind stays in the tuple for the
    // colour a sighted player still sees.
    let log_tail: Vec<(EventKind, String)> = state
        .log
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|event| {
            let tag = sim.catalogue.strings.ui(event_kind_label(event.kind));
            (event.kind, format!("{tag} {}", event.text))
        })
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
        // A confirmation is drawn as an ordinary two-row menu, so every
        // renderer and every input path already knows how to work it.
        Modal::Confirm {
            prompt,
            detail,
            selected,
            ..
        } => OverlayView {
            title: match detail {
                Some(detail) => format!("{prompt} {detail}"),
                None => prompt.clone(),
            },
            items: vec![
                (strings.ui("ui.confirm.yes").to_owned(), None),
                (strings.ui("ui.confirm.no").to_owned(), None),
            ],
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
        in_sight: session.in_sight(),
        hint: session.hint.clone(),
        actions: session.available_actions(),
        log_tail,
        cursor: session.look_point(),
        looking: session.is_looking(),
        inspect: session
            .look_point()
            .and_then(|point| session.inspect(point)),
        legend: map_legend(strings),
        overlay,
    }
}

/// The key to the map's glyphs. A fixed vocabulary drawn from the string
/// table, so a player who cannot lean on colour can still read what each
/// character is. Ordered from the hunter outward, people before things.
fn map_legend(strings: &rh_content::StringTable) -> Vec<LegendEntry> {
    [
        ('@', "ui.legend.hunter"),
        ('W', "ui.legend.villain"),
        ('t', "ui.legend.enemy"),
        ('P', "ui.legend.villager"),
        ('?', "ui.legend.opportunity"),
        ('n', "ui.legend.grave"),
        ('=', "ui.legend.workstation"),
        ('A', "ui.legend.altar"),
        ('^', "ui.legend.snare"),
        ('>', "ui.legend.exit"),
    ]
    .into_iter()
    .map(|(glyph, id)| LegendEntry {
        glyph,
        meaning: strings.ui(id).to_owned(),
    })
    .collect()
}

/// A short, non-colour tag for a log line's kind, so the eight event colours
/// are not the only thing telling a player what a line is about.
pub fn event_kind_label(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Combat => "ui.log.kind.combat",
        EventKind::Telegraph => "ui.log.kind.telegraph",
        EventKind::Clue => "ui.log.kind.clue",
        EventKind::Clock => "ui.log.kind.clock",
        EventKind::Social => "ui.log.kind.social",
        EventKind::Item => "ui.log.kind.item",
        EventKind::Travel => "ui.log.kind.travel",
        EventKind::System => "ui.log.kind.system",
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
        in_sight: Vec::new(),
        hint: None,
        actions: Vec::new(),
        log_tail: Vec::new(),
        cursor: None,
        looking: false,
        inspect: None,
        legend: Vec::new(),
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

/// The case dossier: what is known, what is owed, and what is carried.
///
/// The record answers "what happened"; this answers "where am I". An
/// investigation asks the second question far more often than the first, and
/// before this the only way to answer it was to re-read the whole record.
pub(crate) fn dossier_entries(session: &ClientSession) -> Vec<(String, String)> {
    let Some(run) = session.run.as_ref() else {
        return Vec::new();
    };
    let sim = &run.sim;
    let state = &sim.state;
    let strings = &sim.catalogue.strings;
    let clock = &sim.catalogue.balance.clock;
    let mut entries: Vec<(String, String)> = Vec::new();

    // The quarry: every proof the naming actually turns on, said plainly.
    let mut quarry: Vec<String> = vec![strings.ui_fill(
        "ui.dossier.clock",
        &[
            ("day", &state.clock.min(clock.travel_turns).to_string()),
            ("total", &clock.travel_turns.to_string()),
            ("tier", &state.villain.tier.to_string()),
        ],
    )];
    quarry.push(strings.ui_fill(
        "ui.dossier.quarry.proofs",
        &[("count", &state.identity_clues.len().to_string())],
    ));
    quarry.push(
        strings
            .ui(if state.discriminating_identity.is_empty() {
                "ui.dossier.quarry.discriminating-no"
            } else {
                "ui.dossier.quarry.discriminating-yes"
            })
            .to_owned(),
    );
    quarry.push(
        strings
            .ui(if state.origin_identified {
                "ui.dossier.quarry.origin-known"
            } else {
                "ui.dossier.quarry.origin-unknown"
            })
            .to_owned(),
    );
    quarry.push(
        strings
            .ui(if state.scheme_identified {
                "ui.dossier.quarry.scheme-known"
            } else {
                "ui.dossier.quarry.scheme-unknown"
            })
            .to_owned(),
    );
    quarry.push(
        strings
            .ui(if state.villain_location_known {
                "ui.dossier.quarry.location-known"
            } else {
                "ui.dossier.quarry.location-unknown"
            })
            .to_owned(),
    );
    quarry.push(
        strings
            .ui(if state.villain_uncovered {
                "ui.dossier.quarry.named"
            } else if state.corroboration(&sim.catalogue).corroborated() {
                "ui.dossier.quarry.can-name"
            } else {
                "ui.dossier.quarry.cannot-name"
            })
            .to_owned(),
    );
    entries.push((
        strings.ui("ui.dossier.quarry.title").to_owned(),
        quarry.join("\n"),
    ));

    // Leads outstanding, wherever they are, with the price of each. A lead
    // the hunter cannot currently pay for still shows, with why.
    let mut leads: Vec<String> = Vec::new();
    for opp in &sim.world.opportunities {
        if !state.discovered.contains(&opp.id)
            || state.resolved.contains(&opp.id)
            || state.lost.contains(&opp.id)
        {
            continue;
        }
        let place = &sim.world.map(opp.map).name;
        let lead = opp.lead(strings, rh_core::world::LeadFraming::Act);
        let mut line = strings.ui_fill(
            "ui.dossier.leads.entry",
            &[("name", &lead), ("place", place)],
        );
        if let Some((pool, cost)) =
            rh_core::economy::opportunity_cost(opp.pool, opp.cost, state.settlement_hostile)
        {
            line.push_str(&strings.ui_fill(
                "ui.dossier.leads.cost",
                &[
                    ("cost", &cost.to_string()),
                    ("pool", pool_name(strings, pool)),
                ],
            ));
            if state.hunter.pool(pool) < cost {
                line.push_str(strings.ui("ui.dossier.leads.unaffordable"));
            }
        }
        leads.push(line);
    }
    if leads.is_empty() {
        leads.push(strings.ui("ui.dossier.leads.empty").to_owned());
    }
    entries.push((
        strings.ui("ui.dossier.leads.title").to_owned(),
        leads.join("\n"),
    ));

    // Preparations: whether the counter can be quenched right, and what is
    // actually in the pack that would bite.
    let mut prep: Vec<String> = Vec::new();
    let reagent = &sim.catalogue.origins[&sim.world.villain.origin].counter_reagent;
    if state.origin_identified {
        let reagent_name = sim
            .catalogue
            .items
            .get(reagent)
            .map(|def| strings.get(&def.name).to_owned())
            .unwrap_or_else(|| reagent.clone());
        prep.push(strings.ui_fill(
            "ui.dossier.prep.reagent-known",
            &[("reagent", &reagent_name)],
        ));
        prep.push(
            strings
                .ui(if state.hunter.item_count(reagent) > 0 {
                    "ui.dossier.prep.reagent-held"
                } else {
                    "ui.dossier.prep.reagent-wanted"
                })
                .to_owned(),
        );
    } else {
        prep.push(strings.ui("ui.dossier.prep.reagent-unknown").to_owned());
    }
    let counters: Vec<String> = ["silver-bullet", "cold-iron-pin", "binding-charm"]
        .iter()
        .filter(|item| state.hunter.item_count(item) > 0)
        .map(|item| {
            sim.catalogue
                .items
                .get(*item)
                .map(|def| strings.get(&def.name).to_owned())
                .unwrap_or_else(|| (*item).to_owned())
        })
        .collect();
    prep.push(if counters.is_empty() {
        strings.ui("ui.dossier.prep.nothing").to_owned()
    } else {
        strings.ui_fill(
            "ui.dossier.prep.holding",
            &[("items", &counters.join(", "))],
        )
    });
    prep.push(
        strings
            .ui(if state.church_consecrated {
                "ui.dossier.prep.consecrated"
            } else {
                "ui.dossier.prep.unconsecrated"
            })
            .to_owned(),
    );
    entries.push((
        strings.ui("ui.dossier.prep.title").to_owned(),
        prep.join("\n"),
    ));

    // The pack spelled out. The sidebar has room for names only, so this is
    // where a player finds out what a name is for.
    let mut pack: Vec<String> = Vec::new();
    for (item, count) in &state.hunter.inventory {
        let Some(def) = sim.catalogue.items.get(item) else {
            continue;
        };
        let name = strings.get(&def.name);
        let name = if *count > 1 {
            format!("{name} x{count}")
        } else {
            name.to_owned()
        };
        pack.push(strings.ui_fill(
            "ui.dossier.pack.entry",
            &[("item", &name), ("what", strings.get(&def.description))],
        ));
    }
    if pack.is_empty() {
        pack.push(strings.ui("ui.dossier.pack.empty").to_owned());
    }
    entries.push((
        strings.ui("ui.dossier.pack.title").to_owned(),
        pack.join("\n"),
    ));

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
                    sim.catalogue.strings.ui_fill(
                        "ui.travel.ambush-route",
                        &[
                            ("place", name),
                            ("percent", &sim.world.ambush_percent.to_string()),
                        ],
                    )
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
            tier: String::new(),
            preparations: Vec::new(),
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
    // The routes the generator certified before the world existed, marked
    // step by step against what the hunter actually did. Nothing else in the
    // genre can show the intended solution, because nothing else knows it.
    let strings = &sim.catalogue.strings;
    let routes: Vec<String> = sim
        .world
        .certified_routes
        .iter()
        .map(|route| {
            let steps: Vec<String> = route
                .steps
                .iter()
                .map(|step| {
                    let mark = match step.opportunity() {
                        Some(id) if state.resolved.contains(&id) => {
                            strings.ui("ui.report.step-taken")
                        }
                        Some(_) => strings.ui("ui.report.step-missed"),
                        None => strings.ui("ui.report.step-neutral"),
                    };
                    strings.ui_fill(
                        "ui.report.route-step",
                        &[
                            ("mark", mark),
                            ("turn", &step.turn.to_string()),
                            ("what", &step.description),
                        ],
                    )
                })
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
    // What was carried into the ending, so a defeat says what was short
    // rather than only that it went badly.
    let mut preparations: Vec<String> = Vec::new();
    let reagent = &origin.counter_reagent;
    let reagent_name = sim
        .catalogue
        .items
        .get(reagent)
        .map(|def| strings.get(&def.name).to_owned())
        .unwrap_or_else(|| reagent.clone());
    preparations.push(strings.ui_fill("ui.report.reagent", &[("reagent", &reagent_name)]));
    preparations.push(
        strings
            .ui(if state.origin_identified {
                "ui.report.origin-known"
            } else {
                "ui.report.origin-unknown"
            })
            .to_owned(),
    );
    let weakness = &villain_def.weakness_item;
    preparations.push(
        strings
            .ui_fill(
                if state.hunter.item_count(weakness) > 0 {
                    "ui.report.weakness-held"
                } else {
                    "ui.report.weakness-missing"
                },
                &[(
                    "item",
                    &sim.catalogue
                        .items
                        .get(weakness)
                        .map(|def| strings.get(&def.name).to_owned())
                        .unwrap_or_else(|| weakness.clone()),
                )],
            )
            .to_owned(),
    );
    preparations.push(
        strings
            .ui(if state.church_consecrated {
                "ui.report.consecrated"
            } else {
                "ui.report.unconsecrated"
            })
            .to_owned(),
    );

    CaseReportView {
        outcome,
        villain: strings.ui_fill(
            "ui.report.villain",
            &[
                ("name", strings.get(&villain_def.name)),
                ("title", &sim.world.villain.title),
            ],
        ),
        origin: strings.ui_fill(
            "ui.report.axis",
            &[
                ("name", strings.get(&origin.name)),
                ("description", strings.get(&origin.description)),
            ],
        ),
        scheme: strings.ui_fill(
            "ui.report.axis",
            &[
                ("name", strings.get(&scheme.name)),
                ("description", strings.get(&scheme.description)),
            ],
        ),
        hidden_clues,
        tier: strings.ui_fill(
            "ui.report.tier",
            &[("tier", &state.villain.tier.to_string())],
        ),
        preparations,
        routes,
        share_code: run.share_code(),
    }
}
