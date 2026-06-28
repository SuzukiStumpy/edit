# ADR 0009 — Multi-window MDI, phased in

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The original MS-DOS Edit 1.0 was a single full-screen document, but TurboVision
itself — and the Borland IDE — was multi-window MDI: overlapping windows on a
blue desktop, each draggable/resizable/zoomable, with cascade/tile and Alt+1…9.
That overlapping-windows look *is* the TurboVision signature. Windows are needed
regardless, since dialogs are windows.

## Decision

The framework treats windows as first-class from the start. The editor's **end
goal is full MDI**, but it is **phased**: one editor window works end-to-end
first (roadmap Phase 6), then multi-window is enabled once the window-management
widgets and commands exist (Phase 8). The desktop group already holds N children,
so MDI is largely "enable it" rather than a rewrite.

## Consequences

- Authentic TurboVision experience as the destination.
- More windowing/window-management work overall (drag/resize lands with mouse in
  Phase 9; cascade/tile/Window menu in Phase 8).
- Early phases stay simple with a single maximised window.

## Alternatives considered

- **Single full-screen document** — less work, closer to Edit 1.0, but defers the
  signature TV experience.
- **Split-pane single window** — more modern-editor than classic-TV; a different
  code path from overlapping windows.
