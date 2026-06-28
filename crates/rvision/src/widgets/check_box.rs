//! A toggleable check box (TurboVision's `TCheckBoxes`, single box).
//!
//! A focusable [control](super) drawn as `[X] Label` / `[ ] Label`. `Space`
//! toggles it while focused; `Enter` is left to bubble so the dialog's default
//! button still fires. Focus shows as a full-width highlight bar (ADR 0017).

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::event::{Event, EventResult, KeyCode};
use crate::geometry::{Point, Rect};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};

/// A labelled on/off check box.
pub struct CheckBox {
    bounds: Rect,
    label: String,
    checked: bool,
    focused: bool,
    style: Style,
    focus_style: Style,
}

impl CheckBox {
    /// Creates an unchecked box at `bounds` labelled `label`.
    pub fn new(bounds: Rect, label: &str, theme: &Theme) -> Self {
        Self {
            bounds,
            label: label.to_string(),
            checked: false,
            focused: false,
            style: theme.style(Role::DialogBackground),
            focus_style: theme.style(Role::Selection),
        }
    }

    /// Sets the initial checked state.
    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Whether the box is currently checked.
    pub fn is_checked(&self) -> bool {
        self.checked
    }
}

impl View for CheckBox {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let style = if self.focused {
            self.focus_style
        } else {
            self.style
        };
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(style));
        let mark = if self.checked { 'X' } else { ' ' };
        canvas.put_str(Point::new(0, 0), &format!("[{mark}] {}", self.label), style);
    }

    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        if let Event::Key(key) = event {
            if self.focused && key.code == KeyCode::Char(' ') {
                self.checked = !self.checked;
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
    use crate::command::CommandSet;
    use crate::event::{KeyEvent, Modifiers};
    use crate::geometry::Size;

    fn rect(w: i16) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), Size::new(w, 1))
    }

    fn check(width: i16) -> CheckBox {
        CheckBox::new(rect(width), "Wrap", &Theme::default())
    }

    fn press(c: &mut CheckBox, code: KeyCode) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        c.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx)
    }

    #[test]
    fn space_toggles_when_focused() {
        let mut c = check(12);
        c.set_focused(true);
        assert!(!c.is_checked());
        assert_eq!(press(&mut c, KeyCode::Char(' ')), EventResult::Consumed);
        assert!(c.is_checked());
        press(&mut c, KeyCode::Char(' '));
        assert!(!c.is_checked());
    }

    #[test]
    fn space_is_ignored_when_unfocused() {
        let mut c = check(12);
        assert_eq!(press(&mut c, KeyCode::Char(' ')), EventResult::Ignored);
        assert!(!c.is_checked());
    }

    #[test]
    fn enter_bubbles_for_the_default_button() {
        let mut c = check(12);
        c.set_focused(true);
        assert_eq!(press(&mut c, KeyCode::Enter), EventResult::Ignored);
    }

    #[test]
    fn draws_the_mark() {
        let render = |checked: bool| {
            let c = check(10).with_checked(checked);
            let mut buf = Buffer::new(Size::new(10, 1));
            let mut canvas = Canvas::new(&mut buf);
            c.draw(&mut canvas);
            buf.to_text()
        };
        assert_eq!(render(false), "[ ] Wrap  ");
        assert_eq!(render(true), "[X] Wrap  ");
    }

    #[test]
    fn focus_changes_the_style() {
        let theme = Theme::default();
        let mut c = check(10);
        let style_of = |c: &CheckBox| {
            let mut buf = Buffer::new(Size::new(10, 1));
            let mut canvas = Canvas::new(&mut buf);
            c.draw(&mut canvas);
            buf.get(Point::new(0, 0)).unwrap().style()
        };
        assert_eq!(style_of(&c), theme.style(Role::DialogBackground));
        c.set_focused(true);
        assert_eq!(style_of(&c), theme.style(Role::Selection));
    }
}
