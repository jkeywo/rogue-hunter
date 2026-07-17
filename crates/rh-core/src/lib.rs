//! Authoritative deterministic headless simulation for Rogue Hunter.
//!
//! The simulation alone owns generated world state, the global clock, local
//! tactical turns, actors, resources, discoveries, inventory, and the event
//! log. All inputs arrive as validated semantic commands through
//! [`sim::Sim::apply`]; every rejection carries a player-readable reason.

pub mod ai;
pub mod command;
pub mod events;
pub mod fov;
pub mod geometry;
pub mod hash;
pub mod rng;
pub mod sim;
pub mod state;
pub mod viability;
pub mod world;

pub use command::{Command, Rejection, Target};
pub use events::{EventKind, LogEvent};
pub use geometry::{Direction, Point, MAP_HEIGHT, MAP_WIDTH};
pub use rng::SimRng;
pub use sim::Sim;
pub use state::{Actor, ActorId, ActorKind, HunterState, Outcome, RunState};
pub use world::{
    CertifiedRoute, DiscoveryRule, Disposition, ExitSpec, FeatureId, FeatureKind, FeatureSpec,
    GraveContents, MapId, NpcId, NpcSpec, OpportunityAnchor, OpportunityGrant, OpportunityId,
    OpportunitySpec, RouteStep, VillainSpec, World, WorldMap,
};
