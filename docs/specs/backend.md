# Module spec: `rvision::backend`

- **Status:** In progress
- **Phase:** 1 (output seam) + 2 (`EventSource`, real backend)
- **Related ADRs:** 0002 (Backend/EventSource seam + double-buffer diff), 0013 (TDD)

## Purpose

The seam between the framework and the outside world. The framework draws into an
in-memory back [`Buffer`]; a `Backend` takes a finished frame, diffs it against
what is currently on screen, and emits only the changed cells. The matching
`EventSource` supplies input. A `TestBackend` does the output half headlessly so
rendering is unit-testable; the real `CrosstermBackend` (in `crossterm_backend`,
which confines the one crossterm dependency — ADR 0001) implements *both* traits.

What it is *not*: it does not own the view tree or the draw primitives (that's
`Buffer`), and it does not run the loop (that's `app`).

## Public interface

```rust
pub trait Backend {
    /// The size of the surface being presented to.
    fn size(&self) -> Size;
    /// Present a finished frame: diff it against the current screen and make the
    /// changed cells visible. Fallible because a real flush does terminal I/O.
    fn present(&mut self, frame: &Buffer) -> io::Result<()>;
    /// Copy `text` to the host system clipboard if the backend can reach one.
    /// Default no-op; CrosstermBackend emits OSC 52 write-only (ADR 0021).
    fn set_clipboard(&mut self, text: &str) -> io::Result<()> { Ok(()) }
}

pub trait EventSource {
    /// Block up to `timeout` for the next event. `Ok(None)` means the timeout
    /// elapsed (the loop turns this into `Event::Idle`); `Ok(Some(_))` is an event.
    fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<Event>>;
}

/// Headless backend for tests: keeps the "screen" in memory and records what the
/// last present would have changed.
pub struct TestBackend { /* screen, last_changes, presents, clipboard: Option<String> */ }
impl TestBackend {
    fn new(size: Size) -> Self;
    fn screen(&self) -> &Buffer;     // current on-screen contents
    fn to_text(&self) -> String;     // convenience over screen().to_text()
    fn last_changes(&self) -> usize; // cells emitted by the most recent present
    fn presents(&self) -> usize;     // number of present() calls
    fn clipboard(&self) -> Option<&str>; // last text pushed via set_clipboard (ADR 0021)
}
impl Backend for TestBackend { /* ... */ }
```

## Behaviour & invariants

- **Double buffer (ADR 0002).** The backend holds the *front* (on-screen) buffer.
  `present(frame)` computes `frame.diff(&front)` (the minimal change set), applies
  it, and adopts `frame` as the new front. A second identical `present` therefore
  reports **zero** changes — the proof that updates are minimal.
- `present` normally receives a `frame` of `self.size()`. `TestBackend` adopts any
  frame as the new screen (so a larger frame diffs as a full repaint), which is how
  resize falls out for tests; `CrosstermBackend` keeps the two in step by resetting
  its front buffer when it sees a resize (below).
- `TestBackend` starts as a blank, default-styled screen of the given size.
- Continuation cells of wide graphemes (ADR 0006) ride along in the change set as
  ordinary cells; the test backend stores them verbatim, so its `to_text`
  reproduces the frame exactly. `CrosstermBackend` skips them when flushing — the
  wide grapheme to their left already covers that column.

### `CrosstermBackend` (Phase 2, in `crossterm_backend`)

- `new()` enters raw mode + the alternate screen, enables mouse capture and
  bracketed paste, and hides the cursor; `Drop` reverses them. Restore is
  idempotent and also wired into a panic hook so a crash leaves the terminal
  usable and its message readable (ADR 0001).
- `poll_event` is `crossterm::event::poll` then `read`, mapped to our `Event` by a
  pure `map_event` function (the only unit-tested part — no TTY needed). A
  bracketed paste maps to `Event::Paste` (ADR 0022); unmapped crossterm events
  (focus, key *release*) become `Ok(None)`.
- `set_clipboard` writes the OSC 52 escape (ADR 0021); a hand-rolled Base64
  encoder lives in `osc52`.
- On a resize it updates its cached `size`, blanks its front buffer, and clears the
  physical screen, so the loop's next full draw repaints cleanly at the new size.

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
- `present` re-specifies fg/bg/attrs for every changed cell; tracking the current
  terminal style to emit fewer escapes is a later optimisation.
