//! Geometric primitives shared across the framework.
//!
//! Coordinates are `i16`: terminal screens are tiny, and signed values keep
//! off-screen math (clipping, scrolling, negative offsets) painless. This is
//! the seed module for Phase 1 — [`super::geometry`] gains `Size` and `Rect`
//! alongside `Point` there (see docs/roadmap.md).

/// A point in cell coordinates: column `x`, row `y`, with the origin at the
/// top-left of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Point {
    /// Column, increasing rightward.
    pub x: i16,
    /// Row, increasing downward.
    pub y: i16,
}

impl Point {
    /// Creates a point at `(x, y)`.
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }

    /// Returns this point translated by `dx` columns and `dy` rows.
    pub const fn offset(self, dx: i16, dy: i16) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The first test of the project: proves the TDD harness end-to-end
    // (workspace builds, `cargo test` discovers and runs unit tests).
    #[test]
    fn offset_translates_both_axes() {
        let moved = Point::new(3, 4).offset(-1, 2);
        assert_eq!(moved, Point::new(2, 6));
    }

    #[test]
    fn default_is_origin() {
        assert_eq!(Point::default(), Point::new(0, 0));
    }
}
