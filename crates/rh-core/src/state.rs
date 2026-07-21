//! Mutable authoritative run state.
//!
//! The headless simulation alone owns this: generated world state overlays,
//! global clock, local tactical turn, actors, resources, discoveries,
//! inventory, event log. Everything serializes so state digests can compare
//! runs across native and WASM builds.

use std::collections::{BTreeMap, BTreeSet};

use rh_content::{Catalogue, PoolKind};
use serde::{Deserialize, Serialize};

use crate::events::LogEvent;
use crate::geometry::Point;
use crate::rng::SimRng;
use crate::world::{FeatureId, MapId, NpcId, OpportunityId, World};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActorId(pub u16);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub rng: SimRng,
    /// Global travel turns spent (0-based; final hunt begins at travel_turns).
    pub clock: u8,
    pub final_hunt: bool,
    /// Times the hunter has fallen short of the final hunt. Each one cost a
    /// day and nothing else, so it never reaches the outcome — but a run lost
    /// to the clock after three deaths was lost somewhere quite different
    /// from one lost in the fight, and only this tells them apart.
    pub deaths: u8,
    /// Local encounter turn counter (increments after every hunter action).
    pub local_turn: u32,
    pub current_map: MapId,
    pub hunter: HunterState,
    pub actors: Vec<Actor>,
    next_actor_id: u16,
    pub npcs: Vec<NpcState>,
    pub villain: VillainRunState,
    /// Terrain changed at runtime (forced doors, shifted rubble).
    pub terrain_overrides: BTreeMap<(MapId, Point), rh_content::Terrain>,
    pub discovered: BTreeSet<OpportunityId>,
    pub resolved: BTreeSet<OpportunityId>,
    /// Opportunities lost to fallout (their NPC was attacked or killed).
    pub lost: BTreeSet<OpportunityId>,
    pub identity_clues: BTreeSet<OpportunityId>,
    /// Identity clues that positively eliminate an alternative villain. Naming
    /// the quarry needs at least one of these, so corroboration cannot be
    /// assembled purely from ambiguous signs.
    pub discriminating_identity: BTreeSet<OpportunityId>,
    /// The origin has been pinned by a discriminating sign, so the hunter
    /// knows which reagent their counter must be quenched with.
    pub origin_identified: bool,
    /// The scheme has been pinned by a discriminating sign.
    pub scheme_identified: bool,
    /// The scheme's escalation was blunted before its major event fired.
    pub scheme_preempted: bool,
    pub villain_uncovered: bool,
    pub villain_location_known: bool,
    pub met_npcs: BTreeSet<NpcId>,
    pub known_links: BTreeSet<(NpcId, NpcId)>,
    pub known_secrets: BTreeSet<NpcId>,
    pub disproved_secrets: BTreeSet<NpcId>,
    /// Per-map fog of war: tiles ever seen.
    pub seen: Vec<Vec<bool>>,
    /// Tiles currently visible on the current map.
    pub visible: Vec<bool>,
    pub church_consecrated: bool,
    pub settlement_hostile: bool,
    /// Tiles taken off the hunter's field of view by the run's condition.
    pub sight_penalty: u8,
    /// Tiles added to the hunter's field of view by the run's boon.
    pub sight_bonus: u8,
    /// Maps the hunter has set foot on, for first-arrival narration.
    pub arrived: BTreeSet<MapId>,
    /// How far into each map's optional-event deck the run has drawn.
    pub event_cursor: Vec<u8>,
    pub opened_graves: BTreeSet<FeatureId>,
    pub snares: Vec<Snare>,
    pub wards: Vec<GroundWard>,
    pub log: Vec<LogEvent>,
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunterState {
    pub pos: Point,
    pub hp: u16,
    pub max_hp: u16,
    pub lore: u8,
    pub social: u8,
    pub mystic: u8,
    /// Temporary over-cap Mystic from the mystical favour.
    pub mystic_bonus: u8,
    pub physical: u8,
    pub stamina: u8,
    pub inventory: BTreeMap<String, u16>,
    /// Next ranged attack always hits (Aim).
    pub sure_shot: bool,
    /// Pending melee multiplier numerator over 2 (3 = x1.5 from Power Attack).
    pub melee_multiplier: Option<u8>,
    pub favour_used: bool,
    /// Turns a called-in villager still stands with the hunter (the Confessor's
    /// second). While it lasts, her blows land harder and some of what comes
    /// back is taken by the one beside her. A buff on the hunter rather than an
    /// actor on the board: an ally actor would touch targeting, occupancy, and
    /// the digest, where a second she has earned is one number that counts down.
    pub second_turns: u8,
    /// Extra melee damage the standing second adds each turn it is here.
    pub second_damage: u16,
}

