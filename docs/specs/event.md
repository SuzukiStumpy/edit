# Module spec: `rvision::event`

- **Status:** In progress
- **Phase:** 2 (real terminal & event loop)
- **Related ADRs:** 0004 (three-phase dispatch, `EventResult`), 0001/0002 (the
  crossterm types never appear above the backend seam), 0007 (mouse architected,
  keyboard-first)

## Purpose

The backend-agnostic vocabulary of *things that happen*: an [`Event`] enum and
the typed [`EventResult`] a handler returns to say whether it consumed the event.
This module is pure data — no I/O, no crossterm. The `crossterm_backend` module
translates raw crossterm input into these types so nothing above the seam ever
names a crossterm type (ADR 0001/0002).

What it is *not*: it does not dispatch events (that's `app` now, the view tree in
Phase 3), and it does not read input (that's `EventSource`).

## Public interface

```rust
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Command(Command),   // a posted UI command (Phase 3 builds bubbling around it)
    Broadcast(Command), // a command delivered to all (payload grows later)
    Resize(Size),       // the terminal's new size
    Paste(String),      // a bracketed paste, delivered as one chunk (ADR 0022)
    Idle,               // the poll timeout elapsed — drives blink/idle
}

pub enum EventResult { Consumed, Ignored }
impl EventResult {
    pub const fn is_consumed(self) -> bool;
    pub const fn is_ignored(self) -> bool;
    pub fn or_else(self, f: impl FnOnce() -> EventResult) -> EventResult;
}

pub struct KeyEvent { pub code: KeyCode, pub modifiers: Modifiers }
pub enum KeyCode { Char(char), Enter, Esc, Backspace, Tab, BackTab, Delete,
                   Insert, Left, Right, Up, Down, Home, End, PageUp, PageDown,
                   F(u8) }
pub struct Modifiers(u8); // SHIFT | CONTROL | ALT — a bitset like color::Attributes

pub struct MouseEvent { pub kind: MouseKind, pub pos: Point, pub modifiers: Modifiers }
pub enum MouseKind { Down(MouseButton), Up(MouseButton), Drag(MouseButton),
                     Moved, ScrollUp, ScrollDown }
pub enum MouseButton { Left, Right, Middle }

pub struct Command(pub u16);
```

`Event` is `Clone` but not `Copy` since `Paste` owns a `String` (ADR 0022); every
other variant is heap-free, and dispatch passes events by reference, so a clone is
rare. The smaller types (`KeyEvent`, `MouseEvent`, `Command`, …) stay `Copy`.
`Paste` is a focused-phase event: a `Group` routes it to the focused child, like a
key.

## Behaviour & invariants

- **Consumption is a return value, never a mutation** (ADR 0004). Handlers take
  `&Event`; the immutable borrow makes "clear the event to mark it handled"
  impossible by construction.
- `EventResult::or_else` is the three-phase chaining primitive: a `Consumed`
  short-circuits; an `Ignored` runs the next phase. (The phases themselves arrive
  in Phase 3.)
- `Modifiers` mirrors `color::Attributes`: `contains` is "all of", `|`/`union`
  combine, `NONE`/`is_empty` for the empty set.
- `Idle` is synthesised by the loop when `poll` times out; it is not produced by
  the backend.

## Collaborators

`geometry::{Point, Size}`. Consumed by `app` (the loop), `crossterm_backend` (which
produces `Event`s), and the Phase 3 view tree.

## Test plan (write these first)

- **Logic:** `Modifiers` contains/union/empty; `KeyEvent::char` has no modifiers;
  `EventResult` predicates and `or_else` short-circuit semantics; events are `Eq`.

## Open questions

- `Broadcast` currently carries only a `Command`; a richer payload (sender id,
  data) lands when widgets need to talk (Phase 3+), via a struct it can grow into.
- **`KeyCode` is not yet a universal key set.** It models only the keys the editor
  needs; lock/system/keypad/media keys and `F13`+ are dropped at the seam. Rounding
  this out is wanted to make `rvision` a general-purpose library — do it when a real
  use case needs a missing key, not speculatively.
