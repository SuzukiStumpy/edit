# ADR 0007 — Architect for mouse, build keyboard-first

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

TurboVision leaned hard on the mouse: click to focus, click menus, drag title
bars to move windows, drag corners to resize, drag scrollbars. crossterm reports
mouse events and our dispatch already reserves a positional phase (ADR 0004), so
the architecture is ready. But keyboard events are trivial to inject and assert
in tests, whereas mouse drag/resize hit-testing is fiddly and roughly doubles
each widget's test surface.

## Decision

Keep the `Mouse` event variant and the positional dispatch phase from day one,
but **implement and test keyboard interaction first**. Mouse *behaviours*
(click-to-focus, menu/button/scrollbar clicks, window move/resize by drag,
drag-select) are filled in during a dedicated later phase (roadmap Phase 9).

## Consequences

- Early TDD stays simple; no retrofit needed when mouse arrives (the seam exists).
- The product is fully mouse-capable by the end, authentic to TV.
- Until Phase 9, the app is keyboard-only — acceptable and even period-accurate.
- **Double-click** is synthesised at the event source (the `EventSource`, where a
  clock lives), not in widgets: a second left-press on the same cell within
  ~400 ms becomes a `MouseKind::DoubleClick`, *in addition to* the ordinary
  `Down`/`Up` — so a view that only reads `Down` sees a plain click and a view that
  wants "activate" reads the follow-up. The timing predicate is pure and tested;
  the wiring rides the live terminal. A list double-click is routed through the
  container's existing Enter path (e.g. the file dialog opens the file / enters the
  folder), keeping "double-click = select + Enter" true without per-widget clocks.

## Alternatives considered

- **Full mouse from day one** — most authentic immediately, but doubles every
  widget's tests and front-loads fiddly drag/resize work.
- **Keyboard only, ever** — simplest, but inauthentic and wastes the positional
  phase.
