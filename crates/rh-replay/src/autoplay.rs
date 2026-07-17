//! The route-following autoplayer.
//!
//! Drives a run headlessly: follows the certified early route, uncovers the
//! villain, initiates the hunt, and fights with the same preparations the
//! viability heuristic assumed. Used to mint golden replays, exercise the
//! full command surface in CI, and prove generated runs are playable from
//! start to finish. Fully deterministic for a given seed.

use std::collections::VecDeque;

use rh_core::command::{Command, Target};
use rh_core::fov::is_walkable;
use rh_core::geometry::{Direction, Point, MAP_HEIGHT, MAP_WIDTH};
use rh_core::state::{ActorKind, Outcome};
use rh_core::world::{FeatureKind, MapId, OpportunityAnchor, OpportunityId, RouteStep};

use crate::RunSession;

/// Safety cap: a run that needs more actions than this has stalled.
const MAX_ACTIONS: u32 = 4000;
/// Consecutive no-progress actions before giving up.
const MAX_STALLS: u32 = 60;

/// Drive the session to an outcome. Returns `None` if the bot stalled.
pub fn autoplay(session: &mut RunSession) -> Option<Outcome> {
    let steps: Vec<RouteStep> = session
        .sim
        .world
        .certified_routes
        .first()
        .map(|route| route.steps.clone())
        .unwrap_or_default();
    let mut bot = Bot {
        steps,
        step_index: 0,
        stalls: 0,
        step_actions: 0,
    };
    for _ in 0..MAX_ACTIONS {
        if let Some(outcome) = session.outcome() {
            return Some(outcome);
        }
        if bot.stalls >= MAX_STALLS {
            return None;
        }
        let step_before = bot.step_index;
        let progress_before = (
            session.sim.state.clock,
            session.sim.state.current_map,
            session.sim.state.hunter.pos,
            session.sim.state.resolved.len(),
        );
        bot.act(session);
        // A stuck route step gets skipped rather than stalling the run.
        if bot.step_index == step_before && bot.step_index < bot.steps.len() {
            bot.step_actions += 1;
            if bot.step_actions > STEP_BUDGET {
                bot.step_index += 1;
                bot.step_actions = 0;
            }
        } else {
            bot.step_actions = 0;
        }
        let progressed = progress_before
            != (
                session.sim.state.clock,
                session.sim.state.current_map,
                session.sim.state.hunter.pos,
                session.sim.state.resolved.len(),
            )
            || session.sim.state.villain.active;
        if progressed {
            bot.stalls = 0;
        } else {
            bot.stalls += 1;
        }
    }
    session.outcome()
}

struct Bot {
    steps: Vec<RouteStep>,
    step_index: usize,
    stalls: u32,
    /// Actions spent on the current route step; stuck steps get skipped.
    step_actions: u32,
}

/// Actions allowed per route step before the bot skips it.
const STEP_BUDGET: u32 = 150;

