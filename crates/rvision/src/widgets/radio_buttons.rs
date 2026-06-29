//! A vertical group of radio buttons (TurboVision's `TRadioButtons`).
//!
//! A single focusable [control](super) listing mutually-exclusive options, one
//! per row, drawn `(•) Label` for the selected option and `( ) Label` for the
//! rest. `Up`/`Down` move the selection (clamped at the ends, as in TurboVision);
//! the selected row highlights when the control is focused (ADR 0017).

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::event::{Event, EventResult, KeyCode, MouseButton, MouseKind};
use crate::geometry::{Point, Rect};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};

/// A column of radio options with exactly one selected.
pub struct RadioButtons {
    bounds: Rect,
    labels: Vec<String>,
    selected: usize,
    focused: bool,
    style: Style,
    focus_style: Style,
}

impl RadioButtons {
    /// Creates a radio group at `bounds` over `labels`, the first selected.
    pub fn new(bounds: Rect, labels: &[&str], theme: &Theme) -> Self {
        Self {
            bounds,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            selected: 0,
            focused: false,
            style: theme.style(Role::DialogBackground),
            focus_style: theme.style(Role::Selection),
        }
    }

    /// Sets the initially selected option (clamped to the option count).
    pub fn with_selected(mut self, index: usize) -> Self {
        self.selected = index.min(self.labels.len().saturating_sub(1));
        self
    }

    /// The index of the selected option.
    pub fn selected(&self) -> usize {
        self.selected
    }
}

impl View for RadioButtons {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        for (i, label) in self.labels.iter().enumerate() {
            let marker = if i == self.selected { '•' } else { ' ' };
            let style = if self.focused && i == self.selected {
                self.focus_style
            } else {
                self.style
            };
            let row = i as i16;
            // Repaint the row so a focused selection is a full-width bar.
            let row_area = Rect::from_origin_size(
                Point::new(0, row),
                crate::geometry::Size::new(area.width(), 1),
            );
            canvas.fill(row_area, &Cell::blank(style));
            canvas.put_str(Point::new(0, row), &format!("({marker}) {label}"), style);
        }
    }

    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        match event {
            Event::Key(key) if self.focused => match key.code {
                KeyCode::Up => {
                    self.selected = self.selected.saturating_sub(1);
                    EventResult::Consumed
                }
                KeyCode::Down => {
                    if self.selected + 1 < self.labels.len() {
                        self.selected += 1;
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            // A click selects the option on the clicked row.
            Event::Mouse(m) if matches!(m.kind, MouseKind::Down(MouseButton::Left)) => {
                let row = m.pos.y;
                if row >= 0 && (row as usize) < self.labels.len() {
                    self.selected = row as usize;
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
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
    use crate::event::{KeyEvent, Modifiers, MouseEvent};
    use crate::geometry::Size;

    fn radio() -> RadioButtons {
        RadioButtons::new(
            Rect::from_origin_size(Point::new(0, 0), Size::new(12, 3)),
            &["Unix", "DOS", "Mac"],
            &Theme::default(),
        )
    }

    fn press(r: &mut RadioButtons, code: KeyCode) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        r.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx)
    }

    #[test]
    fn clicking_a_row_selects_that_option() {
        let mut r = radio();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let click = Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(3, 2), // the third row ("Mac")
            modifiers: Modifiers::NONE,
        });
        assert_eq!(r.handle_event(&click, &mut ctx), EventResult::Consumed);
        assert_eq!(r.selected(), 2);
    }

    #[test]
    fn down_and_up_move_the_selection_and_clamp() {
        let mut r = radio();
        r.set_focused(true);
        assert_eq!(r.selected(), 0);
        press(&mut r, KeyCode::Down);
        assert_eq!(r.selected(), 1);
        press(&mut r, KeyCode::Down);
        assert_eq!(r.selected(), 2);
        // Down at the last option stays put (no wrap).
        press(&mut r, KeyCode::Down);
        assert_eq!(r.selected(), 2);
        press(&mut r, KeyCode::Up);
        press(&mut r, KeyCode::Up);
        assert_eq!(r.selected(), 0);
        // Up at the first option stays put.
        press(&mut r, KeyCode::Up);
        assert_eq!(r.selected(), 0);
    }

    #[test]
    fn with_selected_sets_and_clamps() {
        assert_eq!(radio().with_selected(2).selected(), 2);
        assert_eq!(radio().with_selected(99).selected(), 2, "clamped to last");
    }

    #[test]
    fn arrows_are_ignored_when_unfocused() {
        let mut r = radio();
        assert_eq!(press(&mut r, KeyCode::Down), EventResult::Ignored);
        assert_eq!(r.selected(), 0);
    }

    #[test]
    fn snapshot_marks_the_selected_option() {
        let mut r = radio().with_selected(1);
        r.set_focused(true);
        let mut buf = Buffer::new(Size::new(12, 3));
        let mut canvas = Canvas::new(&mut buf);
        r.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }
}
