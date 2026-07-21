//! The route-following autoplayer.
//!
//! Drives a run headlessly: follows the certified early route, uncovers the
//! villain, initiates the hunt, and fights with the same preparations the
//! viability heuristic assumed. Used to mint golden replays, exercise the
//! full command surface in CI, and prove generated runs are playable from
//! start to finish. Fully deterministic for a given seed.

use std::collections::{BTreeMap, VecDeque};

use rh_core::command::{Command, Target};
use rh_core::fov::is_walkable;
use rh_core::geometry::{Direction, Point, MAP_HEIGHT, MAP_WIDTH};
use rh_core::state::{ActorKind, Outcome};
use rh_core::viability::{hunt_viability, HuntLoadout};
use rh_core::world::{
    FeatureKind, MapId, OpportunityAnchor, OpportunityId, RouteAction, RouteStep,
};

use crate::RunSession;

/// Safety cap: a run that needs more actions than this has stalled.
const MAX_ACTIONS: u32 = 4000;
/// Consecutive no-progress actions before giving up.
const MAX_STALLS: u32 = 60;

/// How a driven run finished. Held apart from [`Outcome`] on purpose: the sim
/// knows only whether the hunter won or fell, because that is all a *world*
/// can know. Whether the bot gave up before finding out is a fact about the
/// driver, and putting it in `Outcome` would change the serialized run state
/// and invalidate every share code for a fact no player can observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    Victory,
    Defeat,
    /// The bot ran out of actions or made no progress for too long. Reported
    /// as a loss by callers that only want a win rate, but it is a different
    /// failure with a different remedy, and the two were indistinguishable
    /// until this existed.
    Stalled,
}

/// Where a run was lost. The point of the whole report: a win rate says the
/// estimate is wrong somewhere, and this says where to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossStage {
    Won,
    /// The clock forced the final hunt before the villain was ever named.
    NeverNamed,
    /// She reached the fight carrying far less than the route certified.
    ArrivedUnderprepared,
    /// She reached the fight with what was promised, and lost it anyway.
    FoughtBadly,
    /// She died on the way often enough that the clock ran out.
    DiedBeforeArriving,
    Stalled,
}

/// How far below the certified promise a rescored loadout may fall before the
/// loss is blamed on preparation rather than on the fight. A hundred permille
/// is a turn and a third of margin in the model's own currency.
const UNDERPREPARED_SHORTFALL: u16 = 100;

/// Everything a driven run can say about itself. Built by
/// [`autoplay_reported`]; [`autoplay`] throws all of it away but the outcome.
#[derive(Debug, Clone)]
pub struct AutoplayReport {
    pub outcome: Option<Outcome>,
    pub end: RunEnd,
    pub stage: LossStage,
    pub actions: u32,
    pub clock_at_end: u8,
    pub deaths_before_final: u8,
    pub villain_uncovered: bool,
    pub route_steps_done: usize,
    pub route_steps_total: usize,
    /// Steps abandoned by `STEP_BUDGET` rather than completed.
    pub route_steps_abandoned: usize,
    pub final_hunt_entered: bool,
    /// What the route promised, from `certified_routes.first()`.
    pub certified_permille: u16,
    /// What she was actually carrying when the final hunt began.
    pub loadout_at_final_hunt: Option<HuntLoadout>,
    /// The same estimate the planner ran, against the loadout she actually
    /// assembled. This is the field that splits the diagnosis: if it matches
    /// the promise and she still lost, the estimate is wrong; if it falls far
    /// short, the bot never assembled what was certified.
    pub rescored_permille: Option<u16>,
    pub draughts_unspent: u16,
    /// Commands the bot tried and the sim refused, by tag. A hunter whose
    /// finisher does not exist shows up here as a large count against it.
    pub command_rejections: BTreeMap<&'static str, u32>,
}

impl AutoplayReport {
    /// How far the rescored loadout fell short of the promise, if both known.
    pub fn shortfall(&self) -> Option<i32> {
        self.rescored_permille
            .map(|rescored| i32::from(self.certified_permille) - i32::from(rescored))
    }
}

/// Drive the session to an outcome. Returns `None` if the bot stalled.
pub fn autoplay(session: &mut RunSession) -> Option<Outcome> {
    autoplay_reported(session).outcome
}

