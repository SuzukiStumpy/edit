# ADR 0015 — Owner-relative view coordinates via a translating `Canvas`

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

Phase 3 introduces the `View` trait and `Group` (ADR 0003). Every view needs a
coordinate system for two things: where it draws, and how a positional event
(the mouse) is routed to the view under the cursor (ADR 0004). The retained tree
is parent-owns-children, and TurboVision — which this project rebuilds to learn
from — stores each view's bounds **relative to its owner** and draws each view in
its own local `(0, 0)`-origin space. A `TGroup` clips and translates the drawing
surface for each child.

Two models were on the table:

- **Owner-relative + a translating draw surface.** Faithful to TV. A view's
  `bounds()` are in its owner's coordinates; it draws in local coordinates; the
  parent supplies a surface already offset and clipped to the child's box.
- **Absolute screen coordinates.** Simpler now — views draw straight into the
  shared `Buffer` (which already clips at the screen edge, ADR 0002) using
  absolute positions — but moving a group means repositioning every descendant by
  hand. Window dragging (Phase 9) and MDI (Phase 8) would each be a coordinate
  rewrite.

The cost of the first is one new abstraction now; the cost of the second is paid
later, repeatedly, against the grain.

## Decision

Views use **owner-relative coordinates** and draw through a new **`Canvas`**: a
translating, clipping view onto a `Buffer`.

- A view's `bounds()` is a `Rect` in its **owner's** coordinate space.
- `View::draw(&self, canvas: &mut Canvas)` draws in **local** coordinates, where
  local `(0, 0)` is the view's top-left.
- A `Canvas` wraps `&mut Buffer` plus an `offset` (the absolute position of local
  `(0, 0)`), a nominal `size` (the view's own area), and a `clip` (the absolute
  rectangle writes are confined to — the intersection of this view's box with
  every ancestor's). `canvas.child(area)` — with `area` in the parent's local
  coordinates — returns a sub-`Canvas` translated to the child and clipped to the
  overlap of the parent's clip and the child's absolute box.
- `Canvas` mirrors the `Buffer` draw primitives (`set`, `put_str`, `fill`,
  `draw_box`) but every write is clipped to `clip`, so a child can never paint
  outside its box into a sibling — containment the raw screen-edge clip alone does
  not give.
- Positional routing (ADR 0004) is the same translation in reverse: a `Group`
  subtracts a child's origin from the mouse position to hand the child a
  child-local `Point`.

## Consequences

- **Faithful and future-proof.** Moving or nesting a group (windows, MDI, drag —
  Phases 8/9) is a change to one `bounds`, not a walk over descendants.
- **Containment by construction.** The `clip` stops overdraw at the view boundary,
  not just the screen edge; overlapping siblings and partially-off-parent children
  compose correctly.
- **One new abstraction.** `Canvas` is the only addition the roadmap didn't name
  outright; it is the draw half of the seam the View trait sits on. The grapheme→
  cell iteration shared with `Buffer::put_str` is factored into one helper so the
  width/continuation logic is not duplicated.
- **Borrowed, not owned.** `Canvas` borrows the `Buffer`; `child()` reborrows, so
  a parent draws each child fully (sub-canvas dropped) before the next — which
  matches z-ordered, one-pass drawing and needs no back-references.

## Alternatives considered

- **Absolute screen coordinates (no Canvas).** Less code today; views offset by
  their own absolute origin and lean on the screen-edge clip. Rejected: it pushes
  a coordinate rewrite into Phases 8–9 and gives no per-view containment, so an
  overlong `put_str` bleeds into neighbours.
- **A `Buffer` sub-view / windowing type instead of a translating wrapper.**
  Equivalent power, but either copies cells or complicates `Buffer` with offset
  state; a thin borrowing `Canvas` keeps `Buffer` a plain grid (ADR 0002).
