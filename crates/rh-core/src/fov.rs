//! Fog of war and line of sight.
//!
//! Local tactical maps use fog of war: tiles the hunter has seen stay
//! remembered; only currently visible tiles show live actors. Visibility is
//! symmetric Bresenham line of sight, shared with pounce-lane checks so
//! "you can see it" and "it can leap at you" always agree.

use rh_content::Terrain;

use crate::geometry::{line_between, Point, MAP_HEIGHT, MAP_WIDTH};
use crate::state::RunState;
use crate::world::{MapId, World};

/// Whether terrain blocks sight (and pounce lanes).
pub fn is_opaque(terrain: Terrain) -> bool {
    matches!(
        terrain,
        Terrain::Wall | Terrain::Tree | Terrain::Rubble | Terrain::BarredDoor
    )
}

/// Whether actors can stand on this terrain.
pub fn is_walkable(terrain: Terrain) -> bool {
    matches!(
        terrain,
        Terrain::Floor | Terrain::Door | Terrain::Road | Terrain::Grass | Terrain::Grave
    )
}

/// Clear line of sight between two points (endpoints excluded).
pub fn has_line_of_sight(
    state: &RunState,
    world: &World,
    map: MapId,
    from: Point,
    to: Point,
) -> bool {
    line_between(from, to)
        .iter()
        .all(|point| !is_opaque(state.terrain(world, map, *point)))
}

/// A clear pounce lane: line of sight with no intervening actor either.
pub fn has_clear_lane(state: &RunState, world: &World, map: MapId, from: Point, to: Point) -> bool {
    line_between(from, to).iter().all(|point| {
        !is_opaque(state.terrain(world, map, *point))
            && state.actor_at(map, *point).is_none()
            && state.npc_at(world, map, *point).is_none()
    })
}

/// Recompute current visibility from the hunter's position with the authored
/// FOV radius, and mark newly visible tiles as seen.
pub fn refresh_visibility(state: &mut RunState, world: &World, radius: u8) {
    let radius = i16::from(radius);
    let map = state.current_map;
    let origin = state.hunter.pos;
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let point = Point::new(x, y);
            let index = RunState::seen_index(point);
            let visible = origin.distance(point) <= radius
                && has_line_of_sight(state, world, map, origin, point);
            state.visible[index] = visible;
            if visible {
                state.seen[map.0 as usize][index] = true;
            }
        }
    }
}