/// Drive the session and report how it went, in enough detail to say which
/// stage of the run lost it.
pub fn autoplay_reported(session: &mut RunSession) -> AutoplayReport {
    // Follow the route that reaches a certified hunt soonest, not the one the
    // generator happened to list first. An earlier hunt is a shorter journey
    // through a deadly valley and a fight at a lower threat tier - both of
    // which a frail hunter feels most. Ties break toward the surer estimate.
    // The two routes are certified to be independently walkable, so choosing
    // between them is the driver's to make and changes nothing about the world.
    let route = session
        .sim
        .world
        .certified_routes
        .iter()
        .min_by_key(|route| (route.ready_by_turn, u16::MAX - route.viability_permille));
    let certified_permille = route.map(|route| route.viability_permille).unwrap_or(0);
    let steps: Vec<RouteStep> = route.map(|route| route.steps.clone()).unwrap_or_default();
    let route_steps_total = steps.len();
    let mut bot = Bot {
        steps,
        step_index: 0,
        stalls: 0,
        step_actions: 0,
        abandoned: 0,
        rejections: BTreeMap::new(),
    };
    let mut actions = 0u32;
    let mut stalled = false;
    let mut final_hunt_entered = session.sim.state.final_hunt;
    let mut loadout_at_final_hunt = None;
    let mut rescored_permille = None;
    let mut deaths_before_final = 0u8;
    let mut last_deaths = session.sim.state.deaths;

    for _ in 0..MAX_ACTIONS {
        if session.outcome().is_some() {
            break;
        }
        if bot.stalls >= MAX_STALLS {
            stalled = true;
            break;
        }
        let step_before = bot.step_index;
        let progress_before = (
            session.sim.state.clock,
            session.sim.state.current_map,
            session.sim.state.hunter.pos,
            session.sim.state.resolved.len(),
        );
        bot.act(session);
        actions += 1;
        // The moment the fight starts is the only moment worth pricing: it is
        // what the planner was estimating, and everything gathered afterwards
        // came too late to have been part of the promise.
        if !final_hunt_entered && session.sim.state.final_hunt {
            final_hunt_entered = true;
            let (loadout, rescored) = rescore(session);
            loadout_at_final_hunt = Some(loadout);
            rescored_permille = Some(rescored);
        }
        if session.sim.state.deaths > last_deaths {
            last_deaths = session.sim.state.deaths;
            if !final_hunt_entered {
                deaths_before_final = deaths_before_final.saturating_add(1);
            }
        }
        // A stuck route step gets skipped rather than stalling the run.
        if bot.step_index == step_before && bot.step_index < bot.steps.len() {
            bot.step_actions += 1;
            if bot.step_actions > STEP_BUDGET {
                bot.step_index += 1;
                bot.step_actions = 0;
                bot.abandoned += 1;
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

    let outcome = session.outcome();
    let end = match (outcome, stalled) {
        (Some(Outcome::Victory), _) => RunEnd::Victory,
        (Some(Outcome::Defeat), _) => RunEnd::Defeat,
        (None, _) => RunEnd::Stalled,
    };
    let state = &session.sim.state;
    let stage = classify(
        end,
        state.villain_uncovered,
        final_hunt_entered,
        certified_permille,
        rescored_permille,
    );
    AutoplayReport {
        outcome,
        end,
        stage,
        actions,
        clock_at_end: state.clock,
        deaths_before_final,
        villain_uncovered: state.villain_uncovered,
        route_steps_done: bot.step_index.min(route_steps_total),
        route_steps_total,
        route_steps_abandoned: bot.abandoned,
        final_hunt_entered,
        certified_permille,
        loadout_at_final_hunt,
        rescored_permille,
        draughts_unspent: state.hunter.item_count("wound-draught"),
        command_rejections: bot.rejections,
    }
}

/// Price what the hunter is actually carrying, using the same model and the
/// same loadout mapping the planner certified with. Deliberately calls
/// `HuntLoadout::from_kit` rather than assembling the fields here: a second
/// mapping is a second thing to drift.
fn rescore(session: &RunSession) -> (HuntLoadout, u16) {
    let state = &session.sim.state;
    let catalogue = &session.sim.catalogue;
    let loadout = HuntLoadout::from_kit(
        catalogue,
        |id| state.hunter.item_count(id),
        state.hunter.physical,
        state.church_consecrated
            && state.current_map
                == session
                    .sim
                    .world
                    .map_by_role(rh_content::MapRole::Settlement),
    );
    let viability = hunt_viability(
        catalogue,
        &session.sim.world.villain.archetype,
        state.villain.tier,
        &loadout,
    );
    (loadout, viability)
}

fn classify(
    end: RunEnd,
    villain_uncovered: bool,
    final_hunt_entered: bool,
    certified: u16,
    rescored: Option<u16>,
) -> LossStage {
    match end {
        RunEnd::Victory => LossStage::Won,
        RunEnd::Stalled => LossStage::Stalled,
        RunEnd::Defeat => {
            // Order matters, and an earlier version of this got it wrong by
            // asking about deaths first: a run that died twice on the way and
            // *then* reached the fight underprepared was filed as never having
            // arrived, which hid the largest signal in the corpus behind the
            // smallest one. Deaths are a cost along the way, not a stage; the
            // stage is decided by how far she got and what she had when she
            // got there, and `deaths_before_final` is reported alongside.
            if !villain_uncovered {
                // The clock dragged her into a fight with something she could
                // not name. Nothing about the fight is the story here.
                LossStage::NeverNamed
            } else if !final_hunt_entered {
                LossStage::DiedBeforeArriving
            } else if rescored
                .is_some_and(|r| certified.saturating_sub(r) >= UNDERPREPARED_SHORTFALL)
            {
                LossStage::ArrivedUnderprepared
            } else {
                LossStage::FoughtBadly
            }
        }
    }
}

struct Bot {
    steps: Vec<RouteStep>,
    step_index: usize,
    stalls: u32,
    /// Actions spent on the current route step; stuck steps get skipped.
    step_actions: u32,
    /// Steps given up on rather than completed.
    abandoned: usize,
    rejections: BTreeMap<&'static str, u32>,
}

impl Bot {
    /// Apply a command, recording a refusal against `tag`. The bot has always
    /// swallowed refusals — it has to, since it guesses at what is legal — but
    /// swallowing them silently is how a hunter came to spend a whole corpus
    /// asking for a finisher she does not own without anything noticing.
    fn try_apply(&mut self, session: &mut RunSession, tag: &'static str, command: Command) -> bool {
        match session.apply(command) {
            Ok(()) => true,
            Err(_) => {
                *self.rejections.entry(tag).or_insert(0) += 1;
                false
            }
        }
    }
}

/// Actions allowed per route step before the bot skips it.
const STEP_BUDGET: u32 = 150;

impl Bot {
    fn act(&mut self, session: &mut RunSession) {
        // Free knowledge action first: name the villain once proven.
        if session
            .sim
            .state
            .corroboration(&session.sim.catalogue)
            .met()
            && !session.sim.state.villain_uncovered
            && session.apply(Command::UncoverVillain).is_ok()
        {
            return;
        }

        // Emergency healing beats everything else; with draughts to spare,
        // top up after incidental scrapes too.
        //
        // Both thresholds are shares of the hunter's own health, not the flat
        // 5 and 8 they used to be. Those numbers were the Huntress's twelve
        // health read as though it were everyone's: on the Occultist's nine
        // they meant "drink at eight", which is a single scratch, so she spent
        // her draughts on the road and reached every fight with none. That was
        // the largest single reason she arrived underprepared.
        let hunter = &session.sim.state.hunter;
        let hunter_hp = hunter.hp;
        let max_hp = hunter.max_hp.max(1);
        let draughts = hunter.item_count("wound-draught");
        let below =
            |percent: u16| u32::from(hunter_hp) * 100 <= u32::from(max_hp) * u32::from(percent);
        // Away from the final fight she keeps one back: the estimate certified
        // the hunt on the draughts she would be carrying into it, and drinking
        // the last one on the way is how that promise quietly stops being true.
        let reserve = u16::from(!session.sim.state.final_hunt);
        let spare = draughts.saturating_sub(reserve);
        if ((below(40) && draughts > 0) || (below(65) && spare >= 1))
            && session.apply(Command::UseDraught).is_ok()
        {
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

        // Ordinary enemies that have our scent. How to answer them depends on
        // whether this hunter can win a brawl: a sturdy one with a finisher
        // clears them so they stop whittling her, but a frail one who trades
        // blow for blow against a pack loses the race and, with it, the days
        // the route was counting on. The diagnosis found the Occultist doing
        // exactly that — dying to thralls on the road, respawning, and
        // reaching the fight too late and too poor. A frail hunter kites and
        // keeps moving instead; the pack falls behind and the route gets done.
        let map = session.sim.state.current_map;
        let hunter = session.sim.state.hunter.pos;
        let awake: Vec<(rh_core::state::ActorId, Point)> = session
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
            .map(|actor| (actor.id, actor.pos))
            .collect();
        let nearest = awake
            .iter()
            .min_by_key(|(id, pos)| (pos.distance(hunter), id.0))
            .copied();
        if let Some((enemy_id, enemy_pos)) = nearest {
            let adjacent_count = awake
                .iter()
                .filter(|(_, pos)| hunter.is_adjacent(*pos))
                .count();
            // Frail is measured against the thing that kills her: a pack does
            // its full melee to her every turn, so what matters is whether she
            // can out-heal or out-kill it, which is a signature or the health
            // to trade. Without either, she does not stand in it.
            let brawler = session.sim.catalogue.hunter.physical_cap >= 2
                || session.sim.state.hunter.max_hp >= 12;
            let outnumbered = adjacent_count >= 2;

            if hunter.is_adjacent(enemy_pos) && (brawler || !outnumbered) {
                let _ = session.apply(Command::Melee(Target::Actor(enemy_id)));
                return;
            }
            // Shoot the nearest while it closes, keeping a shot in reserve to
            // cast silver around a regenerating beast when the moment comes.
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
            // Out of shots. A brawler closes; a frail hunter breaks contact and
            // presses on with the route rather than standing to be surrounded.
            if brawler {
                self.walk_toward(session, enemy_pos, true);
                return;
            }
            if outnumbered && self.step_away(session, enemy_pos) {
                return;
            }
            // One foe and no shots: fall through to the route, which walks her
            // onward. If it cannot, meet the one enemy rather than dithering.
            if self.step_index >= self.steps.len() {
                self.walk_toward(session, enemy_pos, true);
                return;
            }
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
        match step.action {
            // Everything is ready; the caller drives the hunt itself.
            RouteAction::InitiateHunt => self.step_index += 1,
            RouteAction::Resolve(opp_id) => {
                if session.sim.state.resolved.contains(&opp_id) {
                    self.step_index += 1;
                } else {
                    self.resolve_opportunity(session, opp_id);
                }
            }
            RouteAction::Travel(destination) => {
                if destination == session.sim.state.current_map {
                    self.step_index += 1;
                } else {
                    self.travel_toward(session, destination);
                }
            }
            RouteAction::Craft { recipe } => {
                if self.goto_feature(session, |kind| matches!(kind, FeatureKind::Workstation)) {
                    // Either way the step is done with: a failure here means
                    // the plan drifted and the ingredients are short, and
                    // retrying would loop forever.
                    self.try_apply(session, "craft", Command::Craft { recipe });
                    self.step_index += 1;
                }
            }
            RouteAction::Consecrate => {
                if self.goto_feature(session, |kind| matches!(kind, FeatureKind::Altar)) {
                    self.try_apply(session, "consecrate", Command::Consecrate);
                    self.step_index += 1;
                }
            }
        }
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
        if hunter.hp + 4 <= hunter.max_hp
            && hunter.item_count("wound-draught") > 0
            && session.apply(Command::UseDraught).is_ok()
        {
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
            // Open the hunt with an aimed silver shot the moment there is a
            // line to the host. An earlier version backed off to open from
            // range first, and when the host held its ground it oscillated
            // into that step forever - stepping away, being followed, stepping
            // away - and burned the whole action budget at day zero. She aims
            // in place and fires from wherever she stands now; the shot reveals
            // the host and lands the silver on the beast either way.
            if has_silver && distance <= 6 {
                if !session.sim.state.hunter.sure_shot
                    && session.sim.state.hunter.stamina >= manoeuvre_cost(session, "aim")
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
        let has_charm = state.hunter.item_count("binding-charm") > 0;
        let has_silver = state.hunter.item_count("silver-bullet") > 0;
        let sure_shot = state.hunter.sure_shot;
        let power_primed = state.hunter.melee_multiplier.is_some();
        let regen_stopped = state.sim_actor_regen_stopped(actor_id).unwrap_or(false);

        // Silver first against a regenerating villain: aim, then the sure shot.
        if has_silver && def.regeneration.is_some() && !regen_stopped {
            if !sure_shot
                && stamina >= manoeuvre_cost(session, "aim")
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

        // Call in the second the moment the fight is joined, so the villager
        // stands for as much of it as she can give. It is the Confessor's whole
        // survival case, and the estimate certified her on the assumption she
        // makes it — so a driver that never did was under-driving her.
        if session.sim.state.hunter.second_turns == 0
            && has_signature(session, "stand-with-me")
            && hunter.distance(villain_pos) <= 4
            && self.try_apply(
                session,
                "signature:stand-with-me",
                Command::Signature {
                    id: "stand-with-me".to_owned(),
                    dir: None,
                    target: None,
                },
            )
        {
            return;
        }

        // Bind a shrouded revenant the moment we stand beside it.
        if adjacent
            && has_charm
            && def.cadence.is_some()
            && !vulnerable
            && !bound
            && !dormant
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
            && self.step_away(session, villain_pos)
        {
            return;
        }

        // A frail hunter losing the exchange breaks contact and shoots rather
        // than standing in a trade she is behind on: her health buys fewer
        // turns than the thing in front of her has.
        // Not frail while someone stands with her: a second both adds blows and
        // takes them, which is exactly the trade she would be running from.
        let frail = session.sim.catalogue.hunter.physical_cap < 2
            && session.sim.state.hunter.max_hp < 12
            && session.sim.state.hunter.second_turns == 0;
        if adjacent
            && frail
            && !dormant
            && session.sim.state.hunter.item_count("flintlock-shot") > 0
            && u32::from(session.sim.state.hunter.hp) * 2
                <= u32::from(session.sim.state.hunter.max_hp)
            && self.step_away(session, villain_pos)
        {
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
            if dormant
                && !power_primed
                && stamina >= manoeuvre_cost(session, "power-attack")
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
            if has_signature(session, "killing-blow")
                && (dormant || trapped || wounded)
                && self.try_apply(
                    session,
                    "signature:killing-blow",
                    Command::Signature {
                        id: "killing-blow".to_owned(),
                        dir: None,
                        target: Some(Target::Actor(actor_id)),
                    },
                )
            {
                return;
            }
            let _ = session.apply(Command::Melee(Target::Actor(actor_id)));
            return;
        }

        // Not adjacent: mark the ground and let it come to us. The Occultist
        // has no finisher and little health, so the ground has to do the work
        // the Huntress does with a snare and a killing blow. Hunters without
        // the signature fall through to the snare below.
        if has_signature(session, "ward-the-ground")
            && villain_pos.distance(hunter) <= 4
            && !dormant
        {
            let already = session
                .sim
                .state
                .wards
                .iter()
                .any(|ward| ward.covers(session.sim.state.current_map, hunter));
            if !already
                && self.try_apply(
                    session,
                    "signature:ward-the-ground",
                    Command::Signature {
                        id: "ward-the-ground".to_owned(),
                        dir: None,
                        target: None,
                    },
                )
            {
                return;
            }
            // Standing on our own marks is the whole point: walking off them to
            // meet the villain hands back the advantage we just paid for. But
            // waiting empty-handed while it crosses wastes the very turns the
            // ward bought — put shot into it as it comes.
            if already {
                if !self.shoot_if_able(session, actor_id, villain_pos, &def, vulnerable) {
                    let _ = session.apply(Command::Wait);
                }
                return;
            }
        }

        // Lay a snare on the approach, then close in.
        if has_signature(session, "set-snare") && villain_pos.distance(hunter) <= 4 && !dormant {
            if let Some(dir) = Direction::toward(hunter, villain_pos) {
                let snare_at = hunter.step(dir);
                let already = session
                    .sim
                    .state
                    .snares
                    .iter()
                    .any(|snare| snare.map == session.sim.state.current_map);
                if !already
                    && snare_at != villain_pos
                    && self.try_apply(
                        session,
                        "signature:set-snare",
                        Command::Signature {
                            id: "set-snare".to_owned(),
                            dir: Some(dir),
                            target: None,
                        },
                    )
                {
                    return;
                }
            }
        }
        // Still at range with nothing better to do: shoot it while it closes,
        // rather than spending the approach taking blows without landing any.
        if self.shoot_if_able(session, actor_id, villain_pos, &def, vulnerable) {
            return;
        }
        self.walk_toward(session, villain_pos, true);
    }

    /// Put ordinary shot into the villain from range, when a shot can land:
    /// within the flintlock's reach, not already toe-to-toe, and either it has
    /// no shroud or the shroud is currently thin. Returns whether it fired.
    fn shoot_if_able(
        &mut self,
        session: &mut RunSession,
        actor_id: rh_core::state::ActorId,
        villain_pos: Point,
        def: &rh_content::VillainDef,
        vulnerable: bool,
    ) -> bool {
        let hunter = session.sim.state.hunter.pos;
        let distance = hunter.distance(villain_pos);
        if !(2..=6).contains(&distance)
            || (def.cadence.is_some() && !vulnerable)
            || session.sim.state.hunter.item_count("flintlock-shot") == 0
        {
            return false;
        }
        session
            .apply(Command::Ranged {
                target: Target::Actor(actor_id),
                silver: false,
            })
            .is_ok()
    }

    /// Walk to the exit leading to `destination` and travel.
    fn travel_toward(&mut self, session: &mut RunSession, destination: MapId) {
        let current = session.sim.state.current_map;
        if current == destination {
            return;
        }
        // Head for the next map on the way, which is the destination itself
        // when there is a direct road and an intermediate hop when there is
        // not. The world is a small triangle today, but a map whose two exits
        // both lead to the same neighbour leaves the third reachable only
        // through it, and the bot used to wait out the clock at a door that
        // does not go where it wanted rather than take the road that does.
        let Some(next_hop) = self.next_travel_hop(session, current, destination) else {
            let _ = session.apply(Command::Wait);
            return;
        };
        let exit_at = session
            .sim
            .world
            .map(current)
            .exits
            .iter()
            .find(|exit| exit.to_map == next_hop)
            .map(|exit| exit.at);
        let Some(exit_at) = exit_at else {
            let _ = session.apply(Command::Wait);
            return;
        };
        if session.sim.state.hunter.pos == exit_at {
            if session.apply(Command::Travel).is_ok() {
                // Route travel steps complete when the map changes. The step's
                // action says which map, so ask it — description is display.
                if self.step_index < self.steps.len()
                    && self.steps[self.step_index].action == RouteAction::Travel(destination)
                {
                    self.step_index += 1;
                }
            }
        } else {
            self.walk_toward(session, exit_at, false);
        }
    }

    /// The first map to travel to on a shortest path from `current` to
    /// `destination`, by breadth-first search over paired exits. `None` when
    /// the destination is unreachable, which a well-formed world never is.
    fn next_travel_hop(
        &self,
        session: &RunSession,
        current: MapId,
        destination: MapId,
    ) -> Option<MapId> {
        // Predecessor per map by index; a handful of maps, so a flat Vec keyed
        // on MapId.0 is the whole graph. `None` means unvisited.
        let map_count = session.sim.world.maps.len();
        let mut came_from: Vec<Option<u8>> = vec![None; map_count];
        came_from[current.0 as usize] = Some(current.0);
        let mut queue = VecDeque::from([current]);
        while let Some(map) = queue.pop_front() {
            if map == destination {
                // Walk the predecessors back to the hop that leaves `current`.
                let mut step = destination.0;
                while came_from[step as usize] != Some(current.0) {
                    step = came_from[step as usize]?;
                }
                return Some(MapId(step));
            }
            for exit in &session.sim.world.map(map).exits {
                let to = exit.to_map.0 as usize;
                if came_from[to].is_none() {
                    came_from[to] = Some(map.0);
                    queue.push_back(exit.to_map);
                }
            }
        }
        None
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

/// Stamina cost of a manoeuvre by id, from authored content (so the bot
/// tracks tuning changes instead of hardcoding costs).
/// Whether this hunter owns the signature and can pay for it now. The combat
/// ladder used to gate on `physical >= 1` alone - a check on the pool, not the
/// kit - so it asked every hunter for the same three abilities regardless of
/// which she has. Harmless in play (the sim refuses and she falls through) but
/// it meant the Huntress reached for a ward she has never owned dozens of times
/// a run. The ladder asks this instead, so a hunter only tries what is hers.
fn has_signature(session: &RunSession, id: &str) -> bool {
    session
        .sim
        .catalogue
        .hunter
        .signatures
        .iter()
        .find(|sig| sig.id == id)
        .is_some_and(|sig| session.sim.state.hunter.physical >= sig.physical_cost)
}

fn manoeuvre_cost(session: &RunSession, id: &str) -> u8 {
    session
        .sim
        .catalogue
        .hunter
        .manoeuvres
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.stamina_cost)
        .unwrap_or(u8::MAX)
}

trait RegenLookup {
    fn sim_actor_regen_stopped(&self, id: rh_core::state::ActorId) -> Option<bool>;
}

impl RegenLookup for rh_core::state::RunState {
    fn sim_actor_regen_stopped(&self, id: rh_core::state::ActorId) -> Option<bool> {
        self.actor(id).map(|actor| actor.regen_stopped)
    }
}