impl Bot {
    fn act(&mut self, session: &mut RunSession) {
        // Free knowledge action first: name the villain once proven.
        if session.sim.state.identity_clues.len() >= 2 && !session.sim.state.villain_uncovered
            && session.apply(Command::UncoverVillain).is_ok() {
                return;
            }

        // Emergency healing beats everything else; with draughts to spare,
        // top up after incidental scrapes too.
        let hunter_hp = session.sim.state.hunter.hp;
        let draughts = session.sim.state.hunter.item_count("wound-draught");
        if ((hunter_hp <= 5 && draughts > 0) || (hunter_hp <= 8 && draughts >= 2))
            && session.apply(Command::UseDraught).is_ok() {
                return;
            }

        // A live villain on our map is the fight we came for.
        if let Some(actor_id) = session.sim.state.villain.actor {
            let on_map = session
                .sim
                .state
                .actor(actor_id)
                .map(|actor| actor.map == session.sim.state.current_map && actor.hp > 0)
                .unwrap_or(false);
            if on_map {
                self.fight_villain(session, actor_id);
                return;
            }
        }

        // Ordinary enemies that have our scent get put down before they whittle us.
        let map = session.sim.state.current_map;
        let hunter = session.sim.state.hunter.pos;
        let threat = session
            .sim
            .state
            .actors
            .iter()
            .filter(|actor| {
                actor.map == map
                    && actor.hp > 0
                    && actor.awake
                    && actor.kind != ActorKind::Villain
                    && actor.pos.distance(hunter) <= 6
            })
            .min_by_key(|actor| (actor.pos.distance(hunter), actor.id.0))
            .map(|actor| (actor.id, actor.pos));
        if let Some((enemy_id, enemy_pos)) = threat {
            if hunter.is_adjacent(enemy_pos) {
                let _ = session.apply(Command::Melee(Target::Actor(enemy_id)));
                return;
            }
            // Answer ranged harassment in kind, keeping one shot in reserve
            // to cast the silver bullet around when the beast demands it.
            let villain_def = session.sim.villain_def();
            let needs_reserve = villain_def.regeneration.is_some()
                && session.sim.state.hunter.item_count("silver-bullet") == 0;
            let reserve = u16::from(needs_reserve);
            if session.sim.state.hunter.item_count("flintlock-shot") > reserve
                && session
                    .apply(Command::Ranged {
                        target: Target::Actor(enemy_id),
                        silver: false,
                    })
                    .is_ok()
            {
                return;
            }
            self.walk_toward(session, enemy_pos, true);
            return;
        }

        // Otherwise: work the certified route, then initiate the hunt.
        if self.step_index < self.steps.len() {
            self.execute_step(session);
        } else {
            self.initiate_hunt(session);
        }
    }

    fn execute_step(&mut self, session: &mut RunSession) {
        let step = self.steps[self.step_index].clone();
        // The final synthetic step is "initiate the hunt".
        if step.opportunity.is_none() && step.description.starts_with("Initiate") {
            self.step_index += 1;
            return;
        }
        if let Some(opp_id) = step.opportunity {
            if session.sim.state.resolved.contains(&opp_id) {
                self.step_index += 1;
                return;
            }
            self.resolve_opportunity(session, opp_id);
            return;
        }
        let description = step.description.as_str();
        if let Some(map_name) = description.strip_prefix("Travel to ") {
            let destination = session
                .sim
                .world
                .maps
                .iter()
                .position(|map| map.name == map_name)
                .map(|index| MapId(index as u8));
            match destination {
                Some(destination) if destination == session.sim.state.current_map => {
                    self.step_index += 1;
                }
                Some(destination) => self.travel_toward(session, destination),
                None => self.step_index += 1,
            }
            return;
        }
        if description.starts_with("Craft: ") {
            let recipe = session
                .sim
                .catalogue
                .recipes
                .iter()
                .find(|(_, def)| description.ends_with(def.name.as_str()))
                .map(|(id, _)| id.clone());
            match recipe {
                Some(recipe) => {
                    if self.goto_feature(session, |kind| matches!(kind, FeatureKind::Workstation)) {
                        if session.apply(Command::Craft { recipe }).is_ok() {
                            self.step_index += 1;
                        } else {
                            // Missing ingredients here means the plan drifted;
                            // skip rather than loop forever.
                            self.step_index += 1;
                        }
                    }
                }
                None => self.step_index += 1,
            }
            return;
        }
        if description.starts_with("Perform the consecration") {
            if self.goto_feature(session, |kind| matches!(kind, FeatureKind::Altar)) {
                let _ = session.apply(Command::Consecrate);
                self.step_index += 1;
            }
            return;
        }
        // Unknown step kinds are skipped.
        self.step_index += 1;
    }

