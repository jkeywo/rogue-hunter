//! The generated world: static output of the graph-first generator.
//!
//! Everything here is fixed once generation succeeds; the mutable run lives
//! in [`crate::state`]. `rh-gen` constructs this; the simulation interprets
//! it against the content catalogue.

use rh_content::{MapRole, PoolKind, Terrain};
use serde::{Deserialize, Serialize};

use crate::geometry::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MapId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NpcId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OpportunityId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FeatureId(pub u16);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub seed: u64,
    pub villain: VillainSpec,
    /// Indexed by `MapId`. Settlement is always map 0 and the starting map.
    pub maps: Vec<WorldMap>,
    pub npcs: Vec<NpcSpec>,
    pub opportunities: Vec<OpportunitySpec>,
    /// Seed-defined ambush chance on the wilderness/outlying route, percent.
    pub ambush_percent: u8,
    /// Solver-certified routes, kept for the inspector and the case report.
    pub certified_routes: Vec<CertifiedRoute>,
    /// How the run opens.
    pub opening: OpeningSituation,
}

/// The situation a run begins in.
///
/// Most runs open on a generic hook that frames the hunt and banks nothing.
/// The rest open having already resolved one node — `prior` — which is what
/// lets both certified routes lean on it: independence exists so that losing
/// an informant cannot strand a run, and a thing already known before play
/// began cannot be lost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpeningSituation {
    /// Entry id in `openings.toml`.
    pub opening: String,
    /// One condition drawn from every axis. The set is shaped rather than
    /// uniform: exactly one bites, exactly one helps, the rest are texture.
    pub conditions: Vec<String>,
    /// The node already resolved when play begins.
    pub prior: Option<OpportunityId>,
}

impl World {
    pub fn map(&self, id: MapId) -> &WorldMap {
        &self.maps[id.0 as usize]
    }

    pub fn npc(&self, id: NpcId) -> &NpcSpec {
        &self.npcs[id.0 as usize]
    }

    pub fn opportunity(&self, id: OpportunityId) -> &OpportunitySpec {
        &self.opportunities[id.0 as usize]
    }

