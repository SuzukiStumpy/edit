# Module spec: `rvision::backend`

- **Status:** In progress
- **Phase:** 1 (rendering core) — `EventSource` deferred to Phase 2
- **Related ADRs:** 0002 (Backend seam + double-buffer diff), 0013 (TDD)

## Purpose

The seam between the framework and the outside world's *output*. The framework
draws into an in-memory back [`Buffer`]; a `Backend` takes a finished frame,
diffs it against what is currently on screen, and emits only the changed cells.
A `TestBackend` does this headlessly so rendering is unit-testable; the real
`CrosstermBackend` arrives in Phase 2.

What it is *not*: it does not own the view tree or the draw primitives (that's
`Buffer`), and it does not supply input — the matching `EventSource` trait lands
in Phase 2 with the `Event` type it carries.

## Public interface

```rust
pub trait Backend {
    /// The size of the surface being presented to.
    fn size(&self) -> Size;
    /// Present a finished frame: diff it against the current screen and make the
    /// changed cells visible.
    fn present(&mut self, frame: &Buffer);
}

/// Headless backend for tests: keeps the "screen" in memory and records what the
/// last present would have changed.
pub struct TestBackend { /* screen: Buffer, last_changes: usize, presents: usize */ }
impl TestBackend {
    fn new(size: Size) -> Self;
    fn screen(&self) -> &Buffer;     // current on-screen contents
    fn to_text(&self) -> String;     // convenience over screen().to_text()
    fn last_changes(&self) -> usize; // cells emitted by the most recent present
    fn presents(&self) -> usize;     // number of present() calls
}
impl Backend for TestBackend { /* ... */ }
```

## Behaviour & invariants

- **Double buffer (ADR 0002).** The backend holds the *front* (on-screen) buffer.
  `present(frame)` computes `frame.diff(&front)` (the minimal change set), applies
  it, and adopts `frame` as the new front. A second identical `present` therefore
  reports **zero** changes — the proof that updates are minimal.
- `present` assumes `frame.size() == self.size()` (resize handling is Phase 2).
- `TestBackend` starts as a blank, default-styled screen of the given size.
- Continuation cells of wide graphemes (ADR 0006) ride along in the change set as
  ordinary cells; the test backend stores them verbatim, so its `to_text`
  reproduces the frame exactly.

## Collaborators

Uses `Buffer` (and its `diff`/`to_text`), `geometry::Size`. Consumed by the app
loop (Phase 2), which draws into a back buffer then calls `present`. The real
`CrosstermBackend` (Phase 2) implements the same trait over crossterm.

## Test plan (write these first)

- **Logic / render:** new backend is blank and reports its size; presenting a
  composed frame (box + text) makes `to_text` equal the frame; presenting the
  same frame twice reports zero changes the second time; a one-cell change
  reports exactly that cell; `presents` counts calls.

## Open questions

- Hardware cursor position (show/hide/move) is added when the editor needs it
  (Phase 6) — likely a `set_cursor(Option<Point>)` on the trait.
- `EventSource` (input half of the seam) is specified in Phase 2 with `Event`.
</content>
