//! The solvability planner.
//!
//! Certifies generated worlds by exhaustively searching the clue graph under
//! the authored budgets: an early hunt-ready route by turn 3 (obscure actions
//! allowed) and an independent, more obvious fallback by turn 5 that reuses
//! none of the early route's opportunities or NPCs. Both must clear the
//! combat-viability threshold, stay within travel and weighted-effort bounds,
//! and never rely on lucky combat drops (the planner simply has no such
//! action). At most one certified route may lean on the mystical favour.

use std::collections::HashSet;

use rh_content::{Catalogue, PoolKind};
use rh_core::viability::{hunt_viability, HuntLoadout};
use rh_core::world::{
    CertifiedRoute, MapId, NpcId, OpportunityAnchor, OpportunityGrant, OpportunityId, RouteStep,
    World,
};

/// Items the planner tracks through gathers and crafting.
const TRACKED: [&str; 8] = [
    "silver",
    "flintlock-shot",
    "moon-herb",
    "bitter-root",
    "grave-dust",
    "wound-draught",
    "silver-bullet",
    "binding-charm",
];

/// Per-item planning caps: beyond these counts the viability model gains
/// nothing, so capping collapses equivalent states and keeps the search fast.
const ITEM_CAP: [u8; 8] = [1, 2, 2, 2, 2, 2, 1, 2];

const SETTLEMENT: u8 = 0;
/// Safety valve for pathological graphs; recorded as a rejection reason.
const NODE_BUDGET: u32 = 200_000;

/// Planner view of one opportunity node in the clue graph.
#[derive(Debug, Clone)]
pub struct PlanOp {
    pub id: OpportunityId,
    pub name: String,
    pub map: u8,
    pub pool: Option<PoolKind>,
    pub cost: u8,
    pub obscurity: u8,
    pub grants: OpGrant,
    pub revealed_by: Option<usize>,
    pub requires: Option<usize>,
    pub npc: Option<NpcId>,
    /// Structural access (forced doors, shifted rubble): stays cleared in
    /// play, so both certified routes may schedule it.
    pub structural: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpGrant {
    Identity,
    Location,
    Favour,
    Lead,
    Items(Vec<usize>),
}

pub struct PlannerConfig {
    pub deadline: u8,
    pub forbidden: u64,
    pub obscurity_budget: Option<u16>,
    pub allow_favour: bool,
    pub label: String,
}

#[derive(Debug, Clone)]
struct PState {
    turn: u8,
    map: u8,
    /// Within-turn symmetry breaking: ops resolve in ascending index order,
    /// resetting when the clock advances. Safe because gates always carry
    /// lower indices than their dependents.
    min_op: u8,
    lore: u8,
    social: u8,
    mystic_bonus: u8,
    physical: u8,
    resolved: u64,
    items: [u8; TRACKED.len()],
    consecrated: bool,
    effort: u16,
    obscurity: u16,
    legs: u8,
}

/// Memo key: effort and obscurity are fully determined by (resolved, legs),
/// so excluding them keeps deduplication exact while collapsing the space.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MemoKey {
    turn: u8,
    map: u8,
    min_op: u8,
    lore: u8,
    social: u8,
    mystic_bonus: u8,
    physical: u8,
    resolved: u64,
    items: [u8; TRACKED.len()],
    consecrated: bool,
    legs: u8,
}

impl PState {
    fn key(&self) -> MemoKey {
        MemoKey {
            turn: self.turn,
            map: self.map,
            min_op: self.min_op,
            lore: self.lore,
            social: self.social,
            mystic_bonus: self.mystic_bonus,
            physical: self.physical,
            resolved: self.resolved,
            items: self.items,
            consecrated: self.consecrated,
            legs: self.legs,
        }
    }
}

#[derive(Debug, Clone)]
enum Action {
    Resolve(usize),
    Craft(String),
    Consecrate,
    Travel(u8),
}

struct Ctx<'a> {
    catalogue: &'a Catalogue,
    world: &'a World,
    ops: Vec<PlanOp>,
    /// Exploration order: identity clues first, then items, leads, location.
    op_order: Vec<usize>,
    villain_map: u8,
    villain_gate: Option<usize>,
    dormant_opening: bool,
}

