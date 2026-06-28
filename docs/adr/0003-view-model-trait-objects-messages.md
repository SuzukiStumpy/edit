# ADR 0003 — Retained-mode view tree: trait objects + message passing

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

TurboVision is fundamentally retained-mode: a persistent tree of view objects
(`TView` → `TGroup` → `TWindow`…), each holding state and receiving events. It
was built on C++ inheritance and free pointer-mixing between views, which Rust's
ownership model is hostile to (the "GUI in Rust" problem). Crucially, TV already
avoided most direct view-to-view references: views post **commands** that bubble
up the owner chain, and the system **broadcasts** messages down the tree. That
message style is both authentic and the idiomatic Rust escape from back-references.

(For the curious: this is retained-mode like the browser DOM or Swing/Qt — *not*
React. React is declarative and diffs an element tree; our only diff is at the
cell level, ADR 0002. The shared intuition is just "data/broadcasts down,
commands/actions up".)

## Decision

A **`View` trait** (draw, handle_event, bounds, focus state). Groups own their
children as `Vec<Box<dyn View>>` (parent-owns-child). Views never hold references
to one another: commands bubble **up**, broadcasts travel **down**, and focus and
message targeting use lightweight integer IDs.

## Consequences

- Sidesteps the borrow checker for cross-view interaction; ownership is a clean
  tree.
- Faithful to TurboVision's command/broadcast model.
- Dynamic dispatch via `dyn View` (negligible cost at terminal scale).
- Some indirection: cross-cutting effects travel as events, not method calls —
  occasionally more verbose, but keeps coupling low.

## Alternatives considered

- **Central arena + handles** — all views in one arena, related by indices.
  Allows direct cross-view access, more boilerplate, less TV-shaped.
- **Single big view enum** — no dynamic dispatch, simplest ownership, but every
  new widget edits central matches; poor extensibility.
