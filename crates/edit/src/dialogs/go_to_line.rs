//! The Go to Line dialog: a number field and OK/Cancel.

use std::cell::RefCell;
use std::rc::Rc;

use rvision::canvas::Canvas;
use rvision::color::Style;
use rvision::command::{CM_CANCEL, CM_OK};
use rvision::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, View};
use rvision::widgets::{Button, InputLine, Window};

use super::modal_window;

/// The dialog's outer size — a [`Window`](rvision::widgets::Window) built
/// around it (via `super::modal_window`) is exactly this, one cell of
/// border swallowing the difference from [`GoToLine`]'s own content-only
/// [`View::bounds`] (ADR 0016: the `Window`'s `Frame` draws the border/title,
/// not the dialog itself).
pub(crate) const SIZE: Size = Size::new(40, 9);

const FOCUS_INPUT: usize = 0;
const FOCUS_OK: usize = 1;
const FOCUS_CANCEL: usize = 2;
const FOCUS_COUNT: usize = 3;

/// A "go to line number" prompt, run modally via `super::modal_window`. Read
/// [`line`](GoToLine::line) off the shared handle after `exec_view` returns
/// `CM_OK`.
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
        let size = Size::new(SIZE.width - 2, SIZE.height - 2);
        let mut dialog = Self {
            size,
            style: theme.style(Role::DialogBackground),
            input: InputLine::new(rect(0, 2, size.width, 1), theme),
            ok: Button::new(
                rect(size.width - 22, size.height - 1, 10, 1),
                "OK",
                CM_OK,
                theme,
            )
            .default(true),
            cancel: Button::new(
                rect(size.width - 11, size.height - 1, 10, 1),
                "Cancel",
                CM_CANCEL,
                theme,
            ),
            focus: FOCUS_INPUT,
        };
        dialog.apply_focus();
        dialog
    }

    /// Builds a fresh dialog wrapped in a ready-to-run [`Window`], plus the
    /// handle to read [`line`](Self::line) back through once
    /// [`exec_view`](rvision::app::Application::exec_view) returns `CM_OK`.
    pub fn window(theme: &Theme) -> (Window, Rc<RefCell<Self>>) {
        modal_window("Go to Line", SIZE, theme, Self::new(theme))
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

    /// Routes a mouse event to the control under the pointer, focusing it on a
    /// press, mirroring the key dispatch in `handle_event`. `m.pos` already
    /// arrives in this view's own local coordinates — the border/title
    /// belongs to the [`Window`](rvision::widgets::Window) built around this
    /// dialog, which translates into it before forwarding (ADR 0016).
    fn handle_mouse(&mut self, m: &MouseEvent, ctx: &mut Context) -> EventResult {
        let p = m.pos;
        let bounds = [self.input.bounds(), self.ok.bounds(), self.cancel.bounds()];
        let Some(i) = bounds.iter().position(|b| b.contains(p)) else {
            return EventResult::Ignored;
        };
        if matches!(m.kind, MouseKind::Down(MouseButton::Left)) {
            self.focus = i;
            self.apply_focus();
        }
        let b = bounds[i];
        let local = Event::Mouse(MouseEvent {
            pos: p.offset(-b.origin().x, -b.origin().y),
            ..*m
        });
        match i {
            FOCUS_INPUT => self.input.handle_event(&local, ctx),
            FOCUS_OK => self.ok.handle_event(&local, ctx),
            FOCUS_CANCEL => self.cancel.handle_event(&local, ctx),
            _ => EventResult::Ignored,
        }
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
        canvas.put_str(Point::new(0, 0), "Line number:", self.style);
        for control in [&self.input as &dyn View, &self.ok, &self.cancel] {
            let mut child = canvas.child(control.bounds());
            control.draw(&mut child);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => key,
            Event::Mouse(m) => return self.handle_mouse(m, ctx),
            _ => return EventResult::Ignored,
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

    fn click(d: &mut GoToLine, x: i16, y: i16) -> (EventResult, Vec<Event>) {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = d.handle_event(
            &Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                pos: Point::new(x, y),
                modifiers: Modifiers::NONE,
            }),
            &mut ctx,
        );
        (r, ctx.take_posted())
    }

    #[test]
    fn clicking_the_ok_button_focuses_it_and_posts_ok() {
        let mut d = dialog();
        // OK sits at (16, 6).
        let (r, posted) = click(&mut d, 17, 6);
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(d.focus, FOCUS_OK);
        assert_eq!(posted, vec![Event::Command(CM_OK)]);
    }

    #[test]
    fn clicking_cancel_posts_cancel() {
        let mut d = dialog();
        let (_, posted) = click(&mut d, 29, 6); // the Cancel button
        assert_eq!(d.focus, FOCUS_CANCEL);
        assert_eq!(posted, vec![Event::Command(CM_CANCEL)]);
    }
}
