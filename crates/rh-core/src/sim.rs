//! The authoritative simulation: semantic command boundary and rules.
//!
//! [`Sim::apply`] is the only way state changes. Every input path (keyboard,
//! mouse, replay, browser) funnels through it, and every rejection carries a
//! player-readable reason so blocked actions are explained, never hidden.

use rh_content::{
    Catalogue, ItemKind, ManoeuvreEffect, PoolKind, SignatureEffect, StringId, Terrain,
};

use crate::combat;
use crate::command::{Command, Rejection, Target};
use crate::events::{EventKind, LogEvent};
use crate::fov::{self, has_line_of_sight, is_walkable};
use crate::geometry::{Direction, Point};
use crate::rng::SimRng;
use crate::state::{Actor, ActorId, ActorKind, GroundWard, Outcome, RunState, Snare};
use crate::world::{
    DiscoveryRule, FeatureId, FeatureKind, GraveContents, MapId, NpcId, OpportunityAnchor,
    OpportunityGrant, OpportunityId, OpportunitySpec, World,
};

/// Why the global clock advanced; controls which pools refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockReason {
    Travel,
    Death,
    CostlyAction,
}

pub struct Sim {
    pub catalogue: Catalogue,
    pub world: World,
    pub state: RunState,
}

impl Sim {
    /// Start a run over a generated world. The RNG continues from where
    /// generation left off: one deterministic stream drives everything.
    pub fn new(catalogue: Catalogue, world: World, rng: SimRng) -> Self {
        let state = RunState::new(&world, &catalogue, rng);
        let mut sim = Self {
            catalogue,
            world,
            state,
        };
        sim.apply_opening();
        let arrival = sim.catalogue.strings.ui_fill(
            "log.run.arrive",
            &[
                (
                    "hunter",
                    sim.catalogue.strings.get(&sim.catalogue.hunter.name),
                ),
                ("place", &sim.world.map(sim.state.current_map).name),
            ],
        );
        sim.log(EventKind::System, arrival);
        sim.note_arrival();
        // Quietly: the opening prose is what turn zero is for.
        sim.refresh_senses_quietly();
        sim
    }

    /// First footfall on a map: narrate how its variation packs dressed it.
    /// Later, this is also where a region's event deck is consulted.
    fn note_arrival(&mut self) {
        let map = self.state.current_map;
        if !self.state.arrived.insert(map) {
            return;
        }
        let template = self.world.map(map).template.clone();
        let pack_ids = self
            .world
            .packs
            .get(map.0 as usize)
            .cloned()
            .unwrap_or_default();
        let lines: Vec<String> = self
            .catalogue
            .maps
            .get(&template)
            .map(|def| {
                pack_ids
                    .iter()
                    .filter_map(|id| def.packs.iter().find(|pack| &pack.id == id))
                    .map(|pack| self.catalogue.strings.get(&pack.label).to_owned())
                    .collect()
            })
            .unwrap_or_default();
        for line in lines {
            self.log(EventKind::Travel, line);
        }
        // The region's event deck: one optional novelty per arrival until it
        // runs dry. Never at run start — turn zero belongs to the opening.
        if self.state.local_turn > 0 {
            self.fire_next_event(map);
        }
    }

    /// Work an authored machine: apply its one observable payoff.
    fn fire_machine(&mut self, machine_id: String, anchor: &OpportunityAnchor) {
        let Some(machine) = self.catalogue.machines.get(&machine_id).cloned() else {
            return;
        };
        let at = match anchor {
            OpportunityAnchor::Tile(at) => *at,
            OpportunityAnchor::Npc(npc) => self.state.npcs[npc.0 as usize].pos,
        };
        let map = self.state.current_map;
        match machine.effect {
            rh_content::MachineEffect::Patch { patches } => {
                for patch in patches {
                    let point = Point::new(i16::from(patch.at[0]), i16::from(patch.at[1]));
                    self.state.terrain_overrides.insert((map, point), patch.to);
                }
            }
            rh_content::MachineEffect::Scatter { tiles } => {
                let ids: Vec<ActorId> = self
                    .state
                    .actors
                    .iter()
                    .filter(|actor| actor.map == map && actor.hp > 0)
                    .map(|actor| actor.id)
                    .collect();
                for id in ids {
                    for _ in 0..tiles {
                        let Some(actor) = self.state.actor(id) else {
                            break;
                        };
                        let from = actor.pos;
                        let dx = (from.x - at.x).signum();
                        let dy = (from.y - at.y).signum();
                        let next = Point::new(from.x + dx, from.y + dy);
                        if !next.in_bounds()
                            || !is_walkable(self.state.terrain(&self.world, map, next))
                            || self.state.actor_at(map, next).is_some()
                        {
                            break;
                        }
                        if let Some(actor) = self.state.actor_mut(id) {
                            actor.pos = next;
                        }
                    }
                }
            }
            rh_content::MachineEffect::Ward { turns, radius } => {
                self.state.wards.push(GroundWard {
                    map,
                    centre: at,
                    radius,
                    turns_left: turns,
                });
            }
        }
    }

    /// Draw the next optional event from this map's deck, if any remain.
    fn fire_next_event(&mut self, map: MapId) {
        let cursor = self
            .state
            .event_cursor
            .get(map.0 as usize)
            .copied()
            .unwrap_or(0) as usize;
        let Some(event_id) = self
            .world
            .event_decks
            .get(map.0 as usize)
            .and_then(|deck| deck.get(cursor))
            .map(|id| id.to_owned())
        else {
            return;
        };
        if let Some(slot) = self.state.event_cursor.get_mut(map.0 as usize) {
            *slot += 1;
        }
        let Some(event) = self.catalogue.events.get(&event_id).cloned() else {
            return;
        };
        let body = self.catalogue.strings.get(&event.body).to_owned();
        self.log(EventKind::Clock, body);
        match event.effect {
            rh_content::EventEffect::None => {}
            rh_content::EventEffect::Cache { items } => {
                for item in items {
                    self.state.hunter.add_item(&item, 1);
                    let name = self.item_name(&item);
                    self.log_fill(EventKind::Item, "log.item.gained", &[("item", &name)]);
                }
            }
            rh_content::EventEffect::Spawn { enemy, count } => {
                for _ in 0..count {
                    self.spawn_event_enemy(&enemy);
                }
            }
            rh_content::EventEffect::Reveal => {
                let found = self
                    .world
                    .opportunities
                    .iter()
                    .find(|opp| {
                        opp.map == map
                            && !self.state.discovered.contains(&opp.id)
                            && matches!(
                                opp.discovery,
                                DiscoveryRule::Sight | DiscoveryRule::SightOr(_)
                            )
                    })
                    .map(|opp| (opp.id, opp.name.clone()));
                if let Some((id, name)) = found {
                    self.state.discovered.insert(id);
                    self.log_fill(
                        EventKind::Clue,
                        "log.discovery.opportunity",
                        &[("what", &name)],
                    );
                }
            }
        }
    }

    /// A walkable, unoccupied tile a wary distance from the hunter, for an
    /// event arrival. Deterministic: candidates in row-major order, one draw.
    fn spawn_event_enemy(&mut self, enemy: &str) {
        let map = self.state.current_map;
        let hunter = self.state.hunter.pos;
        let mut candidates = Vec::new();
        for y in 0..crate::geometry::MAP_HEIGHT {
            for x in 0..crate::geometry::MAP_WIDTH {
                let point = Point::new(x, y);
                let distance = point.distance(hunter);
                if (4..=8).contains(&distance)
                    && is_walkable(self.state.terrain(&self.world, map, point))
                    && self.state.actor_at(map, point).is_none()
                {
                    candidates.push(point);
                }
            }
        }
        if candidates.is_empty() {
            return;
        }
        let at = candidates[self.state.rng.index(candidates.len())];
        let Some(def) = self.catalogue.enemies.get(enemy) else {
            return;
        };
        let hp = def.health;
        let id = self
            .state
            .spawn_actor(ActorKind::Enemy(enemy.to_owned()), map, at, hp);
        if let Some(actor) = self.state.actor_mut(id) {
            actor.awake = true;
        }
    }

    /// Put the run in the situation it opens in.
    ///
    /// Most runs open on a hook that frames the hunt and banks nothing. The
    /// rest open already holding one node: the hunter was told, or stopped on
    /// the way past, before play began. That is construction rather than
    /// action — it costs no clock, no pool and no turn, and it never enters
    /// the command log, because the log must replay to the same state and this
    /// is already a function of the seed.
    fn apply_opening(&mut self) {
        let situation = self.world.opening.clone();
        let body = self
            .catalogue
            .openings
            .iter()
            .find(|opening| opening.id == situation.opening)
            .map(|opening| opening.body.clone())
            .unwrap_or_default();
        let prior_name = situation
            .prior
            .map(|id| self.world.opportunity(id).name.clone())
            .unwrap_or_default();
        let npc_name = situation
            .prior
            .and_then(|id| match self.world.opportunity(id).anchor {
                OpportunityAnchor::Npc(npc) => Some(self.world.npc(npc).name.clone()),
                OpportunityAnchor::Tile(_) => None,
            })
            .unwrap_or_default();
        // One condition from every axis: exactly one bites, one helps, the
        // rest are texture.
        let conditions: Vec<rh_content::ConditionDef> = situation
            .conditions
            .iter()
            .filter_map(|id| {
                self.catalogue
                    .conditions
                    .iter()
                    .find(|condition| condition.id == *id)
                    .cloned()
            })
            .collect();
        let place = self.world.map(self.state.current_map).name.clone();
        // The hook first — why she came — then the condition, which is what the
        // valley is like as she gets there.
        let prose = body
            .into_iter()
            .chain(conditions.iter().flat_map(|c| c.body.clone()));
        for line in prose {
            let text = self
                .catalogue
                .strings
                .get(&line)
                .replace("{npc}", &npc_name)
                .replace("{clue}", &prior_name)
                .replace("{place}", &place);
            self.log(EventKind::System, text);
        }
        for effect in conditions.iter().filter_map(|c| c.effect.clone()) {
            self.apply_condition(&effect);
        }

        let Some(id) = situation.prior else {
            return;
        };
        let spec = self.world.opportunity(id).clone();
        debug_assert!(
            !spec.clears_terrain,
            "a banked node must not be one that forces terrain"
        );
        self.state.discovered.insert(id);
        self.state.resolved.insert(id);
        self.log(EventKind::Clue, spec.reveal.clone());
        self.apply_grant(&spec);
        self.cascade_discovery(id);
    }