/// Certify both routes for a candidate world, or explain why not.
///
/// The two routes must be fully independent: no shared opportunity nodes and
/// no shared NPCs, so losing any single informant to fallout leaves one
/// certified route intact. The pairing is searched in both orders (penalise
/// early then find the obvious fallback, and the reverse assignment) so a
/// greedy first route cannot starve the second of its only viable nodes.
pub fn certify(catalogue: &Catalogue, world: &World) -> Result<Vec<CertifiedRoute>, String> {
    let ctx = build_ctx(catalogue, world)?;
    let generator = &catalogue.balance.generator;

    let early_cfg = |forbidden: u64, allow_favour: bool| PlannerConfig {
        deadline: generator.early_route_deadline,
        forbidden,
        obscurity_budget: None,
        allow_favour,
        label: "early hunt".to_owned(),
    };
    let fallback_cfg = |forbidden: u64, allow_favour: bool| PlannerConfig {
        deadline: generator.fallback_route_deadline,
        forbidden,
        obscurity_budget: Some(generator.fallback_obscurity_budget),
        allow_favour,
        label: "obvious fallback".to_owned(),
    };

    // Preferred assignment: certify the obvious fallback first, then push
    // the early route onto the remaining (possibly niche) nodes. Routes are
    // minimised so neither consumes nodes it does not need. If the early
    // route starves, retry against alternative fallbacks before giving up.
    let fallback_first = (|| -> Result<(CertifiedRoute, CertifiedRoute), String> {
        let mut alternatives_forbidden = 0u64;
        let mut last_reason = String::new();
        for _ in 0..4 {
            let cfg = fallback_cfg(alternatives_forbidden, true);
            let fallback = match search(&ctx, &cfg) {
                Ok(route) => minimise(&ctx, &cfg, route),
                Err(reason) => {
                    last_reason = format!("fallback: {reason}");
                    break;
                }
            };
            let forbidden = route_penalty(&ctx, &fallback);
            let cfg = early_cfg(forbidden, !fallback.uses_mystic_favour);
            match search(&ctx, &cfg) {
                Ok(route) => {
                    let early = minimise(&ctx, &cfg, route);
                    return Ok((early, fallback));
                }
                Err(reason) => {
                    last_reason = format!("early after fallback: {reason}");
                    // Force a structurally different fallback next round.
                    alternatives_forbidden |= route_penalty(&ctx, &fallback);
                }
            }
        }
        Err(last_reason)
    })();
    let (early, fallback) = match fallback_first {
        Ok(pair) => pair,
        Err(first_reason) => {
            // Reverse assignment: early route first, penalise it, then the
            // obvious fallback from what remains.
            let cfg = early_cfg(0, true);
            let early_raw = search(&ctx, &cfg).map_err(|reason| {
                format!(
                    "no early hunt-ready route by turn {}: {reason} \
                     (fallback-first also failed: {first_reason})",
                    generator.early_route_deadline
                )
            })?;
            let early = minimise(&ctx, &cfg, early_raw);
            let forbidden = route_penalty(&ctx, &early);
            let cfg = fallback_cfg(forbidden, !early.uses_mystic_favour);
            let fallback_raw = search(&ctx, &cfg).map_err(|reason| {
                format!(
                    "no independent obvious fallback by turn {}: {reason}",
                    generator.fallback_route_deadline
                )
            })?;
            let fallback = minimise(&ctx, &cfg, fallback_raw);
            (early, fallback)
        }
    };
    Ok(vec![early, fallback])
}