    fn resolve_opportunity(&mut self, session: &mut RunSession, opp_id: OpportunityId) {
        let spec = session.sim.world.opportunity(opp_id).clone();
        if spec.map != session.sim.state.current_map {
            // The route's travel step should have got us here; walk there anyway.
            self.travel_toward(session, spec.map);
            return;
        }
        let target = match spec.anchor {
            OpportunityAnchor::Tile(at) => at,
            OpportunityAnchor::Npc(npc) => session.sim.state.npcs[npc.0 as usize].pos,
        };
        let hunter = session.sim.state.hunter.pos;
        let in_range = match spec.anchor {
            OpportunityAnchor::Tile(at) => hunter == at || hunter.is_adjacent(at),
            OpportunityAnchor::Npc(_) => hunter.is_adjacent(target),
        };
        if in_range {
            match session.apply(Command::Interact(opp_id)) {
                Ok(()) => self.step_index += 1,
                Err(_) => {
                    // Not discovered yet (needs a closer look) or blocked:
                    // waiting a turn lets discovery and NPC movement settle.
                    let _ = session.apply(Command::Wait);
                }
            }
        } else {
            self.walk_toward(session, target, true);
        }
    }

    fn initiate_hunt(&mut self, session: &mut RunSession) {
        if !session.sim.state.villain_uncovered {
            // Not proven yet; wait for the world to turn (final hunt comes).
            let _ = session.apply(Command::Wait);
            return;
        }
        // Walk into the hunt healthy: drink first if a draught is spare.
        let hunter = &session.sim.state.hunter;
        if hunter.hp + 4 <= hunter.max_hp && hunter.item_count("wound-draught") > 0
            && session.apply(Command::UseDraught).is_ok() {
                return;
            }
        let villain_active = session.sim.state.villain.active;
        if villain_active {
            // Villain is somewhere else; head to its map.
            if let Some(actor_id) = session.sim.state.villain.actor {
                if let Some(actor) = session.sim.state.actor(actor_id) {
                    let map = actor.map;
                    if map != session.sim.state.current_map {
                        self.travel_toward(session, map);
                        return;
                    }
                }
            }
            let _ = session.apply(Command::Wait);
            return;
        }
        // Werewolf: confront the host — ideally with an aimed silver shot
        // from range, so the fight opens with the curse already bleeding.
        if let Some(host) = session.sim.world.villain.host {
            let host_map = session.sim.world.npc(host).map;
            if session.sim.state.current_map != host_map {
                self.travel_toward(session, host_map);
                return;
            }
            let npc_state = &session.sim.state.npcs[host.0 as usize];
            if !npc_state.alive || npc_state.fled {
                let _ = session.apply(Command::Wait);
                return;
            }
            let pos = npc_state.pos;
            let hunter = session.sim.state.hunter.pos;
            let distance = hunter.distance(pos);
            let has_silver = session.sim.state.hunter.item_count("silver-bullet") > 0;
            // Open from range: back off before the aimed silver shot rather
            // than starting the fight inside the beast's reach.
            if has_silver && distance < 2
                && self.step_away(session, pos) {
                    return;
                }
            if has_silver && (2..=6).contains(&distance) {
                if !session.sim.state.hunter.sure_shot && session.sim.state.hunter.stamina >= 2
                    && session
                        .apply(Command::Manoeuvre {
                            id: "aim".to_owned(),
                            steps: Vec::new(),
                        })
                        .is_ok()
                    {
                        return;
                    }
                if session
                    .apply(Command::Ranged {
                        target: Target::Npc(host),
                        silver: true,
                    })
                    .is_ok()
                {
                    return;
                }
            }
            if session.sim.state.hunter.pos.is_adjacent(pos) {
                let _ = session.apply(Command::Melee(Target::Npc(host)));
            } else {
                self.walk_toward(session, pos, true);
            }
            return;
        }
        if let Some((grave_map, feature_id)) = session.sim.world.villain.grave {
            if session.sim.state.current_map != grave_map {
                self.travel_toward(session, grave_map);
                return;
            }
            let at = session
                .sim
                .world
                .map(grave_map)
                .feature(feature_id)
                .map(|feature| feature.at);
            let Some(at) = at else {
                let _ = session.apply(Command::Wait);
                return;
            };
            let hunter = session.sim.state.hunter.pos;
            if hunter == at || hunter.is_adjacent(at) {
                if session.apply(Command::OpenGrave(feature_id)).is_err() {
                    let _ = session.apply(Command::Wait);
                }
            } else {
                self.walk_toward(session, at, true);
            }
        } else {
            let _ = session.apply(Command::Wait);
        }
    }

