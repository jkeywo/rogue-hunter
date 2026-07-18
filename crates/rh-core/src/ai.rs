//! Enemy, villain, and NPC turns.
//!
//! Active enemies act after every hunter action; these local turns never
//! advance the global clock. Villain behaviours (pounce lanes, dash/
//! vulnerability cadence) are telegraphed through the event log so their
//! timing is observable in play, per the grimoire contract.

use rh_content::{CadenceDef, EnemyBehaviour, TierEffect};

use crate::events::EventKind;
use crate::fov::{has_clear_lane, has_line_of_sight, is_walkable};
use crate::geometry::{line_between, Direction, Point};
use crate::sim::Sim;
use crate::state::{ActorId, ActorKind};

/// Sight radius for enemies noticing the hunter; mirrors hunter FOV.
fn enemy_sight(sim: &Sim) -> i16 {
    i16::from(sim.catalogue.balance.vision.fov_radius)
}

pub fn world_tick(sim: &mut Sim) {
    let map = sim.state.current_map;
    let actor_ids: Vec<ActorId> = sim
        .state
        .actors
        .iter()
        .filter(|actor| actor.map == map && actor.hp > 0)
        .map(|actor| actor.id)
        .collect();
    for id in actor_ids {
        if sim.state.outcome.is_some() || sim.state.hunter.hp == 0 {
            break;
        }
        act(sim, id);
    }
    npc_routines(sim);
}

fn act(sim: &mut Sim, id: ActorId) {
    let map = sim.state.current_map;
    let hunter = sim.state.hunter.pos;

    // Dormant villains only count down toward rising.
    {
        let Some(actor) = sim.state.actor_mut(id) else {
            return;
        };
        if actor.dormant > 0 {
            actor.dormant -= 1;
            let risen = actor.dormant == 0;
            if risen {
                sim.log(
                    EventKind::Telegraph,
                    "The earth shifts. Whatever slept here sleeps no longer.".to_owned(),
                );
            } else {
                sim.log(
                    EventKind::Telegraph,
                    "The thing in the grave stirs.".to_owned(),
                );
                return;
            }
        }
    }

    // Wake on sight of the hunter.
    let (pos, awake, kind) = {
        let Some(actor) = sim.state.actor(id) else {
            return;
        };
        (actor.pos, actor.awake, actor.kind.clone())
    };
    if !awake {
        let sees = pos.distance(hunter) <= enemy_sight(sim)
            && has_line_of_sight(&sim.state, &sim.world, map, pos, hunter);
        if sees {
            if let Some(actor) = sim.state.actor_mut(id) {
                actor.awake = true;
            }
            let name = sim.actor_name(&kind);
            sim.log(EventKind::Combat, format!("The {name} has your scent."));
        } else {
            return;
        }
    }

    // Held in a snare: struggle, but still bite what stands adjacent.
    let trapped = sim.state.actor(id).map(|actor| actor.trapped).unwrap_or(0);
    if trapped > 0 {
        if let Some(actor) = sim.state.actor_mut(id) {
            actor.trapped -= 1;
            if actor.trapped == 0 {
                let name = sim.actor_name(&kind);
                sim.log(
                    EventKind::Combat,
                    format!("The {name} tears free of the snare."),
                );
            }
        }
        if pos.is_adjacent(hunter) {
            let penalty = sim.catalogue.balance.combat.trapped_attack_penalty_percent;
            attack_hunter(sim, id, penalty);
        }
        tick_villain_cadence(sim, id);
        return;
    }

    match kind {
        ActorKind::Villain => villain_act(sim, id),
        ActorKind::Enemy(enemy) => enemy_act(sim, id, &enemy),
    }
}

