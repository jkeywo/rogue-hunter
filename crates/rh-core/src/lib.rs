//! Authoritative deterministic headless simulation for Rogue Hunter.
//!
//! The simulation alone owns generated world state, the global clock, local
//! tactical turns, actors, resources, discoveries, inventory, and the event
//! log. All inputs arrive as validated semantic commands.
