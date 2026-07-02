# ADR 0008 — `TextBuffer` trait, line-array implementation first

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The editor's in-memory text representation is its most important data structure.
Options range from a line array (`Vec<String>`) through a gap buffer to a
rope/piece table. The project values readability, TDD, and DOS-Edit-scale files
over huge-file performance, and prefers simple-first with a swappable seam (as
elsewhere).

## Decision

Define a **`TextBuffer` trait** (insert/delete, line access, grapheme
navigation) and implement it first as a **line array** — `Vec<String>`, one
UTF-8 line per element, graphemes via `unicode-segmentation` ([rvision's ADR 0006](https://github.com/SuzukiStumpy/rvision/blob/main/docs/adr/0006-unicode-full-now.md)). A gap
buffer or rope can be added later behind the same trait without touching the
editor view.

## Consequences

- Maps directly onto cursor (line, column); split/join on Enter/Backspace is
  trivial; assertions in tests are clean.
- Fine to multi-MB files; not optimised for million-line files or distant jumps —
  acceptable for scope.
- Swappable later: the trait isolates the editor from the representation.

## Alternatives considered

- **Gap buffer** — the classic; O(1) local edits, but needs a maintained
  line-index and shuffles on long jumps. A good later exercise behind the trait.
- **Rope / piece table** — scales hugely, near-free undo (piece table), but the
  most code and correctness risk; overkill now.