impl HunterState {
    pub fn pool(&self, kind: PoolKind) -> u8 {
        match kind {
            PoolKind::Lore => self.lore,
            PoolKind::Social => self.social,
            PoolKind::Mystic => self.mystic + self.mystic_bonus,
            PoolKind::Physical => self.physical,
        }
    }

    pub fn spend_pool(&mut self, kind: PoolKind, amount: u8) {
        match kind {
            PoolKind::Lore => self.lore = self.lore.saturating_sub(amount),
            PoolKind::Social => self.social = self.social.saturating_sub(amount),
            PoolKind::Mystic => {
                // Spend the temporary favour point first: it is over-cap and
                // would otherwise be lost to the cap on the next refresh.
                let from_bonus = amount.min(self.mystic_bonus);
                self.mystic_bonus -= from_bonus;
                self.mystic = self.mystic.saturating_sub(amount - from_bonus);
            }
            PoolKind::Physical => self.physical = self.physical.saturating_sub(amount),
        }
    }

    pub fn item_count(&self, item: &str) -> u16 {
        self.inventory.get(item).copied().unwrap_or(0)
    }

    pub fn add_item(&mut self, item: &str, count: u16) {
        *self.inventory.entry(item.to_owned()).or_insert(0) += count;
    }

    /// Remove `count` of `item`; returns false (unchanged) if short.
    pub fn remove_item(&mut self, item: &str, count: u16) -> bool {
        match self.inventory.get_mut(item) {
            Some(have) if *have >= count => {
                *have -= count;
                if *have == 0 {
                    self.inventory.remove(item);
                }
                true
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub id: ActorId,
    pub kind: ActorKind,
    pub map: MapId,
    pub pos: Point,
    pub hp: u16,
    pub max_hp: u16,
    /// Has noticed the hunter and acts every tick.
    pub awake: bool,
    /// Encounter turns remaining held in a snare.
    pub trapped: u8,
    /// Turns until the pounce/dash is ready again.
    pub pounce_cooldown: u8,
    /// Pounce telegraphed last turn and will fire if the lane holds.
    pub pounce_primed: bool,
    pub dash_cooldown: u8,
    /// Position in the revenant vulnerability cadence.
    pub cadence: u8,
    /// Forced-vulnerability turns remaining (binding charm).
    pub bound: u8,
    /// Werewolf regeneration permanently stopped by silver.
    pub regen_stopped: bool,
    /// Hex-ward charges still standing (the Witch); 0 means the ward is down.
    pub ward_charges: u8,
    /// Encounter turns until a broken ward is rewoven.
    pub ward_reweave: u8,
    /// Shamblers move only when this is true; toggles every tick.
    pub slow_phase: bool,
    /// Dormant countdown: >0 means immobile in the grave; wakes when it
    /// reaches 0 or when attacked (which grants the coup-de-grace bonus).
    pub dormant: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActorKind {
    /// Ordinary enemy; the string is the content enemy id.
    Enemy(String),
    Villain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcState {
    pub pos: Point,
    pub alive: bool,
    /// Fled after being attacked; no longer on the map.
    pub fled: bool,
    /// Attacked by the hunter (fallout applied).
    pub attacked: bool,
    /// Exposed secret leverage: treats disposition as friendly.
    pub leveraged: bool,
    pub hp: u16,
    /// Local turns until this NPC seeks out a linked NPC for a chat.
    pub chat_timer: u8,
    /// The link currently being walked to, as an index into spec links.
    pub chat_target: Option<u8>,
    /// Turns of chatting remaining once adjacent.
    pub chatting: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VillainRunState {
    /// Threat tier 0..=2; each scheme event raises it.
    pub tier: u8,
    /// The villain is materialised as an actor and hunting.
    pub active: bool,
    /// Actor id when active.
    pub actor: Option<ActorId>,
    /// Killed: run won.
    pub dead: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snare {
    pub map: MapId,
    pub at: Point,
}

/// Ground the Occultist has marked. Unnatural things are torn at for
/// crossing it, which turns a corridor or doorway into a lane she owns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundWard {
    pub map: MapId,
    pub centre: Point,
    pub radius: u8,
    pub turns_left: u8,
}

impl GroundWard {
    pub fn covers(&self, map: MapId, at: Point) -> bool {
        self.map == map && self.centre.distance(at) <= i16::from(self.radius)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// The villain is destroyed.
    Victory,
    /// The hunter fell during the final hunt.
    Defeat,
}

/// The case's standing against the corroboration rule, in one place for
/// the sim's gate, the autoplayer, the clients, and the planner's goal:
/// naming the quarry demands `need` identity proofs of which at least one
/// is discriminating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corroboration {
    /// Identity proofs held.
    pub have: u8,
    /// Proofs naming the quarry demands (authored tuning).
    pub need: u8,
    /// Whether any held proof rules an alternative out.
    pub decisive: bool,
}

impl Corroboration {
    /// Whether the quarry can be named right now.
    pub fn met(&self) -> bool {
        self.have >= self.need && self.decisive
    }

    /// Whether enough proofs are held, decisive or not — the corroborated-
    /// but-ambiguous state the UI narrates differently from "keep looking".
    pub fn corroborated(&self) -> bool {
        self.have >= self.need
    }
}

impl RunState {
    /// Where the case stands against the corroboration rule.
    pub fn corroboration(&self, catalogue: &Catalogue) -> Corroboration {
        Corroboration {
            have: self.identity_clues.len() as u8,
            need: catalogue.balance.case.corroborating_proofs,
            decisive: !self.discriminating_identity.is_empty(),
        }
    }

    /// Fresh state for a generated world. The RNG continues from wherever
    /// generation left it: one stream drives generation and runtime.
    pub fn new(world: &World, catalogue: &Catalogue, rng: SimRng) -> Self {
        let hunter_def = &catalogue.hunter;
        let mut inventory = BTreeMap::new();
        for item in &hunter_def.starting_items {
            *inventory.entry(item.clone()).or_insert(0) += 1;
        }
        let settlement = world.map_by_role(rh_content::MapRole::Settlement);
        let entry = world.map(settlement).entry;

        let mut state = Self {
            rng,
            clock: 0,
            final_hunt: false,
            deaths: 0,
            local_turn: 0,
            current_map: settlement,
            hunter: HunterState {
                pos: entry,
                hp: hunter_def.health,
                max_hp: hunter_def.health,
                lore: hunter_def.lore_cap,
                social: hunter_def.social_cap,
                mystic: hunter_def.mystic_cap,
                mystic_bonus: 0,
                physical: hunter_def.physical_cap,
                stamina: hunter_def.stamina_cap,
                inventory,
                sure_shot: false,
                melee_multiplier: None,
                favour_used: false,
                second_turns: 0,
                second_damage: 0,
            },
            actors: Vec::new(),
            next_actor_id: 0,
            npcs: world
                .npcs
                .iter()
                .map(|spec| NpcState {
                    pos: spec.work,
                    alive: true,
                    fled: false,
                    attacked: false,
                    leveraged: false,
                    hp: 2,
                    chat_timer: 6,
                    chat_target: None,
                    chatting: 0,
                })
                .collect(),
            villain: VillainRunState {
                tier: 0,
                active: false,
                actor: None,
                dead: false,
            },
            terrain_overrides: BTreeMap::new(),
            discovered: BTreeSet::new(),
            resolved: BTreeSet::new(),
            lost: BTreeSet::new(),
            identity_clues: BTreeSet::new(),
            discriminating_identity: BTreeSet::new(),
            origin_identified: false,
            scheme_identified: false,
            scheme_preempted: false,
            villain_uncovered: false,
            villain_location_known: false,
            met_npcs: BTreeSet::new(),
            known_links: BTreeSet::new(),
            known_secrets: BTreeSet::new(),
            disproved_secrets: BTreeSet::new(),
            seen: world
                .maps
                .iter()
                .map(|_| {
                    vec![
                        false;
                        crate::geometry::MAP_WIDTH as usize * crate::geometry::MAP_HEIGHT as usize
                    ]
                })
                .collect(),
            visible: vec![
                false;
                crate::geometry::MAP_WIDTH as usize * crate::geometry::MAP_HEIGHT as usize
            ],
            church_consecrated: false,
            settlement_hostile: false,
            sight_penalty: 0,
            sight_bonus: 0,
            arrived: BTreeSet::new(),
            event_cursor: vec![0; world.maps.len()],
            opened_graves: BTreeSet::new(),
            snares: Vec::new(),
            wards: Vec::new(),
            log: Vec::new(),
            outcome: None,
        };

        // Baseline enemies from the world description.
        for (index, map) in world.maps.iter().enumerate() {
            for spawn in &map.initial_enemies {
                let hp = catalogue
                    .enemies
                    .get(&spawn.enemy)
                    .map(|def| def.health)
                    .unwrap_or(1);
                state.spawn_actor(
                    ActorKind::Enemy(spawn.enemy.clone()),
                    MapId(index as u8),
                    spawn.at,
                    hp,
                );
            }
        }
        state
    }

    pub fn spawn_actor(&mut self, kind: ActorKind, map: MapId, pos: Point, hp: u16) -> ActorId {
        let id = ActorId(self.next_actor_id);
        self.next_actor_id += 1;
        self.actors.push(Actor {
            id,
            kind,
            map,
            pos,
            hp,
            max_hp: hp,
            awake: false,
            trapped: 0,
            pounce_cooldown: 0,
            pounce_primed: false,
            dash_cooldown: 0,
            cadence: 0,
            bound: 0,
            regen_stopped: false,
            ward_charges: 0,
            ward_reweave: 0,
            slow_phase: false,
            dormant: 0,
        });
        id
    }

    pub fn actor(&self, id: ActorId) -> Option<&Actor> {
        self.actors.iter().find(|actor| actor.id == id)
    }

    pub fn actor_mut(&mut self, id: ActorId) -> Option<&mut Actor> {
        self.actors.iter_mut().find(|actor| actor.id == id)
    }

    /// Living hostile actor occupying a tile on the current map.
    pub fn actor_at(&self, map: MapId, pos: Point) -> Option<&Actor> {
        self.actors
            .iter()
            .find(|actor| actor.map == map && actor.pos == pos && actor.hp > 0)
    }

    /// Living, present NPC occupying a tile on the current map.
    pub fn npc_at(&self, world: &World, map: MapId, pos: Point) -> Option<NpcId> {
        world
            .npcs
            .iter()
            .zip(self.npcs.iter())
            .find_map(|(spec, npc)| {
                (spec.map == map && npc.alive && !npc.fled && npc.pos == pos).then_some(spec.id)
            })
    }

    pub fn tile_occupied(&self, world: &World, map: MapId, pos: Point) -> bool {
        self.actor_at(map, pos).is_some()
            || self.npc_at(world, map, pos).is_some()
            || (map == self.current_map && self.hunter.pos == pos)
    }

    /// Effective terrain including runtime overrides.
    pub fn terrain(&self, world: &World, map: MapId, at: Point) -> rh_content::Terrain {
        self.terrain_overrides
            .get(&(map, at))
            .copied()
            .unwrap_or_else(|| world.map(map).terrain(at))
    }

    pub fn seen_index(at: Point) -> usize {
        at.y as usize * crate::geometry::MAP_WIDTH as usize + at.x as usize
    }

    pub fn is_seen(&self, map: MapId, at: Point) -> bool {
        at.in_bounds() && self.seen[map.0 as usize][Self::seen_index(at)]
    }

    pub fn is_visible(&self, at: Point) -> bool {
        at.in_bounds() && self.visible[Self::seen_index(at)]
    }
}
