# ADR 0004 — Three-phase event dispatch, `EventResult`, modal `exec_view`

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The event loop drives the view tree (ADR 0003). TurboVision classifies every
event into one of three routing modes and adds a modal execution model that makes
dialogs feel clean. We want that behaviour, but TV signals "handled" by mutating
the event to `evNothing` (`clearEvent`) — a footgun Rust lets us avoid with a
typed return value.

## Decision

- `Event` is a Rust enum: `Key`, `Mouse`, `Command`, `Broadcast`, `Resize`,
  `Idle`.
- **Three-phase dispatch:** *positional* (mouse → view under the cursor),
  *focused* (keys/commands → the focus chain), *broadcast* (delivered to all).
- "Handled" is a returned **`EventResult { Consumed, Ignored }`**, never a mutated
  event.
- Modal dialogs run via **`exec_view`**: a nested loop that blocks until the view
  posts an ending command (`cmOK`/`cmCancel`), which is returned to the caller.

## Consequences

- Type-safe, hard-to-misuse consumption semantics.
- Mouse routing (positional phase) exists from the start even though mouse
  *behaviour* is built later (ADR 0007).
- Broadcasts give widgets a decoupled way to react to one another (e.g. a
  scrollbar telling its owner it moved).
- `exec_view` makes "pop a dialog, get an answer" a single call.

## Alternatives considered

- **Three-phase, faithful `clearEvent`** — maximum fidelity to TV source, less
  idiomatic and more error-prone.
- **Flat dispatch, no phases** — simpler now, but loses positional routing and
  broadcasts that widgets rely on; likely retrofitted later.