    /// The parts of a run's condition that live in the run state. The rest —
    /// ambush chance, the extra things in the wood — were baked into the world
    /// at generation, where certification could see them.
    fn apply_condition(&mut self, effect: &rh_content::ConditionEffect) {
        match effect {
            rh_content::ConditionEffect::SocialSurcharge => {
                // The existing mechanism: consequential Social work costs one
                // more, and nobody will sell to her.
                self.state.settlement_hostile = true;
            }
            rh_content::ConditionEffect::ShortSight { tiles } => {
                self.state.sight_penalty = *tiles;
            }
            rh_content::ConditionEffect::LongSight { tiles } => {
                self.state.sight_bonus = *tiles;
            }
            rh_content::ConditionEffect::WellSupplied { item } => {
                self.state.hunter.add_item(item, 1);
                let name = self.item_name(item);
                self.log_fill(EventKind::Item, "log.item.carried-in", &[("item", &name)]);
            }
            // Baked into the world at generation, where certification saw them.
            rh_content::ConditionEffect::Ambush { .. }
            | rh_content::ConditionEffect::QuietRoads { .. }
            | rh_content::ConditionEffect::Pressure { .. } => {}
        }
    }

    /// Apply one semantic command. On success the event log grew by whatever
    /// happened; on rejection nothing changed at all.
    pub fn apply(&mut self, command: &Command) -> Result<(), Rejection> {
        if self.state.outcome.is_some() {
            return Err(Rejection::RunOver);
        }
        match command {
            Command::Move(dir) => self.cmd_move(*dir),
            Command::Wait => {
                self.log_ui(EventKind::System, "log.turn.wait");
                self.end_action();
                Ok(())
            }
            Command::Melee(target) => self.cmd_melee(*target),
            Command::Ranged { target, silver } => self.cmd_ranged(*target, *silver),
            Command::Manoeuvre { id, steps } => self.cmd_manoeuvre(id, steps),
            Command::Signature { id, dir, target } => self.cmd_signature(id, *dir, *target),
            Command::UseDraught => self.cmd_use_draught(),
            Command::UseBindingCharm { target } => self.cmd_use_charm(*target),
            Command::Interact(id) => self.cmd_interact(*id),
            Command::Talk(npc) => self.cmd_talk(*npc),
            Command::BuyShot(npc) => self.cmd_buy_shot(*npc),
            Command::Travel => self.cmd_travel(),
            Command::Craft { recipe } => self.cmd_craft(recipe),
            Command::Consecrate => self.cmd_consecrate(),
            Command::OpenGrave(feature) => self.cmd_open_grave(*feature),
            Command::Force(dir) => self.cmd_force(*dir),
            Command::UncoverVillain => self.cmd_uncover(),
        }
    }

    /// Force adjacent barred terrain with muscle: 1 Physical point.
    fn cmd_force(&mut self, dir: Direction) -> Result<(), Rejection> {
        let at = self.state.hunter.pos.step(dir);
        let map = self.state.current_map;
        let terrain = self.state.terrain(&self.world, map, at);
        let (cleared, text) = match terrain {
            Terrain::BarredDoor => (Terrain::Door, "log.force.barred-door"),
            Terrain::Rubble => (Terrain::Floor, "log.force.rubble"),
            _ => return Err(Rejection::NothingToForce),
        };
        if self.state.hunter.physical < 1 {
            return Err(Rejection::PoolEmpty {
                pool: PoolKind::Physical,
                needed: 1,
            });
        }
        self.state.hunter.physical -= 1;
        self.state.terrain_overrides.insert((map, at), cleared);
        self.log_ui(EventKind::Clue, text);
        self.end_action();
        Ok(())
    }

    // -- Logging -------------------------------------------------------------

    pub(crate) fn log(&mut self, kind: EventKind, text: String) {
        self.state.log.push(LogEvent {
            global_turn: self.state.clock,
            local_turn: self.state.local_turn,
            kind,
            text,
        });
    }

    /// Log a line of authored content, resolved through the string table.
    pub(crate) fn log_id(&mut self, kind: EventKind, id: &StringId) {
        let text = self.catalogue.strings.get(id).to_owned();
        self.log(kind, text);
    }

    /// Log a code-side line, resolved through the string table.
    pub(crate) fn log_ui(&mut self, kind: EventKind, id: &str) {
        let text = self.catalogue.strings.ui(id).to_owned();
        self.log(kind, text);
    }

    /// Log a code-side line with its `{name}`-style placeholders filled.
    pub(crate) fn log_fill(&mut self, kind: EventKind, id: &str, args: &[(&str, &str)]) {
        let text = self.catalogue.strings.ui_fill(id, args);
        self.log(kind, text);
    }

    /// The English behind an id.
    pub(crate) fn text(&self, id: &StringId) -> &str {
        self.catalogue.strings.get(id)
    }

    // -- Movement and combat ---------------------------------------------------

    fn cmd_move(&mut self, dir: Direction) -> Result<(), Rejection> {
        let dest = self.state.hunter.pos.step(dir);
        if !dest.in_bounds() {
            return Err(Rejection::Blocked {
                what: "the map's edge".to_owned(),
            });
        }
        // Bump attack: moving into a hostile actor melees it.
        if let Some(actor) = self.state.actor_at(self.state.current_map, dest) {
            let id = actor.id;
            return self.cmd_melee(Target::Actor(id));
        }
        if self
            .state
            .npc_at(&self.world, self.state.current_map, dest)
            .is_some()
        {
            return Err(Rejection::Blocked {
                what: "someone standing there".to_owned(),
            });
        }
        let terrain = self
            .state
            .terrain(&self.world, self.state.current_map, dest);
        if !is_walkable(terrain) {
            let what = match terrain {
                Terrain::BarredDoor => "a barred door (a Physical point could force it)",
                Terrain::Rubble => "fallen rubble (a Physical point could shift it)",
                Terrain::Wall => "a wall",
                Terrain::Tree => "dense growth",
                Terrain::Water => "deep water",
                Terrain::Altar => "the altar",
                Terrain::Workstation => "the workbench",
                _ => "the terrain",
            };
            return Err(Rejection::Blocked {
                what: what.to_owned(),
            });
        }
        self.state.hunter.pos = dest;
        if self.exit_at(dest).is_some() {
            self.log(
                EventKind::Travel,
                self.catalogue.strings.ui("log.travel.route-out").to_owned(),
            );
        }
        self.end_action();
        Ok(())
    }

    fn cmd_melee(&mut self, target: Target) -> Result<(), Rejection> {
        let weapon_damage = self.melee_damage();
        match target {
            Target::Actor(id) => {
                let (pos, dormant) = {
                    let actor = self.live_actor(id)?;
                    (actor.pos, actor.dormant > 0)
                };
                if !self.state.hunter.pos.is_adjacent(pos) {
                    return Err(Rejection::NotAdjacent);
                }
                let multiplier = self.take_melee_multiplier();
                let damage = weapon_damage * u16::from(multiplier) / combat::MULTIPLIER_HALVES;
                self.hunter_strike(id, damage, dormant, false);
                self.end_action();
                Ok(())
            }
            Target::Npc(npc) => {
                self.require_npc_adjacent(npc)?;
                self.attack_npc(npc, weapon_damage, false)?;
                self.end_action();
                Ok(())
            }
        }
    }

    fn cmd_ranged(&mut self, target: Target, silver: bool) -> Result<(), Rejection> {
        let Some((range, base_damage)) = self.ranged_weapon() else {
            return Err(Rejection::NoAmmo {
                item: "firearm".to_owned(),
            });
        };
        let (ammo_item, damage) = if silver {
            let Some(def) = self.catalogue.items.get("silver-bullet") else {
                return Err(Rejection::NoAmmo {
                    item: "silver bullet".to_owned(),
                });
            };
            match def.kind {
                ItemKind::WeaknessAmmunition { damage, .. } => ("silver-bullet", damage),
                _ => {
                    return Err(Rejection::NoAmmo {
                        item: "silver bullet".to_owned(),
                    })
                }
            }
        } else {
            ("flintlock-shot", base_damage)
        };
        if self.state.hunter.item_count(ammo_item) == 0 {
            return Err(Rejection::NoAmmo {
                item: self.item_name(ammo_item),
            });
        }
        let target_pos = match target {
            Target::Actor(id) => self.live_actor(id)?.pos,
            Target::Npc(npc) => {
                self.require_npc_present(npc)?;
                self.state.npcs[npc.0 as usize].pos
            }
        };
        let map = self.state.current_map;
        if self.state.hunter.pos.distance(target_pos) > i16::from(range) {
            return Err(Rejection::OutOfRange);
        }
        if !has_line_of_sight(
            &self.state,
            &self.world,
            map,
            self.state.hunter.pos,
            target_pos,
        ) {
            return Err(Rejection::NoLineOfSight);
        }
        self.state.hunter.remove_item(ammo_item, 1);
        let sure = std::mem::take(&mut self.state.hunter.sure_shot);
        match target {
            Target::Actor(id) => {
                let dormant = self.live_actor(id)?.dormant > 0;
                self.hunter_ranged_strike(id, damage, silver, sure || dormant);
            }
            Target::Npc(npc) => {
                self.ranged_attack_npc(npc, damage, sure, silver)?;
            }
        }
        self.end_action();
        Ok(())
    }

