//! Input and lifecycle events, and the typed result of handling one.
//!
//! The event loop ([`crate::app`]) feeds [`Event`]s to a handler, which returns
//! an [`EventResult`] saying whether it consumed the event (ADR 0004). Unlike
//! TurboVision — which signals "handled" by mutating the event to `evNothing` —
//! we never mutate the event: consumption is a typed return value, and handlers
//! receive `&Event`, so "clear the event" is impossible by construction.
//!
//! These types are deliberately backend-agnostic: crossterm's own event types
//! never appear above the backend seam (ADR 0001, 0002). The
//! [`crate::crossterm_backend`] module translates raw crossterm input into them.

use crate::geometry::{Point, Size};
use core::ops::BitOr;

/// Modifier keys held during a key or mouse event, stored as a bitset.
///
/// Mirrors [`crate::color::Attributes`]: combine with `|` (or [`Modifiers::union`])
/// and query with [`Modifiers::contains`], which is true only when *all* of the
/// queried bits are present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// The Shift key.
    pub const SHIFT: Self = Self(1 << 0);
    /// The Control key.
    pub const CONTROL: Self = Self(1 << 1);
    /// The Alt (Option) key.
    pub const ALT: Self = Self(1 << 2);

    /// Returns the empty modifier set.
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Returns whether no modifiers are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns whether every bit in `other` is also set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns the union of two modifier sets.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// A logical key, independent of the terminal's byte encoding.
///
/// Only the keys the editor needs are modelled; anything else a backend reads is
/// dropped at the seam rather than represented here.
///
/// **Not yet universal.** This is intentionally a partial set. Keys a backend can
/// report but we don't yet model — `CapsLock`, `ScrollLock`, `NumLock`,
/// `PrintScreen`, `Pause`, `Menu`, the keypad/media keys, and `F13`..`F24` — are
/// silently discarded at the seam (see `crossterm_backend::map_key_code`). To make
/// `rvision` a truly general-purpose library we should round this out; revisit
/// when a use case actually needs one of them rather than adding speculatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A character key. For control combinations the base character is reported
    /// (e.g. Ctrl-Q is `Char('q')` with [`Modifiers::CONTROL`]).
    Char(char),
    /// Enter / Return.
    Enter,
    /// Escape.
    Esc,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Shift-Tab (back-tab).
    BackTab,
    /// Delete (forward delete).
    Delete,
    /// Insert.
    Insert,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// A function key, `F(1)` through `F(12)`.
    F(u8),
}

/// A key press: which [`KeyCode`] and which [`Modifiers`] were held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// The key itself.
    pub code: KeyCode,
    /// Modifier keys held during the press.
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// Creates a key event from a code and modifier set.
    pub const fn new(code: KeyCode, modifiers: Modifiers) -> Self {
        Self { code, modifiers }
    }

    /// Creates an unmodified character key event.
    pub const fn char(c: char) -> Self {
        Self::new(KeyCode::Char(c), Modifiers::NONE)
    }
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button.
    Middle,
}

/// What a [`MouseEvent`] is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseKind {
    /// A button was pressed.
    Down(MouseButton),
    /// A button was released.
    Up(MouseButton),
    /// The mouse moved with a button held.
    Drag(MouseButton),
    /// The mouse moved with no button held.
    Moved,
    /// The wheel scrolled up.
    ScrollUp,
    /// The wheel scrolled down.
    ScrollDown,
}

/// A mouse event: what happened, where, and with which modifiers.
///
/// The positional dispatch phase (ADR 0004) routes these from day one; mouse
/// *behaviour* in widgets is filled in later (ADR 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    /// What the mouse did.
    pub kind: MouseKind,
    /// Cell position of the pointer.
    pub pos: Point,
    /// Modifier keys held.
    pub modifiers: Modifiers,
}

/// A command identifier: the integer id of a UI action (TurboVision's `cmXxx`).
///
/// Phase 3's `command` module builds enable/disable sets and up-the-owner-chain
/// bubbling around this id; for now it is just the payload of the [`Event`]
/// command variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Command(pub u16);