fn enemy_act(sim: &mut Sim, id: ActorId, enemy: &str) {
    let Some(def) = sim.catalogue.enemies.get(enemy).cloned() else {
        return;
    };
    let map = sim.state.current_map;
    let hunter = sim.state.hunter.pos;
    let pos = match sim.state.actor(id) {
        Some(actor) => actor.pos,
        None => return,
    };

    match def.behaviour {
        EnemyBehaviour::Shambler => {
            let acts = {
                let Some(actor) = sim.state.actor_mut(id) else {
                    return;
                };
                actor.slow_phase = !actor.slow_phase;
                actor.slow_phase
            };
            if !acts {
                return;
            }
            if pos.is_adjacent(hunter) {
                attack_hunter(sim, id, 0);
            } else {
                step_toward(sim, id, hunter);
            }
        }
        EnemyBehaviour::PackHunter => {
            if pos.is_adjacent(hunter) {
                attack_hunter(sim, id, 0);
            } else {
                step_toward(sim, id, hunter);
            }
        }
        EnemyBehaviour::Skirmisher => {
            let distance = pos.distance(hunter);
            if let Some(ranged) = &def.ranged {
                let in_range = distance <= i16::from(ranged.range)
                    && has_line_of_sight(&sim.state, &sim.world, map, pos, hunter);
                if distance <= 2 {
                    // Too close: back off if possible, else fight with steel.
                    if !step_away(sim, id, hunter) && pos.is_adjacent(hunter) {
                        attack_hunter(sim, id, 0);
                    }
                } else if in_range {
                    ranged_attack_hunter(sim, id, ranged.damage, ranged.hit_percent);
                } else {
                    step_toward(sim, id, hunter);
                }
            } else if pos.is_adjacent(hunter) {
                attack_hunter(sim, id, 0);
            } else {
                step_toward(sim, id, hunter);
            }
        }
    }
}

fn villain_act(sim: &mut Sim, id: ActorId) {
    let def = sim.villain_def().clone();
    let tier = sim.state.villain.tier;

    // Regeneration (werewolf) unless silver has stopped it.
    if let Some(regen) = &def.regeneration {
        let healed = {
            let Some(actor) = sim.state.actor_mut(id) else {
                return;
            };
            if !actor.regen_stopped && actor.hp > 0 && actor.hp < actor.max_hp {
                actor.hp = (actor.hp + regen.per_turn).min(actor.max_hp);
                true
            } else {
                false
            }
        };
        if healed {
            let visible = sim
                .state
                .actor(id)
                .map(|actor| sim.state.is_visible(actor.pos))
                .unwrap_or(false);
            if visible {
                sim.log(EventKind::Telegraph, regen.telegraph.clone());
            }
        }
    }

    // A broken hex-ward is rewoven after a few turns of muttering, so the
    // window the hunter tore open does not stay open forever.
    if let Some(ward) = def.ward.clone() {
        let rewoven = {
            let Some(actor) = sim.state.actor_mut(id) else {
                return;
            };
            if actor.ward_charges == 0 && actor.ward_reweave > 0 {
                actor.ward_reweave -= 1;
                actor.ward_reweave == 0
            } else {
                false
            }
        };
        if rewoven {
            let charges = sim.villain_ward_charges();
            if let Some(actor) = sim.state.actor_mut(id) {
                actor.ward_charges = charges;
            }
            sim.log(EventKind::Telegraph, ward.reweave_telegraph.clone());
        }
    }

    if let Some(cadence) = def.cadence.clone() {
        revenant_act(sim, id, &cadence, tier, &def);
        return;
    }
    werewolf_act(sim, id, tier, &def);
}