    fn cmd_manoeuvre(&mut self, id: &str, steps: &[Direction]) -> Result<(), Rejection> {
        let Some(def) = self
            .catalogue
            .hunter
            .manoeuvres
            .iter()
            .find(|m| m.id == id)
            .cloned()
        else {
            return Err(Rejection::UnknownAbility { id: id.to_owned() });
        };
        if self.state.hunter.stamina < def.stamina_cost {
            return Err(Rejection::StaminaShort {
                needed: def.stamina_cost,
            });
        }
        match def.effect {
            ManoeuvreEffect::SureRangedShot => {
                if !steps.is_empty() {
                    return Err(Rejection::BadAbilityArguments);
                }
                self.state.hunter.stamina -= def.stamina_cost;
                self.state.hunter.sure_shot = true;
                self.log(
                    EventKind::Combat,
                    self.catalogue.strings.ui_fill(
                        "log.manoeuvre.aim",
                        &[("what", &self.text(&def.name).to_lowercase())],
                    ),
                );
            }
            ManoeuvreEffect::MeleeDamageMultiplier { numerator } => {
                if !steps.is_empty() {
                    return Err(Rejection::BadAbilityArguments);
                }
                self.state.hunter.stamina -= def.stamina_cost;
                self.state.hunter.melee_multiplier = Some(numerator);
                self.log(
                    EventKind::Combat,
                    self.catalogue
                        .strings
                        .ui("log.manoeuvre.power-attack")
                        .to_owned(),
                );
            }
            ManoeuvreEffect::Dash { tiles } => {
                if steps.len() != usize::from(tiles) {
                    return Err(Rejection::BadAbilityArguments);
                }
                // Validate the whole path before moving.
                let mut probe = self.state.hunter.pos;
                for step in steps {
                    probe = probe.step(*step);
                    let terrain = self
                        .state
                        .terrain(&self.world, self.state.current_map, probe);
                    if !probe.in_bounds() || !is_walkable(terrain) {
                        return Err(Rejection::Blocked {
                            what: "the ground ahead".to_owned(),
                        });
                    }
                    if self.state.actor_at(self.state.current_map, probe).is_some()
                        || self
                            .state
                            .npc_at(&self.world, self.state.current_map, probe)
                            .is_some()
                    {
                        return Err(Rejection::Blocked {
                            what: "someone in the way".to_owned(),
                        });
                    }
                }
                self.state.hunter.stamina -= def.stamina_cost;
                self.state.hunter.pos = probe;
                self.log_ui(EventKind::Combat, "log.manoeuvre.sprint");
            }
        }
        self.end_action();
        Ok(())
    }

    fn cmd_signature(
        &mut self,
        id: &str,
        dir: Option<Direction>,
        target: Option<Target>,
    ) -> Result<(), Rejection> {
        let Some(def) = self
            .catalogue
            .hunter
            .signatures
            .iter()
            .find(|s| s.id == id)
            .cloned()
        else {
            return Err(Rejection::UnknownAbility { id: id.to_owned() });
        };
        if self.state.hunter.physical < def.physical_cost {
            return Err(Rejection::PoolEmpty {
                pool: PoolKind::Physical,
                needed: def.physical_cost,
            });
        }
        match def.effect {
            SignatureEffect::SetSnare => {
                let Some(dir) = dir else {
                    return Err(Rejection::BadAbilityArguments);
                };
                let at = self.state.hunter.pos.step(dir);
                let map = self.state.current_map;
                let terrain = self.state.terrain(&self.world, map, at);
                if !at.in_bounds() || !is_walkable(terrain) {
                    return Err(Rejection::Blocked {
                        what: "unsuitable ground".to_owned(),
                    });
                }
                if self.state.snares.iter().any(|s| s.map == map && s.at == at) {
                    return Err(Rejection::Blocked {
                        what: "a snare already set there".to_owned(),
                    });
                }
                self.state.hunter.physical -= def.physical_cost;
                self.state.snares.push(Snare { map, at });
                self.log(
                    EventKind::Combat,
                    self.catalogue.strings.ui("log.signature.snare").to_owned(),
                );
            }
            SignatureEffect::KillingBlow => {
                let Some(Target::Actor(actor_id)) = target else {
                    return Err(Rejection::BadAbilityArguments);
                };
                let (pos, eligible, dormant) = {
                    let actor = self.live_actor(actor_id)?;
                    let wounded = u32::from(actor.hp) * 100
                        <= u32::from(actor.max_hp)
                            * u32::from(self.catalogue.balance.combat.killing_blow_health_percent);
                    (
                        actor.pos,
                        actor.trapped > 0 || actor.dormant > 0 || wounded,
                        actor.dormant > 0,
                    )
                };
                if !self.state.hunter.pos.is_adjacent(pos) {
                    return Err(Rejection::NotAdjacent);
                }
                if !eligible {
                    return Err(Rejection::BadAbilityArguments);
                }
                self.state.hunter.physical -= def.physical_cost;
                let multiplier = self.take_melee_multiplier();
                let damage = self.melee_damage() * u16::from(multiplier)
                    / combat::MULTIPLIER_HALVES
                    * combat::KILLING_BLOW_MULTIPLIER;
                self.log(
                    EventKind::Combat,
                    self.catalogue
                        .strings
                        .ui("log.signature.killing-blow")
                        .to_owned(),
                );
                self.hunter_strike(actor_id, damage, dormant, false);
            }
            SignatureEffect::ReadTheSign => {
                // Reinterpret a soft sign already held. This is the Occultist's
                // way through the evidence: she cannot out-fight the case, but
                // she can make an ambiguous proof say something definite.
                let Some(&opportunity) = self
                    .state
                    .identity_clues
                    .iter()
                    .find(|id| !self.state.discriminating_identity.contains(id))
                else {
                    return Err(Rejection::NothingLeftToRead);
                };
                self.state.hunter.physical -= def.physical_cost;
                self.state.discriminating_identity.insert(opportunity);
                let name = self
                    .world
                    .opportunities
                    .iter()
                    .find(|opp| opp.id == opportunity)
                    .map(|opp| opp.name.clone())
                    .unwrap_or_else(|| "the sign".to_owned());
                self.log(
                    EventKind::Clue,
                    self.catalogue
                        .strings
                        .ui_fill("log.signature.read-the-sign", &[("sign", &name)]),
                );
            }
            SignatureEffect::WardTheGround { turns, radius } => {
                let map = self.state.current_map;
                let centre = self.state.hunter.pos;
                if self
                    .state
                    .wards
                    .iter()
                    .any(|ward| ward.map == map && ward.centre == centre)
                {
                    return Err(Rejection::Blocked {
                        what: "ground you have already marked".to_owned(),
                    });
                }
                self.state.hunter.physical -= def.physical_cost;
                self.state.wards.push(GroundWard {
                    map,
                    centre,
                    radius,
                    turns_left: turns,
                });
                self.log(
                    EventKind::Combat,
                    self.catalogue
                        .strings
                        .ui("log.signature.ward-ground")
                        .to_owned(),
                );
            }
        }
        self.end_action();
        Ok(())
    }

    fn cmd_use_draught(&mut self) -> Result<(), Rejection> {
        let heal = match self.catalogue.items.get("wound-draught").map(|d| &d.kind) {
            Some(ItemKind::Draught { heal }) => *heal,
            _ => 4,
        };
        if !self.state.hunter.remove_item("wound-draught", 1) {
            return Err(Rejection::NoAmmo {
                item: "wound draught".to_owned(),
            });
        }
        self.state.hunter.hp = (self.state.hunter.hp + heal).min(self.state.hunter.max_hp);
        self.log(
            EventKind::Item,
            self.catalogue.strings.ui("log.item.draught").to_owned(),
        );
        self.end_action();
        Ok(())
    }

    fn cmd_use_charm(&mut self, target: Target) -> Result<(), Rejection> {
        let Target::Actor(actor_id) = target else {
            return Err(Rejection::NoSuchTarget);
        };
        let (pos, is_villain) = {
            let actor = self.live_actor(actor_id)?;
            (actor.pos, actor.kind == ActorKind::Villain)
        };
        if !self.state.hunter.pos.is_adjacent(pos) {
            return Err(Rejection::NotAdjacent);
        }
        let villain_def = self.villain_def();
        let Some(cadence) = villain_def.cadence.clone() else {
            // Using the charm on a werewolf (or an ordinary enemy) wastes it.
            if !self.state.hunter.remove_item("binding-charm", 1) {
                return Err(Rejection::NoAmmo {
                    item: "binding charm".to_owned(),
                });
            }
            self.log(
                EventKind::Item,
                self.catalogue
                    .strings
                    .ui("log.item.charm-no-hold")
                    .to_owned(),
            );
            self.end_action();
            return Ok(());
        };
        if !is_villain {
            if !self.state.hunter.remove_item("binding-charm", 1) {
                return Err(Rejection::NoAmmo {
                    item: "binding charm".to_owned(),
                });
            }
            self.log(
                EventKind::Item,
                self.catalogue
                    .strings
                    .ui("log.item.charm-wasted")
                    .to_owned(),
            );
            self.end_action();
            return Ok(());
        }
        if !self.state.hunter.remove_item("binding-charm", 1) {
            return Err(Rejection::NoAmmo {
                item: "binding charm".to_owned(),
            });
        }
        if let Some(actor) = self.state.actor_mut(actor_id) {
            actor.bound = cadence.bound_vulnerable_turns;
        }
        self.log(
            EventKind::Telegraph,
            self.catalogue
                .strings
                .ui("log.item.charm-strips-shroud")
                .to_owned(),
        );
        self.end_action();
        Ok(())
    }

    // -- Investigation and social ----------------------------------------------

