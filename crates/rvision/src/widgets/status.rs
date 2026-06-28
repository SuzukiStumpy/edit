//! The status line: a bottom row of global hot-key items (TurboVision's
//! `TStatusLine`).
//!
//! Each [`StatusItem`] pairs a shown hint (`F1`, `Alt-X`) and label (`Help`,
//! `Exit`) with the key that fires it and the command it posts. The status line is
//! a *post-process* handler in the shell (ADR 0016): it gets a key only after the
//! focused view has declined it, so its hot-keys never shadow typing in a window.

use crate::canvas::Canvas;
use crate::color::Style;
use crate::command::Command;
use crate::event::{Event, EventResult, KeyEvent};
use crate::geometry::{Point, Rect};
use crate::view::{Context, View};

/// One labelled hot-key on the status line.
pub struct StatusItem {
    hint: String,
    label: String,
    key: KeyEvent,
    command: Command,
}

impl StatusItem {
    /// Creates an item shown as `hint` + `label` (e.g. `"F1"`, `"Help"`), fired by
    /// `key`, posting `command`.
    pub fn new(hint: &str, label: &str, key: KeyEvent, command: Command) -> Self {
        Self {
            hint: hint.to_string(),
            label: label.to_string(),
            key,
            command,
        }
    }
}

/// A row of [`StatusItem`]s.
pub struct StatusLine {
    bounds: Rect,
    items: Vec<StatusItem>,
    style: Style,
    key_style: Style,
}

impl StatusLine {
    /// Creates a status line at `bounds` from `items`, drawing labels in `style`
    /// and the key hints in `key_style`.
    pub fn new(bounds: Rect, items: Vec<StatusItem>, style: Style, key_style: Style) -> Self {
        Self {
            bounds,
            items,
            style,
            key_style,
        }
    }

    /// Repositions the status line (the shell calls this as the terminal resizes).
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
}

impl View for StatusLine {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &crate::cell::Cell::blank(self.style));
        // " hint label   hint label  …" — the hint in key_style, the label in the
        // bar style, two spaces between items.
        let mut x = 1;
        for item in &self.items {
            x = canvas.put_str(Point::new(x, 0), &item.hint, self.key_style);
            x = canvas.put_str(Point::new(x, 0), " ", self.style);
            x = canvas.put_str(Point::new(x, 0), &item.label, self.style);
            x += 2;
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        if let Event::Key(key) = event {
            for item in &self.items {
                if item.key == *key {
                    ctx.post(item.command);
                    return EventResult::Consumed;
                }
            }
        }
        EventResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::{CM_QUIT, CommandSet};
    use crate::event::{KeyCode, Modifiers};
    use crate::geometry::Size;

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    const CM_HELP: Command = Command(crate::command::CM_USER + 1);

    fn line(bounds: Rect) -> StatusLine {
        StatusLine::new(
            bounds,
            vec![
                StatusItem::new(
                    "F1",
                    "Help",
                    KeyEvent::new(KeyCode::F(1), Modifiers::NONE),
                    CM_HELP,
                ),
                StatusItem::new(
                    "Alt-X",
                    "Exit",
                    KeyEvent::new(KeyCode::Char('x'), Modifiers::ALT),
                    CM_QUIT,
                ),
            ],
            Style::new(),
            Style::new(),
        )
    }

    #[test]
    fn snapshot_status_line() {
        let sl = line(rect(0, 0, 30, 1));
        let mut buf = Buffer::new(Size::new(30, 1));
        let mut canvas = Canvas::new(&mut buf);
        sl.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }

    #[test]
    fn a_matching_key_posts_its_command() {
        let mut sl = line(rect(0, 0, 30, 1));
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = sl.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::F(1), Modifiers::NONE)),
            &mut ctx,
        );
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(ctx.posted(), &[Event::Command(CM_HELP)]);
    }

    #[test]
    fn a_modifier_must_match_too() {
        // Plain 'x' (no Alt) is not the Alt-X item; nothing fires.
        let mut sl = line(rect(0, 0, 30, 1));
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = sl.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('x'), Modifiers::NONE)),
            &mut ctx,
        );
        assert_eq!(r, EventResult::Ignored);
        assert!(ctx.posted().is_empty());
    }

    #[test]
    fn a_disabled_items_command_is_not_posted() {
        let mut sl = line(rect(0, 0, 30, 1));
        let mut cs = CommandSet::new();
        cs.disable(CM_HELP);
        let mut ctx = Context::new(&cs);
        // The key still "matches and consumes", but the gated post drops it.
        sl.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::F(1), Modifiers::NONE)),
            &mut ctx,
        );
        assert!(ctx.posted().is_empty());
    }
}