fn werewolf_act(sim: &mut Sim, id: ActorId, tier: u8, def: &rh_content::VillainDef) {
    let map = sim.state.current_map;
    let hunter = sim.state.hunter.pos;
    let Some(pounce) = def.pounce.clone() else {
        chase_or_attack(
            sim,
            id,
            def.melee_damage + tier_bonus_damage(def, tier),
            def.hit_percent,
        );
        return;
    };
    let cooldown_max = pounce_cooldown(def, tier, pounce.cooldown);
    let (pos, cooldown, primed) = {
        let Some(actor) = sim.state.actor(id) else {
            return;
        };
        (actor.pos, actor.pounce_cooldown, actor.pounce_primed)
    };

    let distance = pos.distance(hunter);
    let lane_clear = distance >= 2
        && distance <= i16::from(pounce.range)
        && has_clear_lane(&sim.state, &sim.world, map, pos, hunter);

    if primed {
        if let Some(actor) = sim.state.actor_mut(id) {
            actor.pounce_primed = false;
        }
        if lane_clear {
            // Land on the last free tile before the hunter along the lane.
            let lane = line_between(pos, hunter);
            let landing = lane
                .iter()
                .rev()
                .find(|point| {
                    point.is_adjacent(hunter)
                        && is_walkable(sim.state.terrain(&sim.world, map, **point))
                        && !sim.state.tile_occupied(&sim.world, map, **point)
                })
                .copied();
            if let Some(landing) = landing {
                move_actor(sim, id, landing);
                if let Some(actor) = sim.state.actor_mut(id) {
                    actor.pounce_cooldown = cooldown_max;
                }
                sim.log(EventKind::Telegraph, "The beast leaps!".to_owned());
                let bonus = sim.catalogue.balance.combat.pounce_attack_bonus_percent;
                let damage = def.melee_damage + tier_bonus_damage(def, tier);
                attack_hunter_with(sim, id, damage, def.hit_percent.saturating_add(bonus));
                return;
            }
        }
        sim.log(
            EventKind::Telegraph,
            "The beast checks its leap, lane broken.".to_owned(),
        );
        chase_or_attack(
            sim,
            id,
            def.melee_damage + tier_bonus_damage(def, tier),
            def.hit_percent,
        );
        return;
    }

    if cooldown > 0 {
        if let Some(actor) = sim.state.actor_mut(id) {
            actor.pounce_cooldown -= 1;
        }
    } else if lane_clear {
        if let Some(actor) = sim.state.actor_mut(id) {
            actor.pounce_primed = true;
        }
        sim.log(EventKind::Telegraph, pounce.telegraph.clone());
        return;
    }
    chase_or_attack(
        sim,
        id,
        def.melee_damage + tier_bonus_damage(def, tier),
        def.hit_percent,
    );
}

fn revenant_act(
    sim: &mut Sim,
    id: ActorId,
    cadence: &CadenceDef,
    tier: u8,
    def: &rh_content::VillainDef,
) {
    let map = sim.state.current_map;
    let hunter = sim.state.hunter.pos;

    // Advance the shared vulnerability/dash cadence.
    let (pos, was_bound) = {
        let Some(actor) = sim.state.actor_mut(id) else {
            return;
        };
        actor.cadence = (actor.cadence + 1) % cadence.period;
        if actor.bound > 0 {
            actor.bound -= 1;
        }
        (actor.pos, actor.bound > 0)
    };
    let vulnerable_now = sim.villain_is_vulnerable(id);
    if vulnerable_now && sim.state.is_visible(pos) {
        sim.log(EventKind::Telegraph, cadence.vulnerable_telegraph.clone());
    }

    // Consecrated ground scalds it while it stands there.
    if def.affected_by_consecration
        && sim.state.church_consecrated
        && sim.world.map(map).consecration_area.contains(&pos)
    {
        let dead = {
            let Some(actor) = sim.state.actor_mut(id) else {
                return;
            };
            actor.hp = actor.hp.saturating_sub(cadence.consecrated_damage_per_turn);
            actor.hp == 0
        };
        sim.log(
            EventKind::Telegraph,
            "The warded ground sears the revenant; grave-shadow boils away.".to_owned(),
        );
        if dead {
            sim.state.villain.dead = true;
            sim.state.villain.active = false;
            sim.state.outcome = Some(crate::state::Outcome::Victory);
            sim.log(
                EventKind::System,
                "On holy ground the revenant unravels into cold ash. The valley is delivered."
                    .to_owned(),
            );
            return;
        }
    }

    // Dash decisions share the cadence: the revenant retreats before its
    // vulnerable turn or closes on a distant hunter.
    let dash_cooldown_max = dash_cooldown(def, tier, cadence.period);
    let (dash_ready, next_vulnerable) = {
        let Some(actor) = sim.state.actor(id) else {
            return;
        };
        let next = (actor.cadence + 1) % cadence.period == cadence.period.saturating_sub(1);
        (actor.dash_cooldown == 0, next && actor.bound == 0)
    };
    if !dash_ready {
        if let Some(actor) = sim.state.actor_mut(id) {
            actor.dash_cooldown -= 1;
        }
    }

    // Consecrated ground keeps the shroud open: with nothing to protect by
    // retreating, the revenant goes all-in (per the revenant-cadence-ai spec).
    let on_ward = def.affected_by_consecration
        && sim.state.church_consecrated
        && sim.world.map(map).consecration_area.contains(&pos);

    let distance = pos.distance(hunter);
    if dash_ready && next_vulnerable && distance <= 2 && !was_bound && !on_ward {
        // Retreat before the shroud thins.
        if dash_move(sim, id, hunter, cadence.dash_tiles, false) {
            if let Some(actor) = sim.state.actor_mut(id) {
                actor.dash_cooldown = dash_cooldown_max;
            }
            sim.log(EventKind::Telegraph, cadence.dash_telegraph.clone());
            return;
        }
    }
    if dash_ready && distance > 3 && dash_move(sim, id, hunter, cadence.dash_tiles, true) {
        if let Some(actor) = sim.state.actor_mut(id) {
            actor.dash_cooldown = dash_cooldown_max;
        }
        sim.log(EventKind::Telegraph, cadence.dash_telegraph.clone());
        return;
    }
    chase_or_attack(
        sim,
        id,
        def.melee_damage + tier_bonus_damage(def, tier),
        def.hit_percent,
    );
}

