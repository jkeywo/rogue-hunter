//! The action economy: what taking an action costs and restores.
//!
//! One owner for the prices the sim charges and the planner budgets, so
//! certification prices exactly what play charges — the settlement-hostility
//! surcharge and the clock's pool restores had each been written in two to
//! four places and had already drifted apart once. The planner keeps its
//! deliberately restricted operator set (planner-ignores-non-goal-evidence);
//! what it reads here is the price of an operator, never the set.

use rh_content::PoolKind;

use crate::sim::ClockReason;

/// The pool cost of resolving an opportunity, after the settlement-hostility
/// surcharge on consequential Social work. Free interactions (no pool) stay
/// free. The `cost > 0` guard is documentation more than defence: content
/// validation forbids a pool-costed opportunity at cost zero, so the
/// surcharge can never turn a free action into a charged one.
pub fn opportunity_cost(
    pool: Option<PoolKind>,
    cost: u8,
    settlement_hostile: bool,
) -> Option<(PoolKind, u8)> {
    let pool = pool?;
    let surcharged = settlement_hostile && pool == PoolKind::Social && cost > 0;
    Some((pool, cost + u8::from(surcharged)))
}

/// What a global-clock advance restores. Each restore is one point, up to
/// the pool's cap — and never eats into anything held above cap, like the
/// favour's over-cap Mystic point.
#[derive(Debug, Clone, Copy)]
pub struct ClockRestore {
    /// Physical restores on every global-clock advance.
    pub physical: bool,
    /// Travel additionally restores every investigation pool.
    pub investigation_pools: bool,
}

pub fn clock_restore(reason: ClockReason) -> ClockRestore {
    ClockRestore {
        physical: true,
        investigation_pools: reason == ClockReason::Travel,
    }
}
