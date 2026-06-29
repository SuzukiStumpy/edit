//! The Find dialog: a search field, case / whole-word / direction options, and
//! Find / Cancel buttons.

use rvision::canvas::Canvas;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::{CM_CANCEL, CM_OK, Command};
use rvision::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, Modal, View};
use rvision::widgets::{Button, CheckBox, InputLine};

use crate::search::Query;

const FOCUS_INPUT: usize = 0;
const FOCUS_CASE: usize = 1;
const FOCUS_WORD: usize = 2;
const FOCUS_BACK: usize = 3;
const FOCUS_FIND: usize = 4;
const FOCUS_CANCEL: usize = 5;
const FOCUS_COUNT: usize = 6;

/// A modal Find prompt. After `exec_view` returns `CM_OK`, read
/// [`query`](FindDialog::query) and [`backward`](FindDialog::backward).
pub struct FindDialog {
    size: Size,
    style: Style,
    input: InputLine,
    case: CheckBox,
    word: CheckBox,
    back: CheckBox,
    find: Button,
    cancel: Button,
    focus: usize,
}

impl FindDialog {
    /// Creates the dialog with an empty field and all options off.
    pub fn new(theme: &Theme) -> Self {
        let size = Size::new(48, 11);
        let iw = size.width - 2;
        let ih = size.height - 2;
        let mut dialog = Self {
            size,
            style: theme.style(Role::DialogBackground),
            input: InputLine::new(rect(0, 1, iw, 1), theme),
            case: CheckBox::new(rect(0, 3, iw, 1), "Case sensitive", theme),
            word: CheckBox::new(rect(0, 4, iw, 1), "Whole word", theme),
            back: CheckBox::new(rect(0, 5, iw, 1), "Search backwards", theme),
            find: Button::new(rect(iw - 22, ih - 1, 10, 1), "Find", CM_OK, theme).default(true),
            cancel: Button::new(rect(iw - 11, ih - 1, 10, 1), "Cancel", CM_CANCEL, theme),
            focus: FOCUS_INPUT,
        };
        dialog.apply_focus();
        dialog
    }

    /// The query built from the field and the case/whole-word options.
    pub fn query(&self) -> Query {
        Query {
            needle: self.input.text().to_string(),
            case_sensitive: self.case.is_checked(),
            whole_word: self.word.is_checked(),
        }
    }

    /// Whether the "Search backwards" option is set.
    pub fn backward(&self) -> bool {
        self.back.is_checked()
    }

    /// Pushes the focus flag to whichever control now holds it (ADR 0017).
    fn apply_focus(&mut self) {
        self.input.set_focused(self.focus == FOCUS_INPUT);
        self.case.set_focused(self.focus == FOCUS_CASE);
        self.word.set_focused(self.focus == FOCUS_WORD);
        self.back.set_focused(self.focus == FOCUS_BACK);
        self.find.set_focused(self.focus == FOCUS_FIND);
        self.cancel.set_focused(self.focus == FOCUS_CANCEL);
    }

    /// Moves focus `delta` steps round the controls.
    fn move_focus(&mut self, delta: isize) {
        let n = FOCUS_COUNT as isize;
        self.focus = (((self.focus as isize + delta) % n + n) % n) as usize;
        self.apply_focus();
    }

    /// `Enter`: Cancel cancels, anything else accepts (runs the search).
    fn on_enter(&self, ctx: &mut Context) -> EventResult {
        ctx.post(if self.focus == FOCUS_CANCEL {
            CM_CANCEL
        } else {
            CM_OK
        });
        EventResult::Consumed
    }

    /// Routes a non-navigation key to the focused control.
    fn route(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match self.focus {
            FOCUS_INPUT => self.input.handle_event(event, ctx),
            FOCUS_CASE => self.case.handle_event(event, ctx),
            FOCUS_WORD => self.word.handle_event(event, ctx),
            FOCUS_BACK => self.back.handle_event(event, ctx),
            FOCUS_FIND => self.find.handle_event(event, ctx),
            FOCUS_CANCEL => self.cancel.handle_event(event, ctx),
            _ => EventResult::Ignored,
        }
    }

    /// The interior rectangle (inset one cell on every side) in local coordinates.
    fn interior(&self) -> Rect {
        Rect::from_origin_size(
            Point::new(1, 1),
            Size::new((self.size.width - 2).max(0), (self.size.height - 2).max(0)),
        )
    }