fn tier_bonus_damage(def: &rh_content::VillainDef, tier: u8) -> u16 {
    def.tier_behaviours
        .iter()
        .take(usize::from(tier))
        .map(|behaviour| match behaviour.effect {
            TierEffect::BonusMeleeDamage { amount } => amount,
            _ => 0,
        })
        .sum()
}

fn pounce_cooldown(def: &rh_content::VillainDef, tier: u8, base: u8) -> u8 {
    def.tier_behaviours
        .iter()
        .take(usize::from(tier))
        .filter_map(|behaviour| match behaviour.effect {
            TierEffect::PounceCooldown { turns } => Some(turns),
            _ => None,
        })
        .min()
        .unwrap_or(base)
}

fn dash_cooldown(def: &rh_content::VillainDef, tier: u8, base: u8) -> u8 {
    def.tier_behaviours
        .iter()
        .take(usize::from(tier))
        .filter_map(|behaviour| match behaviour.effect {
            TierEffect::DashCooldown { turns } => Some(turns),
            _ => None,
        })
        .min()
        .unwrap_or(base)
}

/// Straight-line dash toward or away from the hunter. Returns true if moved.
fn dash_move(sim: &mut Sim, id: ActorId, hunter: Point, tiles: u8, toward: bool) -> bool {
    let map = sim.state.current_map;
    let Some(pos) = sim.state.actor(id).map(|actor| actor.pos) else {
        return false;
    };
    let mut best: Option<(i16, Point)> = None;
    for dir in Direction::ALL {
        let mut current = pos;
        let mut steps = 0;
        while steps < tiles {
            let next = current.step(dir);
            if !next.in_bounds()
                || !is_walkable(sim.state.terrain(&sim.world, map, next))
                || sim.state.tile_occupied(&sim.world, map, next)
            {
                break;
            }
            current = next;
            steps += 1;
        }
        if current == pos {
            continue;
        }
        let distance = current.distance(hunter);
        let better = match best {
            None => true,
            Some((best_distance, _)) => {
                if toward {
                    distance < best_distance
                } else {
                    distance > best_distance
                }
            }
        };
        if better {
            best = Some((distance, current));
        }
    }
    let improves = |d: i16| {
        if toward {
            d < pos.distance(hunter)
        } else {
            d > pos.distance(hunter)
        }
    };
    if let Some((distance, target)) = best {
        if improves(distance) {
            move_actor(sim, id, target);
            return true;
        }
    }
    false
}