    fn fight_villain(&mut self, session: &mut RunSession, actor_id: rh_core::state::ActorId) {
        let Some(actor) = session.sim.state.actor(actor_id) else {
            let _ = session.apply(Command::Wait);
            return;
        };
        let villain_pos = actor.pos;
        let dormant = actor.dormant > 0;
        let bound = actor.bound > 0;
        let hunter = session.sim.state.hunter.pos;
        let adjacent = hunter.is_adjacent(villain_pos);
        let state = &session.sim.state;
        let def = session.sim.villain_def().clone();
        let vulnerable = session.sim.villain_is_vulnerable(actor_id);
        let stamina = state.hunter.stamina;
        let physical = state.hunter.physical;
        let has_charm = state.hunter.item_count("binding-charm") > 0;
        let has_silver = state.hunter.item_count("silver-bullet") > 0;
        let sure_shot = state.hunter.sure_shot;
        let power_primed = state.hunter.melee_multiplier.is_some();
        let regen_stopped = state.sim_actor_regen_stopped(actor_id).unwrap_or(false);

        // Silver first against a regenerating villain: aim, then the sure shot.
        if has_silver && def.regeneration.is_some() && !regen_stopped {
            if !sure_shot && stamina >= 2
                && session
                    .apply(Command::Manoeuvre {
                        id: "aim".to_owned(),
                        steps: Vec::new(),
                    })
                    .is_ok()
                {
                    return;
                }
            if session
                .apply(Command::Ranged {
                    target: Target::Actor(actor_id),
                    silver: true,
                })
                .is_ok()
            {
                return;
            }
        }

        // Bind a shrouded revenant the moment we stand beside it.
        if adjacent && has_charm && def.cadence.is_some() && !vulnerable && !bound && !dormant
            && session
                .apply(Command::UseBindingCharm {
                    target: Target::Actor(actor_id),
                })
                .is_ok()
            {
                return;
            }

        // Against a shrouded revenant with the church warded and no charm,
        // the winning ground is the ward: retreat there and let it follow.
        let settlement = session
            .sim
            .world
            .map_by_role(rh_content::MapRole::Settlement);
        let ward_fight = def.cadence.is_some()
            && !has_charm
            && !dormant
            && session.sim.state.church_consecrated
            && session.sim.state.current_map == settlement;
        if ward_fight {
            let area = session.sim.world.map(settlement).consecration_area.clone();
            let hunter_on_ward = area.contains(&hunter);
            if !hunter_on_ward {
                // Head for ward ground deep enough that the fight stays on it.
                let target = area
                    .iter()
                    .copied()
                    .filter(|point| {
                        !session
                            .sim
                            .state
                            .tile_occupied(&session.sim.world, settlement, *point)
                    })
                    .max_by_key(|point| {
                        area.iter()
                            .filter(|other| point.is_adjacent(**other))
                            .count()
                    });
                if let Some(target) = target {
                    self.walk_toward(session, target, false);
                    return;
                }
            } else if !adjacent {
                // On the ward: stand fast, the revenant will come to us.
                let _ = session.apply(Command::Wait);
                return;
            }
        }

        // A snared villain is free damage: pour shot into it from range.
        let trapped_now = session
            .sim
            .state
            .actor(actor_id)
            .map(|actor| actor.trapped > 0)
            .unwrap_or(false);
        if trapped_now
            && !adjacent
            && (def.cadence.is_none() || vulnerable)
            && session.sim.state.hunter.item_count("flintlock-shot") > 0
            && hunter.distance(villain_pos) <= 6
            && session
                .apply(Command::Ranged {
                    target: Target::Actor(actor_id),
                    silver: false,
                })
                .is_ok()
            {
                return;
            }

        // A snared villain cannot follow: back off and use the flintlock.
        if trapped_now
            && adjacent
            && (def.cadence.is_none() || vulnerable)
            && session.sim.state.hunter.item_count("flintlock-shot") > 0
            && self.step_away(session, villain_pos) {
                return;
            }

        if adjacent {
            // Only strike when the blow can land (cadence villains).
            if def.cadence.is_some() && !vulnerable && !dormant {
                // Shroud is up and no charm: step back rather than feed it.
                if !self.step_away(session, villain_pos) {
                    let _ = session.apply(Command::Wait);
                }
                return;
            }
            // Priming Power Attack costs an action, which only pays against
            // a sleeping target: the coup opener. In a live fight, swing.
            if dormant && !power_primed && stamina >= 2
                && session
                    .apply(Command::Manoeuvre {
                        id: "power-attack".to_owned(),
                        steps: Vec::new(),
                    })
                    .is_ok()
                {
                    return;
                }
            let wounded = session
                .sim
                .state
                .actor(actor_id)
                .map(|actor| {
                    u32::from(actor.hp) * 100
                        <= u32::from(actor.max_hp)
                            * u32::from(
                                session
                                    .sim
                                    .catalogue
                                    .balance
                                    .combat
                                    .killing_blow_health_percent,
                            )
                })
                .unwrap_or(false);
            let trapped = session
                .sim
                .state
                .actor(actor_id)
                .map(|actor| actor.trapped > 0)
                .unwrap_or(false);
            if physical >= 1 && (dormant || trapped || wounded)
                && session
                    .apply(Command::Signature {
                        id: "killing-blow".to_owned(),
                        dir: None,
                        target: Some(Target::Actor(actor_id)),
                    })
                    .is_ok()
                {
                    return;
                }
            let _ = session.apply(Command::Melee(Target::Actor(actor_id)));
            return;
        }

        // Not adjacent: lay a snare on the approach, then close in.
        if physical >= 1 && villain_pos.distance(hunter) <= 4 && !dormant {
            if let Some(dir) = Direction::toward(hunter, villain_pos) {
                let snare_at = hunter.step(dir);
                let already = session
                    .sim
                    .state
                    .snares
                    .iter()
                    .any(|snare| snare.map == session.sim.state.current_map);
                if !already && snare_at != villain_pos
                    && session
                        .apply(Command::Signature {
                            id: "set-snare".to_owned(),
                            dir: Some(dir),
                            target: None,
                        })
                        .is_ok()
                    {
                        return;
                    }
            }
        }
        self.walk_toward(session, villain_pos, true);
    }

