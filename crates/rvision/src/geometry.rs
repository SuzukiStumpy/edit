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

/// A 2-D size in cells: `width` columns by `height` rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Size {
    /// Width in columns.
    pub width: i16,
    /// Height in rows.
    pub height: i16,
}

impl Size {
    /// Creates a size of `width` columns by `height` rows.
    pub const fn new(width: i16, height: i16) -> Self {
        Self { width, height }
    }

    /// Returns whether this size covers no cells (either dimension `<= 0`).
    pub const fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

/// An axis-aligned rectangle in cell coordinates.
///
/// Stored as an origin (top-left, inclusive) plus a [`Size`]. Coordinates are
/// **half-open**: the rectangle covers `[left, right) x [top, bottom)`, so
/// [`Rect::bottom_right`] is the first cell *outside* the rectangle and abutting
/// rectangles never double-count their shared edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Rect {
    origin: Point,
    size: Size,
}

impl Rect {
    /// Creates a rectangle with the given top-left `origin` and `size`.
    pub const fn from_origin_size(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    /// Creates a rectangle from a top-left corner `a` (inclusive) and a
    /// bottom-right corner `b` (exclusive). If `b` is above or left of `a` on an
    /// axis, that dimension clamps to zero (an empty rectangle) rather than
    /// going negative.
    pub fn from_corners(a: Point, b: Point) -> Self {
        let width = (b.x - a.x).max(0);
        let height = (b.y - a.y).max(0);
        Self {
            origin: a,
            size: Size::new(width, height),
        }
    }

    /// The top-left corner (inclusive).
    pub const fn origin(self) -> Point {
        self.origin
    }

    /// The rectangle's size.
    pub const fn size(self) -> Size {
        self.size
    }

    /// Width in columns.
    pub const fn width(self) -> i16 {
        self.size.width
    }

    /// Height in rows.
    pub const fn height(self) -> i16 {
        self.size.height
    }

    /// The bottom-right corner, **exclusive** — `origin + size`, i.e. the first
    /// cell outside the rectangle on each axis.
    pub const fn bottom_right(self) -> Point {
        self.origin.offset(self.size.width, self.size.height)
    }

    /// Returns whether the rectangle covers no cells.
    pub const fn is_empty(self) -> bool {
        self.size.is_empty()
    }

    /// Returns whether `p` lies inside the rectangle. Half-open: the right and
    /// bottom edges are excluded, so an empty rectangle contains nothing.
    pub const fn contains(self, p: Point) -> bool {
        let br = self.bottom_right();
        p.x >= self.origin.x && p.x < br.x && p.y >= self.origin.y && p.y < br.y
    }

    /// Returns the overlap of two rectangles, or `None` if they are disjoint.
    /// Half-open, so rectangles that merely touch at an edge do not overlap.
    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let self_br = self.bottom_right();
        let other_br = other.bottom_right();
        let left = self.origin.x.max(other.origin.x);
        let top = self.origin.y.max(other.origin.y);
        let right = self_br.x.min(other_br.x);
        let bottom = self_br.y.min(other_br.y);
        if right > left && bottom > top {
            Some(Rect::from_corners(
                Point::new(left, top),
                Point::new(right, bottom),
            ))
        } else {
            None
        }
    }

