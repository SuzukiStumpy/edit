# ADR 0006 — Full Unicode now (width + segmentation data crates)

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

Terminals deal in *columns*, not characters. A wide character (CJK, many emoji)
occupies two columns; misjudging it corrupts the rest of the line and desyncs the
cursor. A grapheme cluster (base + combining marks, ZWJ emoji, flags) is what a
human calls "one character" but is several scalars, and correct cursor movement
must step over whole clusters. Doing either correctly needs the Unicode Character
Database — data, not logic — which is wildly disproportionate to hand-roll.

## Decision

Handle Unicode correctly from the start, accepting two **pure data-table** crates:
`unicode-width` (column width) and `unicode-segmentation` (grapheme clusters). A
`Cell` therefore holds a **grapheme cluster + computed width**, not a single
`char`; wide cells reserve a following continuation cell the renderer skips.
Editor cursor movement steps by grapheme.

## Consequences

- International text, emoji, and combining marks render and edit correctly.
- The renderer/diff must track width and continuation cells; cursor and column
  math go through a width helper.
- Two more crates in the runtime budget (still data-only, no logic): the budget
  is now `crossterm`, `unicode-width`, `unicode-segmentation`.

## Alternatives considered

- **Single-width first, seam ready** — authentic to DOS's CP437 world and keeps
  crates minimal, but defers correctness; rejected because we'd rather pay the
  width-threading cost once, up front.
- **Hand-roll Unicode tables** — zero deps, wildly disproportionate effort.
