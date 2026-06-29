# Module spec: `rvision::buffer`

- **Status:** In progress
- **Phase:** 1
- **Related ADRs:** 0002 (double-buffer cell rendering), 0006 (Unicode widths)

## Purpose

An in-memory grid of [`Cell`]s — the surface everything draws into before a
backend flushes it. Provides the low-level draw primitives (set, fill, text,
box) and the front-vs-back **diff** that lets a backend write only changed cells.

## Public interface

```rust
pub struct Buffer { /* size: Size, cells: Vec<Cell> (row-major) */ }

impl Buffer {
    fn new(size: Size) -> Self;            // filled with blank, default-styled cells
    fn size(&self) -> Size;
    fn width(&self) -> i16;
    fn height(&self) -> i16;
    fn in_bounds(&self, p: Point) -> bool;
    fn get(&self, p: Point) -> Option<&Cell>;

    fn set(&mut self, p: Point, cell: Cell);          // wide cell sets a continuation
    fn fill(&mut self, area: Rect, cell: &Cell);      // clipped to the buffer
    fn put_str(&mut self, at: Point, s: &str, style: Style) -> i16; // returns end column
    fn draw_box(&mut self, area: Rect, style: Style); // single-line Unicode border

    fn diff<'a>(&self, previous: &Buffer) -> Vec<(Point, &'a Cell)>; // changed cells
    fn to_text(&self) -> String;           // rows joined by '\n', for snapshots/debug
}
```

## Behaviour & invariants

- Row-major, `cells.len() == width * height`. Coordinates use [`Point`]/[`Rect`]
  (half-open). All mutators **clip** silently to the buffer; out-of-bounds writes
  are no-ops, never panics.
- **Wide cells:** writing a width-2 cell at `x` also writes a zero-width
  *continuation* cell at `x+1` (so the diff/backend skip it). If a wide cell
  won't fit before the right edge, a blank is written instead.
- **put_str** segments `s` into grapheme clusters (via `unicode-segmentation`),
  writes each as one cell at successive columns, advancing by the grapheme's
  width, and stops at the right edge / buffer bound. Returns the column after the
  last cell written.
- **draw_box** draws `┌ ┐ └ ┘ ─ │` around `area`; areas narrower/shorter than
  2x2 degrade gracefully (no panic).
- **diff** assumes equal sizes and yields the cells of `self` that differ from
  `previous`, in row-major order.

## Collaborators

Depends on `geometry`, `cell`, `color`, and `unicode-segmentation`. Consumed by
the backend (Phase 2) and every `View::draw` (Phase 3+). `insta` (dev) backs the
snapshot tests.

## Test plan (vertical slices)

1. (tracer) `new` gives right dimensions, blank cells, correct in/out-of-bounds.
2. `set`/`get` round-trips a cell.
3. `fill` paints a clipped rect; outside is untouched.
4. `put_str` writes/advances/clips; wide grapheme consumes two columns.
5. `draw_box` renders the expected border characters.
6. snapshot (insta) of a composed scene (box + text) via `to_text`.
7. `diff` reports exactly the changed cells.

## Open questions

- ~~`shadow` (dim a region for window drop-shadows) deferred until windows
  exist.~~ Resolved (Phase 10): the dim-in-place primitive lives on `Canvas`, not
  `Buffer` — see [canvas.md](canvas.md) `shadow` and the protocol in ADR 0020.
- Style-aware snapshots deferred; Phase 1 snapshots assert text layout, styles
  are asserted via `get().style()`.
