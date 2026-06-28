# Module spec: `rvision::app`

- **Status:** Done (loop + `Root`); the Phase 4 application root `Shell` is specced
  separately in [`shell.md`](shell.md)
- **Phase:** 2 (real terminal & event loop); `app::Shell` added in Phase 4
- **Related ADRs:** 0002 (Backend/EventSource seam), 0004 (events), 0001 (panic-safe
  terminal restore lives at the crossterm boundary), 0016 (`Shell`)

## Purpose

The driver: own the terminal seam and run the main loop — draw a frame, present
it (minimal diff flush), wait for the next event up to a timeout, hand it to the
program, repeat until the program is finished.

What it is *not*: it is not the view tree (Phase 3) and holds no editor concepts.
The thing it drives is abstracted behind the [`Program`] trait so the loop is
unit-testable against a scripted [`TestBackend`]-style terminal with no real TTY.

## Public interface

```rust
/// What the loop drives. In Phase 3 the root view tree implements this.
pub trait Program {
    fn draw(&mut self, frame: &mut Buffer);
    fn handle_event(&mut self, event: &Event) -> EventResult;
    fn is_finished(&self) -> bool;
}

pub struct Application<T> { /* terminal: T, timeout: Duration */ }
impl<T: Backend + EventSource> Application<T> {
    pub fn new(terminal: T) -> Self;                 // default ~100ms idle cadence
    pub fn with_timeout(self, timeout: Duration) -> Self;
    pub fn timeout(&self) -> Duration;
    pub fn terminal(&self) -> &T;
    pub fn terminal_mut(&mut self) -> &mut T;
    pub fn run(&mut self, program: &mut impl Program) -> io::Result<()>;
}
```

## Behaviour & invariants

- **Loop order:** build a fresh `Buffer` at `terminal.size()`, `draw`, `present`,
  break if finished, else `poll_event(timeout)` (`None` ⇒ `Event::Idle`), handle,
  break if finished. Two finish checks bracket the wait so a program that finishes
  while handling an event exits *without* a spurious extra draw, while one that
  starts finished still paints once.
- **Idle:** a timed-out poll becomes `Event::Idle`, so the timeout is the idle/
  blink cadence (filled in later).
- **Resize:** the loop reads `terminal.size()` afresh each frame, so the backend
  updating its reported size while handling a resize (see `EventSource`) is enough
  to relayout next frame.
- **Quitting:** the `Program` decides via `is_finished`. Phase 2 has no command
  bubbling yet, so quit is a flag the program sets (the demo flips it on Ctrl-Q);
  Phase 3 replaces this with a `cmQuit` command bubbling to the app (ADR 0004).
- **Panic safety:** the real backend restores the terminal on `Drop` *and* via a
  panic hook (see `crossterm_backend`); `Application` owning the terminal means any
  unwind through `run` restores it.

## Collaborators

`backend::{Backend, EventSource}`, `buffer::Buffer`, `event::{Event, EventResult}`.

## Test plan (write these first)

- **Interaction (scripted terminal):** quits on the scripted quit key and the
  drawn content reaches the screen; a timed-out poll delivers exactly one `Idle`;
  a scripted resize makes the next `draw` receive the new size.

## Open questions

- Redraw is currently every iteration (the diff makes an unchanged frame flush
  zero cells). Dirty-region tracking is a later optimisation, not needed yet.
- `Program` is a stepping stone to the Phase 3 `View`/`Group` root; expect it to be
  folded into the view tree then.