    /// Walk to the exit leading to `destination` and travel.
    fn travel_toward(&mut self, session: &mut RunSession, destination: MapId) {
        let current = session.sim.state.current_map;
        if current == destination {
            self.step_index = self.step_index.saturating_add(0);
            return;
        }
        let exit = session
            .sim
            .world
            .map(current)
            .exits
            .iter()
            .find(|exit| exit.to_map == destination)
            .map(|exit| exit.at);
        let Some(exit_at) = exit else {
            let _ = session.apply(Command::Wait);
            return;
        };
        if session.sim.state.hunter.pos == exit_at {
            if session.apply(Command::Travel).is_ok() {
                // Route travel steps complete when the map changes.
                if self.step_index < self.steps.len() {
                    let step = &self.steps[self.step_index];
                    if step.description.starts_with("Travel to ") {
                        self.step_index += 1;
                    }
                }
            }
        } else {
            self.walk_toward(session, exit_at, false);
        }
    }

    /// Move to stand adjacent to a feature matching the predicate. Returns
    /// true when already in place.
    fn goto_feature(
        &mut self,
        session: &mut RunSession,
        predicate: impl Fn(&FeatureKind) -> bool,
    ) -> bool {
        let map = session.sim.state.current_map;
        let target = session
            .sim
            .world
            .map(map)
            .features
            .iter()
            .find(|feature| predicate(&feature.kind))
            .map(|feature| feature.at);
        let Some(target) = target else {
            self.step_index += 1;
            return false;
        };
        let hunter = session.sim.state.hunter.pos;
        if hunter == target || hunter.is_adjacent(target) {
            return true;
        }
        self.walk_toward(session, target, true);
        false
    }

