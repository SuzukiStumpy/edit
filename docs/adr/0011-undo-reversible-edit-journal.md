# ADR 0011 — Reversible-edit journal for undo/redo

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The original MS-DOS Edit had essentially no undo, but a modern editor needs it,
and it's a textbook learning piece. Crucially it has an architectural ripple:
clean undo requires that *every* buffer mutation be expressed as a reversible
operation. Retrofitting that onto ad-hoc mutation code is miserable; designing it
in from the start makes undo "record and replay the inverse".

## Decision

Model every editor mutation as a reversible **`Edit`** (insert/delete text at a
position) with `apply` and `invert`, from day one — all editing flows through it.
The actual multi-level undo/redo **stacks** are wired in a later phase (roadmap
Phase 7), recording operations and replaying inverses, coalescing consecutive
typing into sensible undo units.

## Consequences

- Idiomatic command pattern; memory-light; strong learning value.
- A property test guards correctness from the first commit:
  `invert(apply(x)) == x`.
- The editing code is uniform (one mutation path), simplifying selection,
  clipboard, and find/replace later.

## Alternatives considered

- **Snapshot-based undo** — simpler plumbing, but heavier memory or needs a diff
  algorithm; coarser granularity.
- **No undo (Edit 1.0 parity)** — least work, glaring gap, missed learning.