fn chase_or_attack(sim: &mut Sim, id: ActorId, damage: u16, hit_percent: u8) {
    let hunter = sim.state.hunter.pos;
    let Some(pos) = sim.state.actor(id).map(|actor| actor.pos) else {
        return;
    };
    if pos.is_adjacent(hunter) {
        attack_hunter_with(sim, id, damage, hit_percent);
    } else {
        step_toward(sim, id, hunter);
    }
}

/// Step toward a goal: greedy when open ground allows it, breadth-first
/// pathing when walls demand navigation (into buildings, around corners).
fn step_toward(sim: &mut Sim, id: ActorId, goal: Point) {
    let map = sim.state.current_map;
    let Some(pos) = sim.state.actor(id).map(|actor| actor.pos) else {
        return;
    };
    let mut options: Vec<Point> = pos
        .neighbours()
        .filter(|point| {
            point.in_bounds()
                && is_walkable(sim.state.terrain(&sim.world, map, *point))
                && !sim.state.tile_occupied(&sim.world, map, *point)
        })
        .collect();
    options.sort_by_key(|point| (point.distance(goal), point.y, point.x));
    if let Some(next) = options.first().copied() {
        if next.distance(goal) < pos.distance(goal) {
            move_actor(sim, id, next);
            return;
        }
    }
    // Blocked by architecture: path properly.
    if let Some(next) = bfs_step(sim, pos, goal) {
        move_actor(sim, id, next);
    }
}

/// First step of a breadth-first path from `from` toward (adjacent to) `goal`.
fn bfs_step(sim: &Sim, from: Point, goal: Point) -> Option<Point> {
    use crate::geometry::{MAP_HEIGHT, MAP_WIDTH};
    let map = sim.state.current_map;
    let index = |point: Point| point.y as usize * MAP_WIDTH as usize + point.x as usize;
    let mut parent: Vec<Option<Point>> = vec![None; (MAP_WIDTH * MAP_HEIGHT) as usize];
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(from);
    let mut reached: Option<Point> = None;
    'search: while let Some(point) = queue.pop_front() {
        for dir in Direction::ALL {
            let next = point.step(dir);
            if !next.in_bounds() || parent[index(next)].is_some() || next == from {
                continue;
            }
            if !is_walkable(sim.state.terrain(&sim.world, map, next))
                || sim.state.tile_occupied(&sim.world, map, next)
            {
                continue;
            }
            parent[index(next)] = Some(point);
            if next.is_adjacent(goal) || next == goal {
                reached = Some(next);
                break 'search;
            }
            queue.push_back(next);
        }
    }
    let mut current = reached?;
    let mut first = current;
    while let Some(previous) = parent[index(current)] {
        first = current;
        current = previous;
    }
    Some(first)
}

/// Step that increases distance from a threat. Returns true if moved.
fn step_away(sim: &mut Sim, id: ActorId, threat: Point) -> bool {
    let map = sim.state.current_map;
    let Some(pos) = sim.state.actor(id).map(|actor| actor.pos) else {
        return false;
    };
    let mut options: Vec<Point> = pos
        .neighbours()
        .filter(|point| {
            point.in_bounds()
                && is_walkable(sim.state.terrain(&sim.world, map, *point))
                && !sim.state.tile_occupied(&sim.world, map, *point)
        })
        .collect();
    options.sort_by_key(|point| (std::cmp::Reverse(point.distance(threat)), point.y, point.x));
    if let Some(next) = options.first().copied() {
        if next.distance(threat) > pos.distance(threat) {
            move_actor(sim, id, next);
            return true;
        }
    }
    false
}

/// Move an actor, springing any snare on the destination tile.
fn move_actor(sim: &mut Sim, id: ActorId, to: Point) {
    let map = sim.state.current_map;
    if let Some(actor) = sim.state.actor_mut(id) {
        actor.pos = to;
    }
    if let Some(index) = sim
        .state
        .snares
        .iter()
        .position(|snare| snare.map == map && snare.at == to)
    {
        sim.state.snares.remove(index);
        let hold = sim.catalogue.balance.combat.snare_hold_turns;
        let kind = sim.state.actor(id).map(|actor| actor.kind.clone());
        if let Some(actor) = sim.state.actor_mut(id) {
            actor.trapped = hold;
            actor.pounce_primed = false;
        }
        if let Some(kind) = kind {
            let name = sim.actor_name(&kind);
            sim.log(
                EventKind::Combat,
                format!("The snare snaps tight! The {name} is held fast."),
            );
        }
    }
}