    /// Returns the smallest rectangle containing both `self` and `other`. An
    /// empty operand is ignored (the union is the other rectangle).
    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let self_br = self.bottom_right();
        let other_br = other.bottom_right();
        let left = self.origin.x.min(other.origin.x);
        let top = self.origin.y.min(other.origin.y);
        let right = self_br.x.max(other_br.x);
        let bottom = self_br.y.max(other_br.y);
        Rect::from_corners(Point::new(left, top), Point::new(right, bottom))
    }

    /// Returns the rectangle translated by `dx` columns and `dy` rows; the size
    /// is unchanged.
    pub const fn offset(self, dx: i16, dy: i16) -> Rect {
        Rect {
            origin: self.origin.offset(dx, dy),
            size: self.size,
        }
    }

    /// Returns the rectangle inflated by `dx` columns on each left/right edge
    /// and `dy` rows on each top/bottom edge (negative values deflate). Over-
    /// deflating clamps the size to zero rather than going negative.
    pub fn grow(self, dx: i16, dy: i16) -> Rect {
        let width = (self.size.width + 2 * dx).max(0);
        let height = (self.size.height + 2 * dy).max(0);
        Rect {
            origin: self.origin.offset(-dx, -dy),
            size: Size::new(width, height),
        }
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

    // --- Rect (tracer bullet): construction and the two-corner view ---

    #[test]
    fn rect_exposes_origin_size_and_exclusive_corner() {
        let r = Rect::from_origin_size(Point::new(2, 3), Size::new(10, 5));
        assert_eq!(r.origin(), Point::new(2, 3));
        assert_eq!(r.size(), Size::new(10, 5));
        assert_eq!(r.width(), 10);
        assert_eq!(r.height(), 5);
        // bottom_right is the EXCLUSIVE corner: origin + size.
        assert_eq!(r.bottom_right(), Point::new(12, 8));
    }

    #[test]
    fn contains_is_half_open() {
        // Covers columns 2..6 and rows 3..5.
        let r = Rect::from_origin_size(Point::new(2, 3), Size::new(4, 2));
        // Top-left corner is included; so is the last interior cell.
        assert!(r.contains(Point::new(2, 3)));
        assert!(r.contains(Point::new(5, 4)));
        // Right and bottom edges are excluded (half-open).
        assert!(!r.contains(Point::new(6, 4)));
        assert!(!r.contains(Point::new(5, 5)));
        // Plainly outside.
        assert!(!r.contains(Point::new(1, 3)));
        assert!(!r.contains(Point::new(2, 2)));
    }

    #[test]
    fn from_corners_derives_size_and_clamps_reversed() {
        let r = Rect::from_corners(Point::new(2, 3), Point::new(7, 9));
        assert_eq!(r.origin(), Point::new(2, 3));
        assert_eq!(r.size(), Size::new(5, 6));
        // The exclusive bottom-right corner round-trips.
        assert_eq!(r.bottom_right(), Point::new(7, 9));

        // Reversed corners clamp to empty rather than producing a negative size.
        let degenerate = Rect::from_corners(Point::new(7, 9), Point::new(2, 3));
        assert_eq!(degenerate.size(), Size::new(0, 0));
    }

    #[test]
    fn is_empty_for_zero_or_negative_dimensions() {
        assert!(Size::new(0, 5).is_empty());
        assert!(Size::new(5, 0).is_empty());
        assert!(Size::new(-1, 5).is_empty());
        assert!(!Size::new(1, 1).is_empty());

        let origin = Point::new(1, 1);
        assert!(Rect::from_origin_size(origin, Size::new(0, 3)).is_empty());
        assert!(!Rect::from_origin_size(origin, Size::new(3, 3)).is_empty());
    }

    #[test]
    fn intersection_overlap_disjoint_and_edge_touch() {
        let a = Rect::from_corners(Point::new(0, 0), Point::new(4, 4));
        let b = Rect::from_corners(Point::new(2, 2), Point::new(6, 6));
        let overlap = Rect::from_corners(Point::new(2, 2), Point::new(4, 4));

        // Overlapping rects yield the shared sub-rect, and it's symmetric.
        assert_eq!(a.intersection(b), Some(overlap));
        assert_eq!(b.intersection(a), Some(overlap));

        // Fully disjoint rects do not intersect.
        let disjoint = Rect::from_corners(Point::new(10, 10), Point::new(12, 12));
        assert_eq!(a.intersection(disjoint), None);

        // Edge-touching rects share only an (excluded) edge, so no overlap.
        let touching = Rect::from_corners(Point::new(4, 0), Point::new(8, 4));
        assert_eq!(a.intersection(touching), None);
    }

    #[test]
    fn union_is_bounding_box_and_empty_aware() {
        let a = Rect::from_corners(Point::new(0, 0), Point::new(2, 2));
        let b = Rect::from_corners(Point::new(5, 1), Point::new(7, 4));
        assert_eq!(
            a.union(b),
            Rect::from_corners(Point::new(0, 0), Point::new(7, 4))
        );

        // Union with an empty rect yields the other rect unchanged.
        let empty = Rect::from_origin_size(Point::new(100, 100), Size::new(0, 0));
        assert_eq!(a.union(empty), a);
        assert_eq!(empty.union(a), a);
    }

    #[test]
    fn offset_moves_origin_preserves_size() {
        let r = Rect::from_origin_size(Point::new(1, 1), Size::new(4, 3));
        let moved = r.offset(2, -1);
        assert_eq!(moved.origin(), Point::new(3, 0));
        assert_eq!(moved.size(), Size::new(4, 3));
    }

    #[test]
    fn grow_inflates_and_can_collapse_to_empty() {
        // Covers 5..9 on each axis.
        let r = Rect::from_origin_size(Point::new(5, 5), Size::new(4, 4));

        // Inflate one cell each side: origin -1,-1; size +2 per axis.
        let bigger = r.grow(1, 1);
        assert_eq!(bigger.origin(), Point::new(4, 4));
        assert_eq!(bigger.size(), Size::new(6, 6));

        // Deflate symmetrically.
        let smaller = r.grow(-1, -1);
        assert_eq!(smaller.origin(), Point::new(6, 6));
        assert_eq!(smaller.size(), Size::new(2, 2));

        // Over-deflating collapses to empty without panicking or going negative.
        assert!(r.grow(-10, -10).is_empty());
    }
}
