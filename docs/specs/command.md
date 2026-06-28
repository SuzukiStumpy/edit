# Module spec: `rvision::command`

- **Status:** Done
- **Phase:** 3 (View system)
- **Related ADRs:** 0003 (commands bubble up the owner chain), 0004 (`Event::Command`)

## Purpose

The vocabulary of UI **commands** and which of them are currently **enabled**. A
[`Command`] is the integer id of an action (TurboVision's `cmXxx`); a
[`CommandSet`] tracks the enabled/disabled state so a control can grey itself and
a disabled command never fires.

It is **not** the dispatcher: routing a command up the owner chain is the
`view::Group`'s job (ADR 0003). This module is pure data — no views, no events
beyond re-exporting the `Command` id type that `event` already defines.

## Public interface

```rust
pub use crate::event::Command;

// Standard command ids (TV reserves the low numbers).
pub const CM_QUIT: Command;
pub const CM_OK: Command;
pub const CM_CANCEL: Command;

// The framework/application boundary: ids below CM_USER are framework
// standard commands; an app numbers its own from here up (TV's cmUser).
pub const CM_USER: u16 = 100;

pub struct CommandSet { /* set of *disabled* ids; empty => all enabled */ }

impl CommandSet {
    pub fn new() -> Self;                       // everything enabled
    pub fn enable(&mut self, command: Command);
    pub fn disable(&mut self, command: Command);
    pub fn is_enabled(&self, command: Command) -> bool;
}
```

## Behaviour & invariants

- **Open, partitioned namespace.** A `Command` is just a `u16` — an open id space
  (ADR 0003). The framework defines only the standard ids its own widgets emit and
  handle (all below `CM_USER`); an application numbers its commands from `CM_USER`
  up, and the framework routes those opaquely without ever naming them. App
  extensibility is *adding ids*, not editing the framework or the `Event` enum.
- **Enabled by default.** A freshly-`new` set enables every command; you disable
  the exceptions. (Most commands are live most of the time; storing the disabled
  ones keeps the common case allocation-free-ish and the whole `u16` space usable.)
- `enable`/`disable` are idempotent; enabling a never-disabled command is a no-op.
- `is_enabled` is the single query everything else builds on (a control's draw, a
  `Context`'s decision whether to post a command).

## Collaborators

- `event::Command` — the id type, re-exported so callers say `command::Command`.
- `view::Context` — consults `is_enabled` so a disabled command posted by a view
  is silently dropped (a disabled button can't fire), and exposes it so a control
  can render itself greyed.

## Test plan (write these first)

- **Logic:** new set enables an arbitrary id; `disable` then `is_enabled` is false;
  `enable` re-enables; idempotent repeats; two ids are independent.
- **Constants:** the standard ids are distinct and non-zero.

## Open questions

- A 256-bit bitset (like TV) vs a `BTreeSet`: the set is tiny and rarely touched,
  so `BTreeSet` is fine until a profile says otherwise.