/// Shrink a found route to a minimal node set: repeatedly re-search with one
/// used node forbidden; keep any smaller qualifying route that results.
fn minimise(ctx: &Ctx, base_cfg: &PlannerConfig, mut route: CertifiedRoute) -> CertifiedRoute {
    loop {
        let used: Vec<usize> = route
            .steps
            .iter()
            .filter_map(|step| step.opportunity)
            .filter_map(|id| ctx.ops.iter().position(|op| op.id == id))
            .collect();
        let allowed: u64 = used.iter().fold(0u64, |mask, index| mask | (1 << index));
        let everything: u64 = if ctx.ops.len() >= 64 {
            u64::MAX
        } else {
            (1u64 << ctx.ops.len()) - 1
        };
        let mut improved = None;
        for drop in used.iter().rev() {
            let trial_allowed = allowed & !(1 << drop);
            let cfg = PlannerConfig {
                deadline: base_cfg.deadline,
                forbidden: base_cfg.forbidden | (everything & !trial_allowed),
                obscurity_budget: base_cfg.obscurity_budget,
                allow_favour: base_cfg.allow_favour,
                label: base_cfg.label.clone(),
            };
            if let Ok(smaller) = search(ctx, &cfg) {
                improved = Some(smaller);
                break;
            }
        }
        match improved {
            Some(smaller) => route = smaller,
            None => return route,
        }
    }
}

/// Forbidden-node mask for the second search: the first route's used nodes,
/// per the generator contract ("penalise the early route's used/niche
/// nodes"). Routes may consult the same NPC about different things, and
/// structural access ops (forced doors, rubble) stay open to both routes.
fn route_penalty(ctx: &Ctx, route: &CertifiedRoute) -> u64 {
    let mut forbidden = 0u64;
    for step in &route.steps {
        if let Some(op_id) = step.opportunity {
            if let Some(index) = ctx.ops.iter().position(|op| op.id == op_id) {
                if !ctx.ops[index].structural {
                    forbidden |= 1 << index;
                }
            }
        }
    }
    forbidden
}

/// Diagnostic dump used while tuning the generator: raw search outcomes for
/// each config plus the full ops table.
#[allow(dead_code)]
pub(crate) fn debug_certify(catalogue: &Catalogue, world: &World) -> String {
    let mut out = String::new();
    let ctx = match build_ctx(catalogue, world) {
        Ok(ctx) => ctx,
        Err(reason) => return format!("build_ctx failed: {reason}"),
    };
    out.push_str(&format!(
        "villain={} map={} gate={:?} dormant={}\n",
        world.villain.archetype, ctx.villain_map, ctx.villain_gate, ctx.dormant_opening
    ));
    for (index, op) in ctx.ops.iter().enumerate() {
        out.push_str(&format!(
            "  [{index:2}] {} map={} pool={:?} cost={} obs={} grants={:?} rev={:?} req={:?} npc={:?}\n",
            op.name, op.map, op.pool, op.cost, op.obscurity, op.grants, op.revealed_by, op.requires, op.npc
        ));
    }
    let generator = &catalogue.balance.generator;
    let dump = |out: &mut String, label: &str, result: &Result<CertifiedRoute, String>| match result
    {
        Ok(route) => {
            out.push_str(&format!(
                "{label}: OK turn={} viability={} obscurity={} steps:\n",
                route.ready_by_turn, route.viability_permille, route.total_obscurity
            ));
            for step in &route.steps {
                out.push_str(&format!("    t{} {}\n", step.turn, step.description));
            }
        }
        Err(reason) => out.push_str(&format!("{label}: FAIL {reason}\n")),
    };

    // Stage-by-stage pairing, mirroring certify().
    let fallback_cfg = PlannerConfig {
        deadline: generator.fallback_route_deadline,
        forbidden: 0,
        obscurity_budget: Some(generator.fallback_obscurity_budget),
        allow_favour: true,
        label: "fallback".to_owned(),
    };
    let fallback = search(&ctx, &fallback_cfg).map(|route| minimise(&ctx, &fallback_cfg, route));
    dump(&mut out, "stage1 fallback(free, minimised)", &fallback);
    if let Ok(fallback) = &fallback {
        for deadline in [
            generator.early_route_deadline,
            generator.early_route_deadline + 2,
        ] {
            let early_cfg = PlannerConfig {
                deadline,
                forbidden: route_penalty(&ctx, fallback),
                obscurity_budget: None,
                allow_favour: !fallback.uses_mystic_favour,
                label: "early".to_owned(),
            };
            let early = search(&ctx, &early_cfg).map(|route| minimise(&ctx, &early_cfg, route));
            dump(
                &mut out,
                &format!("stage2 early(penalised, deadline {deadline})"),
                &early,
            );
        }
    }
    out
}