    /// BFS one step toward the target (or a tile adjacent to it).
    fn walk_toward(&mut self, session: &mut RunSession, target: Point, adjacent_ok: bool) {
        let step = next_step(session, target, adjacent_ok);
        match step {
            Some(dir) => {
                let _ = session.apply(Command::Move(dir));
            }
            None => {
                let _ = session.apply(Command::Wait);
            }
        }
    }

    /// Step directly away from a threat if any retreat tile is free.
    fn step_away(&mut self, session: &mut RunSession, threat: Point) -> bool {
        let hunter = session.sim.state.hunter.pos;
        let map = session.sim.state.current_map;
        let mut best: Option<(i16, Direction)> = None;
        for dir in Direction::ALL {
            let next = hunter.step(dir);
            if !next.in_bounds()
                || !is_walkable(session.sim.state.terrain(&session.sim.world, map, next))
                || session
                    .sim
                    .state
                    .tile_occupied(&session.sim.world, map, next)
            {
                continue;
            }
            let distance = next.distance(threat);
            if best
                .map(|(d, _)| distance > d)
                .unwrap_or(distance > hunter.distance(threat))
            {
                best = Some((distance, dir));
            }
        }
        match best {
            Some((_, dir)) => session.apply(Command::Move(dir)).is_ok(),
            None => false,
        }
    }
}

/// Breadth-first path on the current map; returns the first step direction.
fn next_step(session: &RunSession, target: Point, adjacent_ok: bool) -> Option<Direction> {
    let map = session.sim.state.current_map;
    let start = session.sim.state.hunter.pos;
    let arrived = |point: Point| point == target || (adjacent_ok && point.is_adjacent(target));
    if arrived(start) {
        return None;
    }
    let index = |point: Point| point.y as usize * MAP_WIDTH as usize + point.x as usize;
    let mut came: Vec<Option<(Point, Direction)>> =
        vec![None; MAP_WIDTH as usize * MAP_HEIGHT as usize];
    let mut queue = VecDeque::new();
    queue.push_back(start);
    let mut goal = None;
    'search: while let Some(point) = queue.pop_front() {
        for dir in Direction::ALL {
            let next = point.step(dir);
            if !next.in_bounds() || came[index(next)].is_some() || next == start {
                continue;
            }
            let walkable = is_walkable(session.sim.state.terrain(&session.sim.world, map, next));
            let occupied = session
                .sim
                .state
                .tile_occupied(&session.sim.world, map, next);
            if !walkable || occupied {
                // The target tile itself may host the NPC/feature we seek.
                if next == target && arrived(point) {
                    goal = Some(point);
                    break 'search;
                }
                continue;
            }
            came[index(next)] = Some((point, dir));
            if arrived(next) {
                goal = Some(next);
                break 'search;
            }
            queue.push_back(next);
        }
    }
    let goal = goal?;
    // Walk parents back to the first step.
    let mut current = goal;
    let mut first_dir = None;
    while current != start {
        let (parent, dir) = came[index(current)]?;
        first_dir = Some(dir);
        current = parent;
    }
    first_dir
}

trait RegenLookup {
    fn sim_actor_regen_stopped(&self, id: rh_core::state::ActorId) -> Option<bool>;
}

impl RegenLookup for rh_core::state::RunState {
    fn sim_actor_regen_stopped(&self, id: rh_core::state::ActorId) -> Option<bool> {
        self.actor(id).map(|actor| actor.regen_stopped)
    }
}
