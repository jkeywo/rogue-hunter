//! Structural combat multipliers, written once.
//!
//! These are the shape of the rules rather than tuning: tuning numbers live
//! in authored content, but how much deeper a blow lands in a vulnerability
//! window, what a coup de grace multiplies, and how melee multipliers are
//! expressed are code. They used to be hand-mirrored between the sim that
//! enacts them and the viability estimate that prices them, which is exactly
//! how the estimate once credited a hunter with a finisher she did not have.
//! Both read here now, and the corpus scan in rh-replay holds the estimate
//! to the driven game end to end.

/// Blows land twice as deep in a vulnerability window (natural, bound, or
/// on consecrated ground). The dormant coup multiplies at strike level
/// instead, so the two bonuses never stack.
pub const VULNERABILITY_MULTIPLIER: u16 = 2;

/// A coup de grace on a sleeping thing lands with terrible weight. Open to
/// anyone who finds the villain dormant; no signature required.
pub const COUP_MULTIPLIER: u16 = 3;

/// The Killing Blow signature doubles melee damage against an eligible
/// target.
pub const KILLING_BLOW_MULTIPLIER: u16 = 2;

/// Melee multipliers are expressed in halves: an authored numerator over
/// this denominator, so a numerator of 3 is one-and-a-half swings.
pub const MULTIPLIER_HALVES: u16 = 2;