    fn cmd_interact(&mut self, id: OpportunityId) -> Result<(), Rejection> {
        if usize::from(id.0) >= self.world.opportunities.len() {
            return Err(Rejection::NoSuchTarget);
        }
        if !self.state.discovered.contains(&id) {
            return Err(Rejection::NotDiscovered);
        }
        if self.state.resolved.contains(&id) || self.state.lost.contains(&id) {
            return Err(Rejection::AlreadyResolved);
        }
        let spec = self.world.opportunity(id).clone();
        if spec.map != self.state.current_map {
            return Err(Rejection::NothingThere);
        }
        match spec.anchor {
            OpportunityAnchor::Tile(at) => {
                if self.state.hunter.pos != at && !self.state.hunter.pos.is_adjacent(at) {
                    return Err(Rejection::NotAdjacent);
                }
            }
            OpportunityAnchor::Npc(npc) => {
                self.require_npc_adjacent(npc)?;
                let npc_state = &self.state.npcs[npc.0 as usize];
                let spec_npc = self.world.npc(npc);
                let hostile = spec_npc.disposition == crate::world::Disposition::Hostile
                    && !npc_state.leveraged;
                // Covert actions (spying, examining, tracking) need no
                // cooperation; conversation with the hostile does.
                if npc_state.attacked || (hostile && !spec.covert) {
                    return Err(Rejection::NpcWillNotTalk);
                }
            }
        }
        // Pool cost, priced by the shared economy (settlement-hostility
        // surcharge included) so play charges what certification budgeted.
        if let Some((pool, cost)) =
            crate::economy::opportunity_cost(spec.pool, spec.cost, self.state.settlement_hostile)
        {
            if self.state.hunter.pool(pool) < cost {
                return Err(Rejection::PoolEmpty { pool, needed: cost });
            }
            self.state.hunter.spend_pool(pool, cost);
        }
        self.state.resolved.insert(id);
        self.log(EventKind::Clue, spec.reveal.clone());
        if spec.clears_terrain {
            if let OpportunityAnchor::Tile(at) = spec.anchor {
                let terrain = self.state.terrain(&self.world, spec.map, at);
                let cleared = match terrain {
                    Terrain::BarredDoor => Some(Terrain::Door),
                    Terrain::Rubble => Some(Terrain::Floor),
                    _ => None,
                };
                if let Some(cleared) = cleared {
                    self.state.terrain_overrides.insert((spec.map, at), cleared);
                }
            }
        }
        self.apply_grant(&spec);
        self.cascade_discovery(id);
        self.end_action();
        Ok(())
    }

    fn apply_grant(&mut self, spec: &OpportunitySpec) {
        match &spec.grants {
            OpportunityGrant::IdentityClue { discriminating } => {
                self.state.identity_clues.insert(spec.id);
                if *discriminating {
                    self.state.discriminating_identity.insert(spec.id);
                }
                let proof = self.state.corroboration(&self.catalogue);
                if !self.state.villain_uncovered {
                    if proof.met() {
                        self.log(
                            EventKind::Clue,
                            self.catalogue
                                .strings
                                .ui("log.clue.can-name-quarry")
                                .to_owned(),
                        );
                    } else if proof.corroborated() {
                        self.log(
                            EventKind::Clue,
                            self.catalogue
                                .strings
                                .ui("log.clue.not-decisive")
                                .to_owned(),
                        );
                    }
                }
            }
            OpportunityGrant::OriginSign { discriminating } => {
                if *discriminating && !self.state.origin_identified {
                    self.state.origin_identified = true;
                    let origin = &self.catalogue.origins[&self.world.villain.origin];
                    self.log(
                        EventKind::Clue,
                        self.catalogue.strings.ui_fill(
                            "log.clue.origin-settled",
                            &[("origin", self.catalogue.strings.get(&origin.name))],
                        ),
                    );
                }
            }
            OpportunityGrant::SchemeSign { discriminating } => {
                if *discriminating && !self.state.scheme_identified {
                    self.state.scheme_identified = true;
                    let scheme = &self.catalogue.schemes[&self.world.villain.scheme];
                    self.log(
                        EventKind::Clue,
                        format!(
                            "That settles what it is working toward: {}. It can be interrupted — \
                             {}.",
                            self.text(&scheme.name),
                            self.text(&scheme.preempt.name).to_lowercase()
                        ),
                    );
                }
            }
            OpportunityGrant::SchemePreempt => {
                self.state.scheme_preempted = true;
            }
            OpportunityGrant::LocationClue => {
                self.state.villain_location_known = true;
                self.log(
                    EventKind::Clue,
                    self.catalogue
                        .strings
                        .ui("log.clue.resting-place")
                        .to_owned(),
                );
            }
            OpportunityGrant::Lead => {}
            OpportunityGrant::Items { items } => {
                for item in items {
                    self.state.hunter.add_item(item, 1);
                    let name = self.item_name(item);
                    self.log_fill(EventKind::Item, "log.item.gained", &[("item", &name)]);
                }
            }
            OpportunityGrant::MysticFavour => {
                self.state.hunter.mystic_bonus += 1;
                self.state.hunter.favour_used = true;
                self.log(
                    EventKind::Social,
                    self.catalogue.strings.ui("log.social.favour").to_owned(),
                );
            }
            OpportunityGrant::Machine { machine } => {
                self.fire_machine(machine.clone(), &spec.anchor);
            }
            OpportunityGrant::RelationshipInfo => {
                if let OpportunityAnchor::Npc(npc) = spec.anchor {
                    self.reveal_link_of(npc);
                }
            }
            OpportunityGrant::SecretInfo => {
                if let OpportunityAnchor::Npc(npc) = spec.anchor {
                    self.state.known_secrets.insert(npc);
                    let text = self.world.npc(npc).secret.text.clone();
                    self.log(EventKind::Social, text);
                }
            }
            OpportunityGrant::Leverage => {
                if let OpportunityAnchor::Npc(npc) = spec.anchor {
                    self.state.npcs[npc.0 as usize].leveraged = true;
                    let name = self.world.npc(npc).name.clone();
                    self.log(
                        EventKind::Social,
                        self.catalogue
                            .strings
                            .ui_fill("log.social.blackmailed", &[("name", &name)]),
                    );
                }
            }
            OpportunityGrant::Disproof { npc } => {
                self.state.disproved_secrets.insert(*npc);
                if let Some(disproof) = self.world.npc(*npc).secret.disproof.clone() {
                    self.log(EventKind::Social, disproof);
                }
            }
        }
    }

    /// Reveal the first undiscovered relationship link of this NPC.
    fn reveal_link_of(&mut self, npc: NpcId) {
        let links = self.world.npc(npc).links.clone();
        for link in links {
            let key = crate::world::link_key(npc, link.to);
            if self.state.known_links.insert(key) {
                self.log(EventKind::Social, link.discovered_text.clone());
                return;
            }
        }
        self.log(
            EventKind::Social,
            self.catalogue
                .strings
                .ui("log.social.nothing-new")
                .to_owned(),
        );
    }

    /// Newly-revealed opportunities gated on the one just resolved.
    fn cascade_discovery(&mut self, resolved: OpportunityId) {
        let unlocked: Vec<(OpportunityId, String)> = self
            .world
            .opportunities
            .iter()
            .filter(|opp| match opp.discovery {
                DiscoveryRule::RevealedBy(source) | DiscoveryRule::SightOr(source) => {
                    source == resolved && !self.state.discovered.contains(&opp.id)
                }
                DiscoveryRule::Sight => false,
            })
            .map(|opp| (opp.id, opp.name.clone()))
            .collect();
        for (id, name) in unlocked {
            self.state.discovered.insert(id);
            self.log_fill(EventKind::Clue, "log.clue.new-lead", &[("lead", &name)]);
        }
    }

    fn cmd_talk(&mut self, npc: NpcId) -> Result<(), Rejection> {
        self.require_npc_adjacent(npc)?;
        let npc_state = &self.state.npcs[npc.0 as usize];
        if npc_state.attacked {
            return Err(Rejection::NpcWillNotTalk);
        }
        let spec = self.world.npc(npc);
        let line = match spec.disposition {
            crate::world::Disposition::Friendly => {
                format!(
                    "{} greets you warmly and talks freely of small things.",
                    spec.name
                )
            }
            crate::world::Disposition::Wary => self
                .catalogue
                .strings
                .ui_fill("log.social.wary-answer", &[("npc", &spec.name)]),
            crate::world::Disposition::Hostile => self
                .catalogue
                .strings
                .ui_fill("log.social.hostile-answer", &[("npc", &spec.name)]),
        };
        self.state.met_npcs.insert(npc);
        self.log(EventKind::Social, line);
        self.end_action();
        Ok(())
    }

    fn cmd_buy_shot(&mut self, npc: NpcId) -> Result<(), Rejection> {
        self.require_npc_adjacent(npc)?;
        let spec = self.world.npc(npc);
        let npc_state = &self.state.npcs[npc.0 as usize];
        if !spec.trades || npc_state.attacked || self.state.settlement_hostile {
            return Err(Rejection::NpcWillNotTalk);
        }
        if self.state.hunter.item_count("coin") < 2 {
            return Err(Rejection::NotEnoughCoin { needed: 2 });
        }
        self.state.hunter.remove_item("coin", 2);
        self.state.hunter.add_item("flintlock-shot", 1);
        self.log(
            EventKind::Item,
            self.catalogue
                .strings
                .ui_fill("log.trade.ammunition", &[("npc", &spec.name)]),
        );
        self.end_action();
        Ok(())
    }

    // -- Time-costing actions ----------------------------------------------------