/// Anything the loop can deliver to a handler.
///
/// `Event` is `Copy`: every variant is small and heap-free, so dispatch never
/// clones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse action occurred.
    Mouse(MouseEvent),
    /// A command was posted (routed to the focus chain / up the owner chain).
    Command(Command),
    /// A command broadcast to every view (payload grows in a later phase).
    Broadcast(Command),
    /// The terminal was resized to the given size.
    Resize(Size),
    /// The poll timeout elapsed with no input — drives blink/idle work.
    Idle,
}

/// Whether a handler consumed an event (ADR 0004). The typed replacement for
/// TurboVision's `clearEvent` mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventResult {
    /// The event was handled; routing stops.
    Consumed,
    /// The event was not handled; routing continues to the next handler/phase.
    Ignored,
}

impl EventResult {
    /// Returns whether the event was consumed.
    pub const fn is_consumed(self) -> bool {
        matches!(self, EventResult::Consumed)
    }

    /// Returns whether the event was ignored.
    pub const fn is_ignored(self) -> bool {
        matches!(self, EventResult::Ignored)
    }

    /// Chaining primitive for three-phase dispatch (ADR 0004): returns `self` if
    /// it is [`Consumed`](EventResult::Consumed), otherwise runs `f` for the next
    /// phase's result.
    pub fn or_else(self, f: impl FnOnce() -> EventResult) -> EventResult {
        match self {
            EventResult::Consumed => EventResult::Consumed,
            EventResult::Ignored => f(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tracer bullet: a modifier set behaves like the attribute bitset it mirrors.
    #[test]
    fn modifiers_combine_and_query() {
        assert!(Modifiers::NONE.is_empty());
        assert!(!Modifiers::NONE.contains(Modifiers::CONTROL));

        let combo = Modifiers::CONTROL | Modifiers::SHIFT;
        assert!(combo.contains(Modifiers::CONTROL));
        assert!(combo.contains(Modifiers::SHIFT));
        assert!(!combo.contains(Modifiers::ALT));

        // `contains` means "all of": a multi-bit query matches only if every bit
        // is present.
        assert!(combo.contains(Modifiers::CONTROL | Modifiers::SHIFT));
        assert!(!combo.contains(Modifiers::CONTROL | Modifiers::ALT));

        assert_eq!(Modifiers::CONTROL.union(Modifiers::SHIFT), combo);
    }

    #[test]
    fn char_key_has_no_modifiers() {
        let k = KeyEvent::char('q');
        assert_eq!(k.code, KeyCode::Char('q'));
        assert!(k.modifiers.is_empty());
    }

    #[test]
    fn events_are_value_comparable() {
        let ctrl_q = Event::Key(KeyEvent::new(KeyCode::Char('q'), Modifiers::CONTROL));
        assert_eq!(ctrl_q, ctrl_q);
        assert_ne!(ctrl_q, Event::Idle);
        assert_eq!(
            Event::Resize(Size::new(80, 25)),
            Event::Resize(Size::new(80, 25))
        );
    }

    #[test]
    fn event_result_predicates() {
        assert!(EventResult::Consumed.is_consumed());
        assert!(!EventResult::Consumed.is_ignored());
        assert!(EventResult::Ignored.is_ignored());
        assert!(!EventResult::Ignored.is_consumed());
    }

    #[test]
    fn or_else_short_circuits_on_consumed() {
        // Consumed never runs the fallback.
        let mut ran = false;
        let r = EventResult::Consumed.or_else(|| {
            ran = true;
            EventResult::Consumed
        });
        assert_eq!(r, EventResult::Consumed);
        assert!(!ran, "fallback must not run once consumed");

        // Ignored defers to the fallback's result.
        let r = EventResult::Ignored.or_else(|| EventResult::Consumed);
        assert_eq!(r, EventResult::Consumed);
        let r = EventResult::Ignored.or_else(|| EventResult::Ignored);
        assert_eq!(r, EventResult::Ignored);
    }
}
