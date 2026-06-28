# Module spec: `rvision::canvas`

- **Status:** Done
- **Phase:** 3 (View system)
- **Related ADRs:** 0002 (render seam / `Buffer`), 0015 (owner-relative coords + Canvas)

## Purpose

A **translating, clipping draw surface** over a [`Buffer`]. A `Canvas` lets a view
draw in its own local `(0, 0)`-origin coordinates while writes land at the right
absolute position and are confined to the view's box (and every ancestor's). It
is the draw half of the seam the `View` trait sits on (ADR 0015).

It is **not** a buffer: it owns no cells, just borrows one. It does not know about
views, focus, or events — only geometry and drawing.

## Public interface

```rust
pub struct Canvas<'a> { /* &mut Buffer + offset + size + clip */ }

impl<'a> Canvas<'a> {
    /// Root canvas: local (0,0) == buffer (0,0), clip == whole buffer.
    pub fn new(buffer: &'a mut Buffer) -> Self;

    /// Sub-canvas for `area` (in *this* canvas's local coords). The child's
    /// local (0,0) is `area.origin`; its clip is the overlap of this clip and
    /// the child's absolute box (so an off-parent child draws nothing there).
    pub fn child(&mut self, area: Rect) -> Canvas<'_>;

    pub fn size(&self) -> Size;          // nominal local size (the view's area)
    pub fn bounds(&self) -> Rect;        // local rect (0,0)..size

    pub fn set(&mut self, at: Point, cell: Cell);           // local coords
    pub fn put_str(&mut self, at: Point, s: &str, style: Style) -> i16;
    pub fn fill(&mut self, area: Rect, cell: &Cell);
    pub fn draw_box(&mut self, area: Rect, style: Style);
}
```

The primitives mirror [`Buffer`]'s so a view's draw code reads the same whether it
targets a buffer directly (tests) or a canvas. `put_str` returns the local column
just past the last cell written, like `Buffer::put_str`.

## Behaviour & invariants

- **Translation.** A write at local `p` lands at absolute `offset + p`.
- **Clip is containment, not just screen-edge.** A write outside `clip` is a
  silent no-op, so a child can never paint into a sibling or past its own box —
  stronger than `Buffer`'s screen-edge clip alone (ADR 0015). `clip` is always a
  subset of the buffer bounds.
- **`child` composes.** `child(a).child(b)` offsets by `a.origin + b.origin` and
  clips to the intersection of both boxes; a child wholly off the parent yields an
  empty clip and draws nothing.
- **Wide graphemes.** A width-2 cell whose continuation column would fall outside
  `clip` is dropped (blank), matching `Buffer`'s right-edge rule — no half-glyph
  spills across the boundary.
- **Negative/empty areas.** `child` of an empty or reversed `area` yields an empty
  clip; all primitives become no-ops. No panics for any input.
- `size` is the view's nominal area even where `clip` is smaller (child clipped by
  an ancestor); `bounds()` is `(0,0)..size`.

## Collaborators

- [`Buffer`] — the borrowed grid it writes into; the final per-cell write goes
  through `Buffer::set` (which also handles wide-cell continuation) once the
  canvas has confirmed the absolute point is inside `clip`.
- `cell`/`color`/`geometry` — `Cell`, `Style`, `Point`/`Rect`/`Size`.
- The grapheme→cell iteration is shared with `Buffer::put_str` via one helper so
  width/continuation logic is not duplicated.
- Used by `view::View::draw` and `view::Group` (the parent builds a child canvas
  per child).

## Test plan (write these first)

- **Logic:** root canvas maps local→absolute 1:1; `child` offsets origin and
  intersects clip; nested `child` composes offsets; off-parent child has empty
  clip.
- **Render (snapshot):** draw a boxed, titled child onto a larger buffer and
  snapshot the buffer — the box sits at the child's offset, content clipped to it.
- **Clipping:** a `put_str` longer than the child's width stops at the child's
  right edge, not the screen edge (no spill into the neighbouring region).
- **Wide grapheme:** a width-2 glyph at the child's right edge is dropped, not
  split across the boundary.
- **Property (light):** for random nested boxes, no cell outside a child's clip is
  ever modified by drawing through that child.

## Open questions

- Do views ever need to read back what they drew (e.g. for cursor placement)? Not
  yet; `Canvas` stays write-only for now. Revisit if the editor's cursor logic
  wants it (Phase 6) rather than adding speculatively.
