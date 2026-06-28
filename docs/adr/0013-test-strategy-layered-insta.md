# ADR 0013 — Layered tests + insta snapshots

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

TDD is a hard requirement, and a TUI needs a deliberate test taxonomy or TDD
doesn't actually work. The render seam (ADR 0002) makes headless testing
possible. Asserting big screens cell-by-cell by hand is brutal, so render tests
need a snapshot mechanism.

## Decision

Four test layers:

1. **Logic units** — geometry, buffers, dispatch, `TextBuffer`, `Edit`
   apply/invert (a property test). Plain `#[test]`.
2. **Render tests** — draw into a `TestBackend` and assert with **`insta`**
   snapshots (dev-only dependency).
3. **Interaction tests** — feed a scripted event sequence; assert screen + model.
4. **Manual** — `examples/` demos and running `edit` (colours, real-terminal feel).

## Consequences

- Rendering and interaction are unit-testable, satisfying the TDD requirement.
- `insta` gives ergonomic snapshots and review tooling (`cargo insta review`).
- One dev-only crate added to the budget; runtime budget is unaffected.

## Alternatives considered

- **Hand-rolled snapshot harness** — honours minimal-crates and is a tidy
  exercise, but reinvents what insta already does well.
- **Hand-written assertions only** — zero infrastructure, but verbose and brittle
  as widgets and screens grow.