    fn cmd_travel(&mut self) -> Result<(), Rejection> {
        if self.state.final_hunt {
            return Err(Rejection::TravelBlockedByFinalHunt);
        }
        let Some(exit) = self.exit_at(self.state.hunter.pos).cloned() else {
            return Err(Rejection::NotAtExit);
        };
        let fleeing = self.hostiles_aware_on_current_map();
        let destination = self.world.map(exit.to_map).name.clone();
        if fleeing {
            self.log(
                EventKind::Travel,
                self.catalogue
                    .strings
                    .ui_fill("log.travel.flee", &[("place", &destination)]),
            );
        } else {
            self.log(
                EventKind::Travel,
                self.catalogue
                    .strings
                    .ui_fill("log.travel.depart", &[("place", &destination)]),
            );
        }
        self.advance_clock_and_settle(ClockReason::Travel, |sim| {
            sim.state.current_map = exit.to_map;
            sim.state.hunter.pos = exit.to_point;
            sim.clear_encounter_buffs();
            sim.log_fill(
                EventKind::Travel,
                "log.travel.arrive",
                &[("place", &destination)],
            );
            if exit.ambush_route {
                let chance = sim.world.ambush_percent;
                if sim.state.rng.percent(chance) {
                    sim.spawn_ambush(exit.to_point);
                }
            }
            sim.note_arrival();
        });
        Ok(())
    }

    fn cmd_craft(&mut self, recipe_id: &str) -> Result<(), Rejection> {
        if !self.at_feature(|kind| matches!(kind, FeatureKind::Workstation)) {
            return Err(Rejection::NotAtWorkstation);
        }
        let Some(recipe) = self.catalogue.recipes.get(recipe_id).cloned() else {
            return Err(Rejection::MissingIngredients {
                recipe: recipe_id.to_owned(),
            });
        };
        // Check inputs (duplicates encode quantity).
        let mut needed = std::collections::BTreeMap::new();
        for input in &recipe.inputs {
            *needed.entry(input.clone()).or_insert(0u16) += 1;
        }
        // A decisive counter must also be quenched in the reagent this case's
        // origin demands, which is what makes reading the origin matter.
        if recipe.requires_origin_reagent {
            let reagent = self.origin_reagent().to_owned();
            *needed.entry(reagent).or_insert(0u16) += 1;
        }
        for (item, count) in &needed {
            if self.state.hunter.item_count(item) < *count {
                // Name the reagent specifically: "you lack the ingredients" is
                // useless when the missing piece is the one you had to deduce.
                if recipe.requires_origin_reagent && *item == self.origin_reagent() {
                    return Err(Rejection::MissingOriginReagent {
                        reagent: self.item_name(item),
                    });
                }
                return Err(Rejection::MissingIngredients {
                    recipe: self.text(&recipe.name).to_owned(),
                });
            }
        }
        for (item, count) in &needed {
            self.state.hunter.remove_item(item, *count);
        }
        self.state.hunter.add_item(&recipe.output, 1);
        let output = self.item_name(&recipe.output);
        self.log_fill(EventKind::Item, "log.craft.done", &[("item", &output)]);
        if recipe.requires_origin_reagent {
            let flavour = self.catalogue.origins[&self.world.villain.origin]
                .counter_flavour
                .clone();
            self.log_id(EventKind::Item, &flavour);
        }
        self.end_action();
        Ok(())
    }

    /// The item id this case's origin demands in every decisive counter.
    pub fn origin_reagent(&self) -> &str {
        &self.catalogue.origins[&self.world.villain.origin].counter_reagent
    }

    fn cmd_consecrate(&mut self) -> Result<(), Rejection> {
        if self.state.final_hunt {
            return Err(Rejection::TravelBlockedByFinalHunt);
        }
        if !self.at_feature(|kind| matches!(kind, FeatureKind::Altar)) {
            return Err(Rejection::NotAtAltar);
        }
        if self.state.church_consecrated {
            return Err(Rejection::AlreadyConsecrated);
        }
        self.state.church_consecrated = true;
        self.log(
            EventKind::Clock,
            self.catalogue
                .strings
                .ui("log.rite.consecration")
                .to_owned(),
        );
        self.advance_clock_and_settle(ClockReason::CostlyAction, |sim| sim.note_arrival());
        Ok(())
    }

    fn cmd_open_grave(&mut self, feature_id: FeatureId) -> Result<(), Rejection> {
        let map = self.state.current_map;
        let Some(feature) = self.world.map(map).feature(feature_id).cloned() else {
            return Err(Rejection::NotAtGrave);
        };
        let FeatureKind::Grave { contents } = feature.kind else {
            return Err(Rejection::NotAtGrave);
        };
        if self.state.hunter.pos != feature.at && !self.state.hunter.pos.is_adjacent(feature.at) {
            return Err(Rejection::NotAdjacent);
        }
        if self.state.opened_graves.contains(&feature_id) {
            return Err(Rejection::GraveAlreadyOpened);
        }
        if self.state.hunter.physical < 1 {
            return Err(Rejection::PoolEmpty {
                pool: PoolKind::Physical,
                needed: 1,
            });
        }
        self.state.hunter.physical -= 1;
        self.state.opened_graves.insert(feature_id);
        self.log(
            EventKind::Clue,
            self.catalogue
                .strings
                .ui_fill("log.grave.open", &[("grave", &feature.name)]),
        );
        match contents {
            GraveContents::Villain => self.expose_dormant_villain(map, feature.at),
            GraveContents::Mundane => self.log(
                EventKind::Clue,
                self.catalogue.strings.ui("log.grave.mundane").to_owned(),
            ),
            GraveContents::Empty => self.log(
                EventKind::Clue,
                self.catalogue.strings.ui("log.grave.empty").to_owned(),
            ),
        }
        self.end_action();
        Ok(())
    }

    fn cmd_uncover(&mut self) -> Result<(), Rejection> {
        if self.state.villain_uncovered {
            return Err(Rejection::AlreadyUncovered);
        }
        let proof = self.state.corroboration(&self.catalogue);
        if !proof.corroborated() {
            return Err(Rejection::NeedMoreIdentityClues {
                have: proof.have,
                need: proof.need,
            });
        }
        // Ambiguous signs agreeing with each other prove nothing: at least one
        // proof must positively rule an alternative out.
        if !proof.decisive {
            return Err(Rejection::EvidenceNotDecisive);
        }
        self.state.villain_uncovered = true;
        self.state.villain_location_known = true;
        let title = self.world.villain.title.clone();
        self.log(
            EventKind::Clue,
            self.catalogue
                .strings
                .ui_fill("log.accusation.correct", &[("quarry", &title)]),
        );
        // Uncovering is a realisation, not an action: no world tick.
        Ok(())
    }

    // -- Helpers -----------------------------------------------------------------

    /// Ward charges the villain stands up at its current threat tier.
    pub fn villain_ward_charges(&self) -> u8 {
        let def = self.villain_def();
        let Some(ward) = &def.ward else { return 0 };
        let tier = usize::from(self.state.villain.tier);
        let bonus: u8 = def
            .tier_behaviours
            .iter()
            .take(tier)
            .map(|behaviour| match behaviour.effect {
                rh_content::TierEffect::WardCharges { amount } => amount,
                _ => 0,
            })
            .sum();
        ward.charges + bonus
    }

    pub fn villain_def(&self) -> &rh_content::VillainDef {
        &self.catalogue.villains[&self.world.villain.archetype]
    }

    fn live_actor(&self, id: ActorId) -> Result<&Actor, Rejection> {
        self.state
            .actor(id)
            .filter(|actor| actor.hp > 0 && actor.map == self.state.current_map)
            .ok_or(Rejection::NoSuchTarget)
    }

    fn require_npc_present(&self, npc: NpcId) -> Result<(), Rejection> {
        let spec = self.world.npc(npc);
        let npc_state = &self.state.npcs[npc.0 as usize];
        if spec.map != self.state.current_map || !npc_state.alive || npc_state.fled {
            return Err(Rejection::NpcUnavailable);
        }
        Ok(())
    }

    fn require_npc_adjacent(&self, npc: NpcId) -> Result<(), Rejection> {
        self.require_npc_present(npc)?;
        let pos = self.state.npcs[npc.0 as usize].pos;
        if !self.state.hunter.pos.is_adjacent(pos) {
            return Err(Rejection::NotAdjacent);
        }
        Ok(())
    }

    fn melee_damage(&self) -> u16 {
        self.best_melee().0
    }

    /// The best melee option carried, and whether it is the counter that cuts
    /// through this villain's ward (cold iron against the Witch).
    fn best_melee(&self) -> (u16, bool) {
        let weakness = &self.villain_def().weakness_item;
        self.state
            .hunter
            .inventory
            .keys()
            .filter_map(
                |item| match self.catalogue.items.get(item).map(|def| &def.kind) {
                    Some(ItemKind::MeleeWeapon { damage }) => Some((*damage, false)),
                    Some(ItemKind::WeaknessBlade { damage }) => Some((*damage, item == weakness)),
                    _ => None,
                },
            )
            .max_by_key(|(damage, _)| *damage)
            .unwrap_or((1, false))
    }

    fn ranged_weapon(&self) -> Option<(u8, u16)> {
        self.state.hunter.inventory.keys().find_map(|item| {
            match self.catalogue.items.get(item).map(|def| &def.kind) {
                Some(ItemKind::RangedWeapon { damage, range, .. }) => Some((*range, *damage)),
                _ => None,
            }
        })
    }

    fn take_melee_multiplier(&mut self) -> u8 {
        self.state.hunter.melee_multiplier.take().unwrap_or(2)
    }

    fn item_name(&self, id: &str) -> String {
        // Falling back to the raw id keeps an unknown item legible in the log
        // rather than blank.
        self.catalogue
            .items
            .get(id)
            .map(|def| self.text(&def.name).to_owned())
            .unwrap_or_else(|| id.to_owned())
    }

    fn exit_at(&self, at: Point) -> Option<&crate::world::ExitSpec> {
        self.world
            .map(self.state.current_map)
            .exits
            .iter()
            .find(|exit| exit.at == at)
    }

    fn at_feature(&self, predicate: impl Fn(&FeatureKind) -> bool) -> bool {
        self.world
            .map(self.state.current_map)
            .features
            .iter()
            .any(|feature| {
                predicate(&feature.kind)
                    && (self.state.hunter.pos == feature.at
                        || self.state.hunter.pos.is_adjacent(feature.at))
            })
    }

