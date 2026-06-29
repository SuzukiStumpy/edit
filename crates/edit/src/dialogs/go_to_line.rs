//! The Go to Line dialog: a number field and OK/Cancel.

use rvision::canvas::Canvas;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::{CM_CANCEL, CM_OK, Command};
use rvision::event::{Event, EventResult, KeyCode};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, Modal, View};
use rvision::widgets::{Button, InputLine};

const FOCUS_INPUT: usize = 0;
const FOCUS_OK: usize = 1;
const FOCUS_CANCEL: usize = 2;
const FOCUS_COUNT: usize = 3;

/// A modal "go to line number" prompt. Read [`line`](GoToLine::line) after
/// `exec_view` returns `CM_OK`.
pub struct GoToLine {
    size: Size,
    style: Style,
    input: InputLine,
    ok: Button,
    cancel: Button,
    focus: usize,
}

impl GoToLine {
    /// Creates the dialog with an empty field (InputLine has no select-all, so a
    /// pre-filled value would only get typed onto — better to start blank).
    pub fn new(theme: &Theme) -> Self {
        let size = Size::new(40, 9);
        let iw = size.width - 2;
        let ih = size.height - 2;
        let mut dialog = Self {
            size,
            style: theme.style(Role::DialogBackground),
            input: InputLine::new(rect(0, 2, iw, 1), theme),
            ok: Button::new(rect(iw - 22, ih - 1, 10, 1), "OK", CM_OK, theme).default(true),
            cancel: Button::new(rect(iw - 11, ih - 1, 10, 1), "Cancel", CM_CANCEL, theme),
            focus: FOCUS_INPUT,
        };
        dialog.apply_focus();
        dialog
    }

    /// The entered line as a 1-based number, or `None` if the field is empty or not
    /// a positive integer. The editor clamps it into the document.
    pub fn line(&self) -> Option<usize> {
        match self.input.text().trim().parse::<usize>() {
            Ok(n) if n >= 1 => Some(n),
            _ => None,
        }
    }

    /// Pushes the focus flag to whichever control now holds it (ADR 0017).
    fn apply_focus(&mut self) {
        self.input.set_focused(self.focus == FOCUS_INPUT);
        self.ok.set_focused(self.focus == FOCUS_OK);
        self.cancel.set_focused(self.focus == FOCUS_CANCEL);
    }

    /// Moves focus `delta` steps round the three controls.
    fn move_focus(&mut self, delta: isize) {
        let n = FOCUS_COUNT as isize;
        self.focus = (((self.focus as isize + delta) % n + n) % n) as usize;
        self.apply_focus();
    }

    /// `Enter`: Cancel cancels, anything else accepts the typed number.
    fn on_enter(&self, ctx: &mut Context) -> EventResult {
        ctx.post(if self.focus == FOCUS_CANCEL {
            CM_CANCEL
        } else {
            CM_OK
        });
        EventResult::Consumed
    }

    /// The interior rectangle (inset one cell on every side) in local coordinates.
    fn interior(&self) -> Rect {
        Rect::from_origin_size(
            Point::new(1, 1),
            Size::new((self.size.width - 2).max(0), (self.size.height - 2).max(0)),
        )
    }
}

fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
    Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
}

impl View for GoToLine {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        canvas.draw_box(area, self.style);
        let title = " Go to Line ";
        let x = ((area.width() - title.chars().count() as i16) / 2).max(1);
        canvas.put_str(Point::new(x, 0), title, self.style);

        let interior = self.interior();
        if interior.is_empty() {
            return;
        }
        let mut sub = canvas.child(interior);
        sub.put_str(Point::new(0, 0), "Line number:", self.style);
        for control in [&self.input as &dyn View, &self.ok, &self.cancel] {
            let mut child = sub.child(control.bounds());
            control.draw(&mut child);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored; // mouse is Phase 9
        };
        match key.code {
            KeyCode::Esc => {
                ctx.post(CM_CANCEL);
                EventResult::Consumed
            }
            KeyCode::Tab => {
                self.move_focus(1);
                EventResult::Consumed
            }
            KeyCode::BackTab => {
                self.move_focus(-1);
                EventResult::Consumed
            }
            KeyCode::Enter => self.on_enter(ctx),
            _ => match self.focus {
                FOCUS_INPUT => self.input.handle_event(event, ctx),
                FOCUS_OK => self.ok.handle_event(event, ctx),
                FOCUS_CANCEL => self.cancel.handle_event(event, ctx),
                _ => EventResult::Ignored,
            },
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Modal for GoToLine {
    fn size(&self) -> Size {
        self.size
    }

    fn ends_on(&self, command: Command) -> bool {
        command == CM_OK || command == CM_CANCEL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvision::command::CommandSet;
    use rvision::event::{KeyEvent, Modifiers};

    fn dialog() -> GoToLine {
        GoToLine::new(&Theme::default())
    }

    fn press(d: &mut GoToLine, code: KeyCode) -> (EventResult, Vec<Event>) {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = d.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx);
        (r, ctx.take_posted())
    }

    fn type_str(d: &mut GoToLine, s: &str) {
        for c in s.chars() {
            press(d, KeyCode::Char(c));
        }
    }

    #[test]
    fn typing_a_number_then_enter_yields_that_line() {
        let mut d = dialog();
        type_str(&mut d, "42");
        let (_, posted) = press(&mut d, KeyCode::Enter);
        assert_eq!(posted, vec![Event::Command(CM_OK)]);
        assert_eq!(d.line(), Some(42));
    }

    #[test]
    fn empty_or_non_numeric_input_is_no_line() {
        let mut d = dialog();
        assert_eq!(d.line(), None, "empty field");
        type_str(&mut d, "abc");
        assert_eq!(d.line(), None, "non-numeric is rejected");
    }

    #[test]
    fn esc_cancels() {
        let mut d = dialog();
        let (r, posted) = press(&mut d, KeyCode::Esc);
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(posted, vec![Event::Command(CM_CANCEL)]);
    }

    #[test]
    fn tab_cycles_focus_and_enter_on_cancel_cancels() {
        let mut d = dialog();
        assert_eq!(d.focus, FOCUS_INPUT);
        press(&mut d, KeyCode::Tab);
        assert_eq!(d.focus, FOCUS_OK);
        press(&mut d, KeyCode::Tab);
        assert_eq!(d.focus, FOCUS_CANCEL);
        let (_, posted) = press(&mut d, KeyCode::Enter);
        assert_eq!(posted, vec![Event::Command(CM_CANCEL)]);
        press(&mut d, KeyCode::Tab);
        assert_eq!(d.focus, FOCUS_INPUT, "wraps");
    }

    #[test]
    fn ends_on_ok_and_cancel_only() {
        let d = dialog();
        assert!(Modal::ends_on(&d, CM_OK));
        assert!(Modal::ends_on(&d, CM_CANCEL));
        assert!(!Modal::ends_on(&d, Command(rvision::command::CM_USER + 1)));
    }
}
