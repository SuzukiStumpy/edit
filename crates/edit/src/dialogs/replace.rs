//! The Replace dialog: find/replacement fields, case and whole-word options, and
//! Replace All / Cancel buttons. Replace-all is the action (interactive
//! one-at-a-time replace is a possible later refinement); the editor applies every
//! match as a single undo unit.

use std::cell::RefCell;
use std::rc::Rc;

use rvision::canvas::Canvas;
use rvision::color::Style;
use rvision::command::{CM_CANCEL, CM_OK};
use rvision::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, View};
use rvision::widgets::{Button, CheckBox, InputLine, Window};

use crate::search::Query;

use super::modal_window;

/// The dialog's outer size — see [`GoToLine::SIZE`](super::go_to_line::SIZE)'s
/// doc comment for why this differs from [`ReplaceDialog`]'s own content-only
/// [`View::bounds`].
pub(crate) const SIZE: Size = Size::new(52, 12);

const FOCUS_FIND: usize = 0;
const FOCUS_REPLACE: usize = 1;
const FOCUS_CASE: usize = 2;
const FOCUS_WORD: usize = 3;
const FOCUS_ALL: usize = 4;
const FOCUS_CANCEL: usize = 5;
const FOCUS_COUNT: usize = 6;

/// A modal Replace prompt. After `exec_view` returns `CM_OK`, read
/// [`query`](ReplaceDialog::query) and [`replacement`](ReplaceDialog::replacement).
pub struct ReplaceDialog {
    size: Size,
    style: Style,
    find: InputLine,
    replace: InputLine,
    case: CheckBox,
    word: CheckBox,
    all: Button,
    cancel: Button,
    focus: usize,
}

impl ReplaceDialog {
    /// Creates the dialog with empty fields and all options off.
    pub fn new(theme: &Theme) -> Self {
        let size = Size::new(SIZE.width - 2, SIZE.height - 2);
        let (iw, ih) = (size.width, size.height);
        let mut dialog = Self {
            size,
            style: theme.style(Role::DialogBackground),
            find: InputLine::new(rect(0, 1, iw, 1), theme),
            replace: InputLine::new(rect(0, 4, iw, 1), theme),
            case: CheckBox::new(rect(0, 6, iw, 1), "Case sensitive", theme),
            word: CheckBox::new(rect(0, 7, iw, 1), "Whole word", theme),
            all: Button::new(rect(iw - 26, ih - 1, 14, 1), "Replace All", CM_OK, theme)
                .default(true),
            cancel: Button::new(rect(iw - 11, ih - 1, 10, 1), "Cancel", CM_CANCEL, theme),
            focus: FOCUS_FIND,
        };
        dialog.apply_focus();
        dialog
    }

    /// Builds a fresh dialog wrapped in a ready-to-run [`Window`], seeded with
    /// the saved Find options so it reopens the way the user last left it
    /// (ADR 0025), plus the handle to read [`query`](Self::query)/
    /// [`replacement`](Self::replacement) back through once
    /// [`exec_view`](rvision::app::Application::exec_view) returns `CM_OK`.
    pub fn window(
        theme: &Theme,
        case_sensitive: bool,
        whole_word: bool,
    ) -> (Window, Rc<RefCell<Self>>) {
        let mut dialog = Self::new(theme);
        dialog.case.set_checked(case_sensitive);
        dialog.word.set_checked(whole_word);
        modal_window("Replace", SIZE, theme, dialog)
    }

    /// The search query built from the find field and the case/whole-word options.
    pub fn query(&self) -> Query {
        Query {
            needle: self.find.text().to_string(),
            case_sensitive: self.case.is_checked(),
            whole_word: self.word.is_checked(),
        }
    }

    /// The replacement text.
    pub fn replacement(&self) -> String {
        self.replace.text().to_string()
    }

    /// Pushes the focus flag to whichever control now holds it (ADR 0017).
    fn apply_focus(&mut self) {
        self.find.set_focused(self.focus == FOCUS_FIND);
        self.replace.set_focused(self.focus == FOCUS_REPLACE);
        self.case.set_focused(self.focus == FOCUS_CASE);
        self.word.set_focused(self.focus == FOCUS_WORD);
        self.all.set_focused(self.focus == FOCUS_ALL);
        self.cancel.set_focused(self.focus == FOCUS_CANCEL);
    }