    /// Routes a left-press (in dialog-local coordinates) to the control under the
    /// pointer, focusing it first. Control bounds are interior-local, so the pointer
    /// is shifted by the interior origin and then into the control's own coordinates.
    fn handle_mouse(&mut self, m: &MouseEvent, ctx: &mut Context) -> EventResult {
        if !matches!(m.kind, MouseKind::Down(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let io = self.interior().origin();
        let p = m.pos.offset(-io.x, -io.y);
        let bounds = [
            self.input.bounds(),
            self.case.bounds(),
            self.word.bounds(),
            self.back.bounds(),
            self.find.bounds(),
            self.cancel.bounds(),
        ];
        let Some(i) = bounds.iter().position(|b| b.contains(p)) else {
            return EventResult::Ignored;
        };
        self.focus = i;
        self.apply_focus();
        let b = bounds[i];
        let local = Event::Mouse(MouseEvent {
            pos: p.offset(-b.origin().x, -b.origin().y),
            ..*m
        });
        self.route(&local, ctx)
    }
}

fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
    Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
}

impl View for FindDialog {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        canvas.draw_box(area, self.style);
        let title = " Find ";
        let x = ((area.width() - title.chars().count() as i16) / 2).max(1);
        canvas.put_str(Point::new(x, 0), title, self.style);

        let interior = self.interior();
        if interior.is_empty() {
            return;
        }
        let mut sub = canvas.child(interior);
        sub.put_str(Point::new(0, 0), "Find what:", self.style);
        for control in [
            &self.input as &dyn View,
            &self.case,
            &self.word,
            &self.back,
            &self.find,
            &self.cancel,
        ] {
            let mut child = sub.child(control.bounds());
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
            _ => self.route(event, ctx),
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Modal for FindDialog {
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

    fn dialog() -> FindDialog {
        FindDialog::new(&Theme::default())
    }

    fn press(d: &mut FindDialog, code: KeyCode) -> Vec<Event> {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        d.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx);
        ctx.take_posted()
    }

    fn type_str(d: &mut FindDialog, s: &str) {
        for c in s.chars() {
            press(d, KeyCode::Char(c));
        }
    }

    #[test]
    fn clicking_a_control_focuses_it() {
        let mut d = dialog();
        assert_eq!(d.focus, FOCUS_INPUT);
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // The "Case sensitive" box is at interior-local (0, 3) → dialog-local (1, 4).
        let click = Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(3, 4),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(d.handle_event(&click, &mut ctx), EventResult::Consumed);
        assert_eq!(d.focus, FOCUS_CASE);
    }

    #[test]
    fn typing_a_query_then_enter_accepts_it() {
        let mut d = dialog();
        type_str(&mut d, "needle");
        let posted = press(&mut d, KeyCode::Enter);
        assert_eq!(posted, vec![Event::Command(CM_OK)]);
        let q = d.query();
        assert_eq!(q.needle, "needle");
        assert!(!q.case_sensitive && !q.whole_word);
        assert!(!d.backward());
    }

    #[test]
    fn the_checkboxes_set_the_query_options() {
        let mut d = dialog();
        type_str(&mut d, "x");
        // Tab to each checkbox and toggle it with Space.
        press(&mut d, KeyCode::Tab); // Case sensitive
        press(&mut d, KeyCode::Char(' '));
        press(&mut d, KeyCode::Tab); // Whole word
        press(&mut d, KeyCode::Char(' '));
        press(&mut d, KeyCode::Tab); // Search backwards
        press(&mut d, KeyCode::Char(' '));
        let q = d.query();
        assert!(q.case_sensitive && q.whole_word);
        assert!(d.backward());
    }

    #[test]
    fn esc_cancels() {
        let mut d = dialog();
        assert_eq!(press(&mut d, KeyCode::Esc), vec![Event::Command(CM_CANCEL)]);
    }

    #[test]
    fn tab_cycles_through_all_six_controls() {
        let mut d = dialog();
        for expected in [
            FOCUS_CASE,
            FOCUS_WORD,
            FOCUS_BACK,
            FOCUS_FIND,
            FOCUS_CANCEL,
            FOCUS_INPUT,
        ] {
            press(&mut d, KeyCode::Tab);
            assert_eq!(d.focus, expected);
        }
    }
}
