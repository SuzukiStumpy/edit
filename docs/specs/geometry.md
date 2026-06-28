# Module spec: `rvision::geometry`

- **Status:** In progress
- **Phase:** 1
- **Related ADRs:** 0002 (cells/buffer use these types for bounds & clipping)

## Purpose

Integer screen geometry shared across the framework: a point, a size, and a
rectangle. Pure value types with no I/O. `Rect` is the workhorse for view bounds
and draw clipping; `Point` for cursor/cell coordinates; `Size` for dimensions.

Not responsible for: drawing, layout policy, or anything terminal-specific.

## Public interface

```rust
pub struct Point { pub x: i16, pub y: i16 }   // origin top-left; +x right, +y down
pub struct Size  { pub width: i16, pub height: i16 }

impl Point { fn new(x,y)->Self; fn offset(self,dx,dy)->Self; }
impl Size  { fn new(w,h)->Self; fn is_empty(self)->bool; }

// Stored as origin + size; corner getters expose the two-corner view too.
pub struct Rect { /* origin: Point, size: Size (private) */ }

impl Rect {
    fn from_origin_size(origin: Point, size: Size) -> Self;
    fn from_corners(a: Point, b: Point) -> Self;   // b exclusive; size clamped >= 0

    fn origin(self) -> Point;          // top-left, inclusive
    fn size(self) -> Size;
    fn width(self) -> i16;
    fn height(self) -> i16;
    fn top_left(self) -> Point;        // == origin
    fn bottom_right(self) -> Point;    // EXCLUSIVE corner == origin + size

    fn is_empty(self) -> bool;
    fn contains(self, p: Point) -> bool;            // half-open
    fn intersection(self, other: Rect) -> Option<Rect>;   // None if disjoint
    fn union(self, other: Rect) -> Rect;            // bounding box
    fn offset(self, dx: i16, dy: i16) -> Rect;      // move
    fn grow(self, dx: i16, dy: i16) -> Rect;        // inflate (negative deflates)
}
```

## Behaviour & invariants

- **Half-open coordinates:** a rect covers `[left, right) x [top, bottom)`.
  `bottom_right()` is the first cell *outside* the rect. Edges never
  double-count when rects abut.
- **Emptiness:** `width <= 0 || height <= 0` is empty. An empty rect
  `contains` nothing.
- **Intersection** of disjoint or edge-touching rects is `None` (half-open: rects
  that only share an edge do not overlap).
- **Union** is the bounding box; unioning with an empty rect yields the other.
- **grow** by negative values can collapse a rect to empty (never panics; size
  clamped at 0).

## Collaborators

Used by `cell`/`buffer` (ADR 0002) for bounds and clip regions, and later by
every `View` for its bounds. Depends on nothing.

## Test plan (vertical slices, one at a time)

1. (tracer) `from_origin_size` exposes origin, size, exclusive `bottom_right`.
2. `contains` is half-open: inside true; left/top edge in; right/bottom edge out.
3. `from_corners` derives size; reversed/degenerate corners clamp to empty.
4. `is_empty` / `Size::is_empty` for zero and negative dimensions.
5. `intersection`: overlap → sub-rect; disjoint → None; edge-touch → None.
6. `union`: bounding box; with empty → other.
7. `offset` moves origin, preserves size.
8. `grow` inflates/deflates symmetrically; over-deflate → empty.

## Open questions

None outstanding.