    fn hostiles_aware_on_current_map(&self) -> bool {
        self.state
            .actors
            .iter()
            .any(|actor| actor.map == self.state.current_map && actor.hp > 0 && actor.awake)
    }

    pub(crate) fn clear_encounter_buffs(&mut self) {
        self.state.hunter.sure_shot = false;
        self.state.hunter.melee_multiplier = None;
    }

    pub(crate) fn refresh_senses(&mut self) {
        self.refresh_senses_announcing(true);
    }

    /// Sense the world without narrating what is found.
    ///
    /// Used once, at the start of a run: the opening prose is the only thing
    /// worth reading at turn zero, and a dozen "something worth a closer look"
    /// lines would push it off the log before the first frame is drawn. The
    /// map already glows where those opportunities are, so nothing is lost by
    /// staying quiet about them.
    pub(crate) fn refresh_senses_quietly(&mut self) {
        self.refresh_senses_announcing(false);
    }

    fn refresh_senses_announcing(&mut self, announce: bool) {
        let radius = self
            .catalogue
            .balance
            .vision
            .fov_radius
            .saturating_add(self.state.sight_bonus)
            .saturating_sub(self.state.sight_penalty)
            .max(1);
        fov::refresh_visibility(&mut self.state, &self.world, radius);
        self.discovery_pass(announce);
    }

    /// Discover sight-based opportunities and meet visible NPCs.
    fn discovery_pass(&mut self, announce: bool) {
        let map = self.state.current_map;
        // Meet NPCs the hunter can currently see.
        let met: Vec<NpcId> = self
            .world
            .npcs
            .iter()
            .zip(self.state.npcs.iter())
            .filter(|(spec, npc)| {
                spec.map == map
                    && npc.alive
                    && !npc.fled
                    && self.state.is_visible(npc.pos)
                    && !self.state.met_npcs.contains(&spec.id)
            })
            .map(|(spec, _)| spec.id)
            .collect();
        for npc in met {
            self.state.met_npcs.insert(npc);
            if !announce {
                continue;
            }
            let spec = self.world.npc(npc);
            let archetype = self
                .catalogue
                .npcs
                .archetypes
                .get(&spec.archetype)
                .map(|def| self.text(&def.name))
                .unwrap_or_default();
            self.log(
                EventKind::Social,
                self.catalogue.strings.ui_fill(
                    "log.social.mark-npc",
                    &[("npc", &spec.name), ("role", &archetype.to_lowercase())],
                ),
            );
        }
        // Discover opportunities whose anchor is now visible.
        let discovered: Vec<(OpportunityId, String)> = self
            .world
            .opportunities
            .iter()
            .filter(|opp| {
                if opp.map != map || self.state.discovered.contains(&opp.id) {
                    return false;
                }
                let sight_allowed = matches!(
                    opp.discovery,
                    DiscoveryRule::Sight | DiscoveryRule::SightOr(_)
                );
                if !sight_allowed {
                    return false;
                }
                match opp.anchor {
                    OpportunityAnchor::Tile(at) => self.state.is_visible(at),
                    OpportunityAnchor::Npc(npc) => {
                        let npc_state = &self.state.npcs[npc.0 as usize];
                        npc_state.alive && !npc_state.fled && self.state.is_visible(npc_state.pos)
                    }
                }
            })
            .map(|opp| (opp.id, opp.name.clone()))
            .collect();
        for (id, name) in discovered {
            self.state.discovered.insert(id);
            if !announce {
                continue;
            }
            self.log(
                EventKind::Clue,
                self.catalogue
                    .strings
                    .ui_fill("log.discovery.opportunity", &[("what", &name)]),
            );
        }
    }

    fn spawn_ambush(&mut self, near: Point) {
        let scheme = &self.catalogue.schemes[&self.world.villain.scheme];
        let enemy = scheme.minion_enemy.clone();
        let hp = self.catalogue.enemies[&enemy].health;
        self.log(
            EventKind::Travel,
            self.catalogue.strings.ui("log.travel.ambush").to_owned(),
        );
        let map = self.state.current_map;
        let spots = self.free_tiles_near(map, near, 3);
        for spot in spots.into_iter().take(2) {
            let id = self
                .state
                .spawn_actor(ActorKind::Enemy(enemy.clone()), map, spot, hp);
            if let Some(actor) = self.state.actor_mut(id) {
                actor.awake = true;
            }
        }
    }