    pub fn map_by_role(&self, role: MapRole) -> MapId {
        let index = self
            .maps
            .iter()
            .position(|map| map.role == role)
            .unwrap_or_default();
        MapId(index as u8)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VillainSpec {
    /// Villain archetype id in the content catalogue ("werewolf", "revenant").
    pub archetype: String,
    pub origin: String,
    pub scheme: String,
    /// Flavour name for the case report ("the thing that was Wat Snare").
    pub title: String,
    /// The NPC secretly hosting the villain (werewolf concealment).
    pub host: Option<NpcId>,
    /// The grave feature holding the dormant villain (revenant concealment).
    pub grave: Option<(MapId, FeatureId)>,
    /// Where the villain retreats between hunts; final-hunt spawn bias.
    pub lair: (MapId, Point),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldMap {
    /// Content template id ("settlement", "wilderness", "outlying").
    pub template: String,
    pub name: String,
    pub role: MapRole,
    /// Row-major 32x20 terrain grid.
    pub tiles: Vec<Terrain>,
    pub exits: Vec<ExitSpec>,
    pub features: Vec<FeatureSpec>,
    /// Tiles that become warded when the church is consecrated.
    pub consecration_area: Vec<Point>,
    /// Where the hunter appears when arriving without a paired exit
    /// (run start, respawn).
    pub entry: Point,
    /// Baseline enemies present from the start of the run.
    pub initial_enemies: Vec<EnemySpawn>,
}

impl WorldMap {
    pub fn terrain(&self, at: Point) -> Terrain {
        if at.in_bounds() {
            self.tiles[at.y as usize * crate::geometry::MAP_WIDTH as usize + at.x as usize]
        } else {
            Terrain::Wall
        }
    }

    pub fn set_terrain(&mut self, at: Point, terrain: Terrain) {
        if at.in_bounds() {
            self.tiles[at.y as usize * crate::geometry::MAP_WIDTH as usize + at.x as usize] =
                terrain;
        }
    }

    pub fn feature_at(&self, at: Point) -> Option<&FeatureSpec> {
        self.features.iter().find(|feature| feature.at == at)
    }

    pub fn feature(&self, id: FeatureId) -> Option<&FeatureSpec> {
        self.features.iter().find(|feature| feature.id == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitSpec {
    pub at: Point,
    pub to_map: MapId,
    /// Arrival point on the destination map (its paired exit tile).
    pub to_point: Point,
    /// Whether using this exit rolls the seed-defined ambush chance.
    pub ambush_route: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSpec {
    pub id: FeatureId,
    pub at: Point,
    pub kind: FeatureKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureKind {
    Workstation,
    Altar,
    /// A named grave. `contents` is fixed at generation.
    Grave {
        contents: GraveContents,
    },
    /// Where the killing that opened the case happened. The scheme's minions
    /// gather here, so this is structure rather than dressing and must be
    /// identified by kind -- never by matching the landmark's display name.
    KillSite,
    /// Landmark with no intrinsic interaction (den, stones, well, camp).
    Landmark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraveContents {
    Empty,
    Mundane,
    /// The dormant villain rests here (revenant runs only).
    Villain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcSpec {
    pub id: NpcId,
    /// Archetype id in the content catalogue.
    pub archetype: String,
    pub name: String,
    pub glyph: char,
    pub disposition: Disposition,
    pub mystical: bool,
    /// Whether this NPC will trade ammunition for coin.
    pub trades: bool,
    pub secret: NpcSecret,
    /// Relationship links to other cast members.
    pub links: Vec<NpcLink>,
    /// Map and tile where the NPC works (routines orbit this point).
    pub map: MapId,
    pub work: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disposition {
    Friendly,
    Wary,
    Hostile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcSecret {
    /// Secret template id.
    pub template: String,
    pub text: String,
    /// False secrets carry their reachable disproof text.
    pub disproof: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcLink {
    pub to: NpcId,
    /// Relationship kind id.
    pub kind: String,
    pub discovered_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnemySpawn {
    /// Enemy id in the content catalogue.
    pub enemy: String,
    pub at: Point,
}

/// A generated, placed opportunity. Always visible once discovered; if its
/// pool is empty the UI explains the blocked action instead of hiding it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunitySpec {
    pub id: OpportunityId,
    /// Clue template id, gather kind, or intrinsic kind for the inspector.
    pub source: String,
    pub name: String,
    pub map: MapId,
    pub anchor: OpportunityAnchor,
    /// Pool the action draws from; `None` means the action is free.
    pub pool: Option<PoolKind>,
    pub cost: u8,
    pub obscurity: u8,
    pub discovery: DiscoveryRule,
    pub grants: OpportunityGrant,
    /// Physical-access prerequisite the planner must schedule first
    /// (e.g. forcing the crypt door). The sim enforces this spatially.
    pub requires: Option<OpportunityId>,
    /// Resolving this clears the anchored tile's terrain (forced door/rubble).
    pub clears_terrain: bool,
    /// Covert actions (spying, examining, tracking) need no cooperation
    /// from a hostile NPC anchor; conversational actions do.
    pub covert: bool,
    pub prompt: String,
    pub reveal: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpportunityAnchor {
    /// Interact while standing on or adjacent to this tile.
    Tile(Point),
    /// Interact by talking to this NPC.
    Npc(NpcId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryRule {
    /// Discovered when its tile (or NPC) first enters the hunter's sight.
    Sight,
    /// Only appears once another opportunity has been resolved.
    RevealedBy(OpportunityId),
    /// Discovered by sight, or revealed early by the referenced opportunity.
    SightOr(OpportunityId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpportunityGrant {
    /// A corroborating identity proof. `discriminating` marks evidence that
    /// positively eliminates at least one other villain; naming the quarry
    /// needs two proofs of which at least one discriminates.
    IdentityClue { discriminating: bool },
    /// Evidence about how the evil began. Discriminating signs pin the origin,
    /// which decides the reagent every counter must be quenched with.
    OriginSign { discriminating: bool },
    /// Evidence about what the evil is working toward. Discriminating signs
    /// pin the scheme, which decides what can be pre-empted.
    SchemeSign { discriminating: bool },
    /// Blunt the scheme's escalation, if performed before its major event.
    SchemePreempt,
    /// Reveals the villain's lair/grave marker without naming it.
    LocationClue,
    /// Knowledge that unlocks other opportunities (weakness sources).
    Lead,
    /// Items granted directly.
    Items { items: Vec<String> },
    /// The mystical favour: one temporary over-cap Mystic point.
    MysticFavour,
    /// Learn an undiscovered relationship link of the anchored NPC.
    RelationshipInfo,
    /// Learn the anchored NPC's secret (spying route).
    SecretInfo,
    /// Expose the anchored NPC's secret: leverage that makes them cooperate.
    Leverage,
    /// Disproves the referenced NPC's false secret (anchored elsewhere).
    Disproof { npc: NpcId },
}

/// Solver-certified route, retained for the inspector and case report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertifiedRoute {
    pub label: String,
    /// Global turn by which the route is hunt-ready.
    pub ready_by_turn: u8,
    pub viability_permille: u16,
    pub total_effort: u16,
    pub total_obscurity: u16,
    pub travel_legs: u8,
    pub uses_mystic_favour: bool,
    pub steps: Vec<RouteStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStep {
    /// Global turn on which the step happens.
    pub turn: u8,
    /// What the step actually is. The autoplayer reads this; `description` is
    /// for the player and nothing may branch on it.
    pub action: RouteAction,
    pub description: String,
}

/// What a certified route step does.
///
/// The planner knows this exactly, so it says so rather than writing prose and
/// leaving the autoplayer to parse it back out. Prose is display, and display
/// gets rewritten and translated; a plan that is read by code has to be data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteAction {
    Resolve(OpportunityId),
    Craft {
        recipe: String,
    },
    Consecrate,
    Travel(MapId),
    /// The synthetic final step: everything is ready, go and hunt.
    InitiateHunt,
}

impl RouteStep {
    /// The opportunity this step resolves, if it resolves one.
    pub fn opportunity(&self) -> Option<OpportunityId> {
        match self.action {
            RouteAction::Resolve(id) => Some(id),
            _ => None,
        }
    }
}

/// Normalised link key so `(a, b)` and `(b, a)` collide.
pub fn link_key(a: NpcId, b: NpcId) -> (NpcId, NpcId) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}
