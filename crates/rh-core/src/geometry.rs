//! Grid geometry: positions, directions, lines of sight.
//!
//! All maps are 32x20. Movement and adjacency are 8-way. Lines of sight and
//! pounce lanes use a symmetric Bresenham walk so visibility and lane checks
//! agree between hunter and monsters.

use serde::{Deserialize, Serialize};

pub const MAP_WIDTH: i16 = 32;
pub const MAP_HEIGHT: i16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

impl Point {
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }

    pub fn in_bounds(self) -> bool {
        (0..MAP_WIDTH).contains(&self.x) && (0..MAP_HEIGHT).contains(&self.y)
    }

    /// Chebyshev distance: turns needed to walk between points on open ground.
    pub fn distance(self, other: Point) -> i16 {
        (self.x - other.x).abs().max((self.y - other.y).abs())
    }

    pub fn step(self, dir: Direction) -> Point {
        let (dx, dy) = dir.delta();
        Point::new(self.x + dx, self.y + dy)
    }

    pub fn is_adjacent(self, other: Point) -> bool {
        self != other && self.distance(other) <= 1
    }

    pub fn neighbours(self) -> impl Iterator<Item = Point> {
        Direction::ALL.iter().map(move |dir| self.step(*dir))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl Direction {
    pub const ALL: [Direction; 8] = [
        Direction::North,
        Direction::NorthEast,
        Direction::East,
        Direction::SouthEast,
        Direction::South,
        Direction::SouthWest,
        Direction::West,
        Direction::NorthWest,
    ];

    /// All eight directions with the four orthogonals first. A breadth-first
    /// search that visits neighbours in this order breaks ties toward
    /// orthogonal steps, so equal-length paths prefer straight movement over
    /// diagonal.
    pub const ORTHOGONAL_FIRST: [Direction; 8] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
        Direction::NorthEast,
        Direction::SouthEast,
        Direction::SouthWest,
        Direction::NorthWest,
    ];

    pub const fn delta(self) -> (i16, i16) {
        match self {
            Direction::North => (0, -1),
            Direction::NorthEast => (1, -1),
            Direction::East => (1, 0),
            Direction::SouthEast => (1, 1),
            Direction::South => (0, 1),
            Direction::SouthWest => (-1, 1),
            Direction::West => (-1, 0),
            Direction::NorthWest => (-1, -1),
        }
    }

    /// The direction that best steps from `from` toward `to`.
    pub fn toward(from: Point, to: Point) -> Option<Direction> {
        if from == to {
            return None;
        }
        let dx = (to.x - from.x).signum();
        let dy = (to.y - from.y).signum();
        Direction::ALL
            .iter()
            .copied()
            .find(|dir| dir.delta() == (dx, dy))
    }
}

/// Every point strictly between `from` and `to` on a Bresenham line.
///
/// Symmetric: the walk always runs from the lexicographically smaller
/// endpoint so `line_between(a, b) == line_between(b, a)`.
pub fn line_between(from: Point, to: Point) -> Vec<Point> {
    let (a, b) = if (from.x, from.y) <= (to.x, to.y) {
        (from, to)
    } else {
        (to, from)
    };
    let mut points = Vec::new();
    let dx = (b.x - a.x).abs();
    let dy = -(b.y - a.y).abs();
    let sx: i16 = if a.x < b.x { 1 } else { -1 };
    let sy: i16 = if a.y < b.y { 1 } else { -1 };
    let mut err = dx + dy;
    let mut current = a;
    loop {
        if current == b {
            break;
        }
        if current != a {
            points.push(current);
        }
        let doubled = 2 * err;
        if doubled >= dy {
            err += dy;
            current.x += sx;
        }
        if doubled <= dx {
            err += dx;
            current.y += sy;
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_chebyshev() {
        assert_eq!(Point::new(0, 0).distance(Point::new(3, 1)), 3);
        assert_eq!(Point::new(5, 5).distance(Point::new(5, 5)), 0);
    }

    #[test]
    fn orthogonal_first_lists_all_orthogonals_before_diagonals() {
        // The pathfinding tie-break depends on every orthogonal direction
        // coming before any diagonal in this order.
        let is_diagonal = |dir: Direction| dir.delta().0 != 0 && dir.delta().1 != 0;
        let first_diagonal = Direction::ORTHOGONAL_FIRST
            .iter()
            .position(|d| is_diagonal(*d))
            .unwrap();
        assert_eq!(first_diagonal, 4, "four orthogonals should come first");
        // Same eight directions, just reordered.
        let mut a = Direction::ALL;
        let mut b = Direction::ORTHOGONAL_FIRST;
        a.sort_by_key(|d| d.delta());
        b.sort_by_key(|d| d.delta());
        assert_eq!(a, b);
    }

    #[test]
    fn line_between_is_symmetric() {
        let a = Point::new(1, 1);
        let b = Point::new(7, 4);
        assert_eq!(line_between(a, b), line_between(b, a));
    }

    #[test]
    fn line_between_excludes_endpoints() {
        let a = Point::new(0, 0);
        let b = Point::new(3, 0);
        let line = line_between(a, b);
        assert_eq!(line, vec![Point::new(1, 0), Point::new(2, 0)]);
    }

    #[test]
    fn adjacent_line_is_empty() {
        assert!(line_between(Point::new(2, 2), Point::new(3, 3)).is_empty());
    }
}
