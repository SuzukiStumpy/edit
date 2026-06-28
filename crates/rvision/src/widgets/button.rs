//! A push button: posts a command when activated (TurboVision's `TButton`).
//!
//! A focusable control for a [`Dialog`](super::Dialog). Pressing `Enter` or
//! `Space` while focused posts the button's command up the owner chain
//! (enabled-gated by [`Context`], ADR 0003). A *default* button is the one the
//! dialog activates when `Enter` is pressed away from any button — the dialog
//! reads [`is_default`](Button::is_default) to find it. Focus drives the colour:
//! [`Role::ButtonFocused`] when focused, [`Role::ButtonNormal`] otherwise
//! (ADR 0017).

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::command::Command;
use crate::event::{Event, EventResult, KeyCode};
use crate::geometry::{Point, Rect};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};

/// A labelled push button.
pub struct Button {
    bounds: Rect,
    label: String,
    command: Command,
    default: bool,
    focused: bool,
    normal: Style,
    focused_style: Style,
}

impl Button {
    /// Creates a button at `bounds` labelled `label` that posts `command` when
    /// activated, taking its colours from `theme`.
    pub fn new(bounds: Rect, label: &str, command: Command, theme: &Theme) -> Self {
        Self {
            bounds,
            label: label.to_string(),
            command,
            default: false,
            focused: false,
            normal: theme.style(Role::ButtonNormal),
            focused_style: theme.style(Role::ButtonFocused),
        }
    }

    /// Marks this the dialog's default button (the one `Enter` activates from
    /// anywhere in the dialog). Returns `self` for chaining.
    pub fn default(mut self, yes: bool) -> Self {
        self.default = yes;
        self
    }

    /// Whether this is the dialog's default button.
    pub fn is_default(&self) -> bool {
        self.default
    }

    /// The command this button posts.
    pub fn command(&self) -> Command {
        self.command
    }

    /// The colour to draw in, given the current focus.
    fn style(&self) -> Style {
        if self.focused {
            self.focused_style
        } else {
            self.normal
        }
    }
}

impl View for Button {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let style = self.style();
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(style));
        // Centre the label on the middle row.
        let row = (area.height() - 1).max(0) / 2;
        let text_w = self.label.chars().count() as i16;
        let x = ((area.width() - text_w) / 2).max(0);
        canvas.put_str(Point::new(x, row), &self.label, style);
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        if let Event::Key(key) = event {
            if self.focused && matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                // Gated by Context: a disabled command never fires (ADR 0003).
                ctx.post(self.command);
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }

    fn focusable(&self) -> bool {
        true
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::{CM_OK, CM_USER, CommandSet};
    use crate::event::{KeyEvent, Modifiers};
    use crate::geometry::Size;

    const CM_APPLY: Command = Command(CM_USER + 1);

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    fn button() -> Button {
        Button::new(rect(0, 0, 10, 1), "OK", CM_OK, &Theme::default())
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, Modifiers::NONE))
    }

    #[test]
    fn exposes_its_command_and_default_flag() {
        let b = Button::new(rect(0, 0, 8, 1), "Apply", CM_APPLY, &Theme::default()).default(true);
        assert_eq!(b.command(), CM_APPLY);
        assert!(b.is_default());
        assert!(!button().is_default(), "buttons are non-default by default");
    }

    #[test]
    fn a_focused_button_posts_on_enter_and_space() {
        let cs = CommandSet::new();
        for code in [KeyCode::Enter, KeyCode::Char(' ')] {
            let mut b = button();
            b.set_focused(true);
            let mut ctx = Context::new(&cs);
            assert_eq!(b.handle_event(&key(code), &mut ctx), EventResult::Consumed);
            assert_eq!(ctx.posted(), &[Event::Command(CM_OK)]);
        }
    }

    #[test]
    fn an_unfocused_button_ignores_activation() {
        let mut b = button(); // not focused
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        assert_eq!(
            b.handle_event(&key(KeyCode::Enter), &mut ctx),
            EventResult::Ignored
        );
        assert!(ctx.posted().is_empty());
    }

    #[test]
    fn a_disabled_buttons_command_is_not_posted() {
        let mut b = button();
        b.set_focused(true);
        let mut cs = CommandSet::new();
        cs.disable(CM_OK);
        let mut ctx = Context::new(&cs);
        // It still consumes the key, but the gated post drops the command.
        assert_eq!(
            b.handle_event(&key(KeyCode::Enter), &mut ctx),
            EventResult::Consumed
        );
        assert!(ctx.posted().is_empty());
    }

    #[test]
    fn focus_chooses_the_draw_colour() {
        let theme = Theme::default();
        let mut b = button();

        // Draw unfocused, then focused, and compare a label cell's style.
        let draw = |b: &Button| {
            let mut buf = Buffer::new(Size::new(10, 1));
            let mut c = Canvas::new(&mut buf);
            b.draw(&mut c);
            buf
        };
        let label_x = (10 - 2) / 2; // "OK" centred in width 10
        let unfocused = draw(&b);
        assert_eq!(
            unfocused.get(Point::new(label_x, 0)).unwrap().style(),
            theme.style(Role::ButtonNormal)
        );
        b.set_focused(true);
        let focused = draw(&b);
        assert_eq!(
            focused.get(Point::new(label_x, 0)).unwrap().style(),
            theme.style(Role::ButtonFocused)
        );
    }

    #[test]
    fn snapshot_centres_its_label() {
        let b = Button::new(rect(0, 0, 12, 1), "Cancel", CM_OK, &Theme::default());
        let mut buf = Buffer::new(Size::new(12, 1));
        let mut c = Canvas::new(&mut buf);
        b.draw(&mut c);
        insta::assert_snapshot!(buf.to_text());
    }
}
