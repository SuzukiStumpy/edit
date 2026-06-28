# ADR 0002 — Backend/EventSource traits + double-buffer cell diff

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

A TUI is hard to test because output goes to a real terminal and input comes from
real keypresses. TDD is a hard requirement (ADR 0013), so we need a seam that
makes rendering and event handling testable headlessly. TurboVision itself drew
into in-memory buffers and blitted damaged regions, so this is also faithful.

## Decision

The framework never talks to crossterm directly. It draws into an in-memory
**back buffer** of `Cell`s (character/grapheme + style). Two traits form the
seam:

- **`Backend`** — flushes a buffer to a target.
- **`EventSource`** — supplies input events.

A `CrosstermBackend`/crossterm event source drives the real app;
a `TestBackend` drives tests, injecting events and exposing the buffer for
assertions. Output uses **double buffering**: the backend diffs the back buffer
against the last-flushed front buffer and emits only changed cells.

## Consequences

- Full TDD of rendering (snapshot the buffer) and interaction (inject events,
  assert the buffer) — see ADR 0013.
- Minimal terminal writes: no flicker, good over SSH.
- Slightly more code than drawing directly: two buffers, a diff, and the trait
  indirection.
- Wide/combining graphemes (ADR 0006) make a `Cell` hold a grapheme + width and
  introduce continuation cells the diff/flush must respect.

## Alternatives considered

- **Single buffer + dirty flags** — lighter, but muddier repaint logic and less
  clean assertions.
- **Draw directly via crossterm** — simplest, but effectively untestable without
  a real terminal; incompatible with the TDD requirement.