    /// Moves focus `delta` steps round the controls.
    fn move_focus(&mut self, delta: isize) {
        let n = FOCUS_COUNT as isize;
        self.focus = (((self.focus as isize + delta) % n + n) % n) as usize;
        self.apply_focus();
    }

    /// `Enter`: Cancel cancels, anything else accepts (runs Replace All).
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
            FOCUS_FIND => self.find.handle_event(event, ctx),
            FOCUS_REPLACE => self.replace.handle_event(event, ctx),
            FOCUS_CASE => self.case.handle_event(event, ctx),
            FOCUS_WORD => self.word.handle_event(event, ctx),
            FOCUS_ALL => self.all.handle_event(event, ctx),
            FOCUS_CANCEL => self.cancel.handle_event(event, ctx),
            _ => EventResult::Ignored,
        }
    }

    /// Routes a left-press to the control under the pointer, focusing it
    /// first. `m.pos` already arrives in this view's own local coordinates —
    /// the border/title belongs to the [`Window`](rvision::widgets::Window)
    /// built around this dialog, which translates into it before forwarding
    /// (ADR 0016).
    fn handle_mouse(&mut self, m: &MouseEvent, ctx: &mut Context) -> EventResult {
        if !matches!(m.kind, MouseKind::Down(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let p = m.pos;
        let bounds = [
            self.find.bounds(),
            self.replace.bounds(),
            self.case.bounds(),
            self.word.bounds(),
            self.all.bounds(),
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

impl View for ReplaceDialog {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.put_str(Point::new(0, 0), "Find what:", self.style);
        canvas.put_str(Point::new(0, 3), "Replace with:", self.style);
        for control in [
            &self.find as &dyn View,
            &self.replace,
            &self.case,
            &self.word,
            &self.all,
            &self.cancel,
        ] {
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
            _ => self.route(event, ctx),
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

    fn dialog() -> ReplaceDialog {
        ReplaceDialog::new(&Theme::default())
    }

    fn press(d: &mut ReplaceDialog, code: KeyCode) -> Vec<Event> {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        d.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx);
        ctx.take_posted()
    }

    fn type_str(d: &mut ReplaceDialog, s: &str) {
        for c in s.chars() {
            press(d, KeyCode::Char(c));
        }
    }

    #[test]
    fn clicking_the_replace_field_focuses_it() {
        let mut d = dialog();
        assert_eq!(d.focus, FOCUS_FIND);
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // The "Replace with" field is at (0, 4).
        let click = Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(2, 4),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(d.handle_event(&click, &mut ctx), EventResult::Consumed);
        assert_eq!(d.focus, FOCUS_REPLACE);
    }

    #[test]
    fn window_seeds_the_checkboxes_from_the_saved_options() {
        let (_, handle) = ReplaceDialog::window(&Theme::default(), true, true);
        let q = handle.borrow().query();
        assert!(q.case_sensitive);
        assert!(q.whole_word);
    }

    #[test]
    fn collects_the_find_and_replace_fields() {
        let mut d = dialog();
        type_str(&mut d, "old"); // find field
        press(&mut d, KeyCode::Tab); // to the replace field
        assert_eq!(d.focus, FOCUS_REPLACE);
        type_str(&mut d, "new");
        assert_eq!(d.query().needle, "old");
        assert_eq!(d.replacement(), "new");
    }

    #[test]
    fn enter_accepts_with_replace_all() {
        let mut d = dialog();
        type_str(&mut d, "x");
        assert_eq!(press(&mut d, KeyCode::Enter), vec![Event::Command(CM_OK)]);
    }

    #[test]
    fn the_options_reach_the_query() {
        let mut d = dialog();
        // Tab to the two checkboxes (find, replace, case, word) and toggle them.
        for _ in 0..2 {
            press(&mut d, KeyCode::Tab);
        }
        assert_eq!(d.focus, FOCUS_CASE);
        press(&mut d, KeyCode::Char(' '));
        press(&mut d, KeyCode::Tab);
        press(&mut d, KeyCode::Char(' '));
        let q = d.query();
        assert!(q.case_sensitive && q.whole_word);
    }

    #[test]
    fn esc_cancels() {
        let mut d = dialog();
        assert_eq!(press(&mut d, KeyCode::Esc), vec![Event::Command(CM_CANCEL)]);
    }
}