    pub(crate) fn free_tiles_near(&self, map: MapId, near: Point, radius: i16) -> Vec<Point> {
        let mut tiles: Vec<Point> = Vec::new();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let point = Point::new(near.x + dx, near.y + dy);
                if point == near || !point.in_bounds() {
                    continue;
                }
                if !is_walkable(self.state.terrain(&self.world, map, point)) {
                    continue;
                }
                if self.state.tile_occupied(&self.world, map, point) {
                    continue;
                }
                tiles.push(point);
            }
        }
        tiles.sort_by_key(|point| (near.distance(*point), point.y, point.x));
        tiles
    }

    /// Put the villain on the board: spawn its actor at tier-adjusted health
    /// with its ward woven, and flip every fact that must move together —
    /// active, actor id, uncovered, location known. Every reveal path goes
    /// through here so none can half-materialise it. `dormant` is the coup
    /// window a grave-opened villain wakes through; the other paths pass 0.
    fn materialise_villain(&mut self, map: MapId, at: Point, dormant: u8) {
        let def = self.villain_def().clone();
        let tier_hp = def.health + def.tier_bonus_health * u16::from(self.state.villain.tier);
        let id = self.state.spawn_actor(ActorKind::Villain, map, at, tier_hp);
        let ward = self.villain_ward_charges();
        if let Some(actor) = self.state.actor_mut(id) {
            actor.dormant = dormant;
            actor.awake = true;
            actor.ward_charges = ward;
        }
        self.state.villain.active = true;
        self.state.villain.actor = Some(id);
        self.state.villain_uncovered = true;
        self.state.villain_location_known = true;
    }

    fn expose_dormant_villain(&mut self, map: MapId, at: Point) {
        let def = self.villain_def().clone();
        self.materialise_villain(map, at, 3);
        self.log(
            EventKind::Telegraph,
            self.catalogue.strings.ui_fill(
                "log.villain.grave-opened",
                &[("villain", self.catalogue.strings.get(&def.name))],
            ),
        );
    }

    /// Attack an NPC in melee: host reveal or innocent fallout.
    fn attack_npc(&mut self, npc: NpcId, damage: u16, _ranged: bool) -> Result<(), Rejection> {
        if self.world.villain.host == Some(npc) {
            let at = self.state.npcs[npc.0 as usize].pos;
            self.reveal_host(npc, at);
            // The strike that outed it lands on the transformed villain.
            if let Some(actor_id) = self.state.villain.actor {
                let multiplier = self.take_melee_multiplier();
                let final_damage = damage * u16::from(multiplier) / combat::MULTIPLIER_HALVES;
                self.hunter_strike(actor_id, final_damage, false, false);
            }
            return Ok(());
        }
        self.harm_innocent(npc, damage);
        Ok(())
    }

    fn ranged_attack_npc(
        &mut self,
        npc: NpcId,
        damage: u16,
        sure: bool,
        silver: bool,
    ) -> Result<(), Rejection> {
        if self.world.villain.host == Some(npc) {
            let at = self.state.npcs[npc.0 as usize].pos;
            self.reveal_host(npc, at);
            if let Some(actor_id) = self.state.villain.actor {
                self.hunter_ranged_strike(actor_id, damage, silver, sure);
            }
            return Ok(());
        }
        self.harm_innocent(npc, damage);
        Ok(())
    }

    fn reveal_host(&mut self, npc: NpcId, at: Point) {
        let name = self.world.npc(npc).name.clone();
        let def = self.villain_def().clone();
        self.state.npcs[npc.0 as usize].fled = true;
        self.materialise_villain(self.state.current_map, at, 0);
        self.log(
            EventKind::Telegraph,
            self.catalogue.strings.ui_fill(
                "log.villain.unmasked",
                &[
                    ("name", &name),
                    ("villain", self.catalogue.strings.get(&def.name)),
                ],
            ),
        );
    }

    fn harm_innocent(&mut self, npc: NpcId, damage: u16) {
        let combat = &self.catalogue.balance.combat;
        let hit = self.state.rng.percent(combat.melee_hit_percent);
        let name = self.world.npc(npc).name.clone();
        let killed = {
            let npc_state = &mut self.state.npcs[npc.0 as usize];
            npc_state.attacked = true;
            if hit {
                npc_state.hp = npc_state.hp.saturating_sub(damage.max(1));
            }
            if npc_state.hp == 0 {
                npc_state.alive = false;
                true
            } else {
                npc_state.fled = true;
                false
            }
        };
        if killed {
            self.log(
                EventKind::Combat,
                self.catalogue
                    .strings
                    .ui_fill("log.social.murdered", &[("npc", &name)]),
            );
        } else {
            self.log(
                EventKind::Combat,
                self.catalogue
                    .strings
                    .ui_fill("log.social.wounded-innocent", &[("npc", &name)]),
            );
        }
        self.apply_innocent_fallout(npc);
    }

    fn apply_innocent_fallout(&mut self, npc: NpcId) {
        self.state.settlement_hostile = true;
        // Everything anchored to this NPC is lost.
        let lost: Vec<OpportunityId> = self
            .world
            .opportunities
            .iter()
            .filter(|opp| opp.anchor == OpportunityAnchor::Npc(npc))
            .filter(|opp| !self.state.resolved.contains(&opp.id))
            .map(|opp| opp.id)
            .collect();
        for id in lost {
            self.state.lost.insert(id);
        }
        self.log(
            EventKind::Social,
            self.catalogue
                .strings
                .ui("log.social.settlement-turns")
                .to_owned(),
        );
    }

    // -- Strikes against hostile actors -------------------------------------------

    /// Resolve a hunter melee-class strike against an actor.
    pub(crate) fn hunter_strike(&mut self, id: ActorId, damage: u16, coup: bool, _ranged: bool) {
        let combat = self.catalogue.balance.combat.clone();
        let hit = coup || self.state.rng.percent(combat.melee_hit_percent);
        if !hit {
            self.log_ui(EventKind::Combat, "log.combat.melee-miss");
            return;
        }
        // A coup de grace on a sleeping thing lands with terrible weight.
        let final_damage = if coup {
            damage * combat::COUP_MULTIPLIER
        } else {
            damage
        };
        let cuts_the_ward = self.best_melee().1;
        self.deal_damage_to_actor(id, final_damage, cuts_the_ward);
    }

    pub(crate) fn hunter_ranged_strike(
        &mut self,
        id: ActorId,
        damage: u16,
        silver: bool,
        sure: bool,
    ) {
        let combat = self.catalogue.balance.combat.clone();
        let coup = self
            .state
            .actor(id)
            .map(|actor| actor.dormant > 0)
            .unwrap_or(false);
        let hit = sure || coup || self.state.rng.percent(combat.ranged_hit_percent);
        if !hit {
            self.log(
                EventKind::Combat,
                self.catalogue
                    .strings
                    .ui("log.combat.ranged-miss")
                    .to_owned(),
            );
            return;
        }
        // A coup de grace on a sleeping thing lands with terrible weight.
        let final_damage = if coup {
            damage * combat::COUP_MULTIPLIER
        } else {
            damage
        };
        self.deal_damage_to_actor(id, final_damage, silver);
    }

    /// Apply damage with villain vulnerability gating; handles death and loot.
    pub(crate) fn deal_damage_to_actor(&mut self, id: ActorId, damage: u16, weakness: bool) {
        let Some(actor) = self.state.actor(id) else {
            return;
        };
        let is_villain = actor.kind == ActorKind::Villain;
        let was_dormant = actor.dormant > 0;
        let mut damage = damage;

        if is_villain {
            let def = self.villain_def().clone();
            let vulnerable = self.villain_is_vulnerable(id);
            if let Some(cadence) = &def.cadence {
                if !vulnerable && !was_dormant {
                    self.log_id(EventKind::Telegraph, &cadence.guarded_telegraph);
                    return;
                }
                // Blows land twice as deep in a vulnerability window. The
                // dormant coup already multiplies at the strike level.
                if vulnerable && !was_dormant {
                    damage *= combat::VULNERABILITY_MULTIPLIER;
                }
            }
            // A hex-ward soaks honest blows until it tears. The counter cuts
            // straight through it and is never absorbed.
            if !weakness {
                if let Some(ward) = def.ward.clone() {
                    let charges = self.state.actor(id).map(|a| a.ward_charges).unwrap_or(0);
                    if charges > 0 {
                        let remaining = charges - 1;
                        if let Some(actor) = self.state.actor_mut(id) {
                            actor.ward_charges = remaining;
                            if remaining == 0 {
                                actor.ward_reweave = ward.reweave_turns;
                            }
                        }
                        damage = ward.leak_damage;
                        let telegraph = if remaining == 0 {
                            ward.break_telegraph.clone()
                        } else {
                            ward.absorb_telegraph.clone()
                        };
                        self.log_id(EventKind::Telegraph, &telegraph);
                    }
                }
            }
            if weakness {
                if let Some(regen) = &def.regeneration {
                    let already = self
                        .state
                        .actor(id)
                        .map(|a| a.regen_stopped)
                        .unwrap_or(true);
                    if !already {
                        if let Some(actor) = self.state.actor_mut(id) {
                            actor.regen_stopped = true;
                        }
                        let _ = regen;
                        self.log(
                            EventKind::Telegraph,
                            self.catalogue
                                .strings
                                .ui("log.combat.silver-bites")
                                .to_owned(),
                        );
                    }
                }
            }
        }

        // Waking a dormant villain.
        if was_dormant {
            if let Some(actor) = self.state.actor_mut(id) {
                actor.dormant = 0;
            }
            if is_villain {
                self.log(
                    EventKind::Telegraph,
                    self.catalogue.strings.ui("log.villain.rises").to_owned(),
                );
            }
        }

        let (killed, kind, pos) = {
            let Some(actor) = self.state.actor_mut(id) else {
                return;
            };
            actor.hp = actor.hp.saturating_sub(damage);
            actor.awake = true;
            (actor.hp == 0, actor.kind.clone(), actor.pos)
        };
        let name = self.actor_name(&kind);
        self.log(
            EventKind::Combat,
            self.catalogue.strings.ui_fill(
                "log.combat.strike",
                &[("target", &name), ("damage", &damage.to_string())],
            ),
        );
        if killed {
            self.handle_actor_death(id, kind, pos);
        }
    }

    /// Whether the villain currently takes damage (vulnerability window,
    /// binding, consecrated ground, or dormancy). Public so clients and the
    /// autoplayer can present and act on the telegraphed state.
    pub fn villain_is_vulnerable(&self, id: ActorId) -> bool {
        let Some(actor) = self.state.actor(id) else {
            return false;
        };
        if actor.kind != ActorKind::Villain {
            return true;
        }
        let def = self.villain_def();
        let Some(cadence) = &def.cadence else {
            // No cadence (werewolf): always woundable; regeneration defends it.
            return true;
        };
        if actor.dormant > 0 || actor.bound > 0 {
            return true;
        }
        if def.affected_by_consecration
            && self.state.church_consecrated
            && self
                .world
                .map(actor.map)
                .consecration_area
                .contains(&actor.pos)
        {
            return true;
        }
        actor.cadence == cadence.period - 1
    }

    /// Display name for an actor kind (clients and the autoplayer use it).
    pub fn actor_name(&self, kind: &ActorKind) -> String {
        match kind {
            ActorKind::Enemy(enemy) => self
                .catalogue
                .enemies
                .get(enemy)
                .map(|def| self.text(&def.name).to_owned())
                .unwrap_or_else(|| enemy.clone()),
            ActorKind::Villain => self.text(&self.villain_def().name).to_owned(),
        }
    }

    fn handle_actor_death(&mut self, id: ActorId, kind: ActorKind, pos: Point) {
        match kind {
            ActorKind::Villain => {
                self.state.villain.dead = true;
                self.state.villain.active = false;
                self.state.outcome = Some(Outcome::Victory);
                let name = self.text(&self.villain_def().name).to_owned();
                self.log_fill(
                    EventKind::System,
                    "log.combat.villain-destroyed",
                    &[("villain", &name)],
                );
            }
            ActorKind::Enemy(_) => {
                let name = self.actor_name(&kind);
                self.log_fill(EventKind::Combat, "log.combat.slain", &[("target", &name)]);
                let chance = self.catalogue.balance.loot.drop_percent;
                if self.state.rng.percent(chance) {
                    self.drop_loot(pos);
                }
            }
        }
        let _ = id;
    }

    /// Seed-determined low-chance loot: ammunition, ingredients, coin, or a
    /// clue hint. Certified routes never depend on these.
    fn drop_loot(&mut self, _pos: Point) {
        let table = [
            "flintlock-shot",
            "coin",
            "moon-herb",
            "bitter-root",
            "clue-hint",
        ];
        let pick = table[self.state.rng.index(table.len())];
        if pick == "clue-hint" {
            // Reveal the nearest undiscovered sight-based opportunity here.
            let map = self.state.current_map;
            let hunter = self.state.hunter.pos;
            let next = self
                .world
                .opportunities
                .iter()
                .filter(|opp| {
                    opp.map == map
                        && !self.state.discovered.contains(&opp.id)
                        && matches!(
                            opp.discovery,
                            DiscoveryRule::Sight | DiscoveryRule::SightOr(_)
                        )
                })
                .min_by_key(|opp| match opp.anchor {
                    OpportunityAnchor::Tile(at) => hunter.distance(at),
                    OpportunityAnchor::Npc(npc) => {
                        hunter.distance(self.state.npcs[npc.0 as usize].pos)
                    }
                })
                .map(|opp| (opp.id, opp.name.clone()));
            if let Some((id, name)) = next {
                self.state.discovered.insert(id);
                self.log(
                    EventKind::Clue,
                    self.catalogue
                        .strings
                        .ui_fill("log.clue.scrap", &[("what", &name)]),
                );
            } else {
                self.state.hunter.add_item("coin", 1);
                self.log_ui(EventKind::Item, "log.loot.coin");
            }
        } else {
            self.state.hunter.add_item(pick, 1);
            let name = self.item_name(pick);
            self.log_fill(EventKind::Item, "log.loot.dropped", &[("item", &name)]);
        }
    }

    // -- Clock --------------------------------------------------------------------

    /// Advance the global clock, run the caller's arrival work once the
    /// day's events have fired, then meet the clock's obligations: final-hunt
    /// onset on the map the hunter actually occupies, and a senses refresh.
    /// Every clock-advancing command goes through here, so none can forget
    /// to start the hunt.
    fn advance_clock_and_settle(&mut self, reason: ClockReason, settle: impl FnOnce(&mut Self)) {
        self.advance_clock(reason);
        if self.state.outcome.is_some() {
            return;
        }
        settle(self);
        self.maybe_begin_final_hunt();
        self.refresh_senses();
    }

    fn advance_clock(&mut self, reason: ClockReason) {
        self.state.clock += 1;
        let clock = self.state.clock;
        self.log(
            EventKind::Clock,
            self.catalogue.strings.ui_fill(
                "log.clock.day-turns",
                &[
                    ("spent", &clock.to_string()),
                    (
                        "total",
                        &self.catalogue.balance.clock.travel_turns.to_string(),
                    ),
                ],
            ),
        );

        // What the clock restores is the economy's to say; the planner
        // budgets from the same answer.
        let restore = crate::economy::clock_restore(reason);
        let caps = &self.catalogue.hunter;
        if restore.physical {
            self.state.hunter.physical = (self.state.hunter.physical + 1).min(caps.physical_cap);
        }
        if restore.investigation_pools {
            self.state.hunter.lore = (self.state.hunter.lore + 1).min(caps.lore_cap);
            self.state.hunter.social = (self.state.hunter.social + 1).min(caps.social_cap);
            self.state.hunter.mystic = (self.state.hunter.mystic + 1).min(caps.mystic_cap);
        }

        let clock_balance = self.catalogue.balance.clock.clone();
        if clock == clock_balance.minor_event_turn {
            self.fire_scheme_event(false);
        }
        if clock == clock_balance.major_event_turn {
            self.fire_scheme_event(true);
        }
        // Final-hunt onset belongs to advance_clock_and_settle, after the
        // caller's arrival work, so the villain appears on the map the
        // hunter actually occupies.
    }

    fn fire_scheme_event(&mut self, major: bool) {
        let scheme = self.catalogue.schemes[&self.world.villain.scheme].clone();
        // A scheme pre-empted in time still stirs, but its escalation fails:
        // no tier gained, no fresh minions.
        if major && self.state.scheme_preempted {
            self.log_id(EventKind::Clock, &scheme.preempt.blunted_text);
            return;
        }
        let event = if major {
            &scheme.major_event
        } else {
            &scheme.minor_event
        };
        self.log_id(EventKind::Clock, &event.text);

        // Threat tier up: +health and the next enhanced behaviour.
        let def = self.villain_def().clone();
        self.state.villain.tier =
            (self.state.villain.tier + 1).min(def.tier_behaviours.len() as u8);
        let tier = self.state.villain.tier;
        if let Some(behaviour) = def.tier_behaviours.get(usize::from(tier) - 1) {
            self.log_id(EventKind::Telegraph, &behaviour.telegraph);
        }
        if let Some(actor_id) = self.state.villain.actor {
            if let Some(actor) = self.state.actor_mut(actor_id) {
                actor.max_hp += def.tier_bonus_health;
                actor.hp += def.tier_bonus_health;
            }
        }

        // Spawn minions on the event's map.
        let site_map = self
            .world
            .maps
            .iter()
            .position(|map| map.template == event.site_map)
            .map(|index| MapId(index as u8));
        if let Some(map) = site_map {
            let enemy = scheme.minion_enemy.clone();
            let hp = self.catalogue.enemies[&enemy].health;
            // Minions gather at the scheme's mark — the kill site if the map
            // has one, else the map's far reaches — drawn to the villain's
            // work rather than camped on the roads travellers arrive by.
            let world_map = self.world.map(map);
            let kill_site = world_map
                .features
                .iter()
                .find(|feature| feature.kind == FeatureKind::KillSite)
                .map(|feature| feature.at);
            let anchor = kill_site.unwrap_or_else(|| {
                let entry = world_map.entry;
                let mut best = entry;
                let mut best_distance = -1;
                for y in 0..crate::geometry::MAP_HEIGHT {
                    for x in 0..crate::geometry::MAP_WIDTH {
                        let point = Point::new(x, y);
                        if !is_walkable(self.state.terrain(&self.world, map, point)) {
                            continue;
                        }
                        let distance = entry.distance(point);
                        if distance > best_distance {
                            best_distance = distance;
                            best = point;
                        }
                    }
                }
                best
            });
            let spots = self.free_tiles_near(map, anchor, 5);
            for spot in spots.into_iter().take(usize::from(event.spawn_minions)) {
                self.state
                    .spawn_actor(ActorKind::Enemy(enemy.clone()), map, spot, hp);
            }
        }
    }

    fn maybe_begin_final_hunt(&mut self) {
        if self.state.final_hunt
            || self.state.villain.dead
            || self.state.clock < self.catalogue.balance.clock.travel_turns
        {
            return;
        }
        self.state.final_hunt = true;
        let def = self.villain_def().clone();
        let map = self.state.current_map;

        // The villain appears somewhere on the current map and pursues.
        if let Some(actor_id) = self.state.villain.actor {
            if let Some(actor) = self.state.actor_mut(actor_id) {
                if actor.hp > 0 {
                    actor.map = map;
                    actor.awake = true;
                    actor.dormant = 0;
                }
            }
            let needs_respawn = self
                .state
                .actor(actor_id)
                .map(|actor| actor.hp == 0)
                .unwrap_or(true);
            if !needs_respawn {
                let far = self.farthest_free_tile(map);
                if let Some(actor) = self.state.actor_mut(actor_id) {
                    actor.pos = far;
                }
                self.log(
                    EventKind::Clock,
                    self.catalogue.strings.ui_fill(
                        "log.villain.final-night-hunts",
                        &[("villain", self.catalogue.strings.get(&def.name))],
                    ),
                );
                return;
            }
        }
        let far = self.farthest_free_tile(map);
        self.materialise_villain(map, far, 0);
        // If the villain was hiding in an NPC, that mask is now gone.
        if let Some(host) = self.world.villain.host {
            self.state.npcs[host.0 as usize].fled = true;
        }
        self.log(
            EventKind::Clock,
            self.catalogue.strings.ui_fill(
                "log.villain.final-night-drops-pretence",
                &[("villain", self.catalogue.strings.get(&def.name))],
            ),
        );
    }

    fn farthest_free_tile(&self, map: MapId) -> Point {
        let hunter = self.state.hunter.pos;
        let mut best = hunter;
        let mut best_distance = -1;
        for y in 0..crate::geometry::MAP_HEIGHT {
            for x in 0..crate::geometry::MAP_WIDTH {
                let point = Point::new(x, y);
                if !is_walkable(self.state.terrain(&self.world, map, point)) {
                    continue;
                }
                if self.state.tile_occupied(&self.world, map, point) {
                    continue;
                }
                let distance = hunter.distance(point);
                if distance > best_distance {
                    best_distance = distance;
                    best = point;
                }
            }
        }
        best
    }

    // -- Hunter death ----------------------------------------------------------------

    pub(crate) fn handle_hunter_death(&mut self) {
        if self.state.final_hunt {
            self.state.outcome = Some(Outcome::Defeat);
            self.log(
                EventKind::System,
                self.catalogue
                    .strings
                    .ui("log.outcome.final-death")
                    .to_owned(),
            );
            return;
        }
        self.log(
            EventKind::System,
            self.catalogue
                .strings
                .ui("log.outcome.death-costs-a-day")
                .to_owned(),
        );
        let settlement = self.world.map_by_role(rh_content::MapRole::Settlement);
        self.state.current_map = settlement;
        self.state.hunter.pos = self.world.map(settlement).entry;
        self.state.hunter.hp = self.state.hunter.max_hp;
        self.clear_encounter_buffs();
        // A lost villain fight sends the villain back to its haunts, healed.
        if let Some(actor_id) = self.state.villain.actor {
            let lair = self.world.villain.lair;
            if let Some(actor) = self.state.actor_mut(actor_id) {
                if actor.hp > 0 {
                    actor.map = lair.0;
                    actor.pos = lair.1;
                    actor.awake = false;
                    actor.hp = actor.max_hp;
                }
            }
        }
        self.advance_clock_and_settle(ClockReason::Death, |_| {});
    }

    /// One world tick after a hunter action: enemies, NPCs, timers, senses.
    fn end_action(&mut self) {
        crate::ai::world_tick(self);
        self.state.local_turn += 1;
        let cap = self.catalogue.hunter.stamina_cap;
        let regen = self.catalogue.balance.combat.stamina_regen_per_turn;
        self.state.hunter.stamina = (self.state.hunter.stamina + regen).min(cap);
        if self.state.outcome.is_none() && self.state.hunter.hp == 0 {
            self.handle_hunter_death();
        }
        if self.state.outcome.is_none() {
            self.refresh_senses();
        }
    }

    // -- Test hooks --------------------------------------------------------------
    //
    // Damage resolution and scheme events are internal because commands are the
    // only supported way in. Reaching a warded villain or a major scheme event
    // through commands alone takes a full winning run, which is what the golden
    // replays already do; these let the mechanics be tested in isolation.

    #[doc(hidden)]
    pub fn deal_damage_to_actor_for_test(&mut self, id: ActorId, damage: u16, weakness: bool) {
        self.deal_damage_to_actor(id, damage, weakness);
    }

    #[doc(hidden)]
    pub fn fire_scheme_event_for_test(&mut self, major: bool) {
        self.fire_scheme_event(major);
    }

    /// Resolve an opportunity as though the hunter had walked to it, without
    /// the walking: machines sit at authored corners of the map, and testing
    /// their payoffs should not require replaying a route to each one.
    #[doc(hidden)]
    pub fn resolve_for_test(&mut self, id: OpportunityId) {
        let spec = self.world.opportunity(id).clone();
        self.state.discovered.insert(id);
        self.state.resolved.insert(id);
        self.apply_grant(&spec);
        self.cascade_discovery(id);
    }

    /// Draw the next optional event from a map's deck, as arrival does.
    #[doc(hidden)]
    pub fn fire_next_event_for_test(&mut self, map: MapId) {
        self.fire_next_event(map);
    }

    /// Put the villain on the board however this case conceals it, so combat
    /// rules can be exercised without playing a whole run up to the reveal.
    #[doc(hidden)]
    pub fn expose_villain_for_test(&mut self) -> Option<ActorId> {
        if let Some(host) = self.world.villain.host {
            let at = self.state.npcs[host.0 as usize].pos;
            self.state.current_map = self.world.npc(host).map;
            self.reveal_host(host, at);
        } else if let Some((map, feature)) = self.world.villain.grave {
            let at = self.world.map(map).feature(feature).map(|f| f.at)?;
            self.expose_dormant_villain(map, at);
        }
        self.state.villain.actor
    }
}