/// Inspector rows: every clue-graph node with its costs and gates.
pub fn node_report(catalogue: &Catalogue, world: &World) -> Vec<crate::NodeReport> {
    match build_ctx(catalogue, world) {
        Ok(ctx) => ctx
            .ops
            .iter()
            .map(|op| crate::NodeReport {
                id: op.id.0,
                name: op.name.clone(),
                map: world.maps[op.map as usize].template.clone(),
                pool: op.pool.map(|pool| format!("{pool:?}")),
                cost: op.cost,
                obscurity: op.obscurity,
                grants: match &op.grants {
                    OpGrant::Identity => "identity clue".to_owned(),
                    OpGrant::Location => "location clue".to_owned(),
                    OpGrant::Favour => "mystic favour".to_owned(),
                    OpGrant::Lead => "lead".to_owned(),
                    OpGrant::Items(items) => format!(
                        "items: {}",
                        items
                            .iter()
                            .map(|index| TRACKED[*index])
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                },
                revealed_by: op.revealed_by.map(|index| ctx.ops[index].id.0),
                requires: op.requires.map(|index| ctx.ops[index].id.0),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn build_ctx<'a>(catalogue: &'a Catalogue, world: &'a World) -> Result<Ctx<'a>, String> {
    let mut ops = Vec::new();
    for spec in &world.opportunities {
        let grants = match &spec.grants {
            OpportunityGrant::IdentityClue => OpGrant::Identity,
            OpportunityGrant::LocationClue => OpGrant::Location,
            OpportunityGrant::MysticFavour => OpGrant::Favour,
            OpportunityGrant::Lead => OpGrant::Lead,
            OpportunityGrant::Items { items } => {
                let tracked: Vec<usize> = items
                    .iter()
                    .filter_map(|item| TRACKED.iter().position(|tracked| tracked == item))
                    .collect();
                if tracked.is_empty() {
                    // Grants nothing the planner values (coin caches).
                    continue;
                }
                OpGrant::Items(tracked)
            }
            // Social texture (secrets, ties, leverage) is not route-relevant.
            _ => continue,
        };
        let npc = match spec.anchor {
            OpportunityAnchor::Npc(npc) => Some(npc),
            OpportunityAnchor::Tile(_) => None,
        };
        ops.push(PlanOp {
            id: spec.id,
            name: spec.name.clone(),
            map: spec.map.0,
            pool: spec.pool,
            cost: spec.cost,
            obscurity: spec.obscurity,
            grants,
            revealed_by: None,
            requires: None,
            npc,
            structural: spec.clears_terrain,
        });
    }
    if ops.len() > 64 {
        return Err(format!(
            "clue graph has {} nodes; planner caps at 64",
            ops.len()
        ));
    }
    // The within-turn canonical ordering requires every gate (revealed_by /
    // requires) to carry a lower index than its dependent; materialisation
    // places force ops and clues before gathers, so this holds by
    // construction. Verify rather than assume.
    let gate_check = |gate: Option<usize>, index: usize, what: &str| -> Result<(), String> {
        match gate {
            Some(gate_index) if gate_index >= index => Err(format!(
                "op #{index} has {what} gate #{gate_index} out of canonical order"
            )),
            _ => Ok(()),
        }
    };
    // Wire knowledge and access gates as op indices.
    let by_id: Vec<OpportunityId> = ops.iter().map(|op| op.id).collect();
    for (index, spec_id) in by_id.iter().enumerate() {
        let spec = world.opportunity(*spec_id);
        let find = |target: OpportunityId| by_id.iter().position(|id| *id == target);
        ops[index].revealed_by = match spec.discovery {
            rh_core::world::DiscoveryRule::RevealedBy(source) => {
                // A gate outside the planner set means the op is unreachable
                // for planning purposes; drop the gate only if it resolves.
                Some(
                    find(source)
                        .ok_or_else(|| format!("op '{}' gated by a non-planner op", spec.name))?,
                )
            }
            _ => None,
        };
        ops[index].requires = spec.requires.and_then(find);
        gate_check(ops[index].revealed_by, index, "revealed-by")?;
        gate_check(ops[index].requires, index, "requires")?;
    }

    // Where the hunt is initiated: the host walks the settlement; a dormant
    // villain is confronted at its grave.
    let villain_map = match world.villain.host {
        Some(host) => world.npc(host).map.0,
        None => world
            .villain
            .grave
            .map(|(map, _)| map.0)
            .unwrap_or(world.villain.lair.0 .0),
    };
    // If the villain's grave is physically gated, the hunt needs that gate.
    let villain_gate = match world.villain.grave {
        Some((map, feature)) => {
            let grave_at = world
                .map(map)
                .feature(feature)
                .map(|f| f.at)
                .ok_or_else(|| "villain grave feature missing".to_owned())?;
            world
                .opportunities
                .iter()
                .find(|opp| {
                    opp.clears_terrain
                        && opp.map == map
                        && matches!(opp.anchor, OpportunityAnchor::Tile(gate)
                            if gate_unlocks(world, map, gate, grave_at))
                })
                .and_then(|opp| ops.iter().position(|op| op.id == opp.id))
        }
        None => None,
    };

    let mut op_order: Vec<usize> = (0..ops.len()).collect();
    op_order.sort_by_key(|index| {
        let bucket = match ops[*index].grants {
            OpGrant::Identity => 0,
            OpGrant::Items(_) => 1,
            OpGrant::Favour | OpGrant::Lead => 2,
            OpGrant::Location => 3,
        };
        (bucket, *index)
    });

    Ok(Ctx {
        catalogue,
        world,
        ops,
        op_order,
        villain_map,
        villain_gate,
        dormant_opening: world.villain.grave.is_some(),
    })
}

/// Does clearing `gate` make `target` reachable from the map entry?
fn gate_unlocks(world: &World, map: MapId, gate: rh_core::Point, target: rh_core::Point) -> bool {
    let world_map = world.map(map);
    let entry = world_map.entry;
    let tiles = &world_map.tiles;
    let walk = |point: rh_core::Point, unlocked: bool| -> bool {
        let terrain = world_map.terrain(point);
        rh_core::fov::is_walkable(terrain) || (unlocked && point == gate)
    };
    // Without the gate the target must be unreachable, with it reachable.
    !flood_reaches(tiles, entry, target, &|p| walk(p, false))
        && flood_reaches(tiles, entry, target, &|p| walk(p, true))
}

fn flood_reaches(
    _tiles: &[rh_content::Terrain],
    from: rh_core::Point,
    to: rh_core::Point,
    passable: &dyn Fn(rh_core::Point) -> bool,
) -> bool {
    let mut seen = HashSet::new();
    let mut queue = vec![from];
    seen.insert((from.x, from.y));
    while let Some(point) = queue.pop() {
        if point == to || point.is_adjacent(to) {
            return true;
        }
        for next in point.neighbours() {
            if next.in_bounds() && passable(next) && seen.insert((next.x, next.y)) {
                queue.push(next);
            }
        }
    }
    false
}

struct RouteFound {
    steps: Vec<(u8, String, Option<OpportunityId>)>,
    state: PState,
    viability: u16,
}

fn search(ctx: &Ctx, cfg: &PlannerConfig) -> Result<CertifiedRoute, String> {
    let hunter = &ctx.catalogue.hunter;
    let mut items = [0u8; TRACKED.len()];
    for item in &hunter.starting_items {
        if let Some(index) = TRACKED.iter().position(|tracked| tracked == item) {
            items[index] += 1;
        }
    }
    // Fail fast when the forbidden mask leaves fewer than two identity clues.
    let identity_available = ctx
        .ops
        .iter()
        .enumerate()
        .filter(|(index, op)| op.grants == OpGrant::Identity && cfg.forbidden & (1 << index) == 0)
        .count();
    if identity_available < 2 {
        return Err(format!(
            "only {identity_available} identity clues remain outside the penalised set"
        ));
    }

    for (index, count) in items.iter_mut().enumerate() {
        *count = (*count).min(ITEM_CAP[index]);
    }
    let start = PState {
        turn: 0,
        map: SETTLEMENT,
        min_op: 0,
        lore: hunter.lore_cap,
        social: hunter.social_cap,
        mystic_bonus: 0,
        physical: hunter.physical_cap,
        resolved: 0,
        items,
        consecrated: false,
        effort: 0,
        obscurity: 0,
        legs: 0,
    };
    // Iterative deepening on the deadline: minimal-turn routes live in far
    // smaller search spaces, so find them before opening the full tree.
    // Each iteration gets its own node budget so the final, full-deadline
    // search is never starved by earlier exhaustive failures.
    let mut nodes = 0u32;
    let mut found_route = None;
    for deadline in 0..=cfg.deadline {
        let sub_cfg = PlannerConfig {
            deadline,
            forbidden: cfg.forbidden,
            obscurity_budget: cfg.obscurity_budget,
            allow_favour: cfg.allow_favour,
            label: cfg.label.clone(),
        };
        let mut memo: HashSet<MemoKey> = HashSet::new();
        let mut path: Vec<(u8, String, Option<OpportunityId>)> = Vec::new();
        nodes = 0;
        if let Some(found) = dfs(
            ctx,
            &sub_cfg,
            start.clone(),
            &mut memo,
            &mut nodes,
            &mut path,
        ) {
            found_route = Some(found);
            break;
        }
    }
    match found_route {
        Some(found) => {
            let favour_index = ctx.ops.iter().position(|op| op.grants == OpGrant::Favour);
            let uses_favour = favour_index
                .map(|index| found.state.resolved & (1 << index) != 0)
                .unwrap_or(false);
            let mut steps: Vec<RouteStep> = found
                .steps
                .iter()
                .map(|(turn, description, opportunity)| RouteStep {
                    turn: *turn,
                    description: description.clone(),
                    opportunity: *opportunity,
                })
                .collect();
            steps.push(RouteStep {
                turn: found.state.turn,
                description: format!(
                    "Initiate the hunt ({}\u{2030} viability at threat tier {})",
                    found.viability,
                    tier_at(ctx, found.state.turn)
                ),
                opportunity: None,
            });
            Ok(CertifiedRoute {
                label: cfg.label.clone(),
                ready_by_turn: found.state.turn,
                viability_permille: found.viability,
                total_effort: found.state.effort,
                total_obscurity: found.state.obscurity,
                travel_legs: found.state.legs,
                uses_mystic_favour: uses_favour,
                steps,
            })
        }
        None => {
            if nodes >= NODE_BUDGET {
                Err(format!("search budget of {NODE_BUDGET} states exhausted"))
            } else {
                Err("search space exhausted without a qualifying route".to_owned())
            }
        }
    }
}

fn tier_at(ctx: &Ctx, turn: u8) -> u8 {
    let clock = &ctx.catalogue.balance.clock;
    u8::from(turn >= clock.minor_event_turn) + u8::from(turn >= clock.major_event_turn)
}

fn goal(ctx: &Ctx, state: &PState) -> Option<u16> {
    let identity = ctx
        .ops
        .iter()
        .enumerate()
        .filter(|(index, op)| op.grants == OpGrant::Identity && state.resolved & (1 << index) != 0)
        .count();
    if identity < 2 {
        return None;
    }
    if state.map != ctx.villain_map {
        return None;
    }
    if let Some(gate) = ctx.villain_gate {
        if state.resolved & (1 << gate) == 0 {
            return None;
        }
    }
    let item = |name: &str| -> u16 {
        TRACKED
            .iter()
            .position(|tracked| *tracked == name)
            .map(|index| u16::from(state.items[index]))
            .unwrap_or(0)
    };
    // Opening the villain's grave itself costs a Physical point; without it
    // the fight starts with no snare or Killing Blow in reserve.
    let physical_at_fight = if ctx.dormant_opening {
        state.physical.saturating_sub(1)
    } else {
        state.physical
    };
    let loadout = HuntLoadout {
        hunter_hp: ctx.catalogue.hunter.health,
        draughts: item("wound-draught"),
        silver_bullets: item("silver-bullet"),
        binding_charms: item("binding-charm"),
        physical: physical_at_fight,
        on_consecrated_ground: state.consecrated && ctx.villain_map == SETTLEMENT,
        dormant_opening: ctx.dormant_opening,
    };
    let viability = hunt_viability(
        ctx.catalogue,
        &ctx.world.villain.archetype,
        tier_at(ctx, state.turn),
        &loadout,
    );
    let threshold = ctx.catalogue.balance.generator.viability_threshold_permille;
    (viability >= threshold).then_some(viability)
}

fn dfs(
    ctx: &Ctx,
    cfg: &PlannerConfig,
    state: PState,
    memo: &mut HashSet<MemoKey>,
    nodes: &mut u32,
    path: &mut Vec<(u8, String, Option<OpportunityId>)>,
) -> Option<RouteFound> {
    if *nodes >= NODE_BUDGET {
        return None;
    }
    *nodes += 1;
    if let Some(viability) = goal(ctx, &state) {
        return Some(RouteFound {
            steps: path.clone(),
            state,
            viability,
        });
    }
    if !memo.insert(state.key()) {
        return None;
    }
    // Dead-end prune: stranded off the villain's map with no turns left.
    if state.map != ctx.villain_map && state.turn >= cfg.deadline {
        return None;
    }

    for action in candidate_actions(ctx, cfg, &state) {
        let (next, description, opportunity) = apply_action(ctx, &state, &action);
        path.push((state.turn, description, opportunity));
        let result = dfs(ctx, cfg, next, memo, nodes, path);
        path.pop();
        if result.is_some() {
            return result;
        }
    }
    None
}

fn candidate_actions(ctx: &Ctx, cfg: &PlannerConfig, state: &PState) -> Vec<Action> {
    let mut actions = Vec::new();
    let generator = &ctx.catalogue.balance.generator;

    // Resolve opportunities on the current map, identity clues first so the
    // depth-first search reaches hunt-readiness with minimal backtracking.
    for index in ctx.op_order.iter().copied() {
        let op = &ctx.ops[index];
        let bit = 1u64 << index;
        // Within-turn canonical ordering: indices ascend until the clock moves.
        if (index as u8) < state.min_op {
            continue;
        }
        if state.resolved & bit != 0 || cfg.forbidden & bit != 0 || op.map != state.map {
            continue;
        }
        if let Some(gate) = op.revealed_by {
            if state.resolved & (1 << gate) == 0 {
                continue;
            }
        }
        if let Some(gate) = op.requires {
            if state.resolved & (1 << gate) == 0 {
                continue;
            }
        }
        if op.grants == OpGrant::Favour && !cfg.allow_favour {
            continue;
        }
        if let Some(budget) = cfg.obscurity_budget {
            if state.obscurity + u16::from(op.obscurity) > budget {
                continue;
            }
        }
        if state.effort + u16::from(op.cost) > generator.route_effort_budget {
            continue;
        }
        // Skip pure item grants that cannot raise any tracked count.
        if let OpGrant::Items(items) = &op.grants {
            if items
                .iter()
                .all(|item| state.items[*item] >= ITEM_CAP[*item])
            {
                continue;
            }
        }
        let affordable = match op.pool {
            None => true,
            Some(PoolKind::Lore) => state.lore >= op.cost,
            Some(PoolKind::Social) => state.social >= op.cost,
            Some(PoolKind::Mystic) => state.mystic_bonus >= op.cost,
            Some(PoolKind::Physical) => state.physical >= op.cost,
        };
        if affordable {
            actions.push(Action::Resolve(index));
        }
    }

    // Craft at the settlement forge (free).
    if state.map == SETTLEMENT {
        for (recipe_id, recipe) in &ctx.catalogue.recipes {
            let mut needed = [0u8; TRACKED.len()];
            let mut craftable = true;
            for input in &recipe.inputs {
                match TRACKED.iter().position(|tracked| tracked == input) {
                    Some(index) => needed[index] += 1,
                    None => {
                        craftable = false;
                        break;
                    }
                }
            }
            // Only track outputs the planner cares about.
            let output_tracked = TRACKED.iter().any(|tracked| *tracked == recipe.output);
            if !craftable || !output_tracked {
                continue;
            }
            if needed
                .iter()
                .zip(state.items.iter())
                .all(|(need, have)| have >= need)
            {
                actions.push(Action::Craft(recipe_id.clone()));
            }
        }
        // The consecration rite costs a global turn; it only ever pays off
        // against a consecration-vulnerable villain hunted on settlement
        // ground. (Against the beast, the rite is candle-smoke.)
        let rite_matters = ctx
            .catalogue
            .villains
            .get(&ctx.world.villain.archetype)
            .map(|def| def.affected_by_consecration)
            .unwrap_or(false);
        if !state.consecrated
            && state.turn < cfg.deadline
            && ctx.villain_map == SETTLEMENT
            && rite_matters
        {
            actions.push(Action::Consecrate);
        }
    }

    // Travel to the other maps.
    if state.turn < cfg.deadline
        && state.legs < generator.route_travel_budget
        && state.effort + 2 <= generator.route_effort_budget
    {
        for destination in 0..3u8 {
            if destination != state.map {
                actions.push(Action::Travel(destination));
            }
        }
    }
    actions
}

fn apply_action(
    ctx: &Ctx,
    state: &PState,
    action: &Action,
) -> (PState, String, Option<OpportunityId>) {
    let mut next = state.clone();
    match action {
        Action::Resolve(index) => {
            let op = &ctx.ops[*index];
            next.resolved |= 1 << index;
            next.min_op = (*index as u8) + 1;
            next.effort += u16::from(op.cost);
            next.obscurity += u16::from(op.obscurity);
            match op.pool {
                None => {}
                Some(PoolKind::Lore) => next.lore -= op.cost,
                Some(PoolKind::Social) => next.social -= op.cost,
                Some(PoolKind::Mystic) => next.mystic_bonus -= op.cost,
                Some(PoolKind::Physical) => next.physical -= op.cost,
            }
            match &op.grants {
                OpGrant::Items(items) => {
                    for item in items {
                        next.items[*item] = (next.items[*item] + 1).min(ITEM_CAP[*item]);
                    }
                }
                OpGrant::Favour => {
                    next.mystic_bonus += 1;
                }
                _ => {}
            }
            (next, format!("Resolve: {}", op.name), Some(op.id))
        }
        Action::Craft(recipe_id) => {
            let recipe = &ctx.catalogue.recipes[recipe_id];
            for input in &recipe.inputs {
                if let Some(index) = TRACKED.iter().position(|tracked| tracked == input) {
                    next.items[index] -= 1;
                }
            }
            if let Some(index) = TRACKED.iter().position(|tracked| *tracked == recipe.output) {
                next.items[index] = (next.items[index] + 1).min(ITEM_CAP[index]);
            }
            (next, format!("Craft: {}", recipe.name), None)
        }
        Action::Consecrate => {
            next.consecrated = true;
            next.turn += 1;
            next.min_op = 0;
            let caps = &ctx.catalogue.hunter;
            next.physical = (next.physical + 1).min(caps.physical_cap);
            (
                next,
                "Perform the consecration rite (one day)".to_owned(),
                None,
            )
        }
        Action::Travel(destination) => {
            next.map = *destination;
            next.turn += 1;
            next.min_op = 0;
            next.legs += 1;
            next.effort += 2;
            let caps = &ctx.catalogue.hunter;
            next.lore = (next.lore + 1).min(caps.lore_cap);
            next.social = (next.social + 1).min(caps.social_cap);
            next.physical = (next.physical + 1).min(caps.physical_cap);
            (
                next,
                format!("Travel to {}", ctx.world.maps[*destination as usize].name),
                None,
            )
        }
    }
}