fn attack_hunter(sim: &mut Sim, id: ActorId, penalty: u8) {
    let Some(actor) = sim.state.actor(id) else {
        return;
    };
    let (damage, hit) = match &actor.kind {
        ActorKind::Enemy(enemy) => {
            let Some(def) = sim.catalogue.enemies.get(enemy) else {
                return;
            };
            (def.melee_damage, def.hit_percent)
        }
        ActorKind::Villain => {
            let def = sim.villain_def();
            let tier = sim.state.villain.tier;
            (
                def.melee_damage + tier_bonus_damage(def, tier),
                def.hit_percent,
            )
        }
    };
    attack_hunter_with(sim, id, damage, hit.saturating_sub(penalty));
}

/// A skirmisher's ranged shot at the hunter.
fn ranged_attack_hunter(sim: &mut Sim, id: ActorId, damage: u16, hit_percent: u8) {
    let kind = match sim.state.actor(id) {
        Some(actor) => actor.kind.clone(),
        None => return,
    };
    let name = sim.actor_name(&kind);
    if sim.state.rng.percent(hit_percent) {
        sim.state.hunter.hp = sim.state.hunter.hp.saturating_sub(damage);
        sim.log(
            EventKind::Combat,
            format!("The {name}'s shot tears into you for {damage}."),
        );
    } else {
        sim.log(EventKind::Combat, format!("The {name}'s shot goes wide."));
    }
}

fn attack_hunter_with(sim: &mut Sim, id: ActorId, damage: u16, hit_percent: u8) {
    let kind = match sim.state.actor(id) {
        Some(actor) => actor.kind.clone(),
        None => return,
    };
    let name = sim.actor_name(&kind);
    if sim.state.rng.percent(hit_percent) {
        sim.state.hunter.hp = sim.state.hunter.hp.saturating_sub(damage);
        sim.log(
            EventKind::Combat,
            format!("The {name} strikes you for {damage}."),
        );
    } else {
        sim.log(EventKind::Combat, format!("The {name} lunges and misses."));
    }
}

/// Villain cadence still ticks while it is held in a snare.
fn tick_villain_cadence(sim: &mut Sim, id: ActorId) {
    let def = sim.villain_def().clone();
    let Some(cadence) = def.cadence else { return };
    let is_villain = sim
        .state
        .actor(id)
        .map(|actor| actor.kind == ActorKind::Villain)
        .unwrap_or(false);
    if !is_villain {
        return;
    }
    let pos = {
        let Some(actor) = sim.state.actor_mut(id) else {
            return;
        };
        actor.cadence = (actor.cadence + 1) % cadence.period;
        if actor.bound > 0 {
            actor.bound -= 1;
        }
        actor.pos
    };
    if sim.villain_is_vulnerable(id) && sim.state.is_visible(pos) {
        sim.log(EventKind::Telegraph, cadence.vulnerable_telegraph.clone());
    }
}

/// Villagers and neutral NPCs take routine local turns: moving, working, and
/// talking, so relationships can be observed and Social play is spatially alive.
fn npc_routines(sim: &mut Sim) {
    let map = sim.state.current_map;
    let npc_count = sim.world.npcs.len();
    for index in 0..npc_count {
        let spec = sim.world.npcs[index].clone();
        if spec.map != map {
            continue;
        }
        {
            let npc = &sim.state.npcs[index];
            if !npc.alive || npc.fled {
                continue;
            }
            // Villagers notice an approaching hunter and pause to be spoken
            // to, so conversation does not become a footrace.
            if npc.pos.distance(sim.state.hunter.pos) <= 2 {
                continue;
            }
        }

        // Chatting in progress: stand together; witnesses learn the link.
        let chatting = sim.state.npcs[index].chatting;
        if chatting > 0 {
            sim.state.npcs[index].chatting -= 1;
            let target_index = sim.state.npcs[index].chat_target;
            if let Some(link_index) = target_index {
                if let Some(link) = spec.links.get(usize::from(link_index)) {
                    let other = &sim.state.npcs[link.to.0 as usize];
                    let both_visible = sim.state.is_visible(sim.state.npcs[index].pos)
                        && sim.state.is_visible(other.pos);
                    let key = crate::world::link_key(spec.id, link.to);
                    if both_visible && !sim.state.known_links.contains(&key) {
                        sim.state.known_links.insert(key);
                        sim.state.met_npcs.insert(spec.id);
                        sim.state.met_npcs.insert(link.to);
                        let text = link.discovered_text.clone();
                        sim.log(
                            EventKind::Social,
                            format!("You watch them together and understand: {text}"),
                        );
                    }
                }
            }
            if sim.state.npcs[index].chatting == 0 {
                sim.state.npcs[index].chat_target = None;
                sim.state.npcs[index].chat_timer = 8;
            }
            continue;
        }

        // Head out to chat with a linked NPC when the timer runs down.
        let timer = sim.state.npcs[index].chat_timer;
        if timer == 0 && !spec.links.is_empty() {
            let pick = sim.state.rng.index(spec.links.len()) as u8;
            let link = &spec.links[usize::from(pick)];
            let other_alive = {
                let other = &sim.state.npcs[link.to.0 as usize];
                other.alive && !other.fled
            };
            if other_alive {
                sim.state.npcs[index].chat_target = Some(pick);
                let goal = sim.state.npcs[link.to.0 as usize].pos;
                let arrived = npc_step_toward(sim, index, goal);
                if arrived {
                    sim.state.npcs[index].chatting = 3;
                }
            } else {
                sim.state.npcs[index].chat_timer = 8;
            }
            continue;
        }
        if timer > 0 {
            sim.state.npcs[index].chat_timer -= 1;
        }
        if let Some(link_index) = sim.state.npcs[index].chat_target {
            let link_to = spec.links.get(usize::from(link_index)).map(|link| link.to);
            if let Some(to) = link_to {
                let goal = sim.state.npcs[to.0 as usize].pos;
                let arrived = npc_step_toward(sim, index, goal);
                if arrived {
                    sim.state.npcs[index].chatting = 3;
                }
                continue;
            }
        }

        // Otherwise drift around the work spot.
        let pos = sim.state.npcs[index].pos;
        if pos.distance(spec.work) > 2 {
            npc_step_toward(sim, index, spec.work);
        } else {
            let roll = sim.state.rng.below(4);
            if roll == 0 {
                let dir = Direction::ALL[sim.state.rng.index(8)];
                let next = pos.step(dir);
                if next.in_bounds()
                    && is_walkable(sim.state.terrain(&sim.world, map, next))
                    && !sim.state.tile_occupied(&sim.world, map, next)
                    && next.distance(spec.work) <= 2
                {
                    sim.state.npcs[index].pos = next;
                }
            }
        }
    }
}

/// Step an NPC toward a goal; returns true once adjacent to it.
fn npc_step_toward(sim: &mut Sim, index: usize, goal: Point) -> bool {
    let map = sim.state.current_map;
    let pos = sim.state.npcs[index].pos;
    if pos.is_adjacent(goal) || pos == goal {
        return true;
    }
    let mut options: Vec<Point> = pos
        .neighbours()
        .filter(|point| {
            point.in_bounds()
                && is_walkable(sim.state.terrain(&sim.world, map, *point))
                && !sim.state.tile_occupied(&sim.world, map, *point)
        })
        .collect();
    options.sort_by_key(|point| (point.distance(goal), point.y, point.x));
    if let Some(next) = options.first().copied() {
        if next.distance(goal) < pos.distance(goal) {
            sim.state.npcs[index].pos = next;
        }
    }
    sim.state.npcs[index].pos.is_adjacent(goal)
}
